//! Feed-behaviour importer: what an outlet's own feed does, measured
//! from `feed`'s export. Posting cadence, how often it revises or
//! withdraws items after publication, how much of its text is a copy
//! of another outlet's (syndication), and how often it is first to a
//! story other outlets also carry.
//!
//! These are Kagami's own observations, so there is no licence to
//! honour and nothing to scrape; they are also the easiest claims to
//! misread, so every one carries the window, the counts and the
//! denominator it was computed from, and outlets with too few items in
//! the window get no claim at all. A measurement is not a verdict: a
//! wire service is *supposed* to be copied.
//!
//! Reads the NDJSON export (`feed export --format ndjson`), which is
//! the frozen data contract, rather than feed's database.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::BufRead;
use std::path::Path;

use serde::Deserialize;

use crate::model::Claim;

pub const SOURCE_ID: &str = "feed_behaviour";
/// Fewer items than this in the window and the outlet gets no claims.
pub const MIN_ITEMS: u32 = 10;
pub const DEFAULT_WINDOW_DAYS: i64 = 90;
const DAY: i64 = 86_400;

#[derive(Debug, Deserialize)]
struct Item {
    id: u64,
    outlet_id: String,
    #[serde(default)]
    fetched_at: i64,
}

#[derive(Debug, Deserialize)]
struct Cluster {
    #[serde(default)]
    outlet_count: u32,
    #[serde(default)]
    origin_outlet_id: String,
    #[serde(default)]
    timeline: Vec<Join>,
    #[serde(default)]
    syndication: Vec<SynGroup>,
}

#[derive(Debug, Deserialize)]
struct Join {
    item_id: u64,
    outlet_id: String,
}

#[derive(Debug, Deserialize)]
struct SynGroup {
    #[serde(default)]
    item_ids: Vec<u64>,
    #[serde(default)]
    first_outlet_id: String,
}

#[derive(Debug, Deserialize)]
struct Event {
    kind: String,
    #[serde(default)]
    item_id: Option<u64>,
}

/// One outlet's counts over the window. Everything the claims say is
/// derivable from these.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub items: u32,
    pub revised: u32,
    pub withdrawn: u32,
    /// Items whose text first appeared at another outlet.
    pub copies: u32,
    /// Items whose text later appeared at another outlet.
    pub copied: u32,
    /// Clusters with at least two outlets that this outlet is in…
    pub shared_stories: u32,
    /// …and was the earliest published member of.
    pub origin_of: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    pub end: i64,
}

impl Window {
    pub fn days(&self) -> i64 {
        ((self.end - self.start) / DAY).max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub window: Window,
    pub outlets: BTreeMap<String, Counts>,
}

fn read_ndjson<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, line) in std::io::BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(&line)
                .map_err(|e| format!("{} line {}: {e}", path.display(), n + 1))?,
        );
    }
    Ok(out)
}

/// Measure every outlet in an export directory over the last
/// `window_days` ending at the newest fetch in the export, so a stale
/// export measures the period it covers rather than an empty one.
pub fn measure(export_dir: &Path, window_days: i64) -> Result<Report, String> {
    let items: Vec<Item> = read_ndjson(&export_dir.join("items.ndjson"))?;
    let clusters: Vec<Cluster> = read_ndjson(&export_dir.join("clusters.ndjson"))?;
    let events: Vec<Event> = read_ndjson(&export_dir.join("events.ndjson"))?;
    let end = items.iter().map(|i| i.fetched_at).max().unwrap_or(0);
    let start = end - window_days.max(1) * DAY;
    let window = Window { start, end };

    let mut outlets: BTreeMap<String, Counts> = BTreeMap::new();
    let mut outlet_of: HashMap<u64, &str> = HashMap::new();
    for i in &items {
        if i.fetched_at < start || i.fetched_at > end {
            continue;
        }
        outlets.entry(i.outlet_id.clone()).or_default().items += 1;
        outlet_of.insert(i.id, &i.outlet_id);
    }
    let mut seen: HashSet<(u64, &str)> = HashSet::new();
    for e in &events {
        let Some(id) = e.item_id else { continue };
        let Some(o) = outlet_of.get(&id) else {
            continue;
        };
        if !seen.insert((id, e.kind.as_str())) {
            continue;
        }
        let c = outlets.get_mut(*o).expect("outlet counted");
        match e.kind.as_str() {
            "revised" => c.revised += 1,
            "withdrawn" => c.withdrawn += 1,
            _ => {}
        }
    }
    for cl in &clusters {
        for g in &cl.syndication {
            for id in &g.item_ids {
                let Some(o) = outlet_of.get(id) else { continue };
                let c = outlets.get_mut(*o).expect("outlet counted");
                if *o == g.first_outlet_id {
                    let others = g
                        .item_ids
                        .iter()
                        .any(|x| outlet_of.get(x).is_some_and(|y| y != o));
                    if others {
                        c.copied += 1;
                    }
                } else {
                    c.copies += 1;
                }
            }
        }
        if cl.outlet_count < 2 {
            continue;
        }
        let mut members: Vec<&str> = Vec::new();
        for j in &cl.timeline {
            if outlet_of.contains_key(&j.item_id) && !members.contains(&j.outlet_id.as_str()) {
                members.push(&j.outlet_id);
            }
        }
        if members.len() < 2 {
            continue;
        }
        for m in members {
            let c = outlets.get_mut(m).expect("outlet counted");
            c.shared_stories += 1;
            if m == cl.origin_outlet_id {
                c.origin_of += 1;
            }
        }
    }
    Ok(Report { window, outlets })
}

