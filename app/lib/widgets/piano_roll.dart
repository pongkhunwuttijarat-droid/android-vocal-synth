import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../models.dart';
import '../theme.dart';

/// Editor tool selected in the piano-roll toolbar.
enum PianoRollTool { view, select, draw, pencil, split, erase }

/// What the select tool acts on (OM-style sub-modes).
enum SelectTarget { all, notes, curve }

/// Piano-roll editor widget: toolbar, note grid with continuous pitch curve,
/// and transport bar. Mirrors ui-mock/index.html. POC-quality — all state
/// lives here, no provider/bloc dependencies.
class PianoRoll extends StatefulWidget {
  const PianoRoll({
    super.key,
    required this.tracks,
    this.selectedTrackIndex = 0,
    this.onPlayRequested,
    this.onPlayStopped,
    this.onInitialSync,
    this.onNotesChanged,
    this.onCurveChanged,
    this.showPhonemes = true,
    this.showFxOverlay = false,
  });

  /// All project tracks; the roll renders [selectedTrackIndex]'s notes.
  final List<Track> tracks;

  /// Index of the track whose notes are shown/edited.
  final int selectedTrackIndex;

  /// Called when the user hits play. The parent (EditorScreen) renders the
  /// project through the engine and plays the audio; the roll itself only
  /// animates the playhead. Null = playhead animation only (no audio).
  final Future<void> Function()? onPlayRequested;

  /// Called when the user stops playback (toggles play off or the
  /// playhead finishes). The parent stops the actual audio — the roll
  /// only stops its own playhead animation, which otherwise left the
  /// sound playing ("graphic หยุดแต่เสียงไม่").
  final VoidCallback? onPlayStopped;

  /// First-load callback (notes + curve). Unlike [onNotesChanged]/
  /// [onCurveChanged] it does NOT represent a user edit — the parent
  /// stores the data without scheduling a re-render.
  final void Function(List<Note> notes, List<PitchPoint> curve)? onInitialSync;

  /// Called whenever the edited note list changes (add/move/edit lyric), so
  /// the parent can keep its project model in sync for export/play.
  final void Function(List<Note> notes)? onNotesChanged;

  /// Called whenever the pitch curve changes — export/play embed these
  /// points in the ustx so the engine bends the f0 (without this, drawn
  /// curves are silently dropped).
  final void Function(List<PitchPoint> points)? onCurveChanged;

  /// SynthV-style: show phoneme labels above the notes (default true).
  /// Purely visual — the roll's editing logic is unchanged.
  final bool showPhonemes;

  /// Show the FX affected-area overlay (chunk marks, gain-reduction
  /// curve, hot-note rings) — mirrors ui-mock's ⚡ FX layer. Purely
  /// visual; toggled from the toolbar.
  final bool showFxOverlay;

  @override
  State<PianoRoll> createState() => _PianoRollState();
}

// --- Geometry & palette constants (kept in sync with ui-mock/index.html) ---
const double _keysWidth = 60.0; // OpenUtau Mobile / index-om.html
const double _timelineHeight = 22.0; // OpenUtau Mobile timeline overlay
// Keep the app's full A0..C8 range, while using the OpenUtau Mobile visual
// treatment from the mock for the rendered viewport.
const int _rows = 88;
const int _topPitch = 48; // C8, relative to C4
const int _bottomPitch = _topPitch - _rows + 1; // A0
const double _baseNoteHeight = 24.0; // at 32px row → 75% of the row

