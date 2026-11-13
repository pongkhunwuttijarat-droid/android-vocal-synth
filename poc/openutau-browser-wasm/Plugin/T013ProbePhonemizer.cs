using OpenUtau.Api;
using OpenUtau.Core.Ustx;

namespace T013.OpenUtauProbe.Plugin;

[Phonemizer("T013 Browser WASM Probe", "T013 PROBE", "Rusted UTAU", language: "en")]
public sealed class T013ProbePhonemizer : Phonemizer {
    public override void SetSinger(USinger singer) { }

    public override Result Process(
        Note[] notes,
        Note? prev,
        Note? next,
        Note? prevNeighbour,
        Note? nextNeighbour,
        Note[] prevs) {
        var note = notes[0];
        return new Result {
            phonemes = [new Phoneme {
                index = 0,
                phoneme = $"wasm:{note.lyric}",
                position = note.position,
                expressions = [],
            }],
        };
    }
}
