import 'package:flutter_test/flutter_test.dart';

import 'package:lilt/models.dart';
import 'package:lilt/project/project_doc.dart';
import 'package:lilt/project/project_store.dart';

void main() {
  group('MemoryProjectStore', () {
    test('save → list → load round-trip preserves everything', () async {
      final store = MemoryProjectStore();
      final doc = ProjectDoc(
        name: 'midnight_demo',
        bpm: 110,
        timeSignature: '4/4',
        tracks: [
          Track(
            name: 'Vocal Scale',
            colorSeed: 0x7358E9,
            notes: [
              Note(
                lyric: 'la[l A]',
                pitch: 0,
                position: 0.5,
                duration: 1.0,
              ),
              Note(
                lyric: 'mi[m i]',
                pitch: 2,
                position: 1.5,
                duration: 0.75,
                phoneme: 'm i',
              ),
            ],
          ),
          Track(
            name: 'Guide',
            colorSeed: 0x4C9EFF,
            notes: [
              Note(lyric: 'o[o]', pitch: 5, position: 0, duration: 2),
            ],
          ),
        ],
      );

      final path = await store.save(doc);
      expect(path, isNotEmpty);

      final listed = await store.list();
      expect(listed, hasLength(1));
      expect(listed.single.name, 'midnight_demo');

      final loaded = await store.load(path);
      expect(loaded, isNotNull);
      expect(loaded!.name, 'midnight_demo');
      expect(loaded.bpm, 110);
      expect(loaded.timeSignature, '4/4');
      expect(loaded.tracks, hasLength(2));
      expect(loaded.tracks[0].name, 'Vocal Scale');
      expect(loaded.tracks[0].colorSeed, 0x7358E9);
      expect(loaded.tracks[0].notes, hasLength(2));
      expect(loaded.tracks[0].notes[0].lyric, 'la[l A]');
      expect(loaded.tracks[0].notes[0].pitch, 0);
      expect(loaded.tracks[0].notes[0].position, 0.5);
      expect(loaded.tracks[0].notes[0].duration, 1.0);
      expect(loaded.tracks[0].notes[1].phoneme, 'm i');
      expect(loaded.tracks[1].name, 'Guide');
      expect(loaded.tracks[1].notes.single.lyric, 'o[o]');
    });

    test('delete removes the project', () async {
      final store = MemoryProjectStore();
      final doc = ProjectDoc(name: 'temp', bpm: 120, tracks: []);
      final path = await store.save(doc);
      expect(await store.list(), hasLength(1));

      await store.delete(path);
      expect(await store.list(), isEmpty);
      expect(await store.load(path), isNull);
    });

    test('list is empty for a fresh store', () async {
      final store = MemoryProjectStore();
      expect(await store.list(), isEmpty);
    });

    test('save with same name updates (upsert), does not duplicate', () async {
      final store = MemoryProjectStore();
      final a = ProjectDoc(name: 'song', bpm: 100, tracks: []);
      final b = ProjectDoc(name: 'song', bpm: 140, tracks: []);
      final p1 = await store.save(a);
      final p2 = await store.save(b);
      expect(p1, p2); // same path for same name
      expect(await store.list(), hasLength(1));
      final loaded = await store.load(p1);
      expect(loaded!.bpm, 140);
    });
  });
}
