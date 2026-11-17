//! Field types: the built-in ones, and those a target adds in `types.yaml`,
//! each compiled to a normalizer through `a2m-expr`.

use a2m_core::{CodeLabel, Decimal, FieldBase, FieldTypePlan, Rounding, Sensitivity, ZoneRule};
use a2m_expr::{DecimalMark, FieldRule, Whitespace};

use crate::codes::{self, kernel};
use crate::diag::{Diagnostic, Stage};
use crate::doc::{Located, Doc, Place};
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
/// the plan lists them.
pub(crate) fn built_ins() -> Vec<(&'static str, FieldRule)> {
    let sixteenth = Decimal::from_units(625, 4).unwrap_or_else(|_| Decimal::from_i64(1));
    vec![
        ("text", text(true, Whitespace::Collapse)),
        ("integer", FieldRule::Integer { mark: DecimalMark::Point }),
        (
            "decimal",
            FieldRule::Decimal {
                places: 6,
                rounding: Rounding::HalfEven,
                mark: DecimalMark::Point,
            },
        ),
        ("date", FieldRule::Date { format: "YYYY-MM-DD".to_owned() }),
        (
            "datetime",
            FieldRule::Datetime {
                format: "YYYY-MM-DD'T'HH:mm:ss".to_owned(),
                zone: "UTC".to_owned(),
            },
        ),
        ("time", FieldRule::Time { format: "HH:mm".to_owned() }),
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
    ]
}

/// What a field type states beside its rule.
#[derive(Debug, Clone, Default)]
pub(crate) struct Labels {
    pub sensitivity: Sensitivity,
    pub untrusted: bool,
    pub zone: Option<ZoneRule>,
}

/// The plan's form of a field type: its rule compiled to a normalizer.
///
/// # Errors
///
/// The sentence `a2m-expr` gives when the rule names something the
/// expression library does not know.
pub(crate) fn plan_of(
    name: &str,
    rule: &FieldRule,
    labels: Labels,
    exprs: &mut ExprTable,
) -> Result<FieldTypePlan, String> {
    let normalizer = rule
        .normalizer()
        .and_then(|compiled| compiled.to_plan_expr())
        .map_err(|error| error.message)?;
    let index = exprs.intern(normalizer);
    let mut plan = FieldTypePlan {
        name: name.to_owned(),
        base: rule.base(),
        ty: rule.value_type(),
        places: None,
        rounding: None,
        unit: None,
        display_unit: None,
        currency: None,
        zone: labels.zone,
        codes: Vec::new(),
        sensitivity: labels.sensitivity,
        untrusted: labels.untrusted,
        normalizer: Some(index),
    };
    match rule {
        FieldRule::Decimal { places, rounding, .. } => {
            plan.places = Some(*places);
            plan.rounding = Some(*rounding);
        }
        FieldRule::Money { currency, places, rounding, .. } => {
            plan.places = Some(places.unwrap_or_else(|| a2m_expr::currency_places(currency)));
            plan.rounding = Some(*rounding);
            plan.currency = Some(currency.clone());
        }
        FieldRule::Measurement { unit, display_unit, step, rounding, .. } => {
            plan.places = Some(step.places());
            plan.rounding = Some(*rounding);
            plan.unit = Some(unit.clone());
            plan.display_unit = Some(display_unit.clone());
        }
        FieldRule::Choice { codes } => plan.codes.clone_from(codes),
        _ => {}
    }
    Ok(plan)
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
    let Some(base) = doc.text("base").and_then(|base| base.parse::<FieldBase>().ok()) else {
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
        untrusted: doc.get("untrusted").and_then(Located::as_bool).unwrap_or(false),
        zone: zone_rule,
    };
    match rule {
        Some(rule) if problems.is_empty() => Ok((rule, labels)),
        _ => Err(problems),
    }
}

#[cfg(test)]
mod tests {
    use super::{Labels, built_ins, plan_of};
    use crate::view::ExprTable;

    #[test]
    fn every_built_in_field_type_compiles_to_a_normalizer() {
        let mut exprs = ExprTable::default();
        for (name, rule) in built_ins() {
            let plan = plan_of(name, &rule, Labels::default(), &mut exprs)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(plan.normalizer.is_some(), "{name}");
        }
    }
}
