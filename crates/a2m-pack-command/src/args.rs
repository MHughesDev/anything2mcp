//! From a command's argument list and the values of a step to the element
//! list a program is started with.
//!
//! Each entry becomes exactly the elements section 306 shows. No element is
//! split or joined to another, and a value is checked against its param
//! before it is written. A value that stands as its own element cannot
//! begin with `-` unless a `--` element came before it, so it cannot become
//! an option; it cannot begin with `@`, which many programs read as the
//! name of a file of more arguments, wherever it stands; and on Windows it
//! cannot begin with `/`, which programs there read as an option. A value
//! joined to its flag is one element that begins with the flag.

use a2m_core::{RecordKey, Value, Values};
use a2m_provider::StepFailure;

use crate::codes;
use crate::model::{Arg, Command, Param, ParamKind};
use crate::pattern;

fn invalid(param: &str, why: &str) -> StepFailure {
    codes::not_sent(
        &codes::row::SCHEMA_INVALID,
        format!("The value of `{param}` cannot be given to the program: {why}."),
        "Correct the value and call again.",
    )
}

fn unsafe_value(param: &str, why: &str) -> StepFailure {
    codes::not_sent(
        &codes::row::ARGUMENT_UNSAFE,
        format!("The value of `{param}` was refused: {why}."),
        "Give a value that is plain data for this param.",
    )
}

/// One value as the text of an element, checked against its param. The
/// message of a refusal names the param and never the value.
fn text_of(name: &str, param: &Param, value: &Value) -> Result<String, StepFailure> {
    let text = match (param.kind, value) {
        (ParamKind::Text, Value::Text(text)) => text.clone(),
        (ParamKind::Int, Value::Int(number)) => number.to_string(),
        (ParamKind::Decimal, Value::Decimal(number)) => number.to_string(),
        (ParamKind::Decimal, Value::Int(number)) => number.to_string(),
        (ParamKind::Bool, Value::Bool(flag)) => flag.to_string(),
        (ParamKind::Date, Value::Date(date)) => {
            format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
        }
        _ => {
            return Err(invalid(
                name,
                &format!("it is a {} and the param takes another kind", value.kind()),
            ));
        }
    };
    if text.contains('\0') {
        return Err(unsafe_value(name, "it holds a NUL character"));
    }
    if !param.choices.is_empty() && !param.choices.contains(&text) {
        return Err(invalid(
            name,
            "it is not one of the values the param allows",
        ));
    }
    if let Some(declared) = &param.pattern {
        let fits = pattern::whole(declared).is_ok_and(|compiled| compiled.is_match(&text));
        if !fits {
            return Err(invalid(name, "it does not match the param's pattern"));
        }
    }
    Ok(text)
}

/// What a step gives the argument list beside its named values.
#[derive(Debug, Clone, Copy)]
pub struct Given<'a> {
    /// The step's values, by param.
    pub values: &'a Values,
    /// The params that take the parts of the record's key, in order.
    pub key_params: &'a [String],
    /// The record's key, when it is known.
    pub key: Option<&'a RecordKey>,
    /// The record's stamp, when it has one.
    pub stamp: Option<&'a str>,
}

impl Given<'_> {
    /// The value of a param: a part of the record's key when the step says
    /// so, or else the step's value of that name. Nothing is `None`.
    fn value(&self, name: &str) -> Option<&Value> {
        let from_key = self
            .key_params
            .iter()
            .position(|param| param == name)
            .and_then(|position| self.key.and_then(|key| key.parts().get(position)));
        from_key
            .or_else(|| self.values.get(name))
            .filter(|value| !value.is_null())
    }
}

/// Why a value may not stand as an element of its own, if it may not.
///
/// `--` ends the options a program reads, so a leading `-` is allowed after
/// it. A file of arguments is read before any option is, and a program on
/// Windows that reads `/` as an option does not know `--`, so neither is
/// allowed anywhere.
fn not_its_own_element(text: &str, options_ended: bool, windows: bool) -> Option<&'static str> {
    if text.starts_with('-') && !options_ended {
        Some("it begins with `-` and would be read as an option")
    } else if text.starts_with('@') {
        Some("it begins with `@` and could be read as the name of a file of arguments")
    } else if windows && text.starts_with('/') {
        Some("it begins with `/` and could be read as an option")
    } else {
        None
    }
}

