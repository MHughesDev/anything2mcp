//! The presence prompt: a prompt the operating system owns, which proves
//! that someone with the user's finger, face, PIN, or password answered at
//! that moment.
//!
//! The person surface asks through [`PresencePrompt`] and never decides
//! what an answer is worth beyond the table on [`PromptAnswer`]. What a
//! prompt can do is asked each time it matters, with
//! [`PresencePrompt::available`], because it can change while the engine
//! runs. The prompt of this machine is [`system_prompt`]: polkit on Linux,
//! LocalAuthentication on macOS, Windows Hello on Windows, and
//! [`NoPrompt`] on any other system.
//!
//! What a passed prompt does not prove is in spec section 222: it stops
//! an agent that acts through the engine or by synthetic input, not a
//! program that replaces the engine or changes its files.
//!
//! One prompt of an operating system is one file beside this one that
//! implements the trait, its `mod` line here, and its arm in
//! [`system_prompt`].

use std::sync::Arc;

use a2m_actions::BoxFuture;
use tokio::time::Instant;

// The macOS and Windows prompts are compiled into the tests on every
// system, so that their mapping is tested everywhere; their calls into the
// system are compiled on their own system only.
pub(crate) mod field;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod local_authentication;
#[cfg(target_os = "linux")]
pub(crate) mod polkit;
#[cfg(any(windows, test))]
pub(crate) mod windows_hello;

/// Which kind of presence check a machine has. `doctor.run` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceKind {
    /// A prompt the operating system owns: a fingerprint, a face, a PIN,
    /// or a password.
    OsPrompt,
    /// A native confirmation dialog alone. It stops mistakes; it is not a
    /// security boundary.
    DialogOnly,
    /// Nothing that can be raised.
    NotAvailable,
}

impl PresenceKind {
    /// The kind as a report writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OsPrompt => "os_prompt",
            Self::DialogOnly => "dialog_only",
            Self::NotAvailable => "not_available",
        }
    }
}

/// Which prompt of an operating system answered. The record of an
/// approval names it, so that an audit can tell a stronger check from a
/// weaker one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptName {
    /// polkit on a Linux desktop: the user's password, or what PAM is set
    /// up to take, for an action an administrator installed. Its dialog
    /// cannot say which change it is for.
    Polkit,
    /// The system's own authentication on macOS: Touch ID, a paired
    /// watch, or the account password.
    LocalAuthentication,
    /// Windows Hello: a face, a fingerprint, or the Hello PIN.
    WindowsHello,
    /// The field harness's prompt: a line read from a pipe only a test
    /// runner holds. The shipped program never constructs a prompt that
    /// answers with it (decision 0310); a record that names it shows that
    /// a field test, not a person at an operating system's prompt, passed.
    FieldHarness,
}

impl PromptName {
    /// The name as a record writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Polkit => "polkit",
            Self::LocalAuthentication => "local_authentication",
            Self::WindowsHello => "windows_hello",
            Self::FieldHarness => "field_harness",
        }
    }
}

/// Why a machine has the kind of presence check it has, as one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceReason {
    /// The prompt can be raised.
    Ready,
    /// This build has no prompt for this operating system.
    NotBuilt,
    /// No system bus answers: a container, or a machine with no desktop.
    NoSystemBus,
    /// The system bus has no polkit on it.
    NoPolkit,
    /// The policy file that declares the engine's action is not installed.
    PolicyNotInstalled,
    /// The engine does not run inside a local, active session of a
    /// person: a remote shell, a container, a machine with no screen, or
    /// an engine started outside the session.
    SessionNotActive,
    /// Nothing in the session shows the prompt: no authentication agent.
    NoAgent,
    /// The operating system would answer yes without asking anybody, so
    /// its answer proves nobody.
    GrantedWithoutAsking,
    /// The engine runs where nothing it shows reaches a screen: as a
    /// service, or in a window station that is not the interactive one.
    NoWindowStation,
    /// The engine runs in a remote desktop session, so a prompt would be
    /// answered somewhere other than at this machine.
    SessionRemote,
    /// The screen of this machine is locked.
    SessionLocked,
    /// Nothing the system could check is set up for this user: no
    /// fingerprint, face, watch, or PIN, and no password it will ask for.
    NotSetUp,
    /// A policy of this machine turns the prompt off.
    DisabledByPolicy,
    /// The device that checks is busy with another check.
    DeviceBusy,
    /// Too many checks failed, and the system has locked them for now.
    LockedOut,
    /// The check itself failed, in a way that has no name of its own.
    CheckFailed,
}

