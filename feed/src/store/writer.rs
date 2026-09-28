//! Single writer task. Owns the only writing connection; batches everything
//! it receives into transactions of up to `BATCH_ROWS` rows or `BATCH_WAIT`.

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

use super::reader::ClusterRow;
use super::to_i64;
use crate::types::{ClusterEvent, Item};

pub const BATCH_ROWS: usize = 256;
pub const BATCH_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct SnapshotRow {
    pub id: u64,
    pub feed_url: String,
    pub fetched_at: i64,
    pub status: u16,
    pub body_hash: u64,
    pub path: String,
    pub item_count: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FeedState {
    pub url: String,
    pub outlet_id: String,
    pub kind: Option<String>,
    /// Configured interval.
    pub base_interval_secs: u64,
    /// Current interval after adaptation.
    pub interval_secs: u64,
    #[serde(skip)]
    pub etag: Option<String>,
    #[serde(skip)]
    pub last_modified: Option<String>,
    pub next_fetch_at: i64,
    pub fail_count: u32,
    pub last_ok_at: Option<i64>,
    pub dead: bool,
    /// Where the feed permanently moved, if a 301/308 was seen.
    pub resolved_url: Option<String>,
    pub last_fetch_at: Option<i64>,
    pub last_status: Option<u16>,
    pub last_error: Option<String>,
    /// New items from the most recent fetch.
    pub last_items: u32,
}

#[derive(Debug)]
pub enum WriteMsg {
    Items(Vec<Item>),
    Snapshot(SnapshotRow),
    FeedState(FeedState),
    /// (item id, L2-normalized vector) pairs from the embedder.
    Embeddings(Vec<(u64, Vec<f32>)>),
    /// Item joined a cluster (or seeded one at rank 0).
    ItemAssigned {
        item_id: u64,
        cluster_id: u64,
        rank: u32,
        joined_at: i64,
    },
    ClusterUpsert(ClusterRow),
    Event(ClusterEvent),
    /// An item's text as seen in a snapshot. The item row is updated to the
    /// latest text; the embedding is left alone (cluster stability).
    Revision {
        item_id: u64,
        snapshot_id: u64,
        seen_at: i64,
        title: String,
        summary: String,
        /// Whether to overwrite `item.title` / `item.summary`. False when
        /// recording the original text retroactively.
        apply: bool,
    },
    /// Commit everything pending, then signal.
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct Writer {
    tx: mpsc::Sender<WriteMsg>,
}

impl Writer {
    pub async fn send(&self, msg: WriteMsg) {
        if self.tx.send(msg).await.is_err() {
            error!("store writer is gone");
        }
    }

    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        self.send(WriteMsg::Flush(tx)).await;
        let _ = rx.await;
    }
}

/// Spawn the writer on a dedicated OS thread. Returns the handle and a join
/// handle that completes once the channel closes and the last batch commits.
pub fn spawn_writer(path: &Path) -> rusqlite::Result<(Writer, std::thread::JoinHandle<()>)> {
    let conn = super::open(path)?;
    let (tx, rx) = mpsc::channel(4096);
    let handle = std::thread::Builder::new()
        .name("store-writer".into())
        .spawn(move || run(conn, rx))
        .expect("spawn store writer");
    Ok((Writer { tx }, handle))
}

fn run(mut conn: Connection, mut rx: mpsc::Receiver<WriteMsg>) {
    let mut batch: Vec<WriteMsg> = Vec::new();
    let mut flushes: Vec<oneshot::Sender<()>> = Vec::new();
    loop {
        // Block for the first message, then drain up to the batch limit or wait.
        let Some(first) = rx.blocking_recv() else {
            break;
        };
        let deadline = Instant::now() + BATCH_WAIT;
        let mut rows = push(&mut batch, &mut flushes, first);
        while rows < BATCH_ROWS && flushes.is_empty() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.try_recv() {
                Ok(m) => rows += push(&mut batch, &mut flushes, m),
                Err(mpsc::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        if let Err(e) = commit(&mut conn, std::mem::take(&mut batch)) {
            error!("store commit failed: {e}");
        }
        for f in flushes.drain(..) {
            let _ = f.send(());
        }
    }
    if let Err(e) = commit(&mut conn, std::mem::take(&mut batch)) {
        error!("store final commit failed: {e}");
    }
    info!("store writer stopped");
}

fn push(batch: &mut Vec<WriteMsg>, flushes: &mut Vec<oneshot::Sender<()>>, m: WriteMsg) -> usize {
    match m {
        WriteMsg::Flush(tx) => {
            flushes.push(tx);
            0
        }
        WriteMsg::Items(ref v) => {
            let n = v.len();
            batch.push(m);
            n
        }
        WriteMsg::Embeddings(ref v) => {
            let n = v.len();
            batch.push(m);
            n
        }
        other => {
            batch.push(other);
            1
        }
    }
}

pub fn commit(conn: &mut Connection, batch: Vec<WriteMsg>) -> rusqlite::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut ins_item = tx.prepare_cached(
            "INSERT OR IGNORE INTO item
             (id, outlet_id, canonical_url, title, summary, published_at, fetched_at, lang,
              text_hash, cluster_id, cluster_rank, joined_at, snapshot_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        )?;
        let mut ins_snap = tx.prepare_cached(
            "INSERT OR REPLACE INTO snapshot (id, feed_url, fetched_at, status, body_hash, path, item_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )?;
        let mut up_emb = tx.prepare_cached("UPDATE item SET embedding=?2 WHERE id=?1")?;
        let mut up_assign = tx.prepare_cached(
            "UPDATE item SET cluster_id=?2, cluster_rank=?3, joined_at=?4 WHERE id=?1",
        )?;
        let mut up_cluster = tx.prepare_cached(
            "INSERT INTO cluster (id, created_at, last_active_at, item_count, outlet_count,
                                  primary_title, centroid, expired)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET
               last_active_at=excluded.last_active_at, item_count=excluded.item_count,
               outlet_count=excluded.outlet_count, primary_title=excluded.primary_title,
               centroid=excluded.centroid, expired=excluded.expired",
        )?;
        let mut ins_rev = tx.prepare_cached(
            "INSERT OR IGNORE INTO item_revision (item_id, snapshot_id, seen_at, title, summary)
             VALUES (?1,?2,?3,?4,?5)",
        )?;
        let mut up_text = tx.prepare_cached("UPDATE item SET title=?2, summary=?3 WHERE id=?1")?;
        let mut ins_event = tx.prepare_cached(
            "INSERT INTO cluster_event (ts, kind, cluster_id, item_id, merged_into, snapshot_id)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )?;
        let mut up_feed = tx.prepare_cached(
            "INSERT INTO feed (url, outlet_id, kind, interval_secs, etag, last_modified, next_fetch_at,
                               fail_count, last_ok_at, dead, base_interval_secs, resolved_url,
                               last_fetch_at, last_status, last_error, last_items)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(url) DO UPDATE SET
               outlet_id=excluded.outlet_id, kind=excluded.kind, interval_secs=excluded.interval_secs,
               etag=excluded.etag, last_modified=excluded.last_modified,
               next_fetch_at=excluded.next_fetch_at, fail_count=excluded.fail_count,
               last_ok_at=excluded.last_ok_at, dead=excluded.dead,
               base_interval_secs=excluded.base_interval_secs, resolved_url=excluded.resolved_url,
               last_fetch_at=excluded.last_fetch_at, last_status=excluded.last_status,
               last_error=excluded.last_error, last_items=excluded.last_items",
        )?;
        for m in batch {
            match m {
                WriteMsg::Items(items) => {
                    for it in items {
                        ins_item.execute(params![
                            to_i64(it.id),
                            it.outlet_id,
                            it.canonical_url,
                            it.title,
                            it.summary,
                            it.published_at,
                            it.fetched_at,
                            it.lang,
                            to_i64(it.text_hash),
                            it.cluster_id.map(to_i64),
                            it.cluster_rank,
                            it.joined_at,
                            to_i64(it.snapshot_id),
                        ])?;
                    }
                }
                WriteMsg::Snapshot(s) => {
                    ins_snap.execute(params![
                        to_i64(s.id),
                        s.feed_url,
                        s.fetched_at,
                        s.status,
                        to_i64(s.body_hash),
                        s.path,
                        s.item_count
                    ])?;
                }
                WriteMsg::Embeddings(rows) => {
                    for (id, v) in rows {
                        up_emb.execute(params![to_i64(id), embedding_blob(&v)])?;
                    }
                }
                WriteMsg::ItemAssigned {
                    item_id,
                    cluster_id,
                    rank,
                    joined_at,
                } => {
                    up_assign.execute(params![
                        to_i64(item_id),
                        to_i64(cluster_id),
                        rank,
                        joined_at
                    ])?;
                }
                WriteMsg::ClusterUpsert(c) => {
                    up_cluster.execute(params![
                        to_i64(c.id),
                        c.created_at,
                        c.last_active_at,
                        c.item_count,
                        c.outlet_count,
                        c.primary_title,
                        embedding_blob(&c.centroid),
                        c.expired as i32
                    ])?;
                }
                WriteMsg::Revision {
                    item_id,
                    snapshot_id,
                    seen_at,
                    title,
                    summary,
                    apply,
                } => {
                    ins_rev.execute(params![
                        to_i64(item_id),
                        to_i64(snapshot_id),
                        seen_at,
                        title,
                        summary
                    ])?;
                    if apply {
                        up_text.execute(params![to_i64(item_id), title, summary])?;
                    }
                }
                WriteMsg::Event(e) => {
                    let kind = serde_json::to_value(e.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default();
                    ins_event.execute(params![
                        e.ts,
                        kind,
                        e.cluster_id.map(to_i64),
                        e.item_id.map(to_i64),
                        e.merged_into.map(to_i64),
                        e.snapshot_id.map(to_i64)
                    ])?;
                }
                WriteMsg::FeedState(f) => {
                    up_feed.execute(params![
                        f.url,
                        f.outlet_id,
                        f.kind,
                        f.interval_secs as i64,
                        f.etag,
                        f.last_modified,
                        f.next_fetch_at,
                        f.fail_count,
                        f.last_ok_at,
                        f.dead as i32,
                        f.base_interval_secs as i64,
                        f.resolved_url,
                        f.last_fetch_at,
                        f.last_status.map(|s| s as i64),
                        f.last_error,
                        f.last_items as i64
                    ])?;
                }
                WriteMsg::Flush(_) => unreachable!("flushes are taken out before commit"),
            }
        }
    }
    tx.commit()
}

/// f32 little-endian, as the data contract specifies for the blob.
pub fn embedding_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

pub fn embedding_from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Upsert the outlet registry. Called once at boot, synchronously.
pub fn sync_outlets(
    conn: &mut Connection,
    outlets: &crate::config::Outlets,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    for o in &outlets.outlets {
        tx.execute(
            "INSERT INTO outlet (id, name) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET name=excluded.name",
            params![o.id, o.name],
        )?;
        for d in &o.domains {
            tx.execute(
                "INSERT OR REPLACE INTO outlet_domain (host, outlet_id) VALUES (?1, ?2)",
                params![d.to_ascii_lowercase(), o.id],
            )?;
        }
    }
    tx.commit()
}
