//! The harness: starts a stand for a test with one call and hands out its
//! control interface.
//!
//! A test never proves a write using Anything2MCP's own read path alone. It
//! asks the control interface what the stand stored.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::control::Control;
use crate::http::ENDPOINT_PATH;
use crate::state::Backing;
use crate::stdio::{STATE_ENV, TOKEN_ENV};

/// The environment variable that, when set, names the stdio program to use,
/// in place of the one the harness builds.
pub const PROGRAM_ENV: &str = "EXAMPLE_MCP_BIN";

/// Why the harness could not do what a test asked.
#[derive(Debug)]
pub enum HarnessError {
    /// The HTTP form could not be started.
    Start(io::Error),
    /// The stand's state could not be read or written.
    State(io::Error),
    /// The stdio program could not be built or found.
    Program(String),
    /// Something a test waited for did not happen.
    Timeout(&'static str),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(e) => write!(f, "the stand could not be started: {e}"),
            Self::State(e) => write!(f, "the stand's state could not be read or written: {e}"),
            Self::Program(why) => write!(f, "the stdio program could not be built: {why}"),
            Self::Timeout(what) => write!(f, "timed out: {what}"),
        }
    }
}

impl std::error::Error for HarnessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(e) | Self::State(e) => Some(e),
            Self::Program(_) | Self::Timeout(_) => None,
        }
    }
}

/// One running HTTP stand, on its own port, stopped when dropped. Two stands
/// in one test share nothing.
#[derive(Debug)]
pub struct HttpStand {
    base: String,
    control: Control,
    server: JoinHandle<()>,
}

impl HttpStand {
    /// Starts a new stand with the seed data on a free loopback port.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start() -> Result<Self, HarnessError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let address = listener.local_addr().map_err(HarnessError::Start)?;
        let backing = Arc::new(Backing::memory());
        let control = Control::new(backing.clone());
        let server = tokio::spawn(async move {
            // The task is aborted when the stand is dropped; an error here
            // shows up in the test as a failed request.
            let _ = crate::http::serve(listener, backing).await;
        });
        Ok(Self {
            base: format!("http://{address}"),
            control,
            server,
        })
    }

    /// The address of the MCP endpoint: `http://127.0.0.1:<port>/mcp`.
    #[must_use]
    pub fn url(&self) -> String {
        format!("{}{ENDPOINT_PATH}", self.base)
    }

    /// The scheme, host, and port, with no path.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The control interface of this stand.
    #[must_use]
    pub fn control(&self) -> &Control {
        &self.control
    }
}

impl Drop for HttpStand {
    fn drop(&mut self) {
        self.server.abort();
    }
}

static FOLDERS: AtomicU32 = AtomicU32::new(0);

/// One stdio stand: a state folder, and the program that serves it.
///
/// The harness does not start the program. Whatever is under test starts it,
/// from [`StdioStand::program`] with the environment of [`StdioStand::env`],
/// as many times as it likes; every program started over one stand shares the
/// stand's orders, settings, and ground truth. The folder is removed when the
/// stand is dropped.
#[derive(Debug)]
pub struct StdioStand {
    folder: PathBuf,
    program: PathBuf,
    control: Control,
}

impl StdioStand {
    /// Makes a new state folder with the seed data, and builds the program if
    /// it is not built.
    ///
    /// # Errors
    ///
    /// Returns an error if the folder could not be made or the program could
    /// not be built.
    pub fn new() -> Result<Self, HarnessError> {
        let program = stdio_program()?;
        let folder = std::env::temp_dir().join(format!(
            "example-mcp-{}-{}",
            std::process::id(),
            FOLDERS.fetch_add(1, Ordering::SeqCst)
        ));
        // A folder left by an earlier process with the same number is stale.
        match std::fs::remove_dir_all(&folder) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(HarnessError::State(e)),
            _ => {}
        }
        std::fs::create_dir_all(&folder).map_err(HarnessError::State)?;
        let control = Control::new(Arc::new(Backing::Folder(folder.clone())));
        control.reset()?;
        Ok(Self {
            folder,
            program,
            control,
        })
    }

    /// The program to start. It takes no arguments.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The state folder.
    #[must_use]
    pub fn state_folder(&self) -> &Path {
        &self.folder
    }

    /// The environment the program needs: the variable that names the state
    /// folder.
    #[must_use]
    pub fn env(&self) -> Vec<(&'static str, OsString)> {
        vec![(STATE_ENV, self.folder.clone().into_os_string())]
    }

    /// The same, with the variable that carries a credential.
    #[must_use]
    pub fn env_with_token(&self, token: &str) -> Vec<(&'static str, OsString)> {
        let mut env = self.env();
        env.push((TOKEN_ENV, token.into()));
        env
    }

    /// A command that starts the program over this stand, for a test that
    /// starts it itself. The caller sets how its input and output are piped.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.envs(self.env());
        command
    }

    /// The control interface of this stand.
    #[must_use]
    pub fn control(&self) -> &Control {
        &self.control
    }
}

impl Drop for StdioStand {
    fn drop(&mut self) {
        // Left behind if a program still holds it open; the next stand with
        // the same name clears it.
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

static PROGRAM: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// The path of the stdio program, `example-mcp`, built if it is not.
///
/// Cargo builds a dependency's library for a test and not its programs, so a
/// crate that uses this stand in its tests cannot rely on the program being
/// there. The first call in a test process asks Cargo to build it and reads
/// the path from Cargo's answer; later calls return the same path. Setting
/// `EXAMPLE_MCP_BIN` names a program to use instead.
///
/// # Errors
///
/// Returns an error if Cargo could not be run or the build failed.
pub fn stdio_program() -> Result<PathBuf, HarnessError> {
    PROGRAM
        .get_or_init(find_or_build)
        .clone()
        .map_err(HarnessError::Program)
}

fn find_or_build() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(PROGRAM_ENV) {
        return Ok(PathBuf::from(path));
    }
    // Cargo sets `CARGO` for the tests it runs.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(cargo)
        .args(["build", "--quiet", "--message-format", "json"])
        .args(["--package", "example-mcp", "--bin", "example-mcp"])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|e| format!("cargo could not be run: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["target"]["name"] == "example-mcp"
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| "cargo did not say where it put the program".to_owned())
}
