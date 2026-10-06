//! Bluesky over XRPC. Mentions come from `app.bsky.notification.listNotifications`,
//! polled: the bot is summoned, so no firehose is needed.
//! Parent and quoted posts are fetched with `app.bsky.feed.getPosts`.
//! Parsing is pure and tested on recorded responses.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::{Found, Listener, Posted, Poster, SearchQuery, Searcher};
use crate::facts::{get, parse_rfc3339, text};
use crate::model::{Mention, Post, Thread};

pub const DEFAULT_PDS: &str = "https://bsky.social";
pub const PASSWORD_ENV: &str = "REPLY_BSKY_PASSWORD";
const PAGE: usize = 50;

pub struct Bluesky {
    pds: String,
    handle: String,
    password: String,
    client: reqwest::Client,
    access: Option<String>,
    did: String,
    /// Newest `indexedAt` handled; older notifications are not re-read.
    high_water: String,
}

impl Bluesky {
    /// `password` is an app password, read from `REPLY_BSKY_PASSWORD` by the caller.
    pub fn new(pds: &str, handle: &str, password: &str) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("truthdecay-reply/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            pds: pds.trim_end_matches('/').to_string(),
            handle: handle.to_string(),
            password: password.to_string(),
            client,
            access: None,
            did: String::new(),
            high_water: String::new(),
        })
    }

    async fn login(&mut self) -> Result<(), String> {
        let v: Value = self
            .client
            .post(format!(
                "{}/xrpc/com.atproto.server.createSession",
                self.pds
            ))
            .json(&json!({ "identifier": self.handle, "password": self.password }))
            .send()
            .await
            .map_err(|e| format!("createSession: {e}"))?
            .error_for_status()
            .map_err(|e| format!("createSession: {e}"))?
            .json()
            .await
            .map_err(|e| format!("createSession: {e}"))?;
        self.access = get(&v, "accessJwt").map(text);
        self.did = get(&v, "did").map(text).unwrap_or_default();
        if self.access.is_none() {
            return Err("createSession: no accessJwt in response".into());
        }
        Ok(())
    }

    async fn xrpc_post(&mut self, method: &str, body: &Value) -> Result<Value, String> {
        if self.access.is_none() {
            self.login().await?;
        }
        for attempt in 0..2 {
            let resp = self
                .client
                .post(format!("{}/xrpc/{method}", self.pds))
                .bearer_auth(self.access.as_deref().unwrap_or_default())
                .json(body)
                .send()
                .await
                .map_err(|e| format!("{method}: {e}"))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.login().await?;
                continue;
            }
            let status = resp.status();
            let v: Value = resp.json().await.map_err(|e| format!("{method}: {e}"))?;
            if !status.is_success() {
                return Err(format!(
                    "{method}: HTTP {status} {}",
                    get(&v, "message").map(text).unwrap_or_default()
                ));
            }
            return Ok(v);
        }
        Err(format!("{method}: unauthorized after re-login"))
    }

    async fn xrpc_get(&mut self, method: &str, params: &[(&str, &str)]) -> Result<Value, String> {
        if self.access.is_none() {
            self.login().await?;
        }
        for attempt in 0..2 {
            let resp = self
                .client
                .get(format!("{}/xrpc/{method}", self.pds))
                .query(params)
                .bearer_auth(self.access.as_deref().unwrap_or_default())
                .send()
                .await
                .map_err(|e| format!("{method}: {e}"))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.login().await?;
                continue;
            }
            return resp
                .error_for_status()
                .map_err(|e| format!("{method}: {e}"))?
                .json()
                .await
                .map_err(|e| format!("{method}: {e}"));
        }
        Err(format!("{method}: unauthorized after re-login"))
    }
}

impl Listener for Bluesky {
    fn name(&self) -> &str {
        "bluesky"
    }

    async fn poll(&mut self) -> Result<Vec<Mention>, String> {
        let limit = PAGE.to_string();
        let v = self
            .xrpc_get(
                "app.bsky.notification.listNotifications",
                &[("limit", &limit)],
            )
            .await?;
        let mut partial = parse_notifications(&v, &self.high_water);
        let refs: Vec<String> = partial
            .iter()
            .flat_map(|p| p.refs.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut posts = HashMap::new();
        for chunk in refs.chunks(25) {
            let params: Vec<(&str, &str)> = chunk.iter().map(|u| ("uris", u.as_str())).collect();
            let v = self.xrpc_get("app.bsky.feed.getPosts", &params).await?;
            posts.extend(parse_posts(&v));
        }
        let mut out = Vec::with_capacity(partial.len());
        for p in partial.drain(..) {
            if p.indexed_at > self.high_water {
                self.high_water = p.indexed_at.clone();
            }
            out.push(p.finish(&posts));
        }
        Ok(out)
    }
}

impl Poster for Bluesky {
    fn name(&self) -> &str {
        "bluesky"
    }

