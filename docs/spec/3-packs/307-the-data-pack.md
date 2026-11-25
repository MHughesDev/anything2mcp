# 307. The data pack

The data pack reaches data stores directly: relational databases first, and reads before writes; document and key-value stores later. It is a request pack ([section 201](../2-kernel/201-access-kinds-and-modes.md)). It brings one mode, `sql`, for relational stores, registered by [decision 0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md) under the rule of [record 0060](../../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md). What is built so far, and what is not, is in the design of [`a2m-pack-data`](../../architecture/crates/a2m-pack-data.md). Packages G1 to G3 of the [roadmap](../../plan/roadmap.md) build it. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

The pack exists because a database is the plainest place to read many records, and because a write to one can be checked with exactly the same tool that made it: a query by key. It is also where an overlay can do the most damage, so most of this section is about what it refuses.

## What it is for, and what it refuses

| For | Refused |
| --- | --- |
| Reading records by key, and lists with a row limit, through declared statements | A statement assembled from inputs. Every statement is text in a target file, reviewed and hashed with the compiled target |
| Writing one record, or one parent with its rows, by key, inside one transaction | A table name, a column name, a sort order, or any other part of a statement chosen by an input |
| Calling a declared stored procedure, as `irreversible` unless a read-back is declared | A statement that changes the schema, grants rights, or administers the store; one that opens another file, changes a setting of its connection, or begins or ends a transaction. The pack has no verb for them, and refuses such a statement whatever it is declared as ("What a read-only connection does not stop", below) |
| Reading the store's own catalog as discovery | Sampling values during discovery unless a person asks and policy allows ([section 218](../2-kernel/218-discovery-and-findings.md)) |
| One connection pool per identity | A shared administrative identity used for everyone |

## Writing beneath an application

A database that an application owns is a lower level of that application ([section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md), rule 4; [section 106](../1-product/106-what-it-latches-onto.md)). Writing to it directly skips the application's validation, its derived values, its audit, and its caches. Reads there are allowed freely. A write there needs the system owner's word.

A `database` surface in `target.yaml` therefore declares what it is.

| `stands` | Means | A write binding |
| --- | --- | --- |
| `alone` | The store is the system of record: nothing else applies rules to it | Compiles as usual |
| `beneath: <surface>` | The store belongs to the application on the named surface | Compiles only when the target's authorization record names this route as supported by the system's owner. The binding is shown first in the review summary, and is `irreversible` unless its read-back uses the application's own surface |
| Not declared | Unknown | A write binding does not compile. Reads compile |

The authorization is a stored record of who agreed, when, and in their own words ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). On one machine the person who added the target gives it; it is never inferred from the fact that the identity can write. Rule 3 of section 401 then applies: a write through the application with read-back from the database is the preferred shape, and hypothesis H-30 measures what it catches.

## The file kind: `statements/`

The pack brings one file kind. A file under `statements/` names a `database` surface, declares the record kinds on it, and declares every statement any binding may run. The name is `statements/` rather than `queries/`, because the kind holds writes as well as reads.

```yaml
# statements/orders.yaml, in the imaginary example target
surface: orders_db                 # a database surface in target.yaml, with stands: alone
records:
  line_item:
    table: order_lines
    key: [order_id, item_code]     # the fields that identify one row, parent first
    version: row_version           # optional: a column the store changes on every write
    fields:
      order_id:   {column: order_id,   type: identifier}
      item_code:  {column: item_code,  type: identifier}
      length:     {column: length_in,  type: length_inches}  # declared in types.yaml
      unit_price: {column: unit_price, type: money}
statements:
  line_item_by_key:
    effect: read
    form: table                    # the pack writes the read of one record by its key:
    record: line_item              # zero rows is "absent"; two is a failure
    isolation: read_committed
    time_limit: 2s
  lines_of_order:
    effect: read
    record: line_item
    text: |
      SELECT order_id, item_code, length_in, unit_price, row_version
      FROM order_lines WHERE order_id = ? ORDER BY item_code
    params: [{name: order_id, type: identifier}]
    returns: many
    max_rows: 500
    time_limit: 5s
  line_item_update:
    effect: write
    form: table                    # the pack writes the statement from the declaration
    record: line_item
    verb: update
    set: [length, unit_price]
    guard: version                 # or before_values, or none
    isolation: read_committed
    time_limit: 5s
```

