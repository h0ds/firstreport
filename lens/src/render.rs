//! The HTML skeleton, the two components (cluster card, claim card
//! with its provenance line) and the Markdown helpers. Design
//! §5, §10 and §11 in code: every page has the same chrome, every
//! fact has its provenance line, every page links its twins.

use std::fmt::Write as _;

use crate::input::{Claim, Cluster, Input, Source};

pub const TOKENS_CSS: &str = include_str!("tokens.css");
pub const LENS_CSS: &str = include_str!("lens.css");
const DAY: i64 = 86_400;

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Unix seconds → `YYYY-MM-DD` (civil-from-days, Howard Hinnant).
pub fn date(ts: i64) -> String {
    let z = ts.div_euclid(DAY) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn hhmm(ts: i64) -> String {
    let s = ts.rem_euclid(DAY);
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

/// `2026-09-22 14:02 UTC`.
pub fn datetime(ts: i64) -> String {
    format!("{} {} UTC", date(ts), hhmm(ts))
}

/// RFC 3339 for `datetime=` attributes and JSON-LD.
pub fn iso(ts: i64) -> String {
    format!("{}T{}:{:02}Z", date(ts), hhmm(ts), ts.rem_euclid(60))
}

pub fn time_el(ts: i64) -> String {
    format!(
        "<time datetime=\"{}\">{}</time>",
        iso(ts),
        esc(&datetime(ts))
    )
}

pub fn n(k: usize, word: &str) -> String {
    if k == 1 {
        format!("{k} {word}")
    } else if let Some(stem) = word.strip_suffix('y') {
        format!("{k} {stem}ies")
    } else {
        format!("{k} {word}s")
    }
}

/// `+40m`, `+3.5h`, `+2d` after a reference time.
pub fn delta(secs: i64) -> String {
    if secs < 3600 {
        format!("+{}m", secs / 60)
    } else if secs < DAY {
        format!("+{:.1}h", secs as f64 / 3600.0)
    } else {
        format!("+{:.1}d", secs as f64 / DAY as f64)
    }
}

pub struct Page<'a> {
    pub title: &'a str,
    pub description: &'a str,
    /// Site-relative directory of this page, e.g. `/story/123/`.
    pub path: &'a str,
    pub wide: bool,
    /// Extra `<head>` content (JSON-LD).
    pub head: &'a str,
    pub body: &'a str,
    /// Right-hand line in the chrome.
    pub status: &'a str,
}

pub fn html(p: &Page) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Kagami</title>
<meta name="description" content="{description}">
<link rel="stylesheet" href="/tokens.css">
<link rel="stylesheet" href="/lens.css">
<link rel="alternate" type="application/json" href="{path}index.json" title="This page as JSON">
<link rel="alternate" type="text/markdown" href="{path}index.md" title="This page as Markdown">
<link rel="alternate" type="text/plain" href="/llms.txt" title="How to read this site">
{head}</head>
<body>
<main{wide}>
<header class="chrome">
  <span class="kg-lockup"><a class="kg-wordmark" href="/">Kagami<span class="kanji">鏡</span></a><span class="rule"></span><span class="tool">lens</span></span>
  <span class="kg-muted">{status}</span>
</header>
{body}
<footer class="kg-muted">Every number on this page is a count from Kagami feed; every label is a source's, with its licence and date. Nothing here rates an outlet. <a href="/search/">Search</a> · <a href="/about/">About</a> · Machine readers: <a href="{path}index.json">JSON</a> · <a href="{path}index.md">Markdown</a> · <a href="/llms.txt">llms.txt</a></footer>
</main>
</body>
</html>
"#,
        title = esc(p.title),
        description = esc(p.description),
        path = esc(p.path),
        head = p.head,
        wide = if p.wide { " class=\"wide\"" } else { "" },
        status = p.status,
        body = p.body,
    )
}

pub fn outlet_link(input: &Input, id: &str) -> String {
    format!(
        "<a href=\"/outlet/{}/\" data-outlet=\"{}\">{}</a>",
        esc(id),
        esc(id),
        esc(input.outlet_name(id))
    )
}

