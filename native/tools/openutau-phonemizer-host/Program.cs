using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using OpenUtau.Api;
using OpenUtau.Classic;
using OpenUtau.Core;
using OpenUtau.Core.Format;
using OpenUtau.Core.Ustx;

// This process is a narrow adapter for Rust's immutable render snapshot. It
// executes OpenUtau's selected official Japanese Presamp plugin and returns
// only derived phoneme data; it never saves or mutates the project document.
Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
try {
    var request = JsonSerializer.Deserialize<Request>(await Console.In.ReadToEndAsync())
        ?? throw new InvalidOperationException("missing request");
    if (request.Protocol != 1) throw new InvalidOperationException($"unsupported protocol {request.Protocol}");
    if (request.Phonemizer != "OpenUtau.Plugin.Builtin.JapanesePresampPhonemizer") {
        throw new InvalidOperationException($"unsupported OpenUtau plugin '{request.Phonemizer}'");
    }
    if (!File.Exists(Path.Combine(request.VoicebankDirectory, "character.txt"))) {
        throw new InvalidOperationException("voicebank directory has no character.txt");
    }

    var pluginPath = Path.Combine(AppContext.BaseDirectory, "OpenUtau.Plugin.Builtin.dll");
    var plugin = Assembly.Load(await File.ReadAllBytesAsync(pluginPath));
    foreach (var type in plugin.GetExportedTypes().Where(t => !t.IsAbstract && typeof(Phonemizer).IsAssignableFrom(t))) {
        PhonemizerFactory.Get(type);
    }
    PhonemizerFactory.Get(typeof(DefaultPhonemizer));
    PhonemizerFactory.BuildList();

    var snapshotPath = Path.Combine(Path.GetTempPath(), $"rusted-utau-openutau-{Guid.NewGuid():N}.ustx");
    try {
        await File.WriteAllTextAsync(snapshotPath, request.ProjectYaml, new UTF8Encoding(false));
        var project = Ustx.Load(snapshotPath);
        if (request.Track < 0 || request.Track >= project.tracks.Count) throw new InvalidOperationException("track index is out of range");
        var voiceParts = project.parts.OfType<UVoicePart>().Where(p => p.trackNo == request.Track).ToArray();
        if (request.Part < 0 || request.Part >= voiceParts.Length) throw new InvalidOperationException("voice part index is out of range");
        var part = voiceParts[request.Part];
        var track = project.tracks[request.Track];
        var overriddenNote = part.notes.FirstOrDefault(n => !string.IsNullOrWhiteSpace(n.PhonemizerOverride)
            && !string.Equals(n.PhonemizerOverride, request.Phonemizer, StringComparison.OrdinalIgnoreCase));
        if (overriddenNote is not null) {
            throw new InvalidOperationException($"note-level phonemizer override '{overriddenNote.PhonemizerOverride}' is not supported by this host");
        }

        VoicebankLoader.IsTest = true;
        var bank = new Voicebank {
            File = Path.Combine(request.VoicebankDirectory, "character.txt"),
            BasePath = Directory.GetParent(request.VoicebankDirectory)?.FullName
                ?? throw new InvalidOperationException("voicebank directory has no parent"),
        };
        VoicebankLoader.LoadVoicebank(bank);
        var singer = new ClassicSinger(bank);
        singer.EnsureLoaded();
        track.Singer = singer;

        var color = project.expressions[Ustx.CLR].Clone();
        var colors = (singer.Subbanks ?? []).Select(s => s.Color).Distinct(StringComparer.Ordinal).OrderBy(s => s, StringComparer.Ordinal).ToArray();
        color.options = colors.Length > 0 ? colors : [""];
        color.max = color.options.Length - 1;
        track.VoiceColorExp = color;

        var groups = MakeGroups(part, track);
        var phonemizer = track.Phonemizer ?? throw new InvalidOperationException("OpenUtau did not resolve the selected phonemizer");
        if (phonemizer.GetType().FullName != request.Phonemizer) {
            throw new InvalidOperationException($"OpenUtau resolved '{phonemizer.GetType().FullName}', expected '{request.Phonemizer}'");
        }
        phonemizer.Testing = true;
        phonemizer.SetSinger(singer);
        phonemizer.SetTiming(project.timeAxis);
        phonemizer.SetUp(groups.Select(g => g.Notes).ToArray(), project, track);

        var phones = new List<Phone>();
        for (var groupIndex = 0; groupIndex < groups.Length; groupIndex++) {
            var current = groups[groupIndex];
            var previous = groupIndex > 0 ? groups[groupIndex - 1] : null;
            var next = groupIndex + 1 < groups.Length ? groups[groupIndex + 1] : null;
            var result = phonemizer.Process(
                current.Notes,
                previous?.Notes.LastOrDefault(),
                next?.Notes.FirstOrDefault(),
                previous is not null && previous.Notes[^1].position + previous.Notes[^1].duration >= current.Notes[0].position ? previous.Notes[^1] : null,
                next is not null && current.Notes[^1].position + current.Notes[^1].duration >= next.Notes[0].position ? next.Notes[0] : null,
                previous?.Notes ?? []);
            if (result.phonemes.Length == 0) {
                throw new InvalidOperationException($"Japanese Presamp returned no phones for note {current.NoteIndexes[0]}");
            }
            for (var i = 0; i < result.phonemes.Length; i++) {
                var p = result.phonemes[i];
                if (p.error is not null) throw new InvalidOperationException($"phonemizer error: {p.error.Message}");
                var rootNoteIndex = current.NoteIndexes[0];
                if (!singer.TryGetOto(p.phoneme, out _) && !singer.TryGetMappedOto(p.phoneme, current.Notes[0].tone, out _)) {
                    throw new InvalidOperationException($"phoneme alias '{p.phoneme}' has no OTO in the selected voicebank");
                }
                var extendedEnd = current.Notes[^1].position + current.Notes[^1].duration - part.position;
                phones.Add(new Phone(rootNoteIndex, p.phoneme, p.position, p.index ?? i, extendedEnd));
            }
        }
        var response = new Response(1, request.Track, request.Part, request.Phonemizer, phones.ToArray());
        Console.Out.Write(JsonSerializer.Serialize(response));
    } finally {
        try { File.Delete(snapshotPath); } catch { }
    }
} catch (Exception e) {
    Console.Error.WriteLine(e);
    Environment.ExitCode = 1;
}

