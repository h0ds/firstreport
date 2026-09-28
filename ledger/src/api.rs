//! Read-only HTTP API (plan §7). The ledger is loaded once at startup;
//! restart to pick up edits. It is small enough that this is fine.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::model::{Claim, Source, CONTRACT_VERSION};
use crate::store::Ledger;

#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Ledger>,
}

pub fn router(ledger: Arc<Ledger>) -> Router {
    Router::new()
        .route("/v1/outlets", get(outlets))
        .route("/v1/outlets/{id}", get(outlet))
        .route("/v1/outlets/{id}/{field}", get(outlet_field))
        .route("/v1/sources", get(sources))
        .route("/v1/stats", get(stats))
        .with_state(AppState { ledger })
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

fn not_found(msg: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(ApiError { error: msg.into() })).into_response()
}

#[derive(Serialize)]
struct OutletSummary {
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
#[derive(Serialize)]
struct ClaimView<'a> {
    #[serde(flatten)]
    claim: &'a Claim,
    age_days: i64,
    stale: bool,
}

#[derive(Serialize)]
struct OutletView<'a> {
    id: String,
    /// Grouped by field; every claim keeps its source. Nothing is merged.
    fields: BTreeMap<String, Vec<ClaimView<'a>>>,
    claims: usize,
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

async fn outlet(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.ledger.claims.get(&id) {
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

async fn sources(State(s): State<AppState>) -> Response {
    let v: Vec<&Source> = s.ledger.sources.iter().collect();
    Json(v).into_response()
}

#[derive(Serialize)]
struct Stats {
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
        let app = router(Arc::new(ledger()));
        let (st, v) = json(&app, "/v1/outlets").await;
        assert_eq!(st, 200);
        assert_eq!(v[0]["id"], "bbc");
        assert_eq!(v[0]["claims"], 3);
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
        let (_, v) = json(&app, "/v1/stats").await;
        assert_eq!(v["claims"], 3);
        assert_eq!(v["claims_by_field"]["bias"], 2);
    }
}
