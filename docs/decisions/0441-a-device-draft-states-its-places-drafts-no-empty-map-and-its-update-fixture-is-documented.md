# 0441. A device draft states its places, drafts no empty map, and its update's fixture is documented

**Status:** Accepted. Design decision, made by the worker on field test 8, findings 50, 51, and 52, under the lead's brief. The owner had not read this record when it was written. No public interface changes.

**Date:** 2026-11-24

**Changes:** `a2m-pack-device` (`src/discover.rs`, `src/published.rs`); the design of `a2m-pack-device`; [spec section 310](../spec/3-packs/310-the-device-pack.md); the [file formats reference](../reference/file-formats.md); a test project of the product crate, `tests/projects/controlwrite`.

## Context

Field test 8 drafted a pump controller's reads from its identification and from a published register map, and wrote an update by hand. Three things cost the person time. The identification drafted a map of its own, `registers/{model}.yaml`, with `registers: {}` and no surface, beside the published map of the same model (finding 52). A register the map scales by `0.1` was drafted as `decimal`, the built-in type, so a speed and a temperature of one map said nothing alike of their precision (finding 51). And the offline test of the update needed a fixture of twelve answers in an order no document gave; it was found by twelve test runs (finding 50).

## Decision

1. **The identification drafts no map.** The first protocol lists no register, so a map from identification has none: nothing to try, and a second map beside the one the documentation gives. The rule drafts nothing and says why in the drafter's report: put the device's published register map in the project's `sources` folder and run the scout again. The identity stays in its finding, and the describer shows it.
2. **A scaled register's read states its places, from its scale.** The places are those of the scale as an exact number: `0.1` one, `0.01` two, a whole scale whole numbers. They are said in the read's description, beside the unit the map states, because a target's field type is not the drafter's to write: a drafting rule writes only its pack's files, operations, and sources, and a drafted `types.yaml` would stand against the person's own.
3. **The order of a device's fixture answers is in the file formats reference,** beside the file and data packs': every step refers to the map, so every answer is in `<device_map>.json`; an update of one register takes twelve, of which only the last, the read-back's `expect`, holds a value, under the output's name; the committing `write_frame` carries the value under the register's name, which `<device_map>.request.json` is compared with.

## What this leaves open

A result carries a decimal in its exact form, with no zeros after its last digit, whatever places its field type has: a speed of `120.0` is `"120"` even in a target whose type says one place. That is the kernel's, for every pack, and is a new finding (53 in [the field tests' findings](../reviews/field-tests.md)).

## Alternatives

| Alternative | Why not |
| --- | --- |
| Draft a field type with places into `types.yaml` | A drafting rule may write no kernel file but operations and sources, two maps would write one file, and a person's own `types.yaml` would make the drafter keep every draft of the finding back as a conflict |
| Let an output state `places` beside its type | A second place for what a field type says, against rule 8 |
| Draft the identification's map only when no published map names the model | A rule sees one finding at a time, and the map with no register is still nothing to try |
| Make the order unnecessary: answer a device's steps by kind | The replay is the kernel's contract for every pack ([decision 0177](0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md)), and the order is the steps' own |

## Proof

`discovery_identifies_the_device_and_changes_nothing`, in `crates/a2m-pack-device/tests/discovery.rs`: the identification drafts nothing, the report names the finding and the reason, and quotes none of the device's words. `a_published_map_drafts_reads_that_read_the_stand_and_send_no_frame_to_draft`, in the same file: the reads of registers scaled by `0.1` and `0.01` state one and two places, and a raw number states none. `a_devices_update_is_tested_offline_from_twelve_answers_in_order`, in `crates/anything2mcp/tests/other_packs.rs`: the update's offline test passes from twelve answers in the documented order, and fails with `fixture_no_answer` from eleven.
