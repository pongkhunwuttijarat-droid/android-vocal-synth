// Shared in-memory USinger over a ClassicFixtureBank (the checked-in
// fixture catalogs carry real bank aliases as data, so browser WASM can
// give phonemizers a singer without any filesystem). Used by the classic
// fixture runner, the official matrix (singer-backed cases) and the
// generic ?dll= loader.

using System.Text.RegularExpressions;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core.Ustx;

namespace OpenUtauBrowserWasmHost;

internal sealed class CatalogSinger : USinger {
    private readonly Dictionary<string, UOto> otoMap = new(StringComparer.Ordinal);
    private readonly List<USubbank> subbanks;
    private readonly List<UOto> otos = [];
    private readonly UOtoSet set;
    private readonly KeyValuePair<Regex, USubbank[]>[] groups;
    private readonly USubbank[] dummySubbanks;
    private readonly string displayName;

    public CatalogSinger(ClassicFixtureBank fixture) {
        found = true;
        displayName = fixture.Name;
        var sourceSubbanks = fixture.Subbanks
            .Select(item => new Subbank {
                Color = item.Color,
                Prefix = item.Prefix,
                Suffix = item.Suffix,
                ToneRanges = item.ToneRanges,
            })
            .ToList();
        subbanks = sourceSubbanks
            .OrderByDescending(item => item.Prefix.Length + item.Suffix.Length)
            .Select(item => new USubbank(item))
            .ToList();
        groups = subbanks
            .GroupBy(subbank => $"^{Regex.Escape(subbank.Prefix)}(.*){Regex.Escape(subbank.Suffix)}$")
            .Select(group => new KeyValuePair<Regex, USubbank[]>(
                new Regex(group.Key), group.ToArray()))
            .ToArray();
        dummySubbanks = [new USubbank(new Subbank())];

        var otoSet = new OtoSet { Name = displayName };
        set = new UOtoSet(otoSet, string.Empty);
        foreach (var alias in fixture.Aliases) {
            var mapped = groups.FirstOrDefault(group => group.Key.IsMatch(alias)).Value ?? dummySubbanks;
            var oto = new Oto { Alias = alias, IsValid = true };
            var singerOto = new UOto(oto, set, mapped);
            otos.Add(singerOto);
            otoMap.TryAdd(alias, singerOto);
        }
    }

    public override string Id => $"t013-fixture-{displayName}";
    public override string Name => displayName;
    public override USingerType SingerType => USingerType.Classic;
    public override string Location => string.Empty;
    public override IList<USubbank> Subbanks => subbanks;
    public override IList<UOto> Otos => otos;

    public override bool TryGetOto(string phoneme, out UOto oto) => otoMap.TryGetValue(phoneme, out oto!);

    public override bool TryGetMappedOto(string phoneme, int tone, out UOto oto) {
        var subbank = subbanks.FirstOrDefault(item => string.IsNullOrEmpty(item.Color) && item.toneSet.Contains(tone));
        return (subbank is not null && otoMap.TryGetValue($"{subbank.Prefix}{phoneme}{subbank.Suffix}", out oto!))
            || TryGetOto(phoneme, out oto!);
    }

    public override bool TryGetMappedOto(string phoneme, int tone, string color, out UOto oto) {
        var subbank = subbanks.FirstOrDefault(item => item.Color == color && item.toneSet.Contains(tone));
        return (subbank is not null && otoMap.TryGetValue($"{subbank.Prefix}{phoneme}{subbank.Suffix}", out oto!))
            || TryGetMappedOto(phoneme, tone, out oto!);
    }
}
