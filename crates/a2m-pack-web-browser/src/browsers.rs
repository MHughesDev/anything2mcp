//! Browsers by profile: one process per profile, started through the
//! scoped supervisor with a data directory inside the profile root, owned
//! by what it answers over its pipe, and stopped with nothing left behind.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use a2m_core::{
    Availability, Digest, Environment, Failure, FailureClass, Identity, Redact, Secret, TargetName,
    Values,
};
use a2m_provider::{
    Access, CheckedPath, Cleanup, Deadline, LaunchSpec, Leftover, PackContext, ProgramRef,
    RootName, ScopedSupervisor, SubsystemId, SubsystemKind,
};

use crate::client::{Client, ClientError, Why};
use crate::error::{BrowserError, Dispatch, LaunchReason, MemoryShort, codes, input};
use crate::launch;
use crate::origin::Origins;
use crate::protocol::{
    CloseBrowser, CloseTarget, CookieToSet, CreateBrowserContext, GetCookies, GetProcessInfo,
    GetTargets, GetVersion, GrantPermissions, SetAutoAttach, SetCookies, SetDocumentContent,
    SetDownloadBehavior, TargetFilter,
};
use crate::safe::SafeRequests;
use crate::tab::{
    ActTab, ObserveTab, Page, PageEnvironment, Posture, WatchStart, WatchTab, open_page, shown,
    watch_windows,
};

/// The named root that holds the browsers' data directories. The manifest
/// of the pack that uses this crate declares it under `write_roots`.
pub const PROFILE_ROOT: &str = "profiles";

/// The folder, inside the profile root, that files for upload are staged
/// in: a file input is given a file of this folder and of no other.
pub const STAGING_FOLDER: &str = "staging";

/// The folder, inside the profile root, that a download a step declares is
/// written into, under a name the browser makes.
pub const DOWNLOADS_FOLDER: &str = "downloads";

/// The kind of subsystem a browser is, as the supervisor counts it.
pub const BROWSER_KIND: &str = "browser";

/// How many hangs of its tabs a browser takes before it is recycled, when
/// the settings say nothing else (decision 0422). A tab hangs when its
/// page gives no answer to a call by the call's deadline, nor to the probe
/// after it ([`BrowserSettings::hang_probe`]).
pub const HANGS_BEFORE_RECYCLE: u32 = 3;

/// How long a tab whose call went unanswered is given to answer a probe
/// before it is called hung and closed, when the settings say nothing else
/// (decision 0422).
pub const HANG_PROBE: Duration = Duration::from_secs(5);

/// The most memory one browser may hold, its processes together, before it
/// is recycled between runs, when the settings say nothing else (decision
/// 0425): two gibibytes. A draft, set when first measured on a long run
/// (spec section 226); a browser of the test application with one tab was
/// measured at about 300 MB on Linux.
pub const MEMORY_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The most memory the engine's browsers may hold together for new browser
/// work to start, when the settings say nothing else (decision 0425): four
/// gibibytes, two browsers at their limit. A draft, as [`MEMORY_LIMIT`] is.
pub const MEMORY_CEILING: u64 = 4 * 1024 * 1024 * 1024;

/// How long a call refused because memory runs short is told to wait
/// before it asks again, when the settings say nothing else (decision
/// 0425).
pub const MEMORY_RETRY: Duration = Duration::from_secs(30);

/// The version of the DevTools protocol the engine speaks.
const PROTOCOL_VERSION: &str = "1.3";

/// The key, in the settings group of a pack built on this crate, of the
/// digests of the browser programs a person has reviewed: a list of
/// texts, each a digest. The pack's manifest declares it, and
/// [`BrowserSettings::with_pack_settings`] reads it.
pub const DIGESTS_SETTING: &str = "browser_digests";

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
    /// How many hangs of its tabs a browser takes before it is recycled:
    /// closed and started afresh, at the next open that finds no tab of it
    /// held, which is between two runs and never inside one. A count of 0
    /// is taken as 1. [`HANGS_BEFORE_RECYCLE`] by default.
    pub hangs_before_recycle: u32,
    /// How long a tab of the engine's whose call went unanswered by its
    /// deadline is given to answer a probe that asks the page for nothing
    /// but an answer. A tab that gives none is hung: it is closed, and the
    /// hang is counted against its browser. [`HANG_PROBE`] by default.
    pub hang_probe: Duration,
    /// The most memory one browser may hold, in bytes, its processes
    /// together, as their operating system counts what each costs. A
    /// browser past it is recycled at the next open that finds no tab of it
    /// held, between two runs and never inside one; so is one whose memory
    /// cannot be measured. [`MEMORY_LIMIT`] by default.
    pub memory_limit: u64,
    /// The most memory, in bytes, the engine's browsers may hold together
    /// for new browser work to start. An open that would begin work, on a
    /// new browser or on one of whose tabs none is held, while they hold it
    /// or more, or while one of them cannot be measured, starts nothing and
    /// answers `busy`, `memory_short`, with [`Self::memory_retry`] as its
    /// retry time. [`MEMORY_CEILING`] by default.
    pub memory_ceiling: u64,
    /// The retry time of a call refused because memory runs short.
    /// [`MEMORY_RETRY`] by default.
    pub memory_retry: Duration,
    /// Test equipment only: starts the browser without its sandbox, for a
    /// machine whose tests run as the superuser. It exists only in a test
    /// build, and nothing in the product can set it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) without_sandbox: bool,
    /// Tests only: the display a window is shown on, in place of the one
    /// the engine's own environment names.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) display: Option<String>,
    /// This crate's own tests only: variables set in the browser's
    /// environment, to stand for what a machine around the engine may hold,
    /// such as a proxy.
    #[cfg(test)]
    pub(crate) environment_for_tests: Vec<(String, std::ffi::OsString)>,
    /// This crate's own tests only: the first launch that has started its
    /// program says so here and then waits for ever, as a start slower
    /// than its caller's limit does, so that a test can drop it there.
    #[cfg(test)]
    pub(crate) stall_after_start: Option<Arc<StallOnce>>,
}

