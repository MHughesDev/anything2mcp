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

    // A name that is itself the secret is a value: the names of its object
    // are withheld whole, a dictionary's (decision 0195); the query keeps
    // its names and the types of its values.
    let (finding, found) = capture.one("endpoint", "/probe/secret.json");
    assert_eq!(found["query"], json!({"token": "text"}));
    assert_eq!(
        found["response"]["fields"]["by_key"],
        json!({"type": "object", "names_withheld": true})
    );
    assert!(
        finding
            .limitations()
            .iter()
            .any(|one| one.sentence.contains("dictionary")),
        "{:?}",
        finding.limitations()
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

#[tokio::test(flavor = "multi_thread")]
async fn an_address_that_looks_like_an_action_is_never_visited_whatever_names_it() {
    let site = Site::start().await;
    let bench = Bench::new("action-paths", no_secrets());
    let orders = site.url("/orders");
    let refused = |failure: Failure| {
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Policy, &codes::ADDRESS_LOOKS_LIKE_AN_ACTION),
            "{failure}"
        );
    };
    // A focus that names one, by its path or by its query, for both of
    // the discoverers that take a focus.
    for path in ["/probe/items/L-101/remove", "/probe/plain?then=delete"] {
        for name in ["web_snapshot", "web_capture"] {
            let look = Look {
                paths: vec![path],
                ..Look::default()
            };
            refused(bench.run(name, &orders, &look).await.err().unwrap());
        }
    }
    // A surface whose own address is one.
    let acting = site.url("/probe/items/L-101/remove");
    refused(
        bench
            .run("web_probe", &acting, &Look::default())
            .await
            .err()
            .unwrap(),
    );
    // The route writes when it is visited, and it never was.
    assert!(site.writes().await.is_empty());

    // A start page that sends its visitor to another origin: the browser
    // ends the move there, and the probe sees a sign-in by redirect.
    let elsewhere = site.url("/edge/elsewhere");
    let probe = must(bench.run("web_probe", &elsewhere, &Look::default()).await);
    let (_, found) = probe.one("surface", "/edge/elsewhere");
    assert_eq!(found["answer"], "sign_in");
    assert_eq!(found["redirected"], true);
    let (_, found) = probe.one("sign_in_method", "/edge/elsewhere");
    assert_eq!(found["method"], "redirect");
    bench.end().await;
}

/// Where the screen finding of the page that shows one order is kept as
/// data, for the web pack's drafting rules to be proven against: the
/// two crates of the pack share findings as data and never name each
/// other. This test holds that the file is what discovery gives.
fn shared_order_screen() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("a2m-pack-web-ui")
        .join("tests")
        .join("fixtures")
        .join("findings")
        .join("order_screen.json")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_that_shows_an_order_gives_the_screen_the_web_pack_drafts_from_the_same_twice() {
    let site = Site::start().await;
    let bench = Bench::new("order-screen", no_secrets());
    let start = site.url("/");
    let look = Look {
        paths: vec!["/orders/A-100"],
        depth: Some(0),
        ..Look::default()
    };
    let first = must(bench.run("web_snapshot", &start, &look).await);
    check(&first);
    let again = must(bench.run("web_snapshot", &start, &look).await);
    assert_eq!(again.ids(), first.ids());
    let (finding, found) = first.one("screen", "/orders/A-100");
    assert_eq!(finding.level(), EvidenceLevel::Discovered);
    assert_eq!(names_of(&found, "table"), ["Line items"]);
    assert_eq!(
        names_of(&found, "columnheader"),
        ["Item code", "Description", "Quantity", "Length (in)"]
    );
    let text = std::fs::read_to_string(shared_order_screen()).unwrap_or_default();
    let shared: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    let wanted = json!({"kind": "screen", "path": "/orders/A-100", "found": finding.found()});
    assert_eq!(
        shared,
        wanted,
        "write this into {}:\n{}",
        shared_order_screen().display(),
        serde_json::to_string_pretty(&wanted).unwrap()
    );
    assert!(site.writes().await.is_empty());
    bench.end().await;
}

