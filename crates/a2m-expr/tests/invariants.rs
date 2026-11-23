//! The invariants of the crate's design, each as a test.

use std::collections::BTreeSet;

use a2m_core::{
    CodeLabel, Date, Decimal, Failure, FailureClass, FieldOf, Millis, PlanExpr, Rounding,
    Timestamp, Value, ValueType, Values,
};
use a2m_expr::{
    Bindings, COST_LIMIT, Checked, DecimalMark, EXPRESSION_COST_EXCEEDED, EXPRESSION_INVALID,
    EXPRESSION_TYPE_ERROR, ExprError, FieldRule, LibraryVersion, Normalizer,
    PLAN_EXPRESSION_UNLOADABLE, Program, Span, TypeEnv, VALUE_ABSENT, VALUE_OUT_OF_RANGE,
    VALUE_UNPARSABLE, Whitespace, compile, library, names,
};

/// The value of a result that a helper expects to be good.
fn ok<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("{error:?}"))
}

fn d(text: &str) -> Decimal {
    ok(Decimal::parse(text))
}

fn row_type() -> ValueType {
    ValueType::Record(vec![
        FieldOf::required("code", ValueType::Text),
        FieldOf::required("qty", ValueType::Decimal),
        FieldOf::optional("note", ValueType::Text),
    ])
}

/// The names a pipeline step may read, with types.
fn env() -> TypeEnv {
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    ok(env.declare(
        names::INPUT,
        ValueType::Record(vec![
            FieldOf::required("name", ValueType::Text),
            FieldOf::required("width", ValueType::Text),
            FieldOf::required("price", ValueType::Decimal),
            FieldOf::required("count", ValueType::Int),
            FieldOf::optional("note", ValueType::Text),
            FieldOf::required("due", ValueType::Date),
            FieldOf::required("seen", ValueType::Instant),
            FieldOf::required("status", ValueType::Choice),
        ]),
    ));
    ok(env.declare(names::ROW, row_type()));
    ok(env.declare(
        names::SOURCE,
        ValueType::Record(vec![FieldOf::required(
            "rows",
            ValueType::list_of(row_type()),
        )]),
    ));
    ok(env.declare(
        names::STEPS,
        ValueType::Record(vec![FieldOf::required(
            "find",
            ValueType::Record(vec![FieldOf::required(
                "result",
                ValueType::Record(vec![
                    FieldOf::required("id", ValueType::Text),
                    FieldOf::required("total", ValueType::Decimal),
                ]),
            )]),
        )]),
    ));
    env
}

fn row(code: &str, qty: &str) -> Value {
    Value::Record(Values::new().with("code", code).with("qty", d(qty)))
}

struct Bound {
    input: Value,
    row: Value,
    source: Value,
    steps: Value,
}

fn bound() -> Bound {
    Bound {
        input: Value::Record(
            Values::new()
                .with("name", "  Caf\u{e9}\u{a0} Table ")
                .with("width", "7' 6 1/2\"")
                .with("price", d("32.5625"))
                .with("count", 3i64)
                .with("due", ok(Date::new(2026, 11, 17)))
                .with("seen", Timestamp::from_unix_millis(1_794_916_800_250))
                .with("status", "A"),
        ),
        row: row("K-1", "2.5"),
        source: Value::Record(Values::new().with(
            "rows",
            vec![row("K-1", "2.5"), row("K-2", "0"), row("K-3", "4")],
        )),
        steps: Value::Record(Values::new().with(
            "find",
            Values::new().with(
                "result",
                Values::new().with("id", "R-77").with("total", d("6.5")),
            ),
        )),
    }
}

fn run(source: &str, expected: Option<&ValueType>) -> Result<Value, Failure> {
    let compiled = compile(source, &env(), expected).map_err(Failure::from)?;
    let plan = compiled.to_plan_expr().map_err(Failure::from)?;
    let program = Program::load(&plan)?;
    let bound = bound();
    let bindings = Bindings::new()
        .with(names::INPUT, &bound.input)
        .with(names::ROW, &bound.row)
        .with(names::SOURCE, &bound.source)
        .with(names::STEPS, &bound.steps);
    program.evaluate(&bindings)
}

fn refused(source: &str) -> ExprError {
    match compile(source, &env(), None) {
        Err(error) => error,
        Ok(compiled) => panic!("`{source}` compiled: {compiled:?}"),
    }
}

#[test]
fn the_same_source_compiles_to_the_same_plan_expression() {
    let env = env();
    let first = compile(
        "round_to( input.price*decimal('2') ,2,\"half_even\" )",
        &env,
        None,
    )
    .unwrap();
    let again = compile(
        "round_to( input.price*decimal('2') ,2,\"half_even\" )",
        &env,
        None,
    )
    .unwrap();
    let spaced = compile(
        "round_to(input.price * decimal(\"2\"),\n  2, 'half_even')",
        &env,
        None,
    )
    .unwrap();
    assert_eq!(first, again);
    assert_eq!(first.to_plan_expr().unwrap(), again.to_plan_expr().unwrap());
    assert_eq!(
        first.to_plan_expr().unwrap(),
        spaced.to_plan_expr().unwrap()
    );
    assert_eq!(
        first.to_plan_expr().unwrap(),
        PlanExpr {
            source: "round_to(input.price * decimal(\"2\"), 2, \"half_even\")".to_owned(),
            result: ValueType::Decimal,
            library: 1,
        }
    );
    // Compiling the canonical text gives the canonical text.
    let canonical = compile(first.source(), &env, None).unwrap();
    assert_eq!(canonical.source(), first.source());
    assert_eq!(first.names(), ["input".to_owned()]);
    let called: Vec<&str> = first.functions().iter().map(|name| name.as_str()).collect();
    assert_eq!(called, ["decimal", "round_to"]);
}

