//! The scenarios of package 1.7 run end to end against the test application,
//! each checking what it measures before it times it: the interface read
//! reaches the application as the opening of its page, the request read as a
//! request for its JSON, and a listing names the published tool.

use a2m_bench::harness::{MonotonicClock, Plan};
use a2m_bench::measure::{
    COLD_BROWSER_START, HTTP_OPERATION, IDLE_MEMORY, LIST_TOOLS, START_TO_READY, UI_STEP, program,
};
use a2m_bench::scenarios::run_named;

const SMALL: Plan = Plan {
    warmup_iterations: 1,
    iterations: 3,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_measured_scenario_runs_and_holds_what_it_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let names: Vec<String> = [LIST_TOOLS, HTTP_OPERATION, UI_STEP, COLD_BROWSER_START]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let results = run_named(&names, &SMALL, &MonotonicClock::start()).await?;
    assert_eq!(results.len(), 4);
    for result in &results {
        assert_eq!(result.operation.count, 3, "{}", result.name);
        assert!(result.operation.median_ns > 0, "{}", result.name);
    }
    // The two reads that are set against the application's own time say so.
    for name in [HTTP_OPERATION, UI_STEP] {
        let read = results.iter().find(|r| r.name == name).ok_or(name)?;
        assert!(read.baseline.is_some() && read.overhead.is_some(), "{name}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_engine_as_a_process_starts_and_its_memory_is_read()
-> Result<(), Box<dyn std::error::Error>> {
    let names = [START_TO_READY.to_owned(), IDLE_MEMORY.to_owned()];
    let results = run_named(&names, &SMALL, &MonotonicClock::start()).await?;
    if program().is_none() {
        // The program is not built beside the harness: nothing is measured,
        // and nothing is reported as measured.
        assert!(results.is_empty());
        return Ok(());
    }
    let start = results
        .iter()
        .find(|r| r.name == START_TO_READY)
        .ok_or("no start")?;
    assert!(start.operation.median_ns > 0);
    if cfg!(unix) {
        let idle = results
            .iter()
            .find(|r| r.name == IDLE_MEMORY)
            .ok_or("no memory")?;
        let bytes = idle.resident_bytes.ok_or("no reading")?;
        assert!(bytes > 1_000_000, "{bytes} bytes is not a process");
    }
    Ok(())
}

#[test]
fn a_program_size_is_the_size_of_the_file_and_holds_no_timing()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("a2m-bench-size-{}", std::process::id()));
    std::fs::write(&path, vec![0_u8; 12_345])?;
    let result = a2m_bench::measure::size_of_file(&path);
    std::fs::remove_file(&path)?;
    let result = result?;
    assert_eq!(result.name, "program_size");
    assert_eq!(result.size_bytes, Some(12_345));
    assert_eq!((result.baseline, result.overhead), (None, None));
    // A file that is not there is an error, never a size of nothing.
    assert!(a2m_bench::measure::size_of_file(&path).is_err());
    Ok(())
}

#[tokio::test]
async fn a_debug_harness_measures_no_size_because_a_debug_program_is_not_the_product()
-> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) && std::env::var_os("A2M_BENCH_PROGRAM").is_none() {
        let names = ["program_size".to_owned()];
        let results = run_named(&names, &SMALL, &MonotonicClock::start()).await?;
        assert!(results.is_empty());
    }
    Ok(())
}