fn pct(n: u32, d: u32) -> String {
    if d == 0 {
        "0%".into()
    } else {
        format!("{:.0}%", 100.0 * n as f64 / d as f64)
    }
}

/// Unix seconds → `YYYY-MM-DD` (civil-from-days, Howard Hinnant).
pub fn date(ts: i64) -> String {
    let z = ts.div_euclid(DAY) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Claims for one outlet. `ref_url` points at feed's API for the
/// outlet, the nearest thing to a page that states these numbers.
pub fn claims(
    outlet_id: &str,
    c: &Counts,
    w: &Window,
    feed_api: &str,
    retrieved_at: i64,
) -> Vec<Claim> {
    if c.items < MIN_ITEMS {
        return vec![];
    }
    let days = w.days();
    let base = |field: &str, value: String, data: BTreeMap<String, serde_json::Value>| {
        let mut data = data;
        data.insert("items".into(), c.items.into());
        data.insert("window_days".into(), days.into());
        data.insert("window_start".into(), date(w.start).into());
        data.insert("window_end".into(), date(w.end).into());
        Claim {
            outlet_id: outlet_id.to_string(),
            field: field.to_string(),
            value,
            data: Some(data),
            source_id: SOURCE_ID.to_string(),
            ref_url: format!(
                "{}/v1/outlets/{}",
                feed_api.trim_end_matches('/'),
                outlet_id
            ),
            retrieved_at,
            as_of: Some(date(w.end)),
            note: None,
        }
    };
    let kv = |pairs: &[(&str, u32)]| -> BTreeMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::from(*v)))
            .collect()
    };
    let mut out = vec![
        base(
            "posting_cadence",
            format!("{:.1} items/day", c.items as f64 / days as f64),
            kv(&[]),
        ),
        base(
            "revision_rate",
            format!(
                "{} of items revised after publication",
                pct(c.revised, c.items)
            ),
            kv(&[("revised", c.revised)]),
        ),
        base(
            "withdrawal_rate",
            format!("{} of items withdrawn", pct(c.withdrawn, c.items)),
            kv(&[("withdrawn", c.withdrawn)]),
        ),
        base(
            "syndication_share",
            format!(
                "{} of items carry text first published elsewhere; {} were carried by others",
                pct(c.copies, c.items),
                pct(c.copied, c.items)
            ),
            kv(&[("copies", c.copies), ("copied_by_others", c.copied)]),
        ),
    ];
    if c.shared_stories > 0 {
        out.push(base(
            "origin_share",
            format!(
                "first to {} of {} stories other outlets also carried",
                pct(c.origin_of, c.shared_stories),
                c.shared_stories
            ),
            kv(&[
                ("origin_of", c.origin_of),
                ("shared_stories", c.shared_stories),
            ]),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/feed_export")
    }

    #[test]
    fn civil_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_756_000_000), "2025-08-24");
    }

    #[test]
    fn measures_the_recorded_export() {
        let r = measure(&export(), 90).unwrap();
        assert_eq!(r.window.days(), 90);
        let wire = &r.outlets["wire"];
        assert_eq!(wire.items, 61);
        assert_eq!(wire.revised, 10);
        assert_eq!(wire.withdrawn, 1);
        assert_eq!(
            (wire.copies, wire.copied),
            (0, 30),
            "wire is copied, never copies"
        );
        assert_eq!((wire.origin_of, wire.shared_stories), (30, 31));
        let gz = &r.outlets["gazette"];
        assert_eq!(gz.items, 31);
        assert_eq!((gz.copies, gz.copied), (30, 0));
        assert_eq!((gz.origin_of, gz.shared_stories), (1, 31));
        assert_eq!(r.outlets["blog"].items, 3);
    }

    #[test]
    fn a_short_window_measures_only_its_span() {
        let r = measure(&export(), 7).unwrap();
        assert_eq!(r.window.days(), 7);
        assert!(r.outlets["wire"].items < 20);
    }

    #[test]
    fn claims_state_window_counts_and_denominators() {
        let r = measure(&export(), 90).unwrap();
        let c = claims(
            "wire",
            &r.outlets["wire"],
            &r.window,
            "http://127.0.0.1:8080/",
            1,
        );
        assert_eq!(c.len(), 5);
        assert_eq!(c[0].field, "posting_cadence");
        assert_eq!(c[0].value, "0.7 items/day");
        assert_eq!(c[0].ref_url, "http://127.0.0.1:8080/v1/outlets/wire");
        let d = c[1].data.as_ref().unwrap();
        assert_eq!(d["revised"], 10);
        assert_eq!(d["items"], 61);
        assert_eq!(d["window_days"], 90);
        assert_eq!(c[1].value, "16% of items revised after publication");
        assert_eq!(c[4].field, "origin_share");
        assert_eq!(
            c[4].value,
            "first to 97% of 31 stories other outlets also carried"
        );
        for x in &c {
            assert!(
                crate::model::KNOWN_FIELDS.contains(&x.field.as_str()),
                "{}",
                x.field
            );
            assert!(!x.field.ends_with("_score"));
        }
        assert!(
            claims("blog", &r.outlets["blog"], &r.window, "http://x", 1).is_empty(),
            "too few items for a claim"
        );
    }
}
