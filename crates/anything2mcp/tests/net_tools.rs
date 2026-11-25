//! The machine's network facts as tools (decisions 0222, 0243, 0260): each
//! answers through the real command, an agent at the MCP door sees
//! `net.standing` listed and reaches the rest through the run-an-action
//! tool, and a proxy's password in the engine's own environment reaches no
//! caller, no log, and no file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use anything2mcp::PASSPHRASE_VARIABLE;
use common::Machine;
use common::served::{Bridge, Served, output, stateless};
use serde_json::{Value as Json, json};

const VERBS: [&str; 7] = [
    "interfaces",
    "routes",
    "resolvers",
    "neighbours",
    "listeners",
    "proxy",
    "standing",
];

/// Whether this system has no reader of the fact yet.
fn not_built_here(verb: &str) -> bool {
    cfg!(target_os = "macos") && matches!(verb, "listeners" | "proxy")
}

/// The scope of every address of every interface of a standing.
fn scopes(standing: &Json) -> Vec<String> {
    standing["interfaces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|interface| interface["addresses"].as_array().unwrap())
        .map(|address| address["scope"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_network_tool_answers_through_the_command() {
    let machine = Machine::new("orders", &[]);
    let world = machine.open().await;
    // A port of this test's own, bound on this machine only, for the
    // listeners to show.
    let bound = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = bound.local_addr().unwrap().port();

    for verb in VERBS {
        let ran = world.agent(&["net", verb]).await;
        if not_built_here(verb) {
            assert_ne!(ran.finished.code, 0, "{verb}");
            assert_eq!(ran.error()["code"], "net_reader_not_built", "{verb}");
            assert_eq!(ran.error()["class"], "unknown_variant", "{verb}");
            continue;
        }
        assert_eq!(ran.finished.code, 0, "{verb}: {:?}", ran.finished);
        let result = ran.result();
        assert!(result.is_object(), "{verb}: {result}");
        // Every call was decided, and the decision is on record as an
        // allow of this action.
        let decision = ran.json["decision"].as_str().unwrap().to_owned();
        let explained = world.ok(&["policy", "explain", &decision]).await;
        assert_eq!(explained["resource"], format!("action:net.{verb}"));
        assert_eq!(explained["answer"], "allow", "{explained}");

        match verb {
            "standing" => {
                assert!(
                    scopes(result).iter().any(|scope| scope == "this_machine"),
                    "{result}"
                );
                assert_eq!(ran.json["next"]["action"], "action.run", "{}", ran.json);
            }
            "interfaces" => {
                assert!(!result["interfaces"].as_array().unwrap().is_empty());
            }
            "listeners" => {
                let ours = result["listeners"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|listener| {
                        listener["port"] == json!(port) && listener["transport"] == "tcp"
                    });
                let ours = ours.unwrap_or_else(|| panic!("port {port} is not listed: {result}"));
                assert_eq!(ours["address"], "127.0.0.1");
                assert_eq!(ours["scope"], "this_machine");
            }
            _ => {}
        }
    }
    drop(bound);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_sees_net_standing_listed_and_reaches_the_rest_through_action_run_at_the_mcp_door()
{
    let machine = Machine::new("orders", &[]).with_passphrase();
    let served = Served::start(&machine);
    let mut bridge = Bridge::start(&machine, &["bridge", "--toolset", "use"]);

    let listed = bridge.ask(&stateless(1, "tools/list", json!({})));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"a2m_net_standing"), "{names:?}");
    // The other six are not listed: they are reached by name.
    assert!(!names.contains(&"a2m_net_interfaces"), "{names:?}");

    let standing = bridge.ask(&stateless(
        2,
        "tools/call",
        json!({"name": "a2m_net_standing", "arguments": {}}),
    ));
    assert_ne!(standing["result"]["isError"], true, "{standing}");
    let content = &standing["result"]["structuredContent"];
    assert!(
        scopes(content).iter().any(|scope| scope == "this_machine"),
        "{standing}"
    );

    let interfaces = bridge.ask(&stateless(
        3,
        "tools/call",
        json!({"name": "a2m_action_run", "arguments": {"action": "net.interfaces"}}),
    ));
    assert_ne!(interfaces["result"]["isError"], true, "{interfaces}");
    let content = &interfaces["result"]["structuredContent"];
    assert!(
        !content["interfaces"].as_array().unwrap().is_empty(),
        "{interfaces}"
    );

    // An MCP result carries no decision's identifier; that each call at
    // this door is decided is proven at the kernel, with an agent at the
    // MCP door as the caller (`a2m-actions`, `tests/net.rs`), and through
    // the command above.
    let without_key = |args: &[&str]| {
        let mut command = machine.program(args);
        command.env_remove(PASSPHRASE_VARIABLE);
        command
    };
    let (code, _, said) = bridge.close();
    assert_eq!(code, Some(0), "{said}");
    let stop = output(without_key(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    assert_eq!(served.ended().0, Some(0));
}

/// A proxy's user name and password, and parts of a password that holds
/// the characters a cleaner might cut at first.
#[cfg(target_os = "linux")]
const PROXY: &str = "http://someone:pa/ss?wo#rd@proxy.example:3128";

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proxy_password_in_the_engines_environment_reaches_no_caller() {
    use common::served::{object, texts_under};

    let machine = Machine::new("orders", &[]).with_passphrase();
    // The engine's own environment holds the proxy. On Linux the reader
    // reads the engine's environment, never the caller's.
    let served = Served::start_with(&machine, &[("http_proxy", PROXY)]);
    let without_key = |args: &[&str]| {
        let mut command = machine.program(args);
        command
            .env_remove(PASSPHRASE_VARIABLE)
            .env("http_proxy", "http://proxy-of-the-caller.example:1");
        command
    };

    let read = output(without_key(&["net", "proxy"])).await;
    assert_eq!(read.status.code(), Some(0), "{read:?}");
    let response = object(&read);
    let settings = response["result"]["settings"].as_array().unwrap();
    let setting = settings
        .iter()
        .find(|setting| setting["from"] == "http_proxy")
        .unwrap_or_else(|| panic!("{response}"));
    assert_eq!(setting["value"], "http://proxy.example:3128", "{response}");
    assert_eq!(setting["credentials_removed"], true);
    assert!(
        !response.to_string().contains("proxy-of-the-caller"),
        "{response}"
    );

    let logs = output(without_key(&["engine", "logs"])).await;
    let stop = output(without_key(&["engine", "stop"])).await;
    assert_eq!(stop.status.code(), Some(0), "{stop:?}");
    let (code, printed, said) = served.ended_saying();
    assert_eq!(code, Some(0), "{said:?}");

    let mut seen = vec![
        (
            "the command".to_owned(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&read.stdout),
                String::from_utf8_lossy(&read.stderr)
            ),
        ),
        (
            "the engine's log".to_owned(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&logs.stdout),
                String::from_utf8_lossy(&logs.stderr)
            ),
        ),
        ("what the engine printed".to_owned(), printed),
        ("what the engine said".to_owned(), said.join("\n")),
    ];
    let mut files = Vec::new();
    texts_under(machine.home.path(), &mut files);
    texts_under(machine.project.path(), &mut files);
    assert!(!files.is_empty());
    seen.extend(
        files
            .into_iter()
            .map(|(path, text)| (path.display().to_string(), text)),
    );
    for (place, text) in &seen {
        for part in ["someone", "pa/ss", "wo#rd"] {
            assert!(!text.contains(part), "{part} reached {place}");
        }
    }
}
