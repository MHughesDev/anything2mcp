//! The drafting rules from a `window` finding to screens and reads over
//! them (spec section 402, decision 0179; the web pack's rules of decision
//! 0183 are the model).
//!
//! From one window's shape they draft:
//!
//! - for the values the window labels, each an `entry`, a `text`, or a
//!   `spin_button` that gives its value as text, whose accessible name it
//!   holds once, one screen whose fields locate them by role and name, and
//!   one `get` read whose outputs they are;
//! - for each table the window names once, and for the one table it gives
//!   no name, with one row of headings and one cell under each heading in
//!   every other row (or, with no rows, its headings and cells as its own
//!   children and the column count it states: decision 0999), one screen
//!   that reads it by its headings and one `list` read whose outputs are
//!   its columns, as lists. A table with no name is located by its
//!   headings alone.
//!
//! A window whose finding gives its title as a pattern, because two windows
//! of one shape showed titles that differ in one word, is reached by that
//! pattern, and each read takes the placeholder as a required input and
//! its key (decision 0999).
//!
//! What they never draft: an action, a field that is set, or anything that
//! could press or type; a control with no accessible name; a control that
//! holds a secret; a name two controls of one role share (that is an open
//! question, quoting the name, in a screen of its own); a type other than
//! `untrusted_text`; a window whose title is not plain words. A rule that
//! drafts nothing from a finding says why.

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName};
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, left_out_sentence,
    names_a_secret,
};
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
const VALUE_ROLES: &[&str] = &["entry", "text", "spin_button"];
/// Of those, the roles read only when the snapshot says the control gives
/// its value as text: a spin button may give only a number.
const TEXT_ONLY_ROLES: &[&str] = &["spin_button"];

struct Element {
    depth: u64,
    role: String,
    name: Option<String>,
    secret: bool,
    /// Whether it gives its value as text, when the snapshot says so.
    text: bool,
    /// For a table, the column count it states.
    columns: Option<usize>,
}

/// Whether an application's words may stand in a drafted screen: plain,
/// bounded, and with nothing that hides or reorders text.
pub(crate) fn plain_words(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= LONGEST_WORDS
        && !text
            .chars()
            .any(|c| c.is_control() || a2m_core::said_hides(c))
}

/// Whether the compiler takes a heading or a control's name for a secret's,
/// as the window writes it or as the output name it becomes (decision
/// 0302).
pub(crate) fn named_as_secret(words: &str, name: &str) -> bool {
    names_a_secret(words) || names_a_secret(name)
}

