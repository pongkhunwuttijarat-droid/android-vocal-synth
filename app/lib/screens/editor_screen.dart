import 'dart:async';
import 'dart:io' show File, Platform;

import 'package:audioplayers/audioplayers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import '../api/engine_client.dart';
import '../api/engine_paths.dart';
import '../editor_ustx.dart';
import '../models.dart';
import '../theme.dart';
import '../widgets/piano_roll.dart';

/// v1 demo render target (CONTRACT-v1 §4/§5): export always renders the
/// FIXED demo project on the engine machine — note editing → .ustx project
/// is out of scope for v1.
///
/// On Android the bundled server runs inside the app and the paths come
/// from the platform channel (EnginePaths); on desktop dev these host-side
/// defaults are used.
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
  static const List<Track> _defaultTracks = [
    // Vocal scale demo — "la la la" (l A): C major, 2 octaves up + down.
    // Phonemes l/A exist as singles AND as the "l A" pair in Teto English,
    // so the scale renders gap-free (unlike the previous song demos).
    Track(
      name: 'Vocal Scale',
      colorSeed: 0,
      notes: [
        Note(lyric: 'la[l A]', pitch: 0, position: 0.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 2, position: 0.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 4, position: 1.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 5, position: 1.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 7, position: 2.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 9, position: 2.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 11, position: 3.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 12, position: 3.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 14, position: 4.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 16, position: 4.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 17, position: 5.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 19, position: 5.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 21, position: 6.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 23, position: 6.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 24, position: 7.0, duration: 0.5),
        // Descending: C6 → C4 (14 notes)
        Note(lyric: 'la[l A]', pitch: 23, position: 7.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 21, position: 8.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 19, position: 8.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 17, position: 9.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 16, position: 9.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 14, position: 10.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 12, position: 10.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 11, position: 11.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 9, position: 11.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 7, position: 12.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 5, position: 12.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 4, position: 13.0, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 2, position: 13.5, duration: 0.5),
        Note(lyric: 'la[l A]', pitch: 0, position: 14.0, duration: 1.0),
      ],
    ),
  ];

  int _selectedTrack = 0;

  late final ApiClient _client = widget.client ?? ApiClient();
  // Lazy: only created when the user actually hits play — constructing it in
  // widget tests would hit the audioplayers platform channel and hang.
  AudioPlayer? _player;
  bool _exporting = false;

  /// Debounce timer for re-render-on-edit (fires 500ms after the last edit).
  Timer? _editDebounce;

  /// Latest rendered audio (kept fresh by `_scheduleReRender` after every
  /// edit) — Play uses this without re-rendering when available.
  Uint8List? _latestAudio;

  /// Current project tracks (default demo, replaced by Open project).
  List<Track> _tracks = List.of(_defaultTracks);

  /// Project tempo — the Vocal Scale demo is authored at 110 BPM. This must
  /// match the demo or the rendered audio differs from the engine reference
  /// (beat→ms conversion uses it for notes AND curve points).
  final int _bpm = 110;

  /// Edited notes per track index (preserved across track switches — the
  /// roll reloads from here instead of the const originals, so edits are
  /// NOT lost when switching tracks).
  final Map<int, List<Note>> _editedNotes = {};

  /// Edited pitch curves per track index (same preservation rule).
  final Map<int, List<PitchPoint>> _editedCurves = {};

  /// Tracks handed to the roll: originals with any per-track edits merged
  /// in, so a switch back to an edited track shows the edited notes.
  List<Track> get _editableTracks => [
    for (var i = 0; i < _tracks.length; i++)
      Track(
        name: _tracks[i].name,
        colorSeed: _tracks[i].colorSeed,
        notes: _editedNotes[i] ?? _tracks[i].notes,
      ),
  ];

  /// Resolve the render target for this platform: build a .ustx from the
  /// editor's CURRENT notes (all tracks) and persist it where the engine
  /// can read it — filesDir/engine/ on Android (via channel), a temp file
  /// on desktop. Returns (projectPath, voicebankName).
  Future<(String project, String voicebank)> _renderTarget() async {
    final ustx = _buildCurrentUstx();
    if (!kIsWeb && Platform.isAndroid) {
      final paths = await EnginePaths.tryResolve();
      if (paths != null) {
        final written = await EnginePaths.writeProject(
          ustx,
          fileName: 'editor-song.ustx',
        );
        if (written != null) {
          return (written, 'teto-english');
        }
      }
    }
    // Desktop dev: write to a temp file the host server can read.
    final tmp = File('/tmp/lilt-editor-song.ustx');
    tmp.writeAsStringSync(ustx);
    return (tmp.path, kDemoRenderVoicebank);
  }

  /// Serialize the editor's current state (every track + its curve).
  String _buildCurrentUstx() {
    return buildUstx(
      name: 'Lilt Editor Song',
      bpm: _bpm,
      tracks: [
        for (var i = 0; i < _editableTracks.length; i++)
          TrackNotes(
            name: _editableTracks[i].name,
            notes: _editedNotes[i] ?? const [],
          ),
      ],
      curvesByTrack: _editedCurves,
    );
  }

  /// Save the current project as a .ustx file (app documents dir).
  Future<String?> _saveProject() async {
    try {
      final dir = await getApplicationDocumentsDirectory();
      final file = File('${dir.path}/lilt-project.ustx');
      await file.writeAsString(_buildCurrentUstx());
      debugPrint('saved project → ${file.path}');
      return file.path;
    } catch (e) {
      debugPrint('save project failed: $e');
      return null;
    }
  }

  /// Open a .ustx project file and load its tracks into the editor.
  Future<bool> _openProject(String path) async {
    try {
      final yaml = await File(path).readAsString();
      final parsed = parseUstx(yaml);
      if (parsed.isEmpty) return false;
      final loaded = <Track>[
        for (var i = 0; i < parsed.length; i++)
          Track(name: parsed[i].name, colorSeed: i, notes: parsed[i].notes),
      ];
      setState(() {
        _tracks = loaded;
        _editedNotes
          ..clear()
          ..addAll({
            for (var i = 0; i < loaded.length; i++) i: List.of(loaded[i].notes),
          });
        _selectedTrack = 0;
      });
      return true;
    } catch (e) {
      debugPrint('open project failed: $e');
      return false;
    }
  }

  /// Open-project flow: list .ustx files in the documents dir and let the
  /// user pick one (POC: no system file picker yet).
  Future<void> _pickAndOpenProject() async {
    try {
      final dir = await getApplicationDocumentsDirectory();
      if (!mounted) return;
      final files = dir
          .listSync()
          .whereType<File>()
          .where((f) => f.path.endsWith('.ustx'))
          .toList();
      if (files.isEmpty) {
        _showSnack('No .ustx projects in documents yet — save first');
        return;
      }
      final chosen = await showDialog<String>(
        context: context,
        builder: (ctx) => SimpleDialog(
          title: const Text('Open project'),
          children: [
            for (final f in files)
              SimpleDialogOption(
                onPressed: () => Navigator.pop(ctx, f.path),
                child: Text(f.path.split('/').last),
              ),
          ],
        ),
      );
      if (chosen != null) {
        final ok = await _openProject(chosen);
        if (!mounted) return;
        _showSnack(ok ? 'Opened $chosen' : 'Could not open project file');
      }
    } catch (e) {
      _showSnack('Open failed: $e');
    }
  }

  void _showSnack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(msg)));
  }

  /// Re-render quietly 500ms after the last edit (debounced). Uses the
  /// engine render cache, so an unchanged edit round-trips fast; stores
  /// the audio so Play is instant. Never interrupts editing.
  void _scheduleReRender() {
    _editDebounce?.cancel();
    _editDebounce = Timer(const Duration(milliseconds: 500), () async {
      try {
        final (project, voicebank) = await _renderTarget();
        final wav = await _client.render(
          project: project,
          voicebank: voicebank,
        );
        _latestAudio = wav;
        debugPrint('re-render after edit: ${wav.length} bytes');
      } catch (e) {
        debugPrint('re-render failed: $e');
      }
    });
  }

  /// Play button → render the demo project through the engine and play the
  /// resulting wav (POC: fixed demo project, like export).
  Future<void> _playDemo() async {
    // First render on-device is slow (WORLD analysis ~1-2 min, then
    // cached). Tell the user we're rendering, or the audio appears to
    // "arrive after the song ended". After an edit, `_latestAudio` is
    // already fresh (see `_scheduleReRender`) — play it directly.
    if (_latestAudio != null) {
      final player = _player ??= AudioPlayer();
      await player.play(BytesSource(_latestAudio!));
      _showSnack('Playing…');
      return;
    }
    _showSnack('Rendering… (first time takes a while)');
    try {
      final (project, voicebank) = await _renderTarget();
      final wav = await _client.render(project: project, voicebank: voicebank);
      _latestAudio = wav;
      final player = _player ??= AudioPlayer();
      // audioplayers BytesSource plays the in-memory wav directly.
      await player.play(BytesSource(wav));
      _showSnack('Playing…');
    } catch (e) {
      debugPrint('play failed: $e');
      _showSnack('Play failed: $e');
    }
  }

  @override
  void dispose() {
    _editDebounce?.cancel();
    _player?.dispose();
    super.dispose();
  }

  /// Export → `POST /render` on the engine server (CONTRACT-v1 §4), then
  /// SAVE the wav to public Downloads/Lilt/ via MediaStore so the user can
  /// find the file (Files app → Downloads → Lilt).
  ///
  /// v1 renders the fixed demo project ([kDemoRenderProject] /
  /// [kDemoRenderVoicebank]).
  Future<void> _exportAudio() async {
    if (_exporting) return;
    final messenger = ScaffoldMessenger.of(context);
    messenger
      ..hideCurrentSnackBar()
      ..showSnackBar(
        const SnackBar(
          content: Text('Rendering demo project on engine…'),
          behavior: SnackBarBehavior.floating,
          duration: Duration(seconds: 2),
        ),
      );
    setState(() => _exporting = true);
    try {
      final (project, voicebank) = await _renderTarget();
      final wav = await _client.render(project: project, voicebank: voicebank);
      // 16-bit mono @ 44100 Hz (worldline SAMPLE_RATE).
      final durationMs = (wav.length / (2 * 44100) * 1000).round();

      // Save to public Downloads/Lilt (Android 10+ MediaStore — no runtime
      // permission needed). On desktop/tests the channel is absent → null.
      final name = 'lilt-${DateTime.now().millisecondsSinceEpoch}.wav';
      final uri = await EnginePaths.saveWav(wav, name);

      if (!mounted) return;
      messenger
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text(
              uri != null
                  ? 'Saved: Downloads/Lilt/$name · ${_formatBytes(wav.length)} · ~$durationMs ms'
                  : 'Rendered ${_formatBytes(wav.length)} · ~$durationMs ms (save only on Android)',
            ),
            behavior: SnackBarBehavior.floating,
            backgroundColor: LiltColors.green.withValues(alpha: 0.85),
            duration: const Duration(seconds: 6),
          ),
        );
    } on ApiException catch (e) {
      if (!mounted) return;
      messenger
        ..hideCurrentSnackBar()
        ..showSnackBar(
          SnackBar(
            content: Text('Render failed: ${e.message}'),
            behavior: SnackBarBehavior.floating,
            backgroundColor: const Color(0xFF8A3B3B),
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
      if (mounted) setState(() => _exporting = false);
    }
  }

  static String _formatBytes(int bytes) {
    if (bytes >= 1024 * 1024) {
      return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    }
    return '${(bytes / 1024).toStringAsFixed(0)} KB';
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
            onPressed: () {},
          ),
          IconButton(
            icon: const Icon(Icons.redo_rounded, size: 20),
            tooltip: 'Redo',
            onPressed: () {},
          ),
          PopupMenuButton<String>(
            icon: const Icon(Icons.more_horiz_rounded, size: 20),
            tooltip: 'More',
            color: const Color(0xFF2E2E2E),
            onSelected: (v) {
              if (v == 'play') _playDemo();
              if (v == 'export') _exportAudio();
              if (v == 'open') _pickAndOpenProject();
            },
            itemBuilder: (context) => [
              const PopupMenuItem(
                value: 'play',
                child: Text('▶  Play', style: TextStyle(fontSize: 13)),
              ),
              PopupMenuItem(
                value: 'export',
                child: Text(
                  _exporting ? '⬇  Rendering…' : '⬇  Export audio',
                  style: const TextStyle(fontSize: 13),
                ),
              ),
              const PopupMenuDivider(),
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
            tracks: _editableTracks,
            selectedTrackIndex: _selectedTrack,
            onPlayRequested: _playDemo,
            onNotesChanged: (notes) {
              _editedNotes[_selectedTrack] = List.of(notes);
              _scheduleReRender();
            },
            onCurveChanged: (points) {
              _editedCurves[_selectedTrack] = List.of(points);
              _scheduleReRender();
            },
          );
          if (c.maxWidth <= 720) return roll;
          return Row(
            children: [
              _buildTrackPanel(),
              Expanded(child: roll),
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

  Widget _trackRow(int i) {
    final t = _tracks[i];
    final sel = i == _selectedTrack;
    final color =
        LiltColors.trackColors[t.colorSeed % LiltColors.trackColors.length];
    return InkWell(
      onTap: () => setState(() => _selectedTrack = i),
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
                      color: sel ? LiltColors.text : const Color(0xFFB5B9C8),
                      fontWeight: sel ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                  Text(
                    'Teto English',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 10,
                      color: sel ? LiltColors.muted : const Color(0xFF666666),
                    ),
                  ),
                ],
              ),
            ),
            // Mute indicator
            Icon(
              Icons.volume_up_rounded,
              size: 14,
              color: sel ? LiltColors.muted : const Color(0xFF555555),
            ),
            const SizedBox(width: 8),
          ],
        ),
      ),
    );
  }
}
