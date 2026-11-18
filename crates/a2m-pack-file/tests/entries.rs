//! Folders as collections of entries, settings files, and a growing file,
//! against the file stand (decision 0246). Every write is proven by asking
//! the stand what is on disk, never by the pack's own read alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{Decimal, FailureClass, Value, Values};
use a2m_ledger::Store;
use a2m_provider::{Posture, StepFailure, StepOutcome};
use common::{Scene, granted, last_values, store};
use example_files::control::EntryKind;
use example_files::faults::{Change, Fault, SaveStyle};
use example_files::seed;

fn inputs(pairs: &[(&str, Value)]) -> Values {
    let mut values = Values::new();
    for (name, value) in pairs {
        values.insert((*name).to_owned(), value.clone());
    }
    values
}

fn text(value: &str) -> Value {
    Value::from(value)
}

fn code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

fn seed_bytes(name: &str) -> Vec<u8> {
    seed::files()
        .into_iter()
        .find(|file| file.name == name)
        .unwrap()
        .bytes
}

/// The seed bytes of a file with runs of bytes replaced, each once.
fn seed_with(name: &str, edits: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = seed_bytes(name);
    for (find, replace) in edits {
        let at = bytes
            .windows(find.len())
            .position(|window| window == find.as_bytes())
            .unwrap_or_else(|| panic!("the seed holds {find:?}"));
        bytes = [&bytes[..at], replace.as_bytes(), &bytes[at + find.len()..]].concat();
    }
    bytes
}

/// Runs one write as the engine will: the steps before the commit point,
/// then `between`, then the save with the record's token, the confirm, and
/// the read-back through a fresh driver. The read-back's values are given.
async fn write(
    scene: &Scene,
    store: &Store,
    caller: u8,
    operation: &str,
    values: &Values,
    between: impl FnOnce(),
) -> Result<Values, StepFailure> {
    let (binding, surface) = scene.binding(operation);
    let grant = granted(store, caller, operation).await;
    let (handle, mut prepared) = scene
        .prepared(&surface, Some(grant.fence), Posture::Live)
        .await;
    let commit = binding.act.iter().position(|step| step.commits).unwrap();
    let run = async {
        scene
            .write(
                &handle,
                &mut prepared,
                &binding.act[..commit],
                values,
                &grant,
            )
            .await?;
        between();
        scene
            .write(
                &handle,
                &mut prepared,
                &binding.act[commit..],
                values,
                &grant,
            )
            .await?;
        scene
            .read(&handle, &mut prepared, &binding.confirm, values)
            .await
    };
    let done = run.await;
    assert!(handle.release(prepared, done.is_err()).await.complete);
    done?;
    let (handle, mut prepared) = scene.prepared(&surface, None, Posture::Live).await;
    let read_back = scene
        .read(&handle, &mut prepared, &binding.read_back, values)
        .await?;
    assert!(handle.release(prepared, false).await.complete);
    Ok(match read_back.last() {
        Some(StepOutcome::Done(observed)) => observed.values.clone(),
        _ => Values::new(),
    })
}

async fn read(scene: &Scene, operation: &str, values: &Values) -> Result<Values, StepFailure> {
    let outcomes = scene.read_operation(operation, values).await?;
    Ok(last_values(&outcomes).clone())
}

// ---- settings files ----

