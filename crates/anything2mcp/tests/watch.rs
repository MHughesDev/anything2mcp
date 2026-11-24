//! Watches, through the real command line, against the test application
//! (decisions 0234, 0262, 0283): an agent asks, its person confirms on the
//! engine's page, and each check is a read whose result is compared with
//! what earlier checks saw. The stand's record is changed by the test
//! itself, through the stand's own interface and outside the product, and
//! the stand is asked what it stored. The engine's clock is moved by hand,
//! and the test tends the engine itself after each move.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Paths, PersonActAsk, PersonSurface, PresenceAsk,
    PresenceProof, SessionAsk, SignInAsk, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, Timestamp};
use a2m_policy::PresenceCheck;
use a2m_telemetry::{Clock, ManualClock, SystemClock};
use anything2mcp::{Engine, EngineOptions};
use common::{Machine, World};
use example_app::harness::TestApp;
use serde_json::{Value as Json, json};

const PLACEHOLDER: &str = "http://127.0.0.1:4010";
const ITEM_GET: &str = "example_line_item_get";

/// How many passes of tending a test waits through for a check to end
/// before it fails by name: thirty seconds of the engine's clock, half an
/// interval, so that no check of the next due time is waited for.
const PASSES: u32 = 300;

/// A person surface with a page for a person's act, as the engine's own
/// page is: it keeps what each page shows, and the test presses there.
struct Pages {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    acts: Mutex<Vec<PersonActAsk>>,
    clock: Arc<dyn Clock>,
}

impl PersonSurface for Pages {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(Arc::new(desk));
    }

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

/// An engine on `machine`, as the product opens one, with the page surface
/// and a clock the test moves.
struct Watching {
    world: World,
    pages: Arc<Pages>,
    clock: Arc<ManualClock>,
}

async fn open(machine: &Machine, starting: Timestamp) -> Watching {
    let clock = Arc::new(ManualClock::starting_at(starting));
    let pages = Arc::new(Pages {
        desk: Mutex::new(None),
        acts: Mutex::new(Vec::new()),
        clock: Arc::clone(&clock) as Arc<dyn Clock>,
    });
    let mut options = EngineOptions::of_machine(
        Paths::under(machine.home.path()),
        machine.environment(),
        Arc::clone(&pages) as Arc<dyn PersonSurface>,
    );
    options.project = Some(machine.project.path().to_path_buf());
    options.place = machine.place;
    options.key_store.clone_from(&machine.key_store);
    options.clock = Arc::clone(&clock) as Arc<dyn Clock>;
    let engine = Engine::open(options).await.expect("the settings load");
    assert!(
        engine.runner_problem().is_none(),
        "{:?}",
        engine.runner_problem()
    );
    Watching {
        world: World {
            engine,
            surface: Arc::clone(&machine.surface),
            clock: Arc::clone(&clock) as Arc<dyn Clock>,
        },
        pages,
        clock,
    }
}

fn now() -> Timestamp {
    SystemClock::new().now()
}

impl Watching {
    /// An agent asks for a watch of one line item, every minute, ending
    /// `lasts` from now; its person confirms it on the page. Gives its
    /// identifier.
    async fn watched(&self, item_code: &str, lasts: Duration) -> String {
        self.watched_with(item_code, lasts, &[]).await
    }

    /// The same, with more arguments of `watch create`.
    async fn watched_with(&self, item_code: &str, lasts: Duration, more: &[&str]) -> String {
        let input = json!({"order_id": "A-100", "item_code": item_code}).to_string();
        let ends_at = (self.clock.now().as_unix_millis()
            + i64::try_from(lasts.as_millis()).unwrap())
        .to_string();
        let mut args = vec![
            "watch",
            "create",
            ITEM_GET,
            "--input",
            &input,
            "--interval-seconds",
            "60",
            "--ends-at",
            &ends_at,
        ];
        args.extend_from_slice(more);
        let asked = self.world.agent(&args).await;
        let result = asked.result();
        assert_eq!(result["state"], "asked", "{:?}", asked.finished);
        let id = result["watch_id"].as_str().unwrap().to_owned();
        // Nothing runs until the person presses on the page.
        assert_eq!(self.watch(&id).await["checks"], 0);
        let ask = self
            .pages
            .acts
            .lock()
            .unwrap()
            .iter()
            .find(|ask| {
                ask.action.as_str() == "watch.confirm"
                    && ask.input.iter().any(|field| field.value == id)
            })
            .cloned()
            .expect("the person was shown the confirmation");
        let person = self
            .world
            .engine
            .caller(a2m_actions::FrontDoor::Command, false)
            .principal
            .principal()
            .id;
        let proof =
            PresenceProof::new(PresenceCheck::OsPrompt, person, self.clock.now()).for_act(ask.act);
        let desk = self.pages.desk.lock().unwrap().clone().unwrap();
        let done = desk.carry_out(ask.act, proof).await.unwrap();
        assert!(done.ok, "{done:?}");
        id
    }

