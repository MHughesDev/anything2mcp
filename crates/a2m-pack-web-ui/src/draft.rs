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
//!   `get` read whose outputs they are;
//! - for a table the page gives no name, the one of its part of the page,
//!   the same as for a named one, with each heading as a person reads it
//!   (decision 0998);
//! - for a list with no name whose items repeat one shape, a screen that
//!   reads its items and a `list` read of them; and for a record whose
//!   pages show different words in their one heading, a screen that reads
//!   the heading and the paragraphs below it, and a `get` read of them
//!   (decision 0999).
//!
//! What a page gives no name or label is drafted held: its locator is an
//! open question that says why and names the locator a person may keep.
//!
//! A placeholder of the address is the read's input and key, of type
//! `text`; the driver refuses a value that is not one plain segment before
//! the tab is sent anywhere.
//!
//! What they never draft: a field that is set, an action, or anything that
//! could write, whatever the page holds; a table whose name another table
//! of the page shares (that is an open question naming both), or two with
//! no name in one part of the page; a column whose heading another column shares; a labelled value
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
    /// The level of a heading.
    level: Option<u64>,
    /// Whether the pages of one screen gave this element different names:
    /// what it shows is the record's value (decision 0999, point 2).
    varies: bool,
}

/// A table of the page, with what is needed to read it.
struct Table {
    /// Its accessible name; empty when the page gives it none.
    name: String,
    headers: Vec<String>,
    /// The element whose part of the page the table is in, by role and
    /// name. `None` when no such element is one of its kind on the page.
    section: Option<(String, String)>,
    /// Whether another table of the page has the same name; for a table
    /// with no name, whether another with none is in its section.
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
                level: one.get("level").and_then(Json::as_u64),
                varies: one.get("varies").and_then(Json::as_bool) == Some(true),
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

/// Every table of the page, in the order of the page: those it names, and
/// those it does not (decision 0998, point 1).
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
        if one.role == "table" {
            let name = one.name.clone().unwrap_or_default();
            let name = if name.trim().is_empty() {
                String::new()
            } else {
                name
            };
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
            let shared = if name.is_empty() {
                // Another table with no name in the same part of the page.
                elements.iter().enumerate().any(|(other, element)| {
                    other != at
                        && element.role == "table"
                        && element.name.as_deref().unwrap_or("").trim().is_empty()
                        && section_of(elements, other).0 == section
                })
            } else {
                named("table", &name) > 1
            };
            out.push(Table {
                shared,
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
        cx.not_drafted(why::CUT_SHORT);
        return Ok(out);
    };
    // A tool is named after its target, and a screen is on one of its
    // surfaces; with neither there is nothing to draft.
    let Some(target) = cx.target() else {
        cx.not_drafted(why::NO_TARGET);
        return Ok(out);
    };
    let Some(origin) = origin(finding) else {
        cx.not_drafted(why::NOT_AN_ADDRESS);
        return Ok(out);
    };
    let Some((path, holes, page)) = reach_of(finding) else {
        cx.not_drafted(why::NO_REACH);
        return Ok(out);
    };
    let Some(surface) = surface(cx, origin) else {
        cx.not_drafted(why::NO_SURFACE);
        return Ok(out);
    };
    // A placeholder of the address named as a secret would be an input
    // that is a secret: nothing is drafted (decision 0302).
    if holes.iter().any(|hole| named_as_secret(hole, &snake(hole))) {
        cx.not_drafted(why::SECRET_PLACEHOLDER);
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
    // A rule that stops on what it cannot read says so by name, when
    // nothing else of the page is drafted.
    let mut stopped: Option<&'static str> = None;
    for table in tables(&elements) {
        // Each heading as a person reads it: a table library's sort words
        // are taken off by the one closed rule the driver also reads by,
        // and a heading with no words left stops the table (decision 0998,
        // point 2).
        let mut headers = Vec::new();
        let mut stripped = false;
        let mut no_words = false;
        for heading in &table.headers {
            match crate::heading::heading(heading) {
                crate::heading::Heading::Words {
                    words,
                    stripped: taken,
                } => {
                    stripped |= taken;
                    headers.push(words);
                }
                crate::heading::Heading::NoWords => no_words = true,
            }
        }
        if no_words {
            stopped = Some(why::HEADING_HAS_NO_WORDS);
            continue;
        }
        let table = Table { headers, ..table };
        let unnamed = table.name.is_empty();
        if !plain_words(&table.name) || !table.headers.iter().all(|heading| plain_words(heading)) {
            continue;
        }
        let words = if unnamed {
            "table".to_owned()
        } else {
            snake(&table.name)
        };
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
        if unnamed && table.shared {
            stopped = Some(why::TABLES_WITH_NO_NAME);
            continue;
        }
        // A table the page gives no name is found by the part of the page it
        // is in and held to its headings; that is weaker than a name, so the
        // locator waits for a person's word (decision 0998, point 1).
        let within = DraftValue::map(vec![
            ("role", DraftValue::text("table")),
            ("name", DraftValue::text("")),
            ("within", locator(&role, &section)),
        ]);
        let locate = if unnamed {
            cx.open_question_quoting(
                &format!(
                    "The page quoted first shows a table the page gives no accessible name, so it cannot be located by its name. It is the one table with no name inside the part of the page quoted second (its role, then its name when it has one), and a read holds it to the headings this screen lists{}. A locator that finds it so is `role: table` with `name: \"\"`, which the screen's fingerprint holds to that part. Is it the table this screen reads? Write that locator here, or a better one, then release this draft.",
                    if stripped {
                        ", each as a person reads it, without the sort words a table library adds"
                    } else {
                        ""
                    }
                ),
                &[path, &role, &section],
            )
        } else if table.shared {
            cx.open_question_quoting(
                "The page quoted first shows more than one table with the name quoted second, so neither can be located by its role and name alone. Which does this screen read, and what tells it apart?",
                &[path, &table.name],
            )
        } else {
            DraftValue::List(vec![locator("table", &table.name)])
        };
        let ready = if unnamed {
            within
        } else {
            locator("table", &table.name)
        };
        let rows = vec![
            ("locate", locate),
            (
                "fingerprint",
                DraftValue::map(vec![("section", locator(&role, &section))]),
            ),
        ];
        let screen = DraftValue::map(vec![
            ("screen", DraftValue::text(&name)),
            ("surface", DraftValue::text(&surface)),
            ("reach", reach()),
            ("ready", DraftValue::map(vec![("visible", ready)])),
            (
                "rows",
                DraftValue::map(
                    rows.into_iter()
                        .chain([
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
                        ])
                        .collect(),
                ),
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

    // A list of items of one shape, and a record headed by its value, each
    // found where the page gives no name or label: held, saying why
    // (decision 0999, points 1 and 2).
    let parts = Parts {
        target: target.as_str(),
        surface: &surface,
        path,
        page: &page,
        holes: &holes,
        seen: &seen,
        sends_to: &sends_to,
    };
    match items_list(&elements) {
        Ok(Some(list)) => parts.list(&list, &mut drafted, &mut out, cx)?,
        Ok(None) => {}
        Err(why) => stopped = Some(why),
    }
    match headed_record(&elements) {
        Ok(Some(record)) => parts.record(&record, &mut drafted, &mut out, cx)?,
        Ok(None) => {}
        Err(why) => stopped = stopped.or(Some(why)),
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
    // A rule that drafts nothing says why: no screen at all, or a screen
    // whose questions hold its read back.
    cx.not_drafted(if out.screens.is_empty() {
        stopped.unwrap_or(why::NOTHING_TO_READ)
    } else {
        why::SCREEN_HAS_QUESTIONS
    });
    Ok(out)
}

/// What the rules of a list and of a record share of the page.
struct Parts<'a> {
    target: &'a str,
    surface: &'a str,
    path: &'a str,
    page: &'a str,
    holes: &'a [String],
    seen: &'a str,
    sends_to: &'a [String],
}

/// A list of the page whose items repeat one shape.
struct ItemList {
    /// The part of the page it is the one list with no name in.
    section: (String, String),
}

/// A record whose heading is its value, and whose values have no labels.
struct HeadedRecord {
    section: (String, String),
    level: u64,
}

/// The roles of a part of a page that holds what the page is about, where
/// a list or a record is looked for: not its navigation, its banner, or
/// its footer.
const CONTENT_ROLES: &[&str] = &["main", "region"];

/// The elements below the one at `at`, with their depth below it.
fn below(elements: &[Element], at: usize) -> impl Iterator<Item = (usize, &Element)> {
    let depth = elements[at].depth;
    elements[at + 1..]
        .iter()
        .enumerate()
        .take_while(move |(_, one)| one.depth > depth)
        .map(move |(offset, one)| (at + 1 + offset, one))
}

fn unnamed(one: &Element) -> bool {
    one.name.as_deref().unwrap_or("").trim().is_empty()
}

/// The one list with no name in a part of the page about its content,
/// with two items or more that all repeat one shape (decision 0999, point
/// 1). `Ok(None)` when the page has no such list; a stop, by name, when it
/// has one whose items differ in shape or one it cannot tell from another.
fn items_list(elements: &[Element]) -> Result<Option<ItemList>, &'static str> {
    let mut found: Vec<(usize, (String, String))> = Vec::new();
    for (at, one) in elements.iter().enumerate() {
        if one.role != "list" || !unnamed(one) {
            continue;
        }
        let (section, in_table) = section_of(elements, at);
        let Some(section) = section.filter(|(role, _)| CONTENT_ROLES.contains(&role.as_str()))
        else {
            continue;
        };
        // A list inside a list's item, or a table, is part of it.
        let nested = {
            let mut depth = one.depth;
            elements[..at].iter().rev().any(|parent| {
                if parent.depth < depth {
                    depth = parent.depth;
                    parent.role == "listitem" || parent.role == "list"
                } else {
                    false
                }
            })
        };
        if in_table || nested {
            continue;
        }
        found.push((at, section));
    }
    let Some((at, section)) = found.first().cloned() else {
        return Ok(None);
    };
    if found.iter().filter(|(_, other)| *other == section).count() > 1 {
        return Err(why::LISTS_WITH_NO_NAME);
    }
    let depth = elements[at].depth;
    let mut shapes: Vec<Vec<(u64, &str)>> = Vec::new();
    for (item_at, item) in below(elements, at) {
        if item.depth != depth + 1 {
            continue;
        }
        if item.role != "listitem" {
            return Err(why::ITEMS_DIFFER);
        }
        shapes.push(
            below(elements, item_at)
                .map(|(_, inner)| (inner.depth - item.depth, inner.role.as_str()))
                .collect(),
        );
    }
    if shapes.len() < 2 {
        return Ok(None);
    }
    if shapes.iter().any(|shape| *shape != shapes[0]) {
        return Err(why::ITEMS_DIFFER);
    }
    Ok(Some(ItemList { section }))
}

/// The record of a screen whose pages show its value as their heading:
/// the one heading in a part of the page about its content whose name the
/// pages of the screen do not share, the one heading of its level there,
/// and the paragraphs of that part, its values with no label (decision
/// 0999, point 2). `Ok(None)` when no heading's name varies; a stop, by
/// name, when one does and the record cannot be read by rule.
fn headed_record(elements: &[Element]) -> Result<Option<HeadedRecord>, &'static str> {
    let varying: Vec<usize> = elements
        .iter()
        .enumerate()
        .filter(|(_, one)| one.role == "heading" && one.varies)
        .map(|(at, _)| at)
        .collect();
    let [at] = varying[..] else {
        return if varying.is_empty() {
            Ok(None)
        } else {
            Err(why::RECORD_NOT_READ)
        };
    };
    let (section, in_table) = section_of(elements, at);
    let Some(section) = section.filter(|(role, _)| CONTENT_ROLES.contains(&role.as_str())) else {
        return Err(why::RECORD_NOT_READ);
    };
    let Some(level) = elements[at].level.filter(|level| (1..=6).contains(level)) else {
        return Err(why::RECORD_NOT_READ);
    };
    let Some(part) = elements
        .iter()
        .position(|one| one.role == section.0 && one.name.as_deref().unwrap_or("") == section.1)
    else {
        return Err(why::RECORD_NOT_READ);
    };
    let same_level = below(elements, part)
        .filter(|(_, one)| one.role == "heading" && one.level == Some(level))
        .count();
    // Every paragraph of the part is one of its values, right under it.
    let paragraphs: Vec<&Element> = below(elements, part)
        .map(|(_, one)| one)
        .filter(|one| one.role == "paragraph")
        .collect();
    let flat = paragraphs
        .iter()
        .all(|one| one.depth == elements[part].depth + 1);
    if in_table || same_level != 1 || paragraphs.is_empty() || !flat {
        return Err(why::RECORD_NOT_READ);
    }
    Ok(Some(HeadedRecord { section, level }))
}

/// The open question a list's or a record's locator waits on.
fn held_locator(
    cx: &DraftContext<'_>,
    sentence: &str,
    path: &str,
    section: &(String, String),
) -> DraftValue {
    cx.open_question_quoting(sentence, &[path, &section.0, &section.1])
}

impl Parts<'_> {
    fn named(&self, words: &str) -> String {
        if self.page.is_empty() {
            words.to_owned()
        } else if self.page == words || self.page.ends_with(&format!("_{words}")) {
            self.page.to_owned()
        } else {
            format!("{}_{words}", self.page)
        }
    }

    fn reach(&self) -> DraftValue {
        DraftValue::map(vec![("url", DraftValue::text(self.path))])
    }

    /// The screen of a list's items and a `list` read of them.
    fn list(
        &self,
        list: &ItemList,
        drafted: &mut Vec<String>,
        out: &mut Drafts,
        cx: &DraftContext<'_>,
    ) -> Result<(), Failure> {
        let name = self.named("items");
        let operation = format!("{}_{name}_list", self.target);
        if !is_name(&name, LONGEST)
            || OperationName::new(&operation).is_err()
            || drafted.contains(&name)
            || self.holes.iter().any(|hole| hole == "items")
        {
            return Ok(());
        }
        drafted.push(name.clone());
        let found_by = DraftValue::map(vec![
            ("role", DraftValue::text("list")),
            ("name", DraftValue::text("")),
            ("within", locator(&list.section.0, &list.section.1)),
        ]);
        let locate = held_locator(
            cx,
            "The page quoted first shows a list whose items all repeat one shape, and the page gives the list no accessible name, so it cannot be located by its name. It is the one list with no name inside the part of the page quoted second (its role, then its name when it has one), and a read gives the text of each of its items, one to a row. A locator that finds it so is `role: list` with `name: \"\"`, which the screen's fingerprint holds to that part. Is it the list this screen reads? Write that locator here, or a better one, then release this draft.",
            self.path,
            &list.section,
        );
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&name)),
                ("surface", DraftValue::text(self.surface)),
                ("reach", self.reach()),
                ("ready", DraftValue::map(vec![("visible", found_by)])),
                (
                    "rows",
                    DraftValue::map(vec![
                        ("locate", locate),
                        (
                            "fingerprint",
                            DraftValue::map(vec![(
                                "section",
                                locator(&list.section.0, &list.section.1),
                            )]),
                        ),
                        ("items", DraftValue::text("listitem")),
                        (
                            "columns",
                            DraftValue::map(vec![(
                                "items",
                                DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
                            )]),
                        ),
                    ]),
                ),
            ]),
        });
        let mut doc = vec![
            ("operation", DraftValue::text(&operation)),
            ("effect", DraftValue::text("read")),
        ];
        keyed(&mut doc, self.holes);
        doc.extend([
            (
                "outputs",
                DraftValue::map(vec![(
                    "items",
                    DraftValue::map(vec![
                        ("type", DraftValue::text(UNTRUSTED_TEXT)),
                        ("list", DraftValue::Bool(true)),
                    ]),
                )]),
            ),
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
                    "Lists the items of one list of {}, each as the text the page shows for it. Drafted by rule from a snapshot; its name is provisional.",
                    self.seen
                )),
            ),
        ]);
        permissions(&mut doc, self.sends_to, self.path, cx);
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{name}_list.yaml"))?,
            body: DraftValue::map(doc),
        });
        Ok(())
    }

    /// The screen of a record headed by its value and a `read` of it: the
    /// heading, and the values with no label as a list, in the page's
    /// order.
    fn record(
        &self,
        record: &HeadedRecord,
        drafted: &mut Vec<String>,
        out: &mut Drafts,
        cx: &DraftContext<'_>,
    ) -> Result<(), Failure> {
        let name = self.named("record");
        let operation = format!("{}_{name}_get", self.target);
        if !is_name(&name, LONGEST)
            || OperationName::new(&operation).is_err()
            || drafted.contains(&name)
            || self
                .holes
                .iter()
                .any(|hole| hole == "heading" || hole == "values")
        {
            return Ok(());
        }
        drafted.push(name.clone());
        let section = locator(&record.section.0, &record.section.1);
        let level = i64::try_from(record.level).unwrap_or(1);
        let locate = held_locator(
            cx,
            &format!(
                "The pages quoted first show different words in their one heading of level {level} inside the part of the page quoted second (its role, then its name when it has one): the heading is the record's own value, not a label, so it cannot be located by its name. The values below it have no labels, so a read gives them as one list, in the page's order, and names none of them. A locator that finds the heading is `heading: {level}`, which the field's fingerprint holds to that part. Is it the record's heading? Write that locator here, or a better one, then release this draft."
            ),
            self.path,
            &record.section,
        );
        out.screens.push(DraftDoc {
            path: RelPath::new(&format!("screens/{name}.yaml"))?,
            body: DraftValue::map(vec![
                ("screen", DraftValue::text(&name)),
                ("surface", DraftValue::text(self.surface)),
                ("reach", self.reach()),
                (
                    "ready",
                    DraftValue::map(vec![(
                        "visible",
                        DraftValue::map(vec![
                            ("heading", DraftValue::Int(level)),
                            ("within", section.clone()),
                        ]),
                    )]),
                ),
                (
                    "fields",
                    DraftValue::map(vec![(
                        "heading",
                        DraftValue::map(vec![
                            ("type", DraftValue::text(UNTRUSTED_TEXT)),
                            ("control", DraftValue::text("text")),
                            ("read", DraftValue::text("text")),
                            ("locate", locate),
                            (
                                "fingerprint",
                                DraftValue::map(vec![("section", section.clone())]),
                            ),
                        ]),
                    )]),
                ),
                (
                    "rows",
                    DraftValue::map(vec![
                        ("locate", DraftValue::List(vec![section.clone()])),
                        ("fingerprint", DraftValue::map(vec![("section", section)])),
                        ("items", DraftValue::text("paragraph")),
                        (
                            "columns",
                            DraftValue::map(vec![(
                                "values",
                                DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
                            )]),
                        ),
                    ]),
                ),
            ]),
        });
        let mut doc = vec![
            ("operation", DraftValue::text(&operation)),
            ("effect", DraftValue::text("read")),
        ];
        keyed(&mut doc, self.holes);
        doc.extend([
            (
                "outputs",
                DraftValue::map(vec![
                    (
                        "heading",
                        DraftValue::map(vec![("type", DraftValue::text(UNTRUSTED_TEXT))]),
                    ),
                    (
                        "values",
                        DraftValue::map(vec![
                            ("type", DraftValue::text(UNTRUSTED_TEXT)),
                            ("list", DraftValue::Bool(true)),
                        ]),
                    ),
                ]),
            ),
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
                    "Reads the record of {}: its heading, which is the record's own value, and the values below it, which have no labels, as one list in the page's order. Drafted by rule from a snapshot; its name is provisional.",
                    self.seen
                )),
            ),
        ]);
        permissions(&mut doc, self.sends_to, self.path, cx);
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{name}_get.yaml"))?,
            body: DraftValue::map(doc),
        });
        Ok(())
    }
}

