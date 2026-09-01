using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Xml.Linq;
using Microsoft.ML.OnnxRuntime;
using Microsoft.ML.OnnxRuntime.Tensors;
using NAudio.Wave;
using NumSharp;
using OpenUtau.Core;
using OpenUtau.Core.Format;
using OpenUtau.Core.Render;
using OpenUtau.Core.SignalChain;
using OpenUtau.Core.Ustx;

namespace OpenUtau.Classic {
    public class WorldlineRenderer : IRenderer {

        readonly int version;
        readonly double frameMs;
        byte[]? vocoderBytes;

        public WorldlineRenderer(int version) {
            if (version != 1 && version != 2) {
                throw new ArgumentException($"Unsupported WorldlineRenderer version: {version}");
            }
            this.version = version;
            frameMs = version == 1 ? 10 : 512.0 * 1000.0 / 44100.0;
        }

        static readonly HashSet<string> supportedExp = new HashSet<string>(){
            Ustx.DYN,
            Ustx.PITD,
            Ustx.CLR,
            Ustx.SHFT,
            Ustx.VEL,
            Ustx.VOL,
            Ustx.MOD,
            Ustx.MODP,
            Ustx.ALT,
            Ustx.GENC,
            Ustx.BREC,
            Ustx.TENC,
            Ustx.VOIC,
            Ustx.DIR,
        };

        public USingerType SingerType => USingerType.Classic;

        public bool SupportsRenderPitch => false;

        public bool SupportsExpression(UExpressionDescriptor descriptor) {
            return supportedExp.Contains(descriptor.abbr);
        }

        public RenderResult Layout(RenderPhrase phrase) {
            return new RenderResult() {
                leadingMs = phrase.leadingMs,
                positionMs = phrase.positionMs,
                estimatedLengthMs = phrase.durationMs + phrase.leadingMs,
            };
        }

