import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lilt/api/engine_client.dart';
import 'package:lilt/main.dart';
import 'package:lilt/screens/editor_screen.dart';
import 'package:lilt/screens/settings_screen.dart';
import 'package:lilt/screens/voicebanks_screen.dart';

const _liveVoicebanks = {
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
      'samples_rate': 44100,
    },
  ],
};

/// Client whose /voicebanks returns [_liveVoicebanks]; every other
/// endpoint 404s unless overridden via [onRequest].
ApiClient liveClient({
  Future<http.Response> Function(http.Request)? onRequest,
}) {
  return ApiClient(
    httpClient: MockClient((req) async {
      if (req.url.path == '/voicebanks') {
        return http.Response(
          jsonEncode(_liveVoicebanks),
          200,
          headers: {'content-type': 'application/json'},
        );
      }
      if (onRequest != null) return onRequest(req);
      return http.Response(
        jsonEncode({'error': 'not found'}),
        404,
        headers: {'content-type': 'application/json'},
      );
    }),
  );
}

/// Client that simulates an unreachable engine server.
ApiClient offlineClient() {
  return ApiClient(
    httpClient: MockClient(
      (_) async => throw http.ClientException('Connection refused'),
    ),
  );
}

/// Let a snackbar's auto-dismiss timer fire so no timers leak at teardown.
Future<void> drainSnackBars(WidgetTester tester) async {
  await tester.pump(const Duration(seconds: 6));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('AppShell renders, loads live voicebanks, switches tabs', (
    tester,
  ) async {
    await tester.pumpWidget(LiltApp(client: liveClient()));
    await tester.pumpAndSettle();

    expect(find.text('Editor'), findsWidgets);
    expect(find.text('Voicebanks'), findsWidgets);
    expect(find.text('Settings'), findsWidgets);

    // Voicebanks tab: live banks from GET /voicebanks (no offline banner).
    await tester.tap(find.text('Voicebanks').first);
    await tester.pumpAndSettle();
    expect(find.text('Teto Mock'), findsOneWidget);
    expect(find.text('Teto English'), findsOneWidget);
    expect(find.text('Offline — showing mock data'), findsNothing);

    // Summary header derived from live data (3 + 912 wavs).
    expect(find.text('915 MB total'), findsOneWidget);

    // Detail sheet still works on a live bank.
    await tester.tap(find.text('Teto Mock'));
    await tester.pumpAndSettle();
    expect(find.text('Set as default'), findsOneWidget);
    // Dismiss the modal sheet via the barrier.
    await tester.tapAt(const Offset(400, 40));
    await tester.pumpAndSettle();
  });

  testWidgets('Voicebanks falls back to mock list + offline banner when '
      'server is unreachable', (tester) async {
    await tester.pumpWidget(LiltApp(client: offlineClient()));
    // Switch to the Voicebanks tab (its content is offstage in the
    // IndexedStack until then).
    await tester.tap(find.text('Voicebanks').first);
    // Bounded pumps, not pumpAndSettle: the mock fallback includes an
    // 'importing' bank whose progress bar animates indefinitely.
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));

    // Options never disappear: mock banks render…
    expect(find.text('Momo English'), findsOneWidget);
    expect(find.text('Hana JP'), findsOneWidget);
    // …with the offline banner + reason + retry.
    expect(find.text('Offline — showing mock data'), findsOneWidget);
    expect(find.text('Retry'), findsOneWidget);
  });

  testWidgets('Voicebanks screen shows spinner then live list', (tester) async {
    await tester.pumpWidget(
      MaterialApp(home: VoicebanksScreen(client: liveClient())),
    );
    expect(find.byType(CircularProgressIndicator), findsOneWidget);

    await tester.pumpAndSettle();
    expect(find.byType(CircularProgressIndicator), findsNothing);
    expect(find.text('Teto Mock'), findsOneWidget);
  });

  testWidgets('Editor export posts /render and shows size + duration', (
    tester,
  ) async {
    // The engine MethodChannel has no host in widget tests — mock it so
    // EnginePaths.saveWav resolves (null = "not on Android") instead of
    // hanging the platform channel.
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('com.lilt.lilt/engine'),
      (call) async => null,
    );
    addTearDown(() {
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        const MethodChannel('com.lilt.lilt/engine'),
        null,
      );
    });

    final wavBytes = List<int>.filled(88200, 42); // 1.0 s @ 44.1k 16-bit
    String? renderedProject;
    String? renderedVoicebank;
    final client = liveClient(
      onRequest: (req) async {
        if (req.url.path == '/render') {
          final body = jsonDecode(req.body) as Map<String, dynamic>;
          renderedProject = body['project'] as String?;
          renderedVoicebank = body['voicebank'] as String?;
          return http.Response.bytes(
            wavBytes,
            200,
            headers: {'content-type': 'audio/wav'},
          );
        }
        return http.Response(jsonEncode({'error': 'not found'}), 404);
      },
    );

    await tester.pumpWidget(MaterialApp(home: EditorScreen(client: client)));

    // Export lives in the ⋯ More menu (OM-style UI).
    await tester.tap(find.byTooltip('More'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('⬇  Export audio'));
    await tester.pumpAndSettle();
    expect(renderedProject, '/tmp/lilt-editor-song.ustx');
    expect(renderedVoicebank, kDemoRenderVoicebank);
    expect(find.textContaining('Rendered'), findsOneWidget);
    expect(find.textContaining('86 KB'), findsOneWidget);
    expect(find.textContaining('~1000 ms'), findsOneWidget);

    await drainSnackBars(tester);
  });

  testWidgets('Editor export shows server error message on failure', (
    tester,
  ) async {
    final client = liveClient(
      onRequest: (req) async {
        if (req.url.path == '/render') {
          return http.Response(
            jsonEncode({'error': 'project /x not found'}),
            404,
            headers: {'content-type': 'application/json'},
          );
        }
        return http.Response(jsonEncode({'error': 'not found'}), 404);
      },
    );

    await tester.pumpWidget(MaterialApp(home: EditorScreen(client: client)));

    // Export lives in the ⋯ More menu (OM-style UI).
    await tester.tap(find.byTooltip('More'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('⬇  Export audio'));
    await tester.pumpAndSettle();

    expect(find.text('Render failed: project /x not found'), findsOneWidget);

    await drainSnackBars(tester);
  });

  testWidgets('Settings test connection shows health result', (tester) async {
    final client = liveClient(
      onRequest: (req) async {
        if (req.url.path == '/health') {
          return http.Response(
            jsonEncode({'status': 'ok', 'version': '0.4.2', 'so_loaded': true}),
            200,
            headers: {'content-type': 'application/json'},
          );
        }
        return http.Response(jsonEncode({'error': 'not found'}), 404);
      },
    );

    await tester.pumpWidget(MaterialApp(home: SettingsScreen(client: client)));

    // Section titles are uppercased by the shared _section helper.
    expect(find.text('ENGINE CONNECTION'), findsOneWidget);
    expect(find.text('Not tested yet'), findsOneWidget);

    await tester.tap(find.text('Test connection'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Connected · engine v0.4.2'), findsOneWidget);
    expect(find.textContaining('renderer loaded'), findsOneWidget);
  });

  testWidgets('Settings test connection shows error when engine is down', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(home: SettingsScreen(client: offlineClient())),
    );

    await tester.tap(find.text('Test connection'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Cannot reach engine'), findsOneWidget);
  });
}
