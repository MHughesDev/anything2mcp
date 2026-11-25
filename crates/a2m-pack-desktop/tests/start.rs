//! Starting the application a surface declares (decisions 0467 and 0480),
//! on Linux and Windows, against the stand as a program of its own: a copy
//! of this test binary that serves the stand's application, or publishes
//! nothing and shows no window, when started under one of the stand's
//! names. Every test asks the stand's marker, the file a started copy
//! writes its process number into, whether anything was started, and the
//! system which program that process runs.
//!
//! On macOS a surface that allows a start, with no application running,
//! stops as `start_not_built` and starts nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
#[cfg(any(target_os = "linux", windows))]
use std::path::PathBuf;
use std::time::Duration;

#[cfg(any(target_os = "linux", windows))]
use a2m_core::Value;
use a2m_core::{Epoch, FailureClass, NoSecrets, RunId};
use a2m_provider::{
    CheckedPaths, Deadline, Destinations, Posture, PrepareContext, ProgramRef, StepFailure,
    Supervisor, SupervisorLimits,
};
use common::{Folder, compile_at, fixture_copy, registry_with, run, shown, yaml_text};
#[cfg(any(target_os = "linux", windows))]
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
/// temporary folder, closed to other users, or open to them when `open`,
/// as the provider's check of a program asks of every folder on the way.
fn program_folder(name: &str, open: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("desk-start-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let mode = if open { 0o777 } else { 0o700 };
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode)).unwrap();
    folder
}

#[cfg(windows)]
/// A folder for the copy of this test binary, under the user's own
/// temporary folder, which only the user and the system may change, as the
/// provider's check of a program asks of every folder on the way. `open`
/// is carried out on the program's file instead ([`open_to_everyone`]).
fn program_folder(name: &str, _open: bool) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("desk-start-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    // The folder as a person writes it: resolved, and on a drive without
    // the verbatim prefix the system's resolution gives it.
    let folder = std::fs::canonicalize(&folder).unwrap();
    let written = folder
        .to_str()
        .and_then(|text| text.strip_prefix(r"\\?\"))
        .map(PathBuf::from);
    written.unwrap_or(folder)
}

#[cfg(windows)]
/// Lets every user write `path`, by an entry granting the well-known group
/// Everyone (`S-1-1-0`) the right to write, set with the system's own
/// `icacls`, started by its path.
fn open_to_everyone(path: &Path) {
    let system =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let made = std::process::Command::new(system.join("System32").join("icacls.exe"))
        .arg(path)
        .args(["/grant", "*S-1-1-0:(W)"])
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "icacls did not grant the right: {}",
        String::from_utf8_lossy(&made.stdout)
    );
}

#[cfg(any(target_os = "linux", windows))]
/// This test binary, copied into `folder` under `name` (with `.exe` after
/// it on Windows), and its digest.
fn copy_of_this_program(folder: &Path, name: &str) -> (PathBuf, String) {
    use sha2::Digest as _;
    let program = folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let digest = sha2::Sha256::digest(std::fs::read(&program).unwrap());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    (program, format!("sha256:{hex}"))
}

/// The name of the application a copy at `program` serves: on Linux the
/// stand's, which it publishes on the bus; on Windows and macOS the name of
/// its program's file, which is what the pack reads there.
fn application_of(program: &Path) -> String {
    if cfg!(target_os = "linux") {
        "example-desk".to_owned()
    } else {
        program
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_owned()
    }
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
        let program_text = yaml_text(&program.to_string_lossy());
        let extra = extra.to_owned();
        let application = application_of(program);
        fixture_copy(target.path(), "target.yaml", move |text| {
            text.replace("/opt/example/bin/example-desk", &program_text)
                .replace(
                    "    application: example-desk\n",
                    &format!("    application: {application}\n"),
                )
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
            // On Windows a freshly copied program may be scanned by the
            // system before it runs.
            deadline: Deadline::after(Duration::from_secs(if cfg!(windows) { 20 } else { 8 })),
            redact: &NoSecrets,
            destinations: &destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
            confirmed: a2m_provider::ConfirmedReads::none(),
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

#[cfg(any(target_os = "linux", windows))]
/// The process a started copy wrote into its marker, if it was started.
fn started_process(program: &Path) -> Option<u32> {
    std::fs::read_to_string(example_desk::marker(program))
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

#[cfg(any(target_os = "linux", windows))]
/// The program the process runs, resolved, or `None` when it has ended.
fn runs(process: u32) -> Option<PathBuf> {
    example_desk::program_of(process).and_then(|program| std::fs::canonicalize(program).ok())
}

#[cfg(any(target_os = "linux", windows))]
fn resolved(program: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(program).ok()
}

#[cfg(any(target_os = "linux", windows))]
#[tokio::test]
async fn a_surface_that_allows_it_has_its_program_started_once_and_read() {
    let folder = program_folder("app", false);
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
    assert_eq!(runs(process), resolved(&program));
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

#[cfg(any(target_os = "linux", windows))]
#[tokio::test]
async fn nothing_is_started_without_leave_in_a_discovery_or_against_its_digest() {
    let folder = program_folder("refused", false);
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

#[cfg(any(target_os = "linux", windows))]
#[tokio::test]
async fn a_program_another_user_can_replace_is_not_started() {
    let folder = program_folder("open", true);
    let (program, digest) = copy_of_this_program(&folder, example_desk::STARTED);
    #[cfg(windows)]
    open_to_everyone(&program);
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

#[cfg(any(target_os = "linux", windows))]
#[tokio::test]
async fn a_started_program_that_publishes_nothing_is_stopped_by_the_deadline() {
    let folder = program_folder("mute", false);
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

#[cfg(not(any(target_os = "linux", windows)))]
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
