//! Lean ratings from outside raters (plan §4), each on its own scale and
//! each named as the source's, never First Report's.
//!
//! - `robertson2018`: partisan audience bias scores for web domains
//!   (Robertson et al. 2018, CC0 on Harvard Dataverse). A measure of who
//!   shares an outlet's links, not of its content, so it is recorded as
//!   `audience_partisanship` with its number and scale, not as a label.

use std::collections::BTreeMap;

use serde_json::json;

use crate::model::Claim;

pub const ROBERTSON: &str = "robertson2018";

pub const ROBERTSON_DOI: &str = "doi:10.7910/DVN/QAN5VX";
pub const ROBERTSON_PAGE: &str =
    "https://dataverse.harvard.edu/dataset.xhtml?persistentId=doi:10.7910/DVN/QAN5VX";

/// `example.com` from a domain or URL, lowercase, without `www.`.
pub fn host_of(s: &str) -> Option<String> {
    let s = s.trim().trim_matches('"').to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let with_scheme = if s.contains("://") {
        s
    } else {
        format!("https://{s}")
    };
    let host = url::Url::parse(&with_scheme).ok()?.host_str()?.to_string();
    Some(host.trim_start_matches("www.").to_string())
}

fn split_csv_line(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' | '\t' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/// `domain → score` from the Robertson et al. file (tab-separated,
/// quoted domains). Only the authors' own `score` column is read: the
/// file also carries other raters' scores, whose terms are their own.
pub fn parse_robertson(text: &str) -> Result<BTreeMap<String, f64>, String> {
    let mut lines = text.lines();
    let header = split_csv_line(lines.next().ok_or("empty file")?);
    let di = header
        .iter()
        .position(|h| h == "domain")
        .ok_or("no domain column")?;
    let si = header
        .iter()
        .position(|h| h == "score")
        .ok_or("no score column")?;
    let mut out = BTreeMap::new();
    for l in lines {
        let r = split_csv_line(l);
        let (Some(d), Some(s)) = (r.get(di), r.get(si)) else {
            continue;
        };
        if let (Some(h), Ok(v)) = (host_of(d), s.parse::<f64>()) {
            out.insert(h, v);
        }
    }
    Ok(out)
}

pub fn robertson_claim(outlet_id: &str, host: &str, score: f64, retrieved_at: i64) -> Claim {
    let mut data = BTreeMap::new();
    data.insert("score".into(), json!(score));
    data.insert("scale".into(), json!({"min": -1, "max": 1, "min_means": "shared only by Democrats", "max_means": "shared only by Republicans"}));
    data.insert("host".into(), json!(host));
    Claim {
        outlet_id: outlet_id.into(),
        field: "audience_partisanship".into(),
        value: format!("{score:+.2} on −1 (shared by Democrats) to +1 (shared by Republicans)"),
        data: Some(data),
        source_id: ROBERTSON.into(),
        ref_url: ROBERTSON_PAGE.into(),
        retrieved_at,
        as_of: Some("2017".into()),
        note: Some("Who shared the outlet's links on Twitter in 2017, among ~500,000 US accounts of known party; a measure of its audience, not of its content. Robertson et al. 2018, CC0.".into()),
    }
}

/// The CC0 file from Harvard Dataverse, via its public API.
pub async fn fetch_robertson(client: &reqwest::Client) -> Result<String, String> {
    let meta = client
        .get("https://dataverse.harvard.edu/api/datasets/:persistentId/")
        .query(&[("persistentId", ROBERTSON_DOI)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let meta: serde_json::Value =
        serde_json::from_str(&meta).map_err(|e| format!("dataverse: {e}"))?;
    let id = meta["data"]["latestVersion"]["files"][0]["dataFile"]["id"]
        .as_i64()
        .ok_or("dataverse: no file id")?;
    client
        .get(format!(
            "https://dataverse.harvard.edu/api/access/datafile/{id}"
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robertson_reads_only_its_own_score() {
        let tab = "domain\tscore\tfb_score\tallsides_score\n\"foxnews.com\"\t0.7003\t0.8\t1\n\"www.bbc.co.uk\"\t-0.1234\t\t\n\"bad\"\tx\t\t\n";
        let m = parse_robertson(tab).unwrap();
        assert_eq!(m.get("foxnews.com"), Some(&0.7003));
        assert_eq!(m.get("bbc.co.uk"), Some(&-0.1234));
        assert_eq!(m.len(), 2);
        let c = robertson_claim("fox", "foxnews.com", 0.7003, 1);
        assert_eq!(c.field, "audience_partisanship");
        assert!(c.value.starts_with("+0.70 on"));
    }
}
