//! `/outlet/<id>/` (plan §3.3): every ledger claim grouped by field with
//! its provenance line, then the outlet's stories in the window with
//! the counts ledger's feed_behaviour importer also reports. Counts,
//! never a rating.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::derive::OutletFacts;
use crate::input::{Input, Outlet};
use crate::render::{
    claim_card, claim_md, datetime, esc, field_label, grouped_claims, html, n, outlet_link,
    time_el, Page,
};

/// Link the owner claim to the owner's own page when the owner is a
/// registry outlet (plan §10): matched by name, case-insensitively.
fn owner_outlet<'a>(input: &'a Input, value: &str) -> Option<&'a str> {
    input
        .outlets
        .values()
        .find(|o| o.name.eq_ignore_ascii_case(value.trim()))
        .map(|o| o.id.as_str())
}

pub fn outlet(input: &Input, o: &Outlet, facts: &OutletFacts, window_days: i64) -> Twins {
    let path = format!("/outlet/{}/", o.id);
    let claims = input.claims.get(&o.id).cloned().unwrap_or_default();
    let groups = grouped_claims(&claims);
    let name = input.outlet_name(&o.id);

    // ---- HTML
    let mut b = String::new();
    let _ = write!(
        b,
        "<h1 data-id=\"{}\">{}</h1><p class=\"kg-muted\">{}{}{} · every value below is a source's, never ours</p>",
        esc(&o.id),
        esc(name),
        esc(&o.domains.join(", ")),
        if o.domains.is_empty() { "" } else { " · " },
        n(claims.len(), "claim")
    );
    if groups.is_empty() {
        b.push_str(
            "<div class=\"empty\">No claims yet. Kagami ledger has not assessed this outlet.</div>",
        );
    }
    // Fields as rows: the name on the left, its claims on the right
    // (design §5, dense claims), not a heading per field.
    // Values from one source with the same provenance share a block, and
    // a note repeated across fields is shown the first time only.
    let mut seen_notes = std::collections::HashSet::new();
    for (field, cs) in &groups {
        let _ = write!(
            b,
            "<section class=\"kg-field\" data-field=\"{}\"><div class=\"fname\">{}</div><div>",
            esc(field),
            esc(&field_label(field))
        );
        let mut merged: Vec<crate::input::Claim> = Vec::new();
        for c in cs.iter() {
            let same = merged.iter_mut().find(|m| {
                m.source_id == c.source_id
                    && m.ref_url == c.ref_url
                    && m.as_of == c.as_of
                    && crate::render::date(m.retrieved_at) == crate::render::date(c.retrieved_at)
                    && m.data.as_ref().and_then(|d| d.get("summary")).is_none()
            });
            match same {
                Some(m) if m.value != c.value => m.value = format!("{} · {}", m.value, c.value),
                Some(_) => {}
                None => merged.push((*c).clone()),
            }
        }
        for mut m in merged {
            if let Some(n) = &m.note {
                if !seen_notes.insert(n.clone()) {
                    m.note = None;
                }
            }
            let c = &m;
            b.push_str(&claim_card(input, c));
            if *field == "owner" {
                if let Some(id) = owner_outlet(input, &c.value) {
                    let _ = write!(
                        b,
                        "<p class=\"kg-muted\">{} is itself in the registry: {}</p>",
                        esc(&c.value),
                        outlet_link(input, id)
                    );
                }
            }
        }
        b.push_str("</div></section>");
    }
    let _ = write!(
        b,
        "<h2>Stories</h2><p class=\"kg-muted\">In the {window_days} days to {}: {}, in {}. First to {} of the {} other outlets also carried. {} of its items carried text first published elsewhere; {} were carried by others. Counts from Kagami feed, not a rating.</p>",
        crate::render::day_el(input.export_time, input.export_time),
        n(facts.items as usize, "item"),
        n(facts.clusters.len(), "story"),
        facts.first_to,
        n(facts.shared_stories as usize, "story"),
        facts.copies,
        facts.copied
    );
    if facts.clusters.is_empty() {
        b.push_str("<div class=\"empty\">No stories in the window.</div>");
    } else {
        b.push_str("<ul class=\"stories\">");
        for id in &facts.clusters {
            let c = &input.clusters[id];
            let _ = write!(
                b,
                "<li data-id=\"{}\"><a href=\"/story/{}/\">{}</a>{}<span class=\"n kg-muted\">{} · {}</span></li>",
                c.id,
                c.id,
                esc(&c.primary_title),
                if c.origin_outlet_id == o.id && c.outlet_count > 1 { " <span class=\"kg-tag\">first</span>" } else { "" },
                n(c.outlet_count as usize, "outlet"),
                time_el(c.created_at, input.export_time)
            );
        }
        b.push_str("</ul>");
    }

    let get = |field: &str| {
        claims
            .iter()
            .find(|c| c.field == field)
            .map(|c| c.value.clone())
    };
    let mut same_as: Vec<String> = Vec::new();
    if let Some(q) = get("wikidata_id") {
        same_as.push(format!("https://www.wikidata.org/wiki/{q}"));
    }
    if let Some(w) = get("wikipedia") {
        same_as.push(w);
    }
    let head = format!(
        "<script type=\"application/ld+json\">{}</script>\n",
        json!({
            "@context": "https://schema.org",
            "@type": "Organization",
            "name": name,
            "url": get("website").or_else(|| o.domains.first().map(|d| format!("https://{d}/"))),
            "sameAs": same_as,
        })
    );
    let description = format!(
        "{}: {} from Kagami ledger, each with its source, and its stories in Kagami feed.",
        name,
        n(claims.len(), "sourced claim")
    );
    let page_html = html(&Page {
        title: name,
        description: &description,
        path: &path,
        wide: false,
        head: &head,
        body: &b,
        status: &format!(
            "outlet · built from an export at {}",
            datetime(input.export_time)
        ),
    });

    // ---- JSON
    let page_json = json!({
        "outlet": o,
        "claims": groups.iter().map(|(f, cs)| (f.to_string(), cs.clone())).collect::<std::collections::BTreeMap<_, _>>(),
        "window_days": window_days,
        "items": facts.items,
        "shared_stories": facts.shared_stories,
        "first_to": facts.first_to,
        "copies": facts.copies,
        "copied": facts.copied,
        "stories": facts.clusters.iter().map(|id| {
            let c = &input.clusters[id];
            json!({"id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count, "created_at": c.created_at, "first": c.origin_outlet_id == o.id})
        }).collect::<Vec<_>>(),
        "export_time": input.export_time,
    });

    // ---- Markdown
    let mut md = format!("# {name}\n\n");
    let _ = writeln!(
        md,
        "Outlet id `{}`{}. {}. Every value below is a source's, never Kagami's; sources that disagree are listed side by side.",
        o.id,
        if o.domains.is_empty() { String::new() } else { format!(", {}", o.domains.join(", ")) },
        n(claims.len(), "claim")
    );
    if groups.is_empty() {
        md.push_str("\nNo claims yet.\n");
    }
    for (field, cs) in &groups {
        let _ = write!(md, "\n## {}\n\n", field_label(field));
        for c in cs {
            md.push_str(&claim_md(input, c));
        }
    }
    let _ = write!(
        md,
        "\n## Stories\n\nIn the {window_days} days to {}: {}, in {}. First to {} of the {} other outlets also carried. {} of its items carried text first published elsewhere; {} were carried by others. Counts, not a rating.\n\n",
        datetime(input.export_time),
        n(facts.items as usize, "item"),
        n(facts.clusters.len(), "story"),
        facts.first_to,
        n(facts.shared_stories as usize, "story"),
        facts.copies,
        facts.copied
    );
    for id in &facts.clusters {
        let c = &input.clusters[id];
        let _ = writeln!(
            md,
            "- [{}](/story/{}/) · {}{} · {}",
            c.primary_title,
            c.id,
            n(c.outlet_count as usize, "outlet"),
            if c.origin_outlet_id == o.id && c.outlet_count > 1 {
                " · first"
            } else {
                ""
            },
            datetime(c.created_at)
        );
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
    use crate::derive::{outlet_facts, DAY};
    use crate::input::fixture_input;

    #[test]
    fn outlet_page_groups_claims_and_lists_stories() {
        let i = fixture_input();
        let facts = outlet_facts(&i, 90 * DAY);
        let t = outlet(&i, &i.outlets["wire"], &facts["wire"], 90);
        assert!(t.html.contains("<h1 data-id=\"wire\">Wire</h1>"));
        let rel = t.html.find("<div class=\"fname\">Reliability</div>").unwrap();
        let owner = t.html.find("<div class=\"fname\">Owner</div>").unwrap();
        let qid = t.html.find("<div class=\"fname\">Wikidata</div>").unwrap();
        assert!(rel < owner && owner < qid, "shared field order");
        assert!(t
            .html
            .contains("First to 30 of the 31 stories other outlets also carried"));
        assert_eq!(
            t.html
                .matches("<span class=\"kg-tag\">first</span>")
                .count(),
            30
        );
        assert!(t.html.contains("\"@type\":\"Organization\""));
        assert!(t.html.contains("https://www.wikidata.org/wiki/Q130879"));
        assert!(!t.html.contains("Rating"));
        assert_eq!(t.json["first_to"], 30);
        assert_eq!(t.json["stories"].as_array().unwrap().len(), 61);
        assert!(t
            .md
            .starts_with("# Wire\n\nOutlet id `wire`, wire.example. 11 claims."));
        assert!(t.md.contains("## Stories\n\nIn the 90 days"));
        let blog = outlet(&i, &i.outlets["blog"], &OutletFacts::default(), 90);
        assert!(blog.html.contains("No claims yet."));
        assert!(blog.html.contains("No stories in the window."));
        assert!(blog.md.contains("No claims yet."));
    }

    #[test]
    fn owner_that_is_a_registry_outlet_is_linked() {
        let i = fixture_input();
        assert_eq!(owner_outlet(&i, "wire"), Some("wire"));
        assert_eq!(owner_outlet(&i, "Wire Holdings"), None);
    }
}
