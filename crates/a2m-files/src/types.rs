//! Field types: the built-in ones, and those a target adds in `types.yaml`,
//! each compiled to a normalizer through `a2m-expr`.

use a2m_core::{
    CodeLabel, CompiledFieldType, Decimal, FieldBase, FieldFormat, Rounding, Sensitivity,
    WhitespaceRule, ZoneRule,
};
use a2m_expr::{DecimalMark, FieldRule, Whitespace};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Stage};
use crate::doc::{Doc, Located, Place};
use crate::schema::fix_for_unknown;
use crate::view::ExprTable;

/// Built-in field types that need more before a value can be normalized:
/// the name, and what a target must state.
pub(crate) const INCOMPLETE_BUILT_INS: &[(&str, &str)] = &[
    ("money", "a currency"),
    ("choice", "the stored codes and their labels"),
];

fn text(case_sensitive: bool, whitespace: Whitespace) -> FieldRule {
    FieldRule::Text {
        case_sensitive,
        whitespace,
    }
}

/// The built-in field types that are complete as they stand, in the order
/// the compiled target lists them.
pub(crate) fn built_ins() -> Vec<(&'static str, FieldRule)> {
    let sixteenth = Decimal::from_units(625, 4).unwrap_or_else(|_| Decimal::from_i64(1));
    vec![
        ("text", text(true, Whitespace::Collapse)),
        (
            "integer",
            FieldRule::Integer {
                mark: DecimalMark::Point,
            },
        ),
        (
            "decimal",
            FieldRule::Decimal {
                places: 6,
                rounding: Rounding::HalfEven,
                mark: DecimalMark::Point,
            },
        ),
        (
            "date",
            FieldRule::Date {
                format: "YYYY-MM-DD".to_owned(),
            },
        ),
        (
            "datetime",
            FieldRule::Datetime {
                format: "YYYY-MM-DD'T'HH:mm:ss".to_owned(),
                zone: "UTC".to_owned(),
            },
        ),
        (
            "time",
            FieldRule::Time {
                format: "HH:mm".to_owned(),
            },
        ),
        (
            "boolean",
            FieldRule::Boolean {
                true_text: "true".to_owned(),
                false_text: "false".to_owned(),
            },
        ),
        (
            "length_ft_in",
            FieldRule::Measurement {
                unit: "in".to_owned(),
                display_unit: a2m_expr::DISPLAY_FEET_INCHES.to_owned(),
                step: sixteenth,
                rounding: Rounding::HalfUp,
                mark: DecimalMark::Point,
            },
        ),
        (
            "length_metric",
            FieldRule::Measurement {
                unit: "mm".to_owned(),
                display_unit: "mm".to_owned(),
                step: Decimal::from_i64(1),
                rounding: Rounding::HalfUp,
                mark: DecimalMark::Point,
            },
        ),
        (
            "percent",
            FieldRule::Decimal {
                places: 2,
                rounding: Rounding::HalfEven,
                mark: DecimalMark::Point,
            },
        ),
        ("email", text(false, Whitespace::Trim)),
        ("phone", text(true, Whitespace::Collapse)),
        ("identifier", text(true, Whitespace::Trim)),
        ("url", text(true, Whitespace::Trim)),
        ("file", text(true, Whitespace::Exact)),
        (UNTRUSTED_TEXT, text(true, Whitespace::Collapse)),
    ]
}

/// The built-in field type of free text that came from the system: `text`,
/// marked untrusted. A drafting rule types such an output with it, so that
/// a drafted read marks what the system said without a `types.yaml`.
pub const UNTRUSTED_TEXT: &str = "untrusted_text";

/// What a built-in field type states beside its rule.
pub(crate) fn built_in_labels(name: &str) -> Labels {
    Labels {
        untrusted: name == UNTRUSTED_TEXT,
        ..Labels::default()
    }
}

/// The offset from UTC, in minutes, of a zone the expression library can
/// fix: `UTC`, or an offset written `+02:00`.
pub(crate) fn utc_offset_minutes(zone: &str) -> Option<i32> {
    if zone == "UTC" {
        return Some(0);
    }
    let sign = match zone.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (hours, minutes) = zone.get(1..)?.split_once(':')?;
    let number = |text: &str, most: i32| {
        (text.len() == 2 && text.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse::<i32>().ok())
            .flatten()
            .filter(|number| *number <= most)
    };
    Some(sign * (number(hours, 14)? * 60 + number(minutes, 59)?))
}

