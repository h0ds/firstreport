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
    claim_card_compact, claim_md, cluster_card, datetime, delta, esc, field_label, grouped_claims,
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

pub fn story(input: &Input, c: &Cluster) -> Twins {
    let f: ClusterFacts = cluster_facts(c, &input.items, &input.events, input.export_time);
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
    b.push_str(&cluster_card(input, c, &f.spark, scale, true, false));
    b.push_str("<h2>Timeline</h2><p class=\"kg-muted\">Each outlet once, in the order they published, with the headline each ran first. Other articles from the same outlet sit beneath. Times are what the outlet claimed; pick-up is when Kagami saw it, shown where it differs from the row above.</p><ol class=\"timeline\">");
    let mut last_pickup: Option<i64> = None;
    for (k, (outlet, items)) in rows.iter().enumerate() {
        let first = &items[0];
        let at = first.joined_at.unwrap_or(first.fetched_at);
        let show_pickup = last_pickup != Some(at);
        last_pickup = Some(at);
        let _ = write!(
            b,
            "<li{} data-outlet=\"{}\"><span class=\"when\" title=\"{} after the first outlet\">{}</span><div><span class=\"outlet\">{}</span>{}",
            if k == 0 { " class=\"first\"" } else { "" },
            esc(outlet),
            if k == 0 { "0 min".to_string() } else { delta(when(first) - t0) },
            delta(when(first) - t0),
            if k == 0 { crate::render::origin_link(input, outlet) } else { outlet_link(input, outlet) },
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
        let mut times = Vec::new();
        if first.published_at > 0 {
            times.push(format!(
                "published {}",
                time_el(first.published_at, input.export_time)
            ));
        }
        if show_pickup || first.published_at == 0 {
            times.push(format!("picked up {}", time_el(at, input.export_time)));
        }
        let _ = write!(b, "<span class=\"times\">{}</span>", times.join(" · "));
        for alt in items.iter().skip(1).filter(|i| i.title != first.title) {
            let _ = write!(
                b,
                "<span class=\"alt\">also from this outlet: <a href=\"{}\" rel=\"noopener\">{}</a></span>",
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
    if !bars.is_empty() {
        b.push_str("<h2>Coverage by label</h2><p class=\"kg-muted\">How a named source labels the outlets in this story. Outlets the source has not rated are shown as such and never redistributed.</p>");
        for bar in &bars {
            b.push_str(&crate::render::label_bar_html(input, bar));
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

    b.push_str("<h2>About the outlets</h2><p class=\"kg-muted\">From Kagami ledger, each fact with its source; the outlet's page has the full record. Outlets with no claims yet are listed as such.</p>");
    for (outlet, _) in &rows {
        let _ = write!(b, "<h3>{}</h3>", outlet_link(input, outlet));
        let claims: Vec<_> = input
            .claims
            .get(*outlet)
            .map(|v| {
                v.iter()
                    .filter(|c| OUTLET_FIELDS.contains(&c.field.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        if claims.is_empty() {
            b.push_str("<div class=\"empty\">No claims yet.</div>");
            continue;
        }
        let owned: Vec<_> = claims.into_iter().cloned().collect();
        for (field, cs) in grouped_claims(&owned) {
            let _ = write!(
                b,
                "<div class=\"kg-muted\">{}</div>",
                esc(&field_label(field))
            );
            for c in cs {
                b.push_str(&claim_card_compact(input, c));
            }
        }
    }

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
        // a single-outlet story has no bar
        let one = story(&i, &i.clusters[&1002]);
        assert!(!one.html.contains("Coverage by label"));
        assert!(one.json["coverage_by_label"].as_array().unwrap().is_empty());
        // bias from a source that forbids redistribution never reaches lens
        assert!(!t.html.contains("data-source=\"allsides\""));
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
        assert_eq!(
            t.html.matches("picked up ").count(),
            1,
            "identical pick-up shown once"
        );
        assert!(
            t.md.contains("- first · **Gazette**") && t.md.contains("- +9.0h · **Wire**"),
            "{}",
            t.md
        );
    }
}
