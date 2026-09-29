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

/// The origin outlet's name inside the origin ring (design §10): no visible
/// word "first", which stays in the accessible name and the twins.
pub fn origin_link(input: &Input, id: &str) -> String {
    format!(
        "<span class=\"kg-origin-name\"><span class=\"kg-sr\">first to publish: </span>{}</span>",
        outlet_link(input, id)
    )
}

/// Largest value across a page's sparklines: the shared y scale (design §5).
pub fn spark_scale<'a>(series: impl IntoIterator<Item = &'a [u32]>) -> u32 {
    series
        .into_iter()
        .flat_map(|s| s.iter().copied())
        .max()
        .unwrap_or(0)
        .max(1)
}

/// Spread sparkline (design §5): cumulative outlets per two-hour step,
/// y scaled to `scale_max` shared across the page, drawn at its own size,
/// with a baseline, an end dot and the final count beside it.
pub fn spark(points: &[u32], scale_max: u32, large: bool) -> String {
    // A series that never moves after its first bucket (every outlet
    // arrived within two hours, or one outlet and no one else) has no
    // curve to show: the count alone, with the reason in the title.
    if points.windows(2).all(|w| w[0] == w[1]) {
        let last = points.last().copied().unwrap_or(0);
        return format!(
            "<span class=\"kg-spark-wrap\" title=\"No change after the first two hours\"><span class=\"kg-spark-n\">{last}</span></span>"
        );
    }
    let (w, h) = if large { (160.0, 32.0) } else { (80.0, 20.0) };
    // 2px inset top and bottom so the stroke and end dot are not clipped.
    let (top, bottom) = (2.0, h - 2.0);
    let max = scale_max.max(1) as f64;
    let step = w / (points.len().max(2) - 1) as f64;
    let xy: Vec<(f64, f64)> = points
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64 * step, bottom - (bottom - top) * (*v as f64 / max)))
        .collect();
    let values = points
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let last = points.last().copied().unwrap_or(0);
    let mut svg = format!(
        "<span class=\"kg-spark-wrap\"><svg class=\"kg-spark{}\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"Outlets carrying the story, two-hour steps: {values}\"><line class=\"base\" x1=\"0\" y1=\"{bottom}\" x2=\"{w}\" y2=\"{bottom}\"/>",
        if large { " lg" } else { "" }
    );
    // A single-outlet story (or an empty series) draws only the baseline
    // and the end dot: an empty curve is still information.
    if points.iter().any(|&v| v > 1) {
        let line = format!(
            "M{}",
            xy.iter()
                .map(|(x, y)| format!("{x:.1},{y:.1}"))
                .collect::<Vec<_>>()
                .join(" L")
        );
        let _ = write!(
            svg,
            "<path class=\"area\" d=\"{line} L{w},{bottom} L0,{bottom} Z\"/><path class=\"line\" d=\"{line}\"/>"
        );
    }
    if let Some((x, y)) = xy.last() {
        let _ = write!(
            svg,
            "<circle class=\"end\" cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"1.5\"/>"
        );
    }
    let _ = write!(svg, "</svg><span class=\"kg-spark-n\">{last}</span></span>");
    svg
}

/// The cluster card (design §5). `outlets_24h` is the derived count.
/// `scale_max` is the page's shared sparkline scale (see `spark_scale`).
pub fn cluster_card(
    input: &Input,
    c: &Cluster,
    spark_points: &[u32],
    scale_max: u32,
    large: bool,
    enter: bool,
) -> String {
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
        "<span>{}</span><span class=\"sep\">·</span><span>{}</span><span class=\"sep\">·</span>{}<span class=\"sep\">·</span><span>first seen {}</span>",
        n(c.outlet_count as usize, "outlet"),
        n(c.item_count as usize, "item"),
        origin_link(input, &c.origin_outlet_id),
        time_el(c.created_at)
    );
    if !c.syndication.is_empty() {
        let _ = write!(
            s,
            "<span class=\"sep\">·</span><span>{}</span>",
            n(c.syndication.len(), "syndication group")
        );
    }
    s.push_str(&spark(spark_points, scale_max, large));
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
    claim_card_with(input, c, true)
}

