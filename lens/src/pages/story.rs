//! `/story/<cluster id>/` (plan §3.2): the cluster card, the timeline
//! with headlines side by side, copies, changes, and a claim card per
//! outlet for its most useful facts. Coverage by source label is N3.

use std::collections::HashMap;
use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::derive::{cluster_facts, ClusterFacts};
use crate::input::{Cluster, Input, Item};
use crate::render::{
    claim_md, cluster_card, datetime, delta, esc, field_label, grouped_claims,
    html, iso, n, outlet_link, time_el, Page,
};

/// Source and field pairs that can draw a label bar, in display order.
/// AllSides bias joins once its terms are confirmed (ledger plan §4).
const LABEL_SOURCES: &[(&str, &str)] = &[("wikipedia_rsp", "reliability"), ("allsides", "bias")];

/// Fields shown per outlet on a story page, in this order.
const OUTLET_FIELDS: &[&str] = &[
    "reliability",
    "owner",
    "country",
    "founded",
    "domain_registered",
];

/// When an item entered the story as the outlet told it: its published
/// time, or the pick-up time when the feed gave none.
fn when(i: &Item) -> i64 {
    if i.published_at > 0 {
        i.published_at
    } else {
        i.joined_at.unwrap_or(i.fetched_at)
    }
}

/// Members grouped by outlet, ordered by published time (pick-up time
/// and item id break ties, so a first poll that picks everything up at
/// once still reads in the order the outlets published), the cluster's
/// origin outlet always first. Within an outlet, its earliest item
/// leads and the rest follow.
fn by_outlet<'a>(input: &'a Input, c: &'a Cluster) -> Vec<(&'a str, Vec<&'a Item>)> {
    let mut items: Vec<&Item> = c
        .timeline
        .iter()
        .filter_map(|j| input.items.get(&j.item_id))
        .collect();
    items.sort_by_key(|i| (when(i), i.joined_at.unwrap_or(i.fetched_at), i.id));
    let mut out: Vec<(&str, Vec<&Item>)> = Vec::new();
    for it in items {
        match out.iter_mut().find(|(o, _)| *o == it.outlet_id) {
            Some((_, v)) => v.push(it),
            None => out.push((&it.outlet_id, vec![it])),
        }
    }
    if let Some(pos) = out.iter().position(|(o, _)| *o == c.origin_outlet_id) {
        let origin = out.remove(pos);
        out.insert(0, origin);
    }
    out
}

/// The story's spread as a picture (design §5, timeline track): each
/// outlet's icon placed along a line by when it published, the origin
/// ringed, icons that would overlap stacked into lanes. The start time
/// and the total spread label the ends. Empty for a single outlet.
fn track(input: &Input, c: &Cluster, rows: &[(&str, Vec<&Item>)], t0: i64) -> String {
    if rows.len() < 2 {
        return String::new();
    }
    let times: Vec<i64> = rows.iter().map(|(_, v)| v.first().map(|i| when(i)).unwrap_or(t0)).collect();
    let end = times.iter().copied().max().unwrap_or(t0);
    let span = (end - t0).max(60);
    // Lanes: an icon goes in the first lane whose last icon is far enough
    // left of it (in percent of the width) not to overlap.
    const GAP: f64 = 3.5;
    let mut lanes: Vec<f64> = Vec::new();
    let mut pts = String::new();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&k| times[k]);
    for k in order {
        let (outlet, _) = &rows[k];
        let x = (times[k] - t0) as f64 / span as f64 * 100.0;
        let lane = match lanes.iter().position(|last| x - last >= GAP) {
            Some(l) => {
                lanes[l] = x;
                l
            }
            None => {
                lanes.push(x);
                lanes.len() - 1
            }
        };
        let origin = *outlet == c.origin_outlet_id;
        let mark = crate::render::outlet_icon(input, outlet, origin)
            .unwrap_or_else(|| format!("<span class=\"dot{}\"></span>", if origin { " kg-origin" } else { "" }));
        let has_page = input.outlets.contains_key(*outlet) || input.claims.contains_key(*outlet);
        let (open, close) = if has_page {
            (format!("<a class=\"pt\" href=\"/outlet/{}/\"", esc(outlet)), "</a>")
        } else {
            ("<span class=\"pt\"".to_string(), "</span>")
        };
        let _ = write!(
            pts,
            "{open} style=\"left: {x:.2}%; top: {}px\" title=\"{} · {}\">{mark}{close}",
            lane * 20,
            esc(input.outlet_name(outlet)),
            if times[k] == t0 { "first".to_string() } else { delta(times[k] - t0) },
        );
    }
    let height = lanes.len() * 20;
    format!(
        "<div class=\"kg-track\" role=\"img\" aria-label=\"{} outlets over {}, in order: {}\"><div class=\"lanes\" style=\"height: {height}px\">{pts}</div><div class=\"axis\"><span>{}</span><span>{} later</span></div></div>",
        rows.len(),
        delta(span).trim_start_matches('+'),
        esc(&rows.iter().map(|(o, _)| input.outlet_name(o)).collect::<Vec<_>>().join(", ")),
        time_el(t0, input.export_time),
        delta(end - t0).trim_start_matches('+')
    )
}

