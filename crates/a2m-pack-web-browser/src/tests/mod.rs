//! Tests against a real browser and the test application.
//!
//! A test that needs a browser fails with `browser_unavailable` when none
//! is found; it never passes by skipping. On a machine whose browser is
//! not in a known place, or whose tests run as the superuser, the test
//! environment says so through the two variables of `test_support`.
//!
//! The tests are inside the crate because a few of them must stand for the
//! page itself: a page that writes by its own script, or a browser killed
//! with a call pending. Everything else goes through the public interface.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{
    AccountName, Digest, Environment, Failure, FailureClass, Identity, NoSecrets, Redact,
    TargetName,
};
use a2m_logs::LogSettings;
use a2m_provider::{
    Access, HostServices, Pack, PackContext, ProviderSettings, Registrar, RegistryBuilder,
    RootName, Roots, Supervisor, SupervisorLimits,
};
use example_app::harness::TestApp;

use crate::{
    Address, Browser, BrowserError, BrowserHealth, BrowserProfileKey, BrowserSettings, Browsers,
    By, Deadline, Dispatch, LaunchReason, Look, Origins, PROFILE_ROOT, PageEnvironment, Query,
    Visibility, codes, test_support,
};

mod acting;
mod cases;
mod escapes;
mod hanging;
mod launching;
mod looking;
mod memory;
mod observing;
mod quiet;
mod state;
mod warm;
mod watching;
mod window;

/// A folder under the system's temporary folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-browser-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A pack that registers nothing and keeps the context it is handed: the
/// one way a test gets a scoped supervisor and a profile root.
struct Capture {
    manifest: &'static str,
    context: Mutex<Option<PackContext>>,
}

impl Pack for Capture {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, cx: &PackContext, _to: &mut Registrar<'_>) -> Result<(), Failure> {
        *self.context.lock().unwrap() = Some(cx.clone());
        Ok(())
    }
}

/// A redactor that knows one secret.
struct Knows(&'static str);

impl Redact for Knows {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, "[redacted]")
    }
}

struct Stand {
    folder: Folder,
    supervisor: Supervisor,
    browsers: Browsers,
    context: PackContext,
}

impl Stand {
    /// A stand whose pack declares the browser of the test environment,
    /// and the programs in `also`.
    fn new(name: &str, settings: BrowserSettings, also: &[&Path]) -> Self {
        Self::with_redactor(name, settings, also, Arc::new(NoSecrets))
    }

