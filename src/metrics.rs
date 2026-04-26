// metrics.rs — lightweight timing instrumentation
#![allow(dead_code)]
//
// A `Timer` records elapsed nanoseconds into a named histogram on Drop.
// `report()` formats a one-line-per-bucket table with count/mean/p50/p95/p99/max.
// `reset()` clears all histograms (used between bench populations).
//
// On wasm32 the timer is a no-op: `std::time::Instant::now()` panics on
// wasm32-unknown-unknown without a host shim. The instrumentation compiles
// to nothing on that target.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

const RING_CAP: usize = 1024;

pub struct Histogram {
    pub count: u64,
    pub total_ns: u128,
    pub min_ns: u64,
    pub max_ns: u64,
    samples: Vec<u64>,
    head: usize,
}

impl Histogram {
    fn new() -> Self {
        Histogram {
            count: 0,
            total_ns: 0,
            min_ns: u64::MAX,
            max_ns: 0,
            samples: Vec::with_capacity(RING_CAP),
            head: 0,
        }
    }

    fn record(&mut self, ns: u64) {
        self.count += 1;
        self.total_ns += ns as u128;
        if ns < self.min_ns {
            self.min_ns = ns;
        }
        if ns > self.max_ns {
            self.max_ns = ns;
        }
        if self.samples.len() < RING_CAP {
            self.samples.push(ns);
        } else {
            self.samples[self.head] = ns;
            self.head = (self.head + 1) % RING_CAP;
        }
    }

    pub fn mean_ns(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            (self.total_ns / self.count as u128) as u64
        }
    }

    pub fn percentile(&self, p: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }
}

static REGISTRY: OnceLock<Mutex<HashMap<&'static str, Histogram>>> = OnceLock::new();
static VALUES: OnceLock<Mutex<HashMap<&'static str, Histogram>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<&'static str, Histogram>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn values_registry() -> &'static Mutex<HashMap<&'static str, Histogram>> {
    VALUES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn record(name: &'static str, ns: u64) {
    if let Ok(mut r) = registry().lock() {
        r.entry(name).or_insert_with(Histogram::new).record(ns);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn record(_name: &'static str, _ns: u64) {}

/// Record a unitless value (count, fan-out, etc.) into the values registry.
pub fn record_value(name: &'static str, v: u64) {
    if let Ok(mut r) = values_registry().lock() {
        r.entry(name).or_insert_with(Histogram::new).record(v);
    }
}

pub fn reset() {
    if let Ok(mut r) = registry().lock() {
        r.clear();
    }
    if let Ok(mut r) = values_registry().lock() {
        r.clear();
    }
}

/// Mean duration in ns for a named timer, or 0 if absent.
pub fn duration_mean_ns(name: &'static str) -> u64 {
    registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.mean_ns()))
        .unwrap_or(0)
}

/// Percentile (0.0..=1.0) of recorded durations, or 0 if absent.
pub fn histogram_percentile_ns(name: &'static str, p: f64) -> u64 {
    registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.percentile(p)))
        .unwrap_or(0)
}

/// Max recorded duration in ns, or 0 if absent.
pub fn histogram_max_ns(name: &'static str) -> u64 {
    registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.max_ns))
        .unwrap_or(0)
}

/// Number of samples recorded for a duration histogram.
pub fn histogram_count(name: &'static str) -> u64 {
    registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.count))
        .unwrap_or(0)
}

/// Mean recorded value for a named counter, or 0 if absent.
pub fn value_mean(name: &'static str) -> u64 {
    values_registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.mean_ns()))
        .unwrap_or(0)
}

/// Total of all recorded samples for a named counter (sum, not count).
/// For e.g. `mesh.bytes_sent`, this is total bytes sent across the process.
pub fn value_total(name: &'static str) -> u128 {
    values_registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.total_ns))
        .unwrap_or(0)
}

/// Number of samples (calls) recorded for a named counter.
/// For e.g. `mesh.bytes_sent`, this is total messages sent.
pub fn value_count(name: &'static str) -> u64 {
    values_registry()
        .lock()
        .ok()
        .and_then(|r| r.get(name).map(|h| h.count))
        .unwrap_or(0)
}

