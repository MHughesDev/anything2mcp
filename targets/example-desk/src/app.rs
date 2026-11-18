//! The stand's application on the accessibility bus: its tree, the AT-SPI
//! interfaces each node answers, its store, and the counts of what it was
//! asked.
//!
//! The tree, as a toolkit would publish it:
//!
//! ```text
//! application "example-desk"
//!   frame "Example order"
//!     panel ""
//!       label "Order number"  -> entry (labelled by it) "A-100"
//!       label "Customer"      -> entry: the stored customer
//!       label "Status"        -> entry: the stored status
//!       label "Tracking"      -> entry ""   (shows LATE when told to)
//!       entry, with no name and no label
//!       label "Reference"     -> entry "R-1"
//!       label "Reference"     -> entry "R-2"
//!       label "Access code"   -> password text, holding SECRET, editable
//!       table "Line items"
//!         table row: column headers "Item code", "Description", "Quantity"
//!         table row: cells, one row for each line
//!       push button "Save"
//!       label "New customer"  -> editable entry, holding at most CUSTOMER_MOST characters
//!       label "New status"    -> editable entry
//!       label "Message"       -> entry: "Saved." once Save has stored
//!       label "Problem"       -> entry, showing only while the application refuses a value
//! ```
//!
//! What is typed into `New customer` and `New status` is stored only when
//! Save is pressed and the values pass the application's own rule; the
//! entries labelled `Customer` and `Status` show what is stored, so they
//! differ from the edit fields until Save. A restarted application shows
//! the stored values in both.
//!
//! The password field answers its text to anyone who asks, which is worse
//! than a real toolkit does, so that a test can see that nobody asked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use zbus::{Connection, OwnedObjectPath, Variant, interface};

use crate::{CUSTOMER_MOST, Fault, FieldSet, LATE, Order, SAVED, SECRET, StandError, Truth};

/// AT-SPI role numbers, as the interface's enumeration gives them.
mod role {
    pub const FRAME: u32 = 23;
    pub const LABEL: u32 = 29;
    pub const PANEL: u32 = 39;
    pub const PASSWORD_TEXT: u32 = 40;
    pub const PUSH_BUTTON: u32 = 43;
    pub const TABLE: u32 = 55;
    pub const TABLE_CELL: u32 = 56;
    pub const TABLE_COLUMN_HEADER: u32 = 57;
    pub const APPLICATION: u32 = 75;
    pub const ENTRY: u32 = 79;
    pub const TABLE_ROW: u32 = 90;
}

/// AT-SPI state bits.
mod state {
    pub const EDITABLE: u64 = 1 << 7;
    pub const ENABLED: u64 = 1 << 8;
    pub const FOCUSABLE: u64 = 1 << 11;
    pub const SENSITIVE: u64 = 1 << 24;
    pub const SHOWING: u64 = 1 << 25;
    pub const SINGLE_LINE: u64 = 1 << 26;
    pub const VISIBLE: u64 = 1 << 30;
    pub const READ_ONLY: u64 = 1 << 43;
}

/// AT-SPI relation numbers.
const LABEL_FOR: u32 = 1;
const LABELLED_BY: u32 = 2;

const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const EVENT_OBJECT: &str = "org.a11y.atspi.Event.Object";

/// What the application says when a status is not one it knows.
pub(crate) const STATUS_REFUSED: &str = "Status must be open, held, or closed.";
/// What the application says when a customer is empty.
pub(crate) const CUSTOMER_REFUSED: &str = "Customer must not be empty.";
/// The statuses the application stores.
const STATUSES: &[&str] = &["open", "held", "closed"];

fn path_of(id: usize) -> String {
    if id == 0 {
        ROOT_PATH.to_owned()
    } else {
        format!("/org/a11y/atspi/accessible/{id}")
    }
}

#[derive(Debug, Clone, Default)]
struct Node {
    role: u32,
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
    states: u64,
    /// What it answers through the Text interface, for a node that has it.
    text: Option<String>,
    labelled_by: Vec<usize>,
    label_for: Vec<usize>,
    /// Whether it answers the Action interface.
    action: bool,
    /// Whether it answers the EditableText interface.
    editable: bool,
    /// The most characters it holds; longer text is cut, as some fields do.
    most: Option<usize>,
    secret: bool,
}

