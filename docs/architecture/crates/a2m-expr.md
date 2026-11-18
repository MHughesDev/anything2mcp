# `a2m-expr`

Expressions: an expression in CEL's syntax is checked and compiled once, when a plan is built, and evaluated many times, when a plan runs. The parser, the type checker, and the evaluator are this crate's own, for the part of CEL the file formats use ([decision 0072](../../decisions/0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md)). It also holds the host functions that every expression and every field type may call: units, numbers, dates, text, and code lists. It never reads a file, a clock, or the network.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside: `unicode-normalization`, for form C ([decision 0073](../../decisions/0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md)). No interpreter crate, and no decimal crate of its own: a decimal is `a2m-core`'s |
| Roadmap packages | Phase 1: slice 1.3 builds compiling and type-checking for the read files; slice 3.4 builds the normalizers; slice 3.9 builds the mapping functions for sources |
| Carries out | Spec sections [205](../../spec/2-kernel/205-target-files.md) (expressions), [210](../../spec/2-kernel/210-sources-and-value-normalization.md) (normalization), [206](../../spec/2-kernel/206-the-compiler-and-the-plan.md) (stage 4); the [expressions](../../reference/file-formats.md#expressions) and field types of the file formats; [decision 0014](../../decisions/0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md) |

## Owns

- The type environment an expression is checked against: which names exist (`input`, `result`, `row`, `source`, `steps`, `env`, or whatever a caller declares) and their types.
- Compiling: parsing, type-checking, and turning the result into the plan's expression form, `a2m-core`'s `PlanExpr`.
- Loading a `PlanExpr` back into a program, and evaluating it against bound values, with a cost limit, to one `Value`.
- The host-function library, closed and versioned with the engine, and the normalizer for each field-type base.
- The evaluator, which works directly in `a2m-core`'s `Value` and `ValueType`. There is no interpreter value to map.

It must never: read a clock, a random source, a file, an environment variable, or the network; offer a function whose answer differs between two calls with the same arguments; produce or compare a float; know a mode, a pack, or a kind of system; accept a host function from outside the crate.

## Interface

```rust
/// The type of a name or of a result is `a2m-core`'s `ValueType`: Null, Bool, Int, Text, Decimal,
/// Instant, Date, Duration, Choice, List, Record. It has no double. This crate adds one thing
/// that never leaves it: an unknown type, allowed while drafting, as a warning.
pub enum Checked { Known(ValueType), Dyn }

/// What an expression may name. The compiler builds one per place: an operation's binding,
/// a pipeline step's `with`, a source mapping, an `expect`. Policy builds its own.
pub struct TypeEnv { /* name -> Checked, plus the library version */ }
impl TypeEnv {
    pub fn new(library: LibraryVersion) -> Self;
    pub fn declare(&mut self, name: &str, ty: ValueType) -> Result<(), ExprError>;
    pub fn declare_unknown(&mut self, name: &str) -> Result<(), ExprError>;   // a draft's name, type not stated yet
}
pub mod names { INPUT, RESULT, ROW, SOURCE, STEPS, ENV, CURRENT, RECORD }   // the file formats' names, as constants; CURRENT and RECORD in a binding's steps (decision 0161)

/// Checks and compiles one expression. `expected` is the type the place requires, when it has one.
pub fn compile(source: &str, env: &TypeEnv, expected: Option<&ValueType>) -> Result<Compiled, ExprError>;

/// A checked expression, in memory, for the compiler's use. Never stored as it is.
pub struct Compiled { /* private; read through the methods */ }
impl Compiled {
    pub fn source(&self) -> &str;              // canonical text: the tree printed back, not the author's spacing
    pub fn result(&self) -> &Checked;          // the place's type when one was given and fits
    pub fn names(&self) -> &[String];          // the names it reads, sorted
    pub fn functions(&self) -> &[HostFnName];  // the host functions it calls, sorted
    pub fn library(&self) -> LibraryVersion;
    /// The plan's form: `a2m-core`'s `PlanExpr`, which holds the canonical text, the `ValueType`,
    /// and the library version. Refuses a `Dyn` result, so nothing unknown reaches a plan.
    pub fn to_plan_expr(&self) -> Result<PlanExpr, ExprError>;
}

/// The parsed form, rebuilt from a plan's `PlanExpr` when the plan is loaded, and kept for its life.
/// `Send + Sync`.
pub struct Program { /* the tree; private */ }
impl Program {
    pub fn load(expr: &PlanExpr) -> Result<Program, Failure>;   // reparses, and checks again
    pub fn result_type(&self) -> &ValueType;
    pub fn names(&self) -> &[String];                           // each must be bound
    /// Pure. Same program, same bindings: same answer, on every machine.
    pub fn evaluate(&self, bindings: &Bindings<'_>) -> Result<Value, Failure>;
}

/// The values for the names a program reads. Borrowed; a value the program does not read is
/// never copied.
pub struct Bindings<'a> { /* name -> &'a Value */ }
impl<'a> Bindings<'a> {
    pub const fn new() -> Self;
    pub fn bind(&mut self, name: &'a str, value: &'a Value);
    pub fn with(self, name: &'a str, value: &'a Value) -> Self;
}

/// The library. Each function has a name, a signature, a sentence, and a test case.
pub struct HostFnName(/* only the library makes one */);
pub struct LibraryVersion(u32);            // LibraryVersion::CURRENT is 1; new(n) is None for a version not carried
pub struct HostFn { /* name(), group(), signature(), summary() */ }
pub struct Library { /* version(), cost_limit(), functions(), get(name) */ }
pub fn library() -> &'static Library;      // for `docs.get` and for the type checker
pub const COST_LIMIT: u64 = 1_000_000;

/// A field type's rule, as far as comparing goes: what `types.yaml` says. One variant per base:
/// Text, Integer, Decimal, Money, Measurement, Date, Datetime, Time, Boolean, Choice.
pub enum FieldRule { /* see the source */ }
impl FieldRule {
    pub const fn base(&self) -> FieldBase;
    pub const fn value_type(&self) -> ValueType;
    /// The normalizer, compiled: one call `as_<base>(value, rules...)`. `a2m-files` stores its
    /// `to_plan_expr()` with the field type.
    pub fn normalizer(&self) -> Result<Compiled, ExprError>;
}
pub enum Whitespace { Collapse, Trim, Exact }
pub enum DecimalMark { Point, Comma }
pub enum DateSystem { From1900, From1904 }
pub fn check_format(format: &str, base: FieldBase) -> Result<(), ExprError>;
pub fn currency_places(code: &str) -> u32;

/// Field-type normalization, as the plan stores it.
pub struct Normalizer { /* a loaded Program over the one name `value` */ }
impl Normalizer {
    pub fn load(expr: &PlanExpr) -> Result<Normalizer, Failure>;
    pub fn value_type(&self) -> &ValueType;
    pub fn normalize(&self, raw: &Value) -> Result<Normalized, Failure>;
}
pub struct Normalized { pub raw: Value, pub normalized: Value }   // both kept, as section 210 says
impl Normalized { pub fn agrees_with(&self, other: &Normalized) -> bool; }

/// The parse functions as Rust, for a source reader: the same code the host functions run.
pub mod parse { decimal, integer, money, feet_inches, date, excel_serial }

/// A compile error: a code, a sentence, and a place inside the expression. `a2m-files` adds the
/// file and line of the expression itself. Converts to `Failure`.
pub struct ExprError { pub code: FailureCode, pub message: String, pub at: Span, pub fix: Option<String> }
pub struct Span { pub start: u32, pub end: u32 }   // byte offsets into the source text
pub const EXPRESSION_INVALID, EXPRESSION_TYPE_ERROR, VALUE_OUT_OF_RANGE, VALUE_UNPARSABLE,
          VALUE_ABSENT, EXPRESSION_COST_EXCEEDED, PLAN_EXPRESSION_UNLOADABLE: FailureCode;
```

Contracts that the signatures do not show:

- **There is no float.** A number with a decimal point or an exponent is refused when the text is read, with its place and the fix `decimal("...")`. No type, no literal, and no function is a double, so there is nothing to switch off. A decimal is `a2m-core`'s `Decimal`, written `decimal("1.5")`, with `+`, `-`, `*`, unary minus, and the comparisons as operators. Division is only `div(a, b, places, rounding)` ([decision 0073](../../decisions/0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md)).
- **No conversion is silent.** `Text` never becomes `Decimal` except through `parse_decimal`, with the field type's places and rounding; `decimal(text)` takes only a literal of the expression. An `Int` meets a `Decimal` only through `decimal()`.
- **Checked twice.** Types are checked when an expression is compiled. A plan stores the text and the result type but not the names' types, so `Program::load` reads every name as unknown, and evaluation checks the kind of every operand and of the answer. A value of the wrong kind stops; it is never reinterpreted.
- **Absence is a value.** `null` may be compared with `==` and `!=`, given a default with `coalesce`, and returned; a field of an absent record is absent. Anything else that needs an absent value stops with `value_absent`.
- **Evaluation is pure and bounded.** Every expression terminates: the source is at most 16 KiB, the tree at most 64 high, and there is no loop but the macros over a list. The cost limit also bounds work over long lists and long text, so a source with many rows cannot make one expression slow. Exceeding it is a failure, never a partial answer. The limit belongs to the library version, not to each expression, so the plan does not carry it.
- **Time is always given, never read.** There is no `now()`. Where an expression needs the moment of the run, the engine binds it as a name, and the trace records the value bound.
- **A zone rule is never assumed.** `parse_datetime` and `format_datetime` take the zone as a literal compiled into the call: `UTC` or a fixed offset. An expression with no zone for an instant does not compile. Named zones are not built.
- **A rule is a literal.** Places, rounding modes, units, formats, zones, decimal marks, currencies, and code lists are written in the expression and checked when it is compiled. They come from a field type, never from a value.
- **The plan stores text.** A `PlanExpr` holds canonical source, and `Program::load` refuses text that is not in the canonical spelling, so a plan's expression is exactly what the compiler wrote. If reparsing misses the plan-load budget, a cached form is added behind the same type.
- **Host functions are the kernel's.** A pack cannot add one. A conversion the library lacks is added here, with tests, or done in a reviewed plug-in ([section 205](../../spec/2-kernel/205-target-files.md)).
- **A failure never carries a value.** A run-time failure names the function or operator and the kind of problem. The caller that knows the expression's name in the plan adds it.
- **No text stands for two choices** ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)). A value read from a system may be a choice's stored code or its label, so a code list is refused, when its expression is compiled and again when a plan's expression is loaded, where two labels are alike, two codes are alike, or a code is the label of another choice; "alike" is form C with whitespace collapsed. A code may be its own label. Without the rule, a write of the code `B` that failed on a record that still holds `A`, shown by its label `B`, would read back as verified. `code_for` holds the same rule where the value is read.
- **A date format is read without guessing.** A part of one or two digits (`M`, `D`, `H`, `h`) may end the format or stand before a separator, a month's name, or `A`; a format in which digits follow it, such as `YYYYMD`, is refused, because `2026111` would be two days at once.
- **A currency symbol is read only for a currency it is written for.** The table is fixed: the dollar sign for the dollar currencies, the pound sign for the pounds, the yen sign for `JPY` and `CNY`, and one currency each for the rest. Another currency's symbol stops, as another currency's code does; the cent sign and the general currency sign stop for every currency. This supersedes the words "one currency symbol" in [decision 0073](../../decisions/0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md).
- **Text compared without case is compared by its capital form.** `as_text` with `insensitive`, and a boolean's two labels, are folded: the lower case of the upper case, in form C. A system that shows text in capitals turns one letter into two, and `Straße` and `STRASSE` are then one text. The function `lower` is unchanged. A character that is not seen, such as a soft hyphen or a zero-width space, is compared as written: a system that strips one gives a mismatch, which stops, and never a match.

