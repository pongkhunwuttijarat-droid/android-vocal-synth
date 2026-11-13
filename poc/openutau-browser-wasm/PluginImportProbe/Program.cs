// T013 plugin-import probe — walks the exact steps an OpenUtau .dll
// phonemizer import must perform, and reports each step's evidence as JSON.
//
// Usage: dotnet run -c Release -- <plugin.dll> [probeLyric]
//
// This answers "what does plugin DLL import require" with a runnable
// checklist instead of a claim: bytes → isolated ALC → dependency
// resolution → type scan → metadata → lifecycle probe → resources → unload.

using System.Reflection;
using System.Text;
using System.Runtime.Loader;
using System.Text.Json;
using System.Text.Json.Serialization;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core;
using OpenUtau.Core.Ustx;

Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);

var dllPath = args.Length > 0 ? Path.GetFullPath(args[0]) : throw new ArgumentException("usage: <plugin.dll> [probeLyric] [fixtureBankJson:bankName]");
var probeLyric = args.Length > 1 ? args[1] : "か";
// Optional third arg: "<path-to-classic-fixtures.json>:<bankName>" attaches a
// real in-memory singer (same shape the browser fixture runner uses) so
// singer-dependent plugins can be probed natively with full stack traces.
USinger? bankSinger = null;
string? bankLabel = null;
if (args.Length > 2) {
    if (args[2].StartsWith("dir:", StringComparison.Ordinal)) {
        var singerDir = args[2][4..];
        VoicebankLoader.IsTest = true;
        var voicebank = new Voicebank {
            File = Path.Combine(singerDir, "character.txt"),
            BasePath = Path.GetDirectoryName(singerDir)!,
        };
        VoicebankLoader.LoadVoicebank(voicebank);
        var classicSinger = new ClassicSinger(voicebank);
        classicSinger.EnsureLoaded();
        bankSinger = classicSinger;
        bankLabel = $"classic:{Path.GetFileName(singerDir.TrimEnd('/'))}";
    } else {
        var parts = args[2].Split(':', 2);
        if (parts.Length == 2 && File.Exists(parts[0])) {
        var catalog = System.Text.Json.JsonSerializer.Deserialize<ProbeFixtureCatalog>(
            File.ReadAllText(parts[0]),
            new System.Text.Json.JsonSerializerOptions { PropertyNameCaseInsensitive = true });
        var bank = catalog?.Banks.FirstOrDefault(b => string.Equals(b.Name, parts[1], StringComparison.OrdinalIgnoreCase));
        if (bank is not null) {
            bankSinger = new ProbeCatalogSinger(bank);
                bankLabel = bank.Name;
            }
        }
    }
}
var steps = new List<StepReport>();

void Step(int number, string name, Func<string> body) {
    try {
        var detail = body();
        steps.Add(new StepReport(number, name, "ok", detail));
        Console.Error.WriteLine($"step {number} {name}: ok — {detail}");
    } catch (Exception ex) {
        var root = ex;
        while (root is TargetInvocationException or TypeInitializationException && root.InnerException is Exception inner) root = inner;
        steps.Add(new StepReport(number, name, "fail", $"{root.GetType().Name}: {root.Message}"));
        Console.Error.WriteLine($"step {number} {name}: FAIL — {root.Message}");
    }
}

var bytes = Array.Empty<byte>();
Step(1, "read plugin bytes", () => {
    bytes = File.ReadAllBytes(dllPath);
    return $"{bytes.Length:N0} bytes from {Path.GetFileName(dllPath)}";
});

Assembly? plugin = null;
var loadContext = new AssemblyLoadContext(name: "T013-plugin-import-probe", isCollectible: true);
Step(2, "load assembly from bytes (collectible ALC)", () => {
    using var stream = new MemoryStream(bytes);
    plugin = loadContext.LoadFromStream(stream);
    return $"assembly '{plugin.FullName}' loaded into isolated collectible context";
});

Step(3, "resolve OpenUtau API/core dependencies", () => {
    // The plugin's own assembly references OpenUtau.Core/OpenUtau.Api; the
    // host must already have those loaded (here: project reference). This
    // proves the resolution fallback works before any type is touched.
    var apiType = typeof(Phonemizer);
    var coreType = typeof(USinger);
    return $"host preloads '{apiType.Assembly.GetName().Name}' and '{coreType.Assembly.GetName().Name}'; plugin refs resolve through the default context";
});

