//! The Linux back end: AT-SPI on the session's accessibility bus, through
//! calls of this module's own over the workspace's D-Bus crate (decision
//! 0198).
//!
//! Every call goes through [`call`], which sends only the members of
//! [`READ_MEMBERS`]: each one asks, and none changes the application. The
//! module never asks the desktop's own tree for anything but the list of
//! applications, and asks an application nothing until its process is
//! proven to be the program the surface declares.

use std::num::NonZeroUsize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use a2m_provider::Deadline;
use futures_core::Stream;
use zbus::{Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant};

use super::{App, AxError, Limits, Window};
use crate::roles;
use crate::tree::{El, Node, Tree};

/// The longest one call to the application may take.
const CALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

const REGISTRY: &str = "org.a11y.atspi.Registry";
const DESKTOP: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const TEXT: &str = "org.a11y.atspi.Text";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// The events a wait listens for: every change an application signals
/// about its objects.
const EVENTS: &str = "object:";

/// Every member this back end may send, as interface and member. Each one
/// asks; none sets, invokes, selects, focuses, scrolls, or types. A test
/// holds the list against the members of AT-SPI that act.
pub(crate) const READ_MEMBERS: &[(&str, &str)] = &[
    ("org.a11y.Bus", "GetAddress"),
    ("org.freedesktop.DBus", "GetConnectionUnixProcessID"),
    (PROPERTIES, "Get"),
    (ACCESSIBLE, "GetChildren"),
    (ACCESSIBLE, "GetRole"),
    (ACCESSIBLE, "GetState"),
    (ACCESSIBLE, "GetRelationSet"),
    (ACCESSIBLE, "GetInterfaces"),
    (TEXT, "GetText"),
    // A client says which events it listens to, so that a toolkit that
    // sends events only to listeners sends them.
    ("org.a11y.atspi.Registry", "RegisterEvent"),
    ("org.a11y.atspi.Registry", "DeregisterEvent"),
];

/// The properties [`PROPERTIES`]`.Get` may ask for.
const READ_PROPERTIES: &[(&str, &str)] = &[(ACCESSIBLE, "Name"), ("org.a11y.Status", "IsEnabled")];

/// AT-SPI's role numbers, mapped to the pack's words.
fn role_word(role: u32) -> &'static str {
    match role {
        7 => "check_box",
        11 => "combo_box",
        16 => "dialog",
        20 => "filler",
        23 => "frame",
        27 => "image",
        29 => "label",
        31 => "list",
        32 => "list_item",
        33 => "menu",
        34 => "menu_bar",
        35 => "menu_item",
        37 => "page_tab",
        38 => "page_tab_list",
        39 => "panel",
        40 => "password_text",
        43 => "push_button",
        44 => "radio_button",
        49 => "scroll_pane",
        52 => "spin_button",
        54 => "status_bar",
        55 => "table",
        56 => "table_cell",
        57 => "table_column_header",
        58 => "table_row_header",
        61 => "text",
        62 => "toggle_button",
        63 => "tool_bar",
        65 => "tree",
        68 => "viewport",
        69 => "window",
        73 => "paragraph",
        75 => "application",
        79 => "entry",
        83 => "heading",
        85 => "section",
        87 => "form",
        90 => "table_row",
        91 => "tree_item",
        116 => "static",
        98 => "list_box",
        _ => roles::OTHER,
    }
}

/// The state bit that says an element is shown.
const SHOWING: u64 = 1 << 25;
/// The relation numbers of "labels" and "labelled by".
const LABEL_FOR: u32 = 1;
const LABELLED_BY: u32 = 2;
/// The role of a password field.
const PASSWORD_TEXT: u32 = 40;

type Address = (String, OwnedObjectPath);

fn lost(error: &zbus::Error) -> AxError {
    AxError::Lost(match error {
        zbus::Error::MethodError(name, _, _) => name.as_str().to_owned(),
        _ => "the bus".to_owned(),
    })
}

/// Sends one member of [`READ_MEMBERS`] and reads its answer, within
/// [`CALL_LIMIT`].
async fn call<B, R>(
    conn: &Connection,
    to: &str,
    path: &str,
    interface: &str,
    member: &str,
    body: &B,
) -> Result<R, AxError>
where
    B: serde::Serialize + zbus::DynamicType,
    R: for<'d> zbus::wire::DynamicDeserialize<'d>,
{
    if !READ_MEMBERS.contains(&(interface, member)) {
        return Err(AxError::Lost("a member that is not a read".to_owned()));
    }
    let sent = conn.call_method(Some(to), path, Some(interface), member, body);
    let reply = match tokio::time::timeout(CALL_LIMIT, sent).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => return Err(lost(&error)),
        Err(_) => return Err(AxError::Timeout),
    };
    reply
        .body()
        .deserialize::<R>()
        .map_err(|error| lost(&error))
}