#[tokio::test]
async fn a_settings_value_is_read_by_its_path_and_a_form_with_two_meanings_stops() {
    let scene = Scene::new();
    let none = Values::new();
    let orders = read(&scene, "example_ini_orders_get", &none).await.unwrap();
    assert_eq!(orders.get("prefix"), Some(&text("A-")));
    assert_eq!(orders.get("next_number"), Some(&Value::Int(103)));
    let plist = read(&scene, "example_plist_get", &none).await.unwrap();
    assert_eq!(plist.get("title"), Some(&text("Rails & brackets")));
    assert_eq!(plist.get("next_number"), Some(&Value::Int(103)));
    assert_eq!(plist.get("open"), Some(&Value::Bool(true)));
    // A quoted value whose document does not say what quotes are, and a
    // comment after a value: each stops by name, and only that read.
    for operation in ["example_ini_note_get", "example_ini_cutoff_get"] {
        let failure = read(&scene, operation, &none).await.unwrap_err();
        assert_eq!(
            code(&failure),
            (FailureClass::UnknownVariant, "shape_unsupported"),
            "{operation}"
        );
        assert!(!failure.failure.message().contains("16:30"));
    }
    // A layout the document does not declare is drift.
    scene
        .stand
        .set_fault(Fault::changed(
            seed::SETTINGS_INI,
            Change::Bytes {
                find: b"[warehouse W1]".to_vec(),
                replace: b"[warehouse W2]".to_vec(),
            },
        ))
        .unwrap();
    let failure = read(&scene, "example_ini_orders_get", &none)
        .await
        .unwrap_err();
    assert_eq!(
        code(&failure),
        (FailureClass::Drift, "fingerprint_mismatch")
    );
    // A binary property list is not built, and says so.
    scene
        .stand
        .set_fault(Fault::changed(
            seed::SETTINGS_PLIST,
            Change::Whole(b"bplist00\x08\x09".to_vec()),
        ))
        .unwrap();
    let failure = read(&scene, "example_plist_get", &none).await.unwrap_err();
    assert_eq!(
        code(&failure),
        (FailureClass::UnknownVariant, "shape_unsupported")
    );
}

#[tokio::test]
async fn a_settings_write_changes_its_value_and_keeps_every_other_byte() {
    let scene = Scene::new();
    let store = store().await;

    // An INI file: a byte-order mark, CRLF, comments, a blank line, and a
    // last line not ended, all kept; only the two values differ.
    let values = inputs(&[("prefix", text("B-")), ("next_number", Value::Int(104))]);
    let back = write(&scene, &store, 1, "example_ini_orders_set", &values, || {})
        .await
        .unwrap();
    assert_eq!(back.get("prefix"), Some(&text("B-")));
    assert_eq!(back.get("next_number"), Some(&Value::Int(104)));
    let expected = seed_with(
        seed::SETTINGS_INI,
        &[
            ("prefix = A-", "prefix = B-"),
            ("next_number=103", "next_number=104"),
        ],
    );
    assert_eq!(
        scene.stand.bytes(seed::SETTINGS_INI).unwrap().unwrap(),
        expected
    );

    // Quotes the document says are around the value stay where they were.
    let values = inputs(&[("note", text("kept again"))]);
    write(
        &scene,
        &store,
        2,
        "example_ini_note_stripped_set",
        &values,
        || {},
    )
    .await
    .unwrap();
    let expected = seed_with(
        seed::SETTINGS_INI,
        &[
            ("prefix = A-", "prefix = B-"),
            ("next_number=103", "next_number=104"),
            ("\"kept as written\"", "\"kept again\""),
        ],
    );
    assert_eq!(
        scene.stand.bytes(seed::SETTINGS_INI).unwrap().unwrap(),
        expected
    );

    // A property list: a reference written for `<`, an integer, and a
    // `true` that becomes `false`; tabs, the comment, and the document type
    // stay as they were.
    let values = inputs(&[
        ("title", text("Rails < bolts")),
        ("next_number", Value::Int(104)),
        ("open", Value::Bool(false)),
    ]);
    let back = write(&scene, &store, 3, "example_plist_set", &values, || {})
        .await
        .unwrap();
    assert_eq!(back.get("title"), Some(&text("Rails < bolts")));
    assert_eq!(back.get("open"), Some(&Value::Bool(false)));
    let rate = inputs(&[("rate", Value::Decimal(Decimal::parse("3.75").unwrap()))]);
    write(&scene, &store, 4, "example_plist_rate_set", &rate, || {})
        .await
        .unwrap();
    let expected = seed_with(
        seed::SETTINGS_PLIST,
        &[
            ("Rails &amp; brackets", "Rails &lt; bolts"),
            ("<integer>103</integer>", "<integer>104</integer>"),
            ("<true/>", "<false/>"),
            ("<real>2.50</real>", "<real>3.75</real>"),
        ],
    );
    assert_eq!(
        scene.stand.bytes(seed::SETTINGS_PLIST).unwrap().unwrap(),
        expected
    );
    assert!(scene.stand.strays().unwrap().is_empty());
}