An operation binds to a statement like this:

```yaml
bindings:
  - mode: sql
    verb: update
    statement: line_item_update
    params: {order_id: input.order_id, item_code: input.item_code,
             length: input.length, unit_price: input.unit_price,
             version: input.row_version}
    read_back: line_item_by_key
```

| Field | Meaning |
| --- | --- |
| `records.<kind>.table`, `key`, `fields` | One table or view, the fields that identify a row, and each column's field type. Units live in the field type and nowhere else ([decision 0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md)) |
| `version` | A column the store changes on every write. Sent back as a precondition, as an `ETag` is for HTTP ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)) |
| `effect` (required) | `read` or `write`. A `read` runs only on a connection the store holds read-only (below) |
| `form` | For writes: `table`, the pack writes the statement from `record`, `verb`, and `set`; or `statement`, hand-written `text` that must declare `hazard: data_elsewhere`. For reads: `table`, the read of one record by its key, which the pack writes from `record`; it is the only read a binding may name as `read_back` ([decision 0141](../../decisions/0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md)). A read with no `form` is hand-written |
| `text` | For hand-written reads and `form: statement`: the statement, in the store's own dialect, with the dialect's positional placeholders. Never a template, never a substitution. One statement: a read begins with `SELECT` or `WITH` |
| `params` (required) | The parameters in placeholder order, each with a field type |
| `returns` | `one_or_none`, `one`, `many`, or `none` |
| `max_rows` | Required with `many`. The pack fetches one row more, so that "more exist" is reported as a fact |
| `isolation` | A standard isolation level: `read_committed`, `repeatable_read`, or `serializable`. A level the store does not offer fails the probe as `misconfigured` |
| `time_limit` (required) | Set in the store as a statement timeout where the dialect offers one, and held by the driver as well |
| `guard` | For `form: table` updates and deletes: `version`, `before_values` (the preview's values of the columns set, for types that compare exactly), or `none` |

**How parameters are bound.** Only through the database driver's own parameter binding, as a prepared statement. The text is a constant of the compiled target. Nothing an input holds is ever placed into it, and the compiler has no way to do so: there is no expression field in `text`. A `form: table` statement is written by the pack from identifiers checked against the store's catalog and quoted by the dialect's rule.

**How the pack knows a statement's shape.** It asks the store. When a statement is checked, and again when a session starts, the driver prepares it without running it, and the store reports the number and types of its parameters and the names and types of its result columns. The driver holds `params` and `fields` to that description the first time a session uses the statement, because the compiler has no connection; a difference is `drift`. The pack never parses a statement to learn what it does. It does read a statement's words, outside quotes and comments, to refuse a form: more than one statement, a first word that is not the one its effect allows, and the words that attach a file, change a setting, or change the schema. Because a store's reader may end a comment or a word where that reading does not, a statement that holds any control character but a line feed and a tab is refused.

## How a read is proved a read

Not by reading the statement. **By the store's refusal.** A `read` statement runs only on a connection that the store itself holds read-only.

| Store | How | Checked by |
| --- | --- | --- |
| A server database | A read-only transaction, in the SQL standard's sense, begun for every read | After beginning, the driver asks the store for the transaction's access mode and goes on only if it says read-only ([section 213](../2-kernel/213-effects-hazards-and-consent.md), "set, read back, go on only if it took") |
| An embedded database file | The file opened in the engine's read-only mode, with writes refused on the connection | The same: the setting is read back before the first statement |
| A store that offers neither | Not supported for reads. The probe reports `unsupported_version` with the reason | Not applicable |

A read statement that tries to change data is refused by the store, rolled back, and stops as `policy` with the code `read_statement_wrote`. The binding is marked unhealthy, and the finding is shown in the next review summary. This is the scout's licence to try a drafted read ([section 402](../4-building-tools/402-the-scout-and-who-does-what.md)): the engine enforces it.

A read-only transaction stops changes to stored data. It does not stop a function that reaches outside the store. That gap is an open point below.

**What a read-only connection does not stop.** In the embedded engine, a connection opened read-only will still attach another file that a parameter names, change its own settings, create a temporary table, and end the transaction it runs in. None of these is a change to stored data, and each returns no rows, as a read that finds nothing does. So a statement is held to one plain read or one keyed write three times ([decision 0141](../../decisions/0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md)): the compiler refuses any other by its words; the driver refuses it again before it is sent, with a read that returns no column and a read the store says is not read-only; and the connection is set, with each setting read back, so that it can attach no file, load no extension, and run no statement that reads or changes a setting, changes a schema, or controls a transaction. The file is opened by an absolute path only, never by an address that carries options. These refusals stop as `policy`, `statement_not_allowed`.

## Verbs and steps

The request access kind's existing steps are HTTP's. The pack adds its own, fixed when it is built: `begin` (a transaction with a declared isolation and access mode), `execute` (one prepared statement with bound parameters), `fetch` (rows into typed values, up to a limit), `expect_rows` (the count against the declared one), and `commit`. Rollback is not a step. The driver rolls back whenever a step fails before `commit`, and when it releases a connection.

| Verb | Lowers to | Commit point |
| --- | --- | --- |
| `read` | `begin` read-only, `execute`, `fetch` with `one_or_none` | None |
| `list`, `search` | `begin` read-only, `execute`, `fetch` up to `max_rows`. The rows come back as records under `rows`, each field also as a list of its values in row order, and `more`; an operation's list outputs are read from the lists ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)) | None |
| `create` | `begin`, `execute` an insert, `expect_rows` 1, `fetch` any key the store assigned, `commit` | `commit` |
| `update` | `begin`, `execute` an update with the key predicate and the guard, `expect_rows` 1, `commit` | `commit` |
| `upsert` | Decided at the preview by reading ([section 209](../2-kernel/209-record-identity-and-dependent-records.md)), then as `create` or `update` | `commit` |
| `delete` | `begin`, `execute` with the key predicate and the guard, `expect_rows` 1, `commit`. Read-back expects absent | `commit` |
| `call` | `begin`, `execute` the declared procedure, `commit`. `irreversible` unless a read-back statement is declared | `commit` |

A parent with its rows saved in one transaction is one commit point and one ledger row; read-back compares each row by its own key, as section 209 describes for one save that carries many records.

## The driver, part by part

| Part | What the driver does |
| --- | --- |
| Prepare | Takes a connection from the session's pool. Checks the store's reported version against the target's recorded identity ([section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md)). Prepares the statements the step will use and compares their description with the compiled target |
| Act | `begin` with the declared isolation; `execute` with bound parameters; for a write, `expect_rows` inside the transaction. Any count other than the expected one rolls back, so nothing is committed. Then `commit`, which is the commit point |
| Confirm | The store's acknowledgement of the commit. Rows affected were already checked before it |
| Read back | The read-back statement, in a new transaction on a connection that is read-only, after the commit. Never inside the writing transaction, which would see its own uncommitted rows. It is the read by key that the pack wrote for the written record, and the record it returns must hold the key that was bound; any other is `drift`, `read_not_by_key` |
| Release | Rolls back anything open and returns the connection. A connection whose state is in doubt after a failure is closed, never returned |

There are no retries inside the driver. A serialization failure or a deadlock is rolled back by the store and returned as `busy`, `store_locked` ([decision 0168](../../decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md)); the engine decides whether to try again, and it does so only for reads and for keyed writes, which are idempotent.

**Confining a write to the rows it names.** Three things, each enforced by the store or by construction:

1. A `form: table` update or delete always carries the full key predicate, because the pack writes the statement. There is no table-form statement without one.
2. `expect_rows` runs inside the transaction. Zero rows is `stale` (the record or its version changed since the preview); more than one is `drift` (the key is not unique, which the catalog said it was). Both roll back. A writing transaction runs one statement, and the driver accepts no expected count but one, so the count checked is the count of everything committed.
3. A guard of `version` or `before_values` makes the write itself a compare-and-set. The record lock and the engine's freshness check ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)) remain; the guard closes the window between that check and the write.

Rows affected counts the rows of the named table. It does not count what a trigger or a cascading key does to other tables. Those are hazards, below.

**Outputs before the commit point.** A `create` learns the key the store assigned before it commits, from `fetch`. The engine must store that output in the ledger before the `commit` step is acted, so that a lost answer to the commit can be reconciled by the key. The driver interface as sketched has no place for an output reported between two steps of one commit point. It is needed here and by the file pack ([section 308](308-the-file-pack.md)), and is to be settled with the driver interface.

## A lost connection

The driver knows which side of `commit` a failure fell on, and says so.

| Lost | Means | Class |
| --- | --- | --- |
| Before `commit` was sent | The store rolls back an open transaction when its connection ends. Nothing landed | `transient`, `connection_lost` |
| After `commit` was sent, before its answer | The write may have landed | `possible_commit`, `no_confirmation` |

On resume the engine reconciles as section 208 says: a new connection, the read-back statement by key, and the three-way comparison of current values with the intended values and the values seen before. Where the dialect can report whether a transaction committed by its identifier, the driver reads that identifier before `commit` as an output before the commit point, and reconcile asks the store directly; the comparison of values still follows, as the proof.

## Identity

| Strategy | In a data store |
| --- | --- |
| `natural`, `composite` | The table's primary key or a unique, non-null constraint the catalog reports. The usual case |
| Key assigned by the store, returned by the statement | Captured from `fetch` before `commit`, as above. From then on the record is found by it |
| `stamped` | A spare column the target declares, written with the record's idempotency token. Needed only when the dialect cannot return an assigned key |
| `find` | A declared read statement on values the insert sets, which must match one row ([section 209](../2-kernel/209-record-identity-and-dependent-records.md)) |
| `none` | A table with no key and no unique columns. Writes are `irreversible` |

If the store assigns a key, the answer to `commit` is lost, and the key was captured before it: reconcile reads by that key. Absent means the write did not land. A server sequence does not hand the same value out again after a rollback; an embedded engine may reuse the next row number, so a row found by that key with other values is neither value, and stops as `stale`. If the key was not captured, `stamped` or `find` is the only way back, and without either the create is `irreversible`.

## Sessions and secrets

- **A session is an identity's credential and a small pool of open connections**, held by the driver on the session's home runner ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)). Runs on the same identity share the pool; each step holds one connection for one transaction. Different identities never share a connection. A connection is kept under everything that says where it leads and as whom (address, port, database, schema, link mode, role), and is held to the compiled target's surface at every prepare; before every transaction the store is asked where it is, and a connection that leads elsewhere is closed and nothing is sent ([decision 0184](../../decisions/0184-the-data-and-stream-packs-after-their-second-safety-audit.md)).
- **The credential lives in the vault** and reaches the connection only through `inject` at connect time. The pack builds connection options as structured fields: there is no connection string in any file, and none is ever built as text with a password in it. `auth.yaml` holds vault references only.
- **Errors are redacted twice.** The password is registered with the redactor, and the outside driver's error text is kept only as untrusted evidence, after redaction. What reaches a caller is the code and the pack's own sentence.
- **No shared administrator.** The probe reads the identity's attributes from the catalog. An identity the store reports as administrative, or able to grant rights, is refused with `policy` and `identity_is_administrator`. For PostgreSQL that is a superuser, a role that bypasses row security, may replicate, or may make roles, a role that owns anything in the surface's schema, and a member of a predefined role that reaches the server's files, programs, other sessions, or every table. Each person uses their own database identity, or a service identity that a person was given for that level ([section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md): a level is not a privilege).
- **Auth providers.** A password provider for database identities, which the pack brings because signing in is opening a connection; and the neutral `client_certificate` and `windows_integrated` providers where the store accepts them. The probe is a declared read statement.
- **The link is encrypted.** A server surface requires an encrypted, verified connection unless it is a loopback address, and a credential is never sent over an unencrypted link to another machine. As built, the encrypted link is not, so a server surface is reached only at a loopback address written as an IP address, and any other stops before anything is sent as `policy`, `link_not_encrypted`; a target file has no switch for it ([decision 0168](../../decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md)).

