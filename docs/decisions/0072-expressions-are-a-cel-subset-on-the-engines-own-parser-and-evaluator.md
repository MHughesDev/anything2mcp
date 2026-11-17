# 0072. Expressions are a CEL subset, on the engine's own parser, checker, and evaluator

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point "Which CEL crate" in the design of `a2m-expr`, and step 1 of its build order. Refines [decision 0014](0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md): the syntax in target files stays CEL; what reads it is ours.

## Context

The design said expressions are CEL and that an interpreter crate would be verified before use. It needs six things from whatever the language stands on: no float can be produced or compared; a decimal is a first-class type; an expression is type-checked once, with a place for every error; evaluation is the same on every machine; evaluation is bounded by a cost limit; and the plan stores text that reads back to the same program.

The candidates on crates.io were checked on 2026-11-17.

| Crate | Version, date, licence | What was found |
| --- | --- | --- |
| `cel` | 0.16.0, 2026-11-14, MIT | The maintained interpreter; its earlier name `cel-interpreter` stopped at 0.10.0 in 2025. Three minor versions in three months, each changing the interface. It has **no type checker**: types are found at run time. Its value type has a `Float(f64)` member that cannot be switched off, and its comparisons treat `1 == 1.0` as true. Maps are hash maps, so their order differs between runs. Its bound is an iteration budget on comprehensions, not a cost over all work. An opaque type can carry the arithmetic operators. Parsed nodes carry byte offsets. The tree it brings: a parser runtime (BSD-3-Clause), `nom`, `thiserror`, and with default features `chrono` with its clock, `regex`, and `base64` |
| `rscel` | 1.0.9, 2026-11-15, MIT | No repository is listed. Brings `chrono`, `regex`, a units crate, and `serde_with`. Floats are part of its value type |
| `cel-cxx` | 0.2.5, 2026-07-01, Apache-2.0 | A binding to the C++ implementation. It would put a C++ build on the path of every install on three operating systems |
| `common-expression-language` | 0.1.0, 2020 | Abandoned |

No candidate checks types before a run, and none can refuse a float when an expression is checked. With any of them this crate would still write the type checker, walk the outside crate's tree to refuse floats, wrap a decimal as an opaque value, sort every map, and add its own cost count, and it would then follow an interface that changes every month.

## Decision

`a2m-expr` has its own parser, type checker, and evaluator for the part of CEL the file formats use. It depends on no interpreter crate.

- **The syntax is a subset of CEL**, with CEL's operators, precedence, literals, field access, indexing, calls, the conditional, list literals, record literals with text keys, and the macros `has`, `all`, `exists`, `exists_one`, `map`, and `filter`. An expression that this crate accepts reads the same way to anyone who knows CEL.
- **What differs from CEL is refused, not reinterpreted.** A number with a decimal point or an exponent, an unsigned number, bytes, and raw text are compile errors with a place and a fix. There is no double, no unsigned integer, no bytes, no dynamic message type, no regular expression, and no timestamp read from a clock.
- **The types are the kernel's.** The checker works in `a2m-core`'s `ValueType` and the evaluator in its `Value`, so there is no interpreter value to map to and from, and a decimal is a type like any other.
- **The cost limit counts every node visited and every host-function call**, with more for work over long text. It is one number that belongs to the library version.
- **A tree is bounded** in source length (16 KiB) and in height (64), so that walking one never needs a deep stack.
- **Canonical text** is the tree printed back in one spelling. The plan stores that text, and reading it gives the same tree.

*Instead of:* the `cel` crate with a checker of our own on top. That keeps the grammar someone else maintains, and costs a float in the value type that only our checker keeps out, an order of map entries we would have to repair, a second cost count, and a tree of outside crates in a layer-2 crate that every other crate builds on. Rule 2 puts safe and correct before easy: here the easy path leaves the float one missed check away from a business value.

## Consequences

- The grammar is ours to keep correct. It is small: about a thousand lines for the lexer, parser, and printer, with tests for every refusal.
- A CEL feature outside the subset is a compile error until it is added here, with tests. That is the same rule as for host functions.
- An expression written for this engine runs in a standard CEL implementation only when it uses no decimal and no host function. Nothing in the design needs that.
- The sentence "Expressions are CEL" in the file formats now says which part, and what is refused.
- The one outside crate this work adds to the tree is for Unicode normalization, recorded in [decision 0073](0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md).

## Revisit if

- A maintained CEL crate gains a type checker that can be told there is no double, a deterministic map, and a cost limit over all work, and has kept its interface for a year.
- Target files turn out to need a part of CEL that is costly to write correctly here, such as regular expressions or protocol-buffer messages.
- A second consumer needs to run the same expressions outside this engine.
