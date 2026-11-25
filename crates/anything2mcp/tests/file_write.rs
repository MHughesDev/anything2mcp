//! Package H3, writes through the file pack, through the product: a write
//! of a file is recorded first, made by putting a new file in the old one's
//! place in one step, and read back by opening the file again. A process
//! ended after the new file was in place is reconciled on resume and the
//! file is never written a second time. A file changed by hand after the
//! person saw the preview makes the approval stale and nothing is written.
//! A name that leaves the folder, or a link, touches nothing. Every claim
//! about what is on disk is asked of the stand's own files, never of the
//! pack's read path alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2m_actions::PresenceProof;
use a2m_core::{RunId, Timestamp};
use a2m_logs::{Clock, ManualClock, SystemClock};
use a2m_policy::{Channel, PresenceCheck};
use common::{Folder, Machine, World};
use example_files::faults::{Change, Fault};
use example_files::harness::FileStand;
use example_files::seed;
use serde_json::Value as Json;

const SET: &str = "example_stock_set";
const NOTE: &str = "example_inbox_set";
const ITEM: &str = "EX-1001";
/// The row of the item in the seed, and the same with the quantity set.
const BEFORE: &str = "EX-1001,Left rail,12,";
const AFTER: &str = "EX-1001,Left rail,30,";
/// How long a test waits for a process to reach its step. Only a failure
/// waits this long.
const PATIENCE: Duration = Duration::from_secs(180);

fn machine(stand: &FileStand) -> Machine {
    Machine::in_folder("stockwrite", &[], Folder::of_another(stand.root())).with_passphrase()
}

async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(
        published.finished.code, 0,
        "{:?} {}",
        published.finished, published.json
    );
    world
}

/// The person approves, at the desk, the diff they were shown.
async fn approve(world: &World, run: &str) {
    let run_id: RunId = run.parse().unwrap();
    let asked = world.surface.last_ask();
    assert_eq!(asked.run, run_id);
    let person = world
        .engine
        .caller(a2m_actions::FrontDoor::Command, false)
        .principal
        .principal()
        .clone();
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, world.clock.now());
    world
        .surface
        .desk()
        .approve(run_id, asked.diff_hash, person, Channel::LocalPrompt, proof)
        .await
        .unwrap();
}

/// A write that rested for approval, and the person approved it.
async fn approved(world: &World, tool: &str, input: &str) -> String {
    let rested = world.agent(&["tool", "call", tool, "--input", input]).await;
    assert_eq!(
        rested.result()["status"],
        "needs_approval",
        "{}",
        rested.json
    );
    let run = rested.result()["run_id"].as_str().unwrap().to_owned();
    approve(world, &run).await;
    run
}

