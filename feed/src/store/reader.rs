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

/// Latest snapshot for a feed, if any (used for revision diffing later).
#[allow(dead_code)] // revision tracking (M4)
pub fn latest_snapshot(c: &Connection, feed: &str) -> rusqlite::Result<Option<SnapshotMeta>> {
    c.query_row(
        &format!("SELECT {SNAP_COLS} FROM snapshot WHERE feed_url=?1 ORDER BY fetched_at DESC, id DESC LIMIT 1"),
        [feed],
        row_snapshot,
    )
    .optional()
}

pub fn feed_states(c: &Connection) -> rusqlite::Result<Vec<FeedState>> {
    let mut st = c.prepare(
        "SELECT url, outlet_id, kind, interval_secs, etag, last_modified, next_fetch_at,
                fail_count, last_ok_at, dead FROM feed",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FeedState {
            url: r.get(0)?,
            outlet_id: r.get(1)?,
            kind: r.get(2)?,
            interval_secs: r.get::<_, i64>(3)? as u64,
            etag: r.get(4)?,
            last_modified: r.get(5)?,
            next_fetch_at: r.get(6)?,
            fail_count: r.get::<_, i64>(7)? as u32,
            last_ok_at: r.get(8)?,
            dead: r.get::<_, i64>(9)? != 0,
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
    fn u64_roundtrip_through_i64() {
        let mut c = crate::store::open_memory().unwrap();
        let big = u64::MAX - 7;
        commit(&mut c, vec![WriteMsg::Items(vec![item(big, 1)])]).unwrap();
        assert_eq!(get_item(&c, big).unwrap().unwrap().id, big);
    }
}
