//! The drafting rules from a snapshot of a screen to screen models and
//! reads over them (spec section 402, decisions 0183 and 0187).
//!
//! A `screen` finding of the web pack's discoverers holds the structure of
//! one page, or of several pages that show one screen at addresses that
//! differ in one segment: then its subject's path holds that segment as a
//! placeholder, `/orders/{order_id}`, and the finding names the pages it
//! was seen on. It keeps each element's role and depth, and the accessible
//! name of a role whose name is structure, such as a table, a column
//! header, a term of a description list, or a definition the page labels.
//! What fills the page is kept as a type only. From it these rules draft:
//!
//! - for each table the page names and the snapshot can tell apart, one
//!   screen that reads the table by its headings, and one `list` read over
//!   it whose outputs are the table's columns, as lists;
//! - for the labelled values of the page, each a definition labelled by a
//!   term of the same name, one screen whose fields locate them, and one
//!   `get` read whose outputs they are.
//!
//! A placeholder of the address is the read's input and key, of type
//! `text`; the driver refuses a value that is not one plain segment before
//! the tab is sent anywhere.
//!
//! What they never draft: a field that is set, an action, or anything that
//! could write, whatever the page holds; a table with no name, or whose
//! name another table of the page shares (that is an open question naming
//! both); a column whose heading another column shares; a labelled value
//! whose label another term shares (an open question, apart); a type other
//! than `untrusted_text`, since what a cell holds is a sample nobody stated
//! the type of; an example; a test.

use a2m_core::{Failure, FileKindName, Finding, FindingKind, OperationName, Subject};
use a2m_files::{
    DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT, left_out_sentence,
    names_a_secret,
};
use serde_json::Value as Json;

use crate::screens::SCREEN_KIND;

/// Whether the compiler takes a column's heading or a value's label for a
/// secret's, as the page writes it or as the output name it becomes
/// (decision 0302).
pub(crate) fn named_as_secret(words: &str, name: &str) -> bool {
    names_a_secret(words) || names_a_secret(name)
}

/// Whether `text` is a name a screen, an output, or a column may have.
pub(crate) fn is_name(text: &str, longest: usize) -> bool {
    text.len() <= longest && crate::screens::is_name(text)
}

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// What a screen seen in a person's use asks in place of what shows it is
/// ready.
const PERSON_READY: &str = "This screen, at the address quoted, was seen while a person used the application in a window the engine watched; the engine never opened it itself. What shows that the page is ready when the engine opens it to read? Write it under `ready`, then release this draft.";