/// The faults a test has switched on, each for the next press of Save.
#[derive(Debug, Default)]
struct Faults {
    silent: bool,
    exit: bool,
    late: Option<Duration>,
}

#[derive(Debug, Default)]
struct Model {
    nodes: Vec<Node>,
    truth: Truth,
    tracking: usize,
    /// The entries that show what is stored.
    customer: usize,
    status: usize,
    /// The edit fields Save stores from.
    new_customer: usize,
    new_status: usize,
    message: usize,
    problem: usize,
    faults: Faults,
}

/// What a press of Save did, for the handler to signal after it lets go
/// of the model.
struct Pressed {
    changed: Vec<usize>,
    late: Option<Duration>,
    exit: bool,
}

impl Model {
    fn add(&mut self, parent: Option<usize>, node: Node) -> usize {
        let id = self.nodes.len();
        let mut node = node;
        node.parent = parent;
        self.nodes.push(node);
        if let Some(parent) = parent {
            self.nodes[parent].children.push(id);
        }
        id
    }

    /// A label and the field it labels, under `parent`.
    fn labelled(&mut self, parent: usize, label: &str, value: &str, kind: Kind) -> usize {
        let shown = state::ENABLED | state::SENSITIVE | state::SHOWING | state::VISIBLE;
        let label_id = self.add(
            Some(parent),
            Node {
                role: role::LABEL,
                name: label.to_owned(),
                states: shown,
                ..Node::default()
            },
        );
        let (role, states, editable, secret) = match kind {
            Kind::Shown => (
                role::ENTRY,
                shown | state::FOCUSABLE | state::SINGLE_LINE | state::READ_ONLY,
                false,
                false,
            ),
            Kind::Edit => (
                role::ENTRY,
                shown | state::FOCUSABLE | state::SINGLE_LINE | state::EDITABLE,
                true,
                false,
            ),
            Kind::Secret => (
                role::PASSWORD_TEXT,
                shown | state::FOCUSABLE | state::SINGLE_LINE | state::EDITABLE,
                true,
                true,
            ),
        };
        let value_id = self.add(
            Some(parent),
            Node {
                role,
                states,
                text: Some(value.to_owned()),
                labelled_by: vec![label_id],
                editable,
                secret,
                ..Node::default()
            },
        );
        self.nodes[label_id].label_for.push(value_id);
        value_id
    }

