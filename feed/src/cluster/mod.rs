//! The clusterer task (plan §5.3): consumes embedded items, assigns them,
//! persists assignments, cluster rows and events, expires idle clusters
//! on the item clock, and checkpoints the active set periodically.
//!
//! Expiry runs on the item clock (the latest `fetched_at` seen), not the
//! wall clock, so a fixture replay expires clusters exactly where a live
//! run would have and never depends on when the test ran.

pub mod active;
pub mod assign;
pub mod checkpoint;
pub mod propagation;
pub mod simd;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::embed::Embedded;
use crate::stats::Stats;
use crate::store::reader::ClusterRow;
use crate::store::writer::Writer;
use crate::store::WriteMsg;
use crate::types::Item;

pub use active::ActiveCluster;
pub use assign::{Clusterer, Outcome};

/// Minimum item-clock advance between expiry passes.
const EXPIRY_PASS_SECS: i64 = 60;

pub fn row_for(c: &ActiveCluster, expired: bool) -> ClusterRow {
    ClusterRow {
        id: c.id,
        created_at: c.created_at,
        last_active_at: c.last_active_at,
        item_count: c.n(),
        outlet_count: c.outlet_count(),
        primary_title: c.primary_title().to_string(),
        centroid: c.centroid.clone(),
        expired,
    }
}

/// Load the checkpoint if it is valid and matches `dim`; otherwise rebuild
/// by re-assigning the embedded items inside the window, oldest first.
/// Rebuild re-derives assignments and emits no events.
pub fn boot(
    cfg: &Config,
    ckpt: &Path,
    dim: Option<usize>,
    backlog: Vec<(Item, Vec<f32>)>,
) -> (Clusterer, Vec<WriteMsg>) {
    match checkpoint::read(ckpt) {
        // An empty checkpoint next to a non-empty backlog means the
        // clusterer never saw those items (for example the model was
        // installed after the last run); rebuilding is the only way to
        // cluster them, so treat it like a missing checkpoint.
        Ok((active, _)) if active.is_empty() && !backlog.is_empty() => {
            info!("checkpoint is empty but embedded items exist; rebuilding from store");
        }
        Ok((active, d)) if dim.map(|x| x == d).unwrap_or(true) => {
            info!(
                clusters = active.len(),
                dim = d,
                "clusterer restored from checkpoint"
            );
            return (Clusterer::from_active(cfg.cluster.clone(), active), vec![]);
        }
        Ok((_, d)) => warn!(
            ckpt_dim = d,
            "checkpoint dimension mismatch; rebuilding from store"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!("checkpoint unreadable ({e}); rebuilding from store"),
    }
    let mut clusterer = Clusterer::new(cfg.cluster.clone());
    let mut writes = Vec::new();
    let mut clock = 0;
    let n = backlog.len();
    for (item, vector) in backlog {
        clock = clock.max(item.fetched_at);
        let a = clusterer.assign(&item, &vector, item.fetched_at);
        if let Some((cluster_id, rank)) = assigned(&a.outcome) {
            writes.push(WriteMsg::ItemAssigned {
                item_id: item.id,
                cluster_id,
                rank,
                joined_at: item.fetched_at,
            });
            if let Some(c) = clusterer.get(cluster_id) {
                writes.push(WriteMsg::ClusterUpsert(row_for(c, false)));
            }
        }
    }
    for (c, _) in clusterer.expire(clock) {
        writes.push(WriteMsg::ClusterUpsert(row_for(&c, true)));
    }
    info!(
        items = n,
        clusters = clusterer.len(),
        "clusterer rebuilt from store"
    );
    (clusterer, writes)
}

fn assigned(o: &Outcome) -> Option<(u64, u32)> {
    match o {
        Outcome::Created { cluster_id } => Some((*cluster_id, 0)),
        Outcome::Joined { cluster_id, rank } => Some((*cluster_id, *rank)),
        Outcome::Skipped => None,
    }
}

pub struct Task {
    pub cfg: Arc<Config>,
    pub clusterer: Clusterer,
    pub writer: Writer,
    pub stats: Arc<Stats>,
    pub ckpt_path: PathBuf,
}

impl Task {
    /// Assign one embedded item and persist everything it changed.
    pub async fn handle(&mut self, e: Embedded, clock: &mut i64, last_expiry: &mut i64) {
        let now = e.item.fetched_at.max(*clock);
        *clock = now;
        let started = Instant::now();
        let a = self.clusterer.assign(&e.item, &e.vector, now);
        self.stats
            .record_assign(started.elapsed(), self.clusterer.len());
        if let Some((cluster_id, rank)) = assigned(&a.outcome) {
            self.writer
                .send(WriteMsg::ItemAssigned {
                    item_id: e.item.id,
                    cluster_id,
                    rank,
                    joined_at: now,
                })
                .await;
            if let Some(c) = self.clusterer.get(cluster_id) {
                self.writer
                    .send(WriteMsg::ClusterUpsert(row_for(c, false)))
                    .await;
            }
            for ev in a.events {
                self.writer.send(WriteMsg::Event(ev)).await;
            }
        }
        if now - *last_expiry >= EXPIRY_PASS_SECS {
            *last_expiry = now;
            self.expire(now).await;
        }
    }

    pub async fn expire(&mut self, now: i64) {
        for (c, ev) in self.clusterer.expire(now) {
            self.writer
                .send(WriteMsg::ClusterUpsert(row_for(&c, true)))
                .await;
            self.writer.send(WriteMsg::Event(ev)).await;
        }
    }

    pub fn checkpoint(&self) {
        let active = self.clusterer.active();
        let dim = active
            .first()
            .and_then(|c| c.vectors.first())
            .map(Vec::len)
            .unwrap_or(0);
        if let Err(e) = checkpoint::write(&self.ckpt_path, active, dim) {
            error!("checkpoint write failed: {e}");
        }
    }

    pub async fn run(
        mut self,
        mut rx: mpsc::Receiver<Embedded>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let every = Duration::from_secs(self.cfg.store.checkpoint_secs.max(5));
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut clock = 0i64;
        let mut last_expiry = 0i64;
        info!(
            active = self.clusterer.len(),
            checkpoint_secs = every.as_secs(),
            "clusterer started"
        );
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tick.tick() => self.checkpoint(),
                next = rx.recv() => match next {
                    Some(e) => self.handle(e, &mut clock, &mut last_expiry).await,
                    None => break,
                },
            }
        }
        self.checkpoint();
        info!(active = self.clusterer.len(), "clusterer stopped");
    }
}
