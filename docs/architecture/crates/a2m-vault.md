# `a2m-vault`

Sealing and opening: envelope encryption in four key layers, the key-store back ends that hold the root, the keyed digest, and the redactor that removes every released secret from text. It turns plain bytes into sealed bytes and back. It does not decide where sealed bytes are kept, and it never decides who may open them.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside, each verified in [decision 0074](../../decisions/0074-the-vaults-cipher-key-stores-and-other-outside-crates.md): `chacha20poly1305` for the cipher; `argon2` for the passphrase route; `getrandom` for keys and nonces; `zeroize` for wiping memory; `aho-corasick` for the redactor's matcher; `keyring-core` with one store crate per desktop system; `tokio` for blocking calls to synchronous key stores |
| Built in | Roadmap package 2.1, the redactor; package 2.2, sealing with the operating system's key store. The planted-secret test of 2.1 waits for `a2m-telemetry` and a front door. The key-service back ends with the server profile, Phase 6; `vault.rotate` in Phase 9 |
| Carries out | Spec sections [216](../../spec/2-kernel/216-the-vault-and-its-keys.md) and the redaction steps of [217](../../spec/2-kernel/217-traces-evidence-and-repair.md); [decision 0038](../../decisions/0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md), [decision 0030](../../decisions/0030-where-session-secrets-live.md), and [decision 0075](../../decisions/0075-the-vaults-interface-as-building-it-settled-it.md); the engine-fault rows for the key store in [section 224](../../spec/2-kernel/224-when-the-engine-itself-fails.md) |

## Owns

- The key hierarchy: a root key per installation, a key per workspace, a key per owner, a data key per item.
- Sealing a plain item into a `Sealed` envelope bound to what it is, and opening it again.
- The `KeyStore` interface for the root, and the back ends for the operating system's store, the passphrase route, and memory for tests.
- The `KeyRecords` interface through which wrapped workspace and owner keys are kept, and nothing else of the store.
- The `Redactor`, which implements `a2m-core`'s `Redact`, and the rule that every secret this crate releases is registered with it before the caller receives it.
- The installation's digest key, and `VaultDigest`, which implements `a2m-core`'s `KeyedDigest` with it.
- Rotation of each layer. Not built yet.

It must never: write a key, or a plain item, to disk; write cryptography of its own; return a secret in any form but `a2m-core`'s `Secret`; print, log, or put in a `Failure` any key, plain item, or part of one; decide whether a job may act as an owner; know a mode, a pack, or a kind of system.

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
pub struct PassphraseKeyStore;  // PassphraseKeyStore::derive(PassphraseRoute, &Secret, &PassphraseSalt).await
pub struct MemoryKeyStore;      // MemoryKeyStore::new(); remove_root() stands for a reset credential store
pub enum PassphraseRoute { NotEnabled, Enabled }     // the protected setting, read by the caller
pub struct PassphraseSalt(/* 16 bytes, not secret; the caller keeps it */);

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
    pub fn with_random(store, records, random: Arc<dyn RandomSource>) -> Vault;   // for tests

    /// A fresh data key and nonce for each call; a rewritten item gets a new key, which is its rotation.
    pub async fn seal(&self, cx: &SealContext, plain: &[u8]) -> Result<Sealed, Failure>;

    /// Opens bytes that are not secrets: evidence, ledger values. The caller keeps them out of logs.
    pub async fn open(&self, cx: &SealContext, sealed: &Sealed) -> Result<Opened, Failure>;

    /// Opens a secret. The value is registered with the redactor before this returns.
    pub async fn open_secret(&self, cx: &SealContext, sealed: &Sealed) -> Result<Secret, Failure>;

    /// For a secret that reached the runtime some other way, such as a token a sign-in returned.
    /// The session keeper calls it before the secret goes anywhere else.
    pub fn register(&self, secret: &Secret);

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
    pub fn health(&self) -> RedactorHealth;           // registered, short, bound
}

pub struct VaultDigest(/* the installation's digest key, in wiped memory */);
impl KeyedDigest for VaultDigest { /* the trait from `a2m-core` */ }

