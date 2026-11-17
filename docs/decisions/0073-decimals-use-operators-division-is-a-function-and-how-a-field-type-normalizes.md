# 0073. Decimals use operators, division is a function, and how a field type normalizes

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of `a2m-expr` on whether decimals use operators or named functions, and the choices made while building its steps 2 to 6. Follows [decision 0072](0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md).

## Context

With its own evaluator the crate is free to give a decimal the operators. What was left to choose was how a decimal is written, what division means, how a field type's rule reaches the plan, and several small things the first build met.

## Decision

**Decimals**

- A decimal literal is `decimal("1.5")`. A bare `1.5` is a compile error whose fix says so. *Instead of:* reading `1.5` as a decimal, which gives CEL text a second meaning that a reader who knows CEL would get wrong.
- `+`, `-`, `*`, unary minus, and the six comparisons work on two decimals. A whole number and a decimal never mix: the fix is `decimal(n)`. *Instead of:* the named functions `add`, `sub`, `mul`, and `cmp` of the first design, which are removed, so there is one way to write each.
- `/` on decimals does not compile. Division is `div(a, b, places, rounding)`, because there is no division without both. `/` and `%` stay for whole numbers.
- `decimal(text)` takes text only when it is a literal of the expression. Text from a value becomes a decimal only through `parse_decimal(text, places, rounding)`.
- Arithmetic that does not fit stops with `value_out_of_range`. Nothing is rounded to make it fit.

**Rules are literals**

- An argument that is a rule (places, a rounding mode, a unit, a format, a zone, a decimal mark, a code list, a currency) is written as a literal and checked when the expression is compiled. *Instead of:* checking it on every run. A rule comes from a field type, never from a value.

**Normalizers**

- A field type's normalizer is one call, `as_<base>(value, rules...)`, over the one name `value`. `a2m-files` builds it with `FieldRule::normalizer()` and stores its `PlanExpr`; the engine loads it with `Normalizer::load`.
- A normalizer reads a value either typed, as a caller sent it, or as text, as a system showed it, and gives the same typed value for both. An absent value stays absent. Normalizing twice changes nothing.
- A measurement's conversion and its step are one rounding, on the exact quotient. *Instead of:* converting to some number of places and then snapping, which can land on the wrong step.
- Display grouping is stripped only when well formed: one separator, groups of three. `1,23` stops. Money accepts the field's currency code or one currency symbol; another code stops. A whole number given for money stops, because it could be units or the smallest unit.
- A time of day is a `Duration` since midnight. *Instead of:* a new value type in `a2m-core` for one field-type base.

**Dates and zones**

- A format is a closed pattern language: `YYYY`, `MM`, `M`, `MMM`, `DD`, `D`, `HH`, `H`, `hh`, `h`, `mm`, `ss`, `SSS`, `A`, and literals, with other letters between single quotes. An unknown letter is a compile error.
- A zone is `UTC` or a fixed offset such as `+02:00`. **Named zones are not built.** They need a zone database, which is an outside crate and a decision of its own; until then a field type with a named zone does not compile.

**The evaluator**

- It works directly in `a2m-core`'s `Value` and `ValueType`. There is no interpreter value to map.
- Types are checked when compiled and kinds again when run. `Program::load` has the text and the result type but not the names' types, so it reads every name as unknown, and evaluation refuses a value of the wrong kind and an answer that is not of the stated type.
- `null` is absence. It may be compared with `==` and `!=`, given a default with `coalesce`, and returned. A field of an absent record is absent. Anything else that needs an absent value stops with the new code `value_absent`.
- `coalesce`, `&&`, `||`, and the conditional do not evaluate the side they do not need.
- The cost limit is 1,000,000 units for one evaluation: one per node visited, one per item passed over, one per 64 bytes of text a function handles. It is not yet measured on a pipeline.

**The outside crate**

- `unicode-normalization` 0.1.26, MIT or Apache-2.0 with the Unicode-3.0 data licence, no default features, for form C. Its tree is one more crate, `tinyvec`. *Instead of:* `icu_normalizer`, already in the lock file through the HTTP client, which would put some twenty crates under a layer-2 crate; and instead of tables of our own, which are the Unicode Character Database and not ours to maintain. The crate's version fixes the Unicode version, so raising it changes answers for newly assigned characters and is a change of library version.

## Consequences

- The file formats' section on expressions says what the language is, how a decimal is written, and lists the functions as built.
- The library is version 1, with 36 functions. Removing or changing one is a new version.
- `a2m-files` must turn `types.yaml` into a `FieldRule`. A field's `step`, `format`, `case`, and `whitespace` live in the normalizer's text, not in `FieldTypePlan`.
- The built-in field types of the file formats (`length_ft_in`, `percent`, and the rest) are `a2m-files`' to define as rules; this crate knows bases only.

## Revisit if

- Authors find `decimal("1.5")` too heavy in practice. A decimal literal with a suffix, which CEL does not have, would then be weighed.
- A target needs a named zone. That slice chooses the zone database.
- Measurement shows the cost limit is wrong by more than a factor of ten either way on the test application's largest pipeline.
- A system shows numbers grouped other than in threes.
