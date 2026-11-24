//! The result of one run, as one JSON file.
//!
//! The file's shape is a contract: the comparison reads a file stored by an
//! earlier run, perhaps an earlier build of the harness. It carries a schema
//! name and a version, and a reader refuses a version it does not know. Version
//! 1 is:
//!
//! ```json
//! {
//!   "schema": "a2m-bench-result",
//!   "version": 1,
//!   "commit": "0123abcd",
//!   "os": "linux",
//!   "arch": "x86_64",
//!   "plan": { "warmup_iterations": 50, "iterations": 500 },
//!   "scenarios": [
//!     {
//!       "name": "direct_read",
//!       "operation": { "count": 500, "min_ns": 1, "median_ns": 2, "p95_ns": 3, "p99_ns": 4, "max_ns": 5 },
//!       "baseline": null,
//!       "overhead": null
//!     }
//!   ]
//! }
//! ```
//!
//! `commit` is null when no commit was given. `baseline` has the shape of
//! `operation`, and `overhead` holds signed `median_ns`, `p95_ns`, and `p99_ns`;
//! both are null for a scenario with no baseline operation. The file holds no
//! wall-clock time, so two runs differ only in what they measured.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::BenchError;
use crate::harness::{Plan, ScenarioResult};
use crate::stats::Stats;

/// The schema name every result file carries.
pub const SCHEMA: &str = "a2m-bench-result";

/// The version of the result file this harness writes and reads.
pub const VERSION: u32 = 1;

/// The environment variable that gives the commit a run measures.
pub const COMMIT_VARIABLE: &str = "A2M_BENCH_COMMIT";

/// Everything one run measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    /// Always [`SCHEMA`].
    pub schema: String,
    /// Always [`VERSION`].
    pub version: u32,
    /// The commit that was measured, if the run was told.
    pub commit: Option<String>,
    /// The operating system the run was made on, as the compiler names it.
    pub os: String,
    /// The processor architecture the run was made on.
    pub arch: String,
    /// How many times each scenario was run.
    pub plan: Plan,
    /// Each scenario's numbers, in the order they were run.
    pub scenarios: Vec<ScenarioResult>,
}

