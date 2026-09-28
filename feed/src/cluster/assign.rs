//! Assignment (plan §5.3): flat cosine scan over the active set, time
//! window, join threshold, same-outlet guard, expiry. Single-threaded and
//! order-dependent by design: same items in the same order give the same
//! clusters.

use std::collections::HashMap;

use super::active::{ActiveCluster, Member};
use super::simd::dot;
use crate::config::Cluster as ClusterCfg;
use crate::types::{ClusterEvent, EventKind, Item};

/// Same-outlet guard kicks in at this many items from one outlet.
pub const SAME_OUTLET_ITEMS: u32 = 3;
/// Extra similarity demanded when the guard is active.
pub const SAME_OUTLET_MARGIN: f32 = 0.08;

pub struct Clusterer {
    cfg: ClusterCfg,
    active: Vec<ActiveCluster>,
    index: HashMap<u64, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Created {
        cluster_id: u64,
    },
    Joined {
        cluster_id: u64,
        rank: u32,
    },
    /// Language excluded from clustering by config.
    Skipped,
}

pub struct Assigned {
    pub outcome: Outcome,
    pub events: Vec<ClusterEvent>,
}

fn event(ts: i64, kind: EventKind, cluster_id: u64, item_id: Option<u64>) -> ClusterEvent {
    ClusterEvent {
        ts,
        kind,
        cluster_id: Some(cluster_id),
        item_id,
        merged_into: None,
        snapshot_id: None,
    }
}

