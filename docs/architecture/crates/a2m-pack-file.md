# `a2m-pack-file`

The file pack: the `file` driver: values read from and written to named places in files under declared roots, by replacing the whole file. The pack is designed in [spec section 308](../../spec/3-packs/308-the-file-pack.md); this document says how its crate is put together, and is completed by the package that builds it.

## Facts

| | |
| --- | --- |
| Side | Pack: file |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | Nothing yet. To come: `a2m-core`, `a2m-files` for registration, `a2m-auth` for the session lease, `a2m-provider` for the contract, `a2m-telemetry` |
| Built in | Packages H2, H3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [308](../../spec/3-packs/308-the-file-pack.md) |

## Owns

What section 308 gives the pack: its mode, its file kinds, its lowerings and lint rules, its driver, its discoverer, and its drafting rules. It must never reach the engine, the scheduler, the store, or another pack's crates.

## Interface

One public function that registers the pack with the registry builder of `a2m-provider`. Everything else is private.

## Used by, and uses

Used by the product crate only, which hands the pack to the registry. Uses the provider contract and the layers below it.

## State

None of its own. A live connection, process, or file handle belongs to a session on its home runner and is held behind the driver.

## Failures

The failure classes and codes of section 308.

## Invariants

Those of section 308, each proven against the pack's test stand under `targets/`.

## Build order

1. Not started.

## Open points

- Everything the building package settles.
