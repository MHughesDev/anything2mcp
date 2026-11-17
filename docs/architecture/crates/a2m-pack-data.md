# `a2m-pack-data`

The data pack: the `sql` mode over relational stores. It brings the `database` surface kind, the `statements/` file kind, the lowering of six verbs into five steps, the driver of those steps, a discoverer that reads a store's own catalog, and a drafting rule from a table to a statements file. Statements are constants of the plan; a value from a tool input reaches the store only as a bound parameter; a read runs only on a connection the store itself holds read-only. A statement is one plain read or one keyed write and nothing else: the compiler refuses any other by its words, the driver refuses it again, and the connection refuses it whatever its words are.

Two stores are built. The embedded SQL engine, through `rusqlite`, always. A server store, PostgreSQL, through `tokio-postgres`, behind the pack's feature `postgres`, which is off by default until it is proven on the three systems ([decision 0168](../../decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md)). Everything that is specific to a store is behind the pack's own small interface.

## Facts

| | |
| --- | --- |
| Side | Pack: data |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (`Plan`, `Step`, `Value`, `Failure`, `Finding`), `a2m-files` (file-kind, lowering, lint, and drafting registration), `a2m-auth` (`SessionLease`, in the driver's signature only), `a2m-provider` (`Pack`, `Driver`, `Discoverer`, `Describer`, `SandboxPort`, `CheckedPath`), `a2m-telemetry` (`Acting`, `Clock`). Outside: `rusqlite`, the workspace's one binding to the embedded engine, with its features `hooks`, `limits`, and `load_extension` (what each adds is under "How a connection is held to its one file"); `serde`, `serde_json`, `tokio`. With the feature `postgres`: `tokio-postgres` (feature `runtime` only), `bytes`, `futures-core`. Tests also use `a2m-expr`, `a2m-ledger`, `a2m-vault`, and the stand `example-db`, with its server side |
| Built in | Packages G2 and G3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [307](../../spec/3-packs/307-the-data-pack.md); [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input changes a statement); [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (set, read back, go on only if it took) |
| Decisions | [0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md), [0101](../../decisions/0101-what-the-data-pack-refuses-and-how-it-names-it.md), [0141](../../decisions/0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md), [0168](../../decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md) |

## Owns

- The `statements/` file kind: its schema, the names it defines, and its compilation into this crate's own section of the plan, an opaque payload that only this crate decodes.
- Writing a `form: table` statement from a record's declaration, with every identifier quoted by the store's dialect: every write, and the read of a record by its key.
- Holding a statement to its form: one `SELECT` for a read, and the one `INSERT`, `UPDATE`, or `DELETE` its verb states for a write.
- Holding a connection to its one file and its access, whatever a statement says.
- The lowering of `read`, `list`, `search`, `create`, `update`, and `delete` into `begin`, `execute`, `fetch`, `expect_rows`, and `commit`.
- Running a statement: binding step arguments to parameters, holding it to its time limit and its row and byte limits, and turning stored values into typed values.
- Proving a read is a read: opening the read-only connection, and reading the setting back before the first statement.
- Confining a write: the full key predicate, one statement in a transaction, the count of rows changed checked to be one inside the transaction, and the check of triggers and cascading keys before the write is sent.
- Reading a write back: the read by key that the pack wrote, held when it runs to the key it was asked for.
- Saying which side of `commit` a failure fell on.
- The `table` finding kind and the schema of its `found`; reading the catalog; drafting from it.

It must never: place a value from an input into a statement's text; let an input choose a table, a column, or a file; run a statement the plan does not hold, or one declared for another surface; run a statement that attaches or detaches a file, reads or changes a setting, changes the schema, loads an extension, or begins or ends a transaction; run as a read a statement that returns no column; retry; write through a connection the store holds read-only, or read through one it does not; commit a transaction whose count of rows was not checked, that ran more than one statement, or that changed any number of rows but one; take a hand-written read as a read-back, or take a record with another key for the one asked for; read a catalog it did not read whole, or that does not hold the table, as showing no trigger; use a connection again after a commit whose answer was lost; read or count rows during discovery; draft a write.

## What it registers

One function hands the pack to the registry builder: this crate's `pack()` returns a `DataPack`, which implements `Pack`. The manifest is `anything2mcp-provider.toml` in the crate, embedded with `include_str!`.

| Registration | Content |
| --- | --- |
| Mode | `sql`, archetype request, rank 30, four reads and one write in a session, a write holds the surface |
| Surface kind | `database`, with a session. Fields: `store` (required; `sqlite`), `file` (the address: the path of the store's file below its root, with forward slashes), `root` (the named root; `project` unless given) |
| Surface kind, feature `postgres` | `database_server`, with a session. Fields: `store` (required; `postgres`), `host` (the address: an IP address on the loopback range), `port`, `database`, `schema` (`public` unless given). The role and its password come from the lease; there is no switch for the link |
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
| Effects | Reads and writes the root `project`. No network without the feature `postgres`; with it, `network = "surfaces"` and sessions on both kinds. The manifest of that build is the embedded one with each change made in its place and three codes appended (`anything2mcp-provider-server.toml`); a test holds each change to its place |
| Auth providers | None. A server store takes the user and password the provider `http_basic` holds, read back from its basic-scheme value; a provider for database identities is owed by `a2m-auth` |

### The `statements/` file, as built

The fields are those of [section 307](../../spec/3-packs/307-the-data-pack.md), with these differences, each in [decision 0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md):

| Field | As built |
| --- | --- |
| `records.<kind>.key` | The names of the record's fields that identify a row, parent first. Each must be under `fields`, so each has a field type |
| `statements.<name>.record` | Required. Every result column of a read is the column of one of the record's fields, or its version column |
| `returns` | `one_or_none` or `many`, for a read. `one` and `none` are not built |
| `form` | `table` only. On a write, `form: statement` is refused as not built. On a read, `form: table` is the read of the record by its key, which the pack writes: it takes no `text`, `params`, or `returns`. A read without `form` is hand-written |
| `text` | A hand-written read. One statement that begins with `SELECT` or `WITH`. Outside quotes and comments it holds none of the words `INSERT`, `UPDATE`, `DELETE`, `INTO`, `REPLACE` (except as the function), `ATTACH`, `DETACH`, `PRAGMA` or a name beginning `pragma_`, `VACUUM`, `CREATE`, `DROP`, `ALTER`, `load_extension`. A table or a column with such a name is written in quotes |
| `verb` | For a write: `insert`, `update`, or `delete` |
| `guard` | `version` or `none`, required on an update and a delete. `before_values` is refused as not built |
| `hazard` | `none` or `data_elsewhere`, on a write. Declares that the table has a trigger or a cascading key |
| `time_limit` | Whole seconds or milliseconds, such as `2s` or `500ms`, at most ten minutes |

An update is written as `UPDATE table SET column = ?, ... WHERE key = ? AND ... [AND version = ?]`, a delete as `DELETE FROM table WHERE key = ? AND ... [AND version = ?]`, and an insert as `INSERT INTO table (columns) VALUES (?, ...)`. When an insert sets no field of the key, the store assigns it and the statement ends with `RETURNING` and the key's columns. The version a guard compares is a parameter named `version`, which the binding fills under `params`; a guarded statement that also takes a field named `version` is refused, because both would be bound to the same argument. The read by key is written as `SELECT column, ... FROM table WHERE key = ? AND ...`: the column of every field, then the version column, and the full key.

### Diagnostics while compiling

Reported with a file, a line, and a fix, under the pack's own codes. An unknown key is the kernel's `kernel.unknown_key`.

| Code | When |
| --- | --- |
| `data.surface_invalid` | A statements file on no `database` surface |
| `data.store_not_built` | A surface names a store this build does not speak to |
| `data.unknown_record`, `data.unknown_field` | A statement names a record, or a key or `set` names a field, that the file does not declare |
| `data.statement_invalid` | A statement whose keys do not fit its effect; a time limit that does not read; a write that is not `form: table`; a guard with no version column; a read, update, or delete by key of a record with no key; a hand-written read that is not one plain `SELECT`; a guarded statement that takes a field named `version` |
| `data.limit_invalid` | `returns: many` with no `max_rows`, or one above the setting |
| `data.unknown_statement` | A binding or a read-back names a statement nobody declared |
| `data.param_unbound` | A parameter with no entry under `params` and no input of its name; an entry under `params` that is no parameter |
| `data.verb_mismatch` | The binding's verb is not the one its statement is bound with |
| `data.read_back_invalid` | A read-back that is not the `form: table` read of the same record on the same surface; a read-back on a binding that reads |
| `data.write_needs_stands_alone` | A write binding on a surface that does not declare `stands: alone` |

### Discovery cannot write

The discoverer is given a `SandboxPort` and nothing else. It sends one step, of the kind `catalog`, which commits nothing; the driver carries it out on the connection the store holds read-only, inside a transaction it rolls back. In a posture that may not write, the driver refuses a transaction that may write before opening one, and the handle refuses `commit`. No row is read and none is counted.

### What the kernel holds the pack to

| Statement | The pack's answer |
| --- | --- |
| Which kinds save (`Lowering::commits`) | `commit`: `Always`. `begin`, `execute`, `fetch`, `expect_rows`, `catalog`: `Never`. A writing transaction changes nothing in the store until it is committed, and no read ends in a `commit` |
| The least effect of a binding (`Lowering::least_effect`) | By the verb, which the lowering holds to what the statement does: `delete` is `irreversible`, `create` and `update` are `write`, the rest `read` |
| The manifest's `commit_steps` | `["commit"]`: the handle refuses a `commit` sent without the mark |
| Which file kinds describe a write (`FileKind::writes`) | `statement`, when any of its statements is not plainly `effect: read`; a missing or open effect counts as a write. The drafter therefore never writes a statements file that holds a write |
| The driver's guard | A transaction that may write begins only in `Live` and in `Sandbox` with test records. In every posture, a `commit` step not marked `commits` is refused unsent, `policy`, `read_only_posture`, and its transaction is rolled back: in the context that writes a test record, the marked commit is the one step that can change the store |

The `found` of a `table` finding is JSON: the surface, the store, the table's name, whether it is a view, its columns with declared type, nullability, and place in the key, its unique constraints, its foreign keys, its triggers, the tables its keys cascade to, and `checks_seen`, which is false for a store that does not report check constraints.

| From a `table` finding | The drafter writes | Left to the developer |
| --- | --- | --- |
| A table with a primary key | One record, with a field per column and the type the declared type states: `integer`, `text`, `date`, `datetime`. One read by key, `form: table`, two seconds: the draft holds no statement text | Units, narrower types, business names, the operation, every write |
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
pub(crate) enum StoreAt { File(PathBuf), Server(ServerAt) }   // `Server` with the feature only
pub(crate) trait Opener: Send + Sync {            // one kind of store
    fn store(&self) -> &'static str;               // the word a surface writes under `store`
    fn quote(&self, identifier: &str) -> Option<String>;
    fn placeholder(&self, position: usize) -> String;   // `?`, or `$1`, `$2`, ...
    fn dialect(&self) -> Dialect;                  // whose words and quotes the form check reads
    fn returns_assigned_key(&self) -> bool;
    fn pooled(&self) -> bool { false }             // feature `postgres`: idle connections are kept
    fn open(&self, at: &StoreAt, access: Access) -> Result<Box<dyn Conn>, Refusal>;
}
pub(crate) trait Conn: Send {                      // one open connection; every method blocks
    fn access(&self) -> Access;                    // what the store holds it to
    fn begin(&mut self, isolation: Isolation, wait: Duration) -> Result<(), Refusal>;
    fn in_transaction(&self) -> bool;
    fn describe(&mut self, text: &str) -> Result<Shape, Refusal>;   // prepared, not run: parameters, columns, and whether the store says it only reads
    fn run(&mut self, text: &str, params: &[Bound], limits: &RunLimits) -> Result<Ran, Refusal>;
    fn commit(&mut self, wait: Duration) -> Result<(), CommitFailed>;   // NotCommitted or Unknown
    fn rollback(&mut self);
    fn catalog(&mut self, tables: usize) -> Result<Catalog, Refusal>;
    fn interrupter(&self) -> Interrupter;          // stops the running statement from another thread
    fn reach(&mut self, table: &str, tables: usize) -> Result<Result<(usize, usize), Unseen>, Refusal>;
                                                   // what fires on a table, and what cascades from it; by the catalog unless the store asks of one table
    fn usable(&self) -> bool { true }              // feature `postgres`
}
```

`Refusal` is what a store's error means in the pack's own words: locked, timed out, a constraint of a named kind, read-only, denied, a changed shape, unavailable, too large, not allowed by the connection's own guard, not guarded, an identity that is an administrator (server store), or a result code with no rule. The store's own error text is never kept, because it may quote a row. What is kept of a refused constraint is its name: a server's constraint, table, and column, read from the fields of its error that name schema, and an embedded store's own words for a unique, not-null, or check constraint after `constraint failed:`, which name columns and checks and no value. It goes in the failure's `said`, apart from the sentence, redacted by the driver before it leaves `act`. `Shape::reads_only` is `None` for a store that cannot say before it runs a statement; its read-only transaction refuses one that writes.

### What the driver is given, and what it gives back

- **The store's file** arrives in `PrepareContext::paths` as a checked path under the surface's name. The driver takes it only when its root is the surface's `root` and its path below the root is the surface's address. When the run was handed none and the root is `project`, the file is the surface's address below the project folder of the request that started the run (`CheckedPaths::project_folder`), never below the folder the engine was started in; a run whose request had no project folder stops `input`/`project_folder_missing` before anything is opened ([decision 0175](../../decisions/0175-the-data-and-device-packs-take-up-the-second-pass-of-the-provider-contract.md)). Any other root is one the manifest declares, and the driver checks it with those roots.
- **The lease** is not used for an embedded file, which has no sign-in. For a server it gives the role and its password, and the key under which its connections are kept: the surface, the session, and the epoch.
- **A step's arguments** are bound by name to the statement's parameters, in placeholder order. Text is bound as text, a whole number as a whole number, a truth value as 0 or 1, a decimal and a date as their exact text. A list, a record, a moment, and a missing argument stop the step unsent.
- **What a step returns.** A read of one record: its fields by name, and its version under the version column's name; nothing found is `Absent`. A list: `rows`, a list of records, and `more`. An insert whose key the store assigns: the key's fields, also as `assigned_key` and in `for_reconcile`, from the `fetch` before `commit`. The read-back of such a write names no argument for the key's parameters: the driver binds each from `Prepared::recorded()`, which holds what `fetch` recorded, within the pass and on a resume. A key neither given nor recorded stops the step unsent, `input`/`value_not_bindable`. Every step also returns the capture `acting`: `means` (the step, the statement, the access) and `measured` (rows, bytes, rows changed). Every text passes the redactor. Values are returned as stored; the field type's normalizer, which the engine applies, is what makes `150.5` and `150.5000` agree.
- **What a stored value may be.** Text for a field whose type is text, a choice, a decimal, a date, or a time; a whole number for an integer; either for a truth value and for a version. Anything else is drift. A float or bytes is `value_kind_unsupported`.

### How the mode fills the loop

| Part | What happens |
| --- | --- |
| Prepare | Checks the surface, its store, and the checked path; decodes the pack's section; opens the read-only connection on a blocking thread, by an absolute path only, and reads back that the store holds it read-only and to its one file. Nothing else is opened |
| `begin` | Read-only: on the reader. Read-write: refused outside a posture that may write; opens the writer at the first write, with foreign keys enforced and read back; takes the store's write lock at once, so that a lock wait ends before anything is written |
| `execute` | Requires an open transaction of the statement's access, and a statement declared for the session's surface. The first time a statement is used in a session its words are held to the form of one read or one keyed write, as the compiler held them; then the store describes it, and its parameter count and result columns are held to the plan. A read that the store says would change data, or that returns no column, stops unsent; so does a read by key that does not return its key's columns. A writing transaction takes one statement. The first time a write names a table with no declared hazard, the catalog is read: a trigger or a cascading key stops the write unsent, and so does a catalog that was cut off or does not hold the table. Then the statement runs with bound parameters, to at most one row more than its limit |
| `expect_rows` | Only in a transaction that wrote, and only for the count one: a plan that states another count is refused. None changed raises the step's own failure, `version_conflict` with a guard and `changed_since_preview` without; more than one is `key_not_unique`. Both roll back |
| `fetch` | Turns the rows into values. The record a read by key returns must hold, in its key's columns, exactly the values that were bound; another record is `read_not_by_key`. Ends a read-only transaction; leaves a writing one open |
| `commit` | Refused unless a writing transaction is open and its count was checked. Never interrupted. The commit point |
| Read back | The `form: table` read of the same record that the binding names, lowered as a read: a new read-only transaction on the reader, after the commit |
| Release | Rolls back what is open and closes both connections on a blocking thread |

Every call into the store runs on a blocking thread; the connection moves there and back. At a step's time limit the running statement is interrupted. When a step's future is dropped the statement is interrupted too, the connection does not come back, and the session sends nothing more.

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | `SqlDriver`. A lock wait, a refusal, and a lost answer are each returned once |
| 2 | Read-back for every write | The read by key the pack wrote for the write's record, in a new read-only transaction, held to the key asked for. A hand-written read is never a read-back. The kernel refuses a write binding without one |
| 3 | Each identity strategy | `natural` and `composite`: the record's key. A key the store assigns: captured before `commit`; the kernel has no strategy for it, so such a create compiles as `none`. `stamped` and `find`: refused as not built. `none`: `irreversible`, by the kernel's rule |
| 4 | A test stand | `targets/example-db`, cases 1 to 10; its server side, `example_db::server`, for a lost answer after `commit`, an administrative identity, a row lock, a statement timeout, and read-only transactions |
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
| A session's two connections, its open transaction, the result of the last `execute`, whether the transaction wrote, and the key the last read by key asked for | Memory, inside `Prepared`, from `prepare` to `release` |
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
| `policy` | `read_statement_wrote` | A statement declared a read would change data: its words say so, or the store does | Not sent; answered when only the store's refusal at run time found it |
| `policy` | `statement_not_allowed` | A statement is not one plain read or one keyed write: it attaches or detaches a file, reads or changes a setting, changes the schema, loads an extension, begins or ends a transaction, is several statements, or is a read that returns no column | Not sent when the driver finds it; answered when the connection's guard does |
| `policy` | `denied_by_store` | The store does not let the identity write | Not sent, or answered |
| `policy` | `read_only_posture` | A transaction that may write, outside a posture that may write | Not sent |
| `drift` | `schema_changed` | A statement no longer prepares; its parameters or columns differ from the plan; a value is stored another way than its field type declares; a table has a trigger or a cascading key the target did not declare; the table a write names is not in the catalog | Not sent when found before the statement ran |
| `drift` | `read_not_by_key` | A read by key does not return its key's columns, or returned a record whose key is not the one asked for | Not sent, or answered |
| `drift` | `key_not_unique` | A read by key, or a keyed write, matched more than one row | Answered |
| `stale` | `version_conflict`, `changed_since_preview` | A write changed no row | Answered |
| `unknown_variant` | `unexpected_status` | A result code with no rule; a step out of order | Answered, or not sent |
| `unknown_variant` | `value_kind_unsupported` | A float or bytes in a result | Answered |
| `unknown_variant` | `result_too_large` | A result holds more text than `packs.data.result_bytes` | Answered |
| `unknown_variant` | `catalog_too_large` | A write declares no hazard, and the store holds more tables and views than one catalog read takes (2000), so the catalog cannot show what the write reaches | Not sent |
| `input` | `value_not_bindable` | An argument is missing, or is a list, a record, or a moment | Not sent |
| `input` | `path_not_allowed` | The run was given no checked path for the store's file | Not sent |
| `policy` | `link_not_encrypted` | Server store: the surface is not on the loopback address, written as an IP address | Not sent |
| `policy` | `identity_is_administrator` | Server store: the role is a superuser, bypasses row security, may replicate or make roles, or owns anything in the schema; or owns the written table | Not sent |
| `input` | `credential_not_carried` | Server store: no lease, or a lease whose credential is not one user and one password | Not sent |

For the server store, a serialization failure, a deadlock, a lock wait at its limit, and too many connections are `busy`, `store_locked`; a cancelled statement is `transient`, `timeout`; a refused password is `denied_by_store`; a value the column's type does not read is `rejected`, `constraint_violated` (kind `value`). An answer to `commit` that ends the session (classes `08`, `57P`, `58`, `XX`), or none, is `possible_commit`.

Whenever a step fails inside a transaction the driver rolls back first.

## How a statement and a connection are held

Three guards, each on its own, because a read-only connection stops changes to the store's data and nothing else ([decision 0141](../../decisions/0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md)).

| Guard | Where | What it refuses |
| --- | --- | --- |
| The statement's words | The compiler, for a hand-written read and for what the pack itself wrote; the driver again, the first time a session uses a statement | Anything that is not one statement beginning `SELECT` or `WITH` for a read, or the word its verb states for a write; the words listed under `text` above. Quoted text and comments are not read |
| The store's description | The driver, before the statement runs | A read that the store says is not read-only; a read with no result column; a column the record does not declare; another number of parameters |
| The connection | The embedded store, when a connection is opened and for every statement the engine compiles on it | Below |

### How a connection is held to its one file

| Setting | Through | Read back |
| --- | --- | --- |
| Only an absolute path is opened. The bundled engine is compiled to read a name beginning `file:` as an address with options, whatever flags it is given | `Path::is_absolute` | Not applicable |
| No right to create the file; read-only for the reader | The open flags | `is_readonly` |
| Writes refused on the reader; foreign keys enforced on the writer | `PRAGMA query_only`, `PRAGMA foreign_keys` | Yes |
| Extension loading off. The bundled engine is compiled with it on for its programming interface | `load_extension_disable`, of the feature `load_extension`. The feature also adds functions that turn loading on; they are `unsafe`, which the workspace forbids | The engine has no way to ask through this binding; a test asks |
| The schema's own tables cannot be written, and a name in double quotes that is no column is an error, never a text | `set_db_config`: `DEFENSIVE` on, `DQS_DML` and `DQS_DDL` off | Yes |
| A file attached all the same could not be made or written | `set_db_config`: `ENABLE_ATTACH_CREATE` and `ENABLE_ATTACH_WRITE` off | Yes |
| No file can be attached | `set_limit(SQLITE_LIMIT_ATTACHED, 0)`, of the feature `limits`, which adds only the two functions that read and set a limit | Yes |
| Every statement the engine compiles is asked about | `authorizer`, of the feature `hooks`, which `a2m-ledger` already uses. It adds the engine's callbacks and no outside crate | Installed last, so that nothing after it changes a setting |

The authorizer allows a statement of a plan to read, to call a function other than `load_extension`, and, on the writer, to insert, update, and delete rows of any table that is not the engine's own, which covers what a declared trigger does. It denies attach, detach, every setting, every change of schema including the temporary one, savepoints, and transaction control. The pack's own statements, `BEGIN`, `COMMIT`, `ROLLBACK`, and the catalog queries, run with a mark set that allows exactly those; a statement of a plan never runs with it. A reader also refuses, before the first step, a statement that the engine says is not read-only or that returns no column: a statement refused half way may already have made a file.

What a reader can see is therefore the store's own schema and an empty temporary schema that no statement can fill. The settings above, the path of the file, and the list of attached files cannot be read by a statement.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A registry with only this pack builds, with the same digest each time, and the manifest declares every code the driver returns | `src/tests/registration.rs` |
| The fixtures compile twice to the same bytes; a table-form write carries the full key and its guard | `src/tests/compile.rs` |
| An unknown key, record, statement, or verb is refused with a file, a line, and a fix; a write compiles only on a surface that stands alone | `src/tests/compile.rs` |
| No input can change a statement: text that would widen a predicate, were it placed in the statement, matches no row and changes nothing | `src/tests/reads.rs` |
| A statement declared a read that writes is refused by the compiler, by the driver, and by the connection, and the stand shows nothing written | `src/tests/reads.rs`, stand case 2; `src/tests/guards.rs` |
| A statement's form: a read is one plain `SELECT`, a write is what its verb states, and quoted text is not read | `src/shape.rs` |
| No connection attaches a file, by path or by address; none changes a setting, the schema, or its own transaction; none loads an extension; none leaves a file behind; a quoted name that is no column is an error | `src/tests/guards.rs`, which asks the file system and the stand |
| A hand-written read that is not one plain `SELECT` does not compile; in a plan the compiler did not make, the driver refuses it unsent | `src/tests/compile.rs`, `src/tests/guards.rs` |
| Only an absolute path is opened | `src/tests/guards.rs` |
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
| A trigger or a table named in another case is the same trigger and the same table; a table the catalog does not hold is not "no trigger" | `src/tests/guards.rs` |
| A catalog that was cut off stops a write that declares no hazard | `src/tests/guards.rs`, with 2000 tables more in the stand |
| A write is read back only by the pack's own read of the same record by its key | `src/tests/compile.rs` |
| A read by key that returns another record, or no key, is stopped | `src/tests/guards.rs` |
| A write that matches more rows than one is rolled back, and the stand shows none written | `src/tests/writes.rs` |
| Nothing is committed without its count checked, after a second statement, or for a count other than one | `src/tests/writes.rs` |
| A guarded statement that takes a field named `version` does not compile | `src/tests/compile.rs` |
| Nothing is written in a posture that may not write | `src/tests/writes.rs` |
| A `commit` not marked `commits` is refused by the handle and by the driver, also in the posture that writes a test record, and the stand shows nothing changed | `src/tests/writes.rs` |
| A write never compiles as a read, nor a delete as a plain write; `commit` is the one kind that saves, in the lowering and in the manifest | `src/tests/compile.rs`, `src/tests/registration.rs` |
| A drafting rule that drafts a statement that writes is refused, and its file is not written | `src/tests/discovery.rs` |
| A lost answer to `commit` is `possible_commit`, the session sends nothing more, and the stand shows one write | `src/tests/writes.rs`, with a fault placed at the store interface; for the server store, `src/tests/server.rs`, with the stand's relay losing the answer and dropping the `COMMIT` |
| Server store: numbers and moments are read exactly; a moment with no zone and a float stop | `src/tests/server.rs` |
| Server store: a read that changes data with no word that says so is refused by the read-only transaction; a held lock is `busy`; a statement past its limit is stopped by the server | `src/tests/server.rs` |
| Server store: a trigger (named in capitals), a rule, a computed column, and a cascading key each stop a write that declares no hazard | `src/tests/server.rs` |
| Server store: an owner and a superuser are refused before anything runs; off the loopback address nothing is sent; a wrong password is refused and not shown | `src/tests/server.rs` |
| Server store: one session's runs share one connection, and a run that failed closes it | `src/tests/server.rs` |
| The server dialect refuses escape and dollar quotes, `SET` outside an update, and the words and functions that reach outside a read | `src/shape.rs` |
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
5a. Done: the second pass of the provider contract ([decision 0175](../../decisions/0175-the-data-and-device-packs-take-up-the-second-pass-of-the-provider-contract.md)): step inputs built by the constructor, the `acting` capture on every step in place of the driver's own notes, the key a store assigns read back from what was recorded, a constraint's name in `said`, and a store's file below the request's project folder. Proven by `five_runs_of_one_read_do_the_same_thing`, `a_key_the_store_assigned_reconciles_a_lost_commit_by_what_was_recorded`, `what_the_store_says_of_a_refused_write_is_said_and_never_the_sentence`, and `a_database_file_is_of_the_project_of_the_request_that_started_the_run`.
5b. Done: what the kernel holds the pack to after the safety audit ([decision 0155](../../decisions/0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md)): `commits`, `least_effect`, `commit_steps`, `writes`, and the commit guard.
6. Done, behind the feature `postgres` ([decision 0168](../../decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md)): the server stand, and PostgreSQL through `tokio-postgres`: the surface kind `database_server`, the server dialect of the form check, numbered parameters bound as text, values read exactly from their binary form, read-only transactions read back, the isolation levels, the server's statement and lock limits, the check of the role, what a write reaches asked of the one table, `possible_commit` found by the relay, a pool per surface and session, and discovery. Proven in `src/tests/server.rs`, seventeen tests against a real server. Not built: the encrypted link (owed: a rustls configuration from the `tls` module of `a2m-auth` with the caller's ALPN), a provider for database identities (it reads `http_basic` now), and a row limit the server keeps to (a portal).
7. Not built: `form: statement` writes, procedures and the verb `call`, `upsert`, `stamped` and `find` identity, the `before_values` guard, `returns: one` and `none` on a hand-written statement, a parent saved with its rows, writes on a surface that stands beneath an application.
8. Not built: a drafted read operation beside the drafted statements file; the scout's five tries; the check of the store's version against the target's recorded identity; binding a moment.

## Open points

- **A write that names no read-back** is refused by the pack (`data.read_back_invalid`) unless the operation is `irreversible` with identity `none`: an append of a row no one names. That binding states `WriteEvidence::Acknowledged`, and the store's count of exactly one row, checked before the commit, is its evidence ([decision 0175](../../decisions/0175-the-data-and-device-packs-take-up-the-second-pass-of-the-provider-contract.md)).
- **A store in a root a person added.** The manifest lists roots by name, so the pack can declare only `project`.
- **How the engine gives the driver the store's file.** Nothing in the plan says that a surface's address is a file in a root, so the engine hands none over. Until a plan can say it, a run that was handed no path gets the file through the pack's own roots: the surface's root and its address below it, checked by the one check that makes a path ([decision 0119](../../decisions/0119-the-product-crate-as-building-it-settled-it.md)).
- **A statement interrupted at its time limit** is built and not proven: the stand cannot slow a statement.
- **A read-only file** maps to `denied_by_store` and is not proven here: the stand's fault does not bind the superuser the tests ran as.
- **Functions that reach outside**, columns of floating type, and reading back from a replica: as in section 307. The embedded engine has no such function once `load_extension` is off, and the pack registers none.
- **A hand-written read may compute what it returns.** Its columns must bear the names of the record's columns, and nothing shows that a value is the stored one and not an expression. That is why a hand-written read is never a read-back. Holding its columns to the store's own account of where each comes from is not built.
- **A key whose column folds case or trailing spaces.** The store then finds a row whose stored key differs from the bound one in case, and the driver stops the read as `read_not_by_key`. Such a key must be asked for exactly as stored.
- **The version guard trusts the store to change the version on every write.** The pack compares the version and does not check that the write changed it. Where no trigger of the store does, two writes with the same version both pass the guard; the engine's record lock and freshness check are then the only bar.
- **A catalog of more than 2000 tables** stops every write that declares no hazard, for the embedded store. The server store asks of the one table, with no cap.
- **Rows past a read's limit** are not kept by the server store, but the server sends them until the statement ends or its time runs out: a portal with a row limit needs the client's transaction object.
- **A foreign key from a table of another schema** is counted by the server store's check of what a write reaches, and is not in a finding, which reads one schema.
- **The server stand on macOS and Windows.** It starts its own server only where the programs and a Unix socket are; elsewhere `A2M_TEST_POSTGRES` names one, or the server tests say they skipped.
