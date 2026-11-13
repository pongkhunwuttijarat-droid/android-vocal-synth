import '../models.dart';

/// Disposable editor view parsed from an authoritative Rust session snapshot.
/// The raw map can be kept for server-owned operations; this class never
/// serializes a project or decides whether a mutation is valid.
class EngineProjectSession {
  EngineProjectSession._({
    required this.sessionId,
    required this.revision,
    required this.canUndo,
    required this.canRedo,
    required this.projectId,
    required this.name,
    required this.bpm,
    required this.timeSignatureNumerator,
    required this.timeSignatureDenominator,
    required this.tracks,
    required this.curvesByTrack,
    required this.raw,
  });

  final String sessionId;
  final int revision;
  final bool canUndo;
  final bool canRedo;
  final String projectId;
  final String name;
  final double bpm;
  final int timeSignatureNumerator;
  final int timeSignatureDenominator;
  final List<Track> tracks;
  final Map<int, List<PitchPoint>> curvesByTrack;
  final Map<String, dynamic> raw;

  List<Map<String, dynamic>> get trackViews {
    final project = raw['project'] as Map<String, dynamic>? ?? const {};
    return (project['tracks'] as List? ?? const [])
        .whereType<Map<String, dynamic>>()
        .toList(growable: false);
  }

  List<Map<String, dynamic>> trackParts(int trackIndex) {
    final tracks = trackViews;
    if (trackIndex < 0 || trackIndex >= tracks.length) return const [];
    return (tracks[trackIndex]['parts'] as List? ?? const [])
        .whereType<Map<String, dynamic>>()
        .toList(growable: false);
  }

  factory EngineProjectSession.fromJson(Map<String, dynamic> json) {
    final project = json['project'] as Map<String, dynamic>? ?? const {};
    final rawTracks = (project['tracks'] as List? ?? const [])
        .whereType<Map<String, dynamic>>()
        .toList(growable: false);
    final curves = <int, List<PitchPoint>>{};
    final tracks = <Track>[];

    for (var trackIndex = 0; trackIndex < rawTracks.length; trackIndex++) {
      final rawTrack = rawTracks[trackIndex];
      final parts = (rawTrack['parts'] as List? ?? const [])
          .whereType<Map<String, dynamic>>()
          .toList(growable: false);
      // PianoRoll is currently one lane per track. It shows the first voice
      // part; the complete Rust document remains in the session for save and
      // render, including any additional parts.
      final part = parts.isEmpty ? null : parts.first;
      final partId = part?['id'] as String?;
      final partStart = (part?['position_ticks'] as num?)?.toInt() ?? 0;
      final rawNotes = (part?['notes'] as List? ?? const [])
          .whereType<Map<String, dynamic>>()
          .toList(growable: false);
      final notes = [
        for (final note in rawNotes)
          Note(
            engineId: note['id'] as String?,
            enginePartId: partId,
            enginePartPositionTicks: partStart,
            lyric: note['lyric'] as String? ?? '',
            pitch: ((note['tone'] as num?)?.toInt() ?? 60) - 60,
            position:
                (partStart + ((note['position_ticks'] as num?)?.toInt() ?? 0)) /
                480.0,
            duration:
                ((note['duration_ticks'] as num?)?.toInt() ?? 480) / 480.0,
            vibrato: Map<String, dynamic>.from(
              note['vibrato'] as Map? ?? const <String, dynamic>{},
            ),
          ),
      ];
      if (part != null && partId != null) {
        final rawCurves = (part['curves'] as List? ?? const [])
            .whereType<Map<String, dynamic>>()
            .where((curve) => curve['abbr'] == 'pitd')
            .toList(growable: false);
        if (rawCurves.isNotEmpty) {
          final points = (rawCurves.first['points'] as List? ?? const [])
              .whereType<Map<String, dynamic>>()
              .map((point) {
                final tick = (point['tick'] as num?)?.toInt() ?? 0;
                final value = (point['value'] as num?)?.toDouble() ?? 0;
                final beat = (partStart + tick) / 480.0;
                final base = _basePitchAt(notes, beat);
                return PitchPoint(beat, base + value / 100.0);
              })
              .toList(growable: false);
          curves[trackIndex] = points;
        }
      }
      tracks.add(
        Track(
          name: rawTrack['name'] as String? ?? 'Track ${trackIndex + 1}',
          colorSeed: trackIndex,
          engineId: rawTrack['id'] as String?,
          enginePartId: partId,
          enginePartPositionTicks: partStart,
          mixFx: rawTrack['mix_fx'] == null
              ? null
              : Map<String, dynamic>.from(rawTrack['mix_fx'] as Map),
          notes: notes,
        ),
      );
    }

    final signature =
        project['time_signature'] as Map<String, dynamic>? ?? const {};
    return EngineProjectSession._(
      sessionId: json['session_id'] as String? ?? '',
      revision: (json['revision'] as num?)?.toInt() ?? 0,
      canUndo: json['can_undo'] as bool? ?? false,
      canRedo: json['can_redo'] as bool? ?? false,
      projectId: project['id'] as String? ?? '',
      name: project['name'] as String? ?? 'Untitled',
      bpm: (project['bpm'] as num?)?.toDouble() ?? 120,
      timeSignatureNumerator: (signature['numerator'] as num?)?.toInt() ?? 4,
      timeSignatureDenominator:
          (signature['denominator'] as num?)?.toInt() ?? 4,
      tracks: tracks,
      curvesByTrack: curves,
      raw: json,
    );
  }

  static double _basePitchAt(List<Note> notes, double beat) {
    if (notes.isEmpty) return 0;
    final nearest = notes.reduce((a, b) {
      final ac = a.position + a.duration / 2;
      final bc = b.position + b.duration / 2;
      return (ac - beat).abs() <= (bc - beat).abs() ? a : b;
    });
    return nearest.pitch.toDouble();
  }
}
