//! Starting the application a surface declares (decision 0467), on Linux,
//! against the stand as a program of its own: a copy of this test binary
//! that serves the stand's application, or publishes nothing, when started
//! under one of the stand's names. Every test asks the stand's marker, the
//! file a started copy writes its process number into, whether anything
//! was started, and the process table which program that process runs.
//!
//! On Windows and macOS a surface that allows a start, with no application
//! running, stops as `start_not_built` and starts nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::time::Duration;

#[cfg(target_os = "linux")]
use a2m_core::Value;
use a2m_core::{Epoch, FailureClass, NoSecrets, RunId};
use a2m_provider::{
    CheckedPaths, Deadline, Destinations, Posture, PrepareContext, ProgramRef, StepFailure,
    Supervisor, SupervisorLimits,
};
use common::{Folder, compile_at, fixture_copy, registry_with, run, shown};
#[cfg(target_os = "linux")]
use example_desk::Order;
use example_desk::Session;

fn stopped(failure: &StepFailure) -> (FailureClass, String) {
    (
        failure.failure.class(),
        failure.failure.code().as_str().to_owned(),
    )
}

#[cfg(target_os = "linux")]
/// A folder for the copy of this test binary, under the build's own
/// temporary folder, closed to other users, as the provider's check of a
/// program asks of every folder on the way.
fn program_folder(name: &str, mode: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("desk-start-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode)).unwrap();
    folder
}

#[cfg(target_os = "linux")]
/// This test binary, copied into `folder` under `name`, and its digest.
fn copy_of_this_program(folder: &Path, name: &str) -> (PathBuf, String) {
    use sha2::Digest as _;
    let program = folder.join(name);
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let digest = sha2::Sha256::digest(std::fs::read(&program).unwrap());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    (program, format!("sha256:{hex}"))
}

/// A session, a registry over a supervisor the test keeps, and the fixture
/// compiled with its surface naming `program` and the lines `extra`.
struct Started {
    _folder: Folder,
    _session: Session,
    supervisor: Supervisor,
    registry: a2m_provider::Registry,
    compiled_target: a2m_core::CompiledTarget,
    surface: a2m_provider::SurfaceRef,
}

impl Started {
    fn new(name: &str, program: &Path, extra: &str) -> Option<Self> {
        let Some(session) = Session::start().unwrap() else {
            eprintln!("skipped: this machine cannot give the desktop stand a session");
            return None;
        };
        let folder = Folder::new(name);
        let supervisor = Supervisor::new(SupervisorLimits::default());
        let registry = registry_with(folder.path(), session.bus(), supervisor.clone());
        let target = Folder::new(&format!("{name}-target"));
        let program_text = program.to_string_lossy().into_owned();
        let extra = extra.to_owned();
        fixture_copy(target.path(), "target.yaml", move |text| {
            text.replace("/opt/example/bin/example-desk", &program_text)
                .replace(
                    "    service: org.example.Desk\n",
                    &format!("    service: org.example.Desk\n{extra}"),
                )
        });
        let compiled_target = match compile_at(&registry, target.path()) {
            Ok(compiled) => compiled.compiled_target,
            Err(problems) => panic!("{}", shown(&problems)),
        };
        let surface = common::surface_of(&compiled_target, &program.to_string_lossy());
        Some(Self {
            _folder: folder,
            _session: session,
            supervisor,
            registry,
            compiled_target,
            surface,
        })
    }

    /// Prepares the driver as the engine does, with the supervisor scoped
    /// to the programs the compiled target declares, and reads the order.
    async fn read(&self, posture: Posture) -> Result<a2m_core::Values, StepFailure> {
        let handle = self.registry.driver(&common::mode()).unwrap();
        let supervisor = handle.supervisor(
            self.surface
                .surface
                .programs
                .iter()
                .map(ProgramRef::from)
                .collect(),
        );
        let destinations = Destinations::new([self.surface.surface.address.clone()]);
        let cx = PrepareContext {
            run: RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
            surface: &self.surface,
            compiled_target: &self.compiled_target,
            posture,
            deadline: Deadline::after(Duration::from_secs(8)),
            redact: &NoSecrets,
            destinations: &destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let mut prepared = handle.prepare(&cx, None).await?;
        let steps = common::binding_of(&self.compiled_target, "example_order_get")
            .act
            .clone();
        let result = run(&handle, &mut prepared, &steps, Duration::from_secs(10)).await;
        handle.release(prepared, result.is_err()).await;
        result
    }
}

#[cfg(target_os = "linux")]
/// The process a started copy wrote into its marker, if it was started.
fn started_process(program: &Path) -> Option<u32> {
    std::fs::read_to_string(example_desk::marker(program))
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

#[cfg(target_os = "linux")]
fn runs(process: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{process}/exe")).ok()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_surface_that_allows_it_has_its_program_started_once_and_read() {
    let folder = program_folder("app", 0o700);
    let (program, digest) = copy_of_this_program(&folder, example_desk::STARTED);
    let Some(scene) = Started::new(
        "start-app",
        &program,
        &format!("    start: true\n    digest: \"{digest}\"\n"),
    ) else {
        return;
    };
    let values = scene.read(Posture::Live).await.unwrap();
    let order = Order::example();
    assert_eq!(
        values.get("customer").and_then(Value::as_text),
        Some(order.customer.as_str())
    );
    // The stand says it was started, by the copy the surface names, under
    // the supervisor the pack's context scopes.
    let process = started_process(&program).expect("the copy was started");
    assert_eq!(runs(process).as_deref(), Some(program.as_path()));
    let listed = scene.supervisor.list();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].kind.as_str(), "desktop_application");
    assert_eq!(listed[0].process, process);
    // A second run finds it running and starts nothing.
    std::fs::remove_file(example_desk::marker(&program)).unwrap();
    scene.read(Posture::Live).await.unwrap();
    assert_eq!(started_process(&program), None);
    assert_eq!(scene.supervisor.list().len(), 1);
    // The engine's stop ends it.
    assert!(scene.supervisor.shutdown().await.complete);
    assert_eq!(runs(process), None);
    let _ = std::fs::remove_dir_all(&folder);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn nothing_is_started_without_leave_in_a_discovery_or_against_its_digest() {
    let folder = program_folder("refused", 0o700);
    let (program, digest) = copy_of_this_program(&folder, example_desk::STARTED);
    // Without `start`, a program nobody runs is not started.
    let Some(scene) = Started::new(
        "start-none",
        &program,
        &format!("    digest: \"{digest}\"\n"),
    ) else {
        return;
    };
    let failure = scene.read(Posture::Live).await.unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Busy, "application_not_running".to_owned())
    );
    assert_eq!(started_process(&program), None);
    drop(scene);
    // With it, a discovery starts nothing.
    let scene = Started::new(
        "start-discovery",
        &program,
        &format!("    start: true\n    digest: \"{digest}\"\n"),
    )
    .unwrap();
    let failure = scene.read(Posture::Discovery).await.unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Busy, "application_not_running".to_owned())
    );
    assert_eq!(started_process(&program), None);
    assert!(scene.supervisor.list().is_empty());
    drop(scene);
    // A program whose bytes are not the digest the surface states is not
    // started: the supervisor measures it first.
    let other = format!("sha256:{}", "0".repeat(64));
    let scene = Started::new(
        "start-digest",
        &program,
        &format!("    start: true\n    digest: \"{other}\"\n"),
    )
    .unwrap();
    let failure = scene.read(Posture::Live).await.unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Policy, "effect_not_declared".to_owned())
    );
    assert_eq!(started_process(&program), None);
    assert!(scene.supervisor.list().is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_program_another_user_can_replace_is_not_started() {
    let folder = program_folder("open", 0o777);
    let (program, digest) = copy_of_this_program(&folder, example_desk::STARTED);
    let Some(scene) = Started::new(
        "start-open",
        &program,
        &format!("    start: true\n    digest: \"{digest}\"\n"),
    ) else {
        return;
    };
    let failure = scene.read(Posture::Live).await.unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Policy, "program_refused".to_owned())
    );
    assert_eq!(started_process(&program), None);
    assert!(scene.supervisor.list().is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_started_program_that_publishes_nothing_is_stopped_by_the_deadline() {
    let folder = program_folder("mute", 0o700);
    let (program, digest) = copy_of_this_program(&folder, example_desk::MUTE);
    let Some(scene) = Started::new(
        "start-mute",
        &program,
        &format!("    start: true\n    digest: \"{digest}\"\n"),
    ) else {
        return;
    };
    let failure = scene.read(Posture::Live).await.unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Busy, "application_not_started".to_owned())
    );
    let process = started_process(&program).expect("the copy was started");
    // The pack stopped what it started and could not prove.
    assert!(scene.supervisor.list().is_empty());
    assert_eq!(runs(process), None);
    let _ = std::fs::remove_dir_all(&folder);
}

#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn a_start_is_not_built_here_and_starts_nothing() {
    let folder = Folder::new("start-elsewhere");
    let program = std::env::current_exe().unwrap();
    let Some(scene) = Started::new("start-elsewhere", &program, "    start: true\n") else {
        return;
    };
    let failure = scene.read(Posture::Live).await.unwrap_err();
    assert_eq!(
        stopped(&failure),
        (FailureClass::Busy, "start_not_built".to_owned())
    );
    assert!(scene.supervisor.list().is_empty());
    drop(folder);
}
