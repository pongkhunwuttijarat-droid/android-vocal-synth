using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading;
using System.Collections.Concurrent;
using System.Threading.Tasks;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core;
using OpenUtau.Core.DiffSinger;
using OpenUtau.Core.Format;
using OpenUtau.Core.Render;
using OpenUtau.Core.SignalChain;
using OpenUtau.Core.Ustx;
using OpenUtau.Core.Util;

namespace GoldenRenderer {

    // Headless golden-reference renderer: renders a .ustx project (or a
    // programmatic single note) with the Teto voicebank using OpenUtau's
    // real RenderEngine, and writes 16-bit PCM mono 44100 Hz wav.
    class Program {
        /// TaskScheduler that runs every work item on one dedicated thread, in
        /// submission order. Used as OpenUtau's "UI thread" so the async
        /// phonemizer's re-Validate cannot race the harness's own Validate —
        /// see the DocManager.Inst.Initialize comment below.
        class SingleThreadTaskScheduler : TaskScheduler {
            readonly BlockingCollection<Action> queue = new BlockingCollection<Action>();
            readonly Thread thread;

            public SingleThreadTaskScheduler() {
                thread = new Thread(() => {
                    foreach (var action in queue.GetConsumingEnumerable()) {
                        try { action(); } catch (Exception e) {
                            Console.Error.WriteLine("UI action error: " + e);
                        }
                    }
                }) { IsBackground = true, Name = "OpenUtauUI" };
                thread.Start();
            }

            /// Run `action` on this scheduler's thread, in submission order.
            public void Post(Action action) {
                queue.Add(action);
            }

            protected override IEnumerable<Task> GetScheduledTasks() => Enumerable.Empty<Task>();

            protected override void QueueTask(Task task) {
                queue.Add(() => {
                    try { TryExecuteTask(task); } catch (Exception e) {
                        Console.Error.WriteLine("UI task error: " + e);
                    }
                });
            }

            protected override bool TryExecuteTaskInline(Task task, bool taskWasPreviouslyQueued) => false;
        }
        const string TETO_DIR_DEFAULT = "/home/seal/project/android-voice-synth/test/golden/teto-english";
        // Overridable so diagnostics can ablate voicebank files (e.g. .frq).
        static string TetoDir => Environment.GetEnvironmentVariable("TETO_DIR") ?? TETO_DIR_DEFAULT;

        static int Main(string[] args) {
            if (args.Length < 2) {
                Console.Error.WriteLine("usage: golden-renderer demo <out.wav> [ustx]");
                Console.Error.WriteLine("       golden-renderer note <out.wav> <lyric> <tone> <durTicks>");
                Console.Error.WriteLine("       golden-renderer phrase <out.wav> [ustx]  (apple-to-apple mono)");
                return 2;
            }
            // Keep every OpenUtau data/cache dir inside /tmp so the renderer
            // never touches the user's real home directories.
            string dataHome = Path.Combine(Path.GetTempPath(), "golden-renderer-data");
            string cacheHome = Path.Combine(Path.GetTempPath(), "golden-renderer-cache");
            Directory.CreateDirectory(dataHome);
            Directory.CreateDirectory(cacheHome);
            Environment.SetEnvironmentVariable("XDG_DATA_HOME", dataHome);
            Environment.SetEnvironmentVariable("XDG_CACHE_HOME", cacheHome);
            Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);

            // Headless DocManager: ALL project work must land on ONE thread.
            //
            // OpenUtau's desktop app gets that for free — the Avalonia UI thread
            // runs UProject.Validate, and the async phonemizer's response is
            // posted back to that same thread (PhonemizerRunner.SendResponse
            // schedules `Project.Validate` on the *main* scheduler handed to
            // DocManager.Initialize). The earlier harness gave the phonemizer
            // the main thread's scheduler while running its own ValidateFull on a
            // separate pump thread, so on a multi-track project two threads were
            // inside UPart.Validate at once, both rebuilding `phonemes`, and it
            // died with "Collection was modified; enumeration operation may not
            // execute." (UPart.cs:277). Single-track runs never overlapped, so
            // the bug stayed hidden.
            //
            // A single-concurrency scheduler is the fix: PostOnUIThread and the
            // phonemizer's main scheduler are both that scheduler, so every
            // Validate/notification is serialized no matter who posts it.
            var uiScheduler = new SingleThreadTaskScheduler();
            DocManager.Inst.PostOnUIThread = action =>
                uiScheduler.Post(action);
            DocManager.Inst.Initialize(Thread.CurrentThread, uiScheduler);
            ToolsManager.Inst.Initialize();