## Effects and hazards

The pack declares three effects, each `enforced`: it uses the network, to the host and port of each declared `database` surface and never to an address from an input; it reads and writes files, for an embedded database only, inside an allowed root ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)); and it holds sessions for `database` surfaces.

| Situation | Hazard |
| --- | --- |
| A `form: table` write to a table with no triggers and no cascading keys, as the catalog reports | `none` |
| A table with a trigger, or a key that cascades a delete or an update | `data_elsewhere`. The diff names the trigger or the dependent tables |
| A `form: statement` write | `data_elsewhere`, always: the pack cannot see what it touches |
| A procedure that the target declares as sending something out | `sends_outside` |

Any hazard other than `none` makes the operation `irreversible` and needs a person every run ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). A trigger that appears after publishing is drift: the catalog is part of the target's identity. Names are compared as the store compares them, without regard to the case of letters. A catalog that could not be read whole, or that does not hold the written table, does not show that a write reaches nothing else: the write stops, as `unknown_variant`, `catalog_too_large`, or as `drift`.

## Discovery and drafting

The discoverer reads the store's own catalog, through the SQL standard's information schema where the store offers it and the dialect's catalog otherwise, inside a read-only transaction.

| Finding | Level | Notes |
| --- | --- | --- |
| Tables, views, columns and their types, primary keys, unique and foreign keys, check constraints, triggers | `discovered`, source `live_system` | Coverage is `partial` with `permission_denied` when the identity cannot see the whole catalog |
| The identity's own rights | `discovered` | Used by the probe, and to draft no write the identity cannot make |
| Row counts | Not taken by default | Values and counts are data |
| A sampled document's shape in a document store | `inferred`, with `sampled` | Field names, types, and how often each was present. Never values |

