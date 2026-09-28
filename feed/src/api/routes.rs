use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::stats::now;
use crate::store::reader;

pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 500;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/items", get(list_items))
        .route("/v1/items/{id}", get(get_item))
        .route("/v1/items/{id}/revisions", get(item_revisions))
        .route("/v1/syndication", get(syndication))
        .route("/v1/snapshots", get(list_snapshots))
        .route("/v1/snapshots/{id}", get(get_snapshot))
        .route("/v1/snapshots/{id}/body", get(snapshot_body))
        .route("/v1/clusters", get(list_clusters))
        .route("/v1/clusters/{id}", get(get_cluster))
        .route("/v1/clusters/{id}/timeline", get(cluster_timeline))
        .route("/v1/events", get(events))
        .route("/v1/feeds", get(feeds))
        .route("/v1/outlets", get(outlets))
        .route("/v1/stats", get(stats))
        .route("/debug", get(debug_page))
}

#[derive(Serialize)]
struct RevisionsOut {
    item_id: u64,
    /// Current text, as `/v1/items/{id}` returns it.
    current: crate::types::Item,
    /// Oldest first; the first entry is the original text once any change
    /// has been recorded. Empty means the text has never changed.
    revisions: Vec<reader::RevisionRow>,
}

async fn item_revisions(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let current = match s.reader.call(move |c| reader::get_item(c, id)).await {
        Ok(Some(i)) => i,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such item"),
        Err(e) => return db_err(e),
    };
    match s.reader.call(move |c| reader::item_revisions(c, id)).await {
        Ok(revisions) => Json(RevisionsOut {
            item_id: id,
            current,
            revisions,
        })
        .into_response(),
        Err(e) => db_err(e),
    }
}

#[derive(Deserialize)]
struct SyndicationQ {
    text_hash: String,
    since: Option<i64>,
    limit: Option<usize>,
}

/// Default window for the syndication scan: 30 days.
const SYNDICATION_WINDOW_SECS: i64 = 30 * 86_400;

#[derive(Serialize)]
struct SyndicationOut {
    text_hash: u64,
    hamming_threshold: u32,
    /// In `published_at` order, so the first entry is the earliest copy.
    items: Vec<crate::types::Item>,
}

async fn syndication(State(s): State<AppState>, Query(q): Query<SyndicationQ>) -> Response {
    let hash = match parse_id(&q.text_hash) {
        Ok(h) => h,
        Err(r) => return r,
    };
    let since = q.since.unwrap_or_else(|| now() - SYNDICATION_WINDOW_SECS);
    let limit = clamp_limit(q.limit);
    let threshold = s.hamming_threshold;
    match s
        .reader
        .call(move |c| reader::syndication_matches(c, hash, threshold, since, limit))
        .await
    {
        Ok(items) => Json(SyndicationOut {
            text_hash: hash,
            hamming_threshold: threshold,
            items,
        })
        .into_response(),
        Err(e) => db_err(e),
    }
}

async fn debug_page() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        super::debug::PAGE,
    )
        .into_response()
}

#[derive(Deserialize)]
struct ListClustersQ {
    since: Option<i64>,
    limit: Option<usize>,
    min_outlets: Option<u32>,
    sort: Option<String>,
    cursor: Option<String>,
    /// `items` to inline each cluster's items (used by the debug page).
    include: Option<String>,
}

/// Cluster plus, optionally, its items. The contract `Cluster` carries the
/// timeline; `items` is an addition, never a removal.
#[derive(Serialize)]
struct ClusterOut {
    #[serde(flatten)]
    cluster: crate::types::Cluster,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<crate::types::Item>>,
}

async fn assemble(
    s: &AppState,
    row: reader::ClusterRow,
    with_items: bool,
    now: i64,
) -> Result<ClusterOut, Response> {
    let id = row.id;
    let items = s
        .reader
        .call(move |c| reader::cluster_items(c, id))
        .await
        .map_err(db_err)?;
    let cluster = crate::cluster::propagation::assemble(&row, &items, now, s.hamming_threshold);
    Ok(ClusterOut {
        cluster,
        items: with_items.then_some(items),
    })
}

