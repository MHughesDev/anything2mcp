# The data pack's test stand

One database file of the embedded SQL engine, holding the schema of an imaginary order-entry system named `example`. The data pack is proven against this file and never against a real store. [Spec section 307](../../docs/spec/3-packs/307-the-data-pack.md) lists the cases a stand for the pack must show, and the [test application plan](../../docs/plan/test-application.md) gives the rules for every stand. This is the embedded stand of roadmap package G1. The server stand that section 307 also asks for is its server side, under "The server side" below.

## Using it in a test

A crate uses the stand in its tests only:

```toml
[dev-dependencies]
example-db = { path = "../../targets/example-db" }
```

```rust
use example_db::faults::Fault;
use example_db::harness::TestDb;
use example_db::schema::Table;

let db = TestDb::start()?;          // a new file in a new folder under the temporary directory
let path = db.path();               // the code under test opens its own connection to this
// ... the code under test reads and writes ...
let row = db.stored_row(Table::OrderLines, &["A-100", "L-101"])?;   // exactly what is stored
let count = db.write_count(Table::OrderLines, &["A-100", "L-101"])?; // how many times it was written
db.set_fault(Fault::Locked)?;       // every fault is a setting
```

`TestDb` is `Send` and `Sync`. Dropping it closes the stand's connection and removes the folder. Drop the connections of the code under test first: some systems do not remove a file that is still open. Two stands share nothing.

| Where the store is | Call | Gives |
| --- | --- | --- |
| The file | `path()` | A path under `std::env::temp_dir()` |
| An address for reading and writing | `uri()` | `file:///...?mode=rw`, which never creates a missing file |
| An address the store holds read-only | `read_only_uri()` | `file:///...?mode=ro` |
| An ordinary connection, for a test's own use | `connect()`, `connect_read_only()` | A connection opened with the address above. It is not the control interface: its writes are logged |

## The schema

No column holds a float. Money and lengths are decimal text, or whole cents and grams. Dates and times are ISO 8601 text.

| Table | Key | Columns | What it is for |
| --- | --- | --- | --- |
| `customers` | `customer_id` | `name`; `email`, unique, may be null; `credit_limit_cents`, checked not negative; `notes`, may be null | A natural key, a unique constraint, a check constraint. No trigger of the system's own |
| `orders` | `order_id` | `customer_id`, a foreign key to `customers`; `status`, checked to be `draft`, `submitted`, or `cancelled`; `ordered_on`, declared `DATE`; `required_by`, `DATE`, may be null; `submitted_at`, declared `TIMESTAMP`, may be null; `customer_ref`, unique, may be null | A foreign key, dates, nulls. A change of `status` writes `order_status_history` |
| `order_lines` | `order_id`, `item_code` | `description`, checked not empty; `quantity`, checked 1 to 9999; `length_in`, decimal text; `unit_price`, decimal text; `row_version` | A composite key, and a version the store raises by one on every update. Deleting an order deletes its lines |
| `shipments` | `shipment_id`, assigned by the store | `order_id`, a foreign key that refuses the deletion of an order with a shipment; `carrier`; `shipped_on`, may be null; `weight_grams`, may be null; `external_ref`, unique, may be null | An assigned key that is reused after the last row is deleted, and a spare reference column |
| `order_notes` | `note_id`, assigned by the store | `order_id`, deleted with the order; `author`; `body` | An assigned key that is never reused, and no spare column |
| `order_status_history` | None | `order_id`, `old_status`, `new_status` | A table with no key, written by a trigger |

The view `order_summaries` gives one row per order: `order_id`, `customer_name`, `status`, `line_count`, `total_quantity`.

The system's own triggers are `orders_record_status_change` and `order_lines_bump_version`.

Foreign keys are enforced by each connection, not by the file. The engine the workspace builds enforces them unless a connection switches them off.

### The stand's own objects

Everything whose name starts with `stand_` belongs to the stand: the tables `stand_state` and `stand_write_log`, and the triggers that keep the log and carry the faults. A pack's statements never name one. A catalog read sees them, because the engine has nowhere to hide them. `set_write_log(false)` takes the logging triggers out of the catalog for a test of what discovery finds; the two tables stay.

The version trigger reads `stand_state`. That is how a change the store makes to a row it has just written is kept from counting as a second write, whether or not the writing connection lets a trigger fire itself.

## Seed data

| Table | Rows |
| --- | --- |
| `customers` | `C-001` Example Customer One; `C-002` Example Customer Two; `C-003` Example Customer Three, with no email, and notes that hold instructions aimed at an agent |
| `orders` | `A-100`, a draft for `C-001`, reference `PO-7781`; `A-101`, submitted, for `C-002`, reference `PO-7790`; `A-102`, a draft for `C-003` with no lines |
| `order_lines` | `A-100`: `L-101` Left rail, 2, `150.5`, `12.50`; `L-102` Right rail, 2, `150.5`, `12.50`; `L-103` Cross brace, 4, `36.0625`, `4.75`. `A-101`: `L-201` Base plate, 1, `24`, `31.00`. Every version is 1 |
| `shipments` | `1`, for `A-101`, reference `SHIP-0001` |
| `order_notes` | `1` on `A-100`; `2` on `A-101`, whose body holds instructions aimed at an agent |
| `order_status_history` | `A-101` from `draft` to `submitted` |

