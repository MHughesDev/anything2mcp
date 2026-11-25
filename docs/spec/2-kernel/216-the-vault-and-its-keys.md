# 216. The vault and its keys

The vault is where everything that must stay secret is kept encrypted: saved sessions, tokens, stored credentials, evidence, and the values the ledger holds. This section defines the key layout, where the top key lives on each kind of machine, how the session-ownership rule is backed by encryption, and what happens on rotation, backup, and loss. It extends the short description in [section 215](215-sessions-and-the-session-keeper.md). Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## What is protected

| Item | Why it matters |
| --- | --- |
| Saved session state and tokens | Equivalent to being signed in as the person |
| Stored credentials for service accounts | Equivalent to the account |
| Evidence blobs and ledger values | May contain business data |
| Runner private keys | Let a runner claim work |

## Key layout

Encryption is layered, so that most keys never need to leave the machine and rotating one layer does not mean re-encrypting everything.

| Layer | One per | Protects | Kept |
| --- | --- | --- | --- |
| Root key | Installation | Workspace keys | In the operating system's key store, or in an external key service that never releases it |
| Workspace key | Workspace | Owner keys | Wrapped by the root key, in the store |
| Owner key | Person or service identity | That owner's data keys | Wrapped by the workspace key |
| Data key | Session, credential, or blob | The data itself | Wrapped by the owner key, beside the data |

- Data is encrypted with a standard authenticated cipher from a vetted Rust library. Anything2MCP writes no cryptography of its own.
- Each ciphertext is bound to what it is: its identifier, owner, and kind are authenticated with it, so one encrypted item cannot be swapped for another.
- The cipher is XChaCha20-Poly1305, from the RustCrypto libraries ([decision 0074](../../decisions/0074-the-vaults-cipher-key-stores-and-other-outside-crates.md)).

**The vault seals and opens, and stores nothing itself.** Sealing returns a sealed item, and the caller keeps it: the ledger keeps blobs and ledger values, and the session keeper keeps saved session state and credentials through the ledger. The data key travels inside the sealed item, wrapped. The wrapped workspace and owner keys must persist, and the vault sits below the store, so the vault defines a small interface for keeping them, `KeyRecords`, which the store implements over a table of its own ([`a2m-vault`](../../architecture/crates/a2m-vault.md)). A key record is inserted once and replaced only by rotation, never overwritten by accident.

## Where the root key lives

| Machine | Root key store | Notes |
| --- | --- | --- |
| Windows, macOS, or Linux desktop | The operating system's credential store: Credential Manager, Keychain, or Secret Service | A maintained Rust crate family covers all three ([research note 15](../../research/15-traces-and-key-stores.md)). This build has no Secret Service back end ([decision 0123](../../decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)), so a Linux desktop takes the row below |
| Server | An external key service or secrets manager; the root key is used there and never exported | Preferred for every server profile |
| Linux with no desktop session | Secret Service is usually absent. Use an external service, or a key derived from a passphrase supplied at start | The passphrase route is reported as weaker. Where the build has an operating-system key store, choosing it is an explicit opt-in in the installation's settings; where the build has none, it is the default, because there is nothing stronger to move off ([decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)) |

The engine never falls back to writing a key to disk in the clear. `anything2mcp doctor` reports which store is in use and warns about the weaker ones.

**A root is made once.** The operating systems' key stores cannot write an entry only when there is none, so two engines that start at the same moment on a fresh installation could each write a root, and what was wrapped under the one written first would never open again. A root is therefore made under a lock outside the key store, an operating-system lock on a file that every engine of the user agrees on, and a key is always wrapped under the root the store holds at that moment. On the passphrase route the same holds for the salt, which is made once under a lock beside its file. A passphrase shorter than 16 bytes is refused, `input`/`vault_passphrase_too_short`: a guess can be checked away from the machine, so nothing slows a search but the passphrase itself ([decision 0138](../../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md)). The salt's file is to record, beside the salt, the derivation that made the root (the algorithm, its version, the memory, the passes, and the lanes), and a root is to be derived with the cost its file records, so that a later release can change the cost of a new installation without locking out an old one; a file of the salt alone, as every build so far writes, reads as today's cost ([decision 0438](../../decisions/0438-the-passphrase-routes-salt-file-records-the-cost-its-root-was-derived-with.md); package YL3, not built).

## Encryption behind the ownership rule

A session signed in as a person may be used only by that person or an agent acting for them ([section 215](215-sessions-and-the-session-keeper.md)). On a server this is backed by keys:

- Each person's saved sessions are encrypted under that person's owner key.
- The owner key is unwrapped only to serve a job whose identity is that owner, and only for a runner holding that job's lease.
- No API returns a session's plain contents to anyone, including administrators.

**The limit.** Someone who controls both the server and the key service can decrypt. That is true of any system that can use a secret on a person's behalf while they are away. It is reduced by the key service's own access rules and logs, and it is removed entirely for sessions that stay on a person's own device.

## Secrets in use

- A secret exists in the clear only in memory, in a wrapper type that cannot be printed, logged, or serialized and is wiped when dropped.
- Every secret released by the vault is registered with the redactor ([section 217](217-traces-evidence-and-repair.md)).
- Target files never contain secrets. They refer to vault entries by name.
- `anything2mcp secret set` reads the value from a prompt or standard input, never from a command-line argument, so it does not land in shell history or an agent's context.
- No action reads a secret back. There is no `secret get`.

## Naming without revealing

Evidence is named by a keyed digest of its content, not a plain hash, so that someone who can read the store cannot confirm a guessed value by hashing it. The vault keeps one digest key per installation, wrapped by the root, and derives each workspace's key from it, so nothing can be matched across workspaces ([decision 0075](../../decisions/0075-the-vaults-interface-as-building-it-settled-it.md)). The digest is offered through the same interface in `a2m-core` as the redactor, so the ledger and telemetry can ask for one without depending on each other or on the vault.

## Rotation

| What | How | Cost |
| --- | --- | --- |
| A data key | Replaced whenever its item is rewritten | None extra |
| An owner or workspace key | Re-wrap the keys beneath it | Small; no data is re-encrypted |
| The root key | By the key service, or `anything2mcp vault rotate` locally | Re-wrap the workspace keys |

If a key is suspected of being exposed, rotating it is not enough: the sessions it protected must be revoked, because a copied session stays valid until the system ends it.

## Backup and loss

- Backups contain ciphertext only. Restoring on another machine also needs the root key or access to the key service.
- On a personal machine, losing the credential-store entry means saved sessions and stored evidence cannot be read. People sign in again; nothing in the system is lost.
- On a server, the key service's own backup and recovery apply.

## Interface

Key stores sit behind one small interface with three operations: wrap a key, unwrap a key, and report health. Back ends: the operating system's store, cloud key services, a secrets manager, and the passphrase route. New back ends are plug-ins.

## Not covered yet

- Binding a runner's identity key to hardware.
- Proving to the control plane what software a runner is running.

Both are worth doing for high-assurance deployments and neither blocks the first release.

## Open points

- Whether locking secret memory is practical on each operating system. It is not done: keys and plain items are wiped when dropped.
- How the Windows credential store behaves for a service running under its own account.
- Research brief 5 covers these.
