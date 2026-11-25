//! The Linux back end: AT-SPI on the session's accessibility bus, through
//! calls of this module's own over the workspace's D-Bus crate (decision
//! 0198).
//!
//! Every read goes through [`call`], which sends only the members of
//! [`READ_MEMBERS`]: each one asks, and none changes the application. The
//! module never asks the desktop's own tree for anything but the list of
//! applications, and asks an application nothing until its process is
//! proven to be the program the surface declares.
//!
//! The acting half is apart (decision 0218): [`Hands`], which only the
//! driver's `set` and `invoke` make, sends through [`act_call`], the one
//! function that sends a member of [`ACT_MEMBERS`]: set an element's text
//! through its editable-text interface, and do a named action of its action
//! interface. Nothing else in the module calls it, and a test holds both
//! lists and the module's source. No member sends a key or a pointer
//! event: there is none in either list.

use std::num::NonZeroUsize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use a2m_provider::Deadline;
use futures_core::Stream;
use zbus::{Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant};

use super::{Acted, App, AxError, Heard, Limits, Settable, Window};
use crate::roles;
use crate::tree::{El, Node, Tree};

/// The longest one call to the application may take.
const CALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

const REGISTRY: &str = "org.a11y.atspi.Registry";
const DESKTOP: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const TEXT: &str = "org.a11y.atspi.Text";
const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
const ACTION: &str = "org.a11y.atspi.Action";
const TABLE: &str = "org.a11y.atspi.Table";
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
    // The names of the actions an element offers, to find the one a
    // screen names. Asking does nothing.
    (ACTION, "GetActions"),
    // A client says which events it listens to, so that a toolkit that
    // sends events only to listeners sends them.
    ("org.a11y.atspi.Registry", "RegisterEvent"),
    ("org.a11y.atspi.Registry", "DeregisterEvent"),
];

/// Every member the acting half may send, as interface and member: set an
/// element's text, and do one of its actions. Only [`act_call`] sends one,
/// and only [`Hands`] calls it. A test holds that no member of
/// [`READ_MEMBERS`] is here, and that each is one of AT-SPI's members that
/// act.
pub(crate) const ACT_MEMBERS: &[(&str, &str)] =
    &[(EDITABLE_TEXT, "SetTextContents"), (ACTION, "DoAction")];

/// The properties [`PROPERTIES`]`.Get` may ask for.
const READ_PROPERTIES: &[(&str, &str)] = &[
    (ACCESSIBLE, "Name"),
    ("org.a11y.Status", "IsEnabled"),
    // How many columns a table states it has, asked of a table only.
    (TABLE, "NColumns"),
];

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
/// The state bits that say an element's text may be changed, and that it
/// may not.
const EDITABLE: u64 = 1 << 7;
/// The state bit that says an element is enabled: a disabled element takes
/// no value whatever else its state says.
const ENABLED: u64 = 1 << 8;
const READ_ONLY: u64 = 1 << 43;
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
    if !may_read(interface, member) {
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

/// Whether the reading path may send a member: only one of
/// [`READ_MEMBERS`], never one of [`ACT_MEMBERS`].
fn may_read(interface: &str, member: &str) -> bool {
    READ_MEMBERS.contains(&(interface, member)) && !ACT_MEMBERS.contains(&(interface, member))
}

/// Sends one member of [`ACT_MEMBERS`] to one element and reads its
/// answer, within [`CALL_LIMIT`]. The one function that changes an
/// application; only [`Hands`] calls it. An error after the call was sent
/// is [`Acted::Unknown`]: whether the application did it is not known.
async fn act_call<B>(
    conn: &Connection,
    el: &El,
    interface: &str,
    member: &str,
    body: &B,
) -> Result<bool, Acted>
where
    B: serde::Serialize + zbus::DynamicType,
{
    if !ACT_MEMBERS.contains(&(interface, member)) {
        return Err(Acted::NotSent(AxError::Lost(
            "a member that does not act".to_owned(),
        )));
    }
    let sent = conn.call_method(
        Some(el.bus.as_str()),
        el.path.as_str(),
        Some(interface),
        member,
        body,
    );
    let reply = match tokio::time::timeout(CALL_LIMIT, sent).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => return Err(Acted::Unknown(lost(&error))),
        Err(_) => return Err(Acted::Unknown(AxError::Timeout)),
    };
    reply
        .body()
        .deserialize::<bool>()
        .map_err(|error| Acted::Unknown(lost(&error)))
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

/// Every member this back end may send to a service the person's session
/// runs, as interface and member (decision 0316): the introspection a
/// service answers about itself, and the value of one property. Each one
/// asks; none calls a method of the service. Only [`service_call`] sends
/// one, and it sends every one with the flag that forbids the bus to start
/// the service.
pub(crate) const SERVICE_MEMBERS: &[(&str, &str)] = &[
    ("org.freedesktop.DBus.Introspectable", "Introspect"),
    (PROPERTIES, "Get"),
];

/// The most bytes of one object's introspection the pack takes.
const MOST_XML: usize = 262_144;
/// The most objects one walk of a service reads.
const MOST_OBJECTS: usize = 200;
/// The most bytes of introspection one walk keeps.
const MOST_WALK_BYTES: usize = 1_048_576;
/// The deepest below the root an object may be, in path parts.
const DEEPEST_OBJECT: usize = 12;

/// What a walk of a service's objects gave.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Walk {
    /// Each object's path and the text its introspection answered.
    pub objects: Vec<(String, String)>,
    /// The bound the walk stopped at, by name, when it stopped at one.
    pub cut: Option<&'static str>,
}

