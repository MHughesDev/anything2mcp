# 0084. The session keeper's outside crates, and TLS for identity services

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the outside crates of [`a2m-auth`](../architecture/crates/a2m-auth.md), which its design left "to be verified", and which TLS the engine uses toward an identity service's token endpoint. The HTTP pack's own TLS choice stays that pack's to record; what was measured here applies to it.

## Decision

**Outside crates**

- **`reqwest` 0.13.5, already in the workspace, makes the token requests.** MIT or Apache-2.0. Only its `json` feature is on; the form body, the query string, and the Basic header are written by this crate, so that no secret passes through an encoder that might keep a copy, and so that the `form` and `query` features and their tree stay out. A client follows no redirect: a credential goes to the address the auth file names and nowhere else.
- **`sha1` 0.11.0 is added, with no default features.** MIT or Apache-2.0, from the same family and on the same tree as `sha2`, which is already here. One-time codes (RFC 6238) use HMAC-SHA-1 unless a system says otherwise, and nearly every system does not. HMAC is written here over the hash, as `a2m-core` already does, and held to the standard's test vectors. *Instead of:* a one-time-code crate, which would bring its own encodings and a larger tree; and *instead of:* writing SHA-1 by hand, when a maintained one costs one small crate.
- **No OAuth crate.** The two grants built are a form post and a JSON answer each. A client crate would bring its own HTTP abstraction, its own secret types that are not `Secret`, and URL and random crates; and the keeper, not a library, must decide what a refusal means. The answers are read by the standard's own error names (RFC 6749, section 5.2) and nothing an identity service writes is put into a failure.
- **No certificate crate.** `client_certificate` holds a certificate and a key and hands them to the driver's sink; the driver's TLS stack reads them.
- **Base64, Base32, percent-encoding, and hexadecimal are this crate's own,** about a hundred lines, held to the examples of RFC 4648.

**TLS**

- **rustls, with the ring provider and the platform's certificate verifier, behind the feature `tls` of `a2m-auth`, which is off by default.** `rustls` 0.23.46 with features `ring`, `std`, `tls12`, and `reqwest`'s `rustls-no-provider`. With the feature on, the keeper installs ring as the process's provider if nobody has. With it off, a token endpoint must be a loopback address, and an `https` one is refused by name: `input`, `tls_not_enabled`.
- *Instead of* `reqwest`'s default, rustls with aws-lc-rs: its tree passes the same licence check, but it builds a larger C library and wants an assembler on Windows, which could not be tried here. Ring needs a C compiler and nothing else on the three platforms.
- *Instead of* native-tls: on Linux it links the system's OpenSSL, which one binary installed through pip cannot count on.
- **Why off by default.** `rustls-no-provider` makes every `reqwest` client in the same build need a provider: building one without it panics. Cargo unites features across a workspace build, so switching the feature on in the product would break the test stands and the benchmark harness, which make plain HTTP clients and install nothing. Whoever switches it on does so in one change with those.

**What was checked**

- Both trees pass `cargo deny check licenses bans sources` once two identifiers are allowed: `ISC` (ring, rustls-webpki, untrusted) and `CDLA-Permissive-2.0` (the bundled root certificates). Both are on the list of permissive licences the build brief allows, and both were added to `deny.toml` with a comment.
- In a throwaway project, a client built this way completed an HTTPS request, and a client built with no provider installed panicked.
- In this crate, with the feature on, an `https` endpoint on a listener that speaks no TLS fails as `transient`, `upstream_unavailable`; with it off, as `tls_not_enabled`. Both builds pass the whole suite.

## Consequences

- The feature compiles C, as SQLite already does through `a2m-ledger`, so the crate cannot be linted for Windows and macOS from a Linux machine; CI does it.
- An HTTPS exchange with a real identity service has not been run. Real systems are tried at the end, with the owner.

## What would reopen it

The HTTP pack choosing another TLS stack, since one binary should carry one; or a platform on which ring does not build.
