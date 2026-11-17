//! The Windows presence prompt.
//!
//! No test here raises a prompt a person would have to answer. The
//! mapping from what the session and Windows Hello can say to what the
//! prompt answers is walked in full, on every operating system, with a
//! made-up system behind the trait the real one implements. On Windows,
//! one test asks the real system what it can do, which raises nothing,
//! and one holds this build's result values to the system's.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::BoxFuture;
use tokio::time::Instant;

use super::world::within;
use crate::presence::windows_hello::{
    Fault, Lock, Session, Verifier, WindowsHelloPrompt, answered, code, message_for, placed,
    standing, unavailable,
};
use crate::{PresenceKind, PresencePrompt, PresenceReason, PromptAnswer, PromptName, PromptText};

const FAR: Duration = Duration::from_secs(900);

const PRESENT: PromptAnswer = PromptAnswer::Present(PromptName::WindowsHello);

/// A local session at the interactive window station, not locked.
const AT_THE_SCREEN: Session = Session {
    services: false,
    interactive_station: true,
    remote: false,
    lock: Lock::Unlocked,
};

fn every_session() -> Vec<Result<Session, Fault>> {
    let mut all = Vec::new();
    for services in [false, true] {
        for interactive_station in [false, true] {
            for remote in [false, true] {
                for lock in [Lock::Locked, Lock::Unlocked, Lock::Unknown] {
                    all.push(Ok(Session {
                        services,
                        interactive_station,
                        remote,
                        lock,
                    }));
                }
            }
        }
    }
    all.extend(every_fault().into_iter().map(Err));
    all
}

fn every_fault() -> Vec<Fault> {
    vec![
        Fault::Call(-2_147_024_891),
        Fault::Call(5),
        Fault::NoWindow,
        Fault::Lost,
    ]
}

/// Every value the system names, the values on either side, and the ends
/// of the range.
fn every_value() -> Vec<i32> {
    let mut all: Vec<i32> = (-1..=8).collect();
    all.extend([i32::MIN, i32::MAX]);
    all
}

fn every_answer() -> Vec<Result<i32, Fault>> {
    every_value()
        .into_iter()
        .map(Ok)
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
    session: Result<Session, Fault>,
    availability: Result<i32, Fault>,
    /// What a verification answers; `None` waits for a person who never
    /// comes.
    after: Option<Result<i32, Fault>>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn new(
        session: Result<Session, Fault>,
        availability: Result<i32, Fault>,
        after: Option<Result<i32, Fault>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            session,
            availability,
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
        self.calls().iter().any(|call| call.starts_with("verify "))
    }
}

/// Writes down that a verification was cancelled before it ended.
struct Away<'a> {
    fake: &'a Fake,
    ended: bool,
}

impl Drop for Away<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.fake.note("cancel".to_owned());
        }
    }
}

struct System(Arc<Fake>);

