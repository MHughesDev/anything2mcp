//! What the pack writes into a compiled target and its driver reads back:
//! compiled screens, and the payload of each step. The kernel stores and
//! hashes these and never decodes them.

use std::collections::BTreeMap;

use a2m_pack_web_browser::{By, Primitive, Query, ReadWhat, TextMatch};
use serde::{Deserialize, Serialize};

/// The version of the shapes below. A compiled target written in another is
/// refused.
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

    /// Puts every locator of the screen inside the frame the screen is in
    /// (spec section 301): the screen's own words stay the page's, and the
    /// browser is asked for the frame's document.
    pub(crate) fn inside(&mut self, frame: &Locator) {
        self.ready.inside(frame);
        let fingerprint = |print: &mut Option<Fingerprint>| {
            if let Some(print) = print {
                print.section.inside(frame);
                print.frame = Some(frame.clone());
            }
        };
        // An element held to a fingerprint is looked for inside the
        // fingerprint's section, which is in the frame; its own locators
        // keep no scope of their own, which the driver would refuse.
        let locators = |locate: &mut [Locator], print: &Option<Fingerprint>| {
            if print.is_none() {
                locate.iter_mut().for_each(|locator| locator.inside(frame));
            }
        };
        for field in self.fields.values_mut() {
            locators(&mut field.locate, &field.fingerprint);
            fingerprint(&mut field.fingerprint);
        }
        for action in self.actions.values_mut() {
            locators(&mut action.locate, &action.fingerprint);
            fingerprint(&mut action.fingerprint);
            action.confirm.inside(frame);
        }
        if let Some(rows) = &mut self.rows {
            locators(&mut rows.locate, &rows.fingerprint);
            fingerprint(&mut rows.fingerprint);
        }
        for variant in self.variants.values_mut() {
            variant.when.inside(frame);
        }
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

    /// Puts the locator inside a frame: the end of its chain of scopes is
    /// inside it, so that the whole match is made in the frame's document.
    pub(crate) fn inside(&mut self, frame: &Locator) {
        match &mut self.within {
            Some(scope) => scope.inside(frame),
            None => self.within = Some(Box::new(frame.clone())),
        }
    }

    /// The base's query for this locator, in the frame when there is one.
    pub(crate) fn query_in(&self, frame: Option<&Locator>) -> Query {
        match frame {
            Some(frame) => {
                let mut framed = self.clone();
                framed.inside(frame);
                framed.query()
            }
            None => self.query(),
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
    /// The frame the screen is in, when it is in one: the element is first
    /// looked for there, and then held to the section, which is there too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Locator>,
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
    /// Puts every locator of the condition inside a frame.
    pub(crate) fn inside(&mut self, frame: &Locator) {
        match self {
            Self::Visible(locator) | Self::Hidden(locator) | Self::Enabled(locator) => {
                locator.inside(frame);
            }
            Self::Text { locate, .. } => locate.inside(frame),
            Self::All(of) | Self::Any(of) => of.iter_mut().for_each(|cond| cond.inside(frame)),
        }
    }

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
    /// For a `radio`: the label of each choice, by the code it stores. The
    /// field's locators find the group; each choice is the radio button of
    /// that label inside it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// For a `grid`: the place of the field's cell among the cells of a
    /// row, counted from zero. The field's locators find the grid; the row
    /// is the one whose first cell shows the key the binding gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
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
    /// The dialog of the browser the action's click is expected to open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<NativeDialog>,
    /// The download the action's click is expected to start: the one file
    /// the page may offer, and at most this many bytes of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<DownloadDecl>,
}

/// A download an action declares (decision 0362, the download record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DownloadDecl {
    /// The most bytes the file may hold.
    pub most_bytes: u64,
    /// The output that takes the name of the file the engine made.
    pub into: Option<String>,
}

/// A dialog of the browser, such as a confirmation before saving, that an
/// action declares as expected: what it says, and how it is answered. A
/// dialog that says anything else is not answered (rule 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NativeDialog {
    /// What the dialog must say, exactly.
    pub says: String,
    /// Whether it is accepted; otherwise it is dismissed.
    pub accept: bool,
}

/// What a variant means for the step that meets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Handling {
    /// The record the screen shows does not exist.
    Absent,
    /// The application refused; the run stops.
    Stop,
    /// A question the page asks of the action just performed, which the
    /// variant's `with` action answers by agreeing (decision 0350).
    Confirm,
    /// A state in the way, such as a notice over the form, which the
    /// variant's `with` action closes.
    Dismiss,
}

/// A state a screen may be in instead of the one its fields describe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Variant {
    /// It is present when at least one element matches and is shown.
    pub when: Locator,
    /// What it means.
    pub handling: Handling,
    /// For `confirm` and `dismiss`: the action of the screen that answers it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<String>,
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

/// Which tab a `goto` step reaches its screen in (decision 0195). The
/// lowering knows what each binding does, and says it here: every step of
/// a read, and every step that reads a written record back, is in a tab
/// that cannot write; only the steps of a write up to its commit point are
/// in one that may.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TabKind {
    /// An observing tab: the browser fails every request of the page whose
    /// method could change the system.
    Observe,
    /// An acting tab, for the steps of an operation that writes.
    Act,
}

/// The payload of a `goto` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GotoStep {
    /// The tab the screen is reached in, and the steps after it act in.
    pub tab: TabKind,
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

/// The payload of a `call_tool` step: the tool a page registered, called
/// by its name, once the page shows the digest the binding holds. Each
/// argument arrives as the step's argument `arg.<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CallToolStep {
    /// The tool's name.
    pub tool: String,
    /// The digest of the tool's name, description, and input schema, as 64
    /// lowercase hexadecimal characters.
    pub digest: String,
    /// The names of the arguments, in the order the binding wrote them.
    pub args: Vec<String>,
    /// The output that takes the tool's text, when the operation has one.
    pub result: Option<String>,
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
    /// Origins the surface's pages are refused without a stop: a request
    /// of the page to one is failed in the browser, nothing is reported,
    /// and the run goes on (package P3b). Each is an origin with its
    /// scheme.
    #[serde(default)]
    pub refuse_quietly: Vec<String>,
    /// The platform profile the surface names: one of `profiles/`, or
    /// `auto`, which the probe is to settle (package E2).
    pub profile: Option<String>,
}

/// A viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct Viewport {
    pub width: u32,
    pub height: u32,
}
