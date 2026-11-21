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
        "<h1 data-id=\"{}\">{}</h1>{}<p class=\"kg-muted\">{}</p>",
        esc(&o.id),
        esc(name),
        {
            let f = crate::render::outlet_flags(input, &o.id);
            if f.is_empty() {
                f
            } else {
                format!("<p class=\"kg-flags\">{f}</p>")
            }
        },
        {
            // The domain, then Wikipedia and Wikidata as links rather than rows.
            let mut bits: Vec<String> = o
                .domains
                .iter()
                .map(|d| {
                    format!(
                        "<a class=\"kg-link-quiet\" href=\"https://{0}/\" rel=\"noopener\">{0}</a>",
                        esc(d)
                    )
                })
                .collect();
            let find = |f: &str| {
                claims
                    .iter()
                    .find(|c| c.field == f)
                    .map(|c| c.value.clone())
            };
            if let Some(w) = find("wikipedia") {
                bits.push(format!(
                    "<a class=\"kg-link-quiet\" href=\"{}\" rel=\"noopener\">Wikipedia</a>",
                    esc(&w)
                ));
            }
            if let Some(q) = find("wikidata_id") {
                bits.push(format!("<a class=\"kg-link-quiet\" href=\"https://www.wikidata.org/wiki/{}\" rel=\"noopener\">Wikidata</a>", esc(&q)));
            }
            bits.join(" · ")
        }
    );
    // At a glance (design §5, outlet page): a few fields' first values as
    // tiles, each linking to its row with the sources. Only fields the
    // outlet has; the label is the source's, never First Report's verdict.
    let mut glance = String::new();
    for (field, label) in [
        ("reliability", "Reliability"),
        ("state_media", "State media"),
        ("sanctioned", "Sanctions"),
        ("country", "Country"),
        ("founded", "Founded"),
        ("owner", "Owner"),
        ("posting_cadence", "Posts"),
    ] {
        let Some(c) = claims.iter().find(|c| c.field == field) else {
            continue;
        };
        // Wikidata's year-precision dates ("1927-01-01") read as the year.
        let value = if field == "founded" && c.value.len() == 10 && c.value.ends_with("-01-01") {
            c.value[..4].to_string()
        } else {
            c.value.clone()
        };
        let _ = write!(
            glance,
            "<a class=\"tile\" href=\"#f-{field}\"><span class=\"k\">{label}</span><span class=\"v\">{}</span></a>",
            esc(&value)
        );
    }
    let _ = write!(
        glance,
        "<a class=\"tile\" href=\"#stories\"><span class=\"k\">Stories</span><span class=\"v\">{} in {window_days} days</span></a>",
        crate::render::thousands(facts.clusters.len())
    );
    let _ = write!(
        b,
        "<nav class=\"kg-glance\" aria-label=\"At a glance\">{glance}</nav>"
    );
    b.push_str("<div class=\"kg-outlet-page\"><div class=\"facts\">");
    if groups.is_empty() {
        b.push_str(
            "<div class=\"empty\">No claims yet. First Report ledger has not assessed this outlet.</div>",
        );
    }
    // The rows in four groups, each under its own heading, so twenty
    // fields read as a few questions rather than one long list.
    const SECTIONS: [(&str, &str); 4] = [
        (
            "What sources say",
            "Labels from other organisations, each with its source and date.",
        ),
        (
            "Who runs it",
            "Ownership, legal form and origin, from Wikidata and other public records.",
        ),
        (
            "The website",
            "The domain and the site, from registry records and First Report's own checks.",
        ),
        (
            "How it publishes",
            "Counts from First Report feed: measurements, not verdicts.",
        ),
    ];
    let section = |f: &str| match f {
        "owner" | "parent" | "legal_form" | "country" | "founded" | "name" => 1,
        "website" | "domain_registered" | "registrar" | "site_https" | "site_pages" => 2,
        "posting_cadence" | "origin_share" | "syndication_share" | "revision_rate"
        | "withdrawal_rate" => 3,
        _ => 0,
    };
    let mut html_groups = groups.clone();
    html_groups.sort_by_key(|(f, _)| section(f));
    let mut open_section: Option<usize> = None;
    // Fields as rows: the name on the left, its claims on the right
    // (design §5, dense claims), not a heading per field.
    // Values from one source with the same provenance share a block, and
    // a note repeated across fields is shown the first time only.
    let mut seen_notes = std::collections::HashSet::new();
    for (field, cs) in &groups {
        // Rows a reader gets nothing from: the website and Wikipedia links
        // are in the header, and HTTPS shows only when a site lacks it.
        // The JSON and Markdown twins keep every claim.
        if matches!(
            *field,
            "website" | "registrar" | "wikipedia" | "wikidata_id"
        ) || (*field == "site_https" && cs.iter().all(|c| c.value.starts_with("https")))
        {
            continue;
        }
        let sec = section(field);
        if open_section != Some(sec) {
            open_section = Some(sec);
            let _ = write!(
                b,
                "<h2>{}</h2><p class=\"kg-muted\">{}</p>",
                SECTIONS[sec].0, SECTIONS[sec].1
            );
        }
        let _ = write!(
            b,
            "<section class=\"kg-field\" id=\"f-{0}\" data-field=\"{0}\"><div class=\"fname\">{1}</div><div>",
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
    b.push_str("</div><aside class=\"side\" id=\"stories\">");
    let _ = write!(
        b,
        "<h2>Stories</h2><p class=\"kg-muted\">In the {window_days} days to {}: {}, in {}. First to {} of the {} other outlets also carried. {} of its items carried text first published elsewhere; {} were carried by others. Counts from First Report feed, not a rating.</p>",
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
        b.push_str(&stories_block(input, o, facts, 1));
    }
    b.push_str("</aside></div>");

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
        "{}: {} from First Report ledger, each with its source, and its stories in First Report feed.",
        name,
        n(claims.len(), "sourced claim")
    );
    let page_html = html(&Page {
        title: name,
        description: &description,
        path: &path,
        wide: true,
        head: &head,
        body: &b,
        status: &crate::render::updated(input.export_time),
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
        "stories": stories_json(input, o, facts, 1),
        "story_pages": story_pages(facts),
        "export_time": input.export_time,
    });

    // ---- Markdown
    let mut md = format!("# {name}\n\n");
    let _ = writeln!(
        md,
        "Outlet id `{}`{}. {}. Every value below is a source's, never First Report's; sources that disagree are listed side by side.",
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
    md.push_str(&stories_md(input, o, facts, 1));

    Twins {
        html: page_html,
        json: page_json,
        md,
    }
}

/// Stories per page of an outlet's list; page 1 is on the outlet page,
/// then `/outlet/<id>/page/N/`.
pub const STORIES_PER_PAGE: usize = 50;

pub fn story_pages(facts: &OutletFacts) -> usize {
    facts.clusters.len().div_ceil(STORIES_PER_PAGE).max(1)
}

fn stories_path(o: &Outlet, page: usize) -> String {
    if page <= 1 {
        format!("/outlet/{}/", o.id)
    } else {
        format!("/outlet/{}/page/{page}/", o.id)
    }
}

fn page_ids(facts: &OutletFacts, page: usize) -> impl Iterator<Item = &u64> {
    facts
        .clusters
        .iter()
        .skip((page - 1) * STORIES_PER_PAGE)
        .take(STORIES_PER_PAGE)
}

/// One page of the outlet's stories, its pager below.
fn stories_block(input: &Input, o: &Outlet, facts: &OutletFacts, page: usize) -> String {
    let pages = story_pages(facts);
    let path = |p: usize| stories_path(o, p);
    let pager = crate::render::pager(
        page,
        pages,
        facts.clusters.len(),
        STORIES_PER_PAGE,
        "Story pages",
        &path,
    );
    let mut b = String::from("<ul class=\"stories\">");
    for id in page_ids(facts, page) {
        let c = &input.clusters[id];
        let _ = write!(
            b,
            "<li data-id=\"{}\"><a {}>{}</a>{}<span class=\"n kg-muted\">{} · {}</span></li>",
            c.id,
            input.story_href(c),
            esc(&c.primary_title),
            if c.origin_outlet_id == o.id && c.outlet_count > 1 {
                " <span class=\"kg-tag\">first</span>"
            } else {
                ""
            },
            n(c.outlet_count as usize, "outlet"),
            time_el(c.created_at, input.export_time)
        );
    }
    b.push_str("</ul>");
    b.push_str(&pager);
    b
}

fn stories_json(
    input: &Input,
    o: &Outlet,
    facts: &OutletFacts,
    page: usize,
) -> Vec<serde_json::Value> {
    page_ids(facts, page)
        .map(|id| {
            let c = &input.clusters[id];
            json!({"id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count, "created_at": c.created_at, "first": c.origin_outlet_id == o.id})
        })
        .collect()
}

fn stories_md(input: &Input, o: &Outlet, facts: &OutletFacts, page: usize) -> String {
    let mut md = String::new();
    for id in page_ids(facts, page) {
        let c = &input.clusters[id];
        let _ = writeln!(
            md,
            "- [{}]({}) · {}{} · {}",
            c.primary_title,
            input.story_url(c),
            n(c.outlet_count as usize, "outlet"),
            if c.origin_outlet_id == o.id && c.outlet_count > 1 {
                " · first"
            } else {
                ""
            },
            datetime(c.created_at)
        );
    }
    let pages = story_pages(facts);
    if pages > 1 {
        let _ = writeln!(
            md,
            "
Page {page} of {pages}; next: {}",
            stories_path(o, (page + 1).min(pages))
        );
    }
    md
}

/// Page `page` (2 or more) of an outlet's stories, at `/outlet/<id>/page/N/`.
pub fn stories_page(
    input: &Input,
    o: &Outlet,
    facts: &OutletFacts,
    window_days: i64,
    page: usize,
) -> Twins {
    let name = input.outlet_name(&o.id);
    let pages = story_pages(facts);
    let path = stories_path(o, page);
    let title = format!("{name}: stories, page {page}");
    let mut b = format!(
        "<h1>{}</h1><p class=\"kg-muted\">Page {page} of {pages} · {} in the {window_days} days to {} · <a href=\"/outlet/{}/\">back to the outlet</a></p>",
        esc(&title),
        n(facts.clusters.len(), "story"),
        crate::render::day_el(input.export_time, input.export_time),
        esc(&o.id)
    );
    b.push_str(&stories_block(input, o, facts, page));
    let page_html = html(&Page {
        title: &title,
        description: &format!("Stories {name} carried, page {page} of {pages}."),
        path: &path,
        wide: false,
        head: "",
        body: &b,
        status: &crate::render::updated(input.export_time),
    });
    let json = json!({
        "outlet": o.id, "page": page, "pages": pages, "page_size": STORIES_PER_PAGE,
        "window_days": window_days, "export_time": input.export_time,
        "stories": stories_json(input, o, facts, page),
    });
    let md = format!(
        "# {title}

Page {page} of {pages}. Outlet: /outlet/{}/

{}",
        o.id,
        stories_md(input, o, facts, page)
    );
    Twins {
        html: page_html,
        json,
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
        // At a glance tiles link to their rows; rows sit under group headings;
        // the header marks Outlets and carries the data stamp.
        assert!(t
            .html
            .contains("<a class=\"tile\" href=\"#f-reliability\">"));
        assert!(t.html.contains("id=\"f-reliability\""));
        assert!(t.html.contains("<h2>What sources say</h2>"));
        assert!(t.html.contains("<h2>How it publishes</h2>"));
        assert!(t
            .html
            .contains("<a href=\"/outlets/\" aria-current=\"page\">Outlets</a>"));
        assert!(t
            .html
            .contains("<span class=\"kg-updated\">Updated <time datetime=\""));
        let rel = t
            .html
            .find("<div class=\"fname\">Reliability</div>")
            .unwrap();
        let owner = t.html.find("<div class=\"fname\">Owner</div>").unwrap();
        assert!(rel < owner, "shared field order");
        assert!(
            !t.html.contains("<div class=\"fname\">Wikidata</div>"),
            "Wikidata is a header link, not a row"
        );
        assert!(t.html.contains(
            "href=\"https://www.wikidata.org/wiki/Q130879\" rel=\"noopener\">Wikidata</a>"
        ));
        assert!(
            t.md.contains("Q130879"),
            "the Markdown twin keeps every claim"
        );
        assert!(t
            .html
            .contains("First to 30 of the 31 stories other outlets also carried"));
        assert_eq!(
            t.html
                .matches("<span class=\"kg-tag\">first</span>")
                .count(),
            24,
            "first page: 50 of 61 stories"
        );
        assert!(t.html.contains("\"@type\":\"Organization\""));
        assert!(t.html.contains("https://www.wikidata.org/wiki/Q130879"));
        assert!(!t.html.contains("Rating"));
        assert_eq!(t.json["first_to"], 30);
        assert_eq!(
            t.json["stories"].as_array().unwrap().len(),
            STORIES_PER_PAGE
        );
        assert_eq!(t.json["story_pages"], 2);
        assert!(t
            .html
            .contains("<a class=\"step\" rel=\"next\" href=\"/outlet/wire/page/2/\">Older →</a>"));
        let p2 = stories_page(&i, &i.outlets["wire"], &facts["wire"], 90, 2);
        assert_eq!(p2.json["stories"].as_array().unwrap().len(), 11);
        assert!(p2.html.contains("aria-current=\"page\">2</span>"));
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
