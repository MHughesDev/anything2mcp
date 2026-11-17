//! The presence prompt of macOS: the system's own authentication.
//!
//! The engine asks LocalAuthentication to evaluate the policy of the
//! device's owner: Touch ID or a paired watch first, and the account's
//! password where neither can be used. The system draws the dialog and
//! checks the answer; the engine gets a yes or a no.
//!
//! Three things keep a yes worth something:
//!
//! - **Before anybody is asked, the window server and the policy are
//!   asked.** A process in no graphical session, in a session another
//!   user's has replaced at the screen, or at a locked screen raises
//!   nothing; nor does one whose policy cannot be evaluated. Each is a
//!   named `not_available`.
//! - **An error is never a yes.** Every answer the system can give maps
//!   to a named outcome that records nothing: [`placed`], [`unavailable`],
//!   and [`answered`] are total, and their tests walk every case. A code
//!   this build does not know is `check_failed`.
//! - **The deadline takes the prompt away**: the context is invalidated
//!   when the ask's future is dropped, at the deadline or for any other
//!   reason. No answer is ever kept for reuse: the reuse duration is set to
//!   none on every context.
//!
//! What it does not prove is in spec section 222: the answer is a value
//! in the engine's process, and a program that replaces the engine can
//! raise a real prompt with false words. The password, unlike a finger,
//! can be typed by a program the person gave the Accessibility permission.
//!
//! The system is behind one small trait, [`Authenticator`], so that the
//! mapping is tested with a made-up system on every operating system. The
//! real one is in the module `system`, compiled on macOS only.

use std::sync::Arc;
use std::time::Duration;

use a2m_actions::BoxFuture;
use tokio::time::Instant;

use super::{Availability, PresencePrompt, PresenceReason, PromptAnswer, PromptName, PromptText};

/// The longest a call that asks nobody may take. A call that raises the
/// prompt is bounded by its deadline instead.
const PATIENCE: Duration = Duration::from_secs(10);

/// The codes of LocalAuthentication's errors that this build knows, as
/// `LAError` numbers them. A test on macOS holds them to the framework's.
pub(crate) mod code {
    pub(crate) const AUTHENTICATION_FAILED: isize = -1;
    pub(crate) const USER_CANCEL: isize = -2;
    pub(crate) const USER_FALLBACK: isize = -3;
    pub(crate) const SYSTEM_CANCEL: isize = -4;
    pub(crate) const PASSCODE_NOT_SET: isize = -5;
    pub(crate) const BIOMETRY_NOT_AVAILABLE: isize = -6;
    pub(crate) const BIOMETRY_NOT_ENROLLED: isize = -7;
    pub(crate) const BIOMETRY_LOCKOUT: isize = -8;
    pub(crate) const APP_CANCEL: isize = -9;
    pub(crate) const INVALID_CONTEXT: isize = -10;
    pub(crate) const COMPANION_NOT_AVAILABLE: isize = -11;
    pub(crate) const BIOMETRY_NOT_PAIRED: isize = -12;
    pub(crate) const BIOMETRY_DISCONNECTED: isize = -13;
    pub(crate) const INVALID_DIMENSIONS: isize = -14;
    pub(crate) const NOT_INTERACTIVE: isize = -1004;
}

/// Where the engine runs, as the window server says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Session {
    /// The window server gives this process no session: a remote shell, a
    /// daemon, or a machine with nobody logged in at the screen.
    NoGraphical,
    /// The user's graphical session exists, and another user's session is
    /// at the screen.
    NotOnConsole,
    /// The session is at the screen and the screen is locked.
    Locked,
    /// The session is at the screen, and the window server does not say
    /// it is locked.
    OnConsole,
    /// The window server's answer could not be read.
    Unreadable,
}

/// A way the system failed or refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// An error of LocalAuthentication, by its code.
    Code(isize),
    /// An error of another domain, by its code.
    Foreign(isize),
    /// No answer came: the context's thread could not start or ended, or
    /// the call that asks nobody took too long.
    Lost,
}

/// LocalAuthentication, as far as the prompt needs it. Every call is for
/// the policy of the device's owner.
pub(crate) trait Authenticator: Send + Sync {
    /// What the window server says about this process's session.
    fn session(&self) -> BoxFuture<'_, Session>;

    /// `canEvaluatePolicy`: whether the policy could be evaluated now. It
    /// shows nothing.
    fn can_evaluate(&self) -> BoxFuture<'_, Result<(), Fault>>;

