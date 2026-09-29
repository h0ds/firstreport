use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Json, Router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::stats::now;
use crate::store::reader;

pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 500;

/// Every route, once. The router and the OpenAPI document are both
/// built from this table.
pub(crate) fn routes() -> Vec<(&'static str, MethodRouter<AppState>)> {
    vec![
        ("/v1/items", get(list_items)),
        ("/v1/items/{id}", get(get_item)),
        ("/v1/items/{id}/revisions", get(item_revisions)),
        ("/v1/syndication", get(syndication)),
        ("/v1/lookup", get(lookup)),
        ("/v1/snapshots", get(list_snapshots)),
        ("/v1/snapshots/{id}", get(get_snapshot)),
        ("/v1/snapshots/{id}/body", get(snapshot_body)),
        ("/v1/clusters", get(list_clusters)),
        ("/v1/clusters/{id}", get(get_cluster)),
        ("/v1/clusters/{id}/timeline", get(cluster_timeline)),
        ("/v1/events", get(events)),
        ("/v1/feeds", get(feeds)),
        ("/v1/outlets", get(outlets)),
        ("/v1/outlets/{id}", get(get_outlet)),
        ("/v1/stats", get(stats)),
        ("/debug", get(debug_page)),
        ("/llms.txt", get(llms_txt)),
        ("/v1/openapi.json", get(openapi)),
        ("/robots.txt", get(robots_txt)),
    ]
}

pub fn router() -> Router<AppState> {
    routes()
        .into_iter()
        .fold(Router::new(), |r, (path, handler)| r.route(path, handler))
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct RevisionsOut {
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

#[derive(Serialize, JsonSchema)]
pub(crate) struct SyndicationOut {
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

#[derive(Deserialize)]
struct LookupQ {
    url: Option<String>,
    text: Option<String>,
    /// Lower bound on `fetched_at` for the text scan; default 30 days ago.
    since: Option<i64>,
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct TextMatches {
    text_hash: u64,
    hamming_threshold: u32,
    /// Earliest published first.
    items: Vec<crate::types::Item>,
}

/// Everything `feed` knows about one URL or one piece of text, in one
/// call: the reply layer's whole backend (reply plan §3). `matched` says
/// how far the lookup got, and `checked` lists what was looked for so a
/// miss can be reported as honestly as a hit.
#[derive(Serialize, JsonSchema)]
pub(crate) struct LookupOut {
    /// `item`: the URL is an item `feed` holds. `text`: no item, but the
    /// text has near-copies in the store. `outlet`: no item, but the host
    /// belongs to an outlet `feed` follows. `none`: nothing.
    matched: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<crate::types::Item>,
    /// Whether `item` is the first published member of its cluster.
    #[serde(skip_serializing_if = "Option::is_none")]
    is_origin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cluster: Option<ClusterOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outlet: Option<OutletOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_matches: Option<TextMatches>,
    /// What was searched, in order, in words a reply can quote.
    checked: Vec<String>,
}

async fn cluster_for(
    s: &AppState,
    cluster_id: Option<u64>,
) -> Result<Option<ClusterOut>, Response> {
    let Some(cid) = cluster_id else {
        return Ok(None);
    };
    let row = s
        .reader
        .call(move |c| reader::get_cluster(c, cid))
        .await
        .map_err(db_err)?;
    match row {
        Some(row) => assemble(s, row, true, now()).await.map(Some),
        None => Ok(None),
    }
}

async fn outlet_for(s: &AppState, host: &str) -> Result<Option<OutletOut>, Response> {
    let h = host.to_string();
    let row = s
        .reader
        .call(move |c| reader::outlet_for_host(c, &h))
        .await
        .map_err(db_err)?;
    Ok(row.map(|o| outlet_out(&s.ledger_url, o)))
}

async fn lookup(State(s): State<AppState>, Query(q): Query<LookupQ>) -> Response {
    let url = q.url.as_deref().map(str::trim).filter(|u| !u.is_empty());
    let text = q.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if url.is_none() && text.is_none() {
        return err(StatusCode::BAD_REQUEST, "url or text is required");
    }
    let mut out = LookupOut {
        matched: "none",
        canonical_url: None,
        host: None,
        item: None,
        is_origin: None,
        cluster: None,
        outlet: None,
        text_matches: None,
        checked: Vec::new(),
    };

    if let Some(raw) = url {
        let Some(canon) = crate::parse::url::canonicalize(raw, None) else {
            return err(
                StatusCode::BAD_REQUEST,
                "url must be an absolute http(s) URL",
            );
        };
        let host = url::Url::parse(&canon)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string));
        let id = crate::types::item_id(&canon);
        out.canonical_url = Some(canon.clone());
        out.host = host.clone();
        out.checked.push(format!("item by canonical url {canon}"));
        let item = match s.reader.call(move |c| reader::get_item(c, id)).await {
            Ok(i) => i,
            Err(e) => return db_err(e),
        };
        if let Some(host) = host.as_deref() {
            out.checked.push(format!("outlet by host {host}"));
            out.outlet = match outlet_for(&s, host).await {
                Ok(o) => o,
                Err(r) => return r,
            };
        }
        if let Some(item) = item {
            out.cluster = match cluster_for(&s, item.cluster_id).await {
                Ok(c) => c,
                Err(r) => return r,
            };
            out.is_origin = out
                .cluster
                .as_ref()
                .map(|c| c.cluster.origin_item_id == item.id);
            out.item = Some(item);
            out.matched = "item";
            return Json(out).into_response();
        }
        if out.outlet.is_some() {
            out.matched = "outlet";
        }
    }

    if let Some(text) = text {
        let hash = crate::parse::simhash::simhash(text);
        let threshold = s.hamming_threshold;
        let since = q.since.unwrap_or_else(|| now() - SYNDICATION_WINDOW_SECS);
        out.checked.push(match q.since {
            None => format!(
                "items with near-identical title and summary over the last {} days",
                SYNDICATION_WINDOW_SECS / 86_400
            ),
            Some(_) => format!("items with near-identical title and summary fetched since {since}"),
        });
        let items = match s
            .reader
            .call(move |c| reader::syndication_matches(c, hash, threshold, since, DEFAULT_LIMIT))
            .await
        {
            Ok(v) => v,
            Err(e) => return db_err(e),
        };
        if let Some(first) = items.first() {
            out.cluster = match cluster_for(&s, first.cluster_id).await {
                Ok(c) => c,
                Err(r) => return r,
            };
            out.matched = "text";
        }
        out.text_matches = Some(TextMatches {
            text_hash: hash,
            hamming_threshold: threshold,
            items,
        });
    }

    Json(out).into_response()
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
#[derive(Serialize, JsonSchema)]
pub(crate) struct ClusterOut {
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
    let cluster = crate::cluster::propagation::assemble(
        &row,
        &items,
        now,
        crate::cluster::propagation::Thresholds {
            hamming: s.hamming_threshold,
            join: s.join_threshold,
        },
    );
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

async fn get_cluster(
    State(s): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let id = match parse_id(&id) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let row = match s.reader.call(move |c| reader::get_cluster(c, id)).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such cluster"),
        Err(e) => return db_err(e),
    };
    let out = match assemble(&s, row, true, now()).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    if wants_markdown(&headers) {
        let names: std::collections::HashMap<String, String> =
            match s.reader.call(reader::outlets).await {
                Ok(v) => v.into_iter().map(|o| (o.id, o.name)).collect(),
                Err(e) => return db_err(e),
            };
        let md = super::agents::cluster_markdown(
            &out.cluster,
            out.items.as_deref().unwrap_or_default(),
            &names,
            &s.ledger_url,
        );
        return ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], md).into_response();
    }
    Json(out).into_response()
}

/// `Accept: text/markdown` asks for the Markdown twin (design §11).
fn wants_markdown(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/markdown"))
}

async fn openapi() -> Response {
    Json(super::openapi::spec()).into_response()
}

async fn llms_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        super::agents::llms_txt(),
    )
        .into_response()
}

