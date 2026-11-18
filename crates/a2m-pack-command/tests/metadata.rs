//! What the scout learns from a fish completion file and a manual page a
//! person hands it, against the command-line test stand (decision 0284).
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
    let plan = common::stand_plan(stand, registry.compiler_set());
    let surface = SurfaceRef {
        target: plan.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: plan.surfaces[0].clone(),
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
        plan: &plan,
        posture: Posture::Discovery,
        deadline: Deadline::after(WITHIN),
        redact: &NoSecrets,
        destinations: &Destinations::default(),
        paths: &CheckedPaths::none(),
        supervisor: &supervisor,
    };
    let prepared = handle.prepare(&cx, None).await.unwrap();
    let before = stand.control().invocations().unwrap().len();
    let discoverers = registry.discoverers_for(&description(&surface));
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
    let plan = common::stand_plan(&stand, registry.compiler_set());
    let surface = SurfaceRef {
        target: plan.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: plan.surfaces[0].clone(),
    };
    let taken: Vec<String> = registry
        .files_taken(&description(&surface))
        .into_iter()
        .collect();
    assert_eq!(taken, ["completions", "manual"]);

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

    let described = registry.discoverers_for(&description(&surface))[0]
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
    assert_eq!(drafted.written.len(), 9, "{drafted:?}");
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
    let draft =
        std::fs::read_to_string(target.path().join("commands").join("item_list.yaml")).unwrap();
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
