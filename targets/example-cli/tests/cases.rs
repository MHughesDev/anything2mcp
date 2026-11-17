//! The stand's own tests: the cases in its `README.md`, the control interface,
//! each fault, and the harness. Every test starts the real program as a
//! process and proves a write by asking the control interface what the data
//! file holds, never by the program's own read commands.

use std::error::Error;
use std::process::Stdio;
use std::time::{Duration, Instant};

use example_cli::faults::{Fault, FaultKind, StopHow, StopPoint, Stream};
use example_cli::harness::{Finished, Programs, Stand};
use example_cli::store::{PushRecord, WriteKind, WriteRecord};
use example_cli::{Build, CONFIRM_PROMPT, HOME_ENV, PASSWORD, PASSWORD_PROMPT, TOKEN_ENV, exit};
use serde_json::Value;

type Outcome = Result<(), Box<dyn Error>>;

/// Long enough for a loaded machine. No test's result depends on how much of it is used.
const PATIENCE: Duration = Duration::from_secs(60);

fn stand() -> Result<Stand, Box<dyn Error>> {
    let programs = Programs::at(
        env!("CARGO_BIN_EXE_example-cli"),
        env!("CARGO_BIN_EXE_example-cli-rebuilt"),
    );
    Ok(Stand::start_with(programs)?)
}

fn code(finished: &Finished) -> Option<i32> {
    finished.code
}

fn status(code: u8) -> Option<i32> {
    Some(i32::from(code))
}

fn quantity(stand: &Stand, sku: &str) -> Result<u32, Box<dyn Error>> {
    let item = stand.control().stored_item(sku)?.ok_or("no such item")?;
    Ok(item.quantity)
}

const SET_12: [&str; 5] = ["item", "set", "SK-1001", "--quantity=12", "--yes"];

/// The slice's proof (C3): change a record through the program, then ask the
/// control interface what was stored and what the program was started with.
#[test]
fn a_write_is_proven_by_asking_the_control_interface() -> Outcome {
    let stand = stand()?;
    let before = stand
        .control()
        .stored_item("SK-1001")?
        .ok_or("no seed item")?;
    assert_eq!((before.quantity, before.version), (120, 1));

    let set = stand.run(&SET_12)?;
    assert_eq!(code(&set), status(exit::OK));
    assert_eq!(set.stdout, "updated SK-1001 quantity=12\n");
    assert_eq!(set.stderr, "");

    let after = stand
        .control()
        .stored_item("SK-1001")?
        .ok_or("the item vanished")?;
    assert_eq!((after.quantity, after.version), (12, 2));
    assert_eq!(stand.control().write_count("SK-1001")?, 1);
    assert_eq!(stand.control().write_count("SK-1002")?, 0);
    let expected = WriteRecord {
        start: 1,
        sku: "SK-1001".to_owned(),
        kind: WriteKind::Update,
    };
    assert_eq!(stand.control().writes()?, [expected]);

    let log = stand.control().invocations()?;
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].start, 1);
    assert_eq!(log[0].command, "item set");
    assert_eq!(log[0].args, SET_12);
    assert_eq!(log[0].build, "first");
    assert_eq!(log[0].exit, Some(exit::OK));
    assert_eq!(
        log[0].home.as_deref(),
        Some(&*stand.home().to_string_lossy())
    );
    Ok(())
}

/// C1: an item as JSON, a list as a tab table, a result as one line of text.
#[test]
fn each_output_form_is_printed() -> Outcome {
    let stand = stand()?;
    let shown = stand.run(&["item", "show", "SK-1002", "--format=json"])?;
    let document: Value = serde_json::from_str(&shown.stdout)?;
    assert_eq!(document["sku"], "SK-1002");
    assert_eq!(document["quantity"], 4000);
    assert_eq!(document["ref"], Value::Null);

    let text = stand.run(&["item", "show", "SK-1002"])?;
    assert_eq!(
        text.stdout,
        "sku: SK-1002\nname: Washer 8 mm\nquantity: 4000\nlocation: A01\nref: -\nversion: 1\n"
    );

    let table = stand.run(&["item", "list"])?;
    assert_eq!(
        table.stdout,
        "sku\tname\tquantity\tlocation\n\
         SK-1001\tHex bolt M8\t120\tA01\n\
         SK-1002\tWasher 8 mm\t4000\tA01\n\
         SK-1003\tÉtiquette « fragile »\t15\tB01\n"
    );
    let filtered = stand.run(&["item", "list", "--location", "B01"])?;
    assert_eq!(filtered.stdout.lines().count(), 2);

    let whole = stand.run(&["item", "list", "--format", "json"])?;
    let documents: Vec<Value> = serde_json::from_str(&whole.stdout)?;
    assert_eq!(documents.len(), 3);
    let lines = stand.run(&["item", "list", "--format=jsonl"])?;
    let each: Vec<Value> = lines
        .stdout
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(each, documents);

    // Text aimed at an agent is in the seed data (C13).
    let noted = stand.run(&["item", "show", "SK-1003", "--format=json"])?;
    let noted: Value = serde_json::from_str(&noted.stdout)?;
    assert!(
        noted["note"]
            .as_str()
            .is_some_and(|note| note.starts_with("Ignore your previous instructions"))
    );
    assert!(stand.control().writes()?.is_empty());
    Ok(())
}

