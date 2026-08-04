import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lilt/api/engine_client.dart';

http.Response _json(Object body, {int status = 200}) => http.Response(
  jsonEncode(body),
  status,
  headers: {'content-type': 'application/json'},
);

void main() {
  group('ApiClient.health', () {
    test('parses ok response', () async {
      final client = ApiClient(
        httpClient: MockClient((req) async {
          expect(req.url.path, '/health');
          expect(req.url.host, '127.0.0.1');
          expect(req.url.port, 18080);
          return _json({'status': 'ok', 'version': '0.4.2', 'so_loaded': true});
        }),
      );

      final health = await client.health();
      expect(health.status, 'ok');
      expect(health.version, '0.4.2');
      expect(health.soLoaded, isTrue);
    });

    test('throws ApiException on non-200 with server error text', () async {
      final client = ApiClient(
        httpClient: MockClient(
          (_) async => _json({'error': 'engine exploded'}, status: 500),
        ),
      );

      await expectLater(
        client.health(),
        throwsA(
          isA<ApiException>()
              .having((e) => e.message, 'message', 'engine exploded')
              .having((e) => e.statusCode, 'statusCode', 500),
        ),
      );
    });

    test(
      'throws ApiException with generic message when body is not JSON',
      () async {
        final client = ApiClient(
          httpClient: MockClient(
            (_) async => http.Response('upstream says no', 502),
          ),
        );

        await expectLater(
          client.health(),
          throwsA(
            isA<ApiException>().having((e) => e.statusCode, 'statusCode', 502),
          ),
        );
      },
    );

    test('times out and throws ApiException', () async {
      final client = ApiClient(
        httpClient: MockClient((_) async {
          await Future<void>.delayed(const Duration(milliseconds: 300));
          return _json({'status': 'ok'});
        }),
        healthTimeout: const Duration(milliseconds: 20),
      );

      await expectLater(
        client.health(),
        throwsA(
          isA<ApiException>().having(
            (e) => e.message,
            'message',
            contains('timed out'),
          ),
        ),
      );
    });

    test('connection failure becomes ApiException', () async {
      final client = ApiClient(
        httpClient: MockClient(
          (_) async => throw http.ClientException('Connection refused'),
        ),
      );

      await expectLater(
        client.health(),
        throwsA(
          isA<ApiException>().having(
            (e) => e.message,
            'message',
            contains('Connection refused'),
          ),
        ),
      );
    });
  });

  group('ApiClient.voicebanks', () {
    test('parses server entries into view models', () async {
      final client = ApiClient(
        httpClient: MockClient((req) async {
          expect(req.url.path, '/voicebanks');
          return _json({
            'voicebanks': [
              {
                'dir': 'mock-voicebank',
                'name': 'Teto Mock',
                'aliases_count': 19,
                'wav_count': 3,
                'samples_rate': 44100,
              },
              {
                'dir': 'library',
                'name': 'Teto English',
                'aliases_count': 120,
                'wav_count': 912,
                'samples_rate': null,
              },
            ],
          });
        }),
      );

      final banks = await client.voicebanks();
      expect(banks, hasLength(2));

      final mock = banks[0];
      expect(mock.name, 'Teto Mock');
      expect(mock.format, 'OpenUtau');
      expect(mock.status, 'ready');
      expect(mock.sizeMb, 3); // wav_count per CONTRACT-v1 §3
      expect(mock.singer, 'mock-voicebank'); // dir id used by /render

      final lib = banks[1];
      expect(lib.name, 'Teto English');
      expect(lib.sizeMb, 912);
    });

    test('throws ApiException on server error', () async {
      final client = ApiClient(
        httpClient: MockClient(
          (_) async =>
              _json({'error': 'voicebanks root not a directory'}, status: 500),
        ),
      );

      await expectLater(
        client.voicebanks(),
        throwsA(
          isA<ApiException>().having(
            (e) => e.message,
            'message',
            'voicebanks root not a directory',
          ),
        ),
      );
    });
  });

  group('ApiClient.render', () {
    test('POSTs project+voicebank and returns wav bytes', () async {
      final wavBytes = List<int>.filled(88200, 42);
      final client = ApiClient(
        httpClient: MockClient((req) async {
          expect(req.url.path, '/render');
          expect(req.method, 'POST');
          final body = jsonDecode(req.body) as Map<String, dynamic>;
          expect(body['project'], '/abs/demo-song.ustx');
          expect(body['voicebank'], 'library');
          return http.Response.bytes(
            wavBytes,
            200,
            headers: {'content-type': 'audio/wav'},
          );
        }),
      );

      final wav = await client.render(
        project: '/abs/demo-song.ustx',
        voicebank: 'library',
      );
      expect(wav, hasLength(88200));
      expect(wav.first, 42);
    });

    test('throws ApiException with server error text', () async {
      final client = ApiClient(
        httpClient: MockClient(
          (_) async => _json({'error': 'project /x not found'}, status: 404),
        ),
      );

      await expectLater(
        client.render(project: '/x', voicebank: 'library'),
        throwsA(
          isA<ApiException>()
              .having((e) => e.message, 'message', 'project /x not found')
              .having((e) => e.statusCode, 'statusCode', 404),
        ),
      );
    });
  });

  group('ApiClient.synthNote', () {
    test('POSTs phoneme params and returns wav bytes', () async {
      final client = ApiClient(
        httpClient: MockClient((req) async {
          expect(req.url.path, '/synth-note');
          final body = jsonDecode(req.body) as Map<String, dynamic>;
          expect(body['voicebank'], 'mock-voicebank');
          expect(body['phoneme'], 'A');
          expect(body['tone'], 60);
          expect(body['duration_ms'], 500.0);
          return http.Response.bytes(
            List<int>.filled(44100, 7),
            200,
            headers: {'content-type': 'audio/wav'},
          );
        }),
      );

      final wav = await client.synthNote(
        voicebank: 'mock-voicebank',
        phoneme: 'A',
        tone: 60,
        durationMs: 500,
      );
      expect(wav, hasLength(44100));
    });
  });

  group('ApiClient.baseUrl', () {
    test('is mutable and strips trailing slashes', () async {
      final client = ApiClient(baseUrl: 'http://127.0.0.1:18080/');
      expect(client.baseUrl, 'http://127.0.0.1:18080');

      String? seenHost;
      final withChange = ApiClient(
        baseUrl: 'http://127.0.0.1:18080/',
        httpClient: MockClient((req) async {
          seenHost = '${req.url.host}:${req.url.port}';
          return _json({'status': 'ok', 'version': 'v', 'so_loaded': false});
        }),
      );
      withChange.baseUrl = 'http://10.0.0.5:9999';
      await withChange.health();
      expect(seenHost, '10.0.0.5:9999');
    });
  });
}
