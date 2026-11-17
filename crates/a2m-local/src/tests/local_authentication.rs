//! The macOS presence prompt.
//!
//! No test here raises a prompt a person would have to answer. The
//! mapping from what the window server and LocalAuthentication can say to
//! what the prompt answers is walked in full, on every operating system,
//! with a made-up system behind the trait the real one implements. On
//! macOS, one test asks the real system what it can do, which raises
//! nothing, and one holds this build's error codes to the framework's.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::BoxFuture;
use tokio::time::Instant;

use super::world::within;
use crate::presence::local_authentication::{
    Authenticator, Fault, LocalAuthenticationPrompt, Session, answered, code, placed, reason_for,
    unavailable,
};
use crate::{PresenceKind, PresencePrompt, PresenceReason, PromptAnswer, PromptName, PromptText};

const FAR: Duration = Duration::from_secs(900);

const PRESENT: PromptAnswer = PromptAnswer::Present(PromptName::LocalAuthentication);

fn every_session() -> [Session; 5] {
    [
        Session::NoGraphical,
        Session::NotOnConsole,
        Session::Locked,
        Session::OnConsole,
        Session::Unreadable,
    ]
}

/// Every code the framework names, the codes on either side of them, and
/// the ends of the range.
fn every_code() -> Vec<isize> {
    let mut all: Vec<isize> = (-16..=1).collect();
    all.extend([-1003, -1004, -1005, isize::MIN, isize::MAX]);
    all
}

fn every_fault() -> Vec<Fault> {
    every_code()
        .into_iter()
        .map(Fault::Code)
        .chain([Fault::Foreign(code::USER_CANCEL), Fault::Lost])
        .collect()
}

fn every_answer() -> Vec<Result<(), Fault>> {
    std::iter::once(Ok(()))
        .chain(every_fault().into_iter().map(Err))
        .collect()
}

fn reason(answer: &PromptAnswer) -> Option<PresenceReason> {
    match answer {
        PromptAnswer::NotAvailable(why) => {
            assert_eq!(why.kind, PresenceKind::NotAvailable);
            assert_eq!(why.prompt, None);
            assert!(!why.message.is_empty() && !why.remedy.is_empty());
            Some(why.reason)
        }
        _ => None,
    }
}

/// A made-up system: it answers as the test set it and writes down what
/// it was asked.
struct Fake {
    session: Session,
    can: Result<(), Fault>,
    /// What an evaluation answers; `None` waits for a person who never
    /// comes.
    after: Option<Result<(), Fault>>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn new(
        session: Session,
        can: Result<(), Fault>,
        after: Option<Result<(), Fault>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            session,
            can,
            after,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn note(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn raised(&self) -> bool {
        self.calls()
            .iter()
            .any(|call| call.starts_with("evaluate "))
    }
}

/// Writes down that a dialog was taken away before it ended.
struct Away<'a> {
    fake: &'a Fake,
    ended: bool,
}

impl Drop for Away<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.fake.note("invalidate".to_owned());
        }
    }
}

struct System(Arc<Fake>);

impl Authenticator for System {
    fn session(&self) -> BoxFuture<'_, Session> {
        Box::pin(async move {
            self.0.note("session".to_owned());
            self.0.session
        })
    }

    fn can_evaluate(&self) -> BoxFuture<'_, Result<(), Fault>> {
        Box::pin(async move {
            self.0.note("can".to_owned());
            self.0.can
        })
    }

    fn evaluate(&self, reason: String) -> BoxFuture<'_, Result<(), Fault>> {
        Box::pin(async move {
            self.0.note(format!("evaluate {reason}"));
            let mut away = Away {
                fake: &self.0,
                ended: false,
            };
            let got = match self.0.after {
                Some(answer) => answer,
                None => std::future::pending().await,
            };
            away.ended = true;
            got
        })
    }
}

fn prompt_over(fake: &Arc<Fake>) -> LocalAuthenticationPrompt {
    LocalAuthenticationPrompt::over(Arc::new(System(Arc::clone(fake))))
}

fn text() -> PromptText {
    PromptText {
        title: "Approve a write by `example_orders_item_upsert`".to_owned(),
        detail: "Change 0123456789ab".to_owned(),
    }
}

