//! The observer (plan §3, §7): account-level propagation of a subject,
//! gathered on demand by searching the platform when a mention names it,
//! recorded as sightings, and the four coordination signals over them,
//! emitted as observations. Accounts are salted hashes: an observation
//! can count them and never name one.

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
/// to every post a search returns, so it only folds what is certain.
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

/// Observe one subject on demand: search the platform for posts carrying
/// it, record them as sightings, run the signals over the retained window
/// so the cohort baseline covers every account seen so far, and return the
/// observations about this subject. `now` is passed in for determinism.
pub async fn observe_subject(
    searcher: &mut dyn crate::platform::Searcher,
    store: &store::Store,
    salt: &str,
    subject_url: Option<&str>,
    subject_text: Option<&str>,
    t: Thresholds,
    now: i64,
) -> Result<Vec<crate::model::Observation>, String> {
    let query = match (subject_url.and_then(subject_url_fn), subject_text) {
        (Some(u), _) => crate::platform::SearchQuery::Url(u),
        (None, Some(text)) => crate::platform::SearchQuery::Text(text.to_string()),
        (None, None) => return Ok(vec![]),
    };
    let found = searcher.search(&query).await?;
    let mut fresh = Vec::with_capacity(found.len());
    for f in found {
        let s = Sighting {
            platform: f.platform,
            account: account_hash(searcher.name(), &f.account_id, salt),
            account_age_days: f.created_at.map(|c| ((now - c).max(0) / 86_400) as u32),
            at: f.at,
            url: f.url.as_deref().and_then(subject_url_fn),
            text_hash: f.text_hash,
        };
        store.append(&s)?;
        fresh.push(s);
    }
    store.prune(now, t.retain_days)?;
    let since = now - t.retain_days as i64 * 86_400;
    let window = store.load(since)?;
    let (obs, _) = signals::run(&window, t, now);
    let mine: Vec<String> = query.subjects();
    let obs: Vec<_> = obs
        .into_iter()
        .filter(|o| o.kind != "first_movers" && mine.contains(&o.subject))
        .collect();
    store.append_observations(&obs)?;
    Ok(obs)
}

fn subject_url_fn(u: &str) -> Option<String> {
    subject_url(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Found, SearchQuery, Searcher};

    /// A platform whose search returns a burst of the same wording by
    /// young accounts for one subject and nothing for any other.
    struct Fake;
    impl Searcher for Fake {
        fn name(&self) -> &str {
            "t"
        }
        fn search<'a>(
            &'a mut self,
            q: &'a SearchQuery,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<Found>, String>> + Send + 'a>,
        > {
            Box::pin(async move {
                let SearchQuery::Url(u) = q else {
                    return Ok(vec![]);
                };
                if u != "https://truth-wire.example/x" {
                    return Ok(vec![]);
                }
                let now = 1_704_200_000;
                Ok((0..25)
                    .map(|i| Found {
                        platform: "t".into(),
                        account_id: format!("acct{i}"),
                        created_at: Some(now - (3 + i % 6) * 86_400),
                        at: now - 600 + i * 9,
                        url: Some("https://www.truth-wire.example/x?utm_source=q".into()),
                        text_hash: Some(simhash::simhash(
                            "BREAKING the bank did the thing and nobody says so",
                        )),
                    })
                    .collect())
            })
        }
    }

    #[tokio::test]
    async fn observes_a_subject_on_demand() {
        let dir = std::env::temp_dir().join(format!("kg-reply-od-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = store::Store::open(&dir).unwrap();
        let t = Thresholds::default();
        let obs = observe_subject(
            &mut Fake,
            &store,
            "salt",
            Some("https://WWW.truth-wire.example/x?utm_source=z"),
            None,
            t,
            1_704_200_000,
        )
        .await
        .unwrap();
        let kinds: Vec<&str> = obs.iter().map(|o| o.kind.as_str()).collect();
        assert_eq!(kinds, vec!["burst", "syndicated_text", "cohort"], "{obs:?}");
        assert!(obs
            .iter()
            .all(|o| o.subject == "https://truth-wire.example/x"));
        assert_eq!(obs[0].count, 25);
        // Sightings were written with hashed accounts and ages, never ids.
        let written = store.load(0).unwrap();
        assert_eq!(written.len(), 25);
        assert!(written
            .iter()
            .all(|s| s.account.len() == 16 && !s.account.contains("acct")));
        assert_eq!(written[0].account_age_days, Some(3));
        // A subject nobody posted: no observations, nothing written.
        let none = observe_subject(
            &mut Fake,
            &store,
            "salt",
            Some("https://quiet.example/"),
            None,
            t,
            1_704_200_000,
        )
        .await
        .unwrap();
        assert!(none.is_empty());
        assert_eq!(store.observations().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
