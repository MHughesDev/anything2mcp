//! Throwaway. Package 8.1 on Linux: find a window by its program, find a text
//! field by role and name, set it through EditableText, read it back through
//! Text, invoke "Save" through Action, and end every wait on an AT-SPI event.
//! The store file the test window writes is the ground truth.
//!
//! Usage: spike-desktop-linux-atspi <store-file>

use std::error::Error;
use std::time::{Duration, Instant};

use atspi_connection::AccessibilityConnection;
use atspi::events::object::{
    ChildrenChangedEvent, PropertyChangeEvent, StateChangedEvent, TextChangedEvent,
};
use atspi::events::window::CreateEvent;
use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::collection::CollectionProxy;
use atspi::proxy::proxy_ext::ProxyExt;
use atspi::{Event, MatchType, ObjectEvents, ObjectMatchRule, ObjectRefOwned, Role, SortOrder};
use futures_lite::StreamExt;
use tokio::sync::mpsc;

type R<T> = Result<T, Box<dyn Error + Send + Sync>>;

const APP: &str = "example-stand";
const WINDOW: &str = "Example line item L-101";
const DEADLINE: Duration = Duration::from_secs(5); // a deadline, never a sleep

struct Seen {
    at: Instant,
    kind: String,
    sender: String,
    path: String,
    detail: String,
}

