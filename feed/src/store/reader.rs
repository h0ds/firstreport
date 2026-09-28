//! Read side. One connection behind a mutex; callers hop to a blocking
//! thread via `Reader::call`. WAL mode lets this run beside the writer.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::writer::FeedState;
use super::{to_i64, to_u64};
use crate::types::Item;

#[derive(Clone)]
pub struct Reader {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotMeta {
    pub id: u64,
    pub feed_url: String,
    pub fetched_at: i64,
    pub status: u16,
    pub body_hash: u64,
    pub item_count: u32,
    #[serde(skip)]
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutletRow {
    pub id: String,
    pub name: String,
    pub domains: Vec<String>,
    pub feeds: Vec<String>,
}

/// The persisted cluster row. Timeline, velocity and syndication are
/// derived from items on read (see `cluster::propagation`).
#[derive(Debug, Clone, Serialize)]
pub struct ClusterRow {
    pub id: u64,
    pub created_at: i64,
    pub last_active_at: i64,
    pub item_count: u32,
    pub outlet_count: u32,
    pub primary_title: String,
    #[serde(skip)]
    pub centroid: Vec<f32>,
    pub expired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClusterSort {
    #[default]
    Recent,
    Velocity1h,
    Outlets6h,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub seq: i64,
    #[serde(flatten)]
    pub event: crate::types::ClusterEvent,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StoreStats {
    pub items_total: u64,
    pub items_last_hour: u64,
    pub items_last_24h: u64,
    pub snapshots_total: u64,
    pub feeds_total: u64,
    pub feeds_dead: u64,
    pub outlets_total: u64,
    pub items_embedded: u64,
    pub clusters_total: u64,
    pub clusters_active: u64,
}

impl Reader {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Ok(Self::from_conn(super::open(path)?))
    }

    pub fn from_conn(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Run `f` on a blocking thread with the connection.
    pub async fn call<T, F>(&self, f: F) -> rusqlite::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(|p| p.into_inner());
            f(&guard)
        })
        .await
        .expect("reader task panicked")
    }

    /// Synchronous variant for boot-time work.
    pub fn with<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let guard = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        f(&guard)
    }
}

// ---- queries, all plain functions over a connection so they are usable from
// either the async `call` path or synchronous boot code.

const ITEM_COLS: &str =
    "id, outlet_id, canonical_url, title, summary, published_at, fetched_at, lang, \
                         text_hash, cluster_id, cluster_rank, joined_at, snapshot_id";

fn row_item(r: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: to_u64(r.get(0)?),
        outlet_id: r.get(1)?,
        canonical_url: r.get(2)?,
        title: r.get(3)?,
        summary: r.get(4)?,
        published_at: r.get(5)?,
        fetched_at: r.get(6)?,
        lang: r.get(7)?,
        text_hash: to_u64(r.get(8)?),
        cluster_id: r.get::<_, Option<i64>>(9)?.map(to_u64),
        cluster_rank: r.get(10)?,
        joined_at: r.get(11)?,
        snapshot_id: to_u64(r.get(12)?),
    })
}

pub fn has_item(c: &Connection, id: u64) -> rusqlite::Result<bool> {
    c.query_row("SELECT 1 FROM item WHERE id=?1", [to_i64(id)], |_| Ok(()))
        .optional()
        .map(|o| o.is_some())
}

#[allow(dead_code)] // clusterer rebuild-from-store (M3)
pub fn get_embedding(c: &Connection, id: u64) -> rusqlite::Result<Option<Vec<f32>>> {
    c.query_row(
        "SELECT embedding FROM item WHERE id=?1",
        [to_i64(id)],
        |r| r.get::<_, Option<Vec<u8>>>(0),
    )
    .optional()
    .map(|o| o.flatten().map(|b| super::writer::embedding_from_blob(&b)))
}

