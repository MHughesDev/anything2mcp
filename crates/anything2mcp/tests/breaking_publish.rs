//! Package YL1 through the product (decision 0439): a person publishes
//! version 1.0.0 of an `example` target; its next draft makes one break of
//! each kind on the record's closed list, and changes one tool in a way
//! that breaks nothing. The publish page names each breaking tool right
//! after where it leads, with why; `publish.request`'s answer names the
//! same; the person still
//! publishes it by pressing; and `diff` gives the same classification for
//! the two published versions. A third draft that breaks nothing shows no
//! break on the page or in the answer. Of a kind the record names two ways,
//! one is made here: an input's type changes (its words narrowing is the
//! other), and an output is gone (one that may now be absent is the
//! other); `a2m-files`' own tests make both ways of each.
//!
//! The target is the order system's JSON interface, read and written from
//! fixtures by the offline tests a publish runs: nothing is sent to any
//! system. The engine is the same `LocalEngine` that `serve` starts, run in
//! this process because the program ships no presence prompt a test can
//! pass; every command is the real program.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use a2m_actions::{BoxFuture, Paths, PersonSurface};
use a2m_local::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText, StopReason};
use anything2mcp::{EngineOptions, LocalEngine, PASSPHRASE_VARIABLE};
use common::{
    Folder, IN_PROCESS_VARIABLE, PASSPHRASE, Surface, USER, copy_changed, project_source,
};
use serde_json::{Value as Json, json};

/// A presence prompt that a person passes, and that counts.
struct Passed(AtomicU32);

impl PresencePrompt for Passed {
    fn available(&self) -> BoxFuture<'_, Availability> {
        Box::pin(async { Availability::ready(PromptName::Polkit, "The prompt of a test.") })
    }

    fn ask(
        &self,
        _text: PromptText,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'_, PromptAnswer> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            PromptAnswer::Present(PromptName::Polkit)
        })
    }
}

const TARGET_YAML: &str = "schema_version: 1
target: example
title: Example order entry
surfaces:
  api:
    kind: http
    base: \"http://127.0.0.1:4010\"
