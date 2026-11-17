# `a2m-expr`

Expressions: a CEL program is checked and compiled once, when a plan is built, and evaluated many times, when a plan runs. It also holds the host functions that every expression and every field type may call: units, numbers, dates, text, and code lists. It never reads a file, a clock, or the network.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside: a CEL interpreter crate, to be verified; the decimal crate that `a2m-core` chooses; `serde` |
| Built in | Phase 1: slice 1.3 builds compiling and type-checking for the read files; slice 3.4 builds the normalizers; slice 3.9 builds the mapping functions for sources |
| Carries out | Spec sections [205](../../spec/2-kernel/205-target-files.md) (expressions), [210](../../spec/2-kernel/210-sources-and-value-normalization.md) (normalization), [206](../../spec/2-kernel/206-the-compiler-and-the-plan.md) (stage 4); the [expressions](../../reference/file-formats.md#expressions) and field types of the file formats; [decision 0014](../../decisions/0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md) |

## Owns

- The type environment an expression is checked against: which names exist (`input`, `result`, `row`, `source`, `steps`, `env`, or whatever a caller declares) and their types.
- Compiling: parsing, type-checking, and turning the result into the plan's expression form, `a2m-core`'s `PlanExpr`.
- Loading a `PlanExpr` back into a program, and evaluating it against bound values, with a cost limit, to one `Value`.
- The host-function library, closed and versioned with the engine, and the normalizer for each field-type base.
- The mapping between `a2m-core`'s `Value` and the interpreter's own values, so that no other crate sees the interpreter.

It must never: read a clock, a random source, a file, an environment variable, or the network; offer a function whose answer differs between two calls with the same arguments; produce or compare a float; know a mode, a pack, or a kind of system; accept a host function from outside the crate.

## Interface

```rust
/// The type of a name or of a result is `a2m-core`'s `ValueType`: Null, Bool, Int, Text, Decimal,
/// Instant, Date, Duration, List, Record, Choice. It has no double. This crate adds one thing
/// that never leaves it: an unknown type, allowed while drafting, as a warning.
pub enum Checked { Known(ValueType), Dyn }

/// What an expression may name. The compiler builds one per place: an operation's binding,
/// a pipeline step's `with`, a source mapping, an `expect`. Policy builds its own.
pub struct TypeEnv { /* name -> ValueType, plus the host-function table version */ }
impl TypeEnv {
    pub fn new(library: LibraryVersion) -> Self;
    pub fn declare(&mut self, name: &str, ty: ValueType) -> Result<(), ExprError>;
}

/// Checks and compiles one expression. `expected` is the type the place requires, when it has one.
pub fn compile(source: &str, env: &TypeEnv, expected: Option<&ValueType>) -> Result<Compiled, ExprError>;

/// A checked expression, in memory, for the compiler's use. Never stored as it is.
pub struct Compiled {
    source: String,              // canonical text: the parsed program printed back, not the author's spacing
    result: Checked,
    names: Vec<String>,          // the names it reads, sorted; used by the compiler's analysis
    functions: Vec<HostFnName>,  // the host functions it calls, sorted
    library: LibraryVersion,
}
impl Compiled {
    /// The plan's form: `a2m-core`'s `PlanExpr`, which holds the canonical text, the `ValueType`,
    /// and the library version. Refuses a `Dyn` result, so nothing unknown reaches a plan.
    pub fn to_plan_expr(&self) -> Result<PlanExpr, ExprError>;
}

/// The parsed form, rebuilt from a plan's `PlanExpr` when the plan is loaded, and kept for its life.
pub struct Program { /* the interpreter's program; private */ }
impl Program {
    pub fn load(expr: &PlanExpr) -> Result<Program, Failure>;   // reparses, and checks the type again
    /// Pure. Same program, same bindings: same answer, on every machine.
    pub fn evaluate(&self, bindings: &Bindings<'_>) -> Result<Value, Failure>;
}

/// The values for the names a program reads. Borrowed; nothing is copied into the interpreter
/// that the program does not read.
pub struct Bindings<'a> { /* name -> &'a Value */ }

/// The library. Each function has a name, a signature, a version, and tests.
pub struct HostFnName(/* checked name */);
pub struct LibraryVersion(u32);
pub fn library() -> &'static Library;   // for `docs.get` and for the type checker

/// Field-type normalization, as the plan stores it: one compiled call chain per field type.
pub struct Normalizer { /* a loaded Program, from the PlanExpr `a2m-files` built for the field type */ }
impl Normalizer {
    pub fn normalize(&self, raw: &Value) -> Result<Normalized, Failure>;
}
pub struct Normalized { pub raw: Value, pub normalized: Value }   // both kept, as section 210 says

/// A compile error: a code, a sentence, and a place inside the expression. `a2m-files` adds the
/// file and line of the expression itself.
pub struct ExprError { pub code: FailureCode, pub message: String, pub at: Span, pub fix: Option<String> }
pub struct Span { pub start: u32, pub end: u32 }   // byte offsets into the source text
```

Contracts that the signatures do not show:

- **There is no float.** CEL has a double type and no decimal type. The type checker refuses a double literal, a double-typed name, and any function that returns one. A decimal is an abstract type of this crate, carried as `a2m-core`'s `Decimal`, with host functions for arithmetic and comparison. Whether CEL's operators can be overloaded for it depends on the interpreter crate; if not, decimals are compared and added only through named functions, and the type checker says so in the fix.
- **No conversion is silent.** `Text` never becomes `Decimal` except through `parse_decimal`, with the field type's places and rounding. An `Int` meets a `Decimal` only through `decimal()`.
- **Evaluation is pure and bounded.** CEL always terminates; the cost limit also bounds work over long lists, so a source with many rows cannot make one expression slow. Exceeding it is a failure, never a partial answer. The limit belongs to the library version, not to each expression, so the plan does not carry it.
- **Time is always given, never read.** There is no `now()`. Where an expression needs the moment of the run, the engine binds it as a name, and the trace records the value bound.
- **A zone rule is never assumed.** `parse_date` and `format_date` take the zone rule from the field type, compiled into the call; an expression with no rule for an instant does not compile.
- **The plan stores text, not the interpreter's tree.** A `PlanExpr` holds canonical source, so a plan does not depend on the outside crate's internal format, and the plan stays byte-identical across builds of that crate. `Program::load` reparses at plan load. If that misses the plan-load budget, a cached form is added behind the same type.
- **Host functions are the kernel's.** A pack cannot add one. A conversion the library lacks is added here, with tests, or done in a reviewed plug-in ([section 205](../../spec/2-kernel/205-target-files.md)).

### The host-function library

The starting list is the one in the [file formats](../../reference/file-formats.md#expressions). This crate adds the functions that normalization needs and that the file formats imply.

| Group | Functions | Notes |
| --- | --- | --- |
| Measurement | `to_inches`, `to_feet_inches`, `to_unit`, `snap_to_step` | A measurement is a decimal in the field's base unit; units are named by the field type, never by the expression ([rule 8](../../spec/1-product/103-design-principles.md)) |
| Numbers | `parse_decimal`, `round_to`, `decimal`, `add`, `sub`, `mul`, `div`, `cmp` | `div` takes a rounding mode and places; there is no division without them |
| Dates | `parse_date`, `format_date`, `excel_serial_to_date` | The last takes the workbook's date system ([section 210](../../spec/2-kernel/210-sources-and-value-normalization.md)) |
| Text | `trim`, `lower`, `upper`, `nfc`, `collapse_space` | `collapse_space` treats non-breaking and narrow no-break spaces as spaces |
| Choices | `code_for`, `label_for` | Against a code list compiled into the call |
| Absence | `coalesce` | The only way to give a default |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-files` | `TypeEnv`, `compile`, `ExprError` in stage 4; `to_plan_expr` for every expression and every field type's normalizer, which the plan then holds |
| `a2m-engine` | `Program::load` at plan load; `evaluate` for bodies, `with`, `when`, `expect`, `success`; `Normalizer` before every comparison |
| `a2m-sources` | `evaluate` for mappings and the `rows` filter; the parse functions |
| `a2m-policy` | `compile` and `evaluate`, with its own `TypeEnv`, if its rules carry conditions |
| `a2m-actions` | `library()`, for `docs.get` |

It calls only `a2m-core`.

## State

None that persists. A loaded `Program` lives as long as the plan that holds it, in the engine's memory. The library is a static table.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `expression_invalid` | The text does not parse, or names something the environment does not declare. Compile time |
| `input` | `expression_type_error` | A type does not fit, including any use of a float. Compile time |
| `input` | `value_out_of_range` | A host function was given a value it cannot represent: a decimal with too many places, a date before the workbook's epoch. Run time |
| `input` | `value_unparsable` | `parse_decimal`, `parse_date`, or `code_for` met text it cannot read. Run time |
| `input` | `expression_cost_exceeded` | Evaluation passed its cost limit. Run time |
| `input` | `plan_expression_unloadable` | A plan's `PlanExpr` names a library version this engine does not have, or no longer checks as the type it states. Plan load |

Run-time failures carry the expression's name in the plan and the host function, never the value that caused them: a value may be business data, and a failure's sentence is shown to callers.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No expression that compiles can produce or compare a float | A type-checker test over every library signature; a test that each double-producing CEL built-in is refused |
| Evaluation gives the same `Value` for the same bindings, across runs and operating systems | A table of cases run in CI on three systems, compared byte for byte |
| `compile` and `to_plan_expr` give the same `PlanExpr` for the same source and environment | A test that compiles twice and compares; part of the compiler's byte-identical plan test |
| A function's answer never depends on the clock, the locale, or the machine's zone | Tests run under two zones and two locales in CI |
| Every library function has a test of its edge cases and is listed by `docs.get` | A test that walks `library()` and fails on a function with no test case or no page |
| A failure's sentence never contains a bound value | A test that evaluates failing cases with a planted marker value and searches the failure |

## Build order

1. Verify a CEL interpreter crate: licence on the closed list, maintenance, the three operating systems, and whether it can disable doubles and overload operators. Record the choice. Slice 1.3.
2. `TypeEnv`, `compile`, `Compiled`, `to_plan_expr`, `Program::load`, and the `Value` mapping for `Null`, `Bool`, `Text`, and records, with the `input` name only. Enough for a read binding's placeholders. Slice 1.3.
3. `Decimal`, the number functions, and `Normalizer` for `decimal`, `money`, and `measurement`. Slice 3.4, which compares a value the application rounds.
4. Dates, zones, and `text` normalization. Slice 3.4.
5. `row`, `source`, `steps`, and the parse functions for sources. Slice 3.9.
6. The cost limit's value, measured on the test application's largest pipeline. Slice 3.9.

## Open points

- Which CEL crate. If none can refuse doubles at check time, this crate checks the parsed tree itself before handing it over.
- Whether decimals use CEL operators or named functions. This changes how every file is written, so the slice that settles it records a decision and updates the file formats.
- Whether `Dyn` should reach a plan at all. As written it cannot: a draft with an unknown type lints with a warning but cannot be tried until the type is stated. If that proves too strict while drafting, `ValueType` would need an unknown member, which `a2m-core` would own.
- Whether a pack should be able to add a host function through the compiler set, with tests. The proposal is no: units live in field types, and field types are the kernel's.
- Section 225 sets no budget for evaluation. A target of a few microseconds for a typical mapping is proposed, to be measured in slice 1.7.