| The drafter writes | It leaves to the developer |
| --- | --- |
| One record kind per table with a declared key; field types from column types, with places and scale where the store states them and no unit. A server's `numeric(p,s)` is `decimal` where the built-in type keeps its scale; the embedded engine's `NUMERIC` may hold a float and stays a question | Units, business names, choice codes the store does not enumerate |
| A `read` statement by key for each such table and a list of its rows in the order of its key, bounded by the pack's row limit, and a read operation over each, provisional names from the table's words ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)) | Joins, which tables matter |
| Nothing for tables without a key, beyond an open question | Every write, its strategy, and whether the route is supported beneath an application |

The scout tries each drafted read in the sandbox, read-only, five times. A read by key has no example, because discovery reads no row: the drafted list is tried, and the read by key waits for a key a person gives. Whether the scout may take a key from the list it tried is open. The trace stores the digests and shapes of results, not their values.

**A column named as a secret is never drafted as a field or an output.** The drafter asks the compiler's own test (`names_a_secret` in `a2m-files`) of each column, as the store writes its name and as the field it becomes. Such a column is no field of the drafted record, no part of the drafted list's text, and no output of either read, and the last sentence of each drafted read's description names the fields left out. A key that holds such a column is no key: the draft holds the question of the key, as for a table whose store states none. The compiler's refusal of a secret in a file does not change ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

