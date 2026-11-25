//! Registration, and the rules that need no browser: types and shapes,
//! path templates in two tiers, links that look like an action, and what
//! a screen finding keeps.

use std::collections::BTreeMap;

use a2m_core::EvidenceLevel;
use a2m_pack_web_browser::{ElementState, KNOWN_PLACES, MASK, Snapshot, SnapshotElement};
use a2m_provider::codes as contract;
use serde_json::json;

use super::*;
use crate::{
    Answer, Exchange, Segment, Shape, Tier, infer_endpoints, looks_like_an_action, type_of_text,
};

#[test]
fn a_registry_with_only_this_pack_builds_and_its_digest_is_stable() {
    let first = Bench::new("registry", no_secrets());
    // The pack produces `endpoint` findings, a kind the HTTP pack owns. With
    // that pack absent it is registered all the same, and the registry says
    // what is missing.
    assert_eq!(first.problems.len(), 1, "{:#?}", first.problems);
    let problem = &first.problems[0];
    assert!(problem.registered);
    assert_eq!(problem.failure.code(), &contract::KIND_OWNER_MISSING);

    let address = "http://app.example.com/";
    let names: Vec<String> = first
        .registry
        .discoverers_for(&Bench::description(address))
        .iter()
        .map(|handle| handle.declared().name.to_string())
        .collect();
    assert_eq!(
        names,
        ["web_auth", "web_capture", "web_probe", "web_snapshot"]
    );
    // The discoverer a person drives is given to a capture job alone
    // (decision 0392, point 2).
    let watching: Vec<String> = first
        .registry
        .capture_discoverers_for(&Bench::description(address))
        .iter()
        .map(|handle| handle.declared().name.to_string())
        .collect();
    assert_eq!(watching, [crate::WEB_USE]);
    // No discoverer claims more than `discovered`, and none suits a surface
    // that is not reached over http.
    for handle in first.registry.discoverers_for(&Bench::description(address)) {
        assert!(handle.declared().max_level <= EvidenceLevel::Discovered);
    }
    assert!(
        first
            .registry
            .discoverers_for(&Bench::description("ftp://app.example.com/"))
            .is_empty()
    );

    let listing = first.registry.listing();
    assert_eq!(listing.len(), 1);
    let manifest = &listing[0].manifest;
    assert_eq!(manifest.provider.name.as_str(), crate::PACK);
    assert!(manifest.modes.is_empty() && manifest.points.driver.is_empty());
    // Every code this crate produces is declared, with its class.
    for (class, code) in crate::codes::ALL {
        assert!(
            manifest
                .failure_codes
                .iter()
                .any(|declared| declared.class == *class && declared.code.as_str() == *code),
            "`{code}` is not declared"
        );
    }

    let second = Bench::new("registry-again", no_secrets());
    assert_eq!(first.registry.digest(), second.registry.digest());
}

#[test]
fn the_manifest_declares_every_known_place_of_a_browser_and_the_profile_root() {
    // The manifest as the product embeds it, not as a test widens it.
    let bench = {
        let folder = Folder::new("manifest");
        let logs = folder.0.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let host = HostServices {
            supervisor: Supervisor::new(SupervisorLimits::default()),
            roots: Roots::none(),
            redact: no_secrets(),
            telemetry: a2m_telemetry::start(&TelemetrySettings::default(), no_secrets(), &logs)
                .unwrap(),
        };
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        crate::add_to(&mut builder, SharedBrowsers::default());
        (builder.build().0, folder)
    };
    let listing = bench.0.listing();
    let manifest = &listing[0].manifest;
    let declared: Vec<&str> = manifest
        .requirements
        .iter()
        .flat_map(|requirement| requirement.paths.iter().map(String::as_str))
        .collect();
    let known: Vec<&str> = KNOWN_PLACES.iter().map(|place| place.path).collect();
    assert_eq!(declared, known);
    assert_eq!(
        manifest
            .effects
            .write_roots
            .iter()
            .map(|root| root.as_str())
            .collect::<Vec<_>>(),
        [PROFILE_ROOT]
    );
    assert!(manifest.effects.processes.is_empty());
}