/// A fact's value for the compact table: Wikidata's year-precision dates
/// ("1982-01-01") read as the year.
fn short_value(c: &crate::input::Claim) -> String {
    let v = c.value.trim();
    if matches!(c.field.as_str(), "founded" | "domain_registered") && v.len() >= 4 {
        if v.ends_with("-01-01") || c.field == "domain_registered" {
            return v[..4].to_string();
        }
    }
    v.to_string()
}

/// "About the outlets" as one row per outlet (design §5, dense claims):
/// icon and name, then reliability, owner, country, founded and domain
/// age as short values. Each value's provenance (source, licence,
/// retrieved date, as-of) is its hover title; the sources used are named
/// once under the heading; the outlet's page has every fact in full.
fn outlets_table(input: &Input, rows: &[(&str, Vec<&Item>)]) -> String {
    let mut used: Vec<&str> = Vec::new();
    let mut body = String::new();
    for (outlet, _) in rows {
        let claims: Vec<&crate::input::Claim> = input
            .claims
            .get(*outlet)
            .map(|v| v.iter().filter(|c| OUTLET_FIELDS.contains(&c.field.as_str())).collect())
            .unwrap_or_default();
        let mut facts: Vec<String> = Vec::new();
        for field in OUTLET_FIELDS {
            let mut vals: Vec<(String, &crate::input::Claim)> = Vec::new();
            for c in claims.iter().filter(|c| c.field == *field) {
                let v = short_value(c);
                if !vals.iter().any(|(x, _)| *x == v) {
                    vals.push((v, c));
                }
            }
            if *field == "domain_registered" {
                vals.sort_by(|a, b| a.0.cmp(&b.0));
                vals.truncate(1);
            }
            if vals.is_empty() {
                continue;
            }
            let label = match *field {
                "founded" => "founded ",
                "domain_registered" => "domain since ",
                "owner" => "owned by ",
                _ => "",
            };
            let shown: Vec<String> = vals
                .iter()
                .map(|(v, c)| {
                    if !used.contains(&c.source_id.as_str()) {
                        used.push(&c.source_id);
                    }
                    let src = input.sources.get(&c.source_id);
                    let title = format!(
                        "{} · {} · retrieved {}{}",
                        src.map(|s| s.name.as_str()).unwrap_or(&c.source_id),
                        src.map(|s| s.licence.as_str()).unwrap_or(""),
                        crate::render::date(c.retrieved_at),
                        c.as_of.as_ref().map(|a| format!(" · as of {a}")).unwrap_or_default()
                    );
                    format!(
                        "<a class=\"fact\" href=\"{}\" rel=\"noopener\" title=\"{}\">{}</a>",
                        esc(&c.ref_url),
                        esc(&title),
                        esc(v)
                    )
                })
                .collect();
            facts.push(format!("<span data-field=\"{field}\">{label}{}</span>", shown.join(", ")));
        }
        let _ = write!(
            body,
            "<li data-outlet=\"{}\">{}<span class=\"facts\">{}</span></li>",
            esc(outlet),
            crate::render::outlet_badge(input, outlet),
            if facts.is_empty() {
                "<span class=\"none\">no facts yet</span>".to_string()
            } else {
                facts.join("<span class=\"sep\"> · </span>")
            }
        );
    }
    let sources = used
        .iter()
        .map(|id| {
            let s = input.sources.get(*id);
            format!(
                "{} ({})",
                esc(s.map(|s| s.name.as_str()).unwrap_or(id)),
                esc(s.map(|s| s.licence.as_str()).unwrap_or(""))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "<h2>About the outlets</h2><p class=\"kg-muted\">From Kagami ledger{}. Hover a fact for its source and date; each outlet's page has the full record.</p><ul class=\"kg-outlet-facts\">{body}</ul>",
        if sources.is_empty() { String::new() } else { format!(": {sources}") }
    )
}

/// The item's text as it stood just before `ts`: the latest revision
/// seen at or before it. None when the export carries no revisions
/// for the item.
fn text_before(input: &Input, item_id: u64, ts: i64) -> Option<&crate::input::Revision> {
    input
        .revisions
        .get(&item_id)?
        .iter()
        .rev()
        .find(|r| r.seen_at < ts)
}

/// The method lines (docs/steward.md §3), the same in HTML and Markdown.
/// An export from before confidence was recorded says so rather than
/// showing zeros.
pub fn method_lines(c: &Cluster) -> Vec<String> {
    let Some(k) = &c.confidence else {
        return vec!["Clustering confidence was not recorded in this export.".to_string()];
    };
    let mut v = vec![format!(
        "Members joined by text similarity at or above {:.2} (cosine to the story's centroid).",
        k.join_threshold
    )];
    match (k.weakest_join, k.join_margin) {
        (Some(w), Some(m)) => v.push(format!(
            "Weakest join {w:.2}, {m:.2} above the threshold{}.",
            if m < 0.05 {
                "; one borderline join holds this story together"
            } else {
                ""
            }
        )),
        _ => v.push("No member has a recorded join similarity.".to_string()),
    }
    if k.unscored > 0 {
        v.push(format!(
            "{} without a recorded similarity (the first item, or moved here by a merge).",
            n(k.unscored as usize, "member")
        ));
    }
    v.push(if k.merged {
        "Formed by merging two stories whose centroids met the merge threshold.".to_string()
    } else {
        "Not merged from another story.".to_string()
    });
    if !k.languages.is_empty() {
        v.push(format!(
            "{}: {}.",
            n(k.languages.len(), "language"),
            k.languages.join(", ")
        ));
    }
    v
}

pub fn story(input: &Input, c: &Cluster) -> Twins {
    let f: ClusterFacts = cluster_facts(
        c,
        &input.items,
        &input.events,
        input.export_time,
        crate::derive::DAY,
    );
    let rows = by_outlet(input, c);
    let t0 = rows
        .first()
        .and_then(|(_, v)| v.first())
        .map(|i| when(i))
        .unwrap_or(c.created_at);
    let path = format!("/story/{}/", c.id);

    // ---- HTML
    let mut b = String::new();
    let scale = crate::render::spark_scale([f.spark.as_slice()]);
    b.push_str(&cluster_card(
        input,
        c,
        &f.spark,
        scale,
        true,
        false,
        Some(&f),
    ));
    b.push_str("<h2>Timeline</h2>");
    b.push_str(&track(input, c, &rows, t0));
    b.push_str("<p class=\"kg-muted\">Each outlet once, in the order it published, with its first headline. Hover a time for when it was published and when Kagami picked it up.</p><ol class=\"timeline\">");
    let mut last_pickup: Option<i64> = None;
    for (k, (outlet, items)) in rows.iter().enumerate() {
        let first = &items[0];
        let at = first.joined_at.unwrap_or(first.fetched_at);
        let _ = last_pickup.replace(at);
        // Published and picked-up times are the time column's hover, not a
        // line of their own on every row.
        let when_title = format!(
            "{}{} · picked up {}",
            if k == 0 { "first".to_string() } else { format!("{} after the first outlet", delta(when(first) - t0)) },
            if first.published_at > 0 { format!(" · published {}", datetime(first.published_at)) } else { String::new() },
            datetime(at)
        );
        let _ = write!(
            b,
            "<li{} data-outlet=\"{}\"><span class=\"when\" title=\"{}\">{}</span><div><span class=\"outlet\">{}</span>{}",
            if k == 0 { " class=\"first\"" } else { "" },
            esc(outlet),
            esc(&when_title),
            delta(when(first) - t0),
            if k == 0 { crate::render::origin_link(input, outlet) } else { crate::render::outlet_badge(input, outlet) },
            match f.copies_of.get(&first.id) {
                Some(src) => format!(
                    " <span class=\"copy\" title=\"Same text as {}'s\">copy of {}</span>",
                    esc(input.outlet_name(src)),
                    esc(input.outlet_name(src))
                ),
                None if f.copied.contains(&first.id) => " <span class=\"copy\">carried by others</span>".to_string(),
                None => String::new(),
            }
        );
        let _ = write!(
            b,
            "<span class=\"hl\"><a href=\"{}\" rel=\"noopener\">{}</a></span>",
            esc(&first.canonical_url),
            esc(&first.title)
        );
        for alt in items.iter().skip(1).filter(|i| i.title != first.title) {
            let _ = write!(
                b,
                "<span class=\"alt\">also: <a href=\"{}\" rel=\"noopener\">{}</a></span>",
                esc(&alt.canonical_url),
                esc(&alt.title)
            );
        }
        b.push_str("</div></li>");
    }
    b.push_str("</ol>");

    if !c.syndication.is_empty() {
        b.push_str("<h2>Copies</h2><p class=\"kg-muted\">The same text at more than one outlet: a copy, not independent coverage.</p><ul class=\"stories\">");
        for g in &c.syndication {
            let outlets: Vec<String> = g
                .item_ids
                .iter()
                .filter_map(|id| input.items.get(id))
                .map(|i| outlet_link(input, &i.outlet_id))
                .collect();
            let _ = write!(
                b,
                "<li><span>{}</span><span class=\"n kg-muted\">first at {} · spread over {}</span></li>",
                outlets.join(", "),
                outlet_link(input, &g.first_outlet_id),
                delta(g.spread_secs as i64).trim_start_matches('+')
            );
        }
        b.push_str("</ul>");
    }

    // Coverage by a named source's labels (plan §3.2): every source
    // and field pair that labels at least two outlets in the story.
    let outlet_ids: Vec<&str> = rows.iter().map(|(o, _)| *o).collect();
    let bars: Vec<crate::render::LabelBar> = LABEL_SOURCES
        .iter()
        .filter_map(|(src, field)| crate::render::label_bar(input, &outlet_ids, src, field))
        .collect();
    let strip = crate::render::audience_strip(input, &outlet_ids, true);
    if !bars.is_empty() || !strip.is_empty() {
        b.push_str("<h2>Coverage by label</h2>");
        for bar in &bars {
            b.push_str(&crate::render::label_bar_html(input, bar));
        }
        if !strip.is_empty() {
            b.push_str("<div class=\"kg-audience-block\"><p class=\"kg-muted\" title=\"Who shared each outlet's links among US Twitter users in 2017, from -1 (Democrats, left) to +1 (Republicans, right): a measure of audiences, not of this story's coverage, and a US measure even for outlets elsewhere.\">Audience lean, left to right · <a class=\"kg-link-quiet\" href=\"https://dataverse.harvard.edu/dataset.xhtml?persistentId=doi:10.7910/DVN/QAN5VX\" rel=\"noopener\">Robertson et al. 2018</a>, CC0</p>");
            b.push_str(&strip);
            b.push_str("</div>");
        }
    }

    if !f.revisions.is_empty() || !f.withdrawals.is_empty() {
        b.push_str("<h2>Changes</h2><p class=\"kg-muted\">Text that changed or disappeared after publication, as seen in later feed fetches.</p><ul class=\"stories changes\">");
        for e in f.revisions.iter().chain(f.withdrawals.iter()) {
            let it = e.item_id.and_then(|id| input.items.get(&id));
            let before = e.item_id.and_then(|id| text_before(input, id, e.ts));
            let _ = write!(
                b,
                "<li><div><span>{} {}</span>{}</div><span class=\"n kg-muted\">{}</span></li>",
                if e.kind == "revised" {
                    "Revised:"
                } else {
                    "Withdrawn:"
                },
                it.map(|i| format!(
                    "{} <a href=\"{}\" rel=\"noopener\">{}</a>",
                    outlet_link(input, &i.outlet_id),
                    esc(&i.canonical_url),
                    esc(&i.title)
                ))
                .unwrap_or_default(),
                match (e.kind.as_str(), before, it) {
                    ("revised", Some(old), Some(i)) if old.title != i.title =>
                        format!("<span class=\"alt\">was: {}</span>", esc(&old.title)),
                    ("revised", Some(old), Some(i)) if old.summary != i.summary => format!(
                        "<span class=\"alt\">summary changed; was: {}</span>",
                        esc(&old.summary)
                    ),
                    ("revised", None, _) =>
                        "<span class=\"alt\">earlier text not in this export</span>".to_string(),
                    _ => String::new(),
                },
                time_el(e.ts, input.export_time)
            );
        }
        b.push_str("</ul>");
    }

    // Method (docs/steward.md §3): how sure the clustering was, as
    // counts over the members. Shown so a borderline story says so.
    // Collapsed, and only the lines that say something about this story;
    // the JSON and Markdown twins keep all of them.
    let method: Vec<String> = method_lines(c)
        .into_iter()
        .filter(|l| {
            !(l.starts_with("No member has a recorded")
                || l.contains("without a recorded similarity")
                || l.starts_with("Not merged")
                || l.starts_with("1 language"))
        })
        .collect();
    b.push_str("<details class=\"kg-method\" data-section=\"method\"><summary>How this story was grouped</summary><ul>");
    for l in &method {
        let _ = write!(b, "<li>{}</li>", esc(l));
    }
    b.push_str("</ul><p class=\"kg-muted\"><a href=\"/about/#story\">What a story is</a></p></details>");

    b.push_str(&outlets_table(input, &rows));

    let head = format!(
        "<script type=\"application/ld+json\">{}</script>\n",
        json!({
            "@context": "https://schema.org",
            "@type": "ItemList",
            "name": c.primary_title,
            "numberOfItems": c.item_count,
            "itemListElement": rows.iter().enumerate().map(|(k, (_, items))| json!({
                "@type": "ListItem",
                "position": k + 1,
                "url": items[0].canonical_url,
                "name": items[0].title,
                "datePublished": if items[0].published_at > 0 { Some(iso(items[0].published_at)) } else { None },
            })).collect::<Vec<_>>()
        })
    );
    let description = format!(
        "{} covered by {}; first published by {}.",
        c.primary_title,
        n(c.outlet_count as usize, "outlet"),
        input.outlet_name(&c.origin_outlet_id)
    );
    let page_html = html(&Page {
        title: &c.primary_title,
        description: &description,
        path: &path,
        wide: false,
        head: &head,
        body: &b,
        status: &format!(
            "story · built from an export at {}",
            datetime(input.export_time)
        ),
    });

    // ---- JSON: the cluster, the derived facts, the items, the claims shown.
    let names: HashMap<&str, &str> = rows
        .iter()
        .map(|(o, _)| (*o, input.outlet_name(o)))
        .collect();
    let page_json = json!({
        "cluster": c,
        "outlets_6h": f.outlets_6h,
        "outlets_24h": f.outlets_24h,
        "spark": f.spark,
        "outlet_names": names,
        "timeline": rows.iter().map(|(o, items)| json!({
            "outlet_id": o,
            "items": items,
            "copy_of": f.copies_of.get(&items[0].id),
            "copied": f.copied.contains(&items[0].id),
        })).collect::<Vec<_>>(),
        "coverage_by_label": bars.iter().map(|bar| json!({
            "source_id": bar.source_id, "field": bar.field,
            "labels": bar.segments.iter().map(|(l, ids)| json!({"label": l, "outlets": ids})).collect::<Vec<_>>(),
            "not_rated": bar.unrated,
        })).collect::<Vec<_>>(),
        "revisions": f.revisions,
        "withdrawals": f.withdrawals,
        "claims": rows.iter().map(|(o, _)| (o.to_string(), input.claims.get(*o).map(|v| v.iter().filter(|c| OUTLET_FIELDS.contains(&c.field.as_str())).collect::<Vec<_>>()).unwrap_or_default())).collect::<HashMap<_, _>>(),
        "export_time": input.export_time,
    });

    // ---- Markdown
    let mut md = format!("# {}\n\n", c.primary_title);
    let _ = writeln!(
        md,
        "Story `{}`. {} · {} · first published by {} · first seen {}{}. As of {}.",
        c.id,
        n(c.outlet_count as usize, "outlet"),
        n(c.item_count as usize, "item"),
        input.outlet_name(&c.origin_outlet_id),
        datetime(c.created_at),
        if c.syndication.is_empty() {
            String::new()
        } else {
            format!(" · {}", n(c.syndication.len(), "syndication group"))
        },
        datetime(input.export_time)
    );
    md.push_str("\nMembership is by text similarity, not by a person; treat it as evidence. No outlet here carries a rating; the facts about each outlet below are a source's, with its licence and date.\n\n## Timeline\n\n");
    let mut last_pickup: Option<i64> = None;
    for (k, (outlet, items)) in rows.iter().enumerate() {
        let first = &items[0];
        let at = first.joined_at.unwrap_or(first.fetched_at);
        let show_pickup = last_pickup != Some(at);
        last_pickup = Some(at);
        let mut times = Vec::new();
        if first.published_at > 0 {
            times.push(format!("published {}", datetime(first.published_at)));
        }
        if show_pickup || first.published_at == 0 {
            times.push(format!("picked up {}", datetime(at)));
        }
        let _ = writeln!(
            md,
            "- {}**{}**: [{}]({}) · {}{}",
            if k == 0 {
                "first · ".to_string()
            } else {
                format!("{} · ", delta(when(first) - t0))
            },
            input.outlet_name(outlet),
            first.title,
            first.canonical_url,
            times.join(" · "),
            match f.copies_of.get(&first.id) {
                Some(src) => format!(" · copy of {}", input.outlet_name(src)),
                None if f.copied.contains(&first.id) => " · carried by others".to_string(),
                None => String::new(),
            }
        );
        for alt in items.iter().skip(1).filter(|i| i.title != first.title) {
            let _ = writeln!(
                md,
                "  - also from this outlet: [{}]({})",
                alt.title, alt.canonical_url
            );
        }
    }
    if !c.syndication.is_empty() {
        md.push_str("\n## Copies\n\n");
        for g in &c.syndication {
            let outlets: Vec<&str> = g
                .item_ids
                .iter()
                .filter_map(|id| input.items.get(id))
                .map(|i| input.outlet_name(&i.outlet_id))
                .collect();
            let _ = writeln!(
                md,
                "- {} · first at {} · spread over {}",
                outlets.join(", "),
                input.outlet_name(&g.first_outlet_id),
                delta(g.spread_secs as i64).trim_start_matches('+')
            );
        }
    }
    if !bars.is_empty() {
        md.push_str("\n## Coverage by label\n\n");
        for bar in &bars {
            md.push_str(&crate::render::label_bar_md(input, bar));
            md.push('\n');
        }
    }
    if !f.revisions.is_empty() || !f.withdrawals.is_empty() {
        md.push_str("\n## Changes\n\n");
        for e in f.revisions.iter().chain(f.withdrawals.iter()) {
            let it = e.item_id.and_then(|id| input.items.get(&id));
            let before = e.item_id.and_then(|id| text_before(input, id, e.ts));
            let _ = writeln!(
                md,
                "- {} {} at {}{}",
                if e.kind == "revised" {
                    "Revised:"
                } else {
                    "Withdrawn:"
                },
                it.map(|i| format!(
                    "{} [{}]({})",
                    input.outlet_name(&i.outlet_id),
                    i.title,
                    i.canonical_url
                ))
                .unwrap_or_default(),
                datetime(e.ts),
                match (e.kind.as_str(), before, it) {
                    ("revised", Some(old), Some(i)) if old.title != i.title =>
                        format!(" · was: {}", old.title),
                    ("revised", Some(old), Some(i)) if old.summary != i.summary =>
                        " · summary changed".to_string(),
                    ("revised", None, _) => " · earlier text not in this export".to_string(),
                    _ => String::new(),
                }
            );
        }
    }
    md.push_str("\n## Method\n\nKagami feed's own judgement of this story, as counts.\n\n");
    // Every line, including the ones the HTML leaves out as noise.
    for l in &method_lines(c) {
        let _ = writeln!(md, "- {l}");
    }
    md.push_str("\n## About the outlets\n");
    for (outlet, _) in &rows {
        let _ = write!(md, "\n### {}\n", input.outlet_name(outlet));
        let claims: Vec<_> = input
            .claims
            .get(*outlet)
            .map(|v| {
                v.iter()
                    .filter(|c| OUTLET_FIELDS.contains(&c.field.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if claims.is_empty() {
            md.push_str("\nNo claims yet.\n");
            continue;
        }
        for (field, cs) in grouped_claims(&claims) {
            let _ = write!(md, "\n{}\n\n", field_label(field));
            for c in cs {
                md.push_str(&claim_md(input, c));
            }
        }
    }

    Twins {
        html: page_html,
        json: page_json,
        md,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn story_page_has_timeline_copies_and_outlet_facts() {
        let i = fixture_input();
        let c = &i.clusters[&1001];
        let t = story(&i, c);
        assert!(t.html.contains("<title>Story 1001 · Kagami</title>"));
        let wire = t
            .html
            .find("<li class=\"first\" data-outlet=\"wire\">")
            .unwrap();
        let gz = t.html.find("<li data-outlet=\"gazette\">").unwrap();
        assert!(wire < gz, "join order");
        assert!(t.html.contains("copy of Wire"), "gazette's item is a copy");
        assert!(t.html.contains("carried by others"));
        assert!(t.html.contains("<h2>Copies</h2>"));
        assert!(
            !t.html.contains("<h2>Changes</h2>"),
            "no revisions in this cluster"
        );
        assert!(
            t.html.contains("data-field=\"owner\"")
                && t.html.contains("data-field=\"reliability\"")
        );
        assert!(t.html.contains("\"@type\":\"ItemList\""));
        assert!(
            !t.html.contains("Rating"),
            "no rating type in JSON-LD, ever"
        );
        assert_eq!(t.json["cluster"]["id"], 1001);
        assert_eq!(t.json["timeline"][1]["copy_of"], "wire");
        assert!(t.md.starts_with("# Story 1001\n"));
        assert!(t
            .md
            .contains("- first · **Wire**: [Story 1001](https://wire.example/1001)"));
        assert!(t.md.contains("- +1.0h · **Gazette**"), "{}", t.md);
        assert!(t.md.contains("· copy of Wire"));
        assert!(t
            .md
            .contains("### Wire\n\nReliability\n\n**Generally reliable**"));
        let rev = story(&i, &i.clusters[&1002]);
        assert!(rev.html.contains("<h2>Changes</h2>") && rev.html.contains("Revised:"));
        assert!(rev
            .html
            .contains("<span class=\"alt\">was: Story 1002, as first seen</span>"));
        assert!(rev.md.contains("- Revised: Wire [Story 1002](https://wire.example/1002) at 2025-08-24 14:53 UTC · was: Story 1002, as first seen"));
        let wd = story(&i, &i.clusters[&1022]);
        assert!(wd.html.contains("Withdrawn:") && !wd.html.contains("was:"));
    }

    #[test]
    fn coverage_by_label_shows_the_sources_map_when_two_outlets_are_rated() {
        let i = fixture_input();
        let t = story(&i, &i.clusters[&1001]);
        assert!(t.html.contains("<h2>Coverage by label</h2>"));
        let bar = t.html.find("<div class=\"kg-labelbar\" data-source=\"wikipedia_rsp\" data-field=\"reliability\">").unwrap();
        let gr = t.html[bar..]
            .find("Generally reliable <span class=\"n\">1</span>")
            .unwrap();
        let nc = t.html[bar..]
            .find("No consensus <span class=\"n\">1</span>")
            .unwrap();
        assert!(gr < nc, "the source's own order, not alphabetical");
        assert!(
            !t.html.contains("class=\"seg none\""),
            "both outlets rated: no unrated segment"
        );
        assert_eq!(
            t.json["coverage_by_label"][0]["labels"][0]["label"],
            "Generally reliable"
        );
        assert!(t.md.contains("## Coverage by label\n\nWikipedia: Reliable sources/Perennial sources on reliability (licence CC-BY-SA-4.0); the source's map, not ours.\n\n- Generally reliable: 1 outlet · Wire\n- No consensus: 1 outlet · Gazette\n"), "{}", t.md);
        // a single-outlet story has no bar; its lean is stated, as unknown
        // when the source does not score it
        let one = story(&i, &i.clusters[&1002]);
        assert!(!one.html.contains("kg-labelbar"));
        assert!(one.json["coverage_by_label"].as_array().unwrap().is_empty());
        // bias from a source that forbids redistribution never reaches lens
        assert!(!t.html.contains("data-source=\"allsides\""));
    }

    #[test]
    fn method_section_states_confidence_or_its_absence() {
        use crate::input::Confidence;
        let i = fixture_input();
        let mut c = i.clusters[&1001].clone();
        // The fixture export predates confidence: said, not zeroed.
        let t = story(&i, &c);
        assert!(t.html.contains("<summary>How this story was grouped</summary>"));
        assert!(t.html.contains("not recorded in this export"));
        assert!(t.md.contains("## Method"));
        assert!(t.json["cluster"].get("confidence").is_none());

        c.confidence = Some(Confidence {
            join_threshold: 0.72,
            weakest_join: Some(0.74),
            join_margin: Some(0.02),
            unscored: 1,
            merged: true,
            languages: vec!["en".into(), "ja".into()],
        });
        let t = story(&i, &c);
        assert!(t.html.contains("Weakest join 0.74, 0.02 above the threshold; one borderline join holds this story together."));
        assert!(!t.html.contains("without a recorded similarity"), "not reader information");
        assert!(t.md.contains("1 member without a recorded similarity"), "kept for machines");
        assert!(t.html.contains("Formed by merging"));
        assert!(t.html.contains("2 languages: en, ja."));
        assert!(
            (t.json["cluster"]["confidence"]["join_margin"]
                .as_f64()
                .unwrap()
                - 0.02)
                .abs()
                < 1e-6
        );
        assert!(t.md.contains("- Weakest join 0.74"));
        assert!(!t.html.contains("Rating"));
    }

    #[test]
    fn first_poll_ties_read_in_published_order_and_first_follows_the_origin() {
        // Everything picked up in one poll; the outlet with the larger
        // item id published earlier and is feed's origin.
        let mut i = fixture_input();
        let cid = 1001;
        let (a, b) = (1001u64, 51001u64);
        i.items.get_mut(&a).unwrap().published_at = 1_756_000_000 + 9 * 3600;
        i.items.get_mut(&a).unwrap().joined_at = Some(1_756_100_000);
        i.items.get_mut(&b).unwrap().published_at = 1_756_000_000;
        i.items.get_mut(&b).unwrap().joined_at = Some(1_756_100_000);
        let c = i.clusters.get_mut(&cid).unwrap();
        c.origin_outlet_id = "gazette".into();
        c.origin_item_id = b;
        for j in &mut c.timeline {
            j.joined_at = 1_756_100_000;
        }
        let c = i.clusters[&cid].clone();
        let t = story(&i, &c);
        let gz = t
            .html
            .find("<li class=\"first\" data-outlet=\"gazette\">")
            .unwrap();
        let wire = t.html.find("<li data-outlet=\"wire\">").unwrap();
        assert!(gz < wire, "origin first even though its item id is larger");
        assert!(
            t.html.contains(">+9.0h<"),
            "offset from the origin's published time: {}",
            t.html
        );
        assert!(!t.html.contains("class=\"times\""), "times are the hover, not a line per row");
        assert!(t.html.contains("title=\"first · published "), "published and picked-up times in the time column's title");
        assert!(
            t.md.contains("- first · **Gazette**") && t.md.contains("- +9.0h · **Wire**"),
            "{}",
            t.md
        );
    }
}
