# 209. Record identity and dependent records

The ledger's guarantees rest on being able to find a record again: to see whether a write landed, and to avoid making it twice. Section 208 assumed the caller supplies a key. Many systems do not work that way: they assign the identifier themselves when a record is created, and other records then depend on it. This section extends the design to cover those cases, which include the most common pipeline of all, a header followed by its lines. Built today (package 3.8).

## The problem

| Case | Example | Why section 208 alone is not enough |
| --- | --- | --- |
| The system assigns the identifier | Creating an order returns order number 4471 | Before the create there is no key to reconcile by |
| A record is identified by several values together | A line is unique by order and line code | One input is not a key |
| Later records need an earlier record's identifier | Lines need the new order's number | A resumed run must not create the order again to learn it |

Without an answer, every create would have to be classed `irreversible` and never resumed. That would make the system safe and nearly useless for data entry.

## Identity strategies

Every write operation declares how its record is found again. The compiler refuses a write operation with no strategy.

| Strategy | How the record is found | Use when |
| --- | --- | --- |
| `natural` | By an input that is the system's own key; or, with `place: fixed`, by the one place every binding names in the target's files | The caller knows the key: an existing record, or a key the caller chooses; or the record is one fixed place, such as a value of a settings file |
| `composite` | By several inputs that are unique together, within a parent | Lines, rows, members of something |
| `stamped` | By a token Anything2MCP writes into a field of the record, such as a reference or external-id field | The system assigns the key and has a spare field that is searchable |
| `find` | By a declared lookup on values the write itself sets, which must match exactly one record | The system assigns the key and has no spare field |
| `none` | It cannot be | Nothing above is possible; the operation is `irreversible` |

**Capturing the assigned key.** When the system returns the new identifier in its response, the engine stores it in the ledger at once, with the `acted` mark. From then on the record is found by that identifier. `stamped` and `find` exist for the gap before that: a stop after the write was sent and before its response was recorded.

**Values known before the commit point.** Some systems hand out the identifier before the write commits: a data store returns the assigned key inside an open transaction, and some report a transaction's identifier before it ends. A step's outcome may carry such values, and the engine records them in the ledger before the step that commits is acted. A lost answer to the commit is then reconciled by the captured value ([section 307](../3-packs/307-the-data-pack.md)). The later steps of the record, its read-back, its lookup, and the read at reconcile, are handed what was recorded ([decision 0162](../../decisions/0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md)).

**A key is never guessed.** A step is handed the record's key in the system: its key inputs, or the key the system assigned. A record's place in the run is the engine's name for it and is never handed to a step; a step of a record that has no key in the system yet is handed none, and a driver that needs one stops by its own name ([decision 0161](../../decisions/0161-the-plan-and-the-engine-after-the-packs-were-built.md)).

**One fixed place.** A write that sets one value at a place the target's files fix, such as a value of a settings file, a record at a keyless path of a JSON file, or a named cell, has no input to name its record, and none may: an input that the binding never uses would be a false key in the tool's schema. Such an operation declares `identity: {strategy: natural, place: fixed}` and no identity input. It compiles only where each binding's lowering names the place it reads and writes, every binding names the same one, and the file kind whose document holds it declares it fixed for that document on the binding's surface: one record that always exists, or that a read says does not, and that no input chooses. A kind whose places an input chooses, such as an entry of a folder by its name, declares none. A duplicate is impossible by construction, and resume reads that one place by the binding's read-back, as for any `natural` record. The compiled target holds such an operation as `natural` with an empty key; the record's key in the ledger is the engine's name for it, the operation's name and its place in the run, which the target's files give and no input does. The place's own name is not yet in the compiled target, so two operations that write one place do not share a lock ([decision 0252](../../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)).

**Rules for `find`.**

- Zero matches means the write did not land.
- One match means it did; the engine captures the identifier and continues to read-back.
- More than one match stops the run with `ambiguous_match`. The pack's lookup step sees the matches and reports it; the engine never picks one.
- The lookup must filter on values specific enough to be unique in practice; the linter warns when it filters on fewer than two fields besides the parent.

**Rules for `stamped`.**

- The token is the record's idempotency token from the ledger, in a short form.
- No record can carry the token before the run has written one, so at the first preview a stamped record is classed `create` without a lookup. The lookup by the token is made from then on: at the freshness check, and on resume.
- What carries the stamp is whatever the system offers for a reference: a field of a form in the web pack, a column the target declares in the data pack, an argument the program takes in the command pack ([section 306](../3-packs/306-the-command-pack.md)). Each pack's binding marks the element that carries it, and a declared read must be able to find the record by it.
- The stamp must be carried in every binding of the operation, which the compiler checks.
- The stamp stays in the record. Where that is unwelcome, use `find`.

## Create or update is decided by reading

An upsert is not one action; it is a read followed by a decision. During the preview the engine looks the record up by its strategy and classes it as `create`, `update`, or `unchanged`. That class is part of the diff the person approves, so nobody approves "update" and gets "create".

## Dependent records

A pipeline step can use an earlier step's result:

```yaml
steps:
  - {id: header, operation: example_order_create, with: {customer: input.customer}}
  - {id: lines, for_each: source.rows, operation: example_line_item_upsert,
     with: {order_id: steps.header.result.order_id, item_code: row.code, length: row.length},
     checkpoint: per_record}
```

- **Results are kept in the ledger.** Each record's outputs, including an assigned key, are stored with its row when its write is marked `acted`. On resume, the line steps read the order number from the ledger. The header step is not run again. A header that was found unchanged, or that reconcile found had landed, has no outputs in its row and is read once more instead.
- **Order follows the steps.** A step that uses another step's result runs after it. The compiler rejects a cycle.
- **A failed parent blocks its children.** They are marked `failed` with the code `blocked_by_parent`, and the run stops. On resume they go back to `planned` without a read, because nothing was sent for them.
- **The preview shows the whole structure.** Lines for an order that does not exist yet are shown after the proposed order, each a `create` that is not looked up. Such a line is planned under its place in the run, never under an identifier that has not been assigned, so one approval covers header and lines. Only an input that is part of the line's key may wait for the header; a value that is written must be known at the preview ([decision 0092](../../decisions/0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md)).

## One save that carries many records

Some systems save a header and its lines in a single commit: one form with a grid, or one request with a nested list. That is one commit point and one ledger row. Read-back compares the nested lines by their own record matching, so a line the system dropped or reordered is caught as a `mismatch`.

## What this adds elsewhere

| Where | Addition |
| --- | --- |
| Operation files | An `identity` field naming the strategy and its fields or lookup |
| Ledger | For each record: the assigned key, once known, values known before the commit point, and its stored outputs |
| Compiler | Checks the strategy exists, is possible in every binding, and that dependencies have no cycle |
| Failure codes | `ambiguous_match` under `possible_commit`; `blocked_by_parent` as a skip reason |
| Publish gates | Create the same record twice and find one; stop between sending a create and recording it, resume, and find one |
| Undo | Removes dependent records before the record they depend on, where delete bindings exist |

## What it does not solve

- A system with no spare field and no values unique enough to search on. Creates there stay `irreversible`, approved one run at a time and never resumed automatically.
- A lookup that is slow or eventually consistent, so a record just written does not appear yet. The binding declares a settle time for its lookup; within it, zero matches means "not known yet" and the run waits.
- Changing a record's key. That is a different record to the ledger; an operation that renames keys is `irreversible`.