            // Run `action` on the UI pump thread and wait for it.
            //
            // ValidateFull has to happen there, not on the main thread: the
            // phonemizer is async and applies its response by posting to the UI
            // thread, and UPart.Validate enumerates `phonemes` while walking
            // notes. Once more than one track phonemizes at the same time, doing
            // that from two threads crashes with
            // "Collection was modified; enumeration operation may not execute."
            // (UPart.cs:277) — which is why the earlier single-track runs never
            // hit it. The desktop app gets this for free because Validate and the
            // posted responses share the one UI thread.
            System.Action<System.Action> runOnUi = body => {
                var done = new ManualResetEventSlim(false);
                Exception failure = null;
                uiScheduler.Post(() => {
                    try { body(); } catch (Exception e) { failure = e; } finally { done.Set(); }
                });
                if (!done.Wait(300000)) {
                    throw new TimeoutException("UI-thread action timed out after 300s");
                }
                // Rethrow through ExceptionDispatchInfo so the original stack
                // (which thread, which UPart.Validate frame) survives — a plain
                // `throw failure` resets it to the rethrow site and hides the
                // race we are trying to diagnose.
                if (failure != null) {
                    System.Runtime.ExceptionServices.ExceptionDispatchInfo
                        .Capture(failure).Throw();
                }
            };

            // --- Load the Teto voicebank -----------------------------------
            // DIFFSINGER_SINGER=<dir> switches to the C# DiffSinger
            // golden path (the kiritan bank adapted for the ref's
            // phonemizer infra — see test/golden/diffsinger-golden-bank).
            var dsSingerDir = Environment.GetEnvironmentVariable("DIFFSINGER_SINGER");
            var loader = new VoicebankLoader(dsSingerDir ?? TetoDir);
            var voicebank = loader.SearchAll().FirstOrDefault();
            if (voicebank == null) {
                Console.Error.WriteLine("ERROR: no voicebank found under " + (dsSingerDir ?? TetoDir));
                return 2;
            }
            USinger singer;
            bool diffSingerMode = !string.IsNullOrEmpty(dsSingerDir);
            if (diffSingerMode) {
                singer = new DiffSingerSinger(voicebank);
                singer.EnsureLoaded();
                if (!singer.Loaded) {
                    Console.Error.WriteLine("ERROR: DiffSinger singer failed to load (Loaded=false)");
                    return 2;
                }
                Console.WriteLine($"Singer: {singer.Name} id={singer.Id} type={singer.SingerType} (DiffSinger)");
            } else {
                singer = new ClassicSinger(voicebank);
                singer.EnsureLoaded();
                if (!singer.Loaded) {
                    Console.Error.WriteLine("ERROR: Teto singer failed to load (Loaded=false)");
                    return 2;
                }
                Console.WriteLine($"Singer: {singer.Name} id={singer.Id} type={singer.SingerType}");
            }
            // Register so track.Validate keeps it (not CreateMissing).
            SingerManager.Inst.Singers[singer.Id] = singer;

            // --- Build the project ------------------------------------------
            UProject project;
            if (args[0] == "demo" || args[0] == "phrase") {
                string ustx = args.Length > 2 ? args[2]
                    : "/home/seal/project/android-voice-synth/native/tools/synth-cli/tests/data/demo-song.ustx";
                project = Ustx.Load(ustx);
                Console.WriteLine($"Loaded {ustx}: {project.tracks.Count} tracks, {project.parts.Count} parts");
            } else if (args[0] == "note") {
                string lyric = args[2];
                int tone = int.Parse(args[3]);
                int durTicks = int.Parse(args[4]);
                project = Ustx.Create();
                var track = new UTrack() { TrackName = "Track1", Singer = singer };
                project.tracks.Add(track);
                var part = new UVoicePart() { name = "Part1", trackNo = 0, position = 0 };
                var note = UNote.Create();
                note.position = 0;
                note.duration = durTicks;
                note.tone = tone;
                note.lyric = lyric;
                // Validate() indexes pitch.data[0] when snapFirst is true —
                // give it the standard two default points like a loaded ustx.
                note.pitch = new UPitch {
                    snapFirst = true,
                    data = new List<PitchPoint> {
                        new PitchPoint(-1, 0),
                        new PitchPoint(1, 0),
                    },
                };
                part.notes.Add(note);
                project.parts.Add(part);
                part.AfterLoad(project, track);
                Console.WriteLine($"Single note: lyric={lyric} tone={tone} dur={durTicks} ticks");
            } else {
                Console.Error.WriteLine("unknown mode: " + args[0]);
                return 2;
            }

