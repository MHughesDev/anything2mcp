# `a2m-pack-data`

The data pack: the `sql` driver: reads and keyed writes against a relational store, with the store's own catalog as its discoverer. The pack is designed in [spec section 307](../../spec/3-packs/307-the-data-pack.md); this document says how its crate is put together, and is completed by the package that builds it.

## Facts

| | |
| --- | --- |
| Side | Pack: data |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | Nothing yet. To come: `a2m-core`, `a2m-files` for registration, `a2m-auth` for the session lease, `a2m-provider` for the contract, `a2m-telemetry` |
| Built in | Packages G2, G3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [307](../../spec/3-packs/307-the-data-pack.md) |

## Owns

What section 307 gives the pack: its mode, its file kinds, its lowerings and lint rules, its driver, its discoverer, and its drafting rules. It must never reach the engine, the scheduler, the store, or another pack's crates.

## Interface

One public function that registers the pack with the registry builder of `a2m-provider`. Everything else is private.

## Used by, and uses

Used by the product crate only, which hands the pack to the registry. Uses the provider contract and the layers below it.

## State

None of its own. A live connection, process, or file handle belongs to a session on its home runner and is held behind the driver.

## Failures

The failure classes and codes of section 307.

## Invariants

Those of section 307, each proven against the pack's test stand under `targets/`.

## Build order

1. Not started.

## Open points

- Everything the building package settles.
