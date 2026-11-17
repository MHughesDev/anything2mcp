# 0054. Devices are reached through their own interfaces, and firmware only through the device's own update

**Status:** Accepted by the agent, under the owner's instruction of 2026-11-17 to design the whole system. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

The owner wants the system to reach from the bottom of the stack upward, firmware included if that is possible. The study of REA showed a careful project that inspects firmware and device artifacts from files and writes nothing to any device ([research note 16](../research/16-rea-methods.md)). Operating a device is therefore ours to design, and the worst outcomes at this level are permanent.

## Decision

- A device is operated only through an interface it offers that can be read as well as written: a management protocol, a console with a closed list of commands, readable registers on a fieldbus.
- Firmware is inspected as discovery, by an optional provider, yielding findings at `inferred`.
- Firmware is updated only through the device's own documented update mechanism, with an image its maker issued, as an `irreversible` operation with the hazard `device_function`, approved by a person every time, and read back after restart.
- Refused: getting past secure start-up, signature checks, or read-out protection; unlocking or exploiting; building, patching, or re-signing images; writing through a debug probe.
- Every device pack is proven first against an emulated device with a control interface, and a real device model enters a target as a test environment before any production use.
- No device pack is built until the command pack has tested the kernel boundary and a section designs it.

## Consequences

- "Anything" includes devices that offer an interface, and does not include forcing one that does not.
- The hazards `device_function` and `physical` exist from the start, so that policy and the review summary are ready before the first device pack.
- Debug probes stay out until someone designs them with a real need in hand.

## Alternatives considered

- Offer debug-probe writes now: reading memory back is exact, but protection settings can lock or erase a device for good, and halting a controller is itself an effect.
- Leave devices out entirely: it would contradict the owner's direction, and devices with a proper management protocol fit the engine as well as any service.
- Treat firmware images as writable artifacts: that makes a new system instead of operating an existing one.

## Revisit if

A real device of interest can be reached in no allowed way, or a maker documents a probe-based procedure as its supported update route.

## Spec sections

- [63-devices-and-firmware.md](../spec/63-devices-and-firmware.md)
- [61-effects-hazards-and-consent.md](../spec/61-effects-hazards-and-consent.md)
