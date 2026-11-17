# 0075. The vault's interface, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** four open points of the [`a2m-vault` design](../architecture/crates/a2m-vault.md): the redactor's rule for short secrets, who holds the keyed digest's key, whether a plug-in key service may run out of process, and the shape of `KeyRecords`. Changes the interface sketched there and in [decision 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md). **Changes sentences of [spec section 216](../spec/2-kernel/216-the-vault-and-its-keys.md) and [217](../spec/2-kernel/217-traces-evidence-and-repair.md).**

## Decision

**The interface**

- **`Vault::new(store, records)` makes the vault; `open` opens an item.** The sketch named both `open`, which one type cannot have. Making a vault reads nothing, so it is neither asynchronous nor fallible.
- **`KeyRecords` keeps one row per key and generation.** `get` takes the generation, `newest` gives the highest, `insert` refuses an existing row, and `replace` swaps a row only if its wrapped bytes are still the ones expected. The sketch had one row per key, which cannot serve rotation: data keys live in the envelopes, so after an owner's key is rotated the old generation must stay readable until its items are rewritten. *Instead of:* re-wrapping every envelope at rotation, which the vault cannot do because it does not hold them.
- **A key's identifier is a digest of what the key is for,** so it is the same on every start and shows no name. The vault derives it from the context and never trusts the one in a record or an envelope.
- **The two traits return boxed futures,** written by hand, because both are used as trait objects. *Instead of:* a macro crate for asynchronous traits.
- **The root is made by the first `wrap`, never by `unwrap`,** and carries a random identifier that travels in every key it wraps. A key wrapped by a root that is gone fails as `key_store_entry_missing` even after a new root exists. *Instead of:* a separate operation to create the root, which every back end and every caller would have to order correctly.
- **`SealContext.workspace` is `a2m-core`'s `WorkspaceId`.** The owner is the vault's own `OwnerRef`, a person or a service identity, because blobs and runner keys have owners that are not an `Identity`; `OwnerRef::of` makes one from an `Identity`.
- **Two failure codes are added:** `vault_passphrase_mismatch` (`input`) and `randomness_unavailable` (`busy`).

**The redactor**

- **Every secret that is not empty is registered, however short.** A short secret may mask unrelated text; refusing it would let it through. Health counts secrets shorter than eight bytes. A base64 form shorter than four characters is not searched for, since it would match in most text: so a secret of one or two bytes is not found inside base64.
- **Percent-encoding and JSON escapes are found by decoding the text,** not by listing spellings: once, twice, and JSON inside a percent-encoded value. Base64 is found by its three alignments in both alphabets, and the mask takes one neighbouring character on each side.
- **The check on the way out is `contains_secret`, and it agrees with `redact`.** A match inside a mask already in the text is ignored by both, and masking repeats when it uncovers another secret, so redacted text never contains one.
- **The bound on the registered set, 4096, is reported and never enforced by dropping.**

**The keyed digest**

- **The vault holds one digest key per installation,** made at first use and kept as a key record of layer `digest`, wrapped by the root. Each workspace's key is derived from it by `a2m-core`'s one construction ([decision 0069](0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)). `Vault::keyed_digest` returns a `VaultDigest`, which implements `KeyedDigest` and which `a2m-actions` hands to the ledger and to telemetry. *Instead of:* a stored key per workspace, which the trait's method, being synchronous and infallible, could not fetch at first use.
- The digest key is never rotated by itself: rotating it renames every blob.

**A plug-in key service out of process**

- **It may run out of process, and then it must be a key service in the full sense:** the root stays inside it, and only the workspace keys and the digest key cross the boundary, to be wrapped and unwrapped. That is the same exposure as any remote key service, and the binding sent with each call lets the service refuse a key it did not wrap for that purpose.
- **A plug-in must never be a place that hands the root itself to the engine.** The `KeyStore` trait has no operation that returns a root, so this is not expressible.
- The channel to such a plug-in carries key material, so it must be local to the machine or encrypted and authenticated; [`a2m-provider`](../architecture/crates/a2m-provider.md) owns that when the back end is built.

## Consequences

- `a2m-ledger` implements the four methods of `KeyRecords` over a table keyed by identifier and generation, and returns `key_record_conflict()` from `insert` and `replace`.
- `a2m-actions` chooses the back end, calls `Vault::new`, and hands on `redactor()` and `keyed_digest()`.
- Section 216 names the cipher, and says how the digest key is held. Section 217 says the redactor masks encoded forms.
- Still open, in the crate's design: recovery after the root is lost, and what a second engine sees after a rotation.