#[test]
fn a_float_an_unknown_name_and_a_wrong_type_are_refused_with_a_place() {
    // A float, wherever it stands.
    let float = refused("input.price * 1.5");
    assert_eq!(float.code, EXPRESSION_TYPE_ERROR);
    assert_eq!(float.at, Span::new(14, 17));
    assert_eq!(
        float.fix.as_deref(),
        Some("Write decimal(\"1.5\") for an exact decimal.")
    );
    // Every way CEL has of making a double is refused.
    for source in [
        "1.0",
        "2e3",
        ".5",
        "double(1)",
        "double(\"1.5\")",
        "math.sqrt(4)",
        "float(1)",
        "1 / 2.0",
        "input.count * 1.0",
        "[1.5]",
        "{\"a\": 0.1}",
    ] {
        let error = refused(source);
        assert!(
            error.code == EXPRESSION_TYPE_ERROR || error.code == EXPRESSION_INVALID,
            "{source}"
        );
    }
    // An unknown name, with the names there are.
    let unknown = refused("trim(inptu.name)");
    assert_eq!(unknown.code, EXPRESSION_INVALID);
    assert_eq!(unknown.at, Span::new(5, 10));
    assert!(unknown.fix.unwrap().contains("input, row, source, steps"));
    let field = refused("input.nmae");
    assert_eq!(field.code, EXPRESSION_INVALID);
    assert!(field.fix.unwrap().contains("name"));
    // Wrong types, each with the place of the part that is wrong.
    for (source, start, end) in [
        ("input.count + input.price", 0, 25),
        ("input.price == 3", 0, 16),
        ("input.price / decimal(\"2\")", 0, 26),
        ("trim(input.count)", 5, 16),
        ("!input.name", 1, 11),
        ("input.name ? 1 : 2", 0, 10),
        ("input.count ? 1 : 2", 0, 11),
        ("true ? 1 : \"a\"", 11, 14),
        ("round_to(input.count, 2, \"half_even\")", 9, 20),
        ("decimal(input.name)", 8, 18),
        ("input.name < 3", 0, 14),
        ("[1, \"a\"]", 4, 7),
        ("source.rows.all(r, r.qty)", 19, 24),
        ("input.name.all(c, true)", 0, 10),
        ("input.count.name", 0, 16),
        ("input.name[0]", 0, 10),
        ("size(input.count)", 5, 16),
        ("null + 1", 0, 8),
        ("coalesce(input.note, 3)", 21, 22),
        ("format_date(input.seen, \"YYYY-MM-DD\")", 12, 22),
    ] {
        let error = refused(source);
        assert_eq!(error.code, EXPRESSION_TYPE_ERROR, "{source}");
        assert_eq!(error.at, Span::new(start, end), "{source}");
    }
    assert!(
        refused("input.count + input.price")
            .fix
            .unwrap()
            .contains("decimal(")
    );
    assert!(
        refused("input.price / decimal(\"2\")")
            .fix
            .unwrap()
            .contains("div(")
    );
    assert!(
        refused("decimal(input.name)")
            .fix
            .unwrap()
            .contains("parse_decimal(")
    );
    // A rule is a literal the library knows, checked when compiled.
    for source in [
        "round_to(input.price, 2, \"nearest\")",
        "round_to(input.price, 29, \"half_even\")",
        "round_to(input.price, input.count, \"half_even\")",
        "to_unit(input.price, \"in\", \"kg\", 2, \"half_even\")",
        "to_unit(input.price, \"in\", \"cubit\", 2, \"half_even\")",
        "parse_date(input.name, \"YY-MM-DD\")",
        "parse_date(input.name, input.name)",
        "parse_datetime(input.name, \"YYYY-MM-DD HH:mm\", \"Europe/Paris\")",
        "parse_decimal(input.name, 2, \"half_even\", \";\")",
        "code_for(input.name, {\"A\": \"Active\", \"B\": \"Active\"})",
        "code_for(input.name, {})",
        "code_for(input.name, {\"A\": \"B\", \"B\": \"C\"})",
        "as_choice(input.name, {\"A\": \"Open\", \"Open\": \"Closed\"})",
        "code_for(input.name, {\"A\": \"x\", \"A \": \"y\"})",
        "parse_date(input.name, \"YYYYMD\")",
        "excel_serial_to_date(input.count, \"1905\")",
        "decimal(\"1,5\")",
        "nosuch(1)",
        "input.name.trim()",
        "trim()",
        "trim(input.name, input.name)",
        "source.rows.all(row, true)",
        "has(input.nmae)",
    ] {
        let error = refused(source);
        assert!(
            error.code == EXPRESSION_INVALID || error.code == EXPRESSION_TYPE_ERROR,
            "{source}"
        );
        assert!(error.at.end > error.at.start, "{source}");
    }
    // The place's type is part of the check.
    let wrong = compile("input.name", &env(), Some(&ValueType::Decimal)).unwrap_err();
    assert_eq!(wrong.code, EXPRESSION_TYPE_ERROR);
    let right = compile("null", &env(), Some(&ValueType::Decimal)).unwrap();
    assert_eq!(right.result(), &Checked::Known(ValueType::Decimal));
}

#[test]
fn an_unknown_type_compiles_as_a_warning_and_cannot_reach_a_plan() {
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    env.declare_unknown("row").unwrap();
    let draft = compile("row.qty", &env, None).unwrap();
    assert_eq!(draft.result(), &Checked::Dyn);
    assert_eq!(
        draft.to_plan_expr().unwrap_err().code,
        EXPRESSION_TYPE_ERROR
    );
    // Once a function states the type, the result is known.
    let typed = compile("parse_decimal(row.qty, 2, \"half_even\")", &env, None).unwrap();
    assert_eq!(typed.result(), &Checked::Known(ValueType::Decimal));
    for (name, _) in [
        ("1x", 0),
        ("true", 0),
        ("trim", 0),
        ("row", 0),
        ("a-b", 0),
        ("", 0),
    ] {
        assert_eq!(
            env.declare(name, ValueType::Text).unwrap_err().code,
            EXPRESSION_INVALID,
            "{name}"
        );
    }
}

