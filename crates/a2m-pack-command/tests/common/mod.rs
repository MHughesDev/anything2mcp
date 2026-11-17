//! What the tests share: a folder of the test's own, a registry that holds
//! only the command pack, and the `example` fixtures compiled into a plan,
//! as they are or with the test stand's program put in place.

#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use a2m_core::{Plan, Redact, Version};
use a2m_files::{
    CompileMode, Compiled, CompilerSet, Diagnostics, RelPath, SourceFile, Sources, TargetRoot,
};
use a2m_provider::{
    Access, HostServices, ProviderSettings, Registry, RegistryBuilder, RootName, Roots, Supervisor,
    SupervisorLimits, current_platform,
};
use a2m_telemetry::TelemetrySettings;
use example_cli::harness::Stand;

/// A folder under the system's temporary folder, removed when dropped.
pub struct Folder(PathBuf);

impl Folder {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "a2m-pack-command-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        // Resolved, so that a checked path under it is the path it names.
        Self(std::fs::canonicalize(&path).unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A registry that holds only the command pack, with the root the pack
/// makes its run folders in, and the redactor given.
pub fn registry(folder: &Folder, redact: Arc<dyn Redact>) -> Registry {
    let logs = folder.path().join("logs");
    let runs = folder.path().join("runs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(&runs).unwrap();
    let mut roots = Roots::none();
    roots
        .add(RootName::new("command_runs").unwrap(), &runs, Access::Write)
        .unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots,
        redact: Arc::clone(&redact),
        telemetry: a2m_telemetry::start(&TelemetrySettings::default(), redact, &logs).unwrap(),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    a2m_pack_command::register(&mut builder);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    registry
}

/// The folder of the `example` fixtures.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("example")
}

/// The fixture files, by their path inside the target folder.
pub const FILES: &[&str] = &[
    "target.yaml",
    "types.yaml",
    "commands/inventory.yaml",
    "operations/catalog_push.yaml",
    "operations/echo_text.yaml",
    "operations/item_add.yaml",
    "operations/item_get.yaml",
    "operations/item_list.yaml",
    "operations/item_remove.yaml",
    "operations/item_set_quantity.yaml",
    "operations/stock_report.yaml",
];

/// The text of one fixture file.
pub fn fixture(path: &str) -> String {
    let file = RelPath::new(path).unwrap().under(&fixtures());
    std::fs::read_to_string(file).unwrap().replace("\r\n", "\n")
}

/// A path as a YAML scalar in single quotes.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// The fixture files with the stand's program in place of the imaginary
/// one: its path, its home folder, and the digest of its file for this
/// platform.
pub fn stand_files(stand: &Stand) -> Vec<(String, String)> {
    let digest = a2m_core::Digest::of(&std::fs::read(stand.program()).unwrap());
    let platform = current_platform();
    FILES
        .iter()
        .map(|path| {
            let mut text = fixture(path);
            if *path == "target.yaml" {
                text = text
                    .replace("'/opt/example/bin/example-cli'", &quoted(stand.program()))
                    .replace("'/opt/example-test/bin/example-cli'", &quoted(stand.program()));
            }
            if *path == "commands/inventory.yaml" {
                text = text.replace("'/var/lib/example/home'", &quoted(stand.home()));
                let lines: Vec<String> = text
                    .lines()
                    .filter(|line| {
                        // The fixture's line for this platform gives way to the real one.
                        !(line.trim_start().starts_with(&format!("{platform}:"))
                            && platform != "linux-x86_64")
                    })
                    .map(|line| {
                        if line.trim_start().starts_with("linux-x86_64:") {
                            format!("    {platform}: [\"sha256:{digest}\"]")
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect();
                text = lines.join("\n");
            }
            ((*path).to_owned(), text)
        })
        .collect()
}

/// Reads files given as text into sources.
pub fn sources(files: &[(String, String)], set: &CompilerSet) -> Result<Sources, Diagnostics> {
    let mut read = Vec::new();
    let mut problems = Diagnostics::new();
    for (path, text) in files {
        match SourceFile::read(RelPath::new(path).unwrap(), text.as_bytes(), set) {
            Ok(file) => read.push(file),
            Err(problem) => problems.push(problem),
        }
    }
    if problems.is_empty() {
        Ok(Sources::from_files(TargetRoot::new(fixtures()), read))
    } else {
        Err(problems)
    }
}

/// Compiles files given as text, as a draft.
pub fn compile(files: &[(String, String)], set: &CompilerSet) -> Result<Compiled, Diagnostics> {
    let sources = sources(files, set)?;
    a2m_files::compile(&sources, set, CompileMode::Draft, Version::new(1, 0, 0))
}

/// The plan of the fixtures with the stand's program in place.
pub fn stand_plan(stand: &Stand, set: &CompilerSet) -> Plan {
    match compile(&stand_files(stand), set) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the fixtures do not compile: {problems:#?}"),
    }
}

/// The fixture files as they are, with one file's text changed.
pub fn changed(path: &str, from: &str, to: &str) -> Vec<(String, String)> {
    FILES
        .iter()
        .map(|file| {
            let text = fixture(file);
            if *file == path {
                assert!(text.contains(from), "`{from}` is not in {path}");
                ((*file).to_owned(), text.replacen(from, to, 1))
            } else {
                ((*file).to_owned(), text)
            }
        })
        .collect()
}
pub mod ledger;
