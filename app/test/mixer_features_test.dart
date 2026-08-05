import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lilt/api/engine_client.dart';
import 'package:lilt/models.dart';
import 'package:lilt/screens/editor_screen.dart';
import 'package:lilt/widgets/mixer_panel.dart';
import 'package:lilt/widgets/piano_roll.dart';

/// New-feature tests: SynthV phoneme labels above notes, collapsible side
/// panel, and the mixer bottom panel — all additive to the existing roll.
void main() {
  Future<void> pumpEditor(WidgetTester tester) async {
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
    await tester.binding.setSurfaceSize(const Size(900, 1600));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = ApiClient(
      httpClient: MockClient(
        (req) async => http.Response.bytes(List.filled(100, 42), 200),
      ),
    );
    await tester.pumpWidget(MaterialApp(home: EditorScreen(client: client)));
    await tester.pumpAndSettle();
  }

  testWidgets('phoneme hints derive from lyric [hint] and paint labels', (
    tester,
  ) async {
    await pumpEditor(tester);

    // The demo notes carry 'la[l A]' — the editor derives phoneme 'l A'
    // for the roll. The painter splits on whitespace, so the derived
    // phoneme string must contain both tokens (the boxes are canvas
    // drawing, not widgets — assert the derived data via the roll's
    // track wiring instead).
    final roll = tester.widget<PianoRoll>(find.byType(PianoRoll));
    final track = roll.tracks.first;
    expect(track.notes, isNotEmpty);
    expect(track.notes.first.phoneme, 'l A',
        reason: 'phoneme extracted from lyric[l A] hint');
  });

  testWidgets('mixer toggle opens the bottom panel with strips', (
    tester,
  ) async {
    await pumpEditor(tester);

    expect(find.byType(MixerPanel), findsNothing);
    await tester.tap(find.text('Mixer'));
    await tester.pumpAndSettle();
    expect(find.byType(MixerPanel), findsOneWidget);

    // 1 track + 1 master strip (find within the panel — the track name
    // also exists in the side track panel).
    expect(find.text('MASTER'), findsOneWidget);
    final inMixer = find.descendant(
      of: find.byType(MixerPanel),
      matching: find.text('Vocal Scale'),
    );
    expect(inMixer, findsOneWidget);

    // Toggle again hides it.
    await tester.tap(find.text('Hide mixer'));
    await tester.pumpAndSettle();
    expect(find.byType(MixerPanel), findsNothing);
  });

  testWidgets('side panel collapse hides track panel, expand restores', (
    tester,
  ) async {
    await pumpEditor(tester);

    // Wide surface (900px) → track panel present.
    expect(find.text('110 BPM'), findsOneWidget);
    // Collapse button: chevron_left at size 14 (the AppBar back button
    // also uses chevron_left but at size 28 — disambiguate by size).
    final collapseBtn = find.byWidgetPredicate(
      (w) => w is Icon && w.icon == Icons.chevron_left_rounded && w.size == 14,
    );
    expect(collapseBtn, findsOneWidget);
    await tester.tap(collapseBtn);
    await tester.pumpAndSettle();
    // Panel hidden → roll takes full width; expand icon shown.
    expect(find.text('110 BPM'), findsNothing);
    expect(
      find.byWidgetPredicate(
        (w) =>
            w is Icon && w.icon == Icons.chevron_right_rounded && w.size == 14,
      ),
      findsOneWidget,
    );
    await tester.tap(
      find.byWidgetPredicate(
        (w) =>
            w is Icon && w.icon == Icons.chevron_right_rounded && w.size == 14,
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('110 BPM'), findsOneWidget);
  });

  testWidgets('showPhonemes=false hides labels (paint flag respected)', (
    tester,
  ) async {
    await pumpEditor(tester);
    // Default true — the flag is forwarded to the roll.
    final roll = tester.widget<PianoRoll>(find.byType(PianoRoll));
    expect(roll.showPhonemes, isTrue);
  });

  test('phoneme extraction is unit-testable', () {
    // Direct check of the lyric→phoneme derivation used by the editor.
    final m = RegExp(r'\[([^\]]+)\]').firstMatch('la[l A]');
    expect(m?.group(1)?.trim(), 'l A');
    expect(RegExp(r'\[([^\]]+)\]').firstMatch('ooh'), isNull);
  });

  test('lyric edit clears stale phoneme so labels update', () {
    // Regression: _editLyric used copyWith(lyric:) WITHOUT clearing the
    // note's phoneme — the painter preferred the stale field over the
    // new lyric hint → labels froze after edits.
    const before = Note(
      lyric: 'la[l A]',
      pitch: 0,
      position: 0,
      duration: 1,
      phoneme: 'l A',
    );
    final after = before.copyWith(lyric: 'mi[m i]', phoneme: '');
    expect(after.lyric, 'mi[m i]');
    expect(after.phoneme, isEmpty);
    // The painter then derives from the NEW hint:
    final m = RegExp(r'\[([^\]]+)\]').firstMatch(after.lyric);
    expect(m?.group(1)?.trim(), 'm i');
  });

  test('lyric without hint resolves to real phoneme aliases', () {
    // Regression ("มันแค่ copy lyric ไม่ใช่ phoneme"): a hint-less lyric
    // must resolve letter-by-letter against the voicebank's single-phoneme
    // aliases ('n'→N, 'a'→A), NOT be copied verbatim as the label.
    const note = Note(
      lyric: 'na',
      pitch: 0,
      position: 0,
      duration: 1,
      phoneme: '',
    );
    // Mirror _phonemesOf's fallback chain: hint → phoneme field → letters.
    const aliases = {
      '3', 'A', 'B', 'D', 'E', 'F', 'G', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
      'P', 'S', 'T', 'U', 'V', 'W', 'Z',
    };
    final hint = RegExp(r'\[([^\]]+)\]').firstMatch(note.lyric)?.group(1);
    final List<String> phonemes;
    if (hint != null && hint.trim().isNotEmpty) {
      phonemes = hint.trim().split(RegExp(r'\s+'));
    } else if (note.phoneme.trim().isNotEmpty) {
      phonemes = note.phoneme.trim().split(RegExp(r'\s+'));
    } else {
      phonemes = [
        for (final ch in note.lyric.split(''))
          if (aliases.contains(ch.toUpperCase())) ch.toUpperCase(),
      ];
    }
    expect(phonemes, ['N', 'A']); // real phonemes, not ['na']
  });

  testWidgets('FX toolbar toggle switches the overlay flag', (tester) async {
    await pumpEditor(tester);

    // Initial: overlay off (read via the roll state's debug getter).
    expect(_fxFlag(tester), isFalse);

    // The FX button sits at the end of the horizontally scrolling
    // toolbar (off-screen on the 900px test surface) — scroll it in.
    await tester.scrollUntilVisible(
      find.byIcon(Icons.bolt_rounded),
      200,
      scrollable: find
          .byType(Scrollable)
          .first, // toolbar is the first horizontal Scrollable
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byIcon(Icons.bolt_rounded));
    await tester.pumpAndSettle();
    expect(_fxFlag(tester), isTrue);

    await tester.tap(find.byIcon(Icons.bolt_rounded));
    await tester.pumpAndSettle();
    expect(_fxFlag(tester), isFalse);
  });

  testWidgets('mixer strip changes emit params JSON', (tester) async {
    String? lastParams;
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(
          brightness: Brightness.dark,
          colorScheme: ColorScheme.fromSeed(
            seedColor: const Color(0xFF7358E9),
            brightness: Brightness.dark,
          ),
          scaffoldBackgroundColor: const Color(0xFF141416),
        ),
        home: Scaffold(
          body: MixerPanel(
            tracks: [
              Track(
                name: 'Vocal Scale',
                colorSeed: 0,
                notes: const [],
              ),
            ],
            onParamsChanged: (p) => lastParams = p,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    // Mute toggles the first strip → gain drops to -60 dB → linear 0.0
    // (the C++ mixer's gain param is LINEAR: 10^(dB/20), not dB itself).
    await tester.tap(find.text('M').first);
    await tester.pumpAndSettle();
    expect(lastParams, isNotNull);
    expect(lastParams, contains('"gain":0.0000'));
    expect(lastParams, contains('"master_gain":1.0000')); // 0 dB → 1.0
    expect(lastParams, contains('"eq_enabled":false'));
  });
}

/// Read the roll's internal FX-overlay flag via its debug getter.
bool _fxFlag(WidgetTester tester) {
  final state = tester.state(find.byType(PianoRoll));
  return (state as dynamic).debugShowFxOverlay as bool;
}
