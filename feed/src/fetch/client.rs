//! HTTP client: conditional GET, gzip/brotli, global and per-host
//! concurrency caps, response size cap, manual redirects so a permanent
//! move is reported, `Retry-After` on 429/503, and a one-time
//! User-Agent fallback for hosts that reject the bare product token.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{
    ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION, RETRY_AFTER, USER_AGENT as UA,
};
use tokio::sync::Semaphore;

pub const USER_AGENT: &str = concat!(
    "kagami-feed/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/h0ds/kagami)"
);
/// The `Mozilla/5.0 (compatible; …)` form that crawlers use: still
/// identifies us and links to the project, but passes naive filters that
/// reject anything not starting with `Mozilla`.
pub const COMPAT_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; kagami-feed/",
    env!("CARGO_PKG_VERSION"),
    "; +https://github.com/h0ds/kagami)"
);
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REDIRECTS: usize = 5;

#[derive(Debug)]
pub struct Fetched {
    pub status: u16,
    /// `None` on 304.
    pub body: Option<Bytes>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Final URL when the feed has permanently moved (301/308 on the
    /// first hop). Temporary redirects are followed but not reported.
    pub resolved_url: Option<String>,
}

#[derive(Debug)]
pub enum FetchError {
    Http(reqwest::Error),
    Status(u16),
    /// 429 or 503 with an optional `Retry-After` in seconds.
    RateLimited {
        status: u16,
        retry_after_secs: Option<u64>,
    },
    TooLarge(usize),
    Redirect(String),
}

impl FetchError {
    pub fn status(&self) -> Option<u16> {
        match self {
            FetchError::Status(s) | FetchError::RateLimited { status: s, .. } => Some(*s),
            FetchError::Http(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            FetchError::RateLimited {
                retry_after_secs, ..
            } => *retry_after_secs,
            _ => None,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Http(e) => {
                // reqwest's Display hides the cause; the cause is the useful part.
                write!(f, "{e}")?;
                let mut src = std::error::Error::source(e);
                while let Some(s) = src {
                    write!(f, ": {s}")?;
                    src = s.source();
                }
                Ok(())
            }
            FetchError::Status(s) => write!(f, "http {s}"),
            FetchError::RateLimited {
                status,
                retry_after_secs: Some(s),
            } => {
                write!(f, "http {status}, retry after {s}s")
            }
            FetchError::RateLimited { status, .. } => write!(f, "http {status}, rate limited"),
            FetchError::TooLarge(n) => write!(f, "body {n} bytes exceeds cap"),
            FetchError::Redirect(m) => write!(f, "redirect: {m}"),
        }
    }
}
impl std::error::Error for FetchError {}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    global: Arc<Semaphore>,
    per_host_cap: usize,
    per_host: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    /// Hosts that answered 403 to the product token but not to the
    /// compatible form; they get the compatible form from then on.
    compat_hosts: Arc<Mutex<HashSet<String>>>,
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// `Retry-After` as seconds. HTTP-date forms are rare on feeds; ignored.
pub fn parse_retry_after(v: Option<&str>) -> Option<u64> {
    v?.trim().parse::<u64>().ok().map(|s| s.min(86_400))
}

impl Client {
    pub fn new(global_concurrency: usize, per_host_concurrency: usize) -> reqwest::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .gzip(true)
            .brotli(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            http,
            global: Arc::new(Semaphore::new(global_concurrency.max(1))),
            per_host_cap: per_host_concurrency.max(1),
            per_host: Arc::new(Mutex::new(HashMap::new())),
            compat_hosts: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    fn host_sem(&self, host: &str) -> Arc<Semaphore> {
        let mut m = self.per_host.lock().unwrap_or_else(|p| p.into_inner());
        m.entry(host.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_cap)))
            .clone()
    }

    fn uses_compat(&self, host: &str) -> bool {
        self.compat_hosts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(host)
    }