            // Set the singer on EVERY track that declares one, and let the whole
            // project re-validate (this sets the default renderer =
            // WORLDLINE-R for classic singers and triggers phonemization).
            //
            // Only track 0 used to be set. On a multi-track project the other
            // tracks resolve their Singer by NAME through SingerManager, and
            // the harness registers the bank under the name the loader reported
            // (which carries the `（かさねてと）` infix this project's
            // `singer:` field does not have). Those tracks therefore ended up
            // with SingerManager.CreateMissing → `Found == false`, so
            // UPhoneme.ValidateOto marked EVERY phoneme
            // Error = "Singer is not loaded." and RenderPhrase.FromPart —
            // which filters on `!phoneme.Error` — returned zero phrases. Their
            // per-track export was then a 0-frame WAV.
            foreach (var track in project.tracks) {
                if (string.IsNullOrEmpty(track.singer)) continue;
                track.Singer = singer;
            }
            project.tracks[0].Singer = singer;
            if (diffSingerMode) {
                // The track's phonemizer must match the bank: the singer's
                // declared phonemizer (character.yaml) — the C# does not
                // auto-assign it on Validate, so set it explicitly.
                var dsFactory = PhonemizerFactory.Get(typeof(DiffSingerChinesePhonemizer));
                project.tracks[0].Phonemizer = dsFactory.Create();
                Console.WriteLine($"  track phonemizer -> {dsFactory.type.FullName}");
            }
            DocManager.Inst.ExecuteCmd(new LoadProjectNotification(project));

            // The DiffSinger phonemizer caches its encoder outputs as a
            // tensorcache under the project's cacheDir — ensure it exists
            // BEFORE ValidateFull triggers the phonemizer.
            // NB: cacheDir may legitimately be empty (projects that ship
            // `cache_dir: ''`), which throws for Directory.CreateDirectory —
            // fall back to the harness default (/tmp/golden-renderer-cache).
            var cacheDir = string.IsNullOrEmpty(project.cacheDir) ? "/tmp/golden-renderer-cache" : project.cacheDir;
            Directory.CreateDirectory(cacheDir);
            Directory.CreateDirectory(Path.Combine(cacheDir, "OpenUtau"));
            // The harness's default cache (when cache_dir is empty) is
            // /tmp/golden-renderer-cache — cover both.
            Directory.CreateDirectory("/tmp/golden-renderer-cache");
            Directory.CreateDirectory("/tmp/golden-renderer-cache/OpenUtau");

            // --- T17: make DEMO mode honour WORLDLINE_RENDERER ----------------
            // Every RenderPhrase bakes `track.RendererSettings.Renderer` when it
            // is constructed (ref-openutau-clean .../Render/RenderPhrase.cs:222),
            // and that renderer is chosen by URenderSettings.Validate() inside
            // the ValidateFull() below. Without this override RenderMixdown
            // always used the track default (v1), so `WORLDLINE_RENDERER=2
            // ./run.sh demo` wrote a byte-identical v1 file — the "golden r2"
            // demo files were really r1 (found in T16). `phrase` mode already
            // honoured the env (RenderPhrasesDirect); this makes the two agree.
            if (!diffSingerMode) {
                string rendererName = RendererVersion() == 2 ? Renderers.WORLDLINE_R2 : Renderers.WORLDLINE_R;
                foreach (var track in project.tracks) {
                    track.RendererSettings.renderer = rendererName;
                    // force URenderSettings.Validate() to rebuild from the name
                    track.RendererSettings.Renderer = null;
                }
                Console.WriteLine($"  renderer override -> {rendererName} (WORLDLINE_RENDERER)");
            }

            runOnUi(() => project.ValidateFull());