#[test]
fn only_the_systems_yes_is_a_persons_presence_and_no_error_is() {
    // After the prompt: of every answer, exactly one is presence.
    for answer in every_answer() {
        let got = answered(answer);
        assert_eq!(got == PRESENT, answer == Ok(()), "{answer:?} gave {got:?}");
        assert_ne!(got, PromptAnswer::Confirmed, "{answer:?}");
    }
    // Each of the others has its name.
    for dismissed in [
        code::USER_CANCEL,
        code::USER_FALLBACK,
        code::SYSTEM_CANCEL,
        code::APP_CANCEL,
    ] {
        assert_eq!(
            answered(Err(Fault::Code(dismissed))),
            PromptAnswer::Dismissed
        );
    }
    for denied in [code::AUTHENTICATION_FAILED, code::BIOMETRY_LOCKOUT] {
        assert_eq!(answered(Err(Fault::Code(denied))), PromptAnswer::Denied);
    }
    let named = |fault| reason(&answered(Err(fault)));
    for not_set_up in [
        code::PASSCODE_NOT_SET,
        code::BIOMETRY_NOT_AVAILABLE,
        code::BIOMETRY_NOT_ENROLLED,
        code::COMPANION_NOT_AVAILABLE,
        code::BIOMETRY_NOT_PAIRED,
        code::BIOMETRY_DISCONNECTED,
    ] {
        assert_eq!(
            named(Fault::Code(not_set_up)),
            Some(PresenceReason::NotSetUp)
        );
    }
    assert_eq!(
        named(Fault::Code(code::NOT_INTERACTIVE)),
        Some(PresenceReason::SessionNotActive)
    );
    // Every code this build does not know, an error of another domain even
    // with a known number, and no answer at all are failures of the check.
    for failed in [
        Fault::Code(code::INVALID_CONTEXT),
        Fault::Code(code::INVALID_DIMENSIONS),
        Fault::Code(1),
        Fault::Code(-1005),
        Fault::Code(isize::MIN),
        Fault::Foreign(code::USER_CANCEL),
        Fault::Foreign(code::AUTHENTICATION_FAILED),
        Fault::Lost,
    ] {
        assert_eq!(
            named(failed),
            Some(PresenceReason::CheckFailed),
            "{failed:?}"
        );
    }

    // Before the prompt, every fault is a reason, never a yes; a lockout
    // there has its own.
    for fault in every_fault() {
        let why = unavailable(&fault);
        assert!(!why.proves_presence(), "{fault:?}");
        assert!(!why.message.is_empty() && !why.remedy.is_empty());
    }
    assert_eq!(
        unavailable(&Fault::Code(code::BIOMETRY_LOCKOUT)).reason,
        PresenceReason::LockedOut
    );

    // Only a session at the screen, not locked, lets a prompt be raised.
    for session in every_session() {
        assert_eq!(placed(session).is_ok(), session == Session::OnConsole);
    }
    let why = |session| placed(session).unwrap_err().reason;
    assert_eq!(why(Session::NoGraphical), PresenceReason::SessionNotActive);
    assert_eq!(why(Session::NotOnConsole), PresenceReason::SessionNotActive);
    assert_eq!(why(Session::Locked), PresenceReason::SessionLocked);
    assert_eq!(why(Session::Unreadable), PresenceReason::CheckFailed);
}

#[tokio::test]
async fn the_prompt_is_raised_only_where_the_system_could_ask_and_passes_only_on_its_yes() {
    for session in every_session() {
        for can in every_answer() {
            for after in every_answer() {
                let fake = Fake::new(session, can, Some(after));
                let got = within(
                    "the ask",
                    prompt_over(&fake).ask(text(), Instant::now() + FAR),
                )
                .await;
                let could = session == Session::OnConsole && can == Ok(());
                assert_eq!(
                    fake.raised(),
                    could,
                    "{session:?}, {can:?}: {:?}",
                    fake.calls()
                );
                assert_eq!(
                    got == PRESENT,
                    could && after == Ok(()),
                    "{session:?}, {can:?}, then {after:?} gave {got:?}"
                );
                if could {
                    assert_eq!(got, answered(after));
                } else {
                    assert!(reason(&got).is_some(), "{session:?}, {can:?} gave {got:?}");
                }
            }
        }
    }

    // What it says it can do raises nothing, and is ready only where an
    // ask would be raised.
    for session in every_session() {
        for can in every_answer() {
            let fake = Fake::new(session, can, Some(Ok(())));
            let available = within("the availability", prompt_over(&fake).available()).await;
            assert!(!fake.raised());
            let could = session == Session::OnConsole && can == Ok(());
            assert_eq!(available.proves_presence(), could, "{session:?}, {can:?}");
            if could {
                assert_eq!(
                    (available.prompt, available.reason),
                    (Some(PromptName::LocalAuthentication), PresenceReason::Ready)
                );
                assert!(available.remedy.is_empty());
            } else {
                assert_eq!(available.kind, PresenceKind::NotAvailable);
            }
        }
    }
    // The window server is asked first, and a session that cannot show
    // the prompt asks the policy nothing.
    let fake = Fake::new(Session::Locked, Ok(()), Some(Ok(())));
    within("the availability", prompt_over(&fake).available()).await;
    assert_eq!(fake.calls(), ["session"]);
}

