//! Bluesky over XRPC. Mentions come from `app.bsky.notification.listNotifications`,
//! polled; the firehose is for the observer (R4), not for being summoned.
//! Parent and quoted posts are fetched with `app.bsky.feed.getPosts`.
//! Parsing is pure and tested on recorded responses.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::Listener;
use crate::facts::{get, parse_rfc3339, text};
use crate::model::{Mention, Post};

pub const DEFAULT_PDS: &str = "https://bsky.social";
pub const PASSWORD_ENV: &str = "REPLY_BSKY_PASSWORD";
const PAGE: usize = 50;

pub struct Bluesky {
    pds: String,
    handle: String,
    password: String,
    client: reqwest::Client,
    access: Option<String>,
    /// Newest `indexedAt` handled; older notifications are not re-read.
    high_water: String,
}

impl Bluesky {
    /// `password` is an app password, read from `REPLY_BSKY_PASSWORD` by the caller.
    pub fn new(pds: &str, handle: &str, password: &str) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("kagami-reply/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            pds: pds.trim_end_matches('/').to_string(),
            handle: handle.to_string(),
            password: password.to_string(),
            client,
            access: None,
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
        if self.access.is_none() {
            return Err("createSession: no accessJwt in response".into());
        }
        Ok(())
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
fn post_content(record: &Value) -> (String, Vec<String>) {
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

        // Only newer than the high-water mark.
        assert_eq!(parse_notifications(&n, "2024-01-01T12:00:00.000Z").len(), 1);
    }
}