    fn build(order: &Order) -> Self {
        let shown = state::ENABLED | state::SENSITIVE | state::SHOWING | state::VISIBLE;
        let mut model = Self::default();
        let root = model.add(
            None,
            Node {
                role: role::APPLICATION,
                name: crate::APPLICATION.to_owned(),
                ..Node::default()
            },
        );
        let frame = model.add(
            Some(root),
            Node {
                role: role::FRAME,
                name: crate::WINDOW.to_owned(),
                states: shown,
                ..Node::default()
            },
        );
        let panel = model.add(
            Some(frame),
            Node {
                role: role::PANEL,
                states: shown,
                ..Node::default()
            },
        );
        model.labelled(panel, "Order number", &order.number, Kind::Shown);
        model.customer = model.labelled(panel, "Customer", &order.customer, Kind::Shown);
        model.status = model.labelled(panel, "Status", &order.status, Kind::Shown);
        model.tracking = model.labelled(panel, "Tracking", "", Kind::Shown);
        model.add(
            Some(panel),
            Node {
                role: role::ENTRY,
                states: shown | state::READ_ONLY,
                text: Some("a value nothing labels".to_owned()),
                ..Node::default()
            },
        );
        model.labelled(panel, "Reference", "R-1", Kind::Shown);
        model.labelled(panel, "Reference", "R-2", Kind::Shown);
        model.labelled(panel, "Access code", SECRET, Kind::Secret);
        let table = model.add(
            Some(panel),
            Node {
                role: role::TABLE,
                name: "Line items".to_owned(),
                states: shown,
                ..Node::default()
            },
        );
        let header = model.add(
            Some(table),
            Node {
                role: role::TABLE_ROW,
                states: shown,
                ..Node::default()
            },
        );
        for heading in ["Item code", "Description", "Quantity"] {
            model.add(
                Some(header),
                Node {
                    role: role::TABLE_COLUMN_HEADER,
                    name: heading.to_owned(),
                    states: shown,
                    ..Node::default()
                },
            );
        }
        for line in &order.lines {
            let row = model.add(
                Some(table),
                Node {
                    role: role::TABLE_ROW,
                    states: shown,
                    ..Node::default()
                },
            );
            for cell in [&line.code, &line.description, &line.quantity] {
                model.add(
                    Some(row),
                    Node {
                        role: role::TABLE_CELL,
                        name: cell.clone(),
                        states: shown,
                        ..Node::default()
                    },
                );
            }
        }
        model.add(
            Some(panel),
            Node {
                role: role::PUSH_BUTTON,
                name: "Save".to_owned(),
                states: shown | state::FOCUSABLE,
                action: true,
                ..Node::default()
            },
        );
        // After Save, so that the elements before it keep their places.
        model.new_customer = model.labelled(panel, "New customer", &order.customer, Kind::Edit);
        let most = model.new_customer;
        model.nodes[most].most = Some(CUSTOMER_MOST);
        model.new_status = model.labelled(panel, "New status", &order.status, Kind::Edit);
        model.message = model.labelled(panel, "Message", "", Kind::Shown);
        model.problem = model.labelled(panel, "Problem", "", Kind::Shown);
        model.nodes[model.problem].states &= !state::SHOWING;
        model
            .truth
            .stored
            .insert("customer".to_owned(), order.customer.clone());
        model
            .truth
            .stored
            .insert("status".to_owned(), order.status.clone());
        model
    }

    /// The title of the window a node is in.
    fn window_of(&self, id: usize) -> String {
        let mut at = Some(id);
        while let Some(node) = at {
            if self.nodes[node].role == role::FRAME {
                return self.nodes[node].name.clone();
            }
            at = self.nodes[node].parent;
        }
        String::new()
    }

    /// The name of the label of a field.
    fn label_of(&self, id: usize) -> String {
        self.nodes[id]
            .labelled_by
            .first()
            .map(|label| self.nodes[*label].name.clone())
            .unwrap_or_default()
    }

    fn set_text(&mut self, id: usize, text: &str) -> Vec<usize> {
        let mut changed = Vec::new();
        if self.nodes[id].text.as_deref() != Some(text) {
            self.nodes[id].text = Some(text.to_owned());
            changed.push(id);
        }
        changed
    }

    /// The message and the problem the application shows, cleared, as an
    /// application clears them once a field changes.
    fn clear_said(&mut self) -> Vec<usize> {
        let mut changed = self.set_text(self.message, "");
        let problem = self.problem;
        if self.nodes[problem].states & state::SHOWING != 0 {
            self.nodes[problem].states &= !state::SHOWING;
            changed.push(problem);
        }
        changed.extend(self.set_text(problem, ""));
        changed
    }

    /// A value set into an editable field. `None` when the field takes
    /// none.
    fn set_field(&mut self, id: usize, text: &str) -> Option<Vec<usize>> {
        let node = &self.nodes[id];
        if !node.editable || node.states & state::READ_ONLY != 0 {
            return None;
        }
        self.truth.sets.push(FieldSet {
            window: self.window_of(id),
            field: self.label_of(id),
            value: if node.secret {
                String::new()
            } else {
                text.to_owned()
            },
        });
        let held: String = match node.most {
            Some(most) => text.chars().take(most).collect(),
            None => text.to_owned(),
        };
        let mut changed = self.set_text(id, &held);
        // The second window's field changes nothing the first one shows.
        if self.window_of(id) == crate::WINDOW {
            changed.extend(self.clear_said());
        }
        Some(changed)
    }