/// Sends one member of [`SERVICE_MEMBERS`] to the service the session's
/// bus names, with `NO_AUTO_START` on the message: a service that is not
/// running is never started, whatever its `.service` file says. Within
/// [`CALL_LIMIT`].
async fn service_call<B, R>(
    conn: &Connection,
    service: &str,
    path: &str,
    interface: &str,
    member: &str,
    body: &B,
) -> Result<R, AxError>
where
    B: serde::Serialize + zbus::DynamicType,
    R: for<'d> zbus::wire::DynamicDeserialize<'d>,
{
    if !SERVICE_MEMBERS.contains(&(interface, member)) {
        return Err(AxError::Lost("a member that is not a read".to_owned()));
    }
    let proxy = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(conn)
        .destination(service)
        .path(path)
        .interface(interface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(|error| lost(&error))?;
    let sent =
        proxy.call_with_flags::<_, _, R>(member, zbus::proxy::MethodFlags::NO_AUTO_START, body);
    match tokio::time::timeout(CALL_LIMIT, sent).await {
        Ok(Ok(Some(answer))) => Ok(answer),
        Ok(Ok(None)) => Err(AxError::Lost("no answer".to_owned())),
        Ok(Err(error)) => Err(service_error(&error)),
        Err(_) => Err(AxError::Timeout),
    }
}

/// An error answer of the bus for a service, read: a name no connection
/// owns is a service that is not running.
fn service_error(error: &zbus::Error) -> AxError {
    let name = match error {
        zbus::Error::MethodError(name, _, _) => name.as_str().to_owned(),
        zbus::Error::FDO(inner) => format!("{inner:?}"),
        _ => String::new(),
    };
    if name.contains("ServiceUnknown") || name.contains("NameHasNoOwner") {
        AxError::ServiceNotRunning
    } else {
        lost(error)
    }
}

/// The look-only handle on one session's accessibility bus.
pub(crate) struct Look {
    session: Connection,
    a11y: Connection,
}

/// The application's signals, and the bus's word that its connection
/// ended, from the moment the watch began.
pub(crate) struct Watch {
    stream: MessageStream,
    gone: MessageStream,
}

impl Watch {
    /// Waits for the application's next signal. `false` when the deadline
    /// passed first.
    pub(crate) async fn next(&mut self, deadline: Deadline) -> Result<bool, AxError> {
        match self.heard(deadline).await? {
            Heard::Changed => Ok(true),
            Heard::Passed => Ok(false),
            Heard::Gone => Err(AxError::Lost("the application left the bus".to_owned())),
        }
    }

    /// Waits for the application's next signal, or the end of its
    /// connection, or the deadline, whichever is first.
    pub(crate) async fn heard(&mut self, deadline: Deadline) -> Result<Heard, AxError> {
        let (stream, gone) = (&mut self.stream, &mut self.gone);
        let next = std::future::poll_fn(|cx| {
            if let std::task::Poll::Ready(left) = Pin::new(&mut *gone).poll_next(cx) {
                return std::task::Poll::Ready(left.map(|_| Heard::Gone));
            }
            Pin::new(&mut *stream)
                .poll_next(cx)
                .map(|signal| signal.map(|_| Heard::Changed))
        });
        match tokio::time::timeout_at(deadline.at(), next).await {
            Ok(Some(heard)) => Ok(heard),
            Ok(None) => Err(AxError::Lost("the bus".to_owned())),
            Err(_) => Ok(Heard::Passed),
        }
    }
}

/// The signals after which a new application may be on the bus, from the
/// moment the listening began (decision 0467).
pub(crate) struct Arrivals {
    registry: MessageStream,
    owners: MessageStream,
}

impl Arrivals {
    /// Waits for the next such signal. `false` when the deadline passed
    /// first.
    pub(crate) async fn next(&mut self, deadline: Deadline) -> Result<bool, AxError> {
        let (registry, owners) = (&mut self.registry, &mut self.owners);
        let next = std::future::poll_fn(|cx| {
            if let std::task::Poll::Ready(signal) = Pin::new(&mut *registry).poll_next(cx) {
                return std::task::Poll::Ready(signal.map(|_| ()));
            }
            Pin::new(&mut *owners)
                .poll_next(cx)
                .map(|signal| signal.map(|_| ()))
        });
        match tokio::time::timeout_at(deadline.at(), next).await {
            Ok(Some(())) => Ok(true),
            Ok(None) => Err(AxError::Lost("the bus".to_owned())),
            Err(_) => Ok(false),
        }
    }
}

/// What the driver proved of the application's process when it was
/// prepared, and proves again before every act: the process that owns the
/// application's connection, its start time, the program it runs, and,
/// when the surface states one, the program's digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Proof {
    process: u32,
    started: u64,
    program: PathBuf,
    digest: Option<String>,
}

