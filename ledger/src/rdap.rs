//! RDAP domain-registration importer: the first source that is ours to
//! measure rather than someone's to license. RDAP is the registries'
//! own JSON protocol (RFC 9083); for a domain it gives the registration
//! date, the last change, the registrar and the status flags. Domain age
//! is a plain fact and a useful one: an outlet whose domain is three
//! months old is a different thing from one registered in 1995.
//!
//! Lookups go through the IANA-listed registry servers via the
//! rdap.org redirector, one request per registrable domain, at most one
//! a second. Parsing is separated from fetching so a saved response
//! tests it offline.

// Wired into `ledger import` in the next commit.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::model::Claim;
use crate::store::FeedOutlet;

pub const SOURCE_ID: &str = "rdap";
pub const REDIRECTOR: &str = "https://rdap.org/domain/";
pub const USER_AGENT: &str = concat!(
    "kagami-ledger/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/h0ds/kagami)"
);

/// The registrable domain of a host: `news.bbc.co.uk` → `bbc.co.uk`,
/// `www.example.com` → `example.com`. A small second-level list stands
/// in for the public suffix list; wrong only for exotic suffixes, and a
/// wrong guess just yields "no such domain" rather than a wrong fact.
pub fn registrable(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() < 2 {
        return None;
    }
    let second_level = [
        "co", "com", "org", "net", "gov", "ac", "edu", "or", "ne", "go", "gob", "mil", "nom",
    ];
    let n = labels.len();
    let tld = labels[n - 1];
    let sld = labels[n - 2];
    let take = if n >= 3 && tld.len() == 2 && second_level.contains(&sld) {
        3
    } else {
        2
    };
    Some(labels[n - take..].join("."))
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(rename = "ldhName", default)]
    ldh_name: String,
    #[serde(default)]
    status: Vec<String>,
    #[serde(default)]
    events: Vec<Event>,
    #[serde(default)]
    entities: Vec<Entity>,
    #[serde(default)]
    links: Vec<Link>,
}

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "eventAction", default)]
    action: String,
    #[serde(rename = "eventDate", default)]
    date: String,
}

#[derive(Debug, Deserialize)]
struct Entity {
    #[serde(default)]
    roles: Vec<String>,
    #[serde(rename = "vcardArray", default)]
    vcard: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct Link {
    #[serde(default)]
    rel: String,
    #[serde(default)]
    href: String,
}

/// What we keep from a domain record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub domain: String,
    pub registered: Option<String>,
    pub last_changed: Option<String>,
    pub expires: Option<String>,
    pub registrar: Option<String>,
    pub status: Vec<String>,
    /// The registry's own record URL, when the response links itself.
    pub self_url: Option<String>,
}

