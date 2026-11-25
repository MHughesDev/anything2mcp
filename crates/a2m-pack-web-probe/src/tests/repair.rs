//! The repair proposer's rule (decision 0999), on the shapes the test
//! application's form of a line item shows under each change of its
//! control interface, written by hand as a `web_snapshot` finding holds
//! them: a reword pairs the renamed box and button; a wrap moves the
//! fingerprints that named the heading; two boxes renamed at once, no
//! snapshot from before, a locator of a kind the rule does not read, and a
//! snapshot cut short propose nothing, each saying why.

use a2m_core::{
    Address, EvidenceLevel, FailureCode, Finding, FindingKind, ProducedBy, Source, Subject,
    Timestamp,
};
use a2m_provider::{RepairAsk, RepairFile, RepairProposal, RepairProposer};
use serde_json::json;

use crate::repair::ScreenRepair;

/// The screen model of the form, as the `pages` project holds it.
const SCREEN: &str = r#"# The form of one line item.
screen: line_item
surface: app
reach: {url: "/orders/{order_id}/items/{item_code}"}
ready: {visible: {role: heading, name: "Line Item Details"}}
fields:
  description:
    type: text
    control: text
    locate:
      - {role: textbox, name: Description}
      - {label: Description}
    fingerprint: {section: {role: form, name: ""}, near_text: "Line Item Details"}
  quantity:
    type: integer
    control: number
    locate:
      - {role: textbox, name: Quantity}
    fingerprint: {section: {role: form, name: ""}, near_text: "Line Item Details"}
  length_in:
    type: length_inches
    control: measurement
    locate:
      - {role: textbox, name: Length}
    fingerprint: {section: {role: group, name: Dimensions}, near_text: inches}
actions:
  save:
    locate:
      - {role: button, name: Save}
    fingerprint: {section: {role: form, name: ""}}
"#;

/// The form's shape: its heading inside the form unless `wrapped`, the
/// boxes named as given, and the button.
fn form(description: &str, quantity: &str, save: &str, wrapped: bool) -> serde_json::Value {
    let mut elements = vec![json!({"depth": 0, "role": "main", "name": ""})];
    elements.push(json!({"depth": 1, "role": "heading", "name": "Order A-100", "level": 1}));
    let base = if wrapped {
        elements.push(json!({"depth": 1, "role": "region", "name": "Line Item Details"}));
        elements
            .push(json!({"depth": 2, "role": "heading", "name": "Line Item Details", "level": 2}));
        elements.push(json!({"depth": 2, "role": "form", "name": ""}));
        3
    } else {
        elements.push(json!({"depth": 1, "role": "form", "name": ""}));
        elements
            .push(json!({"depth": 2, "role": "heading", "name": "Line Item Details", "level": 2}));
        2
    };
    for name in ["Item code", description, quantity] {
        elements
            .push(json!({"depth": base, "role": "textbox", "name": name, "value_type": "text"}));
    }
    elements.push(json!({"depth": base, "role": "group", "name": "Dimensions"}));
    elements.push(
        json!({"depth": base + 1, "role": "textbox", "name": "Length", "value_type": "decimal"}),
    );
    elements.push(json!({"depth": base + 1, "name_type": "text", "role": "generic"}));
    elements.push(json!({"depth": base, "role": "button", "name": save}));
    elements.push(json!({"depth": base, "role": "status", "name": ""}));
    json!({"format": "1", "title": "Order A-100", "cut": false, "elements": elements})
}

