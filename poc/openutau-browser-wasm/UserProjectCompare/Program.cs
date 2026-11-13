using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Reflection;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core;
using OpenUtau.Core.Format;
using OpenUtau.Core.Ustx;

Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
var builtinDllPath = Path.Combine(AppContext.BaseDirectory, "OpenUtau.Plugin.Builtin.dll");
var builtinAssembly = Assembly.Load(await File.ReadAllBytesAsync(builtinDllPath));
var builtinPhonemizers = builtinAssembly.GetExportedTypes()
    .Where(type => !type.IsAbstract && typeof(Phonemizer).IsAssignableFrom(type))
    .ToArray();
foreach (var type in builtinPhonemizers) {
    PhonemizerFactory.Get(type);
}
PhonemizerFactory.Get(typeof(DefaultPhonemizer));
PhonemizerFactory.BuildList();
Console.WriteLine($"Loaded official plugin DLL from bytes: {builtinAssembly.GetName().Name}; discovered {builtinPhonemizers.Length} phonemizer types");

var repo = FindRepositoryRoot();
var projectPath = args.Length > 0
    ? Path.GetFullPath(args[0])
    : Path.Combine(repo, "test-ustx-example/king.ustx");
var singerArgument = args.Length > 1
    ? args[1]
    : "/tmp/t013-teto-bank/重音テト OU用日本語統合ライブラリー";
var outputPath = args.Length > 2
    ? Path.GetFullPath(args[2])
    : Path.Combine(repo, "poc/openutau-browser-wasm/artifacts/king-openutau-presamp.json");

USinger singer;
if (singerArgument == "-") {
    singer = USinger.CreateMissing("unbound project track");
} else {
    var singerDirectory = Path.GetFullPath(singerArgument);
    VoicebankLoader.IsTest = true;
    var voicebank = new Voicebank {
        File = Path.Combine(singerDirectory, "character.txt"),
        BasePath = Directory.GetParent(singerDirectory)!.FullName,
    };
    VoicebankLoader.LoadVoicebank(voicebank);
    var classicSinger = new ClassicSinger(voicebank);
    classicSinger.EnsureLoaded();
    singer = classicSinger;
}

var project = Ustx.Load(projectPath);
var partReports = new List<PartReport>();
foreach (var part in project.parts.OfType<UVoicePart>().OrderBy(part => part.trackNo).ThenBy(part => part.position)) {
    if (part.trackNo < 0 || part.trackNo >= project.tracks.Count) continue;
    var track = project.tracks[part.trackNo];
    var descriptor = project.expressions[Ustx.CLR];
    track.VoiceColorExp = descriptor.Clone();
    var voiceColors = (singer.Subbanks ?? [])
        .Select(subbank => subbank.Color)
        .Distinct(StringComparer.Ordinal)
        .OrderBy(color => color, StringComparer.Ordinal)
        .ToArray();
    track.VoiceColorExp.options = voiceColors.Length > 0 ? voiceColors : [""];
    track.VoiceColorExp.max = track.VoiceColorExp.options.Length - 1;

    var groups = MakeGroups(part, track);
    var phonemizer = track.Phonemizer;
    if (phonemizer is null) {
        throw new InvalidOperationException($"Track {part.trackNo} has no phonemizer after USTX loading.");
    }
    phonemizer.Testing = true;
    phonemizer.SetSinger(singer);
    phonemizer.SetTiming(project.timeAxis);
    phonemizer.SetUp(groups, project, track);

    var outputs = new List<PhoneReport>();
    for (var i = 0; i < groups.Length; i++) {
        var current = groups[i];
        var previousGroup = i > 0 ? groups[i - 1] : [];
        var nextGroup = i + 1 < groups.Length ? groups[i + 1] : [];
        Phonemizer.Note? previous = previousGroup.Length > 0 ? previousGroup[^1] : null;
        Phonemizer.Note? next = nextGroup.Length > 0 ? nextGroup[0] : null;
        var previousNeighbour = previousGroup.Length > 0
            && previousGroup[^1].position + previousGroup[^1].duration >= current[0].position
                ? previous
                : null;
        var nextNeighbour = nextGroup.Length > 0
            && current[^1].position + current[^1].duration >= nextGroup[0].position
                ? next
                : null;
        var result = phonemizer.Process(
            current,
            previous,
            next,
            previousNeighbour,
            nextNeighbour,
            previousGroup);
        outputs.AddRange(result.phonemes.Select((phone, index) => new PhoneReport(
            phone.phoneme,
            phone.position,
            phone.index ?? 0,
            singer.TryGetOto(phone.phoneme, out _)
                || singer.TryGetMappedOto(phone.phoneme, current[0].tone, out _))));
    }

    var noteCount = part.notes.Count;
    partReports.Add(new PartReport(
        part.name,
        track.singer ?? singer.Name,
        phonemizer.GetType().FullName!,
        noteCount,
        groups.Length,
        outputs.Count,
        outputs.Count(phone => !phone.HasOto),
        outputs.Take(24).Select(phone => phone.Alias).ToArray(),
        part.notes.OrderBy(note => note.position)
            .Select(note => new NoteReport(note.lyric, note.tone, part.position + note.position, note.duration))
            .ToArray(),
        outputs.ToArray()));
}

