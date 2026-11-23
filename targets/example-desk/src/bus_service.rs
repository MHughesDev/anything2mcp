//! A service of the stand's own on the session bus: the kind of thing a
//! desktop program offers beside its window, and the smallest that shows
//! what a reader of a service's introspection must not do. It owns the bus
//! name [`NAME`] and answers two objects:
//!
//! ```text
//! /org/example/Desk                  org.example.Desk.Orders
//!   method   Count() -> u                (a call is recorded)
//!   method   Remove(s) -> b              (a call is recorded)
//!   property Total        u   read       (a get is recorded)
//!   property Label        s   readwrite  (a get and a set are recorded)
//!   property AdminPassword     s   read       (named as a secret)
//!   property Level        as  read       (not a form the pack gives as text)
//!   signal   Changed(s)
//! /org/example/Desk/Orders/A100      org.example.Desk.Order
//!   property Number       s   read
//! ```
//!
//! Every member the stand is asked is recorded by name, so that a test asks
//! the stand, and never the pack, what was called. Introspection is
//! answered by the bus library itself and is not a member of the stand: a
//! test sees what a reader asked through what the stand was never asked.

use std::sync::{Arc, Mutex, PoisonError};

use zbus::{Connection, interface};

use crate::StandError;
use crate::session::Session;

/// The bus name the service owns.
pub const NAME: &str = "org.example.Desk";
/// The bus name of a service that is described in a `.service` file and is
/// not running: asking for it starts [`Session::idle_service`]'s program,
/// unless the message forbids it.
pub const IDLE_NAME: &str = crate::session::IDLE_NAME;
/// The object that answers [`ORDERS`].
pub const ROOT: &str = "/org/example/Desk";
/// The object that answers [`ORDER`].
pub const CHILD: &str = "/org/example/Desk/Orders/A100";
/// The interface of the root object.
pub const ORDERS: &str = "org.example.Desk.Orders";
/// The interface of the child object.
pub const ORDER: &str = "org.example.Desk.Order";
/// What `Total` holds.
pub const TOTAL: u32 = 3;
/// What `Label` holds.
pub const LABEL: &str = "Open orders";
/// What `AdminPassword` holds. It must appear in no result, finding, draft, or
/// log of the pack under test. Plainly made up.
pub const ADMIN_PASSWORD: &str = "secret-for-tests-desk-admin-password";
/// What `Number` holds.
pub const NUMBER: &str = "A-100";

/// Every member of the service the stand was asked, in order, by kind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Calls {
    /// Methods called, by name.
    pub methods: Vec<String>,
    /// Properties read, by name.
    pub gets: Vec<String>,
    /// Properties set, by name.
    pub sets: Vec<String>,
}

type Shared = Arc<Mutex<Calls>>;

fn record(calls: &Shared, kind: fn(&mut Calls) -> &mut Vec<String>, name: &str) {
    kind(&mut calls.lock().unwrap_or_else(PoisonError::into_inner)).push(name.to_owned());
}

struct Orders {
    calls: Shared,
    label: String,
}

#[interface(name = "org.example.Desk.Orders")]
impl Orders {
    fn count(&self) -> u32 {
        record(&self.calls, |c| &mut c.methods, "Count");
        TOTAL
    }

    fn remove(&self, _order: String) -> bool {
        record(&self.calls, |c| &mut c.methods, "Remove");
        true
    }

    #[zbus(property)]
    fn total(&self) -> u32 {
        record(&self.calls, |c| &mut c.gets, "Total");
        TOTAL
    }

    #[zbus(property)]
    fn label(&self) -> String {
        record(&self.calls, |c| &mut c.gets, "Label");
        self.label.clone()
    }

    #[zbus(property)]
    fn set_label(&mut self, value: String) {
        record(&self.calls, |c| &mut c.sets, "Label");
        self.label = value;
    }

    #[zbus(property)]
    fn admin_password(&self) -> String {
        record(&self.calls, |c| &mut c.gets, "AdminPassword");
        ADMIN_PASSWORD.to_owned()
    }

    #[zbus(property)]
    fn level(&self) -> Vec<String> {
        record(&self.calls, |c| &mut c.gets, "Level");
        vec!["a".to_owned()]
    }

    #[zbus(signal)]
    async fn changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        order: &str,
    ) -> zbus::Result<()>;
}

struct Order {
    calls: Shared,
}

#[interface(name = "org.example.Desk.Order")]
impl Order {
    #[zbus(property)]
    fn number(&self) -> String {
        record(&self.calls, |c| &mut c.gets, "Number");
        NUMBER.to_owned()
    }
}

/// The service, owning [`NAME`] on a session's bus. Dropping it takes the
/// name off the bus.
pub struct Service {
    calls: Shared,
    _conn: Connection,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service").finish_non_exhaustive()
    }
}

impl Service {
    /// Starts the service on the session's bus, and returns once it owns
    /// its name.
    ///
    /// # Errors
    ///
    /// When the bus cannot be reached or the name cannot be owned.
    pub async fn start(session: &Session) -> Result<Self, StandError> {
        let failed =
            |doing: &'static str| move |error: zbus::Error| StandError(format!("{doing}: {error}"));
        let calls: Shared = Arc::default();
        let conn = zbus::connection::Builder::address(session.address())
            .name(NAME)
            .serve_at(
                ROOT,
                Orders {
                    calls: Arc::clone(&calls),
                    label: LABEL.to_owned(),
                },
            )
            .serve_at(
                CHILD,
                Order {
                    calls: Arc::clone(&calls),
                },
            )
            .build()
            .await
            .map_err(failed("starting the service"))?;
        Ok(Self { calls, _conn: conn })
    }

    /// Every member of the service asked so far.
    #[must_use]
    pub fn calls(&self) -> Calls {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
