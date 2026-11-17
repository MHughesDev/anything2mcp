//! The stand's application on the accessibility bus: its tree, the AT-SPI
//! interfaces each node answers, and the counts of what it was asked.
//!
//! The tree, as a toolkit would publish it:
//!
//! ```text
//! application "example-desk"
//!   frame "Example order"
//!     panel ""
//!       label "Order number"  -> entry (labelled by it) "A-100"
//!       label "Customer"      -> entry "Example Supplies"
//!       label "Status"        -> entry "open"
//!       label "Tracking"      -> entry ""   (shows LATE when told to)
//!       entry, with no name and no label
//!       label "Reference"     -> entry "R-1"
//!       label "Reference"     -> entry "R-2"
//!       label "Access code"   -> password text, holding SECRET
//!       table "Line items"
//!         table row: column headers "Item code", "Description", "Quantity"
//!         table row: cells, one row for each line
//!       push button "Save"
//! ```
//!
//! The password field answers its text to anyone who asks, which is worse
//! than a real toolkit does, so that a test can see that nobody asked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use zbus::{Connection, OwnedObjectPath, Variant, interface};

use crate::{LATE, Order, SECRET, StandError, Truth};

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
    secret: bool,
}

#[derive(Debug, Default)]
struct Model {
    nodes: Vec<Node>,
    truth: Truth,
    tracking: usize,
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
        let labelled = |model: &mut Self, label: &str, value: &str, secret: bool| {
            let label_id = model.add(
                Some(panel),
                Node {
                    role: role::LABEL,
                    name: label.to_owned(),
                    states: shown,
                    ..Node::default()
                },
            );
            let value_id = model.add(
                Some(panel),
                Node {
                    role: if secret {
                        role::PASSWORD_TEXT
                    } else {
                        role::ENTRY
                    },
                    states: shown
                        | state::FOCUSABLE
                        | state::SINGLE_LINE
                        | if secret { 0 } else { state::READ_ONLY },
                    text: Some(value.to_owned()),
                    labelled_by: vec![label_id],
                    secret,
                    ..Node::default()
                },
            );
            model.nodes[label_id].label_for.push(value_id);
            value_id
        };
        labelled(&mut model, "Order number", &order.number, false);
        labelled(&mut model, "Customer", &order.customer, false);
        labelled(&mut model, "Status", &order.status, false);
        let tracking = labelled(&mut model, "Tracking", "", false);
        model.tracking = tracking;
        model.add(
            Some(panel),
            Node {
                role: role::ENTRY,
                states: shown | state::READ_ONLY,
                text: Some("a value nothing labels".to_owned()),
                ..Node::default()
            },
        );
        labelled(&mut model, "Reference", "R-1", false);
        labelled(&mut model, "Reference", "R-2", false);
        labelled(&mut model, "Access code", SECRET, true);
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
        model
    }
}

type Shared = Arc<Mutex<Model>>;

fn lock(model: &Shared) -> std::sync::MutexGuard<'_, Model> {
    model.lock().unwrap_or_else(PoisonError::into_inner)
}

fn asked(model: &Shared, member: &str) {
    *lock(model).truth.asked.entry(member.to_owned()).or_default() += 1;
}

type Address = (String, OwnedObjectPath);

fn address(bus: &str, id: usize) -> Address {
    (
        bus.to_owned(),
        OwnedObjectPath::try_from(path_of(id))
            .unwrap_or_else(|_| OwnedObjectPath::try_from(ROOT_PATH).unwrap_or_default()),
    )
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
                || ("".to_owned(), OwnedObjectPath::try_from("/org/a11y/atspi/null").unwrap_or_default()),
                |child| address(&self.bus, child),
            )
    }

    fn get_index_in_parent(&self) -> i32 {
        asked(&self.model, "Accessible.GetIndexInParent");
        let model = lock(&self.model);
        model.nodes[self.id]
            .parent
            .and_then(|parent| model.nodes[parent].children.iter().position(|c| *c == self.id))
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
                node.labelled_by.iter().map(|id| address(&self.bus, *id)).collect(),
            ));
        }
        if !node.label_for.is_empty() {
            set.push((
                LABEL_FOR,
                node.label_for.iter().map(|id| address(&self.bus, *id)).collect(),
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

/// `org.a11y.atspi.Action`, for the Save button. A press is a write.
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

    fn do_action(&self, _index: i32) -> bool {
        let mut model = lock(&self.model);
        *model.truth.asked.entry("Action.DoAction".to_owned()).or_default() += 1;
        model.truth.writes += 1;
        true
    }
}

/// The application, published.
pub(crate) struct App {
    conn: Connection,
    model: Shared,
}

fn failed(doing: &'static str) -> impl FnOnce(zbus::Error) -> StandError {
    move |error| StandError(format!("{doing}: {error}"))
}

impl App {
    pub(crate) async fn start(session: &str, order: Order) -> Result<Self, StandError> {
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
        let conn = zbus::connection::Builder::address(a11y.as_str())
            .build()
            .await
            .map_err(failed("connecting to the accessibility bus"))?;
        let bus = conn
            .unique_name()
            .map(ToString::to_string)
            .ok_or_else(|| StandError("the accessibility bus gave no name".to_owned()))?;
        let model: Shared = Arc::new(Mutex::new(Model::build(&order)));
        let server = conn.object_server();
        server
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
        let count = lock(&model).nodes.len();
        for id in 0..count {
            let path = path_of(id);
            server
                .at(
                    path.as_str(),
                    Accessible {
                        id,
                        bus: bus.clone(),
                        desktop: desktop.clone(),
                        model: Arc::clone(&model),
                    },
                )
                .await
                .map_err(failed("publishing a node"))?;
            let (text, action) = {
                let model = lock(&model);
                (model.nodes[id].text.is_some(), model.nodes[id].action)
            };
            if text {
                server
                    .at(
                        path.as_str(),
                        Text {
                            id,
                            model: Arc::clone(&model),
                        },
                    )
                    .await
                    .map_err(failed("publishing a node's text"))?;
            }
            if action {
                server
                    .at(
                        path.as_str(),
                        Action {
                            model: Arc::clone(&model),
                        },
                    )
                    .await
                    .map_err(failed("publishing a node's action"))?;
            }
        }
        Ok(Self { conn, model })
    }

    pub(crate) fn truth(&self) -> Truth {
        let model = lock(&self.model);
        let mut truth = model.truth.clone();
        truth.tracking = model.nodes[model.tracking].text.clone().unwrap_or_default();
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
                    "org.a11y.atspi.Event.Object",
                    "TextChanged",
                    &("insert", 0_i32, length, Variant::from(LATE), properties),
                )
                .await;
        });
    }
}
