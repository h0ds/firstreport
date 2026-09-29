//! Min-heap of feeds keyed on `next_fetch_at`, plus the backoff and
//! adaptive-interval policy (plan §5.1). Pure: no clocks, no I/O, so it's
//! fully unit-testable.
//!
//! Adaptive polling: a fetch that yields new items shortens the feed's
//! interval, an unchanged one lengthens it, both within configured bounds.
//! Every scheduled time carries deterministic jitter so feeds sharing an
//! interval drift apart instead of bursting together.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use xxhash_rust::xxh64::xxh64;

use crate::store::writer::FeedState;

/// A feed is declared dead after failing continuously this long.
pub const DEAD_AFTER_SECS: i64 = 7 * 86_400;

/// First retry after a fetch that made no connection (DNS or connect
/// failure); doubles on each further one.
pub const UNREACHABLE_RETRY_SECS: u64 = 120;
/// Consecutive unreachable fetches retried on the short clock before the
/// feed falls back to normal backoff: 2, 4 and 8 min, about 14 min in all.
/// Long enough to ride out a local DNS or network blip, short enough that
/// a host that is really gone backs off like any other failure.
pub const UNREACHABLE_QUICK_TRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct SchedulerCfg {
    pub max_backoff_secs: u64,
    pub adaptive: bool,
    pub min_interval_secs: u64,
    pub max_interval_secs: u64,
    /// Fraction of the interval, e.g. 0.1 for ±10%.
    pub jitter: f64,
}

impl From<&crate::config::Poll> for SchedulerCfg {
    fn from(p: &crate::config::Poll) -> Self {
        Self {
            max_backoff_secs: p.max_backoff_secs,
            adaptive: p.adaptive,
            min_interval_secs: p.min_interval_secs,
            max_interval_secs: p.max_interval_secs,
            jitter: p.jitter,
        }
    }
}

pub struct Scheduler {
    feeds: Vec<FeedState>,
    heap: BinaryHeap<Reverse<(i64, usize)>>,
    cfg: SchedulerCfg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outcome {
    /// 200 with a body.
    Ok,
    /// 304, or a body identical to last time.
    Unchanged,
    /// 4xx/5xx, parse failure, timeout after connecting: anything worth
    /// backing off for.
    #[default]
    Err,
    /// No connection was made (DNS or connect failure). Counts as a
    /// failure but is retried soon at first: when many feeds fail this way
    /// at once it is our network, and a full backoff would leave healthy
    /// feeds looking dead for half an hour.
    Unreachable,
}

/// Everything the poller learned from one fetch.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub outcome: Option<Outcome>,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub retry_after_secs: Option<u64>,
    pub resolved_url: Option<String>,
    pub new_items: u32,
    /// Updated feed health, when the fetch had a body to judge.
    pub health: Option<crate::steward::Health>,
}

impl Report {
    pub fn outcome(&self) -> Outcome {
        self.outcome.unwrap_or(Outcome::Err)
    }
}

impl Scheduler {
    pub fn new(feeds: Vec<FeedState>, cfg: SchedulerCfg) -> Self {
        let mut heap = BinaryHeap::with_capacity(feeds.len());
        for (i, f) in feeds.iter().enumerate() {
            if !f.dead {
                heap.push(Reverse((f.next_fetch_at, i)));
            }
        }
        Self { feeds, heap, cfg }
    }

    pub fn len(&self) -> usize {
        self.feeds.len()
    }

    pub fn feed(&self, idx: usize) -> &FeedState {
        &self.feeds[idx]
    }

    /// When the earliest feed is due, if any are scheduled.
    pub fn next_due_at(&self) -> Option<i64> {
        self.heap.peek().map(|Reverse((t, _))| *t)
    }

