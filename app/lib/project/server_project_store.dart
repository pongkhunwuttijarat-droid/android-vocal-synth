import '../api/engine_client.dart';
import '../models.dart';
import 'project_doc.dart';
import 'project_store.dart';

/// Server-backed project store.
///
/// ARCHITECTURE: ALL project data lives on the server ("ทุก project data
/// เลย"). [save] ships the codec JSON view over HTTP; the server encodes
/// it to `.ustx` and owns the file. [load] gets the JSON view back
/// (server-side decode). This class is pure HTTP — no `dart:io`, no FFI.
class ServerProjectStore implements ProjectStore {
  ServerProjectStore(this._client);

  final ApiClient _client;

  @override
  Future<List<ProjectMeta>> list() async {
    final names = await _client.listProjects();
    return [
      for (final n in names) ProjectMeta(name: n, path: n),
    ];
  }

  @override
  Future<String> save(ProjectDoc doc) async {
    await _client.saveProject(
      name: doc.name,
      projectView: {
        'name': doc.name,
        'bpm': doc.bpm,
        'time_signature': doc.timeSignature,
        'tracks': [
          for (var i = 0; i < doc.tracks.length; i++)
            trackToView(doc.tracks[i], doc.curvesByTrack?[i] ?? const []),
        ],
      },
    );
    return doc.name;
  }

  @override
  Future<ProjectDoc?> load(String path) async {
    final view = await _client.loadProject(path);
    if (view == null) return null;
    return viewToDoc(view);
  }

  @override
  Future<void> delete(String path) async {
    await _client.deleteProject(path);
  }
}

/// Track → codec JSON view (curves as a `pitd` expression, beat*100
/// centi-beats so the round-trip is exact).
Map<String, dynamic> trackToView(Track t, List<PitchPoint> curve) => {
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
          'xs': [for (final p in curve) (p.beat * 100).round()],
          'ys': [for (final p in curve) p.semitones.round()],
        },
      ],
    };

/// Codec JSON view → ProjectDoc (shared with the open flow).
ProjectDoc viewToDoc(Map<String, dynamic> view) {
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
