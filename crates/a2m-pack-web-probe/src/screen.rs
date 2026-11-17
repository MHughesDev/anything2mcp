//! A snapshot as the `found` of a `screen` finding: the structure of a
//! screen, with every value replaced by its type.
//!
//! A screen's structure is what names it: its headings, its controls and
//! their labels, its column headers. What fills it is data: the text of a
//! cell, the value of a control, the name of a link in a row. The first is
//! kept; the second is kept as a type.

use a2m_pack_web_browser::{ElementState, MASK, Snapshot, SnapshotElement};
use serde_json::Value as Json;

use crate::shape::{object, sorted, type_of_text};

/// The roles whose accessible name is part of the screen's structure and
/// is kept. The name of any other role is kept as its type. Data.
pub const NAMED_ROLES: &[&str] = &[
    "banner",
    "button",
    "checkbox",
    "columnheader",
    "combobox",
    "complementary",
    "contentinfo",
    "dialog",
    "form",
    "grid",
    "group",
    "heading",
    "listbox",
    "main",
    "menu",
    "menubar",
    "menuitem",
    "navigation",
    "radio",
    "radiogroup",
    "region",
    "search",
    "searchbox",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "table",
    "tablist",
    "textbox",
    "toolbar",
    "tree",
];

/// The roles that hold one record among several. A link inside one is
/// named by the record, so its name is data.
pub const RECORD_ROLES: &[&str] = &["row", "listitem", "option", "treeitem"];

/// The roles of a labelled value: a description list's term, which is
/// named by what it shows, its label, and a definition, which the browser
/// names only from a label the page gives it, never from what it holds.
/// Their names are kept outside a record, as a link's are. Data.
pub const LABELLED_ROLES: &[&str] = &["term", "definition"];

/// The roles of an element that marks out a part of a page. Data.
pub const SECTION_ROLES: &[&str] = &[
    "main",
    "region",
    "navigation",
    "complementary",
    "banner",
    "contentinfo",
    "search",
    "dialog",
];

/// The roles of controls a person fills or sets.
pub const FIELD_ROLES: &[&str] = &[
    "checkbox",
    "combobox",
    "listbox",
    "radio",
    "searchbox",
    "slider",
    "spinbutton",
    "switch",
    "textbox",
];

fn state(state: ElementState) -> &'static str {
    match state {
        ElementState::Disabled => "disabled",
        ElementState::Checked => "checked",
        ElementState::Expanded => "expanded",
        ElementState::Required => "required",
        ElementState::Invalid => "invalid",
    }
}

fn element(one: &SnapshotElement, in_record: bool) -> Json {
    let mut out = serde_json::Map::new();
    out.insert("depth".to_owned(), Json::from(one.depth));
    out.insert("role".to_owned(), Json::from(one.role.as_str()));
    let named = NAMED_ROLES.contains(&one.role.as_str())
        || ((one.role == "link" || LABELLED_ROLES.contains(&one.role.as_str())) && !in_record);
    if named {
        out.insert("name".to_owned(), Json::from(one.name.as_str()));
    } else if !one.name.trim().is_empty() {
        out.insert("name_type".to_owned(), Json::from(type_of_text(&one.name)));
    }
    if let Some(value) = &one.value {
        out.insert(
            "value_type".to_owned(),
            Json::from(if value == MASK {
                "password"
            } else {
                type_of_text(value)
            }),
        );
    }
    if !one.states.is_empty() {
        out.insert(
            "states".to_owned(),
            Json::Array(one.states.iter().map(|s| Json::from(state(*s))).collect()),
        );
    }
    if let Some(level) = one.level {
        out.insert("level".to_owned(), Json::from(level));
    }
    Json::Object(sorted(out))
}

/// The snapshot as the `found` of a `screen` finding.
#[must_use]
pub fn found(snapshot: &Snapshot, title: &str) -> Json {
    // The roles above the element at hand, by depth.
    let mut above: Vec<&str> = Vec::new();
    let mut elements = Vec::new();
    for one in &snapshot.elements {
        above.truncate(one.depth);
        let in_record = above.iter().any(|role| RECORD_ROLES.contains(role));
        elements.push(element(one, in_record));
        above.push(one.role.as_str());
    }
    object([
        ("format", Json::from("1")),
        ("title", Json::from(title)),
        ("cut", Json::Bool(snapshot.cut)),
        ("elements", Json::Array(elements)),
    ])
}

/// The names of the links of a snapshot that can be told apart: each
/// name once, and only a name no other link shares.
#[must_use]
pub fn link_names(snapshot: &Snapshot) -> (Vec<&str>, u64) {
    let mut names: Vec<&str> = snapshot
        .elements
        .iter()
        .filter(|one| one.role == "link" && !one.name.trim().is_empty())
        .map(|one| one.name.as_str())
        .collect();
    let all = names.len();
    names.sort_unstable();
    let single: Vec<&str> = names
        .iter()
        .enumerate()
        .filter(|(at, name)| {
            (*at == 0 || names[at - 1] != **name)
                && names.get(at + 1).is_none_or(|next| next != *name)
        })
        .map(|(_, name)| *name)
        .collect();
    let shared = u64::try_from(all - single.len()).unwrap_or(u64::MAX);
    (single, shared)
}

