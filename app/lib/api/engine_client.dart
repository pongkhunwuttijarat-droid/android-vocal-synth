/// This file is pure Dart (no Flutter imports) so it can also be exercised
/// by plain `dart run` scripts.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:http/http.dart' as http;

import '../models.dart';

/// Engine transport abstraction (CONTRACT-v1 §0).
///
/// UI screens depend on this interface only — swap the transport without
/// touching UI code:
/// - `HttpEngineClient`/`ApiClient` — dev mode (synth-server over HTTP)
/// - `JniEngineClient` (future, MS3+) — engine embedded in the app
abstract class EngineClient {
  /// `GET /health` — liveness + version + renderer `.so` loaded flag.
  Future<Health> health();

  /// `GET /voicebanks` — banks discovered on the engine, as view models.
  Future<List<Voicebank>> voicebanks();

  /// `POST /synth-note` — synthesize a single note, returns the wav bytes.
  Future<Uint8List> synthNote({
    required String voicebank,
    required String phoneme,
    required int tone,
    double? durationMs,
  });

  /// `POST /render` — render a `.ustx` project, returns the wav bytes.
  Future<Uint8List> render({
    required String project,
    required String voicebank,
    String engine = 'r1',
  });

  /// Hard Stop: abort the in-flight render on the server (the worker
  /// bails between chunks and the render reply comes back as an error).
  Future<void> cancel();

  /// Hot-swap the mixer FX params (`{"gain":..,"low_gain":..,...}`).
  Future<void> setMixerParams(String paramsJson);

  /// Post-synth FX: apply the mixer chain to an already rendered wav.
  Future<Uint8List> postFx({
    required Uint8List rawWav,
    required String paramsJson,
  });

  /// `POST /project` — save a project server-side. The app sends the
  /// codec JSON view; the server encodes it to `.ustx` and stores the
  /// file (ALL project data lives on the server).
  Future<void> saveProject({
    required String name,
    required Map<String, dynamic> projectView,
  });

  /// `GET /projects` — list saved project names.
  Future<List<String>> listProjects();

  /// `GET /project/<name>` — load a project (server decodes .ustx back
  /// to the JSON view; the app never touches .ustx bytes).
  Future<Map<String, dynamic>?> loadProject(String name);

  /// `DELETE /project/<name>` — remove a saved project.
  Future<void> deleteProject(String name);

  /// `POST /phonemize` — resolve a project track's lyrics to voicebank
  /// phoneme aliases (the SynthV-style labels above notes come from the
  /// Rust phonemizer, not a Dart letter-split).
  Future<Map<String, dynamic>> phonemize({
    required String project,
    required String voicebank,
    int track = 0,
  });
}

/// Default engine base URL (see CONTRACT-v1 §1).
const String kDefaultEngineBaseUrl = 'http://127.0.0.1:18080';

/// Error carrying the server's `{"error": ...}` message (or a transport
/// failure description) plus the HTTP status when one was received.
class ApiException implements Exception {
  const ApiException(this.message, {this.statusCode});

  final String message;
  final int? statusCode;

  @override
  String toString() => message;
}

/// `GET /health` payload.
class Health {
  const Health({
    required this.status,
    required this.version,
    required this.soLoaded,
  });

  final String status;
  final String version;
  final bool soLoaded;
}

/// One `{dir, name, aliases_count, wav_count, samples_rate}` entry from
/// `GET /voicebanks`.
class VoicebankInfo {
  const VoicebankInfo({
    required this.dir,
    required this.name,
    required this.aliasesCount,
    required this.wavCount,
    this.samplesRate,
  });

  final String dir;
  final String name;
  final int aliasesCount;
  final int wavCount;
  final int? samplesRate;

  /// Map to the UI view model (CONTRACT-v1 §3): name from the server's
  /// display name, format fixed to OpenUtau, status ready, size = wav count.
  Voicebank toViewModel() => Voicebank(
    name: name,
    format: 'OpenUtau',
    singer: dir, // dir id is what /render accepts as `voicebank`
    status: 'ready',
    sizeMb: wavCount,
  );
}