var report = new ProjectReport(
    Path.GetFileName(projectPath),
    project.tempos.FirstOrDefault()?.bpm ?? 0,
    project.tracks.Count,
    partReports.Sum(part => part.Notes),
    partReports.Sum(part => part.Phones),
    partReports.Sum(part => part.MissingOto),
    partReports.ToArray());
Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
await File.WriteAllTextAsync(outputPath, JsonSerializer.Serialize(report, new JsonSerializerOptions {
    WriteIndented = true,
}));

Console.WriteLine($"Project: {report.Project}; {report.Tracks} tracks; {report.Notes} notes");
Console.WriteLine($"OpenUtau selected-phonemizer phones: {report.Phones}; aliases with no Oto in supplied bank: {report.MissingOto}");
foreach (var part in partReports) {
    Console.WriteLine($"Part {part.Name} ({part.Phonemizer}): {part.Notes} notes -> {part.Phones} phones; missing Oto {part.MissingOto}");
    Console.WriteLine($"  first aliases: {string.Join(" | ", part.FirstAliases)}");
}
Console.WriteLine($"JSON: {outputPath}");

static Phonemizer.Note[][] MakeGroups(UVoicePart part, UTrack track) {
    var groups = new List<Phonemizer.Note[]>();
    var notes = part.notes.OrderBy(note => note.position).ToArray();
    for (var i = 0; i < notes.Length;) {
        var group = new List<UNote> { notes[i++] };
        while (i < notes.Length
            && notes[i].lyric.StartsWith("+", StringComparison.Ordinal)
            && group[^1].position + group[^1].duration == notes[i].position) {
            group.Add(notes[i++]);
        }
        groups.Add(group.Select(note => ToPhonemizerNote(note, track, part)).ToArray());
    }
    return groups.ToArray();
}

static Phonemizer.Note ToPhonemizerNote(UNote note, UTrack track, UVoicePart part) {
    string? hint = null;
    var lyric = Regex.Replace(note.lyric, @"\[(.*?)\]", match => {
        hint = match.Groups[1].Value;
        return string.Empty;
    }).Trim();
    var attributes = new Dictionary<int, Phonemizer.PhonemeAttributes>();
    foreach (var expression in note.phonemeExpressions) {
        if (expression.abbr != Ustx.VEL && expression.abbr != Ustx.ALT
            && expression.abbr != Ustx.CLR && expression.abbr != Ustx.SHFT) continue;
        var index = expression.index ?? 0;
        attributes.TryGetValue(index, out var attr);
        attr.index = index;
        if (expression.abbr == Ustx.VEL) {
            attr.consonantStretchRatio = Math.Pow(2, 1.0 - expression.value / 100.0);
        } else if (expression.abbr == Ustx.ALT && expression.value != 0) {
            attr.alternate = (int)expression.value;
        } else if (expression.abbr == Ustx.CLR && track.VoiceColorExp is not null) {
            var option = (int)expression.value;
            if (option >= 0 && option < track.VoiceColorExp.options.Length) {
                attr.voiceColor = track.VoiceColorExp.options[option];
            }
        } else if (expression.abbr == Ustx.SHFT) {
            attr.toneShift = (int)expression.value;
        }
        attributes[index] = attr;
    }
    return new Phonemizer.Note {
        lyric = lyric,
        phoneticHint = hint?.Trim() ?? string.Empty,
        tone = note.tone,
        position = part.position + note.position,
        duration = note.duration,
        phonemeAttributes = attributes.Values.ToArray(),
    };
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

sealed record PhoneReport(string Alias, int Position, int Index, bool HasOto);
sealed record PartReport(
    string? Name,
    string Singer,
    string Phonemizer,
    int Notes,
    int Groups,
    int Phones,
    int MissingOto,
    string[] FirstAliases,
    NoteReport[] Input,
    PhoneReport[] Output);
sealed record NoteReport(string Lyric, int Tone, int Position, int Duration);
sealed record ProjectReport(
    string Project,
    double Bpm,
    int Tracks,
    int Notes,
    int Phones,
    int MissingOto,
    PartReport[] Parts);
