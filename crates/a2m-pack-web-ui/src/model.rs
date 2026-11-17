//! What the pack writes into a plan and its driver reads back: compiled
//! screens, and the payload of each step. The kernel stores and hashes
//! these and never decodes them.

use std::collections::BTreeMap;

use a2m_pack_web_browser::{By, Primitive, Query, ReadWhat, TextMatch};
use serde::{Deserialize, Serialize};

/// The version of the shapes below. A plan written in another is refused.
pub(crate) const PAYLOAD_FORMAT: u16 = 1;

/// Every screen of the target's web surfaces, by name.
pub(crate) type ScreenSection = BTreeMap<String, Screen>;

/// One compiled screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Screen {
    /// The surface it lives on.
    pub surface: String,
    /// The path of its address, segment by segment.
    pub reach: Vec<Segment>,
    /// True when the screen can be used.
    pub ready: Cond,
    /// Its fields, by name.
    pub fields: BTreeMap<String, Field>,
    /// Its actions, by name.
    pub actions: BTreeMap<String, Action>,
    /// Its table of rows, when it has one.
    pub rows: Option<Rows>,
    /// The states it may be in instead, by name.
    pub variants: BTreeMap<String, Variant>,
}

impl Screen {
    /// The names of the placeholders of its address, in order.
    pub(crate) fn holes(&self) -> impl Iterator<Item = &str> {
        self.reach.iter().filter_map(|segment| match segment {
            Segment::Hole(name) => Some(name.as_str()),
            Segment::Literal(_) => None,
        })
    }

    /// The address as the file wrote it, with its placeholders unfilled.
    pub(crate) fn template(&self) -> String {
        let mut out = String::new();
        for segment in &self.reach {
            out.push('/');
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Hole(name) => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            }
        }
        if out.is_empty() {
            out.push('/');
        }
        out
    }

    /// Whether a variant says the record the screen shows does not exist.
    pub(crate) fn can_be_absent(&self) -> bool {
        self.variants
            .values()
            .any(|variant| variant.handling == Handling::Absent)
    }
}

/// One segment of a screen's address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Segment {
    /// Text that stands as written.
    Literal(String),
    /// A placeholder, filled by the input of this name.
    Hole(String),
}

/// How one element is matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocateBy {
    /// Accessibility role and accessible name.
    Role { role: String, name: String },
    /// The text of the label of a control.
    Label(String),
    /// Placeholder text.
    Placeholder(String),
    /// Visible text.
    Text { text: String, contains: bool },
    /// A test identifier.
    TestId(String),
    /// Title text.
    Title(String),
    /// A selector.
    Css(String),
}

/// One locator of a screen model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Locator {
    /// How the element is matched.
    pub by: LocateBy,
    /// The one element it must be inside.
    pub within: Option<Box<Locator>>,
}

impl Locator {
    fn by(&self) -> By {
        match &self.by {
            LocateBy::Role { role, name } => By::RoleName {
                role: role.clone(),
                name: name.clone(),
            },
            LocateBy::Label(text) => By::Label { text: text.clone() },
            LocateBy::Placeholder(text) => By::Placeholder { text: text.clone() },
            LocateBy::Text { text, contains } => By::Text {
                text: text.clone(),
                contains: *contains,
            },
            LocateBy::TestId(id) => By::TestId { id: id.clone() },
            LocateBy::Title(text) => By::Title { text: text.clone() },
            LocateBy::Css(selector) => By::Css {
                selector: selector.clone(),
            },
        }
    }

    /// The base's query for this locator.
    pub(crate) fn query(&self) -> Query {
        let query = Query::new(self.by());
        match &self.within {
            Some(scope) => query.within(scope.query()),
            None => query,
        }
    }

    /// The same match, inside the one element `section` matches. Used only
    /// for a locator with no `within` of its own.
    pub(crate) fn query_inside(&self, section: &Locator) -> Query {
        Query::new(self.by()).within(section.query())
    }
}

/// What must also be true of a matched element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Fingerprint {
    /// The one element it must be inside.
    pub section: Locator,
    /// Text that must be shown inside that section.
    pub near_text: Option<String>,
}

