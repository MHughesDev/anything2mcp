//! The Linux presence prompt.
//!
//! No test here raises a prompt a person would have to answer. The
//! mapping from what polkit and the bus can say to what the prompt
//! answers is walked in full with a made-up authority behind the traits
//! the real bus implements. One test speaks to a real polkit, and runs
//! only on a machine that was prepared for it: see [`REAL_VARIABLE`].

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant as StdInstant};

use a2m_actions::BoxFuture;
use tokio::time::Instant;

use super::world::within;
use crate::presence::polkit::{
    ACTION_ID, Authority, Bus, Fault, POLICY_FILE, PolkitPrompt, SystemBus, Verdict, answered,
    fault_named, standing, unavailable,
};
use crate::{
    Availability, PresenceKind, PresencePrompt, PresenceReason, PromptAnswer, PromptName,
    PromptText,
};

/// The policy file as it ships.
const POLICY: &str = include_str!("../../polkit/io.github.mhughesdev.anything2mcp.presence.policy");

/// Says that this machine was prepared for the test against a real
/// polkit, and how far: `no_polkit`, `no_policy`, `no_session`, `root`,
/// `no_agent`, or `ready`. The design of this crate has the table, and a
/// CI job prepares each in turn.
const REAL_VARIABLE: &str = "A2M_TEST_POLKIT";
/// The made-up password of the test account on a prepared machine.
const REAL_PASSWORD: &str = "secret-for-tests-presence";

const FAR: Duration = Duration::from_secs(900);

const fn verdict(authorized: bool, challenge: bool, dismissed: bool) -> Verdict {
    Verdict {
        authorized,
        challenge,
        dismissed,
    }
}

/// polkit would have to ask a person.
const WOULD_ASK: Verdict = verdict(false, true, false);
/// The person passed the prompt.
const PASSED: Verdict = verdict(true, false, false);

fn every_verdict() -> Vec<Verdict> {
    let mut all = Vec::new();
    for authorized in [false, true] {
        for challenge in [false, true] {
            for dismissed in [false, true] {
                all.push(verdict(authorized, challenge, dismissed));
            }
        }
    }
    all
}

fn every_fault() -> Vec<Fault> {
    vec![
        Fault::NoSystemBus,
        Fault::NoPolkit,
        Fault::NotRegistered,
        Fault::Refused,
        Fault::Cancelled,
        Fault::Other("org.example.Error.Unheard".to_owned()),
    ]
}

/// Everything a check can end with: each verdict and each fault.
fn every_answer() -> Vec<Result<Verdict, Fault>> {
    every_verdict()
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

/// A made-up authority: it answers as the test set it and writes down
/// what it was asked.
struct Fake {
    /// Whether a bus can be connected to.
    bus: Result<(), Fault>,
    /// What a check that asks nobody answers.
    before: Result<Verdict, Fault>,
    /// What a check that may ask the person answers; `None` waits for a
    /// person who never comes.
    after: Option<Result<Verdict, Fault>>,
    /// Whether polkit knows the action.
    registered: Result<bool, Fault>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn new(before: Result<Verdict, Fault>, after: Option<Result<Verdict, Fault>>) -> Arc<Self> {
        Arc::new(Self {
            bus: Ok(()),
            before,
            after,
            registered: Ok(true),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn note(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    /// Whether a prompt was raised: a check that may ask the person.
    fn raised(&self) -> bool {
        self.calls().iter().any(|call| call.starts_with("ask "))
    }
}

struct FakeBus(Arc<Fake>);

impl Bus for FakeBus {
    fn connect(&self) -> BoxFuture<'_, Result<Arc<dyn Authority>, Fault>> {
        Box::pin(async move {
            self.0.note("connect".to_owned());
            self.0.bus.clone()?;
            Ok(Arc::clone(&self.0) as Arc<dyn Authority>)
        })
    }
}

impl Authority for Fake {
    fn check<'a>(
        &'a self,
        interactive: bool,
        cancellation: &'a str,
    ) -> BoxFuture<'a, Result<Verdict, Fault>> {
        Box::pin(async move {
            if !interactive {
                assert_eq!(cancellation, "", "a check that asks nobody needs no id");
                self.note("probe".to_owned());
                return self.before.clone();
            }
            assert!(!cancellation.is_empty());
            self.note(format!("ask {cancellation}"));
            match &self.after {
                Some(answer) => answer.clone(),
                None => std::future::pending().await,
            }
        })
    }

    fn cancel<'a>(&'a self, cancellation: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.note(format!("cancel {cancellation}")) })
    }

    fn registered(&self) -> BoxFuture<'_, Result<bool, Fault>> {
        Box::pin(async move {
            self.note("registered".to_owned());
            self.registered.clone()
        })
    }
}