/// Client for the synth-server API.
///
/// [baseUrl] is mutable — the Settings screen rewrites it when the user
/// edits the server URL. [httpClient] is injectable so tests can mock the
/// transport (e.g. `MockClient` from `package:http/testing.dart`).
class ApiClient implements EngineClient {
  ApiClient({
    String baseUrl = kDefaultEngineBaseUrl,
    http.Client? httpClient,
    this.healthTimeout = const Duration(seconds: 10),
    this.renderTimeout = const Duration(seconds: 300),
  }) : baseUrl = _normalizeBaseUrl(baseUrl),
       _http = httpClient ?? http.Client();

  static const Duration defaultHealthTimeout = Duration(seconds: 10);
  static const Duration defaultRenderTimeout = Duration(seconds: 300);

  /// Engine base URL (trailing slashes stripped). Assign to change it.
  String baseUrl;

  final http.Client _http;

  /// Timeouts per CONTRACT-v1 §1: 10s health, 60s render.
  final Duration healthTimeout;
  final Duration renderTimeout;

  static String _normalizeBaseUrl(String url) =>
      url.trim().replaceFirst(RegExp(r'/+$'), '');

  /// `GET /health` — liveness + version + renderer `.so` loaded flag.
  @override
  Future<Health> health() async {
    final resp = await _guard(
      _http.get(Uri.parse('$baseUrl/health')),
      healthTimeout,
      'health',
    );
    _ensureOk(resp, 'health');
    final json = _decodeObject(resp);
    return Health(
      status: json['status'] as String? ?? 'unknown',
      version: json['version'] as String? ?? '?',
      soLoaded: json['so_loaded'] as bool? ?? false,
    );
  }

  /// `GET /voicebanks` — banks discovered on the server, as view models.
  @override
  Future<List<Voicebank>> voicebanks() async {
    final resp = await _guard(
      _http.get(Uri.parse('$baseUrl/voicebanks')),
      healthTimeout,
      'voicebanks',
    );
    _ensureOk(resp, 'voicebanks');
    final json = _decodeObject(resp);
    final raw = json['voicebanks'];
    if (raw is! List) {
      throw ApiException('voicebanks: unexpected response shape');
    }
    return [
      for (final item in raw)
        if (item is Map<String, dynamic>)
          VoicebankInfo(
            dir: item['dir'] as String? ?? '',
            name: item['name'] as String? ?? item['dir'] as String? ?? '',
            aliasesCount: (item['aliases_count'] as num?)?.toInt() ?? 0,
            wavCount: (item['wav_count'] as num?)?.toInt() ?? 0,
            samplesRate: (item['samples_rate'] as num?)?.toInt(),
          ).toViewModel(),
    ];
  }

