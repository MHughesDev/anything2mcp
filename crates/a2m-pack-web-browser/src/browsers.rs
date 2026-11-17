//! Browsers by profile: one process per profile, started through the
//! scoped supervisor with a data directory inside the profile root, owned
//! by what it answers over its pipe, and stopped with nothing left behind.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use a2m_core::{Availability, Digest, Environment, Failure, Identity, Redact, TargetName};
use a2m_provider::{
    Access, CheckedPath, Cleanup, Deadline, LaunchSpec, PackContext, ProgramRef, RootName,
    ScopedSupervisor, SubsystemId, SubsystemKind,
};

use crate::client::{Client, ClientError, Why};
use crate::error::{BrowserError, Dispatch, LaunchReason, codes, input};
use crate::launch;
use crate::protocol::{CloseBrowser, GetVersion};
use crate::tab::{ActTab, ObserveTab, PageEnvironment, Posture, SafeRequests, open_page};

/// The named root that holds the browsers' data directories. The manifest
/// of the pack that uses this crate declares it under `write_roots`.
pub const PROFILE_ROOT: &str = "profiles";

/// The kind of subsystem a browser is, as the supervisor counts it.
pub const BROWSER_KIND: &str = "browser";

/// The version of the DevTools protocol the engine speaks.
const PROTOCOL_VERSION: &str = "1.3";

/// How the engine finds and bounds its browsers. Made with
/// [`BrowserSettings::default`] and changed field by field.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BrowserSettings {
    /// A browser program a person named in settings. When it is set,
    /// nothing else is looked for.
    pub program: Option<PathBuf>,
    /// The digests of the browser programs a person has reviewed. When
    /// the list is not empty, a program with another digest is closed.
    pub known_digests: Vec<Digest>,
    /// How long a browser may take to start and answer over its pipe. A
    /// ceiling for a cold machine, not a budget: a browser's first start
    /// after the machine came up, with the system's scanner reading it,
    /// was measured at over 30 seconds on a hosted Windows machine.
    pub launch_deadline: Duration,
    /// How long a browser may take to close by itself before its process
    /// group is ended.
    pub close_deadline: Duration,
    /// The deadline of a protocol call that has none of its own: opening
    /// a tab, setting its environment, answering an intercepted request.
    pub call_deadline: Duration,
    /// The largest message taken from the browser, in bytes. A larger one
    /// closes the link.
    pub max_message_bytes: usize,
    /// The longest text one read may return, in characters.
    pub max_read_chars: usize,
    /// Test equipment only: starts the browser without its sandbox, for a
    /// machine whose tests run as the superuser. It exists only in a test
    /// build, and nothing in the product can set it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) without_sandbox: bool,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            program: None,
            known_digests: Vec::new(),
            launch_deadline: Duration::from_secs(120),
            close_deadline: Duration::from_secs(10),
            call_deadline: Duration::from_secs(30),
            max_message_bytes: 64 * 1024 * 1024,
            max_read_chars: 1024 * 1024,
            #[cfg(any(test, feature = "test-support"))]
            without_sandbox: false,
        }
    }
}

impl BrowserSettings {
    /// The same settings with the browser program a person named.
    #[must_use]
    pub fn with_program(mut self, program: PathBuf) -> Self {
        self.program = Some(program);
        self
    }

    /// The same settings with the digests a person has reviewed.
    #[must_use]
    pub fn with_known_digests(mut self, digests: Vec<Digest>) -> Self {
        self.known_digests = digests;
        self
    }
}

/// Which browser: one per target, environment, and identity. Two keys
/// never share a process or a data directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileKey {
    /// The target.
    pub target: TargetName,
    /// The environment.
    pub environment: Environment,
    /// Who the system sees.
    pub identity: Identity,
}