/// One case for every host function, and the cases of the language itself.
/// The answers are compared as canonical bytes, which are the same on every
/// machine.
const CASES: &[(&str, &str, &str)] = &[
    // function or feature, expression, canonical answer
    (
        "decimal",
        "decimal(\"0.1\") + decimal(\"0.2\") == decimal(\"0.3\")",
        "true",
    ),
    (
        "decimal",
        "decimal(input.count) * input.price",
        "{\"decimal\":\"97.6875\"}",
    ),
    (
        "decimal",
        "-decimal(\"1.50\") - decimal(\"0.5\")",
        "{\"decimal\":\"-2\"}",
    ),
    (
        "decimal",
        "decimal(\"1.0\") == decimal(\"1.00\") && decimal(\"2\") > decimal(\"1.99\")",
        "true",
    ),
    (
        "parse_decimal",
        "parse_decimal(\"1,234.565\", 2, \"half_even\")",
        "{\"decimal\":\"1234.56\"}",
    ),
    (
        "parse_decimal",
        "parse_decimal(\"1.234,565\", 2, \"half_up\", \",\")",
        "{\"decimal\":\"1234.57\"}",
    ),
    ("parse_int", "parse_int(\"1,234\") + input.count", "1237"),
    (
        "parse_money",
        "parse_money(\"$1,234.565\", \"USD\", 2, \"half_even\")",
        "{\"decimal\":\"1234.56\"}",
    ),
    (
        "parse_bool",
        "parse_bool(\" YES \", \"Yes\", \"No\")",
        "true",
    ),
    (
        "round_to",
        "round_to(input.price, 2, \"half_even\")",
        "{\"decimal\":\"32.56\"}",
    ),
    (
        "div",
        "div(decimal(\"2\"), decimal(\"3\"), 4, \"half_up\")",
        "{\"decimal\":\"0.6667\"}",
    ),
    (
        "to_inches",
        "to_inches(input.width)",
        "{\"decimal\":\"90.5\"}",
    ),
    (
        "to_feet_inches",
        "to_feet_inches(decimal(\"90.53\"), decimal(\"0.0625\"))",
        "\"7' 6 1/2\\\"\"",
    ),
    (
        "to_unit",
        "to_unit(to_inches(input.width), \"in\", \"mm\", 1, \"half_even\")",
        "{\"decimal\":\"2298.7\"}",
    ),
    (
        "snap_to_step",
        "snap_to_step(decimal(\"32.56\"), decimal(\"0.0625\"), \"half_even\")",
        "{\"decimal\":\"32.5625\"}",
    ),
    (
        "parse_date",
        "parse_date(\"11/17/2026\", \"MM/DD/YYYY\") == input.due",
        "true",
    ),
    (
        "format_date",
        "format_date(input.due, \"DD MMM YYYY\")",
        "\"17 Nov 2026\"",
    ),
    (
        "parse_datetime",
        "parse_datetime(\"2026-11-17 14:00\", \"YYYY-MM-DD HH:mm\", \"+02:00\") < input.seen",
        "true",
    ),
    (
        "format_datetime",
        "format_datetime(input.seen, \"YYYY-MM-DD'T'HH:mm:ss.SSS\", \"-05:30\")",
        "\"2026-11-17T06:30:00.250\"",
    ),
    (
        "parse_time",
        "parse_time(\"1:05 PM\", \"h:mm A\")",
        "{\"duration\":47100000}",
    ),
    (
        "excel_serial_to_date",
        "excel_serial_to_date(46343, \"1900\")",
        "{\"date\":\"2026-11-17\"}",
    ),
    ("trim", "trim(input.name)", "\"Caf\u{e9}\u{a0} Table\""),
    ("lower", "lower(\"ABC\")", "\"abc\""),
    ("upper", "upper(\"abc\")", "\"ABC\""),
    ("nfc", "nfc(\"cafe\\u0301\") == \"caf\\u00E9\"", "true"),
    (
        "collapse_space",
        "collapse_space(input.name)",
        "\"Caf\u{e9} Table\"",
    ),
    (
        "code_for",
        "code_for(\"Active\", {\"A\": \"Active\", \"I\": \"Inactive\"}) == input.status",
        "true",
    ),
    (
        "label_for",
        "label_for(input.status, {\"A\": \"Active\", \"I\": \"Inactive\"})",
        "\"Active\"",
    ),
    ("coalesce", "coalesce(input.note, \"none\")", "\"none\""),
    (
        "coalesce",
        "coalesce(input.name, trim(input.note))",
        "\"  Caf\u{e9}\u{a0} Table \"",
    ),
    ("size", "size(source.rows) + input.name.size()", "17"),
    (
        "as_text",
        "as_text(input.name, \"insensitive\", \"collapse\")",
        "\"caf\u{e9} table\"",
    ),
    ("as_integer", "as_integer(\"1,234\", \".\")", "1234"),
    (
        "as_decimal",
        "as_decimal(input.price, 2, \"half_even\", \".\")",
        "{\"decimal\":\"32.56\"}",
    ),
    (
        "as_money",
        "as_money(\"USD 5\", \"USD\", 2, \"half_even\", \".\")",
        "{\"decimal\":\"5\"}",
    ),
    (
        "as_measurement",
        "as_measurement(input.width, \"ft_in\", \"in\", decimal(\"0.0625\"), \"half_even\", \".\")",
        "{\"decimal\":\"90.5\"}",
    ),
    (
        "as_date",
        "as_date(\"17.11.2026\", \"D.M.YYYY\")",
        "{\"date\":\"2026-11-17\"}",
    ),
    (
        "as_datetime",
        "as_datetime(\"2026-11-17 12:00\", \"YYYY-MM-DD HH:mm\", \"UTC\")",
        "{\"instant\":1794916800000}",
    ),
    (
        "as_time",
        "as_time(\"13:05\", \"HH:mm\")",
        "{\"duration\":47100000}",
    ),
    ("as_boolean", "as_boolean(\"no\", \"Yes\", \"No\")", "false"),
    (
        "as_choice",
        "as_choice(\"Inactive\", {\"A\": \"Active\", \"I\": \"Inactive\"})",
        "\"I\"",
    ),
    // The language.
    ("", "input.count * 2 - 7 / 2 + 7 % 4", "6"),
    ("", "-input.count", "-3"),
    ("", "input.name + \"!\"", "\"  Caf\u{e9}\u{a0} Table !\""),
    ("", "input.count > 2 ? \"many\" : \"few\"", "\"many\""),
    (
        "",
        "!(input.count > 2) || input.status in [\"A\", \"B\"]",
        "true",
    ),
    ("", "has(input.note)", "false"),
    ("", "has(input.name) && \"name\" in input", "true"),
    ("", "input.note == null", "true"),
    ("", "input[\"count\"]", "3"),
    (
        "",
        "row.code.startsWith(\"K-\") && row.code.endsWith(\"1\") && row.code.contains(\"-\")",
        "true",
    ),
    ("", "source.rows[2].qty", "{\"decimal\":\"4\"}"),
    (
        "",
        "source.rows.filter(r, r.qty > decimal(\"0\")).map(r, r.code)",
        "[\"K-1\",\"K-3\"]",
    ),
    ("", "source.rows.all(r, r.qty >= decimal(\"0\"))", "true"),
    ("", "source.rows.exists(r, r.code == \"K-2\")", "true"),
    (
        "",
        "source.rows.exists_one(r, r.qty > decimal(\"1\"))",
        "false",
    ),
    (
        "",
        "source.rows.map(r, source.rows.filter(s, s.qty <= r.qty).size())",
        "[2,1,3]",
    ),
    (
        "",
        "steps.find.result.total == row.qty + decimal(\"4\")",
        "true",
    ),
    (
        "",
        "{\"id\": steps.find.result.id, \"n\": [1, 2] + [3]}",
        "{\"record\":{\"id\":\"R-77\",\"n\":[1,2,3]}}",
    ),
    (
        "",
        "[input.due] == [parse_date(\"2026-11-17\", \"YYYY-MM-DD\")]",
        "true",
    ),
    ("", "false && trim(input.note) == \"\"", "false"),
];

