//! The presence prompt of a Linux desktop: polkit.
//!
//! The engine asks polkit's authority, on the system bus, whether its own
//! process is authorized for one action of the project's own. The action's
//! policy file says `auth_self` for a local, active session and no for
//! every other, so the desktop's authentication agent asks the person for
//! their password, and a privileged helper of polkit checks it. The
//! engine never sees the password; it gets a yes or a no.
//!
//! Three things keep a yes worth something:
//!
//! - **Before anybody is asked, the authority is asked without
//!   interaction.** Only "would have to ask" lets the prompt be raised. A
//!   yes at that point was given without asking anybody (a rule an
//!   administrator wrote, or an authorization that was kept), and proves
//!   nobody; a plain no means the engine is outside a local, active
//!   session.
//! - **An error is never a yes.** Every way the bus or polkit can fail
//!   maps to a named outcome that records nothing: [`answered`] and
//!   [`standing`] are total, and their tests walk every case.
//! - **The deadline cancels the dialog**, through the cancellation id.
//!
//! What it does not prove is in spec section 222: the dialog shows the
//! policy file's fixed message and not the change, and a program that
//! knows or has captured the password can pass it.
//!
//! The bus is behind two small traits, [`Bus`] and [`Authority`], so that
//! the mapping is tested with a made-up authority. The real one is
//! [`SystemBus`].

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2m_actions::BoxFuture;
use tokio::time::Instant;
use zbus_polkit::policykit1::{AuthorityProxy, CheckAuthorizationFlags, Subject};

use super::{Availability, PresencePrompt, PresenceReason, PromptAnswer, PromptName, PromptText};

/// The one polkit action of the project: "a person is present". The
/// policy file declares it.
pub(crate) const ACTION_ID: &str = "io.github.mhughesdev.anything2mcp.presence";
/// The policy file's name, as it ships in `crates/a2m-local/polkit` and as
/// an administrator installs it.
pub(crate) const POLICY_FILE: &str = "io.github.mhughesdev.anything2mcp.presence.policy";
/// Where an administrator installs the policy file.
pub(crate) const POLICY_DIR: &str = "/usr/share/polkit-1/actions";

/// The longest a call that asks nobody may take. A call that raises the
/// prompt is bounded by its deadline instead.
const PATIENCE: Duration = Duration::from_secs(10);
/// The most characters of an error's name that go into a message.
const NAME_SHOWN: usize = 96;

const SERVICE_UNKNOWN: &str = "org.freedesktop.DBus.Error.ServiceUnknown";
const NAME_HAS_NO_OWNER: &str = "org.freedesktop.DBus.Error.NameHasNoOwner";
const SPAWN_FAILED: &str = "org.freedesktop.DBus.Error.Spawn.";
/// The errors of a service manager that the bus asked to start polkit and
/// that did not: the unit is missing, masked, or failed.
const UNIT_FAILED: &str = "org.freedesktop.systemd1.";
const POLKIT_FAILED: &str = "org.freedesktop.PolicyKit1.Error.Failed";
const POLKIT_CANCELLED: &str = "org.freedesktop.PolicyKit1.Error.Cancelled";
/// The key of a result's details that polkit sets when the person closed
/// the dialog.
const DISMISSED_DETAIL: &str = "polkit.dismissed";

/// What the authority answered to one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Verdict {
    /// The process is authorized.
    pub(crate) authorized: bool,
    /// It could be, if somebody authenticated, and nobody was asked.
    pub(crate) challenge: bool,
    /// The person closed the dialog.
    pub(crate) dismissed: bool,
}

/// A way the bus or the authority failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fault {
    /// No system bus could be connected to.
    NoSystemBus,
    /// The bus has no polkit on it, and could not start one.
    NoPolkit,
    /// polkit does not know the engine's action: the policy file is not
    /// installed.
    NotRegistered,
    /// polkit refused to make the check, and knows the action.
    Refused,
    /// The check was cancelled.
    Cancelled,
    /// Anything else, with the name of the error where it has one.
    Other(String),
}

/// A way to the authority, for one ask.
pub(crate) trait Bus: Send + Sync {
    /// Connects. Every ask connects anew: the bus can come and go while
    /// the engine runs, and a connection that ends takes its prompt along.
    fn connect(&self) -> BoxFuture<'_, Result<Arc<dyn Authority>, Fault>>;
}

