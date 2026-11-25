//! The web pack's repair proposer (decision 0999): a rule of exact pairing
//! over two `screen` findings of one page, one from before a change and
//! one from now, and the screen model the drift names.
//!
//! What changed is read off the two shapes, never guessed. An element
//! renamed is a locator that finds nothing now, whose element before was
//! the only one of its role in its section that is gone, while exactly one
//! of that role in that section is new. A section that moved is a
//! fingerprint whose text was the name of an element inside its section
//! before and is not now; the nearest section around the field that holds
//! that element now takes its place. Anything else is unknown, and nothing
//! is proposed (rule 7). The change replaces only the scalars it names, in
//! place, and is read back: the file must say exactly what was meant.

use std::collections::BTreeMap;

use a2m_core::Finding;
use a2m_files::{Doc, Located, Place, RelPath, normalize_text, parse_yaml};
use a2m_provider::{RepairAsk, RepairFile, RepairProposal, RepairProposer};
use serde_json::Value as Json;

use crate::screen::{FIELD_ROLES, SECTION_ROLES};

/// The roles that hold a section for pairing and for a fingerprint: the
/// sectioning roles of a snapshot, and a form and a group, which a screen
/// model names as a field's section. Data.
const SECTIONS: &[&str] = &["form", "group"];

/// The format of a `screen` finding this rule reads.
const FORMAT: &str = "1";

/// The web pack's proposer for `screen` findings.
pub(crate) struct ScreenRepair;

impl RepairProposer for ScreenRepair {
    fn propose(&self, ask: &RepairAsk<'_>) -> RepairProposal {
        match propose(ask) {
            Ok(proposal) | Err(proposal) => proposal,
        }
    }
}

fn none(reason: impl Into<String>) -> RepairProposal {
    RepairProposal::none(reason)
}

// ---- the shapes ----

/// One element of a snapshot's shape: its role, its name where the shape
/// keeps one, and the elements above it.
#[derive(Debug)]
struct Element {
    role: String,
    name: Option<String>,
    above: Vec<usize>,
}

/// A snapshot's shape, as a `screen` finding holds it.
#[derive(Debug)]
struct Shape {
    elements: Vec<Element>,
}

/// A section, by its role and its name.
type Section = (String, String);

impl Shape {
    fn read(finding: &Finding, which: &str) -> Result<Self, RepairProposal> {
        let found: Json = serde_json::from_str(finding.found()).map_err(|_| {
            none(format!(
                "The snapshot {which} is not a `screen` finding these rules read."
            ))
        })?;
        if found.get("format").and_then(Json::as_str) != Some(FORMAT) {
            return Err(none(format!(
                "The snapshot {which} is of another format than the one these rules read."
            )));
        }
        if found.get("cut").and_then(Json::as_bool) != Some(false) {
            return Err(none(format!(
                "The snapshot {which} was cut short, so an element it does not show may still be on the page."
            )));
        }
        let listed = found
            .get("elements")
            .and_then(Json::as_array)
            .ok_or_else(|| none(format!("The snapshot {which} lists no elements.")))?;
        let mut elements = Vec::with_capacity(listed.len());
        let mut stack: Vec<usize> = Vec::new();
        for one in listed {
            let (Some(depth), Some(role)) = (
                one.get("depth")
                    .and_then(Json::as_u64)
                    .and_then(|depth| usize::try_from(depth).ok()),
                one.get("role").and_then(Json::as_str),
            ) else {
                return Err(none(format!(
                    "An element of the snapshot {which} has no depth or no role."
                )));
            };
            if depth > stack.len() {
                return Err(none(format!(
                    "The snapshot {which} skips a level of its tree."
                )));
            }
            stack.truncate(depth);
            elements.push(Element {
                role: role.to_owned(),
                name: one.get("name").and_then(Json::as_str).map(str::to_owned),
                above: stack.clone(),
            });
            stack.push(elements.len() - 1);
        }
        Ok(Self { elements })
    }

    fn named(&self, at: usize, role: &str, name: &str) -> bool {
        let element = &self.elements[at];
        element.role == role && element.name.as_deref() == Some(name)
    }

    /// The elements of `role` named `name`.
    fn find_named(&self, role: &str, name: &str) -> Vec<usize> {
        (0..self.elements.len())
            .filter(|&at| self.named(at, role, name))
            .collect()
    }