### The language

A subset of CEL: names; `a.f`; `a[i]`; calls; `!`, unary `-`; `*`, `/`, `%`, `+`, `-`; `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`; `&&`, `||`; `c ? a : b`; whole numbers, text in single or double quotes, `true`, `false`, `null`; list literals; record literals with text keys; the member functions `size`, `startsWith`, `endsWith`, `contains`; and the macros `has(a.f)`, `list.all(x, p)`, `exists`, `exists_one`, `map`, `filter`. Refused, with a place: a double, an unsigned number, bytes, raw text, hexadecimal, and anything else of CEL not listed here.

### The host-function library

The starting list is the one in the [file formats](../../reference/file-formats.md#expressions). This crate adds the functions that normalization needs and that the file formats imply. Library version 1 has 36 functions; `library()` lists each with its signature.

| Group | Functions | Notes |
| --- | --- | --- |
| Measurement | `to_inches`, `to_feet_inches`, `to_unit`, `snap_to_step` | A measurement is a decimal in the field's base unit; units are named by the field type, never by the expression ([rule 8](../../spec/1-product/103-design-principles.md)). The units are a closed table of lengths and masses with exact factors. `to_inches` needs the marks: a bare number stops |
| Numbers | `decimal`, `parse_decimal`, `parse_int`, `parse_money`, `parse_bool`, `round_to`, `div` | `div` takes places and a rounding mode; there is no division without them. Grouping is stripped only when it is one separator and groups of three |
| Dates | `parse_date`, `format_date`, `parse_datetime`, `format_datetime`, `parse_time`, `excel_serial_to_date` | Formats are a closed pattern language. The last takes the workbook's date system ([section 210](../../spec/2-kernel/210-sources-and-value-normalization.md)) |
| Text | `trim`, `lower`, `upper`, `nfc`, `collapse_space` | `collapse_space` treats non-breaking and narrow no-break spaces as spaces |
| Choices | `code_for`, `label_for` | Against a code list written into the call as a record literal |
| Absence | `coalesce` | The only way to give a default |
| Lists | `size` | Also as a member function |
| Field types | `as_text`, `as_integer`, `as_decimal`, `as_money`, `as_measurement`, `as_date`, `as_datetime`, `as_time`, `as_boolean`, `as_choice` | One per field-type base. Each reads a value typed or as displayed text and gives the same typed value for both. `FieldRule::normalizer()` writes the call |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-files` | `TypeEnv`, `names`, `compile`, `ExprError` in stage 4; `to_plan_expr` for every expression; `FieldRule::normalizer()` for every field type, whose `PlanExpr` the plan then holds; `check_format`, `currency_places` |
| `a2m-engine` | `Program::load` at plan load; `evaluate` for bodies, `with`, `when`, `expect`, `success`; `Normalizer` before every comparison |
| `a2m-sources` | `Program::load` and `evaluate` for mappings and the `rows` filter; the `parse` module; `DecimalMark`, `DateSystem` |
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
| `input` | `value_unparsable` | `parse_decimal`, `parse_date`, `code_for`, or a normalizer met text it cannot read, or a value is not of the kind the expression needs. Run time |
| `input` | `value_absent` | A value was absent where the expression needs one. Run time |
| `input` | `expression_cost_exceeded` | Evaluation passed its cost limit. Run time |
| `input` | `plan_expression_unloadable` | A plan's `PlanExpr` names a library version this engine does not have, is not in the canonical spelling, or no longer checks as the type it states. Plan load. Also when a program is run without a name it reads |

Run-time failures name the host function or operator, never the value that caused them: a value may be business data, and a failure's sentence is shown to callers. The engine, which knows the expression's name in the plan, adds it.

The codes are declared once, each with its meaning and next step, in the crate's `failure_codes!` table, in its module `codes` ([decision 0258](../../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)). A failure whose case adds nothing to the next step carries the table's; a compile error with a fix carries the fix.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No expression that compiles can produce or compare a float | `a_float_an_unknown_name_and_a_wrong_type_are_refused_with_a_place`: every spelling of a double is refused; `ValueType` has no float member, and no function returns one |
| Evaluation gives the same `Value` for the same bindings, across runs and operating systems | `evaluation_gives_the_same_bytes_every_time`: a table of cases compared as canonical bytes, run in CI on three systems |
| `compile` and `to_plan_expr` give the same `PlanExpr` for the same source and environment | `the_same_source_compiles_to_the_same_plan_expression`, and the printer's round-trip test |
| A function's answer never depends on the clock, the locale, or the machine's zone | By construction: the crate calls no clock, locale, or environment interface. A run under two zones and two locales in CI is still owed |
| Every library function has a test of its edge cases and is listed by `docs.get` | `every_library_function_has_a_case_and_a_page` walks `library()` and fails on a function with no case or no sentence |
| A failure's sentence never contains a bound value | `a_failure_never_carries_a_bound_value`: failing cases with a planted marker |
| A value the system rounds or reformats compares equal under its field type's rule, and a different value does not | `a_value_the_system_rounds_or_reformats_compares_equal_and_another_does_not`, over every base |
| A runaway expression stops with a named failure | `the_cost_limit_stops_a_runaway_with_a_named_failure` |
| A plan's expression loads only when it is what the compiler wrote | `a_plan_expression_loads_only_when_it_is_what_the_compiler_wrote` |
| A choice read back never stands for two choices: the intended value and a stale one never agree | `a_choice_read_back_never_stands_for_two_choices`; `functions::tests`, for the rule where the value is read |
| A date format whose parts run together, another currency's symbol, and a code list in which a text stands for two choices are refused | `a_rule_the_library_does_not_know_is_refused_when_the_field_type_compiles`, `a_value_a_rule_cannot_read_stops_and_is_never_guessed`; `dates::tests`, `numbers::tests` |
| Text a system shows in capitals compares equal to the text it was made from, and folding twice changes nothing | `text::tests`, and the case in `a_value_the_system_rounds_or_reformats_compares_equal_and_another_does_not` |

## Build order

1. Done: the candidates were verified and none fits; the crate has its own parser, checker, and evaluator ([decision 0072](../../decisions/0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md)).
2. Done: `TypeEnv`, `compile`, `Compiled`, `to_plan_expr`, `Program::load`, `evaluate`, `Bindings`, over every `Value` kind.
3. Done: decimals with operators, the number functions, and `Normalizer` for `integer`, `decimal`, `money`, and `measurement` ([decision 0073](../../decisions/0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md)).
4. Done, but for named zones: dates, times of day, moments through a fixed offset, `text`, `boolean`, and `choice` normalization. **Not built:** a named zone such as one from the zone database, and the `user` zone rule; a field type with either does not compile.
5. Done: records and lists for `row`, `source`, and `steps`, and the `parse` module for sources.
6. Done as a number with a test: the cost limit is 1,000,000 units. **Not done:** measuring it on the test application's largest pipeline, which needs the engine.

Not built, and not in the steps above: arithmetic on dates, instants, and durations; `format_time`; a workbook serial with a time of day as an instant; regular expressions; the built-in field types of the file formats as rules, which are `a2m-files`'.

## Open points

- Named zones. They need a zone database: an outside crate, bundled so that the answer does not depend on the machine, and versioned with the library. The slice that first meets a target with a named zone decides.
- Whether `Dyn` should reach a plan at all. As written it cannot: a draft with an unknown type compiles with `Checked::Dyn`, which `a2m-files` reports as a warning, and cannot be tried until the type is stated.
- Whether `Program::load` should take the names' types, so that the full check runs again at plan load. Today the plan does not store them, and evaluation checks kinds instead.
- Whether a pack should be able to add a host function through the compiler set, with tests. The proposal is no: units live in field types, and field types are the kernel's.
- The table of units is lengths and masses. Volumes, areas, and temperatures are added here when a target needs one.
- Section 225 sets no budget for evaluation. A target of a few microseconds for a typical mapping is proposed, to be measured in slice 1.7. A call looks its function up by name on each evaluation; resolving it once at load is the first thing to do if the budget is missed.
- A record literal's fields are held in name order, as `a2m-core`'s `Values` holds them.
