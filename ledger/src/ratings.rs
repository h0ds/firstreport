//! Lean ratings from outside raters (plan §4), each on its own scale and
//! each named as the source's, never Kagami's.
//!
//! - `allsides`: AllSides Media Bias Ratings™ (CC BY-NC 4.0, confirmed in
//!   AllSides' terms of use, October 2025). AllSides blocks automated
//!   access and supplies the ratings as CSV or JSON on request, so the
//!   importer reads that file (`--file`); it never scrapes the site.
//! - `robertson2018`: partisan audience bias scores for web domains
//!   (Robertson et al. 2018, CC0 on Harvard Dataverse). A measure of who
//!   shares an outlet's links, not of its content, so it is recorded as
//!   `audience_partisanship` with its number and scale, not as a label.

use std::collections::BTreeMap;

use serde_json::json;

use crate::model::Claim;
use crate::store::FeedOutlet;

pub const ALLSIDES: &str = "allsides";
pub const ROBERTSON: &str = "robertson2018";

/// AllSides' five labels, in their own order.
pub const ALLSIDES_LABELS: [&str; 5] = ["Left", "Lean Left", "Center", "Lean Right", "Right"];
pub const ALLSIDES_RATINGS_URL: &str = "https://www.allsides.com/media-bias/ratings";

pub const ROBERTSON_DOI: &str = "doi:10.7910/DVN/QAN5VX";
pub const ROBERTSON_PAGE: &str =
    "https://dataverse.harvard.edu/dataset.xhtml?persistentId=doi:10.7910/DVN/QAN5VX";

/// `example.com` from a domain or URL, lowercase, without `www.`.
pub fn host_of(s: &str) -> Option<String> {
    let s = s.trim().trim_matches('"').to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let with_scheme = if s.contains("://") { s } else { format!("https://{s}") };
    let host = url::Url::parse(&with_scheme).ok()?.host_str()?.to_string();
    Some(host.trim_start_matches("www.").to_string())
}

/// The registry outlet whose domain is `host`, or of which `host` is a
/// subdomain.
pub fn outlet_for<'a>(outlets: &'a [FeedOutlet], host: &str) -> Option<&'a FeedOutlet> {
    let bare = |d: &str| d.trim().to_ascii_lowercase().trim_start_matches("www.").to_string();
    outlets
        .iter()
        .find(|o| o.domains.iter().any(|d| bare(d) == host))
        .or_else(|| {
            outlets
                .iter()
                .find(|o| o.domains.iter().any(|d| host.ends_with(&format!(".{}", bare(d)))))
        })
}

/// An AllSides label in its canonical spelling ("lean-left" → "Lean Left").
pub fn allsides_label(s: &str) -> Option<&'static str> {
    let k: String = s
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    ALLSIDES_LABELS
        .iter()
        .find(|l| l.to_ascii_lowercase().replace(' ', "") == k)
        .copied()
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

/// One rated row from AllSides' file: host, label, their name for it,
/// and their page for the source when the file gives one.
#[derive(Debug, Clone, PartialEq)]
pub struct AllSidesRow {
    pub host: String,
    pub label: &'static str,
    pub name: String,
    pub page: Option<String>,
}

/// Rows from AllSides' CSV or JSON. Their exact column names are not
/// published, so columns are found by content: the rating column is the
/// one whose values are AllSides labels, the host column the one whose
/// values are domains or outlet URLs (not allsides.com pages).
pub fn parse_allsides(text: &str) -> Result<Vec<AllSidesRow>, String> {
    let records: Vec<Vec<String>> = if text.trim_start().starts_with('[') || text.trim_start().starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("json: {e}"))?;
        let arr = match &v {
            serde_json::Value::Array(a) => a.clone(),
            serde_json::Value::Object(o) => o
                .values()
                .find_map(|x| x.as_array().cloned())
                .ok_or("json: no array of rows")?,
            _ => return Err("json: no array of rows".into()),
        };
        arr.iter()
            .filter_map(|r| r.as_object())
            .map(|o| {
                o.values()
                    .map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()))
                    .collect()
            })
            .collect()
    } else {
        text.lines().skip(1).filter(|l| !l.trim().is_empty()).map(split_csv_line).collect()
    };
    let mut out = Vec::new();
    for r in records {
        let Some(label) = r.iter().find_map(|v| allsides_label(v)) else {
            continue;
        };
        let page = r
            .iter()
            .find(|v| v.contains("allsides.com/news-source") || v.contains("allsides.com/media-bias"))
            .cloned();
        let host = r.iter().find_map(|v| {
            let looks = v.contains('.') && !v.contains(' ') && !v.contains("allsides.com");
            looks.then(|| host_of(v)).flatten()
        });
        let Some(host) = host else { continue };
        let name = r
            .iter()
            .find(|v| !v.is_empty() && allsides_label(v).is_none() && !v.contains('.') && !v.contains("://"))
            .cloned()
            .unwrap_or_default();
        out.push(AllSidesRow { host, label, name, page });
    }
    if out.is_empty() {
        return Err("no rows with an AllSides label and a domain or URL".into());
    }
    Ok(out)
}

