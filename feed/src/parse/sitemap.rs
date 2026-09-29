//! Google News sitemaps as a source. Publishers without a public feed
//! (Reuters, Nikkei's Japanese edition, most large papers) publish a
//! news sitemap for crawlers: an XML list of recent articles with URL,
//! headline, publication time and language. That is exactly what feed
//! keeps and nothing more; the article page is never fetched.
//!
//! The parser is a small tag scanner over the sitemap schema rather
//! than a dependency: `<url>` entries, `<loc>`, and the `news:` block.
//! Entries without a `news:` block (section pages, plain sitemaps) are
//! skipped. Sitemap index files are not followed: point the config at
//! the news sitemap itself.
//!
//! Rules of use, enforced elsewhere: robots.txt is honoured before any
//! fetch, `Crawl-delay` stretches the interval, and the product
//! User-Agent names the project (`fetch/robots.rs`, `fetch/client.rs`).

use url::Url;

use super::feed::{ParseError, ParsedEntry, ParsedFeed, SUMMARY_MAX_CHARS};
use super::{html, simhash, url as urlc};
use crate::types::FeedKind;

/// Whether a body is a sitemap: a `<urlset` root, after any prolog.
pub fn looks_like_sitemap(body: &[u8]) -> bool {
    let head = &body[..body.len().min(2048)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}');
    let t = t.trim_start();
    let after_prolog = if let Some(i) = t.find("?>") {
        &t[i + 2..]
    } else {
        t
    };
    let mut rest = after_prolog.trim_start();
    // skip comments
    while rest.starts_with("<!--") {
        match rest.find("-->") {
            Some(i) => rest = rest[i + 3..].trim_start(),
            None => return false,
        }
    }
    rest.starts_with("<urlset") || rest.starts_with("<sitemapindex")
}

/// Text of the first `<tag>…</tag>` (or `<ns:tag>`) inside `xml`,
/// with CDATA unwrapped and entities decoded.
fn element<'a>(xml: &'a str, local: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(p) = xml[from..].find('<') {
        let start = from + p;
        let rest = &xml[start + 1..];
        let name_end = rest
            .find(|c: char| c == '>' || c.is_whitespace() || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        let local_name = name.rsplit(':').next().unwrap_or(name);
        if local_name == local && !name.starts_with('/') && !name.starts_with('!') {
            let gt = rest.find('>')? + start + 1;
            if rest[..name_end + (rest[name_end..].find('>').unwrap_or(0))].ends_with('/') {
                return Some("");
            }
            let close = format!("</{name}>");
            let end = xml[gt + 1..].find(&close)? + gt + 1;
            return Some(&xml[gt + 1..end]);
        }
        from = start + 1;
    }
    None
}

fn text(raw: &str) -> String {
    let raw = raw.trim();
    let inner = raw
        .strip_prefix("<![CDATA[")
        .and_then(|r| r.strip_suffix("]]>"))
        .unwrap_or(raw);
    html::strip(inner, 4096)
}

/// `2025-08-24T14:05:00+02:00`, `2025-08-24T12:53:10Z` or `2025-08-24`
/// → unix seconds; 0 for anything else, which the contract allows.
pub fn parse_w3c_datetime(s: &str) -> i64 {
    let s = s.trim();
    let num = |a: usize, l: usize| s.get(a..a + l).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d)) = (num(0, 4), num(5, 2), num(8, 2)) else {
        return 0;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return 0;
    }
    let (hh, mm, ss) = if s.len() >= 16 && s.as_bytes()[10] == b'T' {
        (
            num(11, 2).unwrap_or(0),
            num(14, 2).unwrap_or(0),
            num(17, 2).unwrap_or(0),
        )
    } else {
        (0, 0, 0)
    };
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut t = days * 86_400 + hh * 3600 + mm * 60 + ss;
    if s.len() > 19 {
        let tail = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
        if let Some(sign) = tail.chars().next() {
            if sign == '+' || sign == '-' {
                let oh = tail
                    .get(1..3)
                    .and_then(|x| x.parse::<i64>().ok())
                    .unwrap_or(0);
                let om = tail
                    .get(4..6)
                    .and_then(|x| x.parse::<i64>().ok())
                    .unwrap_or(0);
                let off = oh * 3600 + om * 60;
                t -= if sign == '+' { off } else { -off };
            }
        }
    }
    t
}

