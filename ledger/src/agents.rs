//! What a machine reader gets (design §11): `/llms.txt` saying what
//! this server is and how to use it, `/robots.txt` allowing everything,
//! and a Markdown rendering of an outlet that keeps every provenance
//! line, served when a client asks for `text/markdown`.

use crate::model::{Claim, Source, CONTRACT_VERSION};

pub const ROBOTS_TXT: &str = "User-agent: *\nAllow: /\n";

pub fn llms_txt() -> String {
    format!(
        "# First Report ledger

> Sourced facts about news outlets: who owns them, where they are, when they
> were founded, what third parties say about their bias and reliability, and
> what First Report itself observes about their domains, sites and feeds. Every
> value is a source's, never First Report's, and every claim carries its source,
> licence, retrieval date and the page that states it. There is no score.

Contract version {CONTRACT_VERSION}. Outlet ids are the same as in First Report feed.

## Routes

- `GET /v1/outlets` - outlet ids with claim counts, fields and sources
- `GET /v1/outlets/{{id}}` - all claims for one outlet, grouped by field, each with its source, licence, retrieved date, as-of date and stale flag. Send `Accept: text/markdown` for a Markdown rendering that keeps every provenance line.
- `GET /v1/outlets/{{id}}/{{field}}` - claims for one field
- `GET /v1/search?q=` - find an outlet by name, id or domain
- `GET /v1/sources` - the source registry with licences and refresh intervals
- `GET /v1/stats`
- `GET /v1/openapi.json` - OpenAPI 3.1 description of all of the above
- `GET /lookup` - the same data as a page for people

## Rules of use

- Quote the source, not First Report: a claim is what its source says, and the `ref_url` is the page that says it.
- Carry the licence: each source's terms are in `/v1/sources` and on every claim.
- Sources that disagree are listed side by side. Do not average them, rank them, or turn them into a score.
- Claims from the `observed` sources (domain registry, site checks, feed behaviour) are measurements with their window and counts attached. They describe what an outlet does, not whether that is good.
- `stale` means the claim is older than its source's refresh interval; the value may have changed since.
"
    )
}

fn date(ts: i64) -> String {
    crate::behaviour::date(ts)
}

