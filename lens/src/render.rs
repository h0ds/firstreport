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

/// `22 Sep`, or `22 Sep 2025` when the year differs from `now`'s.
fn day_month(ts: i64, now: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (d, today) = (date(ts), date(now));
    let m: usize = d[5..7].parse().unwrap_or(1);
    let day: u32 = d[8..10].parse().unwrap_or(1);
    let md = format!("{day} {}", MONTHS[m.clamp(1, 12) - 1]);
    if d[..4] == today[..4] {
        md
    } else {
        format!("{md} {}", &d[..4])
    }
}

/// A time as a person reads it on the page (design §8): the clock time on
/// the export's day, `28 Sep 14:02` on another day, with the year only when
/// it differs. The zone is stated once in the page chrome, not per time.
pub fn short_time(ts: i64, now: i64) -> String {
    if date(ts) == date(now) {
        hhmm(ts)
    } else {
        format!("{} {}", day_month(ts, now), hhmm(ts))
    }
}

/// The end of a range of days, `22 Sep`, as a `<time>` with the full
/// timestamp on hover.
pub fn day_el(ts: i64, now: i64) -> String {
    format!(
        "<time datetime=\"{}\" title=\"{}\">{}</time>",
        iso(ts),
        esc(&datetime(ts)),
        esc(&day_month(ts, now))
    )
}

/// `<time>` with the short form shown, the full UTC timestamp on hover and
/// RFC 3339 in `datetime` for machines.
pub fn time_el(ts: i64, now: i64) -> String {
    format!(
        "<time datetime=\"{}\" title=\"{}\">{}</time>",
        iso(ts),
        esc(&datetime(ts)),
        esc(&short_time(ts, now))
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
<title>{title} · First Report</title>
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
  <span class="kg-lockup"><a class="kg-wordmark" href="/">First Report</a></span>
  <span class="kg-muted">{status}</span>
</header>
{body}
<footer class="kg-muted">Every number on this page is a count from First Report feed; every label is a source's, with its licence and date. Nothing here rates an outlet. <a href="/search/">Search</a> · <a href="/outlets/">Outlets</a> · <a href="/about/">About</a> · Machine readers: <a href="{path}index.json">JSON</a> · <a href="{path}index.md">Markdown</a> · <a href="/llms.txt">llms.txt</a></footer>
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
    // Only registry outlets and outlets ledger knows get a page (build.rs).
    // An item feed could not attribute ("unknown:<host>") is named by its
    // host, unlinked, so the site never links to a page it did not build.
    if !input.outlets.contains_key(id) && !input.claims.contains_key(id) {
        return format!(
            "<span data-outlet=\"{}\">{}</span>",
            esc(id),
            esc(id.strip_prefix("unknown:").unwrap_or(id))
        );
    }
    format!(
        "<a href=\"/outlet/{}/\" data-outlet=\"{}\">{}</a>",
        esc(id),
        esc(id),
        esc(input.outlet_name(id))
    )
}

/// The origin outlet's name inside the origin ring (design §10): no visible
/// word "first", which stays in the accessible name and the twins.
/// The outlet's own icon (design §10: its mark, in its colours, never a
/// rating), or nothing when `feed icons` found none. `origin` adds the
/// origin ring.
pub fn outlet_icon(input: &Input, id: &str, origin: bool) -> Option<String> {
    let lean = allsides_label(input, id);
    let classes = format!(
        "{}{}",
        if origin { " kg-origin" } else { "" },
        lean.map(|l| format!(" lean-{}", lean_slug(l)))
            .unwrap_or_default()
    );
    let title = lean
        .map(|l| format!(" title=\"AllSides: {l}\""))
        .unwrap_or_default();
    // No icon: the outlet's first letter on a light tile, the same size,
    // so rows line up and the origin ring always sits on a square.
    let Some(file) = input
        .icons
        .get(id)
        .and_then(|p| p.file_name()?.to_str().map(str::to_string))
    else {
        let name = input.outlet_name(id);
        let letter = name
            .trim_start_matches("unknown:")
            .trim_start_matches("The ")
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        return Some(format!(
            "<span class=\"kg-icon kg-letter{classes}\" aria-hidden=\"true\"{title}>{}</span>",
            esc(&letter)
        ));
    };
    Some(format!(
        "<img class=\"kg-icon{classes}\" src=\"/icons/{}\" alt=\"\" width=\"16\" height=\"16\" decoding=\"async\"{title}>",
        esc(&file)
    ))
}

/// The outlet's AllSides label (Left … Right), if AllSides rates it.
/// Only AllSides drives colour on outlets (a rating of the outlet's
/// content, by a named rater); audience measures never do.
pub fn allsides_label<'a>(input: &'a Input, id: &str) -> Option<&'a str> {
    input
        .claims
        .get(id)?
        .iter()
        .find(|c| c.source_id == "allsides" && c.field == "bias")
        .map(|c| c.value.as_str())
}

