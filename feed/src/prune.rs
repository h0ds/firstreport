//! `feed prune`: the retention knobs that keep the disk line flat.
//!
//! - `--older-than <dur>` deletes items, their revisions, expired
//!   clusters and events older than the cutoff.
//! - `--embeddings` nulls embeddings on items outside the clustering
//!   window. Embeddings are three quarters of every item row and are
//!   only needed while the item can still be assigned; export them to
//!   Parquet first if you want them kept.
//! - Snapshot files older than `snapshots.retention_days` are removed
//!   on every run.

use std::path::Path;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::config::Config;

/// `90d`, `36h`, `15m`, or plain seconds.
pub fn parse_duration(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let (num, unit) = match s.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => s.split_at(i),
        None => (s, "s"),
    };
    let n: i64 = num.parse().map_err(|_| format!("bad duration {s:?}"))?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        _ => return Err(format!("bad duration unit in {s:?}; use s, m, h, d or w")),
    };
    Ok(n * mult)
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct Pruned {
    pub items: usize,
    pub revisions: usize,
    pub clusters: usize,
    pub events: usize,
    pub embeddings_cleared: usize,
    pub snapshot_files: usize,
    /// Files removed by the disk budget, on top of retention.
    pub snapshot_budget_files: usize,
    pub snapshot_budget_bytes: u64,
}

/// Delete rows older than `cutoff`. Active clusters are never deleted
/// here even if old, since the clusterer still holds them; they go once
/// they expire and age past the cutoff.
pub fn delete_older_than(conn: &mut Connection, cutoff: i64) -> rusqlite::Result<Pruned> {
    let tx = conn.transaction()?;
    let revisions = tx.execute(
        "DELETE FROM item_revision WHERE item_id IN (SELECT id FROM item WHERE fetched_at < ?1)",
        params![cutoff],
    )?;
    let items = tx.execute("DELETE FROM item WHERE fetched_at < ?1", params![cutoff])?;
    let clusters = tx.execute(
        "DELETE FROM cluster WHERE expired = 1 AND last_active_at < ?1",
        params![cutoff],
    )?;
    let events = tx.execute("DELETE FROM cluster_event WHERE ts < ?1", params![cutoff])?;
    tx.commit()?;
    Ok(Pruned {
        items,
        revisions,
        clusters,
        events,
        ..Default::default()
    })
}

pub fn clear_embeddings_before(conn: &Connection, cutoff: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE item SET embedding = NULL WHERE embedding IS NOT NULL AND fetched_at < ?1",
        params![cutoff],
    )
}

pub fn run(
    cfg: &Config,
    older_than: Option<&str>,
    embeddings: bool,
    now: i64,
) -> Result<Pruned, Box<dyn std::error::Error>> {
    let mut conn = crate::store::open(&cfg.store.path)?;
    let mut p = Pruned::default();
    if let Some(d) = older_than {
        let secs = parse_duration(d)?;
        p = delete_older_than(&mut conn, now - secs)?;
    }
    if embeddings {
        p.embeddings_cleared = clear_embeddings_before(&conn, now - cfg.cluster.window_secs)?;
    }
    if cfg.snapshots.enabled && Path::new(&cfg.snapshots.dir).exists() {
        p.snapshot_files =
            crate::snapshot::prune(&cfg.snapshots.dir, now, cfg.snapshots.retention_days)?;
        let (f, b) = crate::snapshot::enforce_budget(&cfg.snapshots.dir, cfg.snapshots.max_bytes)?;
        p.snapshot_budget_files = f;
        p.snapshot_budget_bytes = b;
    }
    // Reclaim the space; cheap on a small db, worth it after a big prune.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::reader;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90d").unwrap(), 90 * 86_400);
        assert_eq!(parse_duration("36h").unwrap(), 36 * 3600);
        assert_eq!(parse_duration("15m").unwrap(), 900);
        assert_eq!(parse_duration("2w").unwrap(), 14 * 86_400);
        assert_eq!(parse_duration("42").unwrap(), 42);
        assert!(parse_duration("3y").is_err());
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn prunes_old_rows_and_clears_embeddings() {
        let dir = std::env::temp_dir().join(format!("kg-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (r, path) = crate::export::fixture::populate(&dir);
        // Fixture: items fetched at 101, 201, 301; cluster 1 active (not expired); events at 101, 201, 301.
        let mut conn = crate::store::open(&path).unwrap();
        let p = delete_older_than(&mut conn, 250).unwrap();
        assert_eq!(p.items, 2);
        assert_eq!(p.events, 2);
        assert_eq!(
            p.clusters, 0,
            "active cluster is kept even though it is old"
        );
        assert_eq!(r.with(|c| reader::stats(c, 1000)).unwrap().items_total, 1);

        // Expire the cluster, then it goes.
        conn.execute("UPDATE cluster SET expired = 1", []).unwrap();
        assert_eq!(delete_older_than(&mut conn, 250).unwrap().clusters, 1);

        let (r2, path2) = crate::export::fixture::populate(&dir.join("b"));
        let conn2 = crate::store::open(&path2).unwrap();
        assert_eq!(
            clear_embeddings_before(&conn2, 150).unwrap(),
            1,
            "only item 1 (fetched 101) is before the cutoff"
        );
        assert_eq!(r2.with(|c| reader::get_embedding(c, 1)).unwrap(), None);
        assert!(r2.with(|c| reader::get_embedding(c, 2)).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
