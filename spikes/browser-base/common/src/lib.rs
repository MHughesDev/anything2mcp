//! Throwaway spike code (roadmap package 0.4). Shared by every candidate program.
//!
//! Each candidate does the same steps against the test application and prints one
//! `Report` as JSON. Nothing here is product code.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use example_app::harness::TestApp;
use serde::Serialize;

/// The page every candidate opens, the field it reads, and the value the seed data holds.
pub const FORM_PATH: &str = "/orders/A-100/items/L-101";
pub const FIELD_NAME: &str = "Description";
pub const FIELD_ID: &str = "description";
pub const WARM_READS: usize = 200;

/// The browser program. The spike takes it from the environment or from the
/// place this container has one. The product never does either: see decision 0070.
pub fn browser_program() -> PathBuf {
    std::env::var_os("A2M_SPIKE_BROWSER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/pw-browsers/chromium"))
}

/// Arguments every candidate passes. `--no-sandbox` is for this container only,
/// which runs as root; the product never passes it by default.
pub fn base_args(data_dir: &Path) -> Vec<String> {
    vec![
        "--headless".to_owned(),
        "--no-sandbox".to_owned(),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
        format!("--user-data-dir={}", data_dir.display()),
    ]
}

/// A new data directory that this program owns, under the system's temporary folder.
pub fn owned_data_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("a2m-spike-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the data directory");
    dir
}

/// Process identifiers whose command line names the data directory. Linux only;
/// on other systems the spike cannot check and says so by returning `None`.
pub fn leftovers(data_dir: &Path) -> Option<Vec<u32>> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let needle = data_dir.display().to_string();
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        // A zombie has an empty command line, so a reaped-late child is not counted.
        if let Ok(bytes) = std::fs::read(entry.path().join("cmdline")) {
            if String::from_utf8_lossy(&bytes).contains(&needle) {
                found.push(pid);
            }
        }
    }
    Some(found)
}

/// Waits, checking every 10 ms, until no process names the data directory or the
/// limit passes. This is the spike's own measuring loop, not a product wait.
pub fn wait_no_leftovers(data_dir: &Path, limit: Duration) -> Leftover {
    let start = Instant::now();
    loop {
        match leftovers(data_dir) {
            None => {
                return Leftover {
                    checked: false,
                    remaining: 0,
                    after_ms: 0.0,
                };
            }
            Some(found) if found.is_empty() => {
                return Leftover {
                    checked: true,
                    remaining: 0,
                    after_ms: ms(start.elapsed()),
                };
            }
            Some(found) if start.elapsed() > limit => {
                return Leftover {
                    checked: true,
                    remaining: found.len(),
                    after_ms: ms(start.elapsed()),
                };
            }
            Some(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

pub fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Leftover {
    pub checked: bool,
    pub remaining: usize,
    pub after_ms: f64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Reads {
    pub count: usize,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

pub fn summarize(mut samples: Vec<Duration>) -> Reads {
    samples.sort();
    let at = |q: f64| ms(samples[((samples.len() - 1) as f64 * q).round() as usize]);
    Reads {
        count: samples.len(),
        median_ms: at(0.5),
        p95_ms: at(0.95),
        max_ms: at(1.0),
    }
}

/// What one candidate measured. Every field is filled by the candidate program.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Report {
    pub candidate: String,
    pub transport: String,
    pub browser_version: String,
    /// From starting the browser program to the first field value in hand.
    pub cold_start_to_first_read_ms: f64,
    pub launch_to_connected_ms: f64,
    pub wait_kind: String,
    pub field_value: String,
    pub warm_reads: Reads,
    pub snapshot_kind: String,
    pub snapshot_nodes: usize,
    pub snapshot_ms: f64,
    pub intercepted: String,
    /// What the test application's control interface says it stored after the
    /// intercepted save: the number of writes. It must be 0.
    pub writes_stored_after_blocked_save: usize,
    pub clean_close: Leftover,
    pub kill_mid_call: KillResult,
    pub notes: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct KillResult {
    /// How the pending call ended: an error text, "hung", or "panicked".
    pub outcome: String,
    pub after_kill_ms: f64,
    pub leftover: Leftover,
}

impl Report {
    pub fn print(&self) {
        println!(
            "{}",
            serde_json::to_string_pretty(self).expect("a report serializes")
        );
    }
}

/// Removes the data directory, retrying briefly while the browser's children let go.
pub fn remove_data_dir(dir: &Path) {
    for _ in 0..50 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