    pub async fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<Fetched, FetchError> {
        let _g = self.global.acquire().await.expect("semaphore closed");
        let host = host_of(url);
        let sem = self.host_sem(&host);
        let _h = sem.acquire().await.expect("semaphore closed");

        let compat = self.uses_compat(&host);
        match self.fetch_once(url, etag, last_modified, compat).await {
            Err(FetchError::Status(403)) if !compat => {
                // Some hosts reject anything that isn't browser-shaped.
                // Try the compatible form once; remember it if it helped.
                let again = self.fetch_once(url, etag, last_modified, true).await;
                if !matches!(again, Err(FetchError::Status(403))) {
                    self.compat_hosts
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(host);
                }
                again
            }
            other => other,
        }
    }

    async fn fetch_once(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        compat: bool,
    ) -> Result<Fetched, FetchError> {
        let mut current = url.to_string();
        let mut resolved_url: Option<String> = None;
        let mut permanent_chain = true;

        for hop in 0..=MAX_REDIRECTS {
            let mut req = self.http.get(&current);
            if compat {
                req = req.header(UA, COMPAT_USER_AGENT);
            }
            if let Some(e) = etag {
                req = req.header(IF_NONE_MATCH, e);
            }
            if let Some(lm) = last_modified {
                req = req.header(IF_MODIFIED_SINCE, lm);
            }
            let resp = req.send().await.map_err(FetchError::Http)?;
            let status = resp.status().as_u16();
            let hdr = |name| {
                resp.headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };

            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                if hop == MAX_REDIRECTS {
                    return Err(FetchError::Redirect("too many redirects".into()));
                }
                let loc = hdr(LOCATION)
                    .ok_or_else(|| FetchError::Redirect(format!("{status} without Location")))?;
                let next = url::Url::parse(&current)
                    .and_then(|base| base.join(&loc))
                    .map_err(|e| FetchError::Redirect(format!("bad Location {loc:?}: {e}")))?;
                permanent_chain &= matches!(status, 301 | 308);
                current = next.to_string();
                if permanent_chain {
                    resolved_url = Some(current.clone());
                }
                continue;
            }

            let new_etag = hdr(ETAG);
            let new_lm = hdr(LAST_MODIFIED);
            if status == 304 {
                return Ok(Fetched {
                    status,
                    body: None,
                    etag: new_etag,
                    last_modified: new_lm,
                    resolved_url,
                });
            }
            if status == 429 || status == 503 {
                return Err(FetchError::RateLimited {
                    status,
                    retry_after_secs: parse_retry_after(hdr(RETRY_AFTER).as_deref()),
                });
            }
            if !(200..300).contains(&status) {
                return Err(FetchError::Status(status));
            }
            if let Some(len) = resp.content_length() {
                if len as usize > MAX_BODY_BYTES {
                    return Err(FetchError::TooLarge(len as usize));
                }
            }
            let body = resp.bytes().await.map_err(FetchError::Http)?;
            if body.len() > MAX_BODY_BYTES {
                return Err(FetchError::TooLarge(body.len()));
            }
            return Ok(Fetched {
                status,
                body: Some(body),
                etag: new_etag,
                last_modified: new_lm,
                resolved_url,
            });
        }
        Err(FetchError::Redirect("too many redirects".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_seconds_only() {
        assert_eq!(parse_retry_after(Some("120")), Some(120));
        assert_eq!(parse_retry_after(Some(" 7 ")), Some(7));
        assert_eq!(
            parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            None
        );
        assert_eq!(parse_retry_after(Some("999999")), Some(86_400));
        assert_eq!(parse_retry_after(None), None);
    }

    #[test]
    fn user_agents_identify_the_project() {
        assert!(USER_AGENT.starts_with("kagami-feed/"));
        assert!(COMPAT_USER_AGENT.starts_with("Mozilla/5.0 (compatible; kagami-feed/"));
        assert!(COMPAT_USER_AGENT.contains("github.com/h0ds/kagami"));
    }
}
