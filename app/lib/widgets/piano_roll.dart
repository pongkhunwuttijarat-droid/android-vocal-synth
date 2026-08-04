import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../models.dart';
import '../theme.dart';

/// Editor tool selected in the piano-roll toolbar.
enum PianoRollTool { select, draw, split, erase }

/// Piano-roll editor widget: toolbar, note grid with continuous pitch curve,
/// and transport bar. Mirrors ui-mock/index.html. POC-quality — all state
/// lives here, no provider/bloc dependencies.
class PianoRoll extends StatefulWidget {
  const PianoRoll({
    super.key,
    required this.tracks,
    this.selectedTrackIndex = 0,
    this.onPlayRequested,
    this.onNotesChanged,
    this.onCurveChanged,
  });

  /// All project tracks; the roll renders [selectedTrackIndex]'s notes.
  final List<Track> tracks;

  /// Index of the track whose notes are shown/edited.
  final int selectedTrackIndex;

  /// Called when the user hits play. The parent (EditorScreen) renders the
  /// project through the engine and plays the audio; the roll itself only
  /// animates the playhead. Null = playhead animation only (no audio).
  final Future<void> Function()? onPlayRequested;

  /// Called whenever the edited note list changes (add/move/edit lyric), so
  /// the parent can keep its project model in sync for export/play.
  final void Function(List<Note> notes)? onNotesChanged;

  /// Called whenever the pitch curve changes — export/play embed these
  /// points in the ustx so the engine bends the f0 (without this, drawn
  /// curves are silently dropped).
  final void Function(List<PitchPoint> points)? onCurveChanged;

  @override
  State<PianoRoll> createState() => _PianoRollState();
}

// --- Geometry & palette constants (kept in sync with ui-mock/index.html) ---
const double _keysWidth = 60.0; // OpenUtau Mobile / index-om.html
const double _rowHeight = 32.0; // px per semitone
const double _timelineHeight = 22.0; // OpenUtau Mobile timeline overlay
// Keep the app's full A0..C8 range, while using the OpenUtau Mobile visual
// treatment from the mock for the rendered viewport.
const int _rows = 88;
const int _topPitch = 48; // C8, relative to C4
const int _bottomPitch = _topPitch - _rows + 1; // A0
const double _noteHeight = 24.0;
const double _basePxPerBeat = 80.0; // px per beat at 100% zoom
const double _secondsPerBeat = 60.0 / 128.0; // BPM 128

// OpenUtau Mobile dark theme values from ui-mock/index-om.html.
const Color _bg = Color(0xFF303030);
const Color _rowBg = Color(0xFF3C3C3C);
const Color _rowLine = Color(0x1AFFFFFF);
const Color _beatLine = Color(0x0DFFFFFF);
const Color _barLine = Color(0x2EFFFFFF);
const Color _keysBg = Color(0xFFCC2A63);
const Color _keysBlack = Color(0x00000000);
const Color _toolbarBg = Color(0xFF17181F);
const Color _orange = Color(0xFFFFC16D);
const Color _pink = Color(0xFFFE71A3);
const Color _purple2 = Color(0xFF7358E9);

const List<String> _noteNames = [
  'C',
  'C#',
  'D',
  'D#',
  'E',
  'F',
  'F#',
  'G',
  'G#',
  'A',
  'A#',
  'B',
];
const Set<int> _blackKeyClasses = {1, 3, 6, 8, 10};

/// Pitch (semitones from C4) -> label, e.g. 0 -> "C4", 10 -> "A#4".
String _pitchName(int semis) {
  final idx = ((semis % 12) + 12) % 12;
  final octave = 4 + (semis - idx) ~/ 12;
  return '${_noteNames[idx]}$octave';
}

String _fmtDur(double d) =>
    d == d.roundToDouble() ? d.round().toString() : d.toStringAsFixed(2);

/// Snap step (beats) -> label, e.g. 0.0625 -> "1/16".
String _fmtSnap(double beats) {
  final n = (1 / beats).round();
  return n == 1 ? '1/1' : '1/$n';
}

String _fmtTime(double t) {
  final m = t ~/ 60;
  final s = (t % 60).floor();
  final ms = ((t * 1000) % 1000).floor();
  return '${m.toString().padLeft(2, '0')}:${s.toString().padLeft(2, '0')}'
      '.${ms.toString().padLeft(3, '0')}';
}

