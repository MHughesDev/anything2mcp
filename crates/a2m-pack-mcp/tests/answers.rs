//! What copy test 6 found, each against the HTTP form of the test stand:
//! a read sends the key its mapping names, a server that answers with JSON
//! as text is read one item a row and its write is read back, a save's
//! offline request is recorded under the name its fixture file carries, and
//! the earlier handshake revision is spoken. Every write is proven by
//! asking the stand what it stored.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{CompiledTarget, PackPayload, Step, Value, Values};
use a2m_provider::{
    Authority, Posture, Recorded, RecordedAnswer, Replay, Sent, StepOutcome, answer_name,
};
use common::{
    Folder, compile, fence_of, input, key, listed_pins, prepare, record, registry, remapped,
    running, shown, steps, store, surface, token, write_target,
};
use example_mcp::faults::Drift;
use example_mcp::harness::HttpStand;

/// The values of a step's arguments, each of which is an input by name, as
/// the engine evaluates them: so a test sends what the compiled target says
/// is sent.
fn evaluated(compiled_target: &CompiledTarget, step: &Step, inputs: &Values) -> Values {
    let mut args = Values::new();
    for arg in &step.args {
        let source = &compiled_target.expression(arg.value).unwrap().source;
        let name = source
            .strip_prefix("input.")
            .unwrap_or_else(|| panic!("the argument `{}` is `{source}`", arg.name));
        args.insert(arg.name.clone(), inputs.get(name).unwrap().clone());
    }
    args
}

/// The fixture target with `edit` applied, compiled.
fn compiled_target_with(folder: &Folder, edit: &dyn Fn(&str, String) -> String) -> CompiledTarget {
    let registry = registry(folder);
    let root = folder.path().join("target");
    write_target(&root, edit);
    match compile(&registry, &root) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!("the target does not compile:\n{}", shown(&problems)),
    }
}

fn done(outcome: Result<StepOutcome, a2m_provider::StepFailure>) -> a2m_provider::Observed {
    match outcome {
        Ok(StepOutcome::Done(observed)) => observed,
        other => panic!("the step was not done: {other:?}"),
    }
}