#[tokio::test]
async fn a_settings_value_the_file_cannot_hold_as_written_is_refused_and_nothing_changes() {
    let scene = Scene::new();
    let store = store().await;
    for (caller, prefix) in [(1, "B- ; a comment"), (2, "\"B-"), (3, "#B")] {
        let values = inputs(&[("prefix", text(prefix)), ("next_number", Value::Int(103))]);
        let failure = write(
            &scene,
            &store,
            caller,
            "example_ini_orders_set",
            &values,
            || {},
        )
        .await
        .unwrap_err();
        assert_eq!(failure.sent, a2m_provider::Sent::NotSent, "{prefix:?}");
        assert_eq!(
            scene.stand.bytes(seed::SETTINGS_INI).unwrap().unwrap(),
            seed_bytes(seed::SETTINGS_INI)
        );
    }
    assert!(scene.stand.strays().unwrap().is_empty());
}

// ---- a folder as a collection ----

#[tokio::test]
async fn a_folder_lists_its_entries_in_order_and_reads_one_by_its_name() {
    let scene = Scene::new();
    let list = read(&scene, "example_inbox_list", &Values::new())
        .await
        .unwrap();
    let names: Vec<&str> = list
        .get("name")
        .and_then(Value::as_list)
        .unwrap()
        .iter()
        .map(|name| name.as_text().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "later",
            "later/note-3.txt",
            "note-1.txt",
            "note-2.txt",
            "scan.bin"
        ]
    );
    let kinds = list.get("kind").and_then(Value::as_list).unwrap();
    assert_eq!(kinds[0], text("folder"));
    assert_eq!(kinds[2], text("file"));
    let digests = list.get("digest").and_then(Value::as_list).unwrap();
    assert_eq!(digests[0], Value::Null);
    assert_eq!(
        digests[2].as_text(),
        scene.stand.sha256(seed::INBOX_NOTE_ONE).unwrap().as_deref()
    );
    let sizes = list.get("size").and_then(Value::as_list).unwrap();
    assert_eq!(
        sizes[3],
        Value::Int(i64::try_from(seed_bytes(seed::INBOX_NOTE_TWO).len()).unwrap())
    );
    assert_eq!(list.get("truncated"), Some(&Value::Bool(false)));

    let get = |name: &str| inputs(&[("name", text(name))]);
    let two = read(&scene, "example_inbox_get", &get("note-2.txt"))
        .await
        .unwrap();
    assert_eq!(
        two.get("content"),
        Some(&text("Second note,\r\ntwo lines."))
    );
    let deep = read(&scene, "example_inbox_get", &get("later/note-3.txt"))
        .await
        .unwrap();
    assert_eq!(
        deep.get("content"),
        Some(&text("A note one folder down.\n"))
    );
    let outcomes = scene
        .read_operation("example_inbox_get", &get("missing.txt"))
        .await
        .unwrap();
    assert!(matches!(outcomes.last(), Some(StepOutcome::Absent(_))));

    // Each refusal by its name.
    for (name, expected) in [
        (
            "scan.bin",
            (FailureClass::UnknownVariant, "value_unreadable"),
        ),
        ("../outside.csv", (FailureClass::Input, "path_not_allowed")),
        ("NOTE-1.TXT", (FailureClass::Input, "name_case_conflict")),
        ("later", (FailureClass::Input, "path_not_allowed")),
        ("later/x/y.txt", (FailureClass::Input, "path_not_allowed")),
        ("con.txt", (FailureClass::Input, "path_not_allowed")),
    ] {
        let failure = read(&scene, "example_inbox_get", &get(name))
            .await
            .unwrap_err();
        assert_eq!(code(&failure), expected, "{name}");
    }
    // A link that leads out is listed by its kind and never followed.
    if scene
        .stand
        .set_fault(Fault::LinkOut {
            at: "inbox/out.csv".to_owned(),
        })
        .is_ok()
    {
        let list = read(&scene, "example_inbox_list", &Values::new())
            .await
            .unwrap();
        let names = list.get("name").and_then(Value::as_list).unwrap();
        let at = names
            .iter()
            .position(|name| name == &text("out.csv"))
            .unwrap();
        assert_eq!(
            list.get("kind").and_then(Value::as_list).unwrap()[at],
            text("link")
        );
        assert_eq!(
            list.get("digest").and_then(Value::as_list).unwrap()[at],
            Value::Null
        );
        let failure = read(&scene, "example_inbox_get", &get("out.csv"))
            .await
            .unwrap_err();
        assert_eq!(code(&failure), (FailureClass::Input, "path_not_allowed"));
    }
    assert!(scene.stand.outside_untouched().unwrap());
}