static Group[] MakeGroups(UVoicePart part, UTrack track) {
    var notes = part.notes.OrderBy(n => n.position).ToArray();
    var groups = new List<Group>();
    for (var i = 0; i < notes.Length;) {
        var phonemizerNotes = new List<Phonemizer.Note>();
        var noteIndexes = new List<int>();
        do {
            phonemizerNotes.Add(ToPhonemizerNote(notes[i], track, part));
            noteIndexes.Add(i++);
        } while (i < notes.Length && notes[i].lyric.StartsWith("+", StringComparison.Ordinal)
            && notes[i - 1].position + notes[i - 1].duration == notes[i].position);
        groups.Add(new Group(phonemizerNotes.ToArray(), noteIndexes.ToArray()));
    }
    return groups.ToArray();
}

static Phonemizer.Note ToPhonemizerNote(UNote note, UTrack track, UVoicePart part) {
    string? hint = null;
    var lyric = Regex.Replace(note.lyric, @"\[(.*?)\]", m => { hint = m.Groups[1].Value; return string.Empty; }).Trim();
    var attrs = new Dictionary<int, Phonemizer.PhonemeAttributes>();
    foreach (var exp in note.phonemeExpressions) {
        var index = exp.index ?? 0;
        attrs.TryGetValue(index, out var attr);
        attr.index = index;
        if (exp.abbr == Ustx.VEL) attr.consonantStretchRatio = Math.Pow(2, 1.0 - exp.value / 100.0);
        else if (exp.abbr == Ustx.ALT && exp.value != 0) attr.alternate = (int)exp.value;
        else if (exp.abbr == Ustx.CLR && track.VoiceColorExp is not null) {
            var option = (int)exp.value;
            if (option >= 0 && option < track.VoiceColorExp.options.Length) attr.voiceColor = track.VoiceColorExp.options[option];
        } else if (exp.abbr == Ustx.SHFT) attr.toneShift = (int)exp.value;
        attrs[index] = attr;
    }
    return new Phonemizer.Note {
        lyric = lyric,
        phoneticHint = hint?.Trim() ?? string.Empty,
        tone = note.tone,
        position = part.position + note.position,
        duration = note.duration,
        phonemeAttributes = attrs.Values.ToArray(),
    };
}

sealed record Request(int Protocol, string ProjectYaml, string VoicebankDirectory, int Track, int Part, string Phonemizer);
sealed record Response(int Protocol, int Track, int Part, string Phonemizer, Phone[] Phones);
sealed record Phone(int NoteIndex, string Alias, int Position, int Index, int ExtendedEnd);
sealed record Group(Phonemizer.Note[] Notes, int[] NoteIndexes);
