//! The scenarios of package 1.7 run end to end against the test application,
//! each checking what it measures before it times it: the interface read
//! reaches the application as the opening of its page, the request read as a
//! request for its JSON, and a listing names the published tool.

use a2m_bench::harness::{MonotonicClock, Plan};
use a2m_bench::marks::{CALL_OUTSIDE_RUN, CALL_OVERHEAD, WARM_TAB};
use a2m_bench::measure::{
    API_OPERATION, COLD_BROWSER_START, HTTP_OPERATION, IDLE_MEMORY, LIST_TOOLS,
    PAGE_HTTP_OPERATION, START_TO_READY, TOTALS_ORDER, UI_STEP, api_read_once, page_http_read_once,
    program,
};
use a2m_bench::scenarios::{Stand, run_named};

const SMALL: Plan = Plan {
    warmup_iterations: 1,
    iterations: 3,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_measured_scenario_runs_and_holds_what_it_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let names: Vec<String> = [
        LIST_TOOLS,
        HTTP_OPERATION,
        API_OPERATION,
        PAGE_HTTP_OPERATION,
        UI_STEP,
        COLD_BROWSER_START,
    ]
    .iter()
    .map(|name| (*name).to_owned())
    .collect();
    let results = run_named(&names, &SMALL, &MonotonicClock::start()).await?;
    assert_eq!(results.len(), 6);
    for result in &results {
        assert_eq!(result.operation.count, 3, "{}", result.name);
        assert!(result.operation.median_ns > 0, "{}", result.name);
    }
    // The two reads that are set against the application's own time say so.
    for name in [HTTP_OPERATION, API_OPERATION, PAGE_HTTP_OPERATION, UI_STEP] {
        let read = results.iter().find(|r| r.name == name).ok_or(name)?;
        assert!(read.baseline.is_some() && read.overhead.is_some(), "{name}");
    }
    Ok(())
}

/// The two numbers taken from the trace's time marks (decision 0481): each
/// call's stored trace carries marks that hold together, or the scenario
/// fails, and a number comes from every measured call. Asked for alone, the
/// time outside the run is measured by itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_marked_scenarios_take_their_numbers_from_each_runs_trace()
-> Result<(), Box<dyn std::error::Error>> {
    let names: Vec<String> = [CALL_OVERHEAD, CALL_OUTSIDE_RUN, WARM_TAB]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let results = run_named(&names, &SMALL, &MonotonicClock::start()).await?;
    assert_eq!(
        results.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        [CALL_OVERHEAD, CALL_OUTSIDE_RUN, WARM_TAB]
    );
    for result in &results {
        assert_eq!(result.operation.count, 3, "{}", result.name);
        assert!(result.operation.min_ns > 0, "{}", result.name);
        assert_eq!((&result.baseline, &result.overhead), (&None, &None));
    }
    let alone = run_named(
        &[CALL_OUTSIDE_RUN.to_owned()],
        &SMALL,
        &MonotonicClock::start(),
    )
    .await?;
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].name, CALL_OUTSIDE_RUN);
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

/// The `page_http` read returns what the application itself says an order's
/// totals are, and reaches it as the page's own request: the application
/// hears the order's summary page opened and the totals asked for, and the
/// read sends nothing that writes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_page_http_scenario_reads_what_the_application_holds_by_the_pages_own_request()
-> Result<(), Box<dyn std::error::Error>> {
    let stand = Stand::start("test").await?;
    let read = page_http_read_once(&stand).await?;
    let truth: serde_json::Value = stand
        .app()
        .http()
        .get(
            stand
                .app()
                .url(&format!("/api/orders/{TOTALS_ORDER}/totals")),
        )
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(truth["lines"], 3, "the seed changed: {truth}");
    assert_eq!(read.value["lines"], truth["lines"], "{read:?}");
    assert_eq!(
        read.value["total_quantity"], truth["total_quantity"],
        "{read:?}"
    );
    assert_eq!(read.value["total_quantity"], 8, "{read:?}");
    assert!(
        read.heard
            .contains(&format!("GET /orders/{TOTALS_ORDER}/summary")),
        "{read:?}"
    );
    assert!(
        read.heard
            .contains(&format!("GET /api/orders/{TOTALS_ORDER}/totals")),
        "{read:?}"
    );
    assert!(
        read.heard.iter().all(|line| line.starts_with("GET ")),
        "{read:?}"
    );
    assert!(stand.app().writes().await?.is_empty());
    Ok(())
}

/// The `api` read returns the line item the application stores, bound to its
/// published description: the description is read before the request, and
/// the page is never opened.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_api_scenario_reads_what_the_application_holds_through_its_pinned_description()
-> Result<(), Box<dyn std::error::Error>> {
    let stand = Stand::start("test").await?;
    let read = api_read_once(&stand).await?;
    let stored = stand
        .app()
        .stored_item("A-100", "L-101")
        .await?
        .ok_or("the seeded item is not stored")?;
    assert_eq!(read.value["description"], "Left rail", "{read:?}");
    assert_eq!(
        read.value["description"],
        stored.description.as_str(),
        "{read:?}"
    );
    assert_eq!(read.value["quantity"], 2, "{read:?}");
    assert_eq!(read.value["quantity"], stored.quantity, "{read:?}");
    let truth: serde_json::Value = stand
        .app()
        .http()
        .get(stand.app().url("/api/orders/A-100/items/L-101"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(read.value["length_in"], truth["length_in"], "{read:?}");
    assert_eq!(read.value["length_in"], "150.5", "{read:?}");
    let description = read
        .heard
        .iter()
        .position(|line| line == "GET /api/openapi.json")
        .ok_or("the pinned description was not read")?;
    let request = read
        .heard
        .iter()
        .position(|line| line == "GET /api/orders/A-100/items/L-101")
        .ok_or("the request was not sent")?;
    assert!(description < request, "{read:?}");
    assert!(
        !read
            .heard
            .contains(&"GET /orders/A-100/items/L-101".to_owned()),
        "{read:?}"
    );
    assert!(stand.app().writes().await?.is_empty());
    Ok(())
}
