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
        .route("/v1/snapshots", get(list_snapshots))
        .route("/v1/snapshots/{id}", get(get_snapshot))
        .route("/v1/snapshots/{id}/body", get(snapshot_body))
        .route("/v1/outlets", get(outlets))
        .route("/v1/stats", get(stats))
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