#[tokio::test]
async fn the_dialog_shows_the_engines_words_and_the_deadline_takes_it_away() {
    // The words, exactly as the surface gave them.
    let fake = Fake::new(Session::OnConsole, Ok(()), Some(Ok(())));
    assert_eq!(
        within(
            "the ask",
            prompt_over(&fake).ask(text(), Instant::now() + FAR)
        )
        .await,
        PRESENT
    );
    let reason = reason_for(&text());
    assert_eq!(
        reason,
        "Approve a write by `example_orders_item_upsert`: Change 0123456789ab"
    );
    assert_eq!(
        fake.calls(),
        ["session", "can", &format!("evaluate {reason}")]
    );
    // A reason is never empty, which the system would refuse.
    let empty = PromptText {
        title: String::new(),
        detail: String::new(),
    };
    assert!(!reason_for(&empty).is_empty());

    // The dialog is up and nobody answers it: at the deadline it is taken
    // away and the answer is a dismissal.
    let fake = Fake::new(Session::OnConsole, Ok(()), None);
    let deadline = Instant::now() + Duration::from_millis(300);
    let got = within("the ask", prompt_over(&fake).ask(text(), deadline)).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(Instant::now() >= deadline);
    assert_eq!(fake.calls().last().map(String::as_str), Some("invalidate"));

    // An ask that is dropped takes its dialog away too.
    let fake = Fake::new(Session::OnConsole, Ok(()), None);
    let prompt = prompt_over(&fake);
    let dropped = tokio::time::timeout(
        Duration::from_millis(100),
        prompt.ask(text(), Instant::now() + FAR),
    )
    .await;
    assert!(dropped.is_err());
    assert_eq!(fake.calls().last().map(String::as_str), Some("invalidate"));

    // At or after the deadline nothing is raised at all, even where the
    // person would pass.
    let fake = Fake::new(Session::OnConsole, Ok(()), Some(Ok(())));
    let got = prompt_over(&fake).ask(text(), Instant::now()).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(!fake.raised(), "{:?}", fake.calls());
}

/// This build's codes are the framework's.
#[cfg(target_os = "macos")]
#[test]
fn the_codes_are_the_frameworks() {
    use objc2_local_authentication::LAError;
    for (ours, theirs) in [
        (code::AUTHENTICATION_FAILED, LAError::AuthenticationFailed),
        (code::USER_CANCEL, LAError::UserCancel),
        (code::USER_FALLBACK, LAError::UserFallback),
        (code::SYSTEM_CANCEL, LAError::SystemCancel),
        (code::PASSCODE_NOT_SET, LAError::PasscodeNotSet),
        (code::BIOMETRY_NOT_AVAILABLE, LAError::BiometryNotAvailable),
        (code::BIOMETRY_NOT_ENROLLED, LAError::BiometryNotEnrolled),
        (code::BIOMETRY_LOCKOUT, LAError::BiometryLockout),
        (code::APP_CANCEL, LAError::AppCancel),
        (code::INVALID_CONTEXT, LAError::InvalidContext),
        (
            code::COMPANION_NOT_AVAILABLE,
            LAError::CompanionNotAvailable,
        ),
        (code::BIOMETRY_NOT_PAIRED, LAError::BiometryNotPaired),
        (code::BIOMETRY_DISCONNECTED, LAError::BiometryDisconnected),
        (code::INVALID_DIMENSIONS, LAError::InvalidDimensions),
        (code::NOT_INTERACTIVE, LAError::NotInteractive),
    ] {
        assert_eq!(ours, theirs.0);
    }
}

/// The real system, on a Mac: what it can do is one of the named
/// outcomes, within the bound, and asking raises nothing. A hosted runner
/// may have no graphical session and no Touch ID; either is an answer. The
/// real `ask` is never called here: it would wait for a person.
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_a_mac_the_real_prompt_says_what_it_can_do_by_name() {
    let available = within("the availability", crate::system_prompt().available()).await;
    eprintln!(
        "this Mac: {} {} {:?}: {} {}",
        available.kind.as_str(),
        available.reason.as_str(),
        available.prompt.map(PromptName::as_str),
        available.message,
        available.remedy
    );
    assert!(!available.message.is_empty());
    if available.proves_presence() {
        assert_eq!(available.reason, PresenceReason::Ready);
        assert_eq!(available.prompt, Some(PromptName::LocalAuthentication));
    } else {
        assert_eq!(available.kind, PresenceKind::NotAvailable);
        assert!(!available.remedy.is_empty());
        assert!(
            matches!(
                available.reason,
                PresenceReason::SessionNotActive
                    | PresenceReason::SessionLocked
                    | PresenceReason::NotSetUp
                    | PresenceReason::LockedOut
                    | PresenceReason::CheckFailed
            ),
            "{available:?}"
        );
    }
}
