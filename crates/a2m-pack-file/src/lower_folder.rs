//! How a `file` binding of a folder or of a growing file becomes steps
//! (decision 0246).
//!
//! A folder's entry is named by the key `name`, the one value an input may
//! give that a path is made from: it is one name inside the surface's
//! folder, checked again by the driver at every step. A growing file is
//! named by its declaration and read from the position an input gives.

use a2m_core::{SurfaceName, ValueType};
use a2m_files::{Diagnostic, Diagnostics, Located, LowerContext, Lowered, ResolvedBinding};

use crate::codes;

/// The one record kind of a folder: an entry, by its name.
pub(crate) const FOLDER_RECORD: &str = "entry";
use crate::lower::{LIST_LIMIT, STEP_TIMEOUT_MS, Steps, problem};
use crate::model::{
    ENTRY_CONTENT, ENTRY_NAME, Expect, FOLDER_KIND, FolderChange, FolderSection, FolderSpec,
    LOCATE, LOG_FROM, LOG_KIND, LogSection, OPEN, READ, SAVE, SET, SURFACE_KIND, StepSpec,
    VERB_CREATE, VERB_DELETE, VERB_LIST, VERB_READ, VERB_RENAME, VERB_UPDATE, field_arg, key_arg,
};

fn refuse_keys(
    binding: &ResolvedBinding<'_>,
    keys: &[&str],
    what: &str,
    problems: &mut Diagnostics,
) {
    for key in keys {
        if let Some(found) = binding.field(key) {
            problems.push(problem(
                found,
                format!("{what} takes no `{key}`."),
                "Remove it.",
            ));
        }
    }
}

/// The one expression of a map that must hold exactly `name`.
fn only<'a>(
    binding: &'a ResolvedBinding<'_>,
    map: &str,
    name: &str,
    problems: &mut Diagnostics,
) -> Option<&'a Located<a2m_files::Doc>> {
    let stated = binding.field(map);
    let entries = stated.map(Located::entries).unwrap_or_default();
    let found = entries.iter().find(|entry| entry.key.value == name);
    for entry in entries {
        if entry.key.value != name {
            problems.push(problem(
                &entry.value,
                format!(
                    "A folder's entry has no `{}` under `{map}`.",
                    entry.key.value
                ),
                &format!("Name only `{name}` under `{map}`."),
            ));
        }
    }
    if found.is_none() {
        problems.push(problem(
            stated.unwrap_or(binding.doc),
            format!("The binding gives no `{name}` under `{map}`."),
            &format!("Add `{map}: {{{name}: input.<name>}}`."),
        ));
    }
    found.map(|entry| &entry.value)
}

fn flag(binding: &ResolvedBinding<'_>, key: &str) -> bool {
    binding.field(key).and_then(Located::as_bool) == Some(true)
}

/// The name of the input an expression is, when it is one input and nothing
/// else: `input.<name>`.
fn input_named(doc: &Located<a2m_files::Doc>) -> Option<String> {
    let name = doc.as_text()?.trim().strip_prefix("input.")?;
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| name.to_owned())
}