/// This crate's own tests only: one launch that stops after its program
/// started, and says when it has.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct StallOnce {
    pub(crate) started: tokio::sync::Notify,
    done: std::sync::atomic::AtomicBool,
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
            hangs_before_recycle: HANGS_BEFORE_RECYCLE,
            hang_probe: HANG_PROBE,
            memory_limit: MEMORY_LIMIT,
            memory_ceiling: MEMORY_CEILING,
            memory_retry: MEMORY_RETRY,
            #[cfg(any(test, feature = "test-support"))]
            without_sandbox: false,
            #[cfg(any(test, feature = "test-support"))]
            display: None,
            #[cfg(test)]
            environment_for_tests: Vec::new(),
            #[cfg(test)]
            stall_after_start: None,
        }
    }
}

impl BrowserSettings {
    /// The display a test names for a window, if it names one.
    fn screen_for_tests(&self) -> Option<&str> {
        #[cfg(any(test, feature = "test-support"))]
        return self.display.as_deref();
        #[cfg(not(any(test, feature = "test-support")))]
        None
    }

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

    /// The same settings with what a pack's settings group says about the
    /// browser: under [`DIGESTS_SETTING`], the digests a person has
    /// reviewed. A group that does not hold the key changes nothing.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when the value is not a list of digests. A
    /// pin that cannot be read is never taken as no pin.
    pub fn with_pack_settings(mut self, settings: &Values) -> Result<Self, Failure> {
        let Some(value) = settings.get(DIGESTS_SETTING) else {
            return Ok(self);
        };
        let wrong = || {
            input(
                a2m_core::FailureCode::SCHEMA_INVALID,
                format!("The setting `{DIGESTS_SETTING}` is not a list of digests."),
                "Write each digest as 64 lowercase hexadecimal characters, in a list.",
            )
        };
        let mut digests = Vec::new();
        for one in value.as_list().ok_or_else(wrong)? {
            let text = one.as_text().ok_or_else(wrong)?;
            digests.push(text.parse::<Digest>().map_err(|_| wrong())?);
        }
        self.known_digests = digests;
        Ok(self)
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
    /// A window a person sees, for a sign-in the person carries out
    /// themself (decision 0254). The browser is always started afresh for
    /// it, in the same engine-owned data directory and in-memory context as
    /// any other, and a later `Background` open of the profile closes it.
    /// Its first tab shows `notice`, the engine's own words naming the
    /// target and the person; the browser's own first window is closed.
    Window {
        /// What the first tab says, as plain text.
        notice: String,
    },
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
    /// How many of its tabs hung and were closed since it started.
    pub hangs: u64,
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
    /// How many of its tabs hung and were closed: past the count the
    /// settings give, the browser is recycled between runs.
    pub(crate) hangs: AtomicU64,
    /// The open tabs, by the browser's name for each, so that what the
    /// browser itself refuses can be reported by the tab it concerns.
    pages: Mutex<BTreeMap<String, Weak<Page>>>,
    held: Mutex<Held>,
    /// The windows pages have opened that are still there, by session.
    windows: Mutex<BTreeMap<String, Window>>,
    /// The browser context every tab is opened in: one of the browser's
    /// own making, which keeps cookies and storage in memory and writes
    /// none of them to the data directory (decision 0245).
    pub(crate) context: String,
    /// The mark of the session state last placed into the context, so that
    /// the same state is placed once and a newer cookie the application set
    /// since is not overwritten by an older copy.
    state_mark: Mutex<Option<String>>,
    /// Whether the browser shows a window.
    window: bool,
}

/// A window a page opened. It is kept locked, and closed once the first
/// page it asks for has been refused, or when the tab that opened it goes.
pub(crate) struct Window {
    /// The browser's name for what opened it.
    pub(crate) opener: String,
    /// The browser's name for the window itself.
    pub(crate) target: String,
    /// Whether it has asked for a page yet.
    pub(crate) asked: bool,
}

/// The new pages the browser holds before they run. Only the session the
/// browser attached a page on can let it run (measured, decision 0143),
/// and the browser's word of that session and its answer to the engine's
/// request for the page come in either order. So each side leaves a note
/// for the other: a page nobody made here is never let run.
#[derive(Default)]
struct Held {
    /// Pages the browser holds, with the session each is held on.
    waiting: BTreeMap<String, String>,
    /// Pages the engine made, and has not yet been told are held.
    made: BTreeSet<String>,
}

impl BrowserInner {
    pub(crate) fn pages(&self) -> MutexGuard<'_, BTreeMap<String, Weak<Page>>> {
        self.pages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn windows(&self) -> MutexGuard<'_, BTreeMap<String, Window>> {
        self.windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The windows a tab opened that are still there, by the browser's
    /// name for each, for the tab to close as it goes.
    pub(crate) fn windows_of(&self, opener: &str) -> Vec<String> {
        self.windows()
            .values()
            .filter(|window| window.opener == opener)
            .map(|window| window.target.clone())
            .collect()
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The engine made this page. The session to let it run on, when the
    /// browser has already said it holds the page.
    pub(crate) fn made(&self, target: &str) -> Option<String> {
        let mut held = self.held();
        let session = held.waiting.remove(target);
        if session.is_none() {
            held.made.insert(target.to_owned());
        }
        session
    }

    /// The browser holds this page on this session. Whether the engine
    /// made it, so that it may be let run now.
    pub(crate) fn holds(&self, target: &str, session: &str) -> bool {
        let mut held = self.held();
        let ours = held.made.remove(target);
        if !ours {
            held.waiting.insert(target.to_owned(), session.to_owned());
        }
        ours
    }

    /// Has the tab that opened something report it. When what opened it
    /// is not a tab the engine knows, such as a frame inside one, every
    /// tab reports it: one report too many stops a run, one too few lets
    /// it go on.
    pub(crate) fn report(&self, opener: &str, word: &str, address: &str) {
        let pages = self.pages();
        match pages.get(opener).and_then(Weak::upgrade) {
            Some(page) => page.block(word, address),
            None => {
                for page in pages.values().filter_map(Weak::upgrade) {
                    page.block(word, address);
                }
            }
        }
    }

    /// Whether the browser is due to be recycled: its tabs hung as often
    /// as the settings allow, and no tab of it is held, so that no run is
    /// using it.
    fn due_for_recycling(&self, settings: &BrowserSettings) -> bool {
        let limit = u64::from(settings.hangs_before_recycle.max(1));
        self.hangs.load(Ordering::SeqCst) >= limit && self.tabs.load(Ordering::SeqCst) == 0
    }

    /// The memory the browser holds, in bytes: the sum of what its
    /// processes hold, by the browser's own list of them, each measured by
    /// its operating system. A browser that does not answer within `wait`,
    /// or names no process, or a process that cannot be measured, is an
    /// error, and the caller fails closed on it (decision 0425).
    pub(crate) async fn memory(&self, wait: Duration) -> Result<u64, String> {
        Ok(self
            .processes(wait)
            .await?
            .iter()
            .filter_map(|one| one.bytes)
            .fold(0_u64, u64::saturating_add))
    }

    /// Each process the browser names, with what it holds; `bytes` is
    /// `None` for a process that ended between the list and its
    /// measurement. [`Self::memory`] is their sum; the memory tests print
    /// each.
    pub(crate) async fn processes(&self, wait: Duration) -> Result<Vec<HeldBy>, String> {
        let listed = self
            .client
            .call(None, &GetProcessInfo {}, Deadline::after(wait))
            .await
            .map_err(|_| "it did not say which processes are its own".to_owned())?;
        if listed.process_info.is_empty() {
            return Err("it named no process of its own".to_owned());
        }
        // The system's tables are read off the threads that serve
        // everything else.
        tokio::task::spawn_blocking(move || {
            listed
                .process_info
                .into_iter()
                .map(|one| {
                    Ok(HeldBy {
                        bytes: crate::memory::of_process(one.id)?,
                        id: one.id,
                        kind: one.kind,
                    })
                })
                .collect()
        })
        .await
        .map_err(|_| "its measurement was stopped".to_owned())?
    }

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

/// One process of a browser, by the browser's own list, and what its
/// operating system says it holds.
#[derive(Debug)]
pub(crate) struct HeldBy {
    /// The kind the browser gives it, such as `browser` or `renderer`.
    #[cfg_attr(not(test), allow(dead_code, reason = "only the memory tests print it"))]
    pub(crate) kind: String,
    #[cfg_attr(not(test), allow(dead_code, reason = "only the memory tests print it"))]
    pub(crate) id: u32,
    /// In bytes; `None` when the process ended before it was measured.
    pub(crate) bytes: Option<u64>,
}

pub(crate) struct Shared {
    supervisor: ScopedSupervisor,
    root: Result<CheckedPath, Failure>,
    pub(crate) redact: Arc<dyn Redact>,
    pub(crate) settings: BrowserSettings,
    open: Mutex<BTreeMap<ProfileKey, Arc<BrowserInner>>>,
    /// One launch at a time, so that a profile is never started twice.
    launching: tokio::sync::Mutex<()>,
    /// Browsers whose launch was dropped after their program started,
    /// before they were recorded in `open`, each with the stop begun for
    /// it where a runtime ran. The next launch and the shutdown finish
    /// these stops first.
    abandoned: Mutex<Vec<(SubsystemId, Option<tokio::task::JoinHandle<Cleanup>>)>>,
    pub(crate) next_tab: AtomicU64,
    /// [`BrowserSettings::memory_ceiling`], which this crate's own tests
    /// move while a browser runs.
    memory_ceiling: AtomicU64,
}

/// What each running browser holds, by profile.
type Measured = Vec<(ProfileKey, Result<u64, String>)>;

impl Shared {
    fn open(&self) -> MutexGuard<'_, BTreeMap<ProfileKey, Arc<BrowserInner>>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Ends a browser that is no longer recorded among the open ones: asks
    /// it to close, so that it writes its session to its data directory,
    /// waits for its end up to the close deadline, then has the supervisor
    /// end and reap its whole process group.
    async fn end(&self, browser: &BrowserInner) -> Cleanup {
        if browser.client.closed().is_none() {
            let deadline = Deadline::after(self.settings.close_deadline);
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
        self.supervisor.stop(browser.subsystem).await
    }

    /// What each open browser that still runs holds. One that has ended
    /// holds nothing and is not asked; it is started afresh at its next
    /// open.
    async fn measure(&self) -> Measured {
        let running: Vec<Arc<BrowserInner>> = self
            .open()
            .values()
            .filter(|browser| browser.health() != BrowserHealth::Exited)
            .cloned()
            .collect();
        let mut measured = Vec::with_capacity(running.len());
        for browser in running {
            let held = browser.memory(self.settings.hang_probe).await;
            measured.push((browser.profile.clone(), held));
        }
        measured
    }

    /// Refuses new browser work while the browsers measured hold the
    /// ceiling or more, or one of them could not be measured: `busy`,
    /// `memory_short`, with the retry time (decision 0425).
    fn room(&self, measured: &Measured) -> Result<(), BrowserError> {
        let ceiling = self.memory_ceiling.load(Ordering::SeqCst);
        let retry = self.settings.memory_retry;
        let mut held = 0_u64;
        for (_, one) in measured {
            match one {
                Ok(bytes) => held = held.saturating_add(*bytes),
                Err(why) => {
                    return Err(BrowserError::LaunchFailed(LaunchReason::MemoryShort(
                        MemoryShort {
                            held: None,
                            ceiling,
                            retry,
                            why: Some(shown(self.redact.as_ref(), why)),
                        },
                    )));
                }
            }
        }
        if held >= ceiling {
            return Err(BrowserError::LaunchFailed(LaunchReason::MemoryShort(
                MemoryShort {
                    held: Some(held),
                    ceiling,
                    retry,
                    why: None,
                },
            )));
        }
        Ok(())
    }

    /// Finishes the stop of every browser whose launch was dropped, and
    /// says what could not be cleaned up.
    async fn stop_abandoned(&self) -> Vec<Leftover> {
        let abandoned = std::mem::take(
            &mut *self
                .abandoned
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let mut remaining = Vec::new();
        for (subsystem, stopping) in abandoned {
            let cleanup = match stopping {
                Some(stopping) => match stopping.await {
                    Ok(cleanup) => cleanup,
                    // The stop did not finish: it is asked again, and a
                    // subsystem already stopped is a complete cleanup.
                    Err(_) => self.supervisor.stop(subsystem).await,
                },
                None => self.supervisor.stop(subsystem).await,
            };
            remaining.extend(cleanup.remaining);
        }
        remaining
    }
}

/// A browser whose program started and that is not yet recorded among the
/// open ones. The engine drops a driver's preparation that passes its own
/// limit, and a browser's start may take longer than that; a launch
/// dropped there must leave no subsystem holding the profile's place.
/// Dropped while armed, it begins the stop at once where a runtime runs,
/// and leaves the subsystem for the next launch to finish stopping.
struct Unrecorded<'a> {
    shared: &'a Shared,
    subsystem: Option<SubsystemId>,
}

impl Unrecorded<'_> {
    /// The browser is recorded among the open ones: nothing to stop.
    fn recorded(mut self) {
        self.subsystem = None;
    }

    /// Stops the browser now, on a launch that failed.
    async fn stop(mut self) {
        if let Some(subsystem) = self.subsystem.take() {
            self.shared.supervisor.stop(subsystem).await;
        }
    }
}

impl Drop for Unrecorded<'_> {
    fn drop(&mut self) {
        let Some(subsystem) = self.subsystem.take() else {
            return;
        };
        let stopping = tokio::runtime::Handle::try_current().ok().map(|runtime| {
            let supervisor = self.shared.supervisor.clone();
            runtime.spawn(async move { supervisor.stop(subsystem).await })
        });
        self.shared
            .abandoned
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((subsystem, stopping));
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
        ClientError::Protocol { code, message } => format!(
            "it refused the first request over its pipe with the code {code}: {}",
            shown(redact, message)
        ),
        ClientError::Shape { method } => {
            format!("its answer to {method} over its pipe has another shape")
        }
    }
}

/// Puts on a browser the three locks that are the browser's and not a
/// tab's.
///
/// Every download is denied: the browser writes no file for one, in any
/// tab, so nothing lands outside the roots the engine was given. Every
/// permission a page asks for is refused, for every origin, so that no
/// request waits on a prompt nobody answers and none is granted by one.
/// And every new page is attached before it runs, so that a window a page
/// opens is caught before its first request ([`watch_windows`]).
async fn lock_browser(inner: &Arc<BrowserInner>, call_deadline: Duration) -> Result<(), String> {
    let client = &inner.client;
    let deadline = Deadline::after(call_deadline);
    // Listening starts before the browser is told to attach anything.
    let events = client.subscribe(None);
    tokio::spawn(watch_windows(Arc::downgrade(inner), events, call_deadline));
    // Both refusals hold for the browser's default context and for the
    // context every tab is opened in.
    for context in [None, Some(inner.context.as_str())] {
        client
            .call(
                None,
                &SetDownloadBehavior {
                    behavior: "deny",
                    browser_context_id: context,
                    download_path: None,
                    events_enabled: None,
                },
                deadline,
            )
            .await
            .map_err(|_| "it did not take the refusal of downloads".to_owned())?;
        client
            .call(
                None,
                &GrantPermissions {
                    permissions: Vec::new(),
                    browser_context_id: context,
                },
                deadline,
            )
            .await
            .map_err(|_| "it did not take the refusal of permissions".to_owned())?;
    }
    client
        .call(
            None,
            &SetAutoAttach {
                auto_attach: true,
                wait_for_debugger_on_start: true,
                flatten: true,
                // Pages only. The browser's own parts are not the engine's
                // to hold.
                filter: Some(vec![
                    TargetFilter {
                        kind: Some("page"),
                        exclude: false,
                    },
                    TargetFilter {
                        kind: None,
                        exclude: true,
                    },
                ]),
            },
            deadline,
        )
        .await
        .map_err(|_| "it did not take the watch on new windows".to_owned())?;
    Ok(())
}

/// The first tab of a window: a page the engine writes, which names what
/// the window is for, held to no origin, so that nothing sends it anywhere.
/// Once it is there, the browser's own first window, which is in the
/// browser's default context and is no tab of the engine's, is closed.
async fn show_notice(
    shared: &Arc<Shared>,
    inner: &Arc<BrowserInner>,
    text: &str,
    purpose: NoticeFor,
) -> Result<ActTab, BrowserError> {
    let page = open_page(
        shared,
        inner,
        &PageEnvironment::default(),
        Posture::Act,
        &Origins::default(),
        None,
    )
    .await?;
    let tab = ActTab::new(page);
    let deadline = Deadline::after(shared.settings.call_deadline);
    let html = notice_html(text, purpose);
    tab.page
        .call(
            &SetDocumentContent {
                frame_id: &tab.page.target,
                html: &html,
            },
            deadline,
        )
        .await?;
    let lost = |_| BrowserError::Disconnected {
        dispatch: Dispatch::Unknown,
    };
    let targets = inner
        .client
        .call(None, &GetTargets {}, deadline)
        .await
        .map_err(lost)?;
    for target in targets.target_infos {
        let foreign = target.kind == "page"
            && target.browser_context_id.as_deref() != Some(inner.context.as_str());
        if foreign {
            inner
                .client
                .call(
                    None,
                    &CloseTarget {
                        target_id: &target.target_id,
                    },
                    deadline,
                )
                .await
                .map_err(lost)?;
        }
    }
    Ok(tab)
}

/// What a window is for, and so what its notice says under its heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeFor {
    /// A sign-in the person carries out (decision 0254).
    SignIn,
    /// A use of the system the engine watches (decision 0392, point 1).
    Watch,
}

/// What a window's notice says under its heading, for each purpose.
const SIGN_IN_NOTICE: &str = "The next tab is the sign-in page. The engine types nothing there and reads nothing you type. Close the window to stop.";
const WATCH_NOTICE: &str = "The next tab is the system, opened once on its own address. Use it as you always do. The engine presses and types nothing there, sends it nowhere, and sends nothing of its own. It keeps the shape of each request the page sends and of each screen you visit: names and kinds, never a value you type or the page shows. Close the window to stop.";

/// The notice, as a page: plain text, escaped, with no script.
fn notice_html(text: &str, purpose: NoticeFor) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{escaped}</title></head><body><main><h1>{escaped}</h1><p>{said}</p></main></body></html>",
        said = match purpose {
            NoticeFor::SignIn => SIGN_IN_NOTICE,
            NoticeFor::Watch => WATCH_NOTICE,
        }
    )
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
                open: Mutex::new(BTreeMap::new()),
                launching: tokio::sync::Mutex::new(()),
                abandoned: Mutex::new(Vec::new()),
                next_tab: AtomicU64::new(1),
                memory_ceiling: AtomicU64::new(settings.memory_ceiling),
                settings,
            }),
        })
    }

    /// The folder files for upload are staged in, as a checked path: the
    /// [`STAGING_FOLDER`] of the profile root. A file the pack gives a file
    /// input is joined to it, so that no name reaches another folder.
    ///
    /// # Errors
    ///
    /// The profile root is not declared, or the folder is not inside it.
    pub fn staging_folder(&self) -> Result<a2m_provider::CheckedPath, Failure> {
        self.shared
            .root
            .clone()
            .and_then(|root| root.join(STAGING_FOLDER))
    }

    /// The folder a declared download is written into, as a checked path:
    /// the [`DOWNLOADS_FOLDER`] of the profile root, made when it is not
    /// there.
    ///
    /// # Errors
    ///
    /// The profile root is not declared, or the folder cannot be made.
    pub fn downloads_folder(&self) -> Result<a2m_provider::CheckedPath, Failure> {
        let folder = self
            .shared
            .root
            .clone()
            .and_then(|root| root.join(DOWNLOADS_FOLDER))?;
        folder.create_dir_all()?;
        Ok(folder)
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
    /// process has ended is stopped, cleaned up, and started afresh; so is
    /// one whose tabs hung [`BrowserSettings::hangs_before_recycle`] times,
    /// once no tab of it is held: it is closed as [`Self::close`] closes
    /// it, between two runs and never inside one.
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
        self.open_for(profile, show, NoticeFor::SignIn).await
    }

    /// As [`Self::open`], with a window's notice written for `purpose`.
    pub(crate) async fn open_for(
        &self,
        profile: &ProfileKey,
        show: Visibility,
        purpose: NoticeFor,
    ) -> Result<Browser, BrowserError> {
        let shared = &self.shared;
        let _one = shared.launching.lock().await;
        // A launch dropped earlier may have left a browser holding the
        // place; it is gone before anything starts. What could not be
        // ended is the supervisor's leftover, and it refuses the start.
        shared.stop_abandoned().await;
        let existing = shared.open().get(profile).cloned();
        let notice = match &show {
            Visibility::Background => None,
            Visibility::Window { notice } => Some(notice.clone()),
        };
        let window = notice.is_some();
        // An open that finds a tab of its profile's browser held is a run's
        // work going on, and is let go on; every other open begins browser
        // work, and memory is measured for it (decision 0425).
        let begins = window
            || existing
                .as_ref()
                .is_none_or(|existing| existing.tabs.load(Ordering::SeqCst) == 0);
        let mut measured = if begins {
            shared.measure().await
        } else {
            Vec::new()
        };
        if let Some(existing) = existing {
            // A browser in the background is used again; a window never is,
            // and one is never used for a run in the background. One past
            // its memory limit, or that cannot be measured, is recycled, as
            // one is after its hangs: only when no tab of it is held.
            let exited = existing.health() == BrowserHealth::Exited;
            let due = existing.due_for_recycling(&shared.settings);
            let heavy = begins
                && measured
                    .iter()
                    .find(|(measured, _)| measured == profile)
                    .is_some_and(|(_, held)| {
                        held.as_ref()
                            .map_or(true, |bytes| *bytes >= shared.settings.memory_limit)
                    });
            if !exited && !due && !heavy && !window && !existing.window {
                if begins {
                    shared.room(&measured)?;
                }
                return Ok(Browser {
                    inner: existing,
                    shared: Arc::clone(shared),
                    _notice: None,
                });
            }
            // What it holds goes with it.
            measured.retain(|(measured, _)| measured != profile);
            shared.open().remove(profile);
            if exited {
                existing.client.close();
                shared.supervisor.stop(existing.subsystem).await;
            } else {
                // A browser that still answers is asked to close, so that it
                // writes what it keeps to its data directory first.
                shared.end(&existing).await;
            }
        }
        // Nothing is started while memory runs short.
        shared.room(&measured)?;
        let failed = |reason| BrowserError::LaunchFailed(reason);
        let screen = if window {
            Some(
                launch::screen(shared.settings.screen_for_tests())
                    .ok_or(failed(LaunchReason::NoScreen))?,
            )
        } else {
            None
        };

        let place = profile.place();
        // Looking for the program and making the folder touch the disk:
        // off the threads that serve everything else.
        let named = shared.settings.program.clone();
        let root = shared.root.clone();
        let under = place.clone();
        let found = tokio::task::spawn_blocking(move || {
            let program = launch::find(named.as_deref()).ok_or(LaunchReason::NotInstalled)?;
            let directory = root
                .and_then(|root| root.join(&under))
                .and_then(|directory| directory.create_dir_all().map(|()| directory))
                // The folder holds the session's cookies under a key that
                // is no secret, so the folder itself is what keeps them.
                .and_then(|directory| launch::make_private(&directory).map(|()| directory))
                .map_err(LaunchReason::DataDirectory)?;
            // What an earlier start wrote is not this start's. A file that
            // cannot be removed is written on; what is shown of it is still
            // what the browser wrote.
            let log = directory
                .join(launch::LOG_FILE)
                .map_err(LaunchReason::DataDirectory)?;
            if log.exists() {
                let _ = log.remove_file();
            }
            Ok::<_, LaunchReason>((program, (directory, log)))
        })
        .await;
        let (program, (directory, log)) = match found {
            Ok(found) => found.map_err(failed)?,
            Err(_) => {
                return Err(failed(LaunchReason::DataDirectory(Failure::new(
                    FailureClass::Transient,
                    codes::CONNECTION_LOST,
                    "The engine stopped while the browser's data directory was being made.",
                    "Try again.",
                ))));
            }
        };
        #[allow(unused_mut, reason = "only a test build adds an argument")]
        let mut args = launch::arguments(directory.as_path(), log.as_path(), window);
        #[cfg(any(test, feature = "test-support"))]
        if shared.settings.without_sandbox {
            args.push(crate::test_support::WITHOUT_SANDBOX.into());
        }
        #[allow(unused_mut, reason = "only this crate's tests add a variable")]
        let mut env = launch::environment();
        if let Some(screen) = screen {
            env.pass.extend(screen.pass);
            env.set.extend(screen.set);
        }
        #[cfg(test)]
        env.set
            .extend(shared.settings.environment_for_tests.iter().cloned());
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
                    env,
                    cwd: None,
                    kind,
                    place: Some(place),
                },
                launch::pipe(),
            )
            .await
            .map_err(|failure| failed(LaunchReason::Supervisor(failure)))?;
        let subsystem = link.id;
        // From here until it is recorded, a dropped launch stops it.
        let unrecorded = Unrecorded {
            shared,
            subsystem: Some(subsystem),
        };
        // A program was started: one launch, counted under its profile
        // root, inside the section that lets one launch through at a time.
        #[cfg(any(test, feature = "test-support"))]
        if let Ok(root) = &shared.root {
            crate::test_support::launched(root.as_path());
        }
        #[cfg(test)]
        if let Some(stall) = &shared.settings.stall_after_start
            && !stall.done.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            stall.started.notify_one();
            std::future::pending::<()>().await;
        }

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
                    // can take longer than any deadline. A pipe that ended
                    // after the program wrote why in its log is a browser
                    // that could not start here, such as one that cannot
                    // make its sandbox (decision 0999). Everything else
                    // is a program that is not a browser.
                    Err(match error {
                        ClientError::Timeout { .. } => LaunchReason::NoAnswer(why),
                        ClientError::Closed {
                            why: Why::Ended, ..
                        } => {
                            let read = log.clone();
                            let redact = Arc::clone(&shared.redact);
                            match tokio::task::spawn_blocking(move || {
                                launch::what_it_wrote(&read, redact.as_ref())
                            })
                            .await
                            {
                                Ok(Some(wrote)) => LaunchReason::Stopped(wrote),
                                _ => LaunchReason::NotOwned(why),
                            }
                        }
                        _ => LaunchReason::NotOwned(why),
                    })
                }
            }
        };
        let product = match owned {
            Ok(product) => product,
            Err(reason) => {
                client.close();
                unrecorded.stop().await;
                return Err(failed(reason));
            }
        };
        // A browser that answered needs its log no more. Where the system
        // keeps an open file from being removed, the next start removes it.
        let _ = tokio::task::spawn_blocking(move || log.remove_file()).await;
        // Every tab is opened in a context of the browser's own making,
        // which keeps what pages store in memory: no cookie and no page
        // storage is written to the data directory (decision 0245).
        let context = match client
            .call(
                None,
                &CreateBrowserContext {},
                Deadline::after(shared.settings.call_deadline),
            )
            .await
        {
            Ok(made) => made.browser_context_id,
            Err(_) => {
                client.close();
                unrecorded.stop().await;
                return Err(failed(LaunchReason::NotOwned(
                    "it did not make a context that keeps its state in memory".to_owned(),
                )));
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
            hangs: AtomicU64::new(0),
            pages: Mutex::new(BTreeMap::new()),
            held: Mutex::new(Held::default()),
            windows: Mutex::new(BTreeMap::new()),
            context,
            state_mark: Mutex::new(None),
            window,
        });
        // The locks that hold for the whole browser go on before any tab
        // exists. A browser that does not take them is not one the engine
        // can own: it is closed.
        if let Err(why) = lock_browser(&inner, shared.settings.call_deadline).await {
            inner.client.close();
            unrecorded.stop().await;
            return Err(failed(LaunchReason::NotOwned(why)));
        }
        let notice = match notice {
            None => None,
            Some(text) => match show_notice(shared, &inner, &text, purpose).await {
                Ok(tab) => Some(Arc::new(tab)),
                Err(error) => {
                    inner.client.close();
                    unrecorded.stop().await;
                    return Err(error);
                }
            },
        };
        shared.open().insert(profile.clone(), Arc::clone(&inner));
        unrecorded.recorded();
        Ok(Browser {
            inner,
            shared: Arc::clone(shared),
            _notice: notice,
        })
    }

    /// This crate's own tests only: moves the memory ceiling while the
    /// browsers run, as a person's new setting would at the next start.
    #[cfg(test)]
    pub(crate) fn set_memory_ceiling(&self, bytes: u64) {
        self.shared.memory_ceiling.store(bytes, Ordering::SeqCst);
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
                hangs: browser.hangs.load(Ordering::SeqCst),
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
        shared.end(&browser).await
    }

    /// Closes every browser. What could not be cleaned up is listed.
    pub async fn shutdown(&self) -> Cleanup {
        let mut remaining = {
            let _one = self.shared.launching.lock().await;
            self.shared.stop_abandoned().await
        };
        let profiles: Vec<ProfileKey> = self.shared.open().keys().cloned().collect();
        for profile in profiles {
            remaining.extend(self.close(&profile).await.remaining);
        }
        Cleanup::incomplete(remaining)
    }
}

