//! `/about/` (plan §3.5): what First Report is, what each number means, what
//! lens does not do, the sources with their licences, and the rules of
//! use. Generated from the source registry so it cannot go stale.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::input::Input;
use crate::render::{datetime, esc, html, n, Page};

const WHAT: &[(&str, &str)] = &[
    ("Story", "Headlines that feed grouped as the same event because their text is similar. The grouping is automatic and sometimes wrong."),
    ("First", "The headline with the earliest publish time in its feed. Feeds sometimes get times wrong, so we also show when we picked it up."),
    ("Outlets, items", "How many outlets covered a story, and how many headlines they published. No outlet counts for more than another."),
    ("Reach", "How the front page is ordered. It counts outlets that wrote their own version of the story, adds one for each extra language, then breaks ties by recent coverage and by time. Stories only one outlet covered are listed separately. The topic and the outlets involved play no part. Full rules: docs/algorithms.md."),
    ("Copy", "The same text published by more than one outlet. Copies don't count as separate coverage. Wire stories are meant to be copied."),
    ("Revised, withdrawn", "The headline or summary changed after publication, or the item disappeared from its feed."),
    ("Claim", "A fact about an outlet from one source, with a link, the licence and the date we read it. When sources disagree we show both."),
    ("Stale", "Older than the source's refresh interval, so it may be out of date."),
    ("Measurement", "Facts we measured ourselves from domain records, the outlet's home page and its feed. Each one gives the time window and counts it's based on."),
];

const NOT: &[&str] = &[
    "First Report has no score or rating of its own. The State, Sanctioned and Deprecated tags each come from a named source, shown on hover.",
    "It doesn't summarise stories. Headlines are the outlets' own.",
    "It only reads feeds, never full article pages. The raw fetches are kept.",
    "It doesn't filter outlets by quality. Every feed in the pool is treated the same.",
];

pub fn about(input: &Input) -> Twins {
    let mut b = String::new();
    b.push_str("<h1>About</h1><p>First Report tracks how news stories spread: who published first, who followed, what was copied and what was changed later. It also collects facts about each outlet, with sources.</p>");
    b.push_str("<p>The site is built from two parts: feed, which reads outlets' feeds and groups headlines into stories, and ledger, which stores facts about outlets. Each page shows when it was last built.</p>");
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
    b.push_str("<h2>What First Report doesn't do</h2><ul>");
    for x in NOT {
        let _ = write!(b, "<li>{}</li>", esc(x));
    }
    b.push_str("</ul><h2>Sources</h2><p class=\"kg-muted\">Where our facts about outlets come from. If you reuse them, follow the source's licence.</p>");
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
    b.push_str("<h2>For machine readers</h2><p>Every page has <code>index.json</code> and <code>index.md</code> beside it with with the same content. <a href=\"/llms.txt\">/llms.txt</a> states the rules of use; <a href=\"/sitemap.xml\">/sitemap.xml</a> lists every page; <a href=\"/search/index.json\">/search/index.json</a> is the index of outlets and stories. Please cite the original source and keep its licence. Don't turn these facts into a score.</p>");
    let _ = write!(
        b,
        "<p class=\"kg-muted\">Built from an export at {} covering {} in the pool and {} with claims. First Report is open source: <a href=\"https://github.com/h0ds/firstreport\" rel=\"noopener\">github.com/h0ds/firstreport</a>.</p>",
        datetime(input.export_time),
        n(input.outlets.len(), "outlet"),
        n(input.claims.len(), "outlet")
    );
    let page_html = html(&Page {
        title: "About",
        description: "What First Report is, what the numbers mean, and where the data comes from.",
        path: "/about/",
        wide: false,
        head: "",
        body: &b,
        status: &crate::render::updated(input.export_time),
    });
    let page_json = json!({
        "what": WHAT.iter().map(|(k, v)| json!({"term": k, "meaning": v})).collect::<Vec<_>>(),
        "does_not": NOT,
        "sources": input.sources.values().collect::<Vec<_>>(),
        "export_time": input.export_time,
        "outlets": input.outlets.len(),
        "outlets_with_claims": input.claims.len(),
        "repository": "https://github.com/h0ds/firstreport",
    });
    let mut md = String::from("# About\n\nFirst Report tracks how news stories spread: who published first, who followed, what was copied and what was changed later. It also collects facts about each outlet, with sources.\n\n## What the numbers mean\n\n");
    for (k, v) in WHAT {
        let _ = writeln!(md, "- **{k}**: {v}");
    }
    md.push_str("\n## What First Report doesn't do\n\n");
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
        "\n## For machine readers\n\nEvery page has `index.json` and `index.md` beside it. `/llms.txt` states the rules of use; `/sitemap.xml` lists every page; `/search/index.json` is the index. Please cite the original source and keep its licence. Don't turn these facts into a score.\n\nBuilt from an export at {}. Source: https://github.com/h0ds/firstreport\n",
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
            4
        );
        assert!(t.html.contains("CC-BY-SA-4.0"));
        assert!(t.html.contains("no score or rating of its own"));
        assert_eq!(t.json["sources"].as_array().unwrap().len(), 4);
        assert!(t.md.contains("- **Wikidata** (database) · licence CC0-1.0"));
    }
}
