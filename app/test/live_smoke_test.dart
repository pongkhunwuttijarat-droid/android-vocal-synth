@Tags(['live'])
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:lilt/api/engine_client.dart';
import 'package:lilt/screens/editor_screen.dart';

/// Live smoke test against a RUNNING synth-server (CONTRACT-v1).
///
/// Skipped by default. Start the server first:
///
/// ```sh
/// cd native && cargo run -p synth-server -- \
///   --so build/build-linux/libworldline.so --voicebanks test-data --port 18080
/// ```
///
/// then run:
///
/// ```sh
/// flutter test --tags live --dart-define=LIVE=1 test/live_smoke_test.dart
/// ```
const String _liveFlag = String.fromEnvironment('LIVE', defaultValue: '');
const bool _liveEnabled = _liveFlag == '1' || _liveFlag == 'true';

const String _engineUrl = String.fromEnvironment(
  'ENGINE_URL',
  defaultValue: 'http://127.0.0.1:18080',
);

const String _skipReason =
    'live smoke: pass --dart-define=LIVE=1 (and run synth-server on :18080)';

void main() {
  final client = ApiClient(baseUrl: _engineUrl);

  test(
    'live: GET /health reports ok + version',
    () async {
      final health = await client.health();
      expect(health.status, 'ok');
      expect(health.version, isNotEmpty);
      expect(
        health.soLoaded,
        isTrue,
        reason: 'server should be started with --so libworldline.so',
      );
      // ignore: avoid_print
      print(
        '[live] health: status=${health.status} '
        'version=${health.version} so_loaded=${health.soLoaded}',
      );
    },
    skip: _liveEnabled ? false : _skipReason,
    tags: ['live'],
  );

  test(
    'live: GET /voicebanks lists mock-voicebank',
    () async {
      final banks = await client.voicebanks();
      final mock = banks.where((b) => b.singer == 'mock-voicebank').toList();
      expect(
        mock,
        isNotEmpty,
        reason: 'server should be started with --voicebanks test-data',
      );
      expect(mock.first.name, 'Teto Mock');
      // ignore: avoid_print
      print('[live] voicebanks: ${banks.map((b) => b.name).join(', ')}');
    },
    skip: _liveEnabled ? false : _skipReason,
    tags: ['live'],
  );

  test(
    'live: POST /synth-note returns wav bytes',
    () async {
      final wav = await client.synthNote(
        voicebank: 'mock-voicebank',
        phoneme: 'A',
        tone: 60,
        durationMs: 500,
      );
      expect(wav.length, greaterThan(1000));
      expect(String.fromCharCodes(wav.take(4)), 'RIFF');
      // ignore: avoid_print
      print('[live] synth-note: ${wav.length} bytes');
    },
    skip: _liveEnabled ? false : _skipReason,
    tags: ['live'],
  );

  test(
    'live: POST /render returns wav bytes for the demo project',
    () async {
      final wav = await client.render(
        project: kDemoRenderProject,
        voicebank: kDemoRenderVoicebank,
      );
      expect(wav.length, greaterThan(1000));
      expect(String.fromCharCodes(wav.take(4)), 'RIFF');
      final durationMs = (wav.length / (2 * 44100) * 1000).round();
      // ignore: avoid_print
      print('[live] render: ${wav.length} bytes (~$durationMs ms)');
    },
    skip: _liveEnabled ? false : _skipReason,
    tags: ['live'],
  );
}
