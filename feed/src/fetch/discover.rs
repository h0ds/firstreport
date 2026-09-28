//! Feed autodiscovery: given an outlet's HTML page, find the feeds it
//! advertises via `<link rel="alternate" type="application/rss+xml">` and
//! friends, and fall back to the conventional paths.

use url::Url;

use super::opml::attrs;

pub const FEED_TYPES: &[&str] = &[
    "application/rss+xml",
    "application/atom+xml",
    "application/feed+json",
    "application/json",
];

/// Paths worth probing when a page advertises nothing.
pub const CONVENTIONAL_PATHS: &[&str] = &[
    "/feed",
    "/rss",
    "/rss.xml",
    "/feed.xml",
    "/atom.xml",
    "/index.xml",
    "/feeds/all.atom.xml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub url: String,
    pub kind: String,
    pub title: Option<String>,
}

/// Advertised feeds in document order, deduplicated, resolved against `base`.
pub fn from_html(html: &str, base: &Url) -> Vec<Discovered> {
    let lower = html.to_ascii_lowercase();
    let head_end = lower.find("</head>").unwrap_or(lower.len());
    let mut out: Vec<Discovered> = Vec::new();
    let mut from = 0;
    while let Some(pos) = lower[from..head_end.max(from)].find("<link") {
        let start = from + pos + "<link".len();
        let Some(end_rel) = html[start..].find('>') else {
            break;
        };
        let body = &html[start..start + end_rel];
        from = start + end_rel + 1;
        let a = attrs(body);
        let get = |k: &str| {
            a.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.trim().to_string())
        };
        let rel = get("rel").unwrap_or_default().to_ascii_lowercase();
        let kind = get("type").unwrap_or_default().to_ascii_lowercase();
        if !rel.split_whitespace().any(|r| r == "alternate") || !FEED_TYPES.contains(&kind.as_str())
        {
            continue;
        }
        let Some(href) = get("href") else { continue };
        let Ok(url) = base.join(&href) else { continue };
        let url = url.to_string();
        if out.iter().any(|d| d.url == url) {
            continue;
        }
        out.push(Discovered {
            url,
            kind,
            title: get("title"),
        });
    }
    out
}

/// Whether a body looks like a feed at all (cheap pre-check before parsing).
pub fn looks_like_feed(body: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&body[..body.len().min(2048)]).to_ascii_lowercase();
    head.contains("<rss")
        || head.contains("<feed")
        || head.contains("<rdf:rdf")
        || head.contains("\"version\": \"https://jsonfeed.org")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_advertised_feeds_and_ignores_the_rest() {
        let html = r#"<html><head>
          <link rel="stylesheet" href="/x.css">
          <link rel="alternate" type="application/rss+xml" title="Top" href="/rss/top.xml">
          <LINK REL="alternate" TYPE="application/atom+xml" HREF="https://cdn.example/atom">
          <link rel="alternate" type="application/rss+xml" href="/rss/top.xml">
          <link rel="alternate" type="text/html" hreflang="fr" href="/fr">
        </head><body><link rel="alternate" type="application/rss+xml" href="/body-should-be-ignored"></body></html>"#;
        let base = Url::parse("https://www.example.com/news/").unwrap();
        let d = from_html(html, &base);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].url, "https://www.example.com/rss/top.xml");
        assert_eq!(d[0].title.as_deref(), Some("Top"));
        assert_eq!(d[1].url, "https://cdn.example/atom");
        assert_eq!(d[1].kind, "application/atom+xml");
    }

    #[test]
    fn feed_sniff() {
        assert!(looks_like_feed(
            b"<?xml version=\"1.0\"?><rss version=\"2.0\">"
        ));
        assert!(looks_like_feed(
            b"<feed xmlns=\"http://www.w3.org/2005/Atom\">"
        ));
        assert!(!looks_like_feed(b"<!doctype html><html>"));
    }
}
