//! The decompiling engine as a provider (package X6), proven against the
//! stand that stands in for an installed engine and the command-line stand
//! as the file that is inspected: an engine that is not there is said and
//! the scout goes on; a program whose digest is not pinned is not started;
//! the file is handed over as one argument and never run; and what the
//! engine prints is read in one form only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use a2m_core::{Digest, FailureClass};
use a2m_provider::decompiler::{Decompiler, DecompilerSpec, FILE_ARGUMENT, Installed};
use a2m_provider::{
    Access, CheckedPath, ProgramTrust, RootName, Roots, Supervisor, SupervisorLimits,
};
use common::Folder;
use example_cli::harness::Stand;

/// The stand engine, placed in a folder of the test's own, and what it
/// needs to report to the test.
struct Engine {
    folder: Folder,
    program: PathBuf,
    log: PathBuf,
}

impl Engine {
    fn new() -> Self {
        let built = example_decompiler::program().unwrap();
        let folder = Folder::new("engine");
        let program = folder.path().join(built.file_name().unwrap());
        std::fs::copy(&built, &program).unwrap();
        let log = folder.path().join("starts.log");
        Self {
            folder,
            program,
            log,
        }
    }

    fn digest(&self) -> Digest {
        Digest::of(&std::fs::read(&self.program).unwrap())
    }

    fn spec(&self, mode: &str, pinned: Vec<Digest>) -> DecompilerSpec {
        DecompilerSpec {
            name: "example".to_owned(),
            pinned,
            args: vec![FILE_ARGUMENT.to_owned()],
            env: vec![
                (example_decompiler::env::MODE.to_owned(), mode.into()),
                (
                    example_decompiler::env::LOG.to_owned(),
                    self.log.clone().into_os_string(),
                ),
            ],
            time_limit: Duration::from_secs(60),
            max_output_bytes: 1024 * 1024,
        }
    }

    fn decompiler(&self, spec: DecompilerSpec) -> Decompiler {
        Decompiler::new(
            spec,
            Some(self.program.clone()),
            Supervisor::new(SupervisorLimits::default()),
            ProgramTrust::default(),
        )
        .unwrap()
    }

    /// The arguments of each start the stand engine reports.
    fn starts(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                value["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap().to_owned())
                    .collect()
            })
            .collect()
    }
}

