//! Data contract v1 (plan §2): sources and claims. No aggregate anywhere.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

/// Fields with agreed meaning (plan §3). Others are allowed with a warning.
pub const KNOWN_FIELDS: &[&str] = &[
    "wikidata_id",
    "name",
    "country",
    "founded",
    "owner",
    "parent",
    "legal_form",
    "website",
    "wikipedia",
    "bias",
    "reliability",
    "domain_registered",
    "registrar",
    "posting_cadence",
    "revision_rate",
    "withdrawal_rate",
    "syndication_share",
    "origin_share",
    "site_https",
    "site_pages",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Database,
    Community,
    Rating,
    /// Measured by Kagami itself from what outlets publish.
    Observed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    pub licence: String,
    pub kind: SourceKind,
    #[serde(default)]
    pub notes: String,
    /// Set when the source's terms do not allow redistribution; `check`
    /// refuses to serve such claims (plan §8, L3).
    #[serde(default)]
    pub no_redistribution: bool,
    /// How often this source's claims should be re-imported. 0 means no
    /// schedule (hand-entered sources). `check` reports claims older than
    /// this; `refresh` re-imports sources that are due.
    #[serde(default)]
    pub refresh_days: u32,
}

pub const SECS_PER_DAY: i64 = 86_400;

/// Days since the claim was retrieved.
pub fn age_days(c: &Claim, now: i64) -> i64 {
    (now - c.retrieved_at).max(0) / SECS_PER_DAY
}

/// Older than the source's refresh interval. Never stale for sources
/// without a schedule.
pub fn is_stale(c: &Claim, source: &Source, now: i64) -> bool {
    source.refresh_days > 0 && age_days(c, now) > source.refresh_days as i64
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub outlet_id: String,
    pub field: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<BTreeMap<String, serde_json::Value>>,
    pub source_id: String,
    pub ref_url: String,
    pub retrieved_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Problems found by validation. `Error`s fail `check`; `Warning`s don't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    Error(String),
    Warning(String),
}

impl Finding {
    pub fn is_error(&self) -> bool {
        matches!(self, Finding::Error(_))
    }
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Finding::Error(m) => write!(f, "error: {m}"),
            Finding::Warning(m) => write!(f, "warning: {m}"),
        }
    }
}

/// Validate one claim against the source registry and, when known, the
/// outlet registry.
pub fn validate_claim(
    c: &Claim,
    sources: &[Source],
    outlet_ids: Option<&[String]>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let at = format!("{}/{}", c.outlet_id, c.field);
    if c.outlet_id.trim().is_empty() {
        out.push(Finding::Error(format!("{at}: empty outlet_id")));
    }
    if let Some(ids) = outlet_ids {
        if !ids.iter().any(|i| i == &c.outlet_id) {
            out.push(Finding::Error(format!(
                "{at}: outlet {:?} is not in feed's outlets.toml",
                c.outlet_id
            )));
        }
    }
    if c.field.trim().is_empty() {
        out.push(Finding::Error(format!("{at}: empty field")));
    } else if !KNOWN_FIELDS.contains(&c.field.as_str()) {
        out.push(Finding::Warning(format!(
            "{at}: field {:?} is not in the known list",
            c.field
        )));
    }
    if c.field == "score" || c.field.ends_with("_score") {
        out.push(Finding::Error(format!(
            "{at}: aggregate scores are not claims (plan §2)"
        )));
    }
    if c.value.trim().is_empty() {
        out.push(Finding::Error(format!("{at}: empty value")));
    }
    match sources.iter().find(|s| s.id == c.source_id) {
        None => out.push(Finding::Error(format!(
            "{at}: unknown source {:?}",
            c.source_id
        ))),
        Some(s) if s.no_redistribution => out.push(Finding::Error(format!(
            "{at}: source {:?} does not permit redistribution",
            s.id
        ))),
        _ => {}
    }
    if !(c.ref_url.starts_with("https://") || c.ref_url.starts_with("http://")) {
        out.push(Finding::Error(format!(
            "{at}: ref_url must be a URL, got {:?}",
            c.ref_url
        )));
    }
    if c.retrieved_at <= 0 {
        out.push(Finding::Error(format!("{at}: retrieved_at must be set")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(id: &str, no_redist: bool) -> Source {
        Source {
            id: id.into(),
            name: id.into(),
            url: "https://example".into(),
            licence: "CC0-1.0".into(),
            kind: SourceKind::Database,
            notes: String::new(),
            no_redistribution: no_redist,
            refresh_days: 0,
        }
    }

    fn claim(field: &str, source: &str) -> Claim {
        Claim {
            outlet_id: "bbc".into(),
            field: field.into(),
            value: "United Kingdom".into(),
            data: None,
            source_id: source.into(),
            ref_url: "https://www.wikidata.org/wiki/Q9531".into(),
            retrieved_at: 1_700_000_000,
            as_of: None,
            note: None,
        }
    }

    #[test]
    fn staleness_follows_the_source_schedule() {
        let mut s = src("wikidata", false);
        let c = claim("country", "wikidata"); // retrieved at 1_700_000_000
        let now = 1_700_000_000 + 100 * SECS_PER_DAY;
        assert_eq!(age_days(&c, now), 100);
        assert!(!is_stale(&c, &s, now), "no schedule, never stale");
        s.refresh_days = 90;
        assert!(is_stale(&c, &s, now));
        assert!(!is_stale(&c, &s, 1_700_000_000 + 90 * SECS_PER_DAY));
    }

    #[test]
    fn valid_claim_has_no_findings() {
        let f = validate_claim(
            &claim("country", "wikidata"),
            &[src("wikidata", false)],
            Some(&["bbc".into()]),
        );
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn errors_and_warnings() {
        let sources = [src("wikidata", false), src("closed", true)];
        let f = validate_claim(&claim("country", "nope"), &sources, Some(&["bbc".into()]));
        assert!(f
            .iter()
            .any(|x| x.is_error() && x.to_string().contains("unknown source")));
        let f = validate_claim(&claim("country", "closed"), &sources, None);
        assert!(f.iter().any(|x| x.to_string().contains("redistribution")));
        let f = validate_claim(&claim("audience_size", "wikidata"), &sources, None);
        assert_eq!(f.len(), 1);
        assert!(!f[0].is_error(), "unknown field is a warning");
        let f = validate_claim(&claim("credibility_score", "wikidata"), &sources, None);
        assert!(f
            .iter()
            .any(|x| x.is_error() && x.to_string().contains("aggregate")));
        let mut c = claim("country", "wikidata");
        c.outlet_id = "cnn".into();
        c.ref_url = "wikidata:Q9531".into();
        c.retrieved_at = 0;
        let f = validate_claim(&c, &sources, Some(&["bbc".into()]));
        assert_eq!(f.iter().filter(|x| x.is_error()).count(), 3);
    }
}
