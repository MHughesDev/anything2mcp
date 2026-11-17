# 0166. HTTPS in the product: trust roots at engine start, and one maker of clients

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** what [decision 0157](0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md) left the product to do: reading `network.trust_roots` at engine start, and making every HTTP client of the workspace through one helper, so that the whole workspace can be built and tested with TLS on. It does not flip the default; it says why.

## Decision

**Trust roots at engine start.** `Engine::open_loaded`, which the one-shot engine, `serve`, and an embedding host all go through, first calls `trust::install` of the product crate, before the provider registry is filled and before anything makes a client:

- Each entry of the installation-only setting `network.trust_roots` must be a full path to a file inside a folder that `files.allowed_roots` allows: each full path listed there, and the engine's project folder where the word `<project>` stands. The file is read through `Roots::check` of `a2m-provider` with read access, so a link that leads out of the allowed folder is refused, and at most 1 MiB is read.
- The bytes go to `tls::install(Trust::with_named_roots(..)?)` of `a2m-auth`. A file that cannot be read through the allowed folders, or that holds no certificate, ends the opening with `input`/`trust_roots_unreadable`, naming the entry by its place in the list, not by its path. Roots named in a build without TLS end it with `input`/`tls_not_enabled`. *Instead of* opening without the roots: a person who named an authority expects it, and an engine that quietly trusts less is a failure nobody sees until a connection is refused for another reason.
- The trust is set once for the process. A second engine opened in the same process with the same files installs nothing more; with other files it is refused, `input`/`tls_setup_failed`. With no file named, nothing is installed, and every client trusts the operating system's roots.

**One maker of clients, everywhere.** Every HTTP client of the workspace is made by one of two helpers: `tls::client_builder` of `a2m-auth` for the product's code and for a test that acts through the product, and the stand's `example_app::harness::client_builder` for a stand and for a test that uses a stand as a plain user. Both state their TLS provider, so neither stops the process in a build with TLS on. The product's two test clients in `tests/local_engine.rs`, and the test clients of `a2m-api`, `a2m-mcp`, `a2m-local`, `a2m-pack-web-probe`, and `example-mcp` (which gains `example-app` as a development dependency), were changed so. The rule `every_http_client_of_the_workspace_is_made_by_one_of_the_two_helpers` in `crates/a2m-auth/tests/tls_rules.rs` reads every source file under `crates/`, `targets/`, and `bench/` and refuses any other construction. One file is listed as owed and the rule fails once it is changed and still listed: `crates/a2m-engine/tests/common/mod.rs`, whose `reqwest::Client::new()` at line 486 is that crate's owner's to change, to `tls::client_builder(&tls::Trust::system().unwrap()).build().unwrap()` of `a2m-auth`.

**The workspace run with TLS on** is spelled `cargo test --workspace --features anything2mcp/tls`; the product's feature turns on those of `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp`, and `example-app`, and the feature unification of a workspace run gives every member the same `reqwest`.

**The default is not flipped.** Decision 0157's four conditions, as they stand:

| Condition | State |
| --- | --- |
| The CI job that tests the three crates with their features on is green on Windows, macOS, and Linux | Not met. Run 44142063449: green on Linux and Windows; on macOS the job was never given a runner |
| The product's own tests pass with `anything2mcp/tls` on | Met in code: the two clients are made through the helper. Not run here with the feature on, because crates under the product were being changed by other packages while this one was built |
| The product reads `network.trust_roots` and calls `tls::install` at start | Met |
| Every other crate that makes a client in its tests makes it the same way | Not met: `a2m-engine`'s test helper, above |

When all four hold, the flip is one line in `crates/anything2mcp/Cargo.toml`, `default = ["tls"]`, and the separate CI job changes meaning, as the record's consequences say.

## Consequences

- A build with the feature off is unchanged: no trust root is named by default, so `trust::install` installs nothing.
- Revocation is still not checked for a certificate under a named root.
- The stripped release binary on Linux (x86_64), measured 2026-11-17: 31,362,392 bytes without the feature and 33,250,136 bytes with it, 1.9 MB more. Both are over the wheel's 25 MB target (hypothesis H-28) already without TLS; TLS is not what decides it.
- Proven on Linux through the real program, with the feature on (`tests/trust_roots.rs`): a read over HTTPS from a test-only authority is refused `certificate_untrusted` until the installation names the authority's file inside the project folder, and then is made; a file outside the allowed folders, and one with no certificate, stop the engine opening by name.
