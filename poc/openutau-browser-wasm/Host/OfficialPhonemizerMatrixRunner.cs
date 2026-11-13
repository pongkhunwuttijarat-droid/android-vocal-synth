using System.Reflection;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core.Ustx;

namespace OpenUtauBrowserWasmHost;

public sealed record OfficialPhonemizerCase(
    string Package,
    string Name,
    string Tag,
    string TypeName,
    string Status,
    string Output,
    string Detail);

public static class OfficialPhonemizerMatrixRunner {
    /// <summary>Run the official matrix. When <paramref name="fixtureCatalog"/> is
    /// supplied, tags matching ja/en receive the real in-memory fixture singers
    /// instead of the dummy accept-all singer. When <paramref name="randomLyrics"/>
    /// is set, every case gets a seeded random per-language lyric instead of the
    /// fixed smoke inputs (deterministic: same build → same sequence).</summary>
    public static IReadOnlyList<OfficialPhonemizerCase> Run(
        byte[] officialBuiltinAssemblyBytes,
        ClassicFixtureCatalog? fixtureCatalog = null,
        bool randomLyrics = false) {
        var builtinAssembly = Assembly.Load(officialBuiltinAssemblyBytes);
        var types = GetLoadableTypes(typeof(Phonemizer).Assembly)
            .Concat(GetLoadableTypes(builtinAssembly))
            .Where(type => type is { IsAbstract: false, IsPublic: true }
                && typeof(Phonemizer).IsAssignableFrom(type))
            .Select(type => (Type: type, Metadata: type.GetCustomAttribute<PhonemizerAttribute>()))
            .Where(entry => entry.Metadata is not null)
            .OrderBy(entry => entry.Metadata!.Tag, StringComparer.Ordinal)
            .ThenBy(entry => entry.Metadata!.Name, StringComparer.Ordinal)
            .ToArray();
        var banks = fixtureCatalog?.Banks.ToDictionary(bank => bank.Name, StringComparer.Ordinal);
        // Seeded so a matrix run is reproducible while still sampling every language.
        var random = randomLyrics ? new Random(20261003) : null;

        var cases = new List<OfficialPhonemizerCase>(types.Length);
        foreach (var (type, metadata) in types) {
            var package = type.Name == "PresampSamplePhonemizer"
                ? "OpenUtau.Plugin.Builtin · DEBUG only"
                : type.Assembly == builtinAssembly ? "OpenUtau.Plugin.Builtin" : "OpenUtau.Core";
            var name = metadata!.Name;
            var tag = metadata.Tag;
            Phonemizer phonemizer;
            string lifecycleDetail = string.Empty;
            string? setupDetail = null;
            string processOutput = string.Empty;
            Exception? processError = null;

            try {
                phonemizer = Activator.CreateInstance(type) as Phonemizer
                    ?? throw new InvalidOperationException($"Could not create {type.FullName}.");
                phonemizer.Testing = true;
            } catch (Exception ex) {
                var error = Unwrap(ex);
                cases.Add(new OfficialPhonemizerCase(
                    package, name, tag, type.FullName ?? type.Name,
                    ClassifyFailure(type, string.Empty, null, error), string.Empty,
                    $"{error.GetType().Name}: {error.Message}"));
                continue;
            }

            TryLifecycleStep(() => phonemizer.SetSinger(USinger.CreateMissing("T013 unloaded test singer")), ref lifecycleDetail, "SetSinger(unloaded)");
            TryLifecycleStep(() => phonemizer.SetSinger(null!), ref lifecycleDetail, "SetSinger(null)");
            TryLifecycleStep(() => phonemizer.SetSinger(CreateDummySinger()), ref lifecycleDetail, "SetSinger(dummy singer)");

            // Real singer when the fixture catalog carries the tag's language.
            USinger? bankSinger = null;
            var bankName = PickBankName(tag);
            if (banks is not null && bankName is not null && banks.TryGetValue(bankName, out var bank)) {
                try {
                    bankSinger = new CatalogSinger(bank);
                    phonemizer.SetSinger(bankSinger);
                } catch (Exception ex) {
                    var error = Unwrap(ex);
                    lifecycleDetail = string.Join("; ",
                        new[] { lifecycleDetail, $"SetSinger(fixture {bank.Name}): {error.Message}" }
                            .Where(value => !string.IsNullOrWhiteSpace(value)));
                    bankSinger = null;
                }
            }

            var lyric = "a";
            var phoneticHint = "a";
            (lyric, phoneticHint) = GetSampleInput(tag);
            try {
                if (randomLyrics && random is not null) {
                    lyric = GetRandomLyricInput(tag, random);
                    phoneticHint = string.Empty;
                }
            } catch (Exception ex) {
                lifecycleDetail = $"random lyric fallback: {ex.Message}";
            }
            var note = new Phonemizer.Note {
                lyric = lyric,
                phoneticHint = phoneticHint,
                tone = 60,
                position = 240,
                duration = 480,
                phonemeAttributes = [],
            };
            var group = new[] { note };
            var project = new UProject();
            try {
                phonemizer.SetTiming(project.timeAxis);
                phonemizer.SetUp([group], project, project.tracks[0]);
            } catch (Exception ex) {
                var error = Unwrap(ex);
                setupDetail = $"SetUp(one note): {error.GetType().Name}: {error.Message}";
            }

            try {
                var result = phonemizer.Process(group, null, null, null, null, []);
                processOutput = result.phonemes is null
                    ? "(null phoneme array)"
                    : string.Join(" · ", result.phonemes.Select(phoneme => phoneme.phoneme ?? "(null)"));
                if (processOutput.Length == 0) {
                    processOutput = "(zero phonemes)";
                }
            } catch (Exception ex) {
                processError = Unwrap(ex);
            }

            var hasLifecycleIssue = !string.IsNullOrEmpty(lifecycleDetail) || setupDetail is not null;
            var status = processError is null
                ? hasLifecycleIssue ? "PARTIAL" : "PASS"
                : ClassifyFailure(type, lifecycleDetail, setupDetail, processError);
            var detail = string.Join("; ", new[] {
                string.IsNullOrWhiteSpace(lifecycleDetail) ? null : lifecycleDetail,
                setupDetail,
                processError is null ? null : $"{processError.GetType().Name}: {processError.Message}",
            }.Where(value => !string.IsNullOrWhiteSpace(value)));
            if (bankSinger is not null) {
                detail = string.IsNullOrWhiteSpace(detail)
                    ? $"singer: {PickBankName(tag)}"
                    : $"{detail}; singer: {PickBankName(tag)}";
            }

            cases.Add(new OfficialPhonemizerCase(
                package,
                name,
                tag,
                type.FullName ?? type.Name,
                status,
                processOutput,
                detail));
        }
        return cases;
    }