/// Items fetched since `since` that have no embedding yet, oldest first.
/// Used on boot so a restart or a late-installed model catches up.
pub fn unembedded_since(c: &Connection, since: i64, limit: usize) -> rusqlite::Result<Vec<Item>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {ITEM_COLS} FROM item WHERE embedding IS NULL AND fetched_at >= ?1 ORDER BY fetched_at ASC, id ASC LIMIT ?2"
    ))?;
    let rows = st.query_map(params![since, limit as i64], row_item)?;
    rows.collect()
}

pub fn get_item(c: &Connection, id: u64) -> rusqlite::Result<Option<Item>> {
    c.query_row(
        &format!("SELECT {ITEM_COLS} FROM item WHERE id=?1"),
        [to_i64(id)],
        row_item,
    )
    .optional()
}

/// Ids fetched at or after `since`, for rebuilding the dedupe bloom on boot.
pub fn item_ids_since(c: &Connection, since: i64) -> rusqlite::Result<Vec<u64>> {
    let mut st = c.prepare("SELECT id FROM item WHERE fetched_at >= ?1")?;
    let rows = st.query_map([since], |r| r.get::<_, i64>(0).map(to_u64))?;
    rows.collect()
}

/// Newest first by (fetched_at, id). `cursor` is the (fetched_at, id) of the
/// last row already seen.
pub fn list_items(
    c: &Connection,
    since: i64,
    outlet: Option<&str>,
    cursor: Option<(i64, u64)>,
    limit: usize,
) -> rusqlite::Result<Vec<Item>> {
    let (cf, ci) = cursor
        .map(|(f, i)| (f, to_i64(i)))
        .unwrap_or((i64::MAX, i64::MAX));
    let mut st = c.prepare_cached(&format!(
        "SELECT {ITEM_COLS} FROM item
         WHERE fetched_at >= ?1 AND (?2 IS NULL OR outlet_id = ?2)
           AND (fetched_at < ?3 OR (fetched_at = ?3 AND id < ?4))
         ORDER BY fetched_at DESC, id DESC LIMIT ?5"
    ))?;
    let rows = st.query_map(params![since, outlet, cf, ci, limit as i64], row_item)?;
    rows.collect()
}

fn row_snapshot(r: &Row) -> rusqlite::Result<SnapshotMeta> {
    Ok(SnapshotMeta {
        id: to_u64(r.get(0)?),
        feed_url: r.get(1)?,
        fetched_at: r.get(2)?,
        status: r.get(3)?,
        body_hash: to_u64(r.get(4)?),
        path: r.get(5)?,
        item_count: r.get(6)?,
    })
}

const SNAP_COLS: &str = "id, feed_url, fetched_at, status, body_hash, path, item_count";

pub fn get_snapshot(c: &Connection, id: u64) -> rusqlite::Result<Option<SnapshotMeta>> {
    c.query_row(
        &format!("SELECT {SNAP_COLS} FROM snapshot WHERE id=?1"),
        [to_i64(id)],
        row_snapshot,
    )
    .optional()
}