impl Clusterer {
    pub fn new(cfg: ClusterCfg) -> Self {
        Self {
            cfg,
            active: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn from_active(cfg: ClusterCfg, active: Vec<ActiveCluster>) -> Self {
        let index = active.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        Self { cfg, active, index }
    }

    pub fn active(&self) -> &[ActiveCluster] {
        &self.active
    }

    pub fn get(&self, id: u64) -> Option<&ActiveCluster> {
        self.index.get(&id).map(|&i| &self.active[i])
    }

    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// The item's own clock: what the outlet claims, else when we saw it.
    pub fn item_time(item: &Item) -> i64 {
        if item.published_at > 0 {
            item.published_at
        } else {
            item.fetched_at
        }
    }

    fn language_ok(&self, lang: &str) -> bool {
        self.cfg.languages.is_empty() || self.cfg.languages.iter().any(|l| l == lang)
    }

    pub fn assign(&mut self, item: &Item, vector: &[f32], now: i64) -> Assigned {
        if !self.language_ok(&item.lang) {
            return Assigned {
                outcome: Outcome::Skipped,
                events: vec![],
            };
        }
        let t = Self::item_time(item);
        let window = self.cfg.window_secs;

        // Candidate scan. Ties resolve to the earliest cluster in insertion
        // order, which keeps replay deterministic.
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.active.iter().enumerate() {
            if (t - c.last_active_at).abs() > window || c.n() >= self.cfg.max_cluster_size {
                continue;
            }
            let s = dot(vector, &c.centroid);
            if best.map(|(_, b)| s > b).unwrap_or(true) {
                best = Some((i, s));
            }
        }

        if let Some((i, s)) = best {
            let c = &self.active[i];
            let mut threshold = self.cfg.join_threshold;
            if self.cfg.same_outlet_guard
                && c.items_from_outlet(&item.outlet_id) >= SAME_OUTLET_ITEMS
            {
                threshold += SAME_OUTLET_MARGIN;
            }
            if s >= threshold {
                let old_origin = c.origin().item_id;
                let c = &mut self.active[i];
                let rank = c.add(Member::from_item(item, now), vector.to_vec(), now);
                let mut events = vec![event(now, EventKind::ItemAdded, c.id, Some(item.id))];
                let origin = c.origin().item_id;
                if origin != old_origin {
                    events.push(event(now, EventKind::OriginChanged, c.id, Some(origin)));
                }
                return Assigned {
                    outcome: Outcome::Joined {
                        cluster_id: c.id,
                        rank,
                    },
                    events,
                };
            }
        }

        let c = ActiveCluster::new(Member::from_item(item, now), vector.to_vec(), now);
        let id = c.id;
        self.index.insert(id, self.active.len());
        self.active.push(c);
        Assigned {
            outcome: Outcome::Created { cluster_id: id },
            events: vec![event(now, EventKind::Created, id, Some(item.id))],
        }
    }

    /// Drop clusters idle for longer than `expiry_secs`. Returns them with
    /// their `expired` events.
    pub fn expire(&mut self, now: i64) -> Vec<(ActiveCluster, ClusterEvent)> {
        let cutoff = now - self.cfg.expiry_secs;
        let mut out = Vec::new();
        let mut kept = Vec::with_capacity(self.active.len());
        for c in self.active.drain(..) {
            if c.last_active_at < cutoff {
                let ev = event(now, EventKind::Expired, c.id, None);
                out.push((c, ev));
            } else {
                kept.push(c);
            }
        }
        self.active = kept;
        self.index = self
            .active
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, outlet: &str, t: i64, lang: &str) -> Item {
        Item {
            id,
            outlet_id: outlet.into(),
            canonical_url: String::new(),
            title: format!("t{id}"),
            summary: String::new(),
            published_at: t,
            fetched_at: t,
            lang: lang.into(),
            text_hash: 0,
            cluster_id: None,
            cluster_rank: None,
            joined_at: None,
            snapshot_id: 0,
        }
    }

    fn unit(a: f32, b: f32) -> Vec<f32> {
        let n = (a * a + b * b).sqrt();
        vec![a / n, b / n]
    }

    fn cfg() -> ClusterCfg {
        ClusterCfg {
            join_threshold: 0.9,
            window_secs: 1000,
            expiry_secs: 2000,
            ..ClusterCfg::default()
        }
    }

    #[test]
    fn joins_above_threshold_creates_below() {
        let mut c = Clusterer::new(cfg());
        let a = c.assign(&item(1, "x", 100, "en"), &unit(1.0, 0.0), 100);
        assert_eq!(a.outcome, Outcome::Created { cluster_id: 1 });
        let b = c.assign(&item(2, "y", 110, "en"), &unit(1.0, 0.1), 110);
        assert_eq!(
            b.outcome,
            Outcome::Joined {
                cluster_id: 1,
                rank: 1
            }
        );
        assert_eq!(b.events[0].kind, EventKind::ItemAdded);
        let d = c.assign(&item(3, "z", 120, "en"), &unit(0.0, 1.0), 120);
        assert_eq!(d.outcome, Outcome::Created { cluster_id: 3 });
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn window_blocks_old_clusters_and_expiry_drops_them() {
        let mut c = Clusterer::new(cfg());
        c.assign(&item(1, "x", 100, "en"), &unit(1.0, 0.0), 100);
        let late = c.assign(&item(2, "y", 5000, "en"), &unit(1.0, 0.0), 5000);
        assert_eq!(
            late.outcome,
            Outcome::Created { cluster_id: 2 },
            "outside the window, same vector"
        );
        let gone = c.expire(5000);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].0.id, 1);
        assert_eq!(gone[0].1.kind, EventKind::Expired);
        assert_eq!(c.len(), 1);
        assert!(c.get(2).is_some() && c.get(1).is_none());
    }

    #[test]
    fn same_outlet_guard_and_language_filter() {
        let mut c = Clusterer::new(ClusterCfg {
            languages: vec!["en".into()],
            ..cfg()
        });
        for i in 1..=3 {
            c.assign(&item(i, "x", 100 + i as i64, "en"), &unit(1.0, 0.0), 100);
        }
        // similarity ~0.95: above 0.9 but below 0.9 + 0.08 once the guard is on
        let v = unit(1.0, 0.33);
        let s = dot(&v, &c.active()[0].centroid);
        assert!(s > 0.9 && s < 0.98, "{s}");
        let guarded = c.assign(&item(4, "x", 110, "en"), &v, 110);
        assert!(matches!(guarded.outcome, Outcome::Created { .. }));
        let other = c.assign(&item(5, "y", 111, "en"), &v, 111);
        assert!(
            matches!(other.outcome, Outcome::Joined { .. }),
            "guard is per outlet"
        );
        assert_eq!(
            c.assign(&item(6, "y", 112, "fr"), &v, 112).outcome,
            Outcome::Skipped
        );
    }

    #[test]
    fn origin_change_is_reported() {
        let mut c = Clusterer::new(cfg());
        c.assign(&item(1, "x", 500, "en"), &unit(1.0, 0.0), 500);
        let a = c.assign(&item(2, "y", 400, "en"), &unit(1.0, 0.0), 510);
        assert_eq!(a.events.len(), 2);
        assert_eq!(a.events[1].kind, EventKind::OriginChanged);
        assert_eq!(a.events[1].item_id, Some(2));
    }
}