fn set_input(quantity: i64) -> String {
    format!(r#"{{"item_code": "{ITEM}", "on_hand": {quantity}}}"#)
}

fn kinds(trace: &Json) -> Vec<&str> {
    trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect()
}

/// The seed bytes of a file with the first occurrence of `find` replaced.
fn seed_with(name: &str, find: &str, replace: &str) -> Vec<u8> {
    let bytes = seed::files()
        .into_iter()
        .find(|file| file.name == name)
        .unwrap()
        .bytes;
    let at = bytes
        .windows(find.len())
        .position(|window| window == find.as_bytes())
        .unwrap();
    [&bytes[..at], replace.as_bytes(), &bytes[at + find.len()..]].concat()
}

/// What the stand's file holds, read from the disk directly.
fn on_disk(stand: &FileStand, name: &str) -> Vec<u8> {
    std::fs::read(stand.path(name).unwrap()).unwrap()
}

/// Every file the stand holds below its root, other than the project's own
/// files that the machine put there, with its bytes.
fn held(stand: &FileStand) -> Vec<(String, Vec<u8>)> {
    stand
        .listing()
        .unwrap()
        .into_iter()
        .filter(|name| name.starts_with("prices/") || name.starts_with("inbox"))
        .filter_map(|name| stand.bytes(&name).unwrap().map(|bytes| (name, bytes)))
        .collect()
}

/// The files the stand calls strays, other than the project's own files
/// that the machine put in the same folder.
fn strays(stand: &FileStand) -> Vec<String> {
    stand
        .strays()
        .unwrap()
        .into_iter()
        .filter(|name| name != "anything2mcp.toml" && !name.starts_with("targets/"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_is_approved_made_once_by_a_replacing_file_and_read_back_from_disk() {
    let stand = FileStand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    let before = on_disk(&stand, seed::STOCK_CSV);
    let identity = stand.inspect(seed::STOCK_CSV).unwrap();

    let run = approved(&world, SET, &set_input(30)).await;
    // Nothing was written before the person approved.
    assert_eq!(on_disk(&stand, seed::STOCK_CSV), before);
    assert_eq!(
        stand.write_count(seed::STOCK_CSV).unwrap().content_changes,
        0
    );
    let done = world.agent(&["run", "resume", &run]).await;
    assert_eq!(done.result()["status"], "completed", "{}", done.json);

    // The stand's own files: the file holds the new value and every other
    // byte as it was, and the old file was put out of its place in one step.
    assert_eq!(
        on_disk(&stand, seed::STOCK_CSV),
        seed_with(seed::STOCK_CSV, BEFORE, AFTER)
    );
    let count = stand.write_count(seed::STOCK_CSV).unwrap();
    assert_eq!(count.content_changes, 1, "{count:?}");
    if cfg!(unix) {
        assert_eq!(count.replacements, Some(1), "{count:?}");
        assert_ne!(
            stand.inspect(seed::STOCK_CSV).unwrap().identity,
            identity.identity
        );
    }
    assert_eq!(count.disappearances, 0, "{count:?}");
    // No temporary file stays beside it.
    assert_eq!(strays(&stand), Vec::<String>::new());
    assert!(stand.outside_untouched().unwrap());

    let trace = world.ok(&["trace", "show", &run]).await;
    let kinds = kinds(&trace);
    let at = |kind: &str| {
        kinds
            .iter()
            .position(|found| *found == kind)
            .unwrap_or_else(|| panic!("no {kind} in {kinds:?}"))
    };
    assert!(at("approval_bound") < at("intent"));
    assert!(at("intent") < at("acted"));
    assert!(at("acted") < at("verified"), "{kinds:?}");
    // Resumed again, the run changes nothing.
    let again = world.agent(&["run", "resume", &run]).await;
    assert_eq!(again.result()["status"], "completed", "{}", again.json);
    assert_eq!(
        stand.write_count(seed::STOCK_CSV).unwrap().content_changes,
        1
    );
    world.close().await;
}

/// One try at the kill: the real program resumes an approved write, and is
/// ended at the moment the new file is in its place. `None` when the program
/// was quicker than the kill and the run had ended by then.
async fn killed_after_the_replace(stand: &FileStand) -> Option<(Machine, String)> {
    let machine = machine(stand);
    let world = published(&machine).await;
    let run = approved(&world, SET, &set_input(30)).await;
    world.close().await;

    let path = stand.path(seed::STOCK_CSV).unwrap();
    let old = std::fs::read(&path).unwrap();
    let mut program = machine.program(&["run", "resume", &run]).spawn().unwrap();
    // Watch the file itself and end the program the moment it changes.
    let watched = tokio::task::spawn_blocking(move || {
        let give_up = Instant::now() + PATIENCE;
        let changed = loop {
            if std::fs::read(&path).is_ok_and(|now| now != old) {
                break true;
            }
            if Instant::now() >= give_up {
                break false;
            }
            std::thread::yield_now();
        };
        let _ = program.kill();
        let _ = program.wait();
        changed
    })
    .await
    .unwrap();
    assert!(watched, "the program never replaced the file");
    Some((machine, run))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_ended_after_the_new_file_was_in_place_is_reconciled_and_writes_no_second_time() {
    // The program may finish before the kill reaches it; such a try shows
    // nothing and is made again. A run that ends stopped is the one proved.
    let mut proved = false;
    for attempt in 0..12 {
        let stand = FileStand::start().unwrap();
        let (machine, run) = killed_after_the_replace(&stand).await.unwrap();
        let expected = seed_with(seed::STOCK_CSV, BEFORE, AFTER);
        // The file is the new one, whole, whatever the program had done.
        assert_eq!(on_disk(&stand, seed::STOCK_CSV), expected);
        assert_eq!(
            stand.write_count(seed::STOCK_CSV).unwrap().content_changes,
            1
        );

        // The machine is started again twenty minutes later, so that the
        // lease of the dead process has ended.
        let later = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
            SystemClock::new().now().as_unix_millis() + 1_200_000,
        )));
        let world = machine
            .open_at(Some(Arc::clone(&later) as Arc<dyn Clock>))
            .await;
        world.engine.kernel().tend().await;
        let left = world.agent(&["run", "get", &run]).await;
        if left.result()["status"] != "stopped" {
            // The program was quicker than the kill: the run is complete and
            // the file has been written once. Nothing to reconcile.
            assert_eq!(left.result()["status"], "completed", "{}", left.json);
            world.close().await;
            eprintln!("attempt {attempt}: the run ended before the kill");
            continue;
        }
        assert_eq!(left.error()["class"], "possible_commit", "{}", left.json);
        assert_eq!(left.error()["retry_safe"], false, "{}", left.json);

        // Once the quarantine has passed, resuming reconciles the record
        // before anything is sent, finds the new file, and completes the run.
        later.advance(Duration::from_secs(120));
        world.engine.kernel().tend().await;
        let done = world.agent(&["run", "resume", &run]).await;
        assert_eq!(done.result()["status"], "completed", "{}", done.json);

        // The file is the same file, not written again, and the ledger holds
        // one intent.
        assert_eq!(on_disk(&stand, seed::STOCK_CSV), expected);
        let count = stand.write_count(seed::STOCK_CSV).unwrap();
        // The count is of every change since the stand was made: the one the
        // killed program made, and none by the resume.
        assert_eq!(count.content_changes, 1, "{count:?}");
        if cfg!(unix) {
            assert_eq!(count.replacements, Some(1), "{count:?}");
        }
        let trace = world.ok(&["trace", "show", &run]).await;
        let kinds = kinds(&trace);
        assert_eq!(
            kinds.iter().filter(|kind| **kind == "intent").count(),
            1,
            "{kinds:?}"
        );
        assert!(kinds.contains(&"reconcile_read"), "{kinds:?}");
        // The reconcile read the file, found the intended value, and sent no
        // second write. The killed program may have recorded its own act
        // before it died, or not, as the kill fell; either way there is at
        // most that one "acted", and none after the reconcile read.
        assert!(
            kinds.iter().filter(|kind| **kind == "acted").count() <= 1,
            "{kinds:?}"
        );
        let reconciled = kinds
            .iter()
            .position(|kind| *kind == "reconcile_read")
            .unwrap();
        assert!(!kinds[reconciled..].contains(&"acted"), "{kinds:?}");
        assert!(stand.outside_untouched().unwrap());
        world.close().await;
        proved = true;
        break;
    }
    assert!(
        proved,
        "no try ended the program before its answer was recorded"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_changed_by_hand_after_the_preview_makes_the_approval_stale_and_writes_nothing() {
    let stand = FileStand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;
    let run = approved(&world, SET, &set_input(30)).await;

    // Somebody saves the file with another quantity, after the person
    // approved the diff against the quantity of 12.
    stand
        .set_fault(Fault::changed(
            seed::STOCK_CSV,
            Change::Bytes {
                find: BEFORE.as_bytes().to_vec(),
                replace: b"EX-1001,Left rail,17,".to_vec(),
            },
        ))
        .unwrap();
    let by_hand = on_disk(&stand, seed::STOCK_CSV);
    assert_eq!(
        by_hand,
        seed_with(seed::STOCK_CSV, BEFORE, "EX-1001,Left rail,17,")
    );

    let stale = world.agent(&["run", "resume", &run]).await;
    assert_ne!(stale.result()["status"], "completed", "{}", stale.json);
    assert_eq!(stale.error()["class"], "stale", "{}", stale.json);
    // The file holds what the person at it wrote, byte for byte, and was
    // not replaced; nothing else came in beside it.
    assert_eq!(on_disk(&stand, seed::STOCK_CSV), by_hand);
    let count = stand.write_count(seed::STOCK_CSV).unwrap();
    assert_eq!(count.content_changes, 0, "{count:?}");
    assert_eq!(strays(&stand), Vec::<String>::new());
    let trace = world.ok(&["trace", "show", &run]).await;
    assert!(!kinds(&trace).contains(&"acted"), "{trace}");
    world.close().await;
}

/// A call of a write of a note, taken as far as it goes: it is refused at
/// once, or it rests, the person approves it, and the run is refused when
/// it is resumed. It never completes.
async fn refused_wherever_it_stops(world: &World, name: &str, how: &str) {
    let input = serde_json::json!({"name": name, "content": "Changed note.\n"}).to_string();
    let call = world
        .agent(&["tool", "call", NOTE, "--input", &input])
        .await;
    if call.result()["status"] == "needs_approval" {
        let run = call.result()["run_id"].as_str().unwrap().to_owned();
        approve(world, &run).await;
        let resumed = world.agent(&["run", "resume", &run]).await;
        assert_ne!(
            resumed.result()["status"],
            "completed",
            "{how}: {}",
            resumed.json
        );
        assert_ne!(resumed.finished.code, 0, "{how}: {}", resumed.json);
    } else {
        assert_ne!(call.result()["status"], "completed", "{how}: {}", call.json);
        assert_ne!(call.finished.code, 0, "{how}: {}", call.json);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_that_leaves_the_folder_and_a_link_touch_nothing() {
    let stand = FileStand::start().unwrap();
    let machine = machine(&stand);
    let world = published(&machine).await;

    // The stand's own list of texts that try to name the file outside the
    // root, and two more that try to leave the inbox for a sibling folder.
    let mut names: Vec<(String, String)> = stand
        .escape_attempts()
        .into_iter()
        .map(|attempt| (attempt.how.to_owned(), attempt.text))
        .collect();
    names.push(("a parent step".to_owned(), "../prices/stock.csv".to_owned()));
    names.push(("a folder below".to_owned(), "later/note-3.txt".to_owned()));
    let before = held(&stand);
    for (how, name) in &names {
        refused_wherever_it_stops(&world, name, how).await;
    }
    assert_eq!(held(&stand), before);
    assert!(stand.outside_untouched().unwrap());

    // A link in the folder that leads out of it: a write to its name does
    // not follow it.
    if stand
        .set_fault(Fault::LinkOut {
            at: "inbox/outing.txt".to_owned(),
        })
        .is_ok()
    {
        let before = held(&stand);
        refused_wherever_it_stops(&world, "outing.txt", "a link out of the folder").await;
        assert_eq!(held(&stand), before);
        assert!(stand.outside_untouched().unwrap());
    }
    // No write was made by any of them.
    assert_eq!(
        stand.write_count(seed::STOCK_CSV).unwrap().content_changes,
        0
    );
    assert_eq!(strays(&stand), Vec::<String>::new());
    world.close().await;
}
