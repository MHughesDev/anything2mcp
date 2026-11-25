# 0444. A result's decimal is padded to the places its field type states

**Status:** Accepted. Design decision, made by the worker on field test 8, finding 53, under the lead's brief. The owner had not read this record when it was written. No public interface changes: `Decimal` gains two methods and `CompiledFieldType` one, and the compiled target's format is unchanged.

**Date:** 2026-11-25

**Changes:** `a2m-core` (`src/decimal.rs`, `src/value.rs`, `src/compiled_target.rs`); `a2m-engine` (`src/tool.rs`, `src/loaded.rs`); `a2m-files` (`src/types.rs`, the built-in type's name only); [spec section 210](../spec/2-kernel/210-sources-and-value-normalization.md); the [technical reference](../reference/technical-reference.md), the [file formats reference](../reference/file-formats.md), and [steps, snapshots, and diffs](../reference/steps-snapshots-and-diffs.md); the designs of `a2m-core`, `a2m-engine`, `a2m-actions`, and the product crate.

## Context

A result carried a decimal in its exact form, with no zeros after its last digit, whatever places its field type stated: a speed read as `120.0` was `"120"` beside a temperature of `"21.5"`, though the target's own type of the speed said one place (finding 53, found while finding 51 was fixed). A caller reading two outputs of one precision saw two. The places a type states never reached the caller.

The exact form is right for what compares and hashes values: rule 3 compares a written value with its read-back after normalization, and the canonical form, record keys, digests of normalized values, and diff hashes are all built on it. The question is only what a result shows.

## Decision

1. **A result's decimal is padded to the places its field type states, and never rounded there.** Where an output's field type is a `decimal` or a `money` type, the engine writes the normalized number with exactly the type's places: one place gives `120.0`, two give `21.50`, and money its stated places or its currency's own. The built-in `decimal` pads nothing, because its six places only bound what a value may hold and are not a precision anyone stated; a measurement pads nothing, because its places follow its step and no type states them; no other base has places. A list output pads each item. The padded value is the same number: equal to the exact one, hashed alike, and written in a result, in the store, and in what the store reads back as `"120.0"`.
2. **A value with more places than its type is rounded by the type's own rule, as before, and the padding never rounds.** The field type's normalizer already rounds to the type's places by its stated rounding mode, or half to even when none is stated, as it does for every value it compares ([spec section 210](../spec/2-kernel/210-sources-and-value-normalization.md)): a system that shows `21.5625` for a type of two places gives `21.56`. That rounding is a written rule, so it is no guess under rule 7, and a field type that wants no rounding states more places. The padding comes after it and only adds zeros. A value that reaches the padding with more places than the type states is one no rule gave, and stops the read as `unknown_variant`, `output_unexpected`, by the output's name and never by its value.
3. **The comparison of a read-back is unchanged.** Equality and hashing were already by number, whatever zeros a decimal holds; the canonical form, which digests and record keys are built from, now writes a decimal's exact text explicitly, so a padded value and an exact one give the same bytes. A record key's text is the exact text too. Only what a result shows changes.

How it is carried: `Decimal` holds the places it was made with. Every arithmetic result and every parse is exact, as before; `Decimal::with_places` pads and refuses to round; `Decimal::exact_text` is the exact form; a decimal is written with the places it holds; and a stored form reads back with the places it was written with, so a result read from the store shows what was stored. `CompiledFieldType::result_places` says which places a result is padded to, from the base, the places, and the built-in `decimal`'s name, as the engine already knows the built-in `url` by its name. The caller's faces, which write a decimal as its text, needed no change.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Pad in the result's faces in `a2m-actions`, from the output's field type | The faces see values and no types; each face would need the compiled target to show a value, and a result read back from the store would lose its places on the way |
| Pad in the field type's normalizer | The normalized value is what is sent to a system on a write and compared everywhere; padding there changes what every pack sends and every fixture a person wrote, for a change that is only what a caller is shown |
| Stop a value with more places than its type as `unknown_variant` | The type states how it rounds, so the value has a rule, and the comparison of a read-back would change against the brief; a person who wants a stop states more places |
| Add a `result_places` field to the compiled field type | The places and the base already say it but for the built-in `decimal`; a new field changes the compiled target's bytes and hash for every target with a money or decimal type |
| Pad the built-in `decimal` to six places | `120.000000` states a precision nobody gave |

## Proof

`a_results_decimal_is_padded_to_the_places_its_field_type_states`, in `crates/a2m-engine/tests/write.rs`: a verified write's read-back of 120 inches under a type of one place gives `120.0`, and of 21.5 under a type of two gives `21.50`, as the engine hands the result on and as its stored form reads back; it failed before the change, with `"120"`. `a_results_decimal_with_more_places_than_its_type_is_rounded_by_the_type_alone`, in the same file: 21.5625 under a type of two places gives `21.56`, and the write is verified by its input's own type. `a_decimal_padded_to_its_places_is_the_same_number_written_with_them` and `a_decimal_with_more_places_than_asked_is_never_rounded_to_them`, in `crates/a2m-core/src/decimal.rs`: a padded decimal equals and hashes as the exact one, keeps its places through its stored form, and is never rounded to fewer.

## What would reopen it

A caller or a client that reads a decimal's text as a precision and is misled by money padded to its currency's places, or a measurement whose places a person wants shown, which would need a measurement type to state places of its own.
