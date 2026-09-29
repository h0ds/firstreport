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

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Created {
        cluster_id: u64,
    },
    Joined {
        cluster_id: u64,
        rank: u32,
        /// Cosine to the centroid before the item was added.
        similarity: f32,
    },
    /// Language excluded from clustering by config.
    Skipped,
}

pub struct Assigned {
    pub outcome: Outcome,
    pub events: Vec<ClusterEvent>,
}

/// One merge from a pass: `absorbed` no longer exists; its items now
/// belong to `survivor` at the ranks in `moved`.
#[derive(Debug)]
pub struct Merged {
    pub survivor: u64,
    pub absorbed: u64,
    pub moved: Vec<(u64, u32)>,
    pub event: ClusterEvent,
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
                        similarity: s,
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

    /// Periodic merge pass (plan §5.3): pairwise cosine over active
    /// centroids; any pair at or above `merge_threshold` with overlapping
    /// time windows is merged, smaller into larger, unless the union would
    /// exceed `max_cluster_size`. Pairs are taken in descending similarity
    /// with id tie-breaks, so the result is deterministic. A cluster takes
    /// part in at most one merge per pass; the next pass catches the rest.
    pub fn merge_pass(&mut self, now: i64) -> Vec<Merged> {
        let n = self.active.len();
        if n < 2 {
            return vec![];
        }
        let threshold = self.cfg.merge_threshold;
        let window = self.cfg.window_secs;
        let max_size = self.cfg.max_cluster_size;
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.active[i], &self.active[j]);
                if (a.last_active_at - b.last_active_at).abs() > window || a.n() + b.n() > max_size
                {
                    continue;
                }
                let s = dot(&a.centroid, &b.centroid);
                if s >= threshold {
                    pairs.push((s, i, j));
                }
            }
        }
        if pairs.is_empty() {
            return vec![];
        }
        pairs.sort_by(|x, y| {
            y.0.partial_cmp(&x.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    (self.active[x.1].id, self.active[x.2].id)
                        .cmp(&(self.active[y.1].id, self.active[y.2].id))
                })
        });

        let mut taken = vec![false; n];
        let mut plan: Vec<(usize, usize)> = Vec::new(); // (survivor, absorbed)
        for (_, i, j) in pairs {
            if taken[i] || taken[j] {
                continue;
            }
            taken[i] = true;
            taken[j] = true;
            let (a, b) = (&self.active[i], &self.active[j]);
            // Larger survives; on a tie the older cluster (then lower id) does.
            let a_survives = (
                a.n(),
                std::cmp::Reverse(a.created_at),
                std::cmp::Reverse(a.id),
            ) > (
                b.n(),
                std::cmp::Reverse(b.created_at),
                std::cmp::Reverse(b.id),
            );
            plan.push(if a_survives { (i, j) } else { (j, i) });
        }

        // Apply from the highest index down so removals don't shift the
        // indices still to be processed... except survivors can sit above
        // absorbed ones, so take the absorbed clusters out by id instead.
        let mut out = Vec::with_capacity(plan.len());
        let absorbed_ids: Vec<(u64, u64)> = plan
            .iter()
            .map(|&(s, a)| (self.active[s].id, self.active[a].id))
            .collect();
        for (survivor_id, absorbed_id) in absorbed_ids {
            let pos = self
                .active
                .iter()
                .position(|c| c.id == absorbed_id)
                .expect("absorbed cluster present");
            let absorbed = self.active.remove(pos);
            let s = self
                .active
                .iter()
                .position(|c| c.id == survivor_id)
                .expect("survivor present");
            let moved = self.active[s].absorb(absorbed, now);
            out.push(Merged {
                survivor: survivor_id,
                absorbed: absorbed_id,
                moved,
                event: ClusterEvent {
                    ts: now,
                    kind: EventKind::Merged,
                    cluster_id: Some(absorbed_id),
                    item_id: None,
                    merged_into: Some(survivor_id),
                    snapshot_id: None,
                },
            });
        }
        self.index = self
            .active
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect();
        out
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
            join_similarity: None,
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
        match b.outcome {
            Outcome::Joined {
                cluster_id: 1,
                rank: 1,
                similarity,
            } => assert!(similarity > 0.9 && similarity <= 1.0, "{similarity}"),
            other => panic!("expected a join, got {other:?}"),
        }
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

    #[test]
    fn merge_pass_joins_near_centroids_smaller_into_larger() {
        // Unreachable join threshold so the seeds stay separate; merge at 0.9.
        let mut c = Clusterer::new(ClusterCfg {
            join_threshold: 1.01,
            merge_threshold: 0.9,
            ..cfg()
        });
        c.assign(&item(1, "x", 100, "en"), &unit(1.0, 0.0), 100);
        c.assign(&item(2, "y", 101, "en"), &unit(1.0, 0.0), 101); // identical vector, still separate
        c.assign(&item(3, "z", 102, "en"), &unit(1.0, 0.05), 102); // near 1 and 2
        c.assign(&item(4, "w", 103, "en"), &unit(0.0, 1.0), 103); // far away
        c.assign(&item(5, "v", 104, "en"), &unit(0.0, 1.0), 104); // twin of 4, but outside window below
        assert_eq!(c.len(), 5);

        let merges = c.merge_pass(200);
        // Pairs at or above 0.9: (1,2)=1.0, (1,3),(2,3)≈0.999, (4,5)=1.0.
        // Greedy by similarity: (1,2) and (4,5) first; 3 is left for the next pass.
        assert_eq!(merges.len(), 2);
        let survivors: Vec<u64> = merges.iter().map(|m| m.survivor).collect();
        let absorbed: Vec<u64> = merges.iter().map(|m| m.absorbed).collect();
        assert_eq!(
            survivors,
            vec![1, 4],
            "equal size: the older cluster survives"
        );
        assert_eq!(absorbed, vec![2, 5]);
        assert_eq!(merges[0].moved, vec![(2, 1)]);
        assert_eq!(merges[0].event.kind, EventKind::Merged);
        assert_eq!(merges[0].event.merged_into, Some(1));
        assert_eq!(c.len(), 3);
        assert!(c.get(2).is_none() && c.get(5).is_none());
        assert_eq!(c.get(1).unwrap().n(), 2);

        let second = c.merge_pass(201);
        assert_eq!(second.len(), 1);
        assert_eq!(
            (second[0].survivor, second[0].absorbed),
            (1, 3),
            "larger survives"
        );
        assert_eq!(second[0].moved, vec![(3, 2)]);
        assert_eq!(c.len(), 2);
        assert!(c.merge_pass(202).is_empty());
    }

    #[test]
    fn merge_pass_respects_window_and_size() {
        let mut c = Clusterer::new(ClusterCfg {
            join_threshold: 1.01,
            merge_threshold: 0.9,
            max_cluster_size: 2,
            ..cfg()
        });
        c.assign(&item(1, "x", 100, "en"), &unit(1.0, 0.0), 100);
        c.assign(&item(2, "y", 5000, "en"), &unit(1.0, 0.0), 5000); // outside the 1000 s window of 1
        assert!(c.merge_pass(5000).is_empty(), "window blocks the merge");
        c.assign(&item(3, "z", 5001, "en"), &unit(1.0, 0.0), 5001);
        c.assign(&item(4, "w", 5002, "en"), &unit(1.0, 0.0), 5002);
        // 2, 3, 4 are all mergeable pairwise; with max size 2 only one pair merges.
        let m = c.merge_pass(5003);
        assert_eq!(m.len(), 1);
        assert_eq!(c.get(m[0].survivor).unwrap().n(), 2);
        assert!(
            c.merge_pass(5004).is_empty(),
            "union would exceed max_cluster_size"
        );
    }
}