  /// `POST /synth-note` — synthesize a single note, returns the wav bytes.
  @override
  Future<Uint8List> synthNote({
    required String voicebank,
    required String phoneme,
    required int tone,
    double? durationMs,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/synth-note'),
        headers: _jsonHeaders,
        body: jsonEncode({
          'voicebank': voicebank,
          'phoneme': phoneme,
          'tone': tone,
          'duration_ms': ?durationMs,
        }),
      ),
      renderTimeout,
      'synth-note',
    );
    _ensureOk(resp, 'synth-note');
    return Uint8List.fromList(resp.bodyBytes);
  }

  /// `POST /render` — render a `.ustx` project on the server, returns the
  /// wav bytes.
  ///
  /// `epoch` is a client-side cancellation token: bump [renderEpoch] (via
  /// [cancelRender]) and in-flight renders throw [ApiException] instead of
  /// returning stale audio.
  @override
  Future<Uint8List> render({
    required String project,
    required String voicebank,
    String engine = 'r1',
    int epoch = 0,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/render'),
        headers: _jsonHeaders,
        body: jsonEncode({
          'project': project,
          'voicebank': voicebank,
          'engine': engine,
        }),
      ),
      renderTimeout,
      'render',
    );
    _ensureOk(resp, 'render');
    if (epoch != renderEpoch) {
      throw ApiException('render cancelled');
    }
    return Uint8List.fromList(resp.bodyBytes);
  }

  /// Cancel all in-flight renders: bump the epoch so their results are
  /// discarded on arrival, AND hard-Stop the server-side render so the
  /// worker stops doing work (the render reply then arrives as an error
  /// instead of finishing silently).
  @override
  Future<void> cancel() async {
    renderEpoch++;
    final sessionRenderId = _activeSessionRenderId;
    if (sessionRenderId != null) {
      _activeSessionRenderId = null;
      try {
        await cancelProjectRender(sessionRenderId);
      } on ApiException catch (e) {
        if (e.statusCode != null && e.statusCode! < 500) rethrow;
      }
    }
    try {
      final resp = await _guard(
        _http.post(Uri.parse('$baseUrl/cancel'), headers: _jsonHeaders),
        renderTimeout,
        'cancel',
      );
      _ensureOk(resp, 'cancel');
    } on ApiException catch (e) {
      // Cancel failing (e.g. engine already dead) is not fatal — the
      // epoch bump already invalidates any in-flight result.
      if (e.statusCode != null && e.statusCode! < 500) rethrow;
    }
  }

  /// Hot-swap the mixer FX params on the server worker.
  @override
  Future<void> setMixerParams(String paramsJson) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/mixer-params'),
        headers: _jsonHeaders,
        body: jsonEncode(paramsJson),
      ),
      renderTimeout,
      'mixer-params',
    );
    _ensureOk(resp, 'mixer-params');
  }

  /// Post-synth FX: apply the mixer chain to an ALREADY rendered wav
  /// (no re-synthesis — the fast fader/EQ drag path). Sends the raw wav
  /// bytes; returns the FX'd wav.
  @override
  Future<Uint8List> postFx({
    required Uint8List rawWav,
    required String paramsJson,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/post-fx'),
        headers: {..._jsonHeaders, 'x-mixer-params': paramsJson},
        body: rawWav,
      ),
      renderTimeout,
      'post-fx',
    );
    _ensureOk(resp, 'post-fx');
    return Uint8List.fromList(resp.bodyBytes);
  }

  @override
  Future<void> saveProject({
    required String name,
    required Map<String, dynamic> projectView,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/project'),
        headers: _jsonHeaders,
        body: jsonEncode({'name': name, 'project': projectView}),
      ),
      renderTimeout,
      'save-project',
    );
    _ensureOk(resp, 'save-project');
  }

  @override
  Future<List<String>> listProjects() async {
    final resp = await _guard(
      _http.get(Uri.parse('$baseUrl/projects')),
      renderTimeout,
      'list-projects',
    );
    _ensureOk(resp, 'list-projects');
    final json = jsonDecode(resp.body) as Map<String, dynamic>;
    return (json['projects'] as List? ?? const [])
        .map((e) => e.toString())
        .toList();
  }

  @override
  Future<Map<String, dynamic>?> loadProject(String name) async {
    final resp = await _guard(
      _http.get(Uri.parse('$baseUrl/project/$name')),
      renderTimeout,
      'load-project',
    );
    if (resp.statusCode == 404) return null;
    _ensureOk(resp, 'load-project');
    return jsonDecode(resp.body) as Map<String, dynamic>;
  }

  @override
  Future<void> deleteProject(String name) async {
    final resp = await _guard(
      _http.delete(Uri.parse('$baseUrl/project/$name')),
      renderTimeout,
      'delete-project',
    );
    _ensureOk(resp, 'delete-project');
  }

  @override
  Future<Map<String, dynamic>> phonemize({
    required String project,
    required String voicebank,
    int track = 0,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/phonemize'),
        headers: _jsonHeaders,
        body: jsonEncode({
          'project': project,
          'voicebank': voicebank,
          'track': track,
        }),
      ),
      renderTimeout,
      'phonemize',
    );
    _ensureOk(resp, 'phonemize');
    return jsonDecode(resp.body) as Map<String, dynamic>;
  }

  // ---- Draft v2 project session API (CONTRACT-v1 §6) --------------------

  /// Open a Rust-owned session. `source` is a v2 source descriptor such as
  /// `{type: stored_project, name: ...}` or `{type: imported_ustx, ...}`.
  Future<Map<String, dynamic>> openProjectSession({
    required Map<String, dynamic> source,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/api/v2/sessions'),
        headers: _jsonHeaders,
        body: jsonEncode({'source': source}),
      ),
      renderTimeout,
      'open-project-session',
    );
    _ensureOk(resp, 'open-project-session');
    return _decodeObject(resp);
  }

  Future<Map<String, dynamic>> readProjectSession(String sessionId) async {
    final resp = await _guard(
      _http.get(_sessionUri(sessionId)),
      renderTimeout,
      'read-project-session',
    );
    _ensureOk(resp, 'read-project-session');
    return _decodeObject(resp);
  }

  Future<Map<String, dynamic>> applyProjectCommand({
    required String sessionId,
    required int expectedRevision,
    required Map<String, dynamic> command,
    String? groupId,
  }) async {
    final resp = await _guard(
      _http.post(
        _sessionUri(sessionId, '/commands'),
        headers: _jsonHeaders,
        body: jsonEncode({
          'expected_revision': expectedRevision,
          if (groupId != null) 'group_id': groupId,
          'command': command,
        }),
      ),
      renderTimeout,
      'project-command',
    );
    _ensureOk(resp, 'project-command');
    return _decodeObject(resp);
  }

  Future<Map<String, dynamic>> beginProjectGroup({
    required String sessionId,
    required int expectedRevision,
    required String name,
  }) async {
    final resp = await _guard(
      _http.post(
        _sessionUri(sessionId, '/groups'),
        headers: _jsonHeaders,
        body: jsonEncode({'expected_revision': expectedRevision, 'name': name}),
      ),
      renderTimeout,
      'begin-project-group',
    );
    _ensureOk(resp, 'begin-project-group');
    return _decodeObject(resp);
  }

  Future<Map<String, dynamic>> finishProjectGroup({
    required String sessionId,
    required String groupId,
    required int expectedRevision,
    bool commit = true,
  }) async {
    final resp = commit
        ? await _guard(
            _http.post(
              _sessionUri(sessionId, '/groups/$groupId/commit'),
              headers: _jsonHeaders,
              body: jsonEncode({'expected_revision': expectedRevision}),
            ),
            renderTimeout,
            'commit-project-group',
          )
        : await _guard(
            _http.post(
              _sessionUri(sessionId, '/groups/$groupId/cancel'),
              headers: _jsonHeaders,
            ),
            renderTimeout,
            'cancel-project-group',
          );
    _ensureOk(resp, commit ? 'commit-project-group' : 'cancel-project-group');
    return _decodeObject(resp);
  }

  Future<Map<String, dynamic>> changeProjectHistory({
    required String sessionId,
    required bool undo,
  }) async {
    final resp = await _guard(
      _http.post(
        _sessionUri(sessionId, undo ? '/undo' : '/redo'),
        headers: _jsonHeaders,
      ),
      renderTimeout,
      undo ? 'project-undo' : 'project-redo',
    );
    _ensureOk(resp, undo ? 'project-undo' : 'project-redo');
    return _decodeObject(resp);
  }

  Future<Map<String, dynamic>> saveProjectSession({
    required String sessionId,
    required String name,
    required int expectedRevision,
  }) async {
    final resp = await _guard(
      _http.put(
        _sessionUri(sessionId, '/save'),
        headers: _jsonHeaders,
        body: jsonEncode({'name': name, 'expected_revision': expectedRevision}),
      ),
      renderTimeout,
      'save-project-session',
    );
    _ensureOk(resp, 'save-project-session');
    return _decodeObject(resp);
  }

  Future<Uint8List> renderProjectSession({
    required String sessionId,
    required int expectedRevision,
    required String voicebank,
    String engine = 'r1',
    int track = 0,
  }) async {
    final started = await _guard(
      _http.post(
        _sessionUri(sessionId, '/render'),
        headers: _jsonHeaders,
        body: jsonEncode({
          'expected_revision': expectedRevision,
          'voicebank': voicebank,
          'engine': engine,
          'track': track,
        }),
      ),
      renderTimeout,
      'start-session-render',
    );
    _ensureOk(started, 'start-session-render');
    final renderId = _decodeObject(started)['render_id'] as String?;
    if (renderId == null || renderId.isEmpty) {
      throw const ApiException('session render: missing render_id');
    }

    _activeSessionRenderId = renderId;
    final deadline = DateTime.now().add(renderTimeout);
    try {
      while (DateTime.now().isBefore(deadline)) {
        if (_activeSessionRenderId != renderId) {
          throw const ApiException('render cancelled');
        }
        await Future<void>.delayed(const Duration(milliseconds: 250));
        final statusResponse = await _guard(
          _http.get(Uri.parse('$baseUrl/api/v2/render/$renderId')),
          renderTimeout,
          'session-render-status',
        );
        _ensureOk(statusResponse, 'session-render-status');
        final status = _decodeObject(statusResponse);
        switch (status['status']) {
          case 'done':
            final audio = await _guard(
              _http.get(Uri.parse('$baseUrl/api/v2/render/$renderId/audio')),
              renderTimeout,
              'session-render-audio',
            );
            _ensureOk(audio, 'session-render-audio');
            return Uint8List.fromList(audio.bodyBytes);
          case 'failed':
            final error = status['error'] as Map<String, dynamic>? ?? const {};
            throw ApiException(
              error['message'] as String? ?? 'session render failed',
            );
          case 'cancelled':
            throw const ApiException('render cancelled');
        }
      }
      await cancelProjectRender(renderId);
      throw ApiException(
        'session render timed out after ${renderTimeout.inSeconds}s',
      );
    } finally {
      if (_activeSessionRenderId == renderId) _activeSessionRenderId = null;
    }
  }

  String? _activeSessionRenderId;

  Future<void> cancelProjectRender(String renderId) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/api/v2/render/$renderId/cancel'),
        headers: _jsonHeaders,
      ),
      renderTimeout,
      'cancel-session-render',
    );
    _ensureOk(resp, 'cancel-session-render');
  }

  Future<void> closeProjectSession(String sessionId) async {
    final resp = await _guard(
      _http.delete(_sessionUri(sessionId)),
      renderTimeout,
      'close-project-session',
    );
    _ensureOk(resp, 'close-project-session');
  }

  Uri _sessionUri(String id, [String suffix = '']) =>
      Uri.parse('$baseUrl/api/v2/sessions/${Uri.encodeComponent(id)}$suffix');

  int renderEpoch = 0;

  static const _jsonHeaders = {
    'Content-Type': 'application/json; charset=utf-8',
    'Accept': 'application/json, audio/wav',
  };

  /// Wrap a request with the given timeout; convert [TimeoutException] to
  /// an [ApiException] so callers only handle one error type.
  Future<http.Response> _guard(
    Future<http.Response> request,
    Duration timeout,
    String what,
  ) async {
    try {
      return await request.timeout(timeout);
    } on TimeoutException {
      throw ApiException(
        '$what timed out after ${timeout.inSeconds}s '
        '(is synth-server running at $baseUrl?)',
      );
    } on http.ClientException catch (e) {
      throw ApiException('$what: ${e.message}');
    }
  }

  void _ensureOk(http.Response resp, String what) {
    if (resp.statusCode < 200 || resp.statusCode >= 300) {
      throw ApiException(
        _errorMessage(resp) ?? '$what failed (HTTP ${resp.statusCode})',
        statusCode: resp.statusCode,
      );
    }
  }

  /// Parse the server's `{"error": "..."}` body; null if not parseable.
  static String? _errorMessage(http.Response resp) {
    try {
      final json = jsonDecode(resp.body);
      if (json is Map) {
        if (json['message'] is String) return json['message'] as String;
        if (json['error'] is String) return json['error'] as String;
      }
    } on FormatException {
      // Body was not JSON — fall through to the generic message.
    }
    return null;
  }

  static Map<String, dynamic> _decodeObject(http.Response resp) {
    try {
      final json = jsonDecode(resp.body);
      if (json is Map<String, dynamic>) return json;
    } on FormatException {
      // Fall through.
    }
    throw ApiException('unexpected response (not JSON)');
  }
}