        public Task<RenderResult> Render(RenderPhrase phrase, Progress progress, int trackNo, CancellationTokenSource cancellation, bool isPreRender) {
            var resamplerItems = new List<ResamplerItem>();
            foreach (var phone in phrase.phones) {
                resamplerItems.Add(new ResamplerItem(phrase, phone));
            }
            var task = Task.Run(() => {
                var result = Layout(phrase);
                var wavPath = Path.Join(PathManager.Inst.CachePath, $"wdl-v{version}-{phrase.hash:x16}.wav");
                phrase.AddCacheFile(wavPath);
                string progressInfo = $"Track {trackNo + 1}: {this} {string.Join(" ", phrase.phones.Select(p => p.phoneme))}";
                progress.Complete(0, progressInfo);
                if (File.Exists(wavPath)) {
                    using (var waveStream = Wave.OpenFile(wavPath)) {
                        result.samples = Wave.GetSamples(waveStream.ToSampleProvider().ToMono(1, 0));
                    }
                }
                if (result.samples == null) {
                    // v1 (WORLDLINE-R) calls the raw C++ PhraseSynth
                    // (PhraseSynthNew/AddRequest/SetCurves/Synth) — the
                    // SAME path our Rust r1 engine uses, so the golden
                    // reference measures apples-to-apples against r1.
                    // v2 (WORLDLINE-R2) keeps the feature path
                    // (SynthFeatures → mel.onnx → vocoder).
                    Worldline.PhraseSynth phraseSynthV1 = null;
                    Worldline.PhraseSynthV2 phraseSynth = null;
                    if (version == 1) {
                        phraseSynthV1 = new Worldline.PhraseSynth();
                    } else {
                        phraseSynth = new Worldline.PhraseSynthV2(44100, 512, 2048);
                    }
                    double posOffsetMs = phrase.positionMs - phrase.leadingMs;
                    foreach (var item in resamplerItems) {
                        if (cancellation.IsCancellationRequested) {
                            return result;
                        }
                        double posMs = item.phone.positionMs - item.phone.leadingMs - (phrase.positionMs - phrase.leadingMs);
                        double skipMs = item.skipOver;
                        double lengthMs = item.phone.envelope[4].X - item.phone.envelope[0].X;
                        double fadeInMs = item.phone.envelope[1].X - item.phone.envelope[0].X;
                        double fadeOutMs = item.phone.envelope[4].X - item.phone.envelope[3].X;
                        try {
                            if (phraseSynthV1 != null) {
                                phraseSynthV1.AddRequest(item, posMs, skipMs, lengthMs, fadeInMs, fadeOutMs);
                            } else {
                                phraseSynth.AddRequest(item, posMs, skipMs, lengthMs, fadeInMs, fadeOutMs);
                            }
                        } catch (SynthRequestError e) {
                            if (e is CutOffExceedDurationError cee) {
                                throw new MessageCustomizableException(
                                    $"Failed to render\n Oto error: cutoff exceeds audio duration \n{item.phone.phoneme}",
                                    $"<translate:errors.failed.synth.cutoffexceedduration>\\n{item.phone.phoneme}",
                                    e);
                            }
                            if (e is CutOffBeforeOffsetError cbe) {
                                throw new MessageCustomizableException(
                                    $"Failed to render\n Oto error: cutoff before offset \n{item.phone.phoneme}",
                                    $"<translate:errors.failed.synth.cutoffbeforeoffset>\\n{item.phone.phoneme}",
                                    e);
                            }
                            throw e;
                        }
                    }
                    int frames = (int)Math.Ceiling(result.estimatedLengthMs / frameMs);
                    var f0 = SampleCurve(phrase, phrase.pitches, 0, frames, x => MusicMath.ToneToFreq(x * 0.01));
                    var gender = SampleCurve(phrase, phrase.gender, 0.5, frames, x => 0.5 + 0.005 * x);
                    var tension = SampleCurve(phrase, phrase.tension, 0.5, frames, x => 0.5 + 0.005 * x);
                    var breathiness = SampleCurve(phrase, phrase.breathiness, 0.5, frames, x => 0.5 + 0.005 * x);
                    var voicing = SampleCurve(phrase, phrase.voicing, 1.0, frames, x => 0.01 * x);
                    if (phraseSynthV1 != null) {
                        phraseSynthV1.SetCurves(f0, gender, tension, breathiness, voicing);
                    } else {
                        phraseSynth.SetCurves(f0, gender, tension, breathiness, voicing);
                    }
                    if (version == 1) {
                        result.samples = phraseSynthV1.Synth();
                    } else {
                        var (totalFrames, f0Out, spEnvOut, apOut) = phraseSynth.SynthFeatures();
                        int paddedLength = totalFrames + 8;
                        paddedLength = (int)Math.Ceiling((float)(paddedLength + 4) / 16) * 16;
                        int leftPadding = 4;
                        int rightPadding = paddedLength - totalFrames - leftPadding;
                        int spSize = spEnvOut.shape[1];
                        f0Out = np.concatenate(new NDArray[] { np.zeros((leftPadding)), f0Out, np.zeros((rightPadding)) }, axis: 0);
                        spEnvOut = np.concatenate(new NDArray[] { np.zeros((leftPadding, spSize)), spEnvOut, np.zeros((rightPadding, spSize)) }, axis: 0);
                        apOut = np.concatenate(new NDArray[] { np.ones((leftPadding, spSize)), apOut, np.ones((rightPadding, spSize)) }, axis: 0);
                        var f0Tensor = new DenseTensor<float>(f0Out.astype(typeof(float)).ToArray<float>(), new int[] { 1, paddedLength });
                        var spEnvTensor = new DenseTensor<float>(spEnvOut.astype(typeof(float)).ToArray<float>(), new int[] { 1, paddedLength, spEnvOut.shape[1] });
                        var apTensor = new DenseTensor<float>(apOut.astype(typeof(float)).ToArray<float>(), new int[] { 1, paddedLength, apOut.shape[1] });
                        var inputs = new List<NamedOnnxValue> {
                            NamedOnnxValue.CreateFromTensor("f0", f0Tensor),
                            NamedOnnxValue.CreateFromTensor("sp_env", spEnvTensor),
                            NamedOnnxValue.CreateFromTensor("ap", apTensor)
                        };
                        // Headless golden harness: Resources.mel is null
                        // when the ResXFileRef did not embed the onnx
                        // bytes at build time — fall back to the file on
                        // disk next to this source tree.
                        var mel = OpenUtau.Core.Classic.Data.Resources.mel;
                        if (mel == null) {
                            var melPath = System.IO.Path.Combine(
                                AppContext.BaseDirectory, "mel.onnx");
                            if (!System.IO.File.Exists(melPath)) {
                                melPath = System.IO.Path.Combine(
                                    AppContext.BaseDirectory, "Classic", "Data", "mel.onnx");
                            }
                            if (System.IO.File.Exists(melPath)) {
                                mel = System.IO.File.ReadAllBytes(melPath);
                            } else {
                                throw new FileNotFoundException(
                                    "mel.onnx not found (embedded resource null and no file on disk)", melPath);
                            }
                        }
                        var session = Onnx.getInferenceSession(mel, OnnxRunnerChoice.CPU);
                        using var results = session.Run(inputs);
                        var melOutput = results.First(r => r.Name == "mel").AsTensor<float>();
                        if (Environment.GetEnvironmentVariable("GOLDEN_DUMP_MEL") == "1") {
                            int t = totalFrames;
                            var powers = new List<float>();
                            for (int f = 0; f < t; f++) {
                                float sum = 0;
                                for (int b = 0; b < 128; b++) {
                                    float v = melOutput[0, leftPadding + f, b];
                                    sum += v * v;
                                }
                                powers.Add(sum / 128f);
                            }
                            Console.WriteLine($"[mel-dump] total={t} pad_left={leftPadding} mean={powers.Average():F6} max={powers.Max():F6} first10=[{string.Join(", ", powers.Take(10).Select(p => p.ToString("F2")))}]");
                        }
                        const string vocoderPkg = "pc-nsf-hifigan";
                        string vocoderPath = PackageManager.Inst.GetInstalledPath(vocoderPkg) ?? "";
                        if (vocoderBytes == null) {
                            var configPath = Path.Combine(vocoderPath, "vocoder.yaml");
                            if (!File.Exists(configPath)) {
                                throw new MessageCustomizableException(
                                    $"Error loading package \"{vocoderPkg}\"",
                                    $"<translate:packages.errors.missing>",
                                    new Exception($"Error loading package \"{vocoderPkg}\""),
                                true,
                                    new string[] { vocoderPkg });
                            }
                            var config = Yaml.DefaultDeserializer.Deserialize<Core.DiffSinger.DsVocoderConfig>(
                                File.ReadAllText(configPath, System.Text.Encoding.UTF8));
                            vocoderBytes = File.ReadAllBytes(Path.Combine(vocoderPath, config.model));
                        }
                        var vocoderSession = Onnx.getInferenceSession(vocoderBytes!, OnnxRunnerChoice.Default);
                        var vocoderInputs = new List<NamedOnnxValue> {
                            NamedOnnxValue.CreateFromTensor("mel", melOutput),
                            NamedOnnxValue.CreateFromTensor("f0", f0Tensor),
                        };
                        using var vocoderResults = vocoderSession.Run(vocoderInputs);
                        var audioOutput = vocoderResults.First().AsTensor<float>();
                        result.samples = audioOutput.ToArray();
                        result.samples = result.samples.Skip(leftPadding * 512).Take(totalFrames * 512).ToArray();
                        int easeInSamples = Math.Min(result.samples.Length, 512);
                        for (int i = 0; i < easeInSamples; ++i) {
                            double gain = (double)i / easeInSamples;
                            result.samples[i] = (float)(result.samples[i] * gain);
                        }
                        int easeOutSamples = Math.Min(result.samples.Length, 512);
                        for (int i = 0; i < easeOutSamples; ++i) {
                            double gain = (double)(easeOutSamples - i) / easeOutSamples;
                            result.samples[result.samples.Length - easeOutSamples + i] = (float)(result.samples[result.samples.Length - easeOutSamples + i] * gain);
                        }
                    }
                    AddDirects(phrase, resamplerItems, result);
                    var source = new WaveSource(0, 0, 0, 1);
                    source.SetSamples(result.samples);
                    WaveFileWriter.CreateWaveFile16(wavPath, new ExportAdapter(source).ToMono(1, 0));
                }
                progress.Complete(phrase.phones.Length, progressInfo);
                if (result.samples != null) {
                    Renderers.ApplyDynamics(phrase, result);
                }
                return result;
            });
            return task;
        }