/// The time a process started, in the system's clock ticks since boot, as
/// the process table gives it: what tells a process from a later one with
/// the same number.
fn started(process: u32) -> Option<u64> {
    let stat =
        std::fs::read_to_string(Path::new("/proc").join(process.to_string()).join("stat")).ok()?;
    // The program's name, in parentheses, may hold spaces; the fields
    // after it are numbered from the third, and the start time is the
    // twenty-second.
    let (_, after) = stat.rsplit_once(')')?;
    after.split_whitespace().nth(19)?.parse().ok()
}

/// The acting half of the back end, over one look and the proof of its
/// application. Made only by the driver's `set` and `invoke`; it is the
/// only caller of [`act_call`].
pub(crate) struct Hands<'a> {
    look: &'a Look,
    proof: &'a Proof,
}

impl Hands<'_> {
    /// Proves again, before an act, that the element is the application's
    /// and that the application is still the process proven when the run
    /// was prepared. Only the bus daemon and the process table are asked.
    pub(crate) async fn prove(&self, app: &App, node: &Node) -> Result<(), AxError> {
        if node.el.bus != app.root.bus {
            return Err(AxError::Replaced);
        }
        let process: u32 = call(
            &self.look.a11y,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetConnectionUnixProcessID",
            &(app.root.bus.as_str(),),
        )
        .await?;
        let me = std::fs::metadata("/proc/self")
            .map_err(|_| AxError::Lost("the process table".to_owned()))?
            .uid();
        let same = process == self.proof.process
            && started(process) == Some(self.proof.started)
            && process_program(process, me).as_deref() == Some(self.proof.program.as_path());
        if !same {
            return Err(AxError::Replaced);
        }
        if let Some(stated) = &self.proof.digest
            && running_digest(process).as_deref() != Some(stated.as_str())
        {
            return Err(AxError::Replaced);
        }
        Ok(())
    }

    /// Whether the element's text may be set now, from its interfaces and
    /// its state (enabled, editable, not read-only): two reads.
    pub(crate) async fn settable(&self, node: &Node) -> Result<Settable, AxError> {
        let (bus, path) = (node.el.bus.as_str(), node.el.path.as_str());
        let interfaces: Vec<String> =
            call(&self.look.a11y, bus, path, ACCESSIBLE, "GetInterfaces", &()).await?;
        if !interfaces.iter().any(|known| known == EDITABLE_TEXT) {
            return Ok(Settable::NoInterface);
        }
        let state: Vec<u32> = call(&self.look.a11y, bus, path, ACCESSIBLE, "GetState", &()).await?;
        let bits = state.first().map_or(0, |low| u64::from(*low))
            | state.get(1).map_or(0, |high| u64::from(*high) << 32);
        Ok(if bits & ENABLED == 0 {
            Settable::Disabled
        } else if bits & EDITABLE != 0 && bits & READ_ONLY == 0 {
            Settable::Yes
        } else {
            Settable::ReadOnly
        })
    }

    /// Sets the element's whole text. A control that holds a secret is
    /// never set. `false` when the application says it took nothing.
    pub(crate) async fn set_text(&self, node: &Node, text: &str) -> Result<bool, Acted> {
        if node.secret {
            return Err(Acted::NotSent(AxError::Secret));
        }
        act_call(
            &self.look.a11y,
            &node.el,
            EDITABLE_TEXT,
            "SetTextContents",
            &(text,),
        )
        .await
    }

    /// Does the element's one action that it binds to `key` of the pack's
    /// closed list, as its action interface lists its key bindings
    /// (decision 0467). `Ok(None)` when it binds the key to no action, or
    /// to more than one, and nothing was sent; `Ok(Some(false))` when the
    /// application says it did not do it. No key event is sent.
    pub(crate) async fn press_key(&self, node: &Node, key: &str) -> Result<Option<bool>, Acted> {
        let (bus, path) = (node.el.bus.as_str(), node.el.path.as_str());
        let offered: Vec<(String, String, String)> =
            call(&self.look.a11y, bus, path, ACTION, "GetActions", &())
                .await
                .map_err(Acted::NotSent)?;
        let bound: Vec<usize> = offered
            .iter()
            .enumerate()
            .filter(|(_, (_, _, binding))| crate::keys::binds(binding, key))
            .map(|(at, _)| at)
            .collect();
        let [index] = bound.as_slice() else {
            return Ok(None);
        };
        let index = i32::try_from(*index)
            .map_err(|_| Acted::NotSent(AxError::Lost("an action out of range".to_owned())))?;
        act_call(&self.look.a11y, &node.el, ACTION, "DoAction", &(index,))
            .await
            .map(Some)
    }

    /// Does the element's action named `name`. `Ok(None)` when it offers
    /// no action of that name, and nothing was sent; `Ok(Some(false))` when
    /// the application says it did not do it.
    pub(crate) async fn perform(&self, node: &Node, name: &str) -> Result<Option<bool>, Acted> {
        let (bus, path) = (node.el.bus.as_str(), node.el.path.as_str());
        let offered: Vec<(String, String, String)> =
            call(&self.look.a11y, bus, path, ACTION, "GetActions", &())
                .await
                .map_err(Acted::NotSent)?;
        let named: Vec<usize> = offered
            .iter()
            .enumerate()
            .filter(|(_, (offered, _, _))| offered == name)
            .map(|(at, _)| at)
            .collect();
        let [index] = named.as_slice() else {
            return Ok(None);
        };
        let index = i32::try_from(*index)
            .map_err(|_| Acted::NotSent(AxError::Lost("an action out of range".to_owned())))?;
        act_call(&self.look.a11y, &node.el, ACTION, "DoAction", &(index,))
            .await
            .map(Some)
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
        // A table that answers the Table interface states how many
        // columns it has; one that does not states nothing, and nothing is
        // guessed for it (rule 7).
        let columns = if role_word(role) == "table" && interfaces.iter().any(|known| known == TABLE)
        {
            let value = property(&self.a11y, bus, path, TABLE, "NColumns").await?;
            i32::try_from(value)
                .ok()
                .and_then(|count| u32::try_from(count).ok())
                .filter(|count| *count > 0)
        } else {
            None
        };
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
            columns,
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

    /// Begins listening to the application's signals, and to the bus's
    /// word that the application's connection ended.
    pub(crate) async fn watch(&self, app: &App) -> Result<Watch, AxError> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(app.root.bus.as_str())
            .build()
            .map_err(|error| lost(&error))?;
        let stream = MessageStream::for_match_rule(rule, &self.a11y, NonZeroUsize::new(256))
            .await
            .map_err(|error| lost(&error))?;
        let left = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .interface("org.freedesktop.DBus")
            .member("NameOwnerChanged")
            .arg(0, app.root.bus.as_str())
            .build()
            .map_err(|error| lost(&error))?;
        let gone = MessageStream::for_match_rule(left, &self.a11y, NonZeroUsize::new(4))
            .await
            .map_err(|error| lost(&error))?;
        Ok(Watch { stream, gone })
    }

    /// Whether any process of the engine's own user runs `program`, from
    /// the process table alone: the driver starts no second copy of a
    /// program that runs and publishes nothing (decision 0467).
    pub(crate) fn program_runs(&self, program: &str) -> bool {
        let Ok(want) = std::fs::canonicalize(program) else {
            return false;
        };
        std::fs::metadata("/proc/self").is_ok_and(|me| runs(&want, me.uid()))
    }

    /// Begins listening to what says a new application may have published
    /// itself: the registry's signals (it signals a change of the
    /// desktop's children when it embeds an application), and the bus
    /// daemon's word of a connection's owner. A wait reads the list of
    /// applications again on each, and never decides from a signal.
    pub(crate) async fn arrivals(&self) -> Result<Arrivals, AxError> {
        let registry = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(REGISTRY)
            .build()
            .map_err(|error| lost(&error))?;
        let registry = MessageStream::for_match_rule(registry, &self.a11y, NonZeroUsize::new(64))
            .await
            .map_err(|error| lost(&error))?;
        let owners = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .interface("org.freedesktop.DBus")
            .member("NameOwnerChanged")
            .build()
            .map_err(|error| lost(&error))?;
        let owners = MessageStream::for_match_rule(owners, &self.a11y, NonZeroUsize::new(64))
            .await
            .map_err(|error| lost(&error))?;
        Ok(Arrivals { registry, owners })
    }

    /// What proves the application's process now: for the driver to keep,
    /// and to prove again before every act.
    pub(crate) fn proof(&self, app: &App, digest: Option<&str>) -> Result<Proof, AxError> {
        let me = std::fs::metadata("/proc/self")
            .map_err(|_| AxError::Lost("the process table".to_owned()))?
            .uid();
        let program = process_program(app.process, me).ok_or(AxError::Replaced)?;
        let started = started(app.process).ok_or(AxError::Replaced)?;
        Ok(Proof {
            process: app.process,
            started,
            program,
            digest: digest
                .map(|stated| stated.strip_prefix("sha256:").unwrap_or(stated).to_owned()),
        })
    }

    /// The acting half, over this look and the proof of its application.
    /// Only the driver's `set` and `invoke` ask for it.
    pub(crate) fn hands<'a>(&'a self, proof: &'a Proof) -> Hands<'a> {
        Hands { look: self, proof }
    }

    /// Walks the objects of the service `service` on the session bus,
    /// from `/`, asking each for its introspection and nothing else, within
    /// the bounds. A service no connection owns is
    /// [`AxError::ServiceNotRunning`]: nothing was started.
    pub(crate) async fn introspect(
        &self,
        service: &str,
        deadline: Deadline,
    ) -> Result<Walk, AxError> {
        let mut walk = Walk::default();
        let mut queue = std::collections::VecDeque::from([("/".to_owned(), 0_usize)]);
        let mut bytes = 0_usize;
        while let Some((path, depth)) = queue.pop_front() {
            if deadline.passed() {
                return Err(AxError::Timeout);
            }
            if walk.objects.len() == MOST_OBJECTS {
                walk.cut = Some("objects");
                break;
            }
            let xml: String = service_call(
                &self.session,
                service,
                &path,
                "org.freedesktop.DBus.Introspectable",
                "Introspect",
                &(),
            )
            .await?;
            if xml.len() > MOST_XML || bytes + xml.len() > MOST_WALK_BYTES {
                walk.cut = Some("bytes");
                break;
            }
            bytes += xml.len();
            // The children are named by the document; one the reader
            // refuses ends the walk below it, and the discoverer says so
            // when it reads the same text.
            let children = crate::dbus::read_object(&xml)
                .map(|object| object.children)
                .unwrap_or_default();
            for child in children {
                let below = if path == "/" {
                    format!("/{child}")
                } else {
                    format!("{path}/{child}")
                };
                if depth + 1 > DEEPEST_OBJECT {
                    walk.cut = Some("depth");
                } else if crate::dbus::is_object_path(&below) {
                    queue.push_back((below, depth + 1));
                }
            }
            walk.objects.push((path, xml));
        }
        Ok(walk)
    }

    /// The value of one property of one object of a service, as text: a
    /// string, a path, a flag, or a number. Any other form is
    /// [`AxError::NotText`].
    pub(crate) async fn property_text(
        &self,
        service: &str,
        path: &str,
        interface: &str,
        name: &str,
        limits: &Limits,
    ) -> Result<String, AxError> {
        use zbus::Variant as Value;
        let value: OwnedVariant = service_call(
            &self.session,
            service,
            path,
            PROPERTIES,
            "Get",
            &(interface, name),
        )
        .await?;
        let text = match &*value {
            Value::Str(text) => text.as_str().to_owned(),
            Value::ObjectPath(path) => path.as_str().to_owned(),
            Value::Bool(flag) => flag.to_string(),
            Value::U8(number) => number.to_string(),
            Value::I16(number) => number.to_string(),
            Value::U16(number) => number.to_string(),
            Value::I32(number) => number.to_string(),
            Value::U32(number) => number.to_string(),
            Value::I64(number) => number.to_string(),
            Value::U64(number) => number.to_string(),
            _ => return Err(AxError::NotText),
        };
        if text.chars().count() > limits.chars {
            return Err(AxError::TooLarge);
        }
        Ok(text)
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
    fn the_reading_path_sends_no_member_that_acts() {
        for (interface, member) in READ_MEMBERS {
            assert!(!ACTING.contains(member), "{member}");
            assert!(!ACT_MEMBERS.contains(&(interface, member)), "{member}");
            assert!(may_read(interface, member), "{member}");
        }
        assert!(!READ_MEMBERS.iter().any(|(interface, member)| {
            interface.contains("EditableText")
                || (interface.contains("Action") && *member != "GetActions")
                || interface.contains("Selection")
                || interface.contains("Component")
                || interface.contains("DeviceEventController")
                || interface.contains("Value")
        }));
        // The reading path refuses every member of the acting half, by
        // the same check it makes before anything is sent.
        for (interface, member) in ACT_MEMBERS {
            assert!(!may_read(interface, member), "{member}");
        }
    }

    #[test]
    fn the_acting_half_sets_a_text_and_does_an_action_and_nothing_else() {
        assert_eq!(
            ACT_MEMBERS,
            &[
                ("org.a11y.atspi.EditableText", "SetTextContents"),
                ("org.a11y.atspi.Action", "DoAction"),
            ]
        );
        for (_, member) in ACT_MEMBERS {
            assert!(ACTING.contains(member), "{member}");
        }
        // No key and no pointer: neither list has a member that makes an
        // input event, or a position on the screen.
        for (interface, member) in READ_MEMBERS.iter().chain(ACT_MEMBERS) {
            assert!(!interface.contains("DeviceEventController"), "{interface}");
            assert!(!member.contains("Generate"), "{member}");
            assert!(!member.contains("Point"), "{member}");
        }
    }

    #[test]
    fn only_the_acting_half_calls_the_one_function_that_acts() {
        // The module's own source, cut where its tests begin.
        let source = include_str!("linux.rs");
        let body = source
            .split("#[cfg(test)]")
            .next()
            .expect("the module has a body");
        let hands = body
            .find("impl Hands<'_> {")
            .expect("the acting half is in the module");
        let end = hands
            + body[hands..]
                .find("\n}\n")
                .expect("the acting half's block ends");
        let calls: Vec<usize> = body
            .match_indices("act_call(")
            .map(|(at, _)| at)
            .filter(|at| !body[..*at].ends_with("fn "))
            .collect();
        assert!(!calls.is_empty());
        for at in calls {
            assert!(
                (hands..end).contains(&at),
                "a call of `act_call` outside the acting half, at byte {at}"
            );
        }
    }

    #[test]
    fn the_service_members_ask_and_call_no_method_of_a_service() {
        assert_eq!(
            SERVICE_MEMBERS,
            &[
                ("org.freedesktop.DBus.Introspectable", "Introspect"),
                ("org.freedesktop.DBus.Properties", "Get"),
            ]
        );
        for (interface, member) in SERVICE_MEMBERS {
            assert!(!ACTING.contains(member), "{member}");
            assert!(!ACT_MEMBERS.contains(&(interface, member)), "{member}");
        }
        // The one function that sends one sets the flag that forbids the
        // bus to start the service, and nothing else in the module sends
        // to a service.
        let source = include_str!("linux.rs");
        let body = source.split("#[cfg(test)]").next().expect("a body");
        assert!(body.contains("zbus::proxy::MethodFlags::NO_AUTO_START"));
        assert_eq!(body.matches(".call_with_flags").count(), 1);
        assert!(!body.contains("StartServiceByName"));
    }

    #[test]
    fn a_start_time_is_read_from_the_process_table() {
        let me = std::process::id();
        assert!(started(me).is_some());
        assert_eq!(started(me), started(me));
    }

    #[test]
    fn every_role_word_is_one_the_pack_knows() {
        for number in 0..200 {
            let word = role_word(number);
            assert!(roles::known(word) || word == roles::OTHER, "{number}");
        }
    }
}
