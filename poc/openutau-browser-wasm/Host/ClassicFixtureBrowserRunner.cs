using System.Reflection;
using System.Text.RegularExpressions;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core;
using OpenUtau.Core.Format;
using OpenUtau.Core.Ustx;

namespace OpenUtauBrowserWasmHost;

public sealed record ClassicFixtureCatalog(ClassicFixtureBank[] Banks);
public sealed record ClassicFixtureBank(string Name, string[] Aliases, ClassicFixtureSubbank[] Subbanks);
public sealed record ClassicFixtureSubbank(string Color, string Prefix, string Suffix, string[] ToneRanges);
public sealed record ClassicFixtureResult(
    string Name,
    string Bank,
    string[] Expected,
    string[] Actual,
    bool MatchesExpected,
    string? Error);

public static class ClassicFixtureBrowserRunner {
    private sealed record InputNote(string Lyric, int Tone, string Color = "", string Hint = "");
    private sealed record FixtureCase(
        string Name,
        string Bank,
        string TypeName,
        InputNote[] Notes,
        string[] Expected);

    public static IReadOnlyList<ClassicFixtureResult> Run(
        byte[] officialBuiltinAssemblyBytes,
        ClassicFixtureCatalog fixtureCatalog) {
        var pluginAssembly = Assembly.Load(officialBuiltinAssemblyBytes);
        var banks = fixtureCatalog.Banks.ToDictionary(bank => bank.Name, StringComparer.Ordinal);
        var cases = new[] {
            new FixtureCase("Japanese VCV", "ja_vcv", "JapaneseVCVPhonemizer",
                [new("あ", 60), new("+", 60), new("あ", 60, "Clear"), new("-", 60)],
                ["- あA3", "a あCA3", "-"]),
            new FixtureCase("Japanese VCV color/tone", "ja_vcv", "JapaneseVCVPhonemizer",
                [new("お", 57), new("にょ", 60, "Clear"), new("ひょ", 62), new("みょ", 64),
                 new("びょ", 65, "Whisper"), new("ぴょ", 55), new("りょ", 53)],
                ["- おA3", "o にょCA3", "o ひょD4", "o みょD4", "o びょWD4", "o ぴょA3", "o りょA3"]),
            new FixtureCase("English VCCV words", "en_vccv", "EnglishVCCVPhonemizer",
                [new("test", 60), new("words", 60)],
                ["-te", "es-", "st", "w3", "3d-", "dz-"]),
            new FixtureCase("English VCCV hint", "en_vccv", "EnglishVCCVPhonemizer",
                [new("read", 60, Hint: "r E d")],
                ["-rE", "Ed-"]),
        };
        return cases.Select(testCase => RunCase(pluginAssembly, banks[testCase.Bank], testCase)).ToArray();
    }

    private static ClassicFixtureResult RunCase(
        Assembly pluginAssembly,
        ClassicFixtureBank bank,
        FixtureCase testCase) {
        try {
            var type = pluginAssembly.GetType($"OpenUtau.Plugin.Builtin.{testCase.TypeName}", throwOnError: true)!;
            var phonemizer = (Phonemizer)Activator.CreateInstance(type)!;
            phonemizer.Testing = true;

            var singer = new CatalogSinger(bank);
            phonemizer.SetSinger(singer);

            var project = new UProject();
            Ustx.AddDefaultExpressions(project);
            var track = project.tracks[0];
            project.expressions.TryGetValue(Ustx.CLR, out var descriptor);
            track.VoiceColorExp = descriptor!.Clone();
            track.VoiceColorExp.options = singer.Subbanks.Select(subbank => subbank.Color)
                .Distinct(StringComparer.Ordinal)
                .OrderBy(color => color, StringComparer.Ordinal)
                .ToArray();
            track.VoiceColorExp.max = track.VoiceColorExp.options.Length - 1;

            var groups = BuildGroups(testCase.Notes);
            var timing = new TimeAxis();
            timing.BuildSegments(project);
            phonemizer.SetTiming(timing);
            phonemizer.SetUp(groups, project, track);

            var actual = new List<string>();
            for (var i = 0; i < groups.Length; i++) {
                var result = phonemizer.Process(
                    groups[i],
                    i > 0 ? groups[i - 1].First() : null,
                    i < groups.Length - 1 ? groups[i + 1].First() : null,
                    i > 0 ? groups[i - 1].First() : null,
                    i < groups.Length - 1 ? groups[i + 1].First() : null,
                    i > 0 ? groups[i - 1] : []);
                actual.AddRange(result.phonemes.Select(phoneme => phoneme.phoneme));
            }
            return new ClassicFixtureResult(
                testCase.Name,
                bank.Name,
                testCase.Expected,
                actual.ToArray(),
                testCase.Expected.SequenceEqual(actual),
                null);
        } catch (Exception ex) {
            while (ex is TargetInvocationException { InnerException: not null } tie) ex = tie.InnerException!;
            return new ClassicFixtureResult(
                testCase.Name, bank.Name, testCase.Expected, [], false,
                $"{ex.GetType().Name}: {ex.Message}");
        }
    }

    private static Phonemizer.Note[][] BuildGroups(InputNote[] notes) {
        var groups = new List<Phonemizer.Note[]>();
        var current = new List<Phonemizer.Note>();
        var position = 240;
        foreach (var input in notes) {
            if (!input.Lyric.StartsWith("+", StringComparison.Ordinal) && current.Count > 0) {
                groups.Add(current.ToArray());
                current.Clear();
            }
            current.Add(new Phonemizer.Note {
                lyric = input.Lyric,
                phoneticHint = input.Hint,
                duration = 240,
                position = position,
                tone = input.Tone,
                phonemeAttributes = [new Phonemizer.PhonemeAttributes {
                    index = 0,
                    consonantStretchRatio = 1,
                    voiceColor = input.Color,
                }],
            });
            position += 240;
        }
        if (current.Count > 0) groups.Add(current.ToArray());
        return groups.ToArray();
    }
}