pub fn allsides_claim(outlet_id: &str, row: &AllSidesRow, retrieved_at: i64) -> Claim {
    let mut data = BTreeMap::new();
    data.insert("scale".into(), json!(ALLSIDES_LABELS));
    data.insert("host".into(), json!(row.host));
    if !row.name.is_empty() {
        data.insert("allsides_name".into(), json!(row.name));
    }
    Claim {
        outlet_id: outlet_id.into(),
        field: "bias".into(),
        value: row.label.into(),
        data: Some(data),
        source_id: ALLSIDES.into(),
        ref_url: row.page.clone().unwrap_or_else(|| ALLSIDES_RATINGS_URL.into()),
        retrieved_at,
        as_of: None,
        note: Some("AllSides Media Bias Ratings™ by AllSides.com, licensed CC BY-NC 4.0; AllSides' label on AllSides' scale, not Kagami's.".into()),
    }
}

/// `domain → score` from the Robertson et al. file (tab-separated,
/// quoted domains). Only the authors' own `score` column is read: the
/// file also carries other raters' scores, whose terms are their own.
pub fn parse_robertson(text: &str) -> Result<BTreeMap<String, f64>, String> {
    let mut lines = text.lines();
    let header = split_csv_line(lines.next().ok_or("empty file")?);
    let di = header.iter().position(|h| h == "domain").ok_or("no domain column")?;
    let si = header.iter().position(|h| h == "score").ok_or("no score column")?;
    let mut out = BTreeMap::new();
    for l in lines {
        let r = split_csv_line(l);
        let (Some(d), Some(s)) = (r.get(di), r.get(si)) else { continue };
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
    let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|e| format!("dataverse: {e}"))?;
    let id = meta["data"]["latestVersion"]["files"][0]["dataFile"]["id"]
        .as_i64()
        .ok_or("dataverse: no file id")?;
    client
        .get(format!("https://dataverse.harvard.edu/api/access/datafile/{id}"))
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

    fn outlets() -> Vec<FeedOutlet> {
        vec![
            FeedOutlet { id: "bbc".into(), name: "BBC".into(), domains: vec!["bbc.co.uk".into(), "bbc.com".into()] },
            FeedOutlet { id: "fox".into(), name: "Fox News".into(), domains: vec!["foxnews.com".into()] },
        ]
    }

    #[test]
    fn allsides_csv_columns_are_found_by_content() {
        let csv = "news_source,rating,url,allsides_page\n\"BBC News\",center,https://www.bbc.com/news,https://www.allsides.com/news-sources/bbc-news-media-bias\nFox News (Online),Right,foxnews.com,\nSomething,not a rating,x.com,\n";
        let rows = parse_allsides(csv).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].host.as_str(), rows[0].label), ("bbc.com", "Center"));
        assert_eq!(rows[0].page.as_deref(), Some("https://www.allsides.com/news-sources/bbc-news-media-bias"));
        assert_eq!((rows[1].host.as_str(), rows[1].label), ("foxnews.com", "Right"));
        let o = outlets();
        assert_eq!(outlet_for(&o, &rows[0].host).unwrap().id, "bbc");
        let c = allsides_claim("bbc", &rows[0], 1);
        assert_eq!((c.field.as_str(), c.value.as_str(), c.source_id.as_str()), ("bias", "Center", "allsides"));
    }

    #[test]
    fn allsides_json_and_label_spellings() {
        let json = r#"{"allsides_media_bias_ratings":[{"name":"Fox News","bias_rating":"lean-right","site_url":"https://foxnews.com"}]}"#;
        let rows = parse_allsides(json).unwrap();
        assert_eq!((rows[0].host.as_str(), rows[0].label), ("foxnews.com", "Lean Right"));
        assert_eq!(allsides_label("Lean Left"), Some("Lean Left"));
        assert_eq!(allsides_label("Mixed"), None);
    }

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