pub fn spark(points: &[u32]) -> String {
    let max = points.iter().copied().max().unwrap_or(0).max(1) as f64;
    let step = 80.0 / (points.len().max(2) - 1) as f64;
    let pts: Vec<String> = points
        .iter()
        .enumerate()
        .map(|(i, v)| {
            format!(
                "{:.1},{:.1}",
                i as f64 * step,
                20.0 - 18.0 * (*v as f64 / max)
            )
        })
        .collect();
    let line = format!("M{}", pts.join(" L"));
    format!(
        "<svg class=\"kg-spark\" viewBox=\"0 0 80 20\" preserveAspectRatio=\"none\" role=\"img\" aria-label=\"Outlets over the first day, two-hour steps: {}\"><path class=\"area\" d=\"{line} L80,20 L0,20 Z\"/><path d=\"{line}\"/></svg>",
        points
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The cluster card (design §5). `outlets_24h` is the derived count.
pub fn cluster_card(input: &Input, c: &Cluster, spark_points: &[u32], enter: bool) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<article class=\"kg-cluster{}\" data-id=\"{}\"><p class=\"title\"><a href=\"/story/{}/\">{}</a></p><div class=\"meta kg-muted\">",
        if enter { " kg-enter" } else { "" },
        c.id,
        c.id,
        esc(&c.primary_title)
    );
    let _ = write!(
        s,
        "<span>{}</span><span class=\"sep\">·</span><span>{}</span><span class=\"sep\">·</span><span>first: {}</span><span class=\"sep\">·</span><span>first seen {}</span>",
        n(c.outlet_count as usize, "outlet"),
        n(c.item_count as usize, "item"),
        outlet_link(input, &c.origin_outlet_id),
        time_el(c.created_at)
    );
    if !c.syndication.is_empty() {
        let _ = write!(
            s,
            "<span class=\"sep\">·</span><span>{}</span>",
            n(c.syndication.len(), "syndication group")
        );
    }
    s.push_str(&spark(spark_points));
    s.push_str("</div></article>");
    s
}

/// Field order and labels shared with ledger's lookup page and
/// Markdown twin, so an outlet reads the same everywhere.
pub const FIELD_ORDER: &[(&str, &str)] = &[
    ("reliability", "Reliability"),
    ("bias", "Bias"),
    ("owner", "Owner"),
    ("parent", "Parent organisation"),
    ("legal_form", "Legal form"),
    ("country", "Country"),
    ("founded", "Founded"),
    ("website", "Website"),
    ("domain_registered", "Domain registered"),
    ("registrar", "Registrar"),
    ("site_https", "HTTPS"),
    ("site_pages", "Accountability pages"),
    ("posting_cadence", "Posting cadence"),
    ("origin_share", "First to shared stories"),
    ("syndication_share", "Syndication"),
    ("revision_rate", "Revisions"),
    ("withdrawal_rate", "Withdrawals"),
    ("wikipedia", "Wikipedia"),
    ("name", "Name in source"),
    ("wikidata_id", "Wikidata"),
];

pub fn field_label(f: &str) -> String {
    FIELD_ORDER
        .iter()
        .find(|(k, _)| *k == f)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| {
            let mut c = f.chars();
            match c.next() {
                Some(x) => x.to_uppercase().collect::<String>() + &c.as_str().replace('_', " "),
                None => String::new(),
            }
        })
}

pub fn field_rank(f: &str) -> usize {
    FIELD_ORDER
        .iter()
        .position(|(k, _)| *k == f)
        .unwrap_or(usize::MAX)
}

pub struct Freshness {
    pub age_days: i64,
    pub stale: bool,
}

pub fn freshness(c: &Claim, source: Option<&Source>, now: i64) -> Freshness {
    let age_days = (now - c.retrieved_at).max(0) / DAY;
    let stale = source.is_some_and(|s| s.refresh_days > 0 && age_days > s.refresh_days as i64);
    Freshness { age_days, stale }
}

