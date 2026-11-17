//! Anything, proven once (milestone 4), and then once for each other pack:
//! the same flow as the first read, through another pack and another kind
//! of system. Target files in a project folder, lint, publish, and one
//! value read from the pack's test stand, which is asked what it holds.

mod common;

use std::path::Path;

use common::{Folder, Machine, World};
use example_broker::harness::TestBroker;
use example_cli::harness::Stand;
use example_db::harness::TestDb;
use example_db::schema::Table;
use example_device::TestDevice;
use example_files::harness::FileStand;
use example_mcp::harness::HttpStand;
use serde_json::Value as Json;

/// A path as a YAML scalar in single quotes.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

async fn published(machine: &Machine) -> World {
    let world = machine.open().await;
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world
}

/// The trace signature of a run, and its event kinds.
async fn signature(world: &World, run: &str) -> String {
    let trace = world.ok(&["trace", "show", run]).await;
    assert!(trace["trace"].as_str().unwrap().starts_with("blob:"));
    trace["signature"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_line_program_is_read_through_the_command_pack() {
    let stand = Stand::start().unwrap();
    let digest = a2m_core::Digest::of(&std::fs::read(stand.program()).unwrap());
    let platform = a2m_provider::current_platform();
    let machine = Machine::new(
        "inventory",
        &[
            ("'/opt/example/bin/example-cli'", &quoted(stand.program())),
            ("'/var/lib/example/home'", &quoted(stand.home())),
            (
                "linux-x86_64: [\"sha256:1111111111111111111111111111111111111111111111111111111111111111\"]",
                &format!("{platform}: [\"sha256:{digest}\"]"),
            ),
        ],
    )
    .on_disk();
    let world = published(&machine).await;

    let held = stand.control().stored_item("SK-1001").unwrap().unwrap();
    let before = stand.control().invocations().unwrap().len();
    let mut signatures = Vec::new();
    for _ in 0..2 {
        let ran = world
            .agent(&["tool", "call", "example_item_get", "--input", r#"{"sku": "SK-1001"}"#])
            .await;
        assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
        let result = ran.result();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["mode"], "cli");
        // The value is what the program's own store holds.
        assert_eq!(result["result"]["name"], held.name.as_str());
        assert_eq!(result["result"]["quantity"], held.quantity);
        assert_eq!(result["result"]["location"], held.location.as_str());
        signatures.push(signature(&world, result["run_id"].as_str().unwrap()).await);
    }
    assert_eq!(signatures[0], signatures[1]);
    // The program was started, and only to read.
    let starts = stand.control().invocations().unwrap();
    assert!(starts.len() > before, "{starts:?}");
    assert_eq!(stand.control().stored_item("SK-1001").unwrap().unwrap(), held);
    world.close().await;
}

/// Calls a read tool twice, and gives its result once both runs completed
/// through `mode` with one trace signature.
async fn read_twice(world: &World, tool: &str, input: &str, mode: &str) -> Json {
    let mut signatures = Vec::new();
    let mut results = Vec::new();
    for _ in 0..2 {
        let ran = world.agent(&["tool", "call", tool, "--input", input]).await;
        assert_eq!(ran.finished.code, 0, "{:?}", ran.finished);
        let result = ran.result();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["mode"], mode);
        signatures.push(signature(world, result["run_id"].as_str().unwrap()).await);
        results.push(result["result"].clone());
    }
    assert_eq!(signatures[0], signatures[1]);
    assert_eq!(results[0], results[1]);
    results.remove(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_is_read_through_the_device_pack() {
    let device = TestDevice::start().await.unwrap();
    let address = device.address().to_string();
    let machine = Machine::new(
        "controller",
        &[("127.0.0.1:502", &address), ("127.0.0.1:1502", &address)],
    );
    let world = published(&machine).await;
    let read = read_twice(&world, "example_pump_speed_get", "{}", "device").await;
    // The value is what the device's own registers hold: a tenth of the
    // raw word, and the bit.
    let raw = device.control().number("pump_speed_measured").unwrap();
    let running = device.control().bit("pump_running").unwrap();
    // The speed is an exact decimal: a result carries one as its text.
    let speed = read["speed"]["decimal"]
        .as_str()
        .unwrap_or_else(|| panic!("the speed is not a decimal: {read}"));
    let (whole, part) = speed.split_once('.').unwrap_or((speed, "0"));
    let tenths = whole.parse::<i64>().unwrap() * 10 + part[..1].parse::<i64>().unwrap();
    assert_eq!(tenths, raw, "{read}");
    assert_eq!(read["running"], running, "{read}");
    // Nothing was written to the device.
    assert_eq!(device.control().write_count("pump_speed_setpoint"), 0);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_broker_is_read_through_the_stream_pack() {
    let broker = TestBroker::start().await.unwrap();
    let machine = Machine::new(
        "messages",
        &[("broker.example.test:7001", &broker.address().to_string())],
    );
    let world = published(&machine).await;
    let read = read_twice(
        &world,
        "example_order_status_get",
        r#"{"order_id": "A-100"}"#,
        "stream",
    )
    .await;
    // The value is the one the broker retains for the key, and reading it
    // took nothing from the channel.
    let held = broker.retained("example.orders.status", "A-100").unwrap();
    assert_eq!(read["status"], held.body["status"], "{read}");
    assert!(broker.retained("example.orders.status", "A-100").is_some());
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mcp_server_is_read_through_the_mcp_pack() {
    let stand = HttpStand::start().await.unwrap();
    let machine = Machine::new("upstream", &[("http://127.0.0.1:9/mcp", &stand.url())]);
    let world = published(&machine).await;
    let read = read_twice(&world, "example_order_get", r#"{"order_id": "A-100"}"#, "mcp").await;
    // The value is what the server's own store holds, and the one tool
    // that was called is the one the mapping pins.
    let held = stand.control().stored_order("A-100").unwrap().unwrap();
    assert_eq!(
        read["status"],
        serde_json::to_value(held.status).unwrap(),
        "{read}"
    );
    assert_eq!(read["note"], held.note.as_str(), "{read}");
    assert_eq!(stand.control().calls_to("orders_get").unwrap(), 2);
    assert_eq!(stand.control().write_count("A-100").unwrap(), 0);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_is_read_through_the_file_pack() {
    // The project folder is the stand's own root: the root `files` of the
    // product is the project folder, and the stand keeps its files there.
    let stand = FileStand::start().unwrap();
    let machine = Machine::in_folder("stockfiles", &[], Folder::of_another(stand.root()));
    let world = published(&machine).await;
    let rows = stand.stored_rows("prices/stock.csv", ',').unwrap();
    let row = rows[1].clone();
    let input = format!(r#"{{"item_code": "{}"}}"#, row[0]);
    let before = stand.sha256("prices/stock.csv").unwrap();
    let read = read_twice(&world, "example_stock_get", &input, "file").await;
    // The value is the row the stand's file holds, and the file is as it
    // was, byte for byte.
    assert_eq!(read["description"], row[1].as_str(), "{read}");
    assert_eq!(read["on_hand"].to_string(), row[2], "{read}");
    assert_eq!(stand.sha256("prices/stock.csv").unwrap(), before);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_data_store_is_read_through_the_data_pack() {
    // The project folder is the folder the stand keeps its store in: the
    // root `project` of the product is the project folder, and the target
    // names the store's file below it.
    let stand = TestDb::start().unwrap();
    let folder = Folder::of_another(stand.path().parent().unwrap());
    let file = stand.path().file_name().unwrap().to_str().unwrap().to_owned();
    let machine = Machine::in_folder("storefile", &[("example.db", &file)], folder);
    let world = published(&machine).await;
    let writes = stand.writes().unwrap().len();
    let read = read_twice(
        &world,
        "example_customer_get",
        r#"{"customer_id": "C-002"}"#,
        "sql",
    )
    .await;
    // The value is the row the store holds, and nothing was written.
    let held = stand
        .stored_row(Table::Customers, &["C-002"])
        .unwrap()
        .unwrap();
    assert_eq!(read["name"], held.text("name").unwrap(), "{read}");
    assert_eq!(
        read["credit_limit_cents"],
        held.integer("credit_limit_cents").unwrap(),
        "{read}"
    );
    assert_eq!(read["notes"], held.text("notes").unwrap(), "{read}");
    assert_eq!(stand.writes().unwrap().len(), writes);
    world.close().await;
}
