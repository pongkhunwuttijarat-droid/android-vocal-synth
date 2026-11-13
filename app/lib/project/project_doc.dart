import '../models.dart';

/// A saved Rusted UTAU project — the document model for ProjectStore.
///
/// NOTE (architecture): this is a Dart view-model for the UI + storage
/// orchestration ONLY. The authoritative serialize/parse of the .ustx
/// format lives in the Rust side (libustxcodec.so — see plan Task 2);
/// [ProjectDoc] mirrors that contract for the store layer.
class ProjectDoc {
  const ProjectDoc({
    required this.name,
    required this.bpm,
    required this.tracks,
    this.timeSignature = '4/4',
    this.curvesByTrack,
  });

  final String name;
  final double bpm;
  final String timeSignature;
  final List<Track> tracks;

  /// Drawn pitch curves per track index (saved so the curve survives
  /// save → open). Null = no curves drawn yet.
  final Map<int, List<PitchPoint>>? curvesByTrack;

  ProjectDoc copyWith({
    String? name,
    double? bpm,
    List<Track>? tracks,
    Map<int, List<PitchPoint>>? curvesByTrack,
  }) =>
      ProjectDoc(
        name: name ?? this.name,
        bpm: bpm ?? this.bpm,
        timeSignature: timeSignature,
        tracks: tracks ?? this.tracks,
        curvesByTrack: curvesByTrack ?? this.curvesByTrack,
      );
}

/// Lightweight metadata for the project list UI (no full document).
class ProjectMeta {
  const ProjectMeta({required this.name, required this.path});

  final String name;
  final String path;
}