    fn post<'a>(
        &'a mut self,
        m: &'a Mention,
        posts: &'a [String],
        lang: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Posted> + Send + 'a>> {
        Box::pin(async move {
            let thread = m.thread.clone().unwrap_or_default();
            if thread.cid.is_empty() {
                return Err((vec![], "mention has no cid; cannot reply in thread".into()));
            }
            let root = (
                if thread.root_id.is_empty() {
                    m.id.clone()
                } else {
                    thread.root_id.clone()
                },
                if thread.root_cid.is_empty() {
                    thread.cid.clone()
                } else {
                    thread.root_cid.clone()
                },
            );
            let mut parent = (m.id.clone(), thread.cid.clone());
            let mut ids = Vec::with_capacity(posts.len());
            for text_ in posts {
                let record = post_record(text_, &root, &parent, &now_rfc3339(), lang);
                let body = json!({ "repo": self.did, "collection": "app.bsky.feed.post", "record": record });
                match self.xrpc_post("com.atproto.repo.createRecord", &body).await {
                    Ok(v) => {
                        let uri = get(&v, "uri").map(text).unwrap_or_default();
                        let cid = get(&v, "cid").map(text).unwrap_or_default();
                        ids.push(uri.clone());
                        parent = (uri, cid);
                    }
                    Err(e) => return Err((ids, e)),
                }
            }
            Ok(ids)
        })
    }
}

impl Searcher for Bluesky {
    fn name(&self) -> &str {
        "bluesky"
    }

    /// `app.bsky.feed.searchPosts`, up to 100 posts, newest first.
    fn search<'a>(
        &'a mut self,
        q: &'a SearchQuery,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Found>, String>> + Send + 'a>>
    {
        Box::pin(async move {
            let term = match q {
                SearchQuery::Url(u) => u.clone(),
                SearchQuery::Text(t) => format!("\"{t}\""),
            };
            let mut out = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..4 {
                let mut params = vec![("q", term.as_str()), ("limit", "100")];
                if let Some(c) = cursor.as_deref() {
                    params.push(("cursor", c));
                }
                let v = self.xrpc_get("app.bsky.feed.searchPosts", &params).await?;
                out.extend(parse_search(&v, q));
                cursor = get(&v, "cursor").map(text);
                if cursor.is_none() {
                    break;
                }
            }
            Ok(out)
        })
    }
}