#[test]
fn evaluation_gives_the_same_bytes_every_time() {
    for (_, source, expected) in CASES {
        let first = run(source, None).unwrap_or_else(|failure| panic!("{source}: {failure}"));
        let second = run(source, None).unwrap();
        assert_eq!(
            String::from_utf8(first.canonical_bytes()).unwrap(),
            *expected,
            "{source}"
        );
        assert_eq!(
            first.canonical_bytes(),
            second.canonical_bytes(),
            "{source}"
        );
    }
}

#[test]
fn every_library_function_has_a_case_and_a_page() {
    let tested: BTreeSet<&str> = CASES.iter().map(|(name, _, _)| *name).collect();
    let mut seen = BTreeSet::new();
    for function in library().functions() {
        let name = function.name().as_str();
        assert!(tested.contains(name), "no case for `{name}`");
        assert!(seen.insert(name), "`{name}` is listed twice");
        assert!(function.summary().ends_with('.'), "{name}");
        assert!(
            function.signature().starts_with(&format!("{name}(")),
            "{name}"
        );
        assert!(!function.group().is_empty(), "{name}");
        assert_eq!(library().get(name).unwrap().name(), function.name());
    }
    assert_eq!(library().version(), LibraryVersion::CURRENT);
    assert_eq!(
        library().get("parse_decimal").unwrap().signature(),
        "parse_decimal(text, places, rounding[, mark]) -> decimal"
    );
    // The starting library of the file formats is all here.
    for name in [
        "to_inches",
        "to_feet_inches",
        "to_unit",
        "parse_decimal",
        "round_to",
        "parse_date",
        "format_date",
        "trim",
        "lower",
        "upper",
        "code_for",
        "label_for",
        "coalesce",
    ] {
        assert!(library().get(name).is_some(), "{name}");
    }
}

