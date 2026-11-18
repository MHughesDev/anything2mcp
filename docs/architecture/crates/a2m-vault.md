# `a2m-vault`

Sealing and opening: envelope encryption in four key layers, the key-store back ends that hold the root, the keyed digest, and the redactor that removes every released secret from text. It turns plain bytes into sealed bytes and back. It does not decide where sealed bytes are kept, and it never decides who may open them.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside, each verified in [decision 0074](../../decisions/0074-the-vaults-cipher-key-stores-and-other-outside-crates.md): `chacha20poly1305` for the cipher; `argon2` for the passphrase route; `getrandom` for keys and nonces; `zeroize` for wiping memory; `aho-corasick` for the redactor's matcher; `keyring-core` with one store crate per desktop system; `tokio` for blocking calls to synchronous key stores |
| Roadmap packages | Roadmap package 2.1, the redactor; package 2.2, sealing with the operating system's key store. The planted-secret test of 2.1 waits for `a2m-telemetry` and a front door. The key-service back ends with the server profile, Phase 6; `vault.rotate` in Phase 9 |
| Carries out | Spec sections [216](../../spec/2-kernel/216-the-vault-and-its-keys.md) and the redaction steps of [217](../../spec/2-kernel/217-traces-evidence-and-repair.md); [decision 0038](../../decisions/0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md), [decision 0030](../../decisions/0030-where-session-secrets-live.md), [decision 0075](../../decisions/0075-the-vaults-interface-as-building-it-settled-it.md), and [decision 0138](../../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md), which records what a safety audit changed; the engine-fault rows for the key store in [section 224](../../spec/2-kernel/224-when-the-engine-itself-fails.md) |

## Owns

- The key hierarchy: a root key per installation, a key per workspace, a key per owner, a data key per item.
- Sealing a plain item into a `Sealed` envelope bound to what it is, and opening it again.
- The `KeyStore` interface for the root, and the back ends for the operating system's store, the passphrase route, and memory for tests.
- The `KeyRecords` interface through which wrapped workspace and owner keys are kept, and nothing else of the store.
- The `Redactor`, which implements `a2m-core`'s `Redact`, the `RedactStream` for a text that arrives in pieces, and the rule that every secret this crate releases is registered with it before the caller receives it.
- Making the root, and the passphrase route's salt, once: the creation lock.
- The installation's digest key, and `VaultDigest`, which implements `a2m-core`'s `KeyedDigest` with it.
- Rotation of each layer. Not built yet.

