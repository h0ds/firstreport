//! Everything `lens` computes (plan §4): counts and joins, nothing
//! else. The per-outlet definitions match `ledger`'s feed_behaviour
//! importer so the numbers on an outlet page agree with the claims.
//! All windows are measured from the export time, so a build is a pure
//! function of its inputs.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::input::{Cluster, Event, Input};

pub const HOUR: i64 = 3_600;
pub const DAY: i64 = 86_400;
/// Sparkline: cumulative outlets over the cluster's first day, in 2 h steps.
pub const SPARK_BUCKETS: usize = 12;

/// What the story page needs beyond the cluster itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClusterFacts {
    pub outlets_6h: u32,
    pub outlets_24h: u32,
    pub items_24h: u32,
    /// Cumulative distinct outlets at the end of each 2 h bucket after
    /// the cluster was created. Length `SPARK_BUCKETS`.
    pub spark: Vec<u32>,
    /// Item id → the syndication group's first outlet, for items whose
    /// text was first published by another outlet.
    pub copies_of: HashMap<u64, String>,
    /// Item ids whose text was later carried by another outlet.
    pub copied: HashSet<u64>,
    pub revisions: Vec<Event>,
    pub withdrawals: Vec<Event>,
}

pub fn cluster_facts(c: &Cluster, events: &[Event], now: i64) -> ClusterFacts {
    let mut f = ClusterFacts::default();
    let distinct = |since: i64| -> u32 {
        c.timeline
            .iter()
            .filter(|j| j.joined_at >= since)
            .map(|j| j.outlet_id.as_str())
            .collect::<HashSet<_>>()
            .len() as u32
    };
    f.outlets_6h = distinct(now - 6 * HOUR);
    f.outlets_24h = distinct(now - DAY);
    f.items_24h = c
        .timeline
        .iter()
        .filter(|j| j.joined_at >= now - DAY)
        .count() as u32;
    let mut seen: HashSet<&str> = HashSet::new();
    let mut joins = c.timeline.clone();
    joins.sort_by_key(|j| j.joined_at);
    let mut k = 0;
    for b in 0..SPARK_BUCKETS {
        let end = c.created_at + ((b + 1) as i64) * 2 * HOUR;
        while k < joins.len() && joins[k].joined_at <= end {
            seen.insert(joins[k].outlet_id.as_str());
            k += 1;
        }
        f.spark.push(seen.len() as u32);
    }
    let outlet_of: HashMap<u64, &str> = c
        .timeline
        .iter()
        .map(|j| (j.item_id, j.outlet_id.as_str()))
        .collect();
    for g in &c.syndication {
        let others = g
            .item_ids
            .iter()
            .any(|id| outlet_of.get(id).is_some_and(|o| *o != g.first_outlet_id));
        for id in &g.item_ids {
            match outlet_of.get(id) {
                Some(o) if *o == g.first_outlet_id => {
                    if others {
                        f.copied.insert(*id);
                    }
                }
                Some(_) => {
                    f.copies_of.insert(*id, g.first_outlet_id.clone());
                }
                None => {}
            }
        }
    }
    let members: HashSet<u64> = c.timeline.iter().map(|j| j.item_id).collect();
    for e in events {
        let Some(id) = e.item_id else { continue };
        if !members.contains(&id) {
            continue;
        }
        match e.kind.as_str() {
            "revised" => f.revisions.push(e.clone()),
            "withdrawn" => f.withdrawals.push(e.clone()),
            _ => {}
        }
    }
    f
}

/// What the outlet page needs: the outlet's stories in the window and
/// the counts `ledger`'s feed_behaviour importer also reports.
// Used by the outlet page, next commit.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutletFacts {
    /// Cluster ids the outlet joined in the window, most recently active first.
    pub clusters: Vec<u64>,
    pub items: u32,
    /// Clusters with at least two outlets that this outlet is in…
    pub shared_stories: u32,
    /// …and was the earliest published member of.
    pub first_to: u32,
    /// Items whose text first appeared at another outlet.
    pub copies: u32,
    /// Items whose text later appeared at another outlet.
    pub copied: u32,
}

