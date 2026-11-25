//! Spans and metrics, kept in memory.
//!
//! Nothing here is sent anywhere. With export off, which is the only state
//! that is built, finished spans wait in a bounded buffer and metrics in
//! their series, for `engine.status`, for tests, and for the benchmark
//! harness to read.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use a2m_core::Timestamp;

use crate::attr::Attr;
use crate::clock::micros;

/// How many finished spans are kept. When the buffer is full the oldest span
/// is dropped and counted.
pub const SPAN_BUFFER: usize = 4096;

/// The upper bounds, in microseconds, of the buckets every histogram of
/// durations uses. A duration above the last bound is counted in a final
/// bucket with no bound.
const BUCKET_BOUNDS_MICROS: [u64; 18] = [
    100, 250, 500, 1_000, 2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
    1_000_000, 2_500_000, 5_000_000, 10_000_000, 30_000_000, 60_000_000,
];

/// A span that has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedSpan {
    /// The span's name: a fixed name from the source, or for an MCP call the
    /// method followed by the tool.
    pub name: String,
    /// The trace the span belongs to.
    pub trace_id: u128,
    /// The span's own id, unique within the process.
    pub span_id: u64,
    /// The id of the span this one is a child of.
    pub parent: Option<u64>,
    /// The span's attributes.
    pub attrs: Vec<Attr>,
    /// When the span started.
    pub started: Timestamp,
    /// How long it lasted.
    pub took: Duration,
}

/// One series of a counter: a name, its attributes, and the count so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterSample {
    /// The counter's name.
    pub name: &'static str,
    /// The series' attributes, as keys and values, sorted.
    pub attrs: Vec<(&'static str, String)>,
    /// The count.
    pub value: u64,
}

/// One series of a histogram of durations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistogramSample {
    /// The histogram's name.
    pub name: &'static str,
    /// The series' attributes, as keys and values, sorted.
    pub attrs: Vec<(&'static str, String)>,
    /// How many durations were recorded.
    pub count: u64,
    /// Their sum.
    pub sum: Duration,
    /// The shortest.
    pub min: Duration,
    /// The longest.
    pub max: Duration,
    /// How many fell at or under each bound and above the bound before it.
    /// The last bucket has no bound.
    pub buckets: Vec<Bucket>,
}

/// One bucket of a histogram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket {
    /// The longest duration the bucket counts, or `None` for the last one.
    pub up_to: Option<Duration>,
    /// How many durations fell in it.
    pub count: u64,
}

/// Every metric at one moment, in a stable order: by name, then by attributes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metrics {
    /// The counters.
    pub counters: Vec<CounterSample>,
    /// The histograms of durations.
    pub histograms: Vec<HistogramSample>,
    /// How many finished spans were dropped because the buffer was full.
    pub spans_dropped: u64,
}

impl Metrics {
    /// The value of the counter `name` whose series has exactly `attrs`, or
    /// zero when nothing was counted.
    #[must_use]
    pub fn counter(&self, name: &str, attrs: &[Attr]) -> u64 {
        let wanted = series_attrs(attrs);
        self.counters
            .iter()
            .find(|sample| sample.name == name && sample.attrs == wanted)
            .map_or(0, |sample| sample.value)
    }

    /// The histogram `name` whose series has exactly `attrs`.
    #[must_use]
    pub fn histogram(&self, name: &str, attrs: &[Attr]) -> Option<&HistogramSample> {
        let wanted = series_attrs(attrs);
        self.histograms
            .iter()
            .find(|sample| sample.name == name && sample.attrs == wanted)
    }
}

type SeriesKey = (&'static str, Vec<(&'static str, String)>);

#[derive(Debug, Clone)]
struct Histogram {
    count: u64,
    sum_micros: u64,
    min_micros: u64,
    max_micros: u64,
    buckets: [u64; BUCKET_BOUNDS_MICROS.len() + 1],
}

/// The store behind a `LogsAndTraces` handle.
#[derive(Debug, Default)]
pub(crate) struct Collector {
    spans: VecDeque<FinishedSpan>,
    spans_dropped: u64,
    counters: BTreeMap<SeriesKey, u64>,
    histograms: BTreeMap<SeriesKey, Histogram>,
}

impl Collector {
    pub(crate) fn finish_span(&mut self, span: FinishedSpan) {
        if self.spans.len() == SPAN_BUFFER {
            self.spans.pop_front();
            self.spans_dropped = self.spans_dropped.saturating_add(1);
        }
        self.spans.push_back(span);
    }

    pub(crate) fn count(&mut self, name: &'static str, attrs: &[Attr], by: u64) {
        let value = self
            .counters
            .entry((name, series_attrs(attrs)))
            .or_insert(0);
        *value = value.saturating_add(by);
    }

    pub(crate) fn observe(&mut self, name: &'static str, attrs: &[Attr], took: Duration) {
        let took = micros(took);
        let histogram = self
            .histograms
            .entry((name, series_attrs(attrs)))
            .or_insert(Histogram {
                count: 0,
                sum_micros: 0,
                min_micros: u64::MAX,
                max_micros: 0,
                buckets: [0; BUCKET_BOUNDS_MICROS.len() + 1],
            });
        histogram.count = histogram.count.saturating_add(1);
        histogram.sum_micros = histogram.sum_micros.saturating_add(took);
        histogram.min_micros = histogram.min_micros.min(took);
        histogram.max_micros = histogram.max_micros.max(took);
        let bucket = BUCKET_BOUNDS_MICROS
            .iter()
            .position(|bound| took <= *bound)
            .unwrap_or(BUCKET_BOUNDS_MICROS.len());
        if let Some(count) = histogram.buckets.get_mut(bucket) {
            *count = count.saturating_add(1);
        }
    }

    pub(crate) fn spans(&self) -> Vec<FinishedSpan> {
        self.spans.iter().cloned().collect()
    }

    pub(crate) fn metrics(&self) -> Metrics {
        let counters = self
            .counters
            .iter()
            .map(|((name, attrs), value)| CounterSample {
                name,
                attrs: attrs.clone(),
                value: *value,
            })
            .collect();
        let histograms = self
            .histograms
            .iter()
            .map(|((name, attrs), histogram)| HistogramSample {
                name,
                attrs: attrs.clone(),
                count: histogram.count,
                sum: Duration::from_micros(histogram.sum_micros),
                min: Duration::from_micros(histogram.min_micros),
                max: Duration::from_micros(histogram.max_micros),
                buckets: histogram
                    .buckets
                    .iter()
                    .enumerate()
                    .map(|(index, count)| Bucket {
                        up_to: BUCKET_BOUNDS_MICROS
                            .get(index)
                            .map(|bound| Duration::from_micros(*bound)),
                        count: *count,
                    })
                    .collect(),
            })
            .collect();
        Metrics {
            counters,
            histograms,
            spans_dropped: self.spans_dropped,
        }
    }
}

/// A series' attributes as keys and values, sorted so that the order they
/// were given in does not make two series of one.
fn series_attrs(attrs: &[Attr]) -> Vec<(&'static str, String)> {
    let mut pairs: Vec<_> = attrs
        .iter()
        .map(|attr| (attr.key(), attr.value()))
        .collect();
    pairs.sort();
    pairs
}
