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

pub fn open_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::migrate(&conn)?;
    Ok(conn)
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