async fn list_clusters(State(s): State<AppState>, Query(q): Query<ListClustersQ>) -> Response {
    let cursor = match parse_cursor(q.cursor.as_deref()) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let sort = match q.sort.as_deref().unwrap_or("recent") {
        "recent" => reader::ClusterSort::Recent,
        "velocity_1h" => reader::ClusterSort::Velocity1h,
        "outlets_6h" => reader::ClusterSort::Outlets6h,
        _ => {
            return err(
                StatusCode::BAD_REQUEST,
                "sort must be recent, velocity_1h or outlets_6h",
            )
        }
    };
    let limit = clamp_limit(q.limit);
    let since = q.since.unwrap_or(0);
    let min_outlets = q.min_outlets.unwrap_or(1);
    let t = now();
    let rows = match s
        .reader
        .call(move |c| reader::list_clusters(c, since, min_outlets, sort, cursor, limit, t))
        .await
    {
        Ok(r) => r,
        Err(e) => return db_err(e),
    };
    let next = (sort == reader::ClusterSort::Recent && rows.len() == limit)
        .then(|| {
            rows.last()
                .map(|r| format!("{}:{}", r.last_active_at, r.id))
        })
        .flatten();
    let with_items = q.include.as_deref() == Some("items");
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        match assemble(&s, row, with_items, t).await {
            Ok(c) => out.push(c),
            Err(r) => return r,
        }
    }
    Json(Page { items: out, next }).into_response()
}

async fn get_cluster(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let row = match s.reader.call(move |c| reader::get_cluster(c, id)).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such cluster"),
        Err(e) => return db_err(e),
    };
    match assemble(&s, row, true, now()).await {
        Ok(c) => Json(c).into_response(),
        Err(r) => r,
    }
}

async fn cluster_timeline(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let exists = match s.reader.call(move |c| reader::get_cluster(c, id)).await {
        Ok(r) => r.is_some(),
        Err(e) => return db_err(e),
    };
    if !exists {
        return err(StatusCode::NOT_FOUND, "no such cluster");
    }
    match s.reader.call(move |c| reader::cluster_items(c, id)).await {
        Ok(items) => Json(crate::cluster::propagation::timeline(&items)).into_response(),
        Err(e) => db_err(e),
    }
}

#[derive(Deserialize)]
struct EventsQ {
    after_seq: Option<i64>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct EventsPage {
    items: Vec<reader::EventRow>,
    /// Pass back as `after_seq` for the next page.
    last_seq: Option<i64>,
}

async fn events(State(s): State<AppState>, Query(q): Query<EventsQ>) -> Response {
    let after = q.after_seq.unwrap_or(0);
    let limit = clamp_limit(q.limit);
    match s
        .reader
        .call(move |c| reader::events_after(c, after, limit))
        .await
    {
        Ok(items) => {
            let last_seq = items.last().map(|e| e.seq);
            Json(EventsPage { items, last_seq }).into_response()
        }
        Err(e) => db_err(e),
    }
}

#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    next: Option<String>,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ApiError { error: msg.into() })).into_response()
}

