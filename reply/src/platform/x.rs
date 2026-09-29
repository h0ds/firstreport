//! X (Twitter) over API v2, the primary platform (plan §5). Mentions are
//! polled from `GET /2/users/:id/mentions`, which returns the referenced
//! tweets and authors in `includes`, so no second fetch is needed. Replies
//! go through `POST /2/tweets`. Auth is an OAuth 2.0 user token with
//! `tweet.read tweet.write users.read offline.access`; on 401 the token is
//! refreshed and the rotated refresh token is kept in `data/x_token.json`.
//! Parsing is pure and tested on recorded responses.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{Found, Listener, Posted, Poster, SearchQuery, Searcher};
use crate::facts::{get, parse_rfc3339, text};
use crate::model::{Mention, Post, Thread};

pub const DEFAULT_API: &str = "https://api.x.com";
pub const CLIENT_ID_ENV: &str = "REPLY_X_CLIENT_ID";
pub const CLIENT_SECRET_ENV: &str = "REPLY_X_CLIENT_SECRET";
pub const ACCESS_TOKEN_ENV: &str = "REPLY_X_ACCESS_TOKEN";
pub const REFRESH_TOKEN_ENV: &str = "REPLY_X_REFRESH_TOKEN";
const PAGE: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

pub struct X {
    api: String,
    client_id: String,
    client_secret: String,
    tokens: Tokens,
    token_file: PathBuf,
    client: reqwest::Client,
    /// The bot's own user id, looked up once.
    user_id: String,
    /// Newest tweet id handled; the API takes it as `since_id`.
    since_id: String,
}

impl X {
    /// Tokens come from the environment the first time and from
    /// `data/x_token.json` after the first refresh rotates them.
    pub fn from_env(api: &str, data: &std::path::Path) -> Result<Self, String> {
        let need = |k: &str| {
            std::env::var(k).map_err(|_| format!("listen on x needs {k} in the environment"))
        };
        let token_file = data.join("x_token.json");
        let tokens = crate::model::load_json::<Tokens>(&token_file).unwrap_or(Tokens {
            access_token: need(ACCESS_TOKEN_ENV)?,
            refresh_token: std::env::var(REFRESH_TOKEN_ENV).unwrap_or_default(),
        });
        let client = reqwest::Client::builder()
            .user_agent(concat!("kagami-reply/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            api: api.trim_end_matches('/').to_string(),
            client_id: std::env::var(CLIENT_ID_ENV).unwrap_or_default(),
            client_secret: std::env::var(CLIENT_SECRET_ENV).unwrap_or_default(),
            tokens,
            token_file,
            client,
            user_id: String::new(),
            since_id: String::new(),
        })
    }

    async fn refresh(&mut self) -> Result<(), String> {
        if self.tokens.refresh_token.is_empty() || self.client_id.is_empty() {
            return Err(format!(
                "access token rejected and no refresh possible: set {REFRESH_TOKEN_ENV} and {CLIENT_ID_ENV}"
            ));
        }
        let mut req = self
            .client
            .post(format!("{}/2/oauth2/token", self.api))
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", self.tokens.refresh_token.as_str()),
                ("client_id", self.client_id.as_str()),
            ]);
        if !self.client_secret.is_empty() {
            req = req.basic_auth(&self.client_id, Some(&self.client_secret));
        }
        let v: Value = req
            .send()
            .await
            .map_err(|e| format!("oauth2/token: {e}"))?
            .error_for_status()
            .map_err(|e| format!("oauth2/token: {e}"))?
            .json()
            .await
            .map_err(|e| format!("oauth2/token: {e}"))?;
        let access = get(&v, "access_token")
            .map(text)
            .ok_or("oauth2/token: no access_token")?;
        self.tokens = Tokens {
            access_token: access,
            refresh_token: get(&v, "refresh_token")
                .map(text)
                .unwrap_or_else(|| self.tokens.refresh_token.clone()),
        };
        let body = serde_json::to_string_pretty(&self.tokens).map_err(|e| e.to_string())?;
        std::fs::write(&self.token_file, body)
            .map_err(|e| format!("{}: {e}", self.token_file.display()))?;
        Ok(())
    }

    async fn call(
        &mut self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Value, String> {
        for attempt in 0..2 {
            let mut req = self
                .client
                .request(method.clone(), format!("{}{path}", self.api))
                .query(query)
                .bearer_auth(&self.tokens.access_token);
            if let Some(b) = body {
                req = req.json(b);
            }
            let resp = req.send().await.map_err(|e| format!("{path}: {e}"))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.refresh().await?;
                continue;
            }
            let status = resp.status();
            let v: Value = resp.json().await.map_err(|e| format!("{path}: {e}"))?;
            if !status.is_success() {
                return Err(format!(
                    "{path}: HTTP {status} {}",
                    get(&v, "detail")
                        .or(get(&v, "title"))
                        .map(text)
                        .unwrap_or_default()
                ));
            }
            return Ok(v);
        }
        Err(format!("{path}: unauthorized after refresh"))
    }
}