#[allow(dead_code)]
pub fn outlet_facts(input: &Input, window_secs: i64) -> BTreeMap<String, OutletFacts> {
    let now = input.export_time;
    let start = now - window_secs;
    let mut out: BTreeMap<String, OutletFacts> = BTreeMap::new();
    let mut in_window: HashSet<u64> = HashSet::new();
    for it in input.items.values() {
        if it.fetched_at < start || it.fetched_at > now {
            continue;
        }
        out.entry(it.outlet_id.clone()).or_default().items += 1;
        in_window.insert(it.id);
    }
    let mut active: Vec<(i64, u64)> = Vec::new();
    for c in input.clusters.values() {
        let outlet_of: HashMap<u64, &str> = c
            .timeline
            .iter()
            .map(|j| (j.item_id, j.outlet_id.as_str()))
            .collect();
        for g in &c.syndication {
            for id in &g.item_ids {
                if !in_window.contains(id) {
                    continue;
                }
                let Some(o) = outlet_of.get(id) else { continue };
                let f = out.entry(o.to_string()).or_default();
                if *o == g.first_outlet_id {
                    if g.item_ids
                        .iter()
                        .any(|x| outlet_of.get(x).is_some_and(|y| y != o))
                    {
                        f.copied += 1;
                    }
                } else {
                    f.copies += 1;
                }
            }
        }
        let mut members: Vec<&str> = Vec::new();
        for j in &c.timeline {
            if in_window.contains(&j.item_id) && !members.contains(&j.outlet_id.as_str()) {
                members.push(&j.outlet_id);
            }
        }
        if members.is_empty() {
            continue;
        }
        active.push((c.last_active_at, c.id));
        let shared = members.len() >= 2;
        for m in &members {
            let f = out.entry(m.to_string()).or_default();
            if shared {
                f.shared_stories += 1;
                if *m == c.origin_outlet_id {
                    f.first_to += 1;
                }
            }
        }
    }
    active.sort_by(|a, b| b.cmp(a));
    for (_, id) in active {
        let c = &input.clusters[&id];
        let mut done: HashSet<&str> = HashSet::new();
        for j in &c.timeline {
            if in_window.contains(&j.item_id) && done.insert(&j.outlet_id) {
                out.entry(j.outlet_id.clone())
                    .or_default()
                    .clusters
                    .push(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn outlet_counts_agree_with_ledger_feed_behaviour() {
        let i = fixture_input();
        let f = outlet_facts(&i, 90 * DAY);
        let wire = &f["wire"];
        assert_eq!(wire.items, 61);
        assert_eq!((wire.copies, wire.copied), (0, 30));
        assert_eq!((wire.first_to, wire.shared_stories), (30, 31));
        assert_eq!(wire.clusters.len(), 61, "every cluster wire is in");
        let gz = &f["gazette"];
        assert_eq!(gz.items, 31);
        assert_eq!((gz.copies, gz.copied), (30, 0));
        assert_eq!((gz.first_to, gz.shared_stories), (1, 31));
        assert_eq!(
            f["blog"].clusters.len(),
            0,
            "unclustered items are not stories"
        );
        let recent = &i.clusters[&wire.clusters[0]];
        assert!(
            wire.clusters
                .iter()
                .all(|id| i.clusters[id].last_active_at <= recent.last_active_at),
            "most recently active first"
        );
        let short = outlet_facts(&i, 7 * DAY);
        assert!(short["wire"].items < 20);
    }

    #[test]
    fn cluster_facts_count_outlets_copies_and_changes() {
        let i = fixture_input();
        let c = &i.clusters[&1001];
        let f = cluster_facts(c, &i.events, c.last_active_at);
        assert_eq!(f.outlets_6h, 2);
        assert_eq!(f.outlets_24h, 2);
        assert_eq!(f.items_24h, 2);
        assert_eq!(f.spark.len(), SPARK_BUCKETS);
        assert_eq!(
            f.spark[0], 2,
            "both outlets joined within the first two hours"
        );
        assert_eq!(*f.spark.last().unwrap(), 2);
        let gazette_item = c.timeline[1].item_id;
        assert_eq!(f.copies_of[&gazette_item], "wire");
        assert!(f.copied.contains(&c.timeline[0].item_id));
        assert!(f.revisions.is_empty() && f.withdrawals.is_empty());
        // day 0, k=1 has a revision; day 10, k=1 has a withdrawal
        let rev = &i.clusters[&1002];
        let f = cluster_facts(rev, &i.events, rev.last_active_at);
        assert_eq!(f.revisions.len(), 1);
        let wd = &i.clusters[&1022];
        let f = cluster_facts(wd, &i.events, wd.last_active_at);
        assert_eq!(f.withdrawals.len(), 1);
        // long after the story, the trailing windows are empty
        let f = cluster_facts(c, &i.events, c.last_active_at + 3 * DAY);
        assert_eq!((f.outlets_6h, f.outlets_24h, f.items_24h), (0, 0, 0));
    }
}