    async fn watch(&self, id: &str) -> Json {
        self.world.ok(&["watch", "get", id]).await
    }

    async fn changes(&self, id: &str) -> Vec<Json> {
        self.world.ok(&["watch", "changes", id]).await["changes"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// Moves the clock by `by` and tends until every watch of `ids` has
    /// made `checks` checks and waits for the next. Between two passes the
    /// clock moves on by a tenth of a second, as it would while a check's
    /// requests wait for their budget, which refills by the store's clock.
    async fn checked(&self, by: Duration, ids: &[&str], checks: u64) {
        self.clock.advance(by);
        for pass in 0..PASSES {
            if pass > 0 {
                self.clock.advance(Duration::from_millis(100));
            }
            self.world.engine.kernel().tend().await;
            let mut done = true;
            for id in ids {
                let watch = self.watch(id).await;
                done &= watch["checks"] == checks && watch["state"] == "confirmed";
            }
            if done {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut stood = Vec::new();
        for id in ids {
            stood.push(self.watch(id).await);
        }
        panic!("the checks never ended: {stood:?}");
    }
}

fn kinds(changes: &[Json]) -> Vec<&str> {
    changes
        .iter()
        .map(|change| change["kind"].as_str().unwrap())
        .collect()
}

/// A machine with the `orders` project over `app`. Its surface declares
/// two hundred requests a second, so that the share all watches on it may
/// take, a tenth, leaves room for two checks at once on a clock that moves
/// only when the test moves it.
fn machine_over(app: &TestApp) -> Machine {
    Machine::new(
        "orders",
        &[
            (PLACEHOLDER, &app.url("")),
            (
                "    kind: http\n",
                "    kind: http\n    rate: {requests: 200, per_ms: 1000}\n",
            ),
        ],
    )
}

/// An engine on `machine` with its drafts published by its person.
async fn published(app: &TestApp, machine: &Machine) -> Watching {
    let watching = open(machine, now()).await;
    let done = watching
        .world
        .person(&["version", "publish", "example"])
        .await;
    assert_eq!(done.finished.code, 0, "{:?}", done.finished);
    let _ = app;
    watching
}

/// The stand's own interface changes how many of one line item there are:
/// a change made outside the product, as a person in the application would.
async fn changed_by_the_stand(app: &TestApp, item_code: &str, quantity: i64) {
    let stored = app.stored_item("A-100", item_code).await.unwrap().unwrap();
    let sent = app
        .http()
        .put(app.url(&format!("/api/orders/A-100/items/{item_code}")))
        .json(&json!({
            "description": stored.description,
            "quantity": quantity,
            // Whole sixteenths as exact inches: a sixteenth is 0.0625.
            "length_in": format!(
                "{}.{:04}",
                stored.length_sixteenths / 16,
                (stored.length_sixteenths % 16) * 625
            ),
            "version": stored.version,
        }))
        .send()
        .await
        .unwrap();
    assert!(sent.status().is_success(), "{sent:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_watch_reports_the_record_the_stand_changed_and_no_other() {
    let app = TestApp::start().await.unwrap();
    let machine = machine_over(&app);
    let watching = published(&app, &machine).await;
    let hour = Duration::from_secs(3_600);
    let left = watching.watched("L-101", hour).await;
    let right = watching.watched("L-102", hour).await;

    // The first check of each, within a tenth of the interval, is what the
    // next ones are compared with.
    watching
        .checked(Duration::from_secs(10), &[&left, &right], 1)
        .await;

    // The stand's record changes, outside the product.
    changed_by_the_stand(&app, "L-101", 5).await;
    assert_eq!(app.writes().await.unwrap().len(), 1);

    // The next check of each: the one watch reports the one output that
    // changed, and the other reports nothing.
    watching
        .checked(Duration::from_secs(60), &[&left, &right], 2)
        .await;
    let on_left = watching.changes(&left).await;
    assert_eq!(kinds(&on_left), ["confirmed", "changed"], "{on_left:?}");
    let changed = &on_left[1];
    assert_eq!(changed["outputs"], json!(["quantity"]), "{changed}");
    assert_eq!(changed["check"], 1);
    assert_eq!(changed["older_check"], 0);
    // The change holds no value; the check's run holds the read's.
    assert!(!changed.to_string().contains("rail"), "{changed}");
    let run = changed["run_id"].as_str().unwrap();
    let read = watching.world.ok(&["run", "get", run]).await;
    assert_eq!(read["result"]["quantity"], 5, "{read}");
    let on_right = watching.changes(&right).await;
    assert_eq!(kinds(&on_right), ["confirmed"], "{on_right:?}");

    // The product wrote nothing: the one write the stand holds is the
    // test's own.
    assert_eq!(app.writes().await.unwrap().len(), 1);
    watching.world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_watch_ends_at_the_first_check_on_which_its_condition_holds_of_what_the_stand_changed() {
    let app = TestApp::start().await.unwrap();
    let machine = machine_over(&app);
    let watching = published(&app, &machine).await;
    let id = watching
        .watched_with(
            "L-101",
            Duration::from_secs(3_600),
            &["--condition", "result.quantity >= 9"],
        )
        .await;
    assert_eq!(
        watching.watch(&id).await["condition"],
        "result.quantity >= 9"
    );
    // Two rails: the first check makes the baseline, and the condition does
    // not hold.
    watching.checked(Duration::from_secs(10), &[&id], 1).await;

    // The stand's record changes, outside the product, to nine.
    changed_by_the_stand(&app, "L-101", 9).await;
    watching.clock.advance(Duration::from_secs(60));
    let mut ended = Json::Null;
    for pass in 0..PASSES {
        if pass > 0 {
            watching.clock.advance(Duration::from_millis(100));
        }
        watching.world.engine.kernel().tend().await;
        ended = watching.watch(&id).await;
        if ended["state"] == "ended" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(ended["state"], "ended", "{ended}");
    assert_eq!(ended["reason"], "condition_met", "{ended}");
    assert_eq!(ended["checks"], 2, "{ended}");
    // The check that found it records the change it saw, then that the
    // condition held, then the end; the run it names read nine.
    let changes = watching.changes(&id).await;
    assert_eq!(
        kinds(&changes),
        ["confirmed", "changed", "condition_met", "ended"],
        "{changes:?}"
    );
    assert_eq!(changes[2]["check"], 1, "{changes:?}");
    assert_eq!(changes[3]["reason"], "condition_met", "{changes:?}");
    let run = changes[2]["run_id"].as_str().unwrap();
    let read = watching.world.ok(&["run", "get", run]).await;
    assert_eq!(read["result"]["quantity"], 9, "{read}");

    // Ended, it reads nothing more, and the product wrote nothing: the one
    // write the stand holds is the test's own.
    let reads = app.seen_all().await.unwrap().len();
    watching.clock.advance(Duration::from_secs(600));
    watching.world.engine.kernel().tend().await;
    assert_eq!(app.seen_all().await.unwrap().len(), reads);
    assert_eq!(watching.watch(&id).await["checks"], 2);
    assert_eq!(app.writes().await.unwrap().len(), 1);
    watching.world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_checks_record_no_change() {
    let app = TestApp::start().await.unwrap();
    let machine = machine_over(&app);
    let watching = published(&app, &machine).await;
    let id = watching.watched("L-103", Duration::from_secs(3_600)).await;
    watching.checked(Duration::from_secs(10), &[&id], 1).await;
    for checks in 2..=10 {
        watching
            .checked(Duration::from_secs(60), &[&id], checks)
            .await;
    }
    // Ten checks of a record nothing changed: nothing but the confirmation.
    let changes = watching.changes(&id).await;
    assert_eq!(kinds(&changes), ["confirmed"], "{changes:?}");
    assert_eq!(watching.watch(&id).await["checks"], 10);
    assert!(app.writes().await.unwrap().is_empty());
    watching.world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missed_due_time_is_a_gap_and_is_not_made_up() {
    let app = TestApp::start().await.unwrap();
    let machine = machine_over(&app).on_disk();
    let watching = published(&app, &machine).await;
    let id = watching.watched("L-101", Duration::from_secs(3_600)).await;
    watching.checked(Duration::from_secs(10), &[&id], 1).await;
    let stopped_at = watching.clock.now();
    watching.world.close().await;

    // The engine is started again three and a half intervals later: three
    // due times passed while no engine ran. The two before the latest are
    // one gap, and one check is made for the latest.
    let later = Timestamp::from_unix_millis(stopped_at.as_unix_millis() + 210_000);
    let watching = open(&machine, later).await;
    watching.checked(Duration::ZERO, &[&id], 2).await;
    let changes = watching.changes(&id).await;
    assert_eq!(kinds(&changes), ["confirmed", "gap"], "{changes:?}");
    let gap = &changes[1];
    assert_eq!(gap["failure"]["code"], "watch_due_missed", "{gap}");
    assert_eq!(gap["missed"]["count"], 2, "{gap}");
    // Nothing more is made up: tending again checks nothing until the next
    // due time.
    watching.world.engine.kernel().tend().await;
    assert_eq!(watching.watch(&id).await["checks"], 2);
    watching.world.close().await;
}

/// Decision 0262 holds that a removal is never reported until a read can say
/// it returned every record. Today no compiled target declares a list of
/// records with a key (decision 0283), so every watch compares its read
/// whole, and no check can report a record removed or out of view: what the
/// stand changed is `changed`, of the whole. The half of decision 0234's test
/// that a full list reports `removed` is not built: it needs a declared
/// record key and the engine's completeness flag. The ledger's own test holds
/// the rule for keyed records.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_cut_short_never_reports_a_removal() {
    let app = TestApp::start().await.unwrap();
    let machine = machine_over(&app);
    let watching = published(&app, &machine).await;
    let id = watching.watched("L-102", Duration::from_secs(3_600)).await;
    assert_eq!(watching.watch(&id).await["compared"], "whole");
    watching.checked(Duration::from_secs(10), &[&id], 1).await;
    changed_by_the_stand(&app, "L-102", 7).await;
    watching.checked(Duration::from_secs(60), &[&id], 2).await;
    watching.checked(Duration::from_secs(60), &[&id], 3).await;
    let changes = watching.changes(&id).await;
    assert_eq!(kinds(&changes), ["confirmed", "changed"], "{changes:?}");
    watching.world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_watch_past_its_end_lets_an_idle_engine_stop() {
    let app = TestApp::start().await.unwrap();
    let machine = machine_over(&app);
    let watching = published(&app, &machine).await;
    let kernel = watching.world.engine.kernel().clone();
    assert!(!kernel.watches_hold_the_engine().await.unwrap());

    // A watch that lasts five minutes, and another that its person paused.
    let short = watching.watched("L-101", Duration::from_secs(300)).await;
    let paused = watching.watched("L-102", Duration::from_secs(3_600)).await;
    let pause = watching.world.agent(&["watch", "pause", &paused]).await;
    assert_eq!(pause.result()["state"], "paused", "{:?}", pause.finished);
    // A confirmed watch has checks to make: the engine is not idle.
    assert!(kernel.watches_hold_the_engine().await.unwrap());
    watching
        .checked(Duration::from_secs(10), &[&short], 1)
        .await;
    assert!(kernel.watches_hold_the_engine().await.unwrap());

    // Past its end it ends, and a paused watch holds nothing: an engine
    // with nothing else in hand may stop when it is idle.
    watching.clock.advance(Duration::from_secs(300));
    kernel.tend().await;
    let ended = watching.watch(&short).await;
    assert_eq!(ended["state"], "ended", "{ended}");
    assert_eq!(ended["reason"], "end_reached", "{ended}");
    assert!(!kernel.watches_hold_the_engine().await.unwrap());
    assert_eq!(watching.watch(&paused).await["state"], "paused");
    watching.world.close().await;
}