environments:
  test:
    kind: test
    test_records: [[\"A-102\", \"L-900\"]]
default_environment: test
environment: {locale: en-US, timezone: UTC}
drift_risk: low
";

const TYPES_YAML: &str = "types:
  order_status:
    base: choice
    codes: {draft: \"Draft\", submitted: \"Submitted\"}
  length_unit:
    base: choice
    codes: {in: \"Inches\", cm: \"Centimetres\"}
";

/// The orders of version 1: a read of the order, one that version 2
/// removes, one of its status, and one of its customer.
const ORDERS_1: &str = "operation: example_order_get
title: Get an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
  channel: {type: identifier, required: false, description: \"Where the order was taken, such as web\"}
outputs:
  status: {type: identifier}
  customer: {type: text}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Reads the status and the customer of one order.
---
operation: example_order_find
title: Find an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
outputs:
  status: {type: identifier}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Finds one order by its number and reads its status, for a caller that has only the number.
---
operation: example_order_status_get
title: Get the status of an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
outputs:
  status: {type: identifier}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Reads only the status of one order, for a caller that needs nothing else.
---
operation: example_order_summary_get
title: Summarize an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
outputs:
  customer: {type: text}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Reads the customer of one order, for a summary line.
";

/// Version 2: `example_order_get` loses an input and an output;
/// `example_order_find` is gone; `example_order_status_get` gains a
/// required input with no default and its output's type changes; and
/// `example_order_summary_get` gains an optional input and a new
/// description, which breaks nothing.
const ORDERS_2: &str = "operation: example_order_get
title: Get an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
outputs:
  status: {type: identifier}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Reads the status and the customer of one order.
---
operation: example_order_status_get
title: Get the status of an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
  region: {type: identifier, description: \"The region whose books hold the order, such as north\"}
outputs:
  status: {type: order_status}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Reads only the status of one order, for a caller that needs nothing else.
---
operation: example_order_summary_get
title: Summarize an order
effect: read
key: order_id
inputs:
  order_id: {type: identifier, description: \"Order number shown in the application\"}
  channel: {type: identifier, required: false, description: \"Where the order was taken, such as web\"}
outputs:
  customer: {type: text}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: order_get
description: >
  Reads the customer of one order, for a summary line or a list of orders.
";

/// The line items of version 1: a read and a write.
fn line_items(version: u8) -> String {
    let (key, unit, quantity, effect) = match version {
        1 => (
            "[order_id, item_code]",
            "text",
            "{type: integer}",
            "effect: write",
        ),
        // The key's order changes, the unit's type changes, and the write
        // cannot be undone and may reach data beyond its record.
        _ => (
            "[item_code, order_id]",
            "length_unit",
            "{type: integer}",
            "effect: irreversible\nhazard: data_elsewhere",
        ),
    };
    format!(
        "operation: example_line_item_get
title: Get a line item
effect: read
key: {key}
inputs:
  order_id: {{type: identifier, description: \"Order number shown in the application\"}}
  item_code: {{type: identifier, description: \"Item code, such as L-101\"}}
  unit: {{type: {unit}, required: false, description: \"The unit a length is shown in\"}}
outputs:
  description: {{type: text}}
  quantity: {quantity}
  length_in: {{type: length_ft_in}}
bindings:
  - mode: http
    verb: read
    status: verified
    endpoint: line_item_get
description: >
  Reads one line item of an order.
---
operation: example_line_item_upsert
title: Create or update a line item
{effect}
key: [order_id, item_code]
commit_points: 1
inputs:
  order_id: {{type: identifier, description: \"Order number shown in the application\"}}
  item_code: {{type: identifier, description: \"Item code, such as L-101\"}}
  description: {{type: text, description: \"What the item is\"}}
  quantity: {{type: integer, description: \"How many\"}}
  length_in: {{type: length_ft_in, description: \"Length, in inches\"}}
outputs:
  description: {{type: text}}
  quantity: {{type: integer}}
  length_in: {{type: length_ft_in}}
bindings:
  - mode: http
    verb: upsert
    status: verified
    endpoint: line_item_put
    body:
      description: input.description
      quantity: input.quantity
      length_in: input.length_in
    read_back: {{endpoint: line_item_get}}
description: >
  Creates or updates one line item on a draft order.
"
    )
}

fn test_file(tool: &str, fixture: &str, inputs: &str, expect: &str) -> String {
    format!("test: {tool}\nkind: offline\nfixture: {fixture}\ninputs: {inputs}\nexpect: {expect}\n")
}

/// The target's files for one version: 1, 2, or 3, which is 2 with one
/// description changed.
fn write_version(project: &Path, version: u8) {
    let target = project.join("targets").join("example");
    let orders = project_source("orders").join("targets").join("example");
    for part in ["endpoints", "fixtures"] {
        copy_changed(&orders.join(part), &target.join(part), &[]);
    }
    for part in ["operations", "tests"] {
        let _ = std::fs::remove_dir_all(target.join(part));
        std::fs::create_dir_all(target.join(part)).unwrap();
    }
    std::fs::write(target.join("target.yaml"), TARGET_YAML).unwrap();
    std::fs::write(target.join("types.yaml"), TYPES_YAML).unwrap();
    let orders = match version {
        1 => ORDERS_1.to_owned(),
        2 => ORDERS_2.to_owned(),
        _ => ORDERS_2.replace(
            "Reads the status and the customer of one order.",
            "Reads the status of one order.",
        ),
    };
    std::fs::write(target.join("operations").join("orders.yaml"), orders).unwrap();
    std::fs::write(
        target.join("operations").join("line_items.yaml"),
        line_items(version),
    )
    .unwrap();
    let order = "{order_id: \"A-100\"}";
    let mut tests = vec![
        (
            "example_order_get",
            test_file(
                "example_order_get",
                "order_get",
                order,
                if version == 1 {
                    "{status: draft, customer: \"Example Customer One\"}"
                } else {
                    "{status: draft}"
                },
            ),
        ),
        (
            "example_order_status_get",
            test_file(
                "example_order_status_get",
                "order_get",
                if version == 1 {
                    order
                } else {
                    "{order_id: \"A-100\", region: north}"
                },
                "{status: draft}",
            ),
        ),
        (
            "example_order_summary_get",
            test_file(
                "example_order_summary_get",
                "order_get",
                order,
                "{customer: \"Example Customer One\"}",
            ),
        ),
        (
            "example_line_item_get",
            test_file(
                "example_line_item_get",
                "line_item_get",
                "{order_id: \"A-102\", item_code: \"L-900\"}",
                "{description: \"Test rail\", quantity: 2, length_in: \"90.5\"}",
            ),
        ),
        (
            "example_line_item_upsert",
            test_file(
                "example_line_item_upsert",
                "line_item_upsert",
                "{order_id: \"A-102\", item_code: \"L-900\", description: \"Test rail\", quantity: 2, length_in: \"90.5\"}",
                "{description: \"Test rail\", quantity: 2, length_in: \"90.5\"}",
            ),
        ),
    ];
    if version == 1 {
        tests.push((
            "example_order_find",
            test_file("example_order_find", "order_get", order, "{status: draft}"),
        ));
    }
    for (name, text) in tests {
        std::fs::write(target.join("tests").join(format!("{name}.yaml")), text).unwrap();
    }
    std::fs::write(
        project.join("anything2mcp.toml"),
        "# The project of the imaginary `example` system.\n",
    )
    .unwrap();
}

/// The real program, run in the project folder as an agent runs it.
async fn run(home: &Path, project: &Path, args: &[&str]) -> (Option<i32>, Json) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
    command
        .arg("--json")
        .args(args)
        .current_dir(project)
        .env_remove(PASSPHRASE_VARIABLE)
        .env("A2M_HOME", home)
        .env("USER", USER)
        .env(IN_PROCESS_VARIABLE, "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let done = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    let object = serde_json::from_slice(&done.stdout).unwrap_or_else(|_| {
        panic!(
            "`{}` printed no single object: {} / {}",
            args.join(" "),
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        )
    });
    (done.status.code(), object)
}

/// An agent asks to publish `version`: the answer, and the page the person
/// is sent to.
async fn ask(home: &Path, project: &Path, version: &str) -> (Json, String, String) {
    let (code, asked) = run(home, project, &["publish", "example", "--version", version]).await;
    assert_eq!(code, Some(3), "{asked}");
    assert_eq!(asked["result"]["state"], "needs_person", "{asked}");
    let link = asked["result"]["link"].as_str().unwrap().to_owned();
    let page = http().get(&link).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    (asked, link, page.text().await.unwrap())
}

fn http() -> reqwest::Client {
    a2m_auth::tls::client_builder(&a2m_auth::tls::Trust::system().unwrap())
        .no_proxy()
        .build()
        .unwrap()
}

/// The person presses "Carry out" on the page.
async fn press(link: &str, text: &str) {
    let marker = "name=\"once\" value=\"";
    let from = text.find(marker).unwrap() + marker.len();
    let once = &text[from..from + text[from..].find('"').unwrap()];
    let origin = &link[..link.find("/act/").unwrap()];
    let pressed = http()
        .post(format!("{link}/confirm"))
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("once={once}"))
        .send()
        .await
        .unwrap();
    let status = pressed.status().as_u16();
    let said = pressed.text().await.unwrap();
    assert_eq!(status, 200, "{said}");
    assert!(said.contains("was carried out as you, once"), "{said}");
}

/// The breaks each tool of version 2 makes, as `change` tags, in the
/// closed list's order.
fn expected_breaks() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "example_line_item_get",
            vec!["input_type_changed", "key_changed"],
        ),
        (
            "example_line_item_upsert",
            vec!["effect_rose", "hazard_changed"],
        ),
        ("example_order_find", vec!["tool_gone"]),
        ("example_order_get", vec!["input_gone", "output_gone"]),
        (
            "example_order_status_get",
            vec!["input_required", "output_type_changed"],
        ),
    ]
}

fn tags(tool: &Json) -> Vec<String> {
    tool["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["change"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_break_is_named_first_on_the_publish_page_and_in_the_answer_and_the_publish_still_goes()
 {
    let home = Folder::new("home");
    let project = Folder::new("breaking");
    write_version(project.path(), 1);
    let installation = Paths::under(home.path()).installation_config;
    std::fs::create_dir_all(&installation).unwrap();
    std::fs::write(
        installation.join("settings.toml"),
        "[vault]\nbackend = \"passphrase\"\n",
    )
    .unwrap();
    let mut options = EngineOptions::of_machine(
        Paths::under(home.path()),
        std::collections::BTreeMap::from([
            ("A2M_HOME".to_owned(), home.path().display().to_string()),
            ("USER".to_owned(), USER.to_owned()),
            (PASSPHRASE_VARIABLE.to_owned(), PASSPHRASE.to_owned()),
        ]),
        Surface::new() as Arc<dyn PersonSurface>,
    );
    options.project = Some(project.path().to_path_buf());
    let prompt = Arc::new(Passed(AtomicU32::new(0)));
    let local = LocalEngine::start(options, Arc::clone(&prompt) as Arc<dyn PresencePrompt>)
        .await
        .unwrap();
    let serving = tokio::spawn(local.serve());
    let (home_path, project_path) = (home.path(), project.path());

    // Version 1: nothing to compare with, so nothing is said of breaks.
    let (asked, link, text) = ask(home_path, project_path, "1.0.0").await;
    assert!(asked["result"].get("breaking").is_none(), "{asked}");
    assert!(!text.contains("<h2>What breaks</h2>"), "{text}");
    press(&link, &text).await;

    // Version 2 breaks each kind of the closed list once.
    write_version(project_path, 2);
    let (asked, link, text) = ask(home_path, project_path, "2.0.0").await;
    let result = &asked["result"];
    assert_eq!(result["compared_with"], "1.0.0", "{asked}");
    let breaking = result["breaking"].as_array().unwrap();
    let named: Vec<(&str, Vec<String>)> = breaking
        .iter()
        .map(|tool| (tool["tool"].as_str().unwrap(), tags(tool)))
        .collect();
    let expected: Vec<(&str, Vec<String>)> = expected_breaks()
        .into_iter()
        .map(|(tool, changes)| (tool, changes.into_iter().map(str::to_owned).collect()))
        .collect();
    assert_eq!(named, expected, "{asked}");
    assert!(breaking.iter().all(|tool| tool["class"] == "breaking"));
    // The compatible tool is not among the breaks.
    assert!(
        !asked
            .to_string()
            .contains("\"tool\":\"example_order_summary_get\""),
        "{asked}"
    );
    let warned = asked["warnings"].to_string();
    assert!(
        warned.contains("This publish breaks 5 of version `1.0.0`'s tools"),
        "{asked}"
    );

    // The page: where it leads first, then what breaks, then the tools;
    // each break by its tool and why.
    let at = |heading: &str| {
        text.find(heading)
            .unwrap_or_else(|| panic!("{heading}: {text}"))
    };
    let breaks = at("<h2>What breaks</h2>");
    let leads = at("<h2>Where it leads</h2>");
    let tools = at("<h2>What each tool may do</h2>");
    assert!(leads < breaks && breaks < tools, "{text}");
    let section = &text[breaks..tools];
    for (tool, _) in expected_breaks() {
        assert!(
            section.contains(&format!("<code>{tool}</code>")),
            "{tool}: {section}"
        );
    }
    for why in [
        "the tool is gone: a call to it is refused",
        "the input <code>channel</code> is gone or renamed",
        "the input <code>region</code> must now be given",
        "the input <code>unit</code> was a <code>text</code> and is now a <code>choice</code>",
        "the output <code>customer</code> is gone",
        "the output <code>status</code> was a <code>text</code> and is now a <code>choice</code>",
        "its effect rose from <code>write</code> to <code>irreversible</code>",
        "its hazard was <code>none</code> and is now <code>data_elsewhere</code>",
        "the inputs that name its record were <code>order_id</code>, <code>item_code</code> and are now <code>item_code</code>, <code>order_id</code>",
    ] {
        assert!(section.contains(why), "{why}: {section}");
    }
    assert!(!section.contains("example_order_summary_get"), "{section}");
    // The compatible change is shown with its tool, among the rest.
    let summary = text[tools..]
        .find("<code>example_order_summary_get</code>")
        .unwrap();
    let item = &text[tools + summary..];
    let item = &item[..item.find("</li>").unwrap_or(item.len())];
    assert!(
        item.contains("Changed since version <code>1.0.0</code>, and nothing that breaks"),
        "{item}"
    );
    assert!(
        item.contains("a new input <code>channel</code>, which a call may leave out"),
        "{item}"
    );

    // The person still publishes it.
    press(&link, &text).await;
    assert_eq!(prompt.0.load(Ordering::SeqCst), 2);
    let (code, listed) = run(home_path, project_path, &["tool", "search"]).await;
    assert_eq!(code, Some(0), "{listed}");
    let listed = listed.to_string();
    assert!(!listed.contains("example_order_find"), "{listed}");
    assert!(listed.contains("example_order_status_get"), "{listed}");

    // `diff` classifies the two published versions the same way, and names
    // every tool, the compatible and the unchanged too.
    let (code, diffed) = run(
        home_path,
        project_path,
        &["diff", "example", "--from", "1.0.0", "--to", "2.0.0"],
    )
    .await;
    assert_eq!(code, Some(0), "{diffed}");
    let result = &diffed["result"];
    assert_eq!(result["breaking"], 5, "{diffed}");
    let tools: Vec<(&str, &str, Vec<String>)> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            (
                tool["tool"].as_str().unwrap(),
                tool["class"].as_str().unwrap(),
                tags(tool),
            )
        })
        .collect();
    let mut wanted: Vec<(&str, &str, Vec<String>)> = expected
        .iter()
        .map(|(tool, changes)| (*tool, "breaking", changes.clone()))
        .collect();
    wanted.push((
        "example_order_summary_get",
        "compatible",
        vec!["input_added".to_owned(), "description_changed".to_owned()],
    ));
    assert_eq!(tools, wanted, "{diffed}");

    // Version 3 changes a description alone: no break, on the page or in
    // the answer.
    write_version(project_path, 3);
    let (asked, link, text) = ask(home_path, project_path, "3.0.0").await;
    assert_eq!(asked["result"]["compared_with"], "2.0.0", "{asked}");
    assert_eq!(asked["result"]["breaking"], json!([]), "{asked}");
    assert!(
        !asked["warnings"]
            .to_string()
            .contains("This publish breaks"),
        "{asked}"
    );
    assert!(!text.contains("<h2>What breaks</h2>"), "{text}");
    assert!(text.contains("its description changed"), "{text}");
    press(&link, &text).await;

    let (code, _) = run(home_path, project_path, &["engine", "stop"]).await;
    assert_eq!(code, Some(0));
    assert_eq!(serving.await.unwrap(), StopReason::Asked);
}
