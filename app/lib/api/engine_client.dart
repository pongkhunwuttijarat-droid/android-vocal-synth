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
  @override
  Future<Uint8List> render({
    required String project,
    required String voicebank,
  }) async {
    final resp = await _guard(
      _http.post(
        Uri.parse('$baseUrl/render'),
        headers: _jsonHeaders,
        body: jsonEncode({'project': project, 'voicebank': voicebank}),
      ),
      renderTimeout,
      'render',
    );
    _ensureOk(resp, 'render');
    return Uint8List.fromList(resp.bodyBytes);
  }

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
      if (json is Map && json['error'] is String) {
        return json['error'] as String;
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
