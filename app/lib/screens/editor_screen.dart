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
import '../widgets/mixer_panel.dart';
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

  /// Side panel (Project/Sections) collapsed — mirrors ui-mock's ◀ toggle.
  bool _panelCollapsed = false;

  /// Mixer bottom panel visibility — mirrors ui-mock's ⊞ Mixer toggle.
  bool _mixerOpen = false;

  late final ApiClient _client = widget.client ?? ApiClient();
  // Lazy: only created when the user actually hits play — constructing it in
  // widget tests would hit the audioplayers platform channel and hang.
  AudioPlayer? _player;
  bool _exporting = false;
  /// A render is in flight — the ⋯ menu shows Stop instead of Export.
  bool _renderActive = false;

  /// Debounce timer for re-render-on-edit (fires 500ms after the last edit).
  Timer? _editDebounce;

  /// Latest RAW rendered audio (synth only — NO mixer FX). Re-rendered on
  /// edit; the mixer FX is applied separately at playback time via
  /// `/post-fx`, so fader/EQ drags never re-synthesize.
  Uint8List? _rawAudio;

  /// Latest FINAL audio (raw + current mixer FX). This is what Play uses.
  Uint8List? _latestAudio;

  /// True while audio is actually playing — the menu shows ⏹ Stop so the
  /// user can interrupt playback (previously the Stop item only appeared
  /// during rendering, which is near-instant with the cache → there was
  /// NO way to stop a playing track: "ยังขัดไม่ได้").
  bool _isPlaying = false;

  /// Current mixer FX params JSON (from the MixerPanel); null = no FX
  /// (play the raw synth). Applied via `/post-fx` at playback time.
  String? _mixerParams;

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
  /// Phonemes for the SynthV labels are derived from the lyric's
  /// `word[ph ph]` hint when present (the editor's notes carry them, e.g.
  /// 'la[l A]'), otherwise the lyric is shown without phoneme boxes.
  List<Track> get _editableTracks => [
    for (var i = 0; i < _tracks.length; i++)
      Track(
        name: _tracks[i].name,
        colorSeed: _tracks[i].colorSeed,
        notes: [
          for (final n in _editedNotes[i] ?? _tracks[i].notes)
            n.copyWith(phoneme: _phonemeFromLyric(n.lyric)),
        ],
      ),
  ];

  /// Extract the phoneme hint from a lyric: `word[ph1 ph2]` → "ph1 ph2".
  /// No hint → empty (no labels). This mirrors how the ustx writer
  /// already embeds hints for the phonemizer.
  static String _phonemeFromLyric(String lyric) {
    final m = RegExp(r'\[([^\]]+)\]').firstMatch(lyric);
    return m?.group(1)?.trim() ?? '';
  }

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
  /// the RAW audio so Play is instant. Never interrupts editing.
  void _scheduleReRender() {
    _editDebounce?.cancel();
    _editDebounce = Timer(const Duration(milliseconds: 500), () async {
      try {
        await _renderRaw();
      } catch (e) {
        debugPrint('re-render failed: $e');
      }
    });
  }

  /// Render the current project through the engine (synth only — no mixer
  /// FX), store the raw wav, then re-apply the current mixer FX so
  /// [_latestAudio] stays fresh.
  Future<void> _renderRaw() async {
    final (project, voicebank) = await _renderTarget();
    final wav = await _client.render(
      project: project,
      voicebank: voicebank,
    );
    _rawAudio = wav;
    _latestAudio = await _applyMixerFx(wav);
    debugPrint('rendered raw: ${wav.length} bytes');
  }

  /// Apply the current mixer FX to a raw wav via `/post-fx` (post-synth —
  /// no re-synthesis). Returns the raw wav unchanged when no FX is set.
  Future<Uint8List> _applyMixerFx(Uint8List raw) async {
    final params = _mixerParams;
    if (params == null || params.isEmpty) return raw;
    try {
      return await _client.postFx(rawWav: raw, paramsJson: params);
    } catch (e) {
      debugPrint('post-fx failed (playing raw): $e');
      return raw;
    }
  }

  /// Play button → ensure fresh audio (render if stale, re-FX if mixer
  /// changed) and play it. ALWAYS renders fresh on first play; after an
  /// edit, `_scheduleReRender` keeps the raw audio fresh — but a mixer
  /// drag only re-FXes, never re-synthesizes.
  Future<void> _playDemo() async {
    // If we have fresh audio already, play it directly (no render).
    if (_latestAudio != null) {
      final player = _player ??= AudioPlayer();
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
      if (mounted) setState(() => _isPlaying = false);
    });
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
    setState(() {
      _exporting = true;
      _renderActive = true;
    });
    final epoch = _client.renderEpoch;
    try {
      final (project, voicebank) = await _renderTarget();
      final wav = await _client.render(
        project: project,
        voicebank: voicebank,
        epoch: epoch,
      );
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
            content: Text(
              e.message == 'render cancelled'
                  ? 'Rendering cancelled'
                  : 'Render failed: ${e.message}',
            ),
            behavior: SnackBarBehavior.floating,
            backgroundColor: e.message == 'render cancelled'
                ? LiltColors.green.withValues(alpha: 0.85)
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

  /// Mixer panel changes → store the params and re-apply the FX to the
  /// RAW audio via `/post-fx` (post-synth — milliseconds, NO re-render).
  /// Debounced: fader drags fire dozens of events per second.
  Timer? _mixerDebounce;
  void _onMixerParams(String paramsJson) {
    _mixerParams = paramsJson;
    _mixerDebounce?.cancel();
    _mixerDebounce = Timer(const Duration(milliseconds: 250), () async {
      try {
        final raw = _rawAudio;
        if (raw == null) return; // nothing rendered yet — Play will FX
        _latestAudio = await _applyMixerFx(raw);
      } catch (e) {
        debugPrint('mixer fx failed: $e');
      }
    });
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
              if (v == 'stop') _cancelRenderNow();
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
            onPlayStopped: _stopPlayback,
            onInitialSync: (notes, curve) {
              _editedNotes[_selectedTrack] = List.of(notes);
              _editedCurves[_selectedTrack] = List.of(curve);
            },
            onNotesChanged: (notes) {
              _editedNotes[_selectedTrack] = List.of(notes);
              _scheduleReRender();
            },
            onCurveChanged: (points) {
              _editedCurves[_selectedTrack] = List.of(points);
              _scheduleReRender();
            },
          );
          // SingleChildScrollView: when the mixer is open the panel can
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
                  MixerPanel(
                    tracks: _editableTracks,
                    onParamsChanged: _onMixerParams,
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
              right: BorderSide(color: LiltColors.line),
              top: BorderSide(color: LiltColors.line),
              bottom: BorderSide(color: LiltColors.line),
            ),
            borderRadius: BorderRadius.horizontal(right: Radius.circular(6)),
          ),
          child: Icon(
            _panelCollapsed ? Icons.chevron_right_rounded : Icons.chevron_left_rounded,
            size: 14,
            color: LiltColors.muted,
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
          border: Border(top: BorderSide(color: LiltColors.line)),
        ),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              _mixerOpen ? Icons.keyboard_arrow_down_rounded : Icons.tune_rounded,
              size: 14,
              color: _mixerOpen ? LiltColors.purple : LiltColors.muted,
            ),
            const SizedBox(width: 5),
            Text(
              _mixerOpen ? 'Hide mixer' : 'Mixer',
              style: TextStyle(
                fontSize: 11,
                color: _mixerOpen ? LiltColors.purple : LiltColors.muted,
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