#[test]
fn decimals_are_exact_where_a_float_is_not() {
    assert_eq!(
        run(
            "decimal(\"0.1\") + decimal(\"0.2\") == decimal(\"0.3\")",
            None
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        run("decimal(\"0.1\") + decimal(\"0.2\")", None).unwrap(),
        Value::Decimal(d("0.3"))
    );
    assert_eq!(
        run(
            "decimal(\"0.1\") * decimal(\"3\") != decimal(\"0.3\")",
            None
        )
        .unwrap(),
        Value::Bool(false)
    );
}

fn normalizer(rule: &FieldRule) -> Normalizer {
    let compiled = ok(rule.normalizer());
    assert_eq!(compiled.names(), ["value".to_owned()]);
    ok(Normalizer::load(&ok(compiled.to_plan_expr())))
}

#[test]
fn a_value_the_system_rounds_or_reformats_compares_equal_and_another_does_not() {
    let even = Rounding::HalfEven;
    let point = DecimalMark::Point;
    // What the caller sent, what the system showed, and a different value.
    let cases: Vec<(FieldRule, Value, Value, Value)> = vec![
        (
            FieldRule::Decimal {
                places: 2,
                rounding: even,
                mark: point,
            },
            Value::Decimal(d("32.5625")),
            Value::from("32.56"),
            Value::from("32.57"),
        ),
        (
            FieldRule::Decimal {
                places: 2,
                rounding: even,
                mark: DecimalMark::Comma,
            },
            Value::Decimal(d("1234.5")),
            Value::from("1.234,50"),
            Value::from("1.234,05"),
        ),
        (
            FieldRule::Money {
                currency: "USD".into(),
                places: None,
                rounding: even,
                mark: point,
            },
            Value::Decimal(d("1234.5")),
            Value::from("$1,234.50"),
            Value::from("$1,234.51"),
        ),
        (
            FieldRule::Money {
                currency: "JPY".into(),
                places: None,
                rounding: even,
                mark: point,
            },
            Value::Decimal(d("1200.4")),
            Value::from("\u{a5}1,200"),
            Value::from("\u{a5}1,201"),
        ),
        (
            FieldRule::Measurement {
                unit: "in".into(),
                display_unit: "ft_in".into(),
                step: d("0.0625"),
                rounding: even,
                mark: point,
            },
            Value::Decimal(d("90.53")),
            Value::from("7' 6 1/2\""),
            Value::from("7' 6 9/16\""),
        ),
        (
            FieldRule::Measurement {
                unit: "in".into(),
                display_unit: "mm".into(),
                step: d("0.0625"),
                rounding: even,
                mark: point,
            },
            Value::Decimal(d("32.5625")),
            Value::from("827.1 mm"),
            Value::from("829 mm"),
        ),
        (
            FieldRule::Integer { mark: point },
            Value::Int(1234),
            Value::from("1,234"),
            Value::from("1,235"),
        ),
        (
            FieldRule::Text {
                case_sensitive: false,
                whitespace: Whitespace::Collapse,
            },
            Value::from("Cafe\u{301}  Table"),
            Value::from(" CAF\u{c9}\u{a0}TABLE "),
            Value::from("Cafe Table"),
        ),
        // A system that shows text in capitals turns one letter into two.
        (
            FieldRule::Text {
                case_sensitive: false,
                whitespace: Whitespace::Trim,
            },
            Value::from("Stra\u{df}e 5"),
            Value::from("STRASSE 5"),
            Value::from("STRASSE 6"),
        ),
        (
            FieldRule::Text {
                case_sensitive: true,
                whitespace: Whitespace::Exact,
            },
            Value::from("Cafe\u{301}"),
            Value::from("Caf\u{e9}"),
            Value::from("caf\u{e9}"),
        ),
        (
            FieldRule::Date {
                format: "MM/DD/YYYY".into(),
            },
            Value::Date(Date::new(2026, 11, 17).unwrap()),
            Value::from("11/17/2026"),
            Value::from("11/18/2026"),
        ),
        (
            FieldRule::Datetime {
                format: "M/D/YYYY h:mm A".into(),
                zone: "-05:00".into(),
            },
            Value::Instant(Timestamp::from_unix_millis(1_794_916_800_000)),
            Value::from("11/17/2026 7:00 AM"),
            Value::from("11/17/2026 7:00 PM"),
        ),
        (
            FieldRule::Time {
                format: "HH:mm".into(),
            },
            Value::Duration(Millis::new(47_100_000)),
            Value::from("13:05"),
            Value::from("13:06"),
        ),
        (
            FieldRule::Boolean {
                true_text: "Yes".into(),
                false_text: "No".into(),
            },
            Value::Bool(true),
            Value::from("YES"),
            Value::from("No"),
        ),
        (
            FieldRule::Choice {
                codes: vec![
                    CodeLabel {
                        code: "A".into(),
                        label: "Active".into(),
                    },
                    CodeLabel {
                        code: "I".into(),
                        label: "Inactive".into(),
                    },
                ],
            },
            Value::from("A"),
            Value::from("Active"),
            Value::from("Inactive"),
        ),
    ];
    let mut bases = BTreeSet::new();
    for (rule, sent, shown, other) in &cases {
        bases.insert(rule.base().as_str());
        let normalizer = normalizer(rule);
        assert_eq!(normalizer.value_type(), &rule.value_type());
        let sent = normalizer.normalize(sent).unwrap();
        let shown_normal = normalizer.normalize(shown).unwrap();
        let other = normalizer.normalize(other).unwrap();
        assert!(sent.agrees_with(&shown_normal), "{rule:?}");
        assert!(!sent.agrees_with(&other), "{rule:?}");
        // Both the raw and the normalized value are kept.
        assert_eq!(&shown_normal.raw, shown);
        // Normalizing a normalized value changes nothing.
        let again = normalizer.normalize(&sent.normalized).unwrap();
        assert_eq!(again.normalized, sent.normalized, "{rule:?}");
        // An absent value stays absent.
        assert_eq!(
            normalizer.normalize(&Value::Null).unwrap().normalized,
            Value::Null
        );
    }
    // Every field-type base has a rule.
    assert_eq!(bases.len(), a2m_core::FieldBase::ALL.len());
    // The report of section 210: 32.5625 was rounded to 32.56.
    let decimal = normalizer(&FieldRule::Decimal {
        places: 2,
        rounding: even,
        mark: point,
    });
    let sent = decimal.normalize(&Value::Decimal(d("32.5625"))).unwrap();
    assert_eq!(sent.raw, Value::Decimal(d("32.5625")));
    assert_eq!(sent.normalized, Value::Decimal(d("32.56")));
}

#[test]
fn a_rule_the_library_does_not_know_is_refused_when_the_field_type_compiles() {
    let even = Rounding::HalfEven;
    let point = DecimalMark::Point;
    for rule in [
        FieldRule::Decimal {
            places: 29,
            rounding: even,
            mark: point,
        },
        FieldRule::Money {
            currency: "usd".into(),
            places: None,
            rounding: even,
            mark: point,
        },
        FieldRule::Measurement {
            unit: "in".into(),
            display_unit: "kg".into(),
            step: d("1"),
            rounding: even,
            mark: point,
        },
        FieldRule::Measurement {
            unit: "in".into(),
            display_unit: "ft_in".into(),
            step: d("0"),
            rounding: even,
            mark: point,
        },
        FieldRule::Measurement {
            unit: "kg".into(),
            display_unit: "ft_in".into(),
            step: d("1"),
            rounding: even,
            mark: point,
        },
        FieldRule::Date {
            format: "YYYY-MM".into(),
        },
        FieldRule::Datetime {
            format: "YYYY-MM-DD HH:mm".into(),
            zone: "Europe/Paris".into(),
        },
        FieldRule::Time {
            format: "HH".into(),
        },
        FieldRule::Boolean {
            true_text: "Yes".into(),
            false_text: "yes".into(),
        },
        FieldRule::Choice { codes: vec![] },
        FieldRule::Choice {
            codes: vec![
                CodeLabel {
                    code: "A".into(),
                    label: "Active".into(),
                },
                CodeLabel {
                    code: "A".into(),
                    label: "Other".into(),
                },
            ],
        },
        // A code that is another choice's label: a value read back could
        // be either choice.
        FieldRule::Choice {
            codes: vec![
                CodeLabel {
                    code: "A".into(),
                    label: "B".into(),
                },
                CodeLabel {
                    code: "B".into(),
                    label: "C".into(),
                },
            ],
        },
        FieldRule::Choice {
            codes: vec![
                CodeLabel {
                    code: "open".into(),
                    label: "Open".into(),
                },
                CodeLabel {
                    code: "Open".into(),
                    label: "Closed".into(),
                },
            ],
        },
        // A date whose month and day run together.
        FieldRule::Date {
            format: "YYYYMD".into(),
        },
    ] {
        assert_eq!(
            rule.normalizer().unwrap_err().code,
            EXPRESSION_INVALID,
            "{rule:?}"
        );
    }
}

/// The route of a failed write reported as verified: the caller writes the
/// code `B`, the write fails, the record still holds `A`, and the system
/// shows `A` by its label, which is `B`.
#[test]
fn a_choice_read_back_never_stands_for_two_choices() {
    let pair = |code: &str, label: &str| CodeLabel {
        code: code.into(),
        label: label.into(),
    };
    let ambiguous = FieldRule::Choice {
        codes: vec![pair("A", "B"), pair("B", "C")],
    };
    let refusal = ambiguous.normalizer().unwrap_err();
    assert_eq!(refusal.code, EXPRESSION_INVALID);
    assert!(
        refusal.to_string().contains("label of another choice"),
        "{refusal}"
    );

    // A plan the compiler did not make cannot carry the list either: the
    // text is canonical, and the load checks the rule again.
    let stored = PlanExpr {
        source: "as_choice(value, {\"A\": \"B\", \"B\": \"C\"})".into(),
        result: ValueType::Choice,
        library: LibraryVersion::CURRENT.get(),
    };
    let failure = Normalizer::load(&stored).unwrap_err();
    assert_eq!(failure.code(), &PLAN_EXPRESSION_UNLOADABLE);

    // A code may be its own label, and codes may differ from labels in
    // case: neither makes one text stand for two choices.
    let plain = normalizer(&FieldRule::Choice {
        codes: vec![pair("A", "A"), pair("b", "Bee"), pair("C", "c")],
    });
    let code = |text: &str| plain.normalize(&Value::from(text)).unwrap().normalized;
    assert_eq!(code("A"), Value::from("A"));
    assert_eq!(code("Bee"), Value::from("b"));
    assert_eq!(code("c"), Value::from("C"));
    // The intended value and a stale one never agree.
    let intended = plain.normalize(&Value::from("b")).unwrap();
    for stale in ["A", "C", "c"] {
        let read = plain.normalize(&Value::from(stale)).unwrap();
        assert!(!intended.agrees_with(&read), "{stale}");
    }
    assert!(intended.agrees_with(&plain.normalize(&Value::from(" Bee ")).unwrap()));
}

#[test]
fn a_value_a_rule_cannot_read_stops_and_is_never_guessed() {
    let even = Rounding::HalfEven;
    let point = DecimalMark::Point;
    let cases = [
        (
            FieldRule::Decimal {
                places: 2,
                rounding: even,
                mark: point,
            },
            Value::from("1,23"),
        ),
        (
            FieldRule::Decimal {
                places: 2,
                rounding: even,
                mark: point,
            },
            Value::Bool(true),
        ),
        (
            FieldRule::Money {
                currency: "USD".into(),
                places: None,
                rounding: even,
                mark: point,
            },
            Value::from("EUR 5.00"),
        ),
        // Another currency's symbol is another value.
        (
            FieldRule::Money {
                currency: "EUR".into(),
                places: None,
                rounding: even,
                mark: point,
            },
            Value::from("\u{a3}5.00"),
        ),
        // A whole number of money could be units or cents.
        (
            FieldRule::Money {
                currency: "USD".into(),
                places: None,
                rounding: even,
                mark: point,
            },
            Value::Int(5),
        ),
        (
            FieldRule::Date {
                format: "MM/DD/YYYY".into(),
            },
            Value::from("17/11/2026"),
        ),
        (
            FieldRule::Boolean {
                true_text: "Yes".into(),
                false_text: "No".into(),
            },
            Value::from("Maybe"),
        ),
        (
            FieldRule::Choice {
                codes: vec![CodeLabel {
                    code: "A".into(),
                    label: "Active".into(),
                }],
            },
            Value::from("Archived"),
        ),
        (
            FieldRule::Measurement {
                unit: "in".into(),
                display_unit: "ft_in".into(),
                step: d("0.0625"),
                rounding: even,
                mark: point,
            },
            Value::from("90"),
        ),
        (FieldRule::Integer { mark: point }, Value::Decimal(d("1.5"))),
    ];
    for (rule, raw) in cases {
        let failure = normalizer(&rule).normalize(&raw).unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input, "{rule:?}");
        assert_eq!(failure.code(), &VALUE_UNPARSABLE, "{rule:?}");
    }
}

