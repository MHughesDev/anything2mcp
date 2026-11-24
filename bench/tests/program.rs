//! The `a2m-bench` program, run as CI runs it: `run` writes a result file, and
//! `compare` sets the exit status a build is failed by.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use a2m_bench::harness::{Overhead, Plan, ScenarioResult};
use a2m_bench::report::{COMMIT_VARIABLE, RunReport};
use a2m_bench::stats::Stats;

type Outcome = Result<(), Box<dyn Error>>;

fn program(arguments: &[&Path]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_a2m-bench"))
        .args(arguments)
        .env(COMMIT_VARIABLE, "0123456789abcdef")
        .output()
}

fn scratch_folder(name: &str) -> std::io::Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!("a2m-bench-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// A word of the command line, as the path the program takes its arguments as.
fn word(word: &str) -> &Path {
    Path::new(word)
}

/// A stored result in which an `http` operation has the given overhead.
fn planted(folder: &Path, name: &str, overhead_ns: u64) -> Result<PathBuf, Box<dyn Error>> {
    let stats = |median_ns: u64| Stats {
        count: 100,
        min_ns: median_ns,
        median_ns,
        p95_ns: median_ns,
        p99_ns: median_ns,
        max_ns: median_ns,
    };
    let (operation, baseline) = (stats(400_000 + overhead_ns), stats(400_000));
    let report = RunReport::new(
        Some(name.to_owned()),
        Plan::default(),
        vec![ScenarioResult {
            name: "http_operation".to_owned(),
            overhead: Some(Overhead::between(&operation, &baseline)),
            resident_bytes: None,
            operation,
            baseline: Some(baseline),
        }],
    );
    Ok(report.write_to(folder)?)
}

#[test]
fn run_writes_one_result_file_that_compares_clean_with_itself() -> Outcome {
    let folder = scratch_folder("run")?;
    let ran = program(&[
        word("run"),
        word("--out"),
        &folder,
        word("--iterations"),
        word("5"),
        word("--warmup"),
        word("1"),
    ])?;
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let shown = String::from_utf8_lossy(&ran.stdout);
    assert!(shown.contains("direct_read"), "{shown}");
    // A budget with no scenario is said to be not measured, never passed;
    // the split of a call has none, and the budgeted scenarios that ran have a standing.
    let split = shown
        .lines()
        .find(|line| line.starts_with("call_overhead"))
        .ok_or("no row for the split of a call")?;
    assert!(split.contains("not measured"), "{split}");
    let listing = shown
        .lines()
        .find(|line| line.starts_with("list_tools") && line.contains("target"))
        .ok_or("no standing for listing tools")?;
    assert!(!listing.contains("not measured"), "{listing}");

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&folder)? {
        files.push(entry?.path());
    }
    assert_eq!(files.len(), 1, "one file per run");
    let report = RunReport::read(&files[0])?;
    assert_eq!(report.commit.as_deref(), Some("0123456789abcdef"));
    assert_eq!(report.os, std::env::consts::OS);
    assert_eq!(report.plan.iterations, 5);
    let read = report.scenario("direct_read").ok_or("no direct read")?;
    assert_eq!(read.operation.count, 5);

    let compared = program(&[
        word("compare"),
        word("--baseline"),
        &files[0],
        word("--new"),
        &files[0],
    ])?;
    assert_eq!(compared.status.code(), Some(0));
    std::fs::remove_dir_all(&folder)?;
    Ok(())
}

#[test]
fn compare_exits_with_one_on_a_planted_regression_and_zero_within_the_noise_floor() -> Outcome {
    let folder = scratch_folder("compare")?;
    let stored = planted(&folder, "stored", 1_000_000)?;
    let slower = planted(&folder, "slower", 1_300_000)?;
    let jitter = planted(&folder, "jitter", 1_050_000)?;
    let tiny_before = planted(&folder, "tinybefore", 20_000)?;
    let tiny_after = planted(&folder, "tinyafter", 70_000)?;
    let compare = |stored: &Path, new: &Path, more: &[&str]| -> Result<Output, Box<dyn Error>> {
        let mut arguments = vec![
            word("compare"),
            word("--baseline"),
            stored,
            word("--new"),
            new,
        ];
        arguments.extend(more.iter().map(|more| word(more)));
        Ok(program(&arguments)?)
    };

    // 1 ms to 1.3 ms of overhead: thirty percent, and far over the noise floor.
    let failed = compare(&stored, &slower, &[])?;
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stdout).contains("REGRESSION"));

    // Five percent is within the ten percent allowed.
    assert_eq!(compare(&stored, &jitter, &[])?.status.code(), Some(0));

    // 20 us to 70 us is 250 percent and still only 50 us: under the noise floor.
    let noise = compare(&tiny_before, &tiny_after, &[])?;
    assert_eq!(noise.status.code(), Some(0));
    // With the floor set below the change, the same pair fails.
    let strict = compare(&tiny_before, &tiny_after, &["--noise-floor-us", "10"])?;
    assert_eq!(strict.status.code(), Some(1));

    // A file that is not there, and a wrong command line, are neither a pass nor a regression.
    let absent = folder.join("absent.json");
    assert_eq!(compare(&absent, &stored, &[])?.status.code(), Some(2));
    assert_eq!(program(&[word("compare")])?.status.code(), Some(2));
    std::fs::remove_dir_all(&folder)?;
    Ok(())
}