/// The claim card with its provenance line (design §5), one form
/// everywhere. Order: source · licence · retrieved · as of · status.
pub fn claim_card(input: &Input, c: &Claim) -> String {
    let source = input.sources.get(&c.source_id);
    let (sname, licence) = source
        .map(|s| (s.name.as_str(), s.licence.as_str()))
        .unwrap_or((c.source_id.as_str(), "unknown"));
    let f = freshness(c, source, input.export_time);
    let d = c.data.as_ref();
    let get = |k: &str| d.and_then(|d| d.get(k));
    let mut s = String::new();
    let _ = write!(
        s,
        "<article class=\"kg-claim\" data-outlet=\"{}\" data-field=\"{}\" data-source=\"{}\"><div class=\"value\">{}</div>",
        esc(&c.outlet_id),
        esc(&c.field),
        esc(&c.source_id),
        esc(&c.value)
    );
    if let Some(scope) = get("scope")
        .and_then(|v| v.as_str())
        .filter(|x| *x != c.value)
    {
        let _ = write!(s, "<div class=\"scope\">{}</div>", esc(scope));
    }
    let _ = write!(
        s,
        "<div class=\"kg-provenance\"><a href=\"{}\" rel=\"noopener\">{}</a><span class=\"sep\">·</span><span class=\"kg-tag\">{}</span><span class=\"sep\">·</span><span>retrieved <time datetime=\"{}\">{}</time></span>",
        esc(&c.ref_url),
        esc(sname),
        esc(licence),
        date(c.retrieved_at),
        date(c.retrieved_at)
    );
    if let Some(a) = &c.as_of {
        let _ = write!(
            s,
            "<span class=\"sep\">·</span><span>as of {}</span>",
            esc(a)
        );
    }
    let _ = write!(
        s,
        "<span class=\"sep\">·</span><span class=\"kg-status {}\"><span>{}</span></span></div>",
        if f.stale { "warn" } else { "ok" },
        if f.stale {
            format!("stale, {} days old", f.age_days)
        } else {
            format!("{} days old", f.age_days)
        }
    );
    if let Some(w) = get("window_days").and_then(|v| v.as_i64()) {
        let _ = write!(
            s,
            "<div class=\"note\">Measured over {w} days to {}, {}. A measurement, not a verdict.</div>",
            esc(get("window_end").and_then(|v| v.as_str()).unwrap_or("?")),
            n(get("items").and_then(|v| v.as_u64()).unwrap_or(0) as usize, "item")
        );
    }
    if let Some(sum) = get("summary").and_then(|v| v.as_str()) {
        let _ = write!(s, "<div class=\"note\">{}</div>", esc(sum));
    }
    if c.field == "site_pages" {
        let links: Vec<String> = [
            "about",
            "contact",
            "privacy",
            "corrections",
            "ethics",
            "masthead",
        ]
        .iter()
        .filter_map(|k| get(k).and_then(|v| v.as_str()).map(|u| (k, u)))
        .map(|(k, u)| format!("<a href=\"{}\" rel=\"noopener\">{k}</a>", esc(u)))
        .collect();
        let _ = write!(
            s,
            "<div class=\"note\">{}</div>",
            if links.is_empty() {
                "none linked".to_string()
            } else {
                links.join(" · ")
            }
        );
    }
    if let Some(note) = &c.note {
        let _ = write!(s, "<div class=\"note\">Note: {}</div>", esc(note));
    }
    if let Some(disc) = get("discussions").and_then(|v| v.as_array()) {
        let links: Vec<String> = disc
            .iter()
            .filter_map(|u| u.as_str())
            .enumerate()
            .map(|(i, u)| format!("<a href=\"{}\" rel=\"noopener\">{}</a>", esc(u), i + 1))
            .collect();
        if !links.is_empty() {
            let _ = write!(
                s,
                "<div class=\"note\">Discussions: {}</div>",
                links.join(" ")
            );
        }
    }
    s.push_str("</article>");
    s
}

/// Markdown form of the provenance line, same order.
pub fn claim_md(input: &Input, c: &Claim) -> String {
    let source = input.sources.get(&c.source_id);
    let (sname, licence) = source
        .map(|s| (s.name.as_str(), s.licence.as_str()))
        .unwrap_or((c.source_id.as_str(), "unknown"));
    let f = freshness(c, source, input.export_time);
    let mut md = format!("**{}**\n", c.value);
    if let Some(scope) = c
        .data
        .as_ref()
        .and_then(|d| d.get("scope"))
        .and_then(|v| v.as_str())
        .filter(|x| *x != c.value)
    {
        md.push_str(scope);
        md.push('\n');
    }
    let _ = write!(
        md,
        "Source: [{sname}]({}) · licence {licence} · retrieved {}",
        c.ref_url,
        date(c.retrieved_at)
    );
    if let Some(a) = &c.as_of {
        let _ = write!(md, " · as of {a}");
    }
    if f.stale {
        let _ = writeln!(md, " · stale, {} days old", f.age_days);
    } else {
        let _ = writeln!(md, " · {} days old", f.age_days);
    }
    if let Some(d) = &c.data {
        if let Some(w) = d.get("window_days").and_then(|v| v.as_i64()) {
            let _ = writeln!(
                md,
                "Measured over {w} days to {}, {} items. A measurement, not a verdict.",
                d.get("window_end").and_then(|v| v.as_str()).unwrap_or("?"),
                d.get("items").and_then(|v| v.as_i64()).unwrap_or(0)
            );
        }
        if let Some(s) = d.get("summary").and_then(|v| v.as_str()) {
            md.push_str(s);
            md.push('\n');
        }
    }
    if let Some(note) = &c.note {
        let _ = writeln!(md, "Note: {note}");
    }
    md
}