/// The same card without notes, summaries, measurement lines or
/// discussion links: for a story page listing many outlets, where the
/// outlet page has the full record one click away.
pub fn claim_card_compact(input: &Input, c: &Claim) -> String {
    claim_card_with(input, c, false)
}

fn claim_card_with(input: &Input, c: &Claim, full: bool) -> String {
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
        } else if f.age_days == 0 {
            "retrieved today".to_string()
        } else {
            format!("{} days old", f.age_days)
        }
    );
    if full {
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
    } else if f.age_days == 0 {
        let _ = writeln!(md, " · retrieved today");
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

/// The order a source's labels are shown in: its own scale, not ours.
/// A source not listed here shows its labels alphabetically.
pub fn label_order(source_id: &str, field: &str) -> Option<&'static [&'static str]> {
    match (source_id, field) {
        ("wikipedia_rsp", "reliability") => Some(&[
            "Generally reliable",
            "No consensus",
            "Generally unreliable",
            "Deprecated",
            "Blacklisted",
        ]),
        ("allsides", "bias") => Some(&["Left", "Lean Left", "Center", "Lean Right", "Right"]),
        _ => None,
    }
}

/// One source's labels across a set of outlets (design §5, label bar).
pub struct LabelBar {
    pub source_id: String,
    pub field: String,
    /// (label, outlet ids) in the source's order; labels with no outlet omitted.
    pub segments: Vec<(String, Vec<String>)>,
    pub unrated: Vec<String>,
}

/// Fewer rated outlets than this and there is no bar to draw.
pub const LABEL_BAR_MIN_RATED: usize = 2;

/// Build the bar for `field` from `source_id`'s claims over `outlets`,
/// or None when fewer than two are rated. An outlet with several
/// claims from the source (a row per section, say) counts once, by
/// its first claim.
pub fn label_bar(
    input: &Input,
    outlets: &[&str],
    source_id: &str,
    field: &str,
) -> Option<LabelBar> {
    let mut by_label: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut unrated = Vec::new();
    for o in outlets {
        let claim = input.claims.get(*o).and_then(|v| {
            v.iter()
                .find(|c| c.source_id == source_id && c.field == field)
        });
        match claim {
            Some(c) => by_label
                .entry(c.value.clone())
                .or_default()
                .push(o.to_string()),
            None => unrated.push(o.to_string()),
        }
    }
    let rated: usize = by_label.values().map(|v| v.len()).sum();
    if rated < LABEL_BAR_MIN_RATED {
        return None;
    }
    let mut segments: Vec<(String, Vec<String>)> = Vec::new();
    if let Some(order) = label_order(source_id, field) {
        for l in order {
            if let Some(v) = by_label.remove(*l) {
                segments.push((l.to_string(), v));
            }
        }
    }
    for (l, v) in by_label {
        segments.push((l, v));
    }
    for (_, v) in &mut segments {
        v.sort_by_key(|id| input.outlet_name(id).to_lowercase());
    }
    unrated.sort_by_key(|id| input.outlet_name(id).to_lowercase());
    Some(LabelBar {
        source_id: source_id.to_string(),
        field: field.to_string(),
        segments,
        unrated,
    })
}