class _PianoRollState extends State<PianoRoll>
    with SingleTickerProviderStateMixin {
  late List<Note> _notes;
  Note? _selectedNote;
  Note? _dragNote;
  Offset _dragStartPos = Offset.zero;
  double _dragStartBeats = 0;
  int _dragStartPitch = 0;
  bool _dragMoved = false;

  /// Resize mode: dragging the right edge of a note changes its duration.
  Note? _resizeNote;
  double _resizeStartDur = 0;

  /// True while the lyric dialog is open (guards against double-open).
  bool _lyricDialogOpen = false;

  /// Long-press timer for context menu (like mock's setTimeout 550ms).
  Timer? _longPressTimer;

  /// Read-only view of the edited notes for widget tests.
  @visibleForTesting
  List<Note> get debugNotes => List.unmodifiable(_notes);

  /// Read-only view of the editable curve points for widget tests.
  @visibleForTesting
  List<PitchPoint> get debugCurvePoints => List.unmodifiable(_curvePoints);

  PianoRollTool _tool = PianoRollTool.select;
  bool _showPitch = true;
  int _zoomPercent = 90;
  int _revision = 0; // bumped on every edit so the painter repaints

  /// Snap grid in beats (1/1, 1/2, 1/4, 1/8, 1/16). Used when dragging notes
  /// and when drawing new ones.
  double _snapBeat = 1 / 16;

  /// Editable pitch-curve points — the curve is its own object, independent
  /// from notes. Each point is (beat, semitones from the note reference).
  late List<PitchPoint> _curvePoints;

  /// Curve editing: index of the point being dragged (-1 = none).
  int _dragCurveIdx = -1;
  double _curveDragStartY = 0;
  double _curveDragStartSemis = 0;

  late final AnimationController _playCtrl;
  bool _playing = false;
  final ScrollController _vScroll = ScrollController(initialScrollOffset: 90);
  final ScrollController _hScroll = ScrollController();

  double get _ppb => _basePxPerBeat * _zoomPercent / 100;
  double get _totalBeats =>
      _notes.fold(0.0, (m, n) => math.max(m, n.position + n.duration));

  Track? get _activeTrack {
    final tracks = widget.tracks;
    if (tracks.isEmpty) return null;
    final i = widget.selectedTrackIndex < 0
        ? 0
        : (widget.selectedTrackIndex >= tracks.length
              ? tracks.length - 1
              : widget.selectedTrackIndex);
    return tracks[i];
  }

  Color get _trackColor {
    final t = _activeTrack;
    if (t == null) return LiltColors.purple;
    return LiltColors.trackColors[t.colorSeed % LiltColors.trackColors.length];
  }

  double _noteTopY(int pitch) =>
      (_topPitch - pitch) * _rowHeight + (_rowHeight - _noteHeight) / 2;
  double _noteCenterY(double pitch) =>
      (_topPitch - pitch) * _rowHeight + _rowHeight / 2;

  Rect _noteRect(Note n) => Rect.fromLTWH(
    n.position * _ppb,
    _noteTopY(n.pitch),
    n.duration * _ppb,
    _noteHeight,
  );

  /// Build the curve once from the current notes (one point per note center,
  /// at the note's pitch). Free-form points added by dragging the line
  /// survive.
  void _syncCurveFromNotes() {
    _curvePoints = [
      for (final n in _notes)
        PitchPoint(n.position + n.duration / 2, n.pitch.toDouble()),
    ]..sort((a, b) => a.beat.compareTo(b.beat));
  }

  /// After a note moves/resizes, keep its center anchor glued to the note:
  /// follow its beat always; follow its pitch while the user hasn't hand-
  /// edited that point (semitones still equal to the note pitch).
  void _syncNoteAnchors() {
    for (final n in _notes) {
      final center = n.position + n.duration / 2;
      final i = _curvePoints.indexWhere((p) => (p.beat - center).abs() < 0.05);
      if (i >= 0) {
        final p = _curvePoints[i];
        final followsPitch = (p.semitones - n.pitch).abs() < 0.001;
        _curvePoints[i] = PitchPoint(
          center,
          followsPitch ? n.pitch.toDouble() : p.semitones,
        );
      }
    }
    _curvePoints.sort((a, b) => a.beat.compareTo(b.beat));
  }

  /// The curve as a smooth cubic path through all curve points.
  Path _curvePath() {
    final pts = <Offset>[
      for (final p in _curvePoints)
        Offset(p.beat * _ppb, _noteCenterY(p.semitones)),
    ];
    if (pts.isEmpty) return Path();
    final path = Path()..moveTo(pts.first.dx, pts.first.dy);
    for (var i = 1; i < pts.length; i++) {
      final p0 = pts[i - 1];
      final p1 = pts[i];
      final mx = (p0.dx + p1.dx) / 2;
      path.cubicTo(mx, p0.dy, mx, p1.dy, p1.dx, p1.dy);
    }
    return path;
  }

  /// Hit-test the curve: grab an existing point (12px) or anywhere on the
  /// line (10px — creates a new point there). Returns the point index to
  /// drag, or -1. NOTE: has the side effect of inserting a point when the
  /// line itself is grabbed; only call from pointer-down.
  int _curveHitTest(Offset pos) {
    for (var i = 0; i < _curvePoints.length; i++) {
      final p = _curvePoints[i];
      final d =
          (Offset(p.beat * _ppb, _noteCenterY(p.semitones)) - pos).distance;
      if (d <= 12) return i;
    }
    if (_curvePoints.length < 2) return -1;
    final metrics = _curvePath().computeMetrics();
    for (final m in metrics) {
      for (var d = 0.0; d <= m.length; d += 6) {
        final t = m.getTangentForOffset(d);
        if (t == null) continue;
        if ((t.position - pos).distance <= 10) {
          final beat = math.max(0.0, pos.dx / _ppb);
          _curvePoints.add(PitchPoint(beat, 0));
          _curvePoints.sort((a, b) => a.beat.compareTo(b.beat));
          _notifyCurveChanged();
          return _curvePoints.indexWhere((p) => (p.beat - beat).abs() < 0.001);
        }
      }
    }
    return -1;
  }

  Note? _hitTest(Offset pos) {
    for (final n in _notes.reversed) {
      if (_noteRect(n).contains(pos)) return n;
    }
    return null;
  }

  @override
  void initState() {
    super.initState();
    _playCtrl = AnimationController(vsync: this)
      ..addStatusListener(_onPlayStatus);
    _notes = List.of(_activeTrack?.notes ?? const []);
    _syncCurveFromNotes();
    if (_notes.length > 3) _selectedNote = _notes[3]; // mock default: "the"
    // Sync the initial note set to the parent (export/play use it).
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _notifyNotesChanged();
      _notifyCurveChanged();
    });
  }

  @override
  void didUpdateWidget(PianoRoll oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.selectedTrackIndex != oldWidget.selectedTrackIndex) {
      _notes = List.of(_activeTrack?.notes ?? const []);
      _selectedNote = null;
      _dragNote = null;
      _dragCurveIdx = -1;
      _syncCurveFromNotes();
      // CRITICAL: sync the freshly loaded track back to the parent — the
      // roll's internal copy just changed, and export/play render the
      // parent's notes. Without this, switching tracks plays the PREVIOUS
      // track's melody ("the track that comes out is a different one").
      _notifyNotesChanged();
      _notifyCurveChanged();
    }
  }

  @override
  void dispose() {
    _playCtrl.dispose();
    _vScroll.dispose();
    _hScroll.dispose();
    super.dispose();
  }

  // --- playback ---

  void _onPlayStatus(AnimationStatus status) {
    if (status == AnimationStatus.completed) {
      _playCtrl.reset();
      setState(() => _playing = false);
    }
  }

  void _togglePlay() {
    if (_notes.isEmpty) return;
    if (_playing) {
      _playCtrl.stop();
      setState(() => _playing = false);
    } else {
      // If a parent wired audio, delegate the actual sound to it. The
      // playhead animation runs regardless (fire-and-forget audio).
      final req = widget.onPlayRequested;
      if (req != null) {
        req().catchError((Object e) {
          debugPrint('playback request failed: $e');
        });
      }
      final ms = (_totalBeats * _secondsPerBeat * 1000).round();
      if (ms > 0 && _playCtrl.duration?.inMilliseconds != ms) {
        _playCtrl.duration = Duration(milliseconds: ms);
      }
      _playCtrl.forward(from: 0);
      setState(() => _playing = true);
    }
  }

  // --- pointer handling (raw Listener so scrolling still works on empty
  // --- areas; note drags disable scroll physics while in progress) ---

  void _onPointerDown(PointerDownEvent e) {
    final pos = e.localPosition;
    // NOTE hit-testing wins over the curve: the curve line runs through
    // the notes (same row), so testing the curve first steals every note
    // tap (opens curve-drag, inserts a point mid-pointer-event → the
    // "_dependents.isEmpty" crash). Curve points are still grabbable via
    // their 12px handle; the line only creates points on empty space.
    final hit = _hitTest(pos);
    final curveIdx =
        (hit == null && _tool == PianoRollTool.select && _showPitch)
        ? _curveHitTest(pos)
        : -1;
    setState(() {
      if (curveIdx >= 0) {
        _selectedNote = null;
        _dragCurveIdx = curveIdx;
        _curveDragStartY = pos.dy;
        _curveDragStartSemis = _curvePoints[curveIdx].semitones;
        _dragNote = null;
        return;
      }
      if (hit != null) {
        _selectedNote = hit;
        if (_tool == PianoRollTool.erase) {
          _notes.remove(hit);
          _curvePoints.removeWhere(
            (p) => (p.beat - (hit.position + hit.duration / 2)).abs() < 0.05,
          );
          _selectedNote = null;
          _revision++;
          _notifyNotesChanged();
        } else if (_tool == PianoRollTool.select) {
          // Right-edge grab (8px) resizes the note instead of moving it.
          final rect = _noteRect(hit);
          final nearRightEdge = (pos.dx - rect.right).abs() <= 8;
          if (nearRightEdge) {
            _resizeNote = hit;
            _resizeStartDur = hit.duration;
            _dragStartPos = pos;
            _dragMoved = false;
            _dragNote = null;
          } else {
            _dragNote = hit;
            _dragStartPos = pos;
            _dragStartBeats = hit.position;
            _dragStartPitch = hit.pitch;
            _dragMoved = false;
            // Long-press → context menu (OM: setTimeout 550ms)
            _longPressTimer?.cancel();
            _longPressTimer = Timer(
              const Duration(milliseconds: 550),
              () => _showNoteContextMenu(hit, pos),
            );
          }
        }
        // draw ignores existing notes; split is a stub that just selects.
      } else if (_tool == PianoRollTool.draw) {
        _createNoteAt(pos);
      } else if (_tool == PianoRollTool.select) {
        _selectedNote = null;
      }
    });
  }

  void _onPointerMove(PointerMoveEvent e) {
    // --- curve point drag (edits the pitch curve, not the notes) ---
    if (_dragCurveIdx >= 0) {
      setState(() {
        if (_dragCurveIdx >= _curvePoints.length) {
          _dragCurveIdx = -1;
          return;
        }
        final deltaSemis =
            -((e.localPosition.dy - _curveDragStartY) / _rowHeight);
        final semis = (_curveDragStartSemis + deltaSemis).clamp(-12.0, 12.0);
        _curvePoints[_dragCurveIdx] = PitchPoint(
          _curvePoints[_dragCurveIdx].beat,
          semis,
        );
        _revision++;
        _notifyCurveChanged();
      });
      return;
    }
    // --- note RESIZE (right edge) ---
    final resize = _resizeNote;
    if (resize != null) {
      final pos = e.localPosition;
      final dx = pos.dx - _dragStartPos.dx;
      if (!_dragMoved) {
        if (dx.abs() < 4) return; // still a potential tap
        _dragMoved = true;
      }
      setState(() {
        final idx = _notes.indexOf(resize);
        if (idx < 0) {
          _resizeNote = null;
          return;
        }
        final stepPx = _snapBeat * _ppb;
        final newDur = math.max(
          0.25, // min 1/4 beat
          _resizeStartDur + (dx / stepPx).round() * _snapBeat,
        );
        final updated = resize.copyWith(duration: newDur);
        _notes[idx] = updated;
        _selectedNote = updated;
        _resizeNote = updated;
        _syncNoteAnchors();
        _revision++;
        _notifyNotesChanged();
      });
      return;
    }
    // --- note drag ---
    final note = _dragNote;
    if (note == null) return;
    final pos = e.localPosition;
    final dx = pos.dx - _dragStartPos.dx;
    final dy = pos.dy - _dragStartPos.dy;
    if (!_dragMoved) {
      if (dx.abs() < 4 && dy.abs() < 4) return; // still a potential tap
      _dragMoved = true;
      _longPressTimer?.cancel(); // drag started → cancel long-press
    }
    setState(() {
      final idx = _notes.indexOf(note);
      if (idx < 0) {
        _dragNote = null;
        return;
      }
      final stepPx = _snapBeat * _ppb;
      final newPos = math.max(
        0.0,
        _dragStartBeats + (dx / stepPx).round() * _snapBeat,
      );
      final newPitch = (_dragStartPitch - (dy / _rowHeight).round()).clamp(
        _bottomPitch,
        _topPitch,
      );
      final updated = note.copyWith(position: newPos, pitch: newPitch);
      _notes[idx] = updated;
      _selectedNote = updated;
      _dragNote = updated;
      _syncNoteAnchors();
      _revision++;
      _notifyNotesChanged();
    });
  }

  void _onPointerUp(PointerEvent e) {
    _longPressTimer?.cancel();
    // A clean tap (no drag, no curve drag) on a selected note edits the
    // lyric. showDialog must NOT run inside setState (pushing a route mid-
    // rebuild → "_dependents.isEmpty is not true"); capture the index here,
    // then open the dialog after the state update settles.
    int? tapEditIdx;
    setState(() {
      if (!_dragMoved &&
          _dragCurveIdx < 0 &&
          _selectedNote != null &&
          widget.onNotesChanged != null) {
        final idx = _notes.indexOf(_selectedNote!);
        if (idx >= 0) {
          tapEditIdx = idx;
        }
      }
      _dragNote = null;
      _resizeNote = null;
      _dragCurveIdx = -1;
      _dragMoved = false;
    });
    if (tapEditIdx != null && mounted) {
      _editLyric(tapEditIdx!);
    }
  }

  /// Show a context menu for the given note (OM long-press popup).
  void _showNoteContextMenu(Note note, Offset pos) {
    final idx = _notes.indexOf(note);
    if (idx < 0 || !mounted) return;
    showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(
        pos.dx + 60, // offset for keys column
        pos.dy + 48, // offset for toolbar
        pos.dx + 200,
        pos.dy + 200,
      ),
      color: const Color(0xFF2E2E2E),
      items: [
        PopupMenuItem(
          value: 'lyric',
          child: Row(
            children: [
              const Icon(Icons.edit_rounded, size: 16, color: Colors.white70),
              const SizedBox(width: 8),
              Text(
                'Edit lyrics…',
                style: TextStyle(fontSize: 13, color: LiltColors.text),
              ),
            ],
          ),
        ),
        PopupMenuItem(
          value: 'up',
          child: Row(
            children: [
              const Icon(
                Icons.arrow_upward_rounded,
                size: 16,
                color: Colors.white70,
              ),
              const SizedBox(width: 8),
              Text(
                'Move up',
                style: TextStyle(fontSize: 13, color: LiltColors.text),
              ),
            ],
          ),
        ),
        PopupMenuItem(
          value: 'down',
          child: Row(
            children: [
              const Icon(
                Icons.arrow_downward_rounded,
                size: 16,
                color: Colors.white70,
              ),
              const SizedBox(width: 8),
              Text(
                'Move down',
                style: TextStyle(fontSize: 13, color: LiltColors.text),
              ),
            ],
          ),
        ),
        const PopupMenuDivider(),
        PopupMenuItem(
          value: 'delete',
          child: Row(
            children: [
              const Icon(
                Icons.delete_rounded,
                size: 16,
                color: Color(0xFFFF6B6B),
              ),
              const SizedBox(width: 8),
              Text(
                'Delete',
                style: TextStyle(fontSize: 13, color: const Color(0xFFFF6B6B)),
              ),
            ],
          ),
        ),
      ],
    ).then((action) {
      if (action == null || !mounted) return;
      setState(() {
        switch (action) {
          case 'lyric':
            _editLyric(idx);
            break;
          case 'up':
            _notes[idx] = note.copyWith(pitch: note.pitch + 1);
            _revision++;
            _notifyNotesChanged();
            break;
          case 'down':
            _notes[idx] = note.copyWith(pitch: note.pitch - 1);
            _revision++;
            _notifyNotesChanged();
            break;
          case 'delete':
            _notes.removeAt(idx);
            _selectedNote = null;
            _revision++;
            _notifyNotesChanged();
            break;
        }
      });
    });
  }

  /// Open a lyric editor dialog for the note at [idx]; on save, updates the
  /// note and notifies the parent so export/play use the new lyric.
  Future<void> _editLyric(int idx) async {
    // Guard against double-open (stylus + finger can both fire pointer-up):
    // two stacked dialogs pop in the wrong order → InheritedElement
    // deactivated with dependents ("_dependents.isEmpty is not true").
    if (_lyricDialogOpen) return;
    _lyricDialogOpen = true;
    try {
      final note = _notes[idx];
      final controller = TextEditingController(text: note.lyric);
      final result = await showDialog<String>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: const Text('Edit lyric'),
          content: TextField(
            controller: controller,
            autofocus: true,
            decoration: const InputDecoration(labelText: 'Lyric'),
            onSubmitted: (v) => Navigator.of(ctx).pop(v),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.of(ctx).pop(controller.text),
              child: const Text('Save'),
            ),
          ],
        ),
      );
      // IMPORTANT: do NOT dispose the controller here — the dialog's exit
      // animation is still running when showDialog's future resolves, and
      // the TextField keeps listening to the controller during it.
      // Disposing early → "TextEditingController used after being disposed"
      // → the dialog's inherited elements get torn down mid-animation →
      // "_dependents.isEmpty is not true". The controller is small; let the
      // framework collect it with the route.
      if (result == null || result.isEmpty || !mounted) return;
      setState(() {
        _notes[idx] = note.copyWith(lyric: result.trim());
        _revision++;
        widget.onNotesChanged?.call(List.of(_notes));
      });
    } finally {
      _lyricDialogOpen = false;
    }
  }

  /// Parent sync: call after every note-list mutation (add/move/delete).
  void _notifyNotesChanged() {
    widget.onNotesChanged?.call(List.of(_notes));
  }

  /// Parent sync: call after every pitch-curve mutation so export/play can
  /// embed the drawn curve in the ustx (otherwise it's silently dropped).
  void _notifyCurveChanged() {
    widget.onCurveChanged?.call(List.of(_curvePoints));
  }

  void _createNoteAt(Offset pos) {
    final stepPx = _snapBeat * _ppb;
    final beat = math.max(0.0, (pos.dx / stepPx).round() * _snapBeat);
    final row = (pos.dy / _rowHeight).floor();
    final pitch = (_topPitch - row).clamp(_bottomPitch, _topPitch);
    _notes.add(Note(lyric: 'la', pitch: pitch, position: beat, duration: 0.5));
    _curvePoints.add(PitchPoint(beat + 0.25, pitch.toDouble()));
    _curvePoints.sort((a, b) => a.beat.compareTo(b.beat));
    _selectedNote = _notes.last;
    _revision++;
    _notifyCurveChanged();
    _notifyNotesChanged();
  }

  // --- build ---

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        _buildToolbar(),
        Expanded(
          child: AnimatedBuilder(
            animation: _playCtrl,
            builder: (context, _) => Column(
              children: [
                Expanded(
                  child: Stack(
                    children: [
                      _buildRollArea(),
                      // Floating circular toolbar (OM style)
                      Positioned(
                        right: 12,
                        top: 12,
                        child: Column(
                          children: [
                            _floatingBtn(
                              icon: _tool == PianoRollTool.select
                                  ? Icons.edit_note_rounded
                                  : Icons.edit_off_rounded,
                              active: _tool == PianoRollTool.select,
                              onTap: () => setState(
                                () => _tool = _tool == PianoRollTool.select
                                    ? PianoRollTool.draw
                                    : PianoRollTool.select,
                              ),
                            ),
                            const SizedBox(height: 6),
                            _floatingBtn(
                              icon: Icons.grid_on_rounded,
                              active: _snapBeat <= 0.125,
                              onTap: () => setState(
                                () => _snapBeat = _snapBeat <= 0.125
                                    ? 0.25
                                    : 0.125,
                              ),
                            ),
                            const SizedBox(height: 6),
                            _floatingBtn(
                              icon: Icons.play_arrow_rounded,
                              active: _playing,
                              onTap: _togglePlay,
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ),
                _buildTransport(),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildToolbar() {
    return Container(
      height: 48,
      decoration: const BoxDecoration(
        color: _toolbarBg,
        border: Border(bottom: BorderSide(color: Color(0xFF303441))),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 14),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            _toolButton(
              Icons.north_west_rounded,
              'Select',
              _tool == PianoRollTool.select,
              () => setState(() => _tool = PianoRollTool.select),
            ),
            const SizedBox(width: 2),
            _toolButton(
              Icons.edit_rounded,
              'Draw',
              _tool == PianoRollTool.draw,
              () => setState(() => _tool = PianoRollTool.draw),
            ),
            const SizedBox(width: 2),
            _toolButton(
              Icons.content_cut_rounded,
              'Split',
              _tool == PianoRollTool.split,
              () => setState(() => _tool = PianoRollTool.split),
            ),
            const SizedBox(width: 2),
            _toolButton(
              Icons.backspace_outlined,
              'Erase',
              _tool == PianoRollTool.erase,
              () => setState(() => _tool = PianoRollTool.erase),
            ),
            const SizedBox(width: 10),
            _toolButton(
              Icons.waves_rounded,
              'Pitch',
              _showPitch,
              () => setState(() => _showPitch = !_showPitch),
            ),
            const SizedBox(width: 16),
            const Text(
              'Snap',
              style: TextStyle(fontSize: 11, color: Color(0xFF8E94A7)),
            ),
            const SizedBox(width: 6),
            PopupMenuButton<double>(
              tooltip: 'Snap grid',
              initialValue: _snapBeat,
              onSelected: (v) => setState(() => _snapBeat = v),
              position: PopupMenuPosition.under,
              color: const Color(0xFF20232D),
              itemBuilder: (context) => [
                for (final d in const [1.0, 0.5, 0.25, 0.125, 0.0625])
                  PopupMenuItem(
                    value: d,
                    child: Text(
                      _fmtSnap(d),
                      style: TextStyle(
                        fontSize: 12,
                        color: d == _snapBeat
                            ? LiltColors.purple
                            : LiltColors.text,
                      ),
                    ),
                  ),
              ],
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                decoration: BoxDecoration(
                  color: const Color(0xFF2A263B),
                  borderRadius: BorderRadius.circular(5),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      _fmtSnap(_snapBeat),
                      style: const TextStyle(
                        fontSize: 10,
                        color: Color(0xFFCDD1DF),
                      ),
                    ),
                    const Icon(
                      Icons.arrow_drop_down_rounded,
                      size: 13,
                      color: Color(0xFFCDD1DF),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(width: 14),
            const Text(
              'Zoom',
              style: TextStyle(fontSize: 11, color: Color(0xFF8E94A7)),
            ),
            SizedBox(
              width: 110,
              child: SliderTheme(
                data: SliderThemeData(
                  trackHeight: 2,
                  activeTrackColor: _purple2,
                  inactiveTrackColor: const Color(0xFF303441),
                  thumbColor: LiltColors.purple,
                  overlayColor: const Color(0x337358E9),
                  thumbShape: const RoundSliderThumbShape(
                    enabledThumbRadius: 6,
                  ),
                  overlayShape: const RoundSliderOverlayShape(
                    overlayRadius: 10,
                  ),
                ),
                child: Slider(
                  value: _zoomPercent.toDouble(),
                  min: 60,
                  max: 130,
                  onChanged: (v) => setState(() => _zoomPercent = v.round()),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _toolButton(
    IconData icon,
    String label,
    bool active,
    VoidCallback onTap,
  ) {
    final fg = active ? Colors.white : const Color(0xFF949AAE);
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(7),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
        decoration: BoxDecoration(
          color: active ? const Color(0xFF2A263B) : null,
          borderRadius: BorderRadius.circular(7),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 15, color: fg),
            const SizedBox(width: 5),
            Text(label, style: TextStyle(fontSize: 12, color: fg)),
          ],
        ),
      ),
    );
  }

  /// Floating circular button (OM CircularToolbarButtonStyle).
  Widget _floatingBtn({
    required IconData icon,
    required bool active,
    required VoidCallback onTap,
  }) {
    return Material(
      color: active
          ? const Color(0xFFFE71A3)
          : Colors.black.withValues(alpha: 0.55),
      shape: const CircleBorder(),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        customBorder: const CircleBorder(),
        child: SizedBox(
          width: 34,
          height: 34,
          child: Icon(icon, size: 18, color: Colors.white),
        ),
      ),
    );
  }

  Widget _buildRollArea() {
    return LayoutBuilder(
      builder: (context, c) {
        final rollW = math.max(0.0, c.maxWidth - _keysWidth);
        final contentW = math.max(rollW, (_totalBeats + 3.0) * _ppb);
        final contentH = _rows * _rowHeight;
        final playheadBeat = _playCtrl.value * _totalBeats;
        // Never scroll while dragging a note, a pitch-curve point, or
        // resizing — the Listener doesn't consume the pointer, so without
        // this the scroll view steals the drag (curve edits scroll the roll,
        // right-edge grabs scroll horizontally instead of resizing).
        final physics =
            (_dragNote != null || _dragCurveIdx >= 0 || _resizeNote != null)
            ? const NeverScrollableScrollPhysics()
            : null;
        return ColoredBox(
          color: _bg,
          child: SingleChildScrollView(
            controller: _vScroll,
            physics: physics,
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: _keysWidth,
                  height: contentH,
                  child: CustomPaint(
                    painter: _KeysPainter(
                      rowHeight: _rowHeight,
                      topPitch: _topPitch,
                      rows: _rows,
                    ),
                  ),
                ),
                Expanded(
                  child: SingleChildScrollView(
                    controller: _hScroll,
                    scrollDirection: Axis.horizontal,
                    physics: physics,
                    child: SizedBox(
                      width: contentW,
                      height: contentH,
                      child: Listener(
                        behavior: HitTestBehavior.opaque,
                        onPointerDown: _onPointerDown,
                        onPointerMove: _onPointerMove,
                        onPointerUp: _onPointerUp,
                        onPointerCancel: _onPointerUp,
                        child: Stack(
                          children: [
                            // Layer 1 (bottom): grid + notes + playhead.
                            CustomPaint(
                              size: Size(contentW, contentH),
                              painter: _RollPainter(
                                notes: _notes,
                                ppb: _ppb,
                                topPitch: _topPitch,
                                rowHeight: _rowHeight,
                                trackColor: _trackColor,
                                playheadBeat: playheadBeat,
                                contentWidth: contentW,
                                contentHeight: contentH,
                                revision: _revision,
                              ),
                            ),
                            // Layer 2: the continuous orange pitch curve —
                            // a separate overlay object, like the mock's
                            // .curve-layer (z-index above notes/playhead).
                            CustomPaint(
                              size: Size(contentW, contentH),
                              painter: _CurvePainter(
                                curvePoints: _curvePoints,
                                ppb: _ppb,
                                topPitch: _topPitch,
                                rowHeight: _rowHeight,
                                showPitch: _showPitch,
                                hasSelection:
                                    _selectedNote != null || _dragCurveIdx >= 0,
                                contentWidth: contentW,
                                contentHeight: contentH,
                              ),
                            ),
                            // Layer 3 (top): selected note outline (white),
                            // above the curve like the mock's z-order.
                            CustomPaint(
                              size: Size(contentW, contentH),
                              painter: _SelectionPainter(
                                note: _selectedNote,
                                ppb: _ppb,
                                topPitch: _topPitch,
                                rowHeight: _rowHeight,
                                trackColor: _trackColor,
                              ),
                            ),
                            // OpenUtau Mobile's black timeline is an overlay
                            // over the first 22px of the roll.
                            Positioned(
                              left: 0,
                              top: 0,
                              right: 0,
                              height: _timelineHeight,
                              child: IgnorePointer(
                                child: CustomPaint(
                                  painter: _RollTimelinePainter(ppb: _ppb),
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        );
      },
    );
  }

  Widget _buildTransport() {
    return Container(
      height: 44,
      decoration: const BoxDecoration(
        color: _toolbarBg,
        border: Border(top: BorderSide(color: Color(0xFF303441))),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 14),
      child: LayoutBuilder(
        builder: (context, c) {
          return Row(
            children: [
              _playButton(),
              const SizedBox(width: 12),
              AnimatedBuilder(
                animation: _playCtrl,
                builder: (context, _) {
                  final t = _playCtrl.value * _totalBeats * _secondsPerBeat;
                  return Text(
                    '${_fmtTime(t)} / ${_fmtTime(_totalBeats * _secondsPerBeat)}',
                    style: const TextStyle(
                      fontSize: 12,
                      color: Color(0xFFD9DBEA),
                      fontFeatures: [FontFeature.tabularFigures()],
                    ),
                  );
                },
              ),
              const SizedBox(width: 14),
              Expanded(
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 160),
                    child: SizedBox(
                      width: double.infinity,
                      height: 8,
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          color: const Color(0xFF282B35),
                          borderRadius: BorderRadius.circular(4),
                        ),
                        child: AnimatedBuilder(
                          animation: _playCtrl,
                          builder: (context, _) => FractionallySizedBox(
                            alignment: Alignment.centerLeft,
                            widthFactor: _playCtrl.value,
                            child: const DecoratedBox(
                              decoration: BoxDecoration(
                                borderRadius: BorderRadius.all(
                                  Radius.circular(4),
                                ),
                                gradient: LinearGradient(
                                  colors: [
                                    Color(0xFF7BE3A8),
                                    Color(0xFFFFC16D),
                                  ],
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
              if (c.maxWidth > 520)
                const Text(
                  '● Autosaved',
                  style: TextStyle(fontSize: 11, color: Color(0xFF8E94A7)),
                ),
            ],
          );
        },
      ),
    );
  }

  Widget _playButton() {
    return InkWell(
      onTap: _togglePlay,
      customBorder: const CircleBorder(),
      child: Container(
        width: 34,
        height: 34,
        decoration: const BoxDecoration(
          shape: BoxShape.circle,
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [_purple2, Color(0xFF9B70F8)],
          ),
        ),
        child: Icon(
          _playing ? Icons.pause_rounded : Icons.play_arrow_rounded,
          size: 18,
          color: Colors.white,
        ),
      ),
    );
  }
}

/// Left piano-key label column (58px, pinned while the roll scrolls).
class _KeysPainter extends CustomPainter {
  const _KeysPainter({
    required this.rowHeight,
    required this.topPitch,
    required this.rows,
  });

  final double rowHeight;
  final int topPitch;
  final int rows;

  @override
  void paint(Canvas canvas, Size size) {
    for (var i = 0; i < rows; i++) {
      final pitch = topPitch - i;
      final y = i * rowHeight;
      final isBlack = _blackKeyClasses.contains(((pitch % 12) + 12) % 12);
      final isC = pitch % 12 == 0;
      canvas.drawRect(
        Rect.fromLTWH(0, y, size.width, rowHeight),
        Paint()..color = isBlack ? _keysBlack : _keysBg,
      );
      canvas.drawLine(
        Offset(0, y + rowHeight),
        Offset(size.width, y + rowHeight),
        Paint()..color = _rowLine,
      );
      final tp = TextPainter(
        text: TextSpan(
          text: _pitchName(pitch),
          style: TextStyle(
            fontSize: 10,
            fontWeight: isC ? FontWeight.w700 : FontWeight.w400,
            color: isC ? LiltColors.text : const Color(0xFF777D91),
          ),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      tp.paint(
        canvas,
        Offset(size.width - 7 - tp.width, y + (rowHeight - tp.height) / 2),
      );
    }
    canvas.drawLine(
      Offset(size.width - 0.5, 0),
      Offset(size.width - 0.5, size.height),
      Paint()..color = LiltColors.line,
    );
  }

  @override
  bool shouldRepaint(_KeysPainter oldDelegate) =>
      oldDelegate.rowHeight != rowHeight ||
      oldDelegate.topPitch != topPitch ||
      oldDelegate.rows != rows;
}

/// Layer 1 of the roll: beat grid, pitch rows, gradient notes with lyrics,
/// and the pink playhead. The pitch curve lives in its own painter
/// ([_CurvePainter]) as a separate overlay layer, and the selected-note
/// outline in [_SelectionPainter] on top of that — mirroring the mock's
/// z-order (selected note > curve layer > playhead).
class _RollPainter extends CustomPainter {
  _RollPainter({
    required this.notes,
    required this.ppb,
    required this.topPitch,
    required this.rowHeight,
    required this.trackColor,
    required this.playheadBeat,
    required this.contentWidth,
    required this.contentHeight,
    required this.revision,
  });

  final List<Note> notes;
  final double ppb;
  final int topPitch;
  final double rowHeight;
  final Color trackColor;
  final double playheadBeat;
  final double contentWidth;
  final double contentHeight;
  final int revision;

  double _noteTopY(int pitch) =>
      (topPitch - pitch) * rowHeight + (rowHeight - _noteHeight) / 2;

  @override
  void paint(Canvas canvas, Size size) {
    // background + horizontal pitch rows
    canvas.drawRect(Offset.zero & size, Paint()..color = _bg);
    for (var i = 0; i < _rows; i++) {
      canvas.drawRect(
        Rect.fromLTWH(0, i * rowHeight, size.width, rowHeight),
        Paint()..color = _rowBg,
      );
      final pitch = topPitch - i;
      if (_blackKeyClasses.contains(((pitch % 12) + 12) % 12)) {
        canvas.drawRect(
          Rect.fromLTWH(0, i * rowHeight, size.width, rowHeight),
          Paint()..color = _bg,
        );
      }
    }
    for (var i = 0; i <= _rows; i++) {
      canvas.drawLine(
        Offset(0, i * rowHeight),
        Offset(size.width, i * rowHeight),
        Paint()..color = _rowLine,
      );
    }
    // vertical beat grid (bar line every 4 beats)
    final maxBeat = (size.width / ppb).ceil();
    for (var b = 1; b <= maxBeat; b++) {
      final x = b * ppb;
      canvas.drawLine(
        Offset(x, 0),
        Offset(x, size.height),
        Paint()..color = b % 4 == 0 ? _barLine : _beatLine,
      );
    }
    // notes
    for (final n in notes) {
      _paintNote(canvas, n);
    }
    // playhead
    _paintPlayhead(canvas, size);
  }

  void _paintNote(Canvas canvas, Note n) {
    final rect = Rect.fromLTWH(
      n.position * ppb,
      _noteTopY(n.pitch),
      n.duration * ppb,
      _noteHeight,
    );
    final rrect = RRect.fromRectAndRadius(rect, const Radius.circular(3));
    // soft drop shadow
    canvas.drawRRect(
      rrect.shift(const Offset(0, 3)),
      Paint()
        ..color = const Color(0x33000000)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 9),
    );
    // horizontal gradient (per-track color, darkened -> lightened)
    final dark = Color.lerp(trackColor, Colors.black, 0.25)!;
    final light = Color.lerp(trackColor, Colors.white, 0.38)!;
    canvas.drawRRect(
      rrect,
      Paint()
        ..shader = LinearGradient(colors: [dark, light]).createShader(rect),
    );
    canvas.drawRRect(
      rrect,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = light.withValues(alpha: 0.5),
    );
    // lyric text (left) + duration (right)
    final labelStyle = TextStyle(
      fontSize: 11,
      fontWeight: FontWeight.w700,
      color: Colors.white,
      height: 1.0,
    );
    final lyricTp = TextPainter(
      text: TextSpan(text: n.lyric, style: labelStyle),
      textDirection: TextDirection.ltr,
      maxLines: 1,
      ellipsis: '…',
    )..layout(maxWidth: math.max(0.0, rect.width - 18));
    lyricTp.paint(
      canvas,
      Offset(rect.left + 8, rect.top + (rect.height - lyricTp.height) / 2),
    );
    if (rect.width >= 70) {
      final durTp = TextPainter(
        text: TextSpan(
          text: _fmtDur(n.duration),
          style: labelStyle.copyWith(
            fontWeight: FontWeight.w500,
            color: Colors.white.withValues(alpha: 0.8),
          ),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      durTp.paint(
        canvas,
        Offset(
          rect.right - 8 - durTp.width,
          rect.top + (rect.height - durTp.height) / 2,
        ),
      );
    }
  }

  void _paintPlayhead(Canvas canvas, Size size) {
    final x = playheadBeat * ppb;
    canvas.drawLine(
      Offset(x, 0),
      Offset(x, size.height),
      Paint()
        ..color = _pink.withValues(alpha: 0.30)
        ..strokeWidth = 5,
    );
    canvas.drawLine(
      Offset(x, 0),
      Offset(x, size.height),
      Paint()
        ..color = _pink
        ..strokeWidth = 2,
    );
    final tri = Path()
      ..moveTo(x - 5, 0)
      ..lineTo(x + 5, 0)
      ..lineTo(x, 7)
      ..close();
    canvas.drawPath(tri, Paint()..color = _pink);
  }

  @override
  bool shouldRepaint(_RollPainter oldDelegate) =>
      oldDelegate.revision != revision ||
      oldDelegate.notes != notes ||
      oldDelegate.ppb != ppb ||
      oldDelegate.trackColor != trackColor ||
      oldDelegate.playheadBeat != playheadBeat ||
      oldDelegate.contentWidth != contentWidth ||
      oldDelegate.contentHeight != contentHeight ||
      oldDelegate.topPitch != topPitch ||
      oldDelegate.rowHeight != rowHeight;
}

/// Layer 2 of the roll: the continuous orange pitch curve, drawn as ONE
/// smooth path through its own control points. A separate painter/overlay
/// object (like the mock's .curve-layer), independent from note painting —
/// the curve is an editable object with its own PitchPoint data.
class _CurvePainter extends CustomPainter {
  _CurvePainter({
    required this.curvePoints,
    required this.ppb,
    required this.topPitch,
    required this.rowHeight,
    required this.showPitch,
    required this.hasSelection,
    required this.contentWidth,
    required this.contentHeight,
  });

  final List<PitchPoint> curvePoints;
  final double ppb;
  final int topPitch;
  final double rowHeight;
  final bool showPitch;
  final bool hasSelection;
  final double contentWidth;
  final double contentHeight;

  double _noteCenterY(double pitch) =>
      (topPitch - pitch) * rowHeight + rowHeight / 2;

  @override
  void paint(Canvas canvas, Size size) {
    if (!showPitch || curvePoints.length < 2) {
      // still draw single points as handles (a 1-point curve is editable)
      if (!showPitch || curvePoints.isEmpty) return;
    }
    final pts = <Offset>[
      for (final p in curvePoints)
        Offset(p.beat * ppb, _noteCenterY(p.semitones)),
    ];
    if (pts.length >= 2) {
      final path = Path()..moveTo(pts.first.dx, pts.first.dy);
      for (var i = 1; i < pts.length; i++) {
        final p0 = pts[i - 1];
        final p1 = pts[i];
        final mx = (p0.dx + p1.dx) / 2;
        path.cubicTo(mx, p0.dy, mx, p1.dy, p1.dx, p1.dy);
      }
      final alpha = hasSelection ? 1.0 : 0.6; // mock .95 / .55 layer
      final fill = Path.from(path)
        ..lineTo(size.width, size.height)
        ..lineTo(0, size.height)
        ..close();
      canvas.drawPath(
        fill,
        Paint()..color = _orange.withValues(alpha: 0.10 * alpha),
      );
      canvas.drawPath(
        path,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 7
          ..strokeCap = StrokeCap.round
          ..strokeJoin = StrokeJoin.round
          ..color = _orange.withValues(alpha: 0.22 * alpha),
      );
      canvas.drawPath(
        path,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 2.2
          ..strokeCap = StrokeCap.round
          ..strokeJoin = StrokeJoin.round
          ..color = _orange.withValues(alpha: alpha),
      );
    }
    // draggable handles on every point — grab the line to add one
    final handle = Paint()..color = _orange;
    final handleEdge = Paint()
      ..color = const Color(0xFF111319)
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5;
    for (final p in pts) {
      canvas.drawCircle(p, 5, handleEdge);
      canvas.drawCircle(p, 3.2, handle);
    }
  }

  @override
  bool shouldRepaint(_CurvePainter oldDelegate) =>
      !listEquals(oldDelegate.curvePoints, curvePoints) ||
      oldDelegate.ppb != ppb ||
      oldDelegate.topPitch != topPitch ||
      oldDelegate.rowHeight != rowHeight ||
      oldDelegate.showPitch != showPitch ||
      oldDelegate.hasSelection != hasSelection ||
      oldDelegate.contentWidth != contentWidth ||
      oldDelegate.contentHeight != contentHeight;
}

/// OpenUtau Mobile's black timeline strip, with bar numbers over the roll.
class _RollTimelinePainter extends CustomPainter {
  const _RollTimelinePainter({required this.ppb});

  final double ppb;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(
      Offset.zero & size,
      Paint()..color = const Color(0xFF101010),
    );
    final maxBeat = (size.width / ppb).ceil();
    for (var beat = 0; beat <= maxBeat; beat += 4) {
      final x = beat * ppb;
      canvas.drawLine(
        Offset(x, 0),
        Offset(x, size.height),
        Paint()..color = const Color(0xFF555555),
      );
      final tp = TextPainter(
        text: TextSpan(
          text: '${beat ~/ 4 + 1}',
          style: const TextStyle(
            color: Color(0xFFCCCCCC),
            fontSize: 10,
            fontWeight: FontWeight.w600,
          ),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      tp.paint(canvas, Offset(x + 4, 3));
    }
    canvas.drawLine(
      Offset(0, size.height - .5),
      Offset(size.width, size.height - .5),
      Paint()..color = const Color(0xFF3A3A3A),
    );
  }

  @override
  bool shouldRepaint(_RollTimelinePainter oldDelegate) =>
      oldDelegate.ppb != ppb;
}

/// Layer 3 (top) of the roll: the selected note's cyan outline, painted
/// above the curve layer (mock z-order: selected note > curve > playhead).
class _SelectionPainter extends CustomPainter {
  _SelectionPainter({
    required this.note,
    required this.ppb,
    required this.topPitch,
    required this.rowHeight,
    required this.trackColor,
  });

  final Note? note;
  final double ppb;
  final int topPitch;
  final double rowHeight;
  final Color trackColor;

  double _noteTopY(int pitch) =>
      (topPitch - pitch) * rowHeight + (rowHeight - _noteHeight) / 2;

  @override
  void paint(Canvas canvas, Size size) {
    final n = note;
    if (n == null) return;
    final rect = Rect.fromLTWH(
      n.position * ppb,
      _noteTopY(n.pitch),
      n.duration * ppb,
      _noteHeight,
    );
    final rrect = RRect.fromRectAndRadius(rect, const Radius.circular(3));
    // redraw the note body so the outline sits on top of the curve layer
    canvas.drawRRect(
      rrect.shift(const Offset(0, 3)),
      Paint()
        ..color = const Color(0x33000000)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 9),
    );
    final dark = Color.lerp(trackColor, Colors.black, 0.25)!;
    final light = Color.lerp(trackColor, Colors.white, 0.38)!;
    canvas.drawRRect(
      rrect,
      Paint()
        ..shader = LinearGradient(colors: [dark, light]).createShader(rect),
    );
    canvas.drawRRect(
      rrect,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = light.withValues(alpha: 0.5),
    );
    // OpenUtau Mobile uses a white selected-note border.
    canvas.drawRRect(
      rrect.deflate(1.5),
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..color = Colors.white,
    );
  }

  @override
  bool shouldRepaint(_SelectionPainter oldDelegate) =>
      oldDelegate.note != note ||
      oldDelegate.ppb != ppb ||
      oldDelegate.topPitch != topPitch ||
      oldDelegate.rowHeight != rowHeight ||
      oldDelegate.trackColor != trackColor;
}