/// One browser process. Cheap to clone.
#[derive(Clone)]
pub struct Browser {
    pub(crate) inner: Arc<BrowserInner>,
    shared: Arc<Shared>,
    /// A window's first tab, which says what the window is for. Held, not
    /// used: it is closed when the last clone of this handle goes.
    _notice: Option<Arc<ActTab>>,
}

impl fmt::Debug for Browser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Browser")
            .field("subsystem", &self.inner.subsystem)
            .finish_non_exhaustive()
    }
}

impl Browser {
    /// A new tab that may act, with the fixed environment given, for the
    /// origins given: the destinations the compiled target declares. The tab
    /// goes to no other origin, whoever moves it, and looks at and acts on no
    /// page of another: each of those is [`BrowserError::ForeignOrigin`].
    ///
    /// # Errors
    ///
    /// [`BrowserError::Disconnected`] or [`BrowserError::Timeout`] when the
    /// browser is gone or does not answer.
    pub async fn acting_tab(
        &self,
        env: &PageEnvironment,
        origins: &Origins,
    ) -> Result<ActTab, BrowserError> {
        let page = open_page(&self.shared, &self.inner, env, Posture::Act, origins, None).await?;
        self.inner.runs.fetch_add(1, Ordering::SeqCst);
        Ok(ActTab::new(page))
    }