/// A name in the form target files use.
pub(crate) fn snake(text: &str) -> String {
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

pub(crate) fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    text.len() <= LONGEST
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn unreadable() -> Failure {
    a2m_core::codes::row::FINDING_INVALID.failure(
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
                text: one.get("text").and_then(Json::as_bool) == Some(true),
                columns: one
                    .get("columns")
                    .and_then(Json::as_u64)
                    .and_then(|count| usize::try_from(count).ok())
                    .filter(|count| *count > 0),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(unreadable)?;
    Ok(Some((role, title, program, elements)))
}

/// The target's one surface of the kind `desktop` whose program is this
/// one.
pub(crate) fn surface(cx: &DraftContext<'_>, program: &str) -> Option<String> {
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
/// row of headings, and one cell under each heading in every other row;
/// or no rows, the column count the table states, that many headings
/// first, and the cells after them a whole number of rows (decision 0999).
fn headings(elements: &[Element], at: usize) -> Option<Vec<String>> {
    let depth = elements[at].depth;
    let within: Vec<&Element> = elements[at + 1..]
        .iter()
        .take_while(|one| one.depth > depth)
        .collect();
    let children: Vec<&&Element> = within.iter().filter(|one| one.depth == depth + 1).collect();
    if !children.iter().any(|one| one.role == "table_row") {
        let count = elements[at].columns?;
        if children.len() < count || !(children.len() - count).is_multiple_of(count) {
            return None;
        }
        let (heads, cells) = children.split_at(count);
        if !heads.iter().all(|one| one.role == "table_column_header")
            || !cells.iter().all(|one| one.role == "table_cell")
        {
            return None;
        }
        return heads.iter().map(|one| one.name.clone()).collect();
    }
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
    hole: Option<&str>,
    outputs: Vec<(String, DraftValue)>,
    description: String,
) -> DraftValue {
    let mut doc = vec![
        ("operation", DraftValue::text(operation)),
        ("effect", DraftValue::text("read")),
    ];
    // The placeholder of the window's title is the read's one input, and
    // its key: the title proves the record the window shows.
    if let Some(hole) = hole {
        doc.push(("key", DraftValue::text(hole)));
    }
    doc.push((
        "inputs",
        DraftValue::Map(
            hole.into_iter()
                .map(|hole| {
                    (
                        hole.to_owned(),
                        DraftValue::map(vec![("type", DraftValue::text("text"))]),
                    )
                })
                .collect(),
        ),
    ));
    doc.extend([
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
    ]);
    DraftValue::map(doc)
}

/// One table the rules may draft: its name, when it has one, and its
/// headings.
struct Drafted<'a> {
    name: Option<&'a str>,
    headers: Vec<String>,
}

/// The tables of the window the rules read: each the window names once,
/// and the one it gives no name, each regular with plain headings.
fn tables<'a>(elements: &'a [Element], count: &dyn Fn(&str, &str) -> usize) -> Vec<Drafted<'a>> {
    let unnamed = elements
        .iter()
        .filter(|one| one.role == "table" && one.name.is_none())
        .count();
    elements
        .iter()
        .enumerate()
        .filter(|(_, table)| table.role == "table")
        .filter_map(|(at, table)| {
            let name = table.name.as_deref();
            let alone = match name {
                Some(name) => plain_words(name) && count("table", name) == 1,
                None => unnamed == 1,
            };
            if !alone {
                return None;
            }
            let headers = headings(elements, at)?;
            headers
                .iter()
                .all(|heading| plain_words(heading))
                .then_some(Drafted { name, headers })
        })
        .collect()
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
        cx.not_drafted(why::CUT_SHORT);
        return Ok(out);
    };
    let Some(target) = cx.target() else {
        cx.not_drafted(why::NO_TARGET);
        return Ok(out);
    };
    let Some(surface) = surface(cx, &program) else {
        cx.not_drafted(why::NO_SURFACE);
        return Ok(out);
    };
    // A title with a placeholder, which the discoverer writes only for
    // windows of one shape whose titles differed in one word.
    let Ok(hole) = crate::screens::title_hole(&title) else {
        cx.not_drafted(why::TITLE_NOT_PLAIN);
        return Ok(out);
    };
    let words = match &hole {
        Some(hole) => format!("{} {}", hole.before, hole.after),
        None => title.clone(),
    };
    let window = snake(&words);
    if !plain_words(&title) || !is_name(&window) {
        cx.not_drafted(why::TITLE_NOT_PLAIN);
        return Ok(out);
    }
    let hole_name = hole.as_ref().map(|hole| hole.name.as_str());
    if hole_name.is_some_and(|name| named_as_secret(name, name)) {
        cx.not_drafted(why::SECRET_PLACEHOLDER);
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
    // A window found by its whole title, when the title has a number in
    // it, may name the record it shows; the person is told how to make
    // the title a pattern, and nothing is guessed from one title.
    let one_title = if hole.is_none() && title.chars().any(|c| c.is_ascii_digit()) {
        " The window is found by its whole title, which has a number in it; if the title names the record the window shows, open a second record's window beside it and scout again, so that the word that differs becomes an input."
    } else {
        ""
    };
    let count = |role: &str, name: &str| {
        elements
            .iter()
            .filter(|one| one.role == role && one.name.as_deref() == Some(name))
            .count()
    };
    let mut drafted: Vec<String> = Vec::new();

    // Each table the window names once, and the one it gives no name.
    for table in tables(&elements, &count) {
        let screen = match table.name {
            Some(name) => format!("{window}_{}", snake(name)),
            None => format!("{window}_table"),
        };
        let operation = format!("{}_{screen}_list", target.as_str());
        if !is_name(&screen) || OperationName::new(&operation).is_err() || drafted.contains(&screen)
        {
            continue;
        }
        let headers = &table.headers;
        let mut secrets: Vec<String> = Vec::new();
        let columns: Vec<(String, String)> = headers
            .iter()
            .filter_map(|heading| {
                let column = snake(heading);
                let alone = headers
                    .iter()
                    .filter(|other| snake(other) == column)
                    .count()
                    == 1;
                (alone && is_name(&column) && Some(column.as_str()) != hole_name)
                    .then_some((column, heading))
            })
            .filter_map(|(column, heading)| {
                // A column headed as a secret is never an output
                // (decision 0302).
                if named_as_secret(heading, &column) {
                    secrets.push(column);
                    None
                } else {
                    Some((column, heading.clone()))
                }
            })
            .collect();
        if columns.is_empty() {
            continue;
        }
        drafted.push(screen.clone());
        // A table with a name is located by it; one with none, by its
        // headings alone, which its `header` states (decision 0999).
        let (ready, mut rows) = match table.name {
            Some(name) => (
                locator("table", name),
                vec![("locate", DraftValue::List(vec![locator("table", name)]))],
            ),
            None => (locator("table_column_header", &headers[0]), Vec::new()),
        };
        rows.extend([
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
        ]);
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{screen}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&screen)),
                ("surface", DraftValue::text(&surface)),
                ("reach", reach()),
                ("ready", DraftValue::map(vec![("visible", ready)])),
                ("rows", DraftValue::map(rows)),
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
                hole_name,
                outputs,
                format!(
                    "Lists the rows of one table of one window of the application, each column as the window shows it. Drafted by rule from a snapshot; its name is provisional.{}{one_title}",
                    left_out_sentence(&secrets)
                ),
            ),
        });
    }

    // The labelled values: an entry, a text, or a spin button that gives
    // its value as text, the window names once.
    let mut fields: Vec<(String, String, String)> = Vec::new();
    let mut shared: Vec<(String, String)> = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    let mut textless: Vec<String> = Vec::new();
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
        // A control named as a secret is never an output (decision 0302).
        if named_as_secret(name, &snake(name)) {
            let output = snake(name);
            if is_name(&output) && !secrets.contains(&output) {
                secrets.push(output);
            }
            continue;
        }
        // A control that gives no text would be read as its label: it is
        // left out, and the description says so.
        if TEXT_ONLY_ROLES.contains(&one.role.as_str()) && !one.text {
            let output = snake(name);
            if is_name(&output) && !textless.contains(&output) {
                textless.push(output);
            }
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
        if !is_name(&output)
            || Some(output.as_str()) == hole_name
            || fields.iter().any(|(known, _, _)| *known == output)
        {
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
                hole_name,
                outputs,
                format!(
                    "Reads the values one window of the application labels, each as the window shows it. Drafted by rule from a snapshot; its name is provisional.{}{}{one_title}",
                    left_out_sentence(&secrets),
                    textless_sentence(&textless)
                ),
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
    // A rule that drafts nothing says why: no screen at all, or a screen
    // whose questions hold its read back. Only the rule that drafted
    // nothing is listed with it.
    cx.not_drafted(if out.screens.is_empty() {
        if textless.is_empty() {
            why::NOTHING_TO_READ
        } else {
            why::NO_TEXT
        }
    } else {
        why::SCREEN_HAS_QUESTIONS
    });
    Ok(out)
}

/// The sentence a drafted read's description ends with when it left out
/// labelled controls that give no value as text, naming their outputs.
fn textless_sentence(left: &[String]) -> String {
    if left.is_empty() {
        return String::new();
    }
    let named: Vec<String> = left
        .iter()
        .take(8)
        .map(|name| format!("`{name}`"))
        .collect();
    format!(
        " Left out because the window gives their values only as numbers, which this version does not read: {}.",
        named.join(", ")
    )
}

/// Why a rule from a `window` finding drafts nothing, in plain words, with
/// what a person could do instead (field test 10, finding 58).
mod why {
    pub(super) const CUT_SHORT: &str = "The window's snapshot was cut short at a bound of the walk, so a table's rows or a control may be missing. Raise the pack's settings `max_nodes` or `max_depth` and scout again, or write the window's screen by hand.";
    pub(super) const NO_TARGET: &str = "The target has no name in its target file, and a read is named after its target. Name the target, then draft again.";
    pub(super) const NO_SURFACE: &str = "The window's program is not the address of exactly one `desktop` surface of the target. Give the target one `desktop` surface whose address is the program, and draft again.";
    pub(super) const TITLE_NOT_PLAIN: &str = "The window's title is empty, longer than 64 characters, holds characters that hide or reorder text, or makes no name, so no screen can be named after it or reach it. Write the window's screen by hand, with its title as the application gives it.";
    pub(super) const SECRET_PLACEHOLDER: &str = "The placeholder of the window's title is named as a secret, so a read of the window would take a secret as an input, and nothing is drafted (decision 0302). A secret comes from the session, never from a caller.";
    pub(super) const NOTHING_TO_READ: &str = "The window shows no labelled value, and no table with plain headings that it names once or is the one it gives no name, that a screen could locate; a table whose cells are its own children is read only when it states how many columns it has. Write its screen by hand, naming what it reads.";
    pub(super) const NO_TEXT: &str = "The only labelled values the window shows give no value as text, which this version does not read. Write its screen by hand if another control shows the same value.";
    pub(super) const SCREEN_HAS_QUESTIONS: &str = "Each screen drafted from the window holds a question for a person, so no read is drafted over it yet. Answer the screen's questions, then write the read by hand.";
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
