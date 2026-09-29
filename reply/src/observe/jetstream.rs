//! Bluesky's Jetstream: the firehose as JSON over a websocket, filtered to
//! post creates. Every post carrying a link becomes a sighting; the account
//! is hashed with the local salt before anything is written. Ages are
//! fetched later only for accounts in a burst, through the public API,
//! from an in-memory map of hash to DID that never touches disk.

use std::collections::HashMap;

use futures_util::StreamExt;
use serde_json::Value;

use crate::facts::{get, text};

use super::simhash::simhash;
use super::{account_hash, subject_url, Sighting};

pub const DEFAULT_URL: &str = "wss://jetstream2.us-east.bsky.network/subscribe";
pub const PUBLIC_API: &str = "https://public.api.bsky.app";

/// A Jetstream event → sightings (one per distinct link, or one for the
/// text alone when the text is long enough to be a claim). `None` for
/// anything that is not a new post.
pub fn sightings_from(event: &Value, salt: &str) -> Vec<Sighting> {
    if get(event, "kind").map(text).as_deref() != Some("commit")
        || get(event, "commit.operation").map(text).as_deref() != Some("create")
        || get(event, "commit.collection").map(text).as_deref() != Some("app.bsky.feed.post")
    {
        return vec![];
    }
    let Some(did) = get(event, "did").map(text) else {
        return vec![];
    };
    let Some(record) = get(event, "commit.record") else {
        return vec![];
    };
    let at = get(event, "time_us")
        .and_then(Value::as_i64)
        .map(|us| us / 1_000_000)
        .or_else(|| {
            get(record, "createdAt")
                .map(text)
                .and_then(|s| crate::facts::parse_rfc3339(&s))
        })
        .unwrap_or(0);
    let (body, urls) = crate::platform::bluesky::post_content(record);
    let account = account_hash("bluesky", &did, salt);
    let text_hash = (crate::observe::simhash::normalize(&body).len() >= 5).then(|| simhash(&body));
    let subjects: Vec<String> = urls.iter().filter_map(|u| subject_url(u)).collect();
    if subjects.is_empty() {
        return match text_hash {
            Some(h) => vec![Sighting {
                platform: "bluesky".into(),
                account,
                account_age_days: None,
                at,
                url: None,
                text_hash: Some(h),
            }],
            None => vec![],
        };
    }
    subjects
        .into_iter()
        .map(|u| Sighting {
            platform: "bluesky".into(),
            account: account.clone(),
            account_age_days: None,
            at,
            url: Some(u),
            text_hash,
        })
        .collect()
}

/// Read the stream and hand each sighting to `sink`, together with the
/// DID it came from so ages can be looked up for burst members. Returns
/// when the connection drops; the caller reconnects.
pub async fn run(
    url: &str,
    salt: &str,
    mut sink: impl FnMut(Sighting, &str) -> Result<(), String>,
) -> Result<(), String> {
    let full = format!("{url}?wantedCollections=app.bsky.feed.post");
    let (ws, _) = tokio_tungstenite::connect_async(&full)
        .await
        .map_err(|e| format!("jetstream connect: {e}"))?;
    let (_, mut read) = ws.split();
    while let Some(msg) = read.next().await {
        let msg = msg.map_err(|e| format!("jetstream read: {e}"))?;
        let Ok(t) = msg.into_text() else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&t) else {
            continue;
        };
        let did = get(&v, "did").map(text).unwrap_or_default();
        for s in sightings_from(&v, salt) {
            sink(s, &did)?;
        }
    }
    Ok(())
}

/// Account ages in days for the DIDs given, via the public API, 25 per
/// call. Missing profiles are skipped.
pub async fn ages(
    client: &reqwest::Client,
    api: &str,
    dids: &[String],
    now: i64,
) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for chunk in dids.chunks(25) {
        let params: Vec<(&str, &str)> = chunk.iter().map(|d| ("actors", d.as_str())).collect();
        let Ok(resp) = client
            .get(format!(
                "{}/xrpc/app.bsky.actor.getProfiles",
                api.trim_end_matches('/')
            ))
            .query(&params)
            .send()
            .await
        else {
            continue;
        };
        let Ok(v) = resp.json::<Value>().await else {
            continue;
        };
        for p in get(&v, "profiles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(did), Some(created)) =
                (get(p, "did").map(text), get(p, "createdAt").map(text))
            {
                if let Some(ts) = crate::facts::parse_rfc3339(&created) {
                    out.insert(did, ((now - ts).max(0) / 86_400) as u32);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jetstream_events_become_sightings() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/bluesky/jetstream.ndjson");
        let lines = std::fs::read_to_string(p).unwrap();
        let all: Vec<Sighting> = lines
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .flat_map(|v| sightings_from(&v, "salt"))
            .collect();
        // Four events: a post with two links (one tracking-parametered), a
        // long text-only post, a like, and a short post with no link.
        assert_eq!(all.len(), 3, "{all:?}");
        assert_eq!(all[0].url.as_deref(), Some("https://example.com/story"));
        assert_eq!(all[1].url.as_deref(), Some("https://other.example/b"));
        assert_eq!(all[0].account, all[1].account);
        assert_eq!(all[0].at, 1_704_103_200);
        assert!(all[2].url.is_none() && all[2].text_hash.is_some());
        assert_ne!(all[2].account, all[0].account);
    }
}
