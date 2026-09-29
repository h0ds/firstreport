//! `/about/` (plan §3.5): what Kagami is, what each number means, what
//! lens does not do, the sources with their licences, and the rules of
//! use. Generated from the source registry so it cannot go stale.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::input::Input;
use crate::render::{datetime, esc, html, n, Page};

const WHAT: &[(&str, &str)] = &[
    ("Story", "A cluster of headlines Kagami feed judged to be about the same event, by text similarity against a threshold. A machine's judgement, not a person's; treat membership as evidence."),
    ("First", "The member with the earliest published time as its feed reported it. Feeds misreport times; the picked-up time is what Kagami saw, and both are shown."),
    ("Outlets, items", "Counts of distinct outlets and of headlines in a story. Neither is weighted."),
    ("Reach", "The front page's order, and the only place Kagami says one story matters more than another. Reach is independent reports plus languages minus one, over the page's window: independent reports are the outlets that joined, less those whose every item was a copy of another outlet's text; languages are the distinct languages of the items' own text. Then outlets in the last six hours, then recency. A story with one independent report is listed apart under single reports, newest first. No word, label or outlet is an input: a story many outlets tell independently in several languages ranks high whatever it is about. The full register of rules is docs/algorithms.md in the repository."),
    ("Copy", "The same text at more than one outlet, found by a text fingerprint. A copy is not independent coverage; a wire service is meant to be copied."),
    ("Revised, withdrawn", "The headline or summary changed after publication, or the item left its feed, as seen in later fetches."),
    ("Claim", "One fact about an outlet as one source states it, with the source, its licence, the date Kagami read it and the page that states it. Sources that disagree are shown side by side and never merged."),
    ("Stale", "Older than its source's refresh interval; the value may have changed since."),
    ("Measurement", "Claims from Kagami's own observation (domain registry, the outlet's site, its feed behaviour) carry the window and the counts they came from. They describe what an outlet does, not whether that is good."),
];

const NOT: &[&str] = &[
    "It does not rate outlets, and there is no score anywhere in Kagami.",
    "It does not colour, badge or order outlets by lean or credibility. Outlets are ordered by the time they joined a story or alphabetically.",
    "It does not summarise stories or claims in its own words. Every headline is the outlet's; every claim is a source's.",
    "It does not fetch article pages. Only feeds are read, and the raw feed fetches are kept as evidence.",
    "It does not filter by outlet quality. Fringe and mainstream feeds are ingested identically.",
];

