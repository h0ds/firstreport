//! Feed health (docs/steward.md §2): what the poller sees on every
//! fetch, kept per feed over a rolling window, judged by stated rules,
//! acted on in the open. This judges the feed as a data pipe. It
//! knows nothing about the outlet and never will.

use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Fetches with a body kept per feed.
pub const WINDOW: usize = 20;
/// How long a quarantine lasts before the feed is judged afresh.
pub const QUARANTINE_SECS: i64 = 86_400;
/// Rule 1: this many fetches in the window, half or more failing to parse.
pub const PARSE_MIN_FETCHES: usize = 10;
/// Rule 2: this many fetches with new items, half or more duplicates.
pub const DUP_MIN_FETCHES: usize = 5;
/// Rule 3: a fetch with at least this many new items and this multiple of the median.
pub const FLOOD_MIN_ITEMS: u32 = 50;
pub const FLOOD_MULTIPLE: u32 = 10;
/// Rule 4: median skew above this, or future rate above a tenth.
pub const SKEW_UNRELIABLE_SECS: i64 = 7 * 86_400;

/// One fetch that returned a body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FetchObs {
    pub at: i64,
    pub parsed: bool,
    /// Items new to the store from this fetch.
    pub new_items: u32,
    /// Of those, items whose text fingerprint matched the feed's previous
    /// listing or another item in this fetch: republished under a new URL.
    pub dup_items: u32,
    /// Median of (fetched − claimed published) over the new items that
    /// carried a time; None when none did.
    pub skew_secs: Option<i64>,
    /// New items whose claimed time was ahead of the fetch and was clamped.
    pub future_items: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Quarantine {
    /// One sentence naming the rule.
    pub reason: String,
    pub since: i64,
    pub until: i64,
}

/// Signals over the window, as `/v1/feeds` shows them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Signals {
    pub fetches: usize,
    pub parse_error_rate: f32,
    pub dup_rate: f32,
    pub median_skew_secs: Option<i64>,
    pub future_rate: f32,
    /// Latest fetch's new items over the window's median, when the
    /// median is above zero.
    pub burst: Option<f32>,
    pub empty_run: usize,
}

/// Per-feed rolling state, persisted on the feed row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
    #[serde(default)]
    pub window: VecDeque<FetchObs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<Quarantine>,
    /// Rule 4: claimed publish times are not to be trusted for this feed.
    #[serde(default)]
    pub timestamps_unreliable: bool,
    #[serde(default)]
    pub signals: Signals,
}

/// What the poller does with this fetch's items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub ingest: bool,
    pub quarantined: bool,
    pub reason: Option<String>,
}

fn median(v: &mut [i64]) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

impl Health {
    pub fn is_quarantined(&self, now: i64) -> bool {
        self.quarantine.as_ref().is_some_and(|q| now < q.until)
    }

    fn compute_signals(&mut self) {
        let w = &self.window;
        let fetches = w.len();
        let parse_errors = w.iter().filter(|o| !o.parsed).count();
        let with_items: Vec<&FetchObs> = w.iter().filter(|o| o.parsed && o.new_items > 0).collect();
        let new_total: u32 = with_items.iter().map(|o| o.new_items).sum();
        let dup_total: u32 = with_items.iter().map(|o| o.dup_items).sum();
        let future_total: u32 = with_items.iter().map(|o| o.future_items).sum();
        let mut skews: Vec<i64> = w.iter().filter_map(|o| o.skew_secs).collect();
        let mut counts: Vec<i64> = w
            .iter()
            .filter(|o| o.parsed)
            .map(|o| o.new_items as i64)
            .collect();
        let med_items = median(&mut counts).unwrap_or(0);
        let last = w.back();
        let burst = match (last, med_items) {
            (Some(l), m) if m > 0 && l.parsed => Some(l.new_items as f32 / m as f32),
            _ => None,
        };
        let empty_run = w
            .iter()
            .rev()
            .take_while(|o| o.parsed && o.new_items == 0)
            .count();
        self.signals = Signals {
            fetches,
            parse_error_rate: if fetches == 0 {
                0.0
            } else {
                parse_errors as f32 / fetches as f32
            },
            dup_rate: if new_total == 0 {
                0.0
            } else {
                dup_total as f32 / new_total as f32
            },
            median_skew_secs: median(&mut skews),
            future_rate: if new_total == 0 {
                0.0
            } else {
                future_total as f32 / new_total as f32
            },
            burst,
            empty_run,
        };
    }