It must never: write a key, or a plain item, to disk (the only files it opens are the creation lock, which holds no byte, and the passphrase route's salt, which is not secret); write cryptography of its own; return a secret in any form but `a2m-core`'s `Secret`; print, log, or put in a `Failure` any key, plain item, or part of one; decide whether a job may act as an owner; know a mode, a pack, or a kind of system.

## Where sealed bytes are kept

**The vault seals and opens; the caller keeps the result.** `seal` returns a `Sealed` value, and the caller stores it: `a2m-ledger` for blobs and ledger values, `a2m-auth` through `a2m-ledger` for saved session state and credentials. The data key travels inside the envelope, wrapped, so a sealed item needs nothing beside it but the upper keys.

**The upper keys are kept through a small trait the vault defines and the store implements.** Wrapped workspace and owner keys must persist, and the vault cannot depend on `a2m-ledger`, which sits above it. So the vault defines `KeyRecords`; `a2m-ledger` implements it over a table of its own; `a2m-actions` joins the two at `Kernel::open`. The other way round, a vault that hands wrapped keys to every caller to store, was weighed and refused: it would spread key handling across every crate that seals.

The order at start is therefore: the store opens its connection and runs migrations; the vault is made from the root back end and the store's `KeyRecords`; the ledger is then given the vault. Making the vault reads nothing, so neither calls the other during construction.

## Interface

As built. The asynchronous methods of the two traits return `BoxFuture`, a boxed future, because both traits are used as trait objects.

```rust
/// The root back end: three operations, as section 216 says. A desktop back end holds the root in
/// the operating system's store and wraps in memory; a key service wraps remotely and never
/// releases the root.
pub trait KeyStore: Send + Sync {
    fn describe(&self) -> StoreDescription;          // name, and strength: os_store, key_service, passphrase, memory
    /// A local back end that holds no root yet makes one here.
    fn wrap<'a>(&'a self, key: &'a KeyMaterial, bound_to: &'a Binding) -> BoxFuture<'a, Result<Wrapped, Failure>>;
    /// Never makes a root. A key that another root wrapped is `key_store_entry_missing`.
    fn unwrap<'a>(&'a self, wrapped: &'a Wrapped, bound_to: &'a Binding) -> BoxFuture<'a, Result<KeyMaterial, Failure>>;
    fn health(&self) -> BoxFuture<'_, StoreHealth>;  // ok, unreachable, or entry_missing
}
pub struct Binding { pub key: KeyId, pub layer: Layer, pub generation: Generation }

pub struct OsKeyStore;          // OsKeyStore::open(&InstallationName).await
                                // OsKeyStore::IN_THIS_BUILD: true on Windows and macOS, by the same cfg as the back end
pub struct PassphraseKeyStore;  // PassphraseKeyStore::derive(PassphraseRoute, &Secret, &PassphraseSalt).await
                                // PassphraseKeyStore::SHORTEST_PASSPHRASE is 16 bytes
                                // PassphraseKeyStore::not_given(variable) -> Failure: input/vault_passphrase_missing
pub struct MemoryKeyStore;      // MemoryKeyStore::new(); remove_root() stands for a reset credential store
pub enum PassphraseRoute { NotEnabled, Enabled }     // PassphraseRoute::of_backend(settings.text("vault.backend"))
pub const BACKEND_OS_KEY_STORE: &str = "os_keystore";
pub const BACKEND_PASSPHRASE: &str = "passphrase";
pub const DEFAULT_BACKEND: &str;  // the store where the build has one, the passphrase route where not (decision 0220)
pub struct PassphraseSalt(/* 16 bytes, not secret */);
impl PassphraseSalt {
    /// The salt kept in `file`, made there once, under the creation lock, when the file is missing.
    pub async fn load_or_create(file: &Path, random: &dyn RandomSource) -> Result<PassphraseSalt, Failure>;
}   // also generate(&dyn RandomSource), from_bytes, as_bytes, for a caller that keeps it elsewhere

/// Where wrapped keys persist: one row per key and generation. Implemented by `a2m-ledger`.
pub trait KeyRecords: Send + Sync {
    fn get<'a>(&'a self, id: &'a KeyId, generation: Generation) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>>;
    /// The highest generation of the key, or None when it was never made.
    fn newest<'a>(&'a self, id: &'a KeyId) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>>;
    /// Insert only. Refuses when the id and generation exist: a key is never overwritten by accident.
    fn insert(&self, record: KeyRecord) -> BoxFuture<'_, Result<(), Failure>>;
    /// For rotation of the key above: replaces the record only if its wrapped bytes are still `expected`.
    fn replace<'a>(&'a self, record: KeyRecord, expected: &'a Wrapped) -> BoxFuture<'a, Result<(), Failure>>;
}
pub fn key_record_conflict() -> Failure;             // what `insert` and `replace` return when they refuse
pub struct KeyRecord { pub id: KeyId, pub layer: Layer, pub generation: Generation, pub wrapped: Wrapped, pub wrapped_by: KeyRef }
pub struct KeyRef { pub id: KeyId, pub generation: Generation }
pub struct KeyId(/* a digest of what the key is for; 64 hexadecimal characters */);
pub struct Generation(/* u32; FIRST is 1 */);
pub enum Layer { Root, Workspace, Owner, Digest, Data }
pub struct MemoryKeyRecords;    // for tests; all() lists what was stored

/// What a sealed item is. Authenticated with the ciphertext, so one item cannot be swapped for another.
pub struct SealContext {
    pub workspace: WorkspaceId,          // `a2m-core`'s
    pub owner: OwnerRef,                 // OwnerRef::person(id), OwnerRef::service(name), OwnerRef::of(&Identity)
    pub kind: SealedKind,                // SessionState, Credential, Blob, LedgerValue, RunnerKey
    pub item: ItemId,                    // the blob's content reference, the session's id, the vault entry's name
}
impl SealContext {
    /// A stored credential's context: kind Credential, item `credential:<target>:<reference>` (decision 0248).
    pub fn credential(workspace: WorkspaceId, owner: OwnerRef, target: &TargetName, reference: &str) -> Result<SealContext, Failure>;
}

/// The envelope: format, cipher, the owner's key and its generation, the wrapped data key, the
/// nonce, and the ciphertext with its tag. Holds nothing in the clear.
pub struct Sealed { /* ... */ }
impl Sealed {
    pub fn to_bytes(&self) -> Vec<u8>;                           // for a column of bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Sealed, Failure>;
    pub fn owner_key(&self) -> KeyRef;
}   // also Serialize and Deserialize: hexadecimal text in a readable format, bytes otherwise

pub struct Vault { /* the back end, the key records, unwrapped upper keys cached in wiped memory, the redactor */ }
impl Vault {
    pub fn new(store: Arc<dyn KeyStore>, records: Arc<dyn KeyRecords>) -> Vault;
    /// What the kernel calls, with the one random source the engine was started with. In a running
    /// engine that source is `OsRandom`; a source that is the same on every run is for tests alone.
    pub fn with_random(store, records, random: Arc<dyn RandomSource>) -> Vault;

    /// A fresh data key and nonce for each call; a rewritten item gets a new key, which is its rotation.
    pub async fn seal(&self, cx: &SealContext, plain: &[u8]) -> Result<Sealed, Failure>;

    /// Opens bytes that are not secrets: evidence, ledger values. The caller keeps them out of logs.
    pub async fn open(&self, cx: &SealContext, sealed: &Sealed) -> Result<Opened, Failure>;

    /// Opens a secret. The value is registered with the redactor before this returns.
    pub async fn open_secret(&self, cx: &SealContext, sealed: &Sealed) -> Result<Secret, Failure>;

    /// For a secret that reached the runtime some other way, such as a token a sign-in returned.
    /// The session keeper calls it before the secret goes anywhere else.
    pub fn register(&self, secret: &Secret);
    pub fn register_folded(&self, secret: &Secret);   // for a secret whose form is case-insensitive

    pub fn redactor(&self) -> Arc<Redactor>;          // handed to everything that emits text
    pub async fn keyed_digest(&self) -> Result<Arc<VaultDigest>, Failure>;   // handed to the ledger and telemetry
    pub async fn health(&self) -> VaultHealth;        // for the kernel's health state and `doctor`
}

/// Plain bytes, wiped when dropped. Not `Clone`, not `Serialize`, not `Debug` beyond its length.
pub struct Opened(/* ... */);       // expose(), len()
/// A key in the clear, wiped when dropped. Not `Clone`, not `Serialize`; `Debug` shows nothing.
pub struct KeyMaterial(/* ... */);  // generate(&dyn RandomSource), from_slice(&[u8]), expose()
/// A wrapped key: ciphertext. `Debug` shows its length.
pub struct Wrapped(/* ... */);      // from_bytes(Vec<u8>), as_bytes()
pub trait RandomSource: Send + Sync { fn fill(&self, out: &mut [u8]) -> Result<(), Failure>; }
pub struct OsRandom;

/// The redactor. Holds the registered secrets in wiped memory. Cheap to clone; a clone shares the set.
pub struct Redactor { /* ... */ }
impl Redact for Redactor {                            // the trait from `a2m-core`
    fn redact(&self, text: &str) -> String;
    fn contains_secret(&self, text: &str) -> bool;    // the check on the way out
}
impl Redactor {
    pub fn new() -> Redactor;
    pub fn register(&self, secret: &Secret);
    pub fn register_folded(&self, secret: &Secret);   // masked in any letter case
    pub fn stream(&self) -> RedactStream;             // for a text that arrives in pieces
    pub fn health(&self) -> RedactorHealth;           // registered, short, bound
}
/// A text that arrives in pieces. What `push` and `finish` return, joined, is the whole text redacted.
pub struct RedactStream { /* the redactor, and the end of the text so far, held back */ }
impl RedactStream {
    pub fn push(&mut self, piece: &str) -> String;    // what can be let out now; may be empty
    pub fn finish(self) -> String;                    // the rest
}

pub struct VaultDigest(/* the installation's digest key, in wiped memory */);
impl KeyedDigest for VaultDigest { /* the trait from `a2m-core` */ }

pub mod codes { /* the failure codes below, as `FailureCode` constants */ }
```

Contracts that the signatures do not show:

- **A credential's context names its target.** `SealContext::credential(workspace, owner, target, reference)` builds the context of a stored credential, with the item `credential:<target>:<reference>`, so a credential sealed for one target does not open under another, even under the same reference ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). `a2m-auth` uses it for every stored credential.
- **The owner rule is cryptographic, not a check.** Each owner's items are sealed under that owner's key, and `SealContext.owner` is authenticated with the ciphertext. An item sealed for one owner cannot be opened under another's context, by any caller. Whether the job asking is really acting for that owner, and whether its runner holds the job's lease, is decided by the session keeper and the scheduler before they call. The vault cannot see leases, and says so rather than pretending.
- **A key's identifier comes from the context, never from a record.** `KeyId` is a digest of what the key is for. When an owner's key is unwrapped, the workspace key above it is the one of the context's workspace, whatever the record says, and both identifiers are authenticated with the wrapped key. A record copied from another owner or another workspace opens nothing.
- **The cipher is XChaCha20-Poly1305,** with a random 24-byte nonce for every seal and every wrap. The randomness comes through `RandomSource`, so a test can fix it.
- **The root is made by the first `wrap` and never by `unwrap`.** A local root carries a random identifier, which travels in every key it wraps. After a credential store is reset, a key the old root wrapped is recognized and fails as `key_store_entry_missing`, also once a new root exists.
- **The root is made once, under the creation lock.** The operating systems' key stores cannot write an entry only when there is none: a write replaces what is there. Two engines that start at the same moment on a fresh installation would each find no root and each write one, and everything wrapped under the root written first would never open again. Nothing above the vault prevents it: the engine lock of `a2m-local` holds one engine to a data directory, the root's entry is named for the installation and not for the data directory, and a command that finds no engine runs one inside itself without that lock. So the back end makes a root only while it holds an exclusive operating-system lock on a file in the user's temporary directory, named for the installation. The engine that holds it reads the store again and writes only when it is still empty; the engine that waited reads what the first wrote. The lock is released when its file is closed, so a process that dies holds nothing; the file holds no byte and is never removed.
- **A key is wrapped under the root the store holds at that moment.** `wrap` reads the store each time and does not trust the root in memory: a key wrapped under a root that is no longer the store's would open until the process ends and never again. Keys are wrapped rarely, once for each workspace and once for the digest key. `unwrap` uses the root in memory.
- **The passphrase route's root is derived, so two engines can differ only by their salt.** `PassphraseSalt::load_or_create` makes the salt once: under a creation lock beside the salt's file, written whole under another name and moved into place, and read back. A passphrase shorter than 16 bytes is refused before anything is derived from it. A length is no measure of strength; the floor refuses what falls to a short search however it was chosen. A guess costs one derivation and can be checked away from the machine, against the root's identifier that every wrapped key carries.
- **What the passphrase protects against, and what it does not.** It protects what the engine keeps on disk: a copy of the store, its key records, and the salt, taken from the machine or read while no engine runs, opens nothing without it. It does not protect against a program of the same user while the engine runs: the engine's process holds the derived root in memory for as long as it runs, and the passphrase is in the environment of the shell it came from. It comes from a person or the person's environment, handed to the engine by the command that starts it, and never from a tool call, a request, or a project file; nothing in this crate reads it from anywhere ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)).
- **Randomness comes from whoever built the vault.** The kernel builds it with the engine's one random source, which in a running engine is the operating system's. The vault cannot tell a predictable source from a random one. It refuses the faults it can see: a new key whose two halves are the same, and a nonce that is the beginning of the key drawn just before it, both `randomness_unavailable`. A root is always made from the operating system's source.
- **The redactor masks the forms a secret travels in, and says which it does not.** Exact matches are the rule of section 217. Percent-encoding, JSON escapes, and the character references of a page's source are found by decoding the text and searching what it decodes to, so every spelling of an escape is covered: percent-encoding and JSON once and twice, JSON inside a percent-encoded value, and the five named references with the numbered ones. Base64 is found by searching for what the secret encodes to, in both alphabets, at each of the three positions its bytes can fall in, so a secret inside a larger encoded value is found too; the mask takes the neighbouring character on each side, which holds the secret's remaining bits. Base64 is also searched with its line breaks left out: each run of white space that holds a line break and stands between two base64 characters, in the text and in the text percent-decoded or JSON-unescaped once, so that mail's and key files' wrapping, also written as `\n` in JSON, hides nothing. Only base64 forms count there, or a line break would join two words into a match. The secret's bytes written out in hexadecimal are searched for in each letter case. Each match is masked as `[redacted]`.
- **Letter case.** A secret written only in hexadecimal digits, with or without hyphens or colons between them, is masked in any letter case, because systems print hexadecimal in either. Any other secret is matched as it is written, unless it is registered with `register_folded`, for a form the registrant knows to be case-insensitive. Folded matching is of the secret as written, in the text as it is; it is not combined with the encodings, and only the letters of ASCII are folded.
- **A text that arrives in pieces goes through `RedactStream`.** `redact` sees one text and keeps nothing between two calls, so a secret divided between two pieces passes it in both. The stream holds back the end of what it was given: as much as the longest registered form could still begin in, in every view that is searched, and 72 bytes more for an escape that may still be arriving. What it lets out, joined, is the whole text redacted, wherever the pieces were divided. It runs behind by about the length of the longest secret. No caller redacts piece by piece today; a caller that must holds the `Redactor` itself, because `a2m-core`'s `Redact` has no stream.
- **What the redactor does not cover,** each for a stated reason:
  - *Base64 of an escaped form.* A secret that was JSON-escaped or percent-encoded and then base64-encoded, as in a signed token's payload, is found only when the escaping left it unchanged. That is so for a secret of letters, digits, `-`, `.`, `_`, and `~`. It is not so for a secret holding a quote, a backslash, a control character, a slash where the writer escapes slashes, a character outside ASCII where the writer escapes those, or anything percent-encoding changes. Closing it means decoding every run of base64 characters in every text, which is every word, or registering a form for each spelling of each escape, of which there is no finite list. The value a provider places is registered whole by the lease's sink, which covers a Basic header the engine builds itself.
  - *Nesting beyond the views above:* three levels, JSON inside percent-encoding inside JSON, character references inside another encoding, hexadecimal or another letter case inside base64. Each further view is another pass over every text.
  - *Named character references other than the five,* such as the one for a non-breaking space or an accented letter. There are over two thousand, and a page's source writes those characters as themselves or by number.
  - *Base64 broken by white space with no line break in it,* and line breaks in base64 at a second level of escaping.
  - *A secret cut short before the redactor saw it.* A caller that cuts text and then redacts leaves the first part of a secret that nothing can match. Redact the whole text, then cut.
  - *A secret nobody registered.*