fn prompt_over(fake: &Arc<Fake>) -> PolkitPrompt {
    PolkitPrompt::over(Arc::new(FakeBus(Arc::clone(fake))))
}

fn text() -> PromptText {
    PromptText {
        title: "Approve a write by `example_orders_item_upsert`".to_owned(),
        detail: "Change 0123456789ab".to_owned(),
    }
}

async fn ask(fake: &Arc<Fake>) -> PromptAnswer {
    within(
        "the ask",
        prompt_over(fake).ask(text(), Instant::now() + FAR),
    )
    .await
}

#[test]
fn only_one_answer_of_polkit_is_a_persons_presence_and_no_error_is() {
    // After the prompt: of every verdict and every fault, exactly one is
    // presence. An error is never an approval.
    for answer in every_answer() {
        let got = answered(answer.clone());
        let present = got == PromptAnswer::Present(PromptName::Polkit);
        assert_eq!(present, answer == Ok(PASSED), "{answer:?} gave {got:?}");
        assert_ne!(got, PromptAnswer::Confirmed, "{answer:?}");
    }
    // Each of the others has its name.
    let named = |answer| reason(&answered(answer));
    assert_eq!(
        answered(Ok(verdict(false, false, true))),
        PromptAnswer::Dismissed
    );
    assert_eq!(
        answered(Ok(verdict(false, false, false))),
        PromptAnswer::Denied
    );
    assert_eq!(answered(Err(Fault::Cancelled)), PromptAnswer::Dismissed);
    for no_agent in [verdict(false, true, false), verdict(false, true, true)] {
        assert_eq!(named(Ok(no_agent)), Some(PresenceReason::NoAgent));
    }
    // A yes that also says it is not one is not understood, and so not a
    // yes.
    for odd in [
        verdict(true, true, false),
        verdict(true, false, true),
        verdict(true, true, true),
    ] {
        assert_eq!(named(Ok(odd)), Some(PresenceReason::CheckFailed), "{odd:?}");
    }
    assert_eq!(
        named(Err(Fault::NoSystemBus)),
        Some(PresenceReason::NoSystemBus)
    );
    assert_eq!(named(Err(Fault::NoPolkit)), Some(PresenceReason::NoPolkit));
    assert_eq!(
        named(Err(Fault::NotRegistered)),
        Some(PresenceReason::PolicyNotInstalled)
    );
    for failed in [Fault::Refused, Fault::Other("x".to_owned())] {
        assert_eq!(named(Err(failed)), Some(PresenceReason::CheckFailed));
    }

    // Before the prompt: only "would have to ask" lets a prompt be
    // raised. A yes that nobody was asked for proves nobody, and a plain
    // no means the engine is outside a local, active session.
    for answer in every_answer() {
        let may_ask = standing(answer.clone()).is_ok();
        let would_ask = matches!(answer, Ok(v) if !v.authorized && v.challenge);
        assert_eq!(may_ask, would_ask, "{answer:?}");
    }
    let why = |answer| standing(answer).unwrap_err().reason;
    for granted in [PASSED, verdict(true, true, false)] {
        assert_eq!(why(Ok(granted)), PresenceReason::GrantedWithoutAsking);
    }
    assert_eq!(
        why(Ok(verdict(false, false, false))),
        PresenceReason::SessionNotActive
    );
    assert_eq!(
        why(Err(Fault::NotRegistered)),
        PresenceReason::PolicyNotInstalled
    );

    // What an administrator is told to do names the file and its place.
    let missing = unavailable(&Fault::NotRegistered);
    assert!(missing.message.contains(ACTION_ID), "{missing:?}");
    assert!(missing.remedy.contains(POLICY_FILE), "{missing:?}");
    assert!(
        missing.remedy.contains("/usr/share/polkit-1/actions"),
        "{missing:?}"
    );
}