/// Why a rule from a `screen` finding drafts nothing, in plain words, with
/// what a person could do instead (package 5.6c's follow-up).
mod why {
    // `NO_REACH` states the bound in words.
    const _: () = assert!(super::LONGEST_PATH == 160);

    pub(super) const CUT_SHORT: &str = "The page's snapshot was cut short, or is of a form these rules do not read whole, so a table's rows or headings may be missing. Scout the page again; if it is too large to take whole, write its screen by hand.";
    pub(super) const NO_TARGET: &str = "The target has no name in its target file, and a read is named after its target. Name the target, then draft again.";
    pub(super) const NOT_AN_ADDRESS: &str = "The finding is not about an address on the web, so there is no page to draft a screen for.";
    pub(super) const NO_REACH: &str = "The page's path is longer than 160 characters, or a part of it is neither plain words nor one whole placeholder, or it names one placeholder twice, so no screen can be reached by it. Write the screen by hand if it is wanted.";
    pub(super) const NO_SURFACE: &str = "The page is at an origin that is not exactly one `web` surface of the target. If the page belongs to this application, give the target one `web` surface at that origin and draft again; otherwise it belongs to another system.";
    pub(super) const SECRET_PLACEHOLDER: &str = "A placeholder of the page's path is named as a secret, so a read of the page would take a secret as an input, and nothing is drafted (decision 0302). A secret comes from the session, never from a caller.";
    pub(super) const NOTHING_TO_READ: &str = "The page shows no table with a plain name and plain headings, and no value beside a plain label, that a screen could locate. Write its screen by hand, naming what it reads.";
    pub(super) const HEADING_HAS_NO_WORDS: &str = "A heading of the page's table holds a table library's sort words and no words of its own once they are taken off, so its column cannot be named or read by its heading. Write the table's screen by hand, with the heading as the page shows it to a person.";
    pub(super) const TABLES_WITH_NO_NAME: &str = "The page shows more than one table with no accessible name in one part of the page, so none of them can be located by rule. Write the screen by hand, with a locator that tells the table apart.";
    pub(super) const LISTS_WITH_NO_NAME: &str = "The page shows more than one list with no accessible name in one part of the page, so none of them can be located by rule. Write the screen by hand, with a locator that tells the list apart.";
    pub(super) const ITEMS_DIFFER: &str = "The page's list holds items that differ in shape, or something that is not an item, so a read of it as items of one shape would be a guess. Write the list's screen by hand, naming what it reads.";
    pub(super) const RECORD_NOT_READ: &str = "The pages of the screen show different words in a heading, so the heading is a record's value, but the record cannot be read by rule: more than one heading varies, it is not the one heading of its level in a part of the page about its content, or the values below it are not all paragraphs right under that part. Write the record's screen by hand.";
    pub(super) const SCREEN_HAS_QUESTIONS: &str = "Each screen drafted from the page holds a question for a person, so no read is drafted over it yet. Answer the screen's questions, then write the read by hand.";
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
