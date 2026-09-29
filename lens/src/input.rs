//! Read the two exports and join them on outlet id. `lens` reads these
//! directories and nothing else (plan §2): no network, no databases.
//!
//! The feed export is the outlet registry (names and domains); the
//! ledger export carries claims and the source registry. Fields not
//! needed for pages are left out of the structs and ignored on read,
//! so a contract addition never breaks a build.

use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Outlet {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Item {
    pub id: u64,
    pub outlet_id: String,
    pub canonical_url: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub published_at: i64,
    #[serde(default)]
    pub fetched_at: i64,
    /// Detected from the item's own text by `feed`; never from the outlet.
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub text_hash: u64,
    #[serde(default)]
    pub cluster_id: Option<u64>,
    #[serde(default)]
    pub joined_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Join {
    pub item_id: u64,
    pub outlet_id: String,
    #[serde(default)]
    pub published_at: i64,
    #[serde(default)]
    pub joined_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SynGroup {
    #[serde(default)]
    pub text_hash: u64,
    #[serde(default)]
    pub item_ids: Vec<u64>,
    #[serde(default)]
    pub first_outlet_id: String,
    #[serde(default)]
    pub spread_secs: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Cluster {
    pub id: u64,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub last_active_at: i64,
    #[serde(default)]
    pub item_count: u32,
    #[serde(default)]
    pub outlet_count: u32,
    #[serde(default)]
    pub primary_title: String,
    #[serde(default)]
    pub origin_item_id: u64,
    #[serde(default)]
    pub origin_outlet_id: String,
    #[serde(default)]
    pub timeline: Vec<Join>,
    #[serde(default)]
    pub syndication: Vec<SynGroup>,
    #[serde(default)]
    pub expired: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Event {
    #[serde(default)]
    pub seq: i64,
    #[serde(default)]
    pub ts: i64,
    pub kind: String,
    #[serde(default)]
    pub cluster_id: Option<u64>,
    #[serde(default)]
    pub item_id: Option<u64>,
    #[serde(default)]
    pub merged_into: Option<u64>,
    #[serde(default)]
    pub snapshot_id: Option<u64>,
}

/// One earlier version of an item's text, from `revisions.ndjson`
/// (feed export, added after v1; optional on read).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Revision {
    pub item_id: u64,
    #[serde(default)]
    pub snapshot_id: u64,
    #[serde(default)]
    pub seen_at: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Source {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub licence: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub no_redistribution: bool,
    #[serde(default)]
    pub refresh_days: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Claim {
    pub outlet_id: String,
    pub field: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<BTreeMap<String, serde_json::Value>>,
    pub source_id: String,
    pub ref_url: String,
    #[serde(default)]
    pub retrieved_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Everything a build reads, joined. Maps are keyed by id so pages
/// look things up rather than scan.
#[derive(Debug, Default)]
pub struct Input {
    pub outlets: BTreeMap<String, Outlet>,
    pub items: HashMap<u64, Item>,
    pub clusters: BTreeMap<u64, Cluster>,
    pub events: Vec<Event>,
    /// Per item, oldest first. Empty when the export predates the file.
    pub revisions: HashMap<u64, Vec<Revision>>,
    pub sources: BTreeMap<String, Source>,
    /// Claims per outlet, in export order.
    pub claims: BTreeMap<String, Vec<Claim>>,
    /// Newest `fetched_at` in the feed export: the "now" every window
    /// is measured from, so a build is a pure function of its inputs.
    pub export_time: i64,
}

fn read_ndjson<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, line) in std::io::BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(&line)
                .map_err(|e| format!("{} line {}: {e}", path.display(), n + 1))?,
        );
    }
    Ok(out)
}

pub fn read(feed_export: &Path, ledger_export: &Path) -> Result<Input, String> {
    let outlets: Vec<Outlet> = read_ndjson(&feed_export.join("outlets.ndjson"))?;
    let items: Vec<Item> = read_ndjson(&feed_export.join("items.ndjson"))?;
    let clusters: Vec<Cluster> = read_ndjson(&feed_export.join("clusters.ndjson"))?;
    let events: Vec<Event> = read_ndjson(&feed_export.join("events.ndjson"))?;
    let revisions_path = feed_export.join("revisions.ndjson");
    let revisions: Vec<Revision> = if revisions_path.is_file() {
        read_ndjson(&revisions_path)?
    } else {
        Vec::new()
    };
    let sources: Vec<Source> = read_ndjson(&ledger_export.join("sources.ndjson"))?;
    let claims: Vec<Claim> = read_ndjson(&ledger_export.join("claims.ndjson"))?;

    let export_time = items.iter().map(|i| i.fetched_at).max().unwrap_or(0);
    let mut input = Input {
        outlets: outlets.into_iter().map(|o| (o.id.clone(), o)).collect(),
        items: items.into_iter().map(|i| (i.id, i)).collect(),
        clusters: clusters.into_iter().map(|c| (c.id, c)).collect(),
        events,
        revisions: HashMap::new(),
        sources: sources.into_iter().map(|s| (s.id.clone(), s)).collect(),
        claims: BTreeMap::new(),
        export_time,
    };
    for r in revisions {
        input.revisions.entry(r.item_id).or_default().push(r);
    }
    for v in input.revisions.values_mut() {
        v.sort_by_key(|r| (r.seen_at, r.snapshot_id));
    }
    // Claims for outlets the feed registry does not know are kept:
    // ledger is allowed to know more outlets than feed polls. Claims
    // from a source that forbids redistribution are dropped here as
    // `ledger check` would refuse to serve them.
    for c in claims {
        let allowed = input
            .sources
            .get(&c.source_id)
            .is_some_and(|s| !s.no_redistribution);
        if allowed {
            input.claims.entry(c.outlet_id.clone()).or_default().push(c);
        }
    }
    input.events.sort_by_key(|e| e.seq);
    Ok(input)
}

impl Input {
    /// Display name for an outlet id, falling back to the id.
    pub fn outlet_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.outlets
            .get(id)
            .map(|o| o.name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(id)
    }
}

#[cfg(test)]
pub(crate) fn fixture_input() -> Input {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    read(
        &root.join("../ledger/fixtures/feed_export"),
        &root.join("fixtures/ledger_export"),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_joins_the_recorded_exports() {
        let i = fixture_input();
        assert_eq!(i.outlets.len(), 3);
        assert_eq!(i.outlets["wire"].name, "Wire");
        assert_eq!(i.items.len(), 95);
        assert_eq!(i.clusters.len(), 61);
        assert!(i.events.windows(2).all(|w| w[0].seq <= w[1].seq));
        assert_eq!(i.sources.len(), 5);
        assert_eq!(i.claims["wire"].len(), 11);
        assert_eq!(i.claims["gazette"].len(), 8);
        assert!(i.export_time > 0);
        assert_eq!(i.revisions.len(), 11, "10 revised items and one withdrawn");
        assert_eq!(i.revisions[&1002][0].title, "Story 1002, as first seen");
        assert_eq!(i.revisions[&1002][1].title, "Story 1002");
        let c = &i.clusters[&1001];
        assert_eq!(c.origin_outlet_id, "wire");
        assert_eq!(c.timeline.len(), 2);
        assert_eq!(c.syndication[0].first_outlet_id, "wire");
    }

    #[test]
    fn no_redistribution_claims_are_dropped() {
        let dir = std::env::temp_dir().join(format!("lens-input-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let feed = dir.join("feed");
        let ledger = dir.join("ledger");
        std::fs::create_dir_all(&feed).unwrap();
        std::fs::create_dir_all(&ledger).unwrap();
        for f in ["outlets", "items", "clusters", "events"] {
            std::fs::write(feed.join(format!("{f}.ndjson")), "").unwrap();
        }
        std::fs::write(
            ledger.join("sources.ndjson"),
            "{\"id\":\"open\",\"name\":\"Open\"}\n{\"id\":\"closed\",\"name\":\"Closed\",\"no_redistribution\":true}\n",
        )
        .unwrap();
        std::fs::write(
            ledger.join("claims.ndjson"),
            "{\"outlet_id\":\"x\",\"field\":\"bias\",\"value\":\"a\",\"source_id\":\"open\",\"ref_url\":\"https://o\"}\n{\"outlet_id\":\"x\",\"field\":\"bias\",\"value\":\"b\",\"source_id\":\"closed\",\"ref_url\":\"https://c\"}\n",
        )
        .unwrap();
        let i = read(&feed, &ledger).unwrap();
        assert_eq!(i.claims["x"].len(), 1);
        assert_eq!(i.claims["x"][0].source_id, "open");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
