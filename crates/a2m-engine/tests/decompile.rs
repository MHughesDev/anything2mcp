//! The decompiling engine as a provider of discovery (package X6), proven
//! through the engine's own discovery job: with no engine installed the
//! scout says so and goes on to the other discoverers; a program whose
//! digest is not pinned is not started; what the engine prints is stored as
//! findings no stronger than `discovered`, listed apart by the describer,
//! stripped of a registered secret, and reaches nothing of a run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{Digest, EvidenceLevel, Failure, Finding, Redact, Source};
use a2m_engine::EngineEvent;
use a2m_logs::SystemClock;
use a2m_provider::decompiler::{
    Decompiler, DecompilerDescriber, DecompilerDiscoverer, DecompilerSpec, FILE_ARGUMENT,
};
use a2m_provider::{
    DescribeForm, Describer, Pack, PackContext, ProgramTrust, Registrar, Supervisor,
    SupervisorLimits,
};
use common::{ORDER_GET, Scene, completed};

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "example_decompile"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[points]
discoverer = ["decompile"]

[[discoverers]]
name = "decompile"
archetype = "request"
surface_kinds = ["json_app"]
kinds = ["decompiled_function"]
sources = ["artifact"]
max_level = "inferred"
bounds = { seconds = 60, bytes = 10000000, depth = 4 }
files = ["decompile"]

[[finding_kinds]]
name = "decompiled_function"
surface_kinds = ["json_app"]
"#;

struct DecompilePack {
    decompiler: Arc<Decompiler>,
    redact: Arc<dyn Redact>,
}

impl Pack for DecompilePack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        to.discoverer(
            Arc::new(DecompilerDiscoverer::new(
                Arc::clone(&self.decompiler),
                Arc::new(SystemClock::new()),
                "example_decompile",
                "0.1.0",
            )),
            Arc::new(DecompilerDescriber {
                redact: Arc::clone(&self.redact),
            }),
        )?;
        Ok(())
    }
}

/// The stand engine in a folder of the test's own.
struct Installed {
    _folder: common::Folder,
    program: PathBuf,
    log: PathBuf,
}

impl Installed {
    fn new() -> Self {
        let built = example_decompiler::program().unwrap();
        let folder = common::Folder::new("engine");
        let program = folder.path().join(built.file_name().unwrap());
        std::fs::copy(&built, &program).unwrap();
        let log = folder.path().join("starts.log");
        Self {
            _folder: folder,
            program,
            log,
        }
    }

    fn digest(&self) -> Digest {
        Digest::of(&std::fs::read(&self.program).unwrap())
    }

    fn starts(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .count()
    }

    fn decompiler(
        &self,
        installed: bool,
        pinned: Vec<Digest>,
        mode: &str,
        secret: &str,
    ) -> Arc<Decompiler> {
        let spec = DecompilerSpec {
            name: "example".to_owned(),
            pinned,
            args: vec![FILE_ARGUMENT.to_owned()],
            env: vec![
                (example_decompiler::env::MODE.to_owned(), mode.into()),
                (
                    example_decompiler::env::LOG.to_owned(),
                    self.log.clone().into_os_string(),
                ),
                (example_decompiler::env::SECRET.to_owned(), secret.into()),
            ],
            time_limit: Duration::from_secs(60),
            max_output_bytes: 1024 * 1024,
        };
        Arc::new(
            Decompiler::new(
                spec,
                installed.then(|| self.program.clone()),
                Supervisor::new(SupervisorLimits::default()),
                ProgramTrust::default(),
            )
            .unwrap(),
        )
    }
}

async fn scene_with(decompiler: Arc<Decompiler>) -> Scene {
    Scene::with_packs(
        ORDER_GET,
        |_| {},
        |_| {},
        move |builder, redact| {
            builder.add_pack(&DecompilePack {
                decompiler,
                redact: Arc::clone(redact),
            });
        },
    )
    .await
}

/// Hands the discovery a file of the project's `inputs` folder.
fn handing(scene: &Scene) -> common::Folder {
    let project = common::Folder::new("handed");
    std::fs::create_dir_all(project.path().join("inputs")).unwrap();
    std::fs::write(project.path().join("inputs").join("inspected.bin"), b"B").unwrap();
    *scene.control.project.lock().unwrap() = Some(project.path().to_path_buf());
    *scene.control.discovery_files.lock().unwrap() =
        [("decompile".to_owned(), "inputs/inspected.bin".to_owned())]
            .into_iter()
            .collect();
    project
}

