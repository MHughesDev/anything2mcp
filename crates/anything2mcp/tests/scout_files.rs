//! A person hands the scout files of the project (decision 0263): a file
//! that describes a system, kept in the project's `sources` folder and
//! named under a name a discoverer reads, through the command face.
//!
//! The test application serves its description at no place the HTTP pack
//! looks by convention (a fault answers every request for it with 404), so
//! what the scout knows of the described operations comes from the file
//! alone. The device stand publishes no map; the map is a file. A
//! PowerShell help file describes commands that the command-line stand's
//! program does not know; they are read from the file and nothing is
//! started for them. Every path
//! that leads out of the `sources` folder, every hidden file, and every
//! name no discoverer reads is refused before anything starts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

use crate::common;

use std::path::Path;

use common::{Folder, Machine, World};
use example_app::faults::{Fault, FaultKind};
use example_app::harness::TestApp;
use example_cli::harness::Stand;
use example_device::TestDevice;
use serde_json::{Value as Json, json};

/// Where the test application serves its description.
const DESCRIPTION_PATH: &str = "/api/openapi.json";

/// The file the test writes from the stand's description.
const DESCRIPTION_FILE: &str = "sources/example-openapi.json";

/// The files a scout of the test application drafts for one read: the
/// pinned description that states its request, and its operation
/// (decision 0389).
const DRAFTED: [&str; 2] = ["descriptions/http.yaml", "operations/api_orders_get.yaml"];

/// A register map in the layout the device pack reads, as a vendor of the
/// imaginary controller publishes one. Synthetic.
const MAP: &str = "# model: example-controller
# firmware: 2.4.0
name,table,offset,data,words,word_order,scale,unit,access
Temperature,input,31,i16,,,0.1,°C,read
Uptime ticks,input,32,u32,,high_first,,,read
";

/// The `files` argument naming one file.
fn files(name: &str, path: &str) -> String {
    json!({ name: path }).to_string()
}

/// The test application, with its description written to the project's
/// `sources` folder and then served nowhere: every request for it is
/// answered 404.
async fn described_elsewhere(project: &Path) -> TestApp {
    let app = TestApp::start().await.unwrap();
    let description = app
        .http()
        .get(app.url(DESCRIPTION_PATH))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(description.contains("\"openapi\""), "{description}");
    std::fs::create_dir_all(project.join("sources")).unwrap();
    std::fs::write(project.join(DESCRIPTION_FILE), description).unwrap();
    app.set_fault(&Fault {
        kind: FaultKind::Status { status: 404 },
        path_prefix: Some(DESCRIPTION_PATH.to_owned()),
        times: None,
    })
    .await
    .unwrap();
    let answer = app
        .http()
        .get(app.url(DESCRIPTION_PATH))
        .send()
        .await
        .unwrap();
    assert_eq!(answer.status().as_u16(), 404);
    app
}

/// An engine over a new project that holds the target `example`, added
/// with the test application's address.
async fn with_target(machine: &Machine, app: &TestApp) -> World {
    let world = machine.open().await;
    world
        .ok(&[
            "target",
            "add",
            "example",
            "--address",
            &app.url(""),
            "--kind",
            "http",
        ])
        .await;
    world
}

/// The finding counts of a scout's one surface, by kind and level.
fn found_by_kind(report: &Json) -> Json {
    report["content"]["surfaces"][0]["findings_by_kind"].clone()
}

/// The files a scout's report says it drafted.
fn drafted(report: &Json) -> Vec<String> {
    report["content"]["drafted"]["written"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| file["file"].as_str().map(str::to_owned))
        .collect()
}