#[test]
fn the_cost_limit_stops_a_runaway_with_a_named_failure() {
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    env.declare("rows", ValueType::list_of(ValueType::Int))
        .unwrap();
    let rows = Value::List((0..300).map(Value::Int).collect());
    let bindings = Bindings::new().with("rows", &rows);
    let load = |source: &str| {
        let compiled = compile(source, &env, None).unwrap();
        Program::load(&compiled.to_plan_expr().unwrap()).unwrap()
    };
    // 300 cubed is 27 million steps: far past the limit.
    let runaway = load("rows.all(a, rows.all(b, rows.all(c, a + b + c >= 0)))");
    let failure = runaway.evaluate(&bindings).unwrap_err();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code(), &EXPRESSION_COST_EXCEEDED);
    // The same work over one level is well within it, and gives its answer.
    let bounded = load("rows.all(a, a >= 0)");
    assert_eq!(bounded.evaluate(&bindings).unwrap(), Value::Bool(true));
    // The limit is one number, the library's.
    assert_eq!(library().cost_limit(), COST_LIMIT);
    assert_eq!(COST_LIMIT, 1_000_000);
    // Long text costs more than short text: a function over 64 MiB of text
    // cannot be had for one unit.
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    env.declare("text", ValueType::Text).unwrap();
    let text = Value::Text("a ".repeat(40_000_000));
    let compiled = compile("collapse_space(text)", &env, None).unwrap();
    let program = Program::load(&compiled.to_plan_expr().unwrap()).unwrap();
    let failure = program
        .evaluate(&Bindings::new().with("text", &text))
        .unwrap_err();
    assert_eq!(failure.code(), &EXPRESSION_COST_EXCEEDED);
}

#[test]
fn every_error_is_a_failure_of_class_input_with_its_own_code() {
    // Compile time.
    let invalid: Failure = refused("trim(").into();
    assert_eq!(
        (invalid.class(), invalid.code()),
        (FailureClass::Input, &EXPRESSION_INVALID)
    );
    let typed: Failure = refused("1.5").into();
    assert_eq!(
        (typed.class(), typed.code()),
        (FailureClass::Input, &EXPRESSION_TYPE_ERROR)
    );
    assert_eq!(
        typed.next_step(),
        "Write decimal(\"1.5\") for an exact decimal."
    );
    assert!(typed.message().contains("bytes 0 to 3"));
    // Run time.
    for (source, code) in [
        (
            "parse_decimal(input.name, 2, \"half_even\")",
            &VALUE_UNPARSABLE,
        ),
        ("parse_date(input.name, \"YYYY-MM-DD\")", &VALUE_UNPARSABLE),
        (
            "code_for(input.name, {\"A\": \"Active\"})",
            &VALUE_UNPARSABLE,
        ),
        ("to_inches(input.name)", &VALUE_UNPARSABLE),
        (
            "div(input.price, decimal(\"0\"), 2, \"half_even\")",
            &VALUE_OUT_OF_RANGE,
        ),
        ("input.count / (input.count - 3)", &VALUE_OUT_OF_RANGE),
        ("9223372036854775807 + input.count", &VALUE_OUT_OF_RANGE),
        ("source.rows[3].code", &VALUE_OUT_OF_RANGE),
        ("excel_serial_to_date(60, \"1900\")", &VALUE_OUT_OF_RANGE),
        (
            "decimal(\"79228162514264337593543950335\") * input.price",
            &VALUE_OUT_OF_RANGE,
        ),
        ("trim(input.note)", &VALUE_ABSENT),
        ("input.note + \"x\"", &VALUE_ABSENT),
        ("input.note.startsWith(\"x\")", &VALUE_ABSENT),
    ] {
        let failure = run(source, None).unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input, "{source}");
        assert_eq!(failure.code(), code, "{source}");
        assert!(!failure.next_step().is_empty(), "{source}");
    }
    // An absent value is an answer, not a failure, until something needs it.
    assert_eq!(
        run("input.note", Some(&ValueType::Text)).unwrap(),
        Value::Null
    );
}

