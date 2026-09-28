//! Wikipedia: Reliable sources/Perennial sources (plan §4, L2). The page
//! is one large wikitable; this parses its rendered HTML into rows, each
//! with the source's names and links, the domains listed beside it, the
//! status verdict, the discussion links, the year of the last discussion
//! and the summary. Matching rows to outlets and turning them into claims
//! is `perennial::claims`. Content is CC BY-SA 4.0; every claim links the
//! row it came from.

// Wired into `ledger import` in the next commit.
#![allow(dead_code)]

use serde::Serialize;

pub const SOURCE_ID: &str = "wikipedia_rsp";
pub const PAGE_URL: &str =
    "https://en.wikipedia.org/wiki/Wikipedia:Reliable_sources/Perennial_sources";
/// `action=render` returns the article body without the site chrome.
pub const RENDER_URL: &str =
    "https://en.wikipedia.org/w/index.php?title=Wikipedia:Reliable_sources/Perennial_sources&action=render";
pub const USER_AGENT: &str = concat!(
    "kagami-ledger/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/h0ds/kagami)"
);

/// The list's own status legend, in its own order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    GenerallyReliable,
    NoConsensus,
    GenerallyUnreliable,
    Deprecated,
    Blacklisted,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::GenerallyReliable => "Generally reliable",
            Status::NoConsensus => "No consensus",
            Status::GenerallyUnreliable => "Generally unreliable",
            Status::Deprecated => "Deprecated",
            Status::Blacklisted => "Blacklisted",
        }
    }

    /// From the status cell's text, which carries the legend word.
    pub fn from_text(t: &str) -> Option<Status> {
        let l = t.to_ascii_lowercase();
        if l.contains("blacklist") {
            Some(Status::Blacklisted)
        } else if l.contains("deprecated") {
            Some(Status::Deprecated)
        } else if l.contains("generally unreliable") {
            Some(Status::GenerallyUnreliable)
        } else if l.contains("no consensus")
            || l.contains("unclear")
            || l.contains("additional considerations")
        {
            Some(Status::NoConsensus)
        } else if l.contains("generally reliable") {
            Some(Status::GenerallyReliable)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The `id` on the row's source cell; the page anchor for it.
    pub anchor: String,
    /// Link texts in the source cell, in order: the outlet's name(s).
    pub names: Vec<String>,
    /// Absolute URLs of article links in the source cell.
    pub articles: Vec<String>,
    /// Domains listed in the source cell, lowercase, without `www.`.
    pub domains: Vec<String>,
    /// Full text of the source cell, for the scope qualifier ("politics").
    pub source_text: String,
    pub status: Status,
    pub discussions: Vec<String>,
    pub last: Option<u32>,
    pub summary: String,
}

// ---- a very small HTML toolkit; the page is well-formed MediaWiki output.

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" | "#160" => Some(' '),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tags out, entities decoded, whitespace collapsed.
pub fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                in_tag = true;
                // block-ish tags separate words
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
        let _ = chars.peek();
    }
    decode(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `(tag inner html)` for each `<tag …>…</tag>` at the top level of
/// `html`, tolerating nesting of the same tag.
fn elements<'a>(html: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = lower[i..].find(&open) {
        let start = i + p;
        // must be `<tag>` or `<tag ` not `<table` when looking for `<t`
        let after = lower.as_bytes().get(start + open.len()).copied();
        if !matches!(
            after,
            Some(b' ') | Some(b'>') | Some(b'\n') | Some(b'\t') | Some(b'/')
        ) {
            i = start + open.len();
            continue;
        }
        let Some(gt) = lower[start..].find('>') else {
            break;
        };
        let attrs_start = start + open.len();
        let inner_start = start + gt + 1;
        // find the matching close, counting nested opens
        let mut depth = 1;
        let mut j = inner_start;
        let mut end = None;
        while j < lower.len() {
            let next_open = lower[j..].find(&open).map(|x| j + x);
            let next_close = lower[j..].find(&close).map(|x| j + x);
            match (next_open, next_close) {
                (_, None) => break,
                (Some(o), Some(c)) if o < c => {
                    let a = lower.as_bytes().get(o + open.len()).copied();
                    if matches!(a, Some(b' ') | Some(b'>') | Some(b'\n') | Some(b'/')) {
                        depth += 1;
                    }
                    j = o + open.len();
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(c);
                        break;
                    }
                    j = c + close.len();
                }
            }
        }
        let Some(e) = end else { break };
        out.push((&html[attrs_start..start + gt], &html[inner_start..e]));
        i = e + close.len();
    }
    out
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find(name) {
        let at = from + p;
        let prev = if at == 0 {
            b' '
        } else {
            lower.as_bytes()[at - 1]
        };
        let before_ok =
            !(prev.is_ascii_alphanumeric() || prev == b'-' || prev == b'_' || prev == b':');
        let rest = &attrs[at + name.len()..];
        let rest_t = rest.trim_start();
        if before_ok && rest_t.starts_with('=') {
            let v = rest_t[1..].trim_start();
            let q = v.chars().next()?;
            if q == '"' || q == '\'' {
                let end = v[1..].find(q)?;
                return Some(decode(&v[1..1 + end]));
            }
            let end = v.find(|c: char| c.is_whitespace()).unwrap_or(v.len());
            return Some(decode(&v[..end]));
        }
        from = at + name.len();
    }
    None
}

