//! Parquet export, enabled with `--features parquet`. Same four tables as
//! NDJSON, plus what NDJSON omits: item embeddings and cluster centroids
//! as `list<float32>`. Nested cluster fields (timeline, syndication) are
//! carried as JSON strings so the schema stays flat and readable from any
//! Parquet consumer; velocity is flattened into columns.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanBuilder, Float32Builder, Int64Builder, ListBuilder, StringBuilder,
    UInt32Builder, UInt64Builder,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use super::{for_each_cluster, for_each_event, for_each_revision, Summary};
use crate::store::reader::{self, Reader};

const PAGE: usize = 1000;

fn f32_list() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::Float32, true)))
}

fn utf8_list() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::Utf8, true)))
}

fn writer(
    path: &Path,
    schema: Arc<Schema>,
) -> Result<ArrowWriter<std::fs::File>, Box<dyn std::error::Error>> {
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    Ok(ArrowWriter::try_new(
        std::fs::File::create(path)?,
        schema,
        Some(props),
    )?)
}

fn push_f32s(b: &mut ListBuilder<Float32Builder>, v: Option<&[f32]>) {
    match v {
        Some(v) => {
            for x in v {
                b.values().append_value(*x);
            }
            b.append(true);
        }
        None => b.append(false),
    }
}

fn write_items(reader: &Reader, path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("outlet_id", DataType::Utf8, false),
        Field::new("canonical_url", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("summary", DataType::Utf8, false),
        Field::new("published_at", DataType::Int64, false),
        Field::new("fetched_at", DataType::Int64, false),
        Field::new("lang", DataType::Utf8, false),
        Field::new("text_hash", DataType::UInt64, false),
        Field::new("cluster_id", DataType::UInt64, true),
        Field::new("cluster_rank", DataType::UInt32, true),
        Field::new("joined_at", DataType::Int64, true),
        Field::new("snapshot_id", DataType::UInt64, false),
        Field::new("embedding", f32_list(), true),
    ]));
    let mut w = writer(path, schema.clone())?;
    let mut cursor = None;
    let mut n = 0;
    loop {
        let page = reader.with(|c| reader::list_items_with_embeddings(c, cursor, PAGE))?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().map(|(i, _)| (i.fetched_at, i.id));
        let len = page.len();
        let mut id = UInt64Builder::new();
        let mut outlet = StringBuilder::new();
        let mut url = StringBuilder::new();
        let mut title = StringBuilder::new();
        let mut summary = StringBuilder::new();
        let mut published = Int64Builder::new();
        let mut fetched = Int64Builder::new();
        let mut lang = StringBuilder::new();
        let mut hash = UInt64Builder::new();
        let mut cluster = UInt64Builder::new();
        let mut rank = UInt32Builder::new();
        let mut joined = Int64Builder::new();
        let mut snap = UInt64Builder::new();
        let mut emb = ListBuilder::new(Float32Builder::new());
        for (it, e) in &page {
            id.append_value(it.id);
            outlet.append_value(&it.outlet_id);
            url.append_value(&it.canonical_url);
            title.append_value(&it.title);
            summary.append_value(&it.summary);
            published.append_value(it.published_at);
            fetched.append_value(it.fetched_at);
            lang.append_value(&it.lang);
            hash.append_value(it.text_hash);
            cluster.append_option(it.cluster_id);
            rank.append_option(it.cluster_rank);
            joined.append_option(it.joined_at);
            snap.append_value(it.snapshot_id);
            push_f32s(&mut emb, e.as_deref());
        }
        let cols: Vec<ArrayRef> = vec![
            Arc::new(id.finish()),
            Arc::new(outlet.finish()),
            Arc::new(url.finish()),
            Arc::new(title.finish()),
            Arc::new(summary.finish()),
            Arc::new(published.finish()),
            Arc::new(fetched.finish()),
            Arc::new(lang.finish()),
            Arc::new(hash.finish()),
            Arc::new(cluster.finish()),
            Arc::new(rank.finish()),
            Arc::new(joined.finish()),
            Arc::new(snap.finish()),
            Arc::new(emb.finish()),
        ];
        w.write(&RecordBatch::try_new(schema.clone(), cols)?)?;
        n += len;
        if len < PAGE {
            break;
        }
    }
    w.close()?;
    Ok(n)
}