impl ProfileKey {
    /// The key's data directory, relative to the profile root. An identity
    /// can be any text, so its part of the path is a digest of it: the
    /// same on every operating system, and never a way out of the root.
    #[must_use]
    pub fn place(&self) -> String {
        let kind = match self.identity {
            Identity::Person(_) => "person",
            Identity::Account(_) => "account",
        };
        let digest = Digest::of(self.identity.to_string().as_bytes()).to_string();
        let short: String = digest.chars().take(32).collect();
        format!("{}/{}/{kind}-{short}", self.target, self.environment)
    }
}

/// Whether a browser shows a window.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Visibility {
    /// No window.
    Background,
}

/// What a browser is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserHealth {
    /// Running, with no tab open.
    Idle,
    /// Running, with at least one tab open.
    Busy,
    /// The process has ended, or its link is closed.
    Exited,
}

/// One open browser, for `doctor.run` and the supervisor's listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenBrowser {
    /// Its profile.
    pub profile: ProfileKey,
    /// The supervisor's identifier of its process.
    pub subsystem: SubsystemId,
    /// What it reports itself to be, such as its name and version.
    pub product: String,
    /// The digest of the program that runs.
    pub program_digest: Digest,
    /// Whether that digest is one that was declared or reviewed.
    pub verified: bool,
    /// How many tabs are open.
    pub tabs: usize,
    /// How many acting tabs it has opened since it started.
    pub runs: u64,
    /// What it is doing.
    pub health: BrowserHealth,
}

pub(crate) struct BrowserInner {
    pub(crate) profile: ProfileKey,
    pub(crate) subsystem: SubsystemId,
    pub(crate) client: Client,
    pub(crate) product: String,
    program_digest: Digest,
    verified: bool,
    pub(crate) tabs: AtomicUsize,
    runs: AtomicU64,
}

impl BrowserInner {
    fn health(&self) -> BrowserHealth {
        if self.client.closed().is_some() {
            BrowserHealth::Exited
        } else if self.tabs.load(Ordering::SeqCst) > 0 {
            BrowserHealth::Busy
        } else {
            BrowserHealth::Idle
        }
    }
}

pub(crate) struct Shared {
    supervisor: ScopedSupervisor,
    root: Result<CheckedPath, Failure>,
    pub(crate) redact: Arc<dyn Redact>,
    pub(crate) settings: BrowserSettings,
    open: Mutex<BTreeMap<ProfileKey, Arc<BrowserInner>>>,
    /// One launch at a time, so that a profile is never started twice.
    launching: tokio::sync::Mutex<()>,
    pub(crate) next_tab: AtomicU64,
}

impl Shared {
    fn open(&self) -> MutexGuard<'_, BTreeMap<ProfileKey, Arc<BrowserInner>>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Held by every start of a browser in this process, whichever set of
/// browsers asks, until the browser has answered or been closed.
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// For the tests: how many starts were under way at once, at the most.
#[cfg(test)]
pub(crate) mod starts {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NOW: AtomicUsize = AtomicUsize::new(0);
    static MOST: AtomicUsize = AtomicUsize::new(0);
    static ALL: AtomicUsize = AtomicUsize::new(0);

    pub(crate) struct Counted;