pub fn list_snapshots(
    c: &Connection,
    feed: Option<&str>,
    since: i64,
    cursor: Option<(i64, u64)>,
    limit: usize,
) -> rusqlite::Result<Vec<SnapshotMeta>> {
    let (cf, ci) = cursor
        .map(|(f, i)| (f, to_i64(i)))
        .unwrap_or((i64::MAX, i64::MAX));
    let mut st = c.prepare_cached(&format!(
        "SELECT {SNAP_COLS} FROM snapshot
         WHERE fetched_at >= ?1 AND (?2 IS NULL OR feed_url = ?2)
           AND (fetched_at < ?3 OR (fetched_at = ?3 AND id < ?4))
         ORDER BY fetched_at DESC, id DESC LIMIT ?5"
    ))?;
    let rows = st.query_map(params![since, feed, cf, ci, limit as i64], row_snapshot)?;
    rows.collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct RevisionRow {
    pub snapshot_id: u64,
    pub seen_at: i64,
    pub title: String,
    pub summary: String,
}

/// Text history of an item, oldest first. Empty until the first change.
pub fn item_revisions(c: &Connection, item_id: u64) -> rusqlite::Result<Vec<RevisionRow>> {
    let mut st = c.prepare_cached(
        "SELECT snapshot_id, seen_at, title, summary FROM item_revision
         WHERE item_id=?1 ORDER BY seen_at ASC, snapshot_id ASC",
    )?;
    let rows = st.query_map([to_i64(item_id)], |r| {
        Ok(RevisionRow {
            snapshot_id: to_u64(r.get(0)?),
            seen_at: r.get(1)?,
            title: r.get(2)?,
            summary: r.get(3)?,
        })
    })?;
    rows.collect()
}

/// Items whose simhash is within `threshold` bits of `hash`, across all
/// clusters, fetched since `since`. Hamming distance can't use the index,
/// so this scans the window; `since` bounds the cost.
pub fn syndication_matches(
    c: &Connection,
    hash: u64,
    threshold: u32,
    since: i64,
    limit: usize,
) -> rusqlite::Result<Vec<Item>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {ITEM_COLS} FROM item WHERE fetched_at >= ?1 ORDER BY published_at ASC, id ASC"
    ))?;
    let rows = st.query_map([since], row_item)?;
    let mut out = Vec::new();
    for r in rows {
        let it = r?;
        if crate::parse::simhash::hamming(it.text_hash, hash) <= threshold {
            out.push(it);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// Latest snapshot for a feed, if any; the previous item set for revision
/// diffing is rebuilt from its body on boot.
pub fn latest_snapshot(c: &Connection, feed: &str) -> rusqlite::Result<Option<SnapshotMeta>> {
    c.query_row(
        &format!("SELECT {SNAP_COLS} FROM snapshot WHERE feed_url=?1 ORDER BY fetched_at DESC, id DESC LIMIT 1"),
        [feed],
        row_snapshot,
    )
    .optional()
}

const CLUSTER_COLS: &str =
    "id, created_at, last_active_at, item_count, outlet_count, primary_title, centroid, expired";

fn row_cluster(r: &Row) -> rusqlite::Result<ClusterRow> {
    Ok(ClusterRow {
        id: to_u64(r.get(0)?),
        created_at: r.get(1)?,
        last_active_at: r.get(2)?,
        item_count: r.get(3)?,
        outlet_count: r.get(4)?,
        primary_title: r.get(5)?,
        centroid: super::writer::embedding_from_blob(&r.get::<_, Vec<u8>>(6)?),
        expired: r.get::<_, i64>(7)? != 0,
    })
}

pub fn get_cluster(c: &Connection, id: u64) -> rusqlite::Result<Option<ClusterRow>> {
    c.query_row(
        &format!("SELECT {CLUSTER_COLS} FROM cluster WHERE id=?1"),
        [to_i64(id)],
        row_cluster,
    )
    .optional()
}

/// Every cluster row, for replay output and tests.
pub fn all_clusters(c: &Connection) -> rusqlite::Result<Vec<ClusterRow>> {
    let mut st = c.prepare(&format!("SELECT {CLUSTER_COLS} FROM cluster ORDER BY id"))?;
    let rows = st.query_map([], row_cluster)?;
    rows.collect()
}

/// Clusters active since `since`, filtered by outlet count. `Recent`
/// orders by last activity with a `(last_active_at, id)` cursor; the
/// velocity sorts rank by joins in the trailing window and are top-N
/// only (no cursor), since a moving count can't page stably.
pub fn list_clusters(
    c: &Connection,
    since: i64,
    min_outlets: u32,
    sort: ClusterSort,
    cursor: Option<(i64, u64)>,
    limit: usize,
    now: i64,
) -> rusqlite::Result<Vec<ClusterRow>> {
    match sort {
        ClusterSort::Recent => {
            let (cf, ci) = cursor
                .map(|(f, i)| (f, to_i64(i)))
                .unwrap_or((i64::MAX, i64::MAX));
            let mut st = c.prepare_cached(&format!(
                "SELECT {CLUSTER_COLS} FROM cluster
                 WHERE last_active_at >= ?1 AND outlet_count >= ?2
                   AND (last_active_at < ?3 OR (last_active_at = ?3 AND id < ?4))
                 ORDER BY last_active_at DESC, id DESC LIMIT ?5"
            ))?;
            let rows = st.query_map(
                params![since, min_outlets, cf, ci, limit as i64],
                row_cluster,
            )?;
            rows.collect()
        }
        ClusterSort::Velocity1h | ClusterSort::Outlets6h => {
            let (window, expr) = match sort {
                ClusterSort::Velocity1h => (3600, "COUNT(*)"),
                _ => (6 * 3600, "COUNT(DISTINCT i.outlet_id)"),
            };
            let mut st = c.prepare_cached(&format!(
                "SELECT {CLUSTER_COLS} FROM cluster
                 WHERE last_active_at >= ?1 AND outlet_count >= ?2 AND expired = 0
                 ORDER BY (SELECT {expr} FROM item i WHERE i.cluster_id = cluster.id AND i.joined_at >= ?3) DESC,
                          last_active_at DESC, id DESC
                 LIMIT ?4"
            ))?;
            let rows = st.query_map(
                params![since, min_outlets, now - window, limit as i64],
                row_cluster,
            )?;
            rows.collect()
        }
    }
}

/// Members in join order.
pub fn cluster_items(c: &Connection, cluster_id: u64) -> rusqlite::Result<Vec<Item>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {ITEM_COLS} FROM item WHERE cluster_id=?1 ORDER BY cluster_rank ASC, joined_at ASC, id ASC"
    ))?;
    let rows = st.query_map([to_i64(cluster_id)], row_item)?;
    rows.collect()
}

