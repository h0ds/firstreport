//! Wikidata importer (plan §4, L1). One SPARQL query per outlet matches
//! entities by official website (P856) against the outlet's domains and
//! pulls country, inception, owner, parent, instance-of and the English
//! Wikipedia article. Each value becomes a claim citing the entity page,
//! licence CC0. Response parsing is separated from fetching so responses
//! in the endpoint's JSON format under `fixtures/wikidata/` test it
//! offline (hand-written to that format, since the endpoint could not be
//! reached where this was built; a live run should replace them).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::model::Claim;
use crate::store::FeedOutlet;

pub const SOURCE_ID: &str = "wikidata";
pub const ENDPOINT: &str = "https://query.wikidata.org/sparql";
pub const USER_AGENT: &str = concat!(
    "kagami-ledger/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/h0ds/kagami)"
);

/// Wikidata items that are not the outlet even when they share its domain.
const SKIP_INSTANCES: &[&str] = &[
    "podcast",
    "television program",
    "radio program",
    "web series",
    "mobile app",
    "video game",
];

/// SPARQL for one outlet: any item whose official website is on one of
/// the outlet's domains, with the properties we record. Labels via the
/// label service in English.
pub fn query(domains: &[String]) -> String {
    let filters: Vec<String> = domains
        .iter()
        .map(|d| {
            let d = d.trim().trim_start_matches("www.").to_ascii_lowercase().replace('"', "");
            format!("CONTAINS(LCASE(STR(?website)), \"//{d}/\") || CONTAINS(LCASE(STR(?website)), \"//www.{d}/\") || STRENDS(LCASE(STR(?website)), \"//{d}\") || STRENDS(LCASE(STR(?website)), \"//www.{d}\")")
        })
        .collect();
    format!(
        "SELECT ?item ?itemLabel ?website ?country ?countryLabel ?inception ?owner ?ownerLabel ?parent ?parentLabel ?instance ?instanceLabel ?article WHERE {{
  ?item wdt:P856 ?website .
  FILTER({})
  OPTIONAL {{ ?item wdt:P17 ?country . }}
  OPTIONAL {{ ?item wdt:P571 ?inception . }}
  OPTIONAL {{ ?item wdt:P127 ?owner . }}
  OPTIONAL {{ ?item wdt:P749 ?parent . }}
  OPTIONAL {{ ?item wdt:P31 ?instance . }}
  OPTIONAL {{ ?article schema:about ?item ; schema:isPartOf <https://en.wikipedia.org/> . }}
  SERVICE wikibase:label {{ bd:serviceParam wikibase:language \"en\" . }}
}} LIMIT 200",
        filters.join(" || ")
    )
}

#[derive(Debug, Deserialize)]
struct SparqlResponse {
    results: Results,
}

#[derive(Debug, Deserialize)]
struct Results {
    bindings: Vec<BTreeMap<String, Binding>>,
}

#[derive(Debug, Deserialize)]
struct Binding {
    value: String,
}

/// Structured form attached to a claim (`{"qid": "Q145"}`).
type Data = BTreeMap<String, serde_json::Value>;
/// item qid → (label, field → distinct (value, data) pairs)
type Items = BTreeMap<String, (String, BTreeMap<&'static str, Vec<(String, Option<Data>)>>)>;

fn qid(uri: &str) -> Option<&str> {
    uri.rsplit('/').next().filter(|q| q.starts_with('Q'))
}

/// Turn a SPARQL JSON response into claims for `outlet_id`.
///
/// Several bindings per item are normal (one per instance-of, one per
/// owner); values are deduplicated per field. Items whose instance-of is
/// a podcast, programme or app are skipped: they share the domain but are
/// not the outlet. If more than one item survives, all are recorded and
/// each claim's note names the item so a reviewer can prune.
pub fn parse_response(
    outlet_id: &str,
    json: &str,
    retrieved_at: i64,
) -> Result<Vec<Claim>, String> {
    let resp: SparqlResponse =
        serde_json::from_str(json).map_err(|e| format!("wikidata response: {e}"))?;
    // item qid → (label, field → set of (value, data))
    let mut items: Items = BTreeMap::new();
    let mut skipped: std::collections::HashSet<String> = std::collections::HashSet::new();
    for b in &resp.results.bindings {
        let Some(item_uri) = b.get("item").map(|v| v.value.as_str()) else {
            continue;
        };
        let Some(q) = qid(item_uri) else { continue };
        if let Some(inst) = b.get("instanceLabel").map(|v| v.value.to_ascii_lowercase()) {
            if SKIP_INSTANCES.iter().any(|s| inst == *s) {
                skipped.insert(q.to_string());
                continue;
            }
        }
        let label = b
            .get("itemLabel")
            .map(|v| v.value.clone())
            .unwrap_or_else(|| q.to_string());
        let entry = items
            .entry(q.to_string())
            .or_insert_with(|| (label, BTreeMap::new()));
        let mut push = |field: &'static str,
                        value: String,
                        data: Option<BTreeMap<String, serde_json::Value>>| {
            let list = entry.1.entry(field).or_default();
            if !list.iter().any(|(v, _)| v == &value) {
                list.push((value, data));
            }
        };
        let linked = |key: &str, label_key: &str| -> Option<(String, Data)> {
            let uri = b.get(key)?.value.as_str();
            let q = qid(uri)?;
            let label = b
                .get(label_key)
                .map(|v| v.value.clone())
                .unwrap_or_else(|| q.to_string());
            Some((
                label,
                BTreeMap::from([("qid".to_string(), serde_json::Value::String(q.to_string()))]),
            ))
        };
        if let Some(v) = b.get("website") {
            push("website", v.value.clone(), None);
        }
        if let Some((v, d)) = linked("country", "countryLabel") {
            push("country", v, Some(d));
        }
        if let Some(v) = b.get("inception") {
            let date = v.value.get(..10).unwrap_or(&v.value).to_string();
            push("founded", date, None);
        }
        if let Some((v, d)) = linked("owner", "ownerLabel") {
            push("owner", v, Some(d));
        }
        if let Some((v, d)) = linked("parent", "parentLabel") {
            push("parent", v, Some(d));
        }
        if let Some((v, d)) = linked("instance", "instanceLabel") {
            push("legal_form", v, Some(d));
        }
        if let Some(v) = b.get("article") {
            push("wikipedia", v.value.clone(), None);
        }
    }
    for q in &skipped {
        items.remove(q);
    }
    let multiple = items.len() > 1;
    let mut out = Vec::new();
    for (q, (label, fields)) in &items {
        let ref_url = format!("https://www.wikidata.org/wiki/{q}");
        let note = multiple.then(|| {
            format!(
                "one of {} items matching the domain; this is {q} ({label})",
                items.len()
            )
        });
        let mut claim =
            |field: &str, value: String, data: Option<BTreeMap<String, serde_json::Value>>| {
                out.push(Claim {
                    outlet_id: outlet_id.to_string(),
                    field: field.to_string(),
                    value,
                    data,
                    source_id: SOURCE_ID.to_string(),
                    ref_url: ref_url.clone(),
                    retrieved_at,
                    as_of: None,
                    note: note.clone(),
                });
            };
        claim("wikidata_id", q.clone(), None);
        claim("name", label.clone(), None);
        for (field, values) in fields {
            for (v, d) in values {
                claim(field, v.clone(), d.clone());
            }
        }
    }
    Ok(out)
}

/// Run the query for one outlet against the live endpoint.
pub async fn fetch(client: &reqwest::Client, outlet: &FeedOutlet) -> Result<String, String> {
    let q = query(&outlet.domains);
    let resp = client
        .get(ENDPOINT)
        .query(&[("query", q.as_str()), ("format", "json")])
        .header(reqwest::header::ACCEPT, "application/sparql-results+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "wikidata {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    Ok(body)
}

pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .gzip(true)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/wikidata")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn query_matches_domains_with_and_without_www() {
        let q = query(&["bbc.co.uk".into(), "www.bbc.com".into()]);
        assert!(q.contains("//bbc.co.uk/") && q.contains("//www.bbc.co.uk/"));
        assert!(
            q.contains("//bbc.com/"),
            "www. stripped before building the filter"
        );
        assert!(q.contains("wdt:P856") && q.contains("wdt:P127") && q.contains("schema:about"));
    }

    #[test]
    fn bbc_response_becomes_deduplicated_claims() {
        let claims = parse_response("bbc", &fixture("bbc.json"), 1_700_000_000).unwrap();
        let get = |f: &str| {
            claims
                .iter()
                .filter(|c| c.field == f)
                .map(|c| c.value.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("wikidata_id"), vec!["Q9531"]);
        assert_eq!(get("name"), vec!["BBC"]);
        assert_eq!(
            get("country"),
            vec!["United Kingdom"],
            "two bindings, one country"
        );
        assert_eq!(get("founded"), vec!["1922-10-18"]);
        assert_eq!(
            get("legal_form"),
            vec!["media company", "public broadcaster"]
        );
        assert_eq!(get("wikipedia"), vec!["https://en.wikipedia.org/wiki/BBC"]);
        assert!(get("owner").is_empty());
        let c = claims.iter().find(|c| c.field == "country").unwrap();
        assert_eq!(c.ref_url, "https://www.wikidata.org/wiki/Q9531");
        assert_eq!(c.data.as_ref().unwrap()["qid"], "Q145");
        assert_eq!(c.source_id, "wikidata");
        assert!(c.note.is_none(), "single match needs no note");
        for c in &claims {
            assert!(
                crate::model::KNOWN_FIELDS.contains(&c.field.as_str()),
                "{}",
                c.field
            );
        }
    }

    #[test]
    fn owner_parent_and_skipped_podcast() {
        let claims = parse_response("herald", &fixture("owned.json"), 1).unwrap();
        let get = |f: &str| {
            claims
                .iter()
                .filter(|c| c.field == f)
                .map(|c| c.value.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("owner"), vec!["Example Media Holdings"]);
        assert_eq!(get("parent"), vec!["Example Group"]);
        assert_eq!(
            get("wikidata_id"),
            vec!["Q1000001"],
            "the podcast item is not the outlet"
        );
        assert!(claims.iter().all(|c| c.note.is_none()));
    }

    #[test]
    fn empty_and_broken_responses() {
        assert!(parse_response("x", r#"{"results":{"bindings":[]}}"#, 1)
            .unwrap()
            .is_empty());
        assert!(parse_response("x", "not json", 1).is_err());
    }
}
