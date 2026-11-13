# R2 first-PCM prototype

This isolated POC tests live producer/consumer overlap for the R2 path. Rust
prepares ordered phoneme batches and publishes each stable feature watermark;
the Python process consumes each watermark through persistent stateful ONNX
mel/vocoder sessions while Rust continues preparing later batches. The PCM
consumer checks ordered sample coverage and simulates playback deadlines. It
does not change the product renderer or checked-in ONNX models.

## Run

Build the Rust producer first, then use `run_live_onnx.py` with ONNX Runtime
CUDA, the model files, a project, a voicebank, and `libworldline.so`. Sessions
load and warm before its Play/TTFS clock. The runner writes feature watermarks,
streamed PCM and a whole-phrase reference under the selected output directory.
It fails instead of silently measuring CPU if CUDA is unavailable.

## Live result (2026-10-17)

Fixture: `test/golden/scale-demo-app.ustx`, track 0, 14.512 s; 58 Rust render
requests / 1,250 R2 frames; RTX 5060 Ti with CUDA EP; two concurrent Rust
preparation workers and one ONNX consumer.

- Session creation: 187.85 ms; first-call warm-up: 313.41 ms, both before Play.
- Play to first PCM: **241.76 ms**. Rust published the first eight requests
  after 212.35 ms, covering 175 feature frames; the first delivered PCM block
  contains 32,768 samples (0.743 s).
- Rust continued publishing seven later feature watermarks while ONNX consumed
  them. The last feature batch arrived at 1,609.94 ms; all 58 requests produced
  exactly 640,000 PCM samples in 20 ordered blocks.
- Playback deadline simulation: **0 late blocks**. Full streamed output vs.
  one whole-phrase ONNX inference: correlation **0.999999248**, RMSE
  **0.00037256**, max absolute difference **0.00368743**.
- Artifacts: `first_pcm_block.wav`, `streamed_pcm.wav`,
  `whole_reference_pcm.wav`.

A shorter-context bootstrap experiment reached **99.15 ms** in one run
(another run measured 100.85 ms), but whole-stream correlation fell to
**0.9808** (RMSE 0.0599, max error 1.57). It fails the user's 0.99 quality
floor and is rejected. The quality-preserving 8-request setup is the accepted
result, but it does **not** meet the 100 ms target. Further work must reduce
the first stable feature lookahead or move preparation off the critical path
without reducing model context.

These measurements count delivery to an in-memory PCM consumer and simulated
real-time deadlines, not sound from a physical output device. The CUDA session
warm-up is startup cost, not Play-to-first-PCM latency; a cold runtime that
does not initialize/warm sessions before Play will have a higher TTFS.

## OpenUtau comparison and product boundary

The inspected OpenUtau R2 path prepares the whole phrase, runs full-phrase mel
and vocoder inference, copies the complete output, then trims/fades and caches
the WAV. It has no incremental PCM callback on this path. Its earlier headless
whole-render measurement was 4.519 s, but it used 29 phones while this current
Rust fixture path produced 58 requests; that timing is not an apples-to-apples
sonic or TTFS comparison. The streamed PCM above is compared with the same
Rust features and the same ONNX models, not with OpenUtau audio.

The POC keeps later preparation and model inference overlapping, but production
`R2Engine` still returns one completed `Vec<f32>`. Product integration, a real
audio-device callback measurement, a matched OpenUtau fixture, and performance
on weaker GPUs/CPU remain unverified.

## Local-edit cache probe

`cache_edit_probe.py` changes 16 voiced F0 frames in the extracted real phrase
and compares a naive content key (model identity + block-local feature window)
with actual full-model PCM after the edit. Four positions were tried in the
14.512 s phrase using 64-frame / 0.743 s PCM blocks. Warmed full mel+vocoder
inference took about 174 ms on the RTX 5060 Ti once features were available.

The edit changed only one short F0 region, but cumulative model phase made PCM
differ through the remaining suffix: edits at 1.486, 3.715, 7.430, and 12.632 s
changed 19, 16, 13, and 4 blocks respectively. The prefix before the edit and
its context stayed bit-identical. A local-input-only key incorrectly called
1–16 later blocks cache hits even though their PCM differed. Exact PCM cache
identity must include the incoming mel/vocoder phase checkpoint, or
conservatively invalidate the affected phrase suffix. Unchanged phrases and
tracks remain independent reuse candidates. This is a feature-level F0 probe,
not a project note-edit test; duration-changing edits were not modeled. For the
3.715 s edit, retaining the four unchanged prefix blocks and replacing the
remaining suffix reconstructed the edited full output exactly (`max_abs=0`);
the probe still ran full inference, so compute savings from partial inference
have not yet been measured.

The report at `cache-edit-report.json` records each block's local-key hit,
correlation, RMSE, and peak difference. Reproduce with
`cache_edit_probe.py --features <final-feature-snapshot> --models models/r2
--out <output-directory> --edit-frame <frame>` in the same CUDA/ONNX Runtime
environment as the live run.

## Independent OpenUtau WORLDLINE-R2 project-edit A/B

To distinguish Rusted UTAU's R2 feature-level behavior from the reference engine, an
actual connected 159-note Japanese phrase was extracted from the supplied
`.rustx` project's USTX and rendered twice by the headless OpenUtau golden
renderer using WORLDLINE-R2 and the Japanese Teto bank. Only its first note
changed (tone 68 → 69, duration 160 ticks); all other note events stayed fixed.
The user's project was not edited.

Both outputs are 13.8623 s, 44.1 kHz mono PCM16. Repeating the unchanged
OpenUtau render produced byte-identical PCM. Edited-vs-unedited correlation is
0.1674, RMSE 6,831.8 PCM16 units, and 610,838/611,328 samples differ, beginning
at 1.18 ms and continuing through the phrase end. Every 100 ms bin differs.
Thus an edit to the first note can alter later PCM within this OpenUtau
WORLDLINE-R2 phrase even when later note events and duration are unchanged.
This is exact-waveform evidence, not an audibility judgment or proof that every
edit invalidates every later block. It also does not compare against Rusted UTAU's
engine. Phrase/track jobs outside the edited synthesis phrase remain
independent candidates for reuse.

WAVs and reproduction details are recorded in
`docs/work/tasks/T050-incremental-preparation-streaming.md` under “Independent
OpenUtau edit A/B”.
