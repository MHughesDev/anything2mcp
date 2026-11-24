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

/// The entry file of the stand as a package the JavaScript runtime runs,
/// and the file it loads beside it.
const LAUNCHER_ENTRY: &str = include_str!("../launcher/server.js");
const LAUNCHER_RELAY: &str = include_str!("../launcher/relay.js");

/// One stdio stand: a state folder, and the program that serves it.
///
/// The harness does not start the program. Whatever is under test starts it,
/// from [`StdioStand::program`] with the environment of [`StdioStand::env`],
/// as many times as it likes; every program started over one stand shares the
/// stand's orders, settings, and ground truth. The folders are removed when
/// the stand is dropped.
#[derive(Debug)]
pub struct StdioStand {
    folder: PathBuf,
    program_folder: PathBuf,
    program: PathBuf,
    control: Control,
}

/// Makes `folder` empty and new: a folder left by an earlier process with
/// the same number is stale.
fn fresh_folder(folder: &Path) -> Result<(), HarnessError> {
    match std::fs::remove_dir_all(folder) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(HarnessError::State(e)),
        _ => {}
    }
    std::fs::create_dir_all(folder).map_err(HarnessError::State)
}

/// Puts the built program at `at`. On Linux and macOS a link where the disk
/// allows it: a file that was never open for writing can always be started
/// at once. On Windows a copy: a link there shares one access list with the
/// file the build wrote, which may be open to other users, and a copy takes
/// the access list of the stand's own folder.
fn place_program(built: &Path, at: &Path) -> Result<(), HarnessError> {
    #[cfg(not(windows))]
    if std::fs::hard_link(built, at).is_ok() {
        return Ok(());
    }
    std::fs::copy(built, at)
        .map(|_| ())
        .map_err(HarnessError::State)
}

impl StdioStand {
    /// Makes a new state folder with the seed data, builds the program if it
    /// is not built, and puts it in a folder of the stand's own under the
    /// system's temporary folder.
    ///
    /// The engine refuses a program that another user could replace, and
    /// the folder a build writes to need not be closed to other users: on a
    /// Windows machine whose checkout is on a drive other than the system's,
    /// every signed-in user may change it, as on a hosted CI runner. The
    /// user's temporary folder is closed to them on Windows and macOS, and on
    /// Linux it is a folder with the sticky mark, where only an entry's owner
    /// may replace it.
    ///
    /// # Errors
    ///
    /// Returns an error if a folder could not be made or the program could
    /// not be built or placed.
    pub fn new() -> Result<Self, HarnessError> {
        let built = stdio_program()?;
        let name = format!(
            "example-mcp-{}-{}",
            std::process::id(),
            FOLDERS.fetch_add(1, Ordering::SeqCst)
        );
        let folder = std::env::temp_dir().join(&name);
        let program_folder = std::env::temp_dir().join(format!("{name}-program"));
        fresh_folder(&folder)?;
        fresh_folder(&program_folder)?;
        let program = program_folder.join(
            built
                .file_name()
                .ok_or_else(|| HarnessError::Program("the program has no file name".to_owned()))?,
        );
        place_program(&built, &program)?;
        let control = Control::new(Arc::new(Backing::Folder(folder.clone())));
        control.reset()?;
        Ok(Self {
            folder,
            program_folder,
            program,
            control,
        })
    }

    /// The program to start: the built one, placed in the stand's own
    /// folder. It takes no arguments.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The stand's server as the entry file of an installed package, which
    /// the JavaScript runtime runs: `server.js`, written with the file it
    /// loads beside it into the stand's own folder, whose path this gives.
    /// Its first argument is the stand's program, which the file beside it
    /// starts on the same standard streams; the rest go to that program.
    /// Each call writes both files afresh, so a test may change one.
    ///
    /// # Errors
    ///
    /// Returns an error if a file could not be written.
    pub fn launcher_entry(&self) -> Result<PathBuf, HarnessError> {
        let entry = self.program_folder.join("server.js");
        std::fs::write(&entry, LAUNCHER_ENTRY).map_err(HarnessError::State)?;
        std::fs::write(self.program_folder.join("relay.js"), LAUNCHER_RELAY)
            .map_err(HarnessError::State)?;
        Ok(entry)
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
        let _ = std::fs::remove_dir_all(&self.program_folder);
    }
}

/// Whether the process `id` still runs, as the system's own listing of
/// processes says: `ps` on Linux and macOS, where a process that has ended
/// and is waiting to be reaped counts as gone, and `tasklist` on Windows.
/// Both are programs started by their path, never through a shell.
///
/// # Errors
///
/// Returns an error if the listing could not be read.
pub fn process_runs(id: u32) -> Result<bool, HarnessError> {
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let output = Command::new(system.join("System32").join("tasklist.exe"))
            .args(["/FI", &format!("PID eq {id}"), "/FO", "CSV", "/NH"])
            .output()
            .map_err(|e| HarnessError::Program(format!("tasklist could not be run: {e}")))?;
        // A listed process is one line of quoted fields, the second its
        // identifier; no process is a line of another kind.
        let wanted = format!("\"{id}\"");
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.split(',').nth(1) == Some(wanted.as_str())))
    }
    #[cfg(not(windows))]
    {
        let ps = ["/bin/ps", "/usr/bin/ps"]
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .ok_or_else(|| HarnessError::Program("no ps in /bin or /usr/bin".to_owned()))?;
        let output = Command::new(ps)
            .args(["-o", "stat=", "-p", &id.to_string()])
            .output()
            .map_err(|e| HarnessError::Program(format!("ps could not be run: {e}")))?;
        let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok(output.status.success() && !state.is_empty() && !state.starts_with('Z'))
    }
}

/// Ends the process `id` at once, from outside, as a person or the system
/// would end a program in the middle of its work: `kill -KILL` on Linux
/// and macOS, `taskkill /F` on Windows, each started by its path.
///
/// # Errors
///
/// Returns an error if the program could not be run or did not end it.
pub fn kill_process(id: u32) -> Result<(), HarnessError> {
    #[cfg(windows)]
    let mut command = {
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let mut command = Command::new(system.join("System32").join("taskkill.exe"));
        command.args(["/F", "/PID", &id.to_string()]);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let kill = ["/bin/kill", "/usr/bin/kill"]
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .ok_or_else(|| HarnessError::Program("no kill in /bin or /usr/bin".to_owned()))?;
        let mut command = Command::new(kill);
        command.args(["-KILL", &id.to_string()]);
        command
    };
    let output = command
        .output()
        .map_err(|e| HarnessError::Program(format!("the process could not be ended: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(HarnessError::Program(format!(
            "the process {id} was not ended: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
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