/// Whether a `screen` finding says it was seen as a person used the
/// application (`seen_by: person`, decision 0392).
fn seen_by_a_person(found: &str) -> bool {
    serde_json::from_str::<Json>(found)
        .ok()
        .and_then(|found| {
            found
                .get("seen_by")
                .and_then(Json::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("person")
}

/// The finding kind the web pack's discoverers give for a snapshot.
pub(crate) const SCREEN_FINDING: &str = "screen";

/// The name, in a `screen` finding, of the origins the page tried to send
/// requests to and the observing tab refused (decision 0292, package P3b).
/// The web pack's discoverers write it; it is a list of origins, each as
/// the browser writes it.
const REFUSED_ORIGINS: &str = "refused_origins";

/// The format of a `screen` finding these rules read.
const FORMAT: &str = "1";

/// The longest name a screen or an operation may have.
const LONGEST: usize = 64;

/// The longest of a page's own words a draft holds as a locator's name or
/// a table's heading, in characters, and the longest path it holds.
const LONGEST_WORDS: usize = 64;
const LONGEST_PATH: usize = 160;

/// Whether a page's own words may stand in a drafted screen as a locator's
/// name or a heading: at most [`LONGEST_WORDS`] characters, with no control
/// character and none that can hide or reorder text (decision 0195). Words
/// that cannot are not drafted; a question quotes them through the
/// kernel's quoting, which cleans and bounds them.
fn plain_words(text: &str) -> bool {
    text.chars().count() <= LONGEST_WORDS
        && !text
            .chars()
            .any(|c| c.is_control() || a2m_core::said_hides(c))
}

/// The roles of an element that marks out a part of a page, and so can
/// be a table's section, when the page has one such element of that role
/// and name.
const SECTION_ROLES: &[&str] = &[
    "main",
    "region",
    "navigation",
    "complementary",
    "banner",
    "contentinfo",
    "search",
    "dialog",
];

/// The roles of one cell of a row.
const CELL_ROLES: &[&str] = &["cell", "gridcell", "rowheader"];

/// One element of a snapshot, as the finding keeps it.
struct Element {
    depth: u64,
    role: String,
    /// The accessible name, for a role whose name the finding keeps.
    name: Option<String>,
}

/// A table the page names, with what is needed to read it.
struct Table {
    name: String,
    headers: Vec<String>,
    /// The element whose part of the page the table is in, by role and
    /// name. `None` when no such element is one of its kind on the page.
    section: Option<(String, String)>,
    /// Whether another table of the page has the same name.
    shared: bool,
    /// Whether the table could be read row by row as its headings say:
    /// one row of headings, and every other row with one cell for each.
    regular: bool,
}

fn unreadable() -> Failure {
    a2m_core::codes::row::FINDING_INVALID.failure(
        "A `screen` finding is not in the form the web pack's discoverers write.",
        "Take the snapshot again with the version of the web pack that is enabled.",
    )
}

/// The elements of a snapshot, or `None` when it is not one these rules
/// can read whole: of another format, or cut short.
fn elements(found: &str) -> Result<Option<Vec<Element>>, Failure> {
    let found: Json = serde_json::from_str(found).map_err(|_| unreadable())?;
    if found.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(unreadable());
    }
    // A snapshot that was cut short may lack rows or headings.
    if found.get("cut").and_then(Json::as_bool) != Some(false) {
        return Ok(None);
    }
    let list = found
        .get("elements")
        .and_then(Json::as_array)
        .ok_or_else(unreadable)?;
    list.iter()
        .map(|one| {
            Some(Element {
                depth: one.get("depth")?.as_u64()?,
                role: one.get("role")?.as_str()?.to_owned(),
                name: one.get("name").and_then(Json::as_str).map(str::to_owned),
            })
        })
        .collect::<Option<Vec<_>>>()
        .map(Some)
        .ok_or_else(unreadable)
}

/// The origins the finding says the page tried to send requests to, each
/// once and in order, as destinations a drafted operation may declare. An
/// entry that is not an origin written with its scheme, with a host and a
/// port at most, and nothing else, is left out: a destination is an exact
/// address, and nothing here widens the page's reach beyond what a person
/// then reviews. The page's own origin is never one of them.
fn refused_origins(found: &str, own: &str) -> Vec<String> {
    let Ok(found) = serde_json::from_str::<Json>(found) else {
        return Vec::new();
    };
    let mut origins: Vec<String> = found
        .get(REFUSED_ORIGINS)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_str)
        .filter(|one| {
            // A host of plain characters only: a wildcard is no address.
            let plain = one.split_once("://").is_some_and(|(_, host)| {
                host.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'))
            });
            plain
                && a2m_pack_web_browser::Address::new(one)
                    .is_ok_and(|address| address.origin() == *one && *one != own)
        })
        .map(str::to_owned)
        .collect();
    origins.sort();
    origins.dedup();
    origins
}

/// What a drafted operation may declare of the origins its page was seen to
/// send to: `permissions.destinations`, one open question for each origin.
/// The page put the origin there, not the person, and declaring it widens
/// where the tool's browser may send; so the read waits in the holding
/// folder until a person keeps the origin, by writing it in the question's
/// place, or removes it (decision 0298). Nothing when there were none.
fn permissions(
    doc: &mut Vec<(&'static str, DraftValue)>,
    origins: &[String],
    path: &str,
    cx: &DraftContext<'_>,
) {
    if origins.is_empty() {
        return;
    }
    let asked = origins
        .iter()
        .map(|one| {
            cx.open_question_quoting(
                "While it was scouted, the page quoted first tried to send requests to the origin quoted second, which is not the surface's. Declaring it lets this tool's browser send there. Keep it (write the origin here, with its scheme) or remove this line?",
                &[path, one],
            )
        })
        .collect();
    doc.push((
        "permissions",
        DraftValue::map(vec![("destinations", DraftValue::List(asked))]),
    ));
}

/// Every table of the page that has a name, in the order of the page.
fn tables(elements: &[Element]) -> Vec<Table> {
    let named = |role: &str, name: &str| {
        elements
            .iter()
            .filter(|one| one.role == role && one.name.as_deref() == Some(name))
            .count()
    };
    let mut out = Vec::new();
    // The elements above the one at hand, by depth.
    let mut above: Vec<usize> = Vec::new();
    for (at, one) in elements.iter().enumerate() {
        above.retain(|parent| elements[*parent].depth < one.depth);
        let name = (one.role == "table")
            .then(|| one.name.clone())
            .flatten()
            .filter(|name| !name.trim().is_empty());
        if let Some(name) = name {
            let section = above
                .iter()
                .rev()
                .map(|parent| &elements[*parent])
                .find(|parent| {
                    SECTION_ROLES.contains(&parent.role.as_str())
                        && named(&parent.role, parent.name.as_deref().unwrap_or("")) == 1
                })
                .map(|parent| (parent.role.clone(), parent.name.clone().unwrap_or_default()));
            let (headers, regular) = inside(elements, at);
            out.push(Table {
                shared: named("table", &name) > 1,
                name,
                headers,
                section,
                regular,
            });
        }
        above.push(at);
    }
    out
}

/// The headings of the table at `at`, and whether it is regular: no
/// table inside it, one row of headings, and one cell under each heading
/// in every other row.
fn inside(elements: &[Element], at: usize) -> (Vec<String>, bool) {
    let depth = elements[at].depth;
    let within: Vec<&Element> = elements[at + 1..]
        .iter()
        .take_while(|one| one.depth > depth)
        .collect();
    let mut headers = Vec::new();
    let mut heading_rows = 0;
    let mut cells_in_rows = Vec::new();
    let mut regular = true;
    for (position, one) in within.iter().enumerate() {
        match one.role.as_str() {
            "table" | "grid" | "treegrid" => regular = false,
            "row" => {
                let row: Vec<&&Element> = within[position + 1..]
                    .iter()
                    .take_while(|inner| inner.depth > one.depth)
                    .filter(|inner| inner.depth == one.depth + 1)
                    .collect();
                if row.iter().any(|cell| cell.role == "columnheader") {
                    heading_rows += 1;
                    if row.iter().any(|cell| cell.role != "columnheader") {
                        regular = false;
                    }
                    headers = row
                        .iter()
                        .map(|cell| cell.name.clone().unwrap_or_default())
                        .collect();
                } else {
                    cells_in_rows.push(
                        row.iter()
                            .filter(|cell| CELL_ROLES.contains(&cell.role.as_str()))
                            .count(),
                    );
                }
            }
            _ => {}
        }
    }
    let regular =
        regular && heading_rows == 1 && cells_in_rows.iter().all(|count| *count == headers.len());
    (headers, regular)
}

/// A name in the form target files use: lowercase letters and digits,
/// with one underscore between words.
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

/// The page's address as a screen's `reach` holds it: its path, the names
/// of its placeholders, in order, and the words of its plain segments, when
/// every segment is one a screen's address can hold as written.
fn reach_of(finding: &Finding) -> Option<(&str, Vec<String>, String)> {
    let Subject::Address(address) = finding.subject() else {
        return None;
    };
    let path = address.path();
    if path.chars().count() > LONGEST_PATH {
        return None;
    }
    let segments = crate::screens::parse_reach(path).ok()?;
    let mut holes = Vec::new();
    let mut words = Vec::new();
    for segment in segments {
        match segment {
            crate::model::Segment::Hole(name) => {
                if holes.contains(&name) {
                    return None;
                }
                holes.push(name);
            }
            crate::model::Segment::Literal(text) => words.push(text),
        }
    }
    Some((path, holes, snake(&words.join("_"))))
}

/// The origin of the finding's page.
fn origin(finding: &Finding) -> Option<&str> {
    match finding.subject() {
        Subject::Address(address) => Some(address.origin()),
        _ => None,
    }
}

/// The surface of the target whose origin is the page's, when there is
/// exactly one of the kind `web`.
pub(crate) fn surface(cx: &DraftContext<'_>, origin: &str) -> Option<String> {
    let target = cx.existing()?.docs_of("target").next()?;
    let mut found = target
        .get("surfaces")?
        .entries()
        .iter()
        .filter(|entry| {
            entry.value.text("kind") == Some(crate::screens::WEB_SURFACE.as_str())
                && entry
                    .value
                    .text(crate::ADDRESS_KEY)
                    .is_some_and(|own| own.trim_end_matches('/') == origin)
        })
        .map(|entry| entry.key.value.clone());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// What both rules draft from one finding: for each table, its screen
/// and, when the screen holds no question, its operation.
struct Drafts {
    screens: Vec<DraftDoc>,
    operations: Vec<DraftDoc>,
}

fn locator(role: &str, name: &str) -> DraftValue {
    DraftValue::map(vec![
        ("role", DraftValue::text(role)),
        ("name", DraftValue::text(name)),
    ])
}

/// What a drafted read takes: the placeholders of its screen's address,
/// each an input of type `text`, and together its key.
fn keyed(doc: &mut Vec<(&'static str, DraftValue)>, holes: &[String]) {
    match holes {
        [] => {}
        [only] => doc.push(("key", DraftValue::text(only))),
        many => doc.push((
            "key",
            DraftValue::List(many.iter().map(|hole| DraftValue::text(hole)).collect()),
        )),
    }
    doc.push((
        "inputs",
        DraftValue::Map(
            holes
                .iter()
                .map(|hole| {
                    (
                        hole.clone(),
                        DraftValue::map(vec![("type", DraftValue::text("text"))]),
                    )
                })
                .collect(),
        ),
    ));
}

/// The part of the page the element at `at` is in: the nearest element
/// above it whose role marks out a part of a page and that is the one of
/// its role and name on the page. And whether a table holds it.
fn section_of(elements: &[Element], at: usize) -> (Option<(String, String)>, bool) {
    let named = |role: &str, name: &str| {
        elements
            .iter()
            .filter(|one| one.role == role && one.name.as_deref().unwrap_or("") == name)
            .count()
    };
    let mut above = Vec::new();
    let mut depth = elements[at].depth;
    for parent in elements[..at].iter().rev() {
        if parent.depth < depth {
            above.push(parent);
            depth = parent.depth;
        }
    }
    let in_table = above
        .iter()
        .any(|parent| matches!(parent.role.as_str(), "table" | "grid" | "treegrid"));
    let section = above
        .iter()
        .find(|parent| {
            SECTION_ROLES.contains(&parent.role.as_str())
                && named(&parent.role, parent.name.as_deref().unwrap_or("")) == 1
        })
        .map(|parent| (parent.role.clone(), parent.name.clone().unwrap_or_default()));
    (section, in_table)
}

/// One labelled value: a definition the page labels with the name one of
/// its terms has.
struct Labelled {
    output: String,
    label: String,
    section: (String, String),
}

/// The labelled values of the page, and the labels that cannot be told
/// apart: a name two terms share, or that two definitions have.
fn labelled(elements: &[Element]) -> (Vec<Labelled>, Vec<String>, Vec<String>) {
    let named = |role: &str, name: &str| {
        elements
            .iter()
            .filter(|one| one.role == role && one.name.as_deref() == Some(name))
            .count()
    };
    let mut values: Vec<Labelled> = Vec::new();
    let mut shared: Vec<String> = Vec::new();
    let mut secrets: Vec<String> = Vec::new();
    for (at, one) in elements.iter().enumerate() {
        let Some(label) = one.name.as_deref().filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        if one.role != "term" || shared.iter().any(|known| known == label) {
            continue;
        }
        if !plain_words(label) {
            continue;
        }
        if named("term", label) > 1 || named("definition", label) > 1 {
            shared.push(label.to_owned());
            continue;
        }
        let Some(value) = elements
            .iter()
            .position(|other| other.role == "definition" && other.name.as_deref() == Some(label))
        else {
            continue;
        };
        let (Some(section), false) = section_of(elements, value) else {
            continue;
        };
        // A term and its value in different parts of the page are not one.
        if section_of(elements, at).0.as_ref() != Some(&section) {
            continue;
        }
        let output = snake(label);
        if !is_name(&output, LONGEST) || values.iter().any(|known| known.output == output) {
            continue;
        }
        // A value labelled as a secret is never an output (decision 0302).
        if named_as_secret(label, &output) {
            if !secrets.contains(&output) {
                secrets.push(output);
            }
            continue;
        }
        values.push(Labelled {
            output,
            label: label.to_owned(),
            section,
        });
    }
    (values, shared, secrets)
}

fn values_field(locate: DraftValue, section: &(String, String)) -> DraftValue {
    DraftValue::map(vec![
        ("type", DraftValue::text(UNTRUSTED_TEXT)),
        ("control", DraftValue::text("text")),
        ("read", DraftValue::text("text")),
        ("locate", locate),
        (
            "fingerprint",
            DraftValue::map(vec![("section", locator(&section.0, &section.1))]),
        ),
    ])
}

fn drafts(finding: &Finding, cx: &DraftContext<'_>) -> Result<Drafts, Failure> {
    let mut out = Drafts {
        screens: Vec::new(),
        operations: Vec::new(),
    };
    let Some(elements) = elements(finding.found())? else {
        return Ok(out);
    };
    // A tool is named after its target, and a screen is on one of its
    // surfaces; with neither there is nothing to draft.
    let (Some(target), Some((path, holes, page)), Some(origin)) =
        (cx.target(), reach_of(finding), origin(finding))
    else {
        return Ok(out);
    };
    let Some(surface) = surface(cx, origin) else {
        return Ok(out);
    };
    // A placeholder of the address named as a secret would be an input
    // that is a secret: nothing is drafted (decision 0302).
    if holes.iter().any(|hole| named_as_secret(hole, &snake(hole))) {
        return Ok(out);
    }
    let reach = || DraftValue::map(vec![("url", DraftValue::text(path))]);
    let sends_to = refused_origins(finding.found(), origin);
    let seen = if holes.is_empty() {
        format!("the page `{path}`")
    } else {
        format!("the pages `{path}`")
    };
    let mut drafted: Vec<String> = Vec::new();
    for table in tables(&elements) {
        if !plain_words(&table.name) || !table.headers.iter().all(|heading| plain_words(heading)) {
            continue;
        }
        let words = snake(&table.name);
        let name = if page.is_empty() {
            words
        } else {
            format!("{page}_{words}")
        };
        let operation = format!("{}_{name}_list", target.as_str());
        if !is_name(&name, LONGEST)
            || OperationName::new(&operation).is_err()
            || drafted.contains(&name)
        {
            continue;
        }
        drafted.push(name.clone());
        let Some((role, section)) = table.section.clone() else {
            continue;
        };
        if !table.regular {
            continue;
        }
        // A heading two columns share, or two headings that give one
        // name, cannot say which column is which. A column may not have
        // the name of an input.
        let mut columns = Vec::new();
        let mut secrets: Vec<String> = Vec::new();
        for heading in &table.headers {
            let column = snake(heading);
            let alone = table
                .headers
                .iter()
                .filter(|other| *other == heading)
                .count()
                == 1
                && table
                    .headers
                    .iter()
                    .filter(|other| snake(other) == column)
                    .count()
                    == 1;
            if alone && is_name(&column, LONGEST) && !holes.contains(&column) {
                // A column headed as a secret is never an output
                // (decision 0302).
                if named_as_secret(heading, &column) {
                    secrets.push(column);
                } else {
                    columns.push((column, heading.clone()));
                }
            }
        }
        if columns.is_empty() {
            continue;
        }
        let locate = if table.shared {
            cx.open_question_quoting(
                "The page quoted first shows more than one table with the name quoted second, so neither can be located by its role and name alone. Which does this screen read, and what tells it apart?",
                &[path, &table.name],
            )
        } else {
            DraftValue::List(vec![locator("table", &table.name)])
        };
        let screen = DraftValue::map(vec![
            ("screen", DraftValue::text(&name)),
            ("surface", DraftValue::text(&surface)),
            ("reach", reach()),
            (
                "ready",
                DraftValue::map(vec![("visible", locator("table", &table.name))]),
            ),
            (
                "rows",
                DraftValue::map(vec![
                    ("locate", locate),
                    (
                        "fingerprint",
                        DraftValue::map(vec![("section", locator(&role, &section))]),
                    ),
                    (
                        "header",
                        DraftValue::List(
                            table
                                .headers
                                .iter()
                                .map(|heading| DraftValue::text(heading))
                                .collect(),
                        ),
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
        ]);
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{name}.yaml"))?,
            body: screen,
        });
        // A screen that holds a question does not compile, and a read over
        // it would not either.
        if table.shared {
            continue;
        }
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
        let mut doc = vec![
            ("operation", DraftValue::text(&operation)),
            ("effect", DraftValue::text("read")),
        ];
        keyed(&mut doc, &holes);
        doc.extend([
            ("outputs", DraftValue::Map(outputs)),
            (
                "bindings",
                DraftValue::List(vec![DraftValue::map(vec![
                    ("mode", DraftValue::text(crate::UI_MODE)),
                    ("verb", DraftValue::text("list")),
                    ("screen", DraftValue::text(&name)),
                ])]),
            ),
            (
                "description",
                DraftValue::text(&format!(
                    "Lists the rows of one table of {seen}, each column as the page shows it. Drafted by rule from a snapshot; its name is provisional.{}"
                    , left_out_sentence(&secrets)
                )),
            ),
        ]);
        permissions(&mut doc, &sends_to, path, cx);
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{name}_list.yaml"))?,
            body: DraftValue::map(doc),
        });
    }

    // The labelled values, as one screen and one read; a label that cannot
    // be told apart is a question apart, so that the others compile.
    let (values, shared, secrets) = labelled(&elements);
    let name = if page.is_empty() {
        "values".to_owned()
    } else {
        format!("{page}_values")
    };
    let operation = format!(
        "{}_{}_get",
        target.as_str(),
        if page.is_empty() { "page" } else { &page }
    );
    let fits = is_name(&format!("{name}_undecided"), LONGEST)
        && OperationName::new(&operation).is_ok()
        && !drafted.contains(&name);
    let values: Vec<Labelled> = values
        .into_iter()
        .filter(|value| !holes.contains(&value.output))
        .collect();
    if fits && let Some(first) = values.first() {
        let fields = values
            .iter()
            .map(|value| {
                (
                    value.output.clone(),
                    values_field(
                        DraftValue::List(vec![locator("definition", &value.label)]),
                        &value.section,
                    ),
                )
            })
            .collect();
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&name)),
                ("surface", DraftValue::text(&surface)),
                ("reach", reach()),
                (
                    "ready",
                    DraftValue::map(vec![("visible", locator("definition", &first.label))]),
                ),
                ("fields", DraftValue::Map(fields)),
            ]),
        });
        let outputs = values
            .iter()
            .map(|value| {
                (
                    value.output.clone(),
                    DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
                )
            })
            .collect();
        let mut doc = vec![
            ("operation", DraftValue::text(&operation)),
            ("effect", DraftValue::text("read")),
        ];
        keyed(&mut doc, &holes);
        doc.extend([
            ("outputs", DraftValue::Map(outputs)),
            (
                "bindings",
                DraftValue::List(vec![DraftValue::map(vec![
                    ("mode", DraftValue::text(crate::UI_MODE)),
                    ("verb", DraftValue::text("read")),
                    ("screen", DraftValue::text(&name)),
                ])]),
            ),
            (
                "description",
                DraftValue::text(&format!(
                    "Reads the labelled values of {seen}, each as the page shows it beside its label. Drafted by rule from a snapshot; its name is provisional.{}"
                    , left_out_sentence(&secrets)
                )),
            ),
        ]);
        permissions(&mut doc, &sends_to, path, cx);
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{name}_get.yaml"))?,
            body: DraftValue::map(doc),
        });
    }
    let undecided: Vec<(String, DraftValue)> = shared
        .iter()
        .filter(|label| is_name(&snake(label), LONGEST))
        .map(|label| {
            // The label is the page's own words: quoted apart from the
            // product's sentence, cleaned, and bounded (decision 0195).
            let question = cx.open_question_quoting(
                &format!(
                    "{} shows more than one value with the label quoted second, so none of them can be located by its label alone. Which does this field read, and what tells it apart?",
                    if holes.is_empty() {
                        "The page quoted first"
                    } else {
                        "The pages quoted first"
                    }
                ),
                &[path, label],
            );
            let section = elements
                .iter()
                .position(|one| one.role == "term" && one.name.as_deref() == Some(label))
                .and_then(|at| section_of(&elements, at).0);
            let field = match section {
                Some(section) => values_field(question, &section),
                None => question,
            };
            (snake(label), field)
        })
        .collect();
    if fits && !undecided.is_empty() {
        let undecided_name = format!("{name}_undecided");
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{undecided_name}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&undecided_name)),
                ("surface", DraftValue::text(&surface)),
                ("reach", reach()),
                (
                    "ready",
                    cx.open_question("What shows that the page is ready?"),
                ),
                ("fields", DraftValue::Map(undecided)),
            ]),
        });
    }
    // A screen seen while a person used the application was never opened
    // by the engine: what shows that it is ready when the engine opens it
    // is a person's to say, and every draft from a capture is held
    // (decision 0392, point 3). The reads over it are held with it.
    if seen_by_a_person(finding.found()) {
        for doc in &mut out.screens {
            if let DraftValue::Map(pairs) = &mut doc.body {
                for (key, value) in pairs.iter_mut() {
                    if key == "ready" {
                        *value = cx.open_question_quoting(PERSON_READY, &[path]);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// From a `screen` finding to the screens that read its tables.
pub(crate) struct TableScreenRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for TableScreenRule {
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

/// From a `screen` finding to one `list` operation for each table a
/// drafted screen reads.
pub(crate) struct TableReadRule {
    pub(crate) kind: FindingKind,
}

impl DraftingRule for TableReadRule {
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