    fn press(&mut self) -> Pressed {
        self.truth.writes += 1;
        let faults = std::mem::take(&mut self.faults);
        let mut pressed = Pressed {
            changed: Vec::new(),
            late: None,
            exit: false,
        };
        if faults.silent {
            return pressed;
        }
        let customer = self.nodes[self.new_customer]
            .text
            .clone()
            .unwrap_or_default();
        let status = self.nodes[self.new_status]
            .text
            .clone()
            .unwrap_or_default();
        let refused = if customer.trim().is_empty() {
            Some(CUSTOMER_REFUSED)
        } else if !STATUSES.contains(&status.as_str()) {
            Some(STATUS_REFUSED)
        } else {
            None
        };
        if let Some(said) = refused {
            let problem = self.problem;
            pressed.changed.extend(self.set_text(problem, said));
            self.nodes[problem].states |= state::SHOWING;
            pressed.changed.push(problem);
            return pressed;
        }
        self.truth.stores += 1;
        self.truth
            .stored
            .insert("customer".to_owned(), customer.clone());
        self.truth.stored.insert("status".to_owned(), status.clone());
        let (shown_customer, shown_status) = (self.customer, self.status);
        pressed
            .changed
            .extend(self.set_text(shown_customer, &customer));
        pressed.changed.extend(self.set_text(shown_status, &status));
        if faults.exit {
            pressed.exit = true;
        } else if faults.late.is_some() {
            pressed.late = faults.late;
        } else {
            let message = self.message;
            pressed.changed.extend(self.set_text(message, SAVED));
        }
        pressed
    }

    /// As a restarted application shows itself: the stored values in its
    /// fields, nothing said, no fault.
    fn restarted(&mut self) {
        let customer = self.truth.stored.get("customer").cloned().unwrap_or_default();
        let status = self.truth.stored.get("status").cloned().unwrap_or_default();
        let (new_customer, new_status) = (self.new_customer, self.new_status);
        self.set_text(new_customer, &customer);
        self.set_text(new_status, &status);
        self.clear_said();
        self.faults = Faults::default();
    }
}

/// What a labelled field is.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// It shows a value and takes none.
    Shown,
    /// It takes a value.
    Edit,
    /// It takes a secret, and answers it to anyone who asks.
    Secret,
}

type Shared = Arc<Mutex<Model>>;

fn lock(model: &Shared) -> std::sync::MutexGuard<'_, Model> {
    model.lock().unwrap_or_else(PoisonError::into_inner)
}

fn asked(model: &Shared, member: &str) {
    *lock(model)
        .truth
        .asked
        .entry(member.to_owned())
        .or_default() += 1;
}

type Address = (String, OwnedObjectPath);

fn address(bus: &str, id: usize) -> Address {
    (
        bus.to_owned(),
        OwnedObjectPath::try_from(path_of(id))
            .unwrap_or_else(|_| OwnedObjectPath::try_from(ROOT_PATH).unwrap_or_default()),
    )
}

/// Signals, as a toolkit does, that each of these nodes changed.
async fn signal(conn: &Connection, changed: &[usize]) {
    for id in changed {
        let properties: HashMap<String, Variant> = HashMap::new();
        let _ = conn
            .emit_signal(
                None::<&str>,
                path_of(*id).as_str(),
                EVENT_OBJECT,
                "TextChanged",
                &("insert", 0_i32, 0_i32, Variant::from(""), properties),
            )
            .await;
    }
}

/// `org.a11y.atspi.Accessible`, for one node.
struct Accessible {
    id: usize,
    bus: String,
    desktop: Address,
    model: Shared,
}

impl Accessible {
    fn node(&self) -> Node {
        lock(&self.model).nodes[self.id].clone()
    }
}

#[interface(name = "org.a11y.atspi.Accessible")]
impl Accessible {
    #[zbus(property)]
    fn name(&self) -> String {
        asked(&self.model, "Accessible.Name");
        self.node().name
    }

    #[zbus(property)]
    fn description(&self) -> String {
        asked(&self.model, "Accessible.Description");
        String::new()
    }