impl PresenceReason {
    /// The reason as a report writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NotBuilt => "not_built",
            Self::NoSystemBus => "no_system_bus",
            Self::NoPolkit => "no_polkit",
            Self::PolicyNotInstalled => "policy_not_installed",
            Self::SessionNotActive => "session_not_active",
            Self::NoAgent => "no_agent",
            Self::GrantedWithoutAsking => "granted_without_asking",
            Self::NoWindowStation => "no_window_station",
            Self::SessionRemote => "session_remote",
            Self::SessionLocked => "session_locked",
            Self::NotSetUp => "not_set_up",
            Self::DisabledByPolicy => "disabled_by_policy",
            Self::DeviceBusy => "device_busy",
            Self::LockedOut => "locked_out",
            Self::CheckFailed => "check_failed",
        }
    }
}

/// What a prompt can do on this machine at this moment: the kind, the
/// reason, and what a person can do about it. `doctor.run` and the
/// approval page show it. Every text is the engine's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    /// The kind of check that can be raised now.
    pub kind: PresenceKind,
    /// The operating system's prompt behind it, when there is one.
    pub prompt: Option<PromptName>,
    /// Why, as one word.
    pub reason: PresenceReason,
    /// The same, as a sentence.
    pub message: String,
    /// What a person or an administrator does about it. Empty when there
    /// is nothing to do.
    pub remedy: String,
}

impl Availability {
    /// The operating system's prompt `prompt` can be raised. `message`
    /// says what it will ask for.
    #[must_use]
    pub fn ready(prompt: PromptName, message: impl Into<String>) -> Self {
        Self {
            kind: PresenceKind::OsPrompt,
            prompt: Some(prompt),
            reason: PresenceReason::Ready,
            message: message.into(),
            remedy: String::new(),
        }
    }

    /// Nothing can be raised, for `reason`.
    #[must_use]
    pub fn not_available(
        reason: PresenceReason,
        message: impl Into<String>,
        remedy: impl Into<String>,
    ) -> Self {
        Self {
            kind: PresenceKind::NotAvailable,
            prompt: None,
            reason,
            message: message.into(),
            remedy: remedy.into(),
        }
    }

    /// Whether a prompt that proves a person can be raised.
    #[must_use]
    pub fn proves_presence(&self) -> bool {
        self.kind == PresenceKind::OsPrompt
    }
}

/// What a prompt shows, where the operating system lets it show words:
/// words the engine made, never a caller's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptText {
    /// What is asked, in a line: the action, or the tool whose write
    /// waits. A tool's name begins with its target.
    pub title: String,
    /// What it is about. For an approval, the first twelve characters of
    /// the hash of the change, which the page shows beside Approve.
    pub detail: String,
    /// What the prompt is for. A prompt that answers only some kinds of
    /// ask reads it.
    pub purpose: PromptFor,
}

/// What a prompt is raised for: an approval of one write, or one of a
/// person's own acts (a sign-in, a publish, a confirmation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFor {
    /// A person's act: a sign-in on the engine's own page, a publish, or a
    /// confirmation.
    Act,
    /// An approval of a write, with the effect class of the operation it
    /// would carry out. `None` where the asker does not know it; a prompt
    /// that must refuse an `irreversible` operation treats `None` as
    /// `irreversible` (rule 3).
    Approval(Option<a2m_core::EffectClass>),
}