    /// Record a fetch and apply the rules. Called once per fetch that
    /// returned a body; fetch failures are the scheduler's business.
    pub fn observe(&mut self, obs: FetchObs, now: i64) -> Decision {
        let items_in_fetch = obs.new_items;
        let parsed = obs.parsed;
        let fetch_dups = obs.dup_items;
        self.window.push_back(obs);
        while self.window.len() > WINDOW {
            self.window.pop_front();
        }
        // Rule 5: a quarantine that has run its course is lifted here and
        // the fetch judged afresh below.
        if let Some(q) = &self.quarantine {
            if now >= q.until {
                self.quarantine = None;
            }
        }
        self.compute_signals();
        let s = &self.signals;
        let mut reason: Option<String> = None;
        if s.fetches >= PARSE_MIN_FETCHES && s.parse_error_rate >= 0.5 {
            reason = Some(format!(
                "{} of the last {} fetches failed to parse",
                (s.parse_error_rate * s.fetches as f32).round() as usize,
                s.fetches
            ));
        }
        let with_items = self
            .window
            .iter()
            .filter(|o| o.parsed && o.new_items > 0)
            .count();
        if reason.is_none() && with_items >= DUP_MIN_FETCHES && s.dup_rate >= 0.5 {
            reason = Some(format!(
                "{:.0}% of new items over the last {} fetches were the feed's own earlier items under new URLs",
                s.dup_rate * 100.0,
                with_items
            ));
        }
        let flood = parsed
            && items_in_fetch >= FLOOD_MIN_ITEMS
            && s.burst.is_some_and(|b| b >= FLOOD_MULTIPLE as f32);
        if reason.is_none() && flood {
            reason = Some(format!(
                "{} new items in one fetch, {:.0} times this feed's median",
                items_in_fetch,
                s.burst.unwrap_or(0.0)
            ));
        }
        self.timestamps_unreliable = s.median_skew_secs.is_some_and(|m| m > SKEW_UNRELIABLE_SECS)
            || (with_items >= DUP_MIN_FETCHES && s.future_rate > 0.1);
        let _ = fetch_dups;
        if let Some(r) = reason {
            let already = self.is_quarantined(now);
            if !already {
                self.quarantine = Some(Quarantine {
                    reason: r.clone(),
                    since: now,
                    until: now + QUARANTINE_SECS,
                });
            }
            return Decision {
                ingest: false,
                quarantined: true,
                reason: Some(
                    self.quarantine
                        .as_ref()
                        .map(|q| q.reason.clone())
                        .unwrap_or(r),
                ),
            };
        }
        if self.is_quarantined(now) {
            // Still inside a quarantine whose cause has not recurred on
            // this fetch: hold the items until the day is up.
            return Decision {
                ingest: false,
                quarantined: true,
                reason: self.quarantine.as_ref().map(|q| q.reason.clone()),
            };
        }
        Decision {
            ingest: true,
            quarantined: false,
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(at: i64, new: u32) -> FetchObs {
        FetchObs {
            at,
            parsed: true,
            new_items: new,
            dup_items: 0,
            skew_secs: Some(600),
            future_items: 0,
        }
    }

    #[test]
    fn healthy_feed_ingests_and_reports_signals() {
        let mut h = Health::default();
        for i in 0..30 {
            let d = h.observe(ok(i * 900, 5), i * 900);
            assert!(d.ingest && !d.quarantined);
        }
        assert_eq!(h.window.len(), WINDOW);
        assert_eq!(h.signals.fetches, WINDOW);
        assert_eq!(h.signals.parse_error_rate, 0.0);
        assert_eq!(h.signals.burst, Some(1.0));
        assert_eq!(h.signals.median_skew_secs, Some(600));
        assert!(!h.timestamps_unreliable);
        let json = serde_json::to_string(&h).unwrap();
        let back: Health = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn parse_failures_quarantine_after_ten_fetches() {
        let mut h = Health::default();
        for i in 0..9 {
            let mut o = ok(i, 0);
            o.parsed = i % 2 == 0;
            assert!(h.observe(o, i).ingest, "not yet: fewer than ten fetches");
        }
        let mut o = ok(9, 0);
        o.parsed = false;
        let d = h.observe(o, 9);
        assert!(d.quarantined && !d.ingest);
        assert!(d.reason.unwrap().contains("failed to parse"));
        assert_eq!(h.quarantine.as_ref().unwrap().until, 9 + QUARANTINE_SECS);
    }

    #[test]
    fn duplicates_quarantine_and_lift_after_a_day() {
        let mut h = Health::default();
        for i in 0..5 {
            let mut o = ok(i * 900, 10);
            o.dup_items = 6;
            let d = h.observe(o, i * 900);
            if i < 4 {
                assert!(d.ingest, "fetch {i}");
            } else {
                assert!(d.quarantined, "fifth fetch with new items");
                assert!(d.reason.unwrap().contains("under new URLs"));
            }
        }
        // inside the day: still held even if this fetch is clean
        let d = h.observe(ok(5 * 900, 10), 5 * 900);
        assert!(!d.ingest && d.quarantined);
        // after the day, with a clean window the feed is back
        let mut h2 = h.clone();
        for i in 0..20 {
            h2.observe(ok(200_000 + i * 900, 10), 200_000 + i * 900);
        }
        assert!(h2.quarantine.is_none());
        assert!(h2.observe(ok(300_000, 10), 300_000).ingest);
    }

    #[test]
    fn a_flood_is_held_and_named() {
        let mut h = Health::default();
        for i in 0..10 {
            h.observe(ok(i * 900, 4), i * 900);
        }
        let d = h.observe(ok(10 * 900, 80), 10 * 900);
        assert!(d.quarantined && !d.ingest);
        assert!(d
            .reason
            .unwrap()
            .starts_with("80 new items in one fetch, 20 times"));
        // a big fetch on a feed that is always big is not a flood
        let mut big = Health::default();
        for i in 0..10 {
            assert!(big.observe(ok(i, 200), i).ingest);
        }
    }

    #[test]
    fn lying_timestamps_flag_but_keep_ingesting() {
        let mut h = Health::default();
        for i in 0..6 {
            let mut o = ok(i * 900, 5);
            o.skew_secs = Some(10 * 86_400);
            let d = h.observe(o, i * 900);
            assert!(d.ingest);
        }
        assert!(h.timestamps_unreliable);
        let mut f = Health::default();
        for i in 0..6 {
            let mut o = ok(i, 10);
            o.future_items = 3;
            f.observe(o, i);
        }
        assert!(f.timestamps_unreliable, "future rate above a tenth");
    }
}
