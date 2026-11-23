//! The exit test of Phase 4 (package 4.5): an operation gains a request
//! binding through shadow, returns identical results, and runs at least ten
//! times faster than through its interface binding.

use a2m_bench::harness::{MonotonicClock, Plan, run_scenario};
use a2m_bench::promotion::{PromotionSpeedup, REQUIRED_FACTOR, speedup};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_binding_gained_through_shadow_is_identical_and_ten_times_faster()
-> Result<(), Box<dyn std::error::Error>> {
    let plan = Plan {
        warmup_iterations: 2,
        iterations: 15,
    };
    // Setup is the whole path: publish, shadow, report, promote. A call whose
    // answer differs from the interface binding's stops the run.
    let result = run_scenario(&PromotionSpeedup, &plan, &MonotonicClock::start()).await?;
    let baseline = result.baseline.as_ref().ok_or("no baseline")?;
    let factor = speedup(&result).ok_or("no factor")?;
    eprintln!(
        "interface median {} ns, request median {} ns, factor {factor:.1}",
        baseline.median_ns, result.operation.median_ns
    );
    #[allow(clippy::cast_precision_loss, reason = "a small constant")]
    let required = REQUIRED_FACTOR as f64;
    // The claim is about the product's speed, which is the release build's:
    // CI runs this test with `--release`. An unoptimized build spends its time
    // in the engine, not the system, so it prints the factor and still checks
    // every identity and heard-request condition in the setup.
    if cfg!(debug_assertions) {
        eprintln!("debug build: the factor of {REQUIRED_FACTOR} is asserted in release only");
    } else {
        assert!(factor >= required, "only {factor:.1} times faster");
    }
    Ok(())
}
