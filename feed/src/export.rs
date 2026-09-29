//! Bulk export (plan §5.6): the interface `ledger` and `lens` consume in
//! bulk. NDJSON is always available; Parquet is behind the `parquet`
//! feature because `arrow` is the one heavy dependency the plan allows.
//!
//! Files written into `out_dir`: `items`, `clusters`, `events`, `outlets`,
//! each with the format's extension. Clusters are the full contract
//! object (timeline, syndication, velocity computed at export time).
//! NDJSON omits embeddings and centroids; Parquet includes them.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cluster::propagation;
use crate::store::reader::{self, Reader};
use crate::types::Cluster;

#[cfg(feature = "parquet")]
pub mod parquet;

/// Page size for streaming reads.
const PAGE: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Ndjson,
    Parquet,
}

impl std::str::FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "ndjson" | "jsonl" => Ok(Format::Ndjson),
            "parquet" => Ok(Format::Parquet),
            other => Err(format!("unknown format {other:?}; use ndjson or parquet")),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Summary {
    pub items: usize,
    pub clusters: usize,
    pub events: usize,
    pub outlets: usize,
    pub revisions: usize,
    pub files: Vec<PathBuf>,
}

/// A cluster row plus its derived contract object, as exported.
#[derive(Serialize)]
pub struct ClusterOut {
    #[serde(flatten)]
    pub cluster: Cluster,
    pub expired: bool,
}

/// Walk every item in `fetched_at` descending order, `PAGE` at a time.
pub fn for_each_item(
    reader: &Reader,
    mut f: impl FnMut(crate::types::Item) -> std::io::Result<()>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut cursor = None;
    let mut n = 0;
    loop {
        let page = reader.with(|c| reader::list_items(c, 0, None, cursor, PAGE))?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().map(|i| (i.fetched_at, i.id));
        let len = page.len();
        for it in page {
            f(it)?;
        }
        n += len;
        if len < PAGE {
            break;
        }
    }
    Ok(n)
}

/// Every cluster as a contract object, oldest id first.
pub fn for_each_cluster(
    reader: &Reader,
    now: i64,
    th: propagation::Thresholds,
    mut f: impl FnMut(ClusterOut) -> std::io::Result<()>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let rows = reader.with(reader::all_clusters)?;
    let n = rows.len();
    for row in rows {
        let items = reader.with(|c| reader::cluster_items(c, row.id))?;
        let cluster = propagation::assemble(&row, &items, now, th);
        f(ClusterOut {
            cluster,
            expired: row.expired,
        })?;
    }
    Ok(n)
}

pub fn for_each_event(
    reader: &Reader,
    mut f: impl FnMut(reader::EventRow) -> std::io::Result<()>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut after = 0;
    let mut n = 0;
    loop {
        let page = reader.with(|c| reader::events_after(c, after, PAGE))?;
        if page.is_empty() {
            break;
        }
        after = page.last().map(|e| e.seq).unwrap_or(after);
        let len = page.len();
        for e in page {
            f(e)?;
        }
        n += len;
        if len < PAGE {
            break;
        }
    }
    Ok(n)
}

pub fn for_each_revision(
    reader: &Reader,
    mut f: impl FnMut(reader::RevisionExportRow) -> std::io::Result<()>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut after = 0;
    let mut n = 0;
    loop {
        let page = reader.with(|c| reader::revisions_after(c, after, PAGE))?;
        if page.is_empty() {
            break;
        }
        after = page.last().map(|r| r.rowid).unwrap_or(after);
        let len = page.len();
        for r in page {
            f(r)?;
        }
        n += len;
        if len < PAGE {
            break;
        }
    }
    Ok(n)
}

fn ndjson_file(dir: &Path, name: &str) -> std::io::Result<(PathBuf, BufWriter<std::fs::File>)> {
    let path = dir.join(format!("{name}.ndjson"));
    Ok((path.clone(), BufWriter::new(std::fs::File::create(path)?)))
}

