name: identity

# Identity and read-back

Every write must be checkable. Two things make it so: a way to find the record again, and a way to read it back.

## Finding the record again

A write operation declares one strategy. The compiler refuses a write with none.

| Strategy | The record is found by | Use when |
| --- | --- | --- |
| `natural` | An input that is the system's own key | The caller knows the key |
| `composite` | Several inputs, unique together, parent first | Lines, rows, members of something |
| `stamped` | A token the runtime writes into a field of the record | The system assigns the key and has a searchable spare field |
| `find` | A declared lookup on the values the write sets. It must match exactly one record | The system assigns the key and has no spare field |
| `none` | It cannot be found | Nothing else works. The operation is `irreversible` |

Short form: `key: order_id` is `natural`. `key: [order_id, item_code]` is `composite`. A command-pack operation writes `identity: {strategy: stamped}`. The long forms of `stamped` and `find` are written with the pack that needs them.

- If the system returns the new key, the engine records it at once. From then on the record is found by it.
- `find` with more than one match stops the run as `possible_commit`, `ambiguous_match`. The engine never picks one.
- The stamp stays in the record, and every binding of the operation must carry it.
- Create or update is decided by reading. The preview classes each record `create`, `update`, or `unchanged`, and the person approves that class.

## Read-back

A write binding names a `read_back`: a read that returns the record as the system now holds it.

- It must work from the identity inputs alone, before any write.
- It must report a missing record as absent, not as a failure.
- The engine normalizes both sides with the field types, then compares. A difference is `mismatch`, `read_back_differs`.
- Never use a read that changes what it reads, such as a register that clears when read.
- Where the read-back cannot be declared, the write is `irreversible` and a person approves each run.

Where the read-back is written depends on the mode: `read_back: {call: ...}`, `{screen: ...}`, `{command: ..., key: [...]}`, a statement name, `{record: ...}`, or `{channel: ...}`. See the pack's page.

## Intent comes first

Before acting, the engine records its intent in the ledger. After a stop, a resume reconciles: it reads the record to see whether the write landed, and only then writes. No ledger, no write. Calling `run.resume` does not write twice: it reconciles first.
