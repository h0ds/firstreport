//! Read-only HTTP API (plan §7). The ledger is loaded once at startup;
//! restart to pick up edits. It is small enough that this is fine.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{Claim, Source, CONTRACT_VERSION};
use crate::store::{FeedOutlet, Ledger};

#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Ledger>,
    /// feed's outlet registry, so outlets without claims are still
    /// findable and the answer "not assessed yet" can be given.
    pub outlets: Arc<Vec<FeedOutlet>>,
}

pub fn router(ledger: Arc<Ledger>, registry: Vec<FeedOutlet>) -> Router {
    Router::new()
        .route("/v1/search", get(search))
        .route("/lookup", get(lookup_page))
        .route("/llms.txt", get(llms_txt))
        .route("/v1/openapi.json", get(openapi))
        .route("/robots.txt", get(robots_txt))
        .route("/v1/outlets", get(outlets))
        .route("/v1/outlets/{id}", get(outlet))
        .route("/v1/outlets/{id}/{field}", get(outlet_field))
        .route("/v1/sources", get(sources))
        .route("/v1/stats", get(stats))
        .with_state(AppState {
            ledger,
            outlets: Arc::new(registry),
        })
}

async fn lookup_page() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        crate::lookup::PAGE,
    )
        .into_response()
}

async fn llms_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        crate::agents::llms_txt(),
    )
        .into_response()
}

async fn openapi() -> Response {
    Json(crate::openapi::spec()).into_response()
}

async fn robots_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        crate::agents::ROBOTS_TXT,
    )
        .into_response()
}

/// `Accept: text/markdown` asks for the Markdown twin (design §11).
fn wants_markdown(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/markdown"))
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct ApiError {
    error: String,
}

fn not_found(msg: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(ApiError { error: msg.into() })).into_response()
}

/// An outlet that has claims.
#[derive(Serialize, JsonSchema)]
pub(crate) struct OutletSummary {
    id: String,
    claims: usize,
    fields: Vec<String>,
    sources: Vec<String>,
}

async fn outlets(State(s): State<AppState>) -> Response {
    let out: Vec<OutletSummary> = s
        .ledger
        .claims
        .iter()
        .map(|(id, claims)| {
            let mut fields: Vec<String> = claims.iter().map(|c| c.field.clone()).collect();
            fields.sort();
            fields.dedup();
            let mut sources: Vec<String> = claims.iter().map(|c| c.source_id.clone()).collect();
            sources.sort();
            sources.dedup();
            OutletSummary {
                id: id.clone(),
                claims: claims.len(),
                fields,
                sources,
            }
        })
        .collect();
    Json(out).into_response()
}

/// A claim plus its freshness against the source's schedule.
#[derive(Serialize, JsonSchema)]
pub(crate) struct ClaimView<'a> {
    #[serde(flatten)]
    claim: &'a Claim,
    /// Days since retrieved.
    age_days: i64,
    /// Older than the source's refresh interval; the value may have
    /// changed since.
    stale: bool,
}

/// Every claim about one outlet.
#[derive(Serialize, JsonSchema)]
pub(crate) struct OutletView<'a> {
    id: String,
    /// Grouped by field; every claim keeps its source. Nothing is merged.
    fields: BTreeMap<String, Vec<ClaimView<'a>>>,
    claims: usize,
    /// How many are past their source's refresh interval.
    stale: usize,
}

fn claim_view<'a>(c: &'a Claim, sources: &[Source], now: i64) -> ClaimView<'a> {
    let stale = sources
        .iter()
        .find(|s| s.id == c.source_id)
        .map(|s| crate::model::is_stale(c, s, now))
        .unwrap_or(false);
    ClaimView {
        claim: c,
        age_days: crate::model::age_days(c, now),
        stale,
    }
}

