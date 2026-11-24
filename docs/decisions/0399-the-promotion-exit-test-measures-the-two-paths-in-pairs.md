# 0399. The promotion exit test measures the two paths in pairs

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** The exit test of Phase 4 (`bench/tests/promotion.rs`) times the interface path and the request path in pairs, and asserts two factors, each at least ten: the ratio of the two medians, and the median of the per-pair ratios. It runs 40 iterations. It still checks, in its setup, that every answer equals the interface binding's, and prints the medians, the floors, and the three factors. The statistic and the bar are as they were; what changed is the order of measurement and a second, paired factor asserted beside the first.

## Context

On a continuous-integration runner the test measured 9.0 by the medians (interface 161.6 ms, request 17.9 ms), and 11.0 to 11.9 on a quieter machine. The claim did not change between those runs; the load of other programs did.

## Decision

Iteration `i` runs one interface call and one request call straight after one another, and which goes first alternates, so that load falls on both paths alike. (The harness's own loop already does this for the medians; the test now owns its loop in order to keep each pair.) The test asserts the ratio of medians at ten, as before, and also the median of `interface_i / request_i` at ten. The second is the more faithful measure of "ten times faster" when load drifts during the run, because each ratio compares two calls made under nearly the same load. It is asserted beside the first and never instead of it, so the test is stricter than before, not looser. The floors, which [decision 0369](0369-the-comparison-holds-the-floor-of-each-number-and-the-program-has-a-size-budget.md) holds for the benchmark comparison, are printed and not asserted: the floor of the request path is its single luckiest call and flatters the factor.

## Rejected

- **Compare floors.** It raised the measured factor from about 11.5 to 14 to 18 against the same bar, which is an easier test.
- **Lower the factor.** The factor is the product's bar ([section 107](../spec/1-product/107-the-bar.md)).

## Would reopen it

A runner on which the medians and the pair median still disagree by more than a unit, which would show the noise is not shared between the paired calls.
