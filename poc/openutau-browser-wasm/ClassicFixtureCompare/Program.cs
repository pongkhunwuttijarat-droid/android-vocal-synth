using System.Text;
using System.Text.Json;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core;
using OpenUtau.Core.Format;
using OpenUtau.Core.Ustx;
using OpenUtau.Plugin.Builtin;

Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);

var repository = FindRepositoryRoot();
var fixtureRoot = Path.Combine(repository,
    "ref(openutau+openutau mobile)", "desktop-ref", "OpenUtau.Test", "Files");
var cases = new[] {
    new FixtureCase("Japanese VCV", "ja_vcv", () => new JapaneseVCVPhonemizer(),
        ["あ", "+", "あ", "-"], [60, 60, 60, 60], ["", "", "", ""], ["", "", "Clear", ""],
        ["- あA3", "a あCA3", "-"]),
    new FixtureCase("Japanese VCV color/tone", "ja_vcv", () => new JapaneseVCVPhonemizer(),
        ["お", "にょ", "ひょ", "みょ", "びょ", "ぴょ", "りょ"], [57, 60, 62, 64, 65, 55, 53],
        ["", "", "", "", "", "", ""], ["", "Clear", "", "", "Whisper", "", ""],
        ["- おA3", "o にょCA3", "o ひょD4", "o みょD4", "o びょWD4", "o ぴょA3", "o りょA3"]),
    new FixtureCase("English VCCV words", "en_vccv", () => new EnglishVCCVPhonemizer(),
        ["test", "words"], [60, 60], ["", ""], ["", ""],
        ["-te", "es-", "st", "w3", "3d-", "dz-"]),
    new FixtureCase("English VCCV hint", "en_vccv", () => new EnglishVCCVPhonemizer(),
        ["read"], [60], ["r E d"], [""], ["-rE", "Ed-"]),
};

var results = cases.Select(testCase => RunCase(fixtureRoot, testCase)).ToArray();
var catalog = new ClassicFixtureCatalog([
    BuildBankFixture(fixtureRoot, "ja_vcv"),
    BuildBankFixture(fixtureRoot, "en_vccv"),
]);
var outputPath = args.Length > 0
    ? Path.GetFullPath(args[0])
    : Path.Combine(repository, "poc/openutau-browser-wasm/artifacts/classic-fixture-openutau.json");
Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
await File.WriteAllTextAsync(outputPath, JsonSerializer.Serialize(results, new JsonSerializerOptions {
    WriteIndented = true,
}));
var browserFixturePath = Path.Combine(repository,
    "poc/openutau-browser-wasm/Host/wwwroot/data/classic-fixtures.json");
Directory.CreateDirectory(Path.GetDirectoryName(browserFixturePath)!);
await File.WriteAllTextAsync(browserFixturePath, JsonSerializer.Serialize(catalog, new JsonSerializerOptions {
    PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    WriteIndented = true,
}));

foreach (var result in results) {
    Console.WriteLine($"{(result.MatchesExpected ? "PASS" : "FAIL"),-4} {result.Name}: " +
        $"expected [{string.Join(" | ", result.Expected)}], actual [{string.Join(" | ", result.Actual)}]");
}
Console.WriteLine($"OpenUtau cases: {results.Length}; official expected matches: {results.Count(r => r.MatchesExpected)}/{results.Length}");
Console.WriteLine($"JSON: {outputPath}");
Console.WriteLine($"Browser fixture data: {browserFixturePath}");
if (results.Any(r => !r.MatchesExpected)) Environment.ExitCode = 1;

static ClassicFixtureBank BuildBankFixture(string fixtureRoot, string bankName) {
    VoicebankLoader.IsTest = true;
    var dir = Path.Combine(fixtureRoot, bankName);
    var voicebank = new Voicebank {
        File = Path.Combine(dir, "character.txt"),
        BasePath = fixtureRoot,
    };
    VoicebankLoader.LoadVoicebank(voicebank);
    return new ClassicFixtureBank(
        bankName,
        voicebank.OtoSets.SelectMany(set => set.Otos)
            .Where(oto => oto.IsValid && !string.IsNullOrEmpty(oto.Alias))
            .Select(oto => oto.Alias)
            .Distinct(StringComparer.Ordinal)
            .ToArray(),
        voicebank.Subbanks.Select(subbank => new ClassicFixtureSubbank(
            subbank.Color,
            subbank.Prefix,
            subbank.Suffix,
            subbank.ToneRanges ?? []))
            .ToArray());
}

