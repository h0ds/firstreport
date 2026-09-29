//! Propagation record derived on read (plan §5.3): timeline from item
//! ranks, origin, velocity counters, syndication groups by simhash.
//! Nothing here is stored; it is computed from the item table so the
//! cluster row stays small and the numbers are always current.

use crate::parse::simhash::hamming;
use crate::store::reader::ClusterRow;
use crate::types::{Cluster, Confidence, Item, Join, SynGroup, Velocity};

pub fn timeline(items: &[Item]) -> Vec<Join> {
    let mut joins: Vec<(u32, Join)> = items
        .iter()
        .map(|i| {
            (
                i.cluster_rank.unwrap_or(u32::MAX),
                Join {
                    item_id: i.id,
                    outlet_id: i.outlet_id.clone(),
                    published_at: i.published_at,
                    joined_at: i.joined_at.unwrap_or(i.fetched_at),
                    join_similarity: i.join_similarity,
                },
            )
        })
        .collect();
    joins.sort_by_key(|(r, j)| (*r, j.joined_at, j.item_id));
    joins.into_iter().map(|(_, j)| j).collect()
}

/// Origin: earliest `published_at`, tie-break earliest `fetched_at`.
pub fn origin(items: &[Item]) -> Option<&Item> {
    items.iter().min_by_key(|i| {
        (
            i.published_at,
            i.fetched_at,
            i.cluster_rank.unwrap_or(u32::MAX),
        )
    })
}

pub fn velocity(timeline: &[Join], now: i64) -> Velocity {
    let mut v = Velocity::default();
    let mut outlets_1h = std::collections::HashSet::new();
    let mut outlets_6h = std::collections::HashSet::new();
    let mut outlets_24h = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    let first = timeline.first().map(|j| j.joined_at).unwrap_or(now);
    for j in timeline {
        let age = now - j.joined_at;
        if age <= 3600 {
            v.items_1h += 1;
            outlets_1h.insert(j.outlet_id.as_str());
        }
        if age <= 6 * 3600 {
            v.items_6h += 1;
            outlets_6h.insert(j.outlet_id.as_str());
        }
        if age <= 24 * 3600 {
            v.items_24h += 1;
            outlets_24h.insert(j.outlet_id.as_str());
        }
        if seen.insert(j.outlet_id.as_str()) {
            let secs = (j.joined_at - first).max(0) as u32;
            if seen.len() == 5 {
                v.time_to_5_outlets_secs = Some(secs);
            }
            if seen.len() == 20 {
                v.time_to_20_outlets_secs = Some(secs);
            }
        }
    }
    v.outlets_1h = outlets_1h.len() as u32;
    v.outlets_6h = outlets_6h.len() as u32;
    v.outlets_24h = outlets_24h.len() as u32;
    v
}

/// Group items whose simhashes are within `threshold` bits of the group's
/// first member. Greedy over join order, so the first outlet to publish a
/// given copy leads its group.
pub fn syndication(items: &[Item], threshold: u32) -> Vec<SynGroup> {
    let mut ordered: Vec<&Item> = items.iter().collect();
    ordered.sort_by_key(|i| (i.cluster_rank.unwrap_or(u32::MAX), i.id));
    let mut groups: Vec<(u64, Vec<&Item>)> = Vec::new();
    for it in ordered {
        match groups
            .iter_mut()
            .find(|(h, _)| hamming(*h, it.text_hash) <= threshold)
        {
            Some((_, members)) => members.push(it),
            None => groups.push((it.text_hash, vec![it])),
        }
    }
    groups
        .into_iter()
        .filter(|(_, m)| m.len() > 1)
        .map(|(h, m)| {
            let first = m
                .iter()
                .min_by_key(|i| (i.published_at, i.fetched_at))
                .unwrap();
            let lo = m.iter().map(|i| i.published_at).min().unwrap_or(0);
            let hi = m.iter().map(|i| i.published_at).max().unwrap_or(0);
            SynGroup {
                text_hash: h,
                item_ids: m.iter().map(|i| i.id).collect(),
                first_outlet_id: first.outlet_id.clone(),
                spread_secs: (hi - lo).max(0) as u32,
            }
        })
        .collect()
}

/// The two thresholds an assembled cluster reports: the simhash
/// distance for syndication groups and the cosine join threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub hamming: u32,
    pub join: f32,
}

impl Thresholds {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            hamming: cfg.syndication.hamming_threshold,
            join: cfg.cluster.join_threshold,
        }
    }
}

