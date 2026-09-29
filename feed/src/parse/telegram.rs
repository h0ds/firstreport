//! Public Telegram channels through their preview pages
//! (`https://t.me/s/<channel>`), which need no account and list the
//! channel's recent posts as HTML. One post becomes one entry: the
//! post URL is its identity, the first line of its text is the title,
//! the text is the summary, the post time is `published_at`. Forwards
//! are recorded as the channel's own posts; the simhash marks them as
//! the same copy as their origin when that origin is also polled.
//! Posts with no text (a bare photo or file) are skipped: there is
//! nothing to cluster on.
//!
//! Campaigns often start in channels like these and reach the press
//! later, which is why this is the first non-RSS source. Neutral by
//! construction like the rest: what was posted, by whom, when.

use super::feed::{ParsedEntry, ParsedFeed, SUMMARY_MAX_CHARS};
use super::{html, simhash, url as urlc};
use crate::types::FeedKind;

pub const TITLE_MAX_CHARS: usize = 160;

/// Whether a feed URL is a channel preview page.
pub fn is_channel_url(url: &str) -> bool {
    url::Url::parse(url).ok().is_some_and(|u| {
        u.host_str()
            .is_some_and(|h| h == "t.me" || h == "telegram.me")
            && u.path().starts_with("/s/")
    })
}

/// Attribute value `name="…"` from a tag's attribute string.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(&tag[i..j])
}

/// Inner HTML of the first `<div … class="…{class}…">` after `from`,
/// matching nested divs. Returns (inner, end offset).
fn div_with_class(html: &str, class: &str, from: usize) -> Option<(&'static str, String, usize)> {
    let lower = html.to_ascii_lowercase();
    let mut i = from;
    while let Some(p) = lower[i..].find("<div") {
        let start = i + p;
        let gt = lower[start..].find('>')? + start;
        let tag = &html[start..gt];
        if attr(tag, "class").is_some_and(|c| c.split_whitespace().any(|x| x == class)) {
            let mut depth = 1;
            let mut j = gt + 1;
            while depth > 0 {
                let next_open = lower[j..].find("<div");
                let next_close = lower[j..].find("</div>");
                match (next_open, next_close) {
                    (_, None) => return None,
                    (Some(o), Some(c)) if o < c => {
                        depth += 1;
                        j += o + 4;
                    }
                    (_, Some(c)) => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(("", html[gt + 1..j + c].to_string(), j + c + 6));
                        }
                        j += c + 6;
                    }
                }
            }
        }
        i = gt + 1;
    }
    None
}

/// `2025-08-24T14:05:00+02:00` → unix seconds. Only the shape Telegram
/// emits; anything else is 0, which the contract allows.
pub fn parse_rfc3339(s: &str) -> i64 {
    let b = s.as_bytes();
    if b.len() < 19 {
        return 0;
    }
    let num = |a: usize, l: usize| s.get(a..a + l).and_then(|x| x.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d), Some(hh), Some(mm), Some(ss)) = (
        num(0, 4),
        num(5, 2),
        num(8, 2),
        num(11, 2),
        num(14, 2),
        num(17, 2),
    ) else {
        return 0;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return 0;
    }
    // days from civil (Howard Hinnant)
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut t = days * 86_400 + hh * 3600 + mm * 60 + ss;
    let rest = &s[19..];
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(sign) = rest.chars().next() {
        if sign == '+' || sign == '-' {
            let oh = rest
                .get(1..3)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            let om = rest
                .get(4..6)
                .and_then(|x| x.parse::<i64>().ok())
                .unwrap_or(0);
            let off = oh * 3600 + om * 60;
            t -= if sign == '+' { off } else { -off };
        }
    }
    t
}