fn write_clusters(
    reader: &Reader,
    path: &Path,
    now: i64,
    th: crate::cluster::propagation::Thresholds,
) -> Result<usize, Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("created_at", DataType::Int64, false),
        Field::new("last_active_at", DataType::Int64, false),
        Field::new("item_count", DataType::UInt32, false),
        Field::new("outlet_count", DataType::UInt32, false),
        Field::new("primary_title", DataType::Utf8, false),
        Field::new("origin_item_id", DataType::UInt64, false),
        Field::new("origin_outlet_id", DataType::Utf8, false),
        Field::new("expired", DataType::Boolean, false),
        Field::new("items_1h", DataType::UInt32, false),
        Field::new("items_6h", DataType::UInt32, false),
        Field::new("items_24h", DataType::UInt32, false),
        Field::new("outlets_1h", DataType::UInt32, false),
        Field::new("outlets_6h", DataType::UInt32, false),
        Field::new("outlets_24h", DataType::UInt32, false),
        Field::new("time_to_5_outlets_secs", DataType::UInt32, true),
        Field::new("time_to_20_outlets_secs", DataType::UInt32, true),
        Field::new("timeline_json", DataType::Utf8, false),
        Field::new("syndication_json", DataType::Utf8, false),
        Field::new("centroid", f32_list(), true),
    ]));
    let mut w = writer(path, schema.clone())?;
    // Centroids come from the rows; the derived object doesn't carry them.
    let rows = reader.with(reader::all_clusters)?;
    let centroids: std::collections::HashMap<u64, Vec<f32>> =
        rows.into_iter().map(|r| (r.id, r.centroid)).collect();

    let mut id = UInt64Builder::new();
    let mut created = Int64Builder::new();
    let mut active = Int64Builder::new();
    let mut items = UInt32Builder::new();
    let mut outlets = UInt32Builder::new();
    let mut title = StringBuilder::new();
    let mut origin_item = UInt64Builder::new();
    let mut origin_outlet = StringBuilder::new();
    let mut expired = BooleanBuilder::new();
    let mut v = [
        UInt32Builder::new(),
        UInt32Builder::new(),
        UInt32Builder::new(),
        UInt32Builder::new(),
        UInt32Builder::new(),
        UInt32Builder::new(),
    ];
    let mut t5 = UInt32Builder::new();
    let mut t20 = UInt32Builder::new();
    let mut timeline = StringBuilder::new();
    let mut syndication = StringBuilder::new();
    let mut centroid = ListBuilder::new(Float32Builder::new());

    let n = for_each_cluster(reader, now, th, |c| {
        let k = &c.cluster;
        id.append_value(k.id);
        created.append_value(k.created_at);
        active.append_value(k.last_active_at);
        items.append_value(k.item_count);
        outlets.append_value(k.outlet_count);
        title.append_value(&k.primary_title);
        origin_item.append_value(k.origin_item_id);
        origin_outlet.append_value(&k.origin_outlet_id);
        expired.append_value(c.expired);
        let vel = &k.velocity;
        for (b, x) in v.iter_mut().zip([
            vel.items_1h,
            vel.items_6h,
            vel.items_24h,
            vel.outlets_1h,
            vel.outlets_6h,
            vel.outlets_24h,
        ]) {
            b.append_value(x);
        }
        t5.append_option(vel.time_to_5_outlets_secs);
        t20.append_option(vel.time_to_20_outlets_secs);
        timeline.append_value(serde_json::to_string(&k.timeline).unwrap_or_default());
        syndication.append_value(serde_json::to_string(&k.syndication).unwrap_or_default());
        push_f32s(&mut centroid, centroids.get(&k.id).map(Vec::as_slice));
        Ok(())
    })?;

    let [v1, v6, v24, o1, o6, o24] = v;
    let cols: Vec<ArrayRef> = vec![
        Arc::new(id.finish()),
        Arc::new(created.finish()),
        Arc::new(active.finish()),
        Arc::new(items.finish()),
        Arc::new(outlets.finish()),
        Arc::new(title.finish()),
        Arc::new(origin_item.finish()),
        Arc::new(origin_outlet.finish()),
        Arc::new(expired.finish()),
        Arc::new({
            let mut b = v1;
            b.finish()
        }),
        Arc::new({
            let mut b = v6;
            b.finish()
        }),
        Arc::new({
            let mut b = v24;
            b.finish()
        }),
        Arc::new({
            let mut b = o1;
            b.finish()
        }),
        Arc::new({
            let mut b = o6;
            b.finish()
        }),
        Arc::new({
            let mut b = o24;
            b.finish()
        }),
        Arc::new(t5.finish()),
        Arc::new(t20.finish()),
        Arc::new(timeline.finish()),
        Arc::new(syndication.finish()),
        Arc::new(centroid.finish()),
    ];
    w.write(&RecordBatch::try_new(schema, cols)?)?;
    w.close()?;
    Ok(n)
}