    /// The elements one locator finds.
    fn find(&self, locator: &Locator) -> Vec<usize> {
        (0..self.elements.len())
            .filter(|&at| match &locator.by {
                By::RoleName { role, name } => self.named(at, &role.text, &name.text),
                By::Label(label) => {
                    FIELD_ROLES.contains(&self.elements[at].role.as_str())
                        && self.elements[at].name.as_deref() == Some(label.text.as_str())
                }
            })
            .collect()
    }

    /// The one element a part's locators find, the first that finds
    /// exactly one.
    fn found(&self, part: &Part) -> Option<usize> {
        part.locate.iter().find_map(|locator| match locator {
            Some(locator) => match self.find(locator).as_slice() {
                [one] => Some(*one),
                _ => None,
            },
            None => None,
        })
    }

    /// The section an element is in: the nearest element above it of a
    /// sectioning role, or none for the page itself.
    fn section_of(&self, at: usize) -> Option<Section> {
        self.elements[at]
            .above
            .iter()
            .rev()
            .map(|&above| &self.elements[above])
            .find(|above| is_section(&above.role))
            .map(|above| (above.role.clone(), above.name.clone().unwrap_or_default()))
    }

    /// The nearest element above `at` with the role and the name of
    /// `section`.
    fn above_named(&self, at: usize, section: &Section) -> Option<usize> {
        self.elements[at]
            .above
            .iter()
            .rev()
            .copied()
            .find(|&above| self.named(above, &section.0, &section.1))
    }

    /// Whether `within` holds an element of `role` named `name`.
    fn holds(&self, within: usize, role: &str, name: &str) -> bool {
        (within + 1..self.elements.len())
            .take_while(|&at| self.elements[at].above.contains(&within))
            .any(|at| self.named(at, role, name))
    }

    /// The role of an element named `name` that `within` holds, the first.
    fn role_held(&self, within: usize, name: &str) -> Option<&str> {
        (within + 1..self.elements.len())
            .take_while(|&at| self.elements[at].above.contains(&within))
            .find(|&at| self.elements[at].name.as_deref() == Some(name))
            .map(|at| self.elements[at].role.as_str())
    }

    /// The names of the elements of `role` in `section`, each as often as
    /// it shows.
    fn names_in(&self, role: &str, section: Option<&Section>) -> Vec<String> {
        (0..self.elements.len())
            .filter(|&at| self.elements[at].role == role)
            .filter(|&at| self.section_of(at).as_ref() == section)
            .filter_map(|at| self.elements[at].name.clone())
            .collect()
    }
}

fn is_section(role: &str) -> bool {
    SECTION_ROLES.contains(&role) || SECTIONS.contains(&role)
}

fn shown_section(section: Option<&Section>) -> String {
    match section {
        Some((role, name)) if name.is_empty() => format!("the {role} with no name"),
        Some((role, name)) => format!("the {role} `{name}`"),
        None => "the page outside any section".to_owned(),
    }
}

// ---- the screen ----

/// A text of the file, where it was written, and its path in the
/// document.
#[derive(Debug, Clone)]
struct Scalar {
    text: String,
    at: Place,
    path: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone)]
enum By {
    RoleName { role: Scalar, name: Scalar },
    Label(Scalar),
}

#[derive(Debug, Clone)]
struct Locator {
    by: By,
}

/// A field or an action of the screen: how it is found, and its
/// fingerprint's section and text.
#[derive(Debug)]
struct Part {
    what: String,
    /// Each locator in order; `None` for one of a kind this rule does not
    /// read.
    locate: Vec<Option<Locator>>,
    section: Option<(Scalar, Scalar)>,
    near_text: Option<String>,
}

fn scalar(doc: &Located<Doc>, path: Vec<Step>) -> Option<Scalar> {
    Some(Scalar {
        text: doc.as_text()?.to_owned(),
        at: doc.at.clone(),
        path,
    })
}

fn with(path: &[Step], step: Step) -> Vec<Step> {
    let mut path = path.to_vec();
    path.push(step);
    path
}