    #[zbus(property)]
    fn parent(&self) -> Address {
        asked(&self.model, "Accessible.Parent");
        match self.node().parent {
            Some(parent) => address(&self.bus, parent),
            None => self.desktop.clone(),
        }
    }

    #[zbus(property)]
    fn child_count(&self) -> i32 {
        asked(&self.model, "Accessible.ChildCount");
        i32::try_from(self.node().children.len()).unwrap_or(i32::MAX)
    }

    #[zbus(property)]
    fn locale(&self) -> String {
        "en-US".to_owned()
    }

    #[zbus(property)]
    fn accessible_id(&self) -> String {
        asked(&self.model, "Accessible.AccessibleId");
        String::new()
    }

    fn get_children(&self) -> Vec<Address> {
        asked(&self.model, "Accessible.GetChildren");
        self.node()
            .children
            .iter()
            .map(|child| address(&self.bus, *child))
            .collect()
    }

    fn get_child_at_index(&self, index: i32) -> Address {
        asked(&self.model, "Accessible.GetChildAtIndex");
        usize::try_from(index)
            .ok()
            .and_then(|at| self.node().children.get(at).copied())
            .map_or_else(
                || {
                    (
                        "".to_owned(),
                        OwnedObjectPath::try_from("/org/a11y/atspi/null").unwrap_or_default(),
                    )
                },
                |child| address(&self.bus, child),
            )
    }

    fn get_index_in_parent(&self) -> i32 {
        asked(&self.model, "Accessible.GetIndexInParent");
        let model = lock(&self.model);
        model.nodes[self.id]
            .parent
            .and_then(|parent| {
                model.nodes[parent]
                    .children
                    .iter()
                    .position(|c| *c == self.id)
            })
            .and_then(|at| i32::try_from(at).ok())
            .unwrap_or(-1)
    }

    fn get_relation_set(&self) -> Vec<(u32, Vec<Address>)> {
        asked(&self.model, "Accessible.GetRelationSet");
        let node = self.node();
        let mut set = Vec::new();
        if !node.labelled_by.is_empty() {
            set.push((
                LABELLED_BY,
                node.labelled_by
                    .iter()
                    .map(|id| address(&self.bus, *id))
                    .collect(),
            ));
        }
        if !node.label_for.is_empty() {
            set.push((
                LABEL_FOR,
                node.label_for
                    .iter()
                    .map(|id| address(&self.bus, *id))
                    .collect(),
            ));
        }
        set
    }

    fn get_role(&self) -> u32 {
        asked(&self.model, "Accessible.GetRole");
        self.node().role
    }

    fn get_role_name(&self) -> String {
        asked(&self.model, "Accessible.GetRoleName");
        String::new()
    }

    fn get_localized_role_name(&self) -> String {
        asked(&self.model, "Accessible.GetLocalizedRoleName");
        String::new()
    }

    fn get_state(&self) -> Vec<u32> {
        asked(&self.model, "Accessible.GetState");
        let bits = self.node().states;
        vec![(bits & 0xffff_ffff) as u32, (bits >> 32) as u32]
    }

    fn get_attributes(&self) -> HashMap<String, String> {
        asked(&self.model, "Accessible.GetAttributes");
        HashMap::new()
    }

    fn get_application(&self) -> Address {
        asked(&self.model, "Accessible.GetApplication");
        address(&self.bus, 0)
    }

    fn get_interfaces(&self) -> Vec<String> {
        asked(&self.model, "Accessible.GetInterfaces");
        let node = self.node();
        let mut out = vec!["org.a11y.atspi.Accessible".to_owned()];
        if self.id == 0 {
            out.push("org.a11y.atspi.Application".to_owned());
        }
        if node.text.is_some() {
            out.push("org.a11y.atspi.Text".to_owned());
        }
        if node.editable {
            out.push("org.a11y.atspi.EditableText".to_owned());
        }
        if node.action {
            out.push("org.a11y.atspi.Action".to_owned());
        }
        out
    }
}

/// `org.a11y.atspi.Application`, for the root.
struct Application {
    model: Shared,
    id: i32,
}

