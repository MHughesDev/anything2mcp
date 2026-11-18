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

mod common;

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

/// The files a scout of the test application drafts for one read: its
/// endpoint and its operation.
const DRAFTED: [&str; 2] = ["endpoints/get_order.yaml", "operations/api_orders_get.yaml"];

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
    assert_eq!(read["by_kind"]["described_operation"], 8, "{read}");

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
        json!({"described_operation": {"inferred": 8}, "endpoint": {"discovered": 2}}),
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
        json!({"inferred": 8}),
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
    let written = drafted(&scouted);
    for file in [
        "commands/get_exampleitem.yaml",
        "commands/set_exampleitem.yaml",
    ] {
        assert!(
            written.iter().any(|one| one == file),
            "{file} in {written:?}"
        );
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
