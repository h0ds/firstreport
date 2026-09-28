//! An active cluster: running centroid plus enough member state to keep
//! the propagation record (plan §5.3) without going back to the store.

use std::collections::HashMap;

use super::simd::dot;
use crate::types::Item;

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub item_id: u64,
    pub outlet_id: String,
    pub published_at: i64,
    pub fetched_at: i64,
    pub joined_at: i64,
    pub text_hash: u64,
    pub title: String,
}

impl Member {
    pub fn from_item(item: &Item, joined_at: i64) -> Self {
        Self {
            item_id: item.id,
            outlet_id: item.outlet_id.clone(),
            published_at: item.published_at,
            fetched_at: item.fetched_at,
            joined_at,
            text_hash: item.text_hash,
            title: item.title.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActiveCluster {
    pub id: u64,
    pub created_at: i64,
    pub last_active_at: i64,
    /// Unnormalized sum of member vectors.
    pub sum: Vec<f32>,
    /// L2-normalized `sum`.
    pub centroid: Vec<f32>,
    /// Join order; index is `cluster_rank`.
    pub members: Vec<Member>,
    /// Parallel to `members`.
    pub vectors: Vec<Vec<f32>>,
    outlet_counts: HashMap<String, u32>,
}

impl ActiveCluster {
    pub fn new(member: Member, vector: Vec<f32>, now: i64) -> Self {
        let mut c = Self {
            id: member.item_id,
            created_at: now,
            last_active_at: now,
            sum: vec![0.0; vector.len()],
            centroid: vec![0.0; vector.len()],
            members: Vec::with_capacity(4),
            vectors: Vec::with_capacity(4),
            outlet_counts: HashMap::new(),
        };
        c.add(member, vector, now);
        c
    }

    /// Rebuild from persisted members, recomputing the centroid.
    pub fn from_parts(
        id: u64,
        created_at: i64,
        last_active_at: i64,
        members: Vec<Member>,
        vectors: Vec<Vec<f32>>,
    ) -> Self {
        let dim = vectors.first().map(Vec::len).unwrap_or(0);
        let mut c = Self {
            id,
            created_at,
            last_active_at,
            sum: vec![0.0; dim],
            centroid: vec![0.0; dim],
            members: Vec::with_capacity(members.len()),
            vectors: Vec::with_capacity(vectors.len()),
            outlet_counts: HashMap::new(),
        };
        for (m, v) in members.into_iter().zip(vectors) {
            let t = m.joined_at;
            c.add(m, v, t);
        }
        c.last_active_at = last_active_at;
        c
    }

    /// Append a member and update the centroid. Returns its rank.
    pub fn add(&mut self, member: Member, vector: Vec<f32>, now: i64) -> u32 {
        for (s, v) in self.sum.iter_mut().zip(&vector) {
            *s += v;
        }
        let norm = self.sum.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for (c, s) in self.centroid.iter_mut().zip(&self.sum) {
                *c = s / norm;
            }
        }
        *self
            .outlet_counts
            .entry(member.outlet_id.clone())
            .or_insert(0) += 1;
        self.last_active_at = self.last_active_at.max(now);
        self.members.push(member);
        self.vectors.push(vector);
        (self.members.len() - 1) as u32
    }

    pub fn n(&self) -> u32 {
        self.members.len() as u32
    }

    /// Take every member of `other` into this cluster, appending them in
    /// their existing join order. Returns `(item_id, new_rank)` for each
    /// moved member so the store can be updated.
    pub fn absorb(&mut self, other: ActiveCluster, now: i64) -> Vec<(u64, u32)> {
        let mut moved = Vec::with_capacity(other.members.len());
        let last_active = other.last_active_at;
        for (m, v) in other.members.into_iter().zip(other.vectors) {
            let id = m.item_id;
            let rank = self.add(m, v, now);
            moved.push((id, rank));
        }
        self.last_active_at = self.last_active_at.max(last_active);
        self.created_at = self.created_at.min(other.created_at);
        moved
    }

    pub fn outlet_count(&self) -> u32 {
        self.outlet_counts.len() as u32
    }

    pub fn items_from_outlet(&self, outlet_id: &str) -> u32 {
        self.outlet_counts.get(outlet_id).copied().unwrap_or(0)
    }

    /// Earliest `published_at`, tie-break earliest `fetched_at`, then rank.
    pub fn origin(&self) -> &Member {
        self.members
            .iter()
            .enumerate()
            .min_by_key(|(i, m)| (m.published_at, m.fetched_at, *i))
            .map(|(_, m)| m)
            .expect("cluster has at least one member")
    }

    /// Title of the member nearest the centroid.
    pub fn primary_title(&self) -> &str {
        let mut best = 0;
        let mut best_s = f32::MIN;
        for (i, v) in self.vectors.iter().enumerate() {
            let s = dot(v, &self.centroid);
            if s > best_s {
                best_s = s;
                best = i;
            }
        }
        &self.members[best].title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: u64, outlet: &str, published: i64) -> Member {
        Member {
            item_id: id,
            outlet_id: outlet.into(),
            published_at: published,
            fetched_at: published + 10,
            joined_at: published + 20,
            text_hash: 0,
            title: format!("t{id}"),
        }
    }

    #[test]
    fn centroid_origin_and_counts() {
        let mut c = ActiveCluster::new(m(1, "a", 100), vec![1.0, 0.0], 120);
        assert_eq!(c.id, 1);
        assert_eq!(c.add(m(2, "b", 50), vec![0.0, 1.0], 130), 1);
        assert_eq!(c.add(m(3, "a", 200), vec![1.0, 0.0], 140), 2);
        assert_eq!(c.n(), 3);
        assert_eq!(c.outlet_count(), 2);
        assert_eq!(c.items_from_outlet("a"), 2);
        assert_eq!(
            c.origin().item_id,
            2,
            "earliest published wins even if it joined later"
        );
        assert_eq!(c.last_active_at, 140);
        let n = (c.centroid[0].powi(2) + c.centroid[1].powi(2)).sqrt();
        assert!((n - 1.0).abs() < 1e-6);
        assert!(c.centroid[0] > c.centroid[1]);
        assert_eq!(c.primary_title(), "t1");
    }

    #[test]
    fn absorb_appends_members_and_reranks_them() {
        let mut a = ActiveCluster::new(m(1, "a", 100), vec![1.0, 0.0], 120);
        a.add(m(2, "b", 110), vec![1.0, 0.0], 130);
        let mut b = ActiveCluster::new(m(3, "c", 50), vec![0.0, 1.0], 60);
        b.add(m(4, "a", 55), vec![0.0, 1.0], 65);
        let moved = a.absorb(b, 200);
        assert_eq!(moved, vec![(3, 2), (4, 3)]);
        assert_eq!(a.n(), 4);
        assert_eq!(a.outlet_count(), 3);
        assert_eq!(a.items_from_outlet("a"), 2);
        assert_eq!(a.origin().item_id, 3, "origin recomputed over the union");
        assert_eq!(a.created_at, 60, "created_at is the earlier of the two");
        assert_eq!(a.last_active_at, 200);
        let n = (a.centroid[0].powi(2) + a.centroid[1].powi(2)).sqrt();
        assert!((n - 1.0).abs() < 1e-6);
        assert!(
            (a.centroid[0] - a.centroid[1]).abs() < 1e-6,
            "two of each direction"
        );
    }
}