/// Builds the element list of one start of `command`.
///
/// # Errors
///
/// Unsent: `input`/`schema_invalid` for a missing value or one that breaks
/// its param; `input`/`argument_unsafe` for a value that holds a NUL
/// character, or would become an option or the name of a file of
/// arguments.
pub fn elements(command: &Command, given: &Given<'_>) -> Result<Vec<String>, StepFailure> {
    let mut out: Vec<String> = Vec::new();
    let mut options_ended = false;
    // A value that is an element of its own may not look like an option.
    let own_element = |name: &str, text: String, options_ended: bool| match not_its_own_element(
        &text,
        options_ended,
        cfg!(windows),
    ) {
        Some(why) => Err(unsafe_value(name, why)),
        None => Ok(text),
    };
    let declared = |name: &str| {
        command
            .params
            .get(name)
            .ok_or_else(|| invalid(name, "the command declares no such param"))
    };
    let missing = |name: &str| invalid(name, "the command needs it and none was given");
    for entry in &command.args {
        match entry {
            Arg::Literal { text } => out.push(text.clone()),
            Arg::EndOfOptions => {
                options_ended = true;
                out.push("--".to_owned());
            }
            Arg::Value { param } => {
                let spec = declared(param)?;
                match given.value(param) {
                    Some(value) => {
                        out.push(own_element(
                            param,
                            text_of(param, spec, value)?,
                            options_ended,
                        )?);
                    }
                    None if spec.required => return Err(missing(param)),
                    None => {}
                }
            }
            Arg::Flag { flag, param, join } => {
                let spec = declared(param)?;
                match given.value(param) {
                    Some(value) => {
                        let text = text_of(param, spec, value)?;
                        match join {
                            Some(join) => out.push(format!("{flag}{join}{text}")),
                            None => {
                                out.push(flag.clone());
                                out.push(own_element(param, text, options_ended)?);
                            }
                        }
                    }
                    None if spec.required => return Err(missing(param)),
                    None => {}
                }
            }
            Arg::When { flag, param } => match given.value(param) {
                Some(Value::Bool(true)) => out.push(flag.clone()),
                Some(Value::Bool(false)) | None => {}
                Some(_) => return Err(invalid(param, "it is not true or false")),
            },
            Arg::Each { flag, param, join } => {
                let spec = declared(param)?;
                let items = match given.value(param) {
                    Some(Value::List(items)) => items.as_slice(),
                    Some(_) => return Err(invalid(param, "it is not a list")),
                    None if spec.required => return Err(missing(param)),
                    None => &[],
                };
                for item in items {
                    let text = text_of(param, spec, item)?;
                    match join {
                        Some(join) => out.push(format!("{flag}{join}{text}")),
                        None => {
                            out.push(flag.clone());
                            out.push(own_element(param, text, options_ended)?);
                        }
                    }
                }
            }
            Arg::Stamp { flag, join } => {
                let Some(stamp) = given.stamp else {
                    return Err(invalid(
                        "stamp",
                        "the command carries the record's stamp and the step has none",
                    ));
                };
                if stamp.contains('\0') {
                    return Err(unsafe_value("stamp", "it holds a NUL character"));
                }
                match join {
                    Some(join) => out.push(format!("{flag}{join}{stamp}")),
                    None => {
                        out.push(flag.clone());
                        out.push(own_element("stamp", stamp.to_owned(), options_ended)?);
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use a2m_core::{FailureClass, RecordKey, Value, Values};

    use super::{Given, elements, not_its_own_element};
    use crate::codes::{ARGUMENT_UNSAFE, SCHEMA_INVALID};
    use crate::model::{Arg, Command, Effect, Limits, Output, Param, ParamKind};

    fn param(kind: ParamKind, required: bool, list: bool, pattern: Option<&str>) -> Param {
        Param {
            kind,
            required,
            list,
            pattern: pattern.map(str::to_owned),
            choices: Vec::new(),
        }
    }

    fn command(args: Vec<Arg>) -> Command {
        Command {
            program: 0,
            script: None,
            effect: Effect::Read,
            params: BTreeMap::from([
                (
                    "sku".to_owned(),
                    param(ParamKind::Text, true, false, Some(r"^[A-Z]{2}-\d{4}$")),
                ),
                (
                    "name".to_owned(),
                    param(ParamKind::Text, false, false, Some(r"^.{1,60}$")),
                ),
                (
                    "quantity".to_owned(),
                    param(ParamKind::Int, false, false, None),
                ),
                (
                    "archived".to_owned(),
                    param(ParamKind::Bool, false, false, None),
                ),
                (
                    "tags".to_owned(),
                    param(ParamKind::Text, false, true, Some(r"^[a-z]+$")),
                ),
            ]),
            args,
            exits: BTreeMap::new(),
            output: Output::Ignored,
            assigns: None,
            absent_when_empty: false,
            own_server: Vec::new(),
            limits: Limits::default(),
        }
    }

    fn given(values: &Values) -> Given<'_> {
        Given {
            values,
            key_params: &[],
            key: None,
            stamp: None,
        }
    }

    fn flag(flag: &str, param: &str, join: Option<&str>) -> Arg {
        Arg::Flag {
            flag: flag.to_owned(),
            param: param.to_owned(),
            join: join.map(str::to_owned),
        }
    }

    #[test]
    fn each_entry_becomes_exactly_its_elements() {
        let command = command(vec![
            Arg::Literal {
                text: "item".to_owned(),
            },
            Arg::Value {
                param: "sku".to_owned(),
            },
            flag("--name", "name", None),
            flag("--quantity", "quantity", Some("=")),
            Arg::When {
                flag: "--archived".to_owned(),
                param: "archived".to_owned(),
            },
            Arg::Each {
                flag: "--tag".to_owned(),
                param: "tags".to_owned(),
                join: None,
            },
            Arg::Stamp {
                flag: "--ref".to_owned(),
                join: None,
            },
        ]);
        let values = Values::new()
            .with("sku", "SK-1001")
            .with("name", "a b; $HOME \"q\" 'r'")
            .with("quantity", 12_i64)
            .with("archived", true)
            .with("tags", vec![Value::from("red"), Value::from("blue")]);
        let mut all = given(&values);
        all.stamp = Some("a2m:01");
        assert_eq!(
            elements(&command, &all).unwrap(),
            [
                "item",
                "SK-1001",
                "--name",
                "a b; $HOME \"q\" 'r'",
                "--quantity=12",
                "--archived",
                "--tag",
                "red",
                "--tag",
                "blue",
                "--ref",
                "a2m:01",
            ]
        );
        // An optional param that is absent gives no element at all.
        let only = Values::new().with("sku", "SK-1001");
        let mut bare = given(&only);
        bare.stamp = Some("s");
        assert_eq!(
            elements(&command, &bare).unwrap(),
            ["item", "SK-1001", "--ref", "s"]
        );
        // A part of the record's key stands for the param the step names.
        let key = RecordKey::new(vec![Value::from("SK-2002")]).unwrap();
        let none = Values::new();
        let keyed = Given {
            values: &none,
            key_params: &["sku".to_owned()],
            key: Some(&key),
            stamp: Some("s"),
        };
        assert_eq!(elements(&command, &keyed).unwrap()[1], "SK-2002");
    }

    #[test]
    fn a_value_cannot_become_an_option_or_break_its_param() {
        let positional = command(vec![Arg::Value {
            param: "name".to_owned(),
        }]);
        let after_marker = command(vec![
            Arg::EndOfOptions,
            Arg::Value {
                param: "name".to_owned(),
            },
        ]);
        let flagged = command(vec![flag("--name", "name", None)]);
        let joined = command(vec![flag("--name", "name", Some("="))]);
        let dashed = Values::new().with("name", "--yes");
        for refused in [&positional, &flagged] {
            let failure = elements(refused, &given(&dashed)).unwrap_err().failure;
            assert_eq!(
                (failure.class(), failure.code()),
                (FailureClass::Input, &ARGUMENT_UNSAFE)
            );
            assert!(!failure.message().contains("--yes"), "{failure}");
        }
        assert_eq!(
            elements(&after_marker, &given(&dashed)).unwrap(),
            ["--", "--yes"]
        );
        assert_eq!(
            elements(&joined, &given(&dashed)).unwrap(),
            ["--name=--yes"]
        );

        // A file of arguments is read before any option, so `--` does not
        // help; joined to its flag the value is no element of its own.
        let at = Values::new().with("name", "@/etc/example");
        for refused in [&positional, &flagged, &after_marker] {
            let failure = elements(refused, &given(&at)).unwrap_err().failure;
            assert_eq!(failure.code(), &ARGUMENT_UNSAFE);
            assert!(!failure.message().contains("example"), "{failure}");
        }
        assert_eq!(
            elements(&joined, &given(&at)).unwrap(),
            ["--name=@/etc/example"]
        );
        // On Windows a leading `/` is an option to many programs.
        assert!(not_its_own_element("/c", true, true).is_some());
        assert!(not_its_own_element("/c", true, false).is_none());
        assert!(not_its_own_element("-5", true, true).is_none());

        let nul = Values::new().with("name", "a\0b");
        let failure = elements(&joined, &given(&nul)).unwrap_err().failure;
        assert_eq!(failure.code(), &ARGUMENT_UNSAFE);

        let sku = command(vec![Arg::Value {
            param: "sku".to_owned(),
        }]);
        for bad in [
            Values::new().with("sku", "SK-1001\n--yes"),
            Values::new().with("sku", "sk-1001"),
            Values::new().with("sku", 7_i64),
            Values::new(),
        ] {
            let failure = elements(&sku, &given(&bad)).unwrap_err();
            assert_eq!(failure.failure.code(), &SCHEMA_INVALID);
            assert_eq!(failure.sent, a2m_provider::Sent::NotSent);
        }
    }
}
