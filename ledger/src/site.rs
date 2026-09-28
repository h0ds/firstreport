//! Site checks: what an outlet's own web site shows anyone who visits.
//! Whether it serves HTTPS and moves plain HTTP there, and whether its
//! home page links the pages a reader uses to hold it to account:
//! about, contact, privacy, corrections, ethics or standards, masthead
//! or staff. Observed by Kagami, one request or two per domain, each
//! claim naming the page it was read from.
//!
//! Absence is recorded as absence ("no corrections page linked from
//! the home page"), never as a fault: a link we failed to recognise and
//! a page that does not exist look the same from here, and the claim
//! says so.

// Wired into `ledger import` in the next commit.
#![allow(dead_code)]

use std::collections::BTreeMap;

use url::Url;

use crate::html::{attr, elements, text_of, void_tags};
use crate::model::Claim;

pub const SOURCE_ID: &str = "site";
pub const USER_AGENT: &str = crate::rdap::USER_AGENT;
/// Home pages larger than this are cut; the links we want are in the
/// chrome, not the body.
pub const MAX_BODY: usize = 2 << 20;

/// Page kinds in report order, with the tokens that identify them in a
/// link's path or text.
pub const PAGE_KINDS: &[(&str, &[&str])] = &[
    ("about", &["about"]),
    ("contact", &["contact"]),
    ("privacy", &["privacy"]),
    ("corrections", &["correction", "clarification"]),
    (
        "ethics",
        &[
            "ethics",
            "standards",
            "editorial-guidelines",
            "editorial guidelines",
            "editorial policy",
        ],
    ),
    (
        "masthead",
        &[
            "masthead",
            "our-team",
            "our team",
            "staff",
            "who-we-are",
            "who we are",
            "leadership",
        ],
    ),
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomePage {
    pub site_name: Option<String>,
    /// kind → first matching absolute URL.
    pub pages: BTreeMap<&'static str, String>,
}

fn kind_of(path: &str, text: &str) -> Option<&'static str> {
    let path = path.to_ascii_lowercase();
    let text = text.to_ascii_lowercase();
    PAGE_KINDS
        .iter()
        .find(|(_, toks)| {
            toks.iter()
                .any(|t| path.contains(t) || (text.len() <= 60 && text.contains(t)))
        })
        .map(|(k, _)| *k)
}

pub fn parse_home(html: &str, base: &Url) -> HomePage {
    let html = &html[..html.len().min(MAX_BODY)];
    let mut page = HomePage::default();
    for m in void_tags(html, "meta") {
        let is_site_name = attr(m, "property").as_deref() == Some("og:site_name")
            || attr(m, "name").as_deref() == Some("og:site_name");
        if is_site_name {
            if let Some(c) = attr(m, "content").map(|c| c.trim().to_string()) {
                if !c.is_empty() {
                    page.site_name = Some(c);
                    break;
                }
            }
        }
    }
    for (a, inner) in elements(html, "a") {
        let Some(href) = attr(a, "href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty()
            || href.starts_with('#')
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with("javascript:")
        {
            continue;
        }
        let Ok(url) = base.join(href) else { continue };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        let text = text_of(inner);
        if let Some(kind) = kind_of(url.path(), &text) {
            page.pages.entry(kind).or_insert_with(|| url.to_string());
        }
    }
    page
}

/// Everything observed about one domain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    pub domain: String,
    /// `https://domain/` answered with a success status.
    pub https_ok: bool,
    /// Where the HTTPS request ended up after redirects.
    pub final_url: Option<String>,
    pub status: Option<u16>,
    /// `http://domain/` ended on an https URL; None when HTTP was unreachable.
    pub http_to_https: Option<bool>,
    pub home: Option<HomePage>,
}

pub fn claims(outlet_id: &str, obs: &Observation, retrieved_at: i64) -> Vec<Claim> {
    let home_url = format!("https://{}/", obs.domain);
    let ref_url = obs.final_url.clone().unwrap_or_else(|| home_url.clone());
    let mut out = Vec::new();
    let mut data: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    data.insert("domain".into(), obs.domain.clone().into());
    data.insert("https".into(), obs.https_ok.into());
    if let Some(s) = obs.status {
        data.insert("status".into(), s.into());
    }
    if let Some(f) = &obs.final_url {
        data.insert("final_url".into(), f.clone().into());
    }
    if let Some(r) = obs.http_to_https {
        data.insert("http_redirects_to_https".into(), r.into());
    }
    let value = match (obs.https_ok, obs.http_to_https) {
        (true, Some(true)) => "https; plain http redirects to it".to_string(),
        (true, Some(false)) => "https; plain http is served separately".to_string(),
        (true, None) => "https; plain http unreachable".to_string(),
        (false, _) => match obs.status {
            Some(s) => format!("https not served (status {s})"),
            None => "https not served".to_string(),
        },
    };
    out.push(Claim {
        outlet_id: outlet_id.to_string(),
        field: "site_https".into(),
        value,
        data: Some(data),
        source_id: SOURCE_ID.to_string(),
        ref_url: ref_url.clone(),
        retrieved_at,
        as_of: None,
        note: None,
    });
    let Some(home) = &obs.home else {
        return out;
    };
    let found: Vec<&str> = PAGE_KINDS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| home.pages.contains_key(k))
        .collect();
    let missing: Vec<&str> = PAGE_KINDS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !home.pages.contains_key(k))
        .collect();
    let value = match (found.is_empty(), missing.is_empty()) {
        (false, true) => format!("links {} pages from its home page", found.join(", ")),
        (false, false) => format!(
            "links {} pages from its home page; no {} link found",
            found.join(", "),
            missing.join(", ")
        ),
        (true, _) => "none of the usual accountability pages linked from its home page".into(),
    };
    let mut data: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    data.insert("domain".into(), obs.domain.clone().into());
    for (k, u) in &home.pages {
        data.insert((*k).into(), u.clone().into());
    }
    data.insert("missing".into(), serde_json::json!(missing));
    out.push(Claim {
        outlet_id: outlet_id.to_string(),
        field: "site_pages".into(),
        value,
        data: Some(data),
        source_id: SOURCE_ID.to_string(),
        ref_url: ref_url.clone(),
        retrieved_at,
        as_of: None,
        note: Some("Looked for in home-page links only; a page that exists but is not linked there counts as not found.".into()),
    });
    if let Some(n) = &home.site_name {
        out.push(Claim {
            outlet_id: outlet_id.to_string(),
            field: "name".into(),
            value: n.clone(),
            data: Some(BTreeMap::from([(
                "domain".to_string(),
                serde_json::Value::from(obs.domain.clone()),
            )])),
            source_id: SOURCE_ID.to_string(),
            ref_url,
            retrieved_at,
            as_of: None,
            note: Some("og:site_name on the home page".into()),
        });
    }
    out
}

