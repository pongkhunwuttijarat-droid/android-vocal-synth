# T018 per-track Mix FX proposal — consolidated

**Status:** Superseded as a separate contract draft. The per-track `UMixFx`
contract is incorporated into the canonical combined draft:
[CONTRACT-MK3 §9.3](contract-mk3-proposal.md#93-legacy-ustx-mixfx-contract-merged-from-the-t018-extension).

That section defines the read/write value, validation, revision and invalidation
semantics, USTX persistence, stereo requirement, and legacy EQ → compressor →
reverb order. The new mixer-insert chain remains a distinct model with its own
placement and lifecycle. CONTRACT-v1 §6 remains frozen, and the combined draft
has not been approved or frozen.

T018's remaining implementation and parity work is tracked in
[the T018 task record](../work/tasks/T018-openutau-flags-and-track-fx.md).
Per-note vibrato and renderer-flag work are not part of this mixer contract.