/// Where the screen finding of the pages that show one order each, matched
/// into one screen, is kept as data for the web pack's drafting rules.
fn shared_orders_screen() -> std::path::PathBuf {
    shared_order_screen().with_file_name("orders_screen.json")
}

#[tokio::test(flavor = "multi_thread")]
async fn two_pages_of_one_screen_give_one_screen_with_a_placeholder_the_same_twice() {
    let site = Site::start().await;
    let bench = Bench::new("orders-screen", no_secrets());
    let start = site.url("/");
    let look = Look {
        paths: vec!["/orders/A-100", "/orders/A-101"],
        depth: Some(0),
        ..Look::default()
    };
    let first = must(bench.run("web_snapshot", &start, &look).await);
    check(&first);
    let again = must(bench.run("web_snapshot", &start, &look).await);
    assert_eq!(again.ids(), first.ids());
    // The list of orders is a screen of its own; the two pages of an
    // order are one, and neither is given apart.
    assert_eq!(
        first.paths(),
        ["screen /orders", "screen /orders/{order_id}"],
        "{:#?}",
        first.paths()
    );
    let (finding, found) = first.one("screen", "/orders/{order_id}");
    assert_eq!(found["pages"], json!(["/orders/A-100", "/orders/A-101"]));
    assert!(
        finding
            .limitations()
            .iter()
            .any(|one| one.code == LimitationCode::Sampled)
    );
    // The labels of the values are kept, and the values are not.
    assert_eq!(names_of(&found, "term"), ["Customer", "Status"]);
    assert_eq!(names_of(&found, "definition"), ["Customer", "Status"]);
    for order in ["A-100", "A-101"] {
        let stored = site.stored_order(order).await;
        let customer = stored["customer"].as_str().unwrap();
        assert!(!found.to_string().contains(customer), "{found}");
    }
    assert!(!found.to_string().contains("Draft"), "{found}");
    let text = std::fs::read_to_string(shared_orders_screen()).unwrap_or_default();
    let shared: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    let wanted = json!({"kind": "screen", "path": "/orders/{order_id}", "found": finding.found()});
    assert_eq!(
        shared,
        wanted,
        "write this into {}:\n{}",
        shared_orders_screen().display(),
        serde_json::to_string_pretty(&wanted).unwrap()
    );

    // One page of an order alone is no pattern: it is drafted as it is.
    let alone = Look {
        paths: vec!["/orders/A-100"],
        depth: Some(0),
        ..Look::default()
    };
    let one = must(bench.run("web_snapshot", &start, &alone).await);
    assert_eq!(one.paths(), ["screen /orders", "screen /orders/A-100"]);
    assert!(site.writes().await.is_empty());
    bench.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_the_page_makes_by_itself_is_an_exchange_of_names_and_kinds_only() {
    let site = Site::start().await;
    let bench = Bench::new("page-requests", no_secrets());
    let start = site.url("/");
    let look = Look {
        paths: vec!["/orders/A-100/totals"],
        depth: Some(0),
        ready_text: Some("Loaded."),
        ..Look::default()
    };
    let seen = must(bench.run("web_capture", &start, &look).await);
    check(&seen);
    let (finding, found) = seen.one("exchange", "/api/orders/A-100/totals");
    assert_eq!(finding.level(), EvidenceLevel::Discovered);
    assert_eq!(
        found,
        json!({
            "format": "1", "method": "GET", "path": "/api/orders/A-100/totals",
            "query": {"lang": "unknown", "view": "unknown"}, "answered": true, "status": 200,
            "content": "json",
            "response": {"type": "object", "fields": {
                "lines": {"type": "integer"}, "order_id": {"type": "text"},
                "total_length_in": {"type": "text"}, "total_quantity": {"type": "integer"},
            }},
            "made_by": "/orders/A-100/totals",
            "credential_headers": ["cookie", "x-csrf-token"],
            "set_headers": ["accept", "x-csrf-token"],
            "query_left_out": false,
        })
    );
    // No value of the query, a header, the cookie, or the answer.
    let whole = serde_json::to_string(&seen.findings).unwrap();
    for value in [
        "summary",
        "example-token-for-tests",
        "secret-for-tests-cookie",
    ] {
        assert!(!whole.contains(value), "{whole}");
    }
    // The endpoint it groups into, as for any other exchange.
    let (_, endpoint) = seen.one("endpoint", "/api/orders/A-100/totals");
    assert_eq!(endpoint["method"], "GET");
    assert!(site.writes().await.is_empty());
    bench.end().await;
}

/// The `exchange` finding of the page that asks for an order's totals by
/// itself, with a query and headers its script set, matched over two
/// orders, as the web UI pack's drafting test reads it.
fn shared_totals_exchange() -> std::path::PathBuf {
    shared_order_screen().with_file_name("order_totals_exchange.json")
}

/// The same, of the page that asks for them with nothing a read from
/// inside the page would not send.
fn shared_summary_exchange() -> std::path::PathBuf {
    shared_order_screen().with_file_name("order_summary_exchange.json")
}

/// Discovers two pages of one screen whose own request is one read by
/// key, and holds that it is given once, with the placeholder, and that the
/// finding is the one kept at `file`.
async fn matched_requests_of(name: &str, page: &str, request: &str, file: std::path::PathBuf) {
    let site = Site::start().await;
    let bench = Bench::new(name, no_secrets());
    let start = site.url("/");
    let pages: Vec<&'static str> = ["A-100", "A-101"]
        .iter()
        .map(|order| &*Box::leak(page.replace("{order_id}", order).into_boxed_str()))
        .collect();
    let look = Look {
        paths: pages,
        depth: Some(0),
        ready_text: Some("Loaded."),
        ..Look::default()
    };
    let first = must(bench.run("web_capture", &start, &look).await);
    check(&first);
    let second = must(bench.run("web_capture", &start, &look).await);
    let ids = |seen: &Seen| seen.findings.iter().map(|one| one.id()).collect::<Vec<_>>();
    assert_eq!(ids(&first), ids(&second));
    // One request by key; neither order's own is given apart.
    for order in ["A-100", "A-101"] {
        assert!(
            first
                .at("exchange", &request.replace("{order_id}", order))
                .is_empty()
        );
    }
    let (finding, found) = first.one("exchange", request);
    assert_eq!(found["made_by"], page);
    assert_eq!(
        found["paths"],
        serde_json::json!([
            request.replace("{order_id}", "A-100"),
            request.replace("{order_id}", "A-101")
        ])
    );
    assert!(
        finding
            .limitations()
            .iter()
            .any(|one| one.code == LimitationCode::Sampled)
    );
    // No value of the query, a header, the cookie, or the answer.
    let whole = serde_json::to_string(&first.findings).unwrap();
    for value in [
        "view=",
        "example-token-for-tests",
        "secret-for-tests-cookie",
        "Example Customer",
    ] {
        assert!(!whole.contains(value), "{whole}");
    }
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let shared: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    let wanted = serde_json::json!({
        "kind": "exchange",
        "path": request,
        "found": finding.found(),
    });
    assert_eq!(
        shared,
        wanted,
        "write this into {}:\n{}",
        file.display(),
        serde_json::to_string_pretty(&wanted).unwrap()
    );
    assert!(site.writes().await.is_empty());
    bench.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_pages_that_ask_for_one_read_by_key_give_one_exchange_with_a_placeholder() {
    matched_requests_of(
        "totals-matched",
        "/orders/{order_id}/totals",
        "/api/orders/{order_id}/totals",
        shared_totals_exchange(),
    )
    .await;
    matched_requests_of(
        "summary-matched",
        "/orders/{order_id}/summary",
        "/api/orders/{order_id}/totals",
        shared_summary_exchange(),
    )
    .await;
}

/// Where the screen finding of a page whose address holds a token is kept
/// as data, for the web UI pack's drafting test.
fn shared_invitation_screen() -> std::path::PathBuf {
    shared_order_screen().with_file_name("invitation_screen.json")
}

/// A long random text, made up, as a page's address and its script carry
/// a token.
const TOKEN: &str = "q7Zx9kP2mW4vB8nR3tY6hJ1cF5";

#[tokio::test(flavor = "multi_thread")]
async fn a_token_in_a_pages_address_or_a_bare_entry_of_its_query_reaches_no_finding() {
    let site = Site::start().await;
    let bench = Bench::new("token-in-address", no_secrets());
    let start = site.url("/");
    let page: &'static str = Box::leak(format!("/invitations/{TOKEN}").into_boxed_str());
    let look = Look {
        paths: vec![page],
        depth: Some(0),
        ready_text: Some("Loaded."),
        ..Look::default()
    };
    let mut all = Vec::new();
    for discoverer in ["web_probe", "web_snapshot", "web_capture"] {
        let seen = must(bench.run(discoverer, &start, &look).await);
        check(&seen);
        all.extend(seen.findings);
    }
    let whole = serde_json::to_string(&all).unwrap();
    assert!(!whole.contains(TOKEN), "{whole}");
    let paths: Vec<String> = all
        .iter()
        .filter_map(|finding| match finding.subject() {
            a2m_core::Subject::Address(address) => Some(address.path().to_owned()),
            _ => None,
        })
        .collect();
    assert!(
        paths.contains(&"/invitations/{value}".to_owned()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"/api/invitations/{value}/check".to_owned()),
        "{paths:?}"
    );
    // The bare entry of the query is said to have been there, and is not
    // named.
    let feed: Vec<&a2m_core::Finding> = all
        .iter()
        .filter(|finding| {
            finding.kind().as_str() == "exchange"
                && matches!(finding.subject(), a2m_core::Subject::Address(address) if address.path() == "/api/feed")
        })
        .collect();
    assert_eq!(feed.len(), 1, "{paths:?}");
    let found: Json = serde_json::from_str(feed[0].found()).unwrap();
    assert_eq!(found["query"], json!({}));
    assert_eq!(found["query_left_out"], true);
    // What the web pack drafts from, kept as data.
    let screen = all
        .iter()
        .find(|finding| {
            finding.kind().as_str() == "screen"
                && matches!(finding.subject(), a2m_core::Subject::Address(address) if address.path() == "/invitations/{value}")
        })
        .unwrap();
    let text = std::fs::read_to_string(shared_invitation_screen()).unwrap_or_default();
    let shared: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    let wanted = json!({"kind": "screen", "path": "/invitations/{value}", "found": screen.found()});
    assert_eq!(
        shared,
        wanted,
        "write this into {}:\n{}",
        shared_invitation_screen().display(),
        serde_json::to_string_pretty(&wanted).unwrap()
    );
    assert!(site.writes().await.is_empty());
    bench.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_matched_screen_keeps_what_each_of_its_pages_showed() {
    let site = Site::start().await;
    let bench = Bench::new("notes-matched", no_secrets());
    let start = site.url("/");
    let look = Look {
        paths: vec!["/notes/N-1", "/notes/N-2"],
        depth: Some(0),
        ..Look::default()
    };
    let seen = must(bench.run("web_snapshot", &start, &look).await);
    check(&seen);
    let (finding, _) = seen.one("screen", "/notes/{note_id}");
    // The page of `N-2` tried to write as it loaded; the matched screen
    // says so, though its snapshot is of `N-1`.
    assert!(
        finding
            .limitations()
            .iter()
            .any(|one| one.code == LimitationCode::NotExercised && one.sentence.contains("POST 1")),
        "{:?}",
        finding.limitations()
    );
    assert!(
        site.control("/__control/traps").await["writes"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    bench.end().await;
}
