//! Process-wide counters for `/v1/stats`. Atomics only; no locks on the
//! hot path.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

/// Per-item embed latencies (µs) kept for percentiles. Ring of the last N.
const LATENCY_RING: usize = 4096;

#[derive(Default)]
pub struct Stats {
    pub started_at: AtomicU64,
    pub fetches: AtomicU64,
    pub fetch_304: AtomicU64,
    pub fetch_errors: AtomicU64,
    pub parse_errors: AtomicU64,
    pub items_seen: AtomicU64,
    pub items_new: AtomicU64,
    pub bloom_hits: AtomicU64,
    pub items_embedded: AtomicU64,
    pub embed_batches: AtomicU64,
    pub assigns: AtomicU64,
    pub active_clusters: AtomicU64,
    embed_latency_us: Mutex<Vec<u64>>,
    assign_latency_us: Mutex<Vec<u64>>,
}

#[derive(Debug, Serialize)]
pub struct Latency {
    pub p50_us: u64,
    pub p99_us: u64,
    pub samples: usize,
}

fn percentiles(ring: &Mutex<Vec<u64>>) -> Latency {
    let ring = ring.lock().unwrap_or_else(|p| p.into_inner());
    if ring.is_empty() {
        return Latency {
            p50_us: 0,
            p99_us: 0,
            samples: 0,
        };
    }
    let mut v = ring.clone();
    v.sort_unstable();
    let pct = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    Latency {
        p50_us: pct(0.5),
        p99_us: pct(0.99),
        samples: v.len(),
    }
}

fn record(ring: &Mutex<Vec<u64>>, us: u64) {
    let mut ring = ring.lock().unwrap_or_else(|p| p.into_inner());
    if ring.len() >= LATENCY_RING {
        ring.remove(0);
    }
    ring.push(us);
}

#[derive(Debug, Serialize)]
pub struct StatsSnapshot {
    pub uptime_secs: u64,
    pub fetches: u64,
    pub fetch_304: u64,
    pub fetch_errors: u64,
    pub parse_errors: u64,
    pub items_seen: u64,
    pub items_new: u64,
    pub bloom_hits: u64,
    pub items_embedded: u64,
    pub embed_batches: u64,
    /// Amortized per-item latency: batch wall time divided by batch size.
    pub embed_latency: Latency,
    pub assigns: u64,
    pub active_clusters: u64,
    /// Per-item assignment latency (flat cosine scan over the active set).
    pub assign_latency: Latency,
}

impl Stats {
    pub fn record_embed(&self, n: usize, elapsed: Duration) {
        if n == 0 {
            return;
        }
        self.items_embedded.fetch_add(n as u64, Relaxed);
        self.embed_batches.fetch_add(1, Relaxed);
        record(
            &self.embed_latency_us,
            (elapsed.as_micros() as u64) / n as u64,
        );
    }

    pub fn record_assign(&self, elapsed: Duration, active: usize) {
        self.assigns.fetch_add(1, Relaxed);
        self.active_clusters.store(active as u64, Relaxed);
        record(&self.assign_latency_us, elapsed.as_micros() as u64);
    }

    pub fn inc(&self, c: &AtomicU64) {
        c.fetch_add(1, Relaxed);
    }
    pub fn add(&self, c: &AtomicU64, n: u64) {
        c.fetch_add(n, Relaxed);
    }
    pub fn snapshot(&self, now: i64) -> StatsSnapshot {
        StatsSnapshot {
            uptime_secs: (now as u64).saturating_sub(self.started_at.load(Relaxed)),
            fetches: self.fetches.load(Relaxed),
            fetch_304: self.fetch_304.load(Relaxed),
            fetch_errors: self.fetch_errors.load(Relaxed),
            parse_errors: self.parse_errors.load(Relaxed),
            items_seen: self.items_seen.load(Relaxed),
            items_new: self.items_new.load(Relaxed),
            bloom_hits: self.bloom_hits.load(Relaxed),
            items_embedded: self.items_embedded.load(Relaxed),
            embed_batches: self.embed_batches.load(Relaxed),
            embed_latency: percentiles(&self.embed_latency_us),
            assigns: self.assigns.load(Relaxed),
            active_clusters: self.active_clusters.load(Relaxed),
            assign_latency: percentiles(&self.assign_latency_us),
        }
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