/// CSS slug for an AllSides label: left, lean-left, center, lean-right, right.
pub fn lean_slug(label: &str) -> String {
    label.to_ascii_lowercase().replace(' ', "-")
}

/// Designations that travel with an outlet's name, each a named
/// source's (design §5): a "State" tag when State Media Monitor files it
/// as State-Controlled or Captured Public, and one dark tag for the
/// strongest propaganda designation: "Sanctioned" (EU suspension) over
/// "Deprecated" (Wikipedia RSP). The source and its words are on hover.
pub fn outlet_flags(input: &Input, id: &str) -> String {
    let Some(claims) = input.claims.get(id) else {
        return String::new();
    };
    let find = |src: &str, field: &str| {
        claims
            .iter()
            .find(|c| c.source_id == src && c.field == field)
    };
    let mut s = String::new();
    if let Some(c) = find("state_media_monitor", "state_media") {
        if c.value.ends_with("(SC)") || c.value.ends_with("(CaPu)") {
            let _ = write!(
                s,
                "<abbr class=\"kg-flag state\" title=\"State Media Monitor: {}\">State</abbr>",
                esc(&c.value)
            );
        }
    }
    let flag = find("eu_sanctions", "sanctioned")
        .map(|c| ("Sanctioned", format!("EU: {}", c.value)))
        .or_else(|| {
            find("wikipedia_rsp", "reliability")
                .filter(|c| c.value == "Deprecated")
                .map(|_| {
                    (
                        "Deprecated",
                        "Wikipedia: deprecated as a source".to_string(),
                    )
                })
        });
    if let Some((label, title)) = flag {
        let _ = write!(
            s,
            "<abbr class=\"kg-flag warn\" title=\"{}\">{label}</abbr>",
            esc(&title)
        );
    }
    s
}

/// An outlet as it appears in a row: icon, name, then any designations.
pub fn outlet_badge(input: &Input, id: &str) -> String {
    format!(
        "<span class=\"kg-outlet\">{}{}{}</span>",
        outlet_icon(input, id, false).unwrap_or_default(),
        outlet_link(input, id),
        outlet_flags(input, id)
    )
}

/// The origin outlet (design §10): its icon inside the origin ring, or
/// its name inside the ring when it has no icon. No visible "first".
pub fn origin_link(input: &Input, id: &str) -> String {
    match outlet_icon(input, id, true) {
        Some(icon) => format!(
            "<span class=\"kg-outlet\">{icon}<span class=\"kg-sr\">first to publish: </span>{}{}</span>",
            outlet_link(input, id),
            outlet_flags(input, id)
        ),
        None => format!(
            "<span class=\"kg-origin-name\"><span class=\"kg-sr\">first to publish: </span>{}</span>",
            outlet_link(input, id)
        ),
    }
}

/// Icons of the outlets carrying a story, in the order they joined, the
/// origin first and ringed; up to `max`, then "+N". Empty without icons.
pub fn outlet_stack(input: &Input, c: &Cluster, max: usize) -> String {
    let mut ids: Vec<&str> = Vec::new();
    for j in &c.timeline {
        if !ids.contains(&j.outlet_id.as_str()) {
            ids.push(&j.outlet_id);
        }
    }
    if let Some(p) = ids.iter().position(|i| *i == c.origin_outlet_id) {
        let o = ids.remove(p);
        ids.insert(0, o);
    }
    let shown: Vec<String> = ids
        .iter()
        .take(max)
        .filter_map(|id| {
            outlet_icon(input, id, *id == c.origin_outlet_id).map(|img| {
                img.replace(
                    "<img ",
                    &format!("<img title=\"{}\" ", esc(input.outlet_name(id))),
                )
            })
        })
        .collect();
    if shown.is_empty() {
        return String::new();
    }
    let more = ids.len().saturating_sub(max);
    format!(
        "<span class=\"kg-stack\" aria-hidden=\"true\">{}{}</span>",
        shown.join(""),
        if more > 0 {
            format!("<span class=\"more\">+{more}</span>")
        } else {
            String::new()
        }
    )
}

/// Distinct outlets in a story's timeline, in the order they joined.
pub fn story_outlets(c: &Cluster) -> Vec<&str> {
    let mut ids: Vec<&str> = Vec::new();
    for j in &c.timeline {
        if !ids.contains(&j.outlet_id.as_str()) {
            ids.push(&j.outlet_id);
        }
    }
    ids
}

/// Robertson et al.'s audience score for an outlet, if ledger has it.
fn audience_score(input: &Input, id: &str) -> Option<f64> {
    input.claims.get(id)?.iter().find_map(|c| {
        (c.field == "audience_partisanship")
            .then(|| c.data.as_ref()?.get("score")?.as_f64())
            .flatten()
    })
}