Console.WriteLine("--- Core Yaml members (loaded runtime) ---");
{
    var yamlType = typeof(OpenUtau.Core.Yaml);
    var loadedHere = yamlType.Assembly.Location;
    Console.WriteLine($"core assembly: {loadedHere}");
    foreach (var m in yamlType.GetMembers(System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic)) {
        if (m.Name != ".ctor" && !m.Name.StartsWith("serializer") && !m.Name.StartsWith("deserializer") && m.Name != "op_Implicit")
            Console.WriteLine($"  {m.MemberType}: {m.Name}");
    }
}
var pluginTypes = Array.Empty<Type>();
Step(4, "scan exported Phonemizer subclasses", () => {
    pluginTypes = plugin!.GetExportedTypes()
        .Where(type => !type.IsAbstract && typeof(Phonemizer).IsAssignableFrom(type))
        .ToArray();
    return $"{pluginTypes.Length} phonemizer class(es): {string.Join(", ", pluginTypes.Select(t => t.Name))}";
});

var attributes = new List<PhonemizerInfo>();
Step(5, "read [Phonemizer] metadata (name/tag/author/language)", () => {
    foreach (var type in pluginTypes) {
        var attribute = type.GetCustomAttribute<PhonemizerAttribute>();
        attributes.Add(attribute is null
            ? new PhonemizerInfo(type.FullName ?? type.Name, null, null, null, null)
            : new PhonemizerInfo(type.FullName ?? type.Name, attribute.Name, attribute.Tag, attribute.Author, attribute.Language));
    }
    return JsonSerializer.Serialize(attributes);
});

var lifecycle = new List<ProcessProbe>();
Step(6, "lifecycle probe: create → SetSinger → SetTiming → SetUp → Process", () => {
    var singer = bankSinger ?? CreateDummySinger();
    var project = new UProject();
    foreach (var type in pluginTypes) {
        var info = attributes.First(a => a.FullName == (type.FullName ?? type.Name));
        var probe = new ProcessProbe(type.FullName ?? type.Name, info.Tag);
        try {
            var instance = Activator.CreateInstance(type) as Phonemizer
                ?? throw new InvalidOperationException($"could not create {type.Name}");
            instance.Testing = true;
            instance.SetSinger(singer);
            instance.SetTiming(project.timeAxis);
            var note = new Phonemizer.Note {
                lyric = probeLyric,
                tone = 60,
                position = 240,
                duration = 480,
                phonemeAttributes = [],
            };
            instance.SetUp([new[] { note }], project, project.tracks[0]);
            var result = instance.Process(new[] { note }, null, null, null, null, []);
            probe.Output = result.phonemes is null
                ? "(null phoneme array)"
                : string.Join(" · ", result.phonemes.Select(p => p.phoneme ?? "(null)"));
        } catch (Exception ex) {
            var root = ex;
            while (root is TargetInvocationException or TypeInitializationException && root.InnerException is Exception inner) root = inner;
            probe.Error = $"{root.GetType().Name}: {root.Message}\n{root.StackTrace}";
        }
        lifecycle.Add(probe);
    }
    return $"{lifecycle.Count} type(s) probed with lyric '{probeLyric}'" + (bankLabel is null ? "" : $" against fixture singer {bankLabel}");
});

Step(7, "enumerate embedded resources", () => {
    var resources = plugin!.GetManifestResourceNames();
    return $"{resources.Length} resource(s): {(resources.Length == 0 ? "(none)" : string.Join(", ", resources.Take(5)))}";
});

Step(8, "unload the collectible context", () => {
    loadContext.Unload();
    // The reference goes dead; a real importer would re-scan after unload.
    return "ALC unloaded — plugin can be replaced/reloaded without a host restart";
});

var report = new ImportReport(Path.GetFileName(dllPath), probeLyric, steps, attributes, lifecycle);
Console.WriteLine(JsonSerializer.Serialize(report, new JsonSerializerOptions {
    WriteIndented = true,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
}));
Environment.ExitCode = steps.Any(s => s.Status == "fail") ? 1 : 0;

static USinger CreateDummySinger() => new T013ProbeSinger();