    fn with_redactor(
        name: &str,
        settings: BrowserSettings,
        also: &[&Path],
        redact: Arc<dyn Redact>,
    ) -> Self {
        let folder = Folder::new(name);
        let profiles = folder.0.join("profiles");
        let logs = folder.0.join("logs");
        std::fs::create_dir_all(&profiles).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        let mut roots = Roots::none();
        roots
            .add(
                RootName::new(PROFILE_ROOT).unwrap(),
                &profiles,
                Access::Write,
            )
            .unwrap();
        let supervisor = Supervisor::new(SupervisorLimits::default());
        let host = HostServices {
            supervisor: supervisor.clone(),
            roots,
            redact: Arc::clone(&redact),
            logs_and_traces: a2m_logs::start(&LogSettings::default(), redact, &logs).unwrap(),
        };
        let processes: Vec<String> = also
            .iter()
            .map(|program| format!("{{ path = '{}' }}", program.display()))
            .collect();
        let manifest = format!(
            "manifest_format = 1\ncontract = 1\n\n[provider]\nname = \"web\"\nversion = \"0.1.0\"\n\
             publisher = \"Example\"\nkind = \"pack\"\n\n[effects]\nwrite_roots = [\"{PROFILE_ROOT}\"]\n\
             network = \"surfaces\"\nprocesses = [{}]\n{}",
            processes.join(", "),
            test_support::requirement()
        );
        let pack = Capture {
            manifest: Box::leak(manifest.into_boxed_str()),
            context: Mutex::new(None),
        };
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        builder.add_pack(&pack);
        let (_registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        let context = pack.context.lock().unwrap().clone().unwrap();
        Self {
            browsers: Browsers::new(&context, settings).unwrap(),
            context,
            supervisor,
            folder,
        }
    }

    /// A stand with the browser of the test environment.
    fn with_browser(name: &str) -> Self {
        Self::new(name, test_support::settings(), &[])
    }

    /// The profile's browser. With no browser on the machine this is where
    /// a test fails, by the named failure.
    async fn open(&self, profile: &BrowserProfileKey) -> Browser {
        must(self.browsers.open(profile, Visibility::Background).await)
    }

    /// Closes everything and checks that nothing is left: no subsystem in
    /// the supervisor's list, and, where the operating system lets a test
    /// look, no process that names the stand's folder.
    async fn end(self) {
        let cleanup = self.browsers.shutdown().await;
        assert!(cleanup.complete, "{cleanup:?}");
        assert!(self.supervisor.list().is_empty());
        assert!(self.browsers.list().is_empty());
        // A process that was just ended takes a moment to disappear from
        // the system's list: the test looks until it has, within a bound.
        let started = std::time::Instant::now();
        loop {
            let left = processes_naming(&self.folder.0);
            if left.is_empty() {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "processes are left: {left:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// The processes whose command line names the folder. Linux only; on the
/// other systems the supervisor's own cleanup report is the proof.
fn processes_naming(folder: &Path) -> Vec<u32> {
    let mut found = Vec::new();
    if !cfg!(target_os = "linux") {
        return found;
    }
    let needle = folder.display().to_string();
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(process) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if process == std::process::id() {
            continue;
        }
        if let Ok(line) = std::fs::read(entry.path().join("cmdline"))
            && String::from_utf8_lossy(&line).contains(&needle)
        {
            found.push(process);
        }
    }
    found
}

/// Unwraps, naming the failure class and code when there is one.
#[track_caller]
fn must<T>(result: Result<T, BrowserError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => match error.failure() {
            Some(failure) => panic!("{}/{}: {failure}", failure.class(), failure.code()),
            None => panic!("{error}"),
        },
    }
}

fn profile(identity: &str) -> BrowserProfileKey {
    BrowserProfileKey {
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        identity: Identity::Account(AccountName::new(identity).unwrap()),
    }
}

fn soon() -> Deadline {
    Deadline::after(Duration::from_secs(30))
}

fn environment() -> PageEnvironment {
    PageEnvironment::default()
}

/// The origin of the test application, and the pages a test gives as
/// `data:` addresses for the controls the application lacks.
fn origins(app: &TestApp) -> Origins {
    Origins::of([&Address::new(&app.url("/")).unwrap()]).with_data_pages()
}

/// Only the pages a test gives as `data:` addresses.
fn data_pages() -> Origins {
    Origins::default().with_data_pages()
}

/// The form of one line item of the test application.
const FORM: &str = "/orders/A-100/items/L-101";

fn textbox(name: &str) -> Query {
    Query::role("textbox", name)
}

fn css(selector: &str) -> Query {
    Query::new(By::Css {
        selector: selector.to_owned(),
    })
}

/// A page of the test's own, as an address the browser takes without a
/// server. The tests use it for controls the test application lacks.
fn page_of(html: &str) -> String {
    let mut address = String::from("data:text/html;charset=utf-8,");
    for byte in html.bytes() {
        if byte.is_ascii_alphanumeric() {
            address.push(char::from(byte));
        } else {
            address.push_str(&format!("%{byte:02X}"));
        }
    }
    address
}

/// Ends a process from outside, as a crash would: that one process, with
/// no notice. Its children are not named here; a browser's own children end
/// when it does, and the test then asks whether any process is left.
fn kill(process: u32) {
    let done = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &process.to_string()])
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &process.to_string()])
            .output()
    }
    .unwrap();
    assert!(
        done.status.success(),
        "ending process {process} from outside failed with {:?}: {} {}",
        done.status.code(),
        String::from_utf8_lossy(&done.stdout).trim(),
        String::from_utf8_lossy(&done.stderr).trim()
    );
}

