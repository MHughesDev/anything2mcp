//! An asserted read of a web surface (decision 0394, points 2 and 4): a page
//! request by a method that may write, which a person says changes nothing,
//! compiles only with a closed body, and the drivers hand it to the observing
//! tab only from a published version, live or in an environment of kind
//! `test` (decision 0270). Against the test application's case 39,
//! `/cases/rows-by-post`, whose route searches with `op: search` and
//! archives with `op: archive`; every proof asks the application's control
//! interface what it stored and what it heard. Under the drafts, an entry
//! its person confirmed on the engine's page is let through in a try in an
//! environment of kind `test`, and nowhere else (decision 0475).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{CompiledTarget, Environment, FailureClass, NoSecrets, Value, Values};
use a2m_provider::{ConfirmedReads, Posture, StepFailure};
use common::{Scene, Stand, compile, compile_changed, shown};
use example_app::harness::TestApp;

const ENTRY: &str = "page_requests/cases_rows_search.yaml";
const LIST: &str = "example_case_list";

fn class_and_code(failure: &StepFailure) -> (FailureClass, &str) {
    (failure.failure.class(), failure.failure.code().as_str())
}

/// Every problem of a fixture with one file changed, which must not compile.
fn refused(stand: &Stand, path: &str, change: impl Fn(&str) -> String) -> a2m_files::Diagnostics {
    compile_changed(&stand.registry, "rows", path, change)
        .err()
        .unwrap_or_else(|| panic!("`{path}` compiled"))
}

/// The same compiled target as a published version's: a draft is always
/// compiled as the draft version, and a published version under its own.
fn published(mut compiled_target: CompiledTarget) -> CompiledTarget {
    compiled_target.header.version = a2m_core::Version::new(1, 0, 0);
    compiled_target
}

