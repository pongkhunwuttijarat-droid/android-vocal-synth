import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:rusted_utau/models.dart';
import 'package:rusted_utau/widgets/piano_roll.dart';

/// Regression: the playhead must start ONLY when the audio actually
/// starts. audioplayers takes ~100-500 ms to begin sound after play(),
/// so a playhead that animates immediately runs ahead of the audio,
/// finishes first and the parent's onPlayStopped cuts the tail off the
/// song ("delay ก่อนเริ่ม → เสียงโดนตัดท้าย").
void main() {
  testWidgets('playhead waits for onPlayRequested to complete', (tester) async {
    final playRequested = Completer<void>();
    var audioStarted = false;
    var playStopped = false;
    final roll = PianoRoll(
      tracks: [
        Track(
          name: 'T',
          colorSeed: 0,
          notes: [
            Note(lyric: 'la', pitch: 0, position: 0, duration: 0.5),
            Note(lyric: 'la', pitch: 2, position: 0.5, duration: 0.5),
          ],
        ),
      ],
      onPlayRequested: () {
        // Simulate audioplayers startup latency: the audio begins only
        // after the caller's await resolves.
        return playRequested.future.then((_) => audioStarted = true);
      },
      onPlayStopped: () => playStopped = true,
    );
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: roll)));
    await tester.pump();

    // Tap the roll's play control (first play button in the tree).
    final playBtn = find.byTooltip('Play');
    if (playBtn.evaluate().isEmpty) {
      // fall back to any play icon
      expect(find.byIcon(Icons.play_arrow_rounded), findsWidgets);
      await tester.tap(find.byIcon(Icons.play_arrow_rounded).first);
    } else {
      await tester.tap(playBtn);
    }
    await tester.pump(const Duration(milliseconds: 50));
    // Audio not started yet -> playhead must NOT be animating (the
    // play button must still show ▶, not ⏸ — the old fire-and-forget
    // code flipped to ⏸ immediately and ran the playhead ahead of the
    // audio, which cut the song tail off at the end).
    expect(audioStarted, isFalse, reason: 'play request still pending');
    expect(find.byIcon(Icons.pause_rounded), findsNothing,
        reason: 'playhead must wait for the audio to start');

    // Audio starts now.
    playRequested.complete();
    await tester.pump(const Duration(milliseconds: 50));
    expect(audioStarted, isTrue, reason: 'play request resolved');
    expect(find.byIcon(Icons.pause_rounded), findsOneWidget,
        reason: 'playhead animates only after audio start');
    expect(playStopped, isFalse);
  });
}