const fn compiled_mark(mark: DecimalMark) -> a2m_core::DecimalMark {
    match mark {
        DecimalMark::Point => a2m_core::DecimalMark::Point,
        DecimalMark::Comma => a2m_core::DecimalMark::Comma,
    }
}

/// How the system shows a value, stated as data from the same rule the
/// normalizer is made from, so the two cannot disagree.
fn format_of(rule: &FieldRule) -> FieldFormat {
    match rule {
        FieldRule::Text {
            case_sensitive,
            whitespace,
        } => FieldFormat::Text {
            case_sensitive: *case_sensitive,
            whitespace: match whitespace {
                Whitespace::Collapse => WhitespaceRule::Collapse,
                Whitespace::Trim => WhitespaceRule::Trim,
                Whitespace::Exact => WhitespaceRule::Exact,
            },
        },
        FieldRule::Integer { mark }
        | FieldRule::Decimal { mark, .. }
        | FieldRule::Money { mark, .. }
        | FieldRule::Measurement { mark, .. } => FieldFormat::Number {
            decimal_mark: compiled_mark(*mark),
        },
        FieldRule::Date { format } | FieldRule::Time { format } => FieldFormat::Calendar {
            pattern: format.clone(),
            utc_offset_minutes: None,
        },
        FieldRule::Datetime { format, zone } => FieldFormat::Calendar {
            pattern: format.clone(),
            utc_offset_minutes: utc_offset_minutes(zone),
        },
        FieldRule::Boolean {
            true_text,
            false_text,
        } => FieldFormat::Boolean {
            true_text: true_text.clone(),
            false_text: false_text.clone(),
        },
        FieldRule::Choice { .. } => FieldFormat::Choice,
    }
}

/// What a field type states beside its rule.
#[derive(Debug, Clone, Default)]
pub(crate) struct Labels {
    pub sensitivity: Sensitivity,
    pub untrusted: bool,
    pub zone: Option<ZoneRule>,
}

/// The compiled target's form of a field type: its rule compiled to a
/// normalizer.
///
/// # Errors
///
/// The sentence `a2m-expr` gives when the rule names something the
/// expression library does not know.
pub(crate) fn compiled_of(
    name: &str,
    rule: &FieldRule,
    labels: Labels,
    exprs: &mut ExprTable,
) -> Result<CompiledFieldType, String> {
    let normalizer = rule
        .normalizer()
        .and_then(|compiled| compiled.to_compiled_expr())
        .map_err(|error| error.message)?;
    let index = exprs.intern(normalizer);
    let mut compiled = CompiledFieldType {
        name: name.to_owned(),
        base: rule.base(),
        ty: rule.value_type(),
        places: None,
        rounding: None,
        step: None,
        unit: None,
        display_unit: None,
        currency: None,
        zone: labels.zone,
        codes: Vec::new(),
        sensitivity: labels.sensitivity,
        untrusted: labels.untrusted,
        format: format_of(rule),
        normalizer: Some(index),
    };
    match rule {
        FieldRule::Decimal {
            places, rounding, ..
        } => {
            compiled.places = Some(*places);
            compiled.rounding = Some(*rounding);
        }
        FieldRule::Money {
            currency,
            places,
            rounding,
            ..
        } => {
            compiled.places = Some(places.unwrap_or_else(|| a2m_expr::currency_places(currency)));
            compiled.rounding = Some(*rounding);
            compiled.currency = Some(currency.clone());
        }
        FieldRule::Measurement {
            unit,
            display_unit,
            step,
            rounding,
            ..
        } => {
            compiled.places = Some(step.places());
            compiled.rounding = Some(*rounding);
            compiled.step = Some(*step);
            compiled.unit = Some(unit.clone());
            compiled.display_unit = Some(display_unit.clone());
        }
        FieldRule::Choice { codes } => compiled.codes.clone_from(codes),
        _ => {}
    }
    Ok(compiled)
}

/// The keys of a field type that mean something for each base, beside
/// `base`, `sensitivity`, and `untrusted`, which every base takes.
fn keys_of(base: FieldBase) -> &'static [&'static str] {
    match base {
        FieldBase::Text => &["case", "whitespace"],
        FieldBase::Integer => &["decimal_mark"],
        FieldBase::Decimal => &["places", "rounding", "decimal_mark"],
        FieldBase::Money => &["currency", "places", "rounding", "decimal_mark"],
        FieldBase::Measurement => &["unit", "display_unit", "step", "rounding", "decimal_mark"],
        FieldBase::Date | FieldBase::Time => &["format"],
        FieldBase::Datetime => &["format", "timezone"],
        FieldBase::Boolean => &["true_text", "false_text"],
        FieldBase::Choice => &["codes"],
    }
}

