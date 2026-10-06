//! The two servers the resolver asks. A trait so tests run on recorded
//! responses and nothing in `resolve` knows about HTTP.

use std::future::Future;

use serde_json::Value;

pub trait Api {
    /// GET a JSON document. `Ok(None)` for 404; `Err` for anything that
    /// means the server could not be asked.
    fn get(&self, url: &str) -> impl Future<Output = Result<Option<Value>, String>> + Send;
}

pub struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("truthdecay-reply/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { client })
    }
}

impl Api for Http {
    async fn get(&self, url: &str) -> Result<Option<Value>, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("GET {url}: {e}"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(format!("GET {url}: HTTP {}", resp.status()));
        }
        resp.json::<Value>()
            .await
            .map(Some)
            .map_err(|e| format!("GET {url}: {e}"))
    }
}

/// Canned responses keyed by exact URL; anything else is 404. Tests only.
#[cfg(test)]
#[derive(Default)]
pub struct Recorded {
    pub responses: std::collections::HashMap<String, Value>,
    /// URLs that fail as if the server were down.
    pub down: Vec<String>,
}

#[cfg(test)]
impl Api for Recorded {
    async fn get(&self, url: &str) -> Result<Option<Value>, String> {
        if self.down.iter().any(|d| url.starts_with(d)) {
            return Err(format!("GET {url}: connection refused"));
        }
        Ok(self.responses.get(url).cloned())
    }
}

/// Percent-encode a query value.
pub fn query(s: &str) -> String {
    crate::facts::encode(s)
}
