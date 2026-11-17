//! What the end-to-end tests share: a project folder made in a temporary
//! directory from the target files under `tests/projects/`, a person
//! surface a test answers for, and an engine opened as the product opens
//! it, with every first-party pack.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

pub mod proxy;
pub mod service;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, BoxFuture, Paths, PersonSurface, PresenceAsk, PresenceProof,
    SessionAsk, SignInAsk, StorePlace, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Failure, FailureClass};
use a2m_policy::PresenceCheck;
use a2m_telemetry::{Clock, SystemClock};
use a2m_vault::{KeyStore, MemoryKeyStore};
use anything2mcp::cli::{Finished, run_on};
use anything2mcp::{Engine, EngineOptions};
use serde_json::Value as Json;

/// The operating-system user of every test.
pub const USER: &str = "pat";

/// A folder under the system's temporary folder, removed when dropped; or
/// a folder somebody else owns, which is left alone.
pub struct Folder(PathBuf, bool);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "anything2mcp-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        // The roots resolve a folder once; a test compares paths with the
        // resolved form.
        Self(path.canonicalize().unwrap(), true)
    }

    /// A folder that exists and belongs to somebody else, such as a test
    /// stand's own root.
    pub fn of_another(path: &Path) -> Self {
        Self(path.canonicalize().unwrap(), false)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        if self.1 {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// The folder of one of this crate's projects of target files.
pub fn project_source(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("projects")
        .join(name)
}

/// Copies a folder of text files, replacing each `(from, to)` in every
/// file's text.
pub fn copy_changed(from: &Path, to: &Path, changes: &[(&str, &str)]) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap().path();
        let target = to.join(entry.file_name().unwrap());
        if entry.is_dir() {
            copy_changed(&entry, &target, changes);
        } else {
            let mut text = std::fs::read_to_string(&entry).unwrap();
            for (old, new) in changes {
                text = text.replace(old, new);
            }
            std::fs::write(&target, text).unwrap();
        }
    }
}

/// The person surface of a test: it keeps the desk, notes what it was
/// asked, and answers the presence prompt as the test set it.
pub struct Surface {
    desk: Mutex<Option<Arc<ApprovalDesk>>>,
    pub approvals: Mutex<Vec<ApprovalAsk>>,
    pub presence_asked: AtomicU32,
    pub person_present: Mutex<bool>,
    clock: Arc<dyn Clock>,
}

impl Surface {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            desk: Mutex::new(None),
            approvals: Mutex::new(Vec::new()),
            presence_asked: AtomicU32::new(0),
            person_present: Mutex::new(true),
            clock: Arc::new(SystemClock::new()),
        })
    }

    /// The approval desk of the engine opened last.
    pub fn desk(&self) -> Arc<ApprovalDesk> {
        self.desk
            .lock()
            .unwrap()
            .clone()
            .expect("the kernel handed the desk over")
    }

    pub fn last_ask(&self) -> ApprovalAsk {
        self.approvals
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("an approval was asked for")
    }
}

impl PersonSurface for Surface {
    fn take_desk(&self, desk: ApprovalDesk) {
        *self.desk.lock().unwrap() = Some(Arc::new(desk));
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let run = ask.run;
            self.approvals.lock().unwrap().push(ask);
            Ok(SurfaceLink {
                url: Some(format!("http://127.0.0.1/approve/{run}")),
                text: "Open the approval page.".to_owned(),
            })
        })
    }

    fn ask_sign_in(&self, _ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: "Sign in at the engine's own window.".to_owned(),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            self.presence_asked.fetch_add(1, Ordering::SeqCst);
            if *self.person_present.lock().unwrap() {
                Ok(PresenceProof::new(
                    PresenceCheck::OsPrompt,
                    ask.principal,
                    self.clock.now(),
                ))
            } else {
                Err(Failure::new(
                    FailureClass::Policy,
                    a2m_policy::PRESENCE_REQUIRED,
                    "Nobody answered the presence prompt.",
                    "Answer the prompt at the machine.",
                ))
            }
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async move { PersonAnswer::NoAnswer })
    }
}

/// One machine of a test: a home directory, a project folder, a person,
/// and the key store the machine keeps between engines.
pub struct Machine {
    pub home: Folder,
    pub project: Folder,
    pub surface: Arc<Surface>,
    /// `None` is the machine's own, as its settings choose it.
    pub key_store: Option<Arc<dyn KeyStore>>,
    pub place: StorePlace,
    passphrase: Option<String>,
}

/// A made-up passphrase for the vault of a test machine.
pub const PASSPHRASE: &str = "secret-for-tests-vault-passphrase";

impl Machine {
    /// A machine whose project holds the target files of `project`, with
    /// `changes` made to their text. The store is in memory.
    pub fn new(project: &str, changes: &[(&str, &str)]) -> Self {
        Self::in_folder(project, changes, Folder::new("project"))
    }