#[test]
fn an_error_of_the_bus_is_named_by_its_name_and_shows_nothing_else() {
    let named = |name| fault_named(Some(name));
    assert_eq!(
        named("org.freedesktop.DBus.Error.ServiceUnknown"),
        Fault::NoPolkit
    );
    assert_eq!(
        named("org.freedesktop.DBus.Error.NameHasNoOwner"),
        Fault::NoPolkit
    );
    assert_eq!(
        named("org.freedesktop.DBus.Error.Spawn.ChildExited"),
        Fault::NoPolkit
    );
    assert_eq!(named("org.freedesktop.systemd1.Masked"), Fault::NoPolkit);
    assert_eq!(
        named("org.freedesktop.PolicyKit1.Error.Failed"),
        Fault::Refused
    );
    assert_eq!(
        named("org.freedesktop.PolicyKit1.Error.Cancelled"),
        Fault::Cancelled
    );
    // polkit's "not authorized" and every name the engine has not heard
    // of are failures of the check, never an answer.
    for other in [
        "org.freedesktop.PolicyKit1.Error.NotAuthorized",
        "org.freedesktop.PolicyKit1.Error.CancellationIdNotUnique",
        "org.freedesktop.DBus.Error.AccessDenied",
        "org.freedesktop.DBus.Error.NoReply",
    ] {
        assert_eq!(named(other), Fault::Other(other.to_owned()));
    }
    // Only the plain characters of a name are shown, and not many.
    let shown = named("org.example.Error.<b>Odd</b> name\nwith a line");
    assert_eq!(
        shown,
        Fault::Other("org.example.Error.bOddbnamewithaline".to_owned())
    );
    let Fault::Other(long) = named(&"x".repeat(500)) else {
        panic!("a long name is still a name");
    };
    assert_eq!(long.len(), 96);
    assert_eq!(fault_named(None), Fault::Other("bus_connection".to_owned()));
}

#[tokio::test]
async fn the_prompt_is_raised_only_when_polkit_would_ask_and_passes_only_on_its_yes() {
    // Every answer before the prompt, with every answer after it: the
    // prompt is raised only when polkit says it would have to ask, and
    // the answer is presence only when the prompt was raised and passed.
    for before in every_answer() {
        for after in every_answer() {
            let fake = Fake::new(before.clone(), Some(after.clone()));
            let got = ask(&fake).await;
            let would_ask = matches!(before, Ok(v) if !v.authorized && v.challenge);
            assert_eq!(fake.raised(), would_ask, "{before:?}: {:?}", fake.calls());
            let present = got == PromptAnswer::Present(PromptName::Polkit);
            assert_eq!(
                present,
                would_ask && after == Ok(PASSED),
                "{before:?} then {after:?} gave {got:?}"
            );
            if would_ask {
                assert_eq!(got, answered(after.clone()));
            } else {
                assert!(reason(&got).is_some(), "{before:?} gave {got:?}");
            }
        }
    }

    // No bus: nothing more is asked.
    for fault in every_fault() {
        let fake = Arc::new(Fake {
            bus: Err(fault.clone()),
            ..Arc::into_inner(Fake::new(Ok(WOULD_ASK), Some(Ok(PASSED)))).unwrap()
        });
        let got = ask(&fake).await;
        assert_eq!(reason(&got), Some(unavailable(&fault).reason));
        assert_eq!(fake.calls(), ["connect"]);
    }

    // polkit refuses a check for an action it does not know. Its list of
    // actions tells the missing policy file from any other refusal.
    for (registered, expected) in [
        (Ok(false), PresenceReason::PolicyNotInstalled),
        (Ok(true), PresenceReason::CheckFailed),
        (Err(Fault::NoPolkit), PresenceReason::NoPolkit),
    ] {
        let fake = Arc::new(Fake {
            registered,
            ..Arc::into_inner(Fake::new(Err(Fault::Refused), Some(Ok(PASSED)))).unwrap()
        });
        assert_eq!(reason(&ask(&fake).await), Some(expected));
        assert_eq!(fake.calls(), ["connect", "probe", "registered"]);
        // What it says it can do is the same, and raises nothing either.
        let available = within("the availability", prompt_over(&fake).available()).await;
        assert_eq!(available.reason, expected);
        assert!(!available.proves_presence());
    }

    // Ready: an operating system's prompt, by name, with nothing to do.
    let fake = Fake::new(Ok(WOULD_ASK), Some(Ok(PASSED)));
    let ready = within("the availability", prompt_over(&fake).available()).await;
    assert_eq!(
        (ready.kind, ready.prompt, ready.reason),
        (
            PresenceKind::OsPrompt,
            Some(PromptName::Polkit),
            PresenceReason::Ready
        )
    );
    assert!(ready.remedy.is_empty());
    assert_eq!(fake.calls(), ["connect", "probe"]);
}

