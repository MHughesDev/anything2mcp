//! The engine in this process: the settings, the kernel, the provider
//! registry with every first-party pack, and the in-process runner loop.
//!
//! This is the library facade for a host that embeds the engine, and what
//! a one-shot command opens. The order is fixed: settings, the store and
//! the vault ([`Kernel::prepare`]), the provider registry (built from the
//! redactor and telemetry the first step made), the rest of the kernel
//! ([`Kernel::open`]), and the runner loop.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use a2m_actions::{
    ActionRegistry, Caller, CallerCredential, CloseReason, FrontDoor, Kernel, KernelParts, Paths,
    PersonSurface, Principal, Request, Response, Settings, Sources, StorePlace,
};
use a2m_core::{Failure, FailureClass, NoSecrets, Secret};
use a2m_provider::{
    Access, HostServices, ProviderSettings, RegistryBuilder, RootName, Roots, Supervisor,
    SupervisorLimits,
};
use a2m_telemetry::{Clock, LogLevel, SystemClock, Telemetry};
use a2m_vault::{
    Binding, BoxFuture as VaultFuture, InstallationName, KeyMaterial, KeyStore, OsKeyStore,
    OsRandom, PassphraseKeyStore, PassphraseRoute, PassphraseSalt, RandomSource, StoreDescription,
    StoreHealth, StoreStrength, Wrapped,
};

use crate::packs;

/// The environment variable that holds the vault's passphrase, when the
/// installation's settings choose the passphrase route. It is read once at
/// start and never shown.
pub const PASSPHRASE_VARIABLE: &str = "A2M_VAULT_PASSPHRASE";

/// The name of the file, in the data directory, that keeps the salt of the
/// passphrase route. The salt is not a secret.
const SALT_FILE: &str = "vault-salt";

/// The name of the installation's settings file, in the installation's
/// configuration directory.
const INSTALLATION_SETTINGS_FILE: &str = "settings.toml";

/// How many jobs the in-process runner loop holds at once.
const RUNNER_SLOTS: u32 = 4;

/// The roots the host names for the packs, and where each is.
const ROOT_PROJECT: &str = "project";
const ROOT_FILES: &str = "files";
const ROOT_COMMAND_RUNS: &str = "command_runs";
const ROOT_PROFILES: &str = "profiles";

/// What an engine is opened from. [`EngineOptions::of_machine`] fills it for
/// this machine; a test replaces the parts it wants to hold.
pub struct EngineOptions {
    /// The on-disk layout.
    pub paths: Paths,
    /// The project folder, when the command was run inside one.
    pub project: Option<PathBuf>,
    /// The process environment: `A2M_<GROUP>__<KEY>` variables are settings.
    pub environment: BTreeMap<String, String>,
    /// Settings given as flags, as key and written value.
    pub flags: Vec<(String, String)>,
    /// Where the store is kept.
    pub place: StorePlace,
    /// Where the vault's root key is kept. `None` is this machine's own:
    /// the operating system's key store, or the passphrase route when the
    /// installation's settings choose it.
    pub key_store: Option<Arc<dyn KeyStore>>,
    /// Where time comes from.
    pub clock: Arc<dyn Clock>,
    /// Where random bytes come from.
    pub random: Arc<dyn RandomSource>,
    /// The operating-system user the engine runs as.
    pub os_user: String,
    /// How a person is reached.
    pub person: Arc<dyn PersonSurface>,
}

impl std::fmt::Debug for EngineOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineOptions")
            .field("project", &self.project)
            .field("place", &self.place)
            .finish_non_exhaustive()
    }
}

impl EngineOptions {
    /// The options of this machine: the store on disk under `paths`, the
    /// machine's clock and randomness, and its own key store.
    #[must_use]
    pub fn of_machine(
        paths: Paths,
        environment: BTreeMap<String, String>,
        person: Arc<dyn PersonSurface>,
    ) -> Self {
        let os_user = os_user(&environment);
        Self {
            paths,
            project: None,
            environment,
            flags: Vec::new(),
            place: StorePlace::Disk,
            key_store: None,
            clock: Arc::new(SystemClock::new()),
            random: Arc::new(OsRandom),
            os_user,
            person,
        }
    }
}

/// The name of the operating-system user, from the environment the process
/// was started with. In one process the caller is the process itself, so
/// the name only labels the one caller there is.
fn os_user(environment: &BTreeMap<String, String>) -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .filter_map(|name| environment.get(*name))
        .find(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| "local".to_owned())
}

/// An open engine: the kernel, and the one caller a process on this
/// machine is.
pub struct Engine {
    kernel: Kernel,
    principal: Principal,
    project: Option<PathBuf>,
    /// Why the runner loop is not running, when it is not.
    runner_problem: Option<Failure>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("kernel", &self.kernel)
            .finish_non_exhaustive()
    }
}