/// How a prompt ended.
///
/// | Answer | An approval | A protected setting |
/// | --- | --- | --- |
/// | `Present` | recorded, with the prompt that answered | may be loosened |
/// | `Confirmed` | recorded, with the check `not_available` | refused |
/// | `Dismissed` | not recorded | refused |
/// | `Denied` | not recorded | refused |
/// | `NotAvailable` | not recorded | refused |
///
/// Only `Present` proves a person. An error of the operating system is
/// one of the last three, never `Present`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAnswer {
    /// The operating system's own prompt was passed, and this is which.
    Present(PromptName),
    /// The person pressed yes in a native dialog. Nothing proves a person.
    Confirmed,
    /// The person closed the prompt, or the deadline came first and the
    /// prompt was cancelled.
    Dismissed,
    /// The prompt was answered and not passed: a wrong password, a finger
    /// the system does not know.
    Denied,
    /// No prompt could be raised, and why.
    NotAvailable(Availability),
}

/// A way to ask whether a person is at the machine.
///
/// The person surface raises one prompt at a time: it never calls
/// [`ask`](Self::ask) while an earlier ask of its own has not answered.
pub trait PresencePrompt: Send + Sync {
    /// What this prompt can do on this machine now. It raises nothing and
    /// shows nothing. It is asked each time it is needed, because what it
    /// reads can change while the engine runs.
    fn available(&self) -> BoxFuture<'_, Availability>;

    /// Raises the prompt and waits for its end. At `deadline`, the end of
    /// the approval window, the prompt is cancelled and the answer is
    /// [`PromptAnswer::Dismissed`], whatever arrives afterwards.
    fn ask(&self, text: PromptText, deadline: Instant) -> BoxFuture<'_, PromptAnswer>;
}

/// The prompt of a machine with none: every ask ends `NotAvailable`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPrompt;

impl NoPrompt {
    fn none() -> Availability {
        Availability::not_available(
            PresenceReason::NotBuilt,
            "This build has no presence prompt for this operating system.",
            "Nothing on this machine can approve a write or loosen a protected setting. Decline the write, or use a machine whose operating system has a presence prompt this build can raise.",
        )
    }
}

impl PresencePrompt for NoPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Self::none() })
    }

    fn ask(&self, _text: PromptText, _deadline: Instant) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async { PromptAnswer::NotAvailable(Self::none()) })
    }
}

/// The prompt this build has for this machine's operating system: polkit
/// on Linux, LocalAuthentication on macOS, Windows Hello on Windows, and
/// [`NoPrompt`] on any other. Whether it can be raised on this machine is
/// [`PresencePrompt::available`]'s to say.
#[must_use]
pub fn system_prompt() -> Arc<dyn PresencePrompt> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(polkit::PolkitPrompt::system())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(local_authentication::LocalAuthenticationPrompt::system())
    }
    #[cfg(windows)]
    {
        Arc::new(windows_hello::WindowsHelloPrompt::system())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Arc::new(NoPrompt)
    }
}

/// The prompt this build has for this machine, for the engine whose
/// directories are `paths`: as [`system_prompt`], and on Linux, when polkit
/// does not know the engine's action, its availability writes a copy of
/// the policy file into the folder `polkit` of the data directory, mode
/// 0644, and its remedy names that copy and the one command a person with
/// administrator rights runs to install it. The engine never runs the
/// command, asks for no elevation, and writes nothing outside its own
/// data directory (decision 0220).
#[must_use]
pub fn system_prompt_for(paths: &a2m_actions::Paths) -> Arc<dyn PresencePrompt> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(polkit::PolkitPrompt::system().copying_into(paths.data.join(polkit::COPY_FOLDER)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = paths;
        system_prompt()
    }
}

/// The kind of presence check this machine has now, and why: what
/// [`system_prompt`] answers when asked. `doctor.run` reports it.
pub async fn presence_available() -> Availability {
    system_prompt().available().await
}