    impl Counted {
        pub(crate) fn begin() -> Self {
            let now = NOW.fetch_add(1, Ordering::SeqCst) + 1;
            MOST.fetch_max(now, Ordering::SeqCst);
            ALL.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

    impl Drop for Counted {
        fn drop(&mut self) {
            NOW.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// The most starts under way at once, and how many there were in all.
    pub(crate) fn seen() -> (usize, usize) {
        (MOST.load(Ordering::SeqCst), ALL.load(Ordering::SeqCst))
    }
}

/// Why the first request over the pipe had no answer, with the step that
/// failed: whether the request could be written at all, whether the pipe
/// ended, and what the program said. A start on a new operating system is
/// diagnosed from this sentence alone.
fn unanswered(error: &ClientError, waited: Duration, redact: &dyn Redact) -> String {
    let written = |dispatch: &Dispatch| match dispatch {
        Dispatch::Sent => "the first request was written to it",
        Dispatch::NotSent => "the first request was never written to it",
        Dispatch::Unknown => "the first request could not be written to it whole",
    };
    match error {
        ClientError::Timeout { dispatch } => format!(
            "it did not answer over its pipe within {} s, and its pipe stayed open; {}",
            waited.as_secs(),
            written(dispatch)
        ),
        ClientError::Closed { dispatch, why } => format!(
            "its pipe {} before it answered; {}",
            match why {
                Why::Ended => "ended, so the program exited or closed it,",
                Why::WriteFailed => "could not be written to",
                Why::Malformed => "carried something that is not the protocol",
                Why::Oversized => "carried a message over the size limit",
                Why::Unanswered => "carried a reply to a request that was never made",
                Why::ShutDown => "was closed by the engine",
            },
            written(dispatch)
        ),
        ClientError::Protocol { code, message } => {
            let shown: String = message
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            format!(
                "it refused the first request over its pipe with the code {code}: {}",
                redact.redact(&shown)
            )
        }
        ClientError::Shape { method } => {
            format!("its answer to {method} over its pipe has another shape")
        }
    }
}

/// The engine's browsers. One per engine process; cheap to clone, and
/// every clone is the same set, so that a profile is never opened by two
/// browsers.
#[derive(Clone)]
pub struct Browsers {
    pub(crate) shared: Arc<Shared>,
}

impl fmt::Debug for Browsers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Browsers").finish_non_exhaustive()
    }
}

impl Browsers {
    /// Takes what the browsers need from the context of the pack that uses
    /// this crate: its scoped supervisor, its profile root, and the
    /// redactor. Starts nothing and looks at nothing.
    ///
    /// The pack's manifest must declare [`PROFILE_ROOT`] under
    /// `write_roots` and the places of [`crate::KNOWN_PLACES`] under a
    /// requirement, or [`Browsers::open`] is refused by the supervisor.
    ///
    /// # Errors
    ///
    /// `input`/`profile_invalid` when the profile root's name is not one a
    /// root may have. A root the manifest does not declare is reported by
    /// [`Browsers::open`], so that a pack registers on a machine whose
    /// settings name no profile folder yet.
    pub fn new(ctx: &PackContext, settings: BrowserSettings) -> Result<Self, Failure> {
        let name = RootName::new(PROFILE_ROOT)?;
        Ok(Self {
            shared: Arc::new(Shared {
                supervisor: ctx.supervisor().clone(),
                root: ctx.folder(&name, Access::Write),
                redact: Arc::clone(ctx.redact()),
                settings,
                open: Mutex::new(BTreeMap::new()),
                launching: tokio::sync::Mutex::new(()),
                next_tab: AtomicU64::new(1),
            }),
        })
    }

    /// Whether a browser program is installed. Looks at the machine and
    /// starts nothing.
    #[must_use]
    pub fn availability(&self) -> Availability {
        if launch::find(self.shared.settings.program.as_deref()).is_some() {
            Availability::Ready
        } else {
            Availability::ComponentMissing
        }
    }

