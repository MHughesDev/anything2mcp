//! What the scout learns from a fish completion file and a manual page a
//! person hands it (decision 0284), and from a PowerShell help file and a
//! module manifest (decision 0290), against the command-line test stand.
//! What was started is asked of the stand's own record of every start.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Archetype, Environment, EvidenceLevel, Finding, NoSecrets, Redact, Source, SurfaceKind, Values,
};
use a2m_pack_command::codes;
use a2m_provider::{
    Access, CheckedPaths, Deadline, Destinations, DiscoveryReport, Posture, PrepareContext,
    ProgramRef, ReadOnlyPort, Registry, RootName, Roots, SurfaceDescription, SurfaceRef,
};
use common::{Folder, registry, stand_files};
use example_cli::harness::Stand;

const WITHIN: Duration = Duration::from_secs(60);

/// A redactor that knows one planted secret.
struct Planted(&'static str);

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

struct Found(Vec<Finding>);

impl a2m_provider::FindingSink for Found {
    fn found(&mut self, finding: Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

fn source(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sources")
            .join(name),
    )
    .unwrap()
}

fn description(surface: &SurfaceRef) -> SurfaceDescription {
    SurfaceDescription {
        kind: SurfaceKind::new("program").unwrap(),
        archetype: Archetype::Command,
        address: surface.surface.address.clone(),
        facts: Values::new(),
    }
}

/// Runs the pack's discoverer on the stand, handed `files`: each a name
/// and the bytes of a file put in a `sources` folder of the test's own.
/// Also gives how many starts the stand had recorded when the discovery
/// began, after `prepare` asked the version.
async fn discover(
    stand: &Stand,
    folder: &Folder,
    registry: &Registry,
    files: &[(&str, &str, Vec<u8>)],
    redact: &dyn Redact,
) -> (
    Result<DiscoveryReport, a2m_core::Failure>,
    Vec<Finding>,
    usize,
) {
    let compiled_target = common::stand_compiled_target(stand, registry.compiler_set());
    let surface = SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: compiled_target.surfaces[0].clone(),
    };
    let sources = folder.path().join("sources");
    std::fs::create_dir_all(&sources).unwrap();
    let root = RootName::new("sources").unwrap();
    let mut roots = Roots::none();
    roots.add(root.clone(), &sources, Access::Read).unwrap();
    let mut paths = CheckedPaths::none();
    for (name, file, bytes) in files {
        std::fs::write(sources.join(file), bytes).unwrap();
        paths.insert(*name, roots.check(&root, file, Access::Read).unwrap());
    }
    let handle = registry.driver(&codes::mode()).unwrap();
    let supervisor = handle.supervisor(
        surface
            .surface
            .programs
            .iter()
            .map(ProgramRef::from)
            .collect(),
    );
    let cx = PrepareContext {
        run: a2m_core::RunId::from_bytes([9; 16]),
        epoch: a2m_core::Epoch::new(1),
        surface: &surface,
        compiled_target: &compiled_target,
        posture: Posture::Discovery,
        deadline: Deadline::after(WITHIN),
        redact: &NoSecrets,
        destinations: &Destinations::default(),
        paths: &CheckedPaths::none(),
        supervisor: &supervisor,
    };
    let prepared = handle.prepare(&cx, None).await.unwrap();
    let before = stand.control().invocations().unwrap().len();
    // The pack has two discoverers: the one that runs a program's declared
    // help forms, and the one that reads a binary's bytes and starts nothing.
    let all = registry.discoverers_for(&description(&surface));
    assert_eq!(all.len(), 2);
    let discoverers: Vec<_> = all
        .into_iter()
        .filter(|found| found.declared().name.to_string() == "help")
        .collect();
    assert_eq!(discoverers.len(), 1);
    let port = ReadOnlyPort::new(handle.clone(), prepared);
    let scope = a2m_provider::DiscoveryScope {
        surface: &surface,
        bounds: discoverers[0].declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &paths,
        redact,
        focus: &Values::new(),
    };
    let mut found = Found(Vec::new());
    let report = discoverers[0].discover(&port, &scope, &mut found).await;
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    (report, found.0, before)
}

fn of_kind<'a>(findings: &'a [Finding], kind: &str) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|finding| finding.kind().as_str() == kind)
        .collect()
}

