# 0123. The Linux Secret Service back end is left out until its store crate stands on maintained crates

**Status:** Accepted. Design decision, taken by the lead agent when the first run of the advisory check failed. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** [record 0074](0074-the-vaults-cipher-key-stores-and-other-outside-crates.md), which put the Linux desktop key store behind a cargo feature of `a2m-vault`.

## Context

Record 0074 chose a pure-Rust client of the Secret Service for Linux desktops, behind the cargo feature `secret-service`, off by default. The licence check passed. The advisory check, which only CI can run in full, did not: three crates in that client's tree (`zvariant`, `zvariant_derive`, and `zbus_names`) were declared unmaintained in 2026, because their project merged them into a new major version of its main crate. The store crate has not moved to that version yet. The check counts every feature, so an optional dependency fails the build.

## Decision

The feature and its dependency are removed. On Linux, `OsKeyStore::open` answers `busy`/`key_store_unreachable`, as it already did on a machine with no desktop session, and the root key comes from the passphrase route. Windows and macOS are unchanged.

The advisory is not put on an ignore list. The rule of this repository is that a check is never weakened to get to green, and an unmaintained crate under the vault is exactly what the check is for.

## What follows

- A Linux desktop has no operating-system key store in this build. The passphrase route is the only one there, and the protected setting that allows it must be chosen by a person.
- Section 605's sentence about building Linux desktop releases with the feature no longer applies.
- When the store crate is published on maintained crates, the back end returns: one dependency, one `cfg` branch, and the ignored test that exercises a real store.

## Rejected

- **Ignore the three advisories.** They are notices of a rename, not vulnerabilities, and the feature is off by default. It would still be a named exception in the one check that guards the supply chain, added on the first day the check ran.
- **The client that links the system's D-Bus library.** It needs a system library on every build machine, or a C build of that library.
- **The kernel's keyring.** Its contents are lost at restart.

## What would reopen it

A release of the store crate on the maintained major version; or evidence that the passphrase route is not acceptable for Linux desktop users before then.