#[test]
fn a_text_is_typed_by_its_form_and_a_shape_holds_no_value() {
    for (text, kind) in [
        ("", "empty"),
        ("42", "integer"),
        ("-7", "integer"),
        ("12.50", "decimal"),
        ("true", "boolean"),
        ("2026-11-17", "date"),
        ("2026-11-17T09:30:00Z", "date_time"),
        ("0f8fad5b-d9cb-469f-a165-70867728950e", "uuid"),
        ("9f86d081884c7d659a2feaa0c55ad015", "hex"),
        ("A-100", "text"),
        ("1.2.3", "text"),
    ] {
        assert_eq!(type_of_text(text), kind, "{text:?}");
    }

    let first = Shape::of(&json!({
        "order_id": "A-100", "total": 12.5, "lines": [{"code": "L-101", "quantity": 3}],
        "note": "Deliver to the side door",
    }));
    let second = Shape::of(&json!({
        "order_id": "A-101", "total": 7, "lines": [], "shipped_on": "2026-11-17",
    }));
    let merged = first.merged(&second);
    let text = merged.to_json().to_string();
    for value in ["A-100", "A-101", "L-101", "side door", "12.5", "2026-11-17"] {
        assert!(!text.contains(value), "{value} in {text}");
    }
    assert_eq!(
        merged.to_json(),
        json!({
            "type": "object",
            "fields": {
                "lines": {"type": "list", "seen_in_all": 2, "items": {"type": "object", "fields": {
                    "code": {"type": "text"}, "quantity": {"type": "integer"}}}},
                "note": {"type": "text", "optional": true},
                "order_id": {"type": "text", "seen_in_all": 2},
                "shipped_on": {"type": "date", "optional": true},
                "total": {"type": ["decimal", "integer"], "seen_in_all": 2},
            },
        })
    );
    assert!(merged.has_optional() && !first.has_optional());
    // The same observations in the other order give the same bytes.
    assert_eq!(
        second.merged(&first).to_json().to_string(),
        merged.to_json().to_string()
    );
}

fn get(path: &str, answer: serde_json::Value) -> Exchange {
    Exchange {
        method: "GET".to_owned(),
        origin: "http://app.example.com".to_owned(),
        path: path.to_owned(),
        query: BTreeMap::new(),
        status: Some(200),
        answer: Answer::Json(Shape::of(&answer)),
        made_by: None,
        credential_headers: Vec::new(),
        set_headers: Vec::new(),
        query_left_out: false,
        body: None,
    }
}

