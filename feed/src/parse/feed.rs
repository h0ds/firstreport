//! feed-rs wrapper: bytes → `ParsedEntry`s with cleaned text and a canonical
//! URL. Everything downstream of the network and upstream of the store.

use url::Url;

use super::{html, simhash, url as urlc};
use crate::types::FeedKind;

pub const SUMMARY_MAX_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEntry {
    pub canonical_url: String,
    pub host: String,
    pub title: String,
    pub summary: String,
    /// Unix seconds; 0 if the feed gave nothing usable.
    pub published_at: i64,
    /// Feed- or entry-declared language, lowercase ISO 639-1, if any.
    pub lang_hint: Option<String>,
    pub text_hash: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedFeed {
    pub kind: FeedKind,
    pub entries: Vec<ParsedEntry>,
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ParseError {}

pub fn parse(body: &[u8], feed_url: &str) -> Result<ParsedFeed, ParseError> {
    let base = Url::parse(feed_url).ok();
    let parsed = feed_rs::parser::Builder::new()
        .base_uri(Some(feed_url))
        .build()
        .parse(body)
        .map_err(|e| ParseError(e.to_string()))?;

    let kind = match parsed.feed_type {
        feed_rs::model::FeedType::Atom => FeedKind::Atom,
        feed_rs::model::FeedType::JSON => FeedKind::Json,
        _ => FeedKind::Rss,
    };
    let feed_lang = parsed.language.as_deref().map(norm_lang);

    let mut entries = Vec::with_capacity(parsed.entries.len());
    for e in parsed.entries {
        let link = pick_link(&e.links).or_else(|| {
            // Some RSS feeds put the URL in <guid isPermaLink="true">, which
            // feed-rs surfaces as the id.
            e.id.starts_with("http").then_some(e.id.as_str())
        });
        let Some(link) = link else { continue };
        let Some(canonical_url) = urlc::canonicalize(link, base.as_ref()) else {
            continue;
        };
        let host = Url::parse(&canonical_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();

        let title = html::strip(
            e.title.as_ref().map(|t| t.content.as_str()).unwrap_or(""),
            1024,
        );
        if title.is_empty() {
            continue;
        }
        let raw_summary = e
            .summary
            .as_ref()
            .map(|t| t.content.as_str())
            .or_else(|| e.content.as_ref().and_then(|c| c.body.as_deref()))
            .unwrap_or("");
        let summary = html::strip(raw_summary, SUMMARY_MAX_CHARS);
        let published_at = e
            .published
            .or(e.updated)
            .map(|d| d.timestamp())
            .unwrap_or(0);
        let lang_hint = e
            .language
            .as_deref()
            .map(norm_lang)
            .or_else(|| feed_lang.clone());
        let text_hash = simhash::simhash(&format!("{title} {summary}"));

        entries.push(ParsedEntry {
            canonical_url,
            host,
            title,
            summary,
            published_at,
            lang_hint,
            text_hash,
        });
    }
    Ok(ParsedFeed { kind, entries })
}

fn pick_link(links: &[feed_rs::model::Link]) -> Option<&str> {
    links
        .iter()
        .find(|l| matches!(l.rel.as_deref(), Some("alternate")) && is_html(l))
        .or_else(|| links.iter().find(|l| l.rel.is_none()))
        .or_else(|| links.iter().find(|l| is_html(l)))
        .or_else(|| links.first())
        .map(|l| l.href.as_str())
}

fn is_html(l: &feed_rs::model::Link) -> bool {
    l.media_type
        .as_deref()
        .map(|m| m.contains("html"))
        .unwrap_or(true)
}

/// `en-GB` → `en`.
fn norm_lang(s: &str) -> String {
    s.split(['-', '_']).next().unwrap_or(s).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>T</title><link>https://www.example.com/</link><language>en-gb</language>
<item><title>First &amp; foremost</title><link>https://www.example.com/a?utm_source=rss</link>
<description><![CDATA[<p>Hello <b>there</b></p>]]></description><pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate></item>
<item><title>Relative</title><link>/b</link></item>
<item><title>No link</title></item>
</channel></rss>"#;

    #[test]
    fn parses_rss_with_cleanup() {
        let f = parse(RSS.as_bytes(), "https://www.example.com/feed.xml").unwrap();
        assert_eq!(f.kind, FeedKind::Rss);
        assert_eq!(f.entries.len(), 2);
        let a = &f.entries[0];
        assert_eq!(a.canonical_url, "https://example.com/a");
        assert_eq!(a.host, "example.com");
        assert_eq!(a.title, "First & foremost");
        assert_eq!(a.summary, "Hello there");
        assert_eq!(a.published_at, 1_704_067_200);
        assert_eq!(a.lang_hint.as_deref(), Some("en"));
        assert_ne!(a.text_hash, 0);
        assert_eq!(f.entries[1].canonical_url, "https://example.com/b");
        assert_eq!(f.entries[1].published_at, 0);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse(b"not a feed", "https://x.y/f").is_err());
    }
}