#[interface(name = "org.a11y.atspi.Application")]
impl Application {
    #[zbus(property)]
    fn toolkit_name(&self) -> String {
        asked(&self.model, "Application.ToolkitName");
        "example-desk".to_owned()
    }

    #[zbus(property)]
    fn version(&self) -> String {
        "1".to_owned()
    }

    #[zbus(property)]
    fn atspi_version(&self) -> String {
        "2.1".to_owned()
    }

    #[zbus(property)]
    fn id(&self) -> i32 {
        self.id
    }

    #[zbus(property)]
    fn set_id(&mut self, id: i32) {
        self.id = id;
    }
}

/// `org.a11y.atspi.Text`, for a node that holds text.
struct Text {
    id: usize,
    model: Shared,
}

impl Text {
    fn read(&self, member: &str) -> String {
        let mut model = lock(&self.model);
        *model.truth.asked.entry(member.to_owned()).or_default() += 1;
        let node = model.nodes[self.id].clone();
        if node.secret {
            model.truth.secret_reads += 1;
        }
        node.text.unwrap_or_default()
    }
}

#[interface(name = "org.a11y.atspi.Text")]
impl Text {
    #[zbus(property)]
    fn character_count(&self) -> i32 {
        i32::try_from(self.read("Text.CharacterCount").chars().count()).unwrap_or(i32::MAX)
    }

    #[zbus(property)]
    fn caret_offset(&self) -> i32 {
        0
    }

    fn get_text(&self, start: i32, end: i32) -> String {
        let text = self.read("Text.GetText");
        let chars: Vec<char> = text.chars().collect();
        let from = usize::try_from(start).unwrap_or(0).min(chars.len());
        let to = if end < 0 {
            chars.len()
        } else {
            usize::try_from(end).unwrap_or(0).min(chars.len())
        };
        chars[from..to.max(from)].iter().collect()
    }

    fn set_caret_offset(&self, _offset: i32) -> bool {
        asked(&self.model, "Text.SetCaretOffset");
        false
    }
}

/// `org.a11y.atspi.EditableText`, for a field that takes a value.
struct EditableText {
    id: usize,
    model: Shared,
}

#[interface(name = "org.a11y.atspi.EditableText")]
impl EditableText {
    async fn set_text_contents(
        &self,
        text: String,
        #[zbus(connection)] conn: &Connection,
    ) -> bool {
        let changed = {
            let mut model = lock(&self.model);
            *model
                .truth
                .asked
                .entry("EditableText.SetTextContents".to_owned())
                .or_default() += 1;
            model.set_field(self.id, &text)
        };
        match changed {
            Some(changed) => {
                signal(conn, &changed).await;
                true
            }
            None => false,
        }
    }

    fn insert_text(&self, _position: i32, _text: String, _length: i32) -> bool {
        asked(&self.model, "EditableText.InsertText");
        false
    }

    fn delete_text(&self, _start: i32, _end: i32) -> bool {
        asked(&self.model, "EditableText.DeleteText");
        false
    }
}

/// `org.a11y.atspi.Action`, for the Save button. A press is a write;
/// whether it stores is the application's.
struct Action {
    model: Shared,
}

#[interface(name = "org.a11y.atspi.Action")]
impl Action {
    #[zbus(property)]
    fn n_actions(&self) -> i32 {
        1
    }

    fn get_actions(&self) -> Vec<(String, String, String)> {
        asked(&self.model, "Action.GetActions");
        vec![("click".to_owned(), String::new(), String::new())]
    }

    fn get_name(&self, _index: i32) -> String {
        asked(&self.model, "Action.GetName");
        "click".to_owned()
    }