pub mod codes { /* the failure codes below, as `FailureCode` constants */ }
```

Contracts that the signatures do not show:

- **The owner rule is cryptographic, not a check.** Each owner's items are sealed under that owner's key, and `SealContext.owner` is authenticated with the ciphertext. An item sealed for one owner cannot be opened under another's context, by any caller. Whether the job asking is really acting for that owner, and whether its runner holds the job's lease, is decided by the session keeper and the scheduler before they call. The vault cannot see leases, and says so rather than pretending.
- **A key's identifier comes from the context, never from a record.** `KeyId` is a digest of what the key is for. When an owner's key is unwrapped, the workspace key above it is the one of the context's workspace, whatever the record says, and both identifiers are authenticated with the wrapped key. A record copied from another owner or another workspace opens nothing.
- **The cipher is XChaCha20-Poly1305,** with a random 24-byte nonce for every seal and every wrap. The randomness comes through `RandomSource`, so a test can fix it.
- **The root is made by the first `wrap` and never by `unwrap`.** A local root carries a random identifier, which travels in every key it wraps. After a credential store is reset, a key the old root wrapped is recognized and fails as `key_store_entry_missing`, also once a new root exists.
- **The redactor masks every form a secret travels in.** Exact matches are the rule of section 217. Percent-encoding and JSON escapes are found by decoding the text and searching what it decodes to, so every spelling of an escape is covered: once, twice, and JSON inside a percent-encoded value. Base64 is found by searching for what the secret encodes to, in both alphabets, at each of the three positions its bytes can fall in, so a secret inside a larger encoded value is found too; the mask takes the neighbouring character on each side, which holds the secret's remaining bits. Each match is masked as `[redacted]`. Hexadecimal and other encodings are not covered.
- **Every secret that is not empty is registered, however short.** A short secret may mask unrelated text, which is the safer error. `RedactorHealth` counts the secrets shorter than eight bytes so that `doctor` can say so. One limit: a base64 form shorter than four characters is not searched for, so a secret of one or two bytes is masked exactly, percent-encoded, and JSON-escaped, and not inside base64.
- **`contains_secret` finds exactly what `redact` would mask,** and text that `redact` returned never contains a secret. A match that lies inside a mask already in the text is left alone by both; when masking uncovers another secret the text is masked again; and if that does not end in four rounds, the whole text becomes one mask.
- **Registration outlives the secret's use.** A registered value stays registered for the life of the process, so that a log line written after a session ends is still masked. The set has a bound of 4096 secrets; passing it is reported through `health`, never handled by dropping old entries.
- **Redaction never fails and never shortens the work.** `redact` has no error path. It matches over the whole text, whatever its length. If the matcher's automaton cannot be built, each form is searched for in turn, which is slower and finds the same matches.
- **A synchronous key store runs on a blocking thread.** The desktop key-store crates are synchronous, so calls to them, and the passphrase derivation, are moved off the async threads.
- **An unreachable key store is not an error the caller works around.** Items already opened stay in memory and keep working; nothing new is sealed or opened; the kernel reports the degraded state of section 224.
- **`StoreHealth::EntryMissing` on a new installation is expected** until its first item is sealed. `a2m-actions`, which can ask the store whether any key record exists, tells the two cases apart.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-ledger` | `seal` and `open` for blobs and ledger values; `Sealed::to_bytes` and `from_bytes`; implements `KeyRecords`; the `VaultDigest`, as a `KeyedDigest`, for blob names |
| `a2m-auth` | `seal` and `open_secret` for saved session state and stored credentials; `register` for a secret a sign-in returns; `OwnerRef::of` an identity |
| `a2m-telemetry` | The `Redactor`, as a `Redact`, for every record, artifact, span, and log line; `contains_secret` for the check on the way out; the `VaultDigest` for restricted values |
| Every front door | The `Redactor`, as a `Redact`, on every result and error |
| `a2m-actions` | A back end and `Vault::new` at start; `keyed_digest` and `redactor` to hand on; `seal` for `secret.set`; `health` for `doctor` and the kernel's health state; later `rotate` for `vault.rotate` |
| `a2m-provider` | A `KeyStore` adapter for a key-service back end that a plug-in supplies, joined by `a2m-actions` |

It calls only `a2m-core`, and through its two traits, the back end and the key records it was given.

## State