/// Reads one entry of `types.yaml`, already checked against its schema,
/// into a rule and its labels.
///
/// # Errors
///
/// Diagnostics for a key that means nothing for the base, and for what the
/// base needs and the entry does not state.
pub(crate) fn rule_of(
    name: &str,
    doc: &Located<Doc>,
    fixed_zone: &str,
) -> Result<(FieldRule, Labels), Vec<Diagnostic>> {
    let mut problems = Vec::new();
    let Some(base) = doc
        .text("base")
        .and_then(|base| base.parse::<FieldBase>().ok())
    else {
        return Err(problems);
    };
    let allowed = keys_of(base);
    for entry in doc.entries() {
        let key = entry.key.value.as_str();
        if !allowed.contains(&key) && !["base", "sensitivity", "untrusted"].contains(&key) {
            problems.push(Diagnostic::error(
                kernel(codes::UNKNOWN_KEY),
                Stage::Validate,
                &entry.key.at,
                format!("The key `{key}` means nothing for a field type whose base is `{base}`."),
                fix_for_unknown(key, allowed),
            ));
        }
    }
    let mut need = |key: &str, what: &str, at: &Place| {
        problems.push(Diagnostic::error(
            kernel(codes::FIELD_TYPE_INCOMPLETE),
            Stage::Resolve,
            at,
            format!("The field type `{name}` has the base `{base}` and does not state {what}."),
            format!("Add `{key}:` to the field type, as the system states it."),
        ));
    };
    let word = |key: &str| doc.text(key);
    let rounding = |default: Rounding| {
        word("rounding")
            .and_then(|name| name.parse::<Rounding>().ok())
            .unwrap_or(default)
    };
    let mark = word("decimal_mark")
        .and_then(DecimalMark::from_name)
        .unwrap_or(DecimalMark::Point);
    let places = doc
        .get("places")
        .and_then(Located::as_int)
        .and_then(|places| u32::try_from(places).ok());
    let mut zone_rule = None;
    let rule = match base {
        FieldBase::Text => Some(FieldRule::Text {
            case_sensitive: word("case") != Some("insensitive"),
            whitespace: match word("whitespace") {
                Some("trim") => Whitespace::Trim,
                Some("exact") => Whitespace::Exact,
                _ => Whitespace::Collapse,
            },
        }),
        FieldBase::Integer => Some(FieldRule::Integer { mark }),
        FieldBase::Decimal => match places {
            Some(places) => Some(FieldRule::Decimal {
                places,
                rounding: rounding(Rounding::HalfEven),
                mark,
            }),
            None => {
                need("places", "its decimal places", &doc.at);
                None
            }
        },
        FieldBase::Money => match word("currency") {
            Some(currency) => Some(FieldRule::Money {
                currency: currency.to_owned(),
                places,
                rounding: rounding(Rounding::HalfEven),
                mark,
            }),
            None => {
                need("currency", "its currency", &doc.at);
                None
            }
        },
        FieldBase::Measurement => {
            let step = doc.get("step").and_then(|step| match &step.value {
                Doc::Int(number) => Some(Decimal::from_i64(*number)),
                Doc::Text(text) => Decimal::parse(text).ok(),
                _ => None,
            });
            if word("unit").is_none() {
                need("unit", "the unit it is stored in", &doc.at);
            }
            if step.is_none() {
                need("step", "its smallest increment", &doc.at);
            }
            match (word("unit"), step) {
                (Some(unit), Some(step)) => Some(FieldRule::Measurement {
                    unit: unit.to_owned(),
                    display_unit: word("display_unit").unwrap_or(unit).to_owned(),
                    step,
                    rounding: rounding(Rounding::HalfUp),
                    mark,
                }),
                _ => None,
            }
        }
        FieldBase::Date | FieldBase::Time | FieldBase::Datetime => match word("format") {
            None => {
                need("format", "the format the system shows it in", &doc.at);
                None
            }
            Some(format) if base == FieldBase::Date => Some(FieldRule::Date {
                format: format.to_owned(),
            }),
            Some(format) if base == FieldBase::Time => Some(FieldRule::Time {
                format: format.to_owned(),
            }),
            Some(format) => {
                let zone = match word("timezone") {
                    None | Some("fixed") => {
                        zone_rule = Some(ZoneRule::Fixed);
                        Some(fixed_zone.to_owned())
                    }
                    Some("user") => {
                        need(
                            "timezone",
                            "a zone the compiler can fix; a zone that follows each user is not built yet",
                            &doc.at,
                        );
                        None
                    }
                    Some(named) => {
                        zone_rule = Some(ZoneRule::Named(named.to_owned()));
                        Some(named.to_owned())
                    }
                };
                zone.map(|zone| FieldRule::Datetime {
                    format: format.to_owned(),
                    zone,
                })
            }
        },
        FieldBase::Boolean => Some(FieldRule::Boolean {
            true_text: word("true_text").unwrap_or("true").to_owned(),
            false_text: word("false_text").unwrap_or("false").to_owned(),
        }),
        FieldBase::Choice => {
            let codes: Vec<CodeLabel> = doc
                .get("codes")
                .map(|codes| {
                    codes
                        .entries()
                        .iter()
                        .map(|entry| CodeLabel {
                            code: entry.key.value.clone(),
                            label: entry.value.as_text().unwrap_or_default().to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            if codes.is_empty() {
                need("codes", "its stored codes and their labels", &doc.at);
                None
            } else {
                Some(FieldRule::Choice { codes })
            }
        }
    };
    let labels = Labels {
        sensitivity: word("sensitivity")
            .and_then(|name| name.parse::<Sensitivity>().ok())
            .unwrap_or_default(),
        untrusted: doc
            .get("untrusted")
            .and_then(Located::as_bool)
            .unwrap_or(false),
        zone: zone_rule,
    };
    match rule {
        Some(rule) if problems.is_empty() => Ok((rule, labels)),
        _ => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use super::{Labels, built_ins, compiled_of, utc_offset_minutes};
    use crate::view::ExprTable;
    use a2m_core::{DecimalMark, FieldFormat};

    #[test]
    fn a_zone_the_library_can_fix_has_an_offset_in_minutes() {
        assert_eq!(utc_offset_minutes("UTC"), Some(0));
        assert_eq!(utc_offset_minutes("+02:00"), Some(120));
        assert_eq!(utc_offset_minutes("-05:30"), Some(-330));
        for other in [
            "",
            "Europe/Example",
            "+2:00",
            "+02",
            "+15:00",
            "+02:60",
            "02:00",
        ] {
            assert_eq!(utc_offset_minutes(other), None, "{other}");
        }
    }

    #[test]
    fn a_field_type_states_its_format_as_data_beside_its_normalizer() {
        let mut exprs = ExprTable::default();
        let compiled = |name: &str, exprs: &mut ExprTable| {
            let (_, rule) = built_ins()
                .into_iter()
                .find(|(known, _)| *known == name)
                .unwrap();
            compiled_of(name, &rule, Labels::default(), exprs).unwrap()
        };
        let length = compiled("length_ft_in", &mut exprs);
        assert_eq!(length.step.as_ref().unwrap().to_string(), "0.0625");
        assert_eq!(
            length.format,
            FieldFormat::Number {
                decimal_mark: DecimalMark::Point
            }
        );
        assert_eq!(
            compiled("datetime", &mut exprs).format,
            FieldFormat::Calendar {
                pattern: "YYYY-MM-DD'T'HH:mm:ss".to_owned(),
                utc_offset_minutes: Some(0),
            }
        );
        assert_eq!(
            compiled("date", &mut exprs).format,
            FieldFormat::Calendar {
                pattern: "YYYY-MM-DD".to_owned(),
                utc_offset_minutes: None,
            }
        );
    }

    #[test]
    fn every_built_in_field_type_compiles_to_a_normalizer() {
        let mut exprs = ExprTable::default();
        for (name, rule) in built_ins() {
            let compiled = compiled_of(name, &rule, Labels::default(), &mut exprs)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(compiled.normalizer.is_some(), "{name}");
        }
    }

    #[test]
    fn of_the_built_in_field_types_only_untrusted_text_is_marked_untrusted() {
        let mut exprs = ExprTable::default();
        let marked: Vec<&str> = built_ins()
            .into_iter()
            .filter(|(name, rule)| {
                compiled_of(name, rule, super::built_in_labels(name), &mut exprs)
                    .unwrap()
                    .untrusted
            })
            .map(|(name, _)| name)
            .collect();
        assert_eq!(marked, [super::UNTRUSTED_TEXT]);
    }
}