#[test]
fn every_code_has_one_class_and_the_conversions_use_them() {
    let mut seen = std::collections::BTreeSet::new();
    for (_, code) in codes::ALL {
        assert!(seen.insert(*code), "`{code}` is listed twice");
    }
    // This crate's codes, and the provider contract's it gives as they are.
    let book = a2m_core::CodeBook::new(&[codes::TABLE, a2m_provider::codes::TABLE]).unwrap();
    let class_of =
        |code: &a2m_core::FailureCode| book.explain(code.as_str()).map(|entry| entry.class);
    let cases = [
        (
            BrowserError::LaunchFailed(LaunchReason::NotInstalled),
            Some((FailureClass::Busy, codes::BROWSER_UNAVAILABLE)),
        ),
        (
            BrowserError::LaunchFailed(LaunchReason::NotOwned("x".to_owned())),
            Some((FailureClass::UnknownVariant, codes::BROWSER_NOT_OWNED)),
        ),
        (
            BrowserError::LaunchFailed(LaunchReason::MemoryShort(crate::MemoryShort {
                held: None,
                ceiling: 1,
                retry: Duration::from_secs(1),
                why: Some("x".to_owned()),
            })),
            Some((FailureClass::Busy, codes::MEMORY_SHORT)),
        ),
        (
            BrowserError::LaunchFailed(LaunchReason::NoAnswer("x".to_owned())),
            Some((FailureClass::Transient, codes::TIMEOUT)),
        ),
        (
            BrowserError::Disconnected {
                dispatch: Dispatch::NotSent,
            },
            Some((FailureClass::Transient, codes::CONNECTION_LOST)),
        ),
        (
            BrowserError::Crashed {
                dispatch: Dispatch::NotSent,
            },
            Some((FailureClass::Transient, codes::CONNECTION_LOST)),
        ),
        (
            BrowserError::Timeout {
                dispatch: Dispatch::NotSent,
            },
            Some((FailureClass::Transient, codes::TIMEOUT)),
        ),
        (
            BrowserError::PasswordField,
            Some((FailureClass::Policy, codes::PASSWORD_READ_REFUSED)),
        ),
        // Sent, or not known: only the crate above can say what it means.
        (
            BrowserError::Disconnected {
                dispatch: Dispatch::Sent,
            },
            None,
        ),
        (
            BrowserError::Crashed {
                dispatch: Dispatch::Unknown,
            },
            None,
        ),
        (
            BrowserError::Timeout {
                dispatch: Dispatch::Sent,
            },
            None,
        ),
        (
            BrowserError::ForeignOrigin("https://other.example".to_owned()),
            Some((FailureClass::Policy, codes::SURFACE_NOT_DECLARED)),
        ),
        (BrowserError::NotFound, None),
        (BrowserError::Ambiguous { count: 2 }, None),
    ];
    for (error, expected) in cases {
        let failure = error.failure();
        assert_eq!(
            failure
                .as_ref()
                .map(|failure| (failure.class(), failure.code().clone())),
            expected,
            "{error}"
        );
        if let Some(failure) = failure {
            assert_eq!(class_of(failure.code()), Some(failure.class()));
        }
    }
    assert!(Address::new("https://app.example.com/orders?x=1").is_ok());
    for refused in [
        "app.example.com",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://",
        "https://user:pass@app.example.com/",
        "https://app.example.com/a b",
        "https://app.example.com\\@other.example.com/",
    ] {
        let failure = Address::new(refused).unwrap_err();
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Input, &codes::ADDRESS_INVALID),
            "{refused}"
        );
    }
    let _ = (Digest::of(b""), BrowserHealth::Idle);
}