    /// The same, with the project in a folder that exists already: the
    /// target files are added to what the folder holds.
    pub fn in_folder(project: &str, changes: &[(&str, &str)], folder: Folder) -> Self {
        let home = Folder::new("home");
        copy_changed(&project_source(project), folder.path(), changes);
        Self {
            home,
            project: folder,
            surface: Surface::new(),
            key_store: Some(Arc::new(MemoryKeyStore::new())),
            place: StorePlace::Memory,
            passphrase: None,
        }
    }

    /// The same machine with its store on disk, so that a second engine
    /// finds what the first one left.
    pub fn on_disk(mut self) -> Self {
        self.place = StorePlace::Disk;
        self
    }

    /// The same machine as the real program finds it: the store on disk,
    /// and the vault's root key from a passphrase, which the installation's
    /// settings file chooses and the environment gives. This is the route
    /// a machine with no key store of its own takes.
    pub fn with_passphrase(mut self) -> Self {
        let installation = Paths::under(self.home.path()).installation_config;
        std::fs::create_dir_all(&installation).unwrap();
        std::fs::write(
            installation.join("settings.toml"),
            "[vault]\nbackend = \"passphrase\"\n",
        )
        .unwrap();
        self.place = StorePlace::Disk;
        self.key_store = None;
        self.passphrase = Some(PASSPHRASE.to_owned());
        self
    }

    /// The real program, started in this machine's project folder.
    pub fn program(&self, args: &[&str]) -> std::process::Command {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_anything2mcp"));
        command
            .args(args)
            .current_dir(self.project.path())
            .env_remove(anything2mcp::PASSPHRASE_VARIABLE)
            .envs(self.environment())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command
    }

    pub fn environment(&self) -> BTreeMap<String, String> {
        let mut environment = self.base_environment();
        if let Some(passphrase) = &self.passphrase {
            environment.insert(
                anything2mcp::PASSPHRASE_VARIABLE.to_owned(),
                passphrase.clone(),
            );
        }
        environment
    }

    fn base_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "A2M_HOME".to_owned(),
                self.home.path().display().to_string(),
            ),
            ("USER".to_owned(), USER.to_owned()),
        ])
    }

    /// Opens an engine on this machine, as the product does.
    pub async fn open(&self) -> World {
        self.open_at(None).await
    }

    /// The same, with the machine's clock replaced: an engine started at
    /// a later time than the one before it.
    pub async fn open_at(&self, clock: Option<Arc<dyn Clock>>) -> World {
        let mut options = EngineOptions::of_machine(
            Paths::under(self.home.path()),
            self.environment(),
            Arc::clone(&self.surface) as Arc<dyn PersonSurface>,
        );
        options.project = Some(self.project.path().to_path_buf());
        options.place = self.place;
        options.key_store.clone_from(&self.key_store);
        if let Some(clock) = clock {
            options.clock = clock;
        }
        let engine = Engine::open(options).await.expect("the settings load");
        assert!(
            engine.runner_problem().is_none(),
            "{:?}",
            engine.runner_problem()
        );
        World {
            engine,
            surface: Arc::clone(&self.surface),
        }
    }
}

/// An open engine and the person at its machine.
pub struct World {
    pub engine: Engine,
    pub surface: Arc<Surface>,
}

/// What one command gave, with its standard output read as JSON.
pub struct Ran {
    pub finished: Finished,
    pub json: Json,
}

impl Ran {
    pub fn result(&self) -> &Json {
        &self.json["result"]
    }

    pub fn error(&self) -> &Json {
        &self.json["error"]
    }
}

impl World {
    async fn run(&self, args: &[&str], is_agent: bool) -> Ran {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let finished = run_on(&self.engine, &args, true, is_agent).await;
        let json = serde_json::from_str(finished.stdout.trim())
            .unwrap_or_else(|_| panic!("not one JSON object: {finished:?}"));
        Ran { finished, json }
    }

    /// Runs one command line as an agent would: output that is not a
    /// terminal, and nobody at it.
    pub async fn agent(&self, args: &[&str]) -> Ran {
        self.run(args, true).await
    }

    /// Runs one command line as a person at a terminal.
    pub async fn person(&self, args: &[&str]) -> Ran {
        self.run(args, false).await
    }

    /// The standard output of a command that prints text, such as help.
    pub async fn agent_text(&self, args: &[&str]) -> String {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        run_on(&self.engine, &args, true, true).await.stdout
    }

    /// The same, and the command must succeed.
    pub async fn ok(&self, args: &[&str]) -> Json {
        let ran = self.agent(args).await;
        assert_eq!(ran.finished.code, 0, "{args:?}: {:?}", ran.finished);
        ran.json["result"].clone()
    }

    pub async fn close(self) {
        self.engine.close().await;
    }
}