    /// `evaluatePolicy` with `reason`, which the dialog shows after the
    /// program's name. Dropping the future before it ends invalidates the
    /// context, which takes the dialog away.
    fn evaluate(&self, reason: String) -> BoxFuture<'_, Result<(), Fault>>;
}

fn failed(what: String) -> Availability {
    Availability::not_available(
        PresenceReason::CheckFailed,
        format!("The check with the system's authentication failed ({what})."),
        "Run `anything2mcp doctor run` again. If it stays, the system log of this Mac says why.",
    )
}

/// What the window server's answer means for a person who wants to
/// approve: `Ok` only for a session at the screen that is not locked.
pub(crate) fn placed(session: Session) -> Result<(), Availability> {
    match session {
        Session::OnConsole => Ok(()),
        Session::NoGraphical => Err(Availability::not_available(
            PresenceReason::SessionNotActive,
            "The engine does not run inside a graphical login session, so the system cannot show a presence prompt.",
            "Start the engine from your own login session at the Mac itself. Over a remote shell and on a machine with nobody logged in, nothing can approve.",
        )),
        Session::NotOnConsole => Err(Availability::not_available(
            PresenceReason::SessionNotActive,
            "Another user's session is at the screen of this Mac, so nobody here can answer a prompt of yours.",
            "Switch back to your own session at the Mac, then press again.",
        )),
        Session::Locked => Err(Availability::not_available(
            PresenceReason::SessionLocked,
            "The screen of this Mac is locked, so no presence prompt is raised.",
            "Unlock the Mac, then press again.",
        )),
        Session::Unreadable => Err(failed("the window server's session".to_owned())),
    }
}

/// What a fault means for a person who wants to approve, when no prompt
/// has been raised or the prompt could not be.
pub(crate) fn unavailable(fault: &Fault) -> Availability {
    match *fault {
        Fault::Code(code::PASSCODE_NOT_SET) => Availability::not_available(
            PresenceReason::NotSetUp,
            "This Mac's account has no password, so the system has nothing to ask for.",
            "Set a password for your account in System Settings, then press again.",
        ),
        Fault::Code(
            code::BIOMETRY_NOT_AVAILABLE
            | code::BIOMETRY_NOT_ENROLLED
            | code::COMPANION_NOT_AVAILABLE
            | code::BIOMETRY_NOT_PAIRED
            | code::BIOMETRY_DISCONNECTED,
        ) => Availability::not_available(
            PresenceReason::NotSetUp,
            "Nothing this Mac could check is ready: no Touch ID, no paired watch, and no password the system will ask for.",
            "Set up Touch ID or a password for your account in System Settings, then press again.",
        ),
        Fault::Code(code::BIOMETRY_LOCKOUT) => Availability::not_available(
            PresenceReason::LockedOut,
            "Too many checks failed, and the system has locked them for now.",
            "Unlock the Mac with your password, then press again.",
        ),
        Fault::Code(code::NOT_INTERACTIVE) => Availability::not_available(
            PresenceReason::SessionNotActive,
            "The system may not show a prompt to this process, so no presence prompt can be raised.",
            "Start the engine from your own login session at the Mac itself, then press again.",
        ),
        Fault::Code(other) => failed(format!("local_authentication {other}")),
        Fault::Foreign(other) => failed(format!("another domain's error {other}")),
        Fault::Lost => failed("no answer".to_owned()),
    }
}

/// What an evaluation says once it has ended. Only the system's yes is a
/// person's presence; every fault is not.
pub(crate) fn answered(answer: Result<(), Fault>) -> PromptAnswer {
    match answer {
        Ok(()) => PromptAnswer::Present(PromptName::LocalAuthentication),
        // The person cancelled, chose the fallback, or the system or the
        // engine took the dialog away: nothing was passed.
        Err(Fault::Code(
            code::USER_CANCEL | code::USER_FALLBACK | code::SYSTEM_CANCEL | code::APP_CANCEL,
        )) => PromptAnswer::Dismissed,
        Err(Fault::Code(code::AUTHENTICATION_FAILED | code::BIOMETRY_LOCKOUT)) => {
            PromptAnswer::Denied
        }
        Err(fault) => PromptAnswer::NotAvailable(unavailable(&fault)),
    }
}

/// The reason the dialog shows, after the program's name and "is trying
/// to": the engine's own words, exactly as [`PromptText`] carries them.
/// It is never empty, which the system would refuse.
pub(crate) fn reason_for(text: &PromptText) -> String {
    format!("{}: {}", text.title, text.detail)
}