pub fn report() -> String {
    let r = match registry().lock() {
        Ok(g) => g,
        Err(_) => return String::from("(metrics: lock poisoned)\n"),
    };
    let mut entries: Vec<(&&'static str, &Histogram)> = r.iter().collect();
    entries.sort_by_key(|(k, _)| **k);
    let mut s = String::new();
    s.push_str(&format!(
        "{:<28}  {:>8}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}  {:>12}\n",
        "name", "count", "mean", "p50", "p95", "p99", "max", "total"
    ));
    for (name, h) in entries {
        s.push_str(&format!(
            "{:<28}  {:>8}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}  {:>12}\n",
            name,
            h.count,
            fmt_ns(h.mean_ns()),
            fmt_ns(h.percentile(0.50)),
            fmt_ns(h.percentile(0.95)),
            fmt_ns(h.percentile(0.99)),
            fmt_ns(h.max_ns),
            fmt_ns_total(h.total_ns),
        ));
    }
    s
}

/// Formats the values (count) registry as a table. Plain numbers, not durations.
pub fn report_values() -> String {
    let r = match values_registry().lock() {
        Ok(g) => g,
        Err(_) => return String::from("(metrics: values lock poisoned)\n"),
    };
    let mut entries: Vec<(&&'static str, &Histogram)> = r.iter().collect();
    entries.sort_by_key(|(k, _)| **k);
    let mut s = String::new();
    s.push_str(&format!(
        "{:<28}  {:>8}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}  {:>12}\n",
        "name", "count", "mean", "p50", "p95", "p99", "max", "total"
    ));
    for (name, h) in entries {
        s.push_str(&format!(
            "{:<28}  {:>8}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}  {:>12}\n",
            name,
            h.count,
            fmt_count(h.mean_ns()),
            fmt_count(h.percentile(0.50)),
            fmt_count(h.percentile(0.95)),
            fmt_count(h.percentile(0.99)),
            fmt_count(h.max_ns),
            fmt_count_total(h.total_ns),
        ));
    }
    s
}

fn fmt_count(v: u64) -> String {
    if v >= 1_000_000_000 {
        format!("{:.2}G", v as f64 / 1e9)
    } else if v >= 1_000_000 {
        format!("{:.2}M", v as f64 / 1e6)
    } else if v >= 1_000 {
        format!("{:.2}k", v as f64 / 1e3)
    } else {
        format!("{}", v)
    }
}

fn fmt_count_total(v: u128) -> String {
    if v >= 1_000_000_000 {
        format!("{:.2}G", v as f64 / 1e9)
    } else if v >= 1_000_000 {
        format!("{:.2}M", v as f64 / 1e6)
    } else if v >= 1_000 {
        format!("{:.2}k", v as f64 / 1e3)
    } else {
        format!("{}", v)
    }
}

fn fmt_ns(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.2}s", ns as f64 / 1e9)
    } else if ns >= 1_000_000 {
        format!("{:.2}ms", ns as f64 / 1e6)
    } else if ns >= 1_000 {
        format!("{:.2}us", ns as f64 / 1e3)
    } else {
        format!("{}ns", ns)
    }
}

fn fmt_ns_total(ns: u128) -> String {
    let clamped = ns.min(u64::MAX as u128) as u64;
    fmt_ns(clamped)
}

// ---------------------------------------------------------------------------
// Timer guard: records elapsed ns into the named histogram on Drop.
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
pub struct Timer {
    name: &'static str,
    start: Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl Timer {
    pub fn new(name: &'static str) -> Self {
        Timer {
            name,
            start: Instant::now(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Timer {
    fn drop(&mut self) {
        let ns = self.start.elapsed().as_nanos();
        record(self.name, ns.min(u64::MAX as u128) as u64);
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Timer;

#[cfg(target_arch = "wasm32")]
impl Timer {
    pub fn new(_name: &'static str) -> Self {
        Timer
    }
}