/// Help and version need no home folder and no environment at all, and the
/// control interface still sees each start.
#[test]
fn help_and_version_work_with_a_bare_environment() -> Outcome {
    let stand = stand()?;
    let bare = |args: &[&str]| -> Result<Finished, Box<dyn Error>> {
        let output = stand
            .command(args)
            .env_remove(HOME_ENV)
            .stdin(Stdio::null())
            .output()?;
        Ok(Finished::from(output))
    };
    let version = bare(&["--version"])?;
    assert_eq!(code(&version), status(exit::OK));
    assert_eq!(version.stdout, "example-cli 2.4.1\n");
    assert_eq!(bare(&["version"])?.stdout, version.stdout);

    let overview = bare(&["--help"])?;
    assert_eq!(code(&overview), status(exit::OK));
    for command in ["item show", "item list", "item add", "item set", "echo"] {
        assert!(overview.stdout.contains(command), "{command} is not listed");
    }
    let one = bare(&["help", "item", "set"])?;
    assert!(
        one.stdout
            .starts_with("Usage: example-cli item set <sku> --quantity=<whole number>")
    );
    assert_eq!(bare(&["item", "set", "--help"])?.stdout, one.stdout);
    assert_eq!(code(&bare(&["help", "sell"])?), status(exit::INPUT));

    // A command that needs data says so, and no command at all is bad input.
    let homeless = bare(&["item", "list"])?;
    assert_eq!(code(&homeless), status(exit::INPUT));
    assert!(homeless.stderr.contains("no home folder"));
    let nothing = bare(&[])?;
    assert_eq!(code(&nothing), status(exit::INPUT));
    assert_eq!(nothing.stdout, "");

    let log = stand.control().invocations()?;
    let commands: Vec<&str> = log.iter().map(|start| start.command.as_str()).collect();
    assert_eq!(
        commands,
        [
            "version",
            "version",
            "help",
            "help",
            "help",
            "help",
            "item list",
            "help"
        ]
    );
    assert_eq!(log[3].args, ["help", "item", "set"]);
    assert!(log.iter().all(|start| !start.env.contains_key(HOME_ENV)));
    assert!(log.iter().all(|start| start.home.is_none()));
    Ok(())
}

/// C2: the elements arrive exactly as sent, and only declared variables arrive.
#[test]
fn arguments_and_environment_arrive_exactly() -> Outcome {
    let stand = stand()?;
    let sent = [
        "echo",
        "two words",
        "a \"quoted\" 'one'",
        "$HOME; rm -rf *",
        "line one\nline two",
        "Étiquette « fragile » 標籤",
        "back\\slash\\",
        "",
        "--",
        "-rf",
        "--yes",
    ];
    let echoed = stand.run(&sent)?;
    assert_eq!(code(&echoed), status(exit::OK));
    let received: Value = serde_json::from_str(&echoed.stdout)?;
    let args: Vec<&str> = received["args"]
        .as_array()
        .ok_or("no args")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(args, sent[1..]);

    let env = received["env"].as_object().ok_or("no env")?;
    assert_eq!(
        env[HOME_ENV].as_str(),
        Some(&*stand.home().to_string_lossy())
    );
    // The test process has a search path. The program was not given it.
    assert!(std::env::var_os("PATH").is_some());
    assert!(!env.keys().any(|name| name.eq_ignore_ascii_case("PATH")));

    // The control interface saw the same, without the program's output.
    let log = stand.control().invocations()?;
    assert_eq!(log[0].args, sent);
    assert_eq!(log[0].command, "echo");
    assert!(
        !log[0]
            .env
            .keys()
            .any(|name| name.eq_ignore_ascii_case("PATH"))
    );
    assert_eq!(log[0].cwd, received["cwd"].as_str().unwrap_or_default());
    Ok(())
}

