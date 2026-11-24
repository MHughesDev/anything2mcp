# 0417. The key a local person's runs are sealed under is opened at the start and held for the life of the process

**Status:** Accepted. Design decision, made by the worker who fixed the faults packages 9.3h and 9.3i found, under the lead's brief. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** [`a2m-vault`](../architecture/crates/a2m-vault.md), which gains `Vault::hold_owner_key`; [`a2m-actions`](../architecture/crates/a2m-actions.md), whose `Kernel::open` calls it; the map of proofs in [spec section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md). It leaves the vault's four key layers, the key store's interface, and what a run's trace and evidence keep as they are.

## Context

Section 224 says that a kernel degraded by a key store that has become unreachable still takes reads. A read keeps its input, its trace, and its evidence as blobs, each sealed under the key of the person whose run it is, and that key is wrapped under the workspace's key, which the key store's root wraps. The vault keeps every key it unwraps in wiped memory for the life of the process, but it opened a person's key only at that person's first seal. A kernel that had sealed nothing since its start therefore needed the key store for its first read, and refused it `busy`/`key_store_unreachable` once the store had stopped. `a2m-actions` `a_key_store_that_stops_before_any_read_still_lets_a_read_run` found it.

Three ways were weighed:

- **Open the key at the start, and hold it.** The vault already holds every unwrapped upper key in wiped memory until the process ends ([its design](../architecture/crates/a2m-vault.md), "Where things are kept"); a read that ran before the store stopped holds the same key in the same way. Opening it a little earlier holds nothing the process would not hold after its first read.
- **Seal the trace under a key that needs no key store.** Any such key is either kept on disk beside what it seals, which protects nothing, or derived from something the process holds anyway, which is the first way with a new key layer and a new cryptographic path.
- **Run the read and keep its trace at a capture level that holds no secret, in clear.** A trace without its values still names the run, the tool, its input's shape, and the steps, which every other run keeps sealed; and a read's evidence would then depend on the health of the key store, which section 224 does not say.

## Decision

1. **At `Kernel::open`, on a local profile (`solo`, `workstation`, `embedded`), with the vault joined to the store, the kernel opens the newest key of the local person in the installation's workspace,** through `Vault::hold_owner_key`, and the vault holds it as it holds any unwrapped key: in wiped memory, until the process ends or the key is rotated. When the person has no key yet it is made and recorded, as the first seal would make it. Nothing of the key is returned, logged, or kept anywhere else.
2. **A key store that stops after the start then leaves that person's reads their evidence, sealed as ever.** Writes are still refused `busy`/`engine_degraded` while the kernel is degraded, and nothing new that needs the key store, such as a session restored or another workspace's key, is opened. The workspace's own key is held with the person's, so a key for another person of the same workspace can still be made; on one machine there is no other person.
3. **A failure to open the key at the start is a reason the kernel is degraded,** named with the vault as its part, as an unreachable store is. The vault only joins the store when the store answered for the digest key a moment before, so this is rare.
4. **A server profile opens each person's key at that person's first use,** as before: it serves many people, and opening every key at the start would hold keys for people who never call.

## What was rejected

- A key that needs no key store, and a trace kept in clear at a reduced capture level, for the reasons above.
- **Opening the key at the first read instead.** It is what was built, and it is the fault.

## Consequences

- `a2m-actions` `a_key_store_that_stops_before_any_read_still_lets_a_read_run` runs, with no `ignore`.
- `a2m-vault` `an_owner_key_held_at_start_seals_and_opens_after_the_key_store_stops` holds the vault's half: the person's items seal and open after the store stops, a key of another workspace, which needs the root, is refused `key_store_unreachable`, holding again records nothing new, and what was sealed opens after a restart.
- The tests of `a2m-actions` give the key store a counted random source of its own. Two counted sources that both start at nought hand out one stream, and the person's key, made earlier now, was wrapped under a nonce equal to its own first bytes, which the vault refuses as a stuck source. In a running engine both are the operating system's.
- An engine started while the key store was unreachable is still `degraded` until it is started again, with no vault joined; section 224 says so, and it is not built.

## Revisit if

A local machine serves more than one person, or a key's rotation is built: a rotated key is then opened again at the next start, and the one held must be dropped.