## The control interface

The methods of `TestDb`. They run on the stand's own connection, which the code under test never uses, and they report what is committed.

| Call | What |
| --- | --- |
| `reset()` | Restore the schema and the seed in the same file; empty the log; switch every fault off |
| `stored_row(table, key)` | Exactly what is stored for one row, with each value's storage class, or `None`. The key has one text per key column; for the table with no key, the engine's row number |
| `stored_rows(table)` | Every row of a table, in key order |
| `columns(table)`, `triggers()` | The catalog as it is now: each column's name, declared type, and place in the key; each trigger's name and table |
| `writes()` | Every row inserted, updated, or deleted since the last reset, in order: the table, the key, what happened, and whether the control interface did it or another connection |
| `write_count(table, key)` | How many times another connection wrote that row. A change the control interface made is not counted |
| `data_version()` | A number that differs from an earlier reading if any other connection committed anything in between |
| `someone_else(statement, values)` | Run one statement as another user of the store. Logged as the control interface's, and it moves versions as any write does |
| `set_fault(fault)`, `clear_fault(fault)`, `clear_faults()`, `faults()` | Switch a fault on or off; list the ones that are on |
| `set_write_log(on)` | Install or remove the logging triggers. While off, `writes()` and `write_count()` return an error rather than a count |

**What the stand can observe of statements.** The engine shows one connection nothing of what another runs. The log is therefore of rows changed, not of statements: a statement that changes three rows leaves three records, and a read leaves none. A row deleted by a cascading key is logged. `data_version()` is the only sign of a commit that touched no logged table.

## Faults

Each is a value of `example_db::faults::Fault`, stays on until cleared or reset, and can be combined with the others.

| Fault | What the store does | What another connection sees |
| --- | --- | --- |
| `WritersLocked` | The control interface holds the engine's write lock | Reads work; a write is answered busy |
| `Locked` | The control interface holds the engine's exclusive lock | A read or a write is answered busy |
| `LengthRescaled` | A length written to `order_lines` is stored at four places: `150.5` as `150.5000` | Read-back differs in text and is the same value |
| `PriceRoundedDown` | A price written to `order_lines` loses its cents: `12.49` as `12.00` | Read-back is a different value |
| `DescriptionPadded` | A description is stored padded with spaces to 24 characters | Read-back has trailing spaces |
| `TimeShifted` | A time written to `orders.submitted_at` is stored five hours behind: `2026-03-06T09:30:00+00:00` as `2026-03-06T04:30:00-05:00` | Read-back is the same instant in another zone |
| `LineWritesRefused` | A rule in the store refuses every insert, update, and delete on `order_lines` | A constraint failure with the message `order lines are frozen` |
| `ColumnRenamed` | `order_lines.length_in` becomes `length_inches` | A statement that names the old column no longer prepares |
| `ColumnRetyped` | `order_lines.unit_price` becomes a whole number of cents, declared `INTEGER`, and moves to the end of the table: `12.50` as `1250` | The declared type, the stored value, and the column order differ |
| `ReadOnlyFile` | The file is marked read-only for the operating system | A connection opened afterwards reads, and its writes are refused as read-only |

Three things a test should know:

- **A lock is on the whole database.** The engine has no row lock. One lock is held at a time. While it is held the control interface still reads and writes on its own connection, and what it writes is committed when the lock is cleared. `Locked` stops readers in the engine's default journal mode, which is the one the stand creates; a connection that switches the file to write-ahead logging changes that.
- **`ReadOnlyFile` does not bind a privileged user.** `set_fault` then returns `StandError::FaultNotEffective` and leaves the file writable, so that no test passes without the fault. A connection that was open before the fault keeps the access it had.
- **A constraint that refuses a write needs no fault.** The schema's check, unique, not-null, and foreign-key constraints are always there. `LineWritesRefused` is for refusing a write that the constraints would allow.

## Cases

The numbers are those of the stand table in spec section 307. All ten are shown; the stand's own tests in `tests/cases.rs` prove each one through an ordinary connection.