/// Reads one property of [`READ_PROPERTIES`].
async fn property(
    conn: &Connection,
    to: &str,
    path: &str,
    interface: &str,
    name: &str,
) -> Result<OwnedVariant, AxError> {
    if !READ_PROPERTIES.contains(&(interface, name)) {
        return Err(AxError::Lost("a property that is not read".to_owned()));
    }
    call(conn, to, path, PROPERTIES, "Get", &(interface, name)).await
}

async fn text_property(
    conn: &Connection,
    to: &str,
    path: &str,
    interface: &str,
    name: &str,
) -> Result<String, AxError> {
    let value = property(conn, to, path, interface, name).await?;
    String::try_from(value).map_err(|_| AxError::Lost("a name that is not text".to_owned()))
}

/// The look-only handle on one session's accessibility bus.
pub(crate) struct Look {
    session: Connection,
    a11y: Connection,
}

/// The application's signals, from the moment the watch began.
pub(crate) struct Watch {
    stream: MessageStream,
}

impl Watch {
    /// Waits for the application's next signal. `false` when the deadline
    /// passed first.
    pub(crate) async fn next(&mut self, deadline: Deadline) -> Result<bool, AxError> {
        let stream = &mut self.stream;
        let next = std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx));
        match tokio::time::timeout_at(deadline.at(), next).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Err(AxError::Lost("the bus".to_owned())),
            Err(_) => Ok(false),
        }
    }
}

/// The program a process runs, and whether the engine's own user runs it.
fn process_program(process: u32, me: u32) -> Option<PathBuf> {
    let folder = Path::new("/proc").join(process.to_string());
    let owner = std::fs::metadata(&folder).ok()?.uid();
    (owner == me)
        .then(|| std::fs::read_link(folder.join("exe")).ok())
        .flatten()
}

/// Whether any process of the engine's own user runs `program`. Only the
/// process table is read.
fn runs(program: &Path, me: u32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .and_then(|process| process_program(process, me))
            .is_some_and(|running| running == program)
    })
}