/// A file of a `inputs` root, as a discovery is handed it.
fn handed(folder: &Path, name: &str) -> CheckedPath {
    let mut roots = Roots::none();
    let root = RootName::new("inputs").unwrap();
    roots.add(root.clone(), folder, Access::Read).unwrap();
    roots
        .folder(&root, Access::Read)
        .unwrap()
        .join(name)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_engine_installed_it_says_so_and_nothing_starts() {
    let engine = Engine::new();
    let sources = Folder::new("inputs");
    std::fs::write(sources.path().join("file.bin"), b"x").unwrap();
    let file = handed(sources.path(), "file.bin");
    let spec = engine.spec("ok", vec![engine.digest()]);
    for installed in [None, Some(engine.folder.path().join("no-such-engine"))] {
        let decompiler = Decompiler::new(
            spec.clone(),
            installed,
            Supervisor::new(SupervisorLimits::default()),
            ProgramTrust::default(),
        )
        .unwrap();
        let failure = decompiler.decompile(&file, 100).await.unwrap_err();
        assert_eq!(
            (failure.class(), failure.code().as_str()),
            (FailureClass::Busy, "decompiler_not_installed")
        );
    }
    assert!(engine.starts().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spec_that_pins_nothing_or_names_no_file_is_refused() {
    let engine = Engine::new();
    for bad in [
        engine.spec("ok", Vec::new()),
        DecompilerSpec {
            args: Vec::new(),
            ..engine.spec("ok", vec![engine.digest()])
        },
        DecompilerSpec {
            args: vec![FILE_ARGUMENT.to_owned(), FILE_ARGUMENT.to_owned()],
            ..engine.spec("ok", vec![engine.digest()])
        },
        DecompilerSpec {
            args: vec![format!("--in={FILE_ARGUMENT}")],
            ..engine.spec("ok", vec![engine.digest()])
        },
    ] {
        let refused = Decompiler::new(
            bad,
            None,
            Supervisor::new(SupervisorLimits::default()),
            ProgramTrust::default(),
        )
        .unwrap_err();
        assert_eq!(refused.code().as_str(), "decompiler_spec_invalid");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pinned_engine_is_handed_the_file_as_its_one_argument_and_the_file_is_never_run() {
    let engine = Engine::new();
    // The inspected file is a program that records every start of itself.
    let stand = Stand::start().unwrap();
    let file = handed(
        stand.root(),
        stand.program().file_name().unwrap().to_str().unwrap(),
    );
    let decompiler = engine.decompiler(engine.spec("ok", vec![engine.digest()]));
    assert_eq!(
        decompiler.identify().await,
        Installed::Pinned {
            path: engine.program.clone(),
            digest: engine.digest()
        }
    );
    let made = decompiler.decompile(&file, 100).await.unwrap();
    assert_eq!(made.functions.len(), 3);
    assert_eq!(made.functions[1].function, "fn_1");
    assert_eq!(made.functions[1].address, "0x1010");
    assert!(made.functions[1].text.starts_with("int fn_1"));
    assert_eq!((made.over, made.cut), (0, false));
    assert_eq!(made.engine, engine.digest());
    // One start, with the file's own path as its only argument.
    assert_eq!(
        engine.starts(),
        [vec![file.as_path().to_str().unwrap().to_owned()]]
    );
    // The inspected program was read and never started.
    assert!(stand.control().invocations().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_whose_digest_is_not_pinned_is_refused_and_not_started() {
    let engine = Engine::new();
    let sources = Folder::new("inputs");
    std::fs::write(sources.path().join("file.bin"), b"x").unwrap();
    let file = handed(sources.path(), "file.bin");
    let pinned = engine.digest();
    let decompiler = engine.decompiler(engine.spec("ok", vec![pinned]));
    // Replaced after it was pinned.
    let mut bytes = std::fs::read(&engine.program).unwrap();
    bytes.push(0);
    std::fs::write(&engine.program, &bytes).unwrap();
    assert!(matches!(
        decompiler.identify().await,
        Installed::NotPinned { .. }
    ));
    let failure = decompiler.decompile(&file, 100).await.unwrap_err();
    assert_eq!(
        (failure.class(), failure.code().as_str()),
        (FailureClass::Policy, "decompiler_not_pinned")
    );
    // Another engine's digest is no better.
    let other = engine.decompiler(engine.spec("ok", vec![Digest::of(b"another engine")]));
    let failure = other.decompile(&file, 100).await.unwrap_err();
    assert_eq!(failure.code().as_str(), "decompiler_not_pinned");
    assert!(engine.starts().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_regular_file_is_handed_over() {
    let engine = Engine::new();
    let sources = Folder::new("inputs");
    std::fs::create_dir(sources.path().join("folder")).unwrap();
    let folder = handed(sources.path(), "folder");
    let decompiler = engine.decompiler(engine.spec("ok", vec![engine.digest()]));
    let failure = decompiler.decompile(&folder, 100).await.unwrap_err();
    assert_eq!(failure.code().as_str(), "file_unavailable");
    assert!(engine.starts().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn output_in_any_other_form_is_a_named_stop_and_none_of_it_is_used() {
    let engine = Engine::new();
    let sources = Folder::new("inputs");
    std::fs::write(sources.path().join("file.bin"), b"x").unwrap();
    let file = handed(sources.path(), "file.bin");
    for (mode, class, code) in [
        ("fail", FailureClass::Rejected, "decompiler_failed"),
        (
            "garbage",
            FailureClass::UnknownVariant,
            "decompiler_output_unknown",
        ),
        (
            "no_end",
            FailureClass::UnknownVariant,
            "decompiler_output_unknown",
        ),
    ] {
        let decompiler = engine.decompiler(engine.spec(mode, vec![engine.digest()]));
        let failure = decompiler.decompile(&file, 100).await.unwrap_err();
        assert_eq!(
            (failure.class(), failure.code().as_str()),
            (class, code),
            "{mode}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn what_is_beyond_a_bound_is_counted_and_dropped_and_said() {
    let engine = Engine::new();
    let sources = Folder::new("inputs");
    std::fs::write(sources.path().join("file.bin"), b"x").unwrap();
    let file = handed(sources.path(), "file.bin");
    // More functions than are kept.
    let decompiler = engine.decompiler(engine.spec("flood", vec![engine.digest()]));
    let made = decompiler.decompile(&file, 10).await.unwrap();
    assert_eq!(
        (made.functions.len(), made.over, made.cut),
        (10, 1990, false)
    );
    // Output cut at its own bound: the whole lines before the cut are kept,
    // and the cut is said.
    let mut spec = engine.spec("flood", vec![engine.digest()]);
    spec.max_output_bytes = 10_000;
    let made = engine
        .decompiler(spec)
        .decompile(&file, 100_000)
        .await
        .unwrap();
    assert!(made.cut);
    assert!(!made.functions.is_empty() && made.functions.len() < 2000);
    assert!(made.functions.iter().all(|f| f.text.ends_with("; }")));
}

/// The variable the stand reads is the one the spec set, and no variable of
/// the engine's own reaches it.
#[tokio::test(flavor = "multi_thread")]
async fn the_engine_is_started_with_the_listed_environment_only() {
    let engine = Engine::new();
    let sources = Folder::new("inputs");
    std::fs::write(sources.path().join("file.bin"), b"x").unwrap();
    let file = handed(sources.path(), "file.bin");
    let mut spec = engine.spec("ok", vec![engine.digest()]);
    spec.env
        .push(("A2M_SOMETHING".to_owned(), OsString::from("1")));
    let failure = engine
        .decompiler(spec)
        .decompile(&file, 100)
        .await
        .unwrap_err();
    assert_eq!(failure.code().as_str(), "engine_variable_refused");
    assert!(engine.starts().is_empty());
}