/// A condition of a screen. Each is one the base waits on by an observer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cond {
    /// At least one element matches and is shown.
    Visible(Locator),
    /// No element matches.
    Hidden(Locator),
    /// Exactly one matches and it can be used.
    Enabled(Locator),
    /// Exactly one matches and its text, or its value, is the text given.
    Text {
        locate: Locator,
        text: String,
        contains: bool,
    },
    /// Every one holds.
    All(Vec<Cond>),
    /// At least one holds.
    Any(Vec<Cond>),
}

impl Cond {
    /// The base's condition.
    pub(crate) fn primitive(&self) -> Primitive {
        match self {
            Self::Visible(locator) => Primitive::Visible(locator.query()),
            Self::Hidden(locator) => Primitive::Hidden(locator.query()),
            Self::Enabled(locator) => Primitive::Enabled(locator.query()),
            Self::Text {
                locate,
                text,
                contains,
            } => Primitive::Text(
                locate.query(),
                TextMatch {
                    text: text.clone(),
                    contains: *contains,
                },
            ),
            Self::All(of) => Primitive::All(of.iter().map(Self::primitive).collect()),
            Self::Any(of) => Primitive::Any(of.iter().map(Self::primitive).collect()),
        }
    }
}

/// What a field's rule reads of its element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadHow {
    /// The value of the control.
    Value,
    /// The text a person sees.
    Text,
    /// Whether it is on.
    Checked,
}

impl ReadHow {
    pub(crate) fn what(self) -> ReadWhat {
        match self {
            Self::Value => ReadWhat::Value,
            Self::Text => ReadWhat::Text,
            Self::Checked => ReadWhat::Checked,
        }
    }
}

/// One field of a screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Field {
    /// The name of its control. The driver has a rule for each name it
    /// registered, and stops at any other.
    pub control: String,
    /// Its locators, in order.
    pub locate: Vec<Locator>,
    /// What must also be true of the element.
    pub fingerprint: Option<Fingerprint>,
    /// What is read of it.
    pub read: ReadHow,
}

/// One action of a screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Action {
    /// Its locators, in order.
    pub locate: Vec<Locator>,
    /// What must also be true of the element.
    pub fingerprint: Option<Fingerprint>,
    /// The condition that shows the action took effect.
    pub confirm: Cond,
    /// Whether it saves in the application.
    pub commits: bool,
}

/// What a variant means for the step that meets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Handling {
    /// The record the screen shows does not exist.
    Absent,
    /// The application refused; the run stops.
    Stop,
}

/// A state a screen may be in instead of the one its fields describe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Variant {
    /// It is present when at least one element matches and is shown.
    pub when: Locator,
    /// What it means.
    pub handling: Handling,
}

/// The table of a screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Rows {
    /// Its locators, in order.
    pub locate: Vec<Locator>,
    /// What must also be true of the table.
    pub fingerprint: Option<Fingerprint>,
    /// The text of every column's heading, in the table's order.
    pub header: Vec<String>,
    /// The named columns: each name with the position of its heading.
    pub columns: BTreeMap<String, u32>,
}

/// The payload of a `wait` step: wait for the screen's `ready`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct WaitStep {
    /// Whether a snapshot of the page is kept as evidence.
    pub snapshot: bool,
}

/// One value a `read` step gives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReadItem {
    /// The name the value is returned under.
    pub output: String,
    /// The field, or the column, it is read from.
    pub from: String,
}

/// The payload of a `read` step.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct ReadStep {
    /// The fields read, each once.
    pub fields: Vec<ReadItem>,
    /// The columns of the screen's table read, each as a list.
    pub columns: Vec<ReadItem>,
}

/// The payload of a `set` step. The value arrives as the argument `value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SetStep {
    /// The field that is set.
    pub field: String,
}

/// The payload of an `invoke` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InvokeStep {
    /// The action that is performed.
    pub action: String,
}

/// The fixed environment of a `web` surface: the fields the surface kind
/// adds to a surface beside its address.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub(crate) struct SurfaceFields {
    /// The viewport, in CSS pixels.
    pub viewport: Option<Viewport>,
    /// `light` or `dark`.
    pub color_scheme: Option<String>,
    /// Whether the page is told the person prefers reduced motion.
    pub reduced_motion: Option<bool>,
}

/// A viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct Viewport {
    pub width: u32,
    pub height: u32,
}
