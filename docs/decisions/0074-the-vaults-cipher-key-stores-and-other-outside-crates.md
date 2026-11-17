# 0074. The vault's cipher, key stores, and other outside crates

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** step 2 of the build order of [`a2m-vault`](../architecture/crates/a2m-vault.md); the open point of [spec section 216](../spec/2-kernel/216-the-vault-and-its-keys.md) on the cipher and its library; and the items [research note 15](../research/15-traces-and-key-stores.md) left unverified about the key-store crate. Carries out [decision 0038](0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md).

## Context

The vault writes no cryptography of its own, so it needs an authenticated cipher, a slow derivation for the passphrase route, operating-system randomness, memory wiping, a matcher for the redactor, and a way to the three desktop key stores. Each was checked on crates.io on 2026-11-17: the version exists and is not yanked, the source was read where the vault calls it, and the whole tree passes the licence check with every feature on. The vault was then type-checked and linted for Windows and macOS from Linux, and built on Linux with and without the `secret-service` feature.

## Decision

| Crate | Version, published, licence | Features | For |
| --- | --- | --- | --- |
| `chacha20poly1305` | 0.11.0, 2026-06-28, Apache-2.0 or MIT | `alloc`, `zeroize`; defaults off | The one cipher: XChaCha20-Poly1305 |
| `argon2` | 0.6.0, 2026-08-27, MIT or Apache-2.0 | `alloc`, `zeroize`; defaults off | The passphrase route: Argon2id |
| `getrandom` | 0.4.3, 2026-06-17, MIT or Apache-2.0 | none | Keys and nonces, behind the vault's `RandomSource` |
| `zeroize` | 1.9.1, 2026-10-06, Apache-2.0 or MIT | defaults | Wiping keys, plain items, and registered secrets on drop |
| `aho-corasick` | 1.1.5, 2026-08-03, MIT or Unlicense | defaults | The redactor: every form of every secret in one pass |
| `keyring-core` | 1.0.1, 2026-10-13, MIT or Apache-2.0 | none | The interface over credential stores, and an in-memory store the tests use |
| `apple-native-keyring-store` | 1.0.2, 2026-08-06, MIT or Apache-2.0 | `keychain` | macOS only: the Keychain |
| `windows-native-keyring-store` | 1.1.0, 2026-05-24, MIT or Apache-2.0 | defaults off | Windows only: Credential Manager |
| `zbus-secret-service-keyring-store` | 1.0.1, 2026-08-15, MIT or Apache-2.0 | `crypto-rust` | Linux only, and only with the vault's cargo feature `secret-service`: the Secret Service |

- **The cipher is XChaCha20-Poly1305.** Its nonce is 24 bytes, so a random nonce for every seal and every wrap is safe however many items one key protects, and it is constant-time in software on every machine. The crate is pure Rust, from the RustCrypto project, which also supplies `sha2`. An outside review of this crate was published in 2020; that is recalled, not re-read tonight. *Instead of:* AES-256-GCM from the same project, whose 12-byte nonce limits how many random nonces one key may use and which leans on processor support for speed and for constant time; and `ring`, which brings C and assembly into the build on three systems.
- **The passphrase derivation is Argon2id,** at 64 MiB, three passes, one lane: the second recommended setting of the Argon2 standard, with one lane so the cost does not depend on the machine. It costs memory as well as time for each guess. *Instead of:* PBKDF2 or scrypt.
- **The key stores come from the `keyring` family, linked store by store.** The all-in-one `keyring` crate, 4.2.0, says in its own documentation that an application which chooses its stores should link `keyring-core` and the store crates instead, so the vault does. Each store crate is a dependency only on its own operating system.
- **Nothing here needs a system library.** The Linux back end is the pure-Rust D-Bus client, not the one that links the system's D-Bus library. It brings about a hundred crates, so it sits behind the vault's cargo feature `secret-service`, off by default. Without the feature, `OsKeyStore::open` on Linux returns `key_store_unreachable`, which is also what a Linux machine with no desktop session returns with it. *Instead of:* the Linux kernel keyring, whose contents do not survive a restart.
- **The redactor's matcher is `aho-corasick`,** two crates with `memchr`. A matcher of our own was weighed: it would be the piece of the redactor most likely to be wrong or slow. Our own search, pattern by pattern, is kept as the fall-back for when the automaton cannot be built, and a test holds the two to the same answer. The cost of the outside crate: its automaton holds a copy of the secrets that is freed, not wiped, when the set changes. Registered secrets stay in memory for the life of the process in any case.
- **No base64, percent, or JSON crate.** The redactor's forms are about a hundred lines of its own.
- **`hmac` is not added.** The keyed digest's one construction is in `a2m-core` ([decision 0069](0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)).
- **Secret memory is not locked.** Locking needs calls to each operating system that are unsafe code, and would not cover copies the outside crates make. Keys, plain items, and registered secrets are wiped when dropped.

## Consequences

- The licence list in `deny.toml` is unchanged: every crate above and everything it brings is under a licence already allowed.
- With the `secret-service` feature on, the tree holds two versions of `syn`, which the licence check reports as a warning.
- The default tree of the vault on Linux is about twenty crates beyond what the workspace had; on Windows and macOS a few more for the store.
- A release for Linux desktops must be built with `secret-service`. Packaging decides whether the one binary always carries it.
- Not verified, and still open: the three real key stores have not been exercised, because the build machine has none; the test for them is ignored by default. How Credential Manager behaves for a service under its own account is unknown.