    /// A tab a person signs in in, in a window (decision 0254). It may be
    /// on the origins of `look_on`, the application's, and of
    /// `pass_through`, an identity provider's, and on no other: a move to
    /// another is ended in the browser and reported as `NAVIGATION`, as in
    /// an acting tab. Its helper looks at a page of `look_on` alone: on a
    /// page of `pass_through` every look is [`BrowserError::ForeignOrigin`]
    /// and reads nothing. It hears no network event, and the engine sends
    /// it no input: what the person types there crosses into the engine's
    /// process only in the pause of a document request the hold needs.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Disconnected`] or [`BrowserError::Timeout`] when the
    /// browser is gone or does not answer.
    pub async fn sign_in_tab(
        &self,
        env: &PageEnvironment,
        look_on: &Origins,
        pass_through: &Origins,
    ) -> Result<ActTab, BrowserError> {
        let both = look_on.clone().and(pass_through);
        let page = open_page(
            &self.shared,
            &self.inner,
            env,
            Posture::SignIn,
            &both,
            Some(look_on),
        )
        .await?;
        self.inner.runs.fetch_add(1, Ordering::SeqCst);
        Ok(ActTab::new(page))
    }

    /// Whether the browser shows a window.
    #[must_use]
    pub fn shows_window(&self) -> bool {
        self.inner.window
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

    /// What the browser holds for `origins`: every cookie of its context
    /// whose domain is the host of one of them, or a domain above it. This
    /// is a session's state after a sign-in through the application's own
    /// pages. Each value is a [`Secret`]; the caller registers it with the
    /// redactor before it goes anywhere else.
    ///
    /// Page storage is not read: a session an application keeps in a page's
    /// own storage is not exported (decision 0245).
    ///
    /// # Errors
    ///
    /// [`BrowserError::Disconnected`] or [`BrowserError::Timeout`] when the
    /// browser is gone or does not answer.
    pub async fn export_state(
        &self,
        origins: &Origins,
        deadline: Deadline,
    ) -> Result<BrowserState, BrowserError> {
        let got = self
            .inner
            .client
            .call(
                None,
                &GetCookies {
                    browser_context_id: &self.inner.context,
                },
                deadline,
            )
            .await
            .map_err(|error| lost(&error, self.shared.redact.as_ref()))?;
        let hosts: Vec<String> = origins.iter().filter_map(host_of).collect();
        let cookies = got
            .cookies
            .into_iter()
            .filter(|cookie| hosts.iter().any(|host| domain_covers(&cookie.domain, host)))
            .map(|cookie| StateCookie {
                name: cookie.name,
                value: Secret::new(cookie.value),
                domain: cookie.domain,
                path: cookie.path,
                expires: if cookie.session { None } else { cookie.expires },
                http_only: cookie.http_only,
                secure: cookie.secure,
                same_site: cookie.same_site,
            })
            .collect();
        Ok(BrowserState { cookies })
    }

    /// Places a session's state into the browser's context, unless the
    /// state of the same `mark` was placed there already: a cookie the
    /// application set since is newer than the copy, and is kept. True when
    /// it was placed now.
    ///
    /// # Errors
    ///
    /// [`BrowserError::Disconnected`] or [`BrowserError::Timeout`] when the
    /// browser is gone or does not answer; [`BrowserError::Unexpected`] when
    /// it refuses a cookie. Nothing is marked as placed then.
    pub async fn import_state(
        &self,
        mark: &str,
        state: &BrowserState,
        origins: &Origins,
        deadline: Deadline,
    ) -> Result<bool, BrowserError> {
        if lock(&self.inner.state_mark).as_deref() == Some(mark) {
            return Ok(false);
        }
        // Every cookie is for a host of the run's origins, or a domain above
        // one: a state for another site is placed nowhere (audit finding A7).
        let hosts: Vec<String> = origins.iter().filter_map(host_of).collect();
        if let Some(outside) = state
            .cookies
            .iter()
            .find(|cookie| !hosts.iter().any(|host| domain_covers(&cookie.domain, host)))
        {
            return Err(BrowserError::ForeignOrigin(
                self.shared.redact.redact(&outside.domain),
            ));
        }
        let cookies = state
            .cookies
            .iter()
            .map(|cookie| CookieToSet {
                name: &cookie.name,
                value: cookie.value.expose(),
                domain: &cookie.domain,
                path: &cookie.path,
                expires: cookie.expires.as_ref(),
                http_only: cookie.http_only,
                secure: cookie.secure,
                same_site: cookie.same_site.as_deref(),
            })
            .collect();
        self.inner
            .client
            .call(
                None,
                &SetCookies {
                    cookies,
                    browser_context_id: &self.inner.context,
                },
                deadline,
            )
            .await
            .map_err(|error| lost(&error, self.shared.redact.as_ref()))?;
        *lock(&self.inner.state_mark) = Some(mark.to_owned());
        Ok(true)
    }
}

/// A session's state as a browser holds it: the cookies of an
/// application's origins. It cannot be printed with its values, and is not
/// serializable: the one way it is kept is sealed, by the session keeper,
/// through the provider that made it.
#[derive(Default)]
pub struct BrowserState {
    /// The cookies, in the order the browser gave them.
    pub cookies: Vec<StateCookie>,
}

impl fmt::Debug for BrowserState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.cookies.iter().map(|cookie| &cookie.name))
            .finish()
    }
}

