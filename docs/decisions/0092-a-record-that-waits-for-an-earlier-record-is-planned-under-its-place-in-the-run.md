# 0092. A record that waits for an earlier record is planned under its place in the run

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the sentence of spec section 209 that said the diff hash uses the parent's intended identity.

## Context

Spec section 209 has a header followed by its lines as the most common pipeline: the system assigns the header's key, and each line needs it. The lines must be in the diff a person approves, and the ledger gives every record its key when the preview plans it. At that moment the header does not exist, so a line's own key cannot be worked out.

The first pass of the engine worked every record out before the first write, so a pipeline of this shape could not run at all.

## Decision

- A write step's result is there for the steps after it once every one of its records is verified or unchanged: at the preview when the records already exist, in the write path as soon as the step's last record is done, and on resume from the ledger.
- A `with` expression that reads `steps` and cannot be worked out while an earlier record of the run is still to be written marks its input as **waiting**.
- Only an input that is part of the record's key may wait. A value that is written must be known at the preview, because a person approves it; a pipeline that makes one wait is refused with `input`, `input_invalid`, before anything is written.
- A record with a waiting input is a `create` in the diff, with every value it will write. It is not looked up: it cannot exist before its parent does.
- It is planned under **its place in the run**: the operation's name, the step's position, and the record's position among the step's records. That is the same key a record of an operation with no key input has always had. The ledger row, the lock, the intent token, and the diff use it, from the preview to the end of the run.
- When the write path reaches the record, its inputs are worked out from the earlier record's outputs, the values it writes are checked against what was approved, and it is written and read back as any record, by its real inputs.
- When a record fails, the records still waiting for it are marked `failed` with `busy`, `blocked_by_parent`, and the run stops. On resume each goes back to `planned` without a read: nothing was ever sent for it.

## Alternatives

- **The parent's key, then the line's own parts,** as section 209 had it. The engine would have to know which earlier record an expression depends on, which the plan does not state and an expression's names do not show; a wrong guess would give two lines one key. The place in the run is unique by construction and needs no such knowledge.
- **A stand-in for the parent's result,** filled into the expression so that the line's inputs can be worked out at the preview. A stand-in is a made-up value that looks like a real one; it could reach a lookup, or a diff. Unknown means stop.
- **Planning the lines after the header is written.** The approval is bound to one diff, stored while the run is `validating`; lines added later would not be covered by it.

## Consequences

- One approval covers header and lines, and no identifier that has not been assigned is in the diff or its hash.
- A waiting record's lock is on its place in the run, not on the key it gets once its parent exists. Two runs cannot both be creating lines under the same new header, because the header is new to each; this is an open point of the engine's design all the same.
- A driver that takes the record's key from the step and not from the step's arguments is handed the place in the run for such a record. A binding of an operation used this way must pass the key's parts as arguments.
- The compiler should refuse a write step that makes a written value wait, so that the refusal comes at publish and not at the first preview. It does not yet.