#[test]
fn a_failure_never_carries_a_bound_value() {
    const MARKER: &str = "ZQ7-MARKER-VALUE";
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    env.declare("text", ValueType::Text).unwrap();
    env.declare_unknown("any").unwrap();
    let text = Value::from(MARKER);
    let any = Value::List(vec![Value::from(MARKER)]);
    let bindings = Bindings::new().with("text", &text).with("any", &any);
    for source in [
        "parse_decimal(text, 2, \"half_even\")",
        "parse_int(text)",
        "parse_money(text, \"USD\", 2, \"half_even\")",
        "parse_bool(text, \"Yes\", \"No\")",
        "parse_date(text, \"YYYY-MM-DD\")",
        "parse_datetime(text, \"YYYY-MM-DD HH:mm\", \"UTC\")",
        "parse_time(text, \"HH:mm\")",
        "to_inches(text)",
        "code_for(text, {\"A\": \"Active\"})",
        "label_for(text, {\"A\": \"Active\"})",
        "as_decimal(text, 2, \"half_even\", \".\")",
        "as_date(any, \"YYYY-MM-DD\")",
        "as_choice(text, {\"A\": \"Active\"})",
        "any + 1",
        "decimal(any)",
        "any.name",
        "any[5]",
        "trim(any)",
        "any ? 1 : 2",
        "any == text",
    ] {
        let compiled = compile(source, &env, None).unwrap();
        let plan = PlanExpr {
            source: compiled.source().to_owned(),
            result: match compiled.result() {
                Checked::Known(result) => result.clone(),
                Checked::Dyn => ValueType::Text,
            },
            library: 1,
        };
        let failure = Program::load(&plan)
            .unwrap()
            .evaluate(&bindings)
            .unwrap_err();
        let shown = format!("{failure} {} {failure:?}", failure.next_step());
        assert!(!shown.contains(MARKER), "{source}: {shown}");
        assert!(!shown.contains("ZQ7"), "{source}: {shown}");
    }
}

#[test]
fn a_plan_expression_loads_only_when_it_is_what_the_compiler_wrote() {
    let good = compile("trim(input.name)", &env(), None)
        .unwrap()
        .to_plan_expr()
        .unwrap();
    let program = Program::load(&good).unwrap();
    assert_eq!(program.names(), ["input".to_owned()]);
    assert_eq!(program.result_type(), &ValueType::Text);
    let broken = [
        // A library this engine does not carry.
        PlanExpr {
            library: 2,
            ..good.clone()
        },
        // Text that is not canonical, or not an expression, or holds a float.
        PlanExpr {
            source: "trim( input.name )".into(),
            ..good.clone()
        },
        PlanExpr {
            source: "trim(".into(),
            ..good.clone()
        },
        PlanExpr {
            source: "1.5".into(),
            ..good.clone()
        },
        // A function this engine does not have.
        PlanExpr {
            source: "squash(input.name)".into(),
            ..good.clone()
        },
        // A type the text does not give.
        PlanExpr {
            result: ValueType::Decimal,
            ..good.clone()
        },
    ];
    for plan in broken {
        let failure = Program::load(&plan).unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input, "{plan:?}");
        assert_eq!(failure.code(), &PLAN_EXPRESSION_UNLOADABLE, "{plan:?}");
    }
    // A name the program reads must be bound.
    let failure = program.evaluate(&Bindings::new()).unwrap_err();
    assert_eq!(failure.code(), &PLAN_EXPRESSION_UNLOADABLE);
    // Text from a value never becomes a decimal through `decimal`, even when
    // the checker could not see its type.
    let mut open = TypeEnv::new(LibraryVersion::CURRENT);
    open.declare_unknown("any").unwrap();
    let sneaky = compile("decimal(any)", &open, None)
        .unwrap()
        .to_plan_expr()
        .unwrap();
    let text = Value::from("1.5");
    let failure = Program::load(&sneaky)
        .unwrap()
        .evaluate(&Bindings::new().with("any", &text))
        .unwrap_err();
    assert_eq!(failure.code(), &VALUE_UNPARSABLE);
    // A value of another kind than the name declared stops; it is not reinterpreted.
    let wrong = Value::Record(Values::new().with("name", 7i64));
    let failure = program
        .evaluate(&Bindings::new().with("input", &wrong))
        .unwrap_err();
    assert_eq!(failure.code(), &VALUE_UNPARSABLE);
    // A normalizer reads only the raw value.
    assert_eq!(
        Normalizer::load(&good).unwrap_err().code(),
        &PLAN_EXPRESSION_UNLOADABLE
    );
}

#[test]
fn the_parse_functions_read_a_source_as_the_host_functions_do() {
    use a2m_expr::{DateSystem, parse};
    assert_eq!(
        parse::decimal("1,234.50", DecimalMark::Point).unwrap(),
        d("1234.5")
    );
    assert_eq!(parse::integer("1.234", DecimalMark::Comma).unwrap(), 1234);
    assert_eq!(
        parse::money("5,00 EUR", "EUR", DecimalMark::Comma).unwrap(),
        d("5")
    );
    assert_eq!(parse::feet_inches("7' 6 1/2\"").unwrap(), d("90.5"));
    assert_eq!(
        parse::date("17.11.2026", "D.M.YYYY").unwrap(),
        Date::new(2026, 11, 17).unwrap()
    );
    assert_eq!(
        parse::excel_serial(&d("44881"), DateSystem::From1904).unwrap(),
        Date::new(2026, 11, 17).unwrap()
    );
    assert_eq!(
        parse::decimal("abc", DecimalMark::Point)
            .unwrap_err()
            .code(),
        &VALUE_UNPARSABLE
    );
    assert_eq!(
        parse::date("x", "YYYY").unwrap_err().code(),
        &EXPRESSION_INVALID
    );
    assert_eq!(
        parse::excel_serial(&d("60"), DateSystem::From1900)
            .unwrap_err()
            .code(),
        &VALUE_OUT_OF_RANGE
    );
}

/// A small seeded source of numbers (SplitMix64), so that a property test
/// draws the same values on every machine and a failure names its seed.
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A whole number from `0` to `below - 1`.
    fn below(&mut self, below: u64) -> u64 {
        self.next() % below
    }

    fn int(&mut self, below: u64) -> i64 {
        i64::try_from(self.below(below)).unwrap()
    }
}

/// A whole number written with a comma between each group of three
/// digits, as a system shows it.
fn grouped(digits: &str, mark: char) -> String {
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(mark);
        }
        out.push(digit);
    }
    out
}

