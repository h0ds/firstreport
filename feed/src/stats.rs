//! Process-wide counters for `/v1/stats`. Atomics only; no locks on the
//! hot path.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use serde::Serialize;

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
}

impl Stats {
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
        }
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
