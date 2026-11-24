//! Case 10, the custom controls, and the grid of case 9 read by a row's key,
//! through the driver and the screens of `tests/fixtures/cases`, against the
//! test application's pages `/cases/controls` and `/cases/grid`. Every proof
//! asks the application what its page was told (the page tells its own state,
//! never what the controls show), and never the driver's own account.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, NoSecrets, Value, Values};
use a2m_provider::{DriverHandle, Posture, Prepared, Sent, StepFailure};
use common::{Scene, Stand, Write, compile};
use example_app::harness::TestApp;

async fn scene(name: &str, app: &TestApp) -> Scene {
    let stand = Stand::new(name, Arc::new(NoSecrets));
    let compiled_target = compile(&stand.registry, "cases").compiled_target;
    Scene::over(stand, compiled_target, &app.url(""))
}

/// What the application was told, once it has been told at least `count`
/// things.
async fn told(app: &TestApp, count: usize) -> Vec<String> {
    let started = std::time::Instant::now();
    loop {
        let events = app.case_events().await.unwrap();
        if events.len() >= count || started.elapsed() > Duration::from_secs(20) {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// What the page last said it holds, as JSON: the `saved` event.
fn saved(events: &[String]) -> serde_json::Value {
    let last = events
        .iter()
        .rev()
        .find_map(|event| event.strip_prefix("saved: "))
        .expect("the page was told to save");
    serde_json::from_str(last).unwrap()
}

async fn writing(scene: &Scene) -> (DriverHandle, Prepared, Write) {
    let write = Write::new(&["controls"]).await;
    let (handle, prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    (handle, prepared, write)
}

/// Runs the steps of an operation's binding that write, on a fresh page.
async fn write_steps(
    scene: &Scene,
    handle: &DriverHandle,
    prepared: &mut Prepared,
    write: &Write,
    operation: &str,
    inputs: &Values,
) -> Result<common::Ran, StepFailure> {
    let steps = &scene.binding(operation).act;
    scene
        .run(handle, prepared, steps, inputs, Some(write.authority()))
        .await
}

/// Reads on the page the driver is on: the steps of a read operation after
/// its own `goto` and `wait`, which would load the page afresh.
async fn read_here(
    scene: &Scene,
    handle: &DriverHandle,
    prepared: &mut Prepared,
    operation: &str,
    inputs: &Values,
) -> Result<common::Ran, StepFailure> {
    let steps = &scene.binding(operation).act[2..];
    scene.run(handle, prepared, steps, inputs, None).await
}

fn options() -> Values {
    Values::new()
        .with("carrier", "nw")
        .with("destination", "Harbor View")
        .with("pickup", "2026-12-01T09:30")
        .with("cutoff", "17:30")
        .with("level", "7")
        .with("notes", "Fragile, keep dry")
        .with("insured", true)
        .with("section", "handling")
        .with("panel", "customs")
        .with("location", "Warehouse/North/Bay 2")
}

/// Every custom control of case 10 but the file and the grid is set, saved,
/// and read back. The page says what it holds; the driver's read agrees.
#[tokio::test(flavor = "multi_thread")]
async fn each_custom_control_is_set_saved_and_read_back() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-controls", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    let inputs = options();
    write_steps(
        &scene,
        &handle,
        &mut prepared,
        &write,
        "example_options_submit",
        &inputs,
    )
    .await
    .unwrap();
    let back = read_here(
        &scene,
        &handle,
        &mut prepared,
        "example_options_read",
        &Values::new(),
    )
    .await
    .unwrap();
    assert!(handle.release(prepared, false).await.complete);

    let events = told(&app, 1).await;
    assert_eq!(events.len(), 1, "{events:?}");
    let held = saved(&events);
    for (name, wanted) in [
        ("carrier", serde_json::json!("nw")),
        ("destination", serde_json::json!("Harbor View")),
        ("pickup", serde_json::json!("2026-12-01T09:30")),
        ("cutoff", serde_json::json!("17:30")),
        ("level", serde_json::json!("7")),
        ("notes", serde_json::json!("Fragile, keep dry")),
        ("insured", serde_json::json!(true)),
        ("section", serde_json::json!("handling")),
        ("panel", serde_json::json!("customs")),
        ("location", serde_json::json!("Warehouse/North/Bay 2")),
    ] {
        assert_eq!(held[name], wanted, "the page holds {name}: {held}");
    }
    for (name, wanted) in [
        ("carrier", Value::from("nw")),
        ("destination", Value::from("Harbor View")),
        ("pickup", Value::from("2026-12-01T09:30")),
        ("cutoff", Value::from("17:30")),
        ("level", Value::from("7")),
        ("notes", Value::from("Fragile, keep dry")),
        ("insured", Value::from(true)),
        ("section", Value::from("handling")),
        ("panel", Value::from("customs")),
        ("location", Value::from("Warehouse/North/Bay 2")),
    ] {
        assert_eq!(back.values.get(name), Some(&wanted), "read {name}");
    }
    scene.stand.end().await;
}

/// A slider moves down as well as up, by its keys, and ends on the value.
/// The second pass is on a fresh page, where the slider starts at zero; a
/// value the slider cannot take stops the run.
#[tokio::test(flavor = "multi_thread")]
async fn a_slider_is_moved_by_its_keys_and_a_value_it_cannot_take_stops() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-slider", &app).await;
    let (handle, mut prepared, _first) = writing(&scene).await;
    for level in ["9", "3"] {
        let write = Write::new(&["controls", level]).await;
        write_steps(
            &scene,
            &handle,
            &mut prepared,
            &write,
            "example_options_submit",
            &options().with("level", level),
        )
        .await
        .unwrap();
    }
    // Eleven is past the end of a slider that runs from zero to ten: the
    // keys stop moving it before it gets there.
    let write = Write::new(&["controls", "11"]).await;
    let stopped = write_steps(
        &scene,
        &handle,
        &mut prepared,
        &write,
        "example_options_submit",
        &options().with("level", "11"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        stopped.failure.class(),
        FailureClass::UnknownVariant,
        "{stopped}"
    );
    handle.release(prepared, true).await;
    let events = told(&app, 2).await;
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(saved(&events[..1])["level"], serde_json::json!("9"));
    assert_eq!(saved(&events[1..])["level"], serde_json::json!("3"));
    scene.stand.end().await;
}

/// A file of the staging folder is given to the file input by its name; the
/// page is told its name and size. A path, a name that is not there, and
/// the name of a folder are refused before anything is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_staged_file_is_attached_and_a_path_or_a_missing_name_is_refused() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-file", &app).await;
    let staging = scene.stand.folder.path().join("profiles").join("staging");
    std::fs::create_dir_all(staging.join("a-folder")).unwrap();
    std::fs::write(staging.join("label.txt"), b"twelve bytes").unwrap();
    let (handle, mut prepared, write) = writing(&scene).await;
    write_steps(
        &scene,
        &handle,
        &mut prepared,
        &write,
        "example_attachment_submit",
        &Values::new().with("attachment", "label.txt"),
    )
    .await
    .unwrap();
    let back = read_here(
        &scene,
        &handle,
        &mut prepared,
        "example_attachment_read",
        &Values::new(),
    )
    .await
    .unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(
        back.values.get("attachment"),
        Some(&Value::from("label.txt"))
    );
    let events = told(&app, 1).await;
    assert_eq!(
        saved(&events)["attachment"],
        serde_json::json!("label.txt:12")
    );

    let before = app.case_events().await.unwrap().len();
    for refused in [
        "../label.txt",
        "/etc/hostname",
        "absent.txt",
        "a-folder",
        "",
    ] {
        let (handle, mut prepared, write) = writing(&scene).await;
        let stopped = write_steps(
            &scene,
            &handle,
            &mut prepared,
            &write,
            "example_attachment_submit",
            &Values::new().with("attachment", refused),
        )
        .await
        .unwrap_err();
        assert_eq!(stopped.sent, Sent::NotSent, "{refused}: {stopped}");
        handle.release(prepared, true).await;
    }
    assert_eq!(app.case_events().await.unwrap().len(), before);
    scene.stand.end().await;
}

/// A grid cell is addressed by the key of its row and the place of its
/// column: it is edited in place and read back, and the row that is not
/// there is a stop that sends nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_grid_cell_is_edited_and_read_by_the_key_of_its_row() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-grid-cell", &app).await;
    let (handle, mut prepared, write) = writing(&scene).await;
    write_steps(
        &scene,
        &handle,
        &mut prepared,
        &write,
        "example_parcel_count_submit",
        &Values::new().with("parcel", "P-200").with("count", "12"),
    )
    .await
    .unwrap();
    let mut counts = Vec::new();
    for parcel in ["P-100", "P-200", "P-300"] {
        let back = read_here(
            &scene,
            &handle,
            &mut prepared,
            "example_parcel_count_read",
            &Values::new().with("parcel", parcel),
        )
        .await
        .unwrap();
        counts.push(back.values.get("count").cloned().unwrap());
    }
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(
        counts,
        [Value::from("4"), Value::from("12"), Value::from("1")]
    );
    let events = told(&app, 1).await;
    assert_eq!(
        saved(&events)["counts"],
        serde_json::json!({"P-100": "4", "P-200": "12", "P-300": "1"})
    );

    let (handle, mut prepared, write) = writing(&scene).await;
    let stopped = write_steps(
        &scene,
        &handle,
        &mut prepared,
        &write,
        "example_parcel_count_submit",
        &Values::new().with("parcel", "P-999").with("count", "5"),
    )
    .await
    .unwrap_err();
    assert_eq!(stopped.failure.class(), FailureClass::Drift, "{stopped}");
    assert_eq!(stopped.sent, Sent::NotSent);
    handle.release(prepared, true).await;
    assert_eq!(app.case_events().await.unwrap().len(), 1);
    scene.stand.end().await;
}

/// Case 9, in the `ui` driver: a cell of the virtualized grid, read by the
/// key of its row, in the row the page holds. A row the page does not hold,
/// which only its finder brings in, is a stop; that grid is read through
/// the network (decision 0351).
#[tokio::test(flavor = "multi_thread")]
async fn a_virtualized_grid_is_read_by_row_key_where_the_page_holds_the_row() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-grid-read", &app).await;
    let (handle, mut prepared) = scene.try_prepare(None, Posture::Live).await.unwrap();
    let steps = &scene.binding("example_order_line_quantity_read").act;
    // The key `R-00003` is the third of ten thousand; its quantity is
    // (3 * 7) % 90 + 1, which the page computes.
    let held = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            &Values::new().with("line", "R-00003"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(held.values.get("quantity"), Some(&Value::from("22")));
    let far = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            &Values::new().with("line", "R-09000"),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(far.failure.class(), FailureClass::Drift, "{far}");
    assert!(handle.release(prepared, false).await.complete);
    assert!(app.case_events().await.unwrap().is_empty());
    scene.stand.end().await;
}

/// A choice the field does not list, a path the tree does not hold, and a
/// date field a person can only pick are stops, and the page is told
/// nothing: the first two are known before anything is sent to their
/// control, and the last is a control the text rule cannot type into.
#[tokio::test(flavor = "multi_thread")]
async fn a_choice_with_no_rule_is_a_named_stop_and_the_page_is_told_nothing() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("case-controls-refused", &app).await;
    for (operation, inputs, class) in [
        (
            "example_options_submit",
            options().with("carrier", "zz"),
            FailureClass::Input,
        ),
        (
            "example_options_submit",
            options().with("section", "nonesuch"),
            FailureClass::Input,
        ),
        (
            "example_options_submit",
            options().with("location", "Warehouse/East"),
            FailureClass::Drift,
        ),
        (
            "example_options_submit",
            options().with("location", "Warehouse//Bay 1"),
            FailureClass::Input,
        ),
        (
            "example_picker_submit",
            Values::new().with("picker", "2026-12-01"),
            FailureClass::UnknownVariant,
        ),
    ] {
        let (handle, mut prepared, write) = writing(&scene).await;
        let stopped = write_steps(&scene, &handle, &mut prepared, &write, operation, &inputs)
            .await
            .unwrap_err();
        assert_eq!(stopped.failure.class(), class, "{inputs:?}: {stopped}");
        handle.release(prepared, true).await;
    }
    assert!(app.case_events().await.unwrap().is_empty());
    scene.stand.end().await;
}