/// Clustering confidence (docs/steward.md §3): how sure the method
/// was, as counts over the members. Nothing about any outlet.
pub fn confidence(row: &ClusterRow, items: &[Item], join_threshold: f32) -> Confidence {
    let scored: Vec<f32> = items.iter().filter_map(|i| i.join_similarity).collect();
    let weakest_join = scored.iter().copied().reduce(f32::min);
    let mut languages: Vec<String> = items
        .iter()
        .map(|i| i.lang.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    languages.dedup();
    Confidence {
        join_threshold,
        weakest_join,
        join_margin: weakest_join.map(|w| w - join_threshold),
        unscored: (items.len() - scored.len()) as u32,
        merged: row.merged,
        languages,
    }
}

/// Assemble the contract `Cluster` from its row and items.
pub fn assemble(row: &ClusterRow, items: &[Item], now: i64, th: Thresholds) -> Cluster {
    let tl = timeline(items);
    let o = origin(items);
    Cluster {
        id: row.id,
        created_at: row.created_at,
        last_active_at: row.last_active_at,
        item_count: items.len() as u32,
        outlet_count: items
            .iter()
            .map(|i| i.outlet_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len() as u32,
        primary_title: row.primary_title.clone(),
        origin_item_id: o.map(|i| i.id).unwrap_or(row.id),
        origin_outlet_id: o.map(|i| i.outlet_id.clone()).unwrap_or_default(),
        velocity: velocity(&tl, now),
        syndication: syndication(items, th.hamming),
        confidence: confidence(row, items, th.join),
        timeline: tl,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, outlet: &str, published: i64, joined: i64, rank: u32, hash: u64) -> Item {
        Item {
            id,
            outlet_id: outlet.into(),
            canonical_url: String::new(),
            title: String::new(),
            summary: String::new(),
            published_at: published,
            fetched_at: published + 1,
            lang: "en".into(),
            text_hash: hash,
            cluster_id: Some(1),
            cluster_rank: Some(rank),
            joined_at: Some(joined),
            join_similarity: None,
            snapshot_id: 0,
        }
    }

    #[test]
    fn confidence_is_counts_over_members() {
        let row = ClusterRow {
            id: 1,
            created_at: 0,
            last_active_at: 0,
            item_count: 3,
            outlet_count: 3,
            primary_title: String::new(),
            centroid: vec![],
            expired: false,
            merged: true,
        };
        let mut items = vec![
            item(1, "a", 100, 100, 0, 0),
            item(2, "b", 100, 100, 1, 0),
            item(3, "c", 100, 100, 2, 0),
        ];
        items[1].join_similarity = Some(0.91);
        items[2].join_similarity = Some(0.74);
        items[2].lang = "ja".into();
        let c = confidence(&row, &items, 0.72);
        assert_eq!(c.join_threshold, 0.72);
        assert_eq!(c.weakest_join, Some(0.74));
        assert!((c.join_margin.unwrap() - 0.02).abs() < 1e-6);
        assert_eq!(c.unscored, 1, "the seed has no similarity");
        assert!(c.merged);
        assert_eq!(c.languages, vec!["en", "ja"]);
        // Nothing scored yet (rows from before the field): margin absent, not zero.
        let c = confidence(&row, &items[..1], 0.72);
        assert_eq!((c.weakest_join, c.join_margin, c.unscored), (None, None, 1));
    }

    #[test]
    fn timeline_velocity_origin() {
        let now = 100_000;
        let items = vec![
            item(1, "a", 90_000, 90_100, 0, 0),
            item(2, "b", 80_000, 95_000, 1, 0), // earlier published, joined later
            item(3, "c", 99_000, 99_500, 2, 0),
            item(4, "d", 99_100, 99_600, 3, 0),
            item(5, "e", 99_200, 99_700, 4, 0),
        ];
        let tl = timeline(&items);
        assert_eq!(
            tl.iter().map(|j| j.item_id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        assert_eq!(origin(&items).unwrap().id, 2);
        let v = velocity(&tl, now);
        assert_eq!((v.items_1h, v.items_6h, v.items_24h), (3, 5, 5));
        assert_eq!((v.outlets_1h, v.outlets_6h), (3, 5));
        assert_eq!(v.time_to_5_outlets_secs, Some(99_700 - 90_100));
        assert_eq!(v.time_to_20_outlets_secs, None);
    }

    #[test]
    fn syndication_groups_near_identical_hashes() {
        let items = vec![
            item(1, "wire", 100, 100, 0, 0b1111_0000),
            item(2, "paperA", 160, 200, 1, 0b1111_0001), // 1 bit off the wire copy
            item(3, "paperB", 130, 300, 2, 0b1111_0011), // 2 bits off
            item(4, "other", 140, 400, 3, 0b0000_1111),  // unrelated text
        ];
        let g = syndication(&items, 3);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].item_ids, vec![1, 2, 3]);
        assert_eq!(g[0].first_outlet_id, "wire");
        assert_eq!(g[0].spread_secs, 60);
    }
}
