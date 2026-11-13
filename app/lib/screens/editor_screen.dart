import 'dart:async';

import 'package:audioplayers/audioplayers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../api/engine_client.dart';
import '../api/engine_paths.dart';
import '../models.dart';
import '../project/engine_project_session.dart';
import '../project/server_project_store.dart';
import '../theme.dart';
import '../widgets/piano_roll.dart';
import '../widgets/track_fx_panel.dart';

const String kDemoRenderProject =
    '/home/seal/project/android-voice-synth/native/test-data/demo-song.ustx';
const String kDemoRenderVoicebank =
    '/home/seal/project/android-voice-synth/test/golden/teto-english/library';

/// Piano-roll editor screen: app bar (project title, undo/redo, export),
/// optional track panel on wide layouts, and the [PianoRoll] editor.
class EditorScreen extends StatefulWidget {
  const EditorScreen({super.key, this.client});

  /// Engine client; defaults to a real one when not injected (tests inject
  /// a mocked client).
  final ApiClient? client;

  @override
  State<EditorScreen> createState() => _EditorScreenState();
}

class _EditorScreenState extends State<EditorScreen> {
  int _selectedTrack = 0;

  /// Side panel (Project/Sections) collapsed — mirrors ui-mock's ◀ toggle.
  bool _panelCollapsed = false;

  /// Selected-track FX panel visibility.
  bool _mixerOpen = false;

  late final ApiClient _client = widget.client ?? ApiClient();
  // Lazy: only created when the user actually hits play — constructing it in
  // widget tests would hit the audioplayers platform channel and hang.
  AudioPlayer? _player;

  /// Real-time playback position (seconds) streamed to the roll's
  /// playhead (position-driven sync — the playhead follows the actual
  /// audio, never a linear estimate).
  final StreamController<double> _playbackPositions =
      StreamController<double>.broadcast();

  /// Real audio duration (seconds) — the playhead clamps against THIS so
  /// it reaches the end exactly when the audio ends (engine switches
  /// change the rendered wav length; a beats-based estimate never
  /// completes -> "เล่นไม่จบ").
  final StreamController<double> _playbackDuration =
      StreamController<double>.broadcast();
  double _lastPlaybackDuration = 0;
  bool _posListenerAttached = false;

  void _ensurePositionListener(AudioPlayer player) {
    if (_posListenerAttached) return;
    _posListenerAttached = true;
    player.onPositionChanged.listen((pos) {
      if (mounted && !_playbackPositions.isClosed) {
        _playbackPositions.add(pos.inMilliseconds / 1000.0);
      }
    });
    player.onDurationChanged.listen((d) {
      if (d.inMilliseconds <= 0) return;
      _lastPlaybackDuration = d.inMilliseconds / 1000.0;
      if (mounted && !_playbackDuration.isClosed) {
        _playbackDuration.add(_lastPlaybackDuration);
      }
    });
  }

  bool _exporting = false;

  /// A render is in flight — the ⋯ menu shows Stop instead of Export.
  bool _renderActive = false;

  /// Debounce timer for re-render-on-edit (fires 500ms after the last edit).
  Timer? _editDebounce;

  /// Server phonemizer labels per note index (current track) — pushed to
  /// the roll for the SynthV labels above the notes.
  Map<int, List<String>>? _phonemesByNote;

  /// Latest session-rendered track audio, including its authoritative FX.
  Uint8List? _latestAudio;

  /// True when the rendered audio no longer matches the project (notes,
  /// curve or engine changed since the last render). Play MUST NOT use a
  /// stale `_latestAudio` — the old code played whatever was cached even
  /// right after an edit, so the song kept the previous render until the
  /// debounce finished ("ใช้ old cache ตอนแก้แล้วไม่มี rerender").
  bool _audioDirty = true;

  /// True while audio is actually playing — the menu shows ⏹ Stop so the
  /// user can interrupt playback (previously the Stop item only appeared
  /// during rendering, which is near-instant with the cache → there was
  /// NO way to stop a playing track: "ยังขัดไม่ได้").
  bool _isPlaying = false;

  /// Synthesis engine: "r1" (WORLDLINE-R — C++ PhraseSynth, default),
  /// "r2" (WORLDLINE-R2 — neural mel→HiFiGAN), "classic"
  /// (WORLDLINE-R Classic — SynthFeatures → C++ WorldSynthesis).
  String _engine = 'r1';

  /// Disposable editor view of the Rust-owned session snapshot.
  EngineProjectSession? _session;
  List<Track> _tracks = const [];
  Map<int, List<PitchPoint>> _sessionCurves = const {};
  final Map<int, List<Note>> _pendingNotes = {};
  Future<void> _sessionQueue = Future<void>.value();
  String? _activeGroupId;
  String? _activeCurveId;
  int _currentRevision = 0;
  bool _sessionReady = false;

  /// Display name mirrors the current engine snapshot.
  String _projectName = 'Demo Song';

  /// Path of the last successful save — null until the first save. The
  /// autosave cycle writes here (no Save As dialog on every edit).
  String? _lastSavedPath;