    async fn do_action(&self, index: i32, #[zbus(connection)] conn: &Connection) -> bool {
        if index != 0 {
            asked(&self.model, "Action.DoAction");
            return false;
        }
        let pressed = {
            let mut model = lock(&self.model);
            *model
                .truth
                .asked
                .entry("Action.DoAction".to_owned())
                .or_default() += 1;
            model.press()
        };
        signal(conn, &pressed.changed).await;
        if pressed.exit {
            // The application goes away before it says anything more.
            let gone = conn.clone();
            tokio::spawn(async move {
                let _ = gone.close().await;
            });
        } else if let Some(after) = pressed.late {
            let conn = conn.clone();
            let model = Arc::clone(&self.model);
            tokio::spawn(async move {
                // The stand's own delay, which the test chose: a late
                // confirmation is the thing a waiting driver must not
                // guess at.
                tokio::time::sleep(after).await;
                let changed = {
                    let mut model = lock(&model);
                    let message = model.message;
                    model.set_text(message, SAVED)
                };
                signal(&conn, &changed).await;
            });
        }
        true
    }
}

/// The application, published.
pub(crate) struct App {
    session: String,
    conn: Connection,
    bus: String,
    desktop: Address,
    model: Shared,
}

fn failed(doing: &'static str) -> impl FnOnce(zbus::Error) -> StandError {
    move |error| StandError(format!("{doing}: {error}"))
}

/// A connection to the session's accessibility bus.
async fn connect(session: &str) -> Result<Connection, StandError> {
    let session = zbus::connection::Builder::address(session)
        .build()
        .await
        .map_err(failed("connecting to the session bus"))?;
    let reply = session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.a11y.Bus"),
            "GetAddress",
            &(),
        )
        .await
        .map_err(failed("asking the session for its accessibility bus"))?;
    let a11y: String = reply
        .body()
        .deserialize()
        .map_err(failed("reading the accessibility bus's address"))?;
    zbus::connection::Builder::address(a11y.as_str())
        .build()
        .await
        .map_err(failed("connecting to the accessibility bus"))
}

impl App {
    pub(crate) async fn start(session: &str, order: Order) -> Result<Self, StandError> {
        let model: Shared = Arc::new(Mutex::new(Model::build(&order)));
        Self::publish(session, model).await
    }

    /// Publishes the model's every node on a new connection, and has the
    /// registry embed the application.
    async fn publish(session: &str, model: Shared) -> Result<Self, StandError> {
        let conn = connect(session).await?;
        let bus = conn
            .unique_name()
            .map(ToString::to_string)
            .ok_or_else(|| StandError("the accessibility bus gave no name".to_owned()))?;
        conn.object_server()
            .at(
                ROOT_PATH,
                Application {
                    model: Arc::clone(&model),
                    id: 0,
                },
            )
            .await
            .map_err(failed("publishing the application"))?;
        // The registry answers the root's place on the desktop.
        let reply = conn
            .call_method(
                Some("org.a11y.atspi.Registry"),
                ROOT_PATH,
                Some("org.a11y.atspi.Socket"),
                "Embed",
                &(address(&bus, 0),),
            )
            .await
            .map_err(failed("asking the registry to embed the application"))?;
        let desktop: Address = reply
            .body()
            .deserialize()
            .map_err(failed("reading the registry's answer"))?;
        let app = Self {
            session: session.to_owned(),
            conn,
            bus,
            desktop,
            model,
        };
        let count = lock(&app.model).nodes.len();
        for id in 0..count {
            app.publish_node(id).await?;
        }
        Ok(app)
    }

    async fn publish_node(&self, id: usize) -> Result<(), StandError> {
        let server = self.conn.object_server();
        let path = path_of(id);
        server
            .at(
                path.as_str(),
                Accessible {
                    id,
                    bus: self.bus.clone(),
                    desktop: self.desktop.clone(),
                    model: Arc::clone(&self.model),
                },
            )
            .await
            .map_err(failed("publishing a node"))?;
        let (text, editable, action) = {
            let model = lock(&self.model);
            let node = &model.nodes[id];
            (node.text.is_some(), node.editable, node.action)
        };
        if text {
            server
                .at(
                    path.as_str(),
                    Text {
                        id,
                        model: Arc::clone(&self.model),
                    },
                )
                .await
                .map_err(failed("publishing a node's text"))?;
        }
        if editable {
            server
                .at(
                    path.as_str(),
                    EditableText {
                        id,
                        model: Arc::clone(&self.model),
                    },
                )
                .await
                .map_err(failed("publishing a node's editable text"))?;
        }
        if action {
            server
                .at(
                    path.as_str(),
                    Action {
                        model: Arc::clone(&self.model),
                    },
                )
                .await
                .map_err(failed("publishing a node's action"))?;
        }
        Ok(())
    }