/// `searchPosts` → found posts. The author's `createdAt` gives the age.
pub fn parse_search(v: &Value, q: &SearchQuery) -> Vec<Found> {
    let subject_url = match q {
        SearchQuery::Url(u) => Some(u.as_str()),
        _ => None,
    };
    get(v, "posts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let record = get(p, "record")?;
                    let (body, urls) = post_content(record);
                    let url = match subject_url {
                        Some(want) => urls
                            .iter()
                            .find(|u| crate::observe::subject_url(u).as_deref() == Some(want))
                            .cloned()
                            .or_else(|| urls.first().cloned()),
                        None => None,
                    };
                    Some(Found {
                        platform: "bluesky".into(),
                        account_id: get(p, "author.did").map(text)?,
                        created_at: get(p, "author.createdAt")
                            .map(text)
                            .and_then(|s| parse_rfc3339(&s)),
                        at: get(record, "createdAt")
                            .map(text)
                            .and_then(|s| parse_rfc3339(&s))
                            .or_else(|| {
                                get(p, "indexedAt")
                                    .map(text)
                                    .and_then(|s| parse_rfc3339(&s))
                            })?,
                        url,
                        text_hash: Some(crate::observe::simhash::simhash(&body)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An `app.bsky.feed.post` record replying under `parent` in the thread
/// rooted at `root`, with a link facet for every URL so they are clickable.
pub fn post_record(
    text_: &str,
    root: &(String, String),
    parent: &(String, String),
    created_at: &str,
    lang: &str,
) -> Value {
    json!({
        "$type": "app.bsky.feed.post",
        "text": text_,
        "createdAt": created_at,
        "langs": [lang],
        "reply": {
            "root": { "uri": root.0, "cid": root.1 },
            "parent": { "uri": parent.0, "cid": parent.1 }
        },
        "facets": link_facets(text_),
    })
}

/// Byte-indexed link facets for each http(s) token in the text.
pub fn link_facets(text_: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut pos = 0;
    for tok in text_.split(' ') {
        let start = pos;
        pos += tok.len() + 1;
        if !(tok.starts_with("http://") || tok.starts_with("https://")) {
            continue;
        }
        let trimmed = tok.trim_end_matches(['.', ',', ')', ';']);
        out.push(json!({
            "index": { "byteStart": start, "byteEnd": start + trimmed.len() },
            "features": [ { "$type": "app.bsky.richtext.facet#link", "uri": trimmed } ]
        }));
    }
    out
}

fn now_rfc3339() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = crate::facts::date_time(ts);
    // "YYYY-MM-DD HH:MM UTC" → RFC 3339 with the seconds.
    format!("{}T{}:{:02}Z", &d[..10], &d[11..16], ts.rem_euclid(60))
}

/// A mention before its parent and quoted posts are fetched.
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    pub mention: Mention,
    pub parent_uri: Option<String>,
    pub quoted_uri: Option<String>,
    pub indexed_at: String,
    /// Post URIs that need fetching.
    pub refs: Vec<String>,
}

impl Partial {
    pub fn finish(mut self, posts: &HashMap<String, Post>) -> Mention {
        let take = |uri: &Option<String>| {
            uri.as_ref().map(|u| {
                posts.get(u).cloned().unwrap_or_else(|| Post {
                    id: u.clone(),
                    text: String::new(),
                    urls: vec![],
                })
            })
        };
        self.mention.parent = take(&self.parent_uri);
        self.mention.quoted = take(&self.quoted_uri);
        self.mention
    }
}

/// `listNotifications` → the mentions newer than `after` (an `indexedAt`),
/// oldest first. Only `reason == "mention"`: replies to the bot's own
/// posts and quotes of them are not summonses.
pub fn parse_notifications(v: &Value, after: &str) -> Vec<Partial> {
    let Some(list) = get(v, "notifications").and_then(Value::as_array) else {
        return vec![];
    };
    let mut out: Vec<Partial> = list
        .iter()
        .filter(|n| get(n, "reason").map(text).as_deref() == Some("mention"))
        .filter_map(|n| {
            let indexed_at = get(n, "indexedAt").map(text)?;
            if indexed_at.as_str() <= after {
                return None;
            }
            let record = get(n, "record")?;
            let uri = get(n, "uri").map(text)?;
            let author = get(n, "author.handle").map(text).unwrap_or_default();
            let (text_, urls) = post_content(record);
            let parent_uri = get(record, "reply.parent.uri").map(text);
            let quoted_uri = quoted_uri(record);
            let thread = Some(Thread {
                cid: get(n, "cid").map(text).unwrap_or_default(),
                root_id: get(record, "reply.root.uri").map(text).unwrap_or_default(),
                root_cid: get(record, "reply.root.cid").map(text).unwrap_or_default(),
            });
            let refs = [&parent_uri, &quoted_uri]
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            Some(Partial {
                mention: Mention {
                    id: uri,
                    platform: "bluesky".into(),
                    author,
                    at: get(record, "createdAt")
                        .map(text)
                        .and_then(|s| parse_rfc3339(&s))
                        .or_else(|| parse_rfc3339(&indexed_at))
                        .unwrap_or(0),
                    text: text_,
                    urls,
                    parent: None,
                    quoted: None,
                    thread,
                    lang: None,
                },
                parent_uri,
                quoted_uri,
                indexed_at,
                refs,
            })
        })
        .collect();
    out.sort_by(|a, b| a.indexed_at.cmp(&b.indexed_at));
    out
}

/// `getPosts` → posts by URI.
pub fn parse_posts(v: &Value) -> HashMap<String, Post> {
    get(v, "posts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let uri = get(p, "uri").map(text)?;
                    let record = get(p, "record")?;
                    let (t, urls) = post_content(record);
                    Some((
                        uri.clone(),
                        Post {
                            id: uri,
                            text: t,
                            urls,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Text plus every link the record carries: link facets in the text and
/// an external-link card, in that order.
pub fn post_content(record: &Value) -> (String, Vec<String>) {
    let t = get(record, "text").map(text).unwrap_or_default();
    let mut urls: Vec<String> = Vec::new();
    if let Some(facets) = get(record, "facets").and_then(Value::as_array) {
        for f in facets {
            for feat in get(f, "features")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if get(feat, "$type").map(text).as_deref() == Some("app.bsky.richtext.facet#link") {
                    if let Some(u) = get(feat, "uri").map(text) {
                        if !urls.contains(&u) {
                            urls.push(u);
                        }
                    }
                }
            }
        }
    }
    for path in ["embed.external.uri", "embed.media.external.uri"] {
        if let Some(u) = get(record, path).map(text) {
            if !urls.contains(&u) {
                urls.push(u);
            }
        }
    }
    (t, urls)
}

fn quoted_uri(record: &Value) -> Option<String> {
    ["embed.record.uri", "embed.record.record.uri"]
        .iter()
        .find_map(|p| get(record, p).map(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results_become_found_posts_with_ages() {
        let v = fixture("search.json");
        let q = SearchQuery::Url("https://truth-wire.example/x".into());
        let f = parse_search(&v, &q);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].account_id, "did:plc:p1");
        assert_eq!(f[0].created_at, Some(1_703_980_800));
        assert_eq!(
            f[0].url.as_deref(),
            Some("https://www.truth-wire.example/x?utm_source=a")
        );
        assert_eq!(f[0].at, 1_704_103_200);
        assert!(f[1].created_at.is_none());
        assert_eq!(
            f[0].text_hash, f[1].text_hash,
            "same wording hashes the same"
        );
    }

    #[test]
    fn records_reply_in_thread_with_link_facets() {
        let t =
            "First by X. Full record: https://log.example/v1/log/a%3Ab, and https://x.example/y.";
        let f = link_facets(t);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0]["index"]["byteStart"], 25);
        assert_eq!(
            f[0]["index"]["byteEnd"],
            25 + "https://log.example/v1/log/a%3Ab".len()
        );
        assert_eq!(f[1]["features"][0]["uri"], "https://x.example/y");
        let r = post_record(
            t,
            &("r".into(), "rc".into()),
            &("p".into(), "pc".into()),
            "2024-01-01T00:00:00Z",
            "ja",
        );
        assert_eq!(r["langs"][0], "ja");
        assert_eq!(r["reply"]["root"]["cid"], "rc");
        assert_eq!(r["reply"]["parent"]["uri"], "p");
        assert!(now_rfc3339().ends_with('Z'));
    }

    fn fixture(name: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/bluesky")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn notifications_become_mentions_with_their_posts() {
        let n = fixture("notifications.json");
        let posts = parse_posts(&fixture("posts.json"));
        let partial = parse_notifications(&n, "");
        // Four notifications: a like, a mention with a link facet, a bare
        // summons under a parent, and a quote-summons. Three mentions.
        assert_eq!(partial.len(), 3);
        assert!(partial
            .windows(2)
            .all(|w| w[0].indexed_at <= w[1].indexed_at));

        let ms: Vec<Mention> = partial.into_iter().map(|p| p.finish(&posts)).collect();
        let linked = &ms[0];
        assert_eq!(linked.author, "alice.test");
        assert_eq!(
            linked.urls,
            vec!["https://www.example.com/story?utm_source=x"]
        );
        assert_eq!(linked.at, 1_704_103_200);
        assert!(linked.parent.is_none());

        let under = &ms[1];
        assert_eq!(under.text, "@kagami.test prove it");
        let parent = under.parent.as_ref().unwrap();
        assert_eq!(parent.text, "BREAKING: the bank did the thing");
        assert_eq!(parent.urls, vec!["https://truth-wire.example/x"]);

        let quoting = &ms[2];
        let q = quoting.quoted.as_ref().unwrap();
        assert_eq!(q.id, "at://did:plc:carol/app.bsky.feed.post/q1");
        assert_eq!(q.urls, vec!["https://news.bbc.co.uk/wire/123"]);
        // A parent the fetch did not return is kept as an id with no text.
        assert_eq!(quoting.parent.as_ref().unwrap().text, "");

        let t = under.thread.as_ref().unwrap();
        assert_eq!(
            (t.cid.as_str(), t.root_id.as_str()),
            ("c2", "at://did:plc:carol/app.bsky.feed.post/p1")
        );
        assert_eq!(linked.thread.as_ref().unwrap().root_id, "");

        // Only newer than the high-water mark.
        assert_eq!(parse_notifications(&n, "2024-01-01T12:00:00.000Z").len(), 1);
    }
}