#[tokio::test]
async fn the_deadline_cancels_the_dialog_and_the_answer_is_a_dismissal() {
    // The dialog is up and nobody answers it.
    let fake = Fake::new(Ok(WOULD_ASK), None);
    let prompt = prompt_over(&fake);
    let deadline = Instant::now() + Duration::from_millis(300);
    let got = within("the ask", prompt.ask(text(), deadline)).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(Instant::now() >= deadline);
    // The check that was cancelled is the one that was raised.
    assert_eq!(
        fake.calls(),
        [
            "connect",
            "probe",
            "ask anything2mcp-presence-0",
            "cancel anything2mcp-presence-0"
        ]
    );

    // The next ask has an id of its own.
    let soon = Instant::now() + Duration::from_millis(50);
    let got = within("the second ask", prompt.ask(text(), soon)).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(
        fake.calls()
            .contains(&"cancel anything2mcp-presence-1".to_owned())
    );

    // At or after the deadline nothing is raised at all, even where the
    // person would pass.
    let fake = Fake::new(Ok(WOULD_ASK), Some(Ok(PASSED)));
    let got = prompt_over(&fake).ask(text(), Instant::now()).await;
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(!fake.raised(), "{:?}", fake.calls());
}

#[test]
fn the_policy_file_declares_the_action_the_engine_asks_for_with_its_defaults() {
    assert_eq!(POLICY_FILE, format!("{ACTION_ID}.policy"));
    assert_eq!(
        POLICY.matches("<action id=").count(),
        1,
        "one action and no other"
    );
    assert!(POLICY.contains(&format!("<action id=\"{ACTION_ID}\">")));
    for default in [
        "<allow_any>no</allow_any>",
        "<allow_inactive>no</allow_inactive>",
        "<allow_active>auth_self</allow_active>",
    ] {
        assert!(POLICY.contains(default), "{default}");
    }
    // Never a default that keeps the answer, and never one that asks for
    // an administrator: the person proves that it is them.
    for never in ["_keep", "auth_admin", ">yes<"] {
        assert!(!POLICY.contains(never), "{never}");
    }
    // The dialog cannot name the change, so its fixed message sends the
    // person to the page.
    assert!(POLICY.contains("read the page first"));
}

/// The real bus code, on any Linux machine: an address where nothing
/// listens is "no system bus", and nothing is raised.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_system_bus_the_real_prompt_says_so_and_raises_nothing() {
    let nowhere = std::env::temp_dir().join(format!("a2m-no-bus-{}", std::process::id()));
    let bus = SystemBus::at(&format!("unix:path={}", nowhere.display()));
    let prompt = PolkitPrompt::over(Arc::new(bus));
    let available = within("the availability", prompt.available()).await;
    assert_eq!(available.kind, PresenceKind::NotAvailable);
    assert_eq!(available.reason, PresenceReason::NoSystemBus);
    let got = within("the ask", prompt.ask(text(), Instant::now() + FAR)).await;
    assert_eq!(got, PromptAnswer::NotAvailable(available));
}

/// A text agent of polkit for this process, as a child: `pkttyagent` on
/// a terminal of its own, which `script` gives it. It answers the
/// password prompt with what the test tells it.
struct Agent {
    child: Child,
    said: mpsc::Receiver<String>,
    heard: String,
}