    /// The profile's browser, started on first use. A browser whose
    /// process has ended is stopped, cleaned up, and started afresh.
    ///
    /// # Errors
    ///
    /// [`BrowserError::LaunchFailed`]: no browser is installed
    /// (`busy`/`browser_unavailable`); the supervisor refused the program,
    /// the place, or the count (its own failure, such as
    /// `policy`/`effect_not_declared`); the data directory is not inside
    /// the profile root; or what started is not a browser the engine can
    /// own (`unknown_variant`/`browser_not_owned`), or it did not answer
    /// before the launch deadline (`transient`/`timeout`); in both cases
    /// it was closed.
    pub async fn open(
        &self,
        profile: &ProfileKey,
        show: Visibility,
    ) -> Result<Browser, BrowserError> {
        let shared = &self.shared;
        let _one = shared.launching.lock().await;
        let existing = shared.open().get(profile).cloned();
        if let Some(existing) = existing {
            if existing.health() != BrowserHealth::Exited {
                return Ok(Browser {
                    inner: existing,
                    shared: Arc::clone(shared),
                });
            }
            shared.open().remove(profile);
            existing.client.close();
            shared.supervisor.stop(existing.subsystem).await;
        }
        let Visibility::Background = show;
        let failed = |reason| BrowserError::LaunchFailed(reason);

        let program = launch::find(shared.settings.program.as_deref())
            .ok_or_else(|| failed(LaunchReason::NotInstalled))?;
        let place = profile.place();
        let directory = shared
            .root
            .clone()
            .and_then(|root| root.join(&place))
            .and_then(|directory| directory.create_dir_all().map(|()| directory))
            .map_err(|failure| failed(LaunchReason::DataDirectory(failure)))?;
        #[allow(unused_mut, reason = "only a test build adds an argument")]
        let mut args = launch::arguments(directory.as_path(), false);
        #[cfg(any(test, feature = "test-support"))]
        if shared.settings.without_sandbox {
            args.push(crate::test_support::WITHOUT_SANDBOX.into());
        }
        let kind = SubsystemKind::new(BROWSER_KIND)
            .map_err(|failure| failed(LaunchReason::Supervisor(failure)))?;
        // One start at a time in the whole process, from the start of the
        // program to its first answer. Several browsers that start cold at
        // the same moment each take many times longer than one after the
        // other, and a start is bounded by a deadline.
        let _one_start = STARTING.lock().await;
        #[cfg(test)]
        let _counted = starts::Counted::begin();
        let link = shared
            .supervisor
            .start_foreign(
                LaunchSpec {
                    program: ProgramRef {
                        path: program,
                        digest: None,
                    },
                    args,
                    env: launch::environment(),
                    cwd: None,
                    kind,
                    place: Some(place),
                },
                launch::pipe(),
            )
            .await
            .map_err(|failure| failed(LaunchReason::Supervisor(failure)))?;

        let subsystem = link.id;
        let known = &shared.settings.known_digests;
        let reviewed = known.contains(&link.program_digest);
        let client = Client::connect(
            link.from_child,
            link.to_child,
            shared.settings.max_message_bytes,
        );
        // Ownership: the digest of the program that runs, and what it
        // answers over the pipe only its owner holds.
        let owned = if !known.is_empty() && !reviewed {
            Err(LaunchReason::NotOwned(
                "its digest is not one of the known digests in the settings".to_owned(),
            ))
        } else {
            match client
                .call(
                    None,
                    &GetVersion {},
                    Deadline::after(shared.settings.launch_deadline),
                )
                .await
            {
                Ok(version) if version.protocol_version == PROTOCOL_VERSION => Ok(version.product),
                Ok(_) => Err(LaunchReason::NotOwned(
                    "it speaks another version of the protocol".to_owned(),
                )),
                Err(error) => {
                    let why = unanswered(
                        &error,
                        shared.settings.launch_deadline,
                        shared.redact.as_ref(),
                    );
                    // Silence with the pipe still open is not proof of
                    // anything but slowness: a browser on a cold machine
                    // can take longer than any deadline. Everything else
                    // is a program that is not a browser.
                    Err(match error {
                        ClientError::Timeout { .. } => LaunchReason::NoAnswer(why),
                        _ => LaunchReason::NotOwned(why),
                    })
                }
            }
        };
        let product = match owned {
            Ok(product) => product,
            Err(reason) => {
                client.close();
                shared.supervisor.stop(subsystem).await;
                return Err(failed(reason));
            }
        };
        let inner = Arc::new(BrowserInner {
            profile: profile.clone(),
            subsystem,
            client,
            product,
            program_digest: link.program_digest,
            verified: link.verified || reviewed,
            tabs: AtomicUsize::new(0),
            runs: AtomicU64::new(0),
        });
        shared.open().insert(profile.clone(), Arc::clone(&inner));
        Ok(Browser {
            inner,
            shared: Arc::clone(shared),
        })
    }