/// Claims for one outlet grouped by field in the shared order; the
/// caller renders each group.
pub fn grouped_claims(claims: &[Claim]) -> Vec<(&str, Vec<&Claim>)> {
    let mut fields: Vec<&str> = claims.iter().map(|c| c.field.as_str()).collect();
    fields.sort_by_key(|f| (field_rank(f), *f));
    fields.dedup();
    fields
        .into_iter()
        .map(|f| (f, claims.iter().filter(|c| c.field == f).collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn dates_and_deltas() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(datetime(1_704_098_400), "2024-01-01 08:40 UTC");
        assert_eq!(iso(1_704_098_401), "2024-01-01T08:40:01Z");
        assert_eq!(delta(600), "+10m");
        assert_eq!(delta(9000), "+2.5h");
        assert_eq!(delta(3 * DAY), "+3.0d");
        assert_eq!(esc("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(n(1, "outlet"), "1 outlet");
        assert_eq!(n(2, "outlet"), "2 outlets");
        assert_eq!(n(2, "story"), "2 stories");
        assert_eq!(n(1, "story"), "1 story");
    }

    #[test]
    fn claim_card_carries_the_provenance_line_in_order() {
        let i = fixture_input();
        let rel = i.claims["wire"]
            .iter()
            .find(|c| c.field == "reliability")
            .unwrap();
        let h = claim_card(&i, rel);
        assert!(h.starts_with("<article class=\"kg-claim\" data-outlet=\"wire\" data-field=\"reliability\" data-source=\"wikipedia_rsp\">"));
        let src = h
            .find("Wikipedia: Reliable sources/Perennial sources")
            .unwrap();
        let lic = h.find("CC-BY-SA-4.0").unwrap();
        let ret = h.find("retrieved <time").unwrap();
        let asof = h.find("as of 2025-03").unwrap();
        let status = h.find("kg-status").unwrap();
        assert!(
            src < lic && lic < ret && ret < asof && asof < status,
            "fixed order"
        );
        assert!(h.contains("stale, "), "30-day source retrieved long ago");
        assert!(h.contains("<div class=\"scope\">Wire (wire.example)</div>"));
        assert!(h.contains("Discussions: <a href="));
        let md = claim_md(&i, rel);
        assert!(md.starts_with("**Generally reliable**\nWire (wire.example)\nSource: [Wikipedia: Reliable sources/Perennial sources](https://en.wikipedia.org/wiki/Wikipedia:Reliable_sources/Perennial_sources#Wire) · licence CC-BY-SA-4.0 · retrieved 2025-06-15 · as of 2025-03 · stale, "), "{md}");
        let cad = i.claims["wire"]
            .iter()
            .find(|c| c.field == "posting_cadence")
            .unwrap();
        assert!(claim_card(&i, cad).contains("A measurement, not a verdict."));
        let groups = grouped_claims(&i.claims["wire"]);
        assert_eq!(groups[0].0, "reliability");
        assert_eq!(groups.last().unwrap().0, "wikidata_id");
    }

    #[test]
    fn cluster_card_and_skeleton() {
        let i = fixture_input();
        let c = &i.clusters[&1001];
        let h = cluster_card(&i, c, &[1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2], false);
        assert!(h.contains("data-id=\"1001\""));
        assert!(h.contains("href=\"/story/1001/\""));
        assert!(h.contains("first: <a href=\"/outlet/wire/\" data-outlet=\"wire\">Wire</a>"));
        assert!(h.contains("aria-label=\"Outlets over the first day, two-hour steps: 1, 2, 2"));
        assert!(h.contains("1 syndication group<"));
        let page = html(&Page {
            title: "T",
            description: "D",
            path: "/story/1001/",
            wide: false,
            head: "",
            body: "<p>b</p>",
            status: "s",
        });
        assert!(page.contains(
            "<link rel=\"alternate\" type=\"application/json\" href=\"/story/1001/index.json\""
        ));
        assert!(page.contains(
            "<link rel=\"alternate\" type=\"text/markdown\" href=\"/story/1001/index.md\""
        ));
        assert!(page.contains("<title>T · Kagami</title>"));
        assert!(page.contains("Kagami<span class=\"kanji\">鏡</span></a><span class=\"rule\"></span><span class=\"tool\">lens</span>"));
    }
}