/// C8: a refusal is a message on standard error and a status with a meaning,
/// and nothing is written.
#[test]
fn exit_statuses_have_meaning_and_a_refusal_writes_nothing() -> Outcome {
    let stand = stand()?;
    let run = |args: &[&str]| stand.run(args);

    let absent = run(&["item", "show", "SK-9999"])?;
    assert_eq!(code(&absent), status(exit::ABSENT));
    assert_eq!(absent.stdout, "");
    assert_eq!(absent.stderr, "example-cli: error: no such item: SK-9999\n");
    let absent_write = run(&["item", "set", "SK-9999", "--quantity=1", "--yes"])?;
    assert_eq!(code(&absent_write), status(exit::ABSENT));

    for bad in [
        &["item", "show", "sk-1"][..],
        &["item", "set", "SK-1001", "--quantity=many", "--yes"],
        &["item", "set", "SK-1001", "--quantity", "-5", "--yes"],
        &["item", "set", "SK-1001", "--yes"],
        &["item", "set", "SK-1001", "--quantity=1", "--force"],
        &["item", "add", "--name", "Nut", "--location", "nowhere"],
        &["item", "list", "--format=xml"],
        &["item", "sell", "SK-1001"],
        &["sell"],
    ] {
        assert_eq!(code(&run(bad)?), status(exit::INPUT), "{bad:?}");
    }

    let too_many = run(&["item", "set", "SK-1001", "--quantity=10000", "--yes"])?;
    assert_eq!(code(&too_many), status(exit::REJECTED));
    assert_eq!(too_many.stdout, "");
    assert!(too_many.stderr.contains("quantity 10000 is more than"));
    let nowhere = run(&["item", "add", "--name", "Nut", "--location", "Z99"])?;
    assert_eq!(code(&nowhere), status(exit::REJECTED));
    assert!(nowhere.stderr.contains("there is no location Z99"));

    let stale = run(&[
        "item",
        "set",
        "SK-1001",
        "--quantity=1",
        "--if-version=7",
        "--yes",
    ])?;
    assert_eq!(code(&stale), status(exit::STALE));
    let fresh = run(&[
        "item",
        "set",
        "SK-1002",
        "--quantity=1",
        "--if-version=1",
        "--yes",
    ])?;
    assert_eq!(code(&fresh), status(exit::OK));

    let writes = stand.control().writes()?;
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].sku, "SK-1002");
    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert_eq!(stand.control().stored_items()?.len(), 3);
    Ok(())
}

/// C9: a write asks first, on standard output, and reads standard input.
#[test]
fn a_write_asks_first_unless_told_yes() -> Outcome {
    let stand = stand()?;
    let ask = ["item", "set", "SK-1001", "--quantity=12"];

    let nobody = stand.run(&ask)?;
    assert_eq!(code(&nobody), status(exit::INPUT));
    assert_eq!(nobody.stdout, CONFIRM_PROMPT);
    let no = stand.run_with_input(&ask, "n\n")?;
    assert_eq!(code(&no), status(exit::INPUT));
    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert_eq!(stand.control().write_count("SK-1001")?, 0);

    let yes = stand.run_with_input(&ask, "y\n")?;
    assert_eq!(code(&yes), status(exit::OK));
    assert_eq!(
        yes.stdout,
        format!("{CONFIRM_PROMPT}updated SK-1001 quantity=12\n")
    );
    assert_eq!(quantity(&stand, "SK-1001")?, 12);

    for asks in [&["item", "remove", "SK-1002"][..], &["catalog", "push"]] {
        let asked = stand.run(asks)?;
        assert_eq!(code(&asked), status(exit::INPUT), "{asks:?}");
        assert_eq!(asked.stdout, CONFIRM_PROMPT);
    }
    assert_eq!(stand.control().writes()?.len(), 1);
    Ok(())
}