/// The runs the caller has started.
async fn runs(world: &World) -> usize {
    world.ok(&["run", "list", "--limit", "100"]).await["runs"]
        .as_array()
        .unwrap()
        .len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_description_given_as_a_file_is_imported_by_the_scout_through_the_command() {
    let machine = Machine::new("described", &[]);
    let app = described_elsewhere(machine.project.path()).await;
    let world = with_target(&machine, &app).await;

    // Without the file, nothing is described: the surface serves no
    // description where one is looked for, and the reader says how to hand
    // one.
    let looked = world.ok(&["discover", "run", "example"]).await;
    assert!(
        looked["by_kind"].get("described_operation").is_none(),
        "{looked}"
    );
    // With the file, under the name the HTTP pack's reader declares, the
    // same discovery reads the operations it describes.
    let read = world
        .ok(&[
            "discover",
            "run",
            "example",
            "--files",
            &files("description", DESCRIPTION_FILE),
        ])
        .await;
    assert_eq!(read["by_kind"]["described_operation"], 11, "{read}");

    // The scout, handed the file, drafts from what it describes.
    let scouted = world
        .ok(&[
            "scout",
            "run",
            "example",
            "--fresh",
            "--files",
            &files("description", DESCRIPTION_FILE),
        ])
        .await;
    assert!(scouted["report_hash"].is_string(), "{scouted}");
    assert_eq!(
        found_by_kind(&scouted),
        // The description's two declared signs of a sign-in are found with
        // its operations (decision 0367).
        json!({
            "described_operation": {"inferred": 11},
            "endpoint": {"discovered": 2},
            "sign_in_method": {"inferred": 2}
        }),
        "{scouted}"
    );
    let written = drafted(&scouted);
    for file in DRAFTED {
        assert!(
            written.iter().any(|one| one == file),
            "{file} in {written:?}"
        );
        assert!(
            machine
                .project
                .path()
                .join("targets")
                .join("example")
                .join(file)
                .is_file(),
            "{file}"
        );
    }
    // The report says which file the scout was handed, under which name.
    assert_eq!(
        scouted["content"]["files"],
        json!({"description": DESCRIPTION_FILE}),
        "{scouted}"
    );
    // Nothing was written to the system.
    assert!(app.writes().await.unwrap().is_empty());
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_register_map_given_as_a_file_is_read_by_the_device_scout() {
    let device = TestDevice::start().await.unwrap();
    let address = device.address().to_string();
    let machine = Machine::new(
        "controller",
        &[("127.0.0.1:502", &address), ("127.0.0.1:1502", &address)],
    );
    let sources = machine.project.path().join("sources");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(sources.join("example-map.csv"), MAP).unwrap();
    let world = machine.open().await;
    let scouted = world
        .ok(&[
            "scout",
            "run",
            "example",
            "--wait-seconds",
            "60",
            "--files",
            &files("register_map", "sources/example-map.csv"),
        ])
        .await;
    assert!(scouted["report_hash"].is_string(), "{scouted}");
    assert_eq!(
        found_by_kind(&scouted)["device_register_map"],
        json!({"inferred": 1}),
        "{scouted}"
    );
    assert_eq!(
        scouted["content"]["files"],
        json!({"register_map": "sources/example-map.csv"}),
        "{scouted}"
    );
    // Nothing was written to the device.
    assert_eq!(device.control().write_count("pump_speed_setpoint"), 0);
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spec_import_drafts_from_a_file_as_the_scout_does() {
    let machine = Machine::new("described", &[]);
    let app = described_elsewhere(machine.project.path()).await;
    let world = with_target(&machine, &app).await;
    let imported = world
        .ok(&["spec", "import", "example", "--file", DESCRIPTION_FILE])
        .await;
    assert!(imported["report_hash"].is_string(), "{imported}");
    assert_eq!(
        imported["content"]["files"],
        json!({"description": DESCRIPTION_FILE}),
        "{imported}"
    );
    assert_eq!(
        found_by_kind(&imported)["described_operation"],
        json!({"inferred": 11}),
        "{imported}"
    );
    let written = drafted(&imported);
    for file in DRAFTED {
        assert!(
            written.iter().any(|one| one == file),
            "{file} in {written:?}"
        );
    }
    // It is a scout: its report is the scout's last.
    let report = world.ok(&["scout", "report", "example"]).await;
    assert_eq!(report["scout_id"], imported["scout_id"], "{report}");
    world.close().await;
}

/// How many operations the large description gains.
const MANY: usize = 400;

/// How many of the reads added state the shape of their answer, so that
/// their drafts are sound and stay in place.
const SOUND: usize = 12;

/// The test application's description with `MANY` more described reads
/// added, written to the project's `sources` folder over the first.
fn enlarge_description(project: &Path) {
    let path = project.join(DESCRIPTION_FILE);
    let mut described: Json =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for n in 0..MANY {
        described["paths"][format!("/api/widgets{n}/{{order_id}}")] = json!({"get": {
            "operationId": format!("get_widget_{n}"),
            "parameters": [{"name": "order_id", "in": "path", "required": true,
                "schema": {"type": "string"}, "example": "A-100"}],
            "responses": {"200": {"description": format!("Widget number {n} of the order")}},
        }});
    }
    for n in 0..SOUND {
        described["paths"][format!("/api/gizmos{n}/{{order_id}}")] = json!({"get": {
            "operationId": format!("get_gizmo_{n}"),
            "parameters": [{"name": "order_id", "in": "path", "required": true,
                "schema": {"type": "string"}, "example": "A-100"}],
            "responses": {"200": {"description": "A gizmo", "content": {"application/json": {
                "schema": {"type": "object", "properties": {"label": {"type": "string"}}}}}}},
        }});
    }
    std::fs::write(&path, described.to_string()).unwrap();
}

/// A count in a result, as a number of entries.
fn count(value: &Json) -> usize {
    usize::try_from(value.as_u64().unwrap()).unwrap()
}

/// Every entry of one list of the last scout's report, read in pages.
async fn paged(world: &World, part: &str, page: usize) -> Vec<Json> {
    let mut all = Vec::new();
    loop {
        let read = world
            .ok(&[
                "scout",
                "report",
                "example",
                "--part",
                part,
                "--limit",
                &page.to_string(),
                "--offset",
                &all.len().to_string(),
            ])
            .await;
        assert_eq!(read["part"], part, "{read}");
        let entries = read["entries"].as_array().unwrap();
        all.extend(entries.iter().cloned());
        if read["complete"] == true {
            assert_eq!(count(&read["total"]), all.len(), "{part}");
            return all;
        }
        assert!(!entries.is_empty(), "{read}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scout_of_a_large_description_answers_within_the_cap_and_its_lists_are_paged() {
    let machine = Machine::new("described", &[]);
    let app = described_elsewhere(machine.project.path()).await;
    enlarge_description(machine.project.path());
    let world = with_target(&machine, &app).await;
    let cap: usize = 262_144;
    let described = files("description", DESCRIPTION_FILE);
    let args = ["scout", "run", "example", "--fresh", "--files", &described];
    let scouted = world.ok(&args).await;
    assert!(scouted.get("truncated").is_none(), "{scouted}");
    let size = scouted.to_string().len();
    assert!(size < cap / 2, "{size}");
    assert!(scouted["report_hash"].is_string(), "{scouted}");
    assert!(scouted["scout_id"].is_string(), "{scouted}");
    assert!(scouted["runs"].is_object(), "{scouted}");
    assert!(scouted["text"].as_str().unwrap().len() < 4_000, "{scouted}");

    // No list of the answer, however deep, is longer than the fixed number.
    fn longest(value: &Json) -> usize {
        match value {
            Json::Array(items) => items
                .len()
                .max(items.iter().map(longest).max().unwrap_or(0)),
            Json::Object(map) => map.values().map(longest).max().unwrap_or(0),
            _ => 0,
        }
    }
    assert!(
        longest(&scouted["content"]) <= 20,
        "{}",
        longest(&scouted["content"])
    );

    // The counts are the whole report's.
    let counts = &scouted["counts"];
    // The operations, the two endpoints the live look found, and the two
    // signs of a sign-in the description declares (decision 0367).
    assert_eq!(
        count(&counts["found"]),
        MANY + SOUND + 11 + 2 + 2,
        "{counts}"
    );
    assert!(written_in(&scouted).len() <= 20, "{scouted}");
    assert!(count(&counts["drafted.written"]) > 20, "{counts}");
    // Reads whose answer the description gives no shape of are held with
    // their endpoints (decision 0298); the held list is long too.
    assert!(count(&counts["drafted.held_back"]) > 20, "{counts}");
    assert!(
        scouted["cut_lists"]
            .as_array()
            .unwrap()
            .iter()
            .any(|list| list == "found")
    );
    // What the system said stays marked in every entry shown.
    for found in scouted["content"]["found"].as_array().unwrap() {
        assert!(found["found"]["untrusted"].is_string(), "{found}");
    }

    // The report read again is the same answer, and each list read in
    // pages gives every entry once, in a stable order.
    let again = world.ok(&["scout", "report", "example"]).await;
    assert_eq!(again["report_hash"], scouted["report_hash"]);
    assert_eq!(again["counts"], scouted["counts"]);
    for (part, total) in counts.as_object().unwrap() {
        let whole = paged(&world, part, 37).await;
        assert_eq!(whole.len(), count(total), "{part}");
        assert_eq!(whole, paged(&world, part, 100).await, "{part}");
        // The answer's entries are the first of them.
        let first = part
            .split('.')
            .fold(&scouted["content"], |at, key| &at[key])
            .as_array()
            .unwrap();
        assert_eq!(first.as_slice(), &whole[..first.len()], "{part}");
        if part == "found" || part == "drafted.written" {
            let unique: std::collections::BTreeSet<String> =
                whole.iter().map(Json::to_string).collect();
            assert_eq!(unique.len(), whole.len(), "{part}");
        }
    }

    // A second scout of the same target gives the same report.
    let second = world.ok(&args).await;
    assert_ne!(second["scout_id"], scouted["scout_id"]);
    assert_eq!(second["report_hash"], scouted["report_hash"]);
    world.close().await;
}

/// The files the answer shows as drafted.
fn written_in(report: &Json) -> Vec<String> {
    drafted(report)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_the_report_does_not_have_is_refused_as_input() {
    let machine = Machine::new("described", &[]);
    let app = described_elsewhere(machine.project.path()).await;
    let world = with_target(&machine, &app).await;
    world.ok(&["scout", "run", "example"]).await;
    let ran = world
        .agent(&[
            "scout",
            "report",
            "example",
            "--part",
            "nothing_of_the_kind",
        ])
        .await;
    assert_ne!(ran.finished.code, 0, "{}", ran.json);
    assert_eq!(ran.error()["class"], "input", "{}", ran.json);
    assert_eq!(ran.error()["code"], "schema_invalid", "{}", ran.json);
    world.close().await;
}

/// Makes a link at `link` to `to`, or says why it cannot.
fn linked(to: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(to, link)
    }
    #[cfg(windows)]
    {
        // Only a user who may make links can; the case is skipped for one
        // who may not.
        std::os::windows::fs::symlink_file(to, link)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_outside_the_sources_folder_a_hidden_file_or_a_link_out_is_refused_and_no_run_starts()
 {
    let machine = Machine::new("described", &[]);
    let app = described_elsewhere(machine.project.path()).await;
    let world = with_target(&machine, &app).await;
    let project = machine.project.path();
    let sources = project.join("sources");
    std::fs::write(sources.join(".env"), "made-up=1\n").unwrap();
    let outside = Folder::new("outside");
    let elsewhere = outside.path().join("example-openapi.json");
    std::fs::write(&elsewhere, "{}").unwrap();
    let absolute = elsewhere.display().to_string();
    let mut refused = vec![
        "../x".to_owned(),
        absolute,
        "targets/example/target.yaml".to_owned(),
        "sources/.env".to_owned(),
        "sources/../targets/example/target.yaml".to_owned(),
        "sources".to_owned(),
    ];
    match linked(&elsewhere, &sources.join("out")) {
        Ok(()) => refused.push("sources/out".to_owned()),
        Err(why) => println!("no link out was made, so that case is not tried: {why}"),
    }
    let inside = project.join("targets").join("example").join("target.yaml");
    match linked(&inside, &sources.join("in")) {
        Ok(()) => refused.push("sources/in".to_owned()),
        Err(why) => println!("no link was made, so that case is not tried: {why}"),
    }
    for path in &refused {
        for action in [["scout", "run"], ["discover", "run"]] {
            let ran = world
                .agent(&[
                    action[0],
                    action[1],
                    "example",
                    "--files",
                    &files("description", path),
                ])
                .await;
            assert_ne!(ran.finished.code, 0, "{path}: {}", ran.json);
            assert_eq!(ran.error()["class"], "input", "{path}: {}", ran.json);
            assert_eq!(
                ran.error()["code"],
                "path_not_allowed",
                "{path}: {}",
                ran.json
            );
        }
        let ran = world
            .agent(&["spec", "import", "example", "--file", path])
            .await;
        assert_eq!(
            ran.error()["code"],
            "path_not_allowed",
            "{path}: {}",
            ran.json
        );
    }
    // A file that is not there is refused by name too.
    let ran = world
        .agent(&[
            "scout",
            "run",
            "example",
            "--files",
            &files("description", "sources/absent.json"),
        ])
        .await;
    assert_eq!(ran.error()["code"], "file_unavailable", "{}", ran.json);
    // No run started, and no scout: the first scout made now is a new one,
    // handed nothing.
    assert_eq!(runs(&world).await, 0);
    let refused_report = world.agent(&["scout", "report", "example"]).await;
    assert_eq!(
        refused_report.error()["code"],
        "not_found",
        "{}",
        refused_report.json
    );
    world.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_no_discoverer_takes_is_refused_with_the_names_taken() {
    let machine = Machine::new("described", &[]);
    let app = described_elsewhere(machine.project.path()).await;
    let world = with_target(&machine, &app).await;
    for action in [["scout", "run"], ["discover", "run"]] {
        let ran = world
            .agent(&[
                action[0],
                action[1],
                "example",
                "--files",
                &files("register_map", DESCRIPTION_FILE),
            ])
            .await;
        assert_eq!(ran.error()["class"], "input", "{}", ran.json);
        assert_eq!(
            ran.error()["code"],
            "discovery_file_not_taken",
            "{}",
            ran.json
        );
        let fix = ran.error()["fix"].as_str().unwrap();
        assert!(fix.contains("description"), "{fix}");
    }
    assert_eq!(runs(&world).await, 0);
    world.close().await;
}

/// The synthetic help file of the imaginary module `Example.Inventory`,
/// which the command pack's own tests read.
fn powershell_help() -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("a2m-pack-command")
            .join("tests")
            .join("fixtures")
            .join("sources")
            .join("Example.Inventory-help.xml"),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_powershell_help_file_given_as_a_file_drafts_its_commands_and_starts_nothing_of_it() {
    let stand = Stand::start().unwrap();
    let digest = a2m_core::Digest::of(&std::fs::read(stand.program()).unwrap());
    let platform = a2m_provider::current_platform();
    let quoted = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "''"));
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
    let sources = machine.project.path().join("sources");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(
        sources.join("Example.Inventory-help.xml"),
        powershell_help(),
    )
    .unwrap();
    let handed = files("powershell_help", "sources/Example.Inventory-help.xml");
    let world = machine.open().await;

    let read = world
        .ok(&["discover", "run", "example", "--files", &handed])
        .await;
    assert_eq!(read["by_kind"]["powershell_command"], 2, "{read}");
    assert_eq!(read["by_kind"]["command_file"], 1, "{read}");

    let scouted = world
        .ok(&["scout", "run", "example", "--fresh", "--files", &handed])
        .await;
    assert_eq!(
        found_by_kind(&scouted)["powershell_command"],
        json!({"inferred": 2}),
        "{scouted}"
    );
    // Each command's draft holds a question (the script only a person
    // names), so it waits in the holding folder (decision 0298).
    let held: Vec<&str> = scouted["content"]["drafted"]["held_back"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Json::as_str)
        .collect();
    for file in [
        "held/commands/get_exampleitem.yaml",
        "held/commands/set_exampleitem.yaml",
    ] {
        assert!(held.contains(&file), "{file} in {held:?}");
        let draft = std::fs::read_to_string(
            machine
                .project
                .path()
                .join("targets")
                .join("example")
                .join(file),
        )
        .unwrap();
        // A fixed pair of Windows PowerShell in its one form, whose script
        // only a person names.
        assert!(draft.contains("\"-File\""), "{draft}");
        assert!(draft.contains("script: {open_question:"), "{draft}");
    }
    // Only the program's declared help and version forms were started;
    // nothing of the help file was, and nothing was written.
    let starts = stand.control().invocations().unwrap();
    for start in &starts {
        assert!(
            matches!(start.args[0].as_str(), "--version" | "--help" | "help"),
            "{:?}",
            start.args
        );
    }
    assert!(stand.control().writes().unwrap().is_empty());
    world.close().await;
}

/// Decision 0298: a description gives one sound read and one whose answer
/// has a value the pack cannot type. The scout leaves the first in place and
/// holds the second in `held/`, its pinned description staying in place; the target compiles
/// whole, the sound read is tried with no file deleted by hand, and the held
/// one is listed by lint and refused by name when tried.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_with_an_open_question_is_held_and_the_sound_read_is_tried_with_nothing_deleted() {
    // This project's reads are not tried by default, so the test asks.
    let machine = Machine::new("described", &[("try_reads = false", "try_reads = true")]);
    let app = described_elsewhere(machine.project.path()).await;
    let path = machine.project.path().join(DESCRIPTION_FILE);
    let mut described: Json =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    // The answer's one value states no type this pack can follow.
    described["paths"]["/api/mystery/{order_id}"] = json!({"get": {
        "operationId": "get_mystery",
        "parameters": [{"name": "order_id", "in": "path", "required": true,
            "schema": {"type": "string"}, "example": "A-100"}],
        "responses": {"200": {"description": "The mystery", "content": {"application/json": {
            "schema": {"type": "object", "properties": {"riddle": {}}}}}}},
    }});
    std::fs::write(&path, described.to_string()).unwrap();
    let world = with_target(&machine, &app).await;
    let scouted = world
        .ok(&[
            "scout",
            "run",
            "example",
            "--fresh",
            "--wait-seconds",
            "60",
            "--files",
            &files("description", DESCRIPTION_FILE),
        ])
        .await;
    assert!(scouted["report_hash"].is_string(), "{scouted}");
    let target = machine.project.path().join("targets").join("example");

    // The sound read is where it always was.
    let written = drafted(&scouted);
    for file in DRAFTED {
        assert!(
            written.iter().any(|one| one == file),
            "{file} in {written:?}"
        );
        assert!(target.join(file).is_file(), "{file}");
    }
    // The other read is held. The pinned description that states its
    // request holds no question and names nothing the read defines (the
    // read names the description, not the other way), so it stays in
    // place (decision 0389).
    let held: Vec<String> = scouted["content"]["drafted"]["held_back"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(held.len(), 1, "{scouted}");
    assert!(held[0].starts_with("held/operations/"), "{held:?}");
    let endpoint = written
        .iter()
        .find(|one| one.as_str() == "descriptions/http.yaml")
        .unwrap_or_else(|| panic!("the pinned description in {written:?}"));
    assert!(target.join(endpoint).is_file(), "{endpoint}");
    assert!(
        std::fs::read_to_string(target.join(endpoint))
            .unwrap()
            .contains("operation_id: \"get_mystery\""),
        "{endpoint}"
    );
    assert!(!target.join("held").join(endpoint).exists(), "{endpoint}");
    for file in &held {
        assert!(target.join(file).is_file(), "{file}");
        assert!(!written.contains(file), "{file}");
        let outside = file.strip_prefix("held/").unwrap();
        assert!(!target.join(outside).exists(), "{outside}");
    }
    assert_eq!(
        scouted["content"]["tried"]["left_out"],
        json!(held),
        "{scouted}"
    );

    // The target compiles whole, and the sound read is tried.
    assert_eq!(scouted["content"]["checked"]["compiles"], true, "{scouted}");
    let tools = scouted["content"]["tried"]["tools"].as_array().unwrap();
    assert!(
        tools
            .iter()
            .any(|tool| tool["file"] == "operations/api_orders_get.yaml"
                && tool["state"] == "proven"),
        "{tools:?}"
    );

    // Lint is ok and lists the held drafts with the question.
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    let listed = linted["held"].as_array().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|file| file["file"].clone())
            .collect::<Vec<_>>(),
        held.iter().map(|file| json!(file)).collect::<Vec<_>>(),
        "{linted}"
    );
    let question = &listed[0]["questions"][0]["question"]["untrusted"];
    assert!(question.as_str().unwrap().contains("riddle"), "{linted}");

    // A trial of the held read is refused, naming the file and the question.
    let held_text = std::fs::read_to_string(target.join(&held[0])).unwrap();
    let name = held_text
        .lines()
        .find_map(|line| line.strip_prefix("operation: \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap()
        .to_owned();
    let refused = world
        .agent(&["draft", "try", "example", "--tool", &name])
        .await;
    assert_eq!(refused.error()["code"], "not_found", "{:?}", refused.json);
    let message = refused.error()["message"].as_str().unwrap();
    // The message names the held file; the question is in `said`, not here.
    assert!(
        message.contains(&held[0]) && !message.contains("riddle"),
        "{message}"
    );

    // The sound read goes on to a publish, with the held drafts beside it:
    // the other drafts are removed as the gates say, its test is recorded,
    // its binding promoted, and a person publishes (the stand-in for the
    // presence prompt is this folder's `Machine`, which `first_hour.rs`
    // cannot lend: its `Passed` prompt and engine are private to that file).
    let untested = world.agent(&["test", "example"]).await;
    assert_eq!(untested.error()["code"], "publish_gates_not_passed");
    for entry in untested.result()["missing"].as_array().unwrap() {
        let file = entry["file"].as_str().unwrap();
        if file == "operations/api_orders_get.yaml" {
            continue;
        }
        world
            .ok(&["draft", "delete", "example", "--file", file])
            .await;
    }
    let read = "example_api_orders_get";
    let input = r#"{"order_id": "A-100"}"#;
    let recorded = world
        .ok(&[
            "fixture", "record", "example", "--tool", read, "--input", input,
        ])
        .await;
    assert_eq!(recorded["test_state"], "passed", "{recorded}");
    let promoted = world
        .ok(&[
            "binding", "promote", "example", "--tool", read, "--mode", "api",
        ])
        .await;
    assert_eq!(promoted["promoted"], true, "{promoted}");
    let linted = world.ok(&["draft", "lint", "example"]).await;
    assert_eq!(linted["ok"], true, "{linted}");
    assert_eq!(linted["held"].as_array().unwrap().len(), 1, "{linted}");
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    assert_eq!(
        published.result()["tools"],
        json!([read]),
        "{}",
        published.json
    );
    let called = world.ok(&["call", read, "--input", input]).await;
    assert_eq!(called["status"], "completed", "{called}");
    assert!(app.writes().await.unwrap().is_empty());
    // The held drafts were not published and are still held.
    assert!(target.join(&held[0]).is_file());
    world.close().await;
}