## Failures

| What goes wrong | Class | Code |
| --- | --- | --- |
| Authentication refused; a credential that a person must change | `session_expired` | `needs_login`; the second leaves the session `needs_person` |
| The server cannot be reached, refuses connections, or is in recovery | `transient` | `upstream_unavailable` |
| The connection ends before `commit` was sent | `transient` | `connection_lost` |
| The connection ends after `commit` was sent | `possible_commit` | `no_confirmation` |
| Statement or lock wait timeout; serialization failure or deadlock, rolled back | `transient` | `timeout`; `serialization_failure` (proposed) |
| Too many connections for the identity | `busy` | `session_in_use` |
| An embedded store's lock is held by another connection for the whole time limit | `busy` | `store_locked` ([decision 0101](../../decisions/0101-what-the-data-pack-refuses-and-how-it-names-it.md)) |
| A unique, check, not-null, or foreign-key constraint refuses the write | `rejected` | `constraint_violated` (proposed), the constraint's name as untrusted text |
| The store denies a right; a read statement tried to write; the identity is administrative; the certificate does not verify | `policy` | `denied_by_store`, `read_statement_wrote`, `identity_is_administrator`, `server_identity_mismatch` (all proposed) |
| A statement is not one plain read or one keyed write | `policy` | `statement_not_allowed` |
| A table, column, or parameter differs from the compiled target; a key matched more than one row; a read by key returned another record | `drift` | `schema_changed`, `key_not_unique` (proposed), `read_not_by_key` |
| The catalog is too large to show what a write reaches | `unknown_variant` | `catalog_too_large` |
| A write affected no row: the guard did not hold | `stale` | `changed_since_preview`, or `version_conflict` with a `version` guard |
| An error code from the store with no rule | `unknown_variant` | `unexpected_status` |
| Read-back differs | `mismatch` | `read_back_differs` |