/// polkit's authority, as far as the prompt needs it. Every call is about
/// this process and [`ACTION_ID`].
pub(crate) trait Authority: Send + Sync {
    /// `CheckAuthorization`. With `interactive`, polkit may ask the
    /// person and the call waits for them; `cancellation` then names the
    /// check for [`cancel`](Self::cancel).
    fn check<'a>(
        &'a self,
        interactive: bool,
        cancellation: &'a str,
    ) -> BoxFuture<'a, Result<Verdict, Fault>>;

    /// `CancelCheckAuthorization`: takes the dialog of that check away.
    fn cancel<'a>(&'a self, cancellation: &'a str) -> BoxFuture<'a, ()>;

    /// Whether polkit knows the engine's action.
    fn registered(&self) -> BoxFuture<'_, Result<bool, Fault>>;
}

fn installing() -> String {
    format!(
        "An administrator copies `{POLICY_FILE}` into `{POLICY_DIR}`. The file ships with the product; in its source it is under `crates/a2m-local/polkit`."
    )
}

/// What a fault means for a person who wants to approve.
pub(crate) fn unavailable(fault: &Fault) -> Availability {
    match fault {
        Fault::NoSystemBus => Availability::not_available(
            PresenceReason::NoSystemBus,
            "No system bus answers on this machine, so polkit cannot be asked for a presence prompt.",
            "Nothing on this machine can approve: it is a container, or it has no desktop. Approve at a desktop, with the engine started inside your own session.",
        ),
        Fault::NoPolkit => Availability::not_available(
            PresenceReason::NoPolkit,
            "The system bus of this machine has no polkit on it, so there is no presence prompt.",
            "Install polkit, and use a desktop session that runs a polkit authentication agent.",
        ),
        Fault::NotRegistered => Availability::not_available(
            PresenceReason::PolicyNotInstalled,
            format!(
                "The policy file that declares the engine's polkit action `{ACTION_ID}` is not installed, so polkit cannot ask for it."
            ),
            installing(),
        ),
        Fault::Refused => Availability::not_available(
            PresenceReason::CheckFailed,
            "polkit refused to check the engine's own process.",
            "Run `anything2mcp doctor run` again. If it stays, polkit's log on this machine says why.",
        ),
        Fault::Cancelled => Availability::not_available(
            PresenceReason::CheckFailed,
            "The check with polkit was cancelled before it answered.",
            "Open the approval page again and press once more.",
        ),
        Fault::Other(name) => Availability::not_available(
            PresenceReason::CheckFailed,
            format!("The check with polkit failed ({name})."),
            "Run `anything2mcp doctor run` again. If it stays, the system bus and polkit's log on this machine say why.",
        ),
    }
}

/// What a check that may ask nobody says about this process: `Ok` only
/// when polkit would have to ask a person.
pub(crate) fn standing(answer: Result<Verdict, Fault>) -> Result<(), Availability> {
    let verdict = answer.map_err(|fault| unavailable(&fault))?;
    match (verdict.authorized, verdict.challenge) {
        (false, true) => Ok(()),
        (true, _) => Err(Availability::not_available(
            PresenceReason::GrantedWithoutAsking,
            format!(
                "polkit would allow the engine's action `{ACTION_ID}` without asking anybody, so its answer proves that nobody is present."
            ),
            format!(
                "An administrator removes the rule or the kept authorization that allows the action, and installs `{POLICY_FILE}` as it ships. The engine is never run as the administrator account."
            ),
        )),
        (false, false) => Err(Availability::not_available(
            PresenceReason::SessionNotActive,
            "The engine does not run inside a local, active session of a person, so polkit asks nobody.",
            "Start the engine from your own desktop session at the machine itself. Over a remote shell, in a container, and on a machine with no screen, nothing can approve.",
        )),
    }
}

