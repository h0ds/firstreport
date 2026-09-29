//! HTTP client: conditional GET, gzip/brotli, global and per-host
//! concurrency caps, response size cap, manual redirects so a permanent
//! move is reported, `Retry-After` on 429/503, a one-time User-Agent
//! fallback for hosts that reject the bare product token, and the
//! publisher's robots.txt honoured on every request: a disallowed path
//! is never fetched and `Crawl-delay` spaces requests to the host.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::robots::{self, Robots};

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
/// How long a host's robots.txt is trusted before it is fetched again.
pub const ROBOTS_TTL: Duration = Duration::from_secs(24 * 3600);
/// A robots.txt that could not be read (network error, 5xx) is retried
/// sooner; meanwhile the host is treated as allowing everything, as the
/// RFC says for an unreachable file.
pub const ROBOTS_RETRY: Duration = Duration::from_secs(3600);
/// Longest `Crawl-delay` honoured; anything larger is capped so one
/// host cannot stall the poller.
pub const MAX_CRAWL_DELAY: Duration = Duration::from_secs(120);
/// The window the request budgets (docs/steward.md §5) are counted over.
pub const BUDGET_WINDOW: Duration = Duration::from_secs(3600);

/// Request budgets: how many feed requests a host, and Kagami as a
/// whole, may be sent per window. A request over budget waits for the
/// oldest one in the window to age out. 0 means no budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    pub per_host: u32,
    pub global: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            per_host: 240,
            global: 20_000,
        }
    }
}

#[derive(Default)]
struct BudgetState {
    per_host: HashMap<String, VecDeque<Instant>>,
    global: VecDeque<Instant>,
}

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
    /// The host's robots.txt disallows this path for us.
    Disallowed,
    /// The host is on the opt-out list (config/opt-out.toml).
    OptedOut,
}