fn parts_of(screen: &Located<Doc>) -> Vec<Part> {
    let mut parts = Vec::new();
    for (group, noun) in [("fields", "field"), ("actions", "action")] {
        let Some(listed) = screen.get(group) else {
            continue;
        };
        for entry in listed.entries() {
            let base = vec![
                Step::Key(group.to_owned()),
                Step::Key(entry.key.value.clone()),
            ];
            let at = with(&base, Step::Key("locate".to_owned()));
            let locate = entry
                .value
                .get("locate")
                .map(Located::items)
                .unwrap_or_default()
                .iter()
                .enumerate()
                .map(|(index, one)| {
                    let here = with(&at, Step::Index(index));
                    let keys: Vec<&str> =
                        one.entries().iter().map(|e| e.key.value.as_str()).collect();
                    match keys.as_slice() {
                        ["role", "name"] | ["name", "role"] => Some(Locator {
                            by: By::RoleName {
                                role: scalar(
                                    one.get("role")?,
                                    with(&here, Step::Key("role".to_owned())),
                                )?,
                                name: scalar(
                                    one.get("name")?,
                                    with(&here, Step::Key("name".to_owned())),
                                )?,
                            },
                        }),
                        ["label"] => Some(Locator {
                            by: By::Label(scalar(
                                one.get("label")?,
                                with(&here, Step::Key("label".to_owned())),
                            )?),
                        }),
                        _ => None,
                    }
                })
                .collect();
            let fingerprint = entry.value.get("fingerprint");
            let print = with(&base, Step::Key("fingerprint".to_owned()));
            let section = fingerprint.and_then(|fingerprint| {
                let section = fingerprint.get("section")?;
                let here = with(&print, Step::Key("section".to_owned()));
                Some((
                    scalar(
                        section.get("role")?,
                        with(&here, Step::Key("role".to_owned())),
                    )?,
                    scalar(
                        section.get("name")?,
                        with(&here, Step::Key("name".to_owned())),
                    )?,
                ))
            });
            parts.push(Part {
                what: format!("the {noun} `{}`", entry.key.value),
                locate,
                section,
                near_text: fingerprint
                    .and_then(|fingerprint| fingerprint.text("near_text"))
                    .map(str::to_owned),
            });
        }
    }
    parts
}

// ---- the rule ----

/// One change to the file: a scalar and its new text.
struct Edit {
    scalar: Scalar,
    new: String,
}

fn propose(ask: &RepairAsk<'_>) -> Result<RepairProposal, RepairProposal> {
    let (file, text, screen) = screen_of(ask)?;
    let before = ask
        .before
        .ok_or_else(|| {
            none(
                "No snapshot of the page from before the change was given, so what each element of the screen was, and so what it is now, is not known.",
            )
        })
        .and_then(|before| Shape::read(before, "from before"))?;
    let now = Shape::read(ask.now, "of now")?;
    let parts = parts_of(&screen);
    let mut edits = Vec::new();
    let mut because = Vec::new();
    for part in &parts {
        match now.found(part) {
            None => renamed(part, &before, &now, &mut edits, &mut because)?,
            Some(found) => moved(part, found, &before, &now, &mut edits, &mut because)?,
        }
    }
    if edits.is_empty() {
        return Err(none(
            "Every locator of the screen finds one element of the page now, and no fingerprint the two snapshots can check has moved: the drift is not one these rules see.",
        ));
    }
    let changed = apply(&text, &edits)
        .filter(|changed| reads_as_meant(&file, ask.entry, &text, changed, &edits))
        .ok_or_else(|| {
            none("The file could not be changed in place so that it says exactly what was meant.")
        })?;
    Ok(RepairProposal::Proposed {
        path: file.path.clone(),
        text: changed,
        because,
    })
}

/// The one file that defines the screen the drift names, its text, and
/// the screen's document.
fn screen_of(ask: &RepairAsk<'_>) -> Result<(RepairFile, String, Located<Doc>), RepairProposal> {
    let mut defined = Vec::new();
    for file in ask.files {
        let Ok(path) = RelPath::new(&file.path) else {
            continue;
        };
        let text = normalize_text(&file.text);
        let Ok(docs) = parse_yaml(&path, &text) else {
            continue;
        };
        for doc in docs {
            if doc.text("screen") == Some(ask.entry) {
                defined.push((file.clone(), text.clone(), doc));
            }
        }
    }
    let count = defined.len();
    match defined.pop() {
        Some(one) if count == 1 => Ok(one),
        Some(_) => Err(none(format!(
            "{count} drafts define the screen `{}`; a repair would not know which to change.",
            ask.entry
        ))),
        None => Err(none(format!(
            "No draft that reads defines the screen `{}`.",
            ask.entry
        ))),
    }
}