| State | Where | Notes |
| --- | --- | --- |
| The root key | The operating system's key store, or a key service | One entry per installation, under the service name `anything2mcp` and the installation's name, never a target's |
| Wrapped workspace and owner keys, and the wrapped digest key | The store, through `KeyRecords` | A table the ledger owns; one row per key and generation |
| Wrapped data keys | Inside each `Sealed` envelope | Beside the data, as section 216 says |
| Unwrapped upper keys | Memory, wiped on drop | Cached for the process; dropped on rotation |
| Registered secrets | Memory, wiped on drop, inside the `Redactor` | Never written anywhere. The matcher's automaton holds a copy that is freed but not wiped when the set changes |
| The passphrase salt | Wherever the caller keeps it | Not secret. Made once per installation |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `key_store_unreachable` | The back end does not answer, or this machine has none this build can reach. The kernel is degraded until it does |
| `busy` | `key_store_entry_missing` | The root that wrapped a key is gone, as when a credential store is reset. The next step says that saved sessions must be signed in again |
| `input` | `sealed_item_mismatch` | An envelope does not open under the context given: a swapped item, a wrong owner, a damaged envelope or key record, or a secret that is not text. Never says which |
| `input` | `sealed_format_unsupported` | An envelope's format or cipher id is one this engine does not know |
| `policy` | `vault_passphrase_not_enabled` | The passphrase route was asked for and is not switched on as a protected setting |
| `input` | `vault_passphrase_mismatch` | The passphrase given at start is not the one the keys were wrapped under |
| `busy` | `key_record_conflict` | A key record exists already, or changed before a rotation could replace it; nothing changed |
| `busy` | `randomness_unavailable` | The operating system gave no random bytes, so no key or nonce was made |
| `input` | `schema_invalid` | An owner, an item identifier, or an installation name is empty, longer than 255 bytes, or holds a control character |

A failure from this crate never names a key, a value, or a byte of either, and never says which of the reasons for a mismatch applied: that would help someone trying envelopes. The text of an error from an outside key-store crate is never copied into a failure.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A planted secret appears in no result, log, trace, or artifact | The planted-secret test of package 2.1, against the test application. Not written yet: it needs `a2m-telemetry` and a front door |
| A registered secret never survives `redact` in any encoded form | `tests/redactor.rs`: sixteen forms of one secret, and a secret inside a larger base64 value at every alignment |
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
| Rotating an upper layer re-wraps keys and re-encrypts no data | Not built: with `rotate` |

## Build order

1. Done: the `Redactor`: registration, the encoded forms, `contains_secret`, the rule for short secrets. The planted-secret test of package 2.1 is not written; it belongs with `a2m-telemetry` and a front door.
2. Done: the cipher crate, the key-store crates, the memory-wiping crate, and the rest verified and recorded in [decision 0074](../../decisions/0074-the-vaults-cipher-key-stores-and-other-outside-crates.md).
3. Done: `KeyStore` with the operating-system back end and the in-memory one; `KeyRecords` with an in-memory implementation; `seal`, `open`, `open_secret`; `keyed_digest`. The operating-system back end is proven against the outside crates' in-memory credential store on every machine. Against a real key store it is one ignored test, which has not been run on any of the three systems: this build machine has none. The Windows and macOS code is type-checked from Linux and has never run.
4. Done here: the restart test, over the in-memory records. Not done: `KeyRecords` in `a2m-ledger`.
5. Done: the passphrase route, behind `PassphraseRoute`, which the caller fills from the protected setting.
6. Not built: a key-service back end, for the server profile. Phase 6.
7. Not built: `rotate`, with `vault.rotate`. Phase 9. `KeyRecords` already keeps one row per generation and has `replace` for it.

## Open points

- **Locking secret memory is not done.** It needs calls to each operating system that the workspace's rule against unsafe code forbids without a stated reason, and it does not cover copies the outside crates make. Keys and plain items are wiped on drop; that is all ([brief 5](../../research/11-open-research.md)).
- **`a2m-core`'s `Secret` is not wiped when it is dropped,** though section 216 says a secret in use is. The vault cannot fix that from here.
- **Recovery after the root is lost.** The wrapped keys in the store are then dead, and nothing new can be sealed for those workspaces until their records are replaced. Which action does that, and what it does with the sealed items that can no longer be read, is not designed.
- **What rotation of an owner's key means.** Data keys live in the envelopes, so re-wrapping them would touch every sealed item. The records are therefore kept per generation: a rotated owner key gets a new generation for new items, and the old one stays readable until its items are rewritten. Step 7 confirms or changes this.
- **A second engine does not see a rotation** made by the first until it restarts, because the newest generation of each key is cached. Step 7 settles it.
- How the Windows credential store behaves for a service running under its own account.
- **Linux has no operating-system key store in this build.** The Secret Service back end was removed when the advisory check refused its tree ([decision 0123](../../decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)); the passphrase route is the one route there until the store crate stands on maintained crates.
- **Registering many secrets one at a time, each followed by a search,** rebuilds the matcher each time. Registrations in a row with no search between them build it once. If a session keeper's pattern of use makes this slow, the matcher can be built in two tiers.