/// C6 and C7: the program assigns the code; a reference finds the record; the
/// same add run twice makes two records.
#[test]
fn add_assigns_a_code_and_running_it_twice_makes_two() -> Outcome {
    let stand = stand()?;
    let add = [
        "item",
        "add",
        "--name",
        "Spring washer",
        "--location",
        "A02",
        "--quantity",
        "40",
        "--ref",
        "run:42",
        "--format=json",
    ];
    let first: Value = serde_json::from_str(&stand.run(&add)?.stdout)?;
    assert_eq!(first["sku"], "SK-1004");
    assert_eq!(first["ref"], "run:42");
    let second: Value = serde_json::from_str(&stand.run(&add)?.stdout)?;
    assert_eq!(second["sku"], "SK-1005");

    let plain = stand.run(&[
        "item",
        "add",
        "--name",
        "Spring washer",
        "--location",
        "A02",
    ])?;
    assert_eq!(plain.stdout, "created SK-1006\n");

    let stored = stand.control().stored_items()?;
    let same_name: Vec<_> = stored
        .iter()
        .filter(|item| item.name == "Spring washer")
        .collect();
    assert_eq!(same_name.len(), 3);
    let stamped = stand
        .control()
        .stored_item("SK-1004")?
        .ok_or("not stored")?;
    assert_eq!(stamped.reference.as_deref(), Some("run:42"));
    assert_eq!((stamped.quantity, stamped.version), (40, 1));
    assert_eq!(
        stand.control().stored_item("SK-1006")?.map(|i| i.reference),
        Some(None)
    );
    assert_eq!(stand.control().write_count("SK-1004")?, 1);
    assert!(
        stand
            .control()
            .writes()?
            .iter()
            .all(|write| write.kind == WriteKind::Create)
    );

    // The program's own way to find by reference, for a pack's read-back.
    let found = stand.run(&["item", "list", "--ref", "run:42"])?;
    assert_eq!(found.stdout.lines().count(), 3);
    Ok(())
}

#[test]
fn remove_leaves_nothing_and_a_push_is_recorded() -> Outcome {
    let stand = stand()?;
    let removed = stand.run(&["item", "remove", "SK-1002", "--yes"])?;
    assert_eq!(removed.stdout, "removed SK-1002\n");
    assert_eq!(stand.control().stored_item("SK-1002")?, None);
    assert_eq!(stand.control().writes()?[0].kind, WriteKind::Remove);
    assert_eq!(
        code(&stand.run(&["item", "show", "SK-1002"])?),
        status(exit::ABSENT)
    );
    assert_eq!(
        code(&stand.run(&["item", "remove", "SK-1002", "--yes"])?),
        status(exit::ABSENT)
    );

    let pushed = stand.run(&["catalog", "push", "--yes"])?;
    assert_eq!(pushed.stdout, "pushed 2 items\n");
    assert_eq!(
        stand.control().pushes()?,
        [PushRecord { start: 4, count: 2 }]
    );
    Ok(())
}

/// C11: output arrives row by row on both streams at once, and there can be
/// more of it than a pipe holds.
#[test]
fn a_report_prints_on_both_streams_in_pieces() -> Outcome {
    let stand = stand()?;
    let report = stand.run(&["report", "stock", "--rows", "6000"])?;
    assert_eq!(code(&report), status(exit::OK));
    assert_eq!(report.stdout.lines().count(), 6001);
    assert_eq!(report.stdout.lines().next(), Some("row\tsku\tquantity"));
    assert_eq!(report.stdout.lines().last(), Some("6000\tSK-1003\t15"));
    assert_eq!(report.stderr.lines().count(), 6001);
    assert_eq!(report.stderr.lines().last(), Some("done"));
    // Each stream alone is more than the 64 KiB a pipe commonly holds.
    assert!(report.stdout.len() > 65_536 && report.stderr.len() > 65_536);

    let began = Instant::now();
    let slow = stand.run(&["report", "stock", "--pause-ms", "100"])?;
    // Three rows, a tenth of a second after each. A clock may be a little coarse.
    assert!(began.elapsed() >= Duration::from_millis(200));
    assert_eq!(slow.stdout.lines().count(), 4);
    Ok(())
}

#[test]
fn fault_a_slow_answer() -> Outcome {
    let stand = stand()?;
    let fault = Fault::once(FaultKind::Delay { ms: 400 });
    stand.control().set_fault(&fault)?;
    assert_eq!(stand.control().faults()?, [fault]);

    let began = Instant::now();
    let shown = stand.run(&["item", "show", "SK-1001"])?;
    assert!(began.elapsed() >= Duration::from_millis(300));
    assert_eq!(code(&shown), status(exit::OK));
    assert!(shown.stdout.starts_with("sku: SK-1001\n"));
    // Used up by the one start it applied to.
    assert!(stand.control().faults()?.is_empty());
    Ok(())
}

/// The program says "try again later" and has in fact written.
#[test]
fn fault_a_failure_exit_after_the_write_was_committed() -> Outcome {
    let stand = stand()?;
    let how = StopHow::Exit {
        code: exit::TRANSIENT,
    };
    stand
        .control()
        .set_fault(&Fault::once(FaultKind::stop(StopPoint::AfterCommit, how)))?;
    let set = stand.run(&SET_12)?;
    assert_eq!(code(&set), status(exit::TRANSIENT));
    assert_eq!(set.stdout, "");
    assert_eq!(quantity(&stand, "SK-1001")?, 12);
    assert_eq!(stand.control().write_count("SK-1001")?, 1);
    assert_eq!(
        stand.control().invocations()?[0].exit,
        Some(exit::TRANSIENT)
    );
    Ok(())
}