/// A decimal as a system shows it, with `places` places, its whole part
/// grouped, and the given decimal mark.
fn shown_decimal(value: &Decimal, places: u32, mark: DecimalMark) -> String {
    let text = value.to_places(places, Rounding::HalfEven);
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let (group, point) = match mark {
        DecimalMark::Point => (',', '.'),
        DecimalMark::Comma => ('.', ','),
    };
    let mut out = grouped(whole, group);
    if !fraction.is_empty() {
        out.push(point);
        out.push_str(fraction);
    }
    out
}

/// Inches, a whole number of sixteenths, as a system shows them in feet
/// and inches: `7' 6 1/2"`.
fn shown_feet_inches(inches: &Decimal) -> String {
    let sixteenths = ok(inches
        .checked_mul(&Decimal::from_i64(16))
        .and_then(|units| units.to_i64())
        .ok_or("not a whole number of sixteenths"));
    let (feet, rest) = (sixteenths / 192, sixteenths % 192);
    let (whole, mut over, mut part) = (rest / 16, 16, rest % 16);
    while part != 0 && part % 2 == 0 {
        part /= 2;
        over /= 2;
    }
    if part == 0 {
        format!("{feet}' {whole}\"")
    } else {
        format!("{feet}' {whole} {part}/{over}\"")
    }
}

/// Text as a system that shows capitals and pads spaces shows it.
fn shown_shouting(text: &str, draw: &mut Draw) -> String {
    let mut out = " ".repeat(usize::try_from(draw.below(3)).unwrap());
    for word in text.split(' ') {
        out.push_str(&word.to_uppercase());
        out.push_str(&" ".repeat(1 + usize::try_from(draw.below(3)).unwrap()));
    }
    out
}

/// For each field-type rule a system rounds or re-forms through, many
/// drawn values: normalizing a normalized value changes nothing, and the
/// value a caller sent and what the system shows of it, once it has
/// rounded it to what it can hold, agree after normalization.
#[test]
fn normalizing_is_idempotent_and_the_intended_value_agrees_with_its_read_back() {
    const DRAWS: u32 = 400;
    let even = Rounding::HalfEven;
    let decimal = FieldRule::Decimal {
        places: 2,
        rounding: even,
        mark: DecimalMark::Point,
    };
    let comma = FieldRule::Decimal {
        places: 3,
        rounding: even,
        mark: DecimalMark::Comma,
    };
    let money = FieldRule::Money {
        currency: "USD".into(),
        places: None,
        rounding: even,
        mark: DecimalMark::Point,
    };
    let feet_inches = FieldRule::Measurement {
        unit: "in".into(),
        display_unit: "ft_in".into(),
        step: d("0.0625"),
        rounding: Rounding::HalfUp,
        mark: DecimalMark::Point,
    };
    let millimetres = FieldRule::Measurement {
        unit: "in".into(),
        display_unit: "mm".into(),
        step: d("0.0625"),
        rounding: even,
        mark: DecimalMark::Point,
    };
    let integer = FieldRule::Integer {
        mark: DecimalMark::Point,
    };
    let text = FieldRule::Text {
        case_sensitive: false,
        whitespace: Whitespace::Collapse,
    };
    let rules = [
        &decimal,
        &comma,
        &money,
        &feet_inches,
        &millimetres,
        &integer,
        &text,
    ];
    let normalizers: Vec<Normalizer> = rules.iter().map(|rule| normalizer(rule)).collect();
    let words = ["rail", "bolt", "nut", "washer", "plate", "pin"];
    for seed in 0..u64::from(DRAWS) {
        let mut draw = Draw(seed);
        for (rule, normalizer) in rules.iter().zip(&normalizers) {
            // What a caller sends: an exact number with up to six places,
            // or text of a few words.
            let sent = match rule {
                FieldRule::Integer { .. } => Value::Int(draw.int(10_000_000)),
                FieldRule::Text { .. } => {
                    let count = 1 + draw.below(4);
                    let chosen: Vec<&str> = (0..count)
                        .map(|_| words[usize::try_from(draw.below(6)).unwrap()])
                        .collect();
                    Value::from(chosen.join(" ").as_str())
                }
                _ => {
                    let places = u32::try_from(draw.below(7)).unwrap();
                    Value::Decimal(ok(Decimal::from_units(draw.int(20_000_000), places)))
                }
            };
            let intended = normalizer
                .normalize(&sent)
                .unwrap_or_else(|error| panic!("seed {seed}, {rule:?}, {sent:?}: {error:?}"));
            // Normalizing what was normalized changes nothing.
            let again = ok(normalizer.normalize(&intended.normalized));
            assert_eq!(
                again.normalized, intended.normalized,
                "seed {seed}, {rule:?}"
            );
            // What the system shows of the value it stored, which is the
            // value rounded to what it can hold.
            let held = &intended.normalized;
            let shown = match (rule, held) {
                (FieldRule::Decimal { places, mark, .. }, Value::Decimal(number)) => {
                    shown_decimal(number, *places, *mark)
                }
                (FieldRule::Money { .. }, Value::Decimal(number)) => {
                    format!("${}", shown_decimal(number, 2, DecimalMark::Point))
                }
                (FieldRule::Measurement { display_unit, .. }, Value::Decimal(inches))
                    if display_unit == "ft_in" =>
                {
                    shown_feet_inches(inches)
                }
                (FieldRule::Measurement { .. }, Value::Decimal(inches)) => {
                    let mm = ok(inches.checked_mul(&d("25.4")).ok_or("too large"));
                    format!("{} mm", mm.to_places(1, Rounding::HalfEven))
                }
                (FieldRule::Integer { .. }, Value::Int(number)) => {
                    grouped(&number.to_string(), ',')
                }
                (FieldRule::Text { .. }, Value::Text(words)) => shown_shouting(words, &mut draw),
                _ => panic!("seed {seed}: {rule:?} normalized to {held:?}"),
            };
            let read_back = normalizer
                .normalize(&Value::from(shown.as_str()))
                .unwrap_or_else(|error| panic!("seed {seed}, {rule:?}, {shown}: {error:?}"));
            assert!(
                intended.agrees_with(&read_back),
                "seed {seed}, {rule:?}: sent {sent:?}, shown {shown:?}, {:?} against {:?}",
                intended.normalized,
                read_back.normalized
            );
            let again = ok(normalizer.normalize(&read_back.normalized));
            assert_eq!(
                again.normalized, read_back.normalized,
                "seed {seed}, {rule:?}"
            );
        }
    }
}

#[test]
fn a_loaded_program_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Program>();
    shared::<Normalizer>();
    shared::<a2m_expr::Compiled>();
    shared::<TypeEnv>();
}