fn links(html: &str) -> Vec<(String, String)> {
    elements(html, "a")
        .into_iter()
        .filter_map(|(a, inner)| {
            let href = attr(a, "href")?;
            let href = if href.starts_with('/') {
                format!("https://en.wikipedia.org{href}")
            } else {
                href
            };
            Some((href, text_of(inner)))
        })
        .collect()
}

/// Domains mentioned in a source cell: tokens that look like hosts.
pub fn domains_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in
        text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ';')
    {
        let t = tok
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_ascii_lowercase();
        let t = t.strip_prefix("www.").unwrap_or(&t).to_string();
        let looks_like_host = t.contains('.')
            && !t.starts_with('.')
            && !t.ends_with('.')
            && t.rsplit('.')
                .next()
                .map(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
                .unwrap_or(false)
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if looks_like_host && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The perennial sources table, or the first wikitable whose header has
/// a Status column.
fn find_table(html: &str) -> Option<&str> {
    let tables = elements(html, "table");
    tables
        .iter()
        .find(|(a, _)| {
            attr(a, "class")
                .map(|c| c.contains("perennial"))
                .unwrap_or(false)
        })
        .or_else(|| {
            tables
                .iter()
                .find(|(_, inner)| text_of(&inner[..inner.len().min(2000)]).contains("Status"))
        })
        .map(|(_, inner)| *inner)
}

pub fn parse(html: &str) -> Result<Vec<Row>, String> {
    let table = find_table(html).ok_or("no perennial sources table found")?;
    let mut rows = Vec::new();
    for (_, tr) in elements(table, "tr") {
        let cells = elements(tr, "td");
        if cells.len() < 5 {
            continue; // header or note row
        }
        let (src_attrs, src) = cells[0];
        let status_text = text_of(cells[1].1);
        let Some(status) = Status::from_text(&status_text) else {
            continue;
        };
        let source_links = links(src);
        let names: Vec<String> = source_links
            .iter()
            .map(|(_, t)| t.clone())
            .filter(|t| !t.is_empty())
            .collect();
        let articles: Vec<String> = source_links
            .iter()
            .map(|(h, _)| h.clone())
            .filter(|h| h.contains("/wiki/") && !h.contains("Wikipedia:") && !h.contains('#'))
            .collect();
        let source_text = text_of(src);
        let last = text_of(cells[3].1)
            .split_whitespace()
            .find_map(|t| t.parse::<u32>().ok())
            .filter(|y| (2000..2100).contains(y));
        rows.push(Row {
            anchor: attr(src_attrs, "id").unwrap_or_default(),
            names,
            articles,
            domains: domains_in(&source_text),
            source_text,
            status,
            discussions: links(cells[2].1).into_iter().map(|(h, _)| h).collect(),
            last,
            summary: text_of(cells[4].1),
        });
    }
    if rows.is_empty() {
        return Err("table found but no rows with a status parsed".into());
    }
    Ok(rows)
}

// ---- matching rows to outlets and turning them into claims

use std::collections::BTreeMap;

use crate::model::Claim;
use crate::store::FeedOutlet;

/// How a row was matched to an outlet, recorded in the claim's note so a
/// reviewer can judge the weaker matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// The row links the same Wikipedia article that Wikidata gives for the outlet.
    Article,
    /// A domain listed in the row equals one of the outlet's domains.
    Domain,
    /// The row's link text equals the outlet's name, case-insensitively.
    Name,
}

fn norm_host(h: &str) -> String {
    let h = h.trim().to_ascii_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

fn norm_name(n: &str) -> String {
    n.to_ascii_lowercase()
        .replace("the ", "")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Rows that describe `outlet`, with how each matched. `article_urls` are
/// the outlet's `wikipedia` claims from Wikidata, if any.
pub fn matches<'a>(
    rows: &'a [Row],
    outlet: &FeedOutlet,
    article_urls: &[String],
) -> Vec<(&'a Row, MatchKind)> {
    let domains: Vec<String> = outlet.domains.iter().map(|d| norm_host(d)).collect();
    let articles: Vec<String> = article_urls
        .iter()
        .map(|u| u.trim_end_matches('/').to_string())
        .collect();
    let name = norm_name(&outlet.name);
    let mut out = Vec::new();
    for r in rows {
        let kind = if r
            .articles
            .iter()
            .any(|a| articles.iter().any(|x| x == a.trim_end_matches('/')))
        {
            Some(MatchKind::Article)
        } else if r.domains.iter().any(|d| domains.contains(d)) {
            Some(MatchKind::Domain)
        } else if !name.is_empty() && r.names.iter().any(|n| norm_name(n) == name) {
            Some(MatchKind::Name)
        } else {
            None
        };
        if let Some(k) = kind {
            out.push((r, k));
        }
    }
    out
}

/// One `reliability` claim per matching row. A source with several rows
/// (Fox News: news vs politics) yields several claims; the row's own text
/// carries the scope, so nothing is merged.
pub fn claims(outlet_id: &str, matched: &[(&Row, MatchKind)], retrieved_at: i64) -> Vec<Claim> {
    matched
        .iter()
        .map(|(r, kind)| {
            let mut data: BTreeMap<String, serde_json::Value> = BTreeMap::new();
            data.insert(
                "status".into(),
                serde_json::to_value(r.status).unwrap_or_default(),
            );
            data.insert(
                "scale".into(),
                serde_json::Value::String("wikipedia_rsp".into()),
            );
            if !r.discussions.is_empty() {
                data.insert("discussions".into(), serde_json::json!(r.discussions));
            }
            if !r.summary.is_empty() {
                let summary: String = r.summary.chars().take(400).collect();
                data.insert("summary".into(), serde_json::Value::String(summary));
            }
            data.insert(
                "scope".into(),
                serde_json::Value::String(r.source_text.clone()),
            );
            let ref_url = if r.anchor.is_empty() {
                PAGE_URL.to_string()
            } else {
                format!("{PAGE_URL}#{}", r.anchor)
            };
            let note = match kind {
                MatchKind::Article => None,
                MatchKind::Domain => Some("matched by domain listed in the row".to_string()),
                MatchKind::Name => {
                    Some("matched by name only; verify the row is this outlet".to_string())
                }
            };
            Claim {
                outlet_id: outlet_id.to_string(),
                field: "reliability".into(),
                value: r.status.label().to_string(),
                data: Some(data),
                source_id: SOURCE_ID.to_string(),
                ref_url,
                retrieved_at,
                as_of: r.last.map(|y| y.to_string()),
                note,
            }
        })
        .collect()
}

/// Fetch the rendered page.
pub async fn fetch(client: &reqwest::Client) -> Result<String, String> {
    let resp = client
        .get(RENDER_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("wikipedia {status}"));
    }
    Ok(body)
}

pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .gzip(true)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/wikipedia_rsp/perennial-sample.html"),
        )
        .unwrap()
    }

    #[test]
    fn html_helpers() {
        assert_eq!(
            text_of("<b>Fox</b> News&#39;s <a href='x'>c</a>"),
            "Fox News's c"
        );
        assert_eq!(decode("a &amp; b &lt;c&gt; &#x41;"), "a & b <c> A");
        assert_eq!(
            attr(" id=\"BBC\" class='x y'", "id").as_deref(),
            Some("BBC")
        );
        assert_eq!(attr(" data-id=\"z\" id=BBC ", "id").as_deref(), Some("BBC"));
        assert_eq!(
            domains_in("BBC (bbc.co.uk, bbc.com) News."),
            vec!["bbc.co.uk", "bbc.com"]
        );
        assert!(domains_in("no hosts here. Just text").is_empty());
        let e = elements("<td>a<td>b</td></td><td>c</td>", "td");
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].1, "c");
    }

    #[test]
    fn parses_sample_rows() {
        let rows = parse(&fixture()).unwrap();
        assert_eq!(rows.len(), 5, "header and note rows skipped");
        let bbc = &rows[0];
        assert_eq!(bbc.anchor, "BBC");
        assert_eq!(bbc.names, vec!["BBC"]);
        assert_eq!(bbc.articles, vec!["https://en.wikipedia.org/wiki/BBC"]);
        assert_eq!(bbc.domains, vec!["bbc.co.uk", "bbc.com"]);
        assert_eq!(bbc.status, Status::GenerallyReliable);
        assert_eq!(bbc.discussions.len(), 2);
        assert_eq!(bbc.last, Some(2023));
        assert!(bbc.summary.starts_with("There is consensus"));

        let bb = &rows[1];
        assert_eq!(bb.status, Status::Deprecated);
        assert_eq!(bb.domains, vec!["breitbart.com"]);
        assert!(bb.summary.contains("deprecated in the 2018 RfC"));
        assert!(
            bb.summary.contains("See also: the article"),
            "links flattened to text"
        );

        assert_eq!(rows[2].status, Status::NoConsensus);
        assert!(rows[2].source_text.contains("news excluding politics"));
        assert_eq!(rows[3].status, Status::GenerallyUnreliable);
        assert_eq!(rows[3].anchor, "Fox_News_politics");
        assert_eq!(rows[4].status, Status::Blacklisted);
        assert_eq!(rows[4].last, None);
    }

    #[test]
    fn status_words() {
        assert_eq!(
            Status::from_text("Generally reliable"),
            Some(Status::GenerallyReliable)
        );
        assert_eq!(
            Status::from_text("No consensus, unclear"),
            Some(Status::NoConsensus)
        );
        assert_eq!(
            Status::from_text("Deprecated RfC"),
            Some(Status::Deprecated)
        );
        assert_eq!(Status::from_text("Blacklisted"), Some(Status::Blacklisted));
        assert_eq!(Status::from_text("Legend"), None);
    }

    fn outlet(id: &str, name: &str, domains: &[&str]) -> FeedOutlet {
        FeedOutlet {
            id: id.into(),
            name: name.into(),
            domains: domains.iter().map(|d| d.to_string()).collect(),
        }
    }

    #[test]
    fn matching_by_article_domain_and_name() {
        let rows = parse(&fixture()).unwrap();
        let m = matches(
            &rows,
            &outlet("bbc", "BBC", &["bbc.co.uk", "bbc.com"]),
            &["https://en.wikipedia.org/wiki/BBC".into()],
        );
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].1, MatchKind::Article);

        let m = matches(&rows, &outlet("foxnews", "Fox News", &["foxnews.com"]), &[]);
        assert_eq!(m.len(), 2, "both Fox rows, by domain");
        assert!(m.iter().all(|(_, k)| *k == MatchKind::Domain));

        let m = matches(
            &rows,
            &outlet("wire", "The Example Wire", &["wire.example"]),
            &[],
        );
        assert_eq!(m.len(), 1);
        assert_eq!(
            m[0].1,
            MatchKind::Name,
            "no domain listed in the row, name still matches"
        );

        assert!(matches(&rows, &outlet("npr", "NPR", &["npr.org"]), &[]).is_empty());
    }

    #[test]
    fn claims_carry_status_scope_and_provenance() {
        let rows = parse(&fixture()).unwrap();
        let m = matches(&rows, &outlet("foxnews", "Fox News", &["foxnews.com"]), &[]);
        let c = claims("foxnews", &m, 1_700_000_000);
        assert_eq!(c.len(), 2);
        assert!(c
            .iter()
            .all(|x| x.field == "reliability" && x.source_id == "wikipedia_rsp"));
        let politics = c
            .iter()
            .find(|x| x.value == "Generally unreliable")
            .unwrap();
        assert_eq!(politics.ref_url, format!("{PAGE_URL}#Fox_News_politics"));
        assert_eq!(politics.as_of.as_deref(), Some("2020"));
        let d = politics.data.as_ref().unwrap();
        assert_eq!(d["status"], "generally_unreliable");
        assert!(d["scope"]
            .as_str()
            .unwrap()
            .contains("politics and science"));
        assert!(d["discussions"].as_array().unwrap().len() == 1);
        assert!(politics.note.as_deref().unwrap().contains("domain"));
        for x in &c {
            assert!(crate::model::validate_claim(
                x,
                &[crate::model::Source {
                    id: "wikipedia_rsp".into(),
                    name: String::new(),
                    url: String::new(),
                    licence: "CC-BY-SA-4.0".into(),
                    kind: crate::model::SourceKind::Community,
                    notes: String::new(),
                    no_redistribution: false,
                }],
                None
            )
            .is_empty());
        }
    }

    #[test]
    fn missing_table_is_an_error() {
        assert!(parse("<p>nothing</p>").is_err());
    }
}
