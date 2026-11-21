//! State Media Monitor (plan §4): the Media and Journalism Research
//! Center's typology of state-owned and state-controlled media, CC BY 4.0.
//! Cite: Dragomir, M. (2025). State Media Monitor Global Dataset 2025.
//! MJRC. Zenodo. DOI: 10.5281/zenodo.17219015.
//!
//! The site has one page per state media entity, often a parent group
//! (China Media Group, not CGTN), so outlets are matched by a reviewed
//! list below rather than by name or domain: a page's links are its
//! citations, not the outlet's site. The importer fetches each listed
//! page and reads SMM's own category from it; a page it can't read is
//! reported and skipped, never guessed.

use std::collections::BTreeMap;

use serde_json::json;

use crate::model::Claim;

pub const SOURCE_ID: &str = "state_media_monitor";
pub const FIELD: &str = "state_media";

/// Reviewed matches: outlet id, SMM page, and the SMM entity's name when
/// the page covers a parent group that runs the outlet (the page names
/// the outlet in each case, checked September 2026).
pub const ENTRIES: &[(&str, &str, Option<&str>)] = &[
    ("rt", "https://statemediamonitor.com/2026/09/rt/", None),
    (
        "sputnik",
        "https://statemediamonitor.com/2026/09/rossiya-segodnya/",
        Some("Rossiya Segodnya"),
    ),
    ("tass", "https://statemediamonitor.com/2026/09/tass/", None),
    (
        "cgtn",
        "https://statemediamonitor.com/2026/08/china-media-group/",
        Some("China Media Group"),
    ),
    (
        "globaltimes",
        "https://statemediamonitor.com/2026/08/peoples-daily/",
        Some("People's Daily"),
    ),
    (
        "presstv",
        "https://statemediamonitor.com/2026/06/islamic-republic-of-iran-broadcasting-irib/",
        Some("Islamic Republic of Iran Broadcasting (IRIB)"),
    ),
    (
        "anadolu",
        "https://statemediamonitor.com/2025/09/anadolu-agency/",
        None,
    ),
    (
        "aljazeera",
        "https://statemediamonitor.com/2026/06/al-jazeera-media-network-ajmn/",
        Some("Al Jazeera Media Network"),
    ),
    (
        "nhk",
        "https://statemediamonitor.com/2026/08/japan-broadcasting-corporation-nhk/",
        None,
    ),
    (
        "bbc",
        "https://statemediamonitor.com/2026/09/british-broadcasting-corporation-bbc/",
        None,
    ),
    (
        "france24",
        "https://statemediamonitor.com/2026/09/france-medias-monde-fmm/",
        Some("France Médias Monde"),
    ),
    (
        "abcau",
        "https://statemediamonitor.com/2026/06/australian-broadcasting-corporation-abc/",
        None,
    ),
    (
        "cbc",
        "https://statemediamonitor.com/2026/06/canadian-broadcasting-corporation-radio-canada-cbc/",
        None,
    ),
];

/// SMM's codes and names, in their own words.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("SC", "State-Controlled"),
    ("CaPu", "Captured Public"),
    ("CaPr", "Captured Private"),
    ("ISFM", "Independent State-Funded and State-Managed"),
    ("ISF", "Independent State-Funded"),
    ("ISM", "Independent State-Managed"),
    ("IP", "Independent Public"),
];

fn category(code: &str) -> Option<(&'static str, &'static str)> {
    CATEGORIES.iter().copied().find(|(c, _)| *c == code)
}

/// SMM's current category from an entry page. Newer pages lead with a
/// "Quick Facts" panel whose first figure is the code; older ones state
/// "Typology State-Controlled (SC)" or "classified State-Controlled (SC)".
pub fn parse_category(page: &str) -> Option<(&'static str, &'static str)> {
    let text = crate::html::text_of(page);
    if let Some(i) = text.find("Quick Facts") {
        if let Some(c) = text[i..].split_whitespace().nth(2).and_then(category) {
            return Some(c);
        }
    }
    let lower = text.to_ascii_lowercase();
    for anchor in ["typology", "classified"] {
        for (i, _) in lower.match_indices(anchor) {
            let end = (i + 100).min(text.len());
            let window = text.get(i..end).unwrap_or_default();
            let found = window
                .split('(')
                .skip(1)
                .find_map(|s| category(s.split(')').next()?));
            if let Some(c) = found {
                return Some(c);
            }
        }
    }
    None
}

pub fn claim(
    outlet_id: &str,
    url: &str,
    via: Option<&str>,
    code: &str,
    name: &str,
    retrieved_at: i64,
) -> Claim {
    let mut data = BTreeMap::new();
    data.insert("code".into(), json!(code));
    if let Some(v) = via {
        data.insert("entity".into(), json!(v));
    }
    let whose = via
        .map(|v| format!("SMM's entry for {v}, which runs this outlet; "))
        .unwrap_or_default();
    Claim {
        outlet_id: outlet_id.into(),
        field: FIELD.into(),
        value: format!("{name} ({code})"),
        data: Some(data),
        source_id: SOURCE_ID.into(),
        ref_url: url.into(),
        retrieved_at,
        as_of: Some("2026".into()),
        note: Some(format!(
            "{whose}State Media Monitor's category, not Truth Decay's. Dragomir, M. (2025), State Media Monitor Global Dataset, MJRC, CC BY 4.0."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quick_facts_and_older_pages() {
        let qf = r#"<h2>TASS</h2><span>Russia &middot; Quick Facts</span><div><div>SC</div><div>All conditions met</div></div>"#;
        assert_eq!(parse_category(qf), Some(("SC", "State-Controlled")));
        let old = "<p>State Media Matrix</p><p>Typology</p><p>State-Controlled (SC)</p>";
        assert_eq!(parse_category(old), Some(("SC", "State-Controlled")));
        let isf = "<p>Trajectory 2022 to 2026 ISF throughout. 2026 typology: Independent State-Funded (ISF)</p>";
        assert_eq!(
            parse_category(isf),
            Some(("ISF", "Independent State-Funded"))
        );
        assert_eq!(parse_category("<p>no category here (XYZ)</p>"), None);
    }

    #[test]
    fn claim_names_the_parent_entity() {
        let c = claim(
            "cgtn",
            "https://statemediamonitor.com/x/",
            Some("China Media Group"),
            "SC",
            "State-Controlled",
            1,
        );
        assert_eq!(
            (c.field.as_str(), c.value.as_str()),
            (FIELD, "State-Controlled (SC)")
        );
        assert!(c
            .note
            .unwrap()
            .starts_with("SMM's entry for China Media Group"));
    }
}