    /// A handle that can open observing tabs and nothing else. The pack
    /// gives its discoverers this, never the `Browsers`.
    #[must_use]
    pub fn observer(&self) -> Observer {
        Observer {
            browsers: self.clone(),
        }
    }

    /// Every open browser, in order of profile.
    #[must_use]
    pub fn list(&self) -> Vec<OpenBrowser> {
        self.shared
            .open()
            .values()
            .map(|browser| OpenBrowser {
                profile: browser.profile.clone(),
                subsystem: browser.subsystem,
                product: browser.product.clone(),
                program_digest: browser.program_digest,
                verified: browser.verified,
                tabs: browser.tabs.load(Ordering::SeqCst),
                runs: browser.runs.load(Ordering::SeqCst),
                health: browser.health(),
            })
            .collect()
    }

    /// Closes the profile's browser: asks it to close, so that it writes
    /// its session to its data directory, waits for its end up to the
    /// close deadline, then has the supervisor end and reap its whole
    /// process group. A profile with no browser is a complete cleanup.
    pub async fn close(&self, profile: &ProfileKey) -> Cleanup {
        let shared = &self.shared;
        let _one = shared.launching.lock().await;
        let Some(browser) = shared.open().remove(profile) else {
            return Cleanup::complete();
        };
        if browser.client.closed().is_none() {
            let deadline = Deadline::after(shared.settings.close_deadline);
            let mut ended = browser.client.subscribe(None);
            if browser
                .client
                .call(None, &CloseBrowser {}, deadline)
                .await
                .is_ok()
            {
                // The end of the pipe is the end of the browser.
                let _ = tokio::time::timeout_at(deadline.at(), async {
                    while ended.recv().await.is_some() {}
                })
                .await;
            }
        }
        browser.client.close();
        shared.supervisor.stop(browser.subsystem).await
    }

    /// Closes every browser. What could not be cleaned up is listed.
    pub async fn shutdown(&self) -> Cleanup {
        let profiles: Vec<ProfileKey> = self.shared.open().keys().cloned().collect();
        let mut remaining = Vec::new();
        for profile in profiles {
            remaining.extend(self.close(&profile).await.remaining);
        }
        Cleanup::incomplete(remaining)
    }
}

/// One browser process. Cheap to clone.
#[derive(Clone)]
pub struct Browser {
    inner: Arc<BrowserInner>,
    shared: Arc<Shared>,
}

impl fmt::Debug for Browser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Browser")
            .field("subsystem", &self.inner.subsystem)
            .finish_non_exhaustive()
    }
}

impl Browser {
    /// A new tab that may act, with the fixed environment given.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Disconnected`] or [`BrowserError::Timeout`] when the
    /// browser is gone or does not answer.
    pub async fn acting_tab(&self, env: &PageEnvironment) -> Result<ActTab, BrowserError> {
        let page = open_page(&self.shared, &self.inner, env, Posture::Act).await?;
        self.inner.runs.fetch_add(1, Ordering::SeqCst);
        Ok(ActTab::new(page))
    }

    /// What the browser is doing.
    #[must_use]
    pub fn health(&self) -> BrowserHealth {
        self.inner.health()
    }

    /// What the browser reports itself to be: its name and version.
    #[must_use]
    pub fn product(&self) -> &str {
        &self.inner.product
    }

