import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:lilt/api/engine_client.dart';
import 'package:lilt/models.dart';
import 'package:lilt/screens/editor_screen.dart';
import 'package:lilt/widgets/piano_roll.dart';

/// Regression test for "_dependents.isEmpty is not true" when editing a
/// note's lyric: tap note → dialog opens → type → Save → dialog pops.
/// The framework assert fires in debug builds if the dialog route interferes
/// with the roll's inherited elements — this test reproduces the full flow
/// on the host so the stack trace is visible without a device.
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
    // Tall surface: the OM track panel (BPM chips + track list) stretches
    // the Row, pushing the roll's y-origin down — a 600px surface puts the
    // roll off-screen.
    await tester.binding.setSurfaceSize(const Size(900, 1600));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final client = ApiClient(
      httpClient: MockClient(
        (req) async => http.Response.bytes(List.filled(100, 42), 200),
      ),
    );
    await tester.pumpWidget(MaterialApp(home: EditorScreen(client: client)));
    await tester.pumpAndSettle();
    // Default tool is View (pan) — editing tests explicitly switch to
    // Select so note interactions behave like the UI's edit mode.
    await tester.tap(find.text('Select'));
    await tester.pumpAndSettle();
  }

  testWidgets('tap note → edit lyric → save (no _dependents assert)', (
    tester,
  ) async {
    await pumpEditor(tester);

    // Geometry: _ppb = 80 * 90/100 = 72 px/beat; keys column = 58px.
    // Track panel exists on wide test surface — find the roll's position
    // via its own bounds instead of assuming x=0.
    final rollFinder = find.byType(PianoRoll);
    final rollTopLeft = tester.getTopLeft(rollFinder);
    final roll = tester.widget<PianoRoll>(rollFinder);
    // Scroll the roll so the first note's row is visible: content is
    // 88*32=2816px, viewport ~450px, vScroll starts at 90. Compute the
    // jump from the note's own pitch so the row sits mid-viewport
    // (content y of the row center, minus ~250 to center it).
    final note = roll.tracks.first.notes.first; // 'la[l A]' beat 0.0 (C4)
    // The roll has two Scrollables (vertical content 2816px + horizontal);
    // pick the vertical one (maxScrollExtent > 1000).
    final vScroll = tester
        .stateList<ScrollableState>(find.byType(Scrollable))
        .firstWhere((s) => s.position.maxScrollExtent > 1000);
    // Roll pitches are relative to C4 (topPitch 48 = C8, note.pitch 0 =
    // C4): the note row sits at (48 - pitch) rows from the top.
    final rowCenterY = (48 - note.pitch) * 32.0 + 16.0;
    vScroll.position.jumpTo((rowCenterY - 250).clamp(0.0, 2364.0));
    await tester.pumpAndSettle();
    final x = rollTopLeft.dx + 58.0 + (note.position + 0.1) * 72.0;
    // Tap in VIEWPORT coordinates: roll top (56) + toolbar (48) + the
    // note's content y shifted by the scroll offset.
    final y = rollTopLeft.dy + 48.0 + (rowCenterY - vScroll.position.pixels);

    final gesture = await tester.startGesture(Offset(x, y));
    await tester.pump(const Duration(milliseconds: 50));
    await gesture.up();
    await tester.pumpAndSettle();
    // Lyric dialog should be open.
    expect(find.text('Edit lyric'), findsOneWidget);

    // Type a new lyric and save.
    await tester.enterText(find.byType(TextField), 'mi');
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    // Dialog closed, no exceptions thrown (tester fails on FlutterError).
    expect(find.text('Edit lyric'), findsNothing);
  });

  testWidgets('right-edge drag resizes note duration', (tester) async {
    await pumpEditor(tester);

    // Grab the right edge of the first note (beat 0.0, scale demo C4):
    // x: right edge = 58 + (0.0+duration)*72, minus 4 for the 8px grab zone.
    // y: row (48-pitch) center, shifted by the scroll offset.
    final rollFinder = find.byType(PianoRoll);
    final rollTopLeft = tester.getTopLeft(rollFinder);
    final vScroll = tester
        .stateList<ScrollableState>(find.byType(Scrollable))
        .firstWhere((s) => s.position.maxScrollExtent > 1000);
    final note = tester.widget<PianoRoll>(rollFinder).tracks.first.notes.first;
    // Roll pitches are relative to C4 (topPitch 48 = C8, note.pitch 0 =
    // C4): the note row sits at (48 - pitch) rows from the top.
    final rowCenterY = (48 - note.pitch) * 32.0 + 16.0;
    vScroll.position.jumpTo((rowCenterY - 250).clamp(0.0, 2364.0));
    await tester.pumpAndSettle();
    final start = Offset(
      rollTopLeft.dx + 58.0 + (0.0 + note.duration) * 72.0 - 4.0,
      rollTopLeft.dy + 48.0 + (rowCenterY - vScroll.position.pixels),
    );
    final gesture = await tester.startGesture(start);
    await tester.pump(const Duration(milliseconds: 50));
    // Drag right by 1 beat (72px).
    await gesture.moveBy(const Offset(72, 0));
    await tester.pump(const Duration(milliseconds: 50));
    await gesture.up();
    await tester.pumpAndSettle();

    // No dialog should have opened (it was a drag, not a tap).
    expect(find.text('Edit lyric'), findsNothing);
    // The roll edits its INTERNAL _notes copy (widget.tracks is immutable);
    // read the live notes via the state's @visibleForTesting hook.
    final state = tester.state(find.byType(PianoRoll)) as dynamic;
    final notes = state.debugNotes as List<Note>;
    // The scale demo starts with a 0.5-beat note — after +1.0 drag = 1.5.
    final edited = notes.firstWhere((n) => n.position == 0.0);
    expect(edited.duration, 1.5);
  });
}
