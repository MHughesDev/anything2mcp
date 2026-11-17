# `a2m-vault`

Sealing and opening: envelope encryption in four key layers, the key-store back ends that hold the root, and the redactor that removes every released secret from text. It turns plain bytes into sealed bytes and back. It does not decide where sealed bytes are kept, and it never decides who may open them.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside, each to be verified and recorded: an authenticated cipher from a vetted Rust library; a crate over the three desktop key stores ([research note 15](../../research/15-traces-and-key-stores.md) names one); a memory-wiping crate; a multi-pattern string matcher; `tokio` for blocking calls to synchronous key stores |
| Built in | Roadmap package 2.1, the redactor and the planted-secret test; package 2.2, sealing with the operating system's key store. The key-service back ends with the server profile, Phase 6; `vault.rotate` in Phase 9 |
| Carries out | Spec sections [48](../../spec/48-vault-and-keys.md) and the redaction steps of [47](../../spec/47-traces-and-evidence.md); [decision 0038](../../decisions/0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md) and [decision 0030](../../decisions/0030-where-session-secrets-live.md); the engine-fault rows for the key store in [section 52](../../spec/52-when-the-engine-itself-fails.md) |

## Owns

- The key hierarchy: a root key per installation, a key per workspace, a key per owner, a data key per item.
- Sealing a plain item into a `Sealed` envelope bound to what it is, and opening it again.
- The `KeyStore` interface for the root, and the back ends for the operating system's store and the passphrase route.
- The `KeyRecords` interface through which wrapped workspace and owner keys are kept, and nothing else of the store.
- The `Redactor`, which implements `a2m-core`'s `Redact`, and the rule that every secret this crate releases is registered with it before the caller receives it.
- Rotation of each layer.

It must never: write a key, or a plain item, to disk; write cryptography of its own; return a secret in any form but `a2m-core`'s `Secret`; print, log, or put in a `Failure` any key, plain item, or part of one; decide whether a job may act as an owner; know a mode, a pack, or a kind of system.

## Where sealed bytes are kept

**The vault seals and opens; the caller keeps the result.** `seal` returns a `Sealed` value, and the caller stores it: `a2m-ledger` for blobs and ledger values, `a2m-auth` through `a2m-ledger` for saved session state and credentials. The data key travels inside the envelope, wrapped, so a sealed item needs nothing beside it but the upper keys.

**The upper keys are kept through a small trait the vault defines and the store implements.** Wrapped workspace and owner keys must persist, and the vault cannot depend on `a2m-ledger`, which sits above it. So the vault defines `KeyRecords`; `a2m-ledger` implements it over a table of its own; `a2m-actions` joins the two at `Kernel::open`. The other way round, a vault that hands wrapped keys to every caller to store, was weighed and refused: it would spread key handling across every crate that seals.

The order at start is therefore: the store opens its connection and runs migrations; the vault opens with the root back end and the store's `KeyRecords`; the ledger is then given the vault. Neither calls the other during construction.

## Interface

