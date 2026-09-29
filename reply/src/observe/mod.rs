//! The observer (plan §3, §7): account-level propagation of links and
//! text, recorded as sightings, and the four coordination signals over
//! them, emitted as observations. Accounts are salted hashes: an
//! observation can count them and never name one.

pub mod jetstream;
pub mod signals;
pub mod simhash;
pub mod store;

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

pub const SALT_ENV: &str = "REPLY_SALT";

/// One account posting one subject once (plan §4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sighting {
    pub platform: String,
    /// Salted hash of the account id, as 16 hex chars.
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_age_days: Option<u32>,
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_hash: Option<u64>,
}

impl Sighting {
    /// What the sighting is about: the URL, else the text hash.
    pub fn subject(&self) -> Option<String> {
        self.url
            .clone()
            .or_else(|| self.text_hash.map(|h| format!("text:{h:016x}")))
    }
}

/// `platform + account id + salt` through SipHash; the salt never leaves
/// the machine, so nobody, including Kagami, can go from hash to handle
/// without it.
pub fn account_hash(platform: &str, account_id: &str, salt: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    salt.hash(&mut h);
    platform.hash(&mut h);
    account_id.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Thresholds (plan §14). All in the open, all in config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub burst_window_secs: i64,
    pub burst_min_accounts: usize,
    pub cohort_span_days: u32,
    /// Percent.
    pub cohort_min_share: u32,
    pub movers_window_days: u32,
    pub movers_min_bursts: usize,
    pub retain_days: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            burst_window_secs: 600,
            burst_min_accounts: 20,
            cohort_span_days: 14,
            cohort_min_share: 50,
            movers_window_days: 30,
            movers_min_bursts: 5,
            retain_days: 90,
        }
    }
}

/// A link as a subject: host lowercased without `www.`, no fragment, no
/// tracking parameters. Cheaper than `feed`'s canonicalizer and applied
/// to every post on the firehose, so it only folds what is certain.
pub fn subject_url(raw: &str) -> Option<String> {
    let mut u = url::Url::parse(raw.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    u.set_host(Some(&host)).ok()?;
    u.set_fragment(None);
    let keep: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| {
            let k = k.to_ascii_lowercase();
            !(k.starts_with("utm_")
                || matches!(
                    k.as_str(),
                    "fbclid" | "gclid" | "igshid" | "mc_cid" | "mc_eid" | "ref" | "s" | "t"
                ))
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if keep.is_empty() {
        u.set_query(None);
    } else {
        let q: Vec<String> = keep.iter().map(|(k, v)| format!("{k}={v}")).collect();
        u.set_query(Some(&q.join("&")));
    }
    Some(u.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_salted_and_stable() {
        let a = account_hash("bluesky", "did:plc:x", "salt1");
        assert_eq!(a, account_hash("bluesky", "did:plc:x", "salt1"));
        assert_ne!(a, account_hash("bluesky", "did:plc:x", "salt2"));
        assert_ne!(a, account_hash("x", "did:plc:x", "salt1"));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn subject_urls_fold_tracking_only() {
        assert_eq!(
            subject_url("https://WWW.Example.com/a?utm_source=x&id=3&fbclid=q#frag").as_deref(),
            Some("https://example.com/a?id=3")
        );
        assert_eq!(
            subject_url("https://example.com/a?utm_source=x").as_deref(),
            Some("https://example.com/a")
        );
        assert!(subject_url("ftp://x/y").is_none());
        assert!(subject_url("not a url").is_none());
    }
}