pub fn parse(body: &[u8], feed_url: &str) -> Result<ParsedFeed, super::feed::ParseError> {
    let html_text = String::from_utf8_lossy(body);
    if !html_text.contains("tgme_widget_message") {
        return Err(super::feed::ParseError(
            "not a Telegram channel preview page (no tgme_widget_message)".into(),
        ));
    }
    let base = url::Url::parse(feed_url).ok();
    let mut entries = Vec::new();
    let mut pos = 0;
    while let Some((_, msg, end)) = div_with_class(&html_text, "tgme_widget_message", pos) {
        pos = end;
        // data-post is on the message div itself, which div_with_class
        // returned the inner of; find it in the tag just before.
        let tag_start = html_text[..end].rfind(&msg).unwrap_or(end);
        let head = &html_text[..tag_start];
        let tag_open = head.rfind("<div").unwrap_or(0);
        let tag = &head[tag_open..];
        let Some(post) = attr(tag, "data-post") else {
            continue;
        };
        let Some((channel, id)) = post.split_once('/') else {
            continue;
        };
        let raw_url = format!("https://t.me/{channel}/{id}");
        let Some(canonical_url) = urlc::canonicalize(&raw_url, base.as_ref()) else {
            continue;
        };
        let Some((_, text_html, _)) = div_with_class(&msg, "tgme_widget_message_text", 0) else {
            continue;
        };
        let with_breaks = text_html
            .replace("<br/>", "\n")
            .replace("<br>", "\n")
            .replace("<br />", "\n");
        let summary = html::strip(&with_breaks, SUMMARY_MAX_CHARS);
        if summary.is_empty() {
            continue;
        }
        let first_line = with_breaks
            .split('\n')
            .map(|l| html::strip(l, TITLE_MAX_CHARS))
            .find(|l| !l.is_empty())
            .unwrap_or_default();
        let title = if first_line.chars().count() >= TITLE_MAX_CHARS {
            let mut t: String = first_line.chars().take(TITLE_MAX_CHARS - 1).collect();
            t.push('…');
            t
        } else {
            first_line
        };
        let published_at = msg
            .find("<time")
            .and_then(|i| {
                let gt = msg[i..].find('>')? + i;
                attr(&msg[i..gt], "datetime").map(parse_rfc3339)
            })
            .unwrap_or(0);
        let text_hash = simhash::simhash(&format!("{title} {summary}"));
        entries.push(ParsedEntry {
            canonical_url,
            host: "t.me".into(),
            title,
            summary,
            published_at,
            lang_hint: None,
            text_hash,
        });
    }
    Ok(ParsedFeed {
        kind: FeedKind::Telegram,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/feeds/telegram-channel.html"),
        )
        .unwrap()
    }

    #[test]
    fn channel_urls() {
        assert!(is_channel_url("https://t.me/s/examplechannel"));
        assert!(is_channel_url("https://telegram.me/s/x?before=10"));
        assert!(
            !is_channel_url("https://t.me/examplechannel"),
            "the join page, not the preview"
        );
        assert!(!is_channel_url("https://example.com/s/x"));
    }

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("2025-08-24T12:53:10+00:00"), 1_756_039_990);
        assert_eq!(
            parse_rfc3339("2025-08-24T14:05:00+02:00"),
            1_756_037_100,
            "offset applied"
        );
        assert_eq!(parse_rfc3339("2024-01-01T08:40:00Z"), 1_704_098_400);
        assert_eq!(parse_rfc3339("garbage"), 0);
    }

    #[test]
    fn posts_become_entries_and_media_only_posts_are_skipped() {
        let p = parse(&fixture(), "https://t.me/s/examplechannel").unwrap();
        assert_eq!(p.kind, FeedKind::Telegram);
        assert_eq!(p.entries.len(), 2, "photo-only and service posts skipped");
        let a = &p.entries[0];
        assert_eq!(a.canonical_url, "https://t.me/examplechannel/1201");
        assert_eq!(
            a.title,
            "BREAKING: Central bank holds rates steady, hints at cuts later this year."
        );
        assert!(
            a.summary
                .starts_with("BREAKING: Central bank holds rates steady")
                && a.summary.contains("Full statement: example.com/statement")
        );
        assert_eq!(a.published_at, 1_756_039_990);
        assert_eq!(a.host, "t.me");
        let b = &p.entries[1];
        assert_eq!(b.canonical_url, "https://t.me/examplechannel/1202");
        assert_eq!(
            b.title,
            "Coastal flooding closes two highways after overnight storm & high tide"
        );
        assert_eq!(b.published_at, 1_756_037_100);
        assert!(parse(b"<html>nope</html>", "https://t.me/s/x").is_err());
    }
}
