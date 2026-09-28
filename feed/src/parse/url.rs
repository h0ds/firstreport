//! URL canonicalization (plan §5.1). The canonical form is what gets hashed
//! into the item id, so any change here changes ids: treat it as contract.

use url::Url;

/// Query parameters that never identify content. Exact, case-insensitive
/// names plus the `utm_` prefix.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "dclid",
    "msclkid",
    "mc_cid",
    "mc_eid",
    "yclid",
    "igshid",
    "twclid",
    "ref",
    "source",
    "src",
    "ito",
    "ncid",
    "cmpid",
    "cmp",
    "ocid",
    "ftag",
    "guccounter",
    "guce_referrer",
    "guce_referrer_sig",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "s_kwcid",
    "ns_mchannel",
    "ns_source",
    "ns_campaign",
    "ns_linkname",
    "ns_fee",
    "at_medium",
    "at_campaign",
    "at_custom1",
    "at_custom2",
    "at_custom3",
    "at_custom4",
    "xtor",
    "smid",
    "smtyp",
    "partner",
    "sh",
    "srnd",
    "taid",
    "ceid",
    "oc",
];

fn is_tracking(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("utm_") || TRACKING_PARAMS.contains(&n.as_str())
}

/// AMP toggles carried as query parameters.
fn is_amp_param(name: &str, value: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let v = value.to_ascii_lowercase();
    n == "amp"
        || n == "amp_js_v"
        || n == "usqp"
        || (n == "output" && v == "amp")
        || (n == "outputtype" && v == "amp")
}

/// Canonicalize `raw`, resolving it against `base` (the feed URL) when it is
/// relative. Returns `None` for anything that is not an http(s) URL with a host.
pub fn canonicalize(raw: &str, base: Option<&Url>) -> Option<String> {
    let raw = raw.trim();
    let mut u = match Url::parse(raw) {
        Ok(u) => u,
        Err(url::ParseError::RelativeUrlWithoutBase) => base?.join(raw).ok()?,
        Err(_) => return None,
    };
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    // `www.`, AMP and mobile mirrors all serve the same article.
    let host = ["www.", "amp.", "m.", "mobile."]
        .iter()
        .find_map(|p| host.strip_prefix(p))
        .unwrap_or(&host)
        .to_string();
    if host.is_empty() {
        return None;
    }
    u.set_host(Some(&host)).ok()?;
    u.set_fragment(None);

    // `/amp` suffix variants of an article path.
    let path = u.path().to_string();
    let trimmed = path
        .strip_suffix("/amp/")
        .or_else(|| path.strip_suffix("/amp"))
        .or_else(|| path.strip_suffix(".amp"))
        .or_else(|| {
            path.strip_suffix("/amp.html")
                .map(|p| p.trim_end_matches('/'))
        });
    if let Some(t) = trimmed {
        let t = if t.is_empty() { "/" } else { t };
        u.set_path(t);
    }

    // Drop default ports.
    if matches!(
        (u.scheme(), u.port()),
        ("http", Some(80)) | ("https", Some(443))
    ) {
        u.set_port(None).ok()?;
    }

    let mut params: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, v)| !is_tracking(k) && !is_amp_param(k, v))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if params.is_empty() {
        u.set_query(None);
    } else {
        params.sort();
        let mut q = u.query_pairs_mut();
        q.clear();
        for (k, v) in &params {
            q.append_pair(k, v);
        }
        drop(q);
    }

    // Trailing slash on a bare path is noise; keep it elsewhere since some
    // sites serve different content with and without.
    let mut s = u.to_string();
    if u.path() == "/" && u.query().is_none() {
        s.pop();
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let base = Url::parse("https://www.example.com/feed.xml").unwrap();
        let cases = [
            (
                "HTTPS://WWW.Example.com/a/b#frag",
                "https://example.com/a/b",
            ),
            (
                "https://example.com/a?utm_source=x&id=2&fbclid=y",
                "https://example.com/a?id=2",
            ),
            (
                "https://example.com/a?z=1&a=2",
                "https://example.com/a?a=2&z=1",
            ),
            ("https://example.com/", "https://example.com"),
            ("https://example.com:443/x", "https://example.com/x"),
            ("/relative/path", "https://example.com/relative/path"),
            ("https://example.com/a?ref=tw", "https://example.com/a"),
            (
                "https://amp.example.com/story/amp",
                "https://example.com/story",
            ),
            (
                "https://m.example.com/story/amp/",
                "https://example.com/story",
            ),
            (
                "https://example.com/story?amp=1&id=3",
                "https://example.com/story?id=3",
            ),
            (
                "https://example.com/story?output=amp",
                "https://example.com/story",
            ),
            ("https://example.com/story.amp", "https://example.com/story"),
            ("https://mobile.example.com/amp", "https://example.com"),
        ];
        for (input, want) in cases {
            assert_eq!(
                canonicalize(input, Some(&base)).as_deref(),
                Some(want),
                "input {input}"
            );
        }
    }

    #[test]
    fn rejects_non_http() {
        assert_eq!(canonicalize("mailto:a@b.c", None), None);
        assert_eq!(canonicalize("javascript:alert(1)", None), None);
        assert_eq!(canonicalize("/rel", None), None);
        assert_eq!(canonicalize("", None), None);
    }
}
