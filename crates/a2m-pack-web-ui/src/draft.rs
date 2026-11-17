//! The drafting rules from a snapshot of a screen to a screen model and
//! a read over it (spec section 402, decision 0183).
//!
//! A `screen` finding of the web pack's discoverers holds the structure of
//! one page: each element's role and depth, and the accessible name of a
//! role whose name is structure, such as a table or a column header. What
//! fills the page is kept as a type only. From it these rules draft, for
//! each table the page names and the snapshot can tell apart, one screen
//! that reads the table by its headings, and one `list` operation over it
//! whose outputs are the table's columns, as lists.
//!
//! What they never draft: a field, an action, or anything that could
//! write, whatever the page holds; a table with no name, or whose name
//! another table of the page shares (that is an open question naming
//! both); a column whose heading another column shares; a type other than
//! `untrusted_text`, since what a cell holds is a sample nobody stated the
//! type of; an example; a test. The screen's address is the page's own
//! path, as observed: joining the pages of one screen into one address
//! with placeholders is screen matching, which is not built.

use a2m_core::{Failure, FailureClass, FileKindName, Finding, FindingKind, OperationName, Subject};
use a2m_files::{DraftContext, DraftDoc, DraftValue, DraftingRule, RelPath, UNTRUSTED_TEXT};
use serde_json::Value as Json;

use crate::screens::{SCREEN_KIND, unreserved};

/// Whether `text` is a name a screen, an output, or a column may have.
fn is_name(text: &str, longest: usize) -> bool {
    text.len() <= longest && crate::screens::is_name(text)
}

/// The kernel's file kind for operations.
const OPERATION: FileKindName = FileKindName::fixed("operation");

/// The finding kind the web pack's discoverers give for a snapshot.
pub(crate) const SCREEN_FINDING: &str = "screen";

/// The format of a `screen` finding these rules read.
const FORMAT: &str = "1";

/// The longest name a screen or an operation may have.
const LONGEST: usize = 64;

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
    Failure::new(
        FailureClass::Input,
        a2m_core::FailureCode::FINDING_INVALID,
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
        if one.role == "table" {
            if let Some(name) = one.name.clone().filter(|name| !name.trim().is_empty()) {
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
    let regular = regular
        && heading_rows == 1
        && cells_in_rows.iter().all(|count| *count == headers.len());
    (headers, regular)
}

/// A name in the form target files use: lowercase letters and digits,
/// with one underscore between words.
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

/// The page's path, when every segment of it can stand in a screen's
/// address as written.
fn plain_path(finding: &Finding) -> Option<&str> {
    let Subject::Address(address) = finding.subject() else {
        return None;
    };
    let path = address.path();
    let rest = path.strip_prefix('/')?;
    let plain = rest.is_empty()
        || rest.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.chars().all(unreserved)
        });
    plain.then_some(path)
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
fn surface(cx: &DraftContext<'_>, origin: &str) -> Option<String> {
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
    let (Some(target), Some(path), Some(origin)) =
        (cx.target(), plain_path(finding), origin(finding))
    else {
        return Ok(out);
    };
    let Some(surface) = surface(cx, origin) else {
        return Ok(out);
    };
    let page = snake(path);
    let mut drafted: Vec<String> = Vec::new();
    for table in tables(&elements) {
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
        // name, cannot say which column is which.
        let mut columns = Vec::new();
        for heading in &table.headers {
            let column = snake(heading);
            let alone = table.headers.iter().filter(|other| *other == heading).count() == 1
                && table
                    .headers
                    .iter()
                    .filter(|other| snake(other) == column)
                    .count()
                    == 1;
            if alone && is_name(&column, LONGEST) {
                columns.push((column, heading.clone()));
            }
        }
        if columns.is_empty() {
            continue;
        }
        let locate = if table.shared {
            cx.open_question(&format!(
                "The page `{path}` shows more than one table named as this one, so neither can be located by its role and name alone. Which does this screen read, and what tells it apart?"
            ))
        } else {
            DraftValue::List(vec![locator("table", &table.name)])
        };
        let screen = DraftValue::map(vec![
            ("screen", DraftValue::text(&name)),
            ("surface", DraftValue::text(&surface)),
            ("reach", DraftValue::map(vec![("url", DraftValue::text(path))])),
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
        let doc = DraftValue::map(vec![
            ("operation", DraftValue::text(&operation)),
            ("effect", DraftValue::text("read")),
            ("inputs", DraftValue::Map(Vec::new())),
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
                    "Lists the rows of one table of the page `{path}`, each column as the page shows it. Drafted by rule from one snapshot; its name is provisional."
                )),
            ),
        ]);
        out.operations.push(DraftDoc {
            path: RelPath::new(&format!("operations/{name}_list.yaml"))?,
            body: doc,
        });
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