type Seen = (String, Vec<Finding>, Result<(), String>);

/// What each discoverer found and said, as it reached `Control`.
fn discovered(scene: &Scene) -> Vec<Seen> {
    scene
        .control
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            EngineEvent::Discovered {
                discoverer,
                findings,
                report,
                ..
            } => Some((
                discoverer.clone(),
                findings.clone(),
                report
                    .as_ref()
                    .map(|_| ())
                    .map_err(|failure| failure.code().to_string()),
            )),
            _ => None,
        })
        .collect()
}

fn of<'a>(seen: &'a [Seen], discoverer: &str) -> &'a Seen {
    seen.iter()
        .find(|(name, ..)| name == discoverer)
        .unwrap_or_else(|| panic!("no event of `{discoverer}`: {seen:?}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_engine_installed_the_scout_says_so_and_goes_on() {
    let engine = Installed::new();
    let scene = scene_with(engine.decompiler(false, vec![engine.digest()], "ok", "")).await;
    let _project = handing(&scene);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    let seen = discovered(&scene);
    // The other discoverer of the surface ran and found what it found.
    let (_, findings, report) = of(&seen, "look");
    assert_eq!((findings.len(), report), (1, &Ok(())));
    // The engine is said to be missing, with no finding.
    let (_, findings, report) = of(&seen, "decompile");
    assert!(findings.is_empty());
    assert_eq!(report, &Err("decompiler_not_installed".to_owned()));
    assert_eq!(engine.starts(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_engine_whose_digest_is_not_the_pinned_one_is_refused_and_not_started() {
    let engine = Installed::new();
    let decompiler = engine.decompiler(true, vec![Digest::of(b"another release")], "ok", "");
    let scene = scene_with(decompiler).await;
    let _project = handing(&scene);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    let seen = discovered(&scene);
    let (_, findings, report) = of(&seen, "decompile");
    assert!(findings.is_empty());
    assert_eq!(report, &Err("decompiler_not_pinned".to_owned()));
    assert_eq!(engine.starts(), 0);
    // The scout went on.
    assert_eq!(of(&seen, "look").1.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn what_the_engine_made_is_stored_below_the_levels_a_run_may_rest_on_and_reaches_no_run() {
    let engine = Installed::new();
    let scene = scene_with(engine.decompiler(true, vec![engine.digest()], "ok", "")).await;
    let _project = handing(&scene);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    assert_eq!(engine.starts(), 1);
    let seen = discovered(&scene);
    let (_, findings, report) = of(&seen, "decompile");
    assert_eq!(report, &Ok(()));
    assert_eq!(findings.len(), 3);
    for finding in findings {
        assert_eq!(finding.source(), Source::Artifact);
        assert!(finding.level() <= EvidenceLevel::Discovered);
        assert_eq!(finding.level(), EvidenceLevel::Inferred);
        assert_eq!(finding.produced_by().component, Some(engine.digest()));
        assert!(finding.found().contains("int fn_"));
    }
    // The description says how much and lists the engine's text apart.
    let registry_description = DecompilerDescriber {
        redact: Arc::new(a2m_core::NoSecrets),
    }
    .describe(findings, DescribeForm::Full);
    assert!(!registry_description.text.contains("int fn_"));
    assert_eq!(registry_description.untrusted.len(), 3);
    // Nothing of what the engine printed reached the run: no result, no
    // trace, no capture of the job holds it.
    let everything = format!(
        "{:?}{:?}{:?}",
        scene.control.results.lock().unwrap(),
        scene.control.traces.lock().unwrap().len(),
        scene.control.captures.lock().unwrap().len(),
    );
    assert!(!everything.contains("fn_"), "{everything}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finding_that_holds_a_registered_secret_is_not_kept() {
    let engine = Installed::new();
    let scene =
        scene_with(engine.decompiler(true, vec![engine.digest()], "secret", "hunter-22-xyz")).await;
    scene
        .vault
        .register(&a2m_core::Secret::new("hunter-22-xyz"));
    let _project = handing(&scene);
    let outcome = scene.discover("app").await;
    assert!(completed(&outcome), "{outcome:?}");
    let seen = discovered(&scene);
    let (_, findings, _) = of(&seen, "decompile");
    assert!(
        findings
            .iter()
            .all(|finding| !finding.found().contains("hunter-22-xyz"))
    );
}