            // --- Wait for the async phonemizer ------------------------------
            var part0 = project.parts.OfType<UVoicePart>().FirstOrDefault();
            if (part0 == null) {
                Console.Error.WriteLine("ERROR: no voice part in project");
                return 2;
            }
            if (diffSingerMode) {
                Console.WriteLine($"  track phonemizer: {project.tracks[0].Phonemizer?.GetType().FullName} (declared: {project.tracks[0].Singer?.DefaultPhonemizer})");
                Console.WriteLine($"  part notes: {part0.notes.Count}, first lyric: {part0.notes.First().lyric}");
            }
            int waitedMs = 0;
            // Wait for EVERY voice part, not just the first one. The
            // phonemizer is async and per-part: with a multi-track project
            // only part0 being up to date left tracks 1..3 with zero render
            // phrases, so both the mixdown and every per-track export came
            // out containing track 0 alone (the per-track WAVs were even
            // 0 frames long). Wait for the slowest part, then report each.
            while (waitedMs < 60000) {
                var pending = project.parts.OfType<UVoicePart>()
                    .Where(p => !p.PhonemesUpToDate).ToList();
                if (pending.Count == 0) break;
                Thread.Sleep(200);
                waitedMs += 200;
            }
            foreach (var vp in project.parts.OfType<UVoicePart>()) {
                Console.WriteLine($"  part '{vp.name}' track={vp.trackNo}: notes={vp.notes.Count} " +
                    $"phonemes={(vp.phonemes?.Count ?? -1)} upToDate={vp.PhonemesUpToDate} " +
                    $"renderPhrases={vp.renderPhrases?.Count ?? -1}");
            }
            var stale = project.parts.OfType<UVoicePart>().Where(p => !p.PhonemesUpToDate).ToList();
            if (stale.Count > 0) {
                Console.Error.WriteLine("ERROR: phonemizer did not finish in 60s for: " +
                    string.Join(", ", stale.Select(p => $"'{p.name}'(track {p.trackNo})")));
                return 2;
            }
            // The render phrases are built during Validate, but ONLY IF the
            // phonemes are already up to date at that moment
            // (UPart.cs: `if (PhonemesUpToDate) renderPhrases.AddRange(...)`).
            // The phonemizer is async per part, so on a multi-track project the
            // later parts finish after Validate and are left with zero phrases —
            // their per-track export then came out 0 frames long. Now that every
            // part is up to date, rebuild the phrases for every part that has
            // none. This is the same call Validate makes, not a reimplementation.
            foreach (var vp in project.parts.OfType<UVoicePart>()) {
                if (vp.renderPhrases.Count > 0) continue;
                try {
                    vp.renderPhrases.AddRange(
                        RenderPhrase.FromPart(project, project.tracks[vp.trackNo], vp));
                    Console.WriteLine($"  rebuilt phrases for '{vp.name}' (track {vp.trackNo}): {vp.renderPhrases.Count}");
                } catch (Exception re) {
                    Console.WriteLine($"  FromPart threw for '{vp.name}': {re.Message}");
                    Console.WriteLine($"  FromPart stack: {re.StackTrace}");
                }
            }
            if (diffSingerMode) {
                var phonemeSnapshot = part0.phonemes.ToList();
                Console.WriteLine($"  after re-validate: phonemes={phonemeSnapshot.Count} renderPhrases={part0.renderPhrases?.Count ?? -1}");
                Console.WriteLine($"  phoneme flags: {string.Join("; ", phonemeSnapshot.Select(p => $"{p.phoneme}@{p.position}:err={p.Error}"))}");
                var firstErr = phonemeSnapshot.FirstOrDefault(p => p.ErrorException != null);
                if (firstErr != null) {
                    Console.WriteLine($"  phoneme error: {firstErr.ErrorException.Message}");
                    Console.WriteLine($"  phoneme error inner: {firstErr.ErrorException.InnerException?.Message}");
                }
                // Reflection: read the phonemizer's private unrecognized
                // lyrics map + the last ProcessPart exception (we must NOT
                // modify the reference repo).
                var ph = project.tracks[0].Phonemizer;
                var t = ph.GetType();
                while (t != null && t.GetField("unrecognizedLyrics", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance) == null) {
                    t = t.BaseType;
                }
                var field = t?.GetField("unrecognizedLyrics", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                if (field?.GetValue(ph) is Dictionary<int, string> unrecognized) {
                    Console.WriteLine($"  unrecognized lyrics: {string.Join("; ", unrecognized.Select(kv => $"{kv.Key}={kv.Value}"))}");
                }
                var exField = ph.GetType().GetField("lastProcessPartException", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)
                    ?? ph.GetType().BaseType?.GetField("lastProcessPartException", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);
                var procEx = exField?.GetValue(ph) as Exception;
                if (procEx != null) {
                    Console.WriteLine($"  ProcessPart exception: {procEx.Message}");
                } else {
                    Console.WriteLine($"  ProcessPart exception: none");
                }
            }
            var phrases = part0.GetRenderRequest();
            Console.WriteLine($"Phonemes ready after {waitedMs} ms; phrases: {phrases?.phrases?.Length ?? 0}");
            if (diffSingerMode) {
                foreach (var phrase in phrases.phrases) {
                    var ends = string.Join(",", phrase.phones.Select(p => $"{p.phoneme}@{p.endMs:F1}"));
                    Console.WriteLine($"  phrase ends: {ends}");
                }
                Console.WriteLine($"  part phonemes: {part0.phonemes?.Count ?? -1} (up to date: {part0.PhonemesUpToDate})");
            }
            if (phrases == null || phrases.phrases.Length == 0) {
                Console.Error.WriteLine("ERROR: no render phrases (check lyrics/aliases exist in oto.ini)");
                return 2;
            }
            Console.WriteLine($"Renderer: {phrases.phrases[0].renderer}");

            // --- Apple-to-apple mode: direct phrase render (mono) ----------
            // Mirrors our Rust engine's PhraseSynth::Synth() — the defensible
            // golden comparison target (Sprint 2.3.4).
            if (args[0] == "phrase") {
                return RenderPhrasesDirect(args[1], project, phrases.phrases, diffSingerMode);
            }

            // --- Print timing for golden investigation ----------------------
            foreach (var phrase in phrases.phrases) {
                Console.WriteLine($"  phrase: posMs={phrase.positionMs:F1} leadingMs={phrase.leadingMs:F1} durMs={phrase.durationMs:F1} phones={string.Join(" ", phrase.phones.Select(p => p.phoneme))}");
                foreach (var phone in phrase.phones) {
                    Console.WriteLine($"    phone {phone.phoneme}: posMs={phone.positionMs:F1} leadingMs={phone.leadingMs:F1} durMs={phone.durationMs:F1} env=[{string.Join(", ", phone.envelope.Select(p => $"{p.X:F0}:{p.Y:F2}"))}]");
                }
            }

            // --- Render -------------------------------------------------------
            var engine = new RenderEngine(project);
            var cts = new CancellationTokenSource();
            Console.WriteLine("Rendering...");
            var (mix, _) = engine.RenderMixdown(TaskScheduler.Current, ref cts, wait: true, applyMixFx: false);

            // --- Per-track export (OpenUtau's File > Export wav) -------------
            // `mix` above is the whole-project sum, so it cannot be compared
            // against a single-track render: every note of every other track is
            // mixed into it. `RenderTracks` is what PlaybackManager.RenderToFiles
            // uses for the desktop per-track export, and it keeps one WaveMix per
            // track — that is the apples-to-apples reference for our engine's
            // `render --track N`. Written to <out>.trackNN.wav next to the
            // mixdown so one run produces both.
            var trackMixes = engine.RenderTracks(TaskScheduler.Current, ref cts);
            string outDir = Path.GetDirectoryName(Path.GetFullPath(args[1]));
            string outStem = Path.GetFileNameWithoutExtension(args[1]);
            for (int i = 0; i < trackMixes.Count; i++) {
                if (trackMixes[i] == null) {
                    Console.WriteLine($"  track {i}: no render request — skipped");
                    continue;
                }
                string trackPath = Path.Combine(outDir, $"{outStem}.track{i:D2}.wav");
                using (var tfs = File.Create(trackPath))
                using (var tw = new BinaryWriter(tfs)) {
                    tw.Write(Encoding.ASCII.GetBytes("RIFF"));
                    tw.Write(0);
                    tw.Write(Encoding.ASCII.GetBytes("WAVE"));
                    tw.Write(Encoding.ASCII.GetBytes("fmt "));
                    tw.Write(16);
                    tw.Write((short)1);
                    tw.Write((short)1);          // MONO
                    tw.Write(44100);
                    tw.Write(44100 * 2);
                    tw.Write((short)2);
                    tw.Write((short)16);
                    tw.Write(Encoding.ASCII.GetBytes("data"));
                    tw.Write(0);
                    var tb = new float[4096];
                    double tpeak = 0;
                    int tpos = 0;
                    int tFrames = 0;
                    while (true) {
                        // Same reason as the stereo loop below: WaveMix.Mix does
                        // not clear the buffer and WaveSource.Mix accumulates.
                        Array.Clear(tb, 0, tb.Length);
                        int next = trackMixes[i].Mix(tpos, tb, 0, tb.Length);
                        if (next <= tpos) break;
                        // ISignalSource.Mix speaks in stereo-interleaved FLOAT
                        // units (2 per frame). PlaybackManager.RenderToFiles
                        // exports via `new ExportAdapter(mix).ToMono(1, 0)`, i.e.
                        // NAudio's stereo->mono fold-down. Writing raw floats into
                        // a mono header doubles the length and the pitch comes out
                        // an octave low, so fold to mono here the same way.
                        int floats = next - tpos;
                        int frames = floats / 2;
                        for (int k = 0; k < frames; k++) {
                            float mono = 0.5f * (tb[2 * k] + tb[2 * k + 1]);
                            tpeak = Math.Max(tpeak, Math.Abs(mono));
                            tw.Write((short)Math.Max(-32768, Math.Min(32767, mono * 32767)));
                        }
                        tFrames += frames;
                        tpos = next;
                    }
                    long tend = tfs.Position;
                    tfs.Seek(4, SeekOrigin.Begin);
                    tw.Write((int)(tend - 8));
                    tfs.Seek(40, SeekOrigin.Begin);
                    tw.Write((int)(tend - 44));
                    Console.WriteLine($"  track {i}: {trackPath} ({tFrames} frames, {tFrames * 1000.0 / 44100:F1} ms, peak {tpeak:F4})");
                }
            }

            // --- Write 16-bit PCM STEREO 44100 --------------------------------
            // OpenUtau's internal mix is stereo interleaved (WaveSource
            // uses copies = 2/channels; renderers output stereo pairs), so
            // each frame = 2 floats (L, R). Write them as a stereo wav.
            const int sampleRate = 44100;
            const int chunkSize = 4096;
            var buffer = new float[chunkSize];
            int total = 0;          // total floats consumed
            double peak = 0;
            using (var fs = File.Create(args[1]))
            using (var writer = new BinaryWriter(fs)) {
                // RIFF header placeholder (fixed sizes filled at the end).
                writer.Write(Encoding.ASCII.GetBytes("RIFF"));
                writer.Write(0);
                writer.Write(Encoding.ASCII.GetBytes("WAVE"));
                writer.Write(Encoding.ASCII.GetBytes("fmt "));
                writer.Write(16);
                writer.Write((short)1);            // PCM
                writer.Write((short)2);            // STEREO
                writer.Write(sampleRate);
                writer.Write(sampleRate * 4);      // byte rate
                writer.Write((short)4);            // block align
                writer.Write((short)16);           // bits
                writer.Write(Encoding.ASCII.GetBytes("data"));
                writer.Write(0);
                while (true) {
                    // OpenUtau ISignalSource.Mix returns the NEXT absolute
                    // position (in float units — stereo interleaved).
                    //
                    // NB: WaveMix.Mix never clears the destination and
                    // WaveSource.Mix writes with += — so the buffer MUST be
                    // zeroed here, otherwise every gap inherits stale audio
                    // from the previous chunk and covered ranges accumulate
                    // (that produced the "golden sings through the rest"
                    // artifacts + clipping). MasterAdapter.Read in the real
                    // app clears the buffer before each read for the same
                    // reason.
                    Array.Clear(buffer, 0, chunkSize);
                    int nextPos = mix.Mix(total, buffer, 0, chunkSize);
                    if (nextPos <= total) break;
                    int n = nextPos - total;
                    for (int i = 0; i < n; i++) {
                        peak = Math.Max(peak, Math.Abs(buffer[i]));
                        short s = (short)Math.Max(-32768, Math.Min(32767, buffer[i] * 32767));
                        writer.Write(s);
                    }
                    total += n;
                }
                long end = fs.Position;
                fs.Seek(4, SeekOrigin.Begin);
                writer.Write((int)(end - 8));
                fs.Seek(40, SeekOrigin.Begin);
                writer.Write((int)(end - 44));
            }
            Console.WriteLine($"Wrote {args[1]}: {total / 2} frames ({total / 2 * 1000.0 / sampleRate:F1} ms stereo), peak {peak:F4}");
            return 0;
        }

