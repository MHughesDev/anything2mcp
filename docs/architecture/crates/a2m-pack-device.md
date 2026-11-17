# `a2m-pack-device`

The device pack: the `device` driver: points read from and written to a device over its link, with ranges enforced before a frame is sent. The pack is designed in [spec section 310](../../spec/3-packs/310-the-device-pack.md); this document says how its crate is put together, and is completed by the package that builds it.

## Facts

| | |
| --- | --- |
| Side | Pack: device |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | Nothing yet. To come: `a2m-core`, `a2m-files` for registration, `a2m-auth` for the session lease, `a2m-provider` for the contract, `a2m-telemetry` |
| Built in | Packages K2, K3, K4 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [310](../../spec/3-packs/310-the-device-pack.md) |

## Owns

What section 310 gives the pack: its mode, its file kinds, its lowerings and lint rules, its driver, its discoverer, and its drafting rules. It must never reach the engine, the scheduler, the store, or another pack's crates.

## Interface

One public function that registers the pack with the registry builder of `a2m-provider`. Everything else is private.

## Used by, and uses

Used by the product crate only, which hands the pack to the registry. Uses the provider contract and the layers below it.

## State

None of its own. A live connection, process, or file handle belongs to a session on its home runner and is held behind the driver.

## Failures

The failure classes and codes of section 310.

## Invariants

Those of section 310, each proven against the pack's test stand under `targets/`.

## Build order

1. Not started.

## Open points

- Everything the building package settles.