    /// <summary>Fixture bank name that matches a [Phonemizer] language tag, or
    /// <c>null</c> when the catalog has no bank for it.</summary>
    private static string? PickBankName(string tag) {
        if (string.IsNullOrEmpty(tag)) return null;
        if (tag.StartsWith("JA", StringComparison.Ordinal) || tag.StartsWith("S-VOICEVOX JA", StringComparison.Ordinal)
            || tag.EndsWith("to JA", StringComparison.OrdinalIgnoreCase)) {
            return "ja_vcv";
        }
        if (tag.StartsWith("EN", StringComparison.Ordinal) || tag.StartsWith("S-VOICEVOX EN", StringComparison.Ordinal)
            || tag.EndsWith("to EN", StringComparison.OrdinalIgnoreCase)) {
            return "en_vccv";
        }
        return null;
    }

    private static Type[] GetLoadableTypes(Assembly assembly) {
        try {
            return assembly.GetExportedTypes();
        } catch (ReflectionTypeLoadException ex) {
            return ex.Types.Where(type => type is not null).Cast<Type>().ToArray();
        }
    }

    private static string ClassifyFailure(Type type, string lifecycleDetail, string? setupDetail, Exception processError) {
        var evidence = $"{lifecycleDetail}; {setupDetail}; {processError.GetType().Name}: {processError.Message}";
        if (evidence.Contains("Microsoft.ML.OnnxRuntime.NativeMethods", StringComparison.Ordinal)
            || type.Namespace?.Contains("Vogen", StringComparison.Ordinal) == true && processError is DllNotFoundException) {
            return "NATIVE ML RUNTIME";
        }
        if (evidence.Contains("System.Diagnostics.Process is not supported", StringComparison.Ordinal)
            || evidence.Contains("is not a supported encoding name", StringComparison.Ordinal)) {
            return "WASM API LIMIT";
        }
        if (type.Namespace?.Contains("DiffSinger", StringComparison.Ordinal) == true
            || type.Namespace?.Contains("Enunu", StringComparison.Ordinal) == true
            || type.Namespace?.Contains("Voicevox", StringComparison.Ordinal) == true
            || type.Name.Contains("Enunu", StringComparison.Ordinal)) {
            return "NEEDS CONTEXT";
        }
        if (processError.Message.Contains("Unrecognized lyric", StringComparison.Ordinal)) {
            return "NEEDS INPUT";
        }
        return setupDetail is not null ? "NEEDS CONTEXT" : "PROCESS ERROR";
    }

