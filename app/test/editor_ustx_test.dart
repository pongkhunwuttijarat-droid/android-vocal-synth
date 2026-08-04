import 'package:flutter_test/flutter_test.dart';
import 'package:lilt/editor_ustx.dart';
import 'package:lilt/models.dart';

void main() {
  test('buildUstx serializes notes with ticks/tone mapping', () {
    final ustx = buildUstx(
      name: 'Test Song',
      tracks: [
        TrackNotes(name: 'Lead', notes: [
          Note(lyric: 'mi', pitch: 0, position: 1.0, duration: 0.5),
          Note(lyric: 'la', pitch: 2, position: 2.0, duration: 1.0),
        ]),
      ],
    );

    // Header.
    expect(ustx, contains('ustx_version: 0.9'));
    expect(ustx, contains('name: Test Song'));
    expect(ustx, contains('bpm: 120'));

    // Note 1: position 1.0 beat → 480 ticks, tone 60+0.
    expect(ustx, contains('  - position: 480'));
    expect(ustx, contains('    duration: 240'));
    expect(ustx, contains('    tone: 60'));
    expect(ustx, contains('    lyric: mi'));

    // Note 2: position 2.0 beat → 960 ticks, tone 60+2.
    expect(ustx, contains('  - position: 960'));
    expect(ustx, contains('    duration: 480'));
    expect(ustx, contains('    tone: 62'));
    expect(ustx, contains('    lyric: la'));

    // Single track + voice part scaffolding (engine expects this shape).
    expect(ustx, contains('voice_parts:'));
    expect(ustx, contains('tracks:'));
  });

  test('beatsToTicks maps beats to 480 ticks/beat', () {
    expect(beatsToTicks(0.0), 0);
    expect(beatsToTicks(0.5), 240);
    expect(beatsToTicks(1.0), 480);
    expect(beatsToTicks(4.0), 1920);
  });

  test('buildUstx writes drawn pitch curves as ustx pitch data', () {
    // Note at beat 1.0..1.75; curve point at beat 1.25, +2 semis.
    // x = (1.25-1.0)*500ms = 125ms; y = 2*10 = 20.
    final ustx = buildUstx(
      name: 'Curve',
      tracks: [
        TrackNotes(name: 'Lead', notes: [
          Note(lyric: 'mi', pitch: 12, position: 1.0, duration: 0.75),
        ]),
      ],
      curvesByTrack: {
        0: [PitchPoint(1.25, 2.0)],
      },
    );
    expect(ustx, contains('x: 125'));
    expect(ustx, contains('y: 20'));
  });

  test('buildUstx skips curve points outside the note', () {
    final ustx = buildUstx(
      name: 'Curve2',
      tracks: [
        TrackNotes(name: 'Lead', notes: [
          Note(lyric: 'mi', pitch: 12, position: 1.0, duration: 0.5),
        ]),
      ],
      curvesByTrack: {
        0: [PitchPoint(3.0, 5.0)],
      },
    );
    expect(ustx, isNot(contains('x: 1000')));
  });

  test('parseUstx round-trips multi-track projects', () {
    final ustx = buildUstx(
      name: 'Round Trip',
      tracks: [
        TrackNotes(name: 'Lead Vocal', notes: [
          Note(lyric: 'so[s oU]', pitch: 7, position: 0.0, duration: 1.0),
          Note(lyric: 'can[k A n]', pitch: 7, position: 1.0, duration: 0.5),
        ]),
        TrackNotes(name: 'Harmony', notes: [
          Note(lyric: 'ooh', pitch: 4, position: 1.0, duration: 1.0),
        ]),
      ],
    );
    final parsed = parseUstx(ustx);
    expect(parsed.length, 2);
    expect(parsed[0].name, 'Lead Vocal');
    expect(parsed[1].name, 'Harmony');
    expect(parsed[0].notes.length, 2);
    expect(parsed[0].notes[0].lyric, 'so[s oU]');
    expect(parsed[0].notes[0].pitch, 7);
    expect(parsed[0].notes[0].position, 0.0);
    expect(parsed[0].notes[0].duration, 1.0);
    expect(parsed[0].notes[1].lyric, 'can[k A n]');
    expect(parsed[1].notes[0].pitch, 4);
  });

  test('parseUstx handles pitch > 48 (high notes stay in range)', () {
    // C8 = MIDI 108 → pitch 48; B7 = 107 → 47.
    final ustx = buildUstx(
      name: 'High',
      tracks: [
        TrackNotes(name: 'Lead', notes: [
          Note(lyric: 'hi', pitch: 48, position: 0.0, duration: 1.0),
        ]),
      ],
    );
    final parsed = parseUstx(ustx);
    expect(parsed[0].notes[0].pitch, 48);
  });
}
