/// Data models for the Lilt UI layer.
///
/// These mirror the native engine data contracts (see DATA_CONTRACTS.md) but
/// stay deliberately small: the UI talks to the engine through a future
/// bridge, so these are view models, not the source of truth.
library;

/// A single singing note in the piano roll.
class Note {
  const Note({
    required this.lyric,
    required this.pitch,
    required this.position,
    required this.duration,
    this.phoneme = '',
  });

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
  }) {
    return Note(
      lyric: lyric ?? this.lyric,
      pitch: pitch ?? this.pitch,
      position: position ?? this.position,
      duration: duration ?? this.duration,
      phoneme: phoneme ?? this.phoneme,
    );
  }
}

/// A track inside the editor (vocal track, guide melody, ...).
class Track {
  const Track({
    required this.name,
    required this.colorSeed,
    required this.notes,
  });

  final String name;

  /// Base color seed used to tint notes of this track.
  final int colorSeed;

  final List<Note> notes;

  Track copyWith({List<Note>? notes}) =>
      Track(name: name, colorSeed: colorSeed, notes: notes ?? this.notes);
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