#[tokio::main]
async fn main() -> R<()> {
    let store = std::env::args().nth(1).ok_or("usage: <store-file>")?;
    // Toolkits that build their tree only when an assistive client is present
    // read this session-wide switch. The spike sets it; the pack would not (see
    // research note 19).
    if std::env::var_os("SPIKE_ENABLE").is_some() {
        atspi_connection::set_session_accessibility(true).await?;
        println!("session accessibility switched on");
    }
    let t0 = Instant::now();
    let a11y = AccessibilityConnection::new().await?;
    println!("connect_ms {}", t0.elapsed().as_millis());
    let conn = a11y.connection().clone();

    // Ask the registry for the events every wait below ends on.
    a11y.register_event::<TextChangedEvent>().await?;
    a11y.register_event::<PropertyChangeEvent>().await?;
    a11y.register_event::<StateChangedEvent>().await?;
    a11y.register_event::<CreateEvent>().await?;
    a11y.register_event::<ChildrenChangedEvent>().await?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Seen>();
    let stream = a11y.event_stream();
    tokio::spawn(async move {
        let mut stream = std::pin::pin!(stream);
        while let Some(ev) = stream.next().await {
            let Ok(ev) = ev else { continue };
            let (kind, item, detail) = match &ev {
                Event::Object(ObjectEvents::TextChanged(e)) => (
                    "text-changed",
                    e.item.clone(),
                    format!("{:?} {} {:?}", e.operation, e.start_pos, e.text),
                ),
                Event::Object(ObjectEvents::PropertyChange(e)) => (
                    "property-change",
                    e.item.clone(),
                    format!("{} {:?}", e.property, e.value),
                ),
                Event::Object(ObjectEvents::ChildrenChanged(e)) => (
                    "children-changed",
                    e.item.clone(),
                    format!("{:?}", e.operation),
                ),
                Event::Object(ObjectEvents::StateChanged(e)) => (
                    "state-changed",
                    e.item.clone(),
                    format!("{:?} {}", e.state, e.enabled),
                ),
                _ => continue,
            };
            let _ = tx.send(Seen {
                at: Instant::now(),
                kind: kind.into(),
                sender: item.name_as_str().unwrap_or_default().to_string(),
                path: item.path_as_str().to_string(),
                detail,
            });
        }
    });

    // The application, by its name on the bus, then its process by the bus daemon.
    let root = a11y.root_accessible_on_registry().await?;
    let mut app = None;
    let t_app = Instant::now();
    loop {
        for child in root.get_children().await? {
            let p = child.clone().into_accessible_proxy(&conn).await?;
            // An application that has registered but not yet exported its
            // root answers UnknownInterface; it is not ready, so skip it.
            let Ok(n) = p.name().await else { continue };
            if std::env::var_os("SPIKE_DUMP").is_some() {
                println!("  app {n:?}");
            }
            if n == APP || n == "spike-desktop-stand-egui" {
                app = Some(child);
            }
        }
        if app.is_some() {
            break;
        }
        // Not there yet: wait for the registry to say its children changed.
        wait_for(&mut rx, |s| s.kind == "children-changed").await?;
    }
    println!("app_found_ms {}", t_app.elapsed().as_millis());
    let app = app.ok_or("the test window's application is not on the accessibility bus")?;
    let sender = app.name_as_str().ok_or("no bus name")?.to_string();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let pid = dbus
        .get_connection_unix_process_id(zbus::names::BusName::try_from(sender.as_str())?)
        .await?;
    println!("app {sender} pid {pid}");

    // The window, by title, among the application's children.
    let app_p = app.clone().into_accessible_proxy(&conn).await?;
    let mut window = None;
    loop {
        for w in app_p.get_children().await? {
            let p = w.clone().into_accessible_proxy(&conn).await?;
            let (wn, wr) = (p.name().await?, p.get_role().await?);
            if std::env::var_os("SPIKE_DUMP").is_some() {
                println!("  window {wr:?} {wn:?}");
            }
            if (wn == WINDOW || (wn.is_empty() && std::env::var_os("SPIKE_UNTITLED").is_some()))
                && matches!(wr, Role::Frame | Role::Window)
            {
                window = Some(w);
            }
        }
        if window.is_some() {
            break;
        }
        wait_for(&mut rx, |s| s.kind == "children-changed")
            .await
            .map_err(|_| "window not found")?;
    }
    let window = window.ok_or("window not found")?;

    // Locate by walking the tree, and by the Collection interface (role on the
    // application side, name checked here), and compare.
    let t = Instant::now();
    let (nodes, found) = walk(&conn, &window, &[Role::Text, Role::Entry], "Description").await?;
    println!(
        "walk nodes {nodes} found {} ms {}",
        found.len(),
        t.elapsed().as_millis()
    );
    let t = Instant::now();
    let coll = CollectionProxy::builder(&conn)
        .destination(sender.as_str())?
        .path(app.path_as_str())?
        .build()
        .await?;
    let rule = ObjectMatchRule::builder()
        .roles(&[Role::Text, Role::Entry], MatchType::Any)
        .build();
    let matches = coll
        .get_matches(rule, SortOrder::Canonical, 100, true)
        .await
        .unwrap_or_default(); // AccessKit does not implement Collection
    let mut named = Vec::new();
    for m in &matches {
        let p = m.clone().into_accessible_proxy(&conn).await?;
        if accessible_name(&conn, &p).await? == "Description" {
            named.push(m.clone());
        }
    }
    println!(
        "collection role_matches {} named {} ms {}",
        matches.len(),
        named.len(),
        t.elapsed().as_millis()
    );
    if found.len() != 1 {
        return Err(format!("expected exactly one field, found {}", found.len()).into());
    }
    let field = found[0].clone();
    let field_p = field.clone().into_accessible_proxy(&conn).await?;
    println!(
        "field role {:?} id {:?} path {}",
        field_p.get_role().await?,
        field_p.accessible_id().await.unwrap_or_default(),
        field.path_as_str()
    );

    // Read, set through EditableText, wait for the text-changed event on this
    // element, and read back.
    let px = field_p.proxies().await?;
    let text = px.text().await?;
    println!("before {:?}", text.get_text(0, -1).await?);
    drain(&mut rx);
    let t = Instant::now();
    let ok = px
        .editable_text()
        .await?
        .set_text_contents("Hinge, steel")
        .await?;
    let ev = wait_for(&mut rx, |s| {
        s.kind == "text-changed" && s.path == field.path_as_str() && s.detail.contains("Insert")
    })
    .await?;
    println!(
        "set ok {ok} event_ms {} ({})",
        ev.at.duration_since(t).as_millis(),
        ev.detail
    );
    println!("after {:?}", text.get_text(0, -1).await?);

    // A password field: what does reading it give?
    let (_, pins) = walk(&conn, &window, &[Role::PasswordText], "PIN").await?;
    if let Some(pin) = pins.first() {
        let pp = pin.clone().into_accessible_proxy(&conn).await?;
        let t = pp.proxies().await?.text().await?;
        let shown = t.get_text(0, -1).await?;
        println!(
            "password role {:?} text_len {} all_masked {}",
            pp.get_role().await?,
            shown.chars().count(),
            shown.chars().all(|c| c != 'e' && !c.is_ascii_digit())
        );
    }

    // Invoke Save through Action; the confirmation is the status label's name
    // changing, signalled as an event; then ask the store.
    let (_, buttons) = walk(&conn, &window, &[Role::Button], "Save").await?;
    let button = buttons.first().ok_or("no Save button")?;
    let action = button
        .clone()
        .into_accessible_proxy(&conn)
        .await?
        .proxies()
        .await?
        .action()
        .await?;
    println!("actions {:?}", action.get_actions().await?);
    drain(&mut rx);
    let t = Instant::now();
    action.do_action(0).await?;
    let ev = wait_for(&mut rx, |s| s.detail.contains("Saved")).await?;
    println!(
        "invoke confirm_ms {} via {} ({})",
        ev.at.duration_since(t).as_millis(),
        ev.kind,
        ev.detail
    );
    let stored = std::fs::read_to_string(&store)?;
    println!("store {stored}");

    // A wait that no event ends must end at its deadline, as a failure.
    drain(&mut rx);
    let t = Instant::now();
    let none = wait_for(&mut rx, |s| s.detail.contains("never")).await;
    println!(
        "no_event ends_err {} after_ms {}",
        none.is_err(),
        t.elapsed().as_millis()
    );
    let _ = (ev.sender, ev.path);
    Ok(())
}