/// `step`, or `lost` when it takes longer than a call that asks nobody
/// may.
async fn bounded<T>(lost: T, step: impl Future<Output = T>) -> T {
    tokio::time::timeout(PATIENCE, step).await.unwrap_or(lost)
}

/// The presence prompt over LocalAuthentication.
pub(crate) struct LocalAuthenticationPrompt {
    system: Arc<dyn Authenticator>,
}

impl LocalAuthenticationPrompt {
    /// The prompt over this Mac's own LocalAuthentication.
    #[cfg(target_os = "macos")]
    pub(crate) fn system() -> Self {
        Self::over(Arc::new(system::Mac))
    }

    /// The prompt over `system`.
    pub(crate) fn over(system: Arc<dyn Authenticator>) -> Self {
        Self { system }
    }

    /// Asks the window server and the policy, without raising anything,
    /// whether a prompt can be raised.
    async fn ready(&self) -> Result<(), Availability> {
        placed(bounded(Session::Unreadable, self.system.session()).await)?;
        bounded(Err(Fault::Lost), self.system.can_evaluate())
            .await
            .map_err(|fault| unavailable(&fault))
    }
}

impl PresencePrompt for LocalAuthenticationPrompt {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async move {
            match self.ready().await {
                Ok(()) => Availability::ready(
                    PromptName::LocalAuthentication,
                    "Approvals on this machine ask for Touch ID, your watch, or your account password. The prompt names the tool and the first twelve characters of the change, as the page does.",
                ),
                Err(why) => why,
            }
        })
    }

    fn ask(&self, text: PromptText, deadline: Instant) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            if let Err(why) = self.ready().await {
                return PromptAnswer::NotAvailable(why);
            }
            // At or after the deadline nothing is raised.
            if Instant::now() >= deadline {
                return PromptAnswer::Dismissed;
            }
            tokio::select! {
                // The deadline is looked at first: at or after it the
                // answer is a dismissal, whatever the system says. The
                // evaluation's future is dropped, which takes the dialog
                // away.
                biased;
                () = tokio::time::sleep_until(deadline) => PromptAnswer::Dismissed,
                answer = self.system.evaluate(reason_for(&text)) => answered(answer),
            }
        })
    }
}

/// The real LocalAuthentication and window server of this Mac.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // calls into LocalAuthentication and CoreGraphics; each is explained where it is made
mod system {
    use std::sync::{Mutex, mpsc};

