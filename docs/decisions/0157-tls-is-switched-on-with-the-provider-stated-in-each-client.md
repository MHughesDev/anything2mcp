# 0157. TLS is switched on with the provider stated in each client

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** how the HTTP pack, the MCP pack, and the session keeper reach `https` addresses, which [research note 24](../research/24-turning-tls-on.md) recommended and [decision 0084](0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md) left open ("whoever switches it on does so in one change with those"). It does not settle the server's own listener, mutual TLS, or the data and stream packs' links.

## Decision

**One way to make a client, with the provider stated, not installed.**

- Every HTTP client of `a2m-auth`, `a2m-pack-http`, and `a2m-pack-mcp` is made by `tls::client_builder(&Trust)` of `a2m-auth`. With the feature `tls` on, it hands `reqwest` a ready `rustls::ClientConfig` built with `builder_with_provider(ring)`. Read in `reqwest` 0.13.5's `src/async_impl/client.rs`: a preconfigured configuration is the `BuiltRustls` back end, which never asks for the process's default provider, so the panic the note found is not reached. Proven: `the_system_trust_is_made_and_a_client_built_from_it` builds a client with the feature on and nothing installed. Nothing in the product installs a provider for the process; the keeper's old `install_default` is gone.
- A source test, `every_http_client_of_the_product_crates_is_made_by_the_tls_module` in `crates/a2m-auth/tests/tls_rules.rs`, refuses any other client construction in the three crates, tests included.
- *Instead of* installing ring once, in one function every site calls first: a crate that forgets would make another crate's client stop the process, and a provider set for the process is state that tests share. Stating it costs nothing at run time.

**Trust.**

- A `Trust` holds one client configuration. A certificate is checked first against the roots a person named, by `rustls`'s WebPKI verifier, the same rules on every operating system. A certificate that leads to none of them goes to the operating system's verifier, `rustls-platform-verifier` 0.7.1, the one `reqwest` itself uses. Where that verifier cannot be made, which on Linux means the machine has no roots of its own (a minimal container), the bundled roots of `webpki-roots` 1.0.12 take its place, and `Trust::roots_source` says `bundled`.
- The protocol versions are TLS 1.3 and TLS 1.2, stated. ALPN offers `http/1.1`, the only HTTP the client speaks.
- *Instead of* handing the named roots to the platform verifier's `new_with_extra_roots`: on Windows and macOS the named roots would then be judged by the system's rules, which differ, and a certificate made wrong under them would fail with a reason the system does not always name. Judging them ourselves gives the same answer on all three.

**Named roots.**

- They come from the installation-only setting `network.trust_roots` ([section 226](../spec/2-kernel/226-configuration.md)): PEM files, each inside a root of `files.allowed_roots`, read when the engine starts. `a2m-auth` takes their bytes, `Trust::with_named_roots(&[&[u8]])`, and reads no path itself; the product crate reads the files through its checked roots and calls `tls::install` once. No tool input and no target file reaches them ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)). A file with no certificate is `input`, `trust_roots_unreadable`.
- One trust for the process, set once, read by every client through `tls::installed()`; when nothing was set, the operating system's roots. *Instead of* passing a `Trust` to every constructor: the packs are registered through `a2m-provider`'s `PackContext`, which has no place for it. When `PackContext` gains a view of the network settings, the trust moves there and `install` goes.
- Not built here: the product crate's reading of `network.trust_roots`. Until it is, a build trusts the operating system's roots only.

**No switch turns checking off.** No setting, flag, pack setting, or target-file key does, and no code calls `danger_accept_invalid_certs` or writes a verifier that accepts by itself. Two tests in `tls_rules.rs` read the settings schema, every pack manifest, the target-file kinds, and every source file of the workspace for one.

**Failures.** A request that fails while its connection is made, with a `rustls` error in the chain, is named by `tls::refusal`, class `unknown_variant`: `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified` (refused for another reason, or by a system verifier that does not say which), and `tls_handshake_failed`. Nothing was sent, so a write is `not_sent` and never retried. `unknown_variant` because a peer that cannot prove it is the surface is a case with no rule: not `transient`, since trying again gives the same answer and could hide an attack; not `input`, since the address is right; not `policy`, which is the policy engine's. A TLS error after a request was sent is not named this way: it stays a lost connection, and for a committing request `possible_commit`. Both packs declare the codes in their manifests.

**Redirects and plain HTTP.** No redirect is followed (as before), and a client for an `https` surface is built with `https_only`, so nothing it sends can leave TLS. A plain `http` surface off loopback is still used, with its credentials, when the run may reach it; that is unchanged and left to the egress policy.

**The packs.** `a2m-pack-http/tls` and `a2m-pack-mcp/tls` turn on `a2m-auth/tls` and the stand's `example-app/tls`. With the feature on, an `https` surface is used; with it off, the refusal is exactly as before. A client certificate stays refused, as `auth_scheme_not_built` in the HTTP pack with the feature on.

**The stand.** `example-app/tls` serves the application over TLS and plain HTTP at once, one store behind both, so a test asks the plain control address what was stored. A `TestAuthority` made at test time with `rcgen` 0.14.10 (`pem`, `ring`; MIT or Apache-2.0) signs a good certificate, one for another name, and one that expired in 2000; a second authority stands for an unknown one. A `TlsFront` puts TLS before the MCP stand. `rcgen` and `tokio-rustls` 0.26.6 are the stand's dependencies only, behind its feature.

**The product.** `anything2mcp/tls` turns on the three crates' features and `example-app/tls`. It is off by default.

**When the default flips.** The released binary is built with `--features tls` first, and the default changes to `default = ["tls"]` in `crates/anything2mcp/Cargo.toml` only when all of these hold: the CI job that tests `a2m-auth`, `a2m-pack-http`, and `a2m-pack-mcp` with their features on is green on Windows, macOS, and Linux; the product's own tests pass with `anything2mcp/tls` on, which needs its two test clients in `tests/local_engine.rs` made through `tls::client_builder` of `a2m-auth`; the product reads `network.trust_roots` and calls `tls::install` at start; and the other crates that make a client in their tests (`a2m-engine`, `a2m-api`, `a2m-mcp`, `a2m-local`, `a2m-pack-web-probe`, `example-mcp`) make it the same way, so that `cargo test --workspace`, which unites features, does not stop.

## Consequences

- With the feature off, the default build's tests are unchanged: every new test is behind it, and the old refusal tests are behind its absence.
- On macOS the system's verifier refuses a certificate from an unknown authority without saying which check failed, so there it is `certificate_not_verified`; the tests accept that there and nowhere else. Proven on Linux only; the CI job shows the other two.
- Revocation is not checked for a certificate under a named root; the system's verifier checks it where the system does.
- Research note 24's options A and C are both taken: per-crate features now, and the one helper, used by the three product crates. Option B, the whole workspace on, waits for the conditions above.