#[allow(clippy::too_many_lines)] // one verb after another; the order of the steps is the rule
pub(crate) fn lower_folder(
    binding: &ResolvedBinding<'_>,
    cx: &LowerContext<'_>,
) -> Result<Lowered, Diagnostics> {
    let fail = |found: Diagnostic| Err(Diagnostics::from(found));
    let named = binding.field("folder").unwrap_or(binding.doc);
    let name = named.as_text().unwrap_or_default();
    let found = cx
        .section(&FOLDER_KIND, &SURFACE_KIND)
        .and_then(|(index, section)| {
            let mut decoded: FolderSection = section.payload.decode().ok()?;
            decoded.remove(name).map(|spec| (index, spec))
        });
    let Some((section, spec)) = found else {
        return fail(problem(
            named,
            format!("No file under `folders/` is named `{name}`."),
            "Name a folder by the name of its file, without the extension.",
        ));
    };
    let spec: FolderSpec = spec;
    let Ok(surface) = SurfaceName::new(&spec.surface) else {
        return fail(problem(
            named,
            "The folder is on no surface.",
            "Name its surface.",
        ));
    };
    let steps = Steps {
        section,
        document: name,
        timeout_ms: cx
            .target()
            .default_timeout_ms("step")
            .unwrap_or(STEP_TIMEOUT_MS),
        at: binding.doc,
    };
    let verb = binding.verb;
    let mut problems = Diagnostics::new();
    let what = format!("A `{verb}` binding of a folder");
    refuse_keys(binding, &["record", "from"], &what, &mut problems);
    let writes = [VERB_CREATE, VERB_UPDATE, VERB_RENAME, VERB_DELETE].contains(verb);
    match binding.field("read_back") {
        Some(found) if !writes => problems.push(problem(
            found,
            "A binding that only reads has no read-back.",
            "Remove `read_back`.",
        )),
        Some(found) if found.text("record") != Some(FOLDER_RECORD) => problems.push(problem(
            found,
            "A folder's write is read back from the entry it wrote.",
            "Write `read_back: {record: entry}`.",
        )),
        _ => {}
    }
    let mut lowered = Lowered {
        surface,
        act: Vec::new(),
        confirm: Vec::new(),
        read_back: Vec::new(),
        lookup: Vec::new(),
        settle_ms: 0,
        prefer_response: false,
    };
    let text = ValueType::Text;
    let built: Result<(), Diagnostic> = (|| {
        if *verb == VERB_LIST {
            refuse_keys(binding, &["key", "fields", "content"], &what, &mut problems);
            let limit = binding
                .field("limit")
                .and_then(Located::as_int)
                .and_then(|limit| u32::try_from(limit).ok())
                .unwrap_or(LIST_LIMIT.min(spec.max_entries));
            if limit > spec.max_entries {
                problems.push(problem(
                    binding.field("limit").unwrap_or(binding.doc),
                    "The limit is more entries than the folder's declaration lets one list give.",
                    "Lower `limit`, or raise `max_entries` in the folder's file.",
                ));
            }
            lowered.act = vec![
                steps.step(
                    OPEN,
                    &StepSpec::FolderOpen { for_write: false },
                    Vec::new(),
                    &codes::row::FILE_MISSING,
                    false,
                    false,
                )?,
                steps.step(
                    READ,
                    &StepSpec::FolderList {
                        limit,
                        digest: flag(binding, "digest"),
                    },
                    Vec::new(),
                    &codes::row::VALUE_UNREADABLE,
                    false,
                    false,
                )?,
            ];
            return Ok(());
        }
        refuse_keys(binding, &["limit"], &what, &mut problems);
        let Some(key_doc) = only(binding, "key", ENTRY_NAME, &mut problems) else {
            return Ok(());
        };
        let key = vec![a2m_core::StepArg {
            name: key_arg(ENTRY_NAME),
            value: cx.expr(key_doc, Some(&text))?,
        }];
        let open = |for_write| {
            steps.step(
                OPEN,
                &StepSpec::FolderOpen { for_write },
                Vec::new(),
                &codes::row::FILE_MISSING,
                false,
                false,
            )
        };
        let locate = |expect, args: Vec<a2m_core::StepArg>, lookup| {
            steps.step(
                LOCATE,
                &StepSpec::FolderLocate { expect },
                args,
                &codes::row::PLACE_NOT_FOUND,
                false,
                lookup,
            )
        };
        let read = |digest, content, also: Vec<(String, String)>| {
            steps.step(
                READ,
                &StepSpec::FolderRead {
                    digest,
                    content,
                    also,
                },
                Vec::new(),
                &codes::row::VALUE_UNREADABLE,
                false,
                false,
            )
        };
        if *verb == VERB_READ {
            refuse_keys(binding, &["fields"], &what, &mut problems);
            lowered.act = vec![
                open(false)?,
                locate(Expect::Present, key, true)?,
                read(
                    flag(binding, "digest"),
                    flag(binding, "content"),
                    Vec::new(),
                )?,
            ];
            return Ok(());
        }
        refuse_keys(binding, &["digest", "content"], &what, &mut problems);
        let change = if *verb == VERB_CREATE {
            FolderChange::Create
        } else if *verb == VERB_UPDATE {
            FolderChange::Replace
        } else if *verb == VERB_RENAME {
            FolderChange::Rename
        } else if *verb == VERB_DELETE {
            FolderChange::Delete
        } else {
            return Err(problem(
                binding.doc,
                format!("A folder has no verb `{verb}`."),
                "Use `list`, `read`, `create`, `update`, `rename`, or `delete`.",
            ));
        };
        if !spec.writes.iter().any(|declared| declared == change.verb()) {
            problems.push(problem(
                binding.doc,
                format!(
                    "The folder `{name}` does not declare that it is written by `{verb}`."
                ),
                &format!("Add `{verb}` under `writes` in the folder's file, if its entries are meant to be changed so."),
            ));
        }
        let mut set_args = key.clone();
        let mut read_back_key = key.clone();
        let mut also = Vec::new();
        let mut content = false;
        match change {
            FolderChange::Create | FolderChange::Replace => {
                if let Some(value) = only(binding, "fields", ENTRY_CONTENT, &mut problems) {
                    set_args.push(a2m_core::StepArg {
                        name: field_arg(ENTRY_CONTENT),
                        value: cx.expr(value, Some(&text))?,
                    });
                }
                content = true;
            }
            FolderChange::Rename => {
                if let Some(value) = only(binding, "fields", ENTRY_NAME, &mut problems) {
                    let index = cx.expr(value, Some(&text))?;
                    set_args.push(a2m_core::StepArg {
                        name: field_arg(ENTRY_NAME),
                        value: index,
                    });
                    read_back_key = vec![a2m_core::StepArg {
                        name: key_arg(ENTRY_NAME),
                        value: index,
                    }];
                    match input_named(value) {
                        Some(input) => also.push((ENTRY_NAME.to_owned(), input)),
                        None => problems.push(problem(
                            value,
                            "The new name of a rename is one input of the operation, so that its read-back is compared with it.",
                            "Write `fields: {name: input.<the input that holds the new name>}`.",
                        )),
                    }
                }
            }
            FolderChange::Delete => refuse_keys(binding, &["fields"], &what, &mut problems),
        }
        lowered.act = vec![
            open(true)?,
            locate(
                if change == FolderChange::Create {
                    Expect::Absent
                } else {
                    Expect::Present
                },
                key,
                false,
            )?,
            steps.step(
                SET,
                &StepSpec::FolderSet { change },
                set_args.clone(),
                &codes::row::ROUND_TRIP_FAILED,
                false,
                false,
            )?,
            steps.step(
                SAVE,
                &StepSpec::FolderSave,
                // The values written, again, so that an offline test sees
                // what the committing step sends (finding 22).
                set_args.clone(),
                &a2m_provider::codes::row::NO_CONFIRMATION,
                true,
                false,
            )?,
        ];
        lowered.confirm = vec![steps.step(
            READ,
            &StepSpec::FolderConfirm,
            Vec::new(),
            &codes::row::CHANGED_SINCE_PREVIEW,
            false,
            false,
        )?];
        lowered.read_back = vec![
            open(false)?,
            locate(Expect::Present, read_back_key, true)?,
            read(false, content, also)?,
        ];
        Ok(())
    })();
    match built {
        Ok(()) if problems.is_empty() => Ok(lowered),
        Ok(()) => Err(problems),
        Err(found) => {
            problems.push(found);
            Err(problems)
        }
    }
}