    /// The supervisor's identifier of the browser's process.
    #[must_use]
    pub fn subsystem(&self) -> SubsystemId {
        self.inner.subsystem
    }
}

/// A set of browsers that can only look: it opens observing tabs, which
/// have no method that acts, and there is no way from it to an acting tab.
#[derive(Clone)]
pub struct Observer {
    browsers: Browsers,
}

impl fmt::Debug for Observer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observer").finish_non_exhaustive()
    }
}

impl Observer {
    /// A new tab that can read and cannot act, in the profile's browser,
    /// which is started when it is not running. Every request of the page,
    /// its frames, and its workers whose method is not GET, HEAD, or
    /// OPTIONS is failed before it leaves the browser, unless `allow`
    /// names it; each one is reported by [`ObserveTab::blocked`].
    ///
    /// # Errors
    ///
    /// As [`Browsers::open`] and [`Browser::acting_tab`].
    pub async fn observing_tab(
        &self,
        profile: &ProfileKey,
        env: &PageEnvironment,
        allow: &SafeRequests,
    ) -> Result<ObserveTab, BrowserError> {
        let browser = self.browsers.open(profile, Visibility::Background).await?;
        let page = open_page(
            &browser.shared,
            &browser.inner,
            env,
            Posture::Observe(allow.clone()),
        )
        .await?;
        Ok(ObserveTab::new(page))
    }

    /// Whether a browser program is installed. Starts nothing.
    #[must_use]
    pub fn availability(&self) -> Availability {
        self.browsers.availability()
    }
}

/// An address a tab may be sent to: an absolute `http` or `https` address
/// with no space or control character. Whether its origin is one the
/// target declared is for the crate above, which knows the target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Address(String);

impl Address {
    /// Checks the text.
    ///
    /// # Errors
    ///
    /// `input`/`address_invalid`.
    pub fn new(text: &str) -> Result<Self, Failure> {
        let rest = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://"));
        let host = rest.map(|rest| rest.split(['/', '?', '#']).next().unwrap_or_default());
        let plain = !text
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\');
        match host {
            Some(host) if plain && !host.is_empty() && !host.contains('@') => {
                Ok(Self(text.to_owned()))
            }
            _ => Err(input(
                codes::ADDRESS_INVALID,
                "An address a tab is sent to is an absolute http or https address, with no space, no backslash, and no name and password before the host.",
                "Write the whole address, starting with `https://`.",
            )),
        }
    }

    /// The address as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One set of browsers for the two crates of the web pack. A pack is handed
/// its context only while it registers, and the drivers and the discoverers
/// are two packs that must not each start a browser on one profile: the
/// product crate makes one cell and gives a clone to both. The first of
/// them to register builds the `Browsers`; the other receives the same.
#[derive(Clone, Default)]
pub struct SharedBrowsers {
    cell: Arc<Mutex<Option<Browsers>>>,
}

impl fmt::Debug for SharedBrowsers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedBrowsers").finish_non_exhaustive()
    }
}

impl SharedBrowsers {
    /// The cell's browsers, built by [`Browsers::new`] from the context and
    /// settings of the first caller. A later caller's context and settings
    /// are not used: its manifest must declare the same effects, or it is
    /// the first caller's supervisor that answers.
    ///
    /// # Errors
    ///
    /// As [`Browsers::new`]. A failed build leaves the cell empty.
    pub fn get_or_create(
        &self,
        ctx: &PackContext,
        settings: BrowserSettings,
    ) -> Result<Browsers, Failure> {
        let mut cell = self
            .cell
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(browsers) = cell.as_ref() {
            return Ok(browsers.clone());
        }
        let browsers = Browsers::new(ctx, settings)?;
        *cell = Some(browsers.clone());
        Ok(browsers)
    }

    /// The cell's browsers, when a pack has built them.
    #[must_use]
    pub fn get(&self) -> Option<Browsers> {
        self.cell
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A handle that can only look, when a pack has built the browsers.
    #[must_use]
    pub fn observer(&self) -> Option<Observer> {
        self.get().map(|browsers| browsers.observer())
    }
}
