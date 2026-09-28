//! Offline replay (plan §9): recorded feed snapshots → the full pipeline
//! with no network, a scratch store, single-threaded inference, and the
//! item clock driving everything. Output is a stable JSON description of
//! the clusters, which the golden test diffs against `fixtures/expected`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::cluster::{self, Clusterer, Outcome};
use crate::config::{Config, Feeds, Outlets};
use crate::embed;
use crate::fetch::client::Fetched;
use crate::parse::outlet::Resolver;
use crate::poller::Poller;
use crate::stats::Stats;
use crate::store::{self, Reader, WriteMsg};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(rename = "snapshot")]
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Deserialize)]
pub struct Snapshot {
    pub file: String,
    pub feed_url: String,
    pub outlet: String,
    pub fetched_at: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayCluster {
    pub id: u64,
    pub origin_item_id: u64,
    pub origin_outlet_id: String,
    pub outlet_count: u32,
    pub expired: bool,
    /// (item id, outlet id, title) in join order.
    pub items: Vec<(u64, String, String)>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ReplayResult {
    pub items_seen: u64,
    pub items_new: u64,
    pub clusters: Vec<ReplayCluster>,
}

/// Run every snapshot in `fixtures/manifest.toml` in `fetched_at` order.
/// `cfg` is the base config; the store and snapshot dir are redirected
/// to `scratch`, which is wiped first.
pub async fn run(
    mut cfg: Config,
    fixtures: &Path,
    scratch: &Path,
) -> Result<ReplayResult, Box<dyn std::error::Error>> {
    let manifest_path = fixtures.join("manifest.toml");
    let manifest: Manifest = toml::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    let outlets: Outlets =
        toml::from_str(&std::fs::read_to_string(fixtures.join("outlets.toml"))?)?;
    let mut snaps: Vec<&Snapshot> = manifest.snapshots.iter().collect();
    snaps.sort_by_key(|s| (s.fetched_at, s.feed_url.clone()));

    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch)?;
    cfg.store.path = scratch.join("replay.db");
    cfg.snapshots.dir = scratch.join("snapshots");
    let cfg = Arc::new(cfg);

    let (writer, thread) = store::spawn_writer(&cfg.store.path)?;
    let reader = Reader::open(&cfg.store.path)?;
    {
        let mut conn = store::open(&cfg.store.path)?;
        store::writer::sync_outlets(&mut conn, &outlets)?;
    }
    let stats = Arc::new(Stats::default());
    let resolver = Arc::new(Resolver::from_config(&outlets));
    let mut poller = Poller::new(
        cfg.clone(),
        resolver,
        reader.clone(),
        writer.clone(),
        stats.clone(),
    )?;
    let mut embedder = embed::load_from_config(&cfg, Some(1))?;
    let mut clusterer = Clusterer::new(cfg.cluster.clone());

    // Feed states keyed by url so etag/outlet carry across snapshots.
    let feeds = Feeds {
        feeds: snaps
            .iter()
            .map(|s| crate::config::FeedEntry {
                url: s.feed_url.clone(),
                outlet: s.outlet.clone(),
                interval_secs: None,
                kind: None,
            })
            .fold(Vec::new(), |mut acc: Vec<crate::config::FeedEntry>, f| {
                if !acc.iter().any(|x| x.url == f.url) {
                    acc.push(f);
                }
                acc
            }),
    };
    let states = Poller::build_states(&feeds, vec![], cfg.poll.default_interval_secs, 0);

    let mut clock = 0;
    for s in snaps {
        let state = states
            .iter()
            .find(|f| f.url == s.feed_url)
            .expect("state for every feed");
        let body = std::fs::read(fixtures.join(&s.file))?;
        let fetched = Fetched {
            status: 200,
            body: Some(Bytes::from(body)),
            etag: None,
            last_modified: None,
        };
        let p = poller.process(state, s.fetched_at, Ok(fetched)).await;
        if p.new_items.is_empty() {
            continue;
        }
        let template = cfg.embed.input_template.clone();
        let texts: Vec<String> = p
            .new_items
            .iter()
            .map(|it| embed::input_text(&template, it))
            .collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let vectors = embedder.embed(&refs)?;
        let mut rows = Vec::new();
        for (item, vector) in p.new_items.iter().zip(vectors) {
            clock = clock.max(item.fetched_at);
            rows.push((item.id, vector.clone()));
            let a = clusterer.assign(item, &vector, item.fetched_at);
            let (cluster_id, rank) = match a.outcome {
                Outcome::Created { cluster_id } => (cluster_id, 0),
                Outcome::Joined { cluster_id, rank } => (cluster_id, rank),
                Outcome::Skipped => continue,
            };
            writer
                .send(WriteMsg::ItemAssigned {
                    item_id: item.id,
                    cluster_id,
                    rank,
                    joined_at: item.fetched_at,
                })
                .await;
            if let Some(c) = clusterer.get(cluster_id) {
                writer
                    .send(WriteMsg::ClusterUpsert(cluster::row_for(c, false)))
                    .await;
            }
            for ev in a.events {
                writer.send(WriteMsg::Event(ev)).await;
            }
        }
        writer.send(WriteMsg::Embeddings(rows)).await;
    }
    let mut expired_ids = std::collections::HashSet::new();
    for (c, ev) in clusterer.expire(clock) {
        expired_ids.insert(c.id);
        writer
            .send(WriteMsg::ClusterUpsert(cluster::row_for(&c, true)))
            .await;
        writer.send(WriteMsg::Event(ev)).await;
    }
    writer.flush().await;

    // Read back through the store so the golden file covers persistence too.
    let mut rows = reader.with(store::reader::all_clusters)?;
    rows.sort_by_key(|r| r.id);
    let mut clusters = Vec::with_capacity(rows.len());
    for r in rows {
        let items = reader.with(|c| store::reader::cluster_items(c, r.id))?;
        let origin = cluster::propagation::origin(&items);
        clusters.push(ReplayCluster {
            id: r.id,
            origin_item_id: origin.map(|i| i.id).unwrap_or(r.id),
            origin_outlet_id: origin.map(|i| i.outlet_id.clone()).unwrap_or_default(),
            outlet_count: r.outlet_count,
            expired: r.expired,
            items: items
                .iter()
                .map(|i| (i.id, i.outlet_id.clone(), i.title.clone()))
                .collect(),
        });
    }
    let snap = stats.snapshot(clock);
    let result = ReplayResult {
        items_seen: snap.items_seen,
        items_new: snap.items_new,
        clusters,
    };

    drop(writer);
    drop(poller);
    let _ = thread.join();
    info!(
        clusters = result.clusters.len(),
        items = result.items_new,
        "replay complete"
    );
    Ok(result)
}

pub fn default_scratch() -> PathBuf {
    std::env::temp_dir().join(format!("kagami-replay-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EmbedBackend;

    /// Determinism guarantee: the recorded fixtures produce exactly the
    /// clusters in `fixtures/expected/clusters.json`, using the hash
    /// embedder so no model is needed. Set `KAGAMI_UPDATE_EXPECTED=1` to
    /// regenerate after an intentional change.
    #[tokio::test]
    async fn fixtures_match_golden_clusters() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut cfg = Config::default();
        cfg.embed.backend = EmbedBackend::Hash;
        cfg.embed.hash_dim = 256;
        cfg.cluster.join_threshold = 0.5; // lexical embedder; tuned for the fixtures, not production
        let scratch = default_scratch();
        let got = run(cfg, &root.join("replay"), &scratch).await.unwrap();
        let _ = std::fs::remove_dir_all(&scratch);
        let expected_path = root.join("expected/clusters.json");
        if std::env::var("KAGAMI_UPDATE_EXPECTED").is_ok() {
            std::fs::write(&expected_path, serde_json::to_string_pretty(&got).unwrap()).unwrap();
        }
        let want: ReplayResult =
            serde_json::from_str(&std::fs::read_to_string(&expected_path).unwrap()).unwrap();
        assert_eq!(
            got, want,
            "replay drifted from fixtures/expected/clusters.json"
        );
        // Also sanity check the shape so the golden file can't silently be trivial.
        assert!(
            got.clusters.iter().any(|c| c.outlet_count >= 2),
            "no cross-outlet cluster formed"
        );
        assert!(
            got.clusters.iter().any(|c| c.expired),
            "no cluster expired over the fixture window"
        );
    }
}
