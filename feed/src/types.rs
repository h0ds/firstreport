//! Data contract v1 (plan §4). This is the product: field additions are
//! non-breaking, removals or type changes bump `CONTRACT_VERSION`.

// Contract types land ahead of their consumers (clusters arrive at M3).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

/// Sentence embedding width for the MiniLM-L6 class of models.
pub const EMBED_DIM: usize = 384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedKind {
    Rss,
    Atom,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedRef {
    pub url: String,
    pub kind: FeedKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outlet {
    /// Stable slug, e.g. `reuters`.
    pub id: String,
    pub name: String,
    pub domains: Vec<String>,
    pub feeds: Vec<FeedRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    /// xxhash64 of `canonical_url`.
    pub id: u64,
    pub outlet_id: String,
    pub canonical_url: String,
    pub title: String,
    /// Plain text, at most 512 chars, may be empty.
    pub summary: String,
    /// Unix seconds from the feed; 0 if absent.
    pub published_at: i64,
    pub fetched_at: i64,
    /// ISO 639-1; `und` if unknown.
    pub lang: String,
    /// Simhash of normalized title+summary; the syndication signal.
    pub text_hash: u64,
    pub cluster_id: Option<u64>,
    /// 0 = first item in its cluster (origin), 1 = second, ...
    pub cluster_rank: Option<u32>,
    pub joined_at: Option<i64>,
    pub snapshot_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Join {
    pub item_id: u64,
    pub outlet_id: String,
    pub published_at: i64,
    pub joined_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynGroup {
    pub text_hash: u64,
    pub item_ids: Vec<u64>,
    pub first_outlet_id: String,
    pub spread_secs: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Velocity {
    pub items_1h: u32,
    pub items_6h: u32,
    pub items_24h: u32,
    pub outlets_1h: u32,
    pub outlets_6h: u32,
    pub outlets_24h: u32,
    pub time_to_5_outlets_secs: Option<u32>,
    pub time_to_20_outlets_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    /// Id of the first item in the cluster.
    pub id: u64,
    pub created_at: i64,
    pub last_active_at: i64,
    pub item_count: u32,
    pub outlet_count: u32,
    /// Title of the item nearest the centroid.
    pub primary_title: String,
    pub origin_item_id: u64,
    pub origin_outlet_id: String,
    pub timeline: Vec<Join>,
    pub syndication: Vec<SynGroup>,
    pub velocity: Velocity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// xxhash64(feed_url + fetched_at).
    pub id: u64,
    pub feed_url: String,
    pub fetched_at: i64,
    pub status: u16,
    pub body_hash: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Created,
    ItemAdded,
    Merged,
    Expired,
    OriginChanged,
    Revised,
    Withdrawn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterEvent {
    pub ts: i64,
    pub kind: EventKind,
    pub cluster_id: Option<u64>,
    pub item_id: Option<u64>,
    pub merged_into: Option<u64>,
    pub snapshot_id: Option<u64>,
}

/// Item id derivation, shared by the poller and by anything that needs to
/// look an item up by URL.
pub fn item_id(canonical_url: &str) -> u64 {
    xxhash_rust::xxh64::xxh64(canonical_url.as_bytes(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_id_is_stable() {
        assert_eq!(
            item_id("https://example.com/a"),
            item_id("https://example.com/a")
        );
        assert_ne!(
            item_id("https://example.com/a"),
            item_id("https://example.com/b")
        );
    }

    #[test]
    fn event_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EventKind::ItemAdded).unwrap(),
            "\"item_added\""
        );
    }
}