    private static (string Lyric, string Hint) GetSampleInput(string tag) {
        if (tag.StartsWith("JA ", StringComparison.Ordinal)) return ("な", string.Empty);
        if (tag.StartsWith("S-VOICEVOX JA", StringComparison.Ordinal)) return ("あ", string.Empty);
        if (tag.StartsWith("ZH-YUE SYO", StringComparison.Ordinal)) return ("nei5", string.Empty);
        return ("a", "a");
    }

    // Random per-language pools for the "random lyrics" matrix mode. Seeded by
    // the caller, so one build always produces the same sequence (the results
    // stay reproducible and diffable) while covering every language family the
    // official phonemizers declare.
    private static string GetRandomLyricInput(string tag, Random random) {
        var language = ClassifyTagLanguage(tag);
        if (language == "ja") {
            var morae = JaMorae;
            var count = random!.Next(1, 4);
            return string.Concat(Enumerable.Range(0, count).Select(_ => morae[random.Next(morae.Length)]));
        }
        if (language != "en") {
            var pool = language switch {
                "zh" => ZhPinyin,
                "zh-yue" => YueJyutping,
                "ko" => KoHangul,
                "ru" => RuCyrillic,
                "th" => ThaiSyllables,
                "de" => LatinWords["de"],
                "es" => LatinWords["es"],
                "fr" => LatinWords["fr"],
                "it" => LatinWords["it"],
                "pt" => LatinWords["pt"],
                "pl" => LatinWords["pl"],
                "tr" => LatinWords["tr"],
                "fil" => LatinWords["fil"],
                _ => EnglishWords,
            };
            return pool[random!.Next(pool.Length)];
        }
        // EN (and the default): alternate real words and letter strings so
        // dictionary-driven and rule-driven phonemizers both get coverage.
        return random!.Next(2) == 0
            ? EnglishWords[random.Next(EnglishWords.Length)]
            : string.Concat(Enumerable.Range(0, random.Next(2, 6)).Select(
                _ => "aehioprstuwnky"[random.Next(15)]));
    }

    private static string ClassifyTagLanguage(string tag) {
        if (string.IsNullOrEmpty(tag)) return "en";
        if (tag.StartsWith("JA", StringComparison.Ordinal) || tag.StartsWith("S-VOICEVOX JA", StringComparison.Ordinal)
            || tag.EndsWith("to JA", StringComparison.OrdinalIgnoreCase)) return "ja";
        if (tag.StartsWith("ZH-YUE", StringComparison.Ordinal)) return "zh-yue";
        if (tag.StartsWith("ZH", StringComparison.Ordinal)) return "zh";
        if (tag.StartsWith("KO", StringComparison.Ordinal)) return "ko";
        if (tag.StartsWith("RU", StringComparison.Ordinal)) return "ru";
        if (tag.StartsWith("TH", StringComparison.Ordinal)) return "th";
        if (tag.StartsWith("DE", StringComparison.Ordinal)) return "de";
        if (tag.StartsWith("ES", StringComparison.Ordinal)) return "es";
        if (tag.StartsWith("FR", StringComparison.Ordinal)) return "fr";
        if (tag.StartsWith("IT", StringComparison.Ordinal)) return "it";
        if (tag.StartsWith("PT", StringComparison.Ordinal)) return "pt";
        if (tag.StartsWith("PL", StringComparison.Ordinal)) return "pl";
        if (tag.StartsWith("TR", StringComparison.Ordinal)) return "tr";
        if (tag.StartsWith("FIL", StringComparison.Ordinal)) return "fil";
        return "en";
    }