/// What makes two snapshots one screen: each named table with its column
/// headings, each labelled value, and each part of the page they are in,
/// with their depths. `None` when the snapshot holds neither a named table
/// nor a labelled value, or was cut short: such pages are never joined.
#[must_use]
pub fn signature(found: &Json) -> Option<String> {
    if found.get("cut").and_then(Json::as_bool) != Some(false) {
        return None;
    }
    let mut kept = Vec::new();
    let mut holds = false;
    for one in found.get("elements")?.as_array()? {
        let role = one.get("role").and_then(Json::as_str).unwrap_or_default();
        let Some(name) = one.get("name").and_then(Json::as_str) else {
            continue;
        };
        let wanted = matches!(role, "table" | "grid" | "treegrid" | "columnheader")
            || LABELLED_ROLES.contains(&role)
            || SECTION_ROLES.contains(&role);
        if !wanted {
            continue;
        }
        holds |= (role == "table" || role == "term") && !name.trim().is_empty();
        kept.push(Json::Array(vec![
            one.get("depth").cloned().unwrap_or(Json::Null),
            Json::from(role),
            Json::from(name),
        ]));
    }
    holds.then(|| Json::Array(kept).to_string())
}

/// Pages that show one screen at addresses that differ in one segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    /// The path, with the segment that differs as one placeholder.
    pub pattern: String,
    /// The pages it was seen on, by their place in the input, in order of
    /// path.
    pub pages: Vec<usize>,
}

fn plain(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~'))
}

/// The name of the placeholder at `at`: the plain word before it, as one
/// record of it, with `_id`; `key` when there is none.
fn placeholder(segments: &[&str], at: usize) -> String {
    let word: String = at
        .checked_sub(1)
        .and_then(|before| segments.get(before))
        .map(|word| {
            word.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let word = word.trim_matches('_');
    let word = match word.strip_suffix('s') {
        Some(one) if !one.is_empty() && !one.ends_with('s') => one,
        _ => word,
    };
    let fits = word.starts_with(|c: char| c.is_ascii_lowercase())
        && !word.contains("__")
        && word.len() <= 40;
    if fits {
        format!("{word}_id")
    } else {
        "key".to_owned()
    }
}

/// Screen matching over the pages of one discovery, each given as its
/// origin, its path, and its `found`: the pages whose paths have the same
/// number of plain segments and differ in exactly one, and whose snapshots
/// give one [`signature`]. A page that two such groups would hold is in
/// neither; a group of fewer than two pages is none. In order of the
/// first page of each.
#[must_use]
pub fn matched(pages: &[(&str, &str, &Json)]) -> Vec<Matched> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, usize, Vec<&str>, String), Vec<usize>> = BTreeMap::new();
    for (at, (origin, path, found)) in pages.iter().enumerate() {
        let Some(signature) = signature(found) else {
            continue;
        };
        let Some(rest) = path.strip_prefix('/') else {
            continue;
        };
        let segments: Vec<&str> = rest.split('/').collect();
        if rest.is_empty() || !segments.iter().all(|segment| plain(segment)) {
            continue;
        }
        for hole in 0..segments.len() {
            let mut others = segments.clone();
            others[hole] = "";
            groups
                .entry(((*origin).to_owned(), hole, others, signature.clone()))
                .or_default()
                .push(at);
        }
    }
    groups.retain(|_, members| members.len() >= 2);
    let mut held: BTreeMap<usize, u32> = BTreeMap::new();
    for members in groups.values() {
        for at in members {
            *held.entry(*at).or_default() += 1;
        }
    }
    let mut out: Vec<Matched> = groups
        .into_iter()
        .filter_map(|((_, hole, _, _), members)| {
            let mut members: Vec<usize> = members
                .into_iter()
                .filter(|at| held.get(at) == Some(&1))
                .collect();
            if members.len() < 2 {
                return None;
            }
            members.sort_by(|a, b| pages[*a].1.cmp(pages[*b].1));
            let first = pages[members[0]].1;
            let segments: Vec<&str> = first.trim_start_matches('/').split('/').collect();
            let name = placeholder(&segments, hole);
            let pattern: Vec<String> = segments
                .iter()
                .enumerate()
                .map(|(at, segment)| {
                    if at == hole {
                        format!("{{{name}}}")
                    } else {
                        (*segment).to_owned()
                    }
                })
                .collect();
            Some(Matched {
                pattern: format!("/{}", pattern.join("/")),
                pages: members,
            })
        })
        .collect();
    out.sort_by_key(|group| group.pages.iter().min().copied());
    out
}