    use a2m_actions::BoxFuture;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString, ns_string};
    use objc2_local_authentication::{LAContext, LAErrorDomain, LAPolicy};
    use tokio::sync::oneshot;

    use super::{Authenticator, Fault, Session};

    /// Touch ID or a paired watch first, and the account's password where
    /// neither can be used.
    const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthentication;

    #[link(name = "CoreGraphics", kind = "framework")]
    // SAFETY: the declaration is the one in `CGSession.h`. The function
    // takes nothing and returns a dictionary the caller owns, or null when
    // the caller runs in no window-server session. A `CFDictionaryRef` is
    // toll-free bridged to `NSDictionary`.
    unsafe extern "C" {
        safe fn CGSessionCopyCurrentDictionary() -> *mut NSDictionary<NSString, AnyObject>;
    }

    /// A key's value in the session's dictionary, as a yes or a no.
    fn flag(session: &NSDictionary<NSString, AnyObject>, key: &NSString) -> Option<bool> {
        let value = session.objectForKey(key)?;
        let number = value.downcast::<NSNumber>().ok()?;
        Some(number.boolValue())
    }

    fn read_session() -> Session {
        // SAFETY: the pointer is null or a dictionary this call owns, as
        // `CGSessionCopyCurrentDictionary` returns it; `from_raw` takes that
        // one ownership and releases it once.
        let Some(session) = (unsafe { Retained::from_raw(CGSessionCopyCurrentDictionary()) })
        else {
            return Session::NoGraphical;
        };
        // Not in Apple's documentation: the key is present, and yes, while
        // the screen is locked. Read as "locked" only when it says so.
        if flag(&session, ns_string!("CGSSessionScreenIsLocked")) == Some(true) {
            return Session::Locked;
        }
        // `kCGSessionOnConsoleKey` in `CGSession.h`.
        match flag(&session, ns_string!("kCGSSessionOnConsoleKey")) {
            Some(true) => Session::OnConsole,
            Some(false) => Session::NotOnConsole,
            None => Session::Unreadable,
        }
    }

    fn fault_of(error: &NSError) -> Fault {
        // SAFETY: `LAErrorDomain` is a constant string the framework
        // exports; reading it has no other condition.
        let ours = error.domain().isEqualToString(unsafe { LAErrorDomain });
        if ours {
            Fault::Code(error.code())
        } else {
            Fault::Foreign(error.code())
        }
    }

    /// A new context for the device owner's policy that keeps no answer:
    /// with a reuse duration of none, no earlier unlock of the Mac counts.
    fn context() -> Retained<LAContext> {
        // SAFETY: `new` has no condition. The context is used on the
        // thread that made it, and released there.
        let context = unsafe { LAContext::new() };
        // SAFETY: zero is in the property's range; it is the default,
        // written so that no other default can apply.
        unsafe { context.setTouchIDAuthenticationAllowableReuseDuration(0.0) };
        context
    }

    /// Holds one context on a thread of its own, because a context may not
    /// move between threads: evaluates, waits until the evaluation has
    /// ended or the ask has gone, and invalidates the context.
    fn hold(
        reason: &str,
        answer: oneshot::Sender<Result<(), Fault>>,
        ended: mpsc::Sender<()>,
        stop: &mpsc::Receiver<()>,
    ) {
        if reason.is_empty() {
            // The system refuses an empty reason with an exception.
            let _ = answer.send(Err(Fault::Lost));
            return;
        }
        let context = context();
        let answer = Mutex::new(Some(answer));
        let reply = RcBlock::new(move |passed: Bool, error: *mut NSError| {
            // SAFETY: the framework passes null or an error that lives for
            // the length of this call.
            let fault = unsafe { error.as_ref() }.map(fault_of);
            let got = match (passed.as_bool(), fault) {
                (true, None) => Ok(()),
                // A yes that comes with an error is not understood, and so
                // not a yes.
                (_, Some(fault)) => Err(fault),
                (false, None) => Err(Fault::Lost),
            };
            if let Some(answer) = answer.lock().ok().and_then(|mut slot| slot.take()) {
                let _ = answer.send(got);
            }
            let _ = ended.send(());
        });
        let reason = NSString::from_str(reason);
        // SAFETY: the policy is one the framework defines; the reason is
        // not empty; the block is sendable, since what it holds (a mutex
        // of a channel's end, and a channel's end) may be used from any
        // thread, which the framework's private queue is.
        unsafe { context.evaluatePolicy_localizedReason_reply(POLICY, &reason, &reply) };
        // The evaluation has ended, or the ask has gone.
        let _ = stop.recv();
        // SAFETY: invalidating ends an evaluation still running, which then
        // replies "app cancel" to a channel nobody reads; invalidating an
        // ended one has no effect.
        unsafe { context.invalidate() };
    }

    /// Sends "stop" to the context's thread when the ask ends or is
    /// dropped.
    struct TakeAway(mpsc::Sender<()>);

    impl Drop for TakeAway {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    /// The system of this Mac.
    pub(super) struct Mac;

    impl Authenticator for Mac {
        fn session(&self) -> BoxFuture<'_, Session> {
            Box::pin(async {
                tokio::task::spawn_blocking(read_session)
                    .await
                    .unwrap_or(Session::Unreadable)
            })
        }

        fn can_evaluate(&self) -> BoxFuture<'_, Result<(), Fault>> {
            Box::pin(async {
                tokio::task::spawn_blocking(|| {
                    let context = context();
                    // SAFETY: the policy is one the framework defines; the
                    // call shows nothing.
                    unsafe { context.canEvaluatePolicy_error(POLICY) }
                        .map_err(|error| fault_of(&error))
                })
                .await
                .unwrap_or(Err(Fault::Lost))
            })
        }

        fn evaluate(&self, reason: String) -> BoxFuture<'_, Result<(), Fault>> {
            Box::pin(async move {
                let (answer, answered) = oneshot::channel();
                let (ended, stop) = mpsc::channel();
                let away = TakeAway(ended.clone());
                std::thread::Builder::new()
                    .name("a2m-presence".to_owned())
                    .spawn(move || hold(&reason, answer, ended, &stop))
                    .map_err(|_| Fault::Lost)?;
                let got = answered.await.unwrap_or(Err(Fault::Lost));
                drop(away);
                got
            })
        }
    }
}