/// Finding 38: a read whose binding states no `args` sends the key its
/// mapping names under `key_args`, from the operation's key input, as the
/// compiled target says it does, and the stand receives it.
#[tokio::test]
async fn a_read_with_no_args_sends_the_key_its_mapping_names() {
    let folder = Folder::new("answers-key");
    let compiled_target = compiled_target_with(&folder, &|path, text| {
        if path == "operations/order_get.yaml" {
            text.replace("    args: {order_id: input.order_id}\n", "")
        } else {
            text
        }
    });
    let get = steps(&compiled_target, "example_order_get").act;
    let names: Vec<&str> = get.args.iter().map(|arg| arg.name.as_str()).collect();
    assert_eq!(names, ["order_id"]);

    let stand = HttpStand::start().await.unwrap();
    let registry = registry(&folder);
    let at = surface(&compiled_target, "orders", Some(&stand.url()));
    let (handle, mut prepared) = prepare(
        &registry,
        &compiled_target,
        &at,
        fence_of(3),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let args = evaluated(
        &compiled_target,
        &get,
        &Values::new().with("order_id", "A-100"),
    );
    let read = done(
        handle
            .act(
                &mut prepared,
                &get,
                &input(&args, None, None, None),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(read.values.get("status"), Some(&Value::from("draft")));
    let calls = stand.control().calls_to("orders_get").unwrap();
    assert_eq!(calls, 1);
    let sent = &stand.control().calls().unwrap()[0].arguments;
    assert_eq!(sent, &serde_json::json!({"order_id": "A-100"}));
    handle.release(prepared, false).await;
}

/// A compiled target whose `orders_get` and `orders_list` are mapped as tools
/// that answer with JSON as text, pinned to their definitions with no output
/// schema, as the stand lists them under the `text_rows` drift.
async fn text_rows_compiled_target(
    stand: &HttpStand,
    compiled_target: &CompiledTarget,
) -> CompiledTarget {
    let control = stand.control();
    for tool in ["orders_get", "orders_list"] {
        control
            .set_drift(Drift::TextRows { tool: tool.into() })
            .unwrap();
    }
    let pins = listed_pins(&stand.url()).await;
    let pin = |tool: &str| {
        pins.iter()
            .find(|(name, _)| name == tool)
            .map(|(_, digest)| digest.clone())
            .unwrap()
    };
    let (get_pin, list_pin) = (pin("orders_get"), pin("orders_list"));
    let compiled_target = remapped(compiled_target, "orders", "orders_get", |entry| {
        entry["digest"] = serde_json::json!(get_pin);
        entry["result"] = serde_json::json!("json_text");
        entry["records"] = serde_json::json!("rows");
        entry["absent_error"] = serde_json::Value::Null;
    });
    remapped(&compiled_target, "orders", "orders_list", |entry| {
        entry["digest"] = serde_json::json!(list_pin);
        entry["result"] = serde_json::json!("json_text");
    })
}

/// Finding 37: a list whose server answers one content item a row gives
/// each row, a read by key the one row, and a key the server does not hold
/// is absent, not an empty text that completes.
#[tokio::test]
async fn json_as_text_is_read_one_item_a_row_and_none_is_absent() {
    let folder = Folder::new("answers-rows");
    let registry = registry(&folder);
    let fixture = common::compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let compiled_target = text_rows_compiled_target(&stand, &fixture).await;
    let at = surface(&compiled_target, "orders", Some(&stand.url()));
    let (handle, mut prepared) = prepare(
        &registry,
        &compiled_target,
        &at,
        fence_of(4),
        Posture::Live,
        None,
    )
    .await
    .unwrap();

    // The list: every row, under the member the mapping names.
    let mut list = steps(&compiled_target, "example_order_create")
        .lookup
        .unwrap();
    list.lookup = false;
    let none = Values::new();
    let listed = done(
        handle
            .act(
                &mut prepared,
                &list,
                &input(&none, None, None, None),
                Authority::Read,
            )
            .await,
    );
    let Some(Value::List(rows)) = listed.values.get("orders") else {
        panic!("no rows: {:?}", listed.values);
    };
    assert_eq!(rows.len(), 3);

    // A read by key: the one row; a key the server does not hold: absent.
    let get = steps(&compiled_target, "example_order_get").act;
    let one = done(
        handle
            .act(
                &mut prepared,
                &get,
                &input(&Values::new().with("order_id", "A-101"), None, None, None),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(one.values.get("status"), Some(&Value::from("submitted")));
    let absent = handle
        .act(
            &mut prepared,
            &get,
            &input(&Values::new().with("order_id", "A-999"), None, None, None),
            Authority::Read,
        )
        .await;
    assert!(matches!(absent, Ok(StepOutcome::Absent(_))), "{absent:?}");
    handle.release(prepared, false).await;
}

/// Finding 39: a write whose read tool answers with JSON as text is read
/// back with fields, compared with what the stand stored.
#[tokio::test]
async fn a_write_is_read_back_through_a_tool_that_answers_json_as_text() {
    let folder = Folder::new("answers-readback");
    let registry = registry(&folder);
    let fixture = common::compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    let compiled_target = text_rows_compiled_target(&stand, &fixture).await;
    let at = surface(&compiled_target, "orders", Some(&stand.url()));
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) =
        prepare(&registry, &compiled_target, &at, fence, Posture::Live, None)
            .await
            .unwrap();
    let update = steps(&compiled_target, "example_order_note_update");
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "Leave at the gate");
    let order = key("A-100");
    let intent = token(&ledger, fence, 0, "A-100").await;
    done(
        handle
            .act(
                &mut prepared,
                &update.act,
                &input(&args, Some(record(&fence, 0)), Some(&order), None),
                Authority::Write(&intent),
            )
            .await,
    );
    let stored = stand.control().stored_order("A-100").unwrap().unwrap();
    assert_eq!(stored.note, "Leave at the gate");
    assert_eq!(stand.control().write_count("A-100").unwrap(), 1);

    let none = Values::new();
    let read = done(
        handle
            .act(
                &mut prepared,
                &update.read_back.unwrap(),
                &input(&none, Some(record(&fence, 0)), Some(&order), None),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(
        read.values.get("note").and_then(Value::as_text),
        Some(stored.note.as_str())
    );
    handle.release(prepared, false).await;
}

/// Finding 39, at lint: a write whose read tool's result is `text` reads
/// back no field, and the mapping is refused where it is written.
#[test]
fn a_write_read_back_through_text_alone_is_refused_at_lint() {
    let folder = Folder::new("answers-lint");
    let registry = registry(&folder);
    let root = folder.path().join("target");
    write_target(&root, &|path, text| {
        if path == "mappings/orders.yaml" {
            text.replace(
                "    absent_error: \"There is no order\"\n",
                "    absent_error: \"There is no order\"\n    result: text\n",
            )
        } else {
            text
        }
    });
    let problems = compile(&registry, &root).expect_err("the mapping must be refused");
    let refused: Vec<String> = problems
        .iter()
        .filter(|problem| problem.code.as_str() == "mcp.write_no_readback")
        .map(ToString::to_string)
        .collect();
    assert_eq!(refused.len(), 2, "{}", shown(&problems));
}

/// Finding 40: the offline test of an MCP write records its save under the
/// name a fixture's file carries, the tool's own name, so the request in
/// `<tool>.request.json` is the one compared with what was sent.
#[tokio::test]
async fn a_saves_offline_request_is_recorded_under_its_fixture_files_name() {
    let folder = Folder::new("answers-replay");
    let registry = registry(&folder);
    let compiled_target = common::compiled_fixture(&registry);
    let update = steps(&compiled_target, "example_order_note_update").act;
    assert!(update.commits);
    assert_eq!(answer_name(&update), "orders_update");
    let replay = Replay::new(vec![RecordedAnswer {
        name: "orders_update".to_owned(),
        args: None,
        answer: Recorded::Done(Values::new()),
        repeats: false,
    }]);
    let args = Values::new().with("order_id", "A-100");
    let order = key("A-100");
    let fence = fence_of(5);
    let mut prepared = a2m_provider::Prepared::new(());
    let outcome = a2m_provider::Driver::act(
        replay.as_ref(),
        &mut prepared,
        &update,
        &input(&args, Some(record(&fence, 0)), Some(&order), None),
    )
    .await;
    assert!(matches!(outcome, Ok(StepOutcome::Done(_))), "{outcome:?}");
    assert_eq!(replay.sent(), vec![("orders_update".to_owned(), args)]);
}

/// The earlier handshake revision, 2025-06-18, is spoken when the surface
/// states it, against a server that serves it alone; a surface that states
/// the newer one is told the server offers another, a named stop.
#[tokio::test]
async fn the_earlier_handshake_revision_is_spoken_when_the_surface_states_it() {
    let folder = Folder::new("answers-earlier");
    let registry = registry(&folder);
    let compiled_target = common::compiled_fixture(&registry);
    let stand = HttpStand::start().await.unwrap();
    stand.control().serve_earlier_revision_only(true).unwrap();
    let mut earlier = surface(&compiled_target, "orders", Some(&stand.url()));
    earlier.surface.payload =
        PackPayload::encode(&serde_json::json!({"revision": "2025-06-18"})).unwrap();
    let (handle, mut prepared) = prepare(
        &registry,
        &compiled_target,
        &earlier,
        fence_of(6),
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let get = steps(&compiled_target, "example_order_get").act;
    let read = done(
        handle
            .act(
                &mut prepared,
                &get,
                &input(&Values::new().with("order_id", "A-102"), None, None, None),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(read.values.get("reference"), Some(&Value::from("REF-102")));
    handle.release(prepared, false).await;
    let revisions: Vec<Option<String>> = stand
        .control()
        .connections()
        .unwrap()
        .into_iter()
        .map(|connection| connection.revision)
        .collect();
    assert_eq!(revisions, [Some("2025-06-18".to_owned())]);

    // The newer revision, stated against a server that serves only the
    // earlier one: refused when the server offers another.
    let newer = surface(&compiled_target, "orders", Some(&stand.url()));
    let refused = prepare(
        &registry,
        &compiled_target,
        &newer,
        fence_of(8),
        Posture::Live,
        None,
    )
    .await;
    let failure = match refused {
        Err(failure) => failure,
        Ok((handle, mut prepared)) => handle
            .act(
                &mut prepared,
                &get,
                &input(&Values::new().with("order_id", "A-102"), None, None, None),
                Authority::Read,
            )
            .await
            .expect_err("the newer revision is not spoken with this server"),
    };
    assert_eq!(failure.failure.code().as_str(), "unexpected_status");
    assert_ne!(failure.sent, Sent::Unknown);
    assert_eq!(stand.control().calls_to("orders_get").unwrap(), 1);
}