/// What the compiler refuses of the new keys of a screen and a binding: the
/// `options` of a control that has no choices, a `tabs` field with none, a
/// `grid` with no `column`, a `column` on any other control, a grid cell
/// with no row, and a row for a field that is not a grid cell.
#[test]
fn what_a_screen_or_a_binding_may_not_say_of_choices_columns_and_rows_is_refused() {
    let screen = "screens/controls.yaml";
    let operations = "operations/controls.yaml";
    let refused = |path: &str, change: &dyn Fn(&str) -> String, code: &str| {
        let stand = Stand::new("case-controls-compile", Arc::new(NoSecrets));
        let problems = common::compile_changed(&stand.registry, "cases", path, change)
            .err()
            .unwrap_or_else(|| panic!("the change compiled; `{code}` was expected"));
        let found = problems
            .find(code)
            .unwrap_or_else(|| panic!("no `{code}` in:\n{}", common::shown(&problems)))
            .clone();
        assert!(!found.fix.is_empty());
        assert_eq!(found.at.file.as_str(), path);
    };
    refused(
        screen,
        &|text| {
            text.replacen(
                "control: rich_text\n",
                "control: rich_text\n    options: {a: b}\n",
                1,
            )
        },
        "web.options_not_fitting",
    );
    refused(
        screen,
        &|text| {
            text.replacen(
                "    options: {overview: Overview, handling: Handling, history: History}\n",
                "",
                1,
            )
        },
        "web.options_not_fitting",
    );
    refused(
        screen,
        &|text| {
            text.replacen(
                "    column: 2\n    locate:\n      - {role: grid, name: Parcels}",
                "    locate:\n      - {role: grid, name: Parcels}",
                1,
            )
        },
        "web.column_not_fitting",
    );
    refused(
        screen,
        &|text| text.replacen("control: time\n", "control: time\n    column: 1\n", 1),
        "web.column_not_fitting",
    );
    refused(
        operations,
        &|text| text.replacen("    rows:\n      count: input.parcel\n", "", 1),
        "web.row_missing",
    );
    refused(
        operations,
        &|text| {
            text.replacen(
                "    rows:\n      count: input.parcel\n",
                "    rows:\n      count: input.parcel\n      carrier: input.parcel\n",
                1,
            )
        },
        "web.rows_not_fitting",
    );
}