        /// WORLDLINE_RENDERER env → renderer version used by BOTH modes (T17):
        /// 1 = WORLDLINE-R (WORLD resynthesis, frame 441/10ms),
        /// 2 = WORLDLINE-R2 (SynthFeatures → mel.onnx → HiFiGAN,
        /// frame 512/11.6ms — matches our DEFAULT_FRAME_MS).
        /// Default 1 (unchanged behaviour when the env var is unset).
        static int RendererVersion() {
            int ver = 1;
            var verStr = Environment.GetEnvironmentVariable("WORLDLINE_RENDERER");
            if (!string.IsNullOrEmpty(verStr) && int.TryParse(verStr, out int parsed)) {
                ver = parsed;
            }
            return ver;
        }

        /// Apple-to-apple mode: render each phrase through the renderer
        /// directly (no mixdown, no faders, no stereo upmix) and write a
        /// MONO wav — mirroring what our Rust engine's PhraseSynth::Synth()
        /// produces. This is the defensible comparison target for the
        /// golden test (Sprint 2.3.4).
        static int RenderPhrasesDirect(string outPath, UProject project, RenderPhrase[] phrases, bool diffSingerMode = false) {
            const int sampleRate = 44100;
            // Stitch all phrases into one mono stream (phrase order).
            var all = new List<float>();
            double peak = 0;
            // Renderer version selectable via WORLDLINE_RENDERER env (same helper
            // the demo path uses, so both modes stay in sync — T17).
            int ver = RendererVersion();
            // DiffSinger: render through the phrase's OWN renderer (the
            // DiffSingerRenderer ValidateFull assigned for a DS singer).
            IRenderer renderer = diffSingerMode
                ? phrases[0].renderer
                : new WorldlineRenderer(ver);
            var progress = new Progress(phrases.Length);
            var cts = new CancellationTokenSource();
            for (int i = 0; i < phrases.Length; i++) {
                var phrase = phrases[i];
                var result = renderer.Render(phrase, progress, 0, cts, isPreRender: false).GetAwaiter().GetResult();
                if (result == null || result.samples == null || result.samples.Length == 0) {
                    Console.Error.WriteLine($"phrase {i} produced no samples (renderer {phrase.renderer})");
                    return 2;
                }
                Console.WriteLine($"phrase {i}: renderer={phrase.renderer} samples={result.samples.Length} " +
                    $"({result.samples.Length * 1000.0 / sampleRate:F1} ms) posMs={result.positionMs:F1} " +
                    $"leadingMs={result.leadingMs:F1} estLen={result.estimatedLengthMs:F1}");
                foreach (var s in result.samples) peak = Math.Max(peak, Math.Abs(s));
                all.AddRange(result.samples);
            }
            using (var fs = File.Create(outPath))
            using (var writer = new BinaryWriter(fs)) {
                writer.Write(Encoding.ASCII.GetBytes("RIFF"));
                writer.Write(0);
                writer.Write(Encoding.ASCII.GetBytes("WAVE"));
                writer.Write(Encoding.ASCII.GetBytes("fmt "));
                writer.Write(16);
                writer.Write((short)1);            // PCM
                writer.Write((short)1);            // MONO
                writer.Write(sampleRate);
                writer.Write(sampleRate * 2);      // byte rate
                writer.Write((short)2);            // block align
                writer.Write((short)16);           // bits
                writer.Write(Encoding.ASCII.GetBytes("data"));
                writer.Write(0);
                foreach (var s in all) {
                    short v = (short)Math.Max(-32768, Math.Min(32767, s * 32767));
                    writer.Write(v);
                }
                long end = fs.Position;
                fs.Seek(4, SeekOrigin.Begin);
                writer.Write((int)(end - 8));
                fs.Seek(40, SeekOrigin.Begin);
                writer.Write((int)(end - 44));
            }
            Console.WriteLine($"Wrote {outPath}: {all.Count} mono samples ({all.Count * 1000.0 / sampleRate:F1} ms), peak {peak:F4}");
            return 0;
        }
    }
}