/// What a check that may ask the person says once it has ended. Only one
/// verdict is a person's presence; every fault, and every verdict that
/// does not make sense, is not.
pub(crate) fn answered(answer: Result<Verdict, Fault>) -> PromptAnswer {
    let verdict = match answer {
        Ok(verdict) => verdict,
        Err(Fault::Cancelled) => return PromptAnswer::Dismissed,
        Err(fault) => return PromptAnswer::NotAvailable(unavailable(&fault)),
    };
    match (verdict.authorized, verdict.challenge, verdict.dismissed) {
        (true, false, false) => PromptAnswer::Present(PromptName::Polkit),
        (false, false, true) => PromptAnswer::Dismissed,
        (false, false, false) => PromptAnswer::Denied,
        // Interaction was allowed and polkit still only says "could be":
        // there was no agent to ask through.
        (false, true, _) => PromptAnswer::NotAvailable(Availability::not_available(
            PresenceReason::NoAgent,
            "No polkit authentication agent runs for this session, so nothing can show the presence prompt.",
            "Start your desktop's polkit authentication agent, or log in to a desktop that starts one, and press Approve again.",
        )),
        // A yes that also says it was not given: polkit sends no such
        // answer, and an answer that is not understood is not a yes.
        (true, _, _) => {
            PromptAnswer::NotAvailable(unavailable(&Fault::Other("inconsistent_answer".to_owned())))
        }
    }
}

/// The fault an error of the bus is, by the error's name. An error with
/// no name is trouble with the connection itself.
pub(crate) fn fault_named(name: Option<&str>) -> Fault {
    match name {
        Some(SERVICE_UNKNOWN | NAME_HAS_NO_OWNER) => Fault::NoPolkit,
        // Every call here goes to polkit, so a failure to start a service
        // is a failure to start polkit.
        Some(name) if name.starts_with(SPAWN_FAILED) || name.starts_with(UNIT_FAILED) => {
            Fault::NoPolkit
        }
        Some(POLKIT_FAILED) => Fault::Refused,
        Some(POLKIT_CANCELLED) => Fault::Cancelled,
        // Only the name is shown, and only its plain characters: nothing
        // of an error's text goes into a message.
        Some(name) => Fault::Other(
            name.chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                .take(NAME_SHOWN)
                .collect(),
        ),
        None => Fault::Other("bus_connection".to_owned()),
    }
}

fn fault_of(error: &zbus::Error) -> Fault {
    let name = match error {
        zbus::Error::MethodError(name, _, _) => Some(name.as_str().to_owned()),
        zbus::Error::FDO(inner) => Some(zbus::DBusError::name(inner.as_ref()).as_str().to_owned()),
        _ => None,
    };
    fault_named(name.as_deref())
}

/// `step`, or a fault when it takes longer than a call that asks nobody
/// may.
async fn bounded<T>(step: impl Future<Output = Result<T, Fault>>) -> Result<T, Fault> {
    match tokio::time::timeout(PATIENCE, step).await {
        Ok(done) => done,
        Err(_) => Err(Fault::Other("timed_out".to_owned())),
    }
}

/// The presence prompt over polkit.
pub(crate) struct PolkitPrompt {
    bus: Arc<dyn Bus>,
    /// Counts the asks, so that each check has a cancellation id of its
    /// own.
    asked: AtomicU64,
}

impl PolkitPrompt {
    /// The prompt over the machine's system bus.
    pub(crate) fn system() -> Self {
        Self::over(Arc::new(SystemBus::default()))
    }

    /// The prompt over `bus`.
    pub(crate) fn over(bus: Arc<dyn Bus>) -> Self {
        Self {
            bus,
            asked: AtomicU64::new(0),
        }
    }

    /// Connects and asks the authority, without interaction, how this
    /// process stands. Gives the authority back only when polkit would
    /// have to ask a person.
    async fn ready(&self) -> Result<Arc<dyn Authority>, Availability> {
        let authority = bounded(self.bus.connect())
            .await
            .map_err(|fault| unavailable(&fault))?;
        let mut answer = bounded(authority.check(false, "")).await;
        if answer == Err(Fault::Refused) {
            // polkit refuses a check for an action it does not know. Its
            // list of actions tells that apart from any other refusal.
            answer = match bounded(authority.registered()).await {
                Ok(false) => Err(Fault::NotRegistered),
                Ok(true) => Err(Fault::Refused),
                Err(fault) => Err(fault),
            };
        }
        standing(answer).map(|()| authority)
    }
}