pub fn parse(body: &[u8], sitemap_url: &str) -> Result<ParsedFeed, ParseError> {
    let xml = String::from_utf8_lossy(body);
    if xml.contains("<sitemapindex") {
        return Err(ParseError(
            "this is a sitemap index; point the config at one of the news sitemaps it lists".into(),
        ));
    }
    if !xml.contains("<urlset") {
        return Err(ParseError("not a sitemap (no <urlset>)".into()));
    }
    let base = Url::parse(sitemap_url).ok();
    let mut entries = Vec::new();
    let mut from = 0;
    while let Some(p) = xml[from..].find("<url>") {
        let start = from + p + 5;
        let Some(e) = xml[start..].find("</url>") else {
            break;
        };
        let block = &xml[start..start + e];
        from = start + e + 6;
        let Some(news) = element(block, "news") else {
            continue;
        };
        let Some(loc) = element(block, "loc").map(text).filter(|l| !l.is_empty()) else {
            continue;
        };
        let Some(canonical_url) = urlc::canonicalize(&loc, base.as_ref()) else {
            continue;
        };
        let title = element(news, "title").map(text).unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let title: String = title.chars().take(1024).collect();
        let published_at = element(news, "publication_date")
            .map(|d| parse_w3c_datetime(&text(d)))
            .or_else(|| element(block, "lastmod").map(|d| parse_w3c_datetime(&text(d))))
            .unwrap_or(0);
        let lang_hint = element(news, "language")
            .map(text)
            .filter(|l| !l.is_empty())
            .map(|l| {
                l.split(['-', '_'])
                    .next()
                    .unwrap_or(&l)
                    .to_ascii_lowercase()
            });
        let host = Url::parse(&canonical_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let summary = String::new();
        let text_hash = simhash::simhash(&format!("{title} {summary}"));
        let _ = SUMMARY_MAX_CHARS;
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
    Ok(ParsedFeed {
        kind: FeedKind::Sitemap,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/feeds/news-sitemap.xml"),
        )
        .unwrap()
    }

    #[test]
    fn recognises_sitemaps() {
        assert!(looks_like_sitemap(&fixture()));
        assert!(looks_like_sitemap("\u{feff}<!-- x --><urlset>".as_bytes()));
        assert!(looks_like_sitemap(
            b"<?xml version=\"1.0\"?>\n<sitemapindex>"
        ));
        assert!(!looks_like_sitemap(
            b"<?xml version=\"1.0\"?><rss version=\"2.0\">"
        ));
        assert!(!looks_like_sitemap(b"<html>"));
    }

    #[test]
    fn w3c_datetimes() {
        assert_eq!(
            parse_w3c_datetime("2025-08-24T12:53:10+00:00"),
            1_756_039_990
        );
        assert_eq!(parse_w3c_datetime("2025-08-24T12:53:10Z"), 1_756_039_990);
        assert_eq!(
            parse_w3c_datetime("2025-08-24T14:05:00+02:00"),
            1_756_037_100
        );
        assert_eq!(parse_w3c_datetime("2025-08-24"), 1_755_993_600);
        assert_eq!(
            parse_w3c_datetime("2025-08-24T14:05:00.123-05:00"),
            1_756_037_100 + 7 * 3600
        );
        assert_eq!(parse_w3c_datetime("nope"), 0);
    }

    #[test]
    fn news_entries_become_items_and_plain_urls_are_skipped() {
        let p = parse(
            &fixture(),
            "https://www.example-wire.com/arc/outboundfeeds/news-sitemap/?outputType=xml",
        )
        .unwrap();
        assert_eq!(p.kind, FeedKind::Sitemap);
        assert_eq!(p.entries.len(), 3, "the about page has no news block");
        let a = &p.entries[0];
        assert_eq!(
            a.canonical_url,
            "https://example-wire.com/world/central-bank-holds-rates-2025-08-24/"
        );
        assert_eq!(
            a.title,
            "Central bank holds rates steady, signals cuts later this year"
        );
        assert_eq!(a.published_at, 1_756_039_990);
        assert_eq!(a.lang_hint.as_deref(), Some("en"));
        assert!(a.summary.is_empty());
        let b = &p.entries[1];
        assert_eq!(
            b.canonical_url, "https://example-wire.com/world/coastal-flooding-highways-2025-08-24/",
            "tracking parameter stripped"
        );
        assert_eq!(
            b.title, "Coastal flooding closes two highways after overnight storm & high tide",
            "CDATA unwrapped"
        );
        assert_eq!(b.published_at, 1_756_037_100);
        let c = &p.entries[2];
        assert_eq!(
            c.title, "日銀、政策金利を据え置き & 年内の利下げを示唆",
            "entity decoded"
        );
        assert_eq!(c.lang_hint.as_deref(), Some("ja"));
        assert_eq!(c.published_at, 1_755_993_600, "date-only publication date");
        assert!(parse(b"<sitemapindex></sitemapindex>", "https://x/s.xml")
            .unwrap_err()
            .0
            .contains("sitemap index"));
        assert!(parse(b"<rss/>", "https://x/s.xml").is_err());
    }
}
