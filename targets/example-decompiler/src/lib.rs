//! The stand for an installed decompiling engine: the program is
//! `src/main.rs`, and this library finds it for a test.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The names of the stand's variables.
pub mod env {
    /// What the program does: see the program's own documentation.
    pub const MODE: &str = "EXAMPLE_DECOMPILER_MODE";
    /// The file every start's arguments are appended to.
    pub const LOG: &str = "EXAMPLE_DECOMPILER_LOG";
    /// The text one function holds in the `secret` mode.
    pub const SECRET: &str = "EXAMPLE_DECOMPILER_SECRET";
}

/// Builds the program with the cargo that runs the tests, once, and says
/// where it is.
///
/// # Errors
///
/// Why cargo could not be started, failed, or did not report the program.
pub fn program() -> Result<PathBuf, String> {
    static BUILT: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    BUILT.get_or_init(build).clone()
}

fn build() -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["build", "--package", "example-decompiler", "--bins"])
        .arg("--message-format=json")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cargo could not be started: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| "cargo did not report the program".to_owned())
}