pub fn about(input: &Input) -> Twins {
    let mut b = String::new();
    b.push_str("<h1>About</h1><p>Kagami is an open-source record of how news stories spread: which outlet published first, who joined when, what was copied, and what changed afterwards, together with sourced facts about each outlet. It exists to make provenance as cheap to check as a headline is to share, for people and for the automated systems that increasingly do the sharing.</p>");
    b.push_str("<p>This site is generated from two exports: Kagami feed, which polls outlets' feeds and clusters headlines into stories, and Kagami ledger, which holds claims about outlets with their sources. It is rebuilt from a fresh export on a schedule; the header of every page says when.</p>");
    b.push_str("<h2>What the numbers mean</h2>");
    for (k, v) in WHAT {
        let _ = write!(
            b,
            "<h3 id=\"{}\">{}</h3><p>{}</p>",
            k.to_ascii_lowercase().replace(", ", "-").replace(' ', "-"),
            esc(k),
            esc(v)
        );
    }
    b.push_str("<h2>What this site does not do</h2><ul>");
    for x in NOT {
        let _ = write!(b, "<li>{}</li>", esc(x));
    }
    b.push_str("</ul><h2>Sources</h2><p class=\"kg-muted\">Every claim names one of these. The licence is the source's; carry it with anything you quote.</p>");
    for s in input.sources.values() {
        let _ = write!(
            b,
            "<article class=\"kg-claim\" data-source=\"{}\"><div class=\"value\">{}</div><div class=\"kg-provenance\"><a href=\"{}\" rel=\"noopener\">{}</a><span class=\"sep\">·</span><span class=\"kg-tag\">{}</span><span class=\"sep\">·</span><span>{}</span>{}</div>{}</article>",
            esc(&s.id),
            esc(&s.name),
            esc(&s.url),
            esc(&s.url),
            esc(&s.licence),
            esc(&s.kind),
            if s.refresh_days > 0 { format!("<span class=\"sep\">·</span><span>refreshed every {} days</span>", s.refresh_days) } else { String::new() },
            if s.notes.is_empty() { String::new() } else { format!("<div class=\"note\">{}</div>", esc(&s.notes)) }
        );
    }
    b.push_str("<h2>For machine readers</h2><p>Every page has <code>index.json</code> and <code>index.md</code> beside it with the same facts in the same order, linked from its head. <a href=\"/llms.txt\">/llms.txt</a> states the rules of use; <a href=\"/sitemap.xml\">/sitemap.xml</a> lists every page; <a href=\"/search/index.json\">/search/index.json</a> is the index of outlets and stories. Quote the source, not Kagami; carry the licence; never turn claims into a score.</p>");
    let _ = write!(
        b,
        "<p class=\"kg-muted\">Built from an export at {} covering {} in the pool and {} with claims. Kagami is open source: <a href=\"https://github.com/h0ds/kagami\" rel=\"noopener\">github.com/h0ds/kagami</a>.</p>",
        datetime(input.export_time),
        n(input.outlets.len(), "outlet"),
        n(input.claims.len(), "outlet")
    );
    let page_html = html(&Page {
        title: "About",
        description: "What Kagami is, what each number means, what this site does not do, and the sources with their licences.",
        path: "/about/",
        wide: false,
        head: "",
        body: &b,
        status: &format!("about · built from an export at {}", datetime(input.export_time)),
    });
    let page_json = json!({
        "what": WHAT.iter().map(|(k, v)| json!({"term": k, "meaning": v})).collect::<Vec<_>>(),
        "does_not": NOT,
        "sources": input.sources.values().collect::<Vec<_>>(),
        "export_time": input.export_time,
        "outlets": input.outlets.len(),
        "outlets_with_claims": input.claims.len(),
        "repository": "https://github.com/h0ds/kagami",
    });
    let mut md = String::from("# About\n\nKagami is an open-source record of how news stories spread: which outlet published first, who joined when, what was copied, and what changed afterwards, together with sourced facts about each outlet. This site is generated from Kagami feed and Kagami ledger exports.\n\n## What the numbers mean\n\n");
    for (k, v) in WHAT {
        let _ = writeln!(md, "- **{k}**: {v}");
    }
    md.push_str("\n## What this site does not do\n\n");
    for x in NOT {
        let _ = writeln!(md, "- {x}");
    }
    md.push_str("\n## Sources\n\n");
    for s in input.sources.values() {
        let _ = writeln!(
            md,
            "- **{}** ({}) · licence {} · {}{}",
            s.name,
            s.kind,
            s.licence,
            s.url,
            if s.notes.is_empty() {
                String::new()
            } else {
                format!(" · {}", s.notes)
            }
        );
    }
    let _ = write!(
        md,
        "\n## For machine readers\n\nEvery page has `index.json` and `index.md` beside it. `/llms.txt` states the rules of use; `/sitemap.xml` lists every page; `/search/index.json` is the index. Quote the source, not Kagami; carry the licence; never turn claims into a score.\n\nBuilt from an export at {}. Source: https://github.com/h0ds/kagami\n",
        datetime(input.export_time)
    );
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
    fn about_lists_every_source_with_its_licence() {
        let i = fixture_input();
        let t = about(&i);
        assert_eq!(
            t.html
                .matches("<article class=\"kg-claim\" data-source=")
                .count(),
            5
        );
        assert!(t.html.contains("CC-BY-SA-4.0"));
        assert!(t.html.contains("does not rate outlets"));
        assert_eq!(t.json["sources"].as_array().unwrap().len(), 5);
        assert!(t.md.contains("- **Wikidata** (database) · licence CC0-1.0"));
    }
}