impl Listener for X {
    fn name(&self) -> &str {
        "x"
    }

    async fn poll(&mut self) -> Result<Vec<Mention>, String> {
        if self.user_id.is_empty() {
            let me = self
                .call(reqwest::Method::GET, "/2/users/me", &[], None)
                .await?;
            self.user_id = get(&me, "data.id").map(text).ok_or("users/me: no id")?;
        }
        let max = PAGE.to_string();
        let since = self.since_id.clone();
        let mut q = vec![
            ("max_results", max.as_str()),
            ("tweet.fields", "created_at,conversation_id,referenced_tweets,entities,author_id,in_reply_to_user_id"),
            ("expansions", "author_id,referenced_tweets.id,referenced_tweets.id.author_id"),
            ("user.fields", "username"),
        ];
        if !since.is_empty() {
            q.push(("since_id", since.as_str()));
        }
        let path = format!("/2/users/{}/mentions", self.user_id);
        let v = self.call(reqwest::Method::GET, &path, &q, None).await?;
        let out = parse_mentions(&v);
        if let Some(newest) = get(&v, "meta.newest_id").map(text) {
            self.since_id = newest;
        }
        Ok(out)
    }
}

impl Poster for X {
    fn name(&self) -> &str {
        "x"
    }

    fn post<'a>(
        &'a mut self,
        m: &'a Mention,
        posts: &'a [String],
        _lang: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Posted> + Send + 'a>> {
        Box::pin(async move {
            let mut parent = m.id.clone();
            let mut ids = Vec::with_capacity(posts.len());
            for text_ in posts {
                let body = json!({ "text": text_, "reply": { "in_reply_to_tweet_id": parent } });
                match self
                    .call(reqwest::Method::POST, "/2/tweets", &[], Some(&body))
                    .await
                {
                    Ok(v) => {
                        let id = get(&v, "data.id").map(text).unwrap_or_default();
                        ids.push(id.clone());
                        parent = id;
                    }
                    Err(e) => return Err((ids, e)),
                }
            }
            Ok(ids)
        })
    }
}

impl Searcher for X {
    fn name(&self) -> &str {
        "x"
    }

    /// `GET /2/tweets/search/recent`: the last seven days of posts
    /// carrying the subject, up to 100 per page, a few pages.
    fn search<'a>(
        &'a mut self,
        q: &'a SearchQuery,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Found>, String>> + Send + 'a>>
    {
        Box::pin(async move {
            let query = match q {
                SearchQuery::Url(u) => format!("url:\"{u}\" -is:retweet"),
                SearchQuery::Text(t) => format!("\"{}\" -is:retweet", t.replace('"', "")),
            };
            let mut out = Vec::new();
            let mut next: Option<String> = None;
            for _ in 0..3 {
                let mut params = vec![
                    ("query", query.as_str()),
                    ("max_results", "100"),
                    ("tweet.fields", "created_at,author_id,entities"),
                    ("expansions", "author_id"),
                    ("user.fields", "created_at"),
                ];
                if let Some(n) = next.as_deref() {
                    params.push(("next_token", n));
                }
                let v = self
                    .call(
                        reqwest::Method::GET,
                        "/2/tweets/search/recent",
                        &params,
                        None,
                    )
                    .await?;
                out.extend(parse_search(&v, q));
                next = get(&v, "meta.next_token").map(text);
                if next.is_none() {
                    break;
                }
            }
            Ok(out)
        })
    }
}