    pub(crate) fn truth(&self) -> Truth {
        let model = lock(&self.model);
        let mut truth = model.truth.clone();
        truth.tracking = model.nodes[model.tracking].text.clone().unwrap_or_default();
        truth.message = model.nodes[model.message].text.clone().unwrap_or_default();
        truth
    }

    pub(crate) fn show_late(&self, after: Duration) {
        let conn = self.conn.clone();
        let model = Arc::clone(&self.model);
        tokio::spawn(async move {
            // The stand's own delay, which the test chose: the late value
            // is the thing a waiting reader must not guess at.
            tokio::time::sleep(after).await;
            let id = {
                let mut model = lock(&model);
                let id = model.tracking;
                model.nodes[id].text = Some(LATE.to_owned());
                id
            };
            let length = i32::try_from(LATE.chars().count()).unwrap_or(i32::MAX);
            let properties: HashMap<String, Variant> = HashMap::new();
            let _ = conn
                .emit_signal(
                    None::<&str>,
                    path_of(id).as_str(),
                    EVENT_OBJECT,
                    "TextChanged",
                    &("insert", 0_i32, length, Variant::from(LATE), properties),
                )
                .await;
        });
    }

    pub(crate) fn fault(&self, fault: &Fault) -> Result<(), StandError> {
        let mut model = lock(&self.model);
        match fault {
            Fault::SaveSilently => model.faults.silent = true,
            Fault::ExitAfterSave => model.faults.exit = true,
            Fault::ConfirmLate(after) => model.faults.late = Some(*after),
            Fault::ReadOnly(label) => {
                let field = (0..model.nodes.len())
                    .find(|id| model.nodes[*id].editable && model.label_of(*id) == *label)
                    .ok_or_else(|| StandError(format!("no field is labelled `{label}`")))?;
                model.nodes[field].states =
                    (model.nodes[field].states & !state::EDITABLE) | state::READ_ONLY;
            }
        }
        Ok(())
    }

    /// A second window of the application, titled `title`, with a field
    /// labelled `New customer` of its own.
    pub(crate) async fn open_window(&self, title: &str) -> Result<(), StandError> {
        let shown = state::ENABLED | state::SENSITIVE | state::SHOWING | state::VISIBLE;
        let (first, last) = {
            let mut model = lock(&self.model);
            let first = model.nodes.len();
            let frame = model.add(
                Some(0),
                Node {
                    role: role::FRAME,
                    name: title.to_owned(),
                    states: shown,
                    ..Node::default()
                },
            );
            let panel = model.add(
                Some(frame),
                Node {
                    role: role::PANEL,
                    states: shown,
                    ..Node::default()
                },
            );
            model.labelled(panel, "New customer", "", Kind::Edit);
            (first, model.nodes.len())
        };
        for id in first..last {
            self.publish_node(id).await?;
        }
        let properties: HashMap<String, Variant> = HashMap::new();
        let _ = self
            .conn
            .emit_signal(
                None::<&str>,
                ROOT_PATH,
                EVENT_OBJECT,
                "ChildrenChanged",
                &(
                    "add",
                    i32::try_from(first).unwrap_or(i32::MAX),
                    0_i32,
                    Variant::from(address(&self.bus, first)),
                    properties,
                ),
            )
            .await;
        Ok(())
    }

    /// The application started again after it exited, with what it
    /// stored, on a new connection.
    pub(crate) async fn restart(&mut self) -> Result<(), StandError> {
        let _ = self.conn.clone().close().await;
        lock(&self.model).restarted();
        let again = Self::publish(&self.session, Arc::clone(&self.model)).await?;
        *self = again;
        Ok(())
    }
}
