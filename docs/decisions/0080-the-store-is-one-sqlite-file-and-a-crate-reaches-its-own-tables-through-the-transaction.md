# 0080. The store is one SQLite file, and a crate reaches its own tables through the transaction

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) and of spec section 208 on durability per commit on SQLite; how the async crates above call a database crate that blocks; how a crate above owns its own tables. **Changes:** the interface sketch of `a2m-ledger` in [record 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md), where `Tx` had async methods.

## Context

[Record 0067](0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md) chose `rusqlite`, which blocks. [Record 0061](0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md) says a service owns the meaning of its tables and the store owns the connection, the transactions, and the migrations. Five crates were about to be written against whatever interface carried those two facts.

## Decision

1. **A transaction is a closure, run on the store's own thread.** `Store::write(durability, |tx| ...)` and `Store::read(|tx| ...)` are async; the closure is synchronous, is handed a `Tx`, and commits when it returns `Ok`. One thread owns the writing connection and runs one transaction at a time. A store on disk has two reading threads, each with a connection, so a read never waits for a write. *Instead of:* a `Tx` with async methods, held by the caller across `await`s. That lets one suspended task hold the single writer for as long as it likes, needs a rollback from `Drop`, and makes every statement a round trip between threads. *Instead of:* `spawn_blocking` around a connection behind a mutex: it borrows the runtime's blocking pool, ties the store to one runtime, and still needs the same closure.
2. **A panic inside a closure is caught.** The transaction is taken back, the caller gets `busy`, `store_unavailable`, and the thread goes on.
3. **Durability is a setting changed per transaction.** `Synced` commits with a full sync; `Relaxed` leaves the sync to the next checkpoint of the write-ahead log, so a crash of the process loses nothing and a crash of the machine may lose the last such commits. One writing connection makes the change safe. *Instead of:* two writing connections, which would contend for the one write lock. Not yet measured against hypothesis H-15.
4. **A crate's migrations are files in `crates/a2m-ledger/migrations/`**, named `NNNN-<crate>-<what>.sql`, whose first line is `-- owner: a2m-<crate>`. The build script lists the folder, so a crate adds a migration by adding one file and edits no code of the store. Each file is applied once, in its own transaction, and its checksum is kept; line endings do not change the checksum. A store that holds a migration this engine does not carry, or carries differently, is refused and left as it was. Numbers need not be dense, and a migration with a lower number than one already applied is still applied, because no crate's migration touches another's tables. Until a release has carried a migration, its owner may edit it; after that, only add.
5. **A crate reaches its tables through `tx.tables(Owner::X)`**, with fixed statements (`&'static str`), numbered parameters, and bound values of four kinds: nothing, a whole number, text, bytes. There is no floating-point value. No type of the database crate is public.
6. **The database's authorizer holds a crate to its own tables.** While a statement is prepared, the store is asked about every table it reads or changes, its triggers included. Under `Owner::X` it allows reading and writing rows of the tables X's migrations created, and nothing else: not another crate's table, not the database's own tables, not ending the transaction, not a setting, not a change of schema. The refusal is `unknown_variant`, `table_not_owned`. A migration of X may not touch a table another crate created. The ledger's own statements are held the same way, with the one declared exception: it may read `approvals` and `decisions`. The ledger's tables cannot be reached through `tx.tables` at all. *Instead of:* a check over the source alone, which a statement built at run time or a trigger would pass. *Cost:* another crate's statement is prepared on every call, because a statement kept from one crate could be reused by another with the same text; and the `hooks` feature of `rusqlite`.
7. **The store tells rule 3 which approval rows a transaction changed**, from the database's update hook. So "marked used and started in one transaction" is checked against what happened, not against a time or a flag. `approvals` must stay a table with row numbers.
8. **Time and randomness come in with the configuration**: the `Clock` of `a2m-telemetry` and the `RandomSource` of `a2m-vault`, the two traits that exist, instead of a third and a fourth. A transaction reads the clock once; a crate makes an identifier from `tx.now()` and `tx.random()`.
9. **The vault is attached as itself**: `Store::attach(Arc<Vault>)` takes its sealing, its keyed digest, and its redactor. The vault defines no `Seal` trait, and the store needs all three. The key records hold the store weakly, so the vault does not keep a closed store open.

## What follows

- Functions of the crates above that take a `Tx` are plain functions, not async. A caller that needs two crates' writes to commit together calls both inside one closure.
- A closure is `Send + 'static`, so it owns what it uses. Anything that waits, such as sealing, happens before or after it.
- The store cannot tell which crate calls `tx.tables(Owner::X)`. The documentation audit owes a check that each `Owner::X` appears only in crate X.
- Postgres, later, needs its own driver module beside `src/sqlite.rs` and its own set of migration files.

## What would reopen it

A measured cost of preparing statements, or of one writing thread, that the budgets of spec section 225 cannot bear; or a crate that needs to hold a transaction open while it waits for something outside the store.