async fn robots_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        super::agents::ROBOTS_TXT,
    )
        .into_response()
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

#[derive(Serialize, JsonSchema)]
pub(crate) struct EventsPage {
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

#[derive(Serialize, JsonSchema)]
pub(crate) struct Page<T> {
    items: Vec<T>,
    next: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct ApiError {
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
    /// `failing`, `dead`, `stale` (no success in 24 h), `quarantined`
    /// (held by the steward, docs/steward.md §2), or `all` (default).
    filter: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct FeedOut {
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
            "quarantined" => f.health.as_ref().is_some_and(|h| h.is_quarantined(t)),
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

/// An outlet plus where its sourced facts live. `feed` holds no fact
/// about an outlet beyond its feeds; the link is how a reader gets
/// from a story to who owns the outlet without knowing two servers
/// exist.
#[derive(Serialize, JsonSchema)]
pub(crate) struct OutletOut {
    #[serde(flatten)]
    outlet: reader::OutletRow,
    /// Ledger record for this outlet id (JSON; `Accept: text/markdown` works there too).
    #[serde(skip_serializing_if = "Option::is_none")]
    ledger: Option<String>,
    /// The ledger's page for people.
    #[serde(skip_serializing_if = "Option::is_none")]
    ledger_lookup: Option<String>,
}

pub fn ledger_links(base: &str, id: &str) -> (Option<String>, Option<String>) {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        return (None, None);
    }
    (
        Some(format!("{base}/v1/outlets/{id}")),
        Some(format!("{base}/lookup?q={id}")),
    )
}

#[cfg(test)]
mod ledger_link_tests {
    #[test]
    fn empty_base_means_no_links() {
        assert_eq!(super::ledger_links("", "bbc"), (None, None));
        let (a, b) = super::ledger_links("http://x/", "bbc");
        assert_eq!(a.as_deref(), Some("http://x/v1/outlets/bbc"));
        assert_eq!(b.as_deref(), Some("http://x/lookup?q=bbc"));
    }
}

fn outlet_out(base: &str, o: reader::OutletRow) -> OutletOut {
    let (ledger, ledger_lookup) = ledger_links(base, &o.id);
    OutletOut {
        outlet: o,
        ledger,
        ledger_lookup,
    }
}

async fn outlets(State(s): State<AppState>) -> Response {
    match s.reader.call(reader::outlets).await {
        Ok(v) => Json(
            v.into_iter()
                .map(|o| outlet_out(&s.ledger_url, o))
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => db_err(e),
    }
}

async fn get_outlet(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.reader.call(reader::outlets).await {
        Ok(v) => match v.into_iter().find(|o| o.id == id) {
            Some(o) => Json(outlet_out(&s.ledger_url, o)).into_response(),
            None => err(StatusCode::NOT_FOUND, "no such outlet"),
        },
        Err(e) => db_err(e),
    }
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct StatsOut {
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