async fn walk(
    conn: &zbus::Connection,
    from: &ObjectRefOwned,
    roles: &[Role],
    name: &str,
) -> R<(usize, Vec<ObjectRefOwned>)> {
    let mut stack = vec![from.clone()];
    let mut nodes = 0;
    let mut found = Vec::new();
    while let Some(o) = stack.pop() {
        nodes += 1;
        let p: AccessibleProxy<'_> = o.clone().into_accessible_proxy(conn).await?;
        let role = p.get_role().await?;
        let n = accessible_name(conn, &p).await?;
        if std::env::var_os("SPIKE_DUMP").is_some() {
            println!("  node {role:?} {n:?} {}", o.path_as_str());
        }
        if roles.contains(&role) && n == name {
            found.push(o.clone());
        }
        stack.extend(p.get_children().await?);
    }
    Ok((nodes, found))
}

/// The name a screen reader would speak: the element's own name, else the
/// names of the elements its labelled-by relation points to.
async fn accessible_name(conn: &zbus::Connection, p: &AccessibleProxy<'_>) -> R<String> {
    let own = p.name().await?;
    if !own.is_empty() {
        return Ok(own);
    }
    let mut parts = Vec::new();
    for (kind, targets) in p.get_relation_set().await? {
        if kind == atspi::RelationType::LabelledBy {
            for t in targets {
                parts.push(t.into_accessible_proxy(conn).await?.name().await?);
            }
        }
    }
    Ok(parts.join(" "))
}

fn drain(rx: &mut mpsc::UnboundedReceiver<Seen>) {
    while rx.try_recv().is_ok() {}
}

async fn wait_for(rx: &mut mpsc::UnboundedReceiver<Seen>, want: impl Fn(&Seen) -> bool) -> R<Seen> {
    let deadline = tokio::time::Instant::now() + DEADLINE;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(s)) if want(&s) => return Ok(s),
            Ok(Some(_)) => {}
            Ok(None) => return Err("event stream closed".into()),
            Err(_) => return Err("timeout: no event ended the wait".into()),
        }
    }
}
