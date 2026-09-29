//! `reply serve`: the review page and queue actions (local only), and the
//! public log (plan §10, §13). Approving posts, so the poster lives here.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::compose::keep_sentences;
use crate::model::CONTRACT_VERSION;
use crate::platform::Poster;
use crate::queue::{Entry, Queue};

/// Posting policy (plan §9, §14). Counted over the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub per_account_per_day: usize,
    pub global_per_hour: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_account_per_day: 3,
            global_per_hour: 60,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub data: PathBuf,
    /// Absent means approving is refused: nothing can post.
    pub poster: Option<Arc<Mutex<Box<dyn Poster>>>>,
    pub limits: Limits,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/review", get(page))
        .route("/llms.txt", get(llms_txt))
        .route("/robots.txt", get(robots_txt))
        .route("/v1/queue", get(list_queue))
        .route("/v1/queue/{id}", get(get_queued))
        .route("/v1/queue/{id}/approve", post(approve))
        .route("/v1/queue/{id}/decline", post(decline))
        .route("/v1/log", get(list_log))
        .route("/v1/log/{id}", get(get_log))
        .route("/v1/observations", get(observations))
        .route("/v1/stats", get(stats))
        .with_state(state)
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ApiError { error: msg.into() })).into_response()
}

#[allow(clippy::result_large_err)] // the Err is the HTTP response itself
fn open(s: &AppState) -> Result<Queue, Response> {
    Queue::open(&s.data).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn list_queue(State(s): State<AppState>) -> Response {
    let q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    match q.list() {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn get_queued(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    match q.get(&id) {
        Ok(Some(e)) => Json(e).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such pending entry"),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize, Default)]
struct ApproveBody {
    /// Sentence indexes to keep; empty keeps all. The closing sentence stays.
    #[serde(default)]
    keep: Vec<usize>,
}

/// Which limit a post would break, if any.
pub fn over_limit(log: &[Entry], author: &str, limits: Limits, now: i64) -> Option<String> {
    let posted = log.iter().filter(|e| e.status == "posted");
    let hour = posted
        .clone()
        .filter(|e| e.decided_at.is_some_and(|t| now - t < 3600))
        .count();
    if hour >= limits.global_per_hour {
        return Some(format!(
            "{hour} replies posted in the last hour; limit {}",
            limits.global_per_hour
        ));
    }
    let day = posted
        .filter(|e| {
            e.facts.mention.author == author && e.decided_at.is_some_and(|t| now - t < 86_400)
        })
        .count();
    if day >= limits.per_account_per_day {
        return Some(format!(
            "{day} replies to @{author} in the last day; limit {}",
            limits.per_account_per_day
        ));
    }
    None
}

async fn approve(
    State(s): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<ApproveBody>>,
) -> Response {
    let Some(poster) = s.poster.clone() else {
        return err(
            StatusCode::CONFLICT,
            "no poster configured: serve was started without platform credentials",
        );
    };
    let mut q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    let entry = match q.get(&id) {
        Ok(Some(e)) => e,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such pending entry"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let keep = body.map(|b| b.0.keep).unwrap_or_default();
    let draft = match keep_sentences(&entry.draft, &keep) {
        Ok(d) => d,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let mut poster = poster.lock().await;
    match crate::post::post_and_log(
        &mut q,
        poster.as_mut(),
        &id,
        &entry,
        draft,
        s.limits,
        "human",
        now(),
    )
    .await
    {
        Ok(e) => Json(e).into_response(),
        Err(crate::post::Refused::OverLimit(why)) => err(StatusCode::TOO_MANY_REQUESTS, why),
        Err(crate::post::Refused::Platform(e)) => err(StatusCode::BAD_GATEWAY, e),
        Err(crate::post::Refused::Store(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn decline(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let mut q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    let entry = match q.get(&id) {
        Ok(Some(e)) => e,
        Ok(None) => return err(StatusCode::NOT_FOUND, "no such pending entry"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    match q.decide(&id, "declined", entry.draft, None, now()) {
        Ok(e) => Json(e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// The public log strips the handle of whoever summoned the bot (plan §10).
fn public(mut e: Entry) -> Entry {
    e.facts.mention.author = String::new();
    e
}

async fn list_log(State(s): State<AppState>) -> Response {
    let q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    match q.log() {
        Ok(v) => Json(v.into_iter().map(public).collect::<Vec<_>>()).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn get_log(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    match q.log_entry(&id) {
        Ok(Some(e)) => Json(public(e)).into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "no such log entry"),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize, Default)]
struct ObservationsQ {
    subject: Option<String>,
    kind: Option<String>,
}

/// Latest observation per (kind, subject), filtered. Accounts appear only
/// as salted hashes, in `first_movers` subjects.
async fn observations(
    State(s): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<ObservationsQ>,
) -> Response {
    let store = match crate::observe::store::Store::open(&s.data) {
        Ok(st) => st,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    match store.observations() {
        Ok(v) => Json(
            v.into_iter()
                .filter(|o| q.kind.as_deref().is_none_or(|k| o.kind == k))
                .filter(|o| {
                    q.subject.as_deref().is_none_or(|sub| {
                        o.subject == sub
                            || crate::observe::subject_url(sub).is_some_and(|u| o.subject == u)
                    })
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Serialize)]
struct Stats {
    contract_version: u32,
    pending: usize,
    posted: usize,
    declined: usize,
    poster: Option<String>,
}

async fn stats(State(s): State<AppState>) -> Response {
    let q = match open(&s) {
        Ok(q) => q,
        Err(r) => return r,
    };
    let (pending, log) = match (q.list(), q.log()) {
        (Ok(p), Ok(l)) => (p, l),
        (Err(e), _) | (_, Err(e)) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let poster = match &s.poster {
        Some(p) => Some(p.lock().await.name().to_string()),
        None => None,
    };
    Json(Stats {
        contract_version: CONTRACT_VERSION,
        pending: pending.len(),
        posted: log.iter().filter(|e| e.status == "posted").count(),
        declined: log.iter().filter(|e| e.status == "declined").count(),
        poster,
    })
    .into_response()
}

async fn llms_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!(
            "# Kagami reply

> The public log of a social media account that answers \"prove it\" with
> where a story came from, how it spread and who carried it, every
> sentence linked to a Kagami feed record or ledger claim. It never says
> true or false, and never labels an account.

Contract version {CONTRACT_VERSION}. Times are unix seconds UTC.

## Routes

- `GET /v1/log` - every mention answered or declined, with the facts each reply was built from, the templates that fired, and who approved it
- `GET /v1/log/{{mention id or queue id}}` - one entry; this is what every reply's closing link points at
- `GET /v1/observations?subject=&kind=` - counts over accounts posting a link or text: burst, syndicated_text, cohort, first_movers. Accounts are salted hashes; a count is a count, not a label
- `GET /v1/stats`

The handle of whoever summoned the bot is not in the log. Queue routes
are for the local reviewer and are not part of the contract.
"
        ),
    )
        .into_response()
}

async fn robots_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "User-agent: *\nAllow: /\n",
    )
        .into_response()
}

async fn page() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], PAGE).into_response()
}

/// The review page: pending drafts, each sentence with its cite, the
/// reviewer keeps or drops sentences and approves or declines. Styled
/// from a copy of `design/tokens.css` (project rule: copy, no shared
/// runtime dependency).
const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kagami reply review</title>
<meta name="description" content="Drafted replies waiting for a person. Every sentence links its source; a reviewer keeps or drops sentences and posts or declines.">
<link rel="alternate" type="application/json" href="/v1/log" title="Public log (JSON)">
<link rel="alternate" type="text/plain" href="/llms.txt" title="How to read this server">
<style>
:root{--kg-blue:#2563EB;--kg-blue-hover:#1D4ED8;--kg-blue-soft:#DBEAFE;--kg-blue-ink:#1E3A8A;--kg-bg:#FFFFFF;--kg-bg-subtle:#F8FAFC;--kg-line:#E2E8F0;--kg-line-strong:#CBD5E1;--kg-text-muted:#64748B;--kg-text-secondary:#334155;--kg-text:#0F172A;--kg-ok:#059669;--kg-warn:#D97706;--kg-err:#DC2626;--kg-font-serif:"Instrument Serif","Iowan Old Style","Times New Roman",serif;--kg-font-sans:Inter,-apple-system,"Segoe UI",system-ui,sans-serif;--kg-font-mono:"JetBrains Mono","SF Mono",Menlo,Consolas,monospace;--kg-t-base:200ms;--kg-ease:cubic-bezier(0.2,0,0,1)}
html{background:var(--kg-bg);color:var(--kg-text)}
body{margin:0;font:15px/1.6 var(--kg-font-sans);font-feature-settings:"tnum","cv11","ss01";-webkit-font-smoothing:antialiased}
main{max-width:880px;margin:0 auto;padding:32px 16px 96px}
header{display:flex;align-items:baseline;justify-content:space-between;gap:16px;flex-wrap:wrap;padding-bottom:16px;border-bottom:1px solid var(--kg-line)}
.lockup{display:inline-flex;align-items:baseline;gap:12px;font-size:32px}
.wordmark{font-family:var(--kg-font-serif);font-style:italic;letter-spacing:-0.01em}
.wordmark .kanji{font-family:"Noto Serif JP",serif;font-style:normal;font-size:.6em;color:var(--kg-text-muted);margin-left:.25em}
.rule{align-self:stretch;width:1px;background:var(--kg-line-strong)}
.tool{font-weight:500;font-size:16px}
.muted{color:var(--kg-text-muted);font-size:12px;letter-spacing:.01em}
.mono{font-family:var(--kg-font-mono);font-size:13px}
a{color:var(--kg-blue);text-decoration:none;text-underline-offset:3px}a:hover{text-decoration:underline}
h2{font-family:var(--kg-font-serif);font-weight:400;font-size:28px;line-height:1.15;margin:32px 0 8px}
.entry{padding:20px 0;border-bottom:1px solid var(--kg-line)}
.mention{font-size:13px;color:var(--kg-text-secondary)}
.mention .who{font-weight:500;color:var(--kg-text)}
.quote{margin:6px 0 0;padding-left:12px;border-left:2px solid var(--kg-line-strong);color:var(--kg-text-secondary);font-size:14px}
.sentences{list-style:none;margin:12px 0 0;padding:0}
.sentences li{display:grid;grid-template-columns:20px 1fr;gap:10px;align-items:start;padding:6px 0}
.sentences input{margin:5px 0 0}
.sentences .cite{display:block;font-size:12px;color:var(--kg-text-muted)}
.sentences .cite .tag{font-family:var(--kg-font-mono);font-size:11px;padding:0 5px;border:1px solid var(--kg-line);border-radius:4px;margin-right:6px}
.gaps{margin:8px 0 0;font-size:13px;color:var(--kg-text-secondary)}
.gaps span::before{content:"gap · ";color:var(--kg-text-muted)}
.actions{display:flex;gap:12px;align-items:center;margin-top:14px}
button{font:500 14px var(--kg-font-sans);padding:8px 14px;border-radius:4px;border:1px solid var(--kg-line-strong);background:var(--kg-bg);color:var(--kg-text);cursor:pointer}
button:hover{border-color:var(--kg-text-muted)}
button.primary{background:var(--kg-blue);border-color:var(--kg-blue);color:#fff}button.primary:hover{background:var(--kg-blue-hover)}
button:disabled{opacity:.5;cursor:default}
.result{font-size:13px}.result.ok{color:var(--kg-ok)}.result.err{color:var(--kg-err)}
.empty{color:var(--kg-text-muted);padding:24px 0}
.log .entry{padding:12px 0}
.log .posts{font-size:14px;color:var(--kg-text-secondary);margin:4px 0 0}
.enter{animation:kg-fade-in var(--kg-t-base) var(--kg-ease)}
@keyframes kg-fade-in{from{opacity:0}to{opacity:1}}
@media (prefers-reduced-motion:reduce){.enter{animation:none}}
</style>
</head>
<body>
<main>
<header>
  <span class="lockup"><span class="wordmark">Kagami<span class="kanji">鏡</span></span><span class="rule"></span><span class="tool">reply</span></span>
  <span class="muted" id="counts"></span>
</header>
<h2>Waiting for a person</h2>
<p class="muted">Every sentence links its source. Untick a sentence to leave it out; the closing line always stays. Approving posts under the mention.</p>
<div id="queue"></div>
<h2>Public log</h2>
<div id="log" class="log"></div>
<p class="muted" style="margin-top:48px">Machine readers: the log is at <a href="/v1/log">/v1/log</a>. See <a href="/llms.txt">/llms.txt</a>.</p>
</main>
<script>
const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const when = t => { const d = new Date(t * 1000); return d.toISOString().slice(0, 16).replace('T', ' ') + ' UTC'; };
async function load() {
  const [q, log, st] = await Promise.all([fetch('/v1/queue').then(r => r.json()), fetch('/v1/log').then(r => r.json()), fetch('/v1/stats').then(r => r.json())]);
  $('#counts').textContent = `${st.pending} waiting · ${st.posted} posted · ${st.declined} declined · poster: ${st.poster || 'none, approving is off'}`;
  if (!q.length) { $('#queue').innerHTML = '<div class="empty">Nothing waiting. Run <span class="mono">reply listen</span> to draft replies to new mentions.</div>'; }
  else $('#queue').innerHTML = q.map(e => {
    const m = e.facts.mention, d = e.draft, last = d.sentences.length - 1;
    const subj = e.facts.subject, about = subj.urls.length ? subj.urls[0] : (subj.text || '');
    return `<article class="entry enter" data-id="${esc(e.id)}">
      <div class="mention"><span class="who">@${esc(m.author)}</span> on ${esc(m.platform)} · ${when(m.at)} · <a href="${esc(m.id)}" rel="noopener">mention</a> · subject from ${esc(subj.from)}</div>
      <div class="quote">${esc(m.text)}${about ? `<br><span class="mono">${esc(about)}</span>` : ''}</div>
      <ul class="sentences">${d.sentences.map((s, i) => { const c = d.cites.find(c => c.sentence === i) || {}; return `<li><input type="checkbox" checked ${i === last ? 'disabled' : ''} data-i="${i}"><div>${esc(s)}<span class="cite"><span class="tag">${esc(c.kind)}</span><a href="${esc(c.url)}" rel="noopener">${esc(c.reference)}</a> · ${esc(d.template_ids[i])}</span></div></li>`; }).join('')}</ul>
      ${e.facts.gaps.length ? `<div class="gaps">${e.facts.gaps.map(g => `<span>${esc(g)}</span>`).join('<br>')}</div>` : ''}
      <div class="actions"><button class="primary" data-act="approve">Post reply</button><button data-act="decline">Decline</button><span class="result"></span></div>
    </article>`;
  }).join('');
  $('#log').innerHTML = log.length ? log.slice(-20).reverse().map(e => `<div class="entry"><div class="mention">${esc(e.status)} · ${when(e.decided_at)} · ${esc(e.facts.mention.platform)} · ${esc(e.draft.template_ids.join(', '))} · <a href="/v1/log/${esc(e.id)}">record</a>${e.reply ? ` · ${esc(e.reply.reviewer)}` + e.reply.post_ids.map(p => ` · <a href="${esc(p)}" rel="noopener">post</a>`).join('') : ''}</div><div class="posts">${e.draft.posts.map(esc).join('<br>')}</div></div>`).join('') : '<div class="empty">No replies yet.</div>';
}
document.addEventListener('click', async ev => {
  const b = ev.target.closest('button[data-act]'); if (!b) return;
  const art = b.closest('article'), id = art.dataset.id, out = art.querySelector('.result');
  const keep = [...art.querySelectorAll('input[data-i]')].filter(c => c.checked).map(c => +c.dataset.i);
  art.querySelectorAll('button').forEach(x => x.disabled = true);
  const r = await fetch(`/v1/queue/${id}/${b.dataset.act}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ keep }) });
  const j = await r.json();
  if (r.ok) { out.className = 'result ok'; out.textContent = j.status; setTimeout(load, 600); }
  else { out.className = 'result err'; out.textContent = j.error; art.querySelectorAll('button').forEach(x => x.disabled = false); }
});
load();
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Cite, Draft, Mention};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    struct Fake {
        fail_after: usize,
    }
    impl Poster for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        fn post<'a>(
            &'a mut self,
            m: &'a Mention,
            posts: &'a [String],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::platform::Posted> + Send + 'a>>
        {
            Box::pin(async move {
                let ids: Vec<String> = posts
                    .iter()
                    .enumerate()
                    .take(self.fail_after)
                    .map(|(i, _)| format!("{}/reply/{i}", m.id))
                    .collect();
                if ids.len() < posts.len() {
                    Err((ids, "boom".into()))
                } else {
                    Ok(ids)
                }
            })
        }
    }

    fn draft(id: &str, n: usize) -> Draft {
        Draft {
            contract_version: 1,
            mention_id: id.into(),
            platform: "t".into(),
            template_ids: (0..n).map(|i| format!("t{i}")).collect(),
            sentences: (0..n).map(|i| format!("Sentence {i}.")).collect(),
            posts: vec![],
            max_chars: 20,
            cites: (0..n)
                .map(|i| Cite {
                    sentence: i as u16,
                    reference: "r".into(),
                    url: "u".into(),
                    kind: "k".into(),
                })
                .collect(),
            autonomous: false,
        }
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let b = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null)
    }

    fn req(method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn review_flow() {
        let dir = std::env::temp_dir().join(format!("kg-reply-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut q = Queue::open(&dir).unwrap();
        for id in ["at://a/1", "at://a/2", "at://a/3"] {
            let mut d = draft(id, 3);
            d.posts = crate::compose::keep_sentences(&d, &[]).unwrap().posts;
            q.push(crate::queue::tests::facts(id), d, 1).unwrap();
        }
        let qid = |id: &str| crate::queue::queue_id(id);

        // No poster: approving is refused, declining works.
        let app = router(AppState {
            data: dir.clone(),
            poster: None,
            limits: Limits::default(),
        });
        let r = app
            .clone()
            .oneshot(req(
                "POST",
                &format!("/v1/queue/{}/approve", qid("at://a/1")),
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), 409);
        let r = app
            .clone()
            .oneshot(req(
                "POST",
                &format!("/v1/queue/{}/decline", qid("at://a/3")),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(body_json(r).await["status"], "declined");
        let v = body_json(
            app.clone()
                .oneshot(req("GET", "/v1/queue", ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(v.as_array().unwrap().len(), 2);
        let v = body_json(
            app.clone()
                .oneshot(req("GET", "/v1/stats", ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (v["pending"].as_u64(), v["declined"].as_u64()),
            (Some(2), Some(1))
        );

        // With a poster: keep drops a sentence, the reply is logged with its ids.
        let poster: Arc<Mutex<Box<dyn Poster>>> =
            Arc::new(Mutex::new(Box::new(Fake { fail_after: 99 })));
        let app = router(AppState {
            data: dir.clone(),
            poster: Some(poster),
            limits: Limits {
                per_account_per_day: 1,
                global_per_hour: 60,
            },
        });
        let r = app
            .clone()
            .oneshot(req(
                "POST",
                &format!("/v1/queue/{}/approve", qid("at://a/1")),
                r#"{"keep":[1]}"#,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let v = body_json(r).await;
        assert_eq!(v["status"], "posted");
        assert_eq!(v["draft"]["sentences"].as_array().unwrap().len(), 2);
        assert_eq!(v["reply"]["edited"], true);
        assert_eq!(v["reply"]["post_ids"].as_array().unwrap().len(), 2, "{v}");
        assert_eq!(v["reply"]["reviewer"], "human");

        // Per-account limit: the same author again is refused.
        let r = app
            .clone()
            .oneshot(req(
                "POST",
                &format!("/v1/queue/{}/approve", qid("at://a/2")),
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), 429);

        // Public log hides the handle; the entry is reachable by mention id.
        let v = body_json(
            app.clone()
                .oneshot(req("GET", "/v1/log", ""))
                .await
                .unwrap(),
        )
        .await;
        assert!(v
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["facts"]["mention"]["author"] == ""));
        let r = app
            .clone()
            .oneshot(req("GET", "/v1/log/at:%2F%2Fa%2F1", ""))
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(
            app.clone()
                .oneshot(req("GET", "/v1/log/nope", ""))
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            app.clone()
                .oneshot(req("GET", "/", ""))
                .await
                .unwrap()
                .status(),
            200
        );
        let v = body_json(
            app.clone()
                .oneshot(req("GET", "/v1/observations?kind=burst", ""))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(v.as_array().map(Vec::len), Some(0));

        // A thread that fails part-way is still logged as posted, with a correction.
        let poster: Arc<Mutex<Box<dyn Poster>>> =
            Arc::new(Mutex::new(Box::new(Fake { fail_after: 1 })));
        let app = router(AppState {
            data: dir.clone(),
            poster: Some(poster),
            limits: Limits::default(),
        });
        let v = body_json(
            app.clone()
                .oneshot(req(
                    "POST",
                    &format!("/v1/queue/{}/approve", qid("at://a/2")),
                    "{}",
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(v["status"], "posted");
        assert_eq!(v["reply"]["post_ids"].as_array().unwrap().len(), 1);
        assert!(v["corrections"][0]
            .as_str()
            .unwrap()
            .starts_with("thread incomplete"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limits_count_the_log() {
        let mut e = Entry {
            id: "x".into(),
            facts: crate::queue::tests::facts("m"),
            draft: draft("m", 1),
            status: "posted".into(),
            queued_at: 0,
            decided_at: Some(100),
            reply: None,
            corrections: vec![],
        };
        let l = Limits {
            per_account_per_day: 1,
            global_per_hour: 2,
        };
        assert!(over_limit(&[e.clone()], "a", l, 200).is_some());
        assert!(over_limit(&[e.clone()], "b", l, 200).is_none());
        assert!(over_limit(&[e.clone()], "a", l, 100 + 86_401).is_none());
        e.status = "declined".into();
        assert!(over_limit(&[e], "a", l, 200).is_none());
    }
}