impl RunReport {
    /// A report of a run made on this machine.
    ///
    /// A commit that is empty or only spaces is no commit.
    #[must_use]
    pub fn new(commit: Option<String>, plan: Plan, scenarios: Vec<ScenarioResult>) -> Self {
        let commit = commit
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty());
        Self {
            schema: SCHEMA.to_owned(),
            version: VERSION,
            commit,
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            plan,
            scenarios,
        }
    }

    /// The numbers of the scenario with this name, if it was run.
    #[must_use]
    pub fn scenario(&self, name: &str) -> Option<&ScenarioResult> {
        self.scenarios.iter().find(|s| s.name == name)
    }

    /// The name of this run's file: the operating system, the architecture, and
    /// the first twelve characters of the commit, or `local` without one.
    ///
    /// Only letters, digits, and hyphens of the commit are kept, so that what
    /// an environment variable holds can never name another folder.
    #[must_use]
    pub fn file_name(&self) -> String {
        let commit: String = self
            .commit
            .iter()
            .flat_map(|c| c.chars())
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .take(12)
            .collect();
        let commit = if commit.is_empty() { "local" } else { &commit };
        format!("a2m-bench-{}-{}-{commit}.json", self.os, self.arch)
    }

    /// Writes this run as one file in the folder, making the folder if needed,
    /// and returns the file's path. A file of the same name is replaced.
    ///
    /// # Errors
    ///
    /// Returns an error if the folder or the file could not be written.
    pub fn write_to(&self, folder: &Path) -> Result<PathBuf, BenchError> {
        std::fs::create_dir_all(folder).map_err(|source| BenchError::File {
            path: folder.to_path_buf(),
            source,
        })?;
        let path = folder.join(self.file_name());
        let mut text = serde_json::to_string_pretty(self).map_err(|e| BenchError::Format {
            path: path.clone(),
            message: e.to_string(),
        })?;
        text.push('\n');
        std::fs::write(&path, text).map_err(|source| BenchError::File {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Reads a result file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, is not a result file, or
    /// has a version this harness does not know.
    pub fn read(path: &Path) -> Result<Self, BenchError> {
        let text = std::fs::read_to_string(path).map_err(|source| BenchError::File {
            path: path.to_path_buf(),
            source,
        })?;
        let format = |message: String| BenchError::Format {
            path: path.to_path_buf(),
            message,
        };
        // The schema and version are checked before the rest is read, so that a
        // newer file is refused by its version and not by a field it added.
        let head: Head = serde_json::from_str(&text).map_err(|e| format(e.to_string()))?;
        if head.schema != SCHEMA {
            return Err(format(format!(
                "the schema is {}, and a result file has {SCHEMA}",
                head.schema
            )));
        }
        if head.version != VERSION {
            return Err(format(format!(
                "the result file has version {}, and this harness reads version {VERSION}",
                head.version
            )));
        }
        serde_json::from_str(&text).map_err(|e| format(e.to_string()))
    }

    /// The run as a table for a person to read, times in microseconds.
    #[must_use]
    pub fn table(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{:<28} {:>7} {:>11} {:>11} {:>11} {:>11} {:>11}",
            "scenario", "count", "min us", "median us", "p95 us", "p99 us", "max us"
        );
        for scenario in &self.scenarios {
            if let Some(bytes) = scenario.size_bytes {
                let _ = writeln!(
                    out,
                    "{:<28} {:>7} {} KB file",
                    scenario.name,
                    "",
                    micros(i128::from(bytes))
                );
                continue;
            }
            stats_row(&mut out, &scenario.name, &scenario.operation);
            if let Some(baseline) = &scenario.baseline {
                stats_row(&mut out, "  directly to the stand", baseline);
            }
            if let Some(overhead) = &scenario.overhead {
                let _ = writeln!(
                    out,
                    "{:<28} {:>7} {:>11} {:>11} {:>11} {:>11} {:>11}",
                    "  overhead",
                    "",
                    "",
                    micros(overhead.median_ns),
                    micros(overhead.p95_ns),
                    micros(overhead.p99_ns),
                    ""
                );
            }
            if let Some(bytes) = scenario.resident_bytes {
                let _ = writeln!(
                    out,
                    "{:<28} {:>7} {} KB resident",
                    "  idle memory",
                    "",
                    micros(i128::from(bytes))
                );
            }
        }
        out
    }
}

/// The two fields that say whether the rest of a file can be read.
#[derive(Deserialize)]
struct Head {
    schema: String,
    version: u32,
}

fn stats_row(out: &mut String, label: &str, stats: &Stats) {
    let _ = writeln!(
        out,
        "{:<28} {:>7} {:>11} {:>11} {:>11} {:>11} {:>11}",
        label,
        stats.count,
        micros(i128::from(stats.min_ns)),
        micros(i128::from(stats.median_ns)),
        micros(i128::from(stats.p95_ns)),
        micros(i128::from(stats.p99_ns)),
        micros(i128::from(stats.max_ns)),
    );
}

/// Nanoseconds as microseconds with one decimal, by whole-number arithmetic.
#[must_use]
pub fn micros(nanos: impl Into<i128>) -> String {
    let nanos: i128 = nanos.into();
    let sign = if nanos < 0 { "-" } else { "" };
    let magnitude = nanos.unsigned_abs();
    format!("{sign}{}.{}", magnitude / 1_000, (magnitude % 1_000) / 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Overhead;

    fn stats(median_ns: u64) -> Stats {
        Stats {
            count: 10,
            min_ns: median_ns / 2,
            median_ns,
            p95_ns: median_ns * 2,
            p99_ns: median_ns * 3,
            max_ns: median_ns * 4,
        }
    }

    fn report(commit: Option<&str>) -> RunReport {
        let operation = stats(900_000);
        let baseline = stats(400_000);
        RunReport::new(
            commit.map(str::to_owned),
            Plan {
                warmup_iterations: 2,
                iterations: 10,
            },
            vec![
                ScenarioResult {
                    name: "direct_read".to_owned(),
                    operation: baseline,
                    baseline: None,
                    overhead: None,
                    resident_bytes: None,
                    size_bytes: None,
                },
                ScenarioResult {
                    name: "http_operation".to_owned(),
                    overhead: Some(Overhead::between(&operation, &baseline)),
                    resident_bytes: None,
                    size_bytes: None,
                    operation,
                    baseline: Some(baseline),
                },
            ],
        )
    }

    fn scratch_folder(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("a2m-bench-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_written_run_reads_back_the_same_and_keeps_the_documented_field_names() {
        let folder = scratch_folder("report");
        let written = report(Some("0123456789abcdef0123"));
        let path = written.write_to(&folder).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!(
                "a2m-bench-{}-{}-0123456789ab.json",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        );
        assert_eq!(RunReport::read(&path).unwrap(), written);

        // The names in the file are the contract; this holds them to version 1.
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["schema"], "a2m-bench-result");
        assert_eq!(json["version"], 1);
        assert_eq!(json["commit"], "0123456789abcdef0123");
        assert_eq!(json["os"], std::env::consts::OS);
        assert_eq!(json["plan"]["iterations"], 10);
        assert_eq!(json["scenarios"][0]["name"], "direct_read");
        assert_eq!(json["scenarios"][0]["baseline"], serde_json::Value::Null);
        let measured = &json["scenarios"][1];
        for field in ["count", "min_ns", "median_ns", "p95_ns", "p99_ns", "max_ns"] {
            assert!(measured["operation"][field].is_u64(), "operation.{field}");
            assert!(measured["baseline"][field].is_u64(), "baseline.{field}");
        }
        assert_eq!(measured["overhead"]["median_ns"], 500_000);
        assert_eq!(measured["overhead"]["p95_ns"], 1_000_000);
        assert_eq!(measured["overhead"]["p99_ns"], 1_500_000);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_file_of_another_version_or_schema_is_refused() {
        let folder = scratch_folder("versions");
        let path = report(None).write_to(&folder).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();

        let newer = folder.join("newer.json");
        std::fs::write(&newer, text.replace("\"version\": 1", "\"version\": 2")).unwrap();
        let error = RunReport::read(&newer).unwrap_err();
        assert!(matches!(error, BenchError::Format { .. }), "{error}");
        assert!(error.to_string().contains("version 2"), "{error}");

        let other = folder.join("other.json");
        std::fs::write(&other, text.replace(SCHEMA, "something-else")).unwrap();
        assert!(matches!(
            RunReport::read(&other),
            Err(BenchError::Format { .. })
        ));
        assert!(matches!(
            RunReport::read(&folder.join("absent.json")),
            Err(BenchError::File { .. })
        ));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_commit_cannot_shape_the_file_name() {
        let name = |commit: Option<&str>| report(commit).file_name();
        let local = format!(
            "a2m-bench-{}-{}-local.json",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert_eq!(name(None), local);
        assert_eq!(name(Some("   ")), local);
        assert_eq!(name(Some("../..")), local);
        assert!(name(Some("../ab/cd")).ends_with("-abcd.json"));
        assert_eq!(report(Some("  abc  ")).commit.as_deref(), Some("abc"));
    }

    #[test]
    fn times_are_shown_in_microseconds() {
        assert_eq!(micros(1_234_567_i64), "1234.5");
        assert_eq!(micros(-250_i64), "-0.2");
        assert_eq!(micros(0_u64), "0.0");
        let table = report(None).table();
        assert!(table.contains("http_operation"));
        assert!(table.contains("directly to the stand"));
        assert!(table.contains("overhead"));
    }
}