/// Where the outlets in a story sit on Robertson et al.'s audience scale,
/// -1 (links shared by Democrats) to +1 (by Republicans): one grey dot per
/// scored outlet on a hairline with a centre mark (design §5, lean strip).
/// A measure of the outlets' audiences, named as such; empty with fewer
/// than two scored outlets.
pub fn audience_strip(input: &Input, outlets: &[&str], large: bool) -> String {
    let scored: Vec<(&str, f64)> = outlets
        .iter()
        .filter_map(|id| audience_score(input, id).map(|s| (*id, s)))
        .collect();
    let unknown: Vec<&str> = outlets
        .iter()
        .copied()
        .filter(|id| audience_score(input, id).is_none())
        .collect();
    let names = |ids: &[&str]| {
        ids.iter()
            .map(|id| input.outlet_name(id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if outlets.is_empty() {
        return String::new();
    }
    // No outlet scored: say so rather than show nothing (design §5). On a
    // card that is two words; the story page says it in full.
    if scored.is_empty() && !large {
        return format!(
            "<span class=\"kg-audience none\" data-scored=\"0\" data-unknown=\"{}\" title=\"Not scored by Robertson et al. 2018: {}\"><span class=\"lbl\">lean unknown</span></span>",
            unknown.len(),
            esc(&names(&unknown))
        );
    }
    if scored.is_empty() {
        return format!(
            "<span class=\"kg-audience none\" data-scored=\"0\" data-unknown=\"{}\" title=\"Not scored by Robertson et al. 2018: {}\"><span class=\"lbl\">political lean unknown for {}</span></span>",
            unknown.len(),
            esc(&names(&unknown)),
            n(unknown.len(), "outlet")
        );
    }
    let (w, h) = if large { (240.0, 16.0) } else { (120.0, 12.0) };
    let (pad, mid) = (4.0, h / 2.0);
    let r = if large { 3.5 } else { 3.0 };
    let x = |s: f64| pad + (s.clamp(-1.0, 1.0) + 1.0) / 2.0 * (w - 2.0 * pad);
    let mut dots = String::new();
    // Each dot takes the colour of its fifth of the scale (design §5: a
    // teal-to-amber scale, never red and blue, whose meaning flips between
    // countries).
    let bucket = |s: f64| ((s.clamp(-1.0, 1.0) + 1.0) / 2.0 * 5.0).floor().min(4.0) as usize + 1;
    for (id, s) in &scored {
        let _ = write!(
            dots,
            "<circle class=\"b{}\" cx=\"{:.1}\" cy=\"{mid}\" r=\"{r}\"><title>{} {s:+.2}</title></circle>",
            bucket(*s),
            x(*s),
            esc(input.outlet_name(id))
        );
    }
    // The track in five coloured fifths under the dots.
    let mut track = String::new();
    let seg = (w - 2.0 * pad) / 5.0;
    for k in 0..5 {
        let _ = write!(
            track,
            "<line class=\"t{}\" x1=\"{:.1}\" y1=\"{mid}\" x2=\"{:.1}\" y2=\"{mid}\"/>",
            k + 1,
            pad + seg * k as f64,
            pad + seg * (k + 1) as f64
        );
    }
    // Unknown outlets: hollow dots in their own group after R, never placed
    // on the scale and never counted toward either side.
    let unknown_svg = if unknown.is_empty() {
        String::new()
    } else {
        let step = r * 2.0 + 2.0;
        let uw = (unknown.len().min(6) as f64) * step + 2.0;
        let mut u = String::new();
        for (k, id) in unknown.iter().take(6).enumerate() {
            let _ = write!(
                u,
                "<circle class=\"unknown\" cx=\"{:.1}\" cy=\"{mid}\" r=\"{r}\"><title>{}: not scored</title></circle>",
                1.0 + r + k as f64 * step,
                esc(input.outlet_name(id))
            );
        }
        format!(
            "<svg class=\"unknown-group\" width=\"{uw:.0}\" height=\"{h}\" viewBox=\"0 0 {uw:.0} {h}\" aria-hidden=\"true\">{u}</svg>"
        )
    };
    let label = scored
        .iter()
        .map(|(id, s)| format!("{} {s:+.2}", input.outlet_name(id)))
        .collect::<Vec<_>>()
        .join(", ");
    let unknown_label = if unknown.is_empty() {
        String::new()
    } else {
        format!("; unknown: {}", names(&unknown))
    };
    // Cards carry the dots only; the page's legend names the source once.
    let text = if large {
        format!(
            "<span class=\"lbl\">{} of {} scored{} <span class=\"src\">(Robertson et al. 2018)</span></span>",
            scored.len(),
            outlets.len(),
            if unknown.is_empty() {
                String::new()
            } else {
                format!(" · <span title=\"Not scored by Robertson et al. 2018: {}\">{} unknown</span>", esc(&names(&unknown)), unknown.len())
            }
        )
    } else {
        String::new()
    };
    format!(
        "<span class=\"kg-audience\" data-scored=\"{}\" data-unknown=\"{}\" title=\"Where each outlet's audience leans, left to right, from US sharing data (Robertson et al. 2018): {} of {} scored\"><span class=\"end\">L</span><svg width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"Audience lean of {} outlets, left to right (US sharing data: -1 shared by Democrats, +1 by Republicans; Robertson et al. 2018): {}{}\">{track}<line class=\"axis\" x1=\"{pad}\" y1=\"{mid}\" x2=\"{:.1}\" y2=\"{mid}\"/><line class=\"zero\" x1=\"{:.1}\" y1=\"1\" x2=\"{:.1}\" y2=\"{:.1}\"/>{dots}</svg><span class=\"end\">R</span>{unknown_svg}{text}</span>",
        scored.len(),
        unknown.len(),
        scored.len(),
        outlets.len(),
        scored.len(),
        esc(&label),
        esc(&unknown_label),
        w - pad,
        x(0.0),
        x(0.0),
        h - 1.0,
    )
}

/// Outlets in a story that wrote their own report, and outlets whose
/// every item was the same text as another outlet's (a copy).
pub fn copy_split(c: &Cluster, f: &crate::derive::ClusterFacts) -> (usize, usize) {
    let mut by: std::collections::BTreeMap<&str, bool> = Default::default();
    for j in &c.timeline {
        let is_copy = f.copies_of.contains_key(&j.item_id);
        let e = by.entry(j.outlet_id.as_str()).or_insert(true);
        *e = *e && is_copy;
    }
    let copies = by.values().filter(|v| **v).count();
    (by.len() - copies, copies)
}

/// Countries of the outlets in a story, from ledger's Wikidata country
/// claims, most outlets first. An outlet with several counts each.
pub fn story_countries(input: &Input, outlets: &[&str]) -> Vec<(String, usize)> {
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for id in outlets {
        let mut seen = std::collections::BTreeSet::new();
        for c in input.claims.get(*id).into_iter().flatten() {
            if c.field == "country" && c.source_id == "wikidata" && seen.insert(c.value.clone()) {
                *by.entry(c.value.clone()).or_default() += 1;
            }
        }
    }
    let mut v: Vec<(String, usize)> = by.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// The label bar in one line for a card: thin grey segments in the
/// source's order and the counts as text. Same data and rules as the
/// story page's bar (at least two outlets rated), without the legend.
pub fn label_bar_compact(input: &Input, bar: &LabelBar) -> String {
    // AllSides' five labels fold into Left / Center / Right, as percentages
    // of the rated outlets; unrated outlets are counted beside the bar.
    let count = |labels: &[&str]| -> Vec<String> {
        bar.segments
            .iter()
            .filter(|(l, _)| labels.contains(&l.as_str()))
            .flat_map(|(_, ids)| ids.clone())
            .collect()
    };
    let parts = [
        ("left", "Left", count(&["Left", "Lean Left"])),
        ("center", "Center", count(&["Center"])),
        ("right", "Right", count(&["Lean Right", "Right"])),
    ];
    let rated: usize = parts.iter().map(|(_, _, v)| v.len()).sum();
    if rated == 0 {
        return String::new();
    }
    let mut segs = String::new();
    for (slug, word, ids) in &parts {
        if ids.is_empty() {
            continue;
        }
        let pct = (ids.len() as f64 / rated as f64 * 100.0).round() as usize;
        let _ = write!(
            segs,
            "<span class=\"seg {slug}\" style=\"flex: {}\" title=\"{word}: {}\"><span class=\"t\">{word} {pct}%</span></span>",
            ids.len(),
            esc(&ids.iter().map(|id| input.outlet_name(id)).collect::<Vec<_>>().join(", "))
        );
    }
    let summary = parts
        .iter()
        .filter(|(_, _, v)| !v.is_empty())
        .map(|(_, w, v)| {
            format!(
                "{w} {}%",
                (v.len() as f64 / rated as f64 * 100.0).round() as usize
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "<span class=\"kg-coverage\" data-source=\"{}\" data-field=\"{}\"><span class=\"bar\" role=\"img\" aria-label=\"AllSides on {} rated outlets: {}{}\">{segs}</span><span class=\"lbl\">{}AllSides</span></span>",
        esc(&bar.source_id),
        esc(&bar.field),
        rated,
        esc(&summary),
        if bar.unrated.is_empty() { String::new() } else { format!("; {} not rated", bar.unrated.len()) },
        if bar.unrated.is_empty() { String::new() } else { format!("{} not rated · ", bar.unrated.len()) }
    )
}

/// The lean line under a card: AllSides' labels when at least two
/// outlets are rated, and the audience strip when at least two are scored.
pub fn lean_line(input: &Input, c: &Cluster) -> String {
    let ids = story_outlets(c);
    let bar = label_bar(input, &ids, "allsides", "bias")
        .map(|b| label_bar_compact(input, &b))
        .unwrap_or_default();
    let strip = audience_strip(input, &ids, false);
    format!("{bar}{strip}")
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
    // curve to show, and its count is already the card's count chip, so
    // nothing is drawn: a bare number there read as a stray digit.
    if points.windows(2).all(|w| w[0] == w[1]) {
        return String::new();
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
    reach: Option<&crate::derive::ClusterFacts>,
) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<article class=\"kg-cluster{}\" data-id=\"{}\"{}><p class=\"title\"><a {}>{}</a></p><div class=\"meta kg-muted\">",
        if enter { " kg-enter" } else { "" },
        c.id,
        reach.map(|f| format!(" data-reach=\"{}\"", f.reach)).unwrap_or_default(),
        input.story_href(c),
        esc(&c.primary_title)
    );
    let _ = write!(
        s,
        "<span class=\"kg-count\">{}</span>{}<span class=\"sep\">·</span><span>{}</span><span class=\"sep\">·</span>{}<span class=\"sep\">·</span><span>first seen {}</span>",
        n(c.outlet_count as usize, "outlet"),
        outlet_stack(input, c, 8),
        n(c.item_count as usize, "item"),
        origin_link(input, &c.origin_outlet_id),
        time_el(c.created_at, input.export_time)
    );
    // Own reports against copies (design §5): outlets whose every item on
    // the story was the same text as another outlet's are copies. Shown
    // only when there are some; the count of each is in words.
    if let Some(f) = reach {
        let (own, copies) = copy_split(c, f);
        if copies > 0 {
            let total = (own + copies).max(1) as f64;
            let _ = write!(
                s,
                "<span class=\"sep\">·</span><span class=\"kg-split\" title=\"{own} of {} outlets wrote their own report; {copies} ran the same text as another outlet\"><span class=\"bar\" aria-hidden=\"true\"><span class=\"own\" style=\"width: {:.0}%\"></span></span>{} · {}</span>",
                own + copies,
                own as f64 / total * 100.0,
                n(own, "own report"),
                n(copies, "copy")
            );
        }
        if f.languages > 1 {
            let _ = write!(
                s,
                "<span class=\"sep\">·</span><span>in {} languages</span>",
                f.languages
            );
        }
    }
    // How far across the world: the outlets' countries (Wikidata), when
    // more than one, listed on hover.
    let countries = story_countries(input, &story_outlets(c));
    if countries.len() > 1 {
        let _ = write!(
            s,
            "<span class=\"sep\">·</span><span title=\"{}\">in {} countries</span>",
            esc(&countries
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", ")),
            countries.len()
        );
    }
    s.push_str("</div>");
    // One row of pictures: spread over the first day, then where the
    // outlets' audiences sit (design §5).
    let visuals = format!(
        "{}{}",
        spark(spark_points, scale_max, large),
        lean_line(input, c)
    );
    if !visuals.is_empty() {
        let _ = write!(s, "<div class=\"kg-visuals\">{visuals}</div>");
    }
    s.push_str("</article>");
    s
}

/// Field order and labels shared with ledger's lookup page and
/// Markdown twin, so an outlet reads the same everywhere.
pub const FIELD_ORDER: &[(&str, &str)] = &[
    ("reliability", "Reliability"),
    ("bias", "Bias"),
    ("sanctioned", "Sanctions"),
    ("state_media", "State media"),
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
        if c.field == "site_pages" { site_pages_value(get) } else { esc(&c.value) }
    );
    if let Some(scope) = get("scope")
        .and_then(|v| v.as_str())
        .filter(|x| *x != c.value && *x != input.outlet_name(&c.outlet_id))
    {
        let _ = write!(s, "<div class=\"scope\">{}</div>", esc(scope));
    }
    let _ = write!(
        s,
        "<div class=\"kg-provenance\">{}<span class=\"sep\">·</span><span>retrieved <time datetime=\"{}\">{}</time></span>",
        if is_local(&c.ref_url) {
            format!(
                "<span title=\"Licence: {}\">{}</span>",
                esc(licence),
                esc(sname)
            )
        } else {
            format!(
                "<a href=\"{}\" rel=\"noopener\" title=\"Licence: {}\">{}</a>",
                esc(&c.ref_url),
                esc(licence),
                esc(sname)
            )
        },
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
    // The age only when it matters: a stale fact says so; a fresh one's
    // retrieved date already tells its age.
    if f.stale {
        let _ = write!(
            s,
            "<span class=\"sep\">·</span><span class=\"kg-status warn\"><span>stale, {} days old</span></span>",
            f.age_days
        );
    }
    s.push_str("</div>");
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
            let _ = write!(s, "<div class=\"note\">{}</div>", esc(whole_sentences(sum)));
        }
        if let Some(note) = &c.note {
            let _ = write!(s, "<div class=\"note\">Note: {}</div>", esc(note));
        }
    }
    s.push_str("</article>");
    s
}

/// A summary cut off by its source ends at its last full sentence.
pub fn whole_sentences(s: &str) -> &str {
    let t = s.trim_end();
    if t.ends_with(['.', '!', '?', '"', ')']) {
        return t;
    }
    match t.rfind(". ") {
        Some(i) => &t[..=i],
        None => t,
    }
}

/// "Links to about, contact and privacy pages. No corrections, ethics or
/// masthead page." with each found page linked.
fn site_pages_value<'a>(get: impl Fn(&str) -> Option<&'a serde_json::Value>) -> String {
    const KINDS: [&str; 6] = [
        "about",
        "contact",
        "privacy",
        "corrections",
        "ethics",
        "masthead",
    ];
    let (found, missing): (Vec<&str>, Vec<&str>) = KINDS
        .iter()
        .copied()
        .partition(|k| get(k).and_then(|v| v.as_str()).is_some());
    let list = |xs: Vec<String>| match xs.len() {
        0 => String::new(),
        1 => xs[0].clone(),
        n => format!("{} and {}", xs[..n - 1].join(", "), xs[n - 1]),
    };
    let links = found
        .iter()
        .map(|k| {
            format!(
                "<a href=\"{}\" rel=\"noopener\">{k}</a>",
                esc(get(k).and_then(|v| v.as_str()).unwrap_or_default())
            )
        })
        .collect();
    let mut out = String::new();
    if !found.is_empty() {
        out.push_str(&format!(
            "Links to {} {}.",
            list(links),
            if found.len() == 1 { "page" } else { "pages" }
        ));
    }
    if !missing.is_empty() {
        let or = |xs: Vec<String>| match xs.len() {
            1 => xs[0].clone(),
            n => format!("{} or {}", xs[..n - 1].join(", "), xs[n - 1]),
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!(
            "No {} page.",
            or(missing.iter().map(|k| k.to_string()).collect())
        ));
    }
    out
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
    if is_local(&c.ref_url) {
        let _ = write!(
            md,
            "Source: {sname} · licence {licence} · retrieved {}",
            date(c.retrieved_at)
        );
    } else {
        let _ = write!(
            md,
            "Source: [{sname}]({}) · licence {licence} · retrieved {}",
            c.ref_url,
            date(c.retrieved_at)
        );
    }
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

/// Stories N–M of T with thousands separators, for the pager.
pub fn thousands(n: usize) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (k, ch) in d.chars().enumerate() {
        if k > 0 && (d.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A list's pager (design §5), used for every long list: Newer and Older as plain arrowed
/// links, page numbers in mono with the current one underlined like a tab,
/// "51–100 of 2,219" in words, and a hairline rail with a blue segment
/// showing where this page sits in the whole list.
pub fn pager(
    page: usize,
    pages: usize,
    total: usize,
    per: usize,
    label: &str,
    page_path: &dyn Fn(usize) -> String,
) -> String {
    if pages <= 1 {
        return String::new();
    }
    let first = (page - 1) * per + 1;
    let last_item = (page * per).min(total);
    let mut s = format!(
        "<nav class=\"pager\" aria-label=\"{}\"><div class=\"row\">",
        esc(label)
    );
    if page > 1 {
        let _ = write!(
            s,
            "<a class=\"step\" rel=\"prev\" href=\"{}\">← Newer</a>",
            page_path(page - 1)
        );
    } else {
        s.push_str("<span class=\"step off\">← Newer</span>");
    }
    s.push_str("<span class=\"pages\">");
    let mut last = 0;
    for p in 1..=pages {
        if p == 1 || p == pages || p.abs_diff(page) <= 2 {
            if last != 0 && p > last + 1 {
                s.push_str("<span class=\"gap\">…</span>");
            }
            if p == page {
                let _ = write!(s, "<span class=\"cur\" aria-current=\"page\">{p}</span>");
            } else {
                let _ = write!(s, "<a href=\"{}\">{p}</a>", page_path(p));
            }
            last = p;
        }
    }
    s.push_str("</span>");
    if page < pages {
        let _ = write!(
            s,
            "<a class=\"step\" rel=\"next\" href=\"{}\">Older →</a>",
            page_path(page + 1)
        );
    } else {
        s.push_str("<span class=\"step off\">Older →</span>");
    }
    let left = (first - 1) as f64 / total.max(1) as f64 * 100.0;
    let width = ((last_item - first + 1) as f64 / total.max(1) as f64 * 100.0).max(1.0);
    let _ = write!(
        s,
        "</div><div class=\"where\"><span class=\"rail\" aria-hidden=\"true\"><span class=\"pos\" style=\"left: {left:.2}%; width: {width:.2}%\"></span></span><span class=\"range\">{}–{} of {}</span></div></nav>",
        thousands(first),
        thousands(last_item),
        thousands(total)
    );
    s
}

/// A reference that points at this machine: feed-behaviour claims cite
/// feed's own API on loopback. A public page names the source as text
/// instead of linking somewhere no reader can reach (`docs/deploy.md` §4.3).
pub fn is_local(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let host = rest.split(['/', ':', '?', '#']).next().unwrap_or("");
    host.eq_ignore_ascii_case("localhost")
        || host.starts_with("127.")
        || host == "0.0.0.0"
        || rest.starts_with("[::1]")
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_references_are_local() {
        assert!(is_local("http://127.0.0.1:8080/v1/feeds"));
        assert!(is_local("http://localhost/x"));
        assert!(is_local("http://[::1]:8080/x"));
        assert!(!is_local("https://localhost.example/x"));
        assert!(!is_local("https://www.wikidata.org/wiki/Q1"));
        assert!(!is_local(""));
    }

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
        assert!(
            h.contains("title=\"Licence: CC-BY-SA-4.0\""),
            "licence on hover"
        );
        let ret = h.find("retrieved <time").unwrap();
        let asof = h.find("as of 2025-03").unwrap();
        let status = h.find("kg-status").unwrap();
        assert!(src < ret && ret < asof && asof < status, "fixed order");
        assert_eq!(whole_sentences("One. Two cut of"), "One.");
        assert_eq!(whole_sentences("Whole."), "Whole.");
        assert!(h.contains("stale, "), "30-day source retrieved long ago");
        assert!(h.contains("<div class=\"scope\">Wire (wire.example)</div>"));
        assert!(
            !h.contains("Discussions:"),
            "discussion links stay in the twins"
        );
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
    fn times_are_short_with_the_full_form_on_hover() {
        let now = 1_758_546_200; // 2025-09-22 13:03 UTC
        assert_eq!(short_time(now - 3600, now), "12:03");
        assert_eq!(short_time(now - 2 * DAY, now), "20 Sep 13:03");
        assert_eq!(short_time(now - 400 * DAY, now), "18 Aug 2024 13:03");
        assert!(time_el(now, now).contains("title=\"2025-09-22 13:03 UTC\">13:03</time>"));
        assert!(day_el(now, now).ends_with(">22 Sep</time>"));
    }

    #[test]
    fn lean_visuals_need_two_outlets_and_name_their_source() {
        let mut i = fixture_input();
        let claim =
            |id: &str, field: &str, value: &str, src: &str, data: Option<serde_json::Value>| {
                crate::input::Claim {
                    outlet_id: id.into(),
                    field: field.into(),
                    value: value.into(),
                    data: data.map(|d| serde_json::from_value(d).unwrap()),
                    source_id: src.into(),
                    ref_url: "https://x".into(),
                    retrieved_at: 0,
                    as_of: None,
                    note: None,
                }
            };
        for (id, score) in [("wire", -0.5), ("gazette", 0.5)] {
            i.claims.entry(id.into()).or_default().push(claim(
                id,
                "audience_partisanship",
                "x",
                "robertson2018",
                Some(serde_json::json!({"score": score})),
            ));
        }
        // Nothing scored: said, not hidden.
        let none = audience_strip(&i, &["blog"], false);
        assert!(none.contains("lean unknown") && none.contains("data-unknown=\"1\""));
        assert!(audience_strip(&i, &["blog"], true).contains("political lean unknown for 1 outlet"));
        // One scored outlet still draws, beside its unknowns.
        let one = audience_strip(&i, &["wire", "blog"], false);
        assert!(one.contains("data-unknown=\"1\"") && one.contains("class=\"unknown\""));
        assert!(
            !one.contains("class=\"lbl\""),
            "cards carry dots, not text; the legend names the source"
        );
        assert!(audience_strip(&i, &["wire", "blog"], true).contains("1 of 2 scored · <span"));
        let two = audience_strip(&i, &["wire", "gazette", "blog"], false);
        assert!(
            two.contains("2 of 3 scored") && two.contains("Robertson et al. 2018"),
            "in the hover title"
        );
        assert!(
            two.contains("unknown: Blog"),
            "unknown outlets are named in the aria-label"
        );
        // -0.5 and +0.5 sit a quarter in from each end of the 120px strip.
        assert!(
            two.contains("cx=\"32.0\"") && two.contains("cx=\"88.0\""),
            "{two}"
        );
        assert!(
            !two.contains("fill=") && !two.contains("stroke=") && !two.contains("style="),
            "colours come from the grey tokens only"
        );
        // Designations: State only for SC / CaPu; Sanctioned beats Deprecated.
        i.claims.entry("wire".into()).or_default().push(claim(
            "wire",
            "state_media",
            "Independent Public (IP)",
            "state_media_monitor",
            None,
        ));
        assert_eq!(
            outlet_flags(&i, "wire"),
            "",
            "independent public media carry no tag"
        );
        i.claims.entry("flagged".into()).or_default().push(claim(
            "flagged",
            "state_media",
            "State-Controlled (SC)",
            "state_media_monitor",
            None,
        ));
        i.claims.entry("flagged".into()).or_default().push(claim(
            "flagged",
            "reliability",
            "Deprecated",
            "wikipedia_rsp",
            None,
        ));
        let f = outlet_flags(&i, "flagged");
        assert!(
            f.contains(">State</abbr>") && f.contains(">Deprecated</abbr>"),
            "{f}"
        );
        i.claims.entry("flagged".into()).or_default().push(claim(
            "flagged",
            "sanctioned",
            "Broadcasting suspended in the EU",
            "eu_sanctions",
            None,
        ));
        let f = outlet_flags(&i, "flagged");
        assert!(
            f.contains(">Sanctioned</abbr>") && !f.contains("Deprecated"),
            "one dark tag at most: {f}"
        );
        assert!(outlet_badge(&i, "flagged").contains("kg-flag state"));
        i.claims.get_mut("wire").unwrap().clear();
        // AllSides: no bar with fewer than two rated outlets.
        i.sources.insert(
            "allsides".into(),
            crate::input::Source {
                id: "allsides".into(),
                name: "AllSides Media Bias Ratings".into(),
                licence: "CC-BY-NC-4.0".into(),
                ..Default::default()
            },
        );
        i.claims
            .entry("wire".into())
            .or_default()
            .push(claim("wire", "bias", "Center", "allsides", None));
        assert!(label_bar(&i, &["wire", "gazette"], "allsides", "bias").is_none());
        i.claims.entry("gazette".into()).or_default().push(claim(
            "gazette",
            "bias",
            "Lean Right",
            "allsides",
            None,
        ));
        let bar = label_bar(&i, &["wire", "gazette", "blog"], "allsides", "bias").unwrap();
        let compact = label_bar_compact(&i, &bar);
        assert!(
            compact.contains("Center 50%")
                && compact.contains("Right 50%")
                && !compact.contains("Left "),
            "{compact}"
        );
        assert!(compact.contains("1 not rated") && compact.contains("AllSides"));
        // Only AllSides colours an outlet, and only a rated one.
        assert_eq!(allsides_label(&i, "gazette"), Some("Lean Right"));
        assert_eq!(allsides_label(&i, "blog"), None);
        assert_eq!(lean_slug("Lean Right"), "lean-right");
    }

    #[test]
    fn unattributed_outlet_is_named_not_linked() {
        let i = fixture_input();
        let h = outlet_link(&i, "unknown:partner.example");
        assert_eq!(
            h,
            "<span data-outlet=\"unknown:partner.example\">partner.example</span>"
        );
        assert!(outlet_link(&i, "wire").starts_with("<a href=\"/outlet/wire/\""));
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
            None,
        );
        assert!(h.contains("data-id=\"1001\""));
        assert!(h.contains("href=\"/story/1001/\""));
        // No icon in the fixture: the origin ring sits on a letter tile.
        assert!(h.contains("<span class=\"kg-outlet\"><span class=\"kg-icon kg-letter kg-origin\" aria-hidden=\"true\">W</span><span class=\"kg-sr\">first to publish: </span><a href=\"/outlet/wire/\" data-outlet=\"wire\">Wire</a></span>"), "{h}");
        assert!(!h.contains("first: "), "the origin is a ring, not the word");
        assert!(h.contains("aria-label=\"Outlets carrying the story, two-hour steps: 1, 2, 2"));
        let flat = cluster_card(&i, c, &[2; 12], 2, false, false, None);
        assert!(
            !flat.contains("<svg"),
            "no curve when nothing moved after the first bucket"
        );
        assert!(!flat.contains("kg-spark-n"), "no bare count beside nothing");
        assert!(
            !h.contains("syndication group"),
            "copies are said as own reports against copies"
        );
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
        assert!(page.contains("<title>T · First Report</title>"));
        assert!(page.contains("First Report</a></span>"));
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
        // a series that never moves after its first bucket: nothing at all
        assert_eq!(spark(&[1, 1, 1], 5, false), "");
        assert_eq!(spark(&[8, 8, 8, 8], 8, true), "");
        assert!(spark(&[1, 2], 2, true).contains("width=\"160\" height=\"32\""));
        assert_eq!(spark_scale([&[1u32, 3][..], &[7][..]]), 7);
    }
}