/// A part no locator finds now: renamed, when the pairing is one to one.
fn renamed(
    part: &Part,
    before: &Shape,
    now: &Shape,
    edits: &mut Vec<Edit>,
    because: &mut Vec<String>,
) -> Result<(), RepairProposal> {
    if part.locate.iter().any(Option::is_none) {
        return Err(none(format!(
            "No locator of {} finds an element now, and one of them is of a kind these rules do not read.",
            part.what
        )));
    }
    let Some(old) = before.found(part) else {
        return Err(none(format!(
            "No locator of {} finds an element now, and none found one in the snapshot from before either.",
            part.what
        )));
    };
    let role = before.elements[old].role.clone();
    let Some(name) = before.elements[old].name.clone() else {
        return Err(none(format!(
            "The element {} found before has no name the snapshot keeps.",
            part.what
        )));
    };
    let section = before.section_of(old);
    let was = before.names_in(&role, section.as_ref());
    let is = now.names_in(&role, section.as_ref());
    let gone: Vec<&String> = was.iter().filter(|one| !is.contains(one)).collect();
    let new: Vec<&String> = is.iter().filter(|one| !was.contains(one)).collect();
    let (true, [new]) = (gone == [&name], new.as_slice()) else {
        return Err(none(format!(
            "No locator of {} finds an element now. In {}, {} element(s) of the role `{role}` are gone and {} are new, so which one is the element it named is not sure.",
            part.what,
            shown_section(section.as_ref()),
            gone.len(),
            new.len()
        )));
    };
    let new = (*new).clone();
    for locator in part.locate.iter().flatten() {
        let named = match &locator.by {
            By::RoleName { name: named, .. } | By::Label(named) => named,
        };
        if named.text == name {
            edits.push(Edit {
                scalar: named.clone(),
                new: new.clone(),
            });
        }
    }
    if now.find_named(&role, &new).len() != 1 {
        return Err(none(format!(
            "The element of {} now seems to be named `{new}`, and that name does not find exactly one element of the page.",
            part.what
        )));
    }
    because.push(format!(
        "{}: the {role} `{name}` is gone from {} and the one {role} new there is `{new}`.",
        capital(&part.what),
        shown_section(section.as_ref())
    ));
    Ok(())
}

