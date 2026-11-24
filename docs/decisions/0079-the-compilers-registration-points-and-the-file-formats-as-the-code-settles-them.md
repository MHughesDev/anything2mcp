# 0079. The compiler's registration points, and the file formats, as the code settles them

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point "the read-only views a lowering sees" in the design of `a2m-files`, and several fields the file format reference left open. Changes the interface sketch of that design, which is a public contract.

## Context

Building `a2m-files` against the finished plan types in `a2m-core` showed where the sketch of the registration traits was short, and where the file formats did not yet say enough to compile.

## Decision

**Registration.**

- **A pack registers under its own name.** Every method of `CompilerSetBuilder` takes the provider's name. The kernel writes it into the plan's pack section, refuses a lint code whose owner is not that name, and never lets a pack register as `kernel` or take a kernel file kind or folder.
- **A surface kind and a control are registered as data,** beside the four traits: `SurfaceKindSpec` names the surface kind, the key that holds its address, and the schema of the fields it adds to a surface; a control is a name. Without them the compiler could not refuse an unknown surface kind or control, which the rules require.
- **`FileKind::compile` returns the payload and its format,** not a `PackSection`. The kernel fills in the owner, the file kind, and the surface kind, so a pack cannot write another's name. It is called once for each surface kind that has documents.
- **A document of a pack's kind finds its surface by its `surface` key,** or else is on the first declared surface of a kind its file kind is registered for. That is how two packs that both register `screens/` each get their own documents.
- **`Lowered` names the binding's surface** and carries the lookup steps, the settle time, and whether the save's response may stand for the read-back, because the plan's binding holds each and only the pack knows them.
- **A lowering may give its mode a default verb.** A binding that states no verb takes it; a mode with none requires the verb.
- **The kernel owns four keys of a binding:** `mode`, `verb`, `status`, `precondition`. Every other key, `read_back` included, is checked against the lowering's schema. The kernel still requires that a write binding has a `read_back` key and that its lowering returned read-back steps.
- **A lowering and a file kind make expressions only through their context,** which checks each against `a2m-expr` and returns the plan's `ExprIndex`. The design's `ExprRef` is that type.
- **`LintRule` gains `summary`,** so every code can be listed with what it requires. The kernel sets the code, level, and stage of what a rule returns.
- **A diagnostic's fix is required,** not optional.
- **`compile` takes the version** the plan is published as; a draft passes `DRAFT_VERSION`. `Compiled` carries the encoded bytes and hash beside the plan.
- **A declared read of a kind nobody owns is refused when a binding uses the mode,** not when the set is built, so a disabled pack breaks only the bindings that need it.
- **A small imaginary pack, `stand`, lives in `a2m-files` behind the feature `testing`.** The `example` target under `targets/example/` is written for it, so every crate above can compile a real target in its tests before any pack exists. It is never enabled in the product.

**File formats.** Each is the smallest thing that compiles; the reference is corrected in the same change.

- `auth.yaml` is a map with one key, `auth`, a list of entries. `types.yaml` is a map with one key, `types`.
- Durations are whole milliseconds in keys that end in `_ms`: `rate.per_ms`, `timeouts.idle_ms`, `limits.max_duration_ms`, `headroom_ms`. No duration syntax is parsed.
- An operation's `outputs` is a map from a name to a field type, with `list: true` for a list. A reference to a pack's schema, and nested records, are not built.
- An operation may state `hazard` and `title`. A hazard other than `none` requires the effect `irreversible`; no pack can yet show otherwise.
- A pipeline has a required `description`, since it is a tool.
- A source is written as [decision 0087](0087-the-source-readers-interface-as-building-it-settled-it.md) settled it: template versions with fingerprints, where the data lies, the columns by header text, and row fields that each name a field type and a column or an expression. **The compiled form, `SourceSpec` and the types it is made of, is defined in `a2m-files`,** beside the file formats, and written into the plan's `SourcePlan` as its payload; `a2m-sources`, a layer above, loads it. The two crates then share one definition and not only a shape. The compiler checks what the readers would refuse at load: a fingerprint's digest, a column that is not listed, a key that is not a row field, a file reader with no version.
- An expression of a source reads `row` and `source`, declared with no type, and must end in a known type.
- A success check is an expression; `all_records_verified` is a name it may read.
- A fixture folder's marker is `fixture.yaml` with `scrubbed: true`.
- A field type takes `decimal_mark`, `true_text`, and `false_text`. A date-and-time type's `timezone` is `fixed`, which is the target's `environment.timezone` or UTC, or an offset such as `+02:00` written in its place; the offset travels in the normalizer's text, since the plan's zone rule holds none. A key that means nothing for its base is refused. The built-in `decimal` has six places; `money` and `choice` cannot be used until a target states a currency or the codes.
- A name that starts with a dot is passed over by the loader.

## What the plan cannot yet carry

These are validated and then dropped, because `a2m-core`'s plan has no field for them: all of `auth.yaml`; the target's `environment`, `defaults`, `modules`, `exclusive_session`, `drift_risk`, and `authorization`; an environment's `test_records` and `approve_writes`; a surface's `stands`; an operation's `examples` and `decision_points`. Each needs a place in the plan, or another way to reach the crate that uses it, before that crate is built.

## Alternatives considered

- **Surface kinds and controls behind a fifth trait.** They hold no behavior; data is simpler and crosses a process boundary as it is.
- **`FileKind::compile` returning a whole `PackSection`,** as sketched: the kernel would have to check three fields the pack has no reason to write.
- **A verb required in every binding.** The specification's own example has a binding with none.
- **The source's compiled form defined in `a2m-sources`,** as it was first built: `a2m-files` is a layer below and cannot name it, so the two would agree only by the shape of their JSON.
- **The test pack in each dependent crate's tests.** Four crates would each write one, and the `example` target could compile with none of them.

## Revisit if

The first real pack finds a view too narrow, or a pack needs a hazard without `irreversible`.

## Spec sections

- [205. Target files](../spec/2-kernel/205-target-files.md)
- [206. The compiler and the plan](../spec/2-kernel/206-the-compiler-and-the-compiled-target.md)
- [File formats](../reference/file-formats.md)
- [`a2m-files`](../architecture/crates/a2m-files.md)
