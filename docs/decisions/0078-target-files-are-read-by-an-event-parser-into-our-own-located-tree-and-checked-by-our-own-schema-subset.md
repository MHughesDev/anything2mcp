# 0078. Target files are read by an event parser into our own located tree, and checked by our own schema subset

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

`a2m-files` must report every problem with a file, a line, a column, a code, and a fix. Its design left two outside crates to be verified: a YAML parser that keeps the place of every value, and a JSON Schema validator.

## Decision

- **YAML is parsed by `saphyr-parser` 0.3.4**, used only as a stream of events that each carry a line and a column. Licence MIT or Apache-2.0. Its tree is `arraydeque` and `thiserror`, both MIT or Apache-2.0, and no C code; it passed the licence, ban, and source checks, and the crate lints clean for the Windows and macOS targets.
- **The tree is our own.** `a2m-files` builds `Located<Doc>` from the events, so every value and every key has a place. The typing of plain scalars is ours and has no float: `null`, `true`, `false`, and whole numbers are typed; a number with a decimal point stays text and is read exactly where a decimal is wanted. A key given twice is an error at the second key. Anchors, aliases, and tags are refused: target files hold no indirection.
- **Schemas are checked by our own code.** `JsonSchema` in `a2m-files` is a small closed subset of JSON Schema: objects that list their keys and refuse others, maps, lists, whole numbers with bounds, text, names, closed word lists, exact numbers, expressions, a choice between shapes, and references to a control, a field type, or a surface, which the kernel resolves. It is checked against the located tree, so each finding has a place, and it is published as ordinary JSON Schema, draft 2020-12, through `to_json`, with `additionalProperties: false` on every object.
- **Line endings do not reach the plan.** A file's text is read with every line ending a line feed and no byte-order mark, and its digest is taken of that text. A fixture's recorded files are digested byte for byte.

## Alternatives considered

- **`jsonschema` 0.61.2 as the validator.** Rejected on evidence: it declares 26 dependencies, some of them optional, among them a regular-expression engine, a reference resolver, and an HTTP client behind its default features; it validates a JSON value and reports JSON pointers, so every finding would need mapping back to a line; and its messages are not ours to word, where each of ours must carry a fix.
- **`serde` types with `deny_unknown_fields`.** One Rust type per file kind checks the kernel's kinds, but a pack's schema arrives as data at run time, and a plug-in's across a process boundary; and `serde` reports the first problem only.
- **`yaml-rust2` 0.13.0.** Same family and licence; it pulls an encoding crate by default, and `saphyr-parser` is its maintained successor.
- **`serde_yaml`.** Unmaintained, and gives no place for a value that parsed.
- **A YAML parser of our own.** The grammar of block and flow styles, folded text, and quoting is large and easy to get subtly wrong; the event parser is small and has no C code.

## Consequences

- A pack states its schemas through `JsonSchema`'s constructors and cannot state what the subset does not have, such as a pattern. A check beyond the subset is a lint rule.
- The published schemas carry two extension keys, `x-a2m-refers-to` and `x-a2m-expression`, which an ordinary validator ignores.
- The Unicode tables and YAML conformance are the parser's; a new version of it is a dependency change, checked by the tests that pin places.

## Revisit if

A pack needs a schema feature the subset lacks often enough that lint rules become the schema, or the parser stops being maintained.

## Spec sections

- [205. Target files](../spec/2-kernel/205-target-files.md)
- [206. The compiler and the plan](../spec/2-kernel/206-the-compiler-and-the-plan.md)
- [`a2m-files`](../architecture/crates/a2m-files.md)