pub fn label_bar_html(input: &Input, bar: &LabelBar) -> String {
    let source = input.sources.get(&bar.source_id);
    let (sname, licence) = source
        .map(|s| (s.name.as_str(), s.licence.as_str()))
        .unwrap_or((bar.source_id.as_str(), "unknown"));
    let total = bar.segments.iter().map(|(_, v)| v.len()).sum::<usize>() + bar.unrated.len();
    let rated = total - bar.unrated.len();
    let mut s = String::new();
    let _ = write!(
        s,
        "<div class=\"kg-labelbar\" data-source=\"{}\" data-field=\"{}\"><p class=\"kg-muted\">{} on {} <span class=\"kg-tag\">{}</span> · {} in the story, {} rated. The source's map, not ours.</p><div class=\"bar\" role=\"img\" aria-label=\"{}\">",
        esc(&bar.source_id),
        esc(&bar.field),
        esc(sname),
        esc(&field_label(&bar.field).to_lowercase()),
        esc(licence),
        n(total, "outlet"),
        rated,
        esc(&bar
            .segments
            .iter()
            .map(|(l, v)| format!("{l} {}", v.len()))
            .chain((!bar.unrated.is_empty()).then(|| format!("not rated {}", bar.unrated.len())))
            .collect::<Vec<_>>()
            .join(", "))
    );
    for (k, (label, ids)) in bar.segments.iter().enumerate() {
        let _ = write!(
            s,
            "<span class=\"seg w{}\" style=\"flex: {}\" title=\"{}\">{} <span class=\"n\">{}</span></span>",
            (k % 5) + 1,
            ids.len(),
            esc(&ids.iter().map(|id| input.outlet_name(id)).collect::<Vec<_>>().join(", ")),
            esc(label),
            ids.len()
        );
    }
    if !bar.unrated.is_empty() {
        let _ = write!(
            s,
            "<span class=\"seg none\" style=\"flex: {}\" title=\"{}\">not rated <span class=\"n\">{}</span></span>",
            bar.unrated.len(),
            esc(&bar.unrated.iter().map(|id| input.outlet_name(id)).collect::<Vec<_>>().join(", ")),
            bar.unrated.len()
        );
    }
    s.push_str("</div><div class=\"legend\">");
    for (label, ids) in &bar.segments {
        let _ = write!(
            s,
            "<span><b>{}</b> {}</span>",
            esc(label),
            ids.iter()
                .map(|id| outlet_link(input, id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !bar.unrated.is_empty() {
        let _ = write!(
            s,
            "<span><b>Not rated by {}</b> {}</span>",
            esc(sname),
            bar.unrated
                .iter()
                .map(|id| outlet_link(input, id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    s.push_str("</div></div>");
    s
}

pub fn label_bar_md(input: &Input, bar: &LabelBar) -> String {
    let source = input.sources.get(&bar.source_id);
    let (sname, licence) = source
        .map(|s| (s.name.as_str(), s.licence.as_str()))
        .unwrap_or((bar.source_id.as_str(), "unknown"));
    let mut md = format!(
        "{} on {} (licence {}); the source's map, not ours.\n\n",
        sname,
        field_label(&bar.field).to_lowercase(),
        licence
    );
    for (label, ids) in &bar.segments {
        let _ = writeln!(
            md,
            "- {label}: {} · {}",
            n(ids.len(), "outlet"),
            ids.iter()
                .map(|id| input.outlet_name(id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !bar.unrated.is_empty() {
        let _ = writeln!(
            md,
            "- Not rated by {sname}: {} · {}",
            n(bar.unrated.len(), "outlet"),
            bar.unrated
                .iter()
                .map(|id| input.outlet_name(id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    md
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
        let h = cluster_card(
            &i,
            c,
            &[1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
            2,
            false,
            false,
        );
        assert!(h.contains("data-id=\"1001\""));
        assert!(h.contains("href=\"/story/1001/\""));
        assert!(h.contains("<span class=\"kg-origin-name\"><span class=\"kg-sr\">first to publish: </span><a href=\"/outlet/wire/\" data-outlet=\"wire\">Wire</a></span>"));
        assert!(!h.contains("first: "), "the origin is a ring, not the word");
        assert!(h.contains("aria-label=\"Outlets carrying the story, two-hour steps: 1, 2, 2"));
        let flat = cluster_card(&i, c, &[2; 12], 2, false, false);
        assert!(
            !flat.contains("<svg"),
            "no curve when nothing moved after the first bucket"
        );
        assert!(flat.contains("<span class=\"kg-spark-n\">2</span>"));
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

    // ---- design conformance (design/DESIGN.md §3, §4, §5, §12)

    #[test]
    fn tokens_css_is_the_design_copy() {
        let design = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/tokens.css"),
        )
        .expect("design/tokens.css");
        let norm = |s: &str| s.replace("\r\n", "\n");
        assert_eq!(
            norm(TOKENS_CSS),
            norm(&design),
            "lens/src/tokens.css drifted from design/tokens.css; copy it again"
        );
    }

    /// Stylesheets use only the type scale, grid line heights and spacing
    /// tokens. Raw px are allowed only in token definitions, hairlines
    /// (1px, 2px focus/underline), the tag padding and fixed geometry
    /// (sparkline, dots, breakpoint).
    #[test]
    fn stylesheets_use_only_scale_values() {
        let spacing = [
            "4px", "8px", "12px", "16px", "24px", "32px", "48px", "64px", "96px",
        ];
        for (name, css) in [("tokens.css", TOKENS_CSS), ("lens.css", LENS_CSS)] {
            for (n, line) in css.lines().enumerate() {
                let l = line.trim();
                if l.starts_with("--kg-") || l.starts_with("/*") || l.contains("@media") {
                    continue; // token definitions, comments, breakpoints
                }
                for decl in l.split(';') {
                    let Some((prop, val)) = decl.split_once(':') else {
                        continue;
                    };
                    let prop = prop.trim().rsplit(['{', ' ']).next().unwrap_or("");
                    let val = val.trim();
                    let where_ = format!("{name}:{} `{}`", n + 1, decl.trim());
                    if prop == "font-size" {
                        assert!(
                            val.starts_with("var(--kg-fs-") || val.starts_with("var(--kg-lockup-"),
                            "raw font size {where_}"
                        );
                    }
                    if prop == "font" {
                        assert!(
                            val.contains("var(--kg-fs-") && !val.contains("px"),
                            "font shorthand off the scale {where_}"
                        );
                    }
                    if prop == "line-height" {
                        assert!(
                            val.starts_with("var(--kg-lh-"),
                            "line height not a grid token {where_}"
                        );
                    }
                    if prop.starts_with("margin") || prop.starts_with("padding") || prop == "gap" {
                        for tok in val.split_whitespace() {
                            let tok = tok.trim_end_matches(';');
                            let ok = tok == "0"
                                || tok == "auto"
                                || tok.starts_with("var(--kg-s")
                                || spacing.contains(&tok)
                                || (tok == "2px" && l.contains("off-grid"));
                            assert!(ok, "spacing off the 4px scale {where_}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn sparkline_shares_scale_and_labels_values() {
        let small = spark(&[1, 1, 2, 2], 40, false);
        let big = spark(&[1, 10, 20, 40], 40, false);
        // same scale: the small story's end sits near the baseline, the big one's at the top
        let end_y = |s: &str| -> f64 {
            let i = s.find("class=\"end\" cx=\"").unwrap();
            let rest = &s[i..];
            let j = rest.find("cy=\"").unwrap() + 4;
            rest[j..].split('"').next().unwrap().parse().unwrap()
        };
        assert!(end_y(&small) > 15.0 && end_y(&big) < 3.0);
        assert!(
            !small.contains("preserveAspectRatio"),
            "drawn at size, never stretched"
        );
        assert!(
            small.contains("aria-label=\"Outlets carrying the story, two-hour steps: 1, 1, 2, 2\"")
        );
        assert!(small.contains("<span class=\"kg-spark-n\">2</span>"));
        // a series that never moves after its first bucket: the count only
        let flat = spark(&[1, 1, 1], 5, false);
        assert!(!flat.contains("<svg") && flat.contains("<span class=\"kg-spark-n\">1</span>"));
        let all_at_once = spark(&[8, 8, 8, 8], 8, true);
        assert!(!all_at_once.contains("<svg") && all_at_once.contains("first two hours"));
        assert!(spark(&[1, 2], 2, true).contains("width=\"160\" height=\"32\""));
        assert_eq!(spark_scale([&[1u32, 3][..], &[7][..]]), 7);
    }
}