fn write_events(reader: &Reader, path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("seq", DataType::Int64, false),
        Field::new("ts", DataType::Int64, false),
        Field::new("kind", DataType::Utf8, false),
        Field::new("cluster_id", DataType::UInt64, true),
        Field::new("item_id", DataType::UInt64, true),
        Field::new("merged_into", DataType::UInt64, true),
        Field::new("snapshot_id", DataType::UInt64, true),
    ]));
    let mut w = writer(path, schema.clone())?;
    let mut seq = Int64Builder::new();
    let mut ts = Int64Builder::new();
    let mut kind = StringBuilder::new();
    let mut cluster = UInt64Builder::new();
    let mut item = UInt64Builder::new();
    let mut merged = UInt64Builder::new();
    let mut snap = UInt64Builder::new();
    let n = for_each_event(reader, |e| {
        seq.append_value(e.seq);
        ts.append_value(e.event.ts);
        let k = serde_json::to_value(e.event.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        kind.append_value(k);
        cluster.append_option(e.event.cluster_id);
        item.append_option(e.event.item_id);
        merged.append_option(e.event.merged_into);
        snap.append_option(e.event.snapshot_id);
        Ok(())
    })?;
    let cols: Vec<ArrayRef> = vec![
        Arc::new(seq.finish()),
        Arc::new(ts.finish()),
        Arc::new(kind.finish()),
        Arc::new(cluster.finish()),
        Arc::new(item.finish()),
        Arc::new(merged.finish()),
        Arc::new(snap.finish()),
    ];
    w.write(&RecordBatch::try_new(schema, cols)?)?;
    w.close()?;
    Ok(n)
}

/// Earlier texts of items, in per-item chronological order (plan §5.3a).
fn write_revisions(reader: &Reader, path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("item_id", DataType::UInt64, false),
        Field::new("snapshot_id", DataType::UInt64, false),
        Field::new("seen_at", DataType::Int64, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("summary", DataType::Utf8, false),
    ]));
    let mut w = writer(path, schema.clone())?;
    let mut item = UInt64Builder::new();
    let mut snap = UInt64Builder::new();
    let mut seen = Int64Builder::new();
    let mut title = StringBuilder::new();
    let mut summary = StringBuilder::new();
    let n = for_each_revision(reader, |r| {
        item.append_value(r.item_id);
        snap.append_value(r.snapshot_id);
        seen.append_value(r.seen_at);
        title.append_value(&r.title);
        summary.append_value(&r.summary);
        Ok(())
    })?;
    let cols: Vec<ArrayRef> = vec![
        Arc::new(item.finish()),
        Arc::new(snap.finish()),
        Arc::new(seen.finish()),
        Arc::new(title.finish()),
        Arc::new(summary.finish()),
    ];
    w.write(&RecordBatch::try_new(schema, cols)?)?;
    w.close()?;
    Ok(n)
}