/// Fetch and observe one domain: HTTPS home page, then plain HTTP to
/// see where it lands. Network errors are observations, not failures.
pub async fn observe(client: &reqwest::Client, domain: &str) -> Observation {
    let mut obs = Observation {
        domain: domain.to_string(),
        ..Default::default()
    };
    match client.get(format!("https://{domain}/")).send().await {
        Ok(resp) => {
            obs.status = Some(resp.status().as_u16());
            obs.final_url = Some(resp.url().to_string());
            obs.https_ok = resp.status().is_success();
            if obs.https_ok {
                let base = resp.url().clone();
                if let Ok(body) = resp.text().await {
                    obs.home = Some(parse_home(&body, &base));
                }
            }
        }
        Err(e) => tracing::debug!(domain, "https: {e}"),
    }
    match client.get(format!("http://{domain}/")).send().await {
        Ok(resp) => obs.http_to_https = Some(resp.url().scheme() == "https"),
        Err(e) => tracing::debug!(domain, "http: {e}"),
    }
    obs
}

pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .gzip(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/site/home.html"),
        )
        .unwrap()
    }

    #[test]
    fn finds_accountability_pages_and_site_name() {
        let base = Url::parse("https://www.example-gazette.com/").unwrap();
        let p = parse_home(&fixture(), &base);
        assert_eq!(p.site_name.as_deref(), Some("The Example Gazette"));
        assert_eq!(p.pages["about"], "https://www.example-gazette.com/about-us");
        assert_eq!(
            p.pages["contact"],
            "https://www.example-gazette.com/contact/"
        );
        assert_eq!(
            p.pages["corrections"],
            "https://www.example-gazette.com/corrections"
        );
        assert_eq!(
            p.pages["privacy"], "https://parent.example/legal/privacy",
            "off-site policy still counts"
        );
        assert!(!p.pages.contains_key("ethics"));
        assert!(!p.pages.contains_key("masthead"));
        assert_eq!(p.pages.len(), 4, "mailto, javascript and nav links ignored");
    }

    #[test]
    fn claims_state_found_and_missing() {
        let base = Url::parse("https://www.example-gazette.com/").unwrap();
        let obs = Observation {
            domain: "example-gazette.com".into(),
            https_ok: true,
            final_url: Some(base.to_string()),
            status: Some(200),
            http_to_https: Some(true),
            home: Some(parse_home(&fixture(), &base)),
        };
        let c = claims("gazette", &obs, 1);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].field, "site_https");
        assert_eq!(c[0].value, "https; plain http redirects to it");
        assert_eq!(c[0].ref_url, "https://www.example-gazette.com/");
        assert_eq!(c[1].field, "site_pages");
        assert_eq!(
            c[1].value,
            "links about, contact, privacy, corrections pages from its home page; no ethics, masthead link found"
        );
        let d = c[1].data.as_ref().unwrap();
        assert_eq!(d["missing"], serde_json::json!(["ethics", "masthead"]));
        assert_eq!(c[2].field, "name");
        assert_eq!(c[2].value, "The Example Gazette");
        for x in &c {
            assert!(
                crate::model::KNOWN_FIELDS.contains(&x.field.as_str()),
                "{}",
                x.field
            );
        }
    }

    #[test]
    fn unreachable_site_is_one_claim() {
        let obs = Observation {
            domain: "dead.example".into(),
            ..Default::default()
        };
        let c = claims("dead", &obs, 1);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].value, "https not served");
        assert_eq!(c[0].ref_url, "https://dead.example/");
        let obs = Observation {
            domain: "old.example".into(),
            https_ok: true,
            http_to_https: Some(false),
            home: Some(HomePage::default()),
            ..Default::default()
        };
        let c = claims("old", &obs, 1);
        assert_eq!(c[0].value, "https; plain http is served separately");
        assert!(c[1].value.starts_with("none of the usual"));
    }
}