/// A write is killed part-way: the new contents were prepared and never
/// committed. While it hangs it holds the lock, and the system frees the lock
/// when the process dies.
#[test]
fn fault_a_write_killed_before_commit_stores_nothing() -> Outcome {
    let stand = stand()?;
    let hang = FaultKind::stop(StopPoint::BeforeCommit, StopHow::Hang);
    stand.control().set_fault(&Fault::once(hang))?;
    let mut child = stand
        .command(SET_12)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    assert_eq!(stand.control().wait_until_hung(1, PATIENCE)?, [1]);
    assert!(stand.control().uncommitted_write());
    assert_eq!(quantity(&stand, "SK-1001")?, 120);

    let meanwhile = stand.run(&["item", "set", "SK-1002", "--quantity=1", "--yes"])?;
    assert_eq!(code(&meanwhile), status(exit::BUSY));
    let reading = stand.run(&["item", "show", "SK-1001"])?;
    assert_eq!(code(&reading), status(exit::OK));

    child.kill()?;
    child.wait()?;
    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert!(stand.control().writes()?.is_empty());
    let log = stand.control().invocations()?;
    assert_eq!((log[0].exit, log[0].hung), (None, true));

    // The lock died with the process: the next write goes through.
    let again = stand.run(&SET_12)?;
    assert_eq!(code(&again), status(exit::OK));
    assert_eq!(quantity(&stand, "SK-1001")?, 12);
    assert_eq!(stand.control().write_count("SK-1001")?, 1);
    assert!(!stand.control().uncommitted_write());
    Ok(())
}

/// C5: the program commits, then hangs until killed. The write is there.
#[test]
fn fault_a_write_that_hangs_after_commit_is_stored() -> Outcome {
    let stand = stand()?;
    let hang = FaultKind::stop(StopPoint::AfterCommit, StopHow::Hang);
    stand
        .control()
        .set_fault(&Fault::once(hang).on("item set"))?;
    // A read is not a write and never reaches the point.
    assert_eq!(
        code(&stand.run(&["item", "show", "SK-1001"])?),
        status(exit::OK)
    );

    let mut child = stand
        .command(SET_12)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    assert_eq!(stand.control().wait_until_hung(1, PATIENCE)?, [2]);
    child.kill()?;
    let output = child.wait_with_output()?;
    assert!(output.stdout.is_empty());
    assert_eq!(quantity(&stand, "SK-1001")?, 12);
    assert_eq!(stand.control().write_count("SK-1001")?, 1);
    assert!(!stand.control().uncommitted_write());
    Ok(())
}

/// C10: an exit status nobody declared, and an abnormal end.
#[test]
fn fault_an_undeclared_exit_and_an_abnormal_end() -> Outcome {
    let stand = stand()?;
    let exit_42 = FaultKind::stop(StopPoint::Start, StopHow::Exit { code: 42 });
    stand.control().set_fault(&Fault::once(exit_42))?;
    let odd = stand.run(&["item", "list"])?;
    assert_eq!(code(&odd), Some(42));
    assert_eq!(odd.stdout, "");

    let abort = FaultKind::stop(StopPoint::BeforeCommit, StopHow::Abort);
    stand.control().set_fault(&Fault::once(abort))?;
    let crashed = stand.run(&SET_12)?;
    assert_ne!(code(&crashed), status(exit::OK));
    // Where the system has signals, the program ended by one.
    #[cfg(unix)]
    assert_eq!(code(&crashed), None);
    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert!(stand.control().writes()?.is_empty());
    assert_eq!(stand.control().invocations()?[1].exit, None);
    Ok(())
}

/// Every output form changes; the version does not.
#[test]
fn fault_drift_changes_every_output_form() -> Outcome {
    let stand = stand()?;
    stand
        .control()
        .set_fault(&Fault::until_cleared(FaultKind::Drift))?;
    let table = stand.run(&["item", "list", "--location=B01"])?;
    assert_eq!(
        table.stdout,
        "sku\tname\tqty\tlocation\tversion\nSK-1003\tÉtiquette « fragile »\t15\tB01\t1\n"
    );
    let document: Value = serde_json::from_str(
        &stand
            .run(&["item", "show", "SK-1001", "--format=json"])?
            .stdout,
    )?;
    assert_eq!(document["qty"], 120);
    assert_eq!(document.get("quantity"), None);
    let text = stand.run(&["item", "show", "SK-1001"])?;
    assert!(text.stdout.contains("\nqty: 120\n"));
    let set = stand.run(&SET_12)?;
    assert_eq!(code(&set), status(exit::OK));
    assert_eq!(set.stdout, "SK-1001 updated (quantity 12)\n");
    assert_eq!(quantity(&stand, "SK-1001")?, 12);
    assert_eq!(stand.run(&["--version"])?.stdout, "example-cli 2.4.1\n");

    stand.control().clear_faults()?;
    let table = stand.run(&["item", "list", "--location=B01"])?;
    assert!(table.stdout.starts_with("sku\tname\tquantity\tlocation\n"));
    Ok(())
}