impl FetchError {
    pub fn status(&self) -> Option<u16> {
        match self {
            FetchError::Status(s) | FetchError::RateLimited { status: s, .. } => Some(*s),
            FetchError::Http(e) => e.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
    /// No connection was made: the name did not resolve or the connect
    /// failed or timed out. Often a blip on our side (the machine's DNS or
    /// network) rather than the feed's, so it is retried soon.
    pub fn is_unreachable(&self) -> bool {
        matches!(self, FetchError::Http(e) if e.is_connect())
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
            FetchError::Disallowed => write!(f, "disallowed by the host's robots.txt"),
            FetchError::OptedOut => write!(f, "host asked to be left alone (config/opt-out.toml)"),
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
    /// Per host: the parsed robots.txt and when it was fetched.
    robots: Arc<Mutex<HashMap<String, RobotsEntry>>>,
    /// Per host: when the last request was sent, for Crawl-delay.
    last_request: Arc<Mutex<HashMap<String, Instant>>>,
    /// Hosts that asked to be left alone; checked before anything else.
    opt_out: Arc<Vec<String>>,
    budgets: Budgets,
    budget_window: Duration,
    budget: Arc<Mutex<BudgetState>>,
    /// Requests that waited on a budget, for `/v1/stats`.
    budget_waits: Arc<AtomicU64>,
}

#[derive(Clone)]
struct RobotsEntry {
    rules: Arc<Robots>,
    fetched: Instant,
    /// False when the file could not be read; retried after ROBOTS_RETRY.
    trusted: bool,
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
            robots: Arc::new(Mutex::new(HashMap::new())),
            last_request: Arc::new(Mutex::new(HashMap::new())),
            opt_out: Arc::new(vec![]),
            budgets: Budgets::default(),
            budget_window: BUDGET_WINDOW,
            budget: Arc::new(Mutex::new(BudgetState::default())),
            budget_waits: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Set the opt-out list (docs/steward.md §5): hosts never requested.
    pub fn with_opt_outs(mut self, hosts: Vec<String>) -> Self {
        self.opt_out = Arc::new(hosts);
        self
    }

    /// Set the request budgets (docs/steward.md §5).
    pub fn with_budgets(mut self, budgets: Budgets) -> Self {
        self.budgets = budgets;
        self
    }

    /// Shorten the budget window; for tests.
    #[cfg(test)]
    pub fn with_budget_window(mut self, window: Duration) -> Self {
        self.budget_window = window;
        self
    }

    /// Requests that had to wait for budget so far.
    pub fn budget_waits(&self) -> u64 {
        self.budget_waits.load(Ordering::Relaxed)
    }

    /// How long until this host and the whole client are under budget,
    /// or None when a request may go now; records the request when so.
    fn budget_wait(&self, host: &str, now: Instant) -> Option<Duration> {
        let window = self.budget_window;
        let mut st = self.budget.lock().unwrap_or_else(|p| p.into_inner());
        let trim = |q: &mut VecDeque<Instant>| {
            while q.front().is_some_and(|t| now.duration_since(*t) >= window) {
                q.pop_front();
            }
        };
        let until = |q: &VecDeque<Instant>, cap: u32| -> Option<Duration> {
            if cap == 0 || (q.len() as u32) < cap {
                None
            } else {
                q.front()
                    .map(|t| window.saturating_sub(now.duration_since(*t)))
            }
        };
        trim(&mut st.global);
        let mut wait = until(&st.global, self.budgets.global);
        let hq = st.per_host.entry(host.to_string()).or_default();
        trim(hq);
        if let Some(w) = until(hq, self.budgets.per_host) {
            wait = Some(wait.map_or(w, |g| g.max(w)));
        }
        if wait.is_none() {
            hq.push_back(now);
            st.global.push_back(now);
        }
        wait
    }

    /// Wait until the request fits the budgets, then count it.
    async fn honour_budget(&self, host: &str) {
        let mut waited = false;
        loop {
            match self.budget_wait(host, Instant::now()) {
                None => break,
                Some(d) => {
                    if !waited {
                        waited = true;
                        self.budget_waits.fetch_add(1, Ordering::Relaxed);
                        tracing::info!(
                            host,
                            wait_secs = d.as_secs(),
                            "request budget reached; waiting"
                        );
                    }
                    tokio::time::sleep(d.max(Duration::from_millis(10))).await;
                }
            }
        }
    }

    /// The host's rules, fetched at most once per ROBOTS_TTL. A file
    /// that answers 4xx counts as absent (everything allowed); a network
    /// error or 5xx also allows for now and is retried sooner.
    async fn robots_for(&self, site: &url::Url) -> Arc<Robots> {
        // Keyed by scheme, host and port: the origin the file governs.
        let key = format!(
            "{}://{}:{}",
            site.scheme(),
            site.host_str().unwrap_or_default(),
            site.port_or_known_default().unwrap_or(0)
        );
        if let Some(e) = self
            .robots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
            .cloned()
        {
            let ttl = if e.trusted { ROBOTS_TTL } else { ROBOTS_RETRY };
            if e.fetched.elapsed() < ttl {
                return e.rules;
            }
        }
        let mut url = site.clone();
        url.set_path("/robots.txt");
        url.set_query(None);
        url.set_fragment(None);
        let (rules, trusted) = match self.fetch_once(url.as_str(), None, None, false).await {
            Ok(Fetched {
                status: 200,
                body: Some(b),
                ..
            }) => (robots::parse(&String::from_utf8_lossy(&b)), true),
            Ok(Fetched { status, .. }) if (400..500).contains(&status) => (Robots::default(), true),
            Ok(_) => (Robots::default(), false),
            Err(FetchError::Status(s)) if (400..500).contains(&s) => (Robots::default(), true),
            Err(_) => (Robots::default(), false),
        };
        let rules = Arc::new(rules);
        self.robots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                key,
                RobotsEntry {
                    rules: rules.clone(),
                    fetched: Instant::now(),
                    trusted,
                },
            );
        rules
    }

    /// Wait out the host's Crawl-delay since our last request to it.
    async fn honour_crawl_delay(&self, host: &str, delay: Option<u64>) {
        let Some(secs) = delay else { return };
        let delay = Duration::from_secs(secs).min(MAX_CRAWL_DELAY);
        let last = self
            .last_request
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(host)
            .copied();
        if let Some(last) = last {
            let elapsed = last.elapsed();
            if elapsed < delay {
                tokio::time::sleep(delay - elapsed).await;
            }
        }
    }

    fn note_request(&self, host: &str) {
        self.last_request
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(host.to_string(), Instant::now());
    }

    /// The robots decision for a URL, for tests and for discovery.
    pub async fn allowed(&self, url: &str) -> Result<robots::Decision, FetchError> {
        let parsed =
            url::Url::parse(url).map_err(|e| FetchError::Redirect(format!("bad url: {e}")))?;
        // Before robots.txt is even read: an opted-out host gets no request.
        if super::opt_out::covers(&self.opt_out, parsed.host_str().unwrap_or_default()) {
            return Err(FetchError::OptedOut);
        }
        let rules = self.robots_for(&parsed).await;
        let path = match parsed.query() {
            Some(q) => format!("{}?{q}", parsed.path()),
            None => parsed.path().to_string(),
        };
        Ok(rules.check(robots::OUR_TOKEN, &path))
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

        let decision = self.allowed(url).await?;
        if !decision.allowed {
            return Err(FetchError::Disallowed);
        }
        self.honour_budget(&host).await;
        self.honour_crawl_delay(&host, decision.crawl_delay).await;
        self.note_request(&host);

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