/// A part found now whose fingerprint's text left its section.
fn moved(
    part: &Part,
    found: usize,
    before: &Shape,
    now: &Shape,
    edits: &mut Vec<Edit>,
    because: &mut Vec<String>,
) -> Result<(), RepairProposal> {
    let (Some((role, name)), Some(text)) = (&part.section, &part.near_text) else {
        return Ok(());
    };
    let section: Section = (role.text.clone(), name.text.clone());
    // What the text was, before: the name of an element in the section.
    let Some(old) = before.found(part) else {
        return Ok(());
    };
    let Some(held_role) = before
        .above_named(old, &section)
        .and_then(|within| before.role_held(within, text))
        .map(str::to_owned)
    else {
        // The text was no element's name, so the snapshots cannot say
        // whether it moved.
        return Ok(());
    };
    if now
        .above_named(found, &section)
        .is_some_and(|within| now.holds(within, &held_role, text))
    {
        return Ok(());
    }
    let Some(into) = now.elements[found]
        .above
        .iter()
        .rev()
        .copied()
        .filter(|&above| is_section(&now.elements[above].role))
        .find(|&above| now.holds(above, &held_role, text))
    else {
        return Err(none(format!(
            "The fingerprint of {} names the {held_role} `{text}` in {}, and no section around it holds that now.",
            part.what,
            shown_section(Some(&section))
        )));
    };
    let into_section = (
        now.elements[into].role.clone(),
        now.elements[into].name.clone().unwrap_or_default(),
    );
    if into_section.0 != section.0 {
        edits.push(Edit {
            scalar: role.clone(),
            new: into_section.0.clone(),
        });
    }
    if into_section.1 != section.1 {
        edits.push(Edit {
            scalar: name.clone(),
            new: into_section.1.clone(),
        });
    }
    because.push(format!(
        "{}: the {held_role} `{text}` its fingerprint names is no longer in {}; the nearest section around it that holds it is {}.",
        capital(&part.what),
        shown_section(Some(&section)),
        shown_section(Some(&into_section))
    ));
    Ok(())
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

// ---- the edit ----

/// A text as a scalar of the file: plain where plain reads the same in a
/// flow and in a block, quoted otherwise.
fn written(text: &str) -> String {
    let plain = text.chars().next().is_some_and(char::is_alphabetic)
        && !text.ends_with(' ')
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
        && !text.contains("  ")
        && !matches!(
            text.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "y" | "n"
        );
    if plain {
        text.to_owned()
    } else {
        serde_json::to_string(text).unwrap_or_else(|_| format!("\"{text}\""))
    }
}

/// Where a scalar written at `column` (in characters, from 1) of `line`
/// ends, in bytes from the start of the line, and where it starts.
fn span(line: &str, column: u32) -> Option<(usize, usize)> {
    let start = line
        .char_indices()
        .nth(usize::try_from(column).ok()?.checked_sub(1)?)
        .map(|(at, _)| at)?;
    let rest = &line[start..];
    let mut chars = rest.char_indices();
    let length = match chars.next()?.1 {
        '"' => {
            let mut escaped = false;
            chars
                .find(|&(_, c)| {
                    let closes = c == '"' && !escaped;
                    escaped = c == '\\' && !escaped;
                    closes
                })
                .map(|(at, _)| at + 1)?
        }
        '\'' => {
            let bytes = rest.as_bytes();
            let mut at = 1;
            loop {
                match bytes.get(at) {
                    Some(b'\'') if bytes.get(at + 1) == Some(&b'\'') => at += 2,
                    Some(b'\'') => break at + 1,
                    Some(_) => at += 1,
                    None => return None,
                }
            }
        }
        _ => {
            let end = rest
                .char_indices()
                .find(|&(at, c)| {
                    matches!(c, ',' | '}' | ']') || (c == '#' && rest[..at].ends_with(' '))
                })
                .map_or(rest.len(), |(at, _)| at);
            rest[..end].trim_end().len()
        }
    };
    Some((start, start + length))
}

fn apply(text: &str, edits: &[Edit]) -> Option<String> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let mut by_line: BTreeMap<usize, Vec<&Edit>> = BTreeMap::new();
    for edit in edits {
        let line = usize::try_from(edit.scalar.at.line).ok()?.checked_sub(1)?;
        by_line.entry(line).or_default().push(edit);
    }
    for (line, mut edits) in by_line {
        // From the right, so that each column still holds.
        edits.sort_by_key(|a| std::cmp::Reverse(a.scalar.at.column));
        edits.dedup_by(|a, b| a.scalar.at == b.scalar.at);
        let held = lines.get_mut(line)?;
        for edit in edits {
            let (start, end) = span(held, edit.scalar.at.column)?;
            held.replace_range(start..end, &written(&edit.new));
        }
    }
    Some(lines.join("\n"))
}

/// Whether `changed` reads as `text` does with each edit's scalar holding
/// its new text in the screen's document, and nothing else different.
fn reads_as_meant(
    file: &RepairFile,
    entry: &str,
    text: &str,
    changed: &str,
    edits: &[Edit],
) -> bool {
    let Ok(path) = RelPath::new(&file.path) else {
        return false;
    };
    let (Ok(was), Ok(is)) = (parse_yaml(&path, text), parse_yaml(&path, changed)) else {
        return false;
    };
    if was.len() != is.len() {
        return false;
    }
    was.iter().zip(&is).all(|(was, is)| {
        let mut meant = was.to_json();
        if was.text("screen") == Some(entry) {
            for edit in edits {
                match at_path(&mut meant, &edit.scalar.path) {
                    Some(value) if value.as_str() == Some(edit.scalar.text.as_str()) => {
                        *value = Json::from(edit.new.clone());
                    }
                    _ => return false,
                }
            }
        }
        meant == is.to_json()
    })
}

fn at_path<'a>(doc: &'a mut Json, path: &[Step]) -> Option<&'a mut Json> {
    let mut at = doc;
    for step in path {
        at = match (step, at) {
            (Step::Key(key), Json::Object(map)) => map.get_mut(key)?,
            (Step::Index(index), Json::Array(items)) => items.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(at)
}