impl Verifier for System {
    fn session(&self) -> BoxFuture<'_, Result<Session, Fault>> {
        Box::pin(async move {
            self.0.note("session".to_owned());
            self.0.session
        })
    }

    fn availability(&self) -> BoxFuture<'_, Result<i32, Fault>> {
        Box::pin(async move {
            self.0.note("availability".to_owned());
            self.0.availability
        })
    }

    fn verify(&self, message: String) -> BoxFuture<'_, Result<i32, Fault>> {
        Box::pin(async move {
            self.0.note(format!("verify {message}"));
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

fn prompt_over(fake: &Arc<Fake>) -> WindowsHelloPrompt {
    WindowsHelloPrompt::over(Arc::new(System(Arc::clone(fake))))
}

fn text() -> PromptText {
    PromptText {
        title: "Approve a write by `example_orders_item_upsert`".to_owned(),
        detail: "Change 0123456789ab".to_owned(),
    }
}

#[test]
fn only_verified_is_a_persons_presence_and_no_error_is() {
    // After the prompt: of every result and every fault, exactly one is
    // presence.
    for answer in every_answer() {
        let got = answered(answer);
        assert_eq!(got == PRESENT, answer == Ok(code::VERIFIED), "{answer:?} gave {got:?}");
        assert_ne!(got, PromptAnswer::Confirmed, "{answer:?}");
    }
    // Each of the others has its name.
    assert_eq!(answered(Ok(code::CANCELED)), PromptAnswer::Dismissed);
    assert_eq!(answered(Ok(code::RETRIES_EXHAUSTED)), PromptAnswer::Denied);
    let named = |answer| reason(&answered(answer));
    for not_set_up in [code::DEVICE_NOT_PRESENT, code::NOT_CONFIGURED_FOR_USER] {
        assert_eq!(named(Ok(not_set_up)), Some(PresenceReason::NotSetUp));
    }
    assert_eq!(
        named(Ok(code::DISABLED_BY_POLICY)),
        Some(PresenceReason::DisabledByPolicy)
    );
    assert_eq!(named(Ok(code::DEVICE_BUSY)), Some(PresenceReason::DeviceBusy));
    for unknown in [-1, 7, 8, i32::MIN, i32::MAX] {
        assert_eq!(named(Ok(unknown)), Some(PresenceReason::CheckFailed), "{unknown}");
    }
    for fault in every_fault() {
        assert_eq!(named(Err(fault)), Some(PresenceReason::CheckFailed), "{fault:?}");
    }
    // An error is shown by its code alone, as Windows writes one.
    let shown = unavailable(&Fault::Call(-2_147_024_891));
    assert!(shown.message.contains("0x80070005"), "{shown:?}");

    // Before the prompt: only `Available` lets a prompt be raised. Of the
    // values only a verification has, none is availability.
    for availability in every_answer() {
        assert_eq!(
            standing(availability).is_ok(),
            availability == Ok(code::VERIFIED),
            "{availability:?}"
        );
    }
    let why = |availability| standing(availability).unwrap_err().reason;
    assert_eq!(why(Ok(code::DEVICE_NOT_PRESENT)), PresenceReason::NotSetUp);
    assert_eq!(why(Ok(code::DISABLED_BY_POLICY)), PresenceReason::DisabledByPolicy);
    assert_eq!(why(Ok(code::DEVICE_BUSY)), PresenceReason::DeviceBusy);
    for odd in [code::RETRIES_EXHAUSTED, code::CANCELED, 9] {
        assert_eq!(why(Ok(odd)), PresenceReason::CheckFailed, "{odd}");
    }

    // Only a local session at the interactive window station, not
    // locked, lets a prompt be raised.
    for session in every_session() {
        assert_eq!(
            placed(session).is_ok(),
            session == Ok(AT_THE_SCREEN),
            "{session:?}"
        );
    }
    let why = |session| placed(Ok(session)).unwrap_err().reason;
    let services = Session {
        services: true,
        ..AT_THE_SCREEN
    };
    let own_station = Session {
        interactive_station: false,
        ..AT_THE_SCREEN
    };
    let remote = Session {
        remote: true,
        ..AT_THE_SCREEN
    };
    let locked = Session {
        lock: Lock::Locked,
        ..AT_THE_SCREEN
    };
    let unknown = Session {
        lock: Lock::Unknown,
        ..AT_THE_SCREEN
    };
    assert_eq!(why(services), PresenceReason::NoWindowStation);
    assert_eq!(why(own_station), PresenceReason::NoWindowStation);
    assert_eq!(why(remote), PresenceReason::SessionRemote);
    assert_eq!(why(locked), PresenceReason::SessionLocked);
    assert_eq!(why(unknown), PresenceReason::CheckFailed);
    // Where nothing reaches a screen, that is said first.
    assert_eq!(
        why(Session {
            remote: true,
            lock: Lock::Locked,
            ..services
        }),
        PresenceReason::NoWindowStation
    );
}

#[tokio::test]
async fn the_prompt_is_raised_only_where_windows_hello_could_ask_and_passes_only_on_verified() {
    for session in every_session() {
        for availability in every_answer() {
            for after in every_answer() {
                let fake = Fake::new(session, availability, Some(after));
                let got = within(
                    "the ask",
                    prompt_over(&fake).ask(text(), Instant::now() + FAR),
                )
                .await;
                let could = session == Ok(AT_THE_SCREEN) && availability == Ok(code::VERIFIED);
                assert_eq!(
                    fake.raised(),
                    could,
                    "{session:?}, {availability:?}: {:?}",
                    fake.calls()
                );
                assert_eq!(
                    got == PRESENT,
                    could && after == Ok(code::VERIFIED),
                    "{session:?}, {availability:?}, then {after:?} gave {got:?}"
                );
                if could {
                    assert_eq!(got, answered(after));
                } else {
                    assert!(reason(&got).is_some(), "{session:?} gave {got:?}");
                }
            }
        }
    }

    // What it says it can do raises nothing, and is ready only where an
    // ask would be raised.
    for session in every_session() {
        for availability in every_answer() {
            let fake = Fake::new(session, availability, Some(Ok(code::VERIFIED)));
            let available = within("the availability", prompt_over(&fake).available()).await;
            assert!(!fake.raised());
            let could = session == Ok(AT_THE_SCREEN) && availability == Ok(code::VERIFIED);
            assert_eq!(available.proves_presence(), could, "{session:?}");
            if could {
                assert_eq!(
                    (available.prompt, available.reason),
                    (Some(PromptName::WindowsHello), PresenceReason::Ready)
                );
                assert!(available.remedy.is_empty());
            } else {
                assert_eq!(available.kind, PresenceKind::NotAvailable);
            }
        }
    }
    // The session is asked first, and one that cannot show the prompt
    // asks Windows Hello nothing.
    let fake = Fake::new(
        Ok(Session {
            remote: true,
            ..AT_THE_SCREEN
        }),
        Ok(code::VERIFIED),
        Some(Ok(code::VERIFIED)),
    );
    within("the availability", prompt_over(&fake).available()).await;
    assert_eq!(fake.calls(), ["session"]);
}

#[tokio::test]
async fn the_dialog_shows_the_engines_words_and_the_deadline_cancels_it() {
    // The words, exactly as the surface gave them.
    let fake = Fake::new(Ok(AT_THE_SCREEN), Ok(code::VERIFIED), Some(Ok(code::VERIFIED)));
    assert_eq!(
        within("the ask", prompt_over(&fake).ask(text(), Instant::now() + FAR)).await,
        PRESENT
    );
    let message = message_for(&text());
    assert_eq!(
        message,
        "Approve a write by `example_orders_item_upsert`: Change 0123456789ab"
    );
    assert_eq!(
        fake.calls(),
        ["session", "availability", &format!("verify {message}")]
    );

    // The dialog is up and nobody answers it: at the deadline it is
    // cancelled and the answer is a dismissal.
    let fake = Fake::new(Ok(AT_THE_SCREEN), Ok(code::VERIFIED), None);
    let deadline = Instant::now() + Duration::from_millis(300);
    let got = within("the ask", prompt_over(&fake).ask(text(), deadline)).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(Instant::now() >= deadline);
    assert_eq!(fake.calls().last().map(String::as_str), Some("cancel"));

    // An ask that is dropped cancels its dialog too.
    let fake = Fake::new(Ok(AT_THE_SCREEN), Ok(code::VERIFIED), None);
    let prompt = prompt_over(&fake);
    let dropped =
        tokio::time::timeout(Duration::from_millis(100), prompt.ask(text(), Instant::now() + FAR))
            .await;
    assert!(dropped.is_err());
    assert_eq!(fake.calls().last().map(String::as_str), Some("cancel"));

    // At or after the deadline nothing is raised at all, even where the
    // person would pass.
    let fake = Fake::new(Ok(AT_THE_SCREEN), Ok(code::VERIFIED), Some(Ok(code::VERIFIED)));
    let got = prompt_over(&fake).ask(text(), Instant::now()).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(!fake.raised(), "{:?}", fake.calls());
}

/// This build's values are the system's.
#[cfg(windows)]
#[test]
fn the_values_are_the_systems() {
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult as Result, UserConsentVerifierAvailability as Availability,
    };
    for (ours, theirs) in [
        (code::VERIFIED, Result::Verified.0),
        (code::DEVICE_NOT_PRESENT, Result::DeviceNotPresent.0),
        (code::NOT_CONFIGURED_FOR_USER, Result::NotConfiguredForUser.0),
        (code::DISABLED_BY_POLICY, Result::DisabledByPolicy.0),
        (code::DEVICE_BUSY, Result::DeviceBusy.0),
        (code::RETRIES_EXHAUSTED, Result::RetriesExhausted.0),
        (code::CANCELED, Result::Canceled.0),
        (code::VERIFIED, Availability::Available.0),
        (code::DEVICE_NOT_PRESENT, Availability::DeviceNotPresent.0),
        (code::NOT_CONFIGURED_FOR_USER, Availability::NotConfiguredForUser.0),
        (code::DISABLED_BY_POLICY, Availability::DisabledByPolicy.0),
        (code::DEVICE_BUSY, Availability::DeviceBusy.0),
    ] {
        assert_eq!(ours, theirs);
    }
}

/// The real system, on Windows: what it can do is one of the named
/// outcomes, within the bound, and asking raises nothing. A hosted runner
/// has no Windows Hello set up and may run as a service; either is an
/// answer. The real `ask` is never called here: it would wait for a
/// person.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_windows_the_real_prompt_says_what_it_can_do_by_name() {
    let available = within("the availability", crate::system_prompt().available()).await;
    eprintln!(
        "this machine: {} {} {:?}: {} {}",
        available.kind.as_str(),
        available.reason.as_str(),
        available.prompt.map(PromptName::as_str),
        available.message,
        available.remedy
    );
    assert!(!available.message.is_empty());
    if available.proves_presence() {
        assert_eq!(available.reason, PresenceReason::Ready);
        assert_eq!(available.prompt, Some(PromptName::WindowsHello));
    } else {
        assert_eq!(available.kind, PresenceKind::NotAvailable);
        assert!(!available.remedy.is_empty());
        assert!(
            matches!(
                available.reason,
                PresenceReason::NoWindowStation
                    | PresenceReason::SessionRemote
                    | PresenceReason::SessionLocked
                    | PresenceReason::NotSetUp
                    | PresenceReason::DisabledByPolicy
                    | PresenceReason::DeviceBusy
                    | PresenceReason::CheckFailed
            ),
            "{available:?}"
        );
    }
}
