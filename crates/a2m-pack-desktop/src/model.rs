//! What the pack writes into a plan and its driver reads back: compiled
//! screens, the payload of each step, and the surface's own fields. The
//! kernel stores and hashes these and never decodes them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The version of the shapes below. A plan written in another is refused.
/// Format 2 added a screen's actions and variants, and the payloads of
/// `set` and `invoke` (decision 0218).
pub(crate) const PAYLOAD_FORMAT: u16 = 2;

/// Every screen of the target's desktop surfaces, by name.
pub(crate) type ScreenSection = BTreeMap<String, Screen>;

/// One compiled screen: one window of the application, and what is read
/// in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Screen {
    /// The surface it lives on.
    pub surface: String,
    /// The window, among the application's own.
    pub window: WindowMatch,
    /// True when the window can be read.
    pub ready: Cond,
    /// Its fields, by name.
    pub fields: BTreeMap<String, Field>,
    /// Its table of rows, when it has one.
    pub rows: Option<Rows>,
    /// Its actions, by name.
    pub actions: BTreeMap<String, Action>,
    /// The states it may show instead of an action's confirmation, by
    /// name. Each stops the run as the application's refusal.
    pub variants: BTreeMap<String, Variant>,
}

/// One action of a screen: the element it is done on, the name of the
/// action the element offers, what the window shows once it is done, and
/// whether it saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Action {
    pub locate: Vec<Locator>,
    /// The name of the element's action, as its interface lists it.
    pub perform: String,
    pub confirm: Cond,
    pub commits: bool,
}

/// A state a screen may show after an action instead of its confirmation:
/// the application's refusal. What the element its condition names shows
/// is carried as the application's own words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Variant {
    pub when: Cond,
}

/// How a screen's window is told from the application's other windows: by
/// its role and its title, exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WindowMatch {
    pub role: String,
    pub title: String,
}

/// One locator: a role and an accessible name, both exact. The only kind a
/// desktop screen has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Locator {
    pub role: String,
    pub name: String,
}

/// One field: the control its value is read through, and the locators
/// tried in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Field {
    pub control: String,
    pub locate: Vec<Locator>,
}

/// A table: where it is, its headings as the window shows them, and the
/// position of each column a read may name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Rows {
    pub locate: Vec<Locator>,
    pub header: Vec<String>,
    pub columns: BTreeMap<String, u32>,
}

/// A condition of a screen, decided by a fresh read of the window each
/// time the application signals a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cond {
    /// At least one element matches and is showing.
    Visible(Locator),
    /// Exactly one element matches and its text is, or holds, the text
    /// given.
    Text {
        locate: Locator,
        text: String,
        contains: bool,
    },
    /// Every one holds.
    All(Vec<Cond>),
}

/// One value a `read` step gives: the output it fills, and the field or
/// column it comes from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct ReadItem {
    pub output: String,
    pub from: String,
}

/// The payload of a `read` step.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct ReadStep {
    /// Outputs read from fields.
    pub fields: Vec<ReadItem>,
    /// Outputs read from the table's columns, each a list.
    pub columns: Vec<ReadItem>,
}

/// The payload of a `set` step: the field of its screen it sets. The value
/// is the step's argument [`VALUE_ARG`](crate::lower::VALUE_ARG).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct SetStep {
    pub field: String,
}

/// The payload of an `invoke` step: the action of its screen it does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct InvokeStep {
    pub action: String,
}

/// The fields of a surface of the kind `desktop` beside its program, which
/// is its address.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct SurfaceFields {
    /// The name the application gives itself on the accessibility bus.
    #[serde(default)]
    pub application: String,
    /// The digest the program must have, when the target states one.
    #[serde(default)]
    pub digest: Option<String>,
}