    private static readonly string[] JaMorae = [
        "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と",
        "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み", "む", "め", "も",
        "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "を", "ん", "きゃ", "しゃ", "ちょ",
    ];
    private static readonly string[] EnglishWords = [
        "hello", "world", "banana", "kite", "song", "dream", "night", "city",
        "light", "wonder", "star", "moon", "water", "fire", "happy", "smile",
        "sing", "together", "forever", "music",
    ];
    private static readonly string[] ZhPinyin = ["ni", "hao", "wo", "ai", "ni", "zhong", "guo", "shui", "yue", "guang"];
    private static readonly string[] YueJyutping = ["nei5", "hou2", "ngo5", "oi3", "si6", "je5", "lok6", "hok1"];
    private static readonly string[] KoHangul = ["안녕", "사랑", "고마워", "행복", "노래", "바람"];
    private static readonly string[] RuCyrillic = ["привет", "спасибо", "песня", "друг", "солнце"];
    private static readonly string[] ThaiSyllables = ["สวัสดี", "เพลง", "เพื่อน", "ชีวิต", "รัก"];
    private static readonly Dictionary<string, string[]> LatinWords = new() {
        ["de"] = ["hallo", "welt", "lied", "freund", "sängerin"],
        ["es"] = ["hola", "canción", "amigo", "vida", "luna"],
        ["fr"] = ["bonjour", "chanson", "ami", "vie", "lune"],
        ["it"] = ["ciao", "canzone", "amico", "vita", "luna"],
        ["pt"] = ["olá", "canção", "amigo", "vida", "lua"],
        ["pl"] = ["cześć", "piosenka", "przyjaciel", "życie"],
        ["tr"] = ["merhaba", "şarkı", "arkadaş", "güneş"],
        ["fil"] = ["kamusta", "kanta", "kaibigan", "buhay", "tala"],
    };

    private static void TryLifecycleStep(Action action, ref string details, string stage) {
        try {
            action();
        } catch (Exception ex) {
            var message = $"{stage}: {Unwrap(ex).GetType().Name}: {Unwrap(ex).Message}";
            details = string.IsNullOrEmpty(details) ? message : $"{details}; {message}";
        }
    }

    private static Exception Unwrap(Exception ex) {
        while (ex is TargetInvocationException or TypeInitializationException
            && ex.InnerException is Exception inner) {
            ex = inner;
        }
        return ex;
    }

    private static USinger CreateDummySinger() => new T013DummySinger();

    private sealed class T013DummySinger : USinger {
        private readonly OtoSet otoSet = new() { Name = "T013 in-memory test bank" };
        private readonly List<USubbank> subbanks = [new USubbank(new Subbank())];
        private readonly UOtoSet set;

        public T013DummySinger() {
            set = new UOtoSet(otoSet, string.Empty);
        }

        public override string Id => "T013-dummy";
        public override string Name => "T013 dummy singer";
        public override USingerType SingerType => USingerType.Classic;
        public override string Location => string.Empty;
        public override IList<UOto> Otos => [MakeOto("a")];
        public override IList<USubbank> Subbanks => subbanks;

        public override bool TryGetOto(string phoneme, out UOto oto) {
            oto = string.IsNullOrEmpty(phoneme) ? null! : MakeOto(phoneme);
            return oto is not null;
        }

        public override bool TryGetMappedOto(string phoneme, int tone, string color, out UOto oto) =>
            TryGetOto(phoneme, out oto);

        private UOto MakeOto(string alias) => new(
            new Oto { Alias = alias, IsValid = true },
            set,
            subbanks.ToArray());
    }
}