fn vcard_fn(v: &serde_json::Value) -> Option<String> {
    // ["vcard", [["version",{},"text","4.0"], ["fn",{},"text","Name"], …]]
    let props = v.get(1)?.as_array()?;
    for p in props {
        let arr = p.as_array()?;
        if arr.first()?.as_str()? == "fn" {
            let name = arr.get(3)?.as_str()?.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn date_only(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    (d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-').then(|| d.to_string())
}

pub fn parse_response(json: &str) -> Result<Registration, String> {
    let r: Response = serde_json::from_str(json).map_err(|e| format!("rdap response: {e}"))?;
    let event = |name: &str| {
        r.events
            .iter()
            .find(|e| e.action.eq_ignore_ascii_case(name))
            .and_then(|e| date_only(&e.date))
    };
    Ok(Registration {
        domain: r.ldh_name.to_ascii_lowercase(),
        registered: event("registration"),
        last_changed: event("last changed"),
        expires: event("expiration"),
        registrar: r
            .entities
            .iter()
            .find(|e| e.roles.iter().any(|x| x == "registrar"))
            .and_then(|e| vcard_fn(&e.vcard)),
        status: r.status.clone(),
        self_url: r
            .links
            .iter()
            .find(|l| l.rel == "self")
            .map(|l| l.href.clone())
            .filter(|h| !h.is_empty()),
    })
}

/// Claims for one outlet from one domain record: `domain_registered` is
/// the fact that matters; registrar and last change ride along in data.
pub fn claims(outlet_id: &str, reg: &Registration, retrieved_at: i64) -> Vec<Claim> {
    let Some(registered) = &reg.registered else {
        return vec![];
    };
    let ref_url = reg
        .self_url
        .clone()
        .unwrap_or_else(|| format!("{REDIRECTOR}{}", reg.domain));
    let mut data: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    data.insert(
        "domain".into(),
        serde_json::Value::String(reg.domain.clone()),
    );
    if let Some(r) = &reg.registrar {
        data.insert("registrar".into(), serde_json::Value::String(r.clone()));
    }
    if let Some(c) = &reg.last_changed {
        data.insert("last_changed".into(), serde_json::Value::String(c.clone()));
    }
    if let Some(e) = &reg.expires {
        data.insert("expires".into(), serde_json::Value::String(e.clone()));
    }
    if !reg.status.is_empty() {
        data.insert("status".into(), serde_json::json!(reg.status));
    }
    let mut out = vec![Claim {
        outlet_id: outlet_id.to_string(),
        field: "domain_registered".into(),
        value: registered.clone(),
        data: Some(data),
        source_id: SOURCE_ID.to_string(),
        ref_url: ref_url.clone(),
        retrieved_at,
        as_of: reg.last_changed.clone(),
        note: None,
    }];
    if let Some(r) = &reg.registrar {
        out.push(Claim {
            outlet_id: outlet_id.to_string(),
            field: "registrar".into(),
            value: r.clone(),
            data: Some(BTreeMap::from([(
                "domain".to_string(),
                serde_json::Value::String(reg.domain.clone()),
            )])),
            source_id: SOURCE_ID.to_string(),
            ref_url,
            retrieved_at,
            as_of: reg.last_changed.clone(),
            note: None,
        });
    }
    out
}

/// Distinct registrable domains for an outlet, in its domain order.
pub fn domains_for(outlet: &FeedOutlet) -> Vec<String> {
    let mut out = Vec::new();
    for d in &outlet.domains {
        if let Some(r) = registrable(d) {
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}

pub async fn fetch(client: &reqwest::Client, domain: &str) -> Result<String, String> {
    let url = format!("{REDIRECTOR}{domain}");
    let resp = client
        .get(&url)
        .header(
            reqwest::header::ACCEPT,
            "application/rdap+json, application/json",
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if status.as_u16() == 404 {
        return Err(format!(
            "no RDAP record for {domain} (unregistered, or a registry without RDAP)"
        ));
    }
    if !status.is_success() {
        return Err(format!("rdap {status} for {domain}"));
    }
    Ok(body)
}

pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .gzip(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/rdap/example.json"),
        )
        .unwrap()
    }

    #[test]
    fn registrable_domains() {
        assert_eq!(registrable("news.bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(
            registrable("www.example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(registrable("feeds.npr.org").as_deref(), Some("npr.org"));
        assert_eq!(registrable("abc.net.au").as_deref(), Some("abc.net.au"));
        assert_eq!(registrable("example.com.").as_deref(), Some("example.com"));
        assert_eq!(registrable("localhost"), None);
        let o = FeedOutlet {
            id: "bbc".into(),
            name: "BBC".into(),
            domains: vec![
                "bbc.co.uk".into(),
                "news.bbc.co.uk".into(),
                "bbc.com".into(),
            ],
        };
        assert_eq!(domains_for(&o), vec!["bbc.co.uk", "bbc.com"]);
    }

    #[test]
    fn parses_a_registry_response() {
        let r = parse_response(&fixture()).unwrap();
        assert_eq!(r.domain, "example.com");
        assert_eq!(r.registered.as_deref(), Some("1995-08-14"));
        assert_eq!(r.last_changed.as_deref(), Some("2025-08-14"));
        assert_eq!(r.expires.as_deref(), Some("2026-08-13"));
        assert_eq!(
            r.registrar.as_deref(),
            Some("RESERVED-Internet Assigned Numbers Authority")
        );
        assert_eq!(r.status.len(), 3);
        assert_eq!(
            r.self_url.as_deref(),
            Some("https://rdap.verisign.com/com/v1/domain/EXAMPLE.COM")
        );
    }

    #[test]
    fn claims_carry_the_registration_date_and_provenance() {
        let r = parse_response(&fixture()).unwrap();
        let c = claims("example", &r, 1_700_000_000);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].field, "domain_registered");
        assert_eq!(c[0].value, "1995-08-14");
        assert_eq!(
            c[0].ref_url, "https://rdap.verisign.com/com/v1/domain/EXAMPLE.COM",
            "registry's own record, not the redirector"
        );
        assert_eq!(c[0].as_of.as_deref(), Some("2025-08-14"));
        let d = c[0].data.as_ref().unwrap();
        assert_eq!(d["domain"], "example.com");
        assert_eq!(
            d["registrar"],
            "RESERVED-Internet Assigned Numbers Authority"
        );
        assert_eq!(d["status"].as_array().unwrap().len(), 3);
        assert_eq!(c[1].field, "registrar");
        for x in &c {
            assert!(
                crate::model::KNOWN_FIELDS.contains(&x.field.as_str()),
                "{}",
                x.field
            );
        }
    }

    #[test]
    fn missing_registration_yields_nothing() {
        let r = parse_response(r#"{"ldhName":"X.ORG","events":[{"eventAction":"last changed","eventDate":"2020-01-01T00:00:00Z"}]}"#).unwrap();
        assert!(claims("x", &r, 1).is_empty());
        assert!(parse_response("nope").is_err());
    }
}