| # | The stand does this | How |
| --- | --- | --- |
| 1 | Serves orders and line items with a composite key and a version column | `order_lines`, read by key and by order |
| 2 | Has a statement declared `read` that writes | `example_db::schema::READ_THAT_WRITES` returns rows and changes one. On a read-only open the store refuses it |
| 3 | Refuses a write by a constraint | The named constraints of the schema; `LineWritesRefused` |
| 4 | Has a trigger that writes another table, and a cascading delete | `orders_record_status_change`; `order_lines` and `order_notes` go with their order |
| 5 | Assigns a key on insert, with and without a spare reference column | `shipments` with `external_ref`; `order_notes` without |
| 6 | Lets a second connection change a row between preview and write | `someone_else` |
| 7 | Stores a value differently from what was sent | `LengthRescaled`, `PriceRoundedDown`, `DescriptionPadded`, `TimeShifted` |
| 8 | Holds text with instructions aimed at an agent | `customers.notes` of `C-003`; `order_notes.body` of note `2` |
| 9 | Has a table with no key | `order_status_history` |
| 10 | Changes a column between two scouts | `ColumnRenamed`, `ColumnRetyped` |

## What it does not show

- **A slow statement.** The engine gives one connection no way to slow another's statement. A lock wait is shown by the lock faults; a statement timeout belongs to the server stand.
- **A connection lost after a commit, identities and rights, isolation levels, a partial catalog.** An embedded file has none of them. Section 307 gives them to the server stand.
- **A change between a guard's check and its write inside one statement.** The stand changes a row when a test tells it to, between two calls of the code under test.
- **A column of floating type.** The schema has none, on purpose. The pack's rule for such columns has no case here yet.
- **Who made a write, beyond the control interface or not.** The engine has no identities.

## The server side

`example_db::server::TestServer` is a real PostgreSQL server with the same system's schema in the server's own types, for the data pack's server store ([decision 0168](../../docs/decisions/0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md)). It uses no client library: its control queries run the server's own client, `psql`, so a write is never proven through the library the code under test uses.

```rust
use example_db::server::{TestServer, RelayFault, APP_ROLE, APP_PASSWORD};

let Some(server) = TestServer::start()? else { return Ok(()) };   // None: no server here; the test says it skipped
let address = server.address();      // the relay on 127.0.0.1, at a port the system picked
let database = server.database();    // a new database for this test
// ... the code under test signs in as APP_ROLE with APP_PASSWORD ...
let row = server.stored_row("order_lines", &[("order_id", "A-100"), ("item_code", "L-101")])?;
let count = server.write_count("order_lines", &["A-100", "L-101"])?;
server.arm(RelayFault::LoseCommitAnswer);
```

**How a test gets a server, in this order.**

1. `A2M_TEST_POSTGRES` names a running server, in the server's own form: `host=127.0.0.1 port=5432 user=postgres password=...`, for a role that may make roles and databases. Every operating system.
2. Otherwise, where the server's programs are found (`A2M_POSTGRES_BIN`, `pg_config --bindir`, or the newest `/usr/lib/postgresql/<n>/bin`) and Unix sockets exist, the stand runs `initdb` in a new folder under the temporary folder and starts the server listening on a Unix socket in that folder only, never on a network port. The server refuses to run as the administrator account; a test that runs as that account starts every program of the server through `setpriv` as the account `postgres`, which owns the folder. The server is stopped and the folder removed when the `TestServer` is dropped.
3. Otherwise `start` gives `None`. With `A2M_REQUIRE_POSTGRES=1` it gives an error instead.

**The CI lines.** Linux runners have the programs: set `A2M_REQUIRE_POSTGRES: "1"` and nothing else. macOS runners have no server: `brew install postgresql@16` and `A2M_POSTGRES_BIN: /opt/homebrew/opt/postgresql@16/bin`, or let the server tests skip and write the gap in the journal. Windows runners carry a stopped server: start it and name it, for example `pg_ctl start -D "$env:PGDATA" -w` and `A2M_TEST_POSTGRES: "host=127.0.0.1 port=5432 user=postgres password=root"`. The macOS and Windows lines were not run from here.

| Object | What it is |
| --- | --- |
| Roles | `example_owner` owns the tables (and may sign in, to show a refusal); `example_app` may read and write their rows and owns nothing; `stand_admin` is the superuser of a server of the stand's own |
| Tables | As above, with `numeric(12, 4)` lengths and `numeric(12, 2)` prices, `date`, `timestamptz` `submitted_at`, a `timestamp` with no zone `entered_at`, and a `real` `discount` on `orders`, so that a type with no rule can be shown. `shipments` takes its key from an identity column |
| Triggers | `order_lines_bump_version` raises the version before each update; `orders_record_status_change`; the stand's own `stand_log_order_lines` and `stand_log_shipments`, which write `stand.write_log` through a function that runs as its owner |
| The relay | Passes every byte between the code under test and the server. `arm(LoseCommitAnswer)`: the next `COMMIT` reaches the server and its answer is dropped, and both sides close. `arm(DropCommit)`: the next `COMMIT` never reaches the server |
| Control | `query`, `value`, `execute` (as the administrator, as another program would), `stored_row`, `write_count`, `writes`, `connections_of(role)`, `terminate(role)`, `hold_lock(table, predicate)` |