impl PresencePrompt for PolkitPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async move {
            match self.ready().await {
                Ok(_) => Availability::ready(
                    PromptName::Polkit,
                    "Approvals on this machine ask for your password through polkit. Its dialog cannot show which change it is for, so read the approval page first, and close a dialog you did not ask for.",
                ),
                Err(why) => why,
            }
        })
    }

    fn ask(&self, _text: PromptText, deadline: Instant) -> BoxFuture<'_, PromptAnswer> {
        // The words are not used: polkit lets a caller that is not an
        // administrator pass none, so the dialog shows the policy file's
        // fixed message.
        Box::pin(async move {
            let authority = match self.ready().await {
                Ok(authority) => authority,
                Err(why) => return PromptAnswer::NotAvailable(why),
            };
            // At or after the deadline nothing is raised.
            if Instant::now() >= deadline {
                return PromptAnswer::Dismissed;
            }
            let id = format!(
                "anything2mcp-presence-{}",
                self.asked.fetch_add(1, Ordering::SeqCst)
            );
            tokio::select! {
                // The deadline is looked at first: at or after it the
                // answer is a dismissal, whatever the check says.
                biased;
                () = tokio::time::sleep_until(deadline) => {
                    let _ = tokio::time::timeout(PATIENCE, authority.cancel(&id)).await;
                    PromptAnswer::Dismissed
                }
                answer = authority.check(true, &id) => answered(answer),
            }
        })
    }
}

/// The system bus of this machine, or for a test the bus at an address.
#[derive(Debug, Default)]
pub(crate) struct SystemBus {
    address: Option<String>,
}

impl SystemBus {
    /// The bus at `address` in the place of the system bus.
    #[cfg(test)]
    pub(crate) fn at(address: &str) -> Self {
        Self {
            address: Some(address.to_owned()),
        }
    }
}

impl Bus for SystemBus {
    fn connect(&self) -> BoxFuture<'_, Result<Arc<dyn Authority>, Fault>> {
        Box::pin(async move {
            let connection = match &self.address {
                None => zbus::Connection::system().await,
                Some(address) => {
                    zbus::connection::Builder::address(address.as_str())
                        .build()
                        .await
                }
            }
            .map_err(|_| Fault::NoSystemBus)?;
            let proxy = AuthorityProxy::builder(&connection)
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .map_err(|error| fault_of(&error))?;
            Ok(Arc::new(Polkit { proxy }) as Arc<dyn Authority>)
        })
    }
}

/// polkit's authority on a connected bus.
struct Polkit {
    proxy: AuthorityProxy<'static>,
}

/// This process, as polkit names one: its id, its start time, which
/// polkit's library reads from the process table, and its user. A process
/// id cannot be given to another process while this one runs.
fn own_process() -> Result<Subject, Fault> {
    let user = rustix::process::getuid().as_raw();
    Subject::new_for_pid(std::process::id(), None, Some(user))
        .map_err(|_| Fault::Other("own_process".to_owned()))
}

impl Authority for Polkit {
    fn check<'a>(
        &'a self,
        interactive: bool,
        cancellation: &'a str,
    ) -> BoxFuture<'a, Result<Verdict, Fault>> {
        Box::pin(async move {
            let subject = own_process()?;
            let flags = if interactive {
                CheckAuthorizationFlags::ALLOW_USER_INTERACTION
            } else {
                CheckAuthorizationFlags::empty()
            };
            // No details: polkit refuses them from a caller that is not an
            // administrator, so that no caller can write the dialog's text.
            let answer = self
                .proxy
                .check_authorization(&subject, ACTION_ID, &HashMap::new(), flags, cancellation)
                .await
                .map_err(|error| fault_of(&error))?;
            Ok(Verdict {
                authorized: answer.is_authorized,
                challenge: answer.is_challenge,
                dismissed: answer
                    .details
                    .get(DISMISSED_DETAIL)
                    .is_some_and(|value| !value.is_empty()),
            })
        })
    }

    fn cancel<'a>(&'a self, cancellation: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            // A check that has already ended has nothing to cancel, which
            // polkit answers with an error; either way the dialog is gone.
            let _ = self.proxy.cancel_check_authorization(cancellation).await;
        })
    }

    fn registered(&self) -> BoxFuture<'_, Result<bool, Fault>> {
        Box::pin(async move {
            let actions = self
                .proxy
                .enumerate_actions("")
                .await
                .map_err(|error| fault_of(&error))?;
            Ok(actions.iter().any(|action| action.action_id == ACTION_ID))
        })
    }
}
