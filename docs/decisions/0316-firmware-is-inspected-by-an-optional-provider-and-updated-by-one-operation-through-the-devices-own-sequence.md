# 0316. Firmware is inspected by an optional provider and updated by one operation through the device's own sequence

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Builds what [decision 0054](0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md) decided and [section 310](../spec/3-packs/310-the-device-pack.md) designed, and settles the points the build had to settle.

**Date:** 2026-11-23

**Changes:** the device pack (`a2m-pack-device`), its manifest and a second manifest beside it, and section 310's Firmware part, which now says what is built. Nothing outside the device pack changed.

## Context

Package K4 builds the last part of the device pack: firmware inspection as discovery, through an optional provider, and the update operation, through the simulated device's own mechanism. Rule 5 holds throughout: nothing gets past a device's protection. Rule 3 holds: an update is `irreversible`, a person approves every run, and it is never sent twice. The design left five points to the build.

## Decision

1. **Inspection is a second provider in the device pack's crate, off by default.** The provider `device_firmware` has its own manifest, one discoverer `firmware_image`, and one kind of finding of the same name. `add_to` does not register it. A product that offers it calls `add_firmware_inspection_to` after a person switched it on, so that a registry without it has no such discoverer. It reads one file handed under the name `firmware_image` through the checked paths, sends no frame, and holds no network effect.
2. **What it finds is `inferred`, from the source `artifact`, about the image by its digest.** The finding lists the image's form, version, regions (head, payload, tail, each with its offset and length), whether its checksum holds, and the settings its payload states as `key=value` lines. A setting whose key is a secret's, by the compiler's own test or by a wider one (a key that holds `token`, `credential`, `pass`, `key`, `secret`, or `auth`), has its value left out of the finding, so that a secret is never in one. An image of a form the provider does not read makes no finding and is counted as dropped. An image of another form that looks encrypted (at least 256 bytes with at least 7.2 bits of entropy a byte) is a named stop: `edge_blocked`, `image_protected`. The entropy test is a heuristic that names a stop; nothing is decrypted or looked for either way, and the image is never changed.
3. **The update is one `update` binding with a `firmware` field, and its device's mechanism is declared in the map.** A map that has a device with an update declares `firmware_update`: the mechanism `register_blocks`, the three holding offsets the sequence writes (the command, the image length, and the block), the largest block and image, the three registers it reads (the update's state, its error, and the device's checksum), and the window in which the device must come back. The three offsets the update writes are named by no register, so no other operation can write them. The binding declares the image and the version it carries as expressions, and, before anything is published: the recovery route, what cannot be undone, whether the device checks the maker's signature, optional preconditions (a register and the number it must hold), and optionally the digests it accepts. The compiler refuses an update with no recovery route (`device.firmware_without_recovery`), an operation that is not `irreversible` with the hazard `device_function` (`device.firmware_not_irreversible`), a map with no mechanism (`device.firmware_update_not_declared`), and a digest that is not 64 hexadecimal characters.
4. **One commit point, and what is recorded before it.** The steps are: identify; each declared precondition and the update's own state read; the device's checksum read, for reconcile; `build_frame`, which checks the image and gives out its digest, its length, its blocks, the device's version before, and what the operation declares; one `write_frame` step that commits and carries the device's sequence (the length, update mode, each block in order, verify, and a read of the state, which must say verified); and the commit command, whose reply the confirm steps read. A refusal after update mode began asks the device to leave it and ends the run as `rejected`, `image_refused`: the device's other slot gave up its image when update mode began, and the message says so. A frame whose fate is unknown ends the run as `possible_commit`, `no_confirmation`, drops the link, and sends nothing more. The read-back, after the commit, waits for the device to come back and identify itself within the map's window on a link of its own, compares its model and serial with the map and the surface, gives out the version it reports, and compares its own checksum with the checksum the image carries. A device that does not come back is `possible_commit`, `device_not_returned`; nothing is retried. A checksum that differs is `mismatch`, `image_checksum_differs`. The version the device reports is not compared with the map's list of versions: the update is for a new version, and the next run stops until the map names it.
5. **The image arrives as hexadecimal text, and its bytes are not what is recorded.** The engine has no file or blob input, so the operation takes the image as a text input. What the intent records is the image's digest and the plan of its blocks, because the frames of the sequence carry transaction numbers that depend on the order they are built in, so their bytes cannot be given out whole first. The pack never builds, patches, or signs an image, and never checks a signature in the device's place; the device checks the image's form, its checksum, and its model, and the pack checks the form, the checksum the image carries, the version the operation names, the largest size, and the list of digests.
6. **Six failure codes.** `image_not_accepted` (input), `update_precondition_unmet` (policy), `image_refused` (rejected), `image_checksum_differs` (mismatch), `device_not_returned` (possible commit), and, in the optional provider, `image_protected` (edge blocked). `device_protected` is still not raised.

## What this inferred, for the owner to correct

That "the device's own documented update mechanism" can be a mechanism the map declares and the pack carries by name, with one name today, `register_blocks`; that a second mechanism is a second name. That the preconditions of an update are the operation's to declare, as a register and the number it must hold, with the update's state always one of them. That inspection should be a separate provider and not a setting of the pack, because the design says "an optional provider" and a registry then shows whether it is on.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A new verb `firmware_update` | A verb is a registered name in every pack's mode; the design lists three, and an update with a read-back is an `update` |
| Each command of the sequence as a step of the plan, with several steps that commit | The ledger holds one commit point for one record; a sequence of writes that each commit would be several writes, each approved and each possibly sent |
| Declare the command, length, and block as registers of the map | Any `update` or `submit` could then write them, and an agent could start an update through a write the compiler took for a plain register |
| Take the update's registers from the operation | Knowledge about one device lives in its map (rule 8) |
| Inspection as a setting of the device pack | The registry would list the discoverer whether or not it is on, and the manifest would declare what the pack does not register |
| Compare the version the device reports after the update with the map's list | The update is for a version the map was not proven against; the stop belongs to the next run |

## Proof

In `a2m-pack-device`, against the simulated device: `an_update_goes_through_the_devices_own_sequence_and_is_read_back_after_the_restart` (the stand shows the length, update mode, three blocks, verify, and one commit, an update count that rose once, and the version and checksum read back); `a_link_lost_at_the_commit_is_a_possible_commit_and_nothing_is_sent_again`; `a_device_that_does_not_come_back_is_a_possible_commit_and_nothing_is_retried`; `a_device_that_refuses_the_image_is_told_to_leave_update_mode_and_the_run_ends_refused`; `an_image_that_will_not_be_sent_sends_no_write_frame`; `an_update_without_a_declared_recovery_route_does_not_compile` (the roadmap's proof); `an_image_given_as_a_file_is_read_and_described_as_inferred_and_nothing_is_sent` and `an_image_of_another_form_or_under_another_name_is_not_read_and_a_protected_one_stops`.

## Not built

- What a person sees to approve an update: the engine's approval does not yet show the current and the proposed version, the digest, whether the device checks a signature, or what cannot be undone. The pack records all of them in the capture `firmware`, and the policy engine does not read it yet.
- A file input stored as a blob, so that an image need not be hexadecimal text.
- Inspection of an image the device offers through its own interface, a signature check, a second mechanism, an update through the request pack, and `device_protected`.
- The product crate's call to `add_firmware_inspection_to`, and the switch a person sets to turn the provider on.

## Revisit if

A real device's update does not fit one length, one command register, and blocks to one address; the engine gains a file input; or the optional provider is wanted as a plug-in.