/// Every item with its embedding if present, newest first, paged by
/// `(fetched_at, id)` cursor. For the Parquet export.
#[cfg_attr(not(feature = "parquet"), allow(dead_code))]
pub fn list_items_with_embeddings(
    c: &Connection,
    cursor: Option<(i64, u64)>,
    limit: usize,
) -> rusqlite::Result<Vec<(Item, Option<Vec<f32>>)>> {
    let (cf, ci) = cursor
        .map(|(f, i)| (f, to_i64(i)))
        .unwrap_or((i64::MAX, i64::MAX));
    let mut st = c.prepare_cached(&format!(
        "SELECT {ITEM_COLS}, embedding FROM item
         WHERE (fetched_at < ?1 OR (fetched_at = ?1 AND id < ?2))
         ORDER BY fetched_at DESC, id DESC LIMIT ?3"
    ))?;
    let rows = st.query_map(params![cf, ci, limit as i64], |r| {
        let item = row_item(r)?;
        let blob: Option<Vec<u8>> = r.get(13)?;
        Ok((item, blob.map(|b| super::writer::embedding_from_blob(&b))))
    })?;
    rows.collect()
}

/// Items with embeddings fetched since `since`, oldest first, for
/// rebuilding the active set when there is no usable checkpoint.
pub fn embedded_items_since(c: &Connection, since: i64) -> rusqlite::Result<Vec<(Item, Vec<f32>)>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {ITEM_COLS}, embedding FROM item WHERE embedding IS NOT NULL AND fetched_at >= ?1
         ORDER BY fetched_at ASC, id ASC"
    ))?;
    let rows = st.query_map([since], |r| {
        let item = row_item(r)?;
        let blob: Vec<u8> = r.get(13)?;
        Ok((item, super::writer::embedding_from_blob(&blob)))
    })?;
    rows.collect()
}