    /// Pop one feed that is due at or before `now`.
    pub fn pop_due(&mut self, now: i64) -> Option<usize> {
        match self.heap.peek() {
            Some(Reverse((t, _))) if *t <= now => self.heap.pop().map(|Reverse((_, i))| i),
            _ => None,
        }
    }

    /// Deterministic jitter in `[-jitter, +jitter] * secs`, keyed on the
    /// feed and the moment, so replays and tests are stable.
    fn jittered(&self, url: &str, now: i64, secs: u64) -> i64 {
        if self.cfg.jitter <= 0.0 {
            return secs as i64;
        }
        let h = xxh64(format!("{url}|{now}").as_bytes(), 0);
        let unit = (h % 10_001) as f64 / 10_000.0 * 2.0 - 1.0; // [-1, 1]
        (secs as f64 * (1.0 + unit * self.cfg.jitter)).round() as i64
    }

    /// Record the result of a fetch and put the feed back on the heap.
    /// Returns the updated state for persistence.
    pub fn complete(&mut self, idx: usize, now: i64, r: Report) -> &FeedState {
        let cfg = self.cfg.clone();
        let outcome = r.outcome();
        let retry_after = r.retry_after_secs;
        let f = &mut self.feeds[idx];
        f.last_fetch_at = Some(now);
        f.last_status = r.status;
        f.last_error = r.error.clone();
        f.last_items = r.new_items;
        if r.health.is_some() {
            f.health = r.health.clone();
        }
        // A quarantined feed keeps polling so its signals keep updating,
        // but at its maximum interval (docs/steward.md §2).
        let quarantined = f.health.as_ref().is_some_and(|h| h.is_quarantined(now));
        if let Some(u) = r.resolved_url.clone() {
            if u != f.url {
                f.resolved_url = Some(u);
            }
        }
        match outcome {
            Outcome::Ok | Outcome::Unchanged => {
                f.fail_count = 0;
                f.last_ok_at = Some(now);
                if r.etag.is_some() {
                    f.etag = r.etag;
                }
                if r.last_modified.is_some() {
                    f.last_modified = r.last_modified;
                }
                if cfg.adaptive {
                    let lo = cfg.min_interval_secs.max(60);
                    let hi = cfg.max_interval_secs.max(lo);
                    f.interval_secs = if r.new_items > 0 {
                        (f.interval_secs * 2 / 3).max(lo)
                    } else {
                        (f.interval_secs * 4 / 3).min(hi)
                    };
                }
            }
            Outcome::Err | Outcome::Unreachable => {
                f.fail_count = f.fail_count.saturating_add(1);
                let failing_since = f.last_ok_at.unwrap_or(now);
                if f.last_ok_at.is_some() && now - failing_since >= DEAD_AFTER_SECS {
                    f.dead = true;
                }
            }
        }
        let wait = match outcome {
            Outcome::Unreachable if f.fail_count <= UNREACHABLE_QUICK_TRIES => {
                (UNREACHABLE_RETRY_SECS << (f.fail_count.max(1) - 1)).max(retry_after.unwrap_or(0))
            }
            Outcome::Err | Outcome::Unreachable => {
                let backoff = f
                    .base_interval_secs
                    .saturating_mul(1u64 << f.fail_count.min(20))
                    .min(cfg.max_backoff_secs);
                backoff.max(retry_after.unwrap_or(0))
            }
            _ if quarantined => cfg
                .max_interval_secs
                .max(f.interval_secs)
                .max(retry_after.unwrap_or(0)),
            _ => f.interval_secs.max(retry_after.unwrap_or(0)),
        };
        let url = f.url.clone();
        let jittered = self.jittered(&url, now, wait);
        let f = &mut self.feeds[idx];
        f.next_fetch_at = now + jittered;
        if !f.dead {
            self.heap.push(Reverse((f.next_fetch_at, idx)));
        }
        &self.feeds[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs(url: &str, next: i64) -> FeedState {
        FeedState {
            url: url.into(),
            outlet_id: "o".into(),
            kind: None,
            base_interval_secs: 600,
            interval_secs: 600,
            etag: None,
            last_modified: None,
            next_fetch_at: next,
            fail_count: 0,
            last_ok_at: None,
            dead: false,
            resolved_url: None,
            last_fetch_at: None,
            last_status: None,
            last_error: None,
            last_items: 0,
            health: None,
        }
    }

    fn cfg(adaptive: bool, jitter: f64) -> SchedulerCfg {
        SchedulerCfg {
            max_backoff_secs: 21_600,
            adaptive,
            min_interval_secs: 300,
            max_interval_secs: 3_600,
            jitter,
        }
    }

    fn ok(items: u32) -> Report {
        Report {
            outcome: Some(Outcome::Ok),
            status: Some(200),
            new_items: items,
            ..Default::default()
        }
    }

    fn err() -> Report {
        Report {
            outcome: Some(Outcome::Err),
            error: Some("boom".into()),
            ..Default::default()
        }
    }

    #[test]
    fn pops_in_due_order_and_backs_off() {
        let mut s = Scheduler::new(vec![fs("a", 100), fs("b", 50)], cfg(false, 0.0));
        assert_eq!(s.next_due_at(), Some(50));
        assert_eq!(s.pop_due(49), None);
        assert_eq!(s.pop_due(100), Some(1));
        assert_eq!(s.pop_due(100), Some(0));
        assert_eq!(s.pop_due(100), None);

        let f = s.complete(
            1,
            100,
            Report {
                etag: Some("W/1".into()),
                ..ok(0)
            },
        );
        assert_eq!(f.next_fetch_at, 700);
        assert_eq!(f.etag.as_deref(), Some("W/1"));
        assert_eq!(f.last_status, Some(200));

        let f = s.complete(0, 100, err());
        assert_eq!(f.next_fetch_at, 100 + 1200);
        assert_eq!(f.last_error.as_deref(), Some("boom"));
        s.pop_due(1300);
        let f = s.complete(0, 1300, err());
        assert_eq!(f.next_fetch_at, 1300 + 2400);
        for _ in 0..10 {
            let t = s.feed(0).next_fetch_at;
            s.pop_due(t);
            s.complete(0, t, err());
        }
        let f = s.feed(0);
        assert!(f.next_fetch_at - f.last_fetch_at.unwrap() <= 21_600);
        assert!(
            !f.dead,
            "never succeeded, so no baseline to declare dead from"
        );
    }

    #[test]
    fn quarantined_feed_waits_its_maximum_interval() {
        use crate::steward::{Health, Quarantine};
        let mut s = Scheduler::new(vec![fs("a", 0)], cfg(true, 0.0));
        s.pop_due(0);
        let health = Health {
            quarantine: Some(Quarantine {
                reason: "test".into(),
                since: 0,
                until: 86_400,
            }),
            ..Default::default()
        };
        let f = s.complete(
            0,
            0,
            Report {
                health: Some(health),
                ..ok(5)
            },
        );
        assert_eq!(f.next_fetch_at, 3_600, "max interval while quarantined");
        assert!(f.health.as_ref().unwrap().is_quarantined(100));
        // Once the day is up the health is carried but the wait is normal.
        s.pop_due(3_600);
        let f = s.complete(0, 90_000, ok(5));
        assert!(f.next_fetch_at - 90_000 < 3_600);
    }

    #[test]
    fn retry_after_wins_over_shorter_waits() {
        let mut s = Scheduler::new(vec![fs("a", 0)], cfg(false, 0.0));
        s.pop_due(0);
        let r = Report {
            outcome: Some(Outcome::Err),
            status: Some(429),
            retry_after_secs: Some(5_000),
            ..Default::default()
        };
        assert_eq!(s.complete(0, 0, r).next_fetch_at, 5_000);
    }

    #[test]
    fn adaptive_interval_moves_within_bounds() {
        let mut s = Scheduler::new(vec![fs("a", 0)], cfg(true, 0.0));
        let mut t = 0;
        for _ in 0..10 {
            s.pop_due(t);
            let f = s.complete(0, t, ok(3));
            t = f.next_fetch_at;
        }
        assert_eq!(s.feed(0).interval_secs, 300, "floors at min_interval");
        for _ in 0..20 {
            s.pop_due(t);
            let f = s.complete(
                0,
                t,
                Report {
                    outcome: Some(Outcome::Unchanged),
                    status: Some(304),
                    ..Default::default()
                },
            );
            t = f.next_fetch_at;
        }
        assert_eq!(s.feed(0).interval_secs, 3_600, "ceils at max_interval");
        // Backoff on error is computed from the base interval, not the adapted one.
        s.pop_due(t);
        let f = s.complete(0, t, err());
        assert_eq!(f.next_fetch_at - t, 1200);
    }

    #[test]
    fn jitter_is_bounded_and_deterministic() {
        let mut a = Scheduler::new(vec![fs("a", 0), fs("b", 0)], cfg(false, 0.1));
        let mut b = Scheduler::new(vec![fs("a", 0), fs("b", 0)], cfg(false, 0.1));
        a.pop_due(0);
        a.pop_due(0);
        b.pop_due(0);
        b.pop_due(0);
        let fa = a.complete(0, 0, ok(0)).next_fetch_at;
        let fb = a.complete(1, 0, ok(0)).next_fetch_at;
        assert!((540..=660).contains(&fa) && (540..=660).contains(&fb));
        assert_ne!(fa, fb, "different feeds drift apart");
        assert_eq!(
            b.complete(0, 0, ok(0)).next_fetch_at,
            fa,
            "same inputs, same schedule"
        );
    }

    #[test]
    fn permanent_move_is_recorded_and_dead_after_seven_days() {
        let mut s = Scheduler::new(vec![fs("a", 0)], cfg(false, 0.0));
        s.pop_due(0);
        let f = s.complete(
            0,
            0,
            Report {
                resolved_url: Some("https://new/feed".into()),
                ..ok(1)
            },
        );
        assert_eq!(f.resolved_url.as_deref(), Some("https://new/feed"));
        let mut now = 600;
        for _ in 0..40 {
            s.pop_due(now);
            s.complete(0, now, err());
            now = s.feed(0).next_fetch_at;
            if s.feed(0).dead {
                break;
            }
        }
        assert!(s.feed(0).dead);
        assert_eq!(s.next_due_at(), None);
    }

    #[test]
    fn unreachable_retries_soon_then_backs_off() {
        let unreachable = || Report {
            outcome: Some(Outcome::Unreachable),
            error: Some("dns error".into()),
            ..Default::default()
        };
        let mut s = Scheduler::new(vec![fs("a", 0)], cfg(false, 0.0));
        // 2, 4, 8 minutes: a local DNS blip is ridden out in minutes.
        let mut now = 0;
        for want in [120, 240, 480] {
            let f = s.complete(0, now, unreachable());
            assert_eq!(f.next_fetch_at - now, want);
            now = f.next_fetch_at;
        }
        assert_eq!(s.feed(0).fail_count, 3, "still counted as failures");
        // Then normal backoff: base 600 * 2^4.
        let next = s.complete(0, now, unreachable()).next_fetch_at;
        assert_eq!(next - now, 600 * 16);
        // A success clears it.
        assert_eq!(s.complete(0, next, ok(1)).fail_count, 0);
    }

    #[test]
    fn answered_errors_still_back_off_fully() {
        let err = Report {
            outcome: Some(Outcome::Err),
            status: Some(404),
            ..Default::default()
        };
        let mut s = Scheduler::new(vec![fs("a", 0)], cfg(false, 0.0));
        assert_eq!(s.complete(0, 0, err).next_fetch_at, 1200);
    }
}
