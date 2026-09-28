//! Single writer task. Owns the only writing connection; batches everything
//! it receives into transactions of up to `BATCH_ROWS` rows or `BATCH_WAIT`.

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

use super::to_i64;
use crate::types::Item;

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

#[derive(Debug, Clone)]
pub struct FeedState {
    pub url: String,
    pub outlet_id: String,
    pub kind: Option<String>,
    pub interval_secs: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub next_fetch_at: i64,
    pub fail_count: u32,
    pub last_ok_at: Option<i64>,
    pub dead: bool,
}

#[derive(Debug)]
pub enum WriteMsg {
    Items(Vec<Item>),
    Snapshot(SnapshotRow),
    FeedState(FeedState),
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
        let mut up_feed = tx.prepare_cached(
            "INSERT INTO feed (url, outlet_id, kind, interval_secs, etag, last_modified, next_fetch_at,
                               fail_count, last_ok_at, dead)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(url) DO UPDATE SET
               outlet_id=excluded.outlet_id, kind=excluded.kind, interval_secs=excluded.interval_secs,
               etag=excluded.etag, last_modified=excluded.last_modified,
               next_fetch_at=excluded.next_fetch_at, fail_count=excluded.fail_count,
               last_ok_at=excluded.last_ok_at, dead=excluded.dead",
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
                        f.dead as i32
                    ])?;
                }
                WriteMsg::Flush(_) => unreachable!("flushes are taken out before commit"),
            }
        }
    }
    tx.commit()
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
