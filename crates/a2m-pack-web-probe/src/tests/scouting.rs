//! Discovery against the test application, through a real browser.

use a2m_core::{Coverage, EvidenceLevel, FailureClass, LimitationCode};
use a2m_provider::DescribeForm;
use serde_json::json;

use super::*;
use crate::codes;

/// Every finding says how much was looked at and why it is weaker than it
/// looks, none is above `discovered`, and none holds the planted secret.
#[track_caller]
fn check(seen: &Seen) {
    for finding in &seen.findings {
        assert!(finding.level() <= EvidenceLevel::Discovered);
        assert_eq!(finding.coverage(), Coverage::Partial);
        assert!(!finding.limitations().is_empty(), "{finding:?}");
        let whole = serde_json::to_string(finding).unwrap();
        assert!(!whole.contains(SECRET), "{whole}");
        assert!(!format!("{finding:?}").contains(SECRET));
    }
}

fn names_of(found: &Json, role: &str) -> Vec<String> {
    found["elements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|element| element["role"] == role)
        .filter_map(|element| element["name"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn scouting_the_test_application_gives_its_screens_and_endpoints_the_same_twice() {
    let site = Site::start().await;
    let bench = Bench::new("scouting", Arc::new(Knows(SECRET)));
    let start = site.url("/");

    // The secret is planted in a record, as a value a page shows in a
    // control. This is the test's own write, and the only one.
    let planted = serde_json::json!({
        "description": SECRET, "quantity": 2, "length_in": "3", "version": 1,
    });
    let answer = site
        .own
        .http
        .put(site.url("/api/orders/A-100/items/L-102"))
        .json(&planted)
        .send()
        .await
        .unwrap();
    assert!(answer.status().is_success());
    let stored = site.stored_order("A-100").await;
    assert!(stored.to_string().contains(SECRET));

    let snapshots = Look {
        paths: vec!["/orders/A-100/items/L-102", "/probe/secret"],
        depth: Some(0),
        ..Look::default()
    };
    let captures = Look {
        paths: vec![
            "/api/orders",
            "/api/orders/A-100",
            "/api/orders/A-101",
            "/api/orders/A-100/items/L-102",
            "/probe/secret.json?token=secret-for-tests-7f3a9c",
        ],
        ..Look::default()
    };
    // A page that tries a write when it loads, read once it shows that
    // what it tried has settled.
    let posting = Look {
        paths: vec!["/probe/posts"],
        ready_text: Some("settled"),
        ..Look::default()
    };

    // The probe: the start page redirects to the list of orders, which
    // stands open; no profile knows the platform, and that is said.
    let probe = must(bench.run("web_probe", &start, &Look::default()).await);
    check(&probe);
    assert_eq!(probe.findings.len(), 1);
    let (surface, found) = probe.one("surface", "/");
    assert_eq!(surface.level(), EvidenceLevel::Discovered);
    assert_eq!(
        found,
        json!({
            "format": "1", "answer": "open", "block": null, "status": 200, "redirected": true,
            "interface": "record_list", "platform": null, "platform_state": "unknown_platform",
            "tier": "unsupported",
        })
    );

    // Snapshots: the list, the form of one line item, and a page that
    // shows the secret in a heading, a link, a control, and a cell.
    let screens = must(bench.run("web_snapshot", &start, &snapshots).await);
    check(&screens);
    assert_eq!(screens.findings.len(), 3, "{:#?}", screens.paths());
    let (list, found) = screens.one("screen", "/orders");
    assert_eq!(list.level(), EvidenceLevel::Discovered);
    assert!(
        list.limitations()
            .iter()
            .any(|one| one.code == LimitationCode::SingleVariant)
    );
    assert_eq!(
        names_of(&found, "columnheader"),
        ["Order", "Customer", "Status", "Lines"]
    );
    assert_eq!(names_of(&found, "heading"), ["Orders"]);
    // What fills the list is data: no customer and no order number.
    let stored_list = site.stored_order("A-101").await;
    let customer = stored_list["customer"].as_str().unwrap();
    assert!(!found.to_string().contains(customer), "{found}");
    assert!(!found.to_string().contains("A-101"));

    let (_, found) = screens.one("screen", "/orders/A-100/items/L-102");
    assert_eq!(
        names_of(&found, "textbox"),
        ["Item code", "Description", "Quantity", "Length"]
    );
    assert_eq!(names_of(&found, "button"), ["Save"]);
    let field = |name: &str| {
        found["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|element| element["role"] == "textbox" && element["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(field("Quantity")["value_type"], "integer");
    assert_eq!(field("Description")["value_type"], "text");
    assert_eq!(field("Quantity")["states"], json!(["required"]));

    let (_, found) = screens.one("screen", "/probe/secret");
    assert_eq!(names_of(&found, "textbox"), ["Key"]);
    assert!(found.to_string().contains("[redacted]"), "{found}");

    // Capture: the document of each page, with the shape of a JSON answer.
    let capture = must(bench.run("web_capture", &start, &captures).await);
    check(&capture);
    let (orders, found) = capture.one("endpoint", "/api/orders");
    assert_eq!(orders.level(), EvidenceLevel::Discovered);
    assert_eq!(found["method"], "GET");
    assert_eq!(found["path"], "/api/orders");
    assert_eq!(found["status"], 200);
    assert_eq!(found["answered"], true);
    assert_eq!(found["response"]["type"], "list");
    assert_eq!(
        found["response"]["items"]["fields"]["order_id"],
        json!({"type": "text"})
    );
    assert_eq!(orders.derived_from().len(), 1);
    assert_eq!(
        orders.derived_from()[0],
        capture.one("exchange", "/api/orders").0.id()
    );

    let (_, found) = capture.one("endpoint", "/api/orders/A-100/items/L-102");
    let fields = found["response"]["fields"].as_object().unwrap();
    assert_eq!(fields["description"], json!({"type": "text"}));
    assert_eq!(fields["quantity"], json!({"type": "integer"}));

    // A name that is itself the secret is redacted; the query keeps its
    // names and the types of its values.
    let (_, found) = capture.one("endpoint", "/probe/secret.json");
    assert_eq!(found["query"], json!({"token": "text"}));
    assert_eq!(
        found["response"]["fields"]["[redacted]"],
        json!({"type": "integer"})
    );
    assert_eq!(
        found["response"]["fields"]["items"]["items"]["fields"]["note"],
        json!({"type": "text", "optional": true})
    );

    // The write a page tried was refused, so the endpoint is seen and
    // never answered: inferred, and it says so.
    let posted = must(bench.run("web_capture", &start, &posting).await);
    check(&posted);
    let tried: Vec<_> = posted
        .findings
        .iter()
        .filter(|finding| {
            finding.kind().as_str() == "endpoint" && finding.found().contains("\"PUT\"")
        })
        .collect();
    assert_eq!(tried.len(), 1, "{:#?}", posted.paths());
    assert_eq!(tried[0].level(), EvidenceLevel::Inferred);
    assert!(
        tried[0]
            .limitations()
            .iter()
            .any(|one| one.code == LimitationCode::NotExercised)
    );
    // Without tier two, the two orders are two endpoints.
    assert_eq!(capture.at("endpoint", "/api/orders/A-100").len(), 1);
    assert_eq!(capture.at("endpoint", "/api/orders/A-101").len(), 1);

    // With tier two, they are one, and it is inferred although it
    // answered.
    let varied = must(
        bench
            .run(
                "web_capture",
                &start,
                &Look {
                    paths: vec!["/api/orders/A-100", "/api/orders/A-101"],
                    tier_two: true,
                    ..Look::default()
                },
            )
            .await,
    );
    check(&varied);
    let (joined, found) = varied.one("endpoint", "/api/orders/{p1}");
    assert_eq!(joined.level(), EvidenceLevel::Inferred);
    assert_eq!(found["template"]["parameters"][0]["tier"], 2);
    assert_eq!(found["template"]["parameters"][0]["values_seen"], 2);
    assert_eq!(joined.derived_from().len(), 2);

    // A second run gives the same findings, by identifier.
    let again = must(bench.run("web_probe", &start, &Look::default()).await);
    assert_eq!(again.ids(), probe.ids());
    let again = must(bench.run("web_snapshot", &start, &snapshots).await);
    assert_eq!(again.ids(), screens.ids());
    let again = must(bench.run("web_capture", &start, &captures).await);
    assert_eq!(again.ids(), capture.ids());
    assert_eq!(again.report, capture.report);
    let again = must(bench.run("web_capture", &start, &posting).await);
    assert_eq!(again.ids(), posted.ids());

    // The describer counts what was found and keeps the system's own text
    // apart.
    let handle = bench.discoverer("web_capture", &start);
    let summary = handle.describe(&capture.findings, DescribeForm::Summary);
    assert_eq!(
        summary.data.get("endpoint"),
        Some(&Value::from(
            i64::try_from(
                capture
                    .findings
                    .iter()
                    .filter(|finding| finding.kind().as_str() == "endpoint")
                    .count()
            )
            .unwrap()
        ))
    );
    assert!(!summary.text.contains("/api/orders"));
    assert!(
        summary
            .untrusted
            .iter()
            .any(|line| line == "endpoint GET /api/orders")
    );
    assert!(!format!("{summary:?}").contains(SECRET));

    // The application holds what it held: the test's own write and no
    // other.
    assert_eq!(site.writes().await.len(), 1);
    assert_eq!(site.stored_order("A-100").await, stored);
    bench.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_a_page_or_a_link_does_during_discovery_reaches_the_application() {
    let site = Site::start().await;
    let bench = Bench::new("cannot-write", no_secrets());

    // First, that the test means something: the route a socket's handshake
    // reaches, and the link that acts, each write when they are reached,
    // and the write the pages try is one the application takes.
    assert_eq!(
        site.own
            .http
            .get(site.url("/probe/ws"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(site.writes().await.len(), 1);
    let followed = site
        .own
        .http
        .get(site.url("/probe/items/L-101/remove"))
        .send()
        .await
        .unwrap();
    assert!(followed.status().is_success());
    assert_eq!(site.writes().await.len(), 2);
    site.reset().await;
    assert_eq!(site.own.write("L-102").await, 200);
    assert_eq!(site.writes().await.len(), 1);
    site.reset().await;
    assert!(site.writes().await.is_empty());
    let before = site.stored_order("A-100").await;

    // A page that posts when it loads, a page whose worker posts, and a
    // page that opens a socket. Each is read once what it tried has
    // settled.
    let capture = must(
        bench
            .run(
                "web_capture",
                &site.url("/orders"),
                &Look {
                    paths: vec!["/probe/posts", "/probe/worker", "/probe/socket"],
                    ready_text: Some("settled"),
                    ..Look::default()
                },
            )
            .await,
    );
    // The page's own write and the worker's were each tried, refused, and
    // reported; the system never answered them.
    for item in ["L-101", "L-102"] {
        let (tried, found) = capture.one("exchange", &format!("/api/orders/A-100/items/{item}"));
        assert_eq!(found["method"], "PUT");
        assert_eq!(found["answered"], false);
        assert_eq!(tried.level(), EvidenceLevel::Inferred);
    }
    // The socket was refused when the page made it, and the page's
    // exchange says so.
    let (socket, _) = capture.one("exchange", "/probe/socket");
    assert!(
        socket
            .limitations()
            .iter()
            .any(|one| one.code == LimitationCode::NotExercised
                && one.sentence.contains("WEBSOCKET")),
        "{:?}",
        socket.limitations()
    );
    // No page was late: each showed that what it tried had settled.
    assert!(capture.findings.iter().all(|finding| {
        finding
            .limitations()
            .iter()
            .all(|one| one.code != LimitationCode::TimeLimit)
    }));

    // Links: the plain one is followed; the one whose address looks like
    // an action and the one whose name does are not, and are counted; the
    // one that leads elsewhere is not followed either.
    let screens = must(
        bench
            .run(
                "web_snapshot",
                &site.url("/probe/links"),
                &Look {
                    depth: Some(1),
                    ..Look::default()
                },
            )
            .await,
    );
    let mut looked: Vec<String> = screens.paths();
    looked.sort();
    assert_eq!(looked, ["screen /orders/A-102", "screen /probe/links"]);
    let (links, _) = screens.one("screen", "/probe/links");
    assert!(
        links
            .limitations()
            .iter()
            .any(|one| one.sentence.starts_with("2 link(s) look like an action")),
        "{:?}",
        links.limitations()
    );
    // Two refused, one elsewhere, and the link back to the list of orders
    // on the page that was followed is beyond the depth and not read.
    assert_eq!(screens.report.dropped, 3);

    // The application says nothing was written, and holds what it held.
    assert!(site.writes().await.is_empty());
    assert_eq!(site.stored_order("A-100").await, before);
    bench.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wall_a_block_and_a_bound_are_named_and_never_passed() {
    let site = Site::start().await;
    let bench = Bench::new("walls", no_secrets());

    // A page that sends the visitor to a sign-in form.
    let private = site.url("/probe/private");
    let probe = must(bench.run("web_probe", &private, &Look::default()).await);
    let (_, found) = probe.one("surface", "/probe/private");
    assert_eq!(found["answer"], "sign_in");
    assert_eq!(found["redirected"], true);
    assert_eq!(found["interface"], Json::Null);
    let (method, found) = probe.one("sign_in_method", "/probe/private");
    assert_eq!(method.level(), EvidenceLevel::Inferred);
    assert_eq!(found["method"], "form");
    assert_eq!(method.derived_from().len(), 1);
    // The other discoverers stop there: the wall is not a screen.
    for name in ["web_snapshot", "web_capture"] {
        let failure = bench
            .run(name, &private, &Look::default())
            .await
            .err()
            .unwrap();
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::SessionExpired, &codes::NEEDS_LOGIN)
        );
    }

    // A surface that answers 403 is blocked: the probe names the block,
    // and the others stop with it.
    site.fault(&json!({"kind": "status", "status": 403, "path_prefix": "/orders"}))
        .await;
    let orders = site.url("/orders");
    let probe = must(bench.run("web_probe", &orders, &Look::default()).await);
    let (_, found) = probe.one("surface", "/orders");
    assert_eq!(found["answer"], "blocked");
    assert_eq!(found["block"], "forbidden_by_edge");
    assert_eq!(probe.findings.len(), 1);
    let failure = bench
        .run("web_capture", &orders, &Look::default())
        .await
        .err()
        .unwrap();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::EdgeBlocked, &codes::FORBIDDEN_BY_EDGE)
    );
    site.reset().await;

    // A focus can choose a path of the surface and nothing else.
    for outside in [
        "//elsewhere.example.com/x",
        "https://elsewhere.example.com/",
        "orders",
    ] {
        let failure = bench
            .run(
                "web_snapshot",
                &orders,
                &Look {
                    paths: vec![outside],
                    ..Look::default()
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Input, &codes::FOCUS_INVALID),
            "{outside}"
        );
    }

    // A discovery whose time is up says which bound it hit and how many
    // pages it left unread.
    let late = must(
        bench
            .run(
                "web_capture",
                &orders,
                &Look {
                    paths: vec!["/api/orders", "/api/orders/A-100"],
                    within: Duration::ZERO,
                    ..Look::default()
                },
            )
            .await,
    );
    assert!(late.findings.is_empty());
    assert_eq!(late.report.bounds_hit, ["seconds"]);
    assert_eq!(late.report.dropped, 3);

    assert!(site.writes().await.is_empty());
    bench.end().await;
}
