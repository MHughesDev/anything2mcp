//! The drafting rules from a `window` finding to screens and reads over
//! them (spec section 402, decision 0179; the web pack's rules of decision
//! 0183 are the model).
//!
//! From one window's shape they draft:
//!
//! - for the values the window labels, each an `entry` or a `text` whose
//!   accessible name it holds once, one screen whose fields locate them by
//!   role and name, and one `get` read whose outputs they are;
//! - for each table the window names once, with one row of headings and
//!   one cell under each heading in every other row, one screen that reads
//!   it by its headings and one `list` read whose outputs are its columns,
//!   as lists.
//!
//! What they never draft: an action, a field that is set, or anything that
//! could press or type; a control with no accessible name; a control that
//! holds a secret; a name two controls of one role share (that is an open
//! question, quoting the name, in a screen of its own); a type other than
//! `untrusted_text`; a window whose title is not plain words.

use a2m_core::{Failure, FailureClass, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};
use serde_json::Value as Json;

use crate::discover::FORMAT;
use crate::screens::{DESKTOP_SURFACE, SCREEN_KIND};

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The longest name a screen, an output, or an operation may have.
const LONGEST: usize = 64;
/// The longest of an application's own words a draft holds.
const LONGEST_WORDS: usize = 64;

/// The roles whose value a drafted field reads.
const VALUE_ROLES: &[&str] = &["entry", "text"];

struct Element {
    depth: u64,
    role: String,
    name: Option<String>,
    secret: bool,
}

/// Whether an application's words may stand in a drafted screen: plain,
/// bounded, and with nothing that hides or reorders text.
fn plain_words(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= LONGEST_WORDS
        && !text
            .chars()
            .any(|c| c.is_control() || a2m_core::said_hides(c))
}