fn finding(found: &serde_json::Value) -> Finding {
    Finding::about(
        Subject::Address(
            Address::new("http://127.0.0.1:4010", "/orders/A-100/items/L-101").unwrap(),
        ),
        FindingKind::new("screen").unwrap(),
        found.to_string(),
        Source::LiveSystem,
        ProducedBy {
            provider: "web_probe".to_owned(),
            version: "0.1.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_800_000_000_000),
    )
    .level(EvidenceLevel::Discovered)
    .build()
    .unwrap()
}

fn files(text: &str) -> Vec<RepairFile> {
    vec![
        RepairFile {
            path: "screens/line_item.yaml".to_owned(),
            text: text.to_owned(),
        },
        RepairFile {
            path: "screens/orders.yaml".to_owned(),
            text: "screen: orders\nsurface: app\n".to_owned(),
        },
    ]
}

fn propose(
    text: &str,
    code: &str,
    before: Option<&serde_json::Value>,
    now: &serde_json::Value,
) -> RepairProposal {
    let files = files(text);
    let code = FailureCode::new(code).unwrap();
    let before = before.map(finding);
    let now = finding(now);
    ScreenRepair.propose(&RepairAsk {
        entry: "line_item",
        code: &code,
        files: &files,
        before: before.as_ref(),
        now: &now,
    })
}

fn reason(proposal: RepairProposal) -> String {
    match proposal {
        RepairProposal::None { reason } => reason,
        RepairProposal::Proposed { text, .. } => panic!("a repair was proposed:\n{text}"),
    }
}

#[test]
fn a_reword_pairs_the_renamed_box_and_button_and_changes_only_their_names() {
    let before = form("Description", "Quantity", "Save", false);
    let now = form("Description", "Qty", "Save line item", false);
    let RepairProposal::Proposed {
        path,
        text,
        because,
    } = propose(SCREEN, "locator_not_found", Some(&before), &now)
    else {
        panic!("no repair was proposed");
    };
    assert_eq!(path, "screens/line_item.yaml");
    let expected = SCREEN
        .replace(
            "{role: textbox, name: Quantity}",
            "{role: textbox, name: Qty}",
        )
        .replace(
            "{role: button, name: Save}",
            "{role: button, name: Save line item}",
        );
    assert_eq!(text, expected);
    assert_eq!(because.len(), 2, "{because:?}");
    assert!(
        because[0].contains("`Quantity`") && because[0].contains("`Qty`"),
        "{because:?}"
    );
    assert!(because[1].contains("`Save line item`"), "{because:?}");
}

#[test]
fn a_wrap_moves_the_fingerprints_that_named_the_heading_and_no_other() {
    let before = form("Description", "Quantity", "Save", false);
    let now = form("Description", "Quantity", "Save", true);
    let RepairProposal::Proposed { text, because, .. } =
        propose(SCREEN, "fingerprint_mismatch", Some(&before), &now)
    else {
        panic!("no repair was proposed");
    };
    let expected = SCREEN.replace(
        "section: {role: form, name: \"\"}, near_text: \"Line Item Details\"",
        "section: {role: region, name: Line Item Details}, near_text: \"Line Item Details\"",
    );
    assert_eq!(text, expected);
    // The description and the quantity; not the length, whose text is no
    // element's name, nor the button, whose fingerprint names no text.
    assert_eq!(because.len(), 2, "{because:?}");
    assert!(
        because
            .iter()
            .all(|line| line.contains("the region `Line Item Details`"))
    );
}

#[test]
fn two_boxes_renamed_at_once_propose_nothing_because_the_pairing_is_not_one_to_one() {
    let before = form("Description", "Quantity", "Save", false);
    let now = form("Item", "Qty", "Save", false);
    let why = reason(propose(SCREEN, "locator_not_found", Some(&before), &now));
    assert!(
        why.contains("2 element(s) of the role `textbox` are gone and 2 are new"),
        "{why}"
    );
}

#[test]
fn without_a_snapshot_from_before_nothing_is_proposed() {
    let now = form("Description", "Qty", "Save line item", false);
    let why = reason(propose(SCREEN, "locator_not_found", None, &now));
    assert!(why.contains("from before the change"), "{why}");
}

#[test]
fn a_locator_of_a_kind_the_rule_does_not_read_and_a_cut_snapshot_propose_nothing() {
    let before = form("Description", "Quantity", "Save", false);
    let now = form("Description", "Qty", "Save", false);
    let by_test_id = SCREEN.replace(
        "      - {role: textbox, name: Quantity}\n",
        "      - {role: textbox, name: Quantity}\n      - {test_id: quantity}\n",
    );
    let why = reason(propose(
        &by_test_id,
        "locator_not_found",
        Some(&before),
        &now,
    ));
    assert!(why.contains("of a kind these rules do not read"), "{why}");

    let mut cut = now.clone();
    cut["cut"] = json!(true);
    let why = reason(propose(SCREEN, "locator_not_found", Some(&before), &cut));
    assert!(why.contains("cut short"), "{why}");
}

#[test]
fn a_page_that_still_fits_proposes_nothing_and_says_the_drift_is_not_one_it_sees() {
    let before = form("Description", "Quantity", "Save", false);
    let why = reason(propose(SCREEN, "locator_not_found", Some(&before), &before));
    assert!(why.contains("not one these rules see"), "{why}");
}
