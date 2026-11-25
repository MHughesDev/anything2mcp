//! The shortcut `learn` through the command face (package LS1; decision
//! 0456, point 2): it is `capture.start` alone. It asks for a capture of
//! the person's own use and answers with the page where the person begins
//! it; the capture then waits for its person, and is not ended by the
//! shortcut that asked for it. The agent ends it afterwards with `capture
//! stop`, which says how it ended.
//!
//! No browser is needed: nothing records until a person begins the capture
//! on its page, and this test's person never does.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Paths, PersonActAsk, PersonSurface, PresenceAsk,
    PresenceProof, SessionAsk, SignInAsk, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::Failure;
use a2m_policy::PresenceCheck;
use a2m_telemetry::{Clock, SystemClock};
use anything2mcp::cli::run_on;
use anything2mcp::{Engine, EngineOptions};
use common::Machine;
use serde_json::Value as Json;

/// A person surface that shows the engine's pages for a person's act, and
/// keeps each act it was asked to show.
struct Pages {
    acts: Mutex<Vec<PersonActAsk>>,
    clock: Arc<dyn Clock>,
}

impl PersonSurface for Pages {
    fn take_desk(&self, _desk: ApprovalDesk) {}

    fn ask_approval(&self, _ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: String::new(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async {
            Ok(SurfaceLink {
                url: None,
                text: String::new(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            Ok(PresenceProof::new(
                PresenceCheck::OsPrompt,
                ask.principal,
                self.clock.now(),
            ))
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async { PersonAnswer::NoAnswer })
    }

    fn shows_person_acts(&self) -> bool {
        true
    }

    fn ask_person_act(&self, ask: PersonActAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let link = format!("http://127.0.0.1/act/{}", ask.act);
            self.acts.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(link),
                text: "Open the page to carry it out.".to_owned(),
            })
        })
    }
}

/// One command line, as an agent runs it, read as one JSON object.
async fn agent(engine: &Engine, args: &[&str]) -> (u8, Json) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let finished = run_on(engine, &args, true, true).await;
    let json = serde_json::from_str(finished.stdout.trim())
        .unwrap_or_else(|_| panic!("not one JSON object: {finished:?}"));
    (finished.code, json)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn learn_asks_for_a_capture_and_leaves_it_waiting_for_its_person() {
    let machine = Machine::new("pages", &[]);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let pages = Arc::new(Pages {
        acts: Mutex::new(Vec::new()),
        clock: Arc::clone(&clock),
    });
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Arc::clone(&pages) as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    options.place = machine.place;
    options.key_store.clone_from(&machine.key_store);
    options.clock = clock;
    let engine = Engine::open(options).await.expect("the settings load");

    // `learn` runs one step, `capture.start`: it answers with the page
    // where the person begins the capture, and waits for them.
    let (_, learned) = agent(&engine, &["learn", "example"]).await;
    let result = &learned["result"];
    assert_eq!(result["state"], "asked", "{learned}");
    let id = result["capture_id"].as_str().unwrap().to_owned();
    let link = result["link"].as_str().unwrap().to_owned();
    assert!(link.starts_with("http://127.0.0.1/act/"), "{learned}");
    assert!(learned["error"].is_null(), "{learned}");
    {
        let acts = pages.acts.lock().unwrap();
        assert_eq!(acts.len(), 1, "one page was asked for");
    }
    // Its next step is the agent's own: `capture.stop`, once the person
    // has used the system.
    assert!(
        learned["next"].to_string().contains("capture.stop"),
        "{learned}"
    );

    // The capture was not ended by the shortcut: asked for again, it is
    // given back as it is, still waiting for its person.
    let (_, again) = agent(&engine, &["capture", "start", "example"]).await;
    assert_eq!(again["result"]["capture_id"], id.as_str(), "{again}");
    assert_eq!(again["result"]["state"], "asked", "{again}");

    // The agent ends it. Its person never began it, so nothing was
    // recorded, and it says so.
    let (code, stopped) = agent(&engine, &["capture", "stop", "example"]).await;
    assert_eq!(code, 0, "{stopped}");
    assert_eq!(stopped["result"]["capture_id"], id.as_str(), "{stopped}");
    assert_eq!(stopped["result"]["state"], "ended", "{stopped}");
    assert_eq!(stopped["result"]["ended_by"], "not_begun", "{stopped}");
    assert_eq!(stopped["result"]["findings"], 0, "{stopped}");
    engine.close().await;
}
