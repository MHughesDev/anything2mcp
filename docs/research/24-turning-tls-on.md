# Research: what turning TLS on needs in this workspace

**Status:** completed on 2026-11-17 from the repository and from source of the crates involved. Nothing was compiled and no handshake was made; [decision 0084](../decisions/0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md) ran the one throwaway client and this note leans on that. It answers brief 34 of [open research](11-open-research.md), which was added because the overnight build's journal names the TLS gap as open ("TLS is off", [journal entry](../journal/2026-11-17-the-overnight-build.md)). It feeds the HTTP pack's open point "the TLS stack, verified on three operating systems", the server's `server.tls.*` settings ([section 226](../spec/2-kernel/226-configuration.md)), and the data and stream packs' encrypted links ([note 22](22-stream-wire-protocol.md), [note 23](23-server-database-clients.md)).

## Question

Today a build refuses every `https` address by name: `tls_not_enabled`. What does it take to switch TLS on for the whole product, without breaking the test stands and the benchmark harness, and what else needs TLS besides the HTTP client?

## Answer

- **One stack for everything: `rustls` 0.23.46 with the `ring` provider, `reqwest` 0.13.5 with `rustls-no-provider`, the platform's certificate verifier, and `tokio-rustls` 0.26.6 where a protocol is not HTTP** (a database, a broker, the server's own listener). The tree adds 13 crates to the HTTP client's 73, and every licence is already on the list in `deny.toml`. No assembler and no C library beyond `ring`.
- **The break is real, and it is one panic.** With `rustls-no-provider` on anywhere in a build, `reqwest::Client` construction panics unless a process-wide provider was installed first. It does not matter that the address is plain HTTP. There are 17 construction sites in the repository; 3 are product code.
- **Do not switch it on in the default workspace build.** Give each crate that makes an outbound client a feature `tls`, as `a2m-auth` already has; have the product crate pass them through; build the released binary with it on; add a CI job that tests those crates and the product with it on, on all three systems. This keeps `cargo test --workspace` (which unifies features) as it is, and removes the risk by having a job that shows the shipped build works. The longer-term fix, one construction helper at all 17 sites, is smaller than it looks and is the end state; it is not needed first.
- **The server side needs its own work.** `axum` has no TLS. A server listener is `tokio-rustls`'s acceptor behind axum's `Listener` trait, with the handshake kept off the accept loop.
- **A person who runs the product in a minimal container will see a named failure,** not a mystery, if the trust roots are missing. That needs a failure code and a setting for extra roots.

## What the repository does today

Read from `Cargo.toml`, the crates' manifests, and `.github/workflows/ci.yml`:

| Fact | Where |
| --- | --- |
| `reqwest` is declared once, `default-features = false`, feature `json` only. With no TLS feature, reqwest cannot make an `https` request at all | Root `Cargo.toml` |
| `rustls` is declared once, features `ring`, `std`, `tls12`, and used by `a2m-auth` behind its feature `tls`, which turns on `reqwest/rustls-no-provider` | Root `Cargo.toml`, `crates/a2m-auth/Cargo.toml` |
| `a2m-auth` installs `ring` as the process provider, when none is installed, inside the function that builds its client | `crates/a2m-auth/src/providers/oauth.rs`, lines 207 to 221 |
| `a2m-pack-http` and `a2m-pack-mcp` refuse `https` by name and have no feature for it | `crates/a2m-pack-http/src/driver.rs` line 64; `crates/a2m-pack-mcp/src/driver.rs` line 79 |
| `reqwest::Client` is built at 17 sites in 14 files: 3 in product code (`a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp`), 2 in the example application's harness, and the rest in tests | A search of `Client::builder()` and `Client::new()` |
| CI builds and tests with `--workspace`, so a feature that any member turns on is on for every member in that run | `.github/workflows/ci.yml`, lines 39 to 45 |

## How the panic works, from reqwest's source