fn command<'a>(findings: &'a [Finding], name: &str) -> &'a Finding {
    of_kind(findings, "command")
        .into_iter()
        .find(|finding| finding.found().contains(&format!("\"name\":\"{name}\"")))
        .unwrap_or_else(|| panic!("no finding for {name}"))
}

fn json(finding: &Finding) -> serde_json::Value {
    serde_json::from_str(finding.found()).unwrap()
}

fn option<'a>(found: &'a serde_json::Value, flag: &str) -> &'a serde_json::Value {
    found["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| {
            option["flags"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f == flag)
        })
        .unwrap_or_else(|| panic!("no option {flag} in {found}"))
}

fn from(option: &serde_json::Value) -> Vec<&str> {
    option["said"]
        .as_array()
        .unwrap()
        .iter()
        .map(|said| said["from"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn handed_files_are_read_as_text_and_each_option_keeps_its_source() {
    let folder = Folder::new("metadata");
    let stand = Stand::start().unwrap();
    let control = stand.control().clone();
    let seed = control.stored_items().unwrap();
    let registry = registry(&folder, Arc::new(NoSecrets));

    // The registry takes the two names from the manifest: nothing else is
    // needed for `scout run --files` to accept them.
    let compiled_target = common::stand_compiled_target(&stand, registry.compiler_set());
    let surface = SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: compiled_target.surfaces[0].clone(),
    };
    let taken: Vec<String> = registry
        .files_taken(&description(&surface))
        .into_iter()
        .collect();
    assert_eq!(
        taken,
        [
            "binary",
            "completions",
            "manual",
            "module_manifest",
            "powershell_help"
        ]
    );

    let (report, findings, _) = discover(
        &stand,
        &folder,
        &registry,
        &[
            (
                "completions",
                "example-cli.fish",
                source("example-cli.fish"),
            ),
            (
                "manual",
                "example-cli-item-list.1",
                source("example-cli-item-list.1"),
            ),
        ],
        &NoSecrets,
    )
    .await;
    let report = report.unwrap();

    // One finding for each handed file, which says how much was read.
    let files = of_kind(&findings, "command_file");
    assert_eq!(files.len(), 2);
    for file in &files {
        assert_eq!(file.level(), EvidenceLevel::Inferred);
        assert_eq!(file.source(), Source::Artifact);
        assert_eq!(file.coverage(), a2m_core::Coverage::Partial);
    }
    let fish = json(files[0]);
    assert_eq!(fish["name"], "completions");
    assert_eq!(fish["form"], "fish");
    assert_eq!(fish["read"], 6);
    assert_eq!(
        fish["unread"],
        serde_json::json!({
            "block": 3, "command_substitution": 1, "condition": 1,
            "names_no_option": 2, "variable": 1
        })
    );
    assert_eq!(fish["unlisted"], serde_json::json!(["archive"]));
    assert_eq!(
        fish["program_options"][0]["flags"],
        serde_json::json!(["--home"])
    );
    let manual = json(files[1]);
    assert_eq!(
        (manual["form"].as_str(), manual["read"].as_u64()),
        (Some("man"), Some(4))
    );
    assert_eq!(manual["unread"], serde_json::json!({"optional_value": 1}));
    assert_eq!(report.dropped, 9);

    // Each option is found with its source; where the help and the manual
    // page disagree, the finding says so and chooses neither.
    let list = command(&findings, "item list");
    assert_eq!(list.level(), EvidenceLevel::Inferred);
    assert_eq!(list.source(), Source::Artifact);
    assert_eq!(list.derived_from(), [files[1].id()]);
    let found = json(list);
    let location = option(&found, "--location");
    assert_eq!(location["flags"], serde_json::json!(["-l", "--location"]));
    assert_eq!(from(location), ["help", "manual"]);
    assert_eq!(location["disagree"], false);
    let reference = option(&found, "--ref");
    assert_eq!(reference["disagree"], true);
    assert_eq!(reference["said"][0]["takes"], "value");
    assert_eq!(reference["said"][1]["takes"], "nothing");
    assert_eq!(from(option(&found, "--limit")), ["manual"]);
    let show = json(command(&findings, "item show"));
    assert_eq!(from(option(&show, "--format")), ["help", "completions"]);
    let set = json(command(&findings, "item set"));
    assert_eq!(
        option(&set, "--yes")["disagree"],
        false,
        "not stated agrees"
    );
    // A command no file speaks of is what the help alone gives.
    let add = command(&findings, "item add");
    assert_eq!(add.level(), EvidenceLevel::Discovered);
    assert!(!add.found().contains("options"));

    // Nothing in either file was run: the stand started only the declared
    // help and version forms, and the substitution that would push the
    // catalog never ran.
    let starts = control.invocations().unwrap();
    assert!(!starts.is_empty());
    for start in &starts {
        assert!(
            matches!(start.args[0].as_str(), "--version" | "--help" | "help"),
            "{:?}",
            start.args
        );
        assert!(
            !start.args.iter().any(|arg| arg == "--yes"),
            "{:?}",
            start.args
        );
    }
    assert!(control.writes().unwrap().is_empty());
    assert_eq!(control.stored_items().unwrap(), seed);

    let described = registry
        .discoverers_for(&description(&surface))
        .into_iter()
        .find(|found| found.declared().name.to_string() == "help")
        .unwrap()
        .describe(&findings, a2m_provider::DescribeForm::Summary);
    assert!(
        described.text.contains("2 handed files"),
        "{}",
        described.text
    );

    // The drafts: a file's words word a question, a flag the sources
    // disagree on and a flag only a file states are questions, never
    // parameters, and no effect is written.
    let target = Folder::new("metadata-draft");
    for (path, text) in stand_files(&stand) {
        if path == "target.yaml" || path == "types.yaml" {
            std::fs::write(target.path().join(&path), text).unwrap();
        }
    }
    let root = a2m_files::TargetRoot::new(target.path());
    let drafted = a2m_files::Drafter::new(registry.compiler_set(), &root)
        .draft(&findings)
        .unwrap();
    // Every command is its own finding and each file holds an open
    // question, so all nine wait in the holding folder (decision 0298).
    assert!(drafted.written.is_empty(), "{drafted:?}");
    assert_eq!(drafted.held.len(), 9, "{drafted:?}");
    assert!(
        drafted
            .held
            .iter()
            .all(|path| path.as_str().starts_with("held/commands/")),
        "{drafted:?}"
    );
    assert_eq!(
        drafted.not_covered,
        files.iter().map(|file| file.id()).collect::<Vec<_>>()
    );
    let asked = |command: &str, word: &str| -> String {
        drafted
            .open_questions
            .iter()
            .find(|open| {
                open.quoting.as_ref().is_some_and(|quoting| {
                    quoting.words().first().map(String::as_str) == Some(command)
                        && quoting.words().iter().any(|quoted| quoted == word)
                })
            })
            .map(|open| open.quoting.as_ref().unwrap().sentence().to_owned())
            .unwrap_or_else(|| panic!("no question on {command} quoting {word}"))
    };
    assert!(asked("item list", "--ref").contains("disagree"));
    assert!(asked("item list", "--limit").contains("does not show it"));
    assert!(asked("catalog push", "--dry-run").contains("does not show it"));
    assert!(asked("item show", "How to print the item").contains("what a file handed"));
    assert!(
        !target
            .path()
            .join("commands")
            .join("item_list.yaml")
            .exists()
    );
    let draft = std::fs::read_to_string(
        target
            .path()
            .join("held")
            .join("commands")
            .join("item_list.yaml"),
    )
    .unwrap();
    assert!(!draft.contains("limit:"), "{draft}");
    // Each draft compiles but for the questions it leaves: every problem
    // the compiler and the lints find is an open question, a value that
    // only a person can write.
    for (path, problems) in &drafted.kept_failing {
        for problem in problems.iter() {
            assert_eq!(
                problem.code.as_str(),
                "kernel.open_question",
                "{path:?}: {problem:?}"
            );
        }
    }
    for never in ["effect: read", "effect: write"] {
        assert!(!draft.contains(never), "{draft}");
    }
    assert_eq!(control.invocations().unwrap().len(), starts.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_in_a_handed_file_is_redacted_and_an_unreadable_file_starts_nothing() {
    const SECRET: &str = "secret-for-tests-4471";
    let folder = Folder::new("metadata-secret");
    let stand = Stand::start().unwrap();
    let control = stand.control().clone();
    let registry = registry(&folder, Arc::new(Planted(SECRET)));
    let planted = Planted(SECRET);

    // A page compressed in a form the pack does not read stops the
    // discovery before any program is started.
    let (report, findings, before) = discover(
        &stand,
        &folder,
        &registry,
        &[("manual", "example-cli.1.bz2", b"BZh91AY&SY".to_vec())],
        &planted,
    )
    .await;
    let failure = report.unwrap_err();
    assert_eq!(failure.code(), &codes::COMMAND_FILE_UNREADABLE);
    assert!(findings.is_empty());
    assert_eq!(control.invocations().unwrap().len(), before);

    // A description holding a secret reaches no finding.
    let page = format!(
        ".TH EXAMPLE-CLI-ITEM-SHOW 1\n.SH OPTIONS\n.TP\n\\fB\\-\\-format\\fR \\fIform\\fR\nPrint as given; the key is\n{SECRET}\nand no other.\n"
    );
    let fish = format!(
        "complete -c example-cli -n '__fish_seen_subcommand_from show' -l format -x -d 'uses {SECRET}'\n"
    );
    let (report, findings, _) = discover(
        &stand,
        &folder,
        &registry,
        &[
            ("completions", "example-cli.fish", fish.into_bytes()),
            ("manual", "example-cli-item-show.1", page.into_bytes()),
        ],
        &planted,
    )
    .await;
    report.unwrap();
    for finding in &findings {
        assert!(!finding.found().contains(SECRET), "{}", finding.found());
    }
    let show = command(&findings, "item show").found().to_owned();
    assert!(show.contains(a2m_core::REDACTED), "{show}");
}

/// A target whose one surface is Windows PowerShell, by its file's name,
/// as the pack's lint knows it: the PowerShell drafts are drafted into it.
fn powershell_target(stand: &Stand, folder: &Folder) -> a2m_files::TargetRoot {
    let program = folder
        .path()
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    for (path, text) in stand_files(stand) {
        if path == "target.yaml" || path == "types.yaml" {
            let text = text.replace(
                &format!(
                    "'{}'",
                    stand.program().display().to_string().replace('\'', "''")
                ),
                &format!("'{}'", program.display().to_string().replace('\'', "''")),
            );
            std::fs::write(folder.path().join(&path), text).unwrap();
        }
    }
    let target = std::fs::read_to_string(folder.path().join("target.yaml")).unwrap();
    assert!(target.contains("powershell.exe"), "{target}");
    a2m_files::TargetRoot::new(folder.path())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_powershell_help_file_and_a_manifest_are_read_as_data_and_draft_fixed_pairs() {
    let folder = Folder::new("powershell");
    let stand = Stand::start().unwrap();
    let control = stand.control().clone();
    let seed = control.stored_items().unwrap();
    let registry = registry(&folder, Arc::new(NoSecrets));
    let (report, findings, before) = discover(
        &stand,
        &folder,
        &registry,
        &[
            (
                "powershell_help",
                "Example.Inventory-help.xml",
                source("Example.Inventory-help.xml"),
            ),
            (
                "module_manifest",
                "Example.Inventory.psd1",
                source("Example.Inventory.psd1"),
            ),
        ],
        &NoSecrets,
    )
    .await;
    let report = report.unwrap();

    // One finding for each file, which says how much of it was read.
    let files = of_kind(&findings, "command_file");
    assert_eq!(files.len(), 2);
    let manifest = json(files[0]);
    assert_eq!(
        (manifest["name"].as_str(), manifest["form"].as_str()),
        (Some("module_manifest"), Some("psd1"))
    );
    assert_eq!(manifest["version"], "1.4.0");
    assert_eq!(
        manifest["exported"],
        serde_json::json!([
            "Get-ExampleItem",
            "Set-ExampleItem",
            "Remove-ExampleItem",
            "gei"
        ])
    );
    // What it exports that the help file does not describe.
    assert_eq!(
        manifest["unlisted"],
        serde_json::json!(["Remove-ExampleItem", "gei"])
    );
    assert_eq!(manifest["read"], 9);
    assert_eq!(manifest["unread"], serde_json::json!({"not_literal": 1}));
    assert_eq!(files[0].coverage(), a2m_core::Coverage::Partial);
    let help = json(files[1]);
    assert_eq!(
        (
            help["name"].as_str(),
            help["form"].as_str(),
            help["read"].as_u64()
        ),
        (Some("powershell_help"), Some("maml"), Some(2))
    );
    assert_eq!(help["unread"], serde_json::json!({}));
    assert_eq!(help["unlisted"], serde_json::json!([]));
    assert_eq!(files[1].coverage(), a2m_core::Coverage::Complete);
    for file in &files {
        assert_eq!(file.level(), EvidenceLevel::Inferred);
        assert_eq!(file.source(), Source::Artifact);
    }
    assert_eq!(report.dropped, 1);

    // One finding for each command the help describes, derived from the
    // help file's, with each parameter as the help states it.
    let commands = of_kind(&findings, "powershell_command");
    assert_eq!(commands.len(), 2);
    for command in &commands {
        assert_eq!(command.level(), EvidenceLevel::Inferred);
        assert_eq!(command.source(), Source::Artifact);
        assert_eq!(command.derived_from(), [files[1].id()]);
    }
    let get = json(commands[0]);
    assert_eq!(get["name"], "Get-ExampleItem");
    assert_eq!(get["synopsis"], "Gets one item of the example inventory.");
    let sku = &get["sets"][0][0];
    assert_eq!(
        (
            sku["name"].as_str(),
            sku["type"].as_str(),
            sku["required"].as_bool()
        ),
        (Some("Sku"), Some("System.String"), Some(true))
    );
    assert_eq!(sku["position"], "0");
    assert_eq!(sku["pipeline"], serde_json::json!(["by_value"]));
    assert!(
        sku["about"]
            .as_str()
            .unwrap()
            .starts_with("The item's code")
    );
    let detailed = &get["sets"][0][1];
    assert_eq!(
        (detailed["switch"].as_bool(), detailed["about"].as_str()),
        (Some(true), Some("Prints the item's history too."))
    );
    let set = json(commands[1]);
    assert_eq!(set["sets"].as_array().unwrap().len(), 2);

    // Nothing in either file was run, and no module was imported: the
    // stand started only the declared help and version forms.
    let starts = control.invocations().unwrap();
    for start in &starts {
        assert!(
            matches!(start.args[0].as_str(), "--version" | "--help" | "help"),
            "{:?}",
            start.args
        );
    }
    assert!(starts.len() >= before);
    assert!(control.writes().unwrap().is_empty());
    assert_eq!(control.stored_items().unwrap(), seed);

    // The drafts: each command a fixed pair of Windows PowerShell in its
    // one form and a script the target keeps, which only a person names;
    // each parameter given by name.
    let target = Folder::new("powershell-draft");
    let root = powershell_target(&stand, &target);
    let ours: Vec<Finding> = findings
        .iter()
        .filter(|finding| finding.kind().as_str() == "powershell_command")
        .cloned()
        .collect();
    let drafted = a2m_files::Drafter::new(registry.compiler_set(), &root)
        .draft(&ours)
        .unwrap();
    // Each command holds an open question (the program's digest, the
    // effect), so both wait in the holding folder (decision 0298).
    assert!(drafted.written.is_empty(), "{drafted:?}");
    assert_eq!(
        drafted
            .held
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        [
            "held/commands/get_exampleitem.yaml",
            "held/commands/set_exampleitem.yaml"
        ],
        "{drafted:?}"
    );
    assert!(!target.path().join("commands").exists());
    let draft = |name: &str| {
        std::fs::read_to_string(target.path().join("held").join("commands").join(name)).unwrap()
    };
    let get = draft("get_exampleitem.yaml");
    for expected in [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        "flag: \"-Sku\"",
        "param: \"sku\"",
        "flag: \"-Detailed\"",
        "when: \"detailed\"",
        "flag: \"-Rows\"",
        "type: \"integer\"",
    ] {
        assert!(get.contains(expected), "{expected} in {get}");
    }
    let set = draft("set_exampleitem.yaml");
    for expected in [
        "set_exampleitem_1:",
        "set_exampleitem_2:",
        "flag: \"-Force\"",
        "flag: \"-InputObject\"",
    ] {
        assert!(set.contains(expected), "{expected} in {set}");
    }
    // `-WhatIf` is PowerShell's own: it words the question of the effect
    // and is never a param.
    assert!(!set.contains("-WhatIf"), "{set}");
    let asked = |command: &str, word: &str| -> String {
        drafted
            .open_questions
            .iter()
            .find(|open| {
                open.quoting.as_ref().is_some_and(|quoting| {
                    quoting.words().first().map(String::as_str) == Some(command)
                        && quoting.words().iter().any(|quoted| quoted == word)
                })
            })
            .map(|open| open.quoting.as_ref().unwrap().sentence().to_owned())
            .unwrap_or_else(|| panic!("no question on {command} quoting {word}"))
    };
    assert!(asked("Set-ExampleItem", "WhatIf").contains("changes something"));
    assert!(asked("Get-ExampleItem", "System.String").contains("What type"));
    assert!(asked("Set-ExampleItem", "Example.Inventory.ExampleItem").contains("What type"));
    // `Rows` is a whole number by its type, and not asked.
    assert!(
        drafted.open_questions.iter().all(|open| open
            .quoting
            .as_ref()
            .is_none_or(|quoting| !quoting.words().iter().any(|word| word == "Rows"))),
        "{drafted:?}"
    );
    for never in ["effect: \"read\"", "effect: \"write\""] {
        assert!(!get.contains(never) && !set.contains(never));
    }
    // Each draft compiles but for its questions, on a surface that is
    // Windows PowerShell.
    for (path, problems) in &drafted.kept_failing {
        for problem in problems.iter() {
            assert_eq!(
                problem.code.as_str(),
                "kernel.open_question",
                "{path:?}: {problem:?}"
            );
        }
    }
    assert_eq!(control.invocations().unwrap().len(), starts.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_help_file_with_an_entity_too_large_or_not_help_starts_nothing_and_fetches_nothing() {
    let folder = Folder::new("powershell-refused");
    let stand = Stand::start().unwrap();
    let control = stand.control().clone();
    let registry = registry(&folder, Arc::new(NoSecrets));
    // A listener the external entity names: it must never be reached.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let marker = folder.path().join("marker.txt");
    std::fs::write(&marker, "marker-for-tests-5512").unwrap();
    let with_entity = format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE helpItems [\n  <!ENTITY far SYSTEM \"http://127.0.0.1:{port}/help\">\n  <!ENTITY near SYSTEM \"file:///{}\">\n]>\n<helpItems schema=\"maml\" xmlns=\"http://msh\"><command:command xmlns:command=\"http://schemas.microsoft.com/maml/dev/command/2004/10\"><command:details><command:name>&far;&near;</command:name></command:details></command:command></helpItems>\n",
        marker.display().to_string().replace('\\', "/")
    );
    let many = format!("<helpItems>{}</helpItems>", "<a/>".repeat(150_000));
    let cases: [(&str, &str, Vec<u8>, &str); 5] = [
        (
            "powershell_help",
            "entity-help.xml",
            with_entity.into_bytes(),
            "powershell_file_unreadable",
        ),
        (
            "powershell_help",
            "Example.Inventory.psd1",
            source("Example.Inventory.psd1"),
            "powershell_file_unreadable",
        ),
        (
            "powershell_help",
            "many-help.xml",
            many.into_bytes(),
            "powershell_file_unreadable",
        ),
        (
            "powershell_help",
            "large-help.xml",
            vec![b' '; 1_048_577],
            "file_unavailable",
        ),
        (
            "module_manifest",
            "not-a-manifest.psd1",
            b"Get-ExampleItem -Sku SK-1001\n".to_vec(),
            "powershell_file_unreadable",
        ),
    ];
    for (name, file, bytes, code) in cases {
        let (report, findings, before) = discover(
            &stand,
            &folder,
            &registry,
            &[(name, file, bytes)],
            &NoSecrets,
        )
        .await;
        let failure = report.unwrap_err();
        assert_eq!(failure.code().as_str(), code, "{file}: {failure:?}");
        assert!(findings.is_empty(), "{file}");
        assert!(!failure.message().contains("marker-for-tests"), "{file}");
        // Nothing was started after the file was read.
        assert_eq!(control.invocations().unwrap().len(), before, "{file}");
    }
    // The entity's address was never asked for.
    assert_eq!(
        listener.accept().map_err(|error| error.kind()).err(),
        Some(std::io::ErrorKind::WouldBlock)
    );

    // An element the reader does not know, inside a part it reads, leaves
    // that command unread and counted; the rest of the file is read.
    let unknown = String::from_utf8(source("Example.Inventory-help.xml"))
        .unwrap()
        .replacen(
            "<maml:name>Rows</maml:name>",
            "<maml:name>Rows</maml:name><command:mandatoryInSet>All</command:mandatoryInSet>",
            1,
        )
        .replacen(
            "<command:command ",
            "<providerHelp><Name>Example</Name></providerHelp>\n  <command:command ",
            1,
        );
    let (report, findings, _) = discover(
        &stand,
        &folder,
        &registry,
        &[("powershell_help", "unknown-help.xml", unknown.into_bytes())],
        &NoSecrets,
    )
    .await;
    report.unwrap();
    let help = json(of_kind(&findings, "command_file")[0]);
    assert_eq!(help["read"], 1);
    assert_eq!(
        help["unread"],
        serde_json::json!({"not_command": 1, "unknown_element": 1})
    );
    let commands = of_kind(&findings, "powershell_command");
    assert_eq!(commands.len(), 1);
    assert!(commands[0].found().contains("Set-ExampleItem"));
}

/// Decision 0302: a parameter a program's help shows under a secret's name
/// is never a param of the draft. An optional one is left out; a required one
/// makes the arguments of that command a question.
#[tokio::test(flavor = "multi_thread")]
async fn a_parameter_named_as_a_secret_is_never_drafted_as_a_param() {
    let folder = Folder::new("powershell-secret");
    let stand = Stand::start().unwrap();
    let registry = registry(&folder, Arc::new(NoSecrets));
    let help = String::from_utf8(source("Example.Inventory-help.xml"))
        .unwrap()
        .replace(
            "<maml:name>Detailed</maml:name>",
            "<maml:name>ClientSecret</maml:name>",
        )
        .replace(
            "<maml:name>Sku</maml:name>",
            "<maml:name>Password</maml:name>",
        );
    let (report, findings, _) = discover(
        &stand,
        &folder,
        &registry,
        &[(
            "powershell_help",
            "Example.Inventory-help.xml",
            help.into_bytes(),
        )],
        &NoSecrets,
    )
    .await;
    report.unwrap();
    let ours: Vec<Finding> = findings
        .iter()
        .filter(|finding| finding.kind().as_str() == "powershell_command")
        .cloned()
        .collect();
    assert_eq!(ours.len(), 2);
    let target = Folder::new("powershell-secret-draft");
    let root = powershell_target(&stand, &target);
    let drafted = a2m_files::Drafter::new(registry.compiler_set(), &root)
        .draft(&ours)
        .unwrap();
    assert!(drafted.refused.is_empty(), "{drafted:?}");
    let mut seen = 0;
    for path in &drafted.held {
        let text = std::fs::read_to_string(target.path().join(path.as_str())).unwrap();
        seen += 1;
        // No param, flag, or argument has a secret's name, and `Rows`
        // stays a param: only the secret's names are left out.
        for secret in ["clientsecret", "password"] {
            assert!(
                !text.to_ascii_lowercase().contains(secret),
                "{path}: {text}"
            );
        }
    }
    assert_eq!(seen, 2);
    // Of `Set-ExampleItem`, the set that requires `Sku` has its arguments
    // asked, and the set without it still gives `-InputObject` by name.
    let set =
        std::fs::read_to_string(target.path().join("held/commands/set_exampleitem.yaml")).unwrap();
    assert!(set.contains("flag: \"-InputObject\""), "{set}");
    assert!(set.contains("args: {open_question:"), "{set}");
    // `Sku` was required, so the arguments of that set are a question, and
    // the question names no secret.
    assert!(
        drafted.open_questions.iter().any(|open| open
            .question
            .contains("requires a parameter under a name that is a secret's")),
        "{:?}",
        drafted.open_questions
    );
}

/// Copy test 5, findings 28 and 29 (decision 0364): a program that prints
/// a second version line, and help set out with tabs. The scout starts, reads
/// every command, and says how many lines of the listing it skipped and why.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_version_line_and_a_listing_set_out_with_tabs_do_not_stop_the_scout() {
    use example_cli::faults::{Fault, FaultKind};

    let folder = Folder::new("tabs");
    let stand = Stand::start().unwrap();
    let registry = registry(&folder, Arc::new(NoSecrets));

    let (report, findings, _) = discover(&stand, &folder, &registry, &[], &NoSecrets).await;
    let plain = report.unwrap();
    let names = |findings: &[Finding]| -> Vec<String> {
        of_kind(findings, "command")
            .into_iter()
            .map(|finding| json(finding)["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let expected = names(&findings);
    assert_eq!(expected.len(), 9, "{expected:?}");
    assert_eq!(plain.dropped, 0);

    let control = stand.control();
    control
        .set_fault(&Fault::until_cleared(FaultKind::TabbedHelp).on("help"))
        .unwrap();
    control
        .set_fault(&Fault::until_cleared(FaultKind::UnnamedHelpLine).on("help"))
        .unwrap();
    control
        .set_fault(&Fault::until_cleared(FaultKind::InterfaceLine))
        .unwrap();
    let (report, findings, _) = discover(&stand, &folder, &registry, &[], &NoSecrets).await;
    let report = report.unwrap();
    // Every command is read, with its text, though the listing uses tabs and
    // the version is the first of two lines.
    assert_eq!(names(&findings), expected);
    assert_eq!(
        json(command(&findings, "item show"))["about"],
        "Show one item."
    );
    // The line that holds no name is skipped, counted, and said by reason.
    assert_eq!(report.dropped, 1);
    let listing = of_kind(&findings, "command_file")
        .into_iter()
        .find(|finding| json(finding)["form"] == "program_help")
        .expect("a finding that says the skipped line");
    let said = json(listing);
    assert_eq!(said["unread"], serde_json::json!({"name_not_plain": 1}));
    assert_eq!(said["read"], expected.len());
}