#[test]
fn an_asserted_read_compiles_only_with_a_closed_body_and_a_method_that_may_write() {
    let stand = Stand::new("asserted-compile", Arc::new(NoSecrets));
    let compiled = compile(&stand.registry, "rows");
    assert!(
        compiled
            .compiled_target
            .packs
            .iter()
            .any(|section| section.kind == a2m_pack_web_ui::REQUEST_KIND)
    );
    // A read by `POST` without the person's word does not compile.
    let problems = refused(&stand, ENTRY, |text| {
        text.replace("read_asserted: true\n", "")
    });
    assert!(
        problems.find("web.request_effect_mismatch").is_some(),
        "{}",
        shown(&problems)
    );
    // Nor with an open body, or a body the schema leaves open by default.
    for open in ["additionalProperties: true", ""] {
        let problems = refused(&stand, ENTRY, |text| {
            text.replace("additionalProperties: false", open)
        });
        let found = problems
            .find("web.request_shape_invalid")
            .unwrap_or_else(|| panic!("`{open}`:\n{}", shown(&problems)));
        assert_eq!(found.at.file.as_str(), ENTRY);
    }
    // Nor with an encoding and no body.
    let problems = refused(&stand, ENTRY, |text| {
        let at = text.find("request_schema:").unwrap();
        let end = text.find("response_schema:").unwrap();
        format!("{}{}", &text[..at], &text[end..])
    });
    assert!(
        problems.find("web.read_sends").is_some(),
        "{}",
        shown(&problems)
    );
    // With no body at all it compiles: the request carries none.
    compile_changed(&stand.registry, "rows", ENTRY, |text| {
        let at = text.find("request_encoding:").unwrap();
        let end = text.find("response_schema:").unwrap();
        format!("{}{}", &text[..at], &text[end..])
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    // `read_asserted` on a read by `GET`, or on a write, is refused.
    let problems = refused(&stand, ENTRY, |text| {
        text.replace("method: POST", "method: GET")
    });
    assert!(
        problems.find("web.request_effect_mismatch").is_some(),
        "{}",
        shown(&problems)
    );
    let problems = refused(&stand, ENTRY, |text| {
        text.replace("effect: read", "effect: write")
    });
    assert!(
        problems.find("web.request_effect_mismatch").is_some(),
        "{}",
        shown(&problems)
    );
    // A form's values are texts, and an asserted read carries no token.
    let problems = refused(&stand, ENTRY, |text| {
        text.replace("request_encoding: json", "request_encoding: form")
            .replace(
                "status: {type: string, enum: [open]}",
                "status: {type: integer}",
            )
    });
    assert!(
        problems.find("web.request_shape_invalid").is_some(),
        "{}",
        shown(&problems)
    );
    let problems = refused(&stand, ENTRY, |text| {
        text.replace("effect: read\n", "effect: read\nanti_forgery: none\n")
    });
    assert!(
        problems.find("web.read_sends").is_some(),
        "{}",
        shown(&problems)
    );
    // The `page_http` mode sends one only with the body its entry declares,
    // every required field given (decision 0491).
    let problems = refused(&stand, "operations/rows_by_request.yaml", |_| {
        "operation: example_case_search\neffect: read\ninputs: {}\n\
         outputs:\n  rows: {type: untrusted_text}\n\
         bindings:\n  - {mode: page_http, verb: read, endpoint: cases_rows_search}\n\
         description: Sends the search itself.\n"
            .to_owned()
    });
    assert!(
        problems.find("web.body_incomplete").is_some(),
        "{}",
        shown(&problems)
    );
}

/// A `ui` list of the cases page, prepared in `posture` in the environment
/// named, with what it gave or why it stopped.
async fn list(
    app: &TestApp,
    compiled_target: CompiledTarget,
    environment: &str,
    posture: Posture,
) -> Result<common::Ran, StepFailure> {
    list_confirmed(
        app,
        compiled_target,
        environment,
        posture,
        ConfirmedReads::none(),
    )
    .await
}

/// As [`list`], with what the kernel says the person confirmed.
async fn list_confirmed(
    app: &TestApp,
    compiled_target: CompiledTarget,
    environment: &str,
    posture: Posture,
    confirmed: ConfirmedReads,
) -> Result<common::Ran, StepFailure> {
    let stand = Stand::new("asserted", Arc::new(NoSecrets));
    let mut scene = Scene::over(stand, compiled_target, &app.url(""));
    scene.surface.environment = Environment::new(environment).unwrap();
    scene.confirmed = confirmed;
    let (handle, mut prepared) = scene
        .try_prepare(None, posture)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    let ran = scene
        .read(&handle, &mut prepared, LIST, &Values::new())
        .await;
    assert!(handle.release(prepared, ran.is_err()).await.complete);
    scene.stand.end().await;
    ran
}

#[tokio::test(flavor = "multi_thread")]
async fn a_published_read_lets_the_pages_asserted_search_through_and_nothing_else() {
    let app = TestApp::start().await.unwrap();
    let seeded = app.case_rows().await.unwrap();
    let stand = Stand::new("asserted-targets", Arc::new(NoSecrets));
    let draft = compile(&stand.registry, "rows").compiled_target;
    let page_write_refused = (FailureClass::Policy, "page_write_refused");

    // Under the drafts nothing is let through, in any posture, and the stop
    // names the page request and says to publish it.
    for posture in [
        Posture::Live,
        Posture::Sandbox {
            test_records: false,
        },
    ] {
        let stopped = list(&app, draft.clone(), "test", posture)
            .await
            .unwrap_err();
        assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
        let next = stopped.failure.next_step();
        assert!(
            next.contains("cases_rows_search") && next.contains("Publish"),
            "{next}"
        );
    }
    assert_eq!(app.case_rows().await.unwrap(), seeded);

    // From a published version, live, the rows arrive as the control
    // interface holds them; and in a try in an environment of kind `test`.
    let open: Vec<Value> = seeded
        .rows
        .iter()
        .filter(|row| row.status == "open")
        .map(|row| Value::from(row.case_id.as_str()))
        .collect();
    assert_eq!(open.len(), 2);
    for (environment, posture) in [
        ("production", Posture::Live),
        (
            "test",
            Posture::Sandbox {
                test_records: false,
            },
        ),
    ] {
        let read = list(&app, published(draft.clone()), environment, posture)
            .await
            .unwrap_or_else(|failure| panic!("{environment} {posture:?}: {failure}"));
        assert_eq!(
            read.values.get("case_ids"),
            Some(&Value::List(open.clone()))
        );
    }

    // A published version tried in an environment that is not of kind
    // `test`, and one in discovery, let nothing through.
    for (environment, posture) in [
        (
            "production",
            Posture::Sandbox {
                test_records: false,
            },
        ),
        ("test", Posture::Discovery),
    ] {
        let stopped = list(&app, published(draft.clone()), environment, posture)
            .await
            .unwrap_err();
        assert_eq!(
            class_and_code(&stopped),
            page_write_refused,
            "{environment} {posture:?}: {stopped}"
        );
    }

    // The same page with no assertion stops as before, and its next step
    // names no page request: the entry is a read by `GET` of another path.
    let unasserted = compile_changed(&stand.registry, "rows", ENTRY, |_| {
        "endpoint: cases_rows_get\nsurface: app\nmethod: GET\npath: \"/api/cases/rows\"\n\
         page: \"/cases/rows-by-post\"\neffect: read\n\
         response_schema:\n  type: object\n  properties:\n    rows: {type: array}\n"
            .to_owned()
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
    .compiled_target;
    let stopped = list(&app, published(unasserted), "test", Posture::Live)
        .await
        .unwrap_err();
    assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
    assert!(!stopped.failure.next_step().contains("cases_rows_search"));

    // An assertion the page's body does not fit lets nothing through, and
    // says the request did not fit the entry.
    let narrower = compile_changed(&stand.registry, "rows", ENTRY, |text| {
        text.replace("enum: [open]", "enum: [closed]")
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
    .compiled_target;
    let stopped = list(&app, published(narrower), "test", Posture::Live)
        .await
        .unwrap_err();
    assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
    assert!(
        stopped.failure.next_step().contains("did not fit"),
        "{}",
        stopped.failure.next_step()
    );

    // A request of another page, which no entry asserts, still stops the
    // read, and reaches nothing.
    let stopped = {
        let stand = Stand::new("asserted-trap", Arc::new(NoSecrets));
        let scene = Scene::over(stand, published(draft.clone()), &app.url(""));
        let (handle, mut prepared) = scene.try_prepare(None, Posture::Live).await.unwrap();
        let ran = scene
            .read(
                &handle,
                &mut prepared,
                "example_activity_get",
                &Values::new(),
            )
            .await;
        assert!(handle.release(prepared, true).await.complete);
        scene.stand.end().await;
        ran.unwrap_err()
    };
    assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
    assert!(app.trap_writes().await.unwrap().is_empty());

    // The route heard a search for each read that was let through, and
    // never an archive; no row changed.
    let after = app.case_rows().await.unwrap();
    assert_eq!(after.rows, seeded.rows);
    assert!(
        !after.received.iter().any(|op| op == "archive"),
        "{after:?}"
    );
    assert_eq!(after.received, vec!["search".to_owned(); 2], "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());

    stand.end().await;
}

/// The `page_http` driver opens its observing tab under the same rule: a read
/// from inside the cases page, whose script searches as it loads, stops under
/// the drafts naming the page request, and reads from a published version.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_from_inside_a_page_that_searches_by_post_runs_only_from_a_published_version() {
    let app = TestApp::start().await.unwrap();
    let seeded = app.case_rows().await.unwrap();
    let stored = app.stored_order("A-100").await.unwrap().unwrap();
    let mode = common::page_http_mode();
    let inputs = Values::new().with("order_id", "A-100");
    let read = |compiled_target: CompiledTarget| {
        let app = &app;
        let mode = &mode;
        let inputs = &inputs;
        async move {
            let stand = Stand::new("asserted-page-http", Arc::new(NoSecrets));
            let scene = Scene::over(stand, compiled_target, &app.url(""));
            let (handle, mut prepared) = scene
                .try_prepare_in(mode, None, Posture::Live)
                .await
                .unwrap_or_else(|failure| panic!("{failure}"));
            let ran = scene
                .read_in(
                    mode,
                    &handle,
                    &mut prepared,
                    "example_order_length_get",
                    inputs,
                )
                .await;
            assert!(handle.release(prepared, ran.is_err()).await.complete);
            scene.stand.end().await;
            ran
        }
    };
    let stand = Stand::new("asserted-page-http-targets", Arc::new(NoSecrets));
    let draft = compile(&stand.registry, "rows").compiled_target;

    let stopped = read(draft.clone()).await.unwrap_err();
    assert_eq!(
        class_and_code(&stopped),
        (FailureClass::Policy, "page_write_refused"),
        "{stopped}"
    );
    assert!(
        stopped.failure.next_step().contains("cases_rows_search"),
        "{}",
        stopped.failure.next_step()
    );
    assert_eq!(app.case_rows().await.unwrap(), seeded);

    let ran = read(published(draft)).await.unwrap();
    // The order's total length, in inches, as the store holds its lines.
    let sixteenths: u64 = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity) * u64::from(item.length_sixteenths))
        .sum();
    assert_eq!(
        ran.values.get("total_length"),
        Some(&Value::from(
            example_app::store::format_length_in(u32::try_from(sixteenths).unwrap()).as_str()
        ))
    );
    let after = app.case_rows().await.unwrap();
    assert_eq!(after.rows, seeded.rows);
    assert_eq!(after.received, vec!["search".to_owned()], "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());
    stand.end().await;
}

/// The search's entry, as the kernel names a confirmed one.
fn search_confirmed() -> ConfirmedReads {
    ConfirmedReads::of([(
        "page_request".to_owned(),
        "app".to_owned(),
        "cases_rows_search".to_owned(),
    )])
}

/// Under the drafts, the entry its person confirmed is let through in a try
/// in an environment of kind `test`, and in no other place; a request that
/// does not fit it, and a request of another page that writes, still stop
/// the read and reach nothing (decision 0475).
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_draft_lets_its_search_through_in_a_try_in_test_and_nowhere_else() {
    let app = TestApp::start().await.unwrap();
    let seeded = app.case_rows().await.unwrap();
    let stand = Stand::new("confirmed-targets", Arc::new(NoSecrets));
    let draft = compile(&stand.registry, "rows").compiled_target;
    let page_write_refused = (FailureClass::Policy, "page_write_refused");
    let sandbox = Posture::Sandbox {
        test_records: false,
    };

    // A try of the drafts in an environment of kind `test`, the search
    // confirmed: the rows arrive as the control interface holds them.
    let open: Vec<Value> = seeded
        .rows
        .iter()
        .filter(|row| row.status == "open")
        .map(|row| Value::from(row.case_id.as_str()))
        .collect();
    let read = list_confirmed(&app, draft.clone(), "test", sandbox, search_confirmed())
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(read.values.get("case_ids"), Some(&Value::List(open)));

    // Confirmed, and anywhere else: live under the drafts, a try in an
    // environment of another kind, discovery. Each stops.
    for (environment, posture) in [
        ("test", Posture::Live),
        ("production", sandbox),
        ("test", Posture::Discovery),
    ] {
        let stopped = list_confirmed(
            &app,
            draft.clone(),
            environment,
            posture,
            search_confirmed(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            class_and_code(&stopped),
            page_write_refused,
            "{environment} {posture:?}: {stopped}"
        );
    }

    // A confirmation of another entry covers nothing, and the stop says
    // to ask the person.
    let other = ConfirmedReads::of([(
        "page_request".to_owned(),
        "app".to_owned(),
        "cases_rows_other".to_owned(),
    )]);
    let stopped = list_confirmed(&app, draft.clone(), "test", sandbox, other)
        .await
        .unwrap_err();
    assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
    let next = stopped.failure.next_step();
    assert!(
        next.contains("cases_rows_search") && next.contains("assertion.confirm"),
        "{next}"
    );

    // A confirmed entry the page's body does not fit lets nothing through.
    let narrower = compile_changed(&stand.registry, "rows", ENTRY, |text| {
        text.replace("enum: [open]", "enum: [closed]")
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
    .compiled_target;
    let stopped = list_confirmed(&app, narrower, "test", sandbox, search_confirmed())
        .await
        .unwrap_err();
    assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
    assert!(
        stopped.failure.next_step().contains("did not fit"),
        "{}",
        stopped.failure.next_step()
    );

    // A try that meets a page that writes, with the search confirmed,
    // stops, and the write reaches nothing.
    let stopped = {
        let stand = Stand::new("confirmed-trap", Arc::new(NoSecrets));
        let mut scene = Scene::over(stand, draft.clone(), &app.url(""));
        scene.confirmed = search_confirmed();
        let (handle, mut prepared) = scene.try_prepare(None, sandbox).await.unwrap();
        let ran = scene
            .read(
                &handle,
                &mut prepared,
                "example_activity_get",
                &Values::new(),
            )
            .await;
        assert!(handle.release(prepared, true).await.complete);
        scene.stand.end().await;
        ran.unwrap_err()
    };
    assert_eq!(class_and_code(&stopped), page_write_refused, "{stopped}");
    assert!(app.trap_writes().await.unwrap().is_empty());

    // The route heard the one search that was let through, and never an
    // archive; no row changed.
    let after = app.case_rows().await.unwrap();
    assert_eq!(after.rows, seeded.rows);
    assert_eq!(after.received, vec!["search".to_owned()], "{after:?}");
    assert!(app.writes().await.unwrap().is_empty());
    stand.end().await;
}