impl Agent {
    fn start() -> Self {
        let agent = format!("pkttyagent --process {}", std::process::id());
        let mut child = Command::new("script")
            .args(["-qfec", &agent, "/dev/null"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // A group of its own, so that the whole agent can be ended.
            .process_group(0)
            .spawn()
            .expect("`script` and `pkttyagent` are installed on a prepared machine");
        let stdout = child.stdout.take().unwrap();
        let (send, said) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                // The password prompt ends with no line end, so read what
                // is there and not whole lines.
                let read = match reader.fill_buf() {
                    Ok(bytes) if !bytes.is_empty() => bytes.to_vec(),
                    _ => break,
                };
                reader.consume(read.len());
                if send
                    .send(String::from_utf8_lossy(&read).into_owned())
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            child,
            said,
            heard: String::new(),
        }
    }

    /// Waits until the agent has said `words`.
    fn hears(&mut self, words: &str) {
        if let Err(said) = self.hears_by(words, StdInstant::now() + Duration::from_secs(60)) {
            panic!("the agent never said {words:?}; it said {said:?}");
        }
    }

    /// Waits until the agent has said `words`, or fails with all it said
    /// when its output ends or `deadline` passes.
    fn hears_by(&mut self, words: &str, deadline: StdInstant) -> Result<(), String> {
        while !self.heard.contains(words) {
            let left = deadline.saturating_duration_since(StdInstant::now());
            match self.said.recv_timeout(left) {
                Ok(more) => self.heard.push_str(&more),
                Err(_) => return Err(self.heard.clone()),
            }
        }
        Ok(())
    }

    /// Starts an agent that is registered with polkit and has said
    /// "Password:". polkit allows one agent for this process, and it may
    /// still hold the one before: an agent that is told so exits, and is
    /// replaced once it is gone, until one registers or 30 seconds pass.
    fn started() -> Self {
        let deadline = StdInstant::now() + Duration::from_secs(30);
        loop {
            let mut agent = Self::start();
            match agent.hears_by("Password:", deadline) {
                Ok(()) => return agent,
                Err(said) if said.contains("already exists") => {
                    // Dropping waits for its process to exit.
                    drop(agent);
                    assert!(
                        StdInstant::now() < deadline,
                        "polkit still has the agent before; the last agent said {said:?}"
                    );
                }
                Err(said) => panic!("the agent never said \"Password:\"; it said {said:?}"),
            }
        }
    }

    /// Types `password` at the agent's password prompt.
    fn types(&mut self, password: &str) {
        self.hears("Password:");
        self.heard.clear();
        // The agent prints its prompt and then empties the terminal's
        // input before it reads, so what is typed at once would be lost.
        // A person is slower than that, and so is this.
        std::thread::sleep(Duration::from_secs(1));
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{password}").unwrap();
        stdin.flush().unwrap();
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        // The whole group, so that the text agent's bus connection is
        // closed and polkit has no agent on record for this process.
        if let Some(group) = i32::try_from(self.child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Asks the real prompt until an agent is there to ask through: an agent
/// registers a moment after it starts.
async fn ask_through_agent(prompt: &Arc<PolkitPrompt>, deadline: Instant) -> PromptAnswer {
    within("the agent's registering", async {
        loop {
            let got = prompt.ask(text(), deadline).await;
            if reason(&got) != Some(PresenceReason::NoAgent) {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
}

/// Waits until polkit has let go of the agent that was just stopped. Its
/// process is gone, and polkit learns so a moment later: until then an ask
/// is passed to the agent that is gone and fails at once, which is no
/// answer about the next agent. polkit says "no agent" once it has none.
async fn agent_is_gone(prompt: &Arc<PolkitPrompt>, deadline: Instant) {
    within("polkit's letting go of the agent before", async {
        loop {
            let got = prompt.ask(text(), deadline).await;
            if reason(&got) == Some(PresenceReason::NoAgent) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
}

/// The real prompt against a real polkit on the system bus, on a machine
/// prepared as the variable says. Nothing here needs a person: the
/// password is typed by a text agent the test starts for its own process.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn against_a_real_polkit_the_prompt_answers_as_the_machine_is_prepared() {
    let Ok(prepared) = std::env::var(REAL_VARIABLE) else {
        eprintln!("skipped: {REAL_VARIABLE} is not set, so this machine is not prepared");
        return;
    };
    let prompt = Arc::new(PolkitPrompt::system());
    let available = within("the availability", prompt.available()).await;
    let far = Instant::now() + FAR;
    let expected = match prepared.as_str() {
        // A system bus with no polkit on it.
        "no_polkit" => PresenceReason::NoPolkit,
        // polkit runs; the policy file is not installed.
        "no_policy" => PresenceReason::PolicyNotInstalled,
        // The policy file is installed, and the test's process is in no
        // local, active session, as on any machine with no seat.
        "no_session" => PresenceReason::SessionNotActive,
        // The test runs as the administrator account, which polkit
        // always allows: a yes nobody was asked for.
        "root" => PresenceReason::GrantedWithoutAsking,
        "no_agent" | "ready" => PresenceReason::Ready,
        other => panic!("{REAL_VARIABLE}={other} is not a preparation this test knows"),
    };
    assert_eq!(available.reason, expected, "{available:?}");
    if expected != PresenceReason::Ready {
        // Nothing can be raised, and an ask says the same.
        let got = within("the ask", prompt.ask(text(), far)).await;
        assert_eq!(got, PromptAnswer::NotAvailable(available));
        return;
    }
    assert_eq!(available.prompt, Some(PromptName::Polkit));

    // polkit would ask, and nothing is there to ask through.
    let got = within("the ask", prompt.ask(text(), far)).await;
    assert_eq!(reason(&got), Some(PresenceReason::NoAgent), "{got:?}");
    if prepared == "no_agent" {
        return;
    }

    // A wrong password is not presence.
    let asking = tokio::spawn({
        let prompt = Arc::clone(&prompt);
        async move { ask_through_agent(&prompt, far).await }
    });
    let mut agent = tokio::task::block_in_place(Agent::started);
    tokio::task::block_in_place(|| agent.types("not-the-password-of-the-test-account"));
    let got = within("the wrong password", asking).await.unwrap();
    assert!(
        matches!(got, PromptAnswer::Denied | PromptAnswer::Dismissed),
        "{got:?}"
    );
    drop(agent);
    agent_is_gone(&prompt, far).await;

    // The right one is, and it is polkit that says so.
    let asking = tokio::spawn({
        let prompt = Arc::clone(&prompt);
        async move { ask_through_agent(&prompt, far).await }
    });
    let mut agent = tokio::task::block_in_place(Agent::started);
    tokio::task::block_in_place(|| agent.types(REAL_PASSWORD));
    let got = within("the right password", asking).await.unwrap();
    assert_eq!(got, PromptAnswer::Present(PromptName::Polkit));
    // auth_self keeps nothing: polkit would have to ask again.
    let again = within("the availability", prompt.available()).await;
    assert_eq!(again.reason, PresenceReason::Ready, "{again:?}");

    // A dialog nobody answers is taken away at the deadline, and the
    // answer is a dismissal.
    let asking = tokio::spawn({
        let prompt = Arc::clone(&prompt);
        async move {
            let soon = Instant::now() + Duration::from_secs(5);
            (ask_through_agent(&prompt, soon).await, soon)
        }
    });
    tokio::task::block_in_place(|| agent.hears("Password:"));
    let (got, soon) = within("the deadline", asking).await.unwrap();
    assert_eq!(got, PromptAnswer::Dismissed);
    assert!(Instant::now() >= soon);
    // The text agent may still wait at its terminal for the password of
    // the check that is gone, so the next ask gets an agent of its own.
    // polkit holds nothing against this process from the cancelled check:
    // the next prompt is raised and passed.
    drop(agent);
    agent_is_gone(&prompt, far).await;
    let asking = tokio::spawn({
        let prompt = Arc::clone(&prompt);
        async move { ask_through_agent(&prompt, far).await }
    });
    let mut agent = tokio::task::block_in_place(Agent::started);
    tokio::task::block_in_place(|| agent.types(REAL_PASSWORD));
    let got = within("the ask after a cancelled one", asking)
        .await
        .unwrap();
    assert_eq!(got, PromptAnswer::Present(PromptName::Polkit));
}

/// What an [`Availability`] must always hold, whatever made it.
#[test]
fn an_availability_proves_presence_only_as_a_ready_prompt_of_the_system() {
    let ready = Availability::ready(PromptName::Polkit, "Ready.");
    assert!(ready.proves_presence());
    for fault in every_fault() {
        assert!(!unavailable(&fault).proves_presence(), "{fault:?}");
    }
}