A `many` read that finds more than `max_rows` is not a failure: the result says that more exist.

## The test stand

As real as is cheap ([the test application](../../plan/test-application.md)). The cheap and real stand is **the embedded SQL engine the store already uses on one machine** ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)): a real database engine, no server, the same on three operating systems. It lives in `targets/example-db/`, with seed tables for the `example` target's orders and line items; its [README](../../../targets/example-db/README.md) lists the schema, the faults, and how each case below is shown.

**The control interface** reaches the database by its own handle, never through the pack. Reset restores the seed. Inspect returns a row exactly as stored. Count writes reads a log kept by the stand's own triggers, which the pack's statements never name. Set a fault holds the engine's lock from another connection, changes how a written value is stored, refuses a write, changes the schema, or marks the file read-only. The embedded engine locks the whole database and not a row, and gives the stand no way to slow another connection's statement; a row lock and a slow statement are left to the server stand.

| # | The stand does this | Risk it exercises |
| --- | --- | --- |
| 1 | Serves orders and line items with a composite key and a version column | Read by key; list with a limit; `one_or_none` |
| 2 | Has a statement declared `read` that writes | Refused by the store; `read_statement_wrote` |
| 3 | Refuses a write by a constraint | `rejected` with the constraint's name |
| 4 | Has a trigger that writes another table, and a cascading delete | Hazard `data_elsewhere` found by discovery |
| 5 | Assigns a key on insert, with and without a spare reference column | Captured key; `stamped`; `find` |
| 6 | Lets a second connection change a row between preview and write | The guard; `changed_since_preview` |
| 7 | Stores a value differently from what was sent: a decimal at another scale, padded text, a time in another zone | Normalization; `mismatch` |
| 8 | Holds text with instructions aimed at an agent | Untrusted fields |
| 9 | Has a table with no key | No read by key is drafted; writes are `irreversible` |
| 10 | Changes a column between two scouts | `schema_changed`; levels fall |

**The server stand is built** as `example_db::server`, with a real PostgreSQL server started by the test or named by `A2M_TEST_POSTGRES`, not a container; a server surface is of the kind `database_server` ([decision 0168](../../decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md)). Partial catalogs and certificate checks are not yet among its cases. **To trust the pack against a server database,** an embedded engine is not enough. It has no identities and rights, no network, no read-only transactions in the server sense, and nothing to lose after a commit. A second stand is needed: a server database run locally in a container, of the kind the store uses on servers, with a control identity the pack never uses, triggers that log every write to a table only that identity can read, and a small network relay in the stand that can cut the connection after forwarding a commit. It adds the cases of a lost answer after `commit`, an administrative identity refused, a partial catalog, isolation levels, statement timeouts, and certificate checks. Containers run in CI on Linux only; on Windows and macOS the server cases are a gap written in the journal, and the embedded cases run on all three.

