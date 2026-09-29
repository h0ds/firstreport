//! Schema from plan §5.4, applied idempotently with a `user_version` stamp.

use rusqlite::Connection;

pub const SCHEMA_VERSION: i32 = 4;

const V1: &str = "
CREATE TABLE IF NOT EXISTS outlet (
  id TEXT PRIMARY KEY, name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS outlet_domain (
  host TEXT PRIMARY KEY, outlet_id TEXT NOT NULL REFERENCES outlet(id)
);
CREATE TABLE IF NOT EXISTS feed (
  url TEXT PRIMARY KEY, outlet_id TEXT NOT NULL, kind TEXT,
  interval_secs INTEGER NOT NULL, etag TEXT, last_modified TEXT,
  next_fetch_at INTEGER NOT NULL, fail_count INTEGER NOT NULL DEFAULT 0,
  last_ok_at INTEGER, dead INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS item (
  id INTEGER PRIMARY KEY,
  outlet_id TEXT NOT NULL, canonical_url TEXT NOT NULL,
  title TEXT NOT NULL, summary TEXT NOT NULL,
  published_at INTEGER NOT NULL, fetched_at INTEGER NOT NULL,
  lang TEXT NOT NULL, text_hash INTEGER NOT NULL,
  cluster_id INTEGER, cluster_rank INTEGER, joined_at INTEGER,
  snapshot_id INTEGER NOT NULL,
  embedding BLOB
);
CREATE INDEX IF NOT EXISTS item_cluster ON item(cluster_id, cluster_rank);
CREATE INDEX IF NOT EXISTS item_published ON item(published_at);
CREATE INDEX IF NOT EXISTS item_fetched ON item(fetched_at);
CREATE INDEX IF NOT EXISTS item_text_hash ON item(text_hash);
CREATE TABLE IF NOT EXISTS item_revision (
  item_id INTEGER NOT NULL, snapshot_id INTEGER NOT NULL,
  seen_at INTEGER NOT NULL, title TEXT NOT NULL, summary TEXT NOT NULL,
  PRIMARY KEY (item_id, snapshot_id)
);
CREATE TABLE IF NOT EXISTS snapshot (
  id INTEGER PRIMARY KEY, feed_url TEXT NOT NULL, fetched_at INTEGER NOT NULL,
  status INTEGER NOT NULL, body_hash INTEGER NOT NULL, path TEXT NOT NULL,
  item_count INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS snapshot_feed ON snapshot(feed_url, fetched_at);
CREATE TABLE IF NOT EXISTS cluster (
  id INTEGER PRIMARY KEY, created_at INTEGER NOT NULL,
  last_active_at INTEGER NOT NULL, item_count INTEGER NOT NULL,
  outlet_count INTEGER NOT NULL, primary_title TEXT NOT NULL,
  centroid BLOB NOT NULL, expired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS cluster_event (
  seq INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL,
  kind TEXT NOT NULL,
  cluster_id INTEGER, item_id INTEGER, merged_into INTEGER, snapshot_id INTEGER
);
";

/// v2: per-feed health and adaptive polling state on the feed table.
const V2: &str = "
ALTER TABLE feed ADD COLUMN base_interval_secs INTEGER;
ALTER TABLE feed ADD COLUMN resolved_url TEXT;
ALTER TABLE feed ADD COLUMN last_fetch_at INTEGER;
ALTER TABLE feed ADD COLUMN last_status INTEGER;
ALTER TABLE feed ADD COLUMN last_error TEXT;
ALTER TABLE feed ADD COLUMN last_items INTEGER NOT NULL DEFAULT 0;
";

/// v3: steward feed health as JSON on the feed row.
const V3: &str = "
ALTER TABLE feed ADD COLUMN health TEXT;
";

/// v4: clustering confidence (docs/steward.md §3): the similarity each
/// item joined at, and whether a cluster absorbed another.
const V4: &str = "
ALTER TABLE item ADD COLUMN join_sim REAL;
ALTER TABLE cluster ADD COLUMN merged INTEGER NOT NULL DEFAULT 0;
";

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let v: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v < 1 {
        conn.execute_batch(V1)?;
    }
    if v < 2 {
        // A fresh V1 already lacks these columns, so this applies in both
        // the fresh and the upgrade case.
        conn.execute_batch(V2)?;
    }
    if v < 3 {
        conn.execute_batch(V3)?;
    }
    if v < 4 {
        conn.execute_batch(V4)?;
    }
    if v < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrates_twice_cleanly() {
        let c = crate::store::open_memory().unwrap();
        super::migrate(&c).unwrap();
        let v: i32 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, super::SCHEMA_VERSION);
    }
}