/// One cookie of a session's state.
#[derive(Debug)]
pub struct StateCookie {
    /// Its name. Shown; never a credential.
    pub name: String,
    /// Its value: a credential.
    pub value: Secret,
    /// The domain the browser holds it for, with a leading dot when it is
    /// sent to the hosts below it too.
    pub domain: String,
    /// The path it is sent under.
    pub path: String,
    /// When it ends, in seconds since the epoch, as the browser wrote it;
    /// `None` for a cookie that ends with the browser. Carried, never
    /// compared.
    pub expires: Option<serde_json::Number>,
    /// Whether the page's own script is kept from reading it.
    pub http_only: bool,
    /// Whether it is sent over an encrypted connection alone.
    pub secure: bool,
    /// The browser's same-site rule, as it names it.
    pub same_site: Option<String>,
}

/// The host of an origin as `Origins` writes it: `https://host:port`.
fn host_of(origin: &str) -> Option<String> {
    let (_, rest) = origin.split_once("://")?;
    let host = match rest.strip_prefix('[') {
        Some(inner) => inner.split(']').next()?,
        None => rest.split(':').next()?,
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Whether a cookie held for `domain` is sent to `host`.
fn domain_covers(domain: &str, host: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    let bare = domain.strip_prefix('.').unwrap_or(&domain);
    bare == host || host.ends_with(&format!(".{bare}"))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lost(error: &ClientError, redact: &dyn Redact) -> BrowserError {
    match error {
        ClientError::Closed { dispatch, .. } => BrowserError::Disconnected {
            dispatch: *dispatch,
        },
        ClientError::Timeout { dispatch } => BrowserError::Timeout {
            dispatch: *dispatch,
        },
        ClientError::Protocol { message, .. } => {
            BrowserError::Unexpected(crate::tab::shown(redact, message))
        }
        ClientError::Shape { method } => {
            BrowserError::Unexpected(format!("the reply of {method} has another shape"))
        }
    }
}

/// A set of browsers that can only look: it opens observing tabs and
/// watching tabs, neither of which has a method that acts, and there is no
/// way from it to an acting tab.
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
    /// which is started when it is not running, for the origins given, as
    /// [`Browser::acting_tab`] takes them. Every request of the page,
    /// its frames, and its workers whose method is not GET, HEAD, or
    /// OPTIONS is failed before it leaves the browser, unless `allow`
    /// names it, and the page cannot move the tab; each refusal is
    /// reported by [`ObserveTab::blocked`].
    ///
    /// # Errors
    ///
    /// As [`Browsers::open`] and [`Browser::acting_tab`].
    pub async fn observing_tab(
        &self,
        profile: &ProfileKey,
        env: &PageEnvironment,
        allow: &SafeRequests,
        origins: &Origins,
    ) -> Result<ObserveTab, BrowserError> {
        let browser = self.browsers.open(profile, Visibility::Background).await?;
        let page = open_page(
            &browser.shared,
            &browser.inner,
            env,
            Posture::Observe(allow.clone()),
            origins,
            None,
        )
        .await?;
        Ok(ObserveTab::new(page))
    }

    /// A tab a person uses while the engine watches (decision 0392, point
    /// 1), in a window of the profile's browser, started afresh as for a
    /// sign-in, whose first tab shows `notice` and says what is kept. The
    /// tab is held to `origins` as a sign-in tab is, sent once to `start`,
    /// and sent nowhere after that. Nothing in it is failed for its method,
    /// and it records every request of its page to those origins and the
    /// screens the page shows, by structure, from the moment it opens. It
    /// has no method that presses, types, navigates, or sends.
    ///
    /// # Errors
    ///
    /// [`BrowserError::ForeignOrigin`] when `start` is not on `origins`;
    /// nothing was opened. As [`Browsers::open`] with a window, and as a
    /// `goto` of the first address.
    pub async fn watching_tab(
        &self,
        profile: &ProfileKey,
        env: &PageEnvironment,
        notice: &str,
        origins: &Origins,
        start: &WatchStart,
        deadline: Deadline,
    ) -> Result<WatchTab, BrowserError> {
        if !origins.allows(start.address()) {
            return Err(BrowserError::ForeignOrigin(start.address().origin()));
        }
        let browser = self
            .browsers
            .open_for(
                profile,
                Visibility::Window {
                    notice: notice.to_owned(),
                },
                NoticeFor::Watch,
            )
            .await?;
        let page = open_page(
            &browser.shared,
            &browser.inner,
            env,
            Posture::Watch,
            origins,
            None,
        )
        .await?;
        WatchTab::open(page, browser, start, deadline).await
    }

    /// Whether a browser program is installed. Starts nothing.
    #[must_use]
    pub fn availability(&self) -> Availability {
        self.browsers.availability()
    }
}

/// An address a tab may be sent to: an absolute `http` or `https` address
/// with no space or control character. Which origins a target declared is
/// for the crate above, which knows the target and gives them to the tab
/// as [`Origins`]; the tab holds itself to them.
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