  /// Debounce timer for the autosave cycle: edits are saved ~1.5s after
  /// the last change (notes/curve/name), so switching screens or closing
  /// the app never loses work ("ควรทำระบบ save cycle ได้แล้ว").
  Timer? _autosaveTimer;

  /// Used only for the server-owned project list UI.
  ServerProjectStore? _store;

  int get _bpm => _session?.bpm.round() ?? 120;

  /// Save the authoritative Rust session, including its stable-ID sidecar.
  Future<String?> _saveProject() async {
    try {
      final session = _session;
      if (session == null)
        throw const ApiException('project session is not ready');
      await _client.saveProjectSession(
        sessionId: session.sessionId,
        name: _projectName,
        expectedRevision: session.revision,
      );
      _lastSavedPath = _projectName;
      debugPrint('saved project session → $_projectName');
      if (mounted) _showSnack('💾 Saved: $_projectName');
      return _projectName;
    } catch (e) {
      debugPrint('save project failed: $e');
      return null;
    }
  }

  /// Schedule an autosave ~1.5s after the last edit. The first save
  /// (when the project has never been saved) opens the Save As dialog so
  /// the user picks a name; afterwards saves are silent.
  void _scheduleAutosave() {
    _autosaveTimer?.cancel();
    _autosaveTimer = Timer(const Duration(milliseconds: 1500), () async {
      if (_lastSavedPath == null) return; // never saved → user picks name
      await _saveProject();
    });
  }

  /// Save As: prompt for a project name, then save. Used by the first
  /// save and the "Save As" menu item.
  Future<void> _saveProjectAs() async {
    if (!mounted) return;
    final name = await _promptProjectName();
    if (name == null || name.isEmpty || !mounted) return;
    _projectName = name;
    await _saveProject();
  }

