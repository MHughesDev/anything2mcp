# 0137. A device is held to its serial, a write frame is recorded before it is sent, and a run has a frame ceiling

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** six places where the device pack's code was weaker than [spec section 310](../spec/3-packs/310-the-device-pack.md) on safety, found by reading the code against the section. Three were choices between real alternatives and are decided here: where a device's serial is declared, how the bytes of a write are recorded before they are sent, and what bounds the frames of a run. **Supersedes** in [decision 0107](0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md): the list of step kinds, the lowering table, "the serial is read and not compared", and the default pace of 50 ms. The crate is `a2m-pack-device`; its [design](../architecture/crates/a2m-pack-device.md) lists the tests.

## Context

A device is where a wrong write moves something in the world, so rule two decides every doubt here: safe, then correct, then fast, then easy. The pack as first built compared a device's model and firmware and read its serial without comparing it; kept the bytes of a write only in the step that read the reply; allowed twenty frames a second by default; left two refusals to another part of the product (the discoverer, the compiler); and counted no frames. Each had a test written first that showed it against [the simulated device](../../targets/example-device/README.md).

## Decision

**The serial is declared on the surface, for each environment, and compared in `open`.** A `device_link` surface gains an optional field, `serials`: the serial of the device that answers in each environment, as text, by the environment's name.

```yaml
surfaces:
  controller:
    kind: device_link
    address: "192.0.2.7:502"
    protocol: modbus_tcp
    unit: 1
    serials: {test: "100123", production: "100456"}
```

- `open` compares the serial the device reports with the one declared for the run's environment. Another serial is `unknown_variant`/`device_identity_mismatch`, for reads as well as writes. Neither serial appears in the message.
- Where none is declared for the environment, a read goes on by model and firmware. A write is refused before its frame is built: `policy`/`device_serial_not_declared`, nothing sent.
- The compiler refuses a binding that writes through a surface that declares no serial at all: `device.write_without_serial`. It cannot see which environment a run will use, so the driver holds the run's own environment to the rule.

**The bytes of a write are built and given out by a step before the one that commits.** The mode gains a fifth step kind, `build_frame`. A write lowers to `build_frame` and then `write_frame`, both given the same value.

- `build_frame` commits nothing and sends nothing. It runs every check a write passes, builds the frame, and returns it for reconcile under `frame_<register>`, as hexadecimal, and as a capture named `frame` with the bytes and their SHA-256 digest. The provider contract already carries this: the engine stores what a step before the committing one returns for reconcile, durably, before it calls the committing step.
- `write_frame` runs the checks again and sends the frame that was built for the same register and the same value. A write frame that no step built, or whose value changed since, is refused unsent. While a built frame waits, no other frame is sent on the link.
- The protocol interface inside the pack is split to allow it: `frame` builds a request's bytes, and `send` puts built bytes on the link. The transaction identifier counts in the order frames are built.

**A run sends at most `max_frames_per_run` frames on one link**, a setting of the pack, one hundred by default. A frame past it is refused unsent: `policy`/`frame_budget_exceeded`. A write is not started when the frames left would not cover it and the reads of its read-back, so the ceiling itself never leaves a write unverified.

**Three smaller corrections, with no real alternative.**

- The default gap between frames is 250 ms, four frames a second at most, where it was 50 ms. Section 310 asks for "a few per second". A person who knows the device can take more lowers `min_gap_ms`.
- The driver refuses every frame but identification in the discovery posture, by a check every frame passes: `policy`/`discovery_identifies_only`.
- The driver refuses a step that writes a register with the hazard `physical`: `unknown_variant`/`physical_not_built`. The compiler refused such a binding already; a plan is data, and a driver that would move a physical thing refuses by itself.

## Alternatives weighed

*The serial in the map.* A map is written for a model and a list of firmware versions, and is the same for every unit of the model. A serial there would make one map file for each unit.

*One serial on the surface, for every environment.* The test device and the production device are different units. One serial would be wrong in one of them, or would push people to leave it out.

*Serials by address.* It ties the unit to where it answers. It lost because the serial says which device is meant, whatever its address, and because an address is replaced for each environment and by tests while an environment's name is not.

*Environments carrying a pack's fields.* The kernel's environments hold an address for each surface and nothing else. That would be the better home, and is a kernel change this package could not make.

*Stopping reads too where no serial is declared.* It is stricter. It was left because a read moves nothing, a register whose reading changes it is never read, and the product's own example target reads through a surface that declares none.

*Keeping the frame in the step that sends it, as a capture.* The committing step cannot return anything for reconcile, by the contract, and what a step returns exists only after it ends. A write whose link died would still leave no record of its bytes.

*Reusing `expect` for the step that builds the frame.* No new step kind, and [spec section 201](../spec/2-kernel/201-access-kinds-and-modes.md) would not change. It lost because a trace should say what a step did, and `expect` reads a reply.

*A frame count from the provider contract.* A step is given a deadline and no count, and `prepare` is given no limits of the run. The pack counts by itself until the contract does.

*A ceiling with no reserve for the read-back.* Simpler, and a write could then be the last frame allowed, sent and never read back.

## Consequences

- Sections 310, 201 (the device archetype's steps), and 226 (the pack's settings) were corrected with this record.
- A target that writes to a device must declare `serials` on its surface. The example target of the pack's tests does. Targets that only read are not affected.
- One more step, and no more frames, in every write. Reads are unchanged, and so is their trace signature.
- The ceiling is counted for each link `prepare` opens. A run that prepares a link twice starts a second count.
- The unit address and the protocol are still one for every environment of a surface.

## What would change it

- The provider contract giving a step the run's limits, or a count of what the run has sent. The pack's own ceiling would then be the lesser of the two.
- The kernel's environments carrying a pack's fields for a surface. `serials` would move there, and the unit address with it.
- The engine keeping a step's captures durably before the next step. The frame's capture would then be the record, and the value for reconcile could go.
- A device that reports no serial. The identifier would have to be declared in another form, and the surface's field is where it would go.
