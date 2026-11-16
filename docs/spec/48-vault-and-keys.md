# 48. The vault and its keys

The vault is where everything that must stay secret is kept encrypted: saved sessions, tokens, stored credentials, evidence, and the values the ledger holds. This section defines the key layout, where the top key lives on each kind of machine, how the session-ownership rule is backed by encryption, and what happens on rotation, backup, and loss. It extends the short description in [section 15](15-identity-sign-in-and-sessions.md).

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
| Workspace key | Workspace | Owner keys | Wrapped by the root key, in the database |
| Owner key | Person or service identity | That owner's data keys | Wrapped by the workspace key |
| Data key | Session, credential, or blob | The data itself | Wrapped by the owner key, beside the data |

- Data is encrypted with a standard authenticated cipher from a vetted Rust library. Web2MCP writes no cryptography of its own.
- Each ciphertext is bound to what it is: its identifier, owner, and kind are authenticated with it, so one encrypted item cannot be swapped for another.
- The specific library and cipher are chosen in Phase 2 and recorded as a decision.

## Where the root key lives

| Machine | Root key store | Notes |
| --- | --- | --- |
| Windows, macOS, or Linux desktop | The operating system's credential store: Credential Manager, Keychain, or Secret Service | A maintained Rust crate family covers all three ([research note 15](../research/15-traces-and-key-stores.md)) |
| Server | An external key service or secrets manager; the root key is used there and never exported | Preferred for every server profile |
| Linux with no desktop session | Secret Service is usually absent. Use an external service, or a key derived from a passphrase supplied at start | The passphrase route is an explicit opt-in and is reported as weaker |

The engine never falls back to writing a key to disk in the clear. `web2mcp doctor` reports which store is in use and warns about the weaker ones.

## Encryption behind the ownership rule

A session signed in as a person may be used only by that person or an agent acting for them ([section 44](44-session-keeper.md)). On a server this is backed by keys:

- Each person's saved sessions are encrypted under that person's owner key.
- The owner key is unwrapped only to serve a job whose identity is that owner, and only for a runner holding that job's lease.
- No API returns a session's plain contents to anyone, including administrators.

**The limit.** Someone who controls both the server and the key service can decrypt. That is true of any system that can use a secret on a person's behalf while they are away. It is reduced by the key service's own access rules and logs, and it is removed entirely for sessions that stay on a person's own device.

## Secrets in use

- A secret exists in the clear only in memory, in a wrapper type that cannot be printed, logged, or serialized and is wiped when dropped.
- Every secret released by the vault is registered with the redactor ([section 47](47-traces-and-evidence.md)).
- Target files never contain secrets. They refer to vault entries by name.
- `web2mcp secret set` reads the value from a prompt or standard input, never from a command-line argument, so it does not land in shell history or an agent's context.
- No action reads a secret back. There is no `secret get`.

## Rotation

| What | How | Cost |
| --- | --- | --- |
| A data key | Replaced whenever its item is rewritten | None extra |
| An owner or workspace key | Re-wrap the keys beneath it | Small; no data is re-encrypted |
| The root key | By the key service, or `web2mcp vault rotate` locally | Re-wrap the workspace keys |

If a key is suspected of being exposed, rotating it is not enough: the sessions it protected must be revoked, because a copied session stays valid until the application ends it.

## Backup and loss

- Backups contain ciphertext only. Restoring on another machine also needs the root key or access to the key service.
- On a personal machine, losing the credential-store entry means saved sessions and stored evidence cannot be read. People sign in again; nothing in the application is lost.
- On a server, the key service's own backup and recovery apply.

## Interface

Key stores sit behind one small interface with three operations: wrap a key, unwrap a key, and report health. Back ends: the operating system's store, cloud key services, a secrets manager, and the passphrase route. New back ends are plug-ins.

## Not covered yet

- Binding a runner's identity key to hardware.
- Proving to the control plane what software a runner is running.

Both are worth doing for high-assurance deployments and neither blocks the first release.

## Open points

- The cipher and library, and whether locking secret memory is practical on each operating system.
- How the Windows credential store behaves for a service running under its own account.
- Research brief 5 covers these.
