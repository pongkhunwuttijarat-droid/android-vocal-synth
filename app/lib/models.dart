/// Data models for the Rusted UTAU UI layer.
///
/// These mirror the native engine data contracts (see DATA_CONTRACTS.md) but
/// stay deliberately small: the UI talks to the engine through a future
/// bridge, so these are view models, not the source of truth.
library;

/// A single singing note in the piano roll.
class Note {
  Note({
    required this.lyric,
    required this.pitch,
    required this.position,
    required this.duration,
    this.phoneme = '',
    this.engineId,
    this.enginePartId,
    this.enginePartPositionTicks = 0,
    this.vibrato = const {},
    String? localKey,
  }) : localKey = localKey ?? 'note-${_nextLocalKey++}';

  static int _nextLocalKey = 0;

  /// Stable identity supplied by the Rust project session. Null only for a
  /// transient note preview that has not yet been accepted by the engine.
  final String? engineId;

  /// Part containing this note; used when adding an unpersisted note.
  final String? enginePartId;
  final int enginePartPositionTicks;

  /// Disposable view of the engine-owned OpenUtau UVibrato value.
  final Map<String, dynamic> vibrato;

  /// Ephemeral identity used to match a newly drawn note while its add
  /// command is in flight. This never leaves the frontend.
  final String localKey;

  /// Lyric text shown on the note (e.g. "the").
  final String lyric;

  /// Pitch in semitones relative to a reference (0 = C4 for the mock).
  final int pitch;

  /// Start position in beats.
  final double position;

  /// Length in beats.
  final double duration;

  /// Phonemized form (filled by the phonemizer backend later).
  final String phoneme;

  Note copyWith({
    String? lyric,
    int? pitch,
    double? position,
    double? duration,
    String? phoneme,
    String? engineId,
    String? enginePartId,
    int? enginePartPositionTicks,
    String? localKey,
    Map<String, dynamic>? vibrato,
  }) {
    return Note(
      lyric: lyric ?? this.lyric,
      pitch: pitch ?? this.pitch,
      position: position ?? this.position,
      duration: duration ?? this.duration,
      phoneme: phoneme ?? this.phoneme,
      engineId: engineId ?? this.engineId,
      enginePartId: enginePartId ?? this.enginePartId,
      enginePartPositionTicks:
          enginePartPositionTicks ?? this.enginePartPositionTicks,
      localKey: localKey,
      vibrato: vibrato ?? this.vibrato,
    );
  }
}

/// A track inside the editor (vocal track, guide melody, ...).
class Track {
  const Track({
    required this.name,
    required this.colorSeed,
    required this.notes,
    this.engineId,
    this.enginePartId,
    this.enginePartPositionTicks = 0,
    this.mixFx,
  });

  final String name;

  /// Base color seed used to tint notes of this track.
  final int colorSeed;

  final List<Note> notes;
  final String? engineId;
  final String? enginePartId;
  final int enginePartPositionTicks;
  final Map<String, dynamic>? mixFx;

  Track copyWith({List<Note>? notes}) => Track(
    name: name,
    colorSeed: colorSeed,
    notes: notes ?? this.notes,
    engineId: engineId,
    enginePartId: enginePartId,
    enginePartPositionTicks: enginePartPositionTicks,
    mixFx: mixFx,
  );
}

/// A voicebank installed/imported into the app.
class Voicebank {
  const Voicebank({
    required this.name,
    required this.format,
    required this.status,
    this.singer = '',
    this.sizeMb = 0,
  });

  final String name;
  final String format; // e.g. "OpenUtau", "UTAU"
  final String singer;

  /// ready | importing | error
  final String status;
  final int sizeMb;
}

/// One control point of the continuous pitch curve.
class PitchPoint {
  const PitchPoint(this.beat, this.semitones);
  final double beat;
  final double semitones;
}