/// Recent search → found posts, with the author's creation date from `includes.users`.
pub fn parse_search(v: &Value, q: &SearchQuery) -> Vec<Found> {
    let created: HashMap<String, i64> = get(v, "includes.users")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|u| {
                    Some((
                        get(u, "id").map(text)?,
                        get(u, "created_at")
                            .map(text)
                            .and_then(|s| parse_rfc3339(&s))?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let want = match q {
        SearchQuery::Url(u) => Some(u.as_str()),
        _ => None,
    };
    get(v, "data")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let post = tweet_post(t)?;
                    let author = get(t, "author_id").map(text)?;
                    let url = match want {
                        Some(w) => post
                            .urls
                            .iter()
                            .find(|u| crate::observe::subject_url(u).as_deref() == Some(w))
                            .cloned()
                            .or_else(|| post.urls.first().cloned()),
                        None => None,
                    };
                    Some(Found {
                        platform: "x".into(),
                        created_at: created.get(&author).copied(),
                        account_id: author,
                        at: get(t, "created_at")
                            .map(text)
                            .and_then(|s| parse_rfc3339(&s))?,
                        url,
                        text_hash: Some(crate::observe::simhash::simhash(&post.text)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /2/users/:id/mentions` → mentions, oldest first. The parent
/// (`replied_to`) and quoted tweets come from `includes.tweets`; a
/// referenced tweet the API did not include is kept as an id with no text.
pub fn parse_mentions(v: &Value) -> Vec<Mention> {
    let users: HashMap<String, String> = get(v, "includes.users")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|u| Some((get(u, "id").map(text)?, get(u, "username").map(text)?)))
                .collect()
        })
        .unwrap_or_default();
    let included: HashMap<String, Post> = get(v, "includes.tweets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| tweet_post(t).map(|p| (p.id.clone(), p)))
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<Mention> = get(v, "data")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let post = tweet_post(t)?;
                    let author = get(t, "author_id")
                        .map(text)
                        .and_then(|id| users.get(&id).cloned())
                        .unwrap_or_default();
                    let refs = get(t, "referenced_tweets").and_then(Value::as_array);
                    let find = |kind: &str| {
                        refs.and_then(|r| {
                            r.iter()
                                .find(|x| get(x, "type").map(text).as_deref() == Some(kind))
                                .and_then(|x| get(x, "id").map(text))
                        })
                    };
                    let take = |id: Option<String>| {
                        id.map(|id| {
                            included.get(&id).cloned().unwrap_or(Post {
                                id,
                                text: String::new(),
                                urls: vec![],
                            })
                        })
                    };
                    Some(Mention {
                        id: post.id.clone(),
                        platform: "x".into(),
                        author,
                        at: get(t, "created_at")
                            .map(text)
                            .and_then(|s| parse_rfc3339(&s))
                            .unwrap_or(0),
                        text: post.text,
                        urls: post.urls,
                        parent: take(find("replied_to")),
                        quoted: take(find("quoted")),
                        thread: Some(Thread {
                            cid: String::new(),
                            root_id: get(t, "conversation_id").map(text).unwrap_or_default(),
                            root_cid: String::new(),
                        }),
                        lang: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Tweet ids are snowflakes: numeric order is time order.
    out.sort_by_key(|m| m.id.parse::<u64>().unwrap_or(0));
    out
}

/// A tweet object → a Post with its links expanded past t.co.
fn tweet_post(t: &Value) -> Option<Post> {
    let id = get(t, "id").map(text)?;
    let mut urls = Vec::new();
    for u in get(t, "entities.urls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let expanded = get(u, "unwound_url")
            .or(get(u, "expanded_url"))
            .or(get(u, "url"))
            .map(text);
        if let Some(x) = expanded {
            // A quoted or attached tweet shows up as a link to itself; skip it.
            if x.contains("/status/") && (x.contains("x.com/") || x.contains("twitter.com/")) {
                continue;
            }
            if !urls.contains(&x) {
                urls.push(x);
            }
        }
    }
    Some(Post {
        id,
        text: get(t, "text").map(text).unwrap_or_default(),
        urls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results_become_found_posts_with_ages() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/x/search.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let q = SearchQuery::Url("https://truth-wire.example/x".into());
        let f = parse_search(&v, &q);
        assert_eq!(f.len(), 2);
        assert_eq!(
            (f[0].account_id.as_str(), f[0].created_at),
            ("u9", Some(1_703_980_800))
        );
        assert_eq!(
            f[0].url.as_deref(),
            Some("https://truth-wire.example/x?utm_source=a")
        );
        assert_eq!(f[1].created_at, None);
    }

    #[test]
    fn mentions_carry_parents_quotes_and_expanded_links() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/x/mentions.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let ms = parse_mentions(&v);
        assert_eq!(ms.len(), 3);
        assert!(ms.windows(2).all(|w| w[0].id < w[1].id));

        let linked = &ms[0];
        assert_eq!(
            (linked.id.as_str(), linked.author.as_str()),
            ("1001", "alice")
        );
        assert_eq!(
            linked.urls,
            vec!["https://www.example.com/story?utm_source=x"]
        );
        assert_eq!(linked.at, 1_704_103_200);
        assert!(linked.parent.is_none());
        assert_eq!(linked.thread.as_ref().unwrap().root_id, "1001");

        let under = &ms[1];
        assert_eq!(under.text, "@kagami prove it");
        let parent = under.parent.as_ref().unwrap();
        assert_eq!(parent.id, "990");
        assert_eq!(parent.urls, vec!["https://truth-wire.example/x"]);

        let quoting = &ms[2];
        assert_eq!(
            quoting.quoted.as_ref().unwrap().urls,
            vec!["https://news.bbc.co.uk/wire/123"]
        );
        assert!(
            quoting.urls.is_empty(),
            "the quote's own status link is not a subject"
        );
        assert_eq!(
            quoting.parent.as_ref().unwrap().text,
            "",
            "a parent not in includes keeps only its id"
        );
    }
}