/// The settings sources of a process: the files the paths name, the
/// project's file, the environment, the flags, and every first-party
/// pack's manifest, whose settings are the schema of its group.
fn sources(options: &EngineOptions) -> Result<Sources, Failure> {
    let mut sources = Sources::read(
        &options.paths,
        options.project.as_deref(),
        options.environment.clone(),
    )?;
    for pack in packs::first_party() {
        // A manifest that does not parse is reported by the registry
        // builder, as a named problem of that pack; the settings loader
        // then knows no group for it.
        if let Ok(manifest) = a2m_provider::Manifest::parse(pack.manifest) {
            sources = sources.with_pack(Arc::new(manifest));
        }
    }
    for (key, value) in &options.flags {
        sources = sources.with_flag(key, value);
    }
    Ok(sources)
}

fn root(roots: &mut Roots, name: &str, folder: &Path, make: bool) {
    if make && std::fs::create_dir_all(folder).is_err() {
        return;
    }
    if let Ok(name) = RootName::new(name) {
        // A folder that does not exist is left out: a pack that needs the
        // root learns it when it checks a path, with a named failure.
        let _ = roots.add(name, folder, Access::Write);
    }
}

/// The named roots the packs may be given: the project folder, under the
/// names `project` and `files`, and two folders of the data directory the
/// engine owns.
fn roots(paths: &Paths, project: Option<&Path>, place: StorePlace) -> Roots {
    let mut roots = Roots::none();
    if let Some(project) = project {
        root(&mut roots, ROOT_PROJECT, project, false);
        root(&mut roots, ROOT_FILES, project, false);
    }
    // A store in memory leaves nothing behind, so the engine's own folders
    // are made only beside a store on disk or when they exist already.
    let make = place == StorePlace::Disk;
    root(
        &mut roots,
        ROOT_COMMAND_RUNS,
        &paths.data.join(ROOT_COMMAND_RUNS),
        make,
    );
    root(&mut roots, ROOT_PROFILES, &paths.profiles, make);
    roots
}

/// A key store that does not answer. The kernel opens degraded over it:
/// reads that need no secret go on, and nothing is sealed or written.
struct NoKeyStore {
    name: String,
    why: Failure,
}

impl KeyStore for NoKeyStore {
    fn describe(&self) -> StoreDescription {
        StoreDescription {
            name: self.name.clone(),
            strength: StoreStrength::OsStore,
        }
    }

    fn wrap<'a>(
        &'a self,
        _key: &'a KeyMaterial,
        _bound_to: &'a Binding,
    ) -> VaultFuture<'a, Result<Wrapped, Failure>> {
        Box::pin(async { Err(self.why.clone()) })
    }

    fn unwrap<'a>(
        &'a self,
        _wrapped: &'a Wrapped,
        _bound_to: &'a Binding,
    ) -> VaultFuture<'a, Result<KeyMaterial, Failure>> {
        Box::pin(async { Err(self.why.clone()) })
    }

    fn health(&self) -> VaultFuture<'_, StoreHealth> {
        Box::pin(async { StoreHealth::Unreachable })
    }
}

fn no_key_store(name: &str, why: Failure) -> Arc<dyn KeyStore> {
    Arc::new(NoKeyStore {
        name: name.to_owned(),
        why,
    })
}

fn passphrase_failure(message: &str, fix: &str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        a2m_vault::codes::KEY_STORE_UNREACHABLE,
        message,
        fix,
    )
}

