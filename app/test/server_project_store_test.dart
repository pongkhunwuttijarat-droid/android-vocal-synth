import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';

import 'package:lilt/api/engine_client.dart';
import 'package:lilt/models.dart';
import 'package:lilt/project/project_doc.dart';
import 'package:lilt/project/server_project_store.dart';

/// ServerProjectStore + the new EngineClient project/phonemize methods,
/// driven by a MockClient — no network, no .so needed.
void main() {
  late List<http.Request> requests;
  late ApiClient client;
  late ServerProjectStore store;

  ApiClient makeClient(Future<http.Response> Function(http.Request) handler) {
    requests = [];
    final mock = MockClient((req) async {
      requests.add(req);
      return handler(req);
    });
    return ApiClient(baseUrl: 'http://test', httpClient: mock);
  }

  group('ServerProjectStore', () {
    test('save ships the JSON view to POST /project', () async {
      client = makeClient((req) async {
        if (req.method == 'POST' && req.url.path == '/project') {
          return http.Response('{"ok":true}', 200);
        }
        return http.Response('not found', 404);
      });
      store = ServerProjectStore(client);

      final path = await store.save(ProjectDoc(
        name: 'song',
        bpm: 110,
        tracks: [
          Track(name: 'Vocal', colorSeed: 0, notes: [
            Note(lyric: 'la[l A]', pitch: 0, position: 0.5, duration: 1),
          ]),
        ],
        curvesByTrack: {
          0: [PitchPoint(0.25, 0)],
        },
      ));

      expect(path, 'song');
      expect(requests, hasLength(1));
      final body = jsonDecode(requests.single.body) as Map<String, dynamic>;
      expect(body['name'], 'song');
      final view = body['project'] as Map<String, dynamic>;
      expect(view['tracks'][0]['notes'][0]['lyric'], 'la[l A]');
      // Curve survives into the view (xs = centi-beats).
      final curves = view['tracks'][0]['curves'] as List;
      expect(curves, hasLength(1));
      expect((curves[0] as Map)['abbr'], 'pitd');
      expect((curves[0] as Map)['xs'], [25]);
    });

    test('list reads GET /projects names', () async {
      client = makeClient((req) async {
        if (req.url.path == '/projects') {
          return http.Response(
            '{"projects":["b-song","a-song"]}',
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        return http.Response('not found', 404);
      });
      store = ServerProjectStore(client);

      final list = await store.list();
      expect(list.map((m) => m.name), ['b-song', 'a-song']);
    });

    test('load parses the JSON view back into a doc', () async {
      client = makeClient((req) async {
        if (req.url.path == '/project/song') {
          return http.Response(
            jsonEncode({
              'name': 'song',
              'bpm': 110.0,
              'time_signature': '4/4',
              'tracks': [
                {
                  'name': 'Vocal',
                  'color': '#000000',
                  'notes': [
                    {
                      'lyric': 'mi[m i]',
                      'pitch': 2,
                      'position': 1.5,
                      'duration': 0.75,
                    },
                  ],
                  'curves': [
                    {'abbr': 'pitd', 'xs': [100], 'ys': [3]},
                  ],
                },
              ],
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        return http.Response('not found', 404);
      });
      store = ServerProjectStore(client);

      final doc = await store.load('song');
      expect(doc, isNotNull);
      expect(doc!.name, 'song');
      expect(doc.tracks.single.notes.single.lyric, 'mi[m i]');
      final curve = doc.curvesByTrack![0]!;
      expect(curve.single.beat, closeTo(1.0, 0.001)); // 100 centi-beats
      expect(curve.single.semitones, closeTo(3.0, 0.001));
    });

    test('load returns null on 404', () async {
      client = makeClient((req) async => http.Response('{"error":"nf"}', 404));
      store = ServerProjectStore(client);
      expect(await store.load('missing'), isNull);
    });

    test('delete calls DELETE /project/<name>', () async {
      client = makeClient((req) async {
        if (req.method == 'DELETE' && req.url.path == '/project/song') {
          return http.Response('{"ok":true}', 200);
        }
        return http.Response('not found', 404);
      });
      store = ServerProjectStore(client);

      await store.delete('song');
      expect(requests.single.method, 'DELETE');
      expect(requests.single.url.path, '/project/song');
    });
  });

  group('EngineClient phonemize', () {
    test('posts project + voicebank, returns note phonemes', () async {
      client = makeClient((req) async {
        if (req.url.path == '/phonemize') {
          return http.Response(
            jsonEncode({
              'notes': [
                {'lyric': 'na', 'phonemes': ['n', 'A']},
                {'lyric': 'la[l A]', 'phonemes': ['l', 'A']},
              ],
            }),
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        return http.Response('not found', 404);
      });

      final resp = await client.phonemize(
        project: 'song',
        voicebank: 'teto-english',
      );
      final notes = resp['notes'] as List;
      expect((notes[0] as Map)['phonemes'], ['n', 'A']);
      final body = jsonDecode(requests.single.body) as Map<String, dynamic>;
      expect(body['project'], 'song');
      expect(body['voicebank'], 'teto-english');
      expect(body['track'], 0);
    });
  });
}