fn line<W: Write, T: Serialize>(w: &mut W, v: &T) -> std::io::Result<()> {
    serde_json::to_writer(&mut *w, v)?;
    w.write_all(b"\n")
}

pub fn write_ndjson(
    reader: &Reader,
    out_dir: &Path,
    now: i64,
    th: propagation::Thresholds,
) -> Result<Summary, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out_dir)?;
    let mut s = Summary::default();

    let (p, mut w) = ndjson_file(out_dir, "items")?;
    s.items = for_each_item(reader, |it| line(&mut w, &it))?;
    w.flush()?;
    s.files.push(p);

    let (p, mut w) = ndjson_file(out_dir, "clusters")?;
    s.clusters = for_each_cluster(reader, now, th, |c| line(&mut w, &c))?;
    w.flush()?;
    s.files.push(p);

    let (p, mut w) = ndjson_file(out_dir, "events")?;
    s.events = for_each_event(reader, |e| line(&mut w, &e))?;
    w.flush()?;
    s.files.push(p);

    let (p, mut w) = ndjson_file(out_dir, "revisions")?;
    s.revisions = for_each_revision(reader, |r| line(&mut w, &r))?;
    w.flush()?;
    s.files.push(p);

    let (p, mut w) = ndjson_file(out_dir, "outlets")?;
    let outlets = reader.with(reader::outlets)?;
    for o in &outlets {
        line(&mut w, o)?;
    }
    s.outlets = outlets.len();
    w.flush()?;
    s.files.push(p);
    Ok(s)
}