#[tokio::test]
async fn a_file_is_created_only_where_no_entry_has_its_name() {
    let scene = Scene::new();
    let store = store().await;
    let add =
        |name: &str, content: &str| inputs(&[("name", text(name)), ("content", text(content))]);
    let back = write(
        &scene,
        &store,
        1,
        "example_inbox_add",
        &add("note-4.txt", "Hello\nthere"),
        || {},
    )
    .await
    .unwrap();
    assert_eq!(back.get("content"), Some(&text("Hello\nthere")));
    assert_eq!(
        scene.stand.bytes("inbox/note-4.txt").unwrap().unwrap(),
        b"Hello\nthere"
    );
    // Nothing of the temporary file is left.
    assert_eq!(scene.stand.strays().unwrap(), ["inbox/note-4.txt"]);

    // A name the folder holds, in that case or another, is not taken.
    for (caller, name, expected) in [
        (
            2,
            "note-1.txt",
            (FailureClass::Stale, "changed_since_preview"),
        ),
        (4, "Note-1.txt", (FailureClass::Input, "name_case_conflict")),
    ] {
        let failure = write(
            &scene,
            &store,
            caller,
            "example_inbox_add",
            &add(name, "x"),
            || {},
        )
        .await
        .unwrap_err();
        assert_eq!(code(&failure), expected, "{name}");
    }
    assert_eq!(
        scene.stand.bytes(seed::INBOX_NOTE_ONE).unwrap().unwrap(),
        seed_bytes(seed::INBOX_NOTE_ONE)
    );

    // A file another program puts at the name after the look is never
    // replaced: the second name is given only where nothing has the name.
    let other = scene.stand.path("inbox/note-5.txt").unwrap();
    let failure = write(
        &scene,
        &store,
        3,
        "example_inbox_add",
        &add("note-5.txt", "ours"),
        || {
            std::fs::write(&other, b"theirs").unwrap();
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        code(&failure),
        (FailureClass::Stale, "changed_since_preview")
    );
    assert_eq!(failure.sent, a2m_provider::Sent::NotSent);
    assert_eq!(std::fs::read(&other).unwrap(), b"theirs");
    let strays = scene.stand.strays().unwrap();
    assert_eq!(strays, ["inbox/note-4.txt", "inbox/note-5.txt"]);
}

#[tokio::test]
async fn a_file_is_replaced_only_while_it_is_the_one_that_was_read() {
    let scene = Scene::new();
    let store = store().await;
    let set = |content: &str| inputs(&[("name", text("note-1.txt")), ("content", text(content))]);
    let before = scene.stand.inspect(seed::INBOX_NOTE_ONE).unwrap();
    write(
        &scene,
        &store,
        1,
        "example_inbox_set",
        &set("Replaced.\n"),
        || {},
    )
    .await
    .unwrap();
    let after = scene.stand.inspect(seed::INBOX_NOTE_ONE).unwrap();
    assert_eq!(
        scene.stand.bytes(seed::INBOX_NOTE_ONE).unwrap().unwrap(),
        b"Replaced.\n"
    );
    assert_eq!(after.mode, before.mode);
    assert!(scene.stand.strays().unwrap().is_empty());

    // Another writer between the read and the save: not replaced.
    let failure = write(
        &scene,
        &store,
        2,
        "example_inbox_set",
        &set("Ours.\n"),
        || {
            scene
                .stand
                .set_fault(Fault::ChangedByAnother {
                    file: seed::INBOX_NOTE_ONE.to_owned(),
                    change: Change::Whole(b"Theirs.\n".to_vec()),
                    how: SaveStyle::InPlace,
                })
                .unwrap();
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        code(&failure),
        (FailureClass::Stale, "changed_since_preview")
    );
    assert_eq!(
        scene.stand.bytes(seed::INBOX_NOTE_ONE).unwrap().unwrap(),
        b"Theirs.\n"
    );
    assert!(scene.stand.strays().unwrap().is_empty());
}

#[tokio::test]
async fn an_entry_is_renamed_in_its_folder_and_keeps_its_identity() {
    let scene = Scene::new();
    let store = store().await;
    let before = scene.stand.inspect(seed::INBOX_NOTE_ONE).unwrap();
    let values = inputs(&[
        ("name", text("note-1.txt")),
        ("new_name", text("note-9.txt")),
    ]);
    let back = write(&scene, &store, 1, "example_inbox_rename", &values, || {})
        .await
        .unwrap();
    assert_eq!(back.get("new_name"), Some(&text("note-9.txt")));
    assert_eq!(
        scene.stand.inspect(seed::INBOX_NOTE_ONE).unwrap().kind,
        EntryKind::Absent
    );
    let after = scene.stand.inspect("inbox/note-9.txt").unwrap();
    assert_eq!(after.sha256, before.sha256);
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.links, before.links);

    // A new name the folder holds is never taken, and neither file moves.
    let values = inputs(&[
        ("name", text("note-9.txt")),
        ("new_name", text("note-2.txt")),
    ]);
    let failure = write(&scene, &store, 2, "example_inbox_rename", &values, || {})
        .await
        .unwrap_err();
    assert_eq!(
        code(&failure),
        (FailureClass::Stale, "changed_since_preview")
    );
    assert_eq!(
        scene.stand.sha256("inbox/note-9.txt").unwrap(),
        before.sha256
    );
    assert_eq!(
        scene.stand.bytes(seed::INBOX_NOTE_TWO).unwrap().unwrap(),
        seed_bytes(seed::INBOX_NOTE_TWO)
    );
    // A new name that leaves the folder is refused.
    let values = inputs(&[
        ("name", text("note-9.txt")),
        ("new_name", text("../note.txt")),
    ]);
    let failure = write(&scene, &store, 3, "example_inbox_rename", &values, || {})
        .await
        .unwrap_err();
    assert_eq!(code(&failure), (FailureClass::Input, "path_not_allowed"));
    assert_eq!(scene.stand.strays().unwrap(), ["inbox/note-9.txt"]);
}

#[tokio::test]
async fn an_entry_is_removed_irreversibly_and_a_folder_only_when_empty() {
    let scene = Scene::new();
    let store = store().await;
    let operation = scene
        .plan
        .operations
        .iter()
        .find(|found| found.name.as_str() == "example_inbox_remove")
        .unwrap();
    assert_eq!(operation.effect, a2m_core::EffectClass::Irreversible);
    let remove = |name: &str| inputs(&[("name", text(name))]);
    write(
        &scene,
        &store,
        1,
        "example_inbox_remove",
        &remove("note-2.txt"),
        || {},
    )
    .await
    .unwrap();
    assert_eq!(
        scene.stand.inspect(seed::INBOX_NOTE_TWO).unwrap().kind,
        EntryKind::Absent
    );
    let failure = write(
        &scene,
        &store,
        2,
        "example_inbox_remove",
        &remove("later"),
        || {},
    )
    .await
    .unwrap_err();
    assert_eq!(code(&failure), (FailureClass::Policy, "folder_not_empty"));
    assert_eq!(
        scene.stand.bytes(seed::INBOX_DEEP).unwrap().unwrap(),
        seed_bytes(seed::INBOX_DEEP)
    );
    std::fs::create_dir(scene.stand.path("inbox/empty").unwrap()).unwrap();
    write(
        &scene,
        &store,
        3,
        "example_inbox_remove",
        &remove("empty"),
        || {},
    )
    .await
    .unwrap();
    assert_eq!(
        scene.stand.inspect("inbox/empty").unwrap().kind,
        EntryKind::Absent
    );
}

// ---- a growing file ----

fn log_read(position: Option<&Value>) -> Values {
    inputs(&[("position", position.cloned().unwrap_or(Value::Null))])
}

#[tokio::test]
async fn a_growing_file_is_read_from_where_the_last_read_stopped() {
    let scene = Scene::new();
    let first = read(&scene, "example_orders_log_read", &log_read(None))
        .await
        .unwrap();
    assert_eq!(
        first.get("lines").and_then(Value::as_list).unwrap(),
        [
            text("2026-11-18T08:00:00 order A-100 drafted"),
            text("2026-11-18T08:01:00 order A-100 submitted"),
        ]
    );
    assert_eq!(first.get("outcome"), Some(&text("read")));
    let position = first.get("position").cloned().unwrap();
    // The unfinished line is not a line yet.
    let again = read(
        &scene,
        "example_orders_log_read",
        &log_read(Some(&position)),
    )
    .await
    .unwrap();
    assert!(
        again
            .get("lines")
            .and_then(Value::as_list)
            .unwrap()
            .is_empty()
    );
    assert_eq!(again.get("position"), Some(&position));

    // Another program finishes the line and adds two more; the read gives
    // the first two of them, the declared bound, and says more are there.
    scene
        .stand
        .set_fault(Fault::ChangedByAnother {
            file: seed::ORDERS_LOG.to_owned(),
            change: Change::Appended(
                b"fted\n2026-11-18T08:03:00 order A-102 drafted\n2026-11-18T08:04:00 order A-102 submitted\n".to_vec(),
            ),
            how: SaveStyle::InPlace,
        })
        .unwrap();
    let bytes = scene.stand.bytes(seed::ORDERS_LOG).unwrap();
    let next = read(
        &scene,
        "example_orders_log_read",
        &log_read(Some(&position)),
    )
    .await
    .unwrap();
    assert_eq!(
        next.get("lines").and_then(Value::as_list).unwrap(),
        [
            text("2026-11-18T08:02:00 order A-101 drafted"),
            text("2026-11-18T08:03:00 order A-102 drafted"),
        ]
    );
    assert_eq!(next.get("more"), Some(&Value::Bool(true)));
    let last = read(
        &scene,
        "example_orders_log_read",
        &log_read(next.get("position")),
    )
    .await
    .unwrap();
    assert_eq!(
        last.get("lines").and_then(Value::as_list).unwrap(),
        [text("2026-11-18T08:04:00 order A-102 submitted")]
    );
    assert_eq!(last.get("more"), Some(&Value::Bool(false)));
    // Reading wrote nothing.
    assert_eq!(scene.stand.bytes(seed::ORDERS_LOG).unwrap(), bytes);
    assert!(scene.stand.strays().unwrap().is_empty());

    // A position no read gave is refused by name.
    let failure = read(
        &scene,
        "example_orders_log_read",
        &log_read(Some(&text("v1.x"))),
    )
    .await
    .unwrap_err();
    assert_eq!(code(&failure), (FailureClass::Input, "value_unparsable"));
}

#[tokio::test]
async fn a_growing_file_replaced_cut_short_or_holding_bad_text_is_told_and_never_read_over() {
    let scene = Scene::new();
    let first = read(&scene, "example_orders_log_read", &log_read(None))
        .await
        .unwrap();
    let position = first.get("position").cloned().unwrap();

    // A line that is not UTF-8, after ones that are (the first ends the
    // seed's partial line): the lines before it are given, then the bad line is told by the outcome and passed, and the
    // line after it is read.
    scene
        .stand
        .set_fault(Fault::ChangedByAnother {
            file: seed::ORDERS_LOG.to_owned(),
            change: Change::Appended(b"fted\ngood\nft \xff\xfe\nafter\n".to_vec()),
            how: SaveStyle::InPlace,
        })
        .unwrap();
    let before = read(
        &scene,
        "example_orders_log_read",
        &log_read(Some(&position)),
    )
    .await
    .unwrap();
    assert_eq!(
        before.get("lines").and_then(Value::as_list).unwrap(),
        [
            text("2026-11-18T08:02:00 order A-101 drafted"),
            text("good")
        ]
    );
    assert_eq!(before.get("outcome"), Some(&text("read")));
    assert_eq!(before.get("more"), Some(&Value::Bool(true)));
    let bad = read(
        &scene,
        "example_orders_log_read",
        &log_read(before.get("position")),
    )
    .await
    .unwrap();
    assert_eq!(bad.get("outcome"), Some(&text("unreadable")));
    assert!(
        bad.get("lines")
            .and_then(Value::as_list)
            .unwrap()
            .is_empty()
    );
    assert_eq!(bad.get("more"), Some(&Value::Bool(true)));
    let after = read(
        &scene,
        "example_orders_log_read",
        &log_read(bad.get("position")),
    )
    .await
    .unwrap();
    assert_eq!(
        after.get("lines").and_then(Value::as_list).unwrap(),
        [text("after")]
    );

    // Cut short, in place: told as `truncated`, and nothing is read.
    scene
        .stand
        .set_fault(Fault::ChangedByAnother {
            file: seed::ORDERS_LOG.to_owned(),
            change: Change::Whole(b"2026-11-18T08:00:00 order A-100 drafted\n".to_vec()),
            how: SaveStyle::InPlace,
        })
        .unwrap();
    let cut = read(
        &scene,
        "example_orders_log_read",
        &log_read(Some(&position)),
    )
    .await
    .unwrap();
    assert_eq!(cut.get("outcome"), Some(&text("truncated")));
    assert!(
        cut.get("lines")
            .and_then(Value::as_list)
            .unwrap()
            .is_empty()
    );
    assert_eq!(cut.get("position"), Some(&position));

    // Another file put at the name, longer than the position: told as
    // `replaced`, never read from the old position.
    let mut other = b"2026-11-19T00:00:00 a new log begins\n".to_vec();
    other.extend_from_slice(&[b'x'; 200]);
    other.push(b'\n');
    scene
        .stand
        .set_fault(Fault::ChangedByAnother {
            file: seed::ORDERS_LOG.to_owned(),
            change: Change::Whole(other),
            how: SaveStyle::Replace,
        })
        .unwrap();
    let replaced = read(
        &scene,
        "example_orders_log_read",
        &log_read(Some(&position)),
    )
    .await
    .unwrap();
    assert_eq!(replaced.get("outcome"), Some(&text("replaced")));
    assert!(
        replaced
            .get("lines")
            .and_then(Value::as_list)
            .unwrap()
            .is_empty()
    );
    assert_eq!(replaced.get("position"), Some(&position));
}
