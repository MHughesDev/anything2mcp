//! The exit test of Phase 4 (package 4.5): an operation gains a request
//! binding through shadow, returns identical results, and runs at least ten
//! times faster than through its interface binding.
//!
//! The two paths are measured in pairs: iteration `i` runs one interface call
//! and one request call, one straight after the other, and which goes first
//! alternates, so that load on the machine falls on both alike. Two factors are
//! asserted, each at least ten: the ratio of the two medians, and the median of
//! the per-pair ratios. The floors and the medians are printed beside them.

use std::time::{Duration, Instant};

use a2m_bench::harness::Scenario;
use a2m_bench::promotion::{MOST_ITERATIONS, PromotionSpeedup, REQUIRED_FACTOR};
use a2m_bench::stats::Stats;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        f64::midpoint(values[middle - 1], values[middle])
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_binding_gained_through_shadow_is_identical_and_ten_times_faster()
-> Result<(), Box<dyn std::error::Error>> {
    let scenario = PromotionSpeedup;
    // Setup is the whole path: publish, shadow, report, promote. A call whose
    // answer differs from the interface binding's stops the run.
    let state = scenario.setup().await?;
    if scenario.baseline(&state).is_none() {
        return Err("no baseline".into());
    }
    let (warmup, iterations) = (2_u32, MOST_ITERATIONS);
    let mut interface = Vec::new();
    let mut request = Vec::new();
    for iteration in 0..warmup + iterations {
        let mut times = [Duration::ZERO; 2];
        // Slot 0 is the interface path, slot 1 the request path.
        let order = if iteration % 2 == 0 { [0, 1] } else { [1, 0] };
        for slot in order {
            let started = Instant::now();
            if slot == 0 {
                scenario.baseline(&state).ok_or("no baseline")?.await?;
            } else {
                scenario.operation(&state).await?;
            }
            times[slot] = started.elapsed();
        }
        if iteration >= warmup {
            interface.push(times[0]);
            request.push(times[1]);
        }
    }
    let interface_stats = Stats::from_durations(&interface).ok_or("no timings")?;
    let request_stats = Stats::from_durations(&request).ok_or("no timings")?;
    #[allow(
        clippy::cast_precision_loss,
        reason = "timings in nanoseconds, shown to one decimal"
    )]
    let ratio = |a: u64, b: u64| a as f64 / b as f64;
    let median_factor = ratio(interface_stats.median_ns, request_stats.median_ns);
    let floor_factor = ratio(interface_stats.min_ns, request_stats.min_ns);
    let pair_factor = median(
        interface
            .iter()
            .zip(&request)
            .map(|(i, r)| i.as_secs_f64() / r.as_secs_f64())
            .collect(),
    );
    eprintln!(
        "interface floor {} ns, median {} ns; request floor {} ns, median {} ns; \
         median factor {median_factor:.1}, per-pair median factor {pair_factor:.1}, \
         floor factor {floor_factor:.1}",
        interface_stats.min_ns,
        interface_stats.median_ns,
        request_stats.min_ns,
        request_stats.median_ns
    );
    #[allow(clippy::cast_precision_loss, reason = "a small constant")]
    let required = REQUIRED_FACTOR as f64;
    // The claim is about the product's speed, which is the release build's:
    // CI runs this test with `--release`. An unoptimized build spends its time
    // in the engine, not the system, so it prints the factors and still checks
    // every identity and heard-request condition in the setup.
    if cfg!(debug_assertions) {
        eprintln!("debug build: the factor of {REQUIRED_FACTOR} is asserted in release only");
    } else {
        assert!(
            median_factor >= required,
            "only {median_factor:.1} times faster by the medians"
        );
        assert!(
            pair_factor >= required,
            "only {pair_factor:.1} times faster by the median of the pairs"
        );
    }
    Ok(())
}
