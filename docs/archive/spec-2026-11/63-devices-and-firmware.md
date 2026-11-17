# 63. Devices and firmware

> Archived on 2026-11-17. This is section 63 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 310, The device pack](../../spec/3-packs/310-the-device-pack.md).

The owner asked whether the system can reach all the way down: devices, and perhaps firmware. This section answers plainly what is realistic, how it would be made safe, and what is refused. It is a catalog and a set of rules, not a design of a pack. Decision: [record 0054](../../decisions/0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md).

## The short answer

| Question | Answer |
| --- | --- |
| Can a device be operated through verified tools? | Yes, where the device offers an interface of its own that can be read as well as written: a management protocol, a console with known commands, registers on a fieldbus |
| Can firmware be inspected? | Yes, as discovery. It yields findings and never an operation. An optional outside provider does it |
| Can firmware be updated? | Yes, only through the device's own documented update mechanism, as an `irreversible` operation with the hazard `device_function` |
| Can firmware be changed in other ways, or protections be got around? | No |

## What the REA project shows

REA was studied closely for this ([research note 16](../../research/16-rea-methods.md)). At its lowest levels it is careful and entirely passive.

- It parses firmware containers and images, device trees, kernel modules, saved logic-analyzer recordings, and saved packet captures. All from files.
- It writes nothing to any device. It speaks to no bus and no debug probe. It runs nothing under emulation. Its only changes to a device are six tools for a phone or emulator attached for debugging, each with a fixed argument list.
- Where it does change something, it shows the right habits: take the bytes first, probe before writing, stop on an unclear probe, read back and report the result as its own fact.

So REA is a way to understand a device's software. It is not a way to operate a device, and nothing there can be reused for that. What follows is our own design.

## Ways to reach a device, best first

| Route | Archetype | Read-back means | Worst outcome | Stance |
| --- | --- | --- | --- | --- |
| The device's management protocol | Request | Read the same object after the change is committed | Locking oneself out by changing the network or the credentials in use | Preferred |
| A console with declared commands | Command over a link | A declared query command, with its answer parsed by a strict pattern | A saved setting that stops the device starting; nothing protects the link from noise | Allowed, with a closed list of commands |
| Registers or objects on a fieldbus | Device | Read the register again | Many writes are commands with nothing to read; some move machinery | Allowed for readable registers; the rest is `irreversible` with a hazard |
| The device's own update mechanism | Request or Device | The version and digest the device reports after it restarts | A device that no longer starts, where it has one slot; counters that only go up, even on success | Allowed, under the rules below |
| A debug probe | Not designed | Reading memory back | Permanent lock or erase through protection settings; halting a controller that is driving something | Refused until designed, and then never on a device in service |

## Rules for any device operation

1. **Exclusive use of the link.** One run at a time on one device link, as desktop work is already serialized.
2. **Identify before acting.** The device must report an identity that matches the target's: model and firmware version. A device that does not match is the class `unknown_variant`.
3. **Probe, then write, then read back.** The four habits of [section 61](../../spec/2-kernel/213-effects-hazards-and-consent.md).
4. **No automatic retry of a device write.** A write that timed out is `possible_commit`, and reconcile reads the device.
5. **A hazard on every write** that could affect the device's ability to work or anything physical, with the approvals section 61 requires.
6. **A declared safe state and a stop.** For anything physical, the target declares the state that is safe and the operation that reaches it. The kill switch reaches the device link.
7. **Rate limits from the device's own documentation**, or a conservative default.
8. **Never the link we are standing on.** An operation that changes the address, the credentials, or the interface it is itself using is refused unless the target declares how the session is found again afterwards.

## Updating firmware

An update is one operation with one commit point, and the strictest handling in the design.

| Stage | What happens |
| --- | --- |
| Precondition | The device's identity, its current version, and what the device's documentation requires before an update: power, free space, a second slot |
| The image | Taken as a file input, stored as a blob, its digest recorded. It must be an image the device's maker issued for that model; the runtime does not build, patch, or re-sign images |
| Approval | A person, every time, shown the current and the proposed version, the image's digest, and what cannot be undone |
| Act | The device's documented mechanism, and nothing else |
| Confirm | The device accepts the image |
| Read back | After the restart: the version and digest the device reports |
| If it does not come back | The class `possible_commit`. The run stops, a person is alerted, and the recovery route the target declared is shown. Nothing is retried |

Anything permanent that a successful update causes, such as a counter that prevents going back, is declared in the operation and shown at approval.

## Test stands

A device pack clears the bar of [section 55](../../spec/2-kernel/202-kernel-and-packs.md) with an emulated device: a small program that speaks the protocol, holds registers and settings, and reports through a control interface what it holds and how many times each was written.

- Against an emulated device, read-back is exact and a snapshot undoes anything.
- Passing there shows the pack is right. It does not show that a particular real device behaves the same.
- A real device model enters a target as a test environment first, with test settings that are safe to write, before any production use. The gates run there.

## Firmware as something to understand

Inspecting a firmware image is discovery ([section 58](../../spec/2-kernel/218-discovery-and-findings.md)).

- It is done by an optional provider, off by default.
- The subject of every finding is the image file, by digest.
- What it finds is `inferred`: a setting named in an image is a claim about a device, not an observation of one.
- It may list the regions, files, and configuration an image holds. It decrypts nothing and recovers no keys.

## Refused

| Refused | Why |
| --- | --- |
| Getting past secure start-up, signature checks, or protection against reading a device | Rule five |
| Unlocking, rooting, or exploiting a device | Rule five |
| Building, patching, or re-signing a firmware image | It is not an operation on an existing system; it makes a new one |
| Writing through a debug probe | Not designed; the worst outcome is permanent |
| Changing settings that only go one way, such as fuses, unless the device's maker documents the operation and the target declares it | It can never be read back into the previous state |
| Any device operation on a real device without a test environment first | Rule ten, applied to hardware |

## Open points

- Which protocol a first device pack would support. A management protocol over a network is the safest start.
- Whether emulators for whole devices, beyond a protocol-level test stand, are worth the weight.
- How a person is alerted, and how fast, when a device does not come back.
- Whether a second person should be required, not optional, for the `physical` hazard.