Read in the published crate, version 0.13.5, file `src/async_impl/client.rs`:

- The default TLS backend is rustls when the `__rustls` feature is on and native TLS is not. `rustls-no-provider` turns on `__rustls` and the platform verifier, and not the `aws-lc-rs` feature.
- When a client is built with the rustls backend and no preconfigured configuration, the builder asks rustls for the process default provider. If none is installed it calls a function that panics when the `aws-lc-rs` feature is off, with a message naming the missing provider. This happens at `build`, for every client, whatever the URL.
- `tls_backend_preconfigured` accepts a ready `rustls::ClientConfig` and then skips that path. A client built that way does not panic and does not need the process default.
- reqwest 0.13 made rustls the default and `aws-lc-rs` the default provider; the platform verifier replaced the bundled roots. Changelog read.

So switching `rustls-no-provider` on is safe only where the provider is installed, or where every client is built with a preconfigured configuration.

## The options

| Option | What it costs | What it risks | Verdict |
| --- | --- | --- | --- |
| A. Per-crate feature `tls`, product passes them through, released binary built with them, a CI job runs the tests with them on | A feature line in 3 crates and in the product; one CI job per system; the product's own two test clients and `main` install the provider | The released binary differs from the one `cargo test --workspace` tests; the extra job must cover it | **Recommended for now** |
| B. Turn it on for everything and install the provider in every test and stand | 17 edits; any new test that forgets panics | A new contributor's first test fails with a message about cryptography | End state, after a helper exists |
| C. One construction helper used by all 17 sites that installs the provider and passes a preconfigured configuration | One function in a crate every site already links; 17 edits | The helper's crate gains `rustls` | The best end state; do it with B |
| D. Give reqwest `rustls` with the default `aws-lc-rs` | Nothing to install | Builds a large C library and wants an assembler on Windows; [decision 0084](../decisions/0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md) refused it for this reason | Refused |
| E. Native TLS | No provider issue | On Linux it links the system's OpenSSL, which one wheel installed by pip cannot count on | Refused, as in 0084 |
| F. Leave reqwest TLS-free and write the HTTPS clients on `hyper`, `hyper-util`, and `tokio-rustls` | No panic anywhere; the crates the local channel already uses | Reimplements what reqwest gives (timeouts, body limits, redirects refused, proxy settings); the HTTP pack is built on reqwest today | Not for now; the option to keep if the helper in C proves awkward |

## What the stack costs

From a throwaway project outside the repository, resolved at exact versions and checked with the repository's `deny.toml`. The only error was the project having no licence of its own.