/// Reads the salt of the passphrase route, or makes and keeps a new one.
fn salt(paths: &Paths, random: &dyn RandomSource) -> Result<PassphraseSalt, Failure> {
    let file = paths.data.join(SALT_FILE);
    let unusable = || {
        passphrase_failure(
            "The salt of the vault's passphrase cannot be read or kept in the data directory.",
            "Make the data directory writable, and run `anything2mcp doctor`.",
        )
    };
    match std::fs::read(&file) {
        Ok(bytes) => {
            let bytes: [u8; PassphraseSalt::LENGTH] =
                bytes.as_slice().try_into().map_err(|_| unusable())?;
            Ok(PassphraseSalt::from_bytes(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let made = PassphraseSalt::generate(random)?;
            std::fs::create_dir_all(&paths.data).map_err(|_| unusable())?;
            std::fs::write(&file, made.as_bytes()).map_err(|_| unusable())?;
            Ok(made)
        }
        Err(_) => Err(unusable()),
    }
}

/// This machine's key store, as the installation's settings choose it.
/// Never fails: a store that cannot be reached is one that does not
/// answer, and the kernel opens degraded and says why.
async fn machine_key_store(
    settings: &Settings,
    options: &EngineOptions,
) -> Arc<dyn KeyStore> {
    if settings.text("vault.backend") == Some("passphrase") {
        let name = "a passphrase given at start";
        let Some(passphrase) = options
            .environment
            .get(PASSPHRASE_VARIABLE)
            .filter(|value| !value.is_empty())
        else {
            return no_key_store(
                name,
                passphrase_failure(
                    "The vault is set to take its root key from a passphrase, and none was given.",
                    "Set the environment variable `A2M_VAULT_PASSPHRASE` for the engine's process.",
                ),
            );
        };
        let salt = match salt(&options.paths, options.random.as_ref()) {
            Ok(salt) => salt,
            Err(failure) => return no_key_store(name, failure),
        };
        return match PassphraseKeyStore::derive(
            PassphraseRoute::Enabled,
            &Secret::new(passphrase.clone()),
            &salt,
        )
        .await
        {
            Ok(store) => Arc::new(store),
            Err(failure) => no_key_store(name, failure),
        };
    }
    let name = "the operating system's key store";
    let installation = match InstallationName::new("local") {
        Ok(installation) => installation,
        Err(failure) => return no_key_store(name, failure),
    };
    match OsKeyStore::open(&installation).await {
        Ok(store) => Arc::new(store),
        // The failure keeps its class, code, and sentence, and gains the
        // way out a machine with no key store has: the passphrase route,
        // which only the installation's own settings file can choose.
        Err(failure) => no_key_store(
            name,
            Failure::new(
                failure.class(),
                failure.code().clone(),
                failure.message(),
                format!(
                    "Unlock or start the key store. On a machine that has none, write `backend = \"passphrase\"` under `[vault]` in `{}`, and start the engine with the passphrase in the environment variable `{PASSPHRASE_VARIABLE}`.",
                    options
                        .paths
                        .installation_config
                        .join(INSTALLATION_SETTINGS_FILE)
                        .display()
                ),
            ),
        ),
    }
}

impl Engine {
    /// Opens the engine in this process and starts its runner loop.
    ///
    /// The kernel always opens: a store, a vault, or a pack that cannot be
    /// used is a named reason in `engine.status` and `doctor.run`, never a
    /// failure here.
    ///
    /// # Errors
    ///
    /// `input`/`settings_invalid`, `policy`/`setting_protected`, and
    /// `policy`/`setting_locked` when the settings do not load.
    pub async fn open(options: EngineOptions) -> Result<Self, Failure> {
        let settings = Settings::load(&sources(&options)?)?;
        let key_store = match &options.key_store {
            Some(key_store) => Arc::clone(key_store),
            None => machine_key_store(&settings, &options).await,
        };
        let roots = roots(&options.paths, options.project.as_deref(), options.place);
        let prepared = Kernel::prepare(KernelParts {
            settings,
            paths: options.paths,
            place: options.place,
            key_store,
            clock: options.clock,
            random: options.random,
            os_user: options.os_user.clone(),
        })
        .await;
        let host = prepared.host_services(Supervisor::new(SupervisorLimits::default()), roots);
        let mut builder = RegistryBuilder::new(prepared.settings().provider_settings(), host);
        packs::fill(&mut builder);
        let (registry, problems) = builder.build();
        let kernel = Kernel::open(prepared, registry, problems, options.person).await;
        let principal = match kernel
            .verify_caller(CallerCredential::LocalUser(options.os_user))
            .await
        {
            Ok(principal) => principal,
            // A stopped engine names nobody; it answers its own status.
            Err(_) => kernel.status_only_caller(),
        };
        // A kernel that is stopped or degraded runs no job. The reason is
        // kept for the caller who asks, and is in `engine.status` already.
        let runner_problem = kernel.start_runner_loop(RUNNER_SLOTS).await.err();
        Ok(Self {
            kernel,
            principal,
            project: options.project,
            runner_problem,
        })
    }

    /// The kernel, for a host that needs more than `dispatch`.
    #[must_use]
    pub const fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    /// The project folder the engine was opened in.
    #[must_use]
    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }

    /// Why the runner loop is not running, when it is not.
    #[must_use]
    pub const fn runner_problem(&self) -> Option<&Failure> {
        self.runner_problem.as_ref()
    }

    /// The one caller a process on this machine is, through `door`. An
    /// agent says so; a person at a terminal does not.
    #[must_use]
    pub fn caller(&self, door: FrontDoor, is_agent: bool) -> Caller {
        Caller::new(self.principal.clone(), door, is_agent)
    }

    /// A request by this process's caller, in the engine's project folder.
    #[must_use]
    pub fn request(&self, action: &str, input: serde_json::Value, caller: Caller) -> Request {
        let request = Request::new(action, input, caller);
        match &self.project {
            Some(project) => request.in_project(project.clone()),
            None => request,
        }
    }

    /// Carries out one request. Never fails: a refusal is a response.
    pub async fn dispatch(&self, request: Request) -> Response {
        self.kernel.dispatch(request).await
    }

    /// Closes the engine: the runner loop stops, sessions are saved, and
    /// the store is closed.
    pub async fn close(self) {
        self.kernel.close(CloseReason::Asked).await;
    }
}

/// The action registry of this build, without opening a store: the
/// kernel's actions and those of every first-party pack. The command tree
/// is built from it before any engine is opened, so that help and a
/// malformed command cost nothing and touch nothing.
///
/// # Errors
///
/// `input`/`action_registry_invalid`, as [`ActionRegistry::build`] answers.
pub fn action_registry() -> Result<ActionRegistry, Failure> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::new(NoSecrets),
        telemetry: Telemetry::in_memory(LogLevel::Error, Arc::new(NoSecrets), clock),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    packs::fill(&mut builder);
    let (registry, _problems) = builder.build();
    ActionRegistry::build(&registry)
}