```rust
/// The root back end: three operations, as section 48 says. A desktop back end holds the root in
/// the operating system's store and wraps in memory; a key service wraps remotely and never
/// releases the root.
pub trait KeyStore: Send + Sync {
    fn describe(&self) -> StoreDescription;          // name, and strength: os_store, key_service, passphrase
    async fn wrap(&self, key: &KeyMaterial, bound_to: &Binding) -> Result<Wrapped, Failure>;
    async fn unwrap(&self, wrapped: &Wrapped, bound_to: &Binding) -> Result<KeyMaterial, Failure>;
    async fn health(&self) -> StoreHealth;           // ok, unreachable, or missing the root entry
}

/// Where wrapped workspace and owner keys persist. Implemented by `a2m-ledger`.
pub trait KeyRecords: Send + Sync {
    async fn get(&self, id: &KeyId) -> Result<Option<KeyRecord>, Failure>;
    /// Insert only. Refuses when the id exists: a key is never overwritten by accident.
    async fn insert(&self, record: KeyRecord) -> Result<(), Failure>;
    /// For rotation: replaces the record only if its generation is still `expected`.
    async fn replace(&self, record: KeyRecord, expected: Generation) -> Result<(), Failure>;
}
pub struct KeyRecord { pub id: KeyId, pub layer: Layer, pub generation: Generation, pub wrapped: Wrapped, pub wrapped_by: KeyId }
pub enum Layer { Root, Workspace, Owner, Data }

/// What a sealed item is. Authenticated with the ciphertext, so one item cannot be swapped for another.
pub struct SealContext {
    pub workspace: WorkspaceId,
    pub owner: OwnerRef,                 // a person or a service identity; never a credential
    pub kind: SealedKind,                // session_state, credential, blob, ledger_value, runner_key
    pub item: ItemId,                    // the blob's content reference, the session's id, the vault entry's name
}

/// The envelope: format version, cipher id, the key ids and generations above it, the wrapped
/// data key, the nonce, and the ciphertext with its tag. Serializable; holds nothing in the clear.
#[derive(Serialize, Deserialize)]
pub struct Sealed { /* ... */ }

pub struct Vault { /* the back end, the key records, unwrapped upper keys cached in wiped memory, the redactor */ }
impl Vault {
    pub async fn open(store: Arc<dyn KeyStore>, records: Arc<dyn KeyRecords>) -> Result<Vault, Failure>;

    /// A fresh data key for each call; a rewritten item gets a new one, which is its rotation.
    pub async fn seal(&self, cx: &SealContext, plain: &[u8]) -> Result<Sealed, Failure>;

    /// Opens bytes that are not secrets: evidence, ledger values. The caller keeps them out of logs.
    pub async fn open(&self, cx: &SealContext, sealed: &Sealed) -> Result<Opened, Failure>;

    /// Opens a secret. The value is registered with the redactor before this returns.
    pub async fn open_secret(&self, cx: &SealContext, sealed: &Sealed) -> Result<Secret, Failure>;

    /// For a secret that reached the runtime some other way, such as a token a sign-in returned.
    /// The session keeper calls it before the secret goes anywhere else.
    pub fn register(&self, secret: &Secret);

    pub fn redactor(&self) -> Arc<Redactor>;          // handed to everything that emits text
    pub async fn health(&self) -> VaultHealth;        // for the kernel's health state and `doctor`
    pub async fn rotate(&self, layer: Layer, scope: RotationScope) -> Result<RotationReport, Failure>;
}

/// Plain bytes, wiped when dropped. Not `Clone`, not `Serialize`, not `Debug` beyond its length.
pub struct Opened(/* ... */);

/// The redactor. Holds the registered secrets in wiped memory and masks every exact match.
pub struct Redactor { /* ... */ }
impl Redact for Redactor {                            // the trait from `a2m-core`
    fn redact(&self, text: &str) -> String;
}
impl Redactor {
    /// The check on the way out: true when any registered secret is in `text`. Cheaper than redact.
    pub fn contains_registered(&self, text: &str) -> bool;
}
```

Contracts that the signatures do not show:

