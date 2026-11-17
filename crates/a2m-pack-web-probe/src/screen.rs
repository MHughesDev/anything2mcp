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
    let named =
        NAMED_ROLES.contains(&one.role.as_str()) || (one.role == "link" && !in_record);
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