/// The digest of the program a process runs, as the target states one.
fn running_digest(process: u32) -> Option<String> {
    use sha2::Digest as _;
    use std::io::Read;
    let mut file =
        std::fs::File::open(Path::new("/proc").join(process.to_string()).join("exe")).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

impl Look {
    /// Connects to the accessibility bus of the session at `session`, or of
    /// the session the engine runs in.
    pub(crate) async fn connect(session: Option<&str>) -> Result<Self, AxError> {
        let session = match session {
            Some(address) => zbus::connection::Builder::address(address).build().await,
            None => Connection::session().await,
        }
        .map_err(|error| lost(&error))?;
        let address: String = call(
            &session,
            "org.a11y.Bus",
            "/org/a11y/bus",
            "org.a11y.Bus",
            "GetAddress",
            &(),
        )
        .await?;
        let a11y = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .map_err(|error| lost(&error))?;
        call::<_, ()>(
            &a11y,
            REGISTRY,
            "/org/a11y/atspi/registry",
            REGISTRY,
            "RegisterEvent",
            &(EVENTS,),
        )
        .await?;
        Ok(Self { session, a11y })
    }

    /// The one running application that is `program`, run by the engine's
    /// own user, with the name `name` on the bus; with its digest checked
    /// when `digest` is stated. Only the bus daemon is asked about the
    /// applications that are not it.
    pub(crate) async fn application(
        &self,
        program: &str,
        name: &str,
        digest: Option<&str>,
    ) -> Result<App, AxError> {
        let Ok(want) = std::fs::canonicalize(program) else {
            return Err(AxError::NotRunning);
        };
        let me = std::fs::metadata("/proc/self")
            .map_err(|_| AxError::Lost("the process table".to_owned()))?
            .uid();
        let listed: Vec<Address> = call(
            &self.a11y,
            REGISTRY,
            DESKTOP,
            ACCESSIBLE,
            "GetChildren",
            &(),
        )
        .await?;
        let mut found = Vec::new();
        for (bus, path) in listed {
            if bus.is_empty() {
                continue;
            }
            let Ok(process) = call::<_, u32>(
                &self.a11y,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetConnectionUnixProcessID",
                &(bus.as_str(),),
            )
            .await
            else {
                continue;
            };
            if process_program(process, me).as_deref() != Some(want.as_path()) {
                continue;
            }
            // The program is the declared one: only now is it asked.
            let own = text_property(&self.a11y, &bus, path.as_str(), ACCESSIBLE, "Name").await?;
            if own == name {
                found.push(App {
                    root: El {
                        bus,
                        path: path.as_str().to_owned(),
                    },
                    process,
                });
            }
        }
        match found.len() {
            // Not on the bus. Started by this user, with its accessibility
            // switch off, it publishes nothing; not started, it is not
            // running. The pack starts nothing either way.
            0 if !runs(&want, me) => Err(AxError::NotRunning),
            0 => {
                let enabled = property(
                    &self.session,
                    "org.a11y.Bus",
                    "/org/a11y/bus",
                    "org.a11y.Status",
                    "IsEnabled",
                )
                .await
                .ok()
                .and_then(|value| bool::try_from(value).ok());
                Err(if enabled == Some(false) {
                    AxError::AccessibilityOff
                } else {
                    AxError::NotRunning
                })
            }
            1 => {
                let app = found.remove(0);
                if let Some(stated) = digest {
                    let stated = stated.strip_prefix("sha256:").unwrap_or(stated);
                    if running_digest(app.process).as_deref() != Some(stated) {
                        return Err(AxError::NotProven);
                    }
                }
                Ok(app)
            }
            many => Err(AxError::Ambiguous(many)),
        }
    }

    /// The application's windows: the elements directly under its root.
    pub(crate) async fn windows(&self, app: &App) -> Result<Vec<Window>, AxError> {
        let listed: Vec<Address> = call(
            &self.a11y,
            &app.root.bus,
            &app.root.path,
            ACCESSIBLE,
            "GetChildren",
            &(),
        )
        .await?;
        let mut out = Vec::new();
        for (bus, path) in listed {
            if bus != app.root.bus {
                continue;
            }
            let role: u32 =
                call(&self.a11y, &bus, path.as_str(), ACCESSIBLE, "GetRole", &()).await?;
            let title = text_property(&self.a11y, &bus, path.as_str(), ACCESSIBLE, "Name").await?;
            out.push(Window {
                el: El {
                    bus,
                    path: path.as_str().to_owned(),
                },
                role: role_word(role),
                title,
            });
        }
        Ok(out)
    }

    /// Everything one element says of itself but its value.
    async fn node(&self, el: &El, depth: u32, below: bool) -> Result<(Node, Vec<El>), AxError> {
        let (bus, path) = (el.bus.as_str(), el.path.as_str());
        let (role, own, state, interfaces, relations, children) = tokio::join!(
            call::<_, u32>(&self.a11y, bus, path, ACCESSIBLE, "GetRole", &()),
            text_property(&self.a11y, bus, path, ACCESSIBLE, "Name"),
            call::<_, Vec<u32>>(&self.a11y, bus, path, ACCESSIBLE, "GetState", &()),
            call::<_, Vec<String>>(&self.a11y, bus, path, ACCESSIBLE, "GetInterfaces", &()),
            call::<_, Vec<(u32, Vec<Address>)>>(
                &self.a11y,
                bus,
                path,
                ACCESSIBLE,
                "GetRelationSet",
                &()
            ),
            async {
                if below {
                    call::<_, Vec<Address>>(&self.a11y, bus, path, ACCESSIBLE, "GetChildren", &())
                        .await
                } else {
                    Ok(Vec::new())
                }
            },
        );
        let (role, own, state, interfaces, relations) =
            (role?, own?, state?, interfaces?, relations?);
        let bits = state.first().map_or(0, |low| u64::from(*low))
            | state.get(1).map_or(0, |high| u64::from(*high) << 32);
        let mut name = own;
        if name.is_empty() {
            // A screen reader's name: what labels it, in this application
            // only.
            let mut parts = Vec::new();
            for (kind, targets) in &relations {
                if *kind != LABELLED_BY {
                    continue;
                }
                for (label_bus, label_path) in targets {
                    if *label_bus == el.bus {
                        parts.push(
                            text_property(&self.a11y, bus, label_path.as_str(), ACCESSIBLE, "Name")
                                .await?,
                        );
                    }
                }
            }
            name = parts.join(" ");
        }
        let node = Node {
            depth,
            role: role_word(role),
            name,
            showing: bits & SHOWING != 0,
            secret: role == PASSWORD_TEXT,
            has_text: interfaces.iter().any(|known| known == TEXT),
            labels: relations
                .iter()
                .any(|(kind, targets)| *kind == LABEL_FOR && !targets.is_empty()),
            children: Vec::new(),
            el: el.clone(),
        };
        let children = children?
            .into_iter()
            .filter(|(child_bus, _)| *child_bus == el.bus)
            .map(|(bus, path)| El {
                bus,
                path: path.as_str().to_owned(),
            })
            .collect();
        Ok((node, children))
    }

    /// Walks the tree under `root`, each element before those under it,
    /// within the bounds and the deadline. A walk cut short says where.
    pub(crate) async fn tree(
        &self,
        root: &El,
        limits: &Limits,
        deadline: Deadline,
    ) -> Result<Tree, AxError> {
        let mut tree = Tree::default();
        let mut stack: Vec<(El, u32, Option<usize>)> = vec![(root.clone(), 0, None)];
        while let Some((el, depth, parent)) = stack.pop() {
            if tree.nodes.len() >= limits.nodes {
                tree.cut = Some("nodes");
                break;
            }
            if deadline.passed() {
                tree.cut = Some("seconds");
                break;
            }
            let below = depth < limits.depth;
            let (node, children) = self.node(&el, depth, below).await?;
            let at = tree.nodes.len();
            tree.nodes.push(node);
            if let Some(parent) = parent {
                tree.nodes[parent].children.push(at);
            }
            if !below && tree.cut.is_none() {
                let more: Vec<Address> = call(
                    &self.a11y,
                    &el.bus,
                    &el.path,
                    ACCESSIBLE,
                    "GetChildren",
                    &(),
                )
                .await?;
                if !more.is_empty() {
                    tree.cut = Some("depth");
                }
            }
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1, Some(at)));
            }
        }
        Ok(tree)
    }

    /// The text an element shows, at most `limits.chars` characters. A
    /// control that holds a secret is never asked.
    pub(crate) async fn text(&self, node: &Node, limits: &Limits) -> Result<String, AxError> {
        if node.secret {
            return Err(AxError::Secret);
        }
        if !node.has_text {
            return Ok(node.name.clone());
        }
        let most = i32::try_from(limits.chars.saturating_add(1)).unwrap_or(i32::MAX);
        let text: String = call(
            &self.a11y,
            &node.el.bus,
            &node.el.path,
            TEXT,
            "GetText",
            &(0_i32, most),
        )
        .await?;
        if text.chars().count() > limits.chars {
            return Err(AxError::TooLarge);
        }
        Ok(text)
    }

    /// Begins listening to the application's signals.
    pub(crate) async fn watch(&self, app: &App) -> Result<Watch, AxError> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(app.root.bus.as_str())
            .build()
            .map_err(|error| lost(&error))?;
        let stream = MessageStream::for_match_rule(rule, &self.a11y, NonZeroUsize::new(256))
            .await
            .map_err(|error| lost(&error))?;
        Ok(Watch { stream })
    }

    /// Says the pack no longer listens, and closes both connections.
    pub(crate) async fn close(self) {
        let _ = call::<_, ()>(
            &self.a11y,
            REGISTRY,
            "/org/a11y/atspi/registry",
            REGISTRY,
            "DeregisterEvent",
            &(EVENTS,),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The members of AT-SPI's interfaces that change an application.
    const ACTING: &[&str] = &[
        "DoAction",
        "SetTextContents",
        "InsertText",
        "DeleteText",
        "CutText",
        "PasteText",
        "CopyText",
        "SetCurrentValue",
        "SelectChild",
        "DeselectChild",
        "SelectAll",
        "ClearSelection",
        "GrabFocus",
        "SetSelection",
        "AddSelection",
        "RemoveSelection",
        "SetCaretOffset",
        "ScrollTo",
        "ScrollToPoint",
        "SetPosition",
        "SetExtents",
        "SetSize",
        "GenerateKeyboardEvent",
        "GenerateMouseEvent",
        "Embed",
        "Unembed",
        "Set",
    ];

    #[test]
    fn the_back_end_sends_no_member_that_acts() {
        for (_, member) in READ_MEMBERS {
            assert!(!ACTING.contains(member), "{member}");
        }
        assert!(
            !READ_MEMBERS
                .iter()
                .any(|(interface, _)| interface.contains("EditableText")
                    || interface.contains("Action")
                    || interface.contains("Selection")
                    || interface.contains("Component")
                    || interface.contains("DeviceEventController")
                    || interface.contains("Value"))
        );
    }

    #[test]
    fn every_role_word_is_one_the_pack_knows() {
        for number in 0..200 {
            let word = role_word(number);
            assert!(roles::known(word) || word == roles::OTHER, "{number}");
        }
    }
}