pub fn events_after(
    c: &Connection,
    after_seq: i64,
    limit: usize,
) -> rusqlite::Result<Vec<EventRow>> {
    let mut st = c.prepare_cached(
        "SELECT seq, ts, kind, cluster_id, item_id, merged_into, snapshot_id FROM cluster_event
         WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
    )?;
    let rows = st.query_map(params![after_seq, limit as i64], |r| {
        let kind: String = r.get(2)?;
        let kind = serde_json::from_value(serde_json::Value::String(kind)).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(EventRow {
            seq: r.get(0)?,
            event: crate::types::ClusterEvent {
                ts: r.get(1)?,
                kind,
                cluster_id: r.get::<_, Option<i64>>(3)?.map(to_u64),
                item_id: r.get::<_, Option<i64>>(4)?.map(to_u64),
                merged_into: r.get::<_, Option<i64>>(5)?.map(to_u64),
                snapshot_id: r.get::<_, Option<i64>>(6)?.map(to_u64),
            },
        })
    })?;
    rows.collect()
}

pub fn feed_states(c: &Connection) -> rusqlite::Result<Vec<FeedState>> {
    let mut st = c.prepare(
        "SELECT url, outlet_id, kind, interval_secs, etag, last_modified, next_fetch_at,
                fail_count, last_ok_at, dead, base_interval_secs, resolved_url, last_fetch_at,
                last_status, last_error, last_items FROM feed ORDER BY url",
    )?;
    let rows = st.query_map([], |r| {
        let interval = r.get::<_, i64>(3)? as u64;
        Ok(FeedState {
            url: r.get(0)?,
            outlet_id: r.get(1)?,
            kind: r.get(2)?,
            interval_secs: interval,
            etag: r.get(4)?,
            last_modified: r.get(5)?,
            next_fetch_at: r.get(6)?,
            fail_count: r.get::<_, i64>(7)? as u32,
            last_ok_at: r.get(8)?,
            dead: r.get::<_, i64>(9)? != 0,
            base_interval_secs: r
                .get::<_, Option<i64>>(10)?
                .map(|v| v as u64)
                .unwrap_or(interval),
            resolved_url: r.get(11)?,
            last_fetch_at: r.get(12)?,
            last_status: r.get::<_, Option<i64>>(13)?.map(|v| v as u16),
            last_error: r.get(14)?,
            last_items: r.get::<_, Option<i64>>(15)?.unwrap_or(0) as u32,
        })
    })?;
    rows.collect()
}

