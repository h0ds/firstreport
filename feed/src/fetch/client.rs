//! HTTP client: conditional GET, gzip/brotli, global and per-host
//! concurrency caps, response size cap.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use tokio::sync::Semaphore;

pub const USER_AGENT: &str = concat!(
    "kagami-feed/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/h0ds/kagami)"
);
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct Fetched {
    pub status: u16,
    /// `None` on 304.
    pub body: Option<Bytes>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug)]
pub enum FetchError {
    Http(reqwest::Error),
    Status(u16),
    TooLarge(usize),
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
            FetchError::TooLarge(n) => write!(f, "body {n} bytes exceeds cap"),
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
}

impl Client {
    pub fn new(global_concurrency: usize, per_host_concurrency: usize) -> reqwest::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .gzip(true)
            .brotli(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self {
            http,
            global: Arc::new(Semaphore::new(global_concurrency.max(1))),
            per_host_cap: per_host_concurrency.max(1),
            per_host: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn host_sem(&self, url: &str) -> Arc<Semaphore> {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let mut m = self.per_host.lock().unwrap_or_else(|p| p.into_inner());
        m.entry(host)
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_cap)))
            .clone()
    }

    pub async fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<Fetched, FetchError> {
        let _g = self.global.acquire().await.expect("semaphore closed");
        let host = self.host_sem(url);
        let _h = host.acquire().await.expect("semaphore closed");

        let mut req = self.http.get(url);
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
        let new_etag = hdr(ETAG);
        let new_lm = hdr(LAST_MODIFIED);

        if status == 304 {
            return Ok(Fetched {
                status,
                body: None,
                etag: new_etag,
                last_modified: new_lm,
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
        Ok(Fetched {
            status,
            body: Some(body),
            etag: new_etag,
            last_modified: new_lm,
        })
    }
}
