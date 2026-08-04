using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using OpenUtau.Classic;
using OpenUtau.Core;
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

            // Headless DocManager: notifications run on a dedicated pump
            // thread (a real UI message loop — NOT inline, which would
            // recurse: ExecuteCmd -> PostOnUIThread -> ExecuteCmd -> ...).
            var uiQueue = new System.Collections.Concurrent.ConcurrentQueue<Action>();
            var uiThread = new Thread(() => {
                while (true) {
                    while (uiQueue.TryDequeue(out var action)) {
                        try { action(); } catch (Exception e) {
                            Console.Error.WriteLine("UI action error: " + e.Message);
                        }
                    }
                    Thread.Sleep(2);
                }
            }) { IsBackground = true, Name = "OpenUtauUI" };
            uiThread.Start();
            DocManager.Inst.PostOnUIThread = action => uiQueue.Enqueue(action);
            DocManager.Inst.Initialize(Thread.CurrentThread, TaskScheduler.Current);
            ToolsManager.Inst.Initialize();

            // --- Load the Teto voicebank -----------------------------------
            var loader = new VoicebankLoader(TetoDir);
            var voicebank = loader.SearchAll().FirstOrDefault();
            if (voicebank == null) {
                Console.Error.WriteLine("ERROR: no voicebank found under " + TetoDir);
                return 2;
            }
            var singer = new ClassicSinger(voicebank);
            singer.EnsureLoaded();
            if (!singer.Loaded) {
                Console.Error.WriteLine("ERROR: Teto singer failed to load (Loaded=false)");
                return 2;
            }
            // Register so track.Validate keeps it (not CreateMissing).
            SingerManager.Inst.Singers[singer.Id] = singer;
            Console.WriteLine($"Singer: {singer.Name} id={singer.Id} type={singer.SingerType}");

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

            // Set the singer and let the whole project re-validate (this sets
            // the default renderer = WORLDLINE-R for classic singers and
            // triggers phonemization).
            project.tracks[0].Singer = singer;
            DocManager.Inst.ExecuteCmd(new LoadProjectNotification(project));
            project.ValidateFull();

            // --- Wait for the async phonemizer ------------------------------
            var part0 = project.parts.OfType<UVoicePart>().FirstOrDefault();
            if (part0 == null) {
                Console.Error.WriteLine("ERROR: no voice part in project");
                return 2;
            }
            int waitedMs = 0;
            while (!part0.PhonemesUpToDate && waitedMs < 30000) {
                Thread.Sleep(100);
                waitedMs += 100;
            }
            if (!part0.PhonemesUpToDate) {
                Console.Error.WriteLine("ERROR: phonemizer did not finish in 30s");
                return 2;
            }
            var phrases = part0.GetRenderRequest();
            Console.WriteLine($"Phonemes ready after {waitedMs} ms; phrases: {phrases?.phrases?.Length ?? 0}");
            if (phrases == null || phrases.phrases.Length == 0) {
                Console.Error.WriteLine("ERROR: no render phrases (check lyrics/aliases exist in oto.ini)");
                return 2;
            }
            Console.WriteLine($"Renderer: {phrases.phrases[0].renderer}");

            // --- Apple-to-apple mode: direct phrase render (mono) ----------
            // Mirrors our Rust engine's PhraseSynth::Synth() — the defensible
            // golden comparison target (Sprint 2.3.4).
            if (args[0] == "phrase") {
                return RenderPhrasesDirect(args[1], project, phrases.phrases);
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

        /// Apple-to-apple mode: render each phrase through the renderer
        /// directly (no mixdown, no faders, no stereo upmix) and write a
        /// MONO wav — mirroring what our Rust engine's PhraseSynth::Synth()
        /// produces. This is the defensible comparison target for the
        /// golden test (Sprint 2.3.4).
        static int RenderPhrasesDirect(string outPath, UProject project, RenderPhrase[] phrases) {
            const int sampleRate = 44100;
            // Stitch all phrases into one mono stream (phrase order).
            var all = new List<float>();
            double peak = 0;
            // WORLDLINE-R = version 1 (frame 441 / 10ms — the same numbers
            // as our Rust PhraseSynth). Version 2 is the ONNX mel path.
            var renderer = new WorldlineRenderer(1);
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