#[test]
fn tier_one_takes_identifiers_by_form_and_tier_two_only_when_asked() {
    let order = |id: &str| json!({"order_id": id, "lines": []});
    let exchanges = [
        get("/api/invoices/1001", json!({"number": 1001})),
        get("/api/invoices/1002", json!({"number": 1002, "paid": true})),
        get("/api/orders/A-100", order("A-100")),
        get("/api/orders/A-101", order("A-101")),
        get("/api/stock", json!([{"item": "L-101", "on_hand": 4}])),
        Exchange {
            method: "PUT".to_owned(),
            origin: "http://app.example.com".to_owned(),
            path: "/api/orders/A-100/items/L-101".to_owned(),
            query: BTreeMap::new(),
            status: None,
            answer: Answer::Refused,
            made_by: None,
            credential_headers: Vec::new(),
            set_headers: Vec::new(),
            query_left_out: false,
            body: None,
        },
    ];

    // Tier one: a number is a parameter; `A-100` is not an identifier by
    // its form, so the two orders stay two endpoints.
    let plain = infer_endpoints(&exchanges, false);
    let templates: Vec<String> = plain
        .iter()
        .map(|endpoint| format!("{} {}", endpoint.method, endpoint.template()))
        .collect();
    assert_eq!(
        templates,
        [
            "GET /api/invoices/{p1}",
            "GET /api/orders/A-100",
            "GET /api/orders/A-101",
            "GET /api/stock",
            "PUT /api/orders/A-100/items/L-101",
        ]
    );
    let invoices = &plain[0];
    assert_eq!(invoices.seen, 2);
    assert_eq!(invoices.level(), EvidenceLevel::Discovered);
    assert!(matches!(
        &invoices.segments[2],
        Segment::Parameter { tier: Tier::One, literals, .. } if literals.len() == 2
    ));
    let found = invoices.found();
    assert_eq!(found["template"]["level"], "inferred");
    assert_eq!(found["response"]["fields"]["paid"]["optional"], true);
    // The values themselves are nowhere in what is found.
    assert!(!found.to_string().contains("1001"));
    // An endpoint the system never answered is inferred.
    let put = &plain[4];
    assert_eq!(put.level(), EvidenceLevel::Inferred);
    assert_eq!(put.found()["answered"], false);
    assert_eq!(plain[1].found()["template"]["level"], "observed");

    // Tier two: the segment that varied between otherwise identical
    // requests becomes a parameter, and the endpoint is inferred although
    // it answered. Requests that answer differently are not joined.
    let varied = infer_endpoints(&exchanges, true);
    let templates: Vec<String> = varied
        .iter()
        .map(|endpoint| format!("{} {}", endpoint.method, endpoint.template()))
        .collect();
    assert_eq!(
        templates,
        [
            "GET /api/invoices/{p1}",
            "GET /api/orders/{p1}",
            "GET /api/stock",
            "PUT /api/orders/A-100/items/L-101",
        ]
    );
    let orders = &varied[1];
    assert!(orders.has_tier_two() && orders.answered());
    assert_eq!(orders.level(), EvidenceLevel::Inferred);
    assert_eq!(orders.found()["template"]["parameters"][0]["tier"], 2);
    assert_eq!(orders.sources, [2, 3]);
    assert_eq!(varied[0].level(), EvidenceLevel::Discovered);

    // The same exchanges give the same endpoints, byte for byte.
    let again = infer_endpoints(&exchanges, true);
    assert_eq!(
        varied
            .iter()
            .map(|e| e.found().to_string())
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|e| e.found().to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_link_that_looks_like_an_action_is_told_by_its_address_or_its_name() {
    for acts in [
        "/orders/A-100/delete",
        "/session/sign-out",
        "/account/logout?next=/",
        "Sign out",
        "Log off",
        "Remove line",
        "/invoices/7/pay",
        "/items/L-101/REMOVE",
        "Approve",
    ] {
        assert!(looks_like_an_action(acts), "{acts}");
    }
    for plain in [
        "/orders",
        "/orders/A-100/items/new",
        "Orders",
        "Add line item",
        "/payments",
        "/senders/list",
        "Open order",
    ] {
        assert!(!looks_like_an_action(plain), "{plain}");
    }
}

#[test]
fn a_screen_finding_keeps_structure_and_types_and_no_value() {
    let element = |role: &str, name: &str, value: Option<&str>, depth: usize| SnapshotElement {
        reference: String::new(),
        role: role.to_owned(),
        name: name.to_owned(),
        value: value.map(str::to_owned),
        states: Vec::new(),
        level: None,
        depth,
    };
    let mut quantity = element("textbox", "Quantity", Some("12"), 1);
    quantity.states = vec![ElementState::Required];
    let snapshot = Snapshot {
        address: "http://app.example.com/orders/A-100".to_owned(),
        title: "Order".to_owned(),
        elements: vec![
            element("navigation", "", None, 0),
            element("link", "Orders", None, 1),
            element("form", "", None, 0),
            element("textbox", "Description", Some("Oak board, planed"), 1),
            quantity,
            element("textbox", "Password", Some(MASK), 1),
            element("table", "Line items", None, 0),
            element("columnheader", "Item code", None, 1),
            element("row", "L-101 Oak board 12", None, 1),
            element("cell", "L-101", None, 2),
            element("link", "L-101", None, 3),
            element("cell", "12", None, 2),
            element("text", "Customer: Example Joinery", None, 0),
        ],
        cut: false,
    };
    let found = crate::screen::found(&snapshot, "Order");
    let text = found.to_string();
    for value in ["Oak board", "L-101", "Example Joinery", MASK, "\"12\""] {
        assert!(!text.contains(value), "{value} in {text}");
    }
    let elements = found["elements"].as_array().unwrap();
    assert_eq!(elements.len(), snapshot.elements.len());
    assert_eq!(
        elements[3],
        json!({"depth": 1, "role": "textbox", "name": "Description", "value_type": "text"})
    );
    assert_eq!(
        elements[4],
        json!({"depth": 1, "role": "textbox", "name": "Quantity", "value_type": "integer",
               "states": ["required"]})
    );
    assert_eq!(elements[5]["value_type"], "password");
    // A link outside a record is structure; a link in a row is data.
    assert_eq!(elements[1]["name"], "Orders");
    assert_eq!(
        elements[10],
        json!({"depth": 3, "role": "link", "name_type": "text"})
    );
    assert_eq!(
        elements[11],
        json!({"depth": 2, "role": "cell", "name_type": "integer"})
    );
    assert_eq!(elements[7]["name"], "Item code");
    // Only a link whose name no other link shares can be followed.
    let (names, shared) = crate::screen::link_names(&snapshot);
    assert_eq!((names, shared), (vec!["L-101", "Orders"], 0));

    // A term and a definition are named only as a pair: a term that labels
    // a definition of the same name. A list used as data keeps types.
    let listed = Snapshot {
        address: "http://app.example.com/timeline".to_owned(),
        title: "Timeline".to_owned(),
        elements: vec![
            element("DescriptionList", "", None, 0),
            element("term", "Status", None, 1),
            element("definition", "Status", None, 1),
            element("term", "Someone Example", None, 1),
            element("definition", "", None, 1),
            element("term", "2026-01-05", None, 1),
            element("definition", "Shipped to the side door", None, 1),
        ],
        cut: false,
    };
    let found = crate::screen::found(&listed, "Timeline");
    let text = found.to_string();
    for value in ["Someone Example", "2026-01-05", "side door"] {
        assert!(!text.contains(value), "{value} in {text}");
    }
    let elements = found["elements"].as_array().unwrap();
    assert_eq!(elements[1]["name"], "Status");
    assert_eq!(elements[2]["name"], "Status");
    assert_eq!(elements[3]["name_type"], "text");
    assert_eq!(elements[6]["name_type"], "text");
}

#[test]
fn the_names_of_a_dictionary_are_values_and_are_withheld() {
    let shape = Shape::of(&json!({
        "order_id": "A-100",
        "rates": {"USD": 1, "EUR": 2},
        "by_person": {"someone@example.com": {"seen": 1}},
    }));
    let text = shape.to_json().to_string();
    assert!(!text.contains("someone@example.com"), "{text}");
    assert!(shape.withheld());
    assert_eq!(
        shape.to_json()["fields"]["by_person"],
        json!({"type": "object", "names_withheld": true})
    );
    // Words are names: a dictionary of codes is read as structure, the
    // stated limit of a rule by form.
    assert_eq!(
        shape.to_json()["fields"]["rates"]["fields"]["USD"],
        json!({"type": "integer"})
    );
}

#[test]
fn a_page_that_cannot_be_read_puts_none_of_its_text_in_the_message() {
    let planted = "secret-for-tests-page-text";
    for error in [
        a2m_pack_web_browser::BrowserError::Unexpected(planted.to_owned()),
        a2m_pack_web_browser::BrowserError::RequestFailed(planted.to_owned()),
    ] {
        let failure = crate::look::failure(&error);
        assert_eq!(failure.code(), &crate::codes::PAGE_UNREADABLE);
        assert!(
            !failure.message().contains(planted),
            "{}",
            failure.message()
        );
        assert!(!format!("{failure:?}").contains(planted));
    }
}

#[test]
fn pages_of_one_screen_are_matched_only_when_one_segment_differs_and_nothing_else() {
    use crate::screen::{Matched, matched};
    let screen = |table: &str, heading: &str| {
        serde_json::json!({"format": "1", "title": "", "cut": false, "elements": [
            {"depth": 0, "role": "main", "name": ""},
            {"depth": 1, "role": "heading", "name": heading},
            {"depth": 1, "role": "table", "name": table},
            {"depth": 2, "role": "row"},
            {"depth": 3, "role": "columnheader", "name": "Code"},
        ]})
    };
    let origin = "http://127.0.0.1:4010";
    let lines = screen("Lines", "Order A-1");
    let other_heading = screen("Lines", "Order B-2");
    let other_table = screen("Parts", "Order C-3");
    let mut cut = screen("Lines", "x");
    cut["cut"] = serde_json::Value::Bool(true);
    let nothing = serde_json::json!({"format": "1", "title": "", "cut": false, "elements": [
        {"depth": 0, "role": "main", "name": ""},
    ]});
    // A heading that names the record does not keep two pages apart.
    assert_eq!(
        matched(&[
            (origin, "/orders/A-1", &lines),
            (origin, "/orders/B-2", &other_heading),
            (origin, "/orders/C-3", &other_table),
            (origin, "/orders/D-4", &cut),
            (origin, "/orders/E-5", &nothing),
            (origin, "/orders/F-6", &nothing),
            ("http://127.0.0.1:4011", "/orders/G-7", &lines),
        ]),
        [Matched {
            pattern: "/orders/{order_id}".to_owned(),
            pages: vec![0, 1],
        }]
    );
    // Two segments that differ are not one screen; nor is one page.
    assert!(matched(&[(origin, "/a/1", &lines), (origin, "/b/2", &lines)]).is_empty());
    assert!(matched(&[(origin, "/orders/A-1", &lines)]).is_empty());
    // A page two groups would hold is in neither.
    let groups = matched(&[
        (origin, "/orders/A-1/items/L-1", &lines),
        (origin, "/orders/A-1/items/L-2", &lines),
        (origin, "/orders/A-2/items/L-1", &lines),
    ]);
    assert!(groups.is_empty(), "{groups:?}");
    // With no plain word before it, the placeholder is `key`.
    assert_eq!(
        matched(&[(origin, "/A-1", &lines), (origin, "/B-2", &lines)])[0].pattern,
        "/{key}"
    );
    // One page seen twice at one address, as a start page that redirects
    // to a page a link also leads to, is one page: never `/{key}`.
    assert!(matched(&[(origin, "/orders", &lines), (origin, "/orders", &lines)]).is_empty());
}

#[test]
fn a_field_seen_in_every_answer_is_counted_and_never_required() {
    let one = Shape::of(&json!({"id": 1, "note": "a"}));
    let two = Shape::of(&json!({"id": 2}));
    let three = Shape::of(&json!({"id": 3, "note": "b"}));
    let merged = one.merged(&two).merged(&three);
    let json = merged.to_json();
    assert_eq!(json["fields"]["id"]["seen_in_all"], 3);
    assert_eq!(json["fields"]["note"]["optional"], true);
    assert!(json["fields"]["note"].get("seen_in_all").is_none());
    assert!(!json.to_string().contains("required"));
}

#[test]
fn a_short_number_in_a_path_stays_literal_unless_it_varies_or_has_four_digits() {
    let page = |n: &str| get(&format!("/api/items/page/{n}"), json!({"rows": []}));
    let alone = infer_endpoints(&[page("2"), page("2")], false);
    assert_eq!(alone[0].template(), "/api/items/page/2");
    assert!(alone[0].keeps_a_short_number());
    // Without tier two a second value is another endpoint, not a parameter.
    let apart = infer_endpoints(&[page("2"), page("3")], false);
    assert_eq!(apart.len(), 2);
    // Where it varies and tier two is asked, it is a parameter.
    let varied = infer_endpoints(&[page("2"), page("3")], true);
    assert_eq!(varied.len(), 1);
    assert_eq!(varied[0].template(), "/api/items/page/{p1}");
    assert!(!varied[0].keeps_a_short_number());
    assert_eq!(
        varied[0].found()["template"]["parameters"][0]["values_seen"],
        2
    );
    // Four digits are a parameter by form.
    let long = infer_endpoints(&[page("2024")], false);
    assert_eq!(long[0].template(), "/api/items/page/{p1}");
    assert!(!long[0].keeps_a_short_number());
}

#[test]
fn two_words_the_capture_uses_as_fixed_names_are_not_one_parameter() {
    let answer = json!({"rows": []});
    let words = [
        get("/api/reports/summary", answer.clone()),
        get("/api/reports/daily", answer.clone()),
        get("/api/reports/summary/export", json!({"link": "x"})),
        get("/api/reports/daily/export", json!({"link": "x"})),
    ];
    let kept = infer_endpoints(&words, true);
    assert!(kept.iter().any(|e| e.template() == "/api/reports/summary"));
    assert!(kept.iter().any(|e| e.template() == "/api/reports/daily"));
    // Names with no such use elsewhere are values of one parameter.
    let people = [
        get("/api/people/alice", answer.clone()),
        get("/api/people/bob", answer),
    ];
    let joined = infer_endpoints(&people, true);
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].template(), "/api/people/{p1}");
}

#[test]
fn an_object_keyed_by_identifiers_is_one_map_whatever_its_size() {
    let small = Shape::of(&json!({"by_id": {"1": {"n": 1}, "2": {"n": 2}}}));
    assert_eq!(
        small.to_json()["fields"]["by_id"],
        json!({"type": "map", "items": {"type": "object", "fields": {"n": {"type": "integer", "seen_in_all": 2}}}})
    );
    let mut many = serde_json::Map::new();
    for id in 0..100 {
        many.insert(id.to_string(), json!(id));
    }
    let big = Shape::of(&serde_json::Value::Object(many));
    assert_eq!(
        big.to_json(),
        json!({"type": "map", "items": {"type": "integer"}})
    );
    assert!(!big.withheld());
}