fn write_outlets(reader: &Reader, path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("domains", utf8_list(), false),
        Field::new("feeds", utf8_list(), false),
    ]));
    let mut w = writer(path, schema.clone())?;
    let outlets = reader.with(reader::outlets)?;
    let mut id = StringBuilder::new();
    let mut name = StringBuilder::new();
    let mut domains = ListBuilder::new(StringBuilder::new());
    let mut feeds = ListBuilder::new(StringBuilder::new());
    for o in &outlets {
        id.append_value(&o.id);
        name.append_value(&o.name);
        for d in &o.domains {
            domains.values().append_value(d);
        }
        domains.append(true);
        for f in &o.feeds {
            feeds.values().append_value(f);
        }
        feeds.append(true);
    }
    let cols: Vec<ArrayRef> = vec![
        Arc::new(id.finish()),
        Arc::new(name.finish()),
        Arc::new(domains.finish()),
        Arc::new(feeds.finish()),
    ];
    w.write(&RecordBatch::try_new(schema, cols)?)?;
    w.close()?;
    Ok(outlets.len())
}

pub fn write(
    reader: &Reader,
    out_dir: &Path,
    now: i64,
    th: crate::cluster::propagation::Thresholds,
) -> Result<Summary, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out_dir)?;
    let file = |n: &str| -> PathBuf { out_dir.join(format!("{n}.parquet")) };
    Ok(Summary {
        items: write_items(reader, &file("items"))?,
        clusters: write_clusters(reader, &file("clusters"), now, th)?,
        events: write_events(reader, &file("events"))?,
        outlets: write_outlets(reader, &file("outlets"))?,
        revisions: write_revisions(reader, &file("revisions"))?,
        files: ["items", "clusters", "events", "outlets", "revisions"]
            .iter()
            .map(|n| file(n))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Array, Float32Array, ListArray, UInt64Array};
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    #[test]
    fn parquet_export_roundtrips_embeddings_and_centroids() {
        let dir = std::env::temp_dir().join(format!("kg-parquet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (reader, _) = crate::export::fixture::populate(&dir);
        let out = dir.join("out");
        let s = write(
            &reader,
            &out,
            1000,
            crate::cluster::propagation::Thresholds {
                hamming: 3,
                join: 0.72,
            },
        )
        .unwrap();
        assert_eq!((s.items, s.clusters, s.events, s.outlets), (3, 1, 3, 1));
        assert_eq!(s.revisions, 1);
        assert!(out.join("revisions.parquet").is_file());

        let f = std::fs::File::open(out.join("items.parquet")).unwrap();
        let mut r = ParquetRecordBatchReaderBuilder::try_new(f)
            .unwrap()
            .build()
            .unwrap();
        let batch = r.next().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 3);
        let ids = batch
            .column_by_name("id")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(ids.values(), &[3, 2, 1]);
        let emb = batch
            .column_by_name("embedding")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        assert!(emb.is_null(0), "item 3 has no embedding");
        let e1 = emb.value(2);
        let e1 = e1.as_any().downcast_ref::<Float32Array>().unwrap();
        assert_eq!(e1.values(), &[1.0, 0.0]);

        let f = std::fs::File::open(out.join("clusters.parquet")).unwrap();
        let mut r = ParquetRecordBatchReaderBuilder::try_new(f)
            .unwrap()
            .build()
            .unwrap();
        let batch = r.next().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 1);
        let cen = batch
            .column_by_name("centroid")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let c0 = cen.value(0);
        assert_eq!(
            c0.as_any().downcast_ref::<Float32Array>().unwrap().values(),
            &[0.7, 0.7]
        );
        assert_eq!(batch.column_by_name("items_24h").unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
