//! SQLite persistence (plan §5.4). One writer task receives batched events
//! over a channel; readers use their own connection in WAL mode.

pub mod reader;
pub mod schema;
pub mod writer;

pub use reader::Reader;
pub use writer::{spawn_writer, WriteMsg};

use std::path::Path;

use rusqlite::Connection;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=OFF;
         PRAGMA temp_store=MEMORY;
         PRAGMA busy_timeout=5000;",
    )?;
    schema::migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::migrate(&conn)?;
    Ok(conn)
}

/// Items stored as `unknown:<host>` before that host was in the outlet
/// registry are credited to the outlet `resolve` now names, so a domain
/// added to outlets.toml later applies to what was already ingested (the
/// API, the debug page and exports), not only to new items. Hosts the
/// registry still does not claim stay unknown. Returns items updated.
pub fn reattribute_unknown(
    conn: &Connection,
    resolve: impl Fn(&str) -> String,
) -> rusqlite::Result<usize> {
    let ids: Vec<String> = conn
        .prepare("SELECT DISTINCT outlet_id FROM item WHERE outlet_id LIKE 'unknown:%'")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut n = 0;
    for id in ids {
        let host = id.trim_start_matches("unknown:");
        let now = resolve(host);
        if !now.starts_with("unknown:") {
            n += conn.execute(
                "UPDATE item SET outlet_id = ?1 WHERE outlet_id = ?2",
                [&now, &id],
            )?;
        }
    }
    Ok(n)
}

/// SQLite has no u64; ids are stored as the same bits reinterpreted.
#[inline]
pub fn to_i64(v: u64) -> i64 {
    v as i64
}
#[inline]
pub fn to_u64(v: i64) -> u64 {
    v as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_items_are_credited_once_their_host_is_registered() {
        let c = open_memory().unwrap();
        for (id, outlet) in [
            (1, "unknown:presstv.co.uk"),
            (2, "unknown:partner.example"),
            (3, "bbc"),
        ] {
            c.execute(
                "INSERT INTO item (id, outlet_id, canonical_url, title, summary, published_at, fetched_at, lang, text_hash, snapshot_id) VALUES (?1, ?2, 'u', 't', '', 0, 0, 'en', 0, 0)",
                rusqlite::params![id, outlet],
            )
            .unwrap();
        }
        let n = reattribute_unknown(&c, |h| {
            if h == "presstv.co.uk" {
                "presstv".into()
            } else {
                format!("unknown:{h}")
            }
        })
        .unwrap();
        assert_eq!(n, 1);
        let got: Vec<String> = c
            .prepare("SELECT outlet_id FROM item ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(got, ["presstv", "unknown:partner.example", "bbc"]);
    }
}
