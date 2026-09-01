import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:lilt/models.dart';
import 'package:lilt/project/file_project_store.dart';
import 'package:lilt/project/project_doc.dart';
import 'package:lilt/project/ustx_codec.dart';

/// FileProjectStore tests against the REAL Rust .ustx codec .so.
/// Skipped automatically when the plugin is not built (CI without Rust).
void main() {
  const soPath =
      '/home/seal/project/android-voice-synth/native/target/debug/libustx_codec.so';

  Future<FileProjectStore> openStore(Directory tmp) async {
    final codec = await UstxCodec.load(soDir: File(soPath).parent.path);
    return FileProjectStore.withDir(codec, tmp);
  }

  test('save → list → load round-trip with notes + curves', () async {
    if (!File(soPath).existsSync()) {
      markTestSkipped('libustx_codec.so not built');
      return;
    }
    final tmp = await Directory.systemTemp.createTemp('lilt-store');
    addTearDown(() => tmp.delete(recursive: true));
    final store = await openStore(tmp);

    final doc = ProjectDoc(
      name: 'my song',
      bpm: 110,
      tracks: [
        Track(
          name: 'Vocal',
          colorSeed: 0,
          notes: [
            Note(lyric: 'la[l A]', pitch: 0, position: 0.5, duration: 1.0),
            Note(lyric: 'mi[m i]', pitch: 2, position: 1.5, duration: 0.75),
          ],
        ),
      ],
      curvesByTrack: {
        0: [PitchPoint(0.25, 0), PitchPoint(1.0, 2.0)],
      },
    );

    final path = await store.save(doc);
    expect(path, contains('my song.ustx'));

    final listed = await store.list();
    expect(listed, hasLength(1));
    expect(listed.single.name, 'my song');

    final loaded = await store.load(path);
    expect(loaded, isNotNull);
    expect(loaded!.name, 'my song');
    expect(loaded.bpm, closeTo(110, 0.001));
    expect(loaded.tracks, hasLength(1));
    expect(loaded.tracks[0].notes, hasLength(2));
    expect(loaded.tracks[0].notes[0].lyric, 'la[l A]');
    expect(loaded.tracks[0].notes[0].position, closeTo(0.5, 0.001));
    // The drawn curve must survive the round-trip ("curve หายตอนย้ายหน้า").
    expect(loaded.curvesByTrack, isNotNull);
    final curve = loaded.curvesByTrack![0]!;
    expect(curve, hasLength(2));
    expect(curve[0].beat, closeTo(0.25, 0.01));
    expect(curve[1].semitones, closeTo(2.0, 0.01));
  });

  test('delete removes the project file', () async {
    if (!File(soPath).existsSync()) {
      markTestSkipped('libustx_codec.so not built');
      return;
    }
    final tmp = await Directory.systemTemp.createTemp('lilt-store');
    addTearDown(() => tmp.delete(recursive: true));
    final store = await openStore(tmp);

    final path = await store.save(
      ProjectDoc(name: 'temp', bpm: 120, tracks: []),
    );
    expect(await store.list(), hasLength(1));

    await store.delete(path);
    expect(await store.list(), isEmpty);
    expect(await store.load(path), isNull);
  });

  test('file content is .ustx YAML (not JSON)', () async {
    if (!File(soPath).existsSync()) {
      markTestSkipped('libustx_codec.so not built');
      return;
    }
    final tmp = await Directory.systemTemp.createTemp('lilt-store');
    addTearDown(() => tmp.delete(recursive: true));
    final store = await openStore(tmp);

    final path = await store.save(
      ProjectDoc(
        name: 'fmt',
        bpm: 120,
        tracks: [
          Track(
            name: 'Vocal',
            colorSeed: 0,
            notes: [Note(lyric: 'la[l A]', pitch: 0, position: 0, duration: 1)],
          ),
        ],
      ),
    );
    final text = await File(path).readAsString();
    expect(text, contains('ustx_version'));
    expect(text, contains('voice_parts'));
    expect(() => jsonDecode(text), throwsA(anything)); // not JSON
  });
}