| Tree | Crates |
| --- | --- |
| `reqwest` 0.13.5 with `json` only (today) | 73 |
| The same with `rustls-no-provider`, plus `rustls` 0.23.46 and `tokio-rustls` 0.26.6 on `ring` | 87 on Linux and Windows, 89 on macOS |
| What the 13 or 15 more are | `rustls`, `rustls-webpki`, `ring`, `untrusted`, `rustls-pki-types`, `tokio-rustls`, `hyper-rustls`, `rustls-platform-verifier`, `subtle`, `zeroize`, `cfg-if`, `getrandom` 0.2; on Linux `rustls-native-certs` and `openssl-probe`; on Windows `windows-link` and `windows-sys`; on macOS `security-framework` and the two Core Foundation crates |
| Licences | MIT, Apache-2.0, ISC, BSD-3-Clause, and Unicode-3.0 only. Nothing new is added to `deny.toml` |
| C to build | `ring`. A crate downloaded from the registry carries pre-generated assembly, so no Perl or NASM is needed (read in `ring` 0.17.14's `build.rs`: the generation step runs only in a git checkout) |

## The other users of TLS

| Who | What it needs | Crate |
| --- | --- | --- |
| The session keeper, to identity services | Done behind `a2m-auth`'s feature | `reqwest` |
| The HTTP pack, the MCP pack, the web probe | The same client, with the same feature | `reqwest` |
| Mutual TLS (`client_certificate`, [section 216](../spec/2-kernel/216-sessions-and-the-session-keeper.md)) | A client identity in the client's configuration | reqwest's identity from PEM, or the preconfigured configuration. Not tried |
| The server's listener (`server.tls.certificate` and `key`, from the vault) | An acceptor in front of axum's `serve`. Axum 0.8.9 has no TLS, and its `Listener` trait's `accept` returns a stream with no way to fail, so a handshake must be done on the connection and not inside `accept`, or one slow client stalls all | `tokio-rustls` 0.26.6 (MIT OR Apache-2.0; 80 releases; the latest 2026-09-28) |
| PEM files for that listener | A parser for a certificate and a key | `rustls-pki-types` 1.16.0 has one (`PemObject`), and is already in the tree. `rustls-pemfile` is a thin older crate whose own README now points to `rustls-pki-types`; do not add it |
| The database and stream packs | The connector of [note 23](23-server-database-clients.md) and the acceptor-free client side of `tokio-rustls` | `tokio-postgres-rustls`, `tokio-rustls` |
| The browser | Handles its own TLS; nothing to do | |
| Test stands that need an `https` address | A certificate made at test time | `rcgen` 0.14.10 (MIT OR Apache-2.0; `ring` feature; released 2026-08-28) as a dev-dependency. A committed certificate would expire; a synthetic one made per run does not |

## Trust roots

- **The platform verifier** is what reqwest 0.13 uses and what the other crates here can be given. On Windows it calls the system's certificate verification, on macOS the Security framework, and on Linux it loads the system's CA bundle once at start (its README, read).
- **Minimal containers have no bundle.** Its README says so for Linux and recommends `webpki-roots` for containers deployed often. `webpki-roots` 1.0.12 (CDLA-Permissive-2.0, already on the list) is the fallback.
- **An internal certificate authority** needs a setting, because a company's identity service or database may use one. reqwest's `tls_certs_merge` adds roots; the configuration of the other clients takes a root store. The root file is a path the person names, not an input a caller can change ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)).
- **No switch turns verification off.** reqwest has one (`danger_accept_invalid_certs`), and a lint or test should hold that no code calls it.

## What it means for the design

- Section 226 and the HTTP pack's design need a setting for extra trust roots and a failure code for "no trust roots found" (class `input` or `unavailable`; the lead picks from the technical reference).
- The `tls_not_enabled` failure stays correct until the job in Option A exists; then a build with the feature on never gives it.
- Every pack that opens a network connection must accept TLS or name why not: the data pack requires it except on loopback (note 23); the stream pack's NATS client needs `tokio-rustls` (note 22).
- The statement in decision 0084's consequences, "whoever switches it on does so in one change with those", is right. This note adds the count: three product sites and fourteen test and stand files.

## What could not be verified

- Nothing was built or run. The panic is read in source and was reproduced in decision 0084's throwaway project, not here.
- No handshake was made on any system. `rustls-platform-verifier` was not exercised on Windows or macOS.
- Mutual TLS through reqwest's identity was not looked at.
- Whether the axum `Listener` wrapper can do the handshake off the accept loop without a large amount of code was not tried.
- A Linux image with no CA bundle was not tried; the README's statement is relied on.
- The `a2m-pack-web-probe` and `a2m-engine` use of reqwest was counted, not read.

## Recommendation, and the alternative that lost

Take Option A now: a `tls` feature in `a2m-pack-http` and `a2m-pack-mcp` beside `a2m-auth`'s, passed through by the product crate and on in the released build, with a CI job that tests with it on, on three systems; then Options B and C together as one change; add `tokio-rustls` and `rcgen` (dev only) when the server listener is built. The alternative that lost is switching it on for the whole workspace at once: it is the shortest path to a working HTTPS client, and it turns fourteen files of tests into places that panic for a reason unrelated to what they test.