/// A name in the form target files use.
fn snake(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    text.len() <= LONGEST
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn unreadable() -> Failure {
    Failure::new(
        FailureClass::Input,
        a2m_core::FailureCode::FINDING_INVALID,
        "A `window` finding is not in the form the desktop pack's discoverer writes.",
        "Take the snapshot again with the version of the desktop pack that is enabled.",
    )
}

/// The window's role, title, program, and elements; `None` when the walk
/// was cut short and the shape may lack parts.
type Shape = (String, String, String, Vec<Element>);

fn shape(found: &str) -> Result<Option<Shape>, Failure> {
    let found: Json = serde_json::from_str(found).map_err(|_| unreadable())?;
    if found.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(unreadable());
    }
    if found.get("cut").and_then(Json::as_bool) != Some(false) {
        return Ok(None);
    }
    let text = |value: Option<&Json>| value.and_then(Json::as_str).map(str::to_owned);
    let window = found.get("window").ok_or_else(unreadable)?;
    let (Some(role), Some(title), Some(program)) = (
        text(window.get("role")),
        text(window.get("title")),
        text(found.get("program")),
    ) else {
        return Err(unreadable());
    };
    let elements = found
        .get("elements")
        .and_then(Json::as_array)
        .ok_or_else(unreadable)?
        .iter()
        .map(|one| {
            Some(Element {
                depth: one.get("depth")?.as_u64()?,
                role: one.get("role")?.as_str()?.to_owned(),
                name: one.get("name").and_then(Json::as_str).map(str::to_owned),
                secret: one.get("secret").and_then(Json::as_bool) == Some(true),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(unreadable)?;
    Ok(Some((role, title, program, elements)))
}

/// The target's one surface of the kind `desktop` whose program is this
/// one.
fn surface(cx: &DraftContext<'_>, program: &str) -> Option<String> {
    let target = cx.existing()?.docs_of("target").next()?;
    let mut found = target
        .get("surfaces")?
        .entries()
        .iter()
        .filter(|entry| {
            entry.value.text("kind") == Some(DESKTOP_SURFACE.as_str())
                && entry.value.text(crate::ADDRESS_KEY) == Some(program)
        })
        .map(|entry| entry.key.value.clone());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// Whether the element at `at` is inside a table.
fn in_table(elements: &[Element], at: usize) -> bool {
    let mut depth = elements[at].depth;
    for parent in elements[..at].iter().rev() {
        if parent.depth < depth {
            if parent.role == "table" {
                return true;
            }
            depth = parent.depth;
        }
    }
    false
}

/// The headings of the table at `at`, when it is regular: rows only, one
/// row of headings, and one cell under each heading in every other row.
fn headings(elements: &[Element], at: usize) -> Option<Vec<String>> {
    let depth = elements[at].depth;
    let within: Vec<&Element> = elements[at + 1..]
        .iter()
        .take_while(|one| one.depth > depth)
        .collect();
    let mut headers = None;
    let mut cells = Vec::new();
    for (position, one) in within.iter().enumerate() {
        if one.depth != depth + 1 {
            continue;
        }
        if one.role != "table_row" {
            return None;
        }
        let row: Vec<&&Element> = within[position + 1..]
            .iter()
            .take_while(|inner| inner.depth > one.depth)
            .filter(|inner| inner.depth == one.depth + 1)
            .collect();
        if row.iter().all(|cell| cell.role == "table_column_header") && !row.is_empty() {
            if headers.is_some() {
                return None;
            }
            headers = Some(
                row.iter()
                    .map(|cell| cell.name.clone())
                    .collect::<Option<Vec<_>>>()?,
            );
        } else if row.iter().all(|cell| cell.role == "table_cell") {
            cells.push(row.len());
        } else {
            return None;
        }
    }
    let headers = headers?;
    cells
        .iter()
        .all(|count| *count == headers.len())
        .then_some(headers)
}

fn locator(role: &str, name: &str) -> DraftValue {
    DraftValue::map(vec![
        ("role", DraftValue::text(role)),
        ("name", DraftValue::text(name)),
    ])
}

struct Drafts {
    screens: Vec<DraftDoc>,
    operations: Vec<DraftDoc>,
}

fn read_operation(
    operation: &str,
    verb: &str,
    screen: &str,
    outputs: Vec<(String, DraftValue)>,
    description: String,
) -> DraftValue {
    DraftValue::map(vec![
        ("operation", DraftValue::text(operation)),
        ("effect", DraftValue::text("read")),
        ("inputs", DraftValue::Map(Vec::new())),
        ("outputs", DraftValue::Map(outputs)),
        (
            "bindings",
            DraftValue::List(vec![DraftValue::map(vec![
                ("mode", DraftValue::text(crate::DESKTOP_MODE)),
                ("verb", DraftValue::text(verb)),
                ("screen", DraftValue::text(screen)),
            ])]),
        ),
        ("description", DraftValue::text(&description)),
    ])
}

#[allow(
    clippy::too_many_lines,
    reason = "one pass over one window: its tables, then its labelled values, in a fixed order"
)]
fn drafts(finding: &Finding, cx: &DraftContext<'_>) -> Result<Drafts, Failure> {
    let mut out = Drafts {
        screens: Vec::new(),
        operations: Vec::new(),
    };
    let Some((role, title, program, elements)) = shape(finding.found())? else {
        return Ok(out);
    };
    let (Some(target), Some(surface)) = (cx.target(), surface(cx, &program)) else {
        return Ok(out);
    };
    let window = snake(&title);
    if !plain_words(&title) || !is_name(&window) {
        return Ok(out);
    }
    let reach = || {
        DraftValue::map(vec![(
            "window",
            DraftValue::map(vec![
                ("role", DraftValue::text(&role)),
                ("title", DraftValue::text(&title)),
            ]),
        )])
    };
    let count = |role: &str, name: &str| {
        elements
            .iter()
            .filter(|one| one.role == role && one.name.as_deref() == Some(name))
            .count()
    };
    let mut drafted: Vec<String> = Vec::new();

    // Each table the window names once.
    for (at, table) in elements.iter().enumerate() {
        let Some(name) = table.name.as_deref() else {
            continue;
        };
        if table.role != "table" || !plain_words(name) || count("table", name) != 1 {
            continue;
        }
        let Some(headers) = headings(&elements, at) else {
            continue;
        };
        if !headers.iter().all(|heading| plain_words(heading)) {
            continue;
        }
        let screen = format!("{window}_{}", snake(name));
        let operation = format!("{}_{screen}_list", target.as_str());
        if !is_name(&screen) || OperationName::new(&operation).is_err() || drafted.contains(&screen)
        {
            continue;
        }
        let columns: Vec<(String, String)> = headers
            .iter()
            .filter_map(|heading| {
                let column = snake(heading);
                let alone = headers
                    .iter()
                    .filter(|other| snake(other) == column)
                    .count()
                    == 1;
                (alone && is_name(&column)).then(|| (column, heading.clone()))
            })
            .collect();
        if columns.is_empty() {
            continue;
        }
        drafted.push(screen.clone());
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{screen}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&screen)),
                ("surface", DraftValue::text(&surface)),
                ("reach", reach()),
                (
                    "ready",
                    DraftValue::map(vec![("visible", locator("table", name))]),
                ),
                (
                    "rows",
                    DraftValue::map(vec![
                        ("locate", DraftValue::List(vec![locator("table", name)])),
                        (
                            "header",
                            DraftValue::List(headers.iter().map(|h| DraftValue::text(h)).collect()),
                        ),
                        (
                            "columns",
                            DraftValue::Map(
                                columns
                                    .iter()
                                    .map(|(column, heading)| {
                                        (
                                            column.clone(),
                                            DraftValue::map(vec![
                                                ("header", DraftValue::text(heading)),
                                                ("type", DraftValue::text(UNTRUSTED_TEXT)),
                                            ]),
                                        )
                                    })
                                    .collect(),
                            ),
                        ),
                    ]),
                ),
            ]),
        });
        let outputs = columns
            .iter()
            .map(|(column, _)| {
                (
                    column.clone(),
                    DraftValue::map(vec![
                        ("type", DraftValue::text(UNTRUSTED_TEXT)),
                        ("list", DraftValue::Bool(true)),
                    ]),
                )
            })
            .collect();
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{screen}_list.yaml"))?,
            body: read_operation(
                &operation,
                "list",
                &screen,
                outputs,
                "Lists the rows of one table of one window of the application, each column as the window shows it. Drafted by rule from a snapshot; its name is provisional.".to_owned(),
            ),
        });
    }

    // The labelled values: an entry or a text the window names once.
    let mut fields: Vec<(String, String, String)> = Vec::new();
    let mut shared: Vec<(String, String)> = Vec::new();
    for (at, one) in elements.iter().enumerate() {
        let Some(name) = one.name.as_deref() else {
            continue;
        };
        if !VALUE_ROLES.contains(&one.role.as_str()) || one.secret || in_table(&elements, at) {
            continue;
        }
        if !plain_words(name) {
            continue;
        }
        if count(&one.role, name) > 1 {
            if !shared
                .iter()
                .any(|(role, known)| *role == one.role && known == name)
            {
                shared.push((one.role.clone(), name.to_owned()));
            }
            continue;
        }
        let output = snake(name);
        if !is_name(&output) || fields.iter().any(|(known, _, _)| *known == output) {
            continue;
        }
        fields.push((output, one.role.clone(), name.to_owned()));
    }
    let values = format!("{window}_values");
    let operation = format!("{}_{window}_get", target.as_str());
    let fits = is_name(&format!("{values}_undecided"))
        && OperationName::new(&operation).is_ok()
        && !drafted.contains(&values);
    if fits && let Some((_, first_role, first_name)) = fields.first() {
        let field = |locate: DraftValue| {
            DraftValue::map(vec![
                ("type", DraftValue::text(UNTRUSTED_TEXT)),
                ("control", DraftValue::text("desktop_text")),
                ("locate", locate),
            ])
        };
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{values}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&values)),
                ("surface", DraftValue::text(&surface)),
                ("reach", reach()),
                (
                    "ready",
                    DraftValue::map(vec![("visible", locator(first_role, first_name))]),
                ),
                (
                    "fields",
                    DraftValue::Map(
                        fields
                            .iter()
                            .map(|(output, role, name)| {
                                (
                                    output.clone(),
                                    field(DraftValue::List(vec![locator(role, name)])),
                                )
                            })
                            .collect(),
                    ),
                ),
            ]),
        });
        let outputs = fields
            .iter()
            .map(|(output, _, _)| {
                (
                    output.clone(),
                    DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
                )
            })
            .collect();
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{values}_get.yaml"))?,
            body: read_operation(
                &operation,
                "read",
                &values,
                outputs,
                "Reads the values one window of the application labels, each as the window shows it. Drafted by rule from a snapshot; its name is provisional.".to_owned(),
            ),
        });
    }
    // A name two controls share: a question apart, so that the others
    // compile. The name is the application's own words, quoted.
    let undecided: Vec<(String, DraftValue)> = shared
        .iter()
        .filter(|(_, name)| is_name(&snake(name)))
        .map(|(role, name)| {
            let question = cx.open_question_quoting(
                "The window shows more than one control with the role quoted first and the name quoted second, so none of them can be located by its role and name alone. Which does this field read, and what tells it apart?",
                &[role, name],
            );
            (
                snake(name),
                DraftValue::map(vec![
                    ("type", DraftValue::text(UNTRUSTED_TEXT)),
                    ("control", DraftValue::text("desktop_text")),
                    ("locate", question),
                ]),
            )
        })
        .collect();
    if fits && !undecided.is_empty() {
        let name = format!("{values}_undecided");
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&name)),
                ("surface", DraftValue::text(&surface)),
                ("reach", reach()),
                (
                    "ready",
                    cx.open_question("What shows that the window is ready?"),
                ),
                ("fields", DraftValue::Map(undecided)),
            ]),
        });
    }
    Ok(out)
}

/// From a `window` finding to the screens that read it.
pub(crate) struct WindowScreenRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for WindowScreenRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        SCREEN_KIND
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(drafts(finding, cx)?.screens)
    }
}

/// From a `window` finding to one read for its labelled values and one
/// for each table a drafted screen reads.
pub(crate) struct WindowReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for WindowReadRule {
    fn finding_kind(&self) -> FindingKind {
        self.kind.clone()
    }

    fn file_kind(&self) -> FileKindName {
        OPERATION
    }

    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure> {
        Ok(drafts(finding, cx)?.operations)
    }
}