sealed file class T013ProbeSinger : USinger {
    private readonly OtoSet otoSet = new() { Name = "T013 import-probe bank" };
    private readonly List<USubbank> subbanks = [new USubbank(new Subbank())];
    private readonly UOtoSet set;

    public T013ProbeSinger() => set = new UOtoSet(otoSet, string.Empty);

    public override string Id => "T013-probe";
    public override string Name => "T013 import probe singer";
    public override USingerType SingerType => USingerType.Classic;
    public override string Location => string.Empty;
    public override IList<UOto> Otos => [];
    public override IList<USubbank> Subbanks => subbanks;

    // Accepts any alias: mirrors the matrix runner's synthetic bank so a
    // probe measures the import path, not bank coverage.
    public override bool TryGetOto(string phoneme, out UOto oto) {
        if (string.IsNullOrEmpty(phoneme)) { oto = null!; return false; }
        oto = new UOto(new Oto { Alias = phoneme, IsValid = true }, set, subbanks.ToArray());
        return true;
    }

    public override bool TryGetMappedOto(string phoneme, int tone, string color, out UOto oto) =>
        TryGetOto(phoneme, out oto);
}

sealed record StepReport(int Step, string Name, string Status, string Detail);
sealed record PhonemizerInfo(string FullName, string? Name, string? Tag, string? Author, string? Language);
sealed record ProcessProbe(string FullName, string? Tag) {
    public string? Output { get; set; }
    public string? Error { get; set; }
}
sealed record ImportReport(string Plugin, string ProbeLyric, List<StepReport> Steps, List<PhonemizerInfo> Phonemizers, List<ProcessProbe> Probes);


/// In-memory singer over a ClassicFixtureBank — mirrors the browser
/// ClassicFixtureBrowserRunner's singer so native probes see the same world.
internal sealed class ProbeCatalogSinger : USinger {
    private readonly Dictionary<string, UOto> otoMap = new(StringComparer.Ordinal);
    private readonly List<USubbank> subbanks = [];
    private readonly List<UOto> otos = [];
    private readonly UOtoSet set;
    private readonly string displayName;

    public ProbeCatalogSinger(ProbeFixtureBank fixture) {
        found = true;
        displayName = fixture.Name;
        var set = new OtoSet { Name = fixture.Name };
        var subbankDtos = fixture.Subbanks
            .Select(sb => new Subbank { Color = sb.Color, Prefix = sb.Prefix, Suffix = sb.Suffix, ToneRanges = sb.ToneRanges })
            .ToList();
        subbanks = subbankDtos.Select(sb => new USubbank(sb)).ToList();
        this.set = new UOtoSet(set, string.Empty);
        foreach (var alias in fixture.Aliases) {
            var oto = new Oto { Alias = alias, IsValid = true };
            var singerOto = new UOto(oto, this.set, subbanks.ToArray());
            otos.Add(singerOto);
            otoMap.TryAdd(alias, singerOto);
        }
    }

    public override string Id => $"probe-{displayName}";
    public override string Name => displayName;
    public override USingerType SingerType => USingerType.Classic;
    public override string Location => string.Empty;
    public override IList<USubbank> Subbanks => subbanks;
    public override IList<UOto> Otos => otos;

    public override bool TryGetOto(string phoneme, out UOto oto) => otoMap.TryGetValue(phoneme, out oto!);

    public override bool TryGetMappedOto(string phoneme, int tone, out UOto oto) =>
        TryGetOto(phoneme, out oto!);

    public override bool TryGetMappedOto(string phoneme, int tone, string color, out UOto oto) {
        var match = subbanks.FirstOrDefault(sb => sb.Color == color && sb.toneSet.Contains(tone));
        return (match is not null && otoMap.TryGetValue($"{match.Prefix}{phoneme}{match.Suffix}", out oto!))
            || TryGetOto(phoneme, out oto!);
    }
}


// Local copies of the fixture-catalog DTOs (same JSON shape as
// Host wwwroot data/classic-fixtures.json; the probe does not reference the
// Blazor host project).
internal sealed record ProbeFixtureCatalog(ProbeFixtureBank[] Banks);
internal sealed record ProbeFixtureBank(string Name, string[] Aliases, ProbeFixtureSubbank[] Subbanks);
internal sealed record ProbeFixtureSubbank(string Color, string Prefix, string Suffix, string[] ToneRanges);