pub(crate) fn lower_log(
    binding: &ResolvedBinding<'_>,
    cx: &LowerContext<'_>,
) -> Result<Lowered, Diagnostics> {
    let fail = |found: Diagnostic| Err(Diagnostics::from(found));
    let named = binding.field("log").unwrap_or(binding.doc);
    let name = named.as_text().unwrap_or_default();
    let found = cx
        .section(&LOG_KIND, &SURFACE_KIND)
        .and_then(|(index, section)| {
            let mut decoded: LogSection = section.payload.decode().ok()?;
            decoded.remove(name).map(|spec| (index, spec))
        });
    let Some((section, spec)) = found else {
        return fail(problem(
            named,
            format!("No file under `logs/` is named `{name}`."),
            "Name a growing file by the name of its file, without the extension.",
        ));
    };
    if *binding.verb != VERB_READ {
        return fail(problem(
            binding.doc,
            "A growing file is only read: other programs append to it, and the file pack never writes it.",
            "Use `verb: read`.",
        ));
    }
    let Ok(surface) = SurfaceName::new(&spec.surface) else {
        return fail(problem(
            named,
            "The growing file is on no surface.",
            "Name its surface.",
        ));
    };
    let mut problems = Diagnostics::new();
    refuse_keys(
        binding,
        &[
            "record",
            "key",
            "fields",
            "limit",
            "digest",
            "content",
            "read_back",
        ],
        "A binding of a growing file",
        &mut problems,
    );
    let steps = Steps {
        section,
        document: name,
        timeout_ms: cx
            .target()
            .default_timeout_ms("step")
            .unwrap_or(STEP_TIMEOUT_MS),
        at: binding.doc,
    };
    let mut args = Vec::new();
    if let Some(from) = binding.field("from") {
        match cx.expr(from, None) {
            Ok(value) => args.push(a2m_core::StepArg {
                name: LOG_FROM.to_owned(),
                value,
            }),
            Err(found) => problems.push(found),
        }
    }
    let read = steps.step(
        READ,
        &StepSpec::LogRead,
        args,
        &codes::row::VALUE_UNREADABLE,
        false,
        false,
    );
    match read {
        Ok(step) if problems.is_empty() => Ok(Lowered {
            surface,
            act: vec![step],
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        }),
        Ok(_) => Err(problems),
        Err(found) => {
            problems.push(found);
            Err(problems)
        }
    }
}
