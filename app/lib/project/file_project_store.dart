import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models.dart';
import 'project_doc.dart';
import 'project_store.dart';
import 'ustx_codec.dart';

/// Filesystem-backed project store.
///
/// ARCHITECTURE: Dart = file access only. Serialization goes through the
/// Rust `.ustx` codec plugin (`UstxCodec` — JSON view in, bytes out);
/// this class only walks directories and reads/writes files. Project
/// files live in `<documents>/rusted-utau-projects/<name>.ustx`.
class FileProjectStore implements ProjectStore {
  FileProjectStore(this._codec, [Directory? projectsDir])
      : _overrideDir = projectsDir;

  final UstxCodec _codec;
  final Directory? _overrideDir;

  static Future<FileProjectStore> open() async {
    final codec = await UstxCodec.load();
    return FileProjectStore(codec);
  }

  /// Store rooted at [dir] (tests use a temp dir instead of documents).
  static FileProjectStore withDir(UstxCodec codec, Directory dir) =>
      FileProjectStore(codec, dir);

  Future<Directory> _projectsDir() async {
    final override = _overrideDir;
    if (override != null) return override;
    final docs = await getApplicationDocumentsDirectory();
    final dir = Directory('${docs.path}/rusted-utau-projects');
    if (!dir.existsSync()) dir.createSync(recursive: true);
    return dir;
  }

  String _fileName(String name) {
    final safe = name.replaceAll(RegExp(r'[^\w\- ]'), '_').trim();
    return '${safe.isEmpty ? 'untitled' : safe}.ustx';
  }

  @override
  Future<List<ProjectMeta>> list() async {
    final dir = await _projectsDir();
    final files = dir
        .listSync()
        .whereType<File>()
        .where((f) => f.path.endsWith('.ustx'))
        .toList()
      ..sort((a, b) => b.path.compareTo(a.path)); // newest-ish first
    return [
      for (final f in files)
        ProjectMeta(
          name: f.uri.pathSegments.last.replaceAll('.ustx', ''),
          path: f.path,
        ),
    ];
  }

  @override
  Future<String> save(ProjectDoc doc) async {
    // Build the JSON view (the codec's contract — Dart never knows the
    // .ustx wire format). Curves: one "pitd" curve per track.
    final view = <String, dynamic>{
      'name': doc.name,
      'bpm': doc.bpm,
      'time_signature': doc.timeSignature,
      'tracks': [
        for (var i = 0; i < doc.tracks.length; i++)
          trackToJson(doc.tracks[i], doc.curvesByTrack?[i] ?? const []),
      ],
    };
    final bytes = _codec.encode(view);
    final dir = await _projectsDir();
    final file = File('${dir.path}/${_fileName(doc.name)}');
    await file.writeAsBytes(bytes, flush: true);
    return file.path;
  }

  @override
  Future<ProjectDoc?> load(String path) async {
    final file = File(path);
    if (!file.existsSync()) return null;
    final bytes = await file.readAsBytes();
    final view = _codec.decode(bytes);
    final name = view['name'] as String? ?? 'untitled';
    final bpm = (view['bpm'] as num?)?.toDouble() ?? 120.0;
    final tracks = <Track>[];
    final curves = <int, List<PitchPoint>>{};
    final rawTracks = view['tracks'] as List? ?? [];
    for (var i = 0; i < rawTracks.length; i++) {
      final t = rawTracks[i] as Map<String, dynamic>;
      final notes = <Note>[
        for (final raw in (t['notes'] as List? ?? []))
          Note(
            lyric: (raw as Map)['lyric'] as String? ?? '',
            pitch: ((raw)['pitch'] as num?)?.toInt() ?? 0,
            position: ((raw)['position'] as num?)?.toDouble() ?? 0,
            duration: ((raw)['duration'] as num?)?.toDouble() ?? 1,
          ),
      ];
      tracks.add(Track(
        name: t['name'] as String? ?? 'Track${i + 1}',
        colorSeed: i,
        notes: notes,
      ));
      final rawCurves = t['curves'] as List? ?? [];
      final points = <PitchPoint>[];
      for (final raw in rawCurves) {
        final c = raw as Map<String, dynamic>;
        if (c['abbr'] != 'pitd') continue;
        final xs = (c['xs'] as List? ?? []).cast<num>();
        final ys = (c['ys'] as List? ?? []).cast<num>();
        for (var k = 0; k < xs.length && k < ys.length; k++) {
          points.add(PitchPoint(xs[k].toDouble() / 100, ys[k].toDouble()));
        }
      }
      if (points.isNotEmpty) curves[i] = points;
    }
    return ProjectDoc(
      name: name,
      bpm: bpm,
      timeSignature: view['time_signature'] as String? ?? '4/4',
      tracks: tracks,
      curvesByTrack: curves,
    );
  }

  @override
  Future<void> delete(String path) async {
    final f = File(path);
    if (f.existsSync()) await f.delete();
  }
}

// JSON view helpers shared by tests.
Map<String, dynamic> trackToJson(Track t, List<PitchPoint> curve) => {
      'name': t.name,
      'color': '#000000',
      'notes': [
        for (final n in t.notes)
          {
            'lyric': n.lyric,
            'pitch': n.pitch,
            'position': n.position,
            'duration': n.duration,
          },
      ],
      'curves': [
        {
          'abbr': 'pitd',
          // xs are ticks in the codec; encode beat*100 → centi-beats so
          // 0.25 beat → 25 → decodes back exactly (5× scale rounded
          // away 0.05 precision).
          'xs': [for (final p in curve) (p.beat * 100).round()],
          'ys': [for (final p in curve) p.semitones.round()],
        },
      ],
    };
