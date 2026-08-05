import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:lilt/models.dart';
import 'package:lilt/widgets/piano_roll.dart';

void main() {
  testWidgets('curve pencil snaps beats to the capture grid (no duplicate '
      'points on redraw)', (tester) async {
    // A single long note; the roll maps 1 beat → _basePxPerBeat (80px at
    // 100% zoom). Draw over the same span twice — snapped beats must
    // UPSERT the same points, so the count stays bounded (not 2×).
    final track = Track(
      name: 'Vocal',
      colorSeed: 0,
      notes: [
        Note(
          lyric: 'la[l A]',
          pitch: 0,
          position: 0,
          duration: 4,
        ),
      ],
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(
          brightness: Brightness.dark,
          colorScheme: ColorScheme.fromSeed(
            seedColor: const Color(0xFF7358E9),
            brightness: Brightness.dark,
          ),
        ),
        home: Scaffold(
          body: PianoRoll(tracks: [track]),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final roll = tester.state(find.byType(PianoRoll));
    final State<StatefulWidget> state = roll as State<StatefulWidget>;
    // Access the private debug getters via dynamic (mirrors existing tests).
    final dynamic dyn = state;

    // Pitch-draw mode: tap the 'Pitch' tool (gesture icon → sets
    // _tool=pencil; 'Draw' is the NOTE drawing tool — wrong button) AND
    // the Pitch visibility toggle (waves icon; the pencil only writes
    // when _showPitch is on, otherwise gestures drag notes).
    await tester.tap(find.text('Pitch').first);
    await tester.pumpAndSettle();
    await tester.tap(find.byIcon(Icons.waves_rounded).first);
    await tester.pumpAndSettle();

    // The roll canvas occupies most of the screen; drag across beats
    // 0.5..3.5 (x from 60px to 300px at 80px/beat). Draw HIGH on the roll
    // (well above the note at pitch 0 — the note spans the middle rows),
    // so the gesture is a curve draw and never a note drag.
    const step = 0.125; // Normal grid (1/8 beat)
    final before = List<PitchPoint>.of(dyn.debugCurvePoints as List<PitchPoint>);
    final origin = tester.getTopLeft(find.byType(PianoRoll));
    for (final x in [60.0, 140.0, 220.0, 300.0]) {
      final pos = origin + Offset(x, 80);
      final gesture = await tester.startGesture(pos);
      await gesture.moveBy(const Offset(0, -30));
      await gesture.up();
      await tester.pump();
    }
    final after = List<PitchPoint>.of(dyn.debugCurvePoints as List<PitchPoint>);
    // Note anchors sit at each note's LIVE center (position + duration/2)
    // and are NOT on the capture grid by design — the pencil gesture may
    // have moved the note (anchor follows). Filter by the ACTUAL note
    // centers, then verify every remaining PENCIL point is snapped.
    final liveNotes = dyn.debugNotes as List<Note>;
    final centers = [
      for (final n in liveNotes) n.position + n.duration / 2,
    ];
    final pencilPoints = [
      for (final p in after)
        if (!centers.any((c) => (p.beat - c).abs() < 0.05)) p,
    ];
    expect(pencilPoints, isNotEmpty, reason: 'pencil should add points');
    for (final p in pencilPoints) {
      final snapped = (p.beat / step).round() * step;
      expect((p.beat - snapped).abs() < 1e-9, isTrue,
          reason: 'beat ${p.beat} not on ${step} grid');
    }
    // Re-drawing the same span must not add points beyond the grid size.
    for (final x in [60.0, 140.0, 220.0, 300.0]) {
      final pos = origin + Offset(x, 90);
      final gesture = await tester.startGesture(pos);
      await gesture.moveBy(const Offset(0, -30));
      await gesture.up();
      await tester.pump();
    }
    final finalCount = dyn.debugCurvePoints.length as int;
    expect(finalCount, lessThanOrEqualTo(after.length + 4),
        reason: 'redraw over the same span should upsert, not pile up');
    expect(before, isNotEmpty); // sanity: curve exists
  });
}