pub fn outlets(c: &Connection) -> rusqlite::Result<Vec<OutletRow>> {
    let mut st = c.prepare("SELECT id, name FROM outlet ORDER BY id")?;
    let mut out: Vec<OutletRow> = st
        .query_map([], |r| {
            Ok(OutletRow {
                id: r.get(0)?,
                name: r.get(1)?,
                domains: vec![],
                feeds: vec![],
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut dom = c.prepare("SELECT host FROM outlet_domain WHERE outlet_id=?1 ORDER BY host")?;
    let mut fd = c.prepare("SELECT url FROM feed WHERE outlet_id=?1 ORDER BY url")?;
    for o in &mut out {
        o.domains = dom
            .query_map([&o.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        o.feeds = fd
            .query_map([&o.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    Ok(out)
}

/// The outlet whose registered domain is `host` or a parent of it, so
/// `news.example.com` finds an outlet registered as `example.com`. The
/// longest matching domain wins when several are registered.
pub fn outlet_for_host(c: &Connection, host: &str) -> rusqlite::Result<Option<OutletRow>> {
    let mut st = c.prepare_cached(
        "SELECT outlet_id, host FROM outlet_domain WHERE host = ?1 OR ?1 LIKE '%.' || host \
         ORDER BY length(host) DESC LIMIT 1",
    )?;
    let id: Option<String> = st.query_row([host], |r| r.get(0)).optional()?;
    let Some(id) = id else { return Ok(None) };
    Ok(outlets(c)?.into_iter().find(|o| o.id == id))
}

pub fn stats(c: &Connection, now: i64) -> rusqlite::Result<StoreStats> {
    let one = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> rusqlite::Result<u64> {
        c.query_row(sql, p, |r| r.get::<_, i64>(0))
            .map(|v| v as u64)
    };
    Ok(StoreStats {
        items_total: one("SELECT COUNT(*) FROM item", &[])?,
        items_last_hour: one(
            "SELECT COUNT(*) FROM item WHERE fetched_at >= ?1",
            &[&(now - 3600)],
        )?,
        items_last_24h: one(
            "SELECT COUNT(*) FROM item WHERE fetched_at >= ?1",
            &[&(now - 86_400)],
        )?,
        snapshots_total: one("SELECT COUNT(*) FROM snapshot", &[])?,
        feeds_total: one("SELECT COUNT(*) FROM feed", &[])?,
        feeds_dead: one("SELECT COUNT(*) FROM feed WHERE dead=1", &[])?,
        outlets_total: one("SELECT COUNT(*) FROM outlet", &[])?,
        items_embedded: one("SELECT COUNT(*) FROM item WHERE embedding IS NOT NULL", &[])?,
        clusters_total: one("SELECT COUNT(*) FROM cluster", &[])?,
        clusters_active: one("SELECT COUNT(*) FROM cluster WHERE expired=0", &[])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::writer::{commit, WriteMsg};

    fn item(id: u64, fetched: i64) -> Item {
        Item {
            id,
            outlet_id: "bbc".into(),
            canonical_url: format!("https://bbc.com/{id}"),
            title: "t".into(),
            summary: String::new(),
            published_at: fetched,
            fetched_at: fetched,
            lang: "en".into(),
            text_hash: 1,
            cluster_id: None,
            cluster_rank: None,
            joined_at: None,
            snapshot_id: 9,
        }
    }

    #[test]
    fn write_then_paginate() {
        let mut c = crate::store::open_memory().unwrap();
        commit(
            &mut c,
            vec![WriteMsg::Items(
                (1..=5).map(|i| item(i, 100 + i as i64)).collect(),
            )],
        )
        .unwrap();
        commit(&mut c, vec![WriteMsg::Items(vec![item(3, 999)])]).unwrap(); // dup ignored
        assert!(has_item(&c, 3).unwrap());
        assert!(!has_item(&c, 42).unwrap());
        let page1 = list_items(&c, 0, None, None, 2).unwrap();
        assert_eq!(page1.iter().map(|i| i.id).collect::<Vec<_>>(), vec![5, 4]);
        let last = page1.last().unwrap();
        let page2 = list_items(&c, 0, None, Some((last.fetched_at, last.id)), 2).unwrap();
        assert_eq!(page2.iter().map(|i| i.id).collect::<Vec<_>>(), vec![3, 2]);
        assert_eq!(get_item(&c, 3).unwrap().unwrap().fetched_at, 103);
        assert_eq!(item_ids_since(&c, 104).unwrap().len(), 2);
        let s = stats(&c, 105).unwrap();
        assert_eq!(s.items_total, 5);
        assert_eq!(s.items_last_hour, 5);
    }

    #[test]
    fn embeddings_roundtrip_and_backlog() {
        let mut c = crate::store::open_memory().unwrap();
        commit(
            &mut c,
            vec![WriteMsg::Items(
                (1..=3).map(|i| item(i, 100 + i as i64)).collect(),
            )],
        )
        .unwrap();
        assert_eq!(unembedded_since(&c, 0, 10).unwrap().len(), 3);
        commit(
            &mut c,
            vec![WriteMsg::Embeddings(vec![(2, vec![0.5, -1.25, 3.0])])],
        )
        .unwrap();
        assert_eq!(
            get_embedding(&c, 2).unwrap().unwrap(),
            vec![0.5, -1.25, 3.0]
        );
        assert_eq!(get_embedding(&c, 1).unwrap(), None);
        let backlog = unembedded_since(&c, 0, 10).unwrap();
        assert_eq!(backlog.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!(stats(&c, 200).unwrap().items_embedded, 1);
    }

    #[test]
    fn clusters_assignments_and_events() {
        use crate::types::{ClusterEvent, EventKind};
        let mut c = crate::store::open_memory().unwrap();
        commit(
            &mut c,
            vec![WriteMsg::Items(
                (1..=4).map(|i| item(i, 100 + i as i64)).collect(),
            )],
        )
        .unwrap();
        let row = |id: u64, last: i64, outlets: u32, expired: bool| ClusterRow {
            id,
            created_at: 100,
            last_active_at: last,
            item_count: 2,
            outlet_count: outlets,
            primary_title: format!("c{id}"),
            centroid: vec![0.6, 0.8],
            expired,
        };
        commit(
            &mut c,
            vec![
                WriteMsg::ClusterUpsert(row(1, 150, 2, false)),
                WriteMsg::ClusterUpsert(row(3, 160, 1, false)),
                WriteMsg::ItemAssigned {
                    item_id: 1,
                    cluster_id: 1,
                    rank: 0,
                    joined_at: 101,
                },
                WriteMsg::ItemAssigned {
                    item_id: 2,
                    cluster_id: 1,
                    rank: 1,
                    joined_at: 150,
                },
                WriteMsg::ItemAssigned {
                    item_id: 3,
                    cluster_id: 3,
                    rank: 0,
                    joined_at: 160,
                },
                WriteMsg::Embeddings(vec![(1, vec![1.0, 0.0]), (4, vec![0.0, 1.0])]),
                WriteMsg::Event(ClusterEvent {
                    ts: 150,
                    kind: EventKind::ItemAdded,
                    cluster_id: Some(1),
                    item_id: Some(2),
                    merged_into: None,
                    snapshot_id: None,
                }),
            ],
        )
        .unwrap();
        let got = get_cluster(&c, 1).unwrap().unwrap();
        assert_eq!(got.centroid, vec![0.6, 0.8]);
        assert_eq!(
            cluster_items(&c, 1)
                .unwrap()
                .iter()
                .map(|i| i.id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(cluster_items(&c, 1).unwrap()[1].cluster_rank, Some(1));
        let recent = list_clusters(&c, 0, 1, ClusterSort::Recent, None, 10, 200).unwrap();
        assert_eq!(recent.iter().map(|r| r.id).collect::<Vec<_>>(), vec![3, 1]);
        assert_eq!(
            list_clusters(&c, 0, 2, ClusterSort::Recent, None, 10, 200)
                .unwrap()
                .len(),
            1
        );
        let page2 = list_clusters(&c, 0, 1, ClusterSort::Recent, Some((160, 3)), 10, 200).unwrap();
        assert_eq!(page2[0].id, 1);
        let fast = list_clusters(&c, 0, 1, ClusterSort::Velocity1h, None, 10, 200).unwrap();
        assert_eq!(fast[0].id, 1, "two joins in the last hour beats one");
        // upsert updates in place
        commit(&mut c, vec![WriteMsg::ClusterUpsert(row(1, 170, 3, true))]).unwrap();
        assert!(get_cluster(&c, 1).unwrap().unwrap().expired);
        assert_eq!(all_clusters(&c).unwrap().len(), 2);
        let ev = events_after(&c, 0, 10).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].seq, 1);
        assert_eq!(ev[0].event.kind, EventKind::ItemAdded);
        assert!(events_after(&c, 1, 10).unwrap().is_empty());
        let emb = embedded_items_since(&c, 0).unwrap();
        assert_eq!(
            emb.iter().map(|(i, _)| i.id).collect::<Vec<_>>(),
            vec![1, 4]
        );
        assert_eq!(emb[1].1, vec![0.0, 1.0]);
        let s = stats(&c, 200).unwrap();
        assert_eq!((s.clusters_total, s.clusters_active), (2, 1));
    }

    #[test]
    fn u64_roundtrip_through_i64() {
        let mut c = crate::store::open_memory().unwrap();
        let big = u64::MAX - 7;
        commit(&mut c, vec![WriteMsg::Items(vec![item(big, 1)])]).unwrap();
        assert_eq!(get_item(&c, big).unwrap().unwrap().id, big);
    }
}