/// Note height scales with the Y zoom (75% of the row height), so notes
/// stay proportional to the piano-roll scale when the user zooms the Y
/// axis — the old const 24px broke the proportion at other zooms.
double _noteHeightFor(double rowHeight) =>
    rowHeight * (_baseNoteHeight / 32.0);
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

  /// Read-only FX-overlay visibility for widget tests.
  @visibleForTesting
  bool get debugShowFxOverlay => _showFxOverlay;

  /// Read-only view of the editable curve points for widget tests.
  @visibleForTesting
  List<PitchPoint> get debugCurvePoints => List.unmodifiable(_curvePoints);

  // Default to View (pan) — safe browse mode; editing is an explicit tool.
  PianoRollTool _tool = PianoRollTool.view;
  /// Select sub-mode: all / notes only / curve only.
  SelectTarget _selectTarget = SelectTarget.all;
  bool _showPitch = true;
  /// Two-axis zoom: X = pixels per beat, Y = row height.
  /// Defaults: X 90% (ppb 72), Y 100% (row 32px) — same geometry as the
  /// original single-axis zoom, so tests/UX stay stable.
  int _zoomX = 90;
  int _zoomY = 100;
  double _zoomStart = 90;
  double _zoomYStart = 90;

  /// SynthV-style phoneme labels above notes (mirrors ui-mock).
  bool _showPhonemes = true;

  /// FX affected-area overlay visibility (mirrors ui-mock's ⚡ FX toggle).
  bool _showFxOverlay = false;

  /// Roll canvas (pinch target) — used to convert the gesture focal point
  /// to viewport coords for zoom-relative-to-canvas.
  final GlobalKey _rollKey = GlobalKey();

  /// Apply a two-axis zoom keeping the content point under [viewportPoint]
  /// stationary (zoom relative to canvas — the finger or viewport center
  /// doesn't drift). Callers pass the point in the roll's viewport coords.
  void _applyZoom(int nx, int ny, Offset viewportPoint) {
    if (_zoomX == nx && _zoomY == ny) return;
    // Content coords of the anchor BEFORE the zoom.
    final kx = nx / _zoomX;
    final ky = ny / _zoomY;
    final cx = viewportPoint.dx +
        (_hScroll.hasClients ? _hScroll.offset : 0);
    final cy = viewportPoint.dy +
        (_vScroll.hasClients ? _vScroll.offset : 0);
    setState(() {
      _zoomX = nx;
      _zoomY = ny;
    });
    // After layout, keep the anchor's viewport offset constant.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_hScroll.hasClients) {
        _hScroll.jumpTo((cx * kx - viewportPoint.dx)
            .clamp(0.0, _hScroll.position.maxScrollExtent));
      }
      if (_vScroll.hasClients) {
        _vScroll.jumpTo((cy * ky - viewportPoint.dy)
            .clamp(0.0, _vScroll.position.maxScrollExtent));
      }
    });
  }

  /// Viewport center in the roll's own coords (used by slider/button zoom).
  Offset get _viewportCenter {
    final box =
        _rollKey.currentContext?.findRenderObject() as RenderBox?;
    if (box == null) return Offset.zero;
    return box.size.center(Offset.zero);
  }
  int _revision = 0; // bumped on every edit so the painter repaints

  /// Snap grid in beats (1/1, 1/2, 1/4, 1/8, 1/16). Used when dragging notes
  /// and when drawing new ones.
  double _snapBeat = 1 / 16;

  /// Editable pitch-curve points — the curve is its own object, independent
  /// from notes. Each point is (beat, semitones from the note reference).
  late List<PitchPoint> _curvePoints;


  /// View (pan) tool: start position + scroll offsets at pointer-down.
  bool _panning = false;
  Offset _panStart = Offset.zero;
  double _panH0 = 0;
  double _panV0 = 0;

  /// Line-edit (pencil) tool: last pointer X (px) — the curve follows the
  /// finger at `_capturePx` resolution instead of a coarse beat threshold.
  double _pencilLastDx = double.negativeInfinity;

  /// Pointer-event thinning (px): skip move events closer than this to the
  /// last captured one. The GRID (not this) decides where points land —
  /// this only stops redundant pointer events from re-triggering the
  /// same snapped beat.
  static const double _capturePx = 3;

  /// Curve capture GRID: beats between editable curve points. Points are
  /// SNAPPED to this grid (`round(beat/step)*step`), so redrawing over the
  /// same span upserts the SAME points instead of piling up free-form
  /// points — free-form positions were the cause of the jumpy/duplicate
  /// values after repeated curve edits ("แก้ curve ซ้ำบ่อยๆ ค่าโดด").
  /// Default 1/8 beat = 16th-note grid (OM Mobile "Normal").
  double _captureStep = 0.125;

  late final AnimationController _playCtrl;
  bool _playing = false;
  final ScrollController _vScroll = ScrollController(initialScrollOffset: 90);
  final ScrollController _hScroll = ScrollController();

  double get _ppb => _basePxPerBeat * _zoomX / 100;
  /// Row height (px per semitone) — zoomable on the Y axis.
  double get _rowHeight => 32.0 * _zoomY / 100;
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
      (_topPitch - pitch) * _rowHeight + (_rowHeight - _noteHeightFor(_rowHeight)) / 2;

  Rect _noteRect(Note n) => Rect.fromLTWH(
    n.position * _ppb,
    _noteTopY(n.pitch),
    n.duration * _ppb,
    _noteHeightFor(_rowHeight),
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

  /// Hit-test note at [pos] (topmost first).
  Note? _hitTest(Offset pos) {
    for (final n in _notes.reversed) {
      if (_noteRect(n).contains(pos)) return n;
    }
    return null;
  }

  /// Freehand pitch writing (OM-style pencil): upsert a curve point at
  /// the pointer position, SNAPPED to the capture grid (`_captureStep`).
  ///
  /// Curve points store ABSOLUTE pitch (like `_syncCurveFromNotes`), so
  /// the Y axis maps the tapped row to the full roll range — no ±12 cap
  /// (the old clamp made the line stop at C5 and "not follow the finger"
  /// above it). The top of the roll is the highest key.
  void _pencilWrite(Offset pos) {
    // Snap the beat to the capture grid: repeated edits over the same
    // span land on the SAME beat values, so the upsert below replaces
    // the point instead of stacking near-duplicate free-form points
    // (which made the curve jump after repeated edits).
    final rawBeat = (pos.dx / _ppb).clamp(0.0, _totalBeats + 4.0);
    final beat = (_captureStep <= 0)
        ? rawBeat
        : (rawBeat / _captureStep).round() * _captureStep;
    // FRACTIONAL pitch — never floor to whole semitones. The old
    // `row.floor()` made every drawn line a staircase (one pitch step per
    // row); the curve must follow the finger continuously, like OpenUtau.
    final pitchF =
        _topPitch - (pos.dy / _rowHeight).clamp(0.0, _rows.toDouble() - 1);
    var pitch = pitchF.clamp(_bottomPitch, _topPitch).toDouble();
    // Upsert the EXACT grid point (snapped beats are identical across
    // redraws → the same point gets overwritten, not duplicated).
    final i = _curvePoints.indexWhere((p) => (p.beat - beat).abs() < 1e-9);
    if (i >= 0) {
      _curvePoints[i] = PitchPoint(beat, pitch);
    } else {
      // Smoothness: blend the new point 50% toward its left neighbour so
      // the dense capture doesn't draw a jittery zigzag — the line
      // follows the finger but stays smooth, like OpenUtau.
      var prevIdx = -1;
      for (var k = _curvePoints.length - 1; k >= 0; k--) {
        if (_curvePoints[k].beat < beat) {
          prevIdx = k;
          break;
        }
      }
      if (prevIdx >= 0) {
        final prev = _curvePoints[prevIdx];
        // Light blend (30%): follows the finger closely while damping
        // jitter; the Catmull-Rom render passes through these points.
        pitch = prev.semitones * 0.3 + pitch * 0.7;
        // Outlier guard: a fast flick produces sparse pointer events — the
        // raw new pitch can land far from the line, making ONE point stick
        // out ("มีจุดนึงตกพิเศษ"). Clamp the per-point step to 4 semitones
        // so a single bad sample can't spike the curve.
        pitch = (pitch - prev.semitones).clamp(-4.0, 4.0) + prev.semitones;
      }
      _curvePoints.add(PitchPoint(beat, pitch));
      _curvePoints.sort((a, b) => a.beat.compareTo(b.beat));
    }
    _pencilLastDx = pos.dx;
    _revision++;
    _notifyCurveChanged();
  }

  @override
  void initState() {
    super.initState();
    _playCtrl = AnimationController(vsync: this)
      ..addStatusListener(_onPlayStatus);
    _notes = List.of(_activeTrack?.notes ?? const []);
    _syncCurveFromNotes();
    _showPhonemes = widget.showPhonemes;
    _showFxOverlay = widget.showFxOverlay;
    if (_notes.length > 3) _selectedNote = _notes[3]; // mock default: "the"
    // Sync the initial note set to the parent (export/play use it) via the
    // dedicated onInitialSync callback — NOT onNotesChanged, so the parent
    // does not treat a fresh open as an edit (no spurious re-render).
    WidgetsBinding.instance.addPostFrameCallback((_) {
      widget.onInitialSync?.call(List.of(_notes), List.of(_curvePoints));
      // Scroll the roll to the notes' pitch band (initialScrollOffset 90
      // only shows the top rows, which are empty — C8 down to ~C7). Jump
      // so the LOWEST note is ~1/3 up the viewport.
      if (_vScroll.hasClients && _notes.isNotEmpty) {
        final lowest = _notes
            .map((n) => n.pitch)
            .reduce((a, b) => a < b ? a : b);
        final target =
            ((_topPitch - lowest) * _rowHeight - _rowHeight * 10)
                .clamp(0.0, _vScroll.position.maxScrollExtent);
        _vScroll.jumpTo(target);
      }
    });
  }

  @override
  void didUpdateWidget(PianoRoll oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.showPhonemes != oldWidget.showPhonemes) {
      setState(() => _showPhonemes = widget.showPhonemes);
    }
    if (widget.showFxOverlay != oldWidget.showFxOverlay) {
      setState(() => _showFxOverlay = widget.showFxOverlay);
    }
    if (widget.selectedTrackIndex != oldWidget.selectedTrackIndex) {
      _notes = List.of(_activeTrack?.notes ?? const []);
      _selectedNote = null;
      _dragNote = null;
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
      // Audio finished too (parent tracks completion to stop the player).
      widget.onPlayStopped?.call();
    }
  }

  void _togglePlay() {
    if (_notes.isEmpty) return;
    if (_playing) {
      _playCtrl.stop();
      setState(() => _playing = false);
      // Stop the parent's audio — the roll only animates the playhead,
      // without this the sound keeps playing ("graphic หยุดแต่เสียงไม่").
      widget.onPlayStopped?.call();
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
    // View tool: pan the roll (h/v scroll) — never selects or edits.
    if (_tool == PianoRollTool.view) {
      _panning = true;
      _panStart = pos;
      _panH0 = _hScroll.offset;
      _panV0 = _vScroll.offset;
      return;
    }
    // Pencil tool: freehand pitch drawing starts immediately.
    if (_tool == PianoRollTool.pencil && _showPitch) {
      setState(() => _pencilWrite(pos));
      return;
    }
    // NOTE hit-testing wins over the curve: the curve line runs through
    // the notes (same row), so testing the curve first steals every note
    // tap (opens curve-drag, inserts a point mid-pointer-event → the
    // "_dependents.isEmpty" crash). Curve points are still grabbable via
    // their 12px handle; the line only creates points on empty space.
    final hit = _hitTest(pos);
    // OM-style line editing: dragging reshapes the curve under the finger
    // — no point handles. Active on empty space in All mode, anywhere in
    // Curve mode; never in Notes-only mode.
    final lineDrag = (_tool == PianoRollTool.select &&
        _showPitch &&
        (_selectTarget == SelectTarget.curve ||
            (_selectTarget == SelectTarget.all && hit == null)));
    setState(() {
      if (lineDrag) {
        _selectedNote = null;
        _dragNote = null;
        _pencilWrite(pos);
        return;
      }
      if (hit != null) {
        // Curve-only select mode: notes are inert (no select/move/resize).
        if (_tool == PianoRollTool.select &&
            _selectTarget == SelectTarget.curve) {
          _selectedNote = null;
          return;
        }
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
    // --- view tool: pan the roll (follows the pointer delta) ---
    if (_panning) {
      final dx = e.localPosition.dx - _panStart.dx;
      final dy = e.localPosition.dy - _panStart.dy;
      if (_hScroll.hasClients) {
        _hScroll.jumpTo((_panH0 - dx).clamp(0.0, _hScroll.position.maxScrollExtent));
      }
      if (_vScroll.hasClients) {
        _vScroll.jumpTo((_panV0 - dy).clamp(0.0, _vScroll.position.maxScrollExtent));
      }
      return;
    }
    // --- line edit (select + pencil): the curve follows the finger at
    // ~3px resolution (dense capture — the user asked for a higher input
    // capture rate than the old 0.1-beat thinning) ---
    if (_tool == PianoRollTool.pencil ||
        (_tool == PianoRollTool.select && _showPitch &&
            _pencilLastDx >= 0)) {
      if ((e.localPosition.dx - _pencilLastDx).abs() > _capturePx) {
        setState(() => _pencilWrite(e.localPosition));
      }
      return;
    }
    // --- curve point drag (edits the pitch curve, not the notes) ---
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
    _panning = false;
    _pencilLastDx = double.negativeInfinity;
    _longPressTimer?.cancel();
    // A clean tap (no drag, no curve drag) on a selected note edits the
    // lyric. showDialog must NOT run inside setState (pushing a route mid-
    // rebuild → "_dependents.isEmpty is not true"); capture the index here,
    // then open the dialog after the state update settles.
    int? tapEditIdx;
    setState(() {
      if (!_dragMoved &&
          _selectedNote != null &&
          widget.onNotesChanged != null) {
        final idx = _notes.indexOf(_selectedNote!);
        if (idx >= 0) {
          tapEditIdx = idx;
        }
      }
      _dragNote = null;
      _resizeNote = null;
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
        // Clear the note's phoneme when the lyric changes: the painter
        // derives labels from the lyric's `[hint]`, and a stale `phoneme`
        // field would win over the NEW hint → labels froze after edits
        // ("phoneme เหนือ note ไม่ update").
        _notes[idx] = note.copyWith(lyric: result.trim(), phoneme: '');
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
              Icons.pan_tool_alt_outlined,
              'View',
              _tool == PianoRollTool.view,
              () => setState(() => _tool = PianoRollTool.view),
            ),
            const SizedBox(width: 2),
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
              Icons.gesture_rounded,
              'Pitch',
              _tool == PianoRollTool.pencil,
              () => setState(() => _tool = PianoRollTool.pencil),
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
            const SizedBox(width: 2),
            _toolButton(
              _selectTarget == SelectTarget.notes
                  ? Icons.music_note_outlined
                  : (_selectTarget == SelectTarget.curve
                      ? Icons.show_chart_rounded
                      : Icons.select_all_rounded),
              _selectTarget == SelectTarget.all
                  ? 'Select All'
                  : (_selectTarget == SelectTarget.notes ? 'Select Notes' : 'Select Curve'),
              _tool == PianoRollTool.select,
              () => setState(() {
                _tool = PianoRollTool.select;
                _selectTarget = _selectTarget == SelectTarget.all
                    ? SelectTarget.notes
                    : (_selectTarget == SelectTarget.notes
                        ? SelectTarget.curve
                        : SelectTarget.all);
              }),
            ),
            const SizedBox(width: 10),
            _toolButton(
              Icons.waves_rounded,
              'Pitch',
              _showPitch,
              () => setState(() => _showPitch = !_showPitch),
            ),
            const SizedBox(width: 2),
            _toolButton(
              Icons.bolt_rounded,
              'FX',
              _showFxOverlay,
              () => setState(() => _showFxOverlay = !_showFxOverlay),
            ),
            const SizedBox(width: 2),
            PopupMenuButton<double>(
              tooltip: 'Capture grid',
              initialValue: _captureStep,
              onSelected: (v) => setState(() => _captureStep = v),
              position: PopupMenuPosition.under,
              color: const Color(0xFF20232D),
              itemBuilder: (context) => [
                for (final (label, v) in const [
                  ('Fine  (1/16 beat)', 0.0625),
                  ('Normal (1/8 beat)', 0.125),
                  ('Coarse (1/4 beat)', 0.25),
                ])
                  PopupMenuItem(
                    value: v,
                    child: Text(
                      label,
                      style: TextStyle(
                        fontSize: 12,
                        color: v == _captureStep
                            ? LiltColors.purple
                            : LiltColors.text,
                      ),
                    ),
                  ),
              ],
              child: Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                decoration: BoxDecoration(
                  color: const Color(0xFF2A263B),
                  borderRadius: BorderRadius.circular(5),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      _captureStep <= 0.07
                          ? 'Fine'
                          : (_captureStep >= 0.2 ? 'Coarse' : 'Normal'),
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
                  value: _zoomX.toDouble(),
                  min: 60,
                  max: 130,
                  onChanged: (v) => _applyZoom(v.round(), _zoomY, _viewportCenter),
                ),
              ),
            ),
            // Reliable −/+ zoom buttons (pinch can lose the gesture arena
            // to the scroll views on device).
            IconButton(
              icon: const Icon(Icons.zoom_out_rounded, size: 18),
              visualDensity: VisualDensity.compact,
              tooltip: 'Zoom X out',
              onPressed: () => _applyZoom(
                (_zoomX - 10).clamp(30, 300), _zoomY, _viewportCenter),
            ),
            IconButton(
              icon: const Icon(Icons.zoom_in_rounded, size: 18),
              visualDensity: VisualDensity.compact,
              tooltip: 'Zoom X in',
              onPressed: () => _applyZoom(
                (_zoomX + 10).clamp(30, 300), _zoomY, _viewportCenter),
            ),
            const SizedBox(width: 8),
            const Text(
              'Y',
              style: TextStyle(fontSize: 11, color: Color(0xFF8E94A7)),
            ),
            SizedBox(
              width: 110,
              child: Slider(
                value: _zoomY.toDouble(),
                min: 50,
                max: 150,
                onChanged: (v) => _applyZoom(_zoomX, v.round(), _viewportCenter),
              ),
            ),
            IconButton(
              icon: const Icon(Icons.zoom_out_rounded, size: 18),
              visualDensity: VisualDensity.compact,
              tooltip: 'Zoom Y out',
              onPressed: () => _applyZoom(
                _zoomX, (_zoomY - 10).clamp(30, 300), _viewportCenter),
            ),
            IconButton(
              icon: const Icon(Icons.zoom_in_rounded, size: 18),
              visualDensity: VisualDensity.compact,
              tooltip: 'Zoom Y in',
              onPressed: () => _applyZoom(
                _zoomX, (_zoomY + 10).clamp(30, 300), _viewportCenter),
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
        // Never scroll while dragging a note, editing the pitch curve
        // (line edit in progress), or resizing — the Listener doesn't
        // consume the pointer, so without this the scroll view steals the
        // drag (curve edits scroll the roll instead of reshaping it).
        final physics =
            (_dragNote != null ||
                    _resizeNote != null ||
                    _pencilLastDx >= 0)
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
                  child: GestureDetector(
                    key: _rollKey,
                    // Pinch-to-zoom on the roll (30%..300% of base),
                    // relative to the canvas: the content point under the
                    // finger stays put (no drift while zooming).
                    onScaleStart: (_) {
                      _zoomStart = _zoomX.toDouble();
                      _zoomYStart = _zoomY.toDouble();
                    },
                    onScaleUpdate: (d) {
                      final nx =
                          (_zoomStart * d.scale).clamp(30.0, 300.0).round();
                      final ny =
                          (_zoomYStart * d.scale).clamp(30.0, 300.0).round();
                      if ((nx - _zoomX).abs() >= 1 ||
                          (ny - _zoomY).abs() >= 1) {
                        final box = _rollKey.currentContext?.findRenderObject()
                            as RenderBox?;
                        final local = box == null
                            ? Offset.zero
                            : box.globalToLocal(d.focalPoint);
                        _applyZoom(nx, ny, local);
                      }
                    },
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
                                showPhonemes: _showPhonemes,
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
                                    _selectedNote != null,
                                contentWidth: contentW,
                                contentHeight: contentH,
                              ),
                            ),
                            // Layer 3: FX affected-area overlay (chunk
                            // marks + GR curve + hot rings) — above the
                            // curve, below the selection outline.
                            if (_showFxOverlay)
                              CustomPaint(
                                size: Size(contentW, contentH),
                                painter: _FxOverlayPainter(
                                  notes: _notes,
                                  ppb: _ppb,
                                  contentWidth: contentW,
                                  contentHeight: contentH,
                                  chunkPx: 230,
                                ),
                              ),
                            // Layer 4 (top): selected note outline (white),
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
    this.showPhonemes = false,
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

  /// SynthV-style: draw phoneme labels ABOVE each note (dark boxes, one
  /// per segment) when the note carries phonemes. Mirrors ui-mock.
  final bool showPhonemes;

  /// Phonemes for the label boxes: the lyric's `word[ph ph]` hint FIRST
  /// (the painter must always reflect the CURRENT lyric — a stale
  /// `phoneme` field would freeze the labels after lyric edits), falling
  /// back to the note's `phoneme` field when the lyric has no hint (a
  /// real phonemizer's output, e.g. future IPA support), and finally a
  /// letter→alias split of the lyric against the voicebank's oto alias
  /// set — NOT a raw copy of the lyric word ("มันแค่ copy lyric ไม่ใช่
  /// phoneme"): 'na' resolves to `['n', 'A']` when both single-phoneme
  /// aliases exist.
  List<String> _phonemesOf(Note n) {
    final m = RegExp(r'\[([^\]]+)\]').firstMatch(n.lyric);
    final hint = m?.group(1)?.trim() ?? '';
    if (hint.isNotEmpty) {
      return hint.split(RegExp(r'\s+')).where((s) => s.isNotEmpty).toList();
    }
    final raw = n.phoneme.trim();
    if (raw.isNotEmpty) {
      return raw.split(RegExp(r'\s+')).where((s) => s.isNotEmpty).toList();
    }
    // No hint and no phoneme field: split the lyric into its letters and
    // keep only those that exist as single-phoneme aliases in the
    // voicebank (case-normalized — 'a' → 'A', 'i' → 'I', ...). This is a
    // real phoneme resolution, not a lyric copy. E.g. 'na' → n + A.
    final word = n.lyric.replaceAll(RegExp(r'\[[^\]]*\]'), '').trim();
    if (word.isNotEmpty) {
      final resolved = <String>[];
      for (final ch in word.split('')) {
        final upper = ch.toUpperCase();
        if (_otoAliases.contains(upper)) {
          resolved.add(upper);
        }
      }
      if (resolved.isNotEmpty) return resolved;
      return [word]; // no resolvable letters → show the word as-is
    }
    return const [];
  }

  /// Single-phoneme aliases present in the Teto English oto.ini (verified
  /// against the bank — EXACT set: 3 A b d D e E f g i I j k l m n N O p
  /// s S t T u U v V w z Z; uppercase-normalized here since the painter
  /// compares upper-cased letters). Used by [_phonemesOf] to resolve a
  /// hint-less lyric letter-by-letter into real voicebank phonemes.
  static const Set<String> _otoAliases = {
    '3', 'A', 'B', 'D', 'E', 'F', 'G', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'S', 'T', 'U', 'V', 'W', 'Z',
  };

  double _noteTopY(int pitch) =>
      (topPitch - pitch) * rowHeight + (rowHeight - _noteHeightFor(rowHeight)) / 2;

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
      _noteHeightFor(rowHeight),
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
    // SynthV-style: phoneme labels ABOVE the note (dark boxes, one per
    // segment, spanning the note width). Mirrors ui-mock/index.html.
    // NOTE: `segs.isNotEmpty` (NOT `length > 1`) — a lyric without a
    // hint falls back to a single word box (e.g. 'na'), and the old
    // `> 1` guard silently skipped it → the label vanished entirely
    // ("ไม่มี label ด้วยซ้ำ").
    if (showPhonemes) {
      final segs = _phonemesOf(n);
      if (segs.isNotEmpty && rect.width >= 24) {
        final boxH = _noteHeightFor(rowHeight) * 0.46; // scales with Y zoom
        final top = rect.top - boxH - 2;
        final totalW = rect.width;
        final nSegs = segs.length;
        final boxW = (totalW - (nSegs - 1) * 2) / nSegs;
        final segStyle = TextStyle(
          fontSize: 8,
          fontWeight: FontWeight.w700,
          color: Colors.white,
          height: 1.0,
        );
        for (var i = 0; i < nSegs; i++) {
          final box = Rect.fromLTWH(rect.left + i * (boxW + 2), top, boxW, boxH);
          canvas.drawRRect(
            RRect.fromRectAndRadius(box, const Radius.circular(3)),
            Paint()..color = const Color(0xB3000000),
          );
          canvas.drawRRect(
            RRect.fromRectAndRadius(box, const Radius.circular(3)),
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1
              ..color = Colors.white.withValues(alpha: 0.25),
          );
          if (boxW >= 12) {
            final tp = TextPainter(
              text: TextSpan(text: segs[i], style: segStyle),
              textDirection: TextDirection.ltr,
              maxLines: 1,
            )..layout(maxWidth: boxW - 2);
            tp.paint(
              canvas,
              Offset(
                box.left + (box.width - tp.width) / 2,
                box.top + (box.height - tp.height) / 2,
              ),
            );
          }
        }
      }
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
      oldDelegate.rowHeight != rowHeight ||
      oldDelegate.showPhonemes != showPhonemes;
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
      // Catmull-Rom spline: smooth AND passes through every point (the
      // old midpoint-cubic didn't touch the points, so the drawn curve
      // drifted from where the finger actually drew).
      final path = Path()..moveTo(pts.first.dx, pts.first.dy);
      for (var i = 0; i < pts.length - 1; i++) {
        final p0 = pts[i > 0 ? i - 1 : i];
        final p1 = pts[i];
        final p2 = pts[i + 1];
        final p3 = pts[i + 2 < pts.length ? i + 2 : i + 1];
        const steps = 8;
        for (var s = 1; s <= steps; s++) {
          final t = s / steps;
          final t2 = t * t;
          final t3 = t2 * t;
          final x = 0.5 *
              ((2 * p1.dx) +
                  (-p0.dx + p2.dx) * t +
                  (2 * p0.dx - 5 * p1.dx + 4 * p2.dx - p3.dx) * t2 +
                  (-p0.dx + 3 * p1.dx - 3 * p2.dx + p3.dx) * t3);
          final y = 0.5 *
              ((2 * p1.dy) +
                  (-p0.dy + p2.dy) * t +
                  (2 * p0.dy - 5 * p1.dy + 4 * p2.dy - p3.dy) * t2 +
                  (-p0.dy + 3 * p1.dy - 3 * p2.dy + p3.dy) * t3);
          path.lineTo(x, y);
        }
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
    // No point handles — OM-style: the LINE is the edit surface. Dragging
    // anywhere on the curve reshapes it under the finger (points are
    // upserted along the drag in the pointer handlers).
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

/// Layer 3 of the roll: the FX affected-area overlay (chunk marks,
/// gain-reduction curve, hot-note rings) — mirrors ui-mock's ⚡ FX layer.
class _FxOverlayPainter extends CustomPainter {
  _FxOverlayPainter({
    required this.notes,
    required this.ppb,
    required this.contentWidth,
    required this.contentHeight,
    required this.chunkPx,
  });

  final List<Note> notes;
  final double ppb;
  final double contentWidth;
  final double contentHeight;

  /// Chunk boundary spacing in px (mock: ~230px ≈ 2s @ 128bpm).
  final double chunkPx;

  @override
  void paint(Canvas canvas, Size size) {
    // Chunk boundaries: dashed vertical lines with a small hash label
    // (mirrors the backend's ~2s time-based chunk cache keys).
    final chunkPaint = Paint()
      ..color = const Color(0x6655D7DE)
      ..strokeWidth = 1;
    for (var x = chunkPx; x < size.width; x += chunkPx) {
      final dash = Path();
      for (var y = 0.0; y < size.height; y += 8) {
        dash
          ..moveTo(x, y)
          ..lineTo(x, (y + 4).clamp(0, size.height));
      }
      canvas.drawPath(dash, chunkPaint);
      final label = TextPainter(
        text: TextSpan(
          text: ((x * 2654435761) % 0x1000000).toInt().toRadixString(16).padLeft(6, '0'),
          style: const TextStyle(
            fontSize: 8,
            color: Color(0xFF55D7DE),
            fontWeight: FontWeight.w600,
          ),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      label.paint(canvas, Offset(x + 4, 26));
    }
    // Gain-reduction curve (stylized cyan dashed line over the notes).
    if (notes.length >= 2) {
      final sorted = List.of(notes)
        ..sort((a, b) => a.position.compareTo(b.position));
      final line = Path();
      final grPaint = Paint()
        ..color = const Color(0xCC55D7DE)
        ..strokeWidth = 2
        ..style = PaintingStyle.stroke;
      for (var i = 0; i < sorted.length; i++) {
        final n = sorted[i];
        final x = (n.position + n.duration / 2) * ppb;
        final y = size.height - 20 - (40 * (0.55 + 0.45 * (i % 3) / 2));
        if (i == 0) {
          line.moveTo(x, y);
        } else {
          line.lineTo(x, y);
        }
      }
      canvas.drawPath(line, grPaint);
    }
    // Hot notes: notes the FX pushes hardest get a cyan glow ring.
    for (var i = 0; i < notes.length; i += 2) {
      final n = notes[i];
      final rect = Rect.fromLTWH(
        n.position * ppb,
        (48 - n.pitch) * 32 + (32 - _noteHeightFor(32)) / 2,
        n.duration * ppb,
        _noteHeightFor(32),
      );
      canvas.drawRRect(
        RRect.fromRectAndRadius(rect, const Radius.circular(3)),
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.5
          ..color = const Color(0x8855D7DE),
      );
    }
  }

  @override
  bool shouldRepaint(_FxOverlayPainter oldDelegate) =>
      oldDelegate.notes != notes ||
      oldDelegate.ppb != ppb ||
      oldDelegate.chunkPx != chunkPx;
}

/// Layer 4 (top) of the roll: the selected note's cyan outline, painted
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
      (topPitch - pitch) * rowHeight + (rowHeight - _noteHeightFor(rowHeight)) / 2;

  @override
  void paint(Canvas canvas, Size size) {
    final n = note;
    if (n == null) return;
    final rect = Rect.fromLTWH(
      n.position * ppb,
      _noteTopY(n.pitch),
      n.duration * ppb,
      _noteHeightFor(rowHeight),
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