  Future<String?> _promptProjectName() {
    final controller = TextEditingController(text: _projectName);
    return showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Save project as…'),
        content: TextField(
          controller: controller,
          autofocus: true,
          decoration: const InputDecoration(labelText: 'Project name'),
          onSubmitted: (v) => Navigator.pop(ctx, v),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, controller.text),
            child: const Text('Save'),
          ),
        ],
      ),
    );
  }

  /// Open a .ustx project file (via the store's Rust codec) and load its
  /// tracks + curves into the editor.
  Future<bool> _openProject(String path) async {
    try {
      final opened = await _client.openProjectSession(
        source: {'type': 'stored_project', 'name': path},
      );
      final snap = (opened['snapshot'] as Map).cast<String, dynamic>();
      final sessionId = opened['session_id'] as String? ?? '';
      final normalized = Map<String, dynamic>.from(snap)
        ..['session_id'] = sessionId;
      final view = EngineProjectSession.fromJson(normalized);
      if (view.tracks.isEmpty) return false;
      await _replaceSession(view);
      _projectName = view.name;
      _lastSavedPath = path;
      return true;
    } catch (e) {
      debugPrint('open project failed: $e');
      return false;
    }
  }

  /// Open-project flow: list saved projects (store) and let the user
  /// pick one.
  Future<void> _pickAndOpenProject() async {
    try {
      final store = _store ??= ServerProjectStore(_client);
      if (!mounted) return;
      final projects = await store.list();
      if (!mounted) return;
      if (projects.isEmpty) {
        _showSnack('No saved projects yet — Save As to create one');
        return;
      }
      final chosen = await showDialog<String>(
        context: context,
        builder: (ctx) => SimpleDialog(
          title: const Text('Open project'),
          children: [
            for (final p in projects)
              SimpleDialogOption(
                onPressed: () => Navigator.pop(ctx, p.path),
                child: Text(p.name),
              ),
          ],
        ),
      );
      if (chosen == null) return;
      final ok = await _openProject(chosen);
      if (ok) _showSnack('📂 Opened: $_projectName');
    } catch (e) {
      debugPrint('open project flow failed: $e');
    }
  }

  void _showSnack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(msg)));
  }

  /// Display name for an engine id (r1/r2/classic).
  String _engineLabel(String id) => switch (id) {
    'r2' => 'WORLDLINE-R2',
    'classic' => 'Classic',
    'diffsinger' => 'DiffSinger',
    _ => 'WORLDLINE-R',
  };

  Future<void> _initializeSession() async {
    try {
      final yaml = await rootBundle.loadString(
        'android/app/src/main/assets/engine/demo-song.ustx',
      );
      final opened = await _client.openProjectSession(
        source: {'type': 'imported_ustx', 'name': 'Demo Song', 'ustx': yaml},
      );
      final snapshot = (opened['snapshot'] as Map).cast<String, dynamic>();
      final normalized = Map<String, dynamic>.from(snapshot)
        ..['session_id'] = opened['session_id'];
      await _replaceSession(EngineProjectSession.fromJson(normalized));
      if (mounted) _scheduleReRender();
    } catch (e) {
      debugPrint('open project session failed: $e');
      if (mounted) _showSnack('Could not open project: $e');
    }
  }

  Future<void> _replaceSession(EngineProjectSession next) async {
    final previous = _session;
    if (previous != null && previous.sessionId != next.sessionId) {
      try {
        await _client.closeProjectSession(previous.sessionId);
      } catch (e) {
        debugPrint('close previous project session failed: $e');
      }
    }
    if (!mounted) return;
    setState(() {
      _session = next;
      _currentRevision = next.revision;
      _tracks = next.tracks;
      _sessionCurves = next.curvesByTrack;
      _projectName = next.name;
      _selectedTrack = _selectedTrack
          .clamp(0, (next.tracks.length - 1).clamp(0, 999))
          .toInt();
      _pendingNotes
        ..clear()
        ..addAll({
          for (var i = 0; i < next.tracks.length; i++) i: next.tracks[i].notes,
        });
      _activeGroupId = null;
      _activeCurveId = _findPitchCurveId(next);
      _sessionReady = true;
      _audioDirty = true;
      _latestAudio = null;
    });
    _refreshPhonemes();
  }

  String? _findPitchCurveId(EngineProjectSession session) {
    final parts = session.trackParts(_selectedTrack);
    if (parts.isEmpty) return null;
    final curves = (parts.first['curves'] as List? ?? const [])
        .whereType<Map<String, dynamic>>();
    for (final curve in curves) {
      if (curve['abbr'] == 'pitd') return curve['id'] as String?;
    }
    return null;
  }

  void _enqueueSession(Future<void> Function() operation) {
    _sessionQueue = _sessionQueue.then((_) async {
      try {
        await operation();
      } catch (e) {
        debugPrint('project session operation failed: $e');
        _activeGroupId = null;
        final current = _session;
        if (current != null) await _refreshSessionSnapshot(current.sessionId);
        if (mounted) _showSnack('Edit rejected: $e');
      }
    });
  }

  void _beginEditGroup(String label) {
    if (!_sessionReady) return;
    _enqueueSession(() async {
      final session = _session;
      if (session == null || _activeGroupId != null) return;
      final result = await _client.beginProjectGroup(
        sessionId: session.sessionId,
        expectedRevision: _currentRevision,
        name: label,
      );
      _activeGroupId = result['group_id'] as String?;
    });
  }

  void _endEditGroup() {
    _enqueueSession(() async {
      final session = _session;
      final groupId = _activeGroupId;
      if (session == null || groupId == null) return;
      await _client.finishProjectGroup(
        sessionId: session.sessionId,
        groupId: groupId,
        expectedRevision: _currentRevision,
      );
      _activeGroupId = null;
      await _refreshSessionSnapshot(session.sessionId);
      _audioDirty = true;
      _scheduleReRender();
      _scheduleAutosave();
    });
  }

  void _onNotesChanged(List<Note> notes) {
    if (!_sessionReady || _selectedTrack >= _tracks.length) return;
    final trackIndex = _selectedTrack;
    final before = List<Note>.of(
      _pendingNotes[trackIndex] ?? _tracks[trackIndex].notes,
    );
    final after = List<Note>.of(notes);
    _pendingNotes[trackIndex] = after;
    _enqueueSession(() async {
      final session = _session;
      if (session == null) return;
      final afterByKey = {for (final note in after) note.localKey: note};
      final beforeKeys = {for (final note in before) note.localKey};
      for (final old in before) {
        final updated = afterByKey[old.localKey];
        if (updated == null) {
          if (old.engineId != null) {
            await _sendSessionCommand({
              'type': 'note.remove',
              'note_id': old.engineId,
            });
          }
          continue;
        }
        final noteId = old.engineId ?? updated.engineId;
        if (noteId == null) continue;
        final deltaPosition = ((updated.position - old.position) * 480).round();
        final deltaTone = updated.pitch - old.pitch;
        if (deltaPosition != 0 || deltaTone != 0) {
          await _sendSessionCommand({
            'type': 'note.move',
            'note_id': noteId,
            'delta_position_ticks': deltaPosition,
            'delta_tone': deltaTone,
          });
        }
        final deltaDuration = ((updated.duration - old.duration) * 480).round();
        if (deltaDuration != 0) {
          await _sendSessionCommand({
            'type': 'note.resize',
            'note_id': noteId,
            'delta_duration_ticks': deltaDuration,
            'from_start': false,
          });
        }
        if (updated.lyric != old.lyric) {
          await _sendSessionCommand({
            'type': 'note.set_lyric',
            'note_id': noteId,
            'lyric': updated.lyric,
          });
        }
      }
      for (final note in after.where((n) => !beforeKeys.contains(n.localKey))) {
        final partId = note.enginePartId ?? _tracks[trackIndex].enginePartId;
        if (partId == null)
          throw const ApiException('track has no editable voice part');
        final partStart = note.enginePartPositionTicks != 0
            ? note.enginePartPositionTicks
            : _tracks[trackIndex].enginePartPositionTicks;
        final result = await _sendSessionCommand({
          'type': 'note.add',
          'part_id': partId,
          'position_ticks': (note.position * 480).round() - partStart,
          'duration_ticks': (note.duration * 480).round(),
          'tone': note.pitch + 60,
          'lyric': note.lyric,
        });
        final changes = result['changes'] as List? ?? const [];
        for (final raw in changes.whereType<Map<String, dynamic>>()) {
          if (raw['kind'] == 'note.added' && raw['note_id'] is String) {
            final index = after.indexWhere(
              (item) => item.localKey == note.localKey,
            );
            if (index >= 0)
              after[index] = note.copyWith(engineId: raw['note_id'] as String);
          }
        }
        _pendingNotes[trackIndex] = after;
      }
      if (_activeGroupId == null) {
        await _refreshSessionSnapshot(session.sessionId);
        _scheduleReRender();
        _scheduleAutosave();
      }
    });
  }

  Future<Map<String, dynamic>> _sendSessionCommand(
    Map<String, dynamic> command,
  ) async {
    final session = _session;
    if (session == null)
      throw const ApiException('project session is not ready');
    final result = await _client.applyProjectCommand(
      sessionId: session.sessionId,
      expectedRevision: _currentRevision,
      command: command,
      groupId: _activeGroupId,
    );
    _currentRevision =
        (result['revision'] as num?)?.toInt() ?? _currentRevision;
    for (final raw
        in (result['changes'] as List? ?? const [])
            .whereType<Map<String, dynamic>>()) {
      if (raw['abbr'] == 'pitd' && raw['curve_id'] is String) {
        _activeCurveId = raw['curve_id'] as String;
      }
    }
    return result;
  }

  void _onCurveChanged(List<PitchPoint> points) {
    if (!_sessionReady || _selectedTrack >= _tracks.length) return;
    final trackIndex = _selectedTrack;
    final track = _tracks[trackIndex];
    final parts = _session?.trackParts(trackIndex) ?? const [];
    if (parts.isEmpty || track.enginePartId == null) return;
    final part = parts.first;
    final partStart = (part['position_ticks'] as num?)?.toInt() ?? 0;
    final partDuration = (part['duration_ticks'] as num?)?.toInt() ?? 0;
    if (partDuration <= 0) return;
    final notes = _pendingNotes[trackIndex] ?? track.notes;
    _enqueueSession(() async {
      final session = _session;
      if (session == null) return;
      final curvePoints = points
          .map((point) {
            final base = _baseNotePitch(notes, point.beat);
            return {
              'tick': ((point.beat * 480).round() - partStart).clamp(
                0,
                partDuration,
              ),
              'value': ((point.semitones - base) * 100).round(),
            };
          })
          .toList(growable: false);
      final create = _activeCurveId == null;
      await _sendSessionCommand({
        'type': 'curve.apply_stroke',
        'part_id': track.enginePartId,
        'curve_id': create ? null : _activeCurveId,
        'abbr': 'pitd',
        'create': create,
        'mode': 'replace',
        'start_tick': 0,
        'end_tick': partDuration,
        'points': curvePoints,
      });
      if (_activeGroupId == null) {
        await _refreshSessionSnapshot(session.sessionId);
        _scheduleReRender();
        _scheduleAutosave();
      }
    });
  }

  double _baseNotePitch(List<Note> notes, double beat) {
    if (notes.isEmpty) return 0;
    final nearest = notes.reduce((a, b) {
      final da = (a.position + a.duration / 2 - beat).abs();
      final db = (b.position + b.duration / 2 - beat).abs();
      return da <= db ? a : b;
    });
    return nearest.pitch.toDouble();
  }

  Future<void> _refreshSessionSnapshot(String sessionId) async {
    final json = await _client.readProjectSession(sessionId);
    final next = EngineProjectSession.fromJson(json);
    if (!mounted) return;
    setState(() {
      _session = next;
      _currentRevision = next.revision;
      _tracks = next.tracks;
      _sessionCurves = next.curvesByTrack;
      _pendingNotes
        ..clear()
        ..addAll({
          for (var i = 0; i < next.tracks.length; i++) i: next.tracks[i].notes,
        });
      _activeCurveId = _findPitchCurveId(next);
    });
    _refreshPhonemes();
  }

  void _changeHistory({required bool undo}) {
    _enqueueSession(() async {
      final session = _session;
      if (session == null) return;
      await _client.changeProjectHistory(
        sessionId: session.sessionId,
        undo: undo,
      );
      await _refreshSessionSnapshot(session.sessionId);
      _audioDirty = true;
      _scheduleReRender();
      _scheduleAutosave();
    });
  }

  /// Re-render quietly 500ms after the last edit (debounced). Uses the
  /// engine render cache, so an unchanged edit round-trips fast; stores
  /// the RAW audio so Play is instant. Never interrupts editing.
  void _scheduleReRender() {
    _editDebounce?.cancel();
    _editDebounce = Timer(const Duration(milliseconds: 500), () async {
      try {
        // Phoneme labels first (fast — no synthesis), then the audio.
        await _refreshPhonemes();
        await _renderRaw();
      } catch (e) {
        debugPrint('re-render failed: $e');
      }
    });
  }

  /// Fetch the REAL server phonemizer output for the current track and
  /// push it into the roll's note labels (above each note, SynthV-style).
  /// Falls back to the painter's hint/letter-split when the engine is
  /// down — labels never block playback.
  Future<void> _refreshPhonemes() async {
    // The legacy phonemize endpoint requires a saved project path. A
    // session-scoped read model is not part of the frozen v2 contract yet.
    _phonemesByNote = null;
  }

  /// Render the selected track from the authoritative session. The engine
  /// applies that track's fader, pan and UMixFx and returns stereo WAV.
  Future<void> _renderRaw() async {
    final session = _session;
    if (session == null) return;
    final wav = await _client.renderProjectSession(
      sessionId: session.sessionId,
      expectedRevision: session.revision,
      voicebank: 'teto-english',
      engine: _engine,
      track: _selectedTrack,
    );
    _latestAudio = wav;
    _audioDirty = false;
    debugPrint('rendered stereo track: ${wav.length} bytes');
  }

  /// Play button → ensure fresh audio (render if stale, re-FX if mixer
  /// changed) and play it. ALWAYS renders fresh on first play; after an
  /// edit, `_scheduleReRender` keeps the raw audio fresh — but a mixer
  /// drag only re-FXes, never re-synthesizes.
  Future<void> _playDemo() async {
    // Fresh audio only: a stale `_latestAudio` (edit/engine switch since
    // the last render) must NEVER be played — render first, then play.
    if (_latestAudio != null && !_audioDirty) {
      final player = _player ??= AudioPlayer();
      _ensurePositionListener(player);
      setState(() => _isPlaying = true);
      _trackPlaybackEnd(player);
      await player.play(BytesSource(_latestAudio!));
      _showSnack('Playing…');
      return;
    }
    _showSnack('Rendering… (first time takes a while)');
    setState(() => _renderActive = true);
    try {
      await _renderRaw();
      final player = _player ??= AudioPlayer();
      _ensurePositionListener(player);
      setState(() => _isPlaying = true);
      _trackPlaybackEnd(player);
      await player.play(BytesSource(_latestAudio!));
      _showSnack('Playing…');
    } catch (e) {
      if (e.toString().contains('render cancelled')) {
        _showSnack('Rendering cancelled');
      } else {
        debugPrint('play failed: $e');
        _showSnack('Play failed: $e');
      }
    } finally {
      if (mounted) setState(() => _renderActive = false);
    }
  }

  /// `await player.play()` returns as soon as playback STARTS, not when it
  /// ends — so `_isPlaying` must be cleared by the player's completion
  /// event (or the Stop button), never by the awaited play call.
  void _trackPlaybackEnd(AudioPlayer player) {
    player.onPlayerComplete.listen((_) {
      if (mounted) {
        setState(() => _isPlaying = false);
        // Push the final position = real duration so the roll's playhead
        // reaches exactly 1.0 and stops (its stop logic runs on
        // position >= duration — without this a wav shorter than the
        // beats estimate leaves the playhead stuck before the end).
        if (_lastPlaybackDuration > 0 && !_playbackPositions.isClosed) {
          _playbackPositions.add(_lastPlaybackDuration);
        }
      }
    });
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _initializeSession());
  }

  @override
  void dispose() {
    _editDebounce?.cancel();
    _autosaveTimer?.cancel();
    _playbackPositions.close();
    _playbackDuration.close();
    _player?.dispose();
    super.dispose();
  }

  /// Export → `POST /render` on the engine server (CONTRACT-v1 §4), then
  /// SAVE the wav to public Downloads/rusted Utau/ via MediaStore so the user can
  /// find the file (Files app → Downloads → rusted Utau).
  ///
  /// Render the current Rust session and save its audio artifact.
  Future<void> _exportAudio() async {
    if (_exporting) return;
    final messenger = ScaffoldMessenger.of(context);
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(
        const SnackBar(
          content: Text('Rendering current project…'),
          behavior: SnackBarBehavior.floating,
          duration: Duration(seconds: 2),
        ),
      );
    setState(() {
      _exporting = true;
      _renderActive = true;
    });
    try {
      final session = _session;
      if (session == null)
        throw const ApiException('project session is not ready');
      final wav = await _client.renderProjectSession(
        sessionId: session.sessionId,
        expectedRevision: session.revision,
        voicebank: 'teto-english',
        engine: _engine,
        track: _selectedTrack,
      );
      final durationMs = _wavDurationMillis(wav);

      // Save to public Downloads/rusted Utau (Android 10+ MediaStore — no runtime
      // permission needed). On desktop/tests the channel is absent → null.
      final name = 'rusted-utau-${DateTime.now().millisecondsSinceEpoch}.wav';
      final uri = await EnginePaths.saveWav(wav, name);

      if (!mounted) return;
      messenger
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(
              uri != null
                  ? 'Saved: Downloads/rusted Utau/$name · ${_formatBytes(wav.length)} · ~$durationMs ms'
                  : 'Rendered ${_formatBytes(wav.length)} · ~$durationMs ms (save only on Android)',
            ),
            behavior: SnackBarBehavior.floating,
            backgroundColor: RustedUtauColors.green.withValues(alpha: 0.85),
            duration: const Duration(seconds: 6),
          ),
        );
    } on ApiException catch (e) {
      if (!mounted) return;
      messenger
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(
              e.message == 'render cancelled'
                  ? 'Rendering cancelled'
                  : 'Render failed: ${e.message}',
            ),
            behavior: SnackBarBehavior.floating,
            backgroundColor: e.message == 'render cancelled'
                ? RustedUtauColors.green.withValues(alpha: 0.85)
                : const Color(0xFF8A3B3B),
            duration: const Duration(seconds: 6),
          ),
        );
    } catch (e) {
      if (!mounted) return;
      messenger
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text('Render failed: $e'),
            behavior: SnackBarBehavior.floating,
            backgroundColor: const Color(0xFF8A3B3B),
            duration: const Duration(seconds: 6),
          ),
        );
    } finally {
      if (mounted) {
        setState(() {
          _exporting = false;
          _renderActive = false;
        });
      }
    }
  }

  /// Stop the audio player (called by the roll when playback is toggled
  /// off or the playhead finishes). The roll's playhead animation is its
  /// own; without this hook the sound kept playing after the graphic
  /// stopped ("graphic หยุดแต่เสียงไม่").
  void _stopPlayback() {
    _player?.stop();
    if (mounted && _isPlaying) setState(() => _isPlaying = false);
  }

  /// Cancel the in-flight render: hard-Stop on the server (the worker
  /// bails between chunks) + epoch bump so any late result is discarded.
  /// Also stops any audio currently playing ("ยังขัดไม่ได้" — the Stop
  /// item previously only appeared while rendering, never during playback).
  void _cancelRenderNow() {
    _stopPlayback();
    _client.cancel(); // async; server aborts between chunks
    setState(() {
      _renderActive = false;
      _exporting = false;
      _isPlaying = false;
    });
    final messenger = ScaffoldMessenger.of(context);
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(
        const SnackBar(
          content: Text('Stopping…'),
          behavior: SnackBarBehavior.floating,
          duration: Duration(seconds: 2),
        ),
      );
  }

  Future<void> _applyTrackFx(Map<String, dynamic>? value) async {
    final trackIndex = _selectedTrack;
    _enqueueSession(() async {
      final session = _session;
      if (session == null || trackIndex >= session.trackViews.length) return;
      final trackId = session.trackViews[trackIndex]['id'] as String?;
      if (trackId == null) return;
      await _sendSessionCommand({
        'type': 'track.set_mix_fx',
        'track_id': trackId,
        'mix_fx': value,
      });
      await _refreshSessionSnapshot(session.sessionId);
      _audioDirty = true;
      _scheduleReRender();
      _scheduleAutosave();
    });
    await _sessionQueue;
  }

  Future<void> _onVibratoChanged(
    String noteId,
    Map<String, dynamic> vibrato,
  ) async {
    _enqueueSession(() async {
      final session = _session;
      if (session == null) return;
      await _sendSessionCommand({
        'type': 'note.set_vibrato',
        'note_id': noteId,
        'vibrato': vibrato,
      });
      await _refreshSessionSnapshot(session.sessionId);
      _audioDirty = true;
      _scheduleReRender();
      _scheduleAutosave();
    });
    await _sessionQueue;
  }

  static String _formatBytes(int bytes) {
    if (bytes >= 1024 * 1024) {
      return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    }
    return '${(bytes / 1024).toStringAsFixed(0)} KB';
  }

  static int _wavDurationMillis(Uint8List wav) {
    if (wav.length < 44) return 0;
    final header = ByteData.sublistView(wav);
    final channels = header.getUint16(22, Endian.little);
    final sampleRate = header.getUint32(24, Endian.little);
    final dataBytes = header.getUint32(40, Endian.little);
    if (channels == 0 || sampleRate == 0) return 0;
    return (dataBytes / (channels * 2 * sampleRate) * 1000).round();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        titleSpacing: 0,
        leading: IconButton(
          icon: const Icon(Icons.chevron_left_rounded, size: 28),
          tooltip: 'Back',
          onPressed: () {},
        ),
        title: const Text(
          'midnight_demo / Verse 01',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
        ),
        centerTitle: true,
        actions: [
          // Engine select: WORLDLINE-R (r1) / WORLDLINE-R2 (r2) /
          // Classic — the server routes per request (engine field).
          Padding(
            padding: const EdgeInsets.only(right: 4),
            child: Center(
              child: DropdownButton<String>(
                value: _engine,
                underline: const SizedBox.shrink(),
                icon: const Icon(Icons.expand_more_rounded, size: 16),
                style: const TextStyle(fontSize: 12, color: Colors.white70),
                dropdownColor: const Color(0xFF2A2F3A),
                items: const [
                  DropdownMenuItem(value: 'r1', child: Text('WORLDLINE-R')),
                  DropdownMenuItem(value: 'r2', child: Text('WORLDLINE-R2')),
                  DropdownMenuItem(value: 'classic', child: Text('Classic')),
                  DropdownMenuItem(
                    value: 'diffsinger',
                    child: Text('DiffSinger'),
                  ),
                ],
                onChanged: (v) {
                  if (v != null && v != _engine) {
                    // Engine switch invalidates every rendered chunk (the
                    // server cache is salted per engine) — drop the stale
                    // audio and re-render the song with the new engine, so
                    // Play never hears the old engine's rendering.
                    setState(() {
                      _engine = v;
                      _latestAudio = null;
                      _audioDirty = true;
                    });
                    _showSnack('Engine: ${_engineLabel(v)} — re-rendering…');
                    _scheduleReRender();
                  }
                },
              ),
            ),
          ),
          IconButton(
            icon: const Icon(Icons.save_outlined, size: 20),
            tooltip: 'Save project (.ustx)',
            onPressed: () async {
              final path = await _saveProject();
              _showSnack(
                path == null
                    ? 'Save failed'
                    : 'Saved → ${path.split('/').last}',
              );
            },
          ),
          IconButton(
            icon: const Icon(Icons.undo_rounded, size: 20),
            tooltip: 'Undo',
            onPressed: _session?.canUndo == true
                ? () => _changeHistory(undo: true)
                : null,
          ),
          IconButton(
            icon: const Icon(Icons.redo_rounded, size: 20),
            tooltip: 'Redo',
            onPressed: _session?.canRedo == true
                ? () => _changeHistory(undo: false)
                : null,
          ),
          PopupMenuButton<String>(
            icon: const Icon(Icons.more_horiz_rounded, size: 20),
            tooltip: 'More',
            color: const Color(0xFF2E2E2E),
            onSelected: (v) {
              if (v == 'play') _playDemo();
              if (v == 'export') _exportAudio();
              if (v == 'stop') _cancelRenderNow();
              if (v == 'save') _saveProjectAs();
              if (v == 'autosave') _saveProject();
              if (v == 'open') _pickAndOpenProject();
            },
            itemBuilder: (context) => [
              const PopupMenuItem(
                value: 'play',
                child: Text('▶  Play', style: TextStyle(fontSize: 13)),
              ),
              PopupMenuItem(
                value: (_renderActive || _isPlaying) ? 'stop' : 'export',
                child: Text(
                  (_renderActive || _isPlaying)
                      ? '⏹  Stop'
                      : (_exporting ? '⬇  Rendering…' : '⬇  Export audio'),
                  style: const TextStyle(fontSize: 13),
                ),
              ),
              const PopupMenuDivider(),
              const PopupMenuItem(
                value: 'save',
                child: Text('💾  Save As…', style: TextStyle(fontSize: 13)),
              ),
              const PopupMenuItem(
                value: 'open',
                child: Text('📂  Open project', style: TextStyle(fontSize: 13)),
              ),
            ],
          ),
          const SizedBox(width: 4),
        ],
      ),
      body: LayoutBuilder(
        builder: (context, c) {
          final roll = PianoRoll(
            tracks: _tracks,
            selectedTrackIndex: _selectedTrack,
            curvesByTrack: _sessionCurves,
            // The playhead timeline MUST match the render tempo — the
            // roll's old const BPM 128 ran 6% faster than songs rendered
            // at 110 BPM (playhead finished early → tail cut off).
            secondsPerBeat: 60.0 / _bpm,
            // Position-driven playhead: stream the player's REAL audio
            // position so the playhead can never drift from the sound
            // (startup latency, tempo, decode hiccups all covered).
            playbackPositions: _playbackPositions.stream,
            playbackDuration: _playbackDuration.stream,
            // REAL phonemizer labels (server /phonemize) above the notes.
            phonemesByNote: _phonemesByNote,
            onPlayRequested: _playDemo,
            onPlayStopped: _stopPlayback,
            onEditGroupBegin: _beginEditGroup,
            onEditGroupEnd: _endEditGroup,
            onInitialSync: (notes, curve) {
              if (_selectedTrack < _tracks.length) {
                _pendingNotes[_selectedTrack] = List.of(notes);
              }
            },
            onNotesChanged: (notes) => _onNotesChanged(notes),
            onCurveChanged: (points) => _onCurveChanged(points),
            onVibratoChanged: _onVibratoChanged,
          );
          // SingleChildScrollView: when the track FX editor is open it can
          // exceed the viewport (tablet system bars) — scroll instead of
          // clipping. The roll gets a bounded height so its own internal
          // scrolling still works inside the outer scroll view.
          final editor = SingleChildScrollView(
            child: Column(
              children: [
                SizedBox(
                  height: _mixerOpen
                      ? (c.maxHeight * 0.55).clamp(240.0, 480.0)
                      : (c.maxHeight - 30).clamp(200.0, double.infinity),
                  child: c.maxWidth <= 720
                      ? roll
                      : Row(
                          children: [
                            if (!_panelCollapsed) _buildTrackPanel(),
                            Expanded(child: roll),
                          ],
                        ),
                ),
                _buildMixerToggle(),
                if (_mixerOpen)
                  TrackFxPanel(
                    track: _selectedTrack < _tracks.length
                        ? _tracks[_selectedTrack]
                        : null,
                    onApply: _applyTrackFx,
                  ),
              ],
            ),
          );
          return Stack(
            children: [
              // SafeArea: landscape tablets have system bars on the sides
              // and bottom — without it the mixer panel can render off-
              // screen ("mixer ตกจอ").
              SafeArea(child: editor),
              if (c.maxWidth > 720)
                Positioned(
                  left: _panelCollapsed ? 0 : 232,
                  top: 14,
                  child: _collapseButton(),
                ),
            ],
          );
        },
      ),
    );
  }

  Widget _buildTrackPanel() {
    return Container(
      width: 236,
      color: const Color(0xFF242424),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // BPM / Beat / Key chips (OM detailed track header top row)
          Padding(
            padding: const EdgeInsets.fromLTRB(8, 8, 8, 4),
            child: Row(
              children: [
                _headerChip('110 BPM'),
                const SizedBox(width: 4),
                _headerChip('4/4'),
                const SizedBox(width: 4),
                _headerChip('C'),
              ],
            ),
          ),
          const Divider(height: 1, color: Color(0xFF343434)),
          // Track list
          Expanded(
            child: ListView.builder(
              padding: const EdgeInsets.symmetric(vertical: 4),
              itemCount: _tracks.length,
              itemBuilder: (context, i) => _trackRow(i),
            ),
          ),
        ],
      ),
    );
  }

  Widget _headerChip(String label) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: const Color(0xFF343434),
        borderRadius: BorderRadius.circular(4),
      ),
      child: Text(
        label,
        style: const TextStyle(fontSize: 11, color: Color(0xFFDDDDDD)),
      ),
    );
  }

  /// Collapse/expand button at the track panel's right edge (ui-mock's ◀).
  Widget _collapseButton() {
    return Material(
      color: const Color(0xFF17181F),
      child: InkWell(
        onTap: () => setState(() => _panelCollapsed = !_panelCollapsed),
        child: Container(
          width: 20,
          height: 26,
          alignment: Alignment.center,
          decoration: const BoxDecoration(
            border: Border(
              right: BorderSide(color: RustedUtauColors.line),
              top: BorderSide(color: RustedUtauColors.line),
              bottom: BorderSide(color: RustedUtauColors.line),
            ),
            borderRadius: BorderRadius.horizontal(right: Radius.circular(6)),
          ),
          child: Icon(
            _panelCollapsed
                ? Icons.chevron_right_rounded
                : Icons.chevron_left_rounded,
            size: 14,
            color: RustedUtauColors.muted,
          ),
        ),
      ),
    );
  }

  /// Mixer toggle bar above the bottom of the editor (ui-mock's ⊞ Mixer).
  Widget _buildMixerToggle() {
    return InkWell(
      onTap: () => setState(() => _mixerOpen = !_mixerOpen),
      child: Container(
        height: 30,
        alignment: Alignment.center,
        decoration: const BoxDecoration(
          color: Color(0xFF17181F),
          border: Border(top: BorderSide(color: RustedUtauColors.line)),
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              _mixerOpen
                  ? Icons.keyboard_arrow_down_rounded
                  : Icons.tune_rounded,
              size: 14,
              color: _mixerOpen ? RustedUtauColors.purple : RustedUtauColors.muted,
            ),
            const SizedBox(width: 5),
            Text(
              _mixerOpen ? 'Hide track FX' : 'Track FX',
              style: TextStyle(
                fontSize: 11,
                color: _mixerOpen ? RustedUtauColors.purple : RustedUtauColors.muted,
                fontWeight: _mixerOpen ? FontWeight.w700 : FontWeight.w500,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _trackRow(int i) {
    final t = _tracks[i];
    final sel = i == _selectedTrack;
    final color =
        RustedUtauColors.trackColors[t.colorSeed % RustedUtauColors.trackColors.length];
    return InkWell(
      onTap: () {
        if (_selectedTrack != i) {
          setState(() {
            _selectedTrack = i;
            _latestAudio = null;
            _audioDirty = true;
          });
          _phonemesByNote = null; // stale (old track) until re-fetched
          _refreshPhonemes();
          _scheduleReRender();
        }
      },
      child: Container(
        height: 44,
        color: sel ? const Color(0xFF333333) : null,
        child: Row(
          children: [
            // Colored tag strip (OM style)
            Container(width: 4, color: color),
            const SizedBox(width: 8),
            // Avatar circle (OM style)
            Container(
              width: 28,
              height: 28,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                gradient: LinearGradient(
                  colors: [color, Color.lerp(color, Colors.black, 0.4)!],
                ),
              ),
              child: Center(
                child: Text(
                  t.name.isNotEmpty ? t.name[0].toUpperCase() : '?',
                  style: const TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w800,
                    color: Colors.white,
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            // Track name + singer
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    t.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 11,
                      color: sel ? RustedUtauColors.text : const Color(0xFFB5B9C8),
                      fontWeight: sel ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                  Text(
                    'Teto English',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 10,
                      color: sel ? RustedUtauColors.muted : const Color(0xFF666666),
                    ),
                  ),
                ],
              ),
            ),
            // Mute indicator
            Icon(
              Icons.volume_up_rounded,
              size: 14,
              color: sel ? RustedUtauColors.muted : const Color(0xFF555555),
            ),
            const SizedBox(width: 8),
          ],
        ),
      ),
    );
  }
}