- **The owner rule is cryptographic, not a check.** Each owner's items are sealed under that owner's key, and `SealContext.owner` is authenticated with the ciphertext. An item sealed for one owner cannot be opened under another's context, by any caller. Whether the job asking is really acting for that owner, and whether its runner holds the job's lease, is decided by the session keeper and the scheduler before they call. The vault cannot see leases, and says so rather than pretending.
- **The redactor masks every form a secret travels in.** Exact matches are the rule of section 47. A secret is also registered in the forms text commonly carries it: percent-encoded, JSON-escaped, and base64. Each form is masked as `[redacted]`.
- **Registration outlives the secret's use.** A registered value stays registered for the life of the process, so that a log line written after a session ends is still masked. The set has a size bound; reaching it is reported through `health`, never handled by dropping old entries.
- **Redaction never fails and never shortens the work.** `redact` has no error path. It matches over the whole text, whatever its length.
- **A synchronous key store runs on a blocking thread.** The desktop key-store crate is synchronous, so calls to it are moved off the async threads.
- **An unreachable key store is not an error the caller works around.** Items already opened stay in memory and keep working; nothing new is sealed or opened; the kernel reports the degraded state of section 52.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-ledger` | `seal` and `open` for blobs and ledger values; implements `KeyRecords` |
| `a2m-auth` | `seal` and `open_secret` for saved session state and stored credentials; `register` for a secret a sign-in returns |
| `a2m-telemetry` | The `Redactor`, as a `Redact`, for every record, artifact, span, and log line; `contains_registered` for the check on the way out |
| Every front door | The `Redactor`, as a `Redact`, on every result and error |
| `a2m-actions` | `Vault::open` at start; `rotate` for `vault.rotate`; `seal` for `secret.set`; `health` for `doctor` and the kernel's health state |
| `a2m-provider` | A `KeyStore` adapter for a key-service back end that a plug-in supplies, joined by `a2m-actions` |

It calls only `a2m-core`, and through its two traits, the back end and the key records it was given.

## State

| State | Where | Notes |
| --- | --- | --- |
| The root key | The operating system's key store, or a key service | One entry per installation, named after the installation, never after a target |
| Wrapped workspace and owner keys | The store, through `KeyRecords` | A table the ledger owns; one row per key and generation |
| Wrapped data keys | Inside each `Sealed` envelope | Beside the data, as section 48 says |
| Unwrapped upper keys | Memory, wiped on drop | Cached for the process; dropped on rotation |
| Registered secrets | Memory, wiped on drop, inside the `Redactor` | Never written anywhere |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `key_store_unreachable` | The back end does not answer. The kernel is degraded until it does |
| `busy` | `key_store_entry_missing` | The root entry is gone, as when a credential store is reset. The next step says that saved sessions must be signed in again |
| `input` | `sealed_item_mismatch` | An envelope does not open under the context given: a swapped item, a wrong owner, a damaged envelope. Never says which |
| `input` | `sealed_format_unsupported` | An envelope's format or cipher id is one this engine does not know |
| `policy` | `vault_passphrase_not_enabled` | The passphrase route was asked for and is not switched on as a protected setting |
| `busy` | `key_record_conflict` | A rotation lost a race with another rotation; nothing changed |

A failure from this crate never names a key, a value, or a byte of either, and never says which of the reasons for a mismatch applied: that would help someone trying envelopes.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A planted secret appears in no result, log, trace, or artifact | The planted-secret test of package 2.1, against the test application |
| Every secret `open_secret` returns was registered first | A test that opens a secret, then redacts text containing it in each encoded form |
| No key and no plain item is ever written to disk | A test that seals and opens with the data directory watched, then searches every file for the plain bytes and the key bytes |
| An item sealed for one owner, kind, or id does not open under another | A table test over every field of `SealContext` |
| Rotating an upper layer re-wraps keys and re-encrypts no data | A test that counts envelope bytes changed by `rotate`: none |
| Without the root, nothing opens | The proof of package 2.2: restart with the key-store entry removed, and every open fails with `key_store_entry_missing` |
| `Opened`, `KeyMaterial`, and `Wrapped` cannot be printed or serialized in the clear | A test that fails to compile otherwise, as for `Secret` |

## Build order

1. `Redact` in `a2m-core` and the `Redactor` here: registration, the encoded forms, `contains_registered`. No keys yet. Package 2.1, with the planted-secret test written against `a2m-telemetry` and a front door.
2. Verify and record the cipher crate, the key-store crate, and the memory-wiping crate. Package 2.2, before any key is made.
3. `KeyStore` with the operating-system back end; `KeyRecords` with an in-memory implementation for tests; `seal`, `open`, `open_secret`. Package 2.2.
4. `KeyRecords` in `a2m-ledger`, and the restart test. Package 2.2.
5. The passphrase route for Linux without a desktop session, behind a protected setting. Phase 2.
6. A key-service back end, for the server profile. Phase 6.
7. `rotate`, with `vault.rotate`. Phase 9.

## Open points

- The cipher and its library, and whether locking secret memory is practical on each operating system ([brief 5](../../research/11-open-research.md)).
- **Blob identifiers leak equality.** The ledger schema names a blob by the content hash of its plain data, so anyone who can read the store can confirm a guessed value. A keyed digest, with a key per workspace kept in the vault, would close this. It needs a way for `a2m-telemetry` and `a2m-ledger` to ask for a keyed digest without depending on each other; the likely answer is a second trait beside `Redact` in `a2m-core`.
- Whether the redactor should refuse to register a secret too short to mask safely, such as a four-digit code, or register it and accept masks in unrelated text. Refusing would let the secret through; masking too much is the safer error.
- How the Windows credential store behaves for a service running under its own account.
- Whether a plug-in key-service back end may run out of process at all, since the root's wrap and unwrap would cross a process boundary.
