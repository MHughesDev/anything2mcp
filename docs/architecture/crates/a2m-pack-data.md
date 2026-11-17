# `a2m-pack-data`

The data pack: the `sql` mode over relational stores. It brings the `database` surface kind, the `statements/` file kind, the lowering of six verbs into five steps, the driver of those steps, a discoverer that reads a store's own catalog, and a drafting rule from a table to a statements file. Statements are constants of the plan; a value from a tool input reaches the store only as a bound parameter; a read runs only on a connection the store itself holds read-only.

The first and only store built is the embedded SQL engine, through `rusqlite`. Everything that is specific to it is behind the pack's own small interface, so that a server store can be added beside it.

## Facts

| | |
| --- | --- |
| Side | Pack: data |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (`Plan`, `Step`, `Value`, `Failure`, `Finding`), `a2m-files` (file-kind, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, in the driver's signature only), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`, `CheckedPath`), `a2m-telemetry` (`Acting`, `Clock`). Outside: `rusqlite`, the workspace's one binding to the embedded engine; `serde`, `serde_json`, `tokio`. Tests also use `a2m-expr`, `a2m-ledger`, `a2m-vault`, and the stand `example-db` |
| Built in | Packages G2 and G3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [307](../../spec/3-packs/307-the-data-pack.md); [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input changes a statement); [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (set, read back, go on only if it took) |
| Decisions | [0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md), [0101](../../decisions/0101-what-the-data-pack-refuses-and-how-it-names-it.md) |

## Owns

- The `statements/` file kind: its schema, the names it defines, and its compilation into this crate's own section of the plan, an opaque payload that only this crate decodes.
- Writing a `form: table` statement from a record's declaration, with every identifier quoted by the store's dialect.
- The lowering of `read`, `list`, `search`, `create`, `update`, and `delete` into `begin`, `execute`, `fetch`, `expect_rows`, and `commit`.
- Running a statement: binding step arguments to parameters, holding it to its time limit and its row and byte limits, and turning stored values into typed values.
- Proving a read is a read: opening the read-only connection, and reading the setting back before the first statement.
- Confining a write: the full key predicate, the count of rows changed checked inside the transaction, and the check of triggers and cascading keys before the write is sent.
- Saying which side of `commit` a failure fell on.
- The `table` finding kind and the schema of its `found`; reading the catalog; drafting from it.

It must never: place a value from an input into a statement's text; let an input choose a table, a column, or a file; run a statement the plan does not hold; retry; write through a connection the store holds read-only, or read through one it does not; commit a transaction whose count of rows was not checked; use a connection again after a commit whose answer was lost; read or count rows during discovery; draft a write.

## What it registers

One function hands the pack to the registry builder: this crate's `pack()` returns a `DataPack`, which implements `Pack`. The manifest is `anything2mcp-provider.toml` in the crate, embedded with `include_str!`.

| Registration | Content |
| --- | --- |
| Mode | `sql`, archetype request, rank 30, four reads and one write in a session, a write holds the surface |
| Surface kind | `database`, with a session. Fields: `store` (required; `sqlite` is the one built), `file` (the address: the path of the store's file below its root, with forward slashes), `root` (the named root; `project` unless given) |
| Step kinds | `begin`, `execute`, `fetch`, `expect_rows`, `commit`; and `catalog`, which only the discoverer sends |
| Verbs | `read` (the default), `list`, `search`, `create`, `update`, `delete` |
| Driver | `SqlDriver`, for `sql` |
| File kind | `statement`, in `statements/`, for `database` surfaces |
| Binding fields | `statement` (required), `params` (a map from a parameter's name to an expression), `read_back` (the name of a read statement) |
| Finding kind | `table` |
| Discoverer | `catalog`, with the describer below. Sources `live_system`, level `discovered`, 30 seconds, 4 MB |
| Drafting rule | From `table` findings to `statement` files |
| Lint rules | `data.hazard_not_declared` (error), `data.statement_unused` (warning) |
| Settings | `packs.data.max_rows` (the most a statement's `max_rows` may ask for; 1000), `packs.data.result_bytes` (the most text one result may hold; 4 MB) |
| Effects | Reads and writes the root `project`. No network, no process, no auth provider: no server store is built |
| Auth providers | None. The password provider for database identities comes with the first server store |

### The `statements/` file, as built

The fields are those of [section 307](../../spec/3-packs/307-the-data-pack.md), with these differences, each in [decision 0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md):

| Field | As built |
| --- | --- |
| `records.<kind>.key` | The names of the record's fields that identify a row, parent first. Each must be under `fields`, so each has a field type |
| `statements.<name>.record` | Required. Every result column of a read is the column of one of the record's fields, or its version column |
| `returns` | `one_or_none` or `many`, for a read. `one` and `none` are not built |
| `form` | `table` only. `form: statement` is refused as not built |
| `verb` | For a write: `insert`, `update`, or `delete` |
| `guard` | `version` or `none`, required on an update and a delete. `before_values` is refused as not built |
| `hazard` | `none` or `data_elsewhere`, on a write. Declares that the table has a trigger or a cascading key |
| `time_limit` | Whole seconds or milliseconds, such as `2s` or `500ms`, at most ten minutes |

An update is written as `UPDATE table SET column = ?, ... WHERE key = ? AND ... [AND version = ?]`, a delete as `DELETE FROM table WHERE key = ? AND ... [AND version = ?]`, and an insert as `INSERT INTO table (columns) VALUES (?, ...)`. When an insert sets no field of the key, the store assigns it and the statement ends with `RETURNING` and the key's columns. The version a guard compares is a parameter named `version`, which the binding fills under `params`.

### Diagnostics while compiling

Reported with a file, a line, and a fix, under the pack's own codes. An unknown key is the kernel's `kernel.unknown_key`.

| Code | When |
| --- | --- |
| `data.surface_invalid` | A statements file on no `database` surface |
| `data.store_not_built` | A surface names a store this build does not speak to |
| `data.unknown_record`, `data.unknown_field` | A statement names a record, or a key or `set` names a field, that the file does not declare |
| `data.statement_invalid` | A statement whose keys do not fit its effect; a time limit that does not read; a write that is not `form: table`; a guard with no version column; an update or delete of a record with no key |
| `data.limit_invalid` | `returns: many` with no `max_rows`, or one above the setting |
| `data.unknown_statement` | A binding or a read-back names a statement nobody declared |
| `data.param_unbound` | A parameter with no entry under `params` and no input of its name; an entry under `params` that is no parameter |
| `data.verb_mismatch` | The binding's verb is not the one its statement is bound with |
| `data.read_back_invalid` | A read-back that is not a read of one record by its key on the same surface; a read-back on a binding that reads |
| `data.write_needs_stands_alone` | A write binding on a surface that does not declare `stands: alone` |

### Discovery cannot write

The discoverer is given a `SandboxPort` and nothing else. It sends one step, of the kind `catalog`, which commits nothing; the driver carries it out on the connection the store holds read-only, inside a transaction it rolls back. In a posture that may not write, the driver refuses a transaction that may write before opening one, and the handle refuses `commit`. No row is read and none is counted.

The `found` of a `table` finding is JSON: the surface, the store, the table's name, whether it is a view, its columns with declared type, nullability, and place in the key, its unique constraints, its foreign keys, its triggers, the tables its keys cascade to, and `checks_seen`, which is false for a store that does not report check constraints.

| From a `table` finding | The drafter writes | Left to the developer |
| --- | --- | --- |
| A table with a primary key | One record, with a field per column and the type the declared type states: `integer`, `text`, `date`, `datetime`. One read by key, `returns: one_or_none`, two seconds | Units, narrower types, business names, the operation, every write |
| A column whose declared type states none of those | An open question in place of the type | The type |
| A table with no key, or a view | The record, with an open question in place of its key, and no statement | The key |

## Interface

```rust
/// The one public function. The product crate calls
/// `builder.add_pack(&pack())`, with this crate's `pack`.
pub fn pack() -> DataPack;
pub struct DataPack { /* which stores it opens */ }
impl Pack for DataPack { /* manifest, register */ }

/// The failure codes the driver returns, each with its one class.
pub mod codes { pub const STORE_LOCKED: FailureCode; /* ... */ pub const ALL: &[(FailureClass, &str)]; }
```

Inside the crate, the store interface:

```rust
pub(crate) trait Opener: Send + Sync {            // one kind of store
    fn store(&self) -> &'static str;               // the word a surface writes under `store`
    fn quote(&self, identifier: &str) -> Option<String>;
    fn placeholder(&self) -> &'static str;
    fn returns_assigned_key(&self) -> bool;
    fn open(&self, file: &Path, access: Access) -> Result<Box<dyn Conn>, Refusal>;
}
pub(crate) trait Conn: Send {                      // one open connection; every method blocks
    fn access(&self) -> Access;                    // what the store holds it to
    fn begin(&mut self, isolation: Isolation, wait: Duration) -> Result<(), Refusal>;
    fn in_transaction(&self) -> bool;
    fn describe(&mut self, text: &str) -> Result<Shape, Refusal>;   // prepared, not run
    fn run(&mut self, text: &str, params: &[Bound], limits: &RunLimits) -> Result<Ran, Refusal>;
    fn commit(&mut self, wait: Duration) -> Result<(), CommitFailed>;   // NotCommitted or Unknown
    fn rollback(&mut self);
    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal>;
    fn interrupter(&self) -> Interrupter;          // stops the running statement from another thread
}
```

`Refusal` is what a store's error means in the pack's own words: locked, timed out, a constraint of a named kind, read-only, denied, a changed shape, unavailable, too large, not guarded, or a result code with no rule. The store's own error text is never kept, because it may quote a row.

### What the driver is given, and what it gives back

- **The store's file** arrives in `PrepareContext::paths` as a checked path under the surface's name. The driver takes it only when its root is the surface's `root` and its path below the root is the surface's address. It resolves no path.
- **The lease** is not used: an embedded file has no sign-in.
- **A step's arguments** are bound by name to the statement's parameters, in placeholder order. Text is bound as text, a whole number as a whole number, a truth value as 0 or 1, a decimal and a date as their exact text. A list, a record, a moment, and a missing argument stop the step unsent.
- **What a step returns.** A read of one record: its fields by name, and its version under the version column's name; nothing found is `Absent`. A list: `rows`, a list of records, and `more`. An insert whose key the store assigns: the key's fields, also as `assigned_key` and in `for_reconcile`, from the `fetch` before `commit`. Every text passes the redactor. Values are returned as stored; the field type's normalizer, which the engine applies, is what makes `150.5` and `150.5000` agree.
- **What a stored value may be.** Text for a field whose type is text, a choice, a decimal, a date, or a time; a whole number for an integer; either for a truth value and for a version. Anything else is drift. A float or bytes is `value_kind_unsupported`.

### How the mode fills the loop

| Part | What happens |
| --- | --- |
| Prepare | Checks the surface, its store, and the checked path; decodes the pack's section; opens the read-only connection on a blocking thread and reads back that the store holds it read-only. Nothing else is opened |
| `begin` | Read-only: on the reader. Read-write: refused outside a posture that may write; opens the writer at the first write, with foreign keys enforced and read back; takes the store's write lock at once, so that a lock wait ends before anything is written |
| `execute` | Requires an open transaction of the statement's access. The first time a statement is used in a session the store describes it, and its parameter count and result columns are held to the plan. The first time a write names a table with no declared hazard, the catalog is read and a trigger or a cascading key stops the write unsent. Then the statement runs with bound parameters, to at most one row more than its limit |
| `expect_rows` | Compares the count of rows the statement changed with the declared one. None changed raises the step's own failure, `version_conflict` with a guard and `changed_since_preview` without; more than one is `key_not_unique`. Both roll back |
| `fetch` | Turns the rows into values. Ends a read-only transaction; leaves a writing one open |
| `commit` | Refused unless a writing transaction is open and its count was checked. Never interrupted. The commit point |
| Read back | The read statement the binding names, lowered as a read: a new read-only transaction on the reader, after the commit |
| Release | Rolls back what is open and closes both connections on a blocking thread |

Every call into the store runs on a blocking thread; the connection moves there and back. At a step's time limit the running statement is interrupted. When a step's future is dropped the statement is interrupted too, the connection does not come back, and the session sends nothing more.

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `SqlDriver`. A lock wait, a refusal, and a lost answer are each returned once |
| 2 | Read-back for every write | The read statement the binding names, by key, in a new read-only transaction. The kernel refuses a write binding without one |
| 3 | Each identity strategy | `natural` and `composite`: the record's key. A key the store assigns: captured before `commit`; the kernel has no strategy for it, so such a create compiles as `none`. `stamped` and `find`: refused as not built. `none`: `irreversible`, by the kernel's rule |
| 4 | A test stand | `targets/example-db`, cases 1 to 10. The server stand is not built |
| 5 | A discoverer | `catalog`: `discovered`, `live_system`, nothing sampled |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Files in the root `project`, sessions on `database` surfaces. A hazard is declared on the statement, required of the operation by a lint rule, and checked against the catalog before a write |
| 8 | A record for the mode | [Decision 0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md) |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| The product crate | `pack()`, only |
| `a2m-engine`, through the registry's handle | The driver |
| `a2m-files`, through the gathered set | The file kind, the lowering, the lint rules, the drafting rule |
| `a2m-actions`, through the registry | The discoverer and the describer |

## State

| State | Where |
| --- | --- |
| A session's two connections, its open transaction, the result of the last `execute` | Memory, inside `Prepared`, from `prepare` to `release` |
| Which statements were described and which tables were looked at | The same, so each is done once in a session |
| Credentials | None |

Nothing is kept between runs. A pool of connections shared by the runs of one identity is not built.

## Failures

| Class | Code | When | Sent |
| --- | --- | --- | --- |
| `busy` | `store_locked` | Another connection held the store's lock for the whole time limit | Not sent at a writer's `begin`; answered elsewhere |
| `transient` | `timeout` | A statement was interrupted at its time limit | Answered |
| `transient` | `upstream_unavailable` | The store's file cannot be opened | Not sent |
| `transient` | `connection_lost` | A connection did not come back, or the session is in doubt after a lost answer | Not sent |
| `possible_commit` | `no_confirmation` | `commit` was sent and nothing says whether it happened | Unknown |
| `rejected` | `constraint_violated` | A constraint refused the write; the message names the kind of constraint, never the store's text | Answered |
| `policy` | `read_statement_wrote` | A statement declared a read tried to change data, and the store refused it | Answered |
| `policy` | `denied_by_store` | The store does not let the identity write | Not sent, or answered |
| `policy` | `read_only_posture` | A transaction that may write, outside a posture that may write | Not sent |
| `drift` | `schema_changed` | A statement no longer prepares; its parameters or columns differ from the plan; a value is stored another way than its field type declares; a table has a trigger or a cascading key the target did not declare | Not sent when found before the statement ran |
| `drift` | `key_not_unique` | A read by key, or a keyed write, matched more than one row | Answered |
| `stale` | `version_conflict`, `changed_since_preview` | A write changed no row | Answered |
| `unknown_variant` | `unexpected_status` | A result code with no rule; a step out of order | Answered, or not sent |
| `unknown_variant` | `value_kind_unsupported` | A float or bytes in a result | Answered |
| `unknown_variant` | `result_too_large` | A result holds more text than `packs.data.result_bytes` | Answered |
| `input` | `value_not_bindable` | An argument is missing, or is a list, a record, or a moment | Not sent |
| `input` | `path_not_allowed` | The run was given no checked path for the store's file | Not sent |

Whenever a step fails inside a transaction the driver rolls back first.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A registry with only this pack builds, with the same digest each time, and the manifest declares every code the driver returns | `src/tests/registration.rs` |
| The fixtures compile twice to the same bytes; a table-form write carries the full key and its guard | `src/tests/compile.rs` |
| An unknown key, record, statement, or verb is refused with a file, a line, and a fix; a write compiles only on a surface that stands alone | `src/tests/compile.rs` |
| No input can change a statement: text that would widen a predicate, were it placed in the statement, matches no row and changes nothing | `src/tests/reads.rs` |
| A statement declared a read that writes is refused by the store, and the stand shows nothing written | `src/tests/reads.rs`, stand case 2 |
| A read by key that matches several rows is a failure, never a pick; a list says that more exist | `src/tests/reads.rs`, case 1 |
| Five runs of one read return the same values and note the same means | `src/tests/reads.rs` |
| A secret in a row reaches no result and no noted detail | `src/tests/reads.rs` |
| A lock is named `busy`, and the prepared driver works again when it is gone | `src/tests/reads.rs`, `src/tests/writes.rs` |
| A renamed or retyped column is drift | `src/tests/reads.rs`, case 10 |
| `commit` reaches the driver only with the ledger's token; one update is one write at the stand; the read-back agrees through the field type's normalizer | `src/tests/writes.rs` |
| A value the store rescales or pads verifies; a value it changes does not | `src/tests/writes.rs`, case 7 |
| A row changed since the preview is stale, and nothing is written | `src/tests/writes.rs`, case 6 |
| A constraint refuses a write and nothing lands | `src/tests/writes.rs`, case 3 |
| A key the store assigns is known before `commit` | `src/tests/writes.rs`, case 5 |
| A trigger the target did not declare stops a write unsent | `src/tests/writes.rs`, case 4 |
| Nothing is written in a posture that may not write | `src/tests/writes.rs` |
| A lost answer to `commit` is `possible_commit`, the session sends nothing more, and the stand shows one write | `src/tests/writes.rs`, with a fault placed at the store interface |
| Discovery writes nothing, reads no row, and its findings match the stand's own account of the catalog | `src/tests/discovery.rs`, cases 4 and 9 |
| A draft holds no write and no unit; a table with no key gets a question and no statement; the same findings give the same bytes | `src/tests/discovery.rs` |

The tests are inside the crate, under `src/tests/`, because the fault of a lost answer is placed at the store interface, which is private. They reach the driver only through the registry's handle.

**How the stand's own triggers are treated.** The pack does not know the prefix `stand_`. While the stand's write log is on, every table has a trigger, and the pack treats it as it treats any trigger. So the statements that are proven written exactly once declare `hazard: data_elsewhere`, and the rule for a table with no hazard is proven with the log off.

## Build order

1. Done: the manifest, the registration, and `pack()`.
2. Done: the `statements/` file kind, table-form statements, the lowering of the six verbs, two lint rules, and the fixtures in `tests/fixtures/example/`.
3. Done: the driver for reads: prepare, `begin`, `execute`, `fetch`, release; the read-only proof; limits; drift.
4. Done: writes: `form: table` update, delete, and insert; `guard: version` and `none`; `expect_rows`; an assigned key before `commit`; `possible_commit`.
5. Done: the `catalog` discoverer and describer, and the drafting rule for statements files.
6. Not built: the server stand and a server store, with its password provider, network effect, encrypted link, read-only transactions, isolation levels the store must offer, a pool per identity, and the check that the identity is not administrative.
7. Not built: `form: statement` writes, procedures and the verb `call`, `upsert`, `stamped` and `find` identity, the `before_values` guard, `returns: one` and `none` on a hand-written statement, a parent saved with its rows, writes on a surface that stands beneath an application.
8. Not built: a drafted read operation beside the drafted statements file; the scout's five tries; the check of the store's version against the target's recorded identity; binding a moment.

## Open points

- **The step trace.** The driver notes for each step what it did (`means`: the step, the statement, the access) and what it measured (`measured`: rows, bytes, rows changed). `Observed` has no place for an `Acting`, so the notes stay in the session and only the tests read them.
- **Evidence on a failure.** `StepFailure` carries no capture, so the name of a refused constraint cannot be handed on as untrusted evidence. The message names the kind of constraint only.
- **A store in a root a person added.** The manifest lists roots by name, so the pack can declare only `project`.
- **How the engine gives the driver the store's file.** Nothing in the plan says that a surface's address is a file in a root. The driver expects the checked path under the surface's name.
- **A statement interrupted at its time limit** is built and not proven: the stand cannot slow a statement.
- **A read-only file** maps to `denied_by_store` and is not proven here: the stand's fault does not bind the superuser the tests ran as.
- **Functions that reach outside**, columns of floating type, and reading back from a replica: as in section 307.