pub fn run(
    reader: &Reader,
    format: Format,
    out_dir: &Path,
    now: i64,
    th: propagation::Thresholds,
) -> Result<Summary, Box<dyn std::error::Error>> {
    match format {
        Format::Ndjson => write_ndjson(reader, out_dir, now, th),
        #[cfg(feature = "parquet")]
        Format::Parquet => parquet::write(reader, out_dir, now, th),
        #[cfg(not(feature = "parquet"))]
        Format::Parquet => {
            Err("built without the `parquet` feature; rebuild with --features parquet".into())
        }
    }
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A small populated store shared by the export tests.
    use std::path::PathBuf;

    use crate::store::reader::{ClusterRow, Reader};
    use crate::store::writer::{commit, WriteMsg};
    use crate::types::{ClusterEvent, EventKind, Item};

    pub fn item(id: u64, outlet: &str, t: i64, hash: u64) -> Item {
        Item {
            id,
            outlet_id: outlet.into(),
            canonical_url: format!("https://{outlet}.example/{id}"),
            title: format!("title {id}"),
            summary: "s".into(),
            published_at: t,
            fetched_at: t + 1,
            lang: "en".into(),
            text_hash: hash,
            cluster_id: None,
            cluster_rank: None,
            joined_at: None,
            join_similarity: None,
            snapshot_id: 1,
        }
    }

    /// Three items, two in one cluster, embeddings on two, three events.
    pub fn populate(dir: &std::path::Path) -> (Reader, PathBuf) {
        let path = dir.join("export.db");
        let mut c = crate::store::open(&path).unwrap();
        let outlets = crate::config::Outlets {
            outlets: vec![crate::config::OutletEntry {
                id: "a".into(),
                name: "A".into(),
                domains: vec!["a.example".into()],
            }],
        };
        crate::store::writer::sync_outlets(&mut c, &outlets).unwrap();
        commit(
            &mut c,
            vec![
                WriteMsg::Items(vec![
                    item(1, "a", 100, 7),
                    item(2, "b", 200, 7),
                    item(3, "c", 300, 99),
                ]),
                WriteMsg::Embeddings(vec![(1, vec![1.0, 0.0]), (2, vec![0.0, 1.0])]),
                WriteMsg::ClusterUpsert(ClusterRow {
                    id: 1,
                    created_at: 101,
                    last_active_at: 201,
                    item_count: 2,
                    outlet_count: 2,
                    primary_title: "title 1".into(),
                    centroid: vec![0.7, 0.7],
                    expired: false,
                    merged: false,
                }),
                WriteMsg::ItemAssigned {
                    item_id: 1,
                    cluster_id: 1,
                    rank: 0,
                    joined_at: 101,
                    similarity: None,
                },
                WriteMsg::ItemAssigned {
                    item_id: 2,
                    cluster_id: 1,
                    rank: 1,
                    joined_at: 201,
                    similarity: None,
                },
                WriteMsg::Event(ClusterEvent {
                    ts: 101,
                    kind: EventKind::Created,
                    cluster_id: Some(1),
                    item_id: Some(1),
                    merged_into: None,
                    snapshot_id: None,
                }),
                WriteMsg::Event(ClusterEvent {
                    ts: 201,
                    kind: EventKind::ItemAdded,
                    cluster_id: Some(1),
                    item_id: Some(2),
                    merged_into: None,
                    snapshot_id: None,
                }),
                WriteMsg::Revision {
                    item_id: 3,
                    snapshot_id: 8,
                    seen_at: 300,
                    title: "title 3, as first seen".into(),
                    summary: String::new(),
                    apply: false,
                },
                WriteMsg::Event(ClusterEvent {
                    ts: 301,
                    kind: EventKind::Revised,
                    cluster_id: None,
                    item_id: Some(3),
                    merged_into: None,
                    snapshot_id: Some(9),
                }),
            ],
        )
        .unwrap();
        drop(c);
        (Reader::open(&path).unwrap(), path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndjson_export_writes_all_five_files() {
        let dir = std::env::temp_dir().join(format!("kg-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (reader, _) = fixture::populate(&dir);
        let out = dir.join("out");
        let s = write_ndjson(
            &reader,
            &out,
            1000,
            propagation::Thresholds {
                hamming: 3,
                join: 0.72,
            },
        )
        .unwrap();
        assert_eq!((s.items, s.clusters, s.events, s.outlets), (3, 1, 3, 1));
        assert_eq!(s.revisions, 1);
        assert_eq!(s.files.len(), 5);
        let revs = std::fs::read_to_string(out.join("revisions.ndjson")).unwrap();
        let r: serde_json::Value = serde_json::from_str(revs.lines().next().unwrap()).unwrap();
        assert_eq!(r["item_id"], 3);
        assert_eq!(r["title"], "title 3, as first seen");
        assert!(r.get("rowid").is_none(), "cursor is not contract");

        let items = std::fs::read_to_string(out.join("items.ndjson")).unwrap();
        let lines: Vec<serde_json::Value> = items
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["id"], 3, "newest first");
        assert!(
            lines[0].get("embedding").is_none(),
            "NDJSON omits embeddings"
        );
        assert_eq!(lines[2]["cluster_id"], 1);

        let clusters = std::fs::read_to_string(out.join("clusters.ndjson")).unwrap();
        let c: serde_json::Value = serde_json::from_str(clusters.lines().next().unwrap()).unwrap();
        assert_eq!(c["id"], 1);
        assert_eq!(c["item_count"], 2);
        assert_eq!(c["origin_item_id"], 1);
        assert_eq!(c["timeline"].as_array().unwrap().len(), 2);
        assert_eq!(
            c["syndication"][0]["item_ids"],
            serde_json::json!([1, 2]),
            "same simhash → one group"
        );
        assert_eq!(c["expired"], false);
        assert!(c.get("centroid").is_none());

        let events = std::fs::read_to_string(out.join("events.ndjson")).unwrap();
        let e: Vec<serde_json::Value> = events
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0]["seq"], 1);
        assert_eq!(e[2]["kind"], "revised");

        assert_eq!(
            std::fs::read_to_string(out.join("outlets.ndjson"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!("parquet".parse::<Format>().is_ok() && "csv".parse::<Format>().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