/// Markdown for one outlet: heading, one section per field in the
/// lookup page's order, one block per claim with the provenance line
/// in the fixed order from design §5.
pub fn outlet_markdown(
    id: &str,
    name: Option<&str>,
    claims: &[Claim],
    sources: &[Source],
    now: i64,
) -> String {
    const ORDER: &[&str] = &[
        "reliability",
        "bias",
        "owner",
        "parent",
        "legal_form",
        "country",
        "founded",
        "website",
        "domain_registered",
        "registrar",
        "site_https",
        "site_pages",
        "posting_cadence",
        "origin_share",
        "syndication_share",
        "revision_rate",
        "withdrawal_rate",
        "wikipedia",
        "name",
        "wikidata_id",
    ];
    let label = |f: &str| -> String {
        match f {
            "legal_form" => "Legal form".into(),
            "parent" => "Parent organisation".into(),
            "domain_registered" => "Domain registered".into(),
            "site_https" => "HTTPS".into(),
            "site_pages" => "Accountability pages".into(),
            "posting_cadence" => "Posting cadence".into(),
            "origin_share" => "First to shared stories".into(),
            "syndication_share" => "Syndication".into(),
            "revision_rate" => "Revisions".into(),
            "withdrawal_rate" => "Withdrawals".into(),
            "name" => "Name in source".into(),
            "wikidata_id" => "Wikidata".into(),
            other => {
                let mut c = other.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            }
        }
    };
    let mut fields: Vec<&str> = claims.iter().map(|c| c.field.as_str()).collect();
    fields.sort_by_key(|f| {
        ORDER
            .iter()
            .position(|o| o == f)
            .map(|p| (p, ""))
            .unwrap_or((usize::MAX, *f))
    });
    fields.dedup();
    let stale = claims
        .iter()
        .filter(|c| {
            sources
                .iter()
                .find(|s| s.id == c.source_id)
                .is_some_and(|s| crate::model::is_stale(c, s, now))
        })
        .count();
    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", name.unwrap_or(id)));
    md.push_str(&format!(
        "Outlet id `{id}`. {} claims, {stale} past their source's refresh interval. Every value below is a source's, never First Report's; sources that disagree are listed side by side.\n",
        claims.len()
    ));
    for f in fields {
        md.push_str(&format!("\n## {}\n", label(f)));
        for c in claims.iter().filter(|c| c.field == f) {
            let src = sources.iter().find(|s| s.id == c.source_id);
            let (sname, licence) = src
                .map(|s| (s.name.as_str(), s.licence.as_str()))
                .unwrap_or((c.source_id.as_str(), "unknown"));
            let is_stale = src.is_some_and(|s| crate::model::is_stale(c, s, now));
            let age = crate::model::age_days(c, now);
            md.push_str(&format!("\n**{}**\n", c.value));
            if let Some(scope) = c
                .data
                .as_ref()
                .and_then(|d| d.get("scope"))
                .and_then(|v| v.as_str())
                .filter(|s| *s != c.value)
            {
                md.push_str(&format!("{scope}\n"));
            }
            md.push_str(&format!(
                "Source: [{sname}]({}) · licence {licence} · retrieved {}",
                c.ref_url,
                date(c.retrieved_at)
            ));
            if let Some(a) = &c.as_of {
                md.push_str(&format!(" · as of {a}"));
            }
            md.push_str(&if is_stale {
                format!(" · stale, {age} days old\n")
            } else {
                format!(" · {age} days old\n")
            });
            if let Some(d) = &c.data {
                if let Some(w) = d.get("window_days").and_then(|v| v.as_i64()) {
                    md.push_str(&format!(
                        "Measured over {w} days to {}, {} items. A measurement, not a verdict.\n",
                        d.get("window_end").and_then(|v| v.as_str()).unwrap_or("?"),
                        d.get("items").and_then(|v| v.as_i64()).unwrap_or(0)
                    ));
                }
                if let Some(s) = d.get("summary").and_then(|v| v.as_str()) {
                    md.push_str(&format!("{s}\n"));
                }
            }
            if let Some(n) = &c.note {
                md.push_str(&format!("Note: {n}\n"));
            }
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn markdown_keeps_every_provenance_line() {
        let sources = vec![Source {
            id: "wikidata".into(),
            name: "Wikidata".into(),
            url: String::new(),
            licence: "CC0-1.0".into(),
            kind: crate::model::SourceKind::Database,
            notes: String::new(),
            no_redistribution: false,
            refresh_days: 90,
        }];
        let c = |field: &str,
                 source: &str,
                 value: &str,
                 data: Option<BTreeMap<String, serde_json::Value>>| Claim {
            outlet_id: "bbc".into(),
            field: field.into(),
            value: value.into(),
            data,
            source_id: source.into(),
            ref_url: "https://x/y".into(),
            retrieved_at: 86_400 * 10,
            as_of: Some("2024".into()),
            note: None,
        };
        let claims = vec![
            c("wikidata_id", "wikidata", "Q9531", None),
            c("bias", "b", "Lean Left", None),
            c("bias", "a", "Center", None),
            c(
                "posting_cadence",
                "feed_behaviour",
                "2.0 items/day",
                Some(BTreeMap::from([
                    ("window_days".to_string(), serde_json::json!(30)),
                    ("window_end".to_string(), serde_json::json!("2026-09-22")),
                    ("items".to_string(), serde_json::json!(61)),
                ])),
            ),
        ];
        let md = outlet_markdown("bbc", Some("BBC"), &claims, &sources, 86_400 * 20);
        assert!(md.starts_with("# BBC\n"));
        let bias = md.find("## Bias").unwrap();
        let cadence = md.find("## Posting cadence").unwrap();
        let qid = md.find("## Wikidata").unwrap();
        assert!(bias < cadence && cadence < qid, "lookup page order");
        assert_eq!(
            md.matches("Source: [").count(),
            4,
            "one provenance line per claim"
        );
        assert!(md.contains("Source: [Wikidata](https://x/y) · licence CC0-1.0 · retrieved 1970-01-11 · as of 2024 · 10 days old"));
        assert!(
            md.contains("licence unknown"),
            "unregistered source still shown, never hidden"
        );
        assert!(md.contains("A measurement, not a verdict."));
        assert!(
            md.contains("**Lean Left**") && md.contains("**Center**"),
            "disagreement side by side"
        );
        assert!(llms_txt().contains("Accept: text/markdown"));
    }
}
