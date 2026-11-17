# 0068. Values, names, and identifiers, as the core crate settles them

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** seven open points of the [`a2m-core` design](../architecture/crates/a2m-core.md): the decimal crate, dates and instants, the order of `Values`, whether `Verb` is closed, whether `OperationName` enforces four parts, `ValueType`, and whether `DoctorState` and `Availability` are one list. **Adds an outside crate.**

## Context

Every crate above `a2m-core` is coded against it next. The open points at the end of its design each blocked one of them: the expression crate needs the value and its type, the store needs identifiers and the record key, the compiler needs the names. They are settled together, in the crate, with tests.

## Decision

**Decimals**

- `Decimal` wraps the outside crate `rust_decimal` 1.43.0 (MIT, no features; its tree is `arrayvec` and `num-traits`, both MIT or Apache-2.0). No other crate names it: the wrapper is the only type in a public interface, so the crate can be replaced without touching a caller.
- The wrapper owns three rules the outside crate does not give. Equality is by number, and the text form drops zeros after the last digit, so `1.5` and `1.50` are equal, are written alike, and hash alike. Addition and multiplication are exact or refused: the outside crate rounds a result that does not fit, so the wrapper does its working in whole numbers and returns nothing when the result has more than 28 significant digits. Division always takes places and a rounding mode, and the rounding is decided on the exact remainder.
- `Rounding` is a closed list of seven modes: `half_up`, `half_even`, `half_down`, `up`, `down`, `ceiling`, `floor`.
- *Instead of:* a decimal of our own on a 128-bit whole number, which is a few hundred lines of arithmetic to prove; and exposing the outside type, which would spread its rounding behavior and its text form across every crate.

**Dates, moments, and lengths of time**

- A small type of our own for each, and no outside crate. `Date` is a year, a month, and a day that exists, with the count of days from 1970 in both directions. A moment is the existing `Timestamp`, milliseconds since 1970 in UTC; the value variant is named `Instant`. `Millis` is a length of time in whole milliseconds and may be negative.
- Zone rules are not here. Which day a moment falls on needs a zone database, which belongs to the expression library's host functions.
- *Instead of:* an outside date crate in layer 1, which would bring a zone database or a large tree to a crate that only needs to hold and order the values.

**`Value`, `ValueType`, and `Values`**

- `Value` has ten kinds: null, bool, int, text, decimal, instant, date, duration, list, record. `Int` and `Duration` are added to the sketch because the expression language has both and the type list names both. A choice is held as the text of its stored code. Kinds are never converted: a whole number does not equal a decimal.
- `ValueType` lives in `a2m-core` and is the one description of a type that the expression checker and the plan share. A list carries the type of its items; a record carries its fields in declared order, each required or not. It has no float and no unknown member.
- **`Values` holds its names sorted, by their bytes.** The same named values are then equal, encode alike, and have the same digest, whoever built them and in whatever order, and no producer has to remember to sort. The order a person sees comes from the schema in the plan, which lists fields in declared order. *Instead of:* declared order, which the design leaned to. It would make the digest of an input depend on the order a caller happened to send, unless every front door reordered by the schema before hashing.
- A field's name in `Values` is any text, not a checked name, because systems choose the names they return.
- `RecordKey` is one or more plain values in declared order, parent first. Order is part of the key. An absent value, a list, or a record cannot be a part.

**Names**

- **`Verb` is a checked name.** How the engine treats a binding is stated by the plan, not by the verb's spelling: each lowered step says whether it `commits`, is a `lookup`, or is `idempotent`, and the operation carries its effect class and identity strategy. The compiler's analysis holds the two together: a read that commits, or a write with no identity strategy, does not compile. The verb travels in the plan for traces and messages. *Instead of:* a closed enum, which every pack beyond the web would have to extend in the kernel.
- `OperationName` checks the shape of a name, at most 64 characters, and offers `belongs_to(target)`. It cannot check four parts, because each part may hold an underscore. The compiler, which knows the target and its modules, checks the parts.
- `LintCode` is an owner and a rule joined by one dot, such as `kernel.no_fixed_wait`.
- New checked names: `TargetName`, `SurfaceName`, `Environment`, `SurfaceKind`, `StepKind`, `FileKindName`, `ControlName`, `AccountName`.
- `Availability` and `DoctorState` stay two lists. They share three names and answer two questions: can this run here, and is this working now.

**Identifiers**

- An opaque identifier is 16 bytes laid out as a time-ordered UUID, version 7, so identifiers made later sort later and a store's own identifier type accepts them. It is written as a one-letter prefix, an underscore, and 32 hexadecimal characters: `r_` for a run, `j_` job, `s_` session, `a_` approval, `d_` decision, `w_` workspace, `p_` principal, `n_` runner.
- `from_parts` takes the moment and ten random bytes as arguments. The crate reads no clock and no random source.
- `Identity` is `Person(PrincipalId)` or `Account(AccountName)`: who the system sees, never a credential. `Epoch` is a lease number. `Version` is three whole numbers.

## Consequences

- The expression crate uses `ValueType` in place of the type its own design sketched, and maps the interpreter's values to and from `Value`.
- A crate that shows values to a person orders them by the plan's schema, not by iterating `Values`.
- The provider design's manifest example writes a lint code as `SHEET001`; the shape settled here is `owner.rule`.
- `rust_decimal` is in the workspace's dependencies. The licence list is unchanged.

## Alternatives considered

Given beside each choice above.

## Revisit if

A business value needs more than 28 significant digits; a pack needs a verb whose treatment the step flags cannot state; or sorted names make a common case, such as rendering a diff, need the schema where it is not at hand.

## Spec sections

- [The `a2m-core` design](../architecture/crates/a2m-core.md)
- [210-sources-and-value-normalization.md](../spec/2-kernel/210-sources-and-value-normalization.md)
- [209-record-identity-and-dependent-records.md](../spec/2-kernel/209-record-identity-and-dependent-records.md)
- [The technical reference](../reference/technical-reference.md)