fn view<'a>(id: &str, claims: &'a [Claim], sources: &[Source], now: i64) -> OutletView<'a> {
    let mut fields: BTreeMap<String, Vec<ClaimView<'a>>> = BTreeMap::new();
    let mut stale = 0;
    for c in claims {
        let v = claim_view(c, sources, now);
        stale += v.stale as usize;
        fields.entry(c.field.clone()).or_default().push(v);
    }
    OutletView {
        id: id.to_string(),
        fields,
        claims: claims.len(),
        stale,
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn outlet(State(s): State<AppState>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    match s.ledger.claims.get(&id) {
        Some(c) if wants_markdown(&headers) => {
            let name = s
                .outlets
                .iter()
                .find(|o| o.id == id)
                .map(|o| o.name.as_str())
                .filter(|n| !n.is_empty());
            (
                [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
                crate::agents::outlet_markdown(&id, name, c, &s.ledger.sources, now_secs()),
            )
                .into_response()
        }
        Some(c) => Json(view(&id, c, &s.ledger.sources, now_secs())).into_response(),
        None => not_found("no claims for that outlet"),
    }
}

async fn outlet_field(
    State(s): State<AppState>,
    Path((id, field)): Path<(String, String)>,
) -> Response {
    match s.ledger.claims.get(&id) {
        Some(c) => {
            let now = now_secs();
            let v: Vec<ClaimView> = c
                .iter()
                .filter(|x| x.field == field)
                .map(|x| claim_view(x, &s.ledger.sources, now))
                .collect();
            if v.is_empty() {
                not_found("no claims for that field")
            } else {
                Json(v).into_response()
            }
        }
        None => not_found("no claims for that outlet"),
    }
}

#[derive(Deserialize)]
struct SearchQ {
    q: Option<String>,
    limit: Option<usize>,
}

/// A registry outlet matching a search.
#[derive(Serialize, JsonSchema)]
pub(crate) struct SearchHit {
    id: String,
    name: String,
    domains: Vec<String>,
    /// 0 means in the registry but not assessed yet.
    claims: usize,
    stale: usize,
    /// Which of id, name or a domain matched.
    matched: &'static str,
}

/// Case-insensitive substring search over outlet id, name and domains.
/// Covers every outlet in feed's registry, claims or not, so "no claims
/// yet" is a real answer; registry-less ledgers search claim ids only.
fn search_outlets(
    q: &str,
    outlets: &[FeedOutlet],
    ledger: &Ledger,
    now: i64,
    limit: usize,
) -> Vec<SearchHit> {
    let q = q.trim().to_ascii_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let q_host = q
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/');
    let mut hits: Vec<(u8, SearchHit)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let counts = |id: &str| -> (usize, usize) {
        ledger
            .claims
            .get(id)
            .map(|cs| {
                let stale = cs
                    .iter()
                    .filter(|c| {
                        ledger
                            .sources
                            .iter()
                            .find(|s| s.id == c.source_id)
                            .map(|s| crate::model::is_stale(c, s, now))
                            .unwrap_or(false)
                    })
                    .count();
                (cs.len(), stale)
            })
            .unwrap_or((0, 0))
    };
    for o in outlets {
        let id = o.id.to_ascii_lowercase();
        let name = o.name.to_ascii_lowercase();
        let rank_match = if id == q || name == q {
            Some((0, if id == q { "id" } else { "name" }))
        } else if o
            .domains
            .iter()
            .any(|d| d.trim_start_matches("www.").eq_ignore_ascii_case(q_host))
        {
            Some((0, "domain"))
        } else if id.contains(&q) {
            Some((1, "id"))
        } else if name.contains(&q) {
            Some((1, "name"))
        } else if o
            .domains
            .iter()
            .any(|d| d.to_ascii_lowercase().contains(q_host))
        {
            Some((2, "domain"))
        } else {
            None
        };
        if let Some((rank, matched)) = rank_match {
            let (claims, stale) = counts(&o.id);
            seen.insert(o.id.clone());
            hits.push((
                rank,
                SearchHit {
                    id: o.id.clone(),
                    name: o.name.clone(),
                    domains: o.domains.clone(),
                    claims,
                    stale,
                    matched,
                },
            ));
        }
    }
    for id in ledger.claims.keys() {
        if !seen.contains(id) && id.to_ascii_lowercase().contains(&q) {
            let (claims, stale) = counts(id);
            hits.push((
                1,
                SearchHit {
                    id: id.clone(),
                    name: String::new(),
                    domains: vec![],
                    claims,
                    stale,
                    matched: "id",
                },
            ));
        }
    }
    hits.sort_by(|a, b| (a.0, &a.1.id).cmp(&(b.0, &b.1.id)));
    hits.into_iter().map(|(_, h)| h).take(limit).collect()
}

async fn search(State(s): State<AppState>, Query(q): Query<SearchQ>) -> Response {
    let hits = search_outlets(
        q.q.as_deref().unwrap_or(""),
        &s.outlets,
        &s.ledger,
        now_secs(),
        q.limit.unwrap_or(20).clamp(1, 100),
    );
    Json(hits).into_response()
}

async fn sources(State(s): State<AppState>) -> Response {
    let v: Vec<&Source> = s.ledger.sources.iter().collect();
    Json(v).into_response()
}

#[derive(Serialize, JsonSchema)]
pub(crate) struct Stats {
    contract_version: u32,
    outlets: usize,
    claims: usize,
    sources: usize,
    claims_by_source: BTreeMap<String, usize>,
    claims_by_field: BTreeMap<String, usize>,
}

async fn stats(State(s): State<AppState>) -> Response {
    let mut by_source = BTreeMap::new();
    let mut by_field = BTreeMap::new();
    let mut n = 0;
    for claims in s.ledger.claims.values() {
        for c in claims {
            n += 1;
            *by_source.entry(c.source_id.clone()).or_insert(0) += 1;
            *by_field.entry(c.field.clone()).or_insert(0) += 1;
        }
    }
    Json(Stats {
        contract_version: CONTRACT_VERSION,
        outlets: s.ledger.claims.len(),
        claims: n,
        sources: s.ledger.sources.len(),
        claims_by_source: by_source,
        claims_by_field: by_field,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn ledger() -> Ledger {
        let mut l = Ledger::default();
        let c = |field: &str, source: &str, value: &str| Claim {
            outlet_id: "bbc".into(),
            field: field.into(),
            value: value.into(),
            data: None,
            source_id: source.into(),
            ref_url: "https://x".into(),
            retrieved_at: 1,
            as_of: None,
            note: None,
        };
        l.sources.push(Source {
            id: "wikidata".into(),
            name: "Wikidata".into(),
            url: String::new(),
            licence: "CC0-1.0".into(),
            kind: crate::model::SourceKind::Database,
            notes: String::new(),
            no_redistribution: false,
            refresh_days: 90,
        });
        l.claims.insert(
            "bbc".into(),
            vec![
                c("country", "wikidata", "United Kingdom"),
                c("bias", "a", "Center"),
                c("bias", "b", "Lean Left"),
            ],
        );
        l
    }

    async fn json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let r = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let st = r.status();
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        (
            st,
            serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn routes() {
        let outlets = vec![
            FeedOutlet {
                id: "bbc".into(),
                name: "BBC".into(),
                domains: vec!["bbc.co.uk".into(), "bbc.com".into()],
            },
            FeedOutlet {
                id: "npr".into(),
                name: "NPR".into(),
                domains: vec!["npr.org".into()],
            },
        ];
        let app = router(Arc::new(ledger()), outlets);
        let (st, v) = json(&app, "/v1/outlets").await;
        assert_eq!(st, 200);
        assert_eq!(v[0]["id"], "bbc");
        assert_eq!(v[0]["claims"], 3);
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/outlets/bbc")
                    .header("accept", "text/markdown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/markdown"));
        let md = String::from_utf8(
            axum::body::to_bytes(r.into_body(), 1 << 20)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(md.starts_with("# BBC\n"), "{md}");
        assert!(md.contains("Source: [Wikidata](https://x)"));
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/llms.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let (st, spec) = json(&app, "/v1/openapi.json").await;
        assert_eq!(st, 200);
        assert_eq!(spec["openapi"], "3.1.0");
        for p in crate::openapi::sample_paths() {
            let r = app
                .clone()
                .oneshot(Request::builder().uri(&p).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_ne!(r.status(), 404, "documented path {p} is not routed");
        }
        let (_, v) = json(&app, "/v1/outlets/bbc").await;
        assert_eq!(
            v["fields"]["bias"].as_array().unwrap().len(),
            2,
            "disagreeing sources side by side"
        );
        assert_eq!(v["fields"]["country"][0]["value"], "United Kingdom");
        assert_eq!(
            v["fields"]["country"][0]["stale"], true,
            "retrieved at t=1 on a 90-day schedule"
        );
        assert_eq!(v["fields"]["bias"][0]["stale"], false, "unscheduled source");
        assert_eq!(v["stale"], 1);
        let (_, v) = json(&app, "/v1/outlets/bbc/bias").await;
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(json(&app, "/v1/outlets/cnn").await.0, 404);
        assert_eq!(json(&app, "/v1/outlets/bbc/owner").await.0, 404);
        let (_, v) = json(&app, "/v1/search?q=bbc").await;
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["claims"], 3);
        assert_eq!(v[0]["matched"], "id");
        let (_, v) = json(&app, "/v1/search?q=https://www.npr.org/").await;
        assert_eq!(v[0]["id"], "npr");
        assert_eq!(v[0]["claims"], 0, "in the registry, not assessed yet");
        assert_eq!(v[0]["matched"], "domain");
        let (_, v) = json(&app, "/v1/search?q=zzz").await;
        assert!(v.as_array().unwrap().is_empty());
        let (_, v) = json(&app, "/v1/search").await;
        assert!(v.as_array().unwrap().is_empty());
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/lookup")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let (_, v) = json(&app, "/v1/stats").await;
        assert_eq!(v["claims"], 3);
        assert_eq!(v["claims_by_field"]["bias"], 2);
    }
}
