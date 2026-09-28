//! Min-heap of feeds keyed on `next_fetch_at`, plus the backoff policy
//! (plan §5.1). Pure: no clocks, no I/O, so it's fully unit-testable.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::store::writer::FeedState;

/// A feed is declared dead after failing continuously this long.
pub const DEAD_AFTER_SECS: i64 = 7 * 86_400;

pub struct Scheduler {
    feeds: Vec<FeedState>,
    heap: BinaryHeap<Reverse<(i64, usize)>>,
    max_backoff_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 200 with a body, or 304.
    Ok,
    /// Network error, 5xx, parse failure, anything worth backing off for.
    Err,
}

impl Scheduler {
    pub fn new(feeds: Vec<FeedState>, max_backoff_secs: u64) -> Self {
        let mut heap = BinaryHeap::with_capacity(feeds.len());
        for (i, f) in feeds.iter().enumerate() {
            if !f.dead {
                heap.push(Reverse((f.next_fetch_at, i)));
            }
        }
        Self {
            feeds,
            heap,
            max_backoff_secs,
        }
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

    /// Record the outcome of a fetch and put the feed back on the heap.
    /// Returns the updated state for persistence.
    pub fn complete(
        &mut self,
        idx: usize,
        now: i64,
        outcome: Outcome,
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> &FeedState {
        let max_backoff = self.max_backoff_secs;
        let f = &mut self.feeds[idx];
        match outcome {
            Outcome::Ok => {
                f.fail_count = 0;
                f.last_ok_at = Some(now);
                if etag.is_some() {
                    f.etag = etag;
                }
                if last_modified.is_some() {
                    f.last_modified = last_modified;
                }
                f.next_fetch_at = now + f.interval_secs as i64;
            }
            Outcome::Err => {
                f.fail_count = f.fail_count.saturating_add(1);
                let backoff = f
                    .interval_secs
                    .saturating_mul(1u64 << f.fail_count.min(20))
                    .min(max_backoff);
                f.next_fetch_at = now + backoff as i64;
                let failing_since = f.last_ok_at.unwrap_or(now - backoff as i64);
                if f.last_ok_at.is_some() && now - failing_since >= DEAD_AFTER_SECS {
                    f.dead = true;
                }
            }
        }
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
            interval_secs: 600,
            etag: None,
            last_modified: None,
            next_fetch_at: next,
            fail_count: 0,
            last_ok_at: None,
            dead: false,
        }
    }

    #[test]
    fn pops_in_due_order_and_backs_off() {
        let mut s = Scheduler::new(vec![fs("a", 100), fs("b", 50)], 21_600);
        assert_eq!(s.next_due_at(), Some(50));
        assert_eq!(s.pop_due(49), None);
        assert_eq!(s.pop_due(100), Some(1));
        assert_eq!(s.pop_due(100), Some(0));
        assert_eq!(s.pop_due(100), None);

        let f = s.complete(1, 100, Outcome::Ok, Some("W/1".into()), None);
        assert_eq!(f.next_fetch_at, 700);
        assert_eq!(f.etag.as_deref(), Some("W/1"));

        let f = s.complete(0, 100, Outcome::Err, None, None);
        assert_eq!(f.next_fetch_at, 100 + 1200);
        s.pop_due(1300);
        let f = s.complete(0, 1300, Outcome::Err, None, None);
        assert_eq!(f.next_fetch_at, 1300 + 2400);
        // capped
        for _ in 0..10 {
            let t = s.feed(0).next_fetch_at;
            s.pop_due(t);
            s.complete(0, t, Outcome::Err, None, None);
        }
        let f = s.feed(0);
        assert!(f.next_fetch_at - f.last_ok_at.unwrap_or(0) <= 21_600 + 1_000_000);
        assert!(
            !f.dead,
            "never succeeded, so no baseline to declare dead from"
        );
    }

    #[test]
    fn dead_after_seven_days_of_failures() {
        let mut s = Scheduler::new(vec![fs("a", 0)], 21_600);
        s.pop_due(0);
        s.complete(0, 0, Outcome::Ok, None, None);
        let mut now = 600;
        for _ in 0..40 {
            s.pop_due(now);
            s.complete(0, now, Outcome::Err, None, None);
            now = s.feed(0).next_fetch_at;
            if s.feed(0).dead {
                break;
            }
        }
        assert!(s.feed(0).dead);
        assert_eq!(s.next_due_at(), None);
    }
}
