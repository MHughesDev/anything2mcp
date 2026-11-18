# 0106. The device pack speaks its first protocol itself, and checks a range before a frame is built

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of [spec section 310](../spec/3-packs/310-the-device-pack.md) on which protocol comes first, for the pack as well as for its test stand, and the item of its "What it needs from outside" on a client crate. The crate's design is [`a2m-pack-device`](../architecture/crates/a2m-pack-device.md); the rest of what building it settled is in [decision 0107](0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md).

## Context

A device is where a write can move something in the world. Section 310 asks two things of the first link: that every frame sent is one the pack can account for, and that a value outside a register's safe range never leaves. It left the protocol open, said the test stand's frames had proved small enough to write by hand, and said a client crate for the protocol "may not be needed".

The stand, [the simulated device](../../targets/example-device/README.md), speaks a subset of the Modbus application protocol over TCP: the four tables a register map already uses, framed over a network link, with no serial hardware. Its frames are written in the stand with no protocol library.

## Decision

**The first protocol is a subset of the Modbus application protocol over TCP, written in the pack.** It is one module of the crate, with no outside protocol library and no code shared with the stand. It carries functions 1 to 6 and 16, and 43 with type 14 to read the device's identification, with the protocol's exception replies. One request is answered before the next is sent. The transaction identifier counts from one on every link, so one run sends the same bytes each time. The pack's only outside dependencies are `serde`, `serde_json`, and Tokio with its `io-util` feature, all already in the workspace.

**A protocol is one interface inside the pack.** The driver knows registers, ranges, and failure classes. A protocol knows frames: it turns a request (identify, read bits, read words, write one bit, write words) into one frame, reads one reply, and says what the reply means. Another protocol, or the same one over a serial port, is another implementation and another word in the surface's `protocol` list. Today that list holds `modbus_tcp` alone.

**A value is checked against its register's range twice, in the driver, before a frame is built.** First as the value in the register's unit, against the map's `range`. Then as the raw number it encodes to, against the same range converted by the register's scale, and against what the register's bits can hold. A value that is not a whole number of the register's steps is refused, never rounded. A register that holds a number and has no range is refused here too, even though the compiler already refuses such a map. Each refusal is `input`/`value_out_of_range` or `input`/`schema_invalid`, marked as not sent, and the stand's frame log shows no frame.

## Alternatives weighed

*An outside client crate for the protocol.* Section 310 listed one as a dependency to verify. It lost because the pack has to say of every failure whether a frame left, and must refuse to send anything a map does not name. That needs the pack to own the moment a frame is written and the reading of every reply. The subset, with its tests, is under five hundred lines. A library brings functions and framings the pack must then keep from being used, and a tree to keep under the licence and advisory checks. No candidate crate was evaluated against the closed licence list, because the pack did not need one.

*The range checked by the kernel's input validation alone, before the preview,* as section 310 says. It lost on two counts. The range lives in the map, beside the register's address and layout, and the kernel's field types carry no range, so the kernel's check cannot see it. And a check on the value in its unit alone cannot catch a raw number that a scale rounds or that overflows the register's bits; the second check exists for exactly that case.

*Rounding a value to the register's step.* It lost to "nothing is rounded on the way to a device": a person asked for one value, and a different one would be written.

## Consequences

- Section 310 is wrong in two places, which the lead owes: its "Range is enforced before a frame is sent, twice" bullet says the first check is the kernel's, before the preview; and its "What it needs from outside" still reads as if a client crate is to come.
- The protocol subset is this project's to keep correct. The stand and the pack share no code, so a mistake in one is caught by the other, but both were written against the same reading of the protocol, and no real device has run against the pack.
- A serial link, a console on a link, or a second fieldbus protocol is a new implementation of the one interface. None changes the driver's range checks.

## What would change it

- A second protocol whose framing or state is large enough that a maintained library under a licence on the closed list is the smaller risk. The interface inside the pack is where it would go.
- The kernel gaining a range on field types, or a per-pack input check that reads the map before the preview. Then the first check could move there; the second stays in the driver.
- A real device, tried with the owner, that answers in a form the subset does not read.