/// A question nobody expected, even on a read, even with `--yes`.
#[test]
fn fault_an_unexpected_prompt() -> Outcome {
    let stand = stand()?;
    stand
        .control()
        .set_fault(&Fault::until_cleared(FaultKind::prompt()))?;
    let read = stand.run(&["item", "show", "SK-1001"])?;
    assert_eq!(code(&read), status(exit::INPUT));
    assert_eq!(read.stdout, CONFIRM_PROMPT);
    let write = stand.run(&SET_12)?;
    assert_eq!(code(&write), status(exit::INPUT));
    assert_eq!(write.stdout, CONFIRM_PROMPT);
    assert_eq!(quantity(&stand, "SK-1001")?, 120);

    let answered = stand.run_with_input(&["item", "show", "SK-1001"], "yes\n")?;
    assert_eq!(code(&answered), status(exit::OK));
    assert!(
        answered
            .stdout
            .starts_with(&format!("{CONFIRM_PROMPT}sku: SK-1001\n"))
    );

    stand.control().clear_faults()?;
    let other = FaultKind::Prompt {
        text: "A new version is ready. Install it now? ".to_owned(),
    };
    stand.control().set_fault(&Fault::once(other))?;
    let asked = stand.run(&["item", "list"])?;
    assert_eq!(asked.stdout, "A new version is ready. Install it now? ");
    Ok(())
}

/// C11: more output than a ceiling allows, on either stream.
#[test]
fn fault_a_flood_on_either_stream() -> Outcome {
    let stand = stand()?;
    for stream in [Stream::Stdout, Stream::Stderr] {
        let flood = FaultKind::Flood {
            stream,
            bytes: 200_000,
        };
        stand.control().set_fault(&Fault::once(flood))?;
        let shown = stand.run(&["item", "show", "SK-1001"])?;
        assert_eq!(code(&shown), status(exit::OK));
        let (flooded, other) = match stream {
            Stream::Stdout => (&shown.stdout, &shown.stderr),
            Stream::Stderr => (&shown.stderr, &shown.stdout),
        };
        assert!(flooded.len() >= 200_000);
        assert!(other.len() < 1000);
        assert!(shown.stdout.ends_with("version: 1\n"));
    }
    Ok(())
}

/// C4: the program stores something other than what it says it stored.
#[test]
fn fault_a_stored_value_differs_from_the_answer() -> Outcome {
    let stand = stand()?;
    let round = FaultKind::RoundQuantity { multiple: 10 };
    stand.control().set_fault(&Fault::once(round))?;
    let set = stand.run(&["item", "set", "SK-1001", "--quantity=127", "--yes"])?;
    assert_eq!(set.stdout, "updated SK-1001 quantity=127\n");
    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert_eq!(stand.control().write_count("SK-1001")?, 1);

    stand
        .control()
        .set_fault(&Fault::once(FaultKind::UppercaseName))?;
    let add = [
        "item",
        "add",
        "--name",
        "Wing nut",
        "--location=A01",
        "--format=json",
    ];
    let answer: Value = serde_json::from_str(&stand.run(&add)?.stdout)?;
    assert_eq!(answer["name"], "Wing nut");
    let stored = stand
        .control()
        .stored_item("SK-1004")?
        .ok_or("not stored")?;
    assert_eq!(stored.name, "WING NUT");
    Ok(())
}