## Outside drivers needed

Stated as needs. Each is checked before it is added: maintained, under a licence on the closed list in `deny.toml`, and building on Windows, macOS, and Linux without a system library a person must install.

| Need | Checks beyond the usual |
| --- | --- |
| A binding to the embedded SQL engine, built from bundled source | The engine's own source carries a public-domain dedication, which is not on the closed list. Whether that needs a line in the list is decided on purpose, with a line in the journal, before it is added. The same binding may already be the store's |
| A client for each server protocol supported, asynchronous on Tokio, preferably in Rust alone | Prepared statements with parameter binding; reporting a statement's parameter and column types without running it; read-only transactions; cancelling a statement; the workspace's one TLS stack |
| Exact decimal values from numeric columns | Into the core's decimal type, never through a float |

One client per protocol is preferred to a layer over many databases: the pack needs each protocol's own answers about read-only mode, statement descriptions, and errors, and a layer tends to flatten exactly those.

## Document and key-value stores

They wait. They fit the request access kind ([section 201](../2-kernel/201-access-kinds-and-modes.md)): a read by key, a keyed write, the store's acknowledgement. But they offer no statements to declare, and many have neither transactions nor a read-only mode the store enforces, so the proof of a read moves from the transaction to the identity's rights, which the store must report. They will get a mode of their own, by its own record, after relational writes are proven. Their discoverer is already described above: sampled shapes, `inferred`.

## The entry bar

| # | Item of [section 202](../2-kernel/202-kernel-and-packs.md) | Answered by |
| --- | --- | --- |
| 1 | A driver, no retries inside | The driver, part by part |
| 2 | A read-back rule for every write | A read statement by key, in a new transaction; a `call` without one is `irreversible` |
| 3 | Each identity strategy | Identity |
| 4 | A test stand with ground truth | The embedded stand, and the server stand for what it cannot show |
| 5 | A discoverer that claims no more than it saw | The catalog as `discovered`; samples as `inferred` |
| 6 | Failures mapped to the classes | Failures |
| 7 | Effects and hazards | Effects and hazards |
| 8 | A decision record for the mode | `sql`: [decision 0100](../../decisions/0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md) |

## Build order

1. **G1.** The embedded stand and its control interface, with its own tests.
2. **G2.** A read by key through the engine loop: a `statements/` file, prepare and describe, a read-only connection read back, `one_or_none`; the proof is that a statement that writes is refused by the store. Then the catalog discoverer, the drafting rules, and the scout's tries, which measure hypothesis H-31 for data stores. Then the server stand, and the same reads against it.
3. **G3.** `form: table` updates with a guard and `expect_rows`; read-back by key; `possible_commit` through the relay that cuts the connection. Then inserts with an assigned key, `stamped`, and `find`; then `form: statement` writes, procedures, and `stands: beneath`.
4. Record what the kernel had to change, for hypothesis H-26, and whether `sql` fitted the request access kind without bending it, for H-29.

## Open points

- **Functions that reach outside.** A read-only transaction does not stop a function in a read statement from sending something out of the store. Whether the probe should list such functions from the catalog and refuse statements on a store that has them, without parsing statements, is not settled. The embedded engine has none once extension loading is off, which the pack sets; the question is open for server stores.
- **Outputs before the commit point.** The driver interface needs it for assigned keys and transaction identifiers. To be settled with the interface.
- **Which dialects first.** The embedded engine's, then one server protocol. The choice of server protocol is a decision at G2.
- **Reading back from a replica.** A replica may lag behind the write. Read-back goes to the store that took the write until a settle time is designed for replicas.
- **Columns of floating type.** They cannot be compared exactly. Mapped to `decimal` with an open question, or refused as a guard, until a rule is chosen.
- **An embedded database file is also a file.** It belongs to this pack, by its format, and never to the file pack. A file pack definition that names one does not compile.