- **Every secret that is not empty is registered, however short.** A short secret may mask unrelated text, which is the safer error. `RedactorHealth` counts the secrets shorter than eight bytes so that `doctor` can say so. One limit: an encoded form shorter than four characters is not searched for, so a secret of one or two bytes is masked exactly, percent-encoded, and JSON-escaped, and not inside base64, and a secret of one byte is not masked in hexadecimal.
- **`contains_secret` finds exactly what `redact` would mask,** and text that `redact` returned never contains a secret. A match that lies inside a mask already in the text is left alone by both; when masking uncovers another secret the text is masked again; and if that does not end in four rounds, the whole text becomes one mask.
- **Registration outlives the secret's use.** A registered value stays registered for the life of the process, so that a log line written after a session ends is still masked. The set has a bound of 4096 secrets; passing it is reported through `health`, never handled by dropping old entries. The set is kept as the forms themselves, in wiped memory, and nothing else: no digest of a secret is kept, because a digest of a short secret can be searched and would outlive the secret in memory that is never wiped.
- **An owner's name is not built from input.** A dedicated account is the service identity named `account:` and the account's name, so `OwnerRef::service("account:x")` is the owner of account `x`, deliberately: the store makes an owner again from the kind and the identifier it kept. Nothing in the product makes a service owner from text a caller chose, and nothing may.
- **Redaction never fails and never shortens the work.** `redact` has no error path. It matches over the whole text, whatever its length. If the matcher's automaton cannot be built, each form is searched for in turn, which is slower and finds the same matches.
- **A synchronous key store runs on a blocking thread.** The desktop key-store crates are synchronous, so calls to them, and the passphrase derivation, are moved off the async threads.
- **An unreachable key store is not an error the caller works around.** Items already opened stay in memory and keep working; nothing new is sealed or opened; the kernel reports the degraded state of section 224.
- **`StoreHealth::EntryMissing` on a new installation is expected** until its first item is sealed. `a2m-actions`, which can ask the store whether any key record exists, tells the two cases apart.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-ledger` | `seal` and `open` for blobs and ledger values; `Sealed::to_bytes` and `from_bytes`; implements `KeyRecords`; the `VaultDigest`, as a `KeyedDigest`, for blob names |
| `a2m-auth` | `seal` and `open_secret` for saved session state and stored credentials; `register` for a secret a sign-in returns; `OwnerRef::of` an identity; the `Redactor` on a provider's failure |
| `a2m-telemetry` | The `Redactor`, as a `Redact`, for every record, artifact, span, and log line; `contains_secret` for the check on the way out; the `VaultDigest` for restricted values |
| Every front door | The `Redactor`, as a `Redact`, on every result and error |
| `a2m-actions` | A back end and `Vault::with_random` at start, with the engine's random source; `keyed_digest` and `redactor` to hand on; `seal` for `secret.set`; `health` for `doctor` and the kernel's health state; later `rotate` for `vault.rotate` |
| `a2m-provider` | A `KeyStore` adapter for a key-service back end that a plug-in supplies, joined by `a2m-actions` |
| `anything2mcp` | `OsKeyStore::open`, or `PassphraseKeyStore::derive` with the salt of `PassphraseSalt::load_or_create`, to build the machine's key store; `PassphraseKeyStore::not_given` when the route is chosen and no passphrase came |
| `a2m-actions` (settings) | `DEFAULT_BACKEND`, as the default of `vault.backend` |

It calls only `a2m-core`, and through its two traits, the back end and the key records it was given.

## State

| State | Where | Notes |
| --- | --- | --- |
| The root key | The operating system's key store, or a key service | One entry per installation, under the service name `anything2mcp` and the installation's name, never a target's. The product names every installation `local`, so every engine of one user shares one entry, whatever its data directory |
| The root's creation lock | A file in the user's temporary directory, `anything2mcp-root-` and a digest of the installation's name | Holds no byte. Locked only while a root is made. Never removed |
| Wrapped workspace and owner keys, and the wrapped digest key | The store, through `KeyRecords` | A table the ledger owns; one row per key and generation |
| Wrapped data keys | Inside each `Sealed` envelope | Beside the data, as section 216 says |
| Unwrapped upper keys | Memory, wiped on drop | Cached for the process; dropped on rotation |
| Registered secrets | Memory, wiped on drop, inside the `Redactor` | Never written anywhere, and never digested. The matcher's automaton holds a copy that is freed but not wiped when the set changes |
| The passphrase salt | A file the caller names, with a lock file beside it, through `PassphraseSalt::load_or_create`; or wherever the caller keeps it | Not secret. Made once per installation |
| The end of a streamed text | Memory, inside a `RedactStream` | Not yet redacted; let out by `push` and `finish` only |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `key_store_unreachable` | The back end does not answer, or this machine has none this build can reach; the creation lock cannot be taken; the salt's file cannot be read or kept, or holds something that is not a salt. The kernel is degraded until it does |
| `busy` | `key_store_entry_missing` | The root that wrapped a key is gone, as when a credential store is reset. The next step says that saved sessions must be signed in again |
| `input` | `sealed_item_mismatch` | An envelope does not open under the context given: a swapped item, a wrong owner, a damaged envelope or key record, or a secret that is not text. Never says which |
| `input` | `sealed_format_unsupported` | An envelope's format or cipher id is one this engine does not know |
| `policy` | `vault_passphrase_not_enabled` | The passphrase route was asked for and `vault.backend` does not choose it |
| `input` | `vault_passphrase_missing` | The passphrase route is chosen and no passphrase was given. The next step names the environment variable and says what the passphrase is for |
| `input` | `vault_passphrase_mismatch` | The passphrase given at start is not the one the keys were wrapped under |
| `input` | `vault_passphrase_too_short` | The passphrase has fewer than 16 bytes. Nothing is derived from it |
| `busy` | `key_record_conflict` | A key record exists already, or changed before a rotation could replace it; nothing changed |
| `busy` | `randomness_unavailable` | The source gave no random bytes, or gave the same bytes again, so no key or nonce was made |
| `input` | `schema_invalid` | An owner, an item identifier, or an installation name is empty, longer than 255 bytes, or holds a control character |

A failure from this crate never names a key, a value, or a byte of either, and never says which of the reasons for a mismatch applied: that would help someone trying envelopes. The text of an error from an outside key-store crate is never copied into a failure.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A planted secret appears in no result, log, trace, or artifact | The planted-secret test of package 2.1, against the test application. Not written yet: it needs `a2m-telemetry` and a front door |
| A registered secret never survives `redact` in any encoded form | `tests/redactor.rs`: sixteen forms of one secret, and a secret inside a larger base64 value at every alignment; `a_secret_in_base64_broken_into_lines_is_masked`, `a_secret_written_with_character_references_is_masked`, `a_secret_written_out_in_hexadecimal_is_masked` |
| A secret whose form is case-insensitive is masked in any letter case | `a_hexadecimal_secret_is_masked_in_any_letter_case` |
| A secret divided between the pieces of a stream is masked, and a stream lets out what `redact` gives for the whole text | `a_secret_split_across_the_pieces_of_a_stream_is_masked`: twenty-three forms, each divided in two at every place and into pieces of eight sizes; `a_stream_lets_text_out_as_it_goes_and_changes_nothing_without_a_secret` |
| A secret that equals a form of another is a secret of its own | `a_secret_equal_to_a_form_of_another_is_registered_whole` |
| `contains_secret` finds exactly what `redact` would mask, and redacted text holds no secret | `tests/redactor.rs`, over texts with and without secrets, short secrets, overlapping secrets, and secrets that meet the mask's own text |
| Every secret `open_secret` returns was registered first | `tests/vault.rs`: opens a secret, then redacts text containing it in each encoded form |
| No key and no plain item is in anything the caller keeps or may print | `tests/vault.rs`: records every key made, then searches the envelope, the key records, every `Debug` output, and every failure for the keys and the plain bytes. The vault itself opens no file |
| An item sealed for one workspace, owner, kind, or id does not open under another | `tests/vault.rs`: a table test over every field of `SealContext`, with one failure for every reason |
| A sealed item with any bit changed, cut short, or lengthened does not open | `tests/vault.rs`: every bit of an envelope in turn |
| Two seals of the same value differ, and randomness comes only from the source given | `tests/vault.rs` |
| Everything survives a restart | `tests/vault.rs`: every object dropped, then rebuilt from the key store and the records. `a2m-ledger` repeats it over its table |
| Without the root, nothing opens | `tests/vault.rs`: restart with the key-store entry removed, and every open and seal fails with `key_store_entry_missing`; the same against the operating-system back end over an in-memory credential store |
| `Opened`, `KeyMaterial`, and `VaultDigest` cannot be printed, copied, or serialized in the clear | A test that fails to compile otherwise, as for `Secret` |
| A key record copied from another owner, or damaged, opens nothing | `tests/vault.rs` |
| Two engines that make the same key at once end with one | `tests/vault.rs`, with key records that lose the race |
| Two engines that start at once on a fresh installation end with one root | `two_engines_that_start_at_once_end_with_one_root`, over a credential store that holds both engines until each has found it empty and holds the second write until the first writer has read its own back, which is the order that lost a root; `engines_that_start_at_once_over_one_key_store_end_with_one_root` against the store in memory; `one_holder_at_a_time_and_a_dead_holder_holds_nothing` for the lock itself |
| The same on the passphrase route: one salt, one root | `engines_that_start_at_once_on_the_passphrase_route_end_with_one_root`, and between two processes `two_processes_that_start_at_once_end_with_one_salt` |
| An empty or short passphrase derives nothing | `a_passphrase_that_is_empty_or_short_is_refused_by_name` |
| A random source that repeats itself seals nothing | `a_source_that_repeats_itself_seals_nothing` |
| A dedicated account's owner is the service identity named for it, and a person of the same text is another | `an_accounts_owner_is_the_service_identity_named_for_it` |
| Rotating an upper layer re-wraps keys and re-encrypts no data | Not built: with `rotate` |

## Build order

1. Done: the `Redactor`: registration, the encoded forms, `contains_secret`, the rule for short secrets. The planted-secret test of package 2.1 is not written; it belongs with `a2m-telemetry` and a front door.
2. Done: the cipher crate, the key-store crates, the memory-wiping crate, and the rest verified and recorded in [decision 0074](../../decisions/0074-the-vaults-cipher-key-stores-and-other-outside-crates.md).
3. Done: `KeyStore` with the operating-system back end and the in-memory one; `KeyRecords` with an in-memory implementation; `seal`, `open`, `open_secret`; `keyed_digest`. The operating-system back end is proven against the outside crates' in-memory credential store on every machine. Against a real key store it is one ignored test, which has not been run on any of the three systems: this build machine has none. The Windows and macOS code is type-checked from Linux and has never run.
4. Done here: the restart test, over the in-memory records. Not done: `KeyRecords` in `a2m-ledger`.
5. Done: the passphrase route, behind `PassphraseRoute`, which the caller fills from `vault.backend`. Where the build has no operating-system key store, the route is that setting's default ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)): `DEFAULT_BACKEND`, from the same `cfg` as the back end, proven by `where_the_build_has_no_key_store_the_passphrase_route_is_the_default_and_opens`. The settings schema takes its default from it once its owner changes one line.
6. Not built: a key-service back end, for the server profile. Phase 6.
7. Not built: `rotate`, with `vault.rotate`. Phase 9. `KeyRecords` already keeps one row per generation and has `replace` for it.
8. Done: what the safety audit of [decision 0138](../../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md) found: the creation lock for the root and the salt, the passphrase floor, the redactor's stream and its further forms, the registered set without digests, and the check on a random source that repeats itself. The creation lock's Windows and macOS code is the standard library's file lock; it is type-checked from Linux and has run only on Linux.

## Open points

- **Locking secret memory is not done.** It needs calls to each operating system that the workspace's rule against unsafe code forbids without a stated reason, and it does not cover copies the outside crates make. Keys and plain items are wiped on drop; that is all ([brief 5](../../research/11-open-research.md)).
- **Recovery after the root is lost.** The wrapped keys in the store are then dead, and nothing new can be sealed for those workspaces until their records are replaced. Which action does that, and what it does with the sealed items that can no longer be read, is not designed.
- **What rotation of an owner's key means.** Data keys live in the envelopes, so re-wrapping them would touch every sealed item. The records are therefore kept per generation: a rotated owner key gets a new generation for new items, and the old one stays readable until its items are rewritten. Step 7 confirms or changes this.
- **A second engine does not see a rotation** made by the first until it restarts, because the newest generation of each key is cached. Step 7 settles it.
- How the Windows credential store behaves for a service running under its own account.
- **The creation lock is shared only by engines that agree on the temporary directory.** Two engines of one user whose temporary directories differ, as on a Windows server that gives each sign-in session its own, do not exclude each other, and can still lose a root between them. Reading the store at every `wrap` narrows the time in which that can happen and does not close it. The macOS Keychain can refuse to add an item that exists, which would let the key store itself decide; the store crate offers no such call, and going around it means matching its item's attributes in code that cannot be run here.
- **A predictable random source in a running engine is not detected.** Only one that repeats itself is. The product hands the kernel the operating system's source; nothing in the types holds it to that.
- **A dedicated account as a third kind of owner,** instead of a service identity with a reserved beginning to its name. It needs the store's reading of an owner changed in the same step, because the store makes an owner again from the kind and identifier it kept.
- **Linux has no operating-system key store in this build.** The Secret Service back end was removed when the advisory check refused its tree ([decision 0123](../../decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)); the passphrase route is the one route there, and the default, until the store crate stands on maintained crates ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)). When the back end returns, `OsKeyStore::IN_THIS_BUILD` turns true on Linux, the default there becomes the key store again, and an installation that sealed under the passphrase must keep `backend = "passphrase"` in its installation file or its keys stop opening: that release needs a note and a check in `doctor`.
- **Registering many secrets one at a time, each followed by a search,** rebuilds the matcher each time. Registrations in a row with no search between them build it once. If a session keeper's pattern of use makes this slow, the matcher can be built in two tiers.