static OpenUtauCaseResult RunCase(string fixtureRoot, FixtureCase testCase) {
    VoicebankLoader.IsTest = true;
    var dir = Path.Combine(fixtureRoot, testCase.Bank);
    var voicebank = new Voicebank {
        File = Path.Combine(dir, "character.txt"),
        BasePath = fixtureRoot,
    };
    VoicebankLoader.LoadVoicebank(voicebank);
    var singer = new ClassicSinger(voicebank);
    singer.EnsureLoaded();

    var project = new UProject();
    Ustx.AddDefaultExpressions(project);
    var track = project.tracks[0];
    project.expressions.TryGetValue(Ustx.CLR, out var descriptor);
    track.VoiceColorExp = descriptor!.Clone();
    var colors = singer.Subbanks.Select(subbank => subbank.Color).ToHashSet();
    track.VoiceColorExp.options = colors.OrderBy(color => color, StringComparer.Ordinal).ToArray();
    track.VoiceColorExp.max = track.VoiceColorExp.options.Length - 1;

    var groups = BuildGroups(testCase);
    var timeAxis = new TimeAxis();
    timeAxis.BuildSegments(project);
    var phonemizer = testCase.Create();
    phonemizer.Testing = true;
    phonemizer.SetSinger(singer);
    phonemizer.SetTiming(timeAxis);
    phonemizer.SetUp(groups, project, track);

    var actual = new List<string>();
    for (var i = 0; i < groups.Length; i++) {
        var results = phonemizer.Process(
            groups[i],
            i > 0 ? groups[i - 1].First() : null,
            i < groups.Length - 1 ? groups[i + 1].First() : null,
            i > 0 ? groups[i - 1].First() : null,
            i < groups.Length - 1 ? groups[i + 1].First() : null,
            i > 0 ? groups[i - 1] : null!);
        actual.AddRange(results.phonemes.Select(phoneme => phoneme.phoneme));
    }
    return new OpenUtauCaseResult(testCase.Name, testCase.Bank,
        testCase.Lyrics, testCase.Hints, testCase.Tones,
        testCase.Expected, actual.ToArray(), testCase.Expected.SequenceEqual(actual));
}

static Phonemizer.Note[][] BuildGroups(FixtureCase testCase) {
    var groups = new List<Phonemizer.Note[]>();
    var group = new List<Phonemizer.Note>();
    var position = 240;
    for (var i = 0; i < testCase.Lyrics.Length; i++) {
        if (!testCase.Lyrics[i].StartsWith("+", StringComparison.Ordinal) && group.Count > 0) {
            groups.Add(group.ToArray());
            group.Clear();
        }
        group.Add(new Phonemizer.Note {
            lyric = testCase.Lyrics[i],
            phoneticHint = testCase.Hints[i],
            duration = 240,
            position = position,
            tone = testCase.Tones[i],
            phonemeAttributes = [new Phonemizer.PhonemeAttributes {
                index = 0,
                consonantStretchRatio = 1,
                voiceColor = testCase.Colors[i],
            }],
        });
        position += 240;
    }
    if (group.Count > 0) groups.Add(group.ToArray());
    return groups.ToArray();
}

static string FindRepositoryRoot() {
    var current = new DirectoryInfo(Directory.GetCurrentDirectory());
    while (current is not null) {
        if (Directory.Exists(Path.Combine(current.FullName, "native"))
            && Directory.Exists(Path.Combine(current.FullName, "ref(openutau+openutau mobile)"))) {
            return current.FullName;
        }
        current = current.Parent;
    }
    throw new DirectoryNotFoundException("Could not locate the repository root from the current directory.");
}

sealed record FixtureCase(
    string Name,
    string Bank,
    Func<Phonemizer> Create,
    string[] Lyrics,
    int[] Tones,
    string[] Hints,
    string[] Colors,
    string[] Expected);

sealed record OpenUtauCaseResult(
    string Name,
    string Bank,
    string[] Lyrics,
    string[] Hints,
    int[] Tones,
    string[] Expected,
    string[] Actual,
    bool MatchesExpected);

sealed record ClassicFixtureCatalog(ClassicFixtureBank[] Banks);

sealed record ClassicFixtureBank(string Name, string[] Aliases, ClassicFixtureSubbank[] Subbanks);

sealed record ClassicFixtureSubbank(string Color, string Prefix, string Suffix, string[] ToneRanges);
