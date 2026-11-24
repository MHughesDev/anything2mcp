//! The commands of a program that the name list refuses, such as a
//! language launcher, a version manager, or a container runner, which run
//! no code but the program's own (decision 0299, step one).
//!
//! Which commands those are is the provider's closed list
//! ([`a2m_provider::runs_its_own_code`]), kept beside the name list it
//! qualifies. This module holds a program's files to it, where the files
//! are compiled and again before the program is started: every argument
//! list the program could be given, its commands', its version check's,
//! and its help forms', is one of the list's forms, element for element;
//! every command is declared `read`; and the program is given no variable,
//! because each of these programs reads variables that name what it runs
//! or where it reaches.

use a2m_provider::{OwnCode, OwnCodeForm};

use crate::model::{Arg, Command, Effect, Program};

/// Whether the command's argument list is the form: its elements as
/// written, then, where the form takes names, `--` and one or more values,
/// each one element.
fn is_the_form(form: &OwnCodeForm, args: &[Arg]) -> bool {
    if args.len() < form.args.len() {
        return false;
    }
    let (fixed, rest) = args.split_at(form.args.len());
    let same = fixed
        .iter()
        .zip(form.args)
        .all(|(arg, its)| matches!(arg, Arg::Literal { text } if text == its));
    if !same {
        return false;
    }
    match rest {
        [] => !form.names,
        [Arg::EndOfOptions, values @ ..] => {
            form.names
                && !values.is_empty()
                && values.iter().all(|arg| matches!(arg, Arg::Value { .. }))
        }
        _ => false,
    }
}

/// Whether a fixed list of elements, such as a version check's, is one of
/// the forms that takes no names.
fn is_a_fixed_form(row: &OwnCode, elements: &[String]) -> bool {
    row.forms.iter().any(|form| {
        !form.names
            && form.args.len() == elements.len()
            && form
                .args
                .iter()
                .zip(elements)
                .all(|(its, given)| its == given)
    })
}

/// The forms of the row, as the message of a refusal shows them.
fn shown_forms(row: &OwnCode) -> String {
    row.forms
        .iter()
        .map(|form| {
            let mut shown = form.args.join(" ");
            if form.names {
                shown.push_str(" -- <name>...");
            }
            format!("`{shown}`")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Why the file's `program` block, or one of its commands, cannot run on a
/// program of the row; nothing when every argument list is one of its
/// forms. `program` is a file that declares no interpreter.
pub(crate) fn refusal<'a>(
    row: &OwnCode,
    shown_program: &str,
    program: &Program,
    commands: impl IntoIterator<Item = (&'a str, &'a Command)>,
) -> Option<String> {
    let forms = shown_forms(row);
    if !program.env_set.is_empty() || !program.env_pass.is_empty() {
        return Some(format!(
            "The program `{shown_program}` is one whose commands are started only in the forms that run no code but its own, and its file gives it variables: it is given none, since such a program reads variables that name what it runs."
        ));
    }
    if let Some(check) = &program.version
        && !is_a_fixed_form(row, &check.args)
    {
        return Some(format!(
            "The version check of the program `{shown_program}` is not one of the commands of it that run no code but its own, which are {forms}."
        ));
    }
    if program.help.iter().any(|form| !is_a_fixed_form(row, form)) {
        return Some(format!(
            "A help form of the program `{shown_program}` is not one of the commands of it that run no code but its own, which are {forms}: its help may start other programs."
        ));
    }
    for (name, command) in commands {
        if command.effect != Effect::Read {
            return Some(format!(
                "The command `{name}` of the program `{shown_program}` is not declared `read`: of such a program only the commands that read it and run no code but its own are started."
            ));
        }
        if command.script.is_some()
            || !row
                .forms
                .iter()
                .any(|form| is_the_form(form, &command.args))
        {
            return Some(format!(
                "The command `{name}` of the program `{shown_program}` is not one of the commands of it that run no code but its own, which are {forms}."
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use a2m_provider::runs_its_own_code;

    use super::is_the_form;
    use crate::model::Arg;

    fn literal(text: &str) -> Arg {
        Arg::Literal {
            text: text.to_owned(),
        }
    }

    fn value(param: &str) -> Arg {
        Arg::Value {
            param: param.to_owned(),
        }
    }

    /// A form is matched element for element, with names only after `--`
    /// and only where the form takes them.
    #[test]
    fn an_argument_list_is_one_of_the_forms_element_for_element() {
        let row = runs_its_own_code("/usr/bin/docker").unwrap();
        let fits = |args: &[Arg]| row.forms.iter().any(|form| is_the_form(form, args));
        assert!(fits(&[literal("--version")]));
        let inspect = ["--context", "default", "container", "inspect"].map(literal);
        let mut named = inspect.to_vec();
        named.extend([Arg::EndOfOptions, value("name"), value("other")]);
        assert!(fits(&named));
        // Names without the marker, no names after it, and a flag after
        // the form are not the form.
        let mut bare = inspect.to_vec();
        bare.push(value("name"));
        assert!(!fits(&bare));
        let mut empty = inspect.to_vec();
        empty.push(Arg::EndOfOptions);
        assert!(!fits(&empty));
        assert!(!fits(&inspect));
        let mut flagged = vec![literal("--version")];
        flagged.push(literal("-H"));
        assert!(!fits(&flagged));
        // A form that takes no names takes none.
        assert!(!fits(&[
            literal("--version"),
            Arg::EndOfOptions,
            value("name")
        ]));
        // The context first, or the command is not listed.
        assert!(!fits(
            &["ps", "--all", "--no-trunc", "--format", "json"].map(literal)
        ));
        assert!(!fits(&[literal("run"), literal("example")]));
        assert!(!fits(&[literal("--help")]));
    }
}