        double[] SampleCurve(RenderPhrase phrase, float[] curve, double defaultValue, int length, Func<double, double> convert) {
            const int interval = 5;
            var result = new double[length];
            if (curve == null) {
                Array.Fill(result, defaultValue);
                return result;
            }
            for (int i = 0; i < length; i++) {
                double posMs = phrase.positionMs - phrase.leadingMs + i * frameMs;
                int ticks = phrase.timeAxis.MsPosToTickPos(posMs) - (phrase.position - phrase.leading);
                int index = Math.Max(0, (int)((double)ticks / interval));
                if (index < curve.Length) {
                    result[i] = convert(curve[index]);
                }
            }
            return result;
        }

        private static void AddDirects(RenderPhrase phrase, List<ResamplerItem> resamplerItems, RenderResult result) {
            foreach (var item in resamplerItems) {
                if (!item.phone.direct) {
                    continue;
                }
                double posMs = item.phone.positionMs - item.phone.leadingMs - (phrase.positionMs - phrase.leadingMs);
                int startPhraseIndex = (int)(posMs / 1000 * 44100);
                using (var waveStream = Wave.OpenFile(item.phone.oto.File)) {
                    if (waveStream == null) {
                        continue;
                    }
                    float[] samples = Wave.GetSamples(waveStream!.ToSampleProvider().ToMono(1, 0));
                    int offset = (int)(item.phone.oto.Offset / 1000 * 44100);
                    int cutoff = (int)(item.phone.oto.Cutoff / 1000 * 44100);
                    int length = cutoff >= 0 ? (samples.Length - offset - cutoff) : -cutoff;
                    samples = samples.Skip(offset).Take(length).ToArray();
                    item.ApplyEnvelope(samples);
                    for (int i = 0; i < Math.Min(samples.Length, result.samples.Length - startPhraseIndex); ++i) {
                        result.samples[startPhraseIndex + i] = samples[i];
                    }
                }
            }
        }

        public RenderPitchResult LoadRenderedPitch(RenderPhrase phrase) {
            return null;
        }

        public UExpressionDescriptor[] GetSuggestedExpressions(USinger singer, URenderSettings renderSettings) {
            return new UExpressionDescriptor[] { };
        }

        public override string ToString() => version == 1 ? Renderers.WORLDLINE_R : Renderers.WORLDLINE_R2;
    }
}