/// C12: another version from the same file, and the same version from another file.
#[test]
fn fault_another_version_and_another_build() -> Outcome {
    let stand = stand()?;
    let version = FaultKind::ReportVersion {
        version: "9.0.0".to_owned(),
    };
    stand.control().set_fault(&Fault::once(version))?;
    // A fault that names no command is not used up by other commands.
    assert_eq!(code(&stand.run(&["item", "list"])?), status(exit::OK));
    assert_eq!(stand.run(&["--version"])?.stdout, "example-cli 9.0.0\n");
    assert_eq!(stand.run(&["--version"])?.stdout, "example-cli 2.4.1\n");

    let before = std::fs::read(stand.program())?;
    stand.replace_program(Build::Rebuilt)?;
    let after = std::fs::read(stand.program())?;
    assert_ne!(before, after);
    assert_eq!(stand.run(&["--version"])?.stdout, "example-cli 2.4.1\n");
    let set = stand.run(&SET_12)?;
    assert_eq!(set.stdout, "updated SK-1001 quantity=12\n");
    assert_eq!(quantity(&stand, "SK-1001")?, 12);
    let builds: Vec<String> = stand
        .control()
        .invocations()?
        .into_iter()
        .map(|start| start.build)
        .collect();
    assert_eq!(builds, ["first", "first", "first", "rebuilt", "rebuilt"]);
    Ok(())
}

/// A session that is needed, kept, and ended; the password asked for on
/// standard error; and C13, the secret printed back.
#[test]
fn fault_a_session_is_required_and_a_secret_leaks() -> Outcome {
    let stand = stand()?;
    stand
        .control()
        .set_fault(&Fault::until_cleared(FaultKind::RequireSession))?;
    let list = ["item", "list"];
    let refused = stand.run(&list)?;
    assert_eq!(code(&refused), status(exit::SESSION_EXPIRED));
    assert_eq!(refused.stdout, "");
    assert_eq!(code(&stand.run(&SET_12)?), status(exit::SESSION_EXPIRED));
    assert!(stand.control().writes()?.is_empty());

    let wrong = stand.run_with_input(&["login"], "guess\n")?;
    assert_eq!(code(&wrong), status(exit::REJECTED));
    assert_eq!(code(&stand.run(&["login"])?), status(exit::INPUT));
    assert!(!stand.control().signed_in());

    let login = stand.run_with_input(&["login"], &format!("{PASSWORD}\n"))?;
    assert_eq!(code(&login), status(exit::OK));
    assert_eq!(login.stdout, "signed in\n");
    assert_eq!(login.stderr, PASSWORD_PROMPT);
    assert!(stand.control().signed_in());
    assert_eq!(code(&stand.run(&list)?), status(exit::OK));

    stand.control().expire_session()?;
    assert_eq!(code(&stand.run(&list)?), status(exit::SESSION_EXPIRED));
    let by_token = stand
        .command(list)
        .env(TOKEN_ENV, PASSWORD)
        .stdin(Stdio::null())
        .output()?;
    assert_eq!(by_token.status.code(), status(exit::OK));

    stand
        .control()
        .set_fault(&Fault::until_cleared(FaultKind::LeakSecret))?;
    let quiet = stand.run_with_input(&["login", "--password-stdin"], PASSWORD)?;
    assert_eq!(code(&quiet), status(exit::OK));
    let leaked = format!("debug: password={PASSWORD}\n");
    assert_eq!(quiet.stderr, leaked);
    assert_eq!(quiet.stdout, format!("{leaked}signed in\n"));
    let with_token = Finished::from(
        stand
            .command(list)
            .env(TOKEN_ENV, PASSWORD)
            .stdin(Stdio::null())
            .output()?,
    );
    assert!(with_token.stdout.contains(PASSWORD) && with_token.stderr.contains(PASSWORD));
    Ok(())
}