fn db_err(e: rusqlite::Error) -> Response {
    tracing::error!("query failed: {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "store error")
}

/// Cursor is `<fetched_at>:<id>` of the last row on the previous page.
#[allow(clippy::result_large_err)] // the Err is the HTTP response itself
fn parse_cursor(c: Option<&str>) -> Result<Option<(i64, u64)>, Response> {
    match c {
        None | Some("") => Ok(None),
        Some(s) => {
            let (a, b) = s
                .split_once(':')
                .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad cursor"))?;
            let f = a
                .parse()
                .map_err(|_| err(StatusCode::BAD_REQUEST, "bad cursor"))?;
            let i = b
                .parse()
                .map_err(|_| err(StatusCode::BAD_REQUEST, "bad cursor"))?;
            Ok(Some((f, i)))
        }
    }
}

fn clamp_limit(l: Option<usize>) -> usize {
    l.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[allow(clippy::result_large_err)]
fn parse_id(s: &str) -> Result<u64, Response> {
    s.parse::<u64>()
        .or_else(|_| u64::from_str_radix(s.trim_start_matches("0x"), 16))
        .map_err(|_| err(StatusCode::BAD_REQUEST, "bad id"))
}

#[derive(Deserialize)]
struct ListItemsQ {
    since: Option<i64>,
    outlet: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

async fn list_items(State(s): State<AppState>, Query(q): Query<ListItemsQ>) -> Response {
    let cursor = match parse_cursor(q.cursor.as_deref()) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let limit = clamp_limit(q.limit);
    let since = q.since.unwrap_or(0);
    let outlet = q.outlet.clone();
    let rows = match s
        .reader
        .call(move |c| reader::list_items(c, since, outlet.as_deref(), cursor, limit))
        .await
    {
        Ok(r) => r,
        Err(e) => return db_err(e),
    };
    let next = (rows.len() == limit)
        .then(|| rows.last().map(|i| format!("{}:{}", i.fetched_at, i.id)))
        .flatten();
    Json(Page { items: rows, next }).into_response()
}

async fn get_item(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    match s.reader.call(move |c| reader::get_item(c, id)).await {
        Ok(Some(i)) => Json(i).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such item"),
        Err(e) => db_err(e),
    }
}

#[derive(Deserialize)]
struct ListSnapshotsQ {
    feed: Option<String>,
    since: Option<i64>,
    limit: Option<usize>,
    cursor: Option<String>,
}

async fn list_snapshots(State(s): State<AppState>, Query(q): Query<ListSnapshotsQ>) -> Response {
    let cursor = match parse_cursor(q.cursor.as_deref()) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let limit = clamp_limit(q.limit);
    let since = q.since.unwrap_or(0);
    let feed = q.feed.clone();
    let rows = match s
        .reader
        .call(move |c| reader::list_snapshots(c, feed.as_deref(), since, cursor, limit))
        .await
    {
        Ok(r) => r,
        Err(e) => return db_err(e),
    };
    let next = (rows.len() == limit)
        .then(|| rows.last().map(|i| format!("{}:{}", i.fetched_at, i.id)))
        .flatten();
    Json(Page { items: rows, next }).into_response()
}

async fn get_snapshot(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    match s.reader.call(move |c| reader::get_snapshot(c, id)).await {
        Ok(Some(m)) => Json(m).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such snapshot"),
        Err(e) => db_err(e),
    }
}

async fn snapshot_body(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let meta = match s.reader.call(move |c| reader::get_snapshot(c, id)).await {
        Ok(Some(m)) => m,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such snapshot"),
        Err(e) => return db_err(e),
    };
    if meta.path.is_empty() {
        return err(StatusCode::NOT_FOUND, "snapshot body was not archived");
    }
    let dir = s.snapshot_dir.clone();
    let rel = std::path::PathBuf::from(meta.path);
    match tokio::task::spawn_blocking(move || crate::snapshot::read(&dir, &rel)).await {
        Ok(Ok(body)) => (
            [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
            body,
        )
            .into_response(),
        Ok(Err(e)) => {
            tracing::warn!("snapshot read failed: {e}");
            err(StatusCode::NOT_FOUND, "snapshot body missing on disk")
        }
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "read task failed"),
    }
}

#[derive(Deserialize)]
struct FeedsQ {
    /// `failing`, `dead`, `stale` (no success in 24 h), or `all` (default).
    filter: Option<String>,
}

#[derive(Serialize)]
struct FeedOut {
    #[serde(flatten)]
    state: crate::store::writer::FeedState,
    /// Seconds since the last successful fetch, if any.
    ok_age_secs: Option<i64>,
}

/// Per-feed health, newest failures first.
async fn feeds(State(s): State<AppState>, Query(q): Query<FeedsQ>) -> Response {
    let t = now();
    let rows = match s.reader.call(reader::feed_states).await {
        Ok(r) => r,
        Err(e) => return db_err(e),
    };
    let filter = q.filter.unwrap_or_else(|| "all".into());
    let mut out: Vec<FeedOut> = rows
        .into_iter()
        .filter(|f| match filter.as_str() {
            "failing" => f.fail_count > 0 && !f.dead,
            "dead" => f.dead,
            "stale" => f.last_ok_at.map(|o| t - o > 86_400).unwrap_or(true),
            _ => true,
        })
        .map(|f| FeedOut {
            ok_age_secs: f.last_ok_at.map(|o| t - o),
            state: f,
        })
        .collect();
    out.sort_by(|a, b| {
        (b.state.dead, b.state.fail_count, a.state.url.clone()).cmp(&(
            a.state.dead,
            a.state.fail_count,
            b.state.url.clone(),
        ))
    });
    Json(out).into_response()
}

async fn outlets(State(s): State<AppState>) -> Response {
    match s.reader.call(reader::outlets).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => db_err(e),
    }
}

#[derive(Serialize)]
struct StatsOut {
    contract_version: u32,
    process: crate::stats::StatsSnapshot,
    store: reader::StoreStats,
}

async fn stats(State(s): State<AppState>) -> Response {
    let t = now();
    match s.reader.call(move |c| reader::stats(c, t)).await {
        Ok(store) => Json(StatsOut {
            contract_version: crate::types::CONTRACT_VERSION,
            process: s.stats.snapshot(t),
            store,
        })
        .into_response(),
        Err(e) => db_err(e),
    }
}
