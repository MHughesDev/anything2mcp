# Research: client crates for a server database beside the embedded one

**Status:** completed on 2026-11-17 from registry pages, source of the chosen crate, and a licence and size check of each tree. Nothing was compiled and no database was run. It answers brief 33 of [open research](11-open-research.md), which was added for roadmap package 6.1 (Postgres beside SQLite in the ledger) and package G3's "server store and the later parts are not built" (the data pack). Registry pages and the crates' sources were read; the PostgreSQL and MySQL wire documentation hosts were not needed and not opened.

## Question

Two crates need a server database client: `a2m-ledger` on Postgres (its design says "Phase 6", with the statements already written to run on both), and `a2m-pack-data` for a target's own server database. Which crates, with which features, are maintained, fit the closed licence list, carry the least C, and run on Windows, macOS, and Linux? What does turning on an encrypted link cost? What must the code around the client decide, which the client does not?

## Answer

- **PostgreSQL: `tokio-postgres` 0.7.18, with `tokio-postgres-rustls` 0.14.0 for the encrypted link.** Pure Rust apart from `ring`, which the workspace already accepts for TLS ([decision 0084](../decisions/0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md)). The same crate serves the ledger and the data pack, so one wire client is in the binary.
- **No pool crate yet.** A pool is a small amount of code beside the pack's per-identity rule (a pool per identity, never shared across identities). `deadpool-postgres` 0.14.2 adds 8 crates and is the choice if the pool grows past a screenful of code.
- **No `sqlx`.** It is large, has its own pool and its own idea of retry, and its compile-time checks need a live database at build time.
- **MySQL and SQL Server are not built now.** If asked: `mysql_async` 0.37.1 (130 crates, rustls with `ring`, no C beyond `ring`) is usable. `tiberius` 0.13.0 with rustls pulls `aws-lc-sys`, a C library with an assembler requirement on Windows; do not take it before a decision on the TLS provider.
- **Every database call needs a deadline and a fate of its own,** because the client crate gives only an error value. Section 307 of the specification already states the fates; this note says how they map (below).

## The candidates

Looked up on crates.io on 2026-11-17. The tree and licence figures are from a throwaway project outside the repository, resolved at the exact version, with the repository's `deny.toml` copied in and `cargo-deny check licenses bans sources` run. The only error in each case was the throwaway crate having no licence of its own. "Crates" counts the whole normal-dependency tree, including Tokio's.

| Candidate | Version, date | Licence | Crates | Passes `deny.toml` | C code | TLS | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `tokio-postgres` | 0.7.18, 2026-06-12; 38 releases since 2016; four in the last twelve months | MIT OR Apache-2.0 | 58 | Yes, with no new identifier. Two crates in the tree are triple-licensed (`whoami`: Apache-2.0 OR BSL-1.0 OR MIT; `r-efi` offers LGPL as one option of several), so the list is met by the permissive option | None | None built in; a connector is passed in | **Chosen** |
| the same, with `tokio-postgres-rustls` 0.14.0 and `rustls` with `ring` | 0.14.0, 2026-05-21; 17 releases | MIT | 76 | Yes. ISC for `ring`, `rustls`, and the verifier, already on the list | `ring` | rustls; features `ring`, `native-certs`, `webpki-roots` | **Chosen for the link** |
| `postgres` (the blocking wrapper of the same project) | 0.19.14, 2026-06-12 | MIT OR Apache-2.0 | Not resolved; its dependency list is the same crate plus a small set | Expected yes | None | Through the same connector | The ledger's option if its closures stay blocking; see below |
| `deadpool-postgres` | 0.14.2, 2026-08-26; the previous release was 2024-12-18 | MIT OR Apache-2.0 | 66 with `tokio-postgres` (8 more) | Yes | None | Passes the connector on | Held back |
| `bb8-postgres` | 0.9.0, 2024-12-09 | MIT | Not resolved | | | | Not looked at further: no release in eleven months |
| `sqlx` with `postgres`, `runtime-tokio`, `tls-rustls-ring` | 0.9.0, 2026-05-21 | MIT OR Apache-2.0 | 125 | Yes, with `webpki-roots` under CDLA-Permissive-2.0, already on the list | `ring` | rustls | Refused: size, its own pool and retry, a build that wants a database |
| `mysql_async` with `default-rustls-ring` | 0.37.1, 2026-09-01; 75 releases | MIT OR Apache-2.0 | 130 | Yes | `ring` | rustls | The MySQL client, when a pack mode needs it |
| `tiberius` (SQL Server), features `rustls`, `tds73` | 0.13.0, 2026-09-25 | MIT/Apache-2.0 | 56 | Yes (ISC) | **`aws-lc-sys`** | rustls with `aws-lc-rs` | Not now |
| `pgwire` (a library for the server side of the protocol) | 0.42.3, 2026-11-13 | MIT/Apache-2.0 | Not resolved | | | | Server side of the protocol. A possible base for a Postgres-speaking test stand; not needed while a real server runs in CI |

How each fact was established:

| Fact | How |
| --- | --- |
| Versions, dates, licences, release counts, features | The crates.io interface, 2026-11-17 |
| Trees, licences, C code | The throwaway projects and `cargo-deny list`, `cargo tree`, 2026-11-17. A `-sys` crate or `ring` in the tree counts as C |
| `tokio-postgres` offers `SslMode` of `Disable`, `Prefer`, `Require` only; verification of the server's certificate is the connector's job | Source of 0.7.18, `src/config.rs` |
| It offers a read-only and an isolation-level transaction builder | Same, `src/transaction_builder.rs` |
| Its error carries the server's error code and a flag for a closed connection | Same, `src/error/mod.rs` |
| It has a connect timeout, keep-alive settings, a TCP user timeout, a target-session attribute, and a channel-binding setting | Same, `src/config.rs` |
| `tokio-postgres-rustls` passes the server certificate to the SCRAM channel binding through the `x509-cert` crate, and has a `ring` feature | The registry's feature list and dependency list |
| Ubuntu 24.04 and Windows 2025 hosted images carry a PostgreSQL server; the macOS 15 image's readme lists none | The readmes of [`actions/runner-images`](https://github.com/actions/runner-images), read on 2026-11-17 |

## What the client does not decide, and the pack must

| Question | Finding | Consequence |
| --- | --- | --- |
| Is the link encrypted? | `SslMode::Prefer` falls back to plain text. Only `Require` refuses | The pack passes `Require` and a connector that verifies the name. Never `Prefer`. A target that has no TLS is named in its surface file and is refused unless the address is loopback, the same rule as [note 24](24-turning-tls-on.md) |
| Where do roots come from? | The connector takes a rustls client configuration. The platform verifier of `rustls-platform-verifier` (the one the HTTP client already uses) is the same choice on all three systems | One trust decision for the whole binary |
| What is the fate of a failed call? | The error says whether the connection closed and gives a server code if there was one. A closed connection after a commit was sent is unknown | The data pack maps: error before send, or a server code in class 08 (connection) before the statement ran, to `transient`; a server answer with a code to its class; a closed connection after `COMMIT` was sent to `possible_commit`. This is the rule of section 307 and must be tested by closing the connection from the stand between the statement and the answer |
| Is a statement read-only? | The transaction builder can start a read-only transaction. The server then refuses a write | Matches the section's "refused by the database, not by parsing". A function that reaches outside is not stopped by it, and the section's open point stays open |
| Exact numbers | `numeric` arrives in Rust as a type of the optional `rust_decimal` feature of a separate crate, or as text | Read `numeric` as text (a cast in the statement the pack builds from a typed column) and parse with the workspace's decimal wrapper. This avoids a second decimal crate and a float on the path. Judgment; not tried |
| Timeouts | The client has a connect timeout and keep-alive, and no per-statement deadline | Every call is wrapped in the engine's deadline, and a `statement_timeout` is set on the session at connect. A cancelled call must send the server's cancel request, or the statement may still run. The client crate offers a cancel token |
| Pooling per identity | A pool must never serve a second identity | A small map from identity to connections, owned by the pack; its closing rules are part of the pack, not of a crate |

## The ledger

The ledger design runs every transaction as a closure on the store's own thread and says the SQLite crate blocks. Postgres has two ways in:

1. **The blocking `postgres` crate** on the same kind of thread, with the same closure. Least change to `Store::write` and `Store::read`. It runs its own small runtime inside, so no database call is made on an async thread, which the design requires.
2. **`tokio-postgres` directly,** with `Tx` made asynchronous for the Postgres back end only. Larger change to every crate that persists, because they are handed a `Tx` in a blocking closure.

The recommendation is the first. It keeps one idea ("a closure on a thread that owns the connection") for both databases. This is judgment, and the ledger crate's owner should confirm it; the second option is the better one only if the runner link needs hundreds of concurrent transactions on one control plane, which no measurement says.

Other points that came out while reading:

- The migrations are in SQLite's dialect. Postgres needs a second set, and the check that both give the same answers runs in CI on both ([ledger design](../architecture/crates/a2m-ledger.md), build order 7).
- Claiming a job with `FOR UPDATE SKIP LOCKED` is the pattern section 208 names; nothing in the client crate needs to change for it.
- CI needs a server on all three systems. The hosted Linux and Windows images carry one; the macOS image's readme names none, so macOS needs a package-manager install or the Postgres tests run on Linux and Windows only, with the gap written in the journal as the exit-test rule allows.

## What could not be verified

- Nothing was compiled, so no build on Windows or macOS was seen. `ring` is the only C in the chosen tree and the workspace already depends on it for TLS.
- No server was started: the mapping of a closed connection to `possible_commit` rests on the client's error type as read, not on a test.
- `postgres` (the blocking crate), `bb8-postgres`, and `pgwire` trees were not resolved.
- The runner image readme for macOS may have been shorter than the image itself; a package manager was not tried.
- The behaviour of `tokio-postgres-rustls` with the platform verifier was not tried; the crate's own features offer system roots and a bundled set.

## Recommendation, and the alternative that lost

Take `tokio-postgres` 0.7.18 and `tokio-postgres-rustls` 0.14.0 (feature `ring`), for the data pack's server store and, through the blocking `postgres` wrapper, for the ledger. Hold the pool in the pack. Require TLS except on loopback. Add no other database client until a target needs one. The alternative that lost is `sqlx`, which would have given one crate for three databases, at 125 crates and a retry and pool design the pack cannot let decide a write's fate.