/// C14: the program starts a child that outlives it, and the control
/// interface knows whether the child is alive.
#[test]
fn fault_a_child_outlives_the_program() -> Outcome {
    let stand = stand()?;
    let child = FaultKind::SpawnChild {
        lifetime_ms: 600_000,
    };
    stand.control().set_fault(&Fault::once(child))?;
    // The child keeps the program's output streams, so they are not read to
    // their end here: that end would not come until the child is gone.
    let finished = stand
        .command(["item", "show", "SK-1001"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    assert_eq!(finished.code(), status(exit::OK));
    assert_eq!(stand.control().living_children()?, [1]);

    stand.control().stop_children()?;
    stand.control().wait_until_no_children(PATIENCE)?;
    assert!(stand.control().living_children()?.is_empty());

    // A reset ends children by itself, and lets later ones live.
    stand.control().reset()?;
    let child = FaultKind::SpawnChild {
        lifetime_ms: 600_000,
    };
    stand.control().set_fault(&Fault::once(child))?;
    let finished = stand
        .command(["item", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    assert_eq!(finished.code(), status(exit::OK));
    assert_eq!(stand.control().living_children()?, [1]);
    stand.control().reset()?;
    assert!(stand.control().living_children()?.is_empty());
    Ok(())
}

#[test]
fn a_held_lock_makes_a_write_busy() -> Outcome {
    let stand = stand()?;
    let lock = stand.control().hold_write_lock()?;
    let busy = stand.run(&SET_12)?;
    assert_eq!(code(&busy), status(exit::BUSY));
    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert_eq!(code(&stand.run(&["item", "list"])?), status(exit::OK));
    drop(lock);
    assert_eq!(code(&stand.run(&SET_12)?), status(exit::OK));
    assert_eq!(stand.control().write_count("SK-1001")?, 1);
    Ok(())
}

#[test]
fn a_fault_applies_to_the_command_it_names_for_as_long_as_it_says() -> Outcome {
    let stand = stand()?;
    let once = Fault::once(FaultKind::Drift).on("item list");
    let lasting = Fault::until_cleared(FaultKind::RoundQuantity { multiple: 100 }).on("item");
    stand.control().set_fault(&once)?;
    stand.control().set_fault(&lasting)?;

    let show = stand.run(&["item", "show", "SK-1001"])?;
    assert!(show.stdout.contains("quantity: 120"));
    assert_eq!(stand.control().faults()?.len(), 2);
    let list = stand.run(&["item", "list"])?;
    assert!(list.stdout.starts_with("sku\tname\tqty"));
    let list = stand.run(&["item", "list"])?;
    assert!(list.stdout.starts_with("sku\tname\tquantity"));
    assert_eq!(stand.control().faults()?, [lasting]);

    for _ in 0..2 {
        stand.run(&["item", "set", "SK-1001", "--quantity=250", "--yes"])?;
        assert_eq!(quantity(&stand, "SK-1001")?, 200);
    }
    stand.control().clear_faults()?;
    stand.run(&["item", "set", "SK-1001", "--quantity=250", "--yes"])?;
    assert_eq!(quantity(&stand, "SK-1001")?, 250);
    Ok(())
}

#[test]
fn reset_restores_the_seed_and_forgets_the_rest() -> Outcome {
    let stand = stand()?;
    stand.run(&SET_12)?;
    stand.run_with_input(&["login", "--password-stdin"], PASSWORD)?;
    stand
        .control()
        .set_fault(&Fault::until_cleared(FaultKind::Drift))?;
    stand.control().reset()?;

    assert_eq!(quantity(&stand, "SK-1001")?, 120);
    assert!(stand.control().writes()?.is_empty());
    assert!(stand.control().invocations()?.is_empty());
    assert!(stand.control().faults()?.is_empty());
    assert!(!stand.control().signed_in());
    assert_eq!(stand.run(&SET_12)?.stdout, "updated SK-1001 quantity=12\n");
    assert_eq!(stand.control().invocations()?[0].start, 1);
    Ok(())
}

#[test]
fn two_stands_share_nothing() -> Outcome {
    let one = stand()?;
    let other = stand()?;
    assert_ne!(one.program(), other.program());
    one.control()
        .set_fault(&Fault::until_cleared(FaultKind::Drift))?;
    one.run(&SET_12)?;
    assert_eq!(quantity(&one, "SK-1001")?, 12);
    assert_eq!(quantity(&other, "SK-1001")?, 120);
    assert!(other.control().invocations()?.is_empty());
    assert!(other.control().faults()?.is_empty());

    // A home folder the stand did not make: the program is told where it is,
    // and the control interface is pointed at it.
    let elsewhere = one.root().join("another home");
    let elsewhere_text = elsewhere.to_string_lossy().into_owned();
    one.control().clear_faults()?;
    let set = one.run(&[
        "--home",
        &elsewhere_text,
        "item",
        "set",
        "SK-1002",
        "--quantity=5",
        "--yes",
    ])?;
    assert_eq!(code(&set), status(exit::OK));
    let there = one.control().for_home(&elsewhere);
    assert_eq!(
        there.stored_item("SK-1002")?.map(|item| item.quantity),
        Some(5)
    );
    assert_eq!(quantity(&one, "SK-1002")?, 4000);
    let log = one.control().invocations()?;
    assert_eq!(log[1].home.as_deref(), Some(elsewhere_text.as_str()));

    let root = one.root().to_path_buf();
    drop(one);
    assert!(!root.exists());
    Ok(())
}

/// What a test in another crate does: no path is known ahead of time.
#[test]
fn the_program_is_found_or_built_for_any_crate() -> Outcome {
    let programs = Programs::built()?;
    assert!(programs.path(Build::First).is_file());
    assert!(programs.path(Build::Rebuilt).is_file());
    let stand = Stand::start()?;
    assert_eq!(stand.run(&["--version"])?.stdout, "example-cli 2.4.1\n");
    assert_eq!(stand.control().invocations()?.len(), 1);
    Ok(())
}
