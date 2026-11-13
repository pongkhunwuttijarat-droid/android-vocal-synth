import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:rusted_utau/project/ustx_codec.dart';

/// Integration test for the Rust `.ustx` codec plugin — loads the real
/// `libustx_codec.so` (cdylib output from `cargo build -p ustx-codec`).
///
/// Skipped automatically when the .so is not present (e.g. CI without a
/// Rust toolchain): the plugin boundary itself is Rust-tested; this
/// verifies the Dart wrapper's byte contract against the real .so.
void main() {
  const soPath =
      '/home/seal/project/android-voice-synth/native/target/debug/libustx_codec.so';

  test('decode parses a .ustx file into the JSON view', () async {
    if (!File(soPath).existsSync()) {
      markTestSkipped('libustx_codec.so not built — run cargo build -p ustx-codec');
      return;
    }
    final codec = await UstxCodec.load(soDir: File(soPath).parent.path);

    const ustx = '''
ustx_version: '0.9'
name: midnight_demo
resolution_ppq: 192
tempos:
  - position: 0
    bpm: 110
time_signatures:
  - bar_position: 0
    beat_per_bar: 4
    beat_unit: 4
tracks:
  - track_name: Vocal Scale
    color: '#7358E9'
    singer: ''
    phonemizer: ''
voice_parts:
  - name: Part1
    track_no: 0
    position: 0
    duration: 192
    notes:
      - position: 96
        duration: 192
        tone: 0
        lyric: la[l A]
''';

    final view = codec.decode(utf8.encode(ustx));
    expect(view['name'], 'midnight_demo');
    expect(view['bpm'], 110);
    final tracks = view['tracks'] as List;
    expect(tracks, hasLength(1));
    final track = tracks[0] as Map<String, dynamic>;
    expect(track['name'], 'Vocal Scale');
    final notes = track['notes'] as List;
    expect(notes, hasLength(1));
    final note = notes[0] as Map<String, dynamic>;
    expect(note['lyric'], 'la[l A]');
    expect(note['pitch'], 0);
    expect(note['position'], closeTo(0.5, 1e-9)); // 96 ticks / 192
    expect(note['duration'], closeTo(1.0, 1e-9));
  });

  test('encode builds .ustx bytes from the JSON view (round-trip)', () async {
    if (!File(soPath).existsSync()) {
      markTestSkipped('libustx_codec.so not built — run cargo build -p ustx-codec');
      return;
    }
    final codec = await UstxCodec.load(soDir: File(soPath).parent.path);

    final yaml = codec.encode({
      'name': 'song',
      'bpm': 120,
      'time_signature': '4/4',
      'tracks': [
        {
          'name': 'Vocal',
          'color': '#000000',
          'notes': [
            {'lyric': 'la[l A]', 'pitch': 0, 'position': 0.0, 'duration': 1.0},
          ],
        },
      ],
    });
    final text = utf8.decode(yaml);
    expect(text, contains('ustx_version'));
    expect(text, contains('name: song'));
    // Round-trip: decode what we just encoded.
    final view = codec.decode(yaml);
    expect(view['bpm'], 120);
    final tracks = view['tracks'] as List;
    expect(tracks, hasLength(1));
    final notes = (tracks[0] as Map)['notes'] as List;
    expect(notes, hasLength(1));
    expect((notes[0] as Map)['lyric'], 'la[l A]');
  });
}
