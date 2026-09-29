//! The view a template sees: the `FactSet` as JSON plus derived values
//! under `d.` in the words a sentence needs (times as dates, durations
//! as "2 h 10 min", shares as percentages). Deterministic; no clock.

use serde_json::{json, Map, Value};

use crate::model::FactSet;

/// Resolve a dotted path against a JSON value. Array indexes are numeric
/// segments. `None` for a missing or null leaf.
pub fn get<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in path.split('.') {
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    (!cur.is_null()).then_some(cur)
}

/// Render a leaf as it appears in a sentence.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

pub fn view(fs: &FactSet) -> Value {
    let mut root = serde_json::to_value(fs).unwrap_or(Value::Null);
    let mut d = Map::new();
    let lookup = fs.lookup.as_ref();
    let cluster = lookup.and_then(|l| get(l, "cluster"));
    let mut names = fs.names.outlets.clone();
    if let Some(l) = lookup {
        if let (Some(id), Some(n)) = (get(l, "outlet.id"), get(l, "outlet.name")) {
            names.entry(text(id)).or_insert_with(|| text(n));
        }
    }
    let name = |id: &Value| -> String {
        let id = text(id);
        names.get(&id).cloned().unwrap_or(id)
    };
    let source = |id: &Value| -> String {
        let id = text(id);
        fs.names.sources.get(&id).cloned().unwrap_or(id)
    };

    d.insert("has_lookup".into(), json!(lookup.is_some()));
    if let Some(l) = lookup {
        if let Some(c) = get(l, "checked") {
            d.insert("checked".into(), Value::String(text(c)));
        }
        if let Some(o) = get(l, "outlet") {
            d.insert(
                "outlet_name".into(),
                json!(name(get(o, "name").unwrap_or(&Value::Null))),
            );
        }
    }
    if let Some(c) = cluster {
        if let Some(o) = get(c, "origin_outlet_id") {
            d.insert("origin_outlet".into(), json!(name(o)));
        }
        if let Some(t) = origin_published(c) {
            d.insert("origin_time".into(), json!(date_time(t)));
        }
        if let Some(n) = get(c, "outlet_count").and_then(Value::as_u64) {
            d.insert("outlet_count".into(), json!(n));
            d.insert("outlets_word".into(), json!(plural(n, "outlet")));
        }
        if let Some(s) = get(c, "velocity.time_to_5_outlets_secs").and_then(Value::as_u64) {
            d.insert("t5".into(), json!(duration(s)));
        }
        if let Some((share, first, size)) = biggest_syndication(c) {
            d.insert("syn_share".into(), json!(share));
            d.insert(
                "syn_share_pct".into(),
                json!(format!("{}%", (share * 100.0).round() as u64)),
            );
            d.insert("syn_size".into(), json!(size));
            d.insert("syn_first_outlet".into(), json!(name(&first)));
        }
        if let Some(j) = get(c, "timeline").and_then(Value::as_array) {
            let carried: Vec<String> = j
                .iter()
                .filter_map(|x| get(x, "outlet_id"))
                .map(name)
                .collect::<Vec<_>>();
            let mut seen = std::collections::BTreeSet::new();
            let unique: Vec<String> = carried
                .into_iter()
                .filter(|o| seen.insert(o.clone()))
                .collect();
            d.insert("carried_by".into(), json!(list_of(&unique, 4)));
        }
    }
    // Ledger claims for the subject outlet: first claim per field wins.
    let mut ledger = Map::new();
    if let Some(o) = fs.outlets.first() {
        for c in &o.claims {
            if let Some(f) = get(c, "field").map(text) {
                ledger.entry(f).or_insert_with(|| {
                    json!({
                        "value": get(c, "value").map(text).unwrap_or_default(),
                        "source": get(c, "source_id").map(&source).unwrap_or_default(),
                        "ref_url": get(c, "ref_url").map(text).unwrap_or_default(),
                    })
                });
            }
        }
        d.insert("ledger_outlet".into(), json!(o.outlet_id));
    }
    d.insert("ledger".into(), Value::Object(ledger));
    // Observations by kind, first of each.
    let mut obs = Map::new();
    for o in &fs.observations {
        obs.entry(o.kind.clone()).or_insert_with(|| {
            json!({
                "count": o.count,
                "denominator": o.denominator,
                "window": duration(o.window_secs as u64),
                "subject": o.subject,
                "detail": o.detail,
                "share_pct": if o.denominator > 0 {
                    format!("{}%", (o.count as f64 * 100.0 / o.denominator as f64).round() as u64)
                } else { String::new() },
            })
        });
    }
    d.insert("obs".into(), Value::Object(obs));
    if !fs.gaps.is_empty() {
        d.insert("gaps".into(), json!(fs.gaps.join("; ")));
    }
    if let Value::Object(m) = &mut root {
        m.insert("d".into(), Value::Object(d));
    }
    root
}

fn origin_published(c: &Value) -> Option<i64> {
    let oid = get(c, "origin_item_id")?;
    get(c, "timeline")?
        .as_array()?
        .iter()
        .find(|j| get(j, "item_id") == Some(oid))
        .and_then(|j| get(j, "published_at"))
        .and_then(Value::as_i64)
}

/// The largest syndication group: its share of the cluster's items, the
/// outlet it appeared at first, and its size.
fn biggest_syndication(c: &Value) -> Option<(f64, Value, u64)> {
    let total = get(c, "item_count")?.as_u64()?;
    let groups = get(c, "syndication")?.as_array()?;
    let g = groups.iter().max_by_key(|g| {
        get(g, "item_ids")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    })?;
    let size = get(g, "item_ids")?.as_array()?.len() as u64;
    if total == 0 || size < 2 {
        return None;
    }
    Some((
        size as f64 / total as f64,
        get(g, "first_outlet_id")?.clone(),
        size,
    ))
}

fn plural(n: u64, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

fn list_of(names: &[String], max: usize) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        n if n <= max => {
            let (last, rest) = names.split_last().unwrap();
            format!("{} and {last}", rest.join(", "))
        }
        n => format!("{} and {} others", names[..max].join(", "), n - max),
    }
}

/// Percent-encode one path segment (RFC 3986 unreserved characters pass).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    match (h, m) {
        (0, 0) => format!("{secs} s"),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// `YYYY-MM-DD HH:MM UTC` from unix seconds, without a date crate.
pub fn date_time(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let rem = ts.rem_euclid(86_400);
    let (y, mo, da) = civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{da:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Unix seconds from an RFC 3339 timestamp such as `2024-01-01T09:00:00.123Z`
/// or `2024-01-01T09:00:00+02:00`. `None` if it does not parse.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_at(s.find('T').or_else(|| s.find(' '))?);
    let rest = &rest[1..];
    let mut d = date.split('-');
    let (y, mo, da): (i64, u32, u32) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let tz_at = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (time, tz) = rest.split_at(tz_at);
    let mut t = time.split(':');
    let (h, mi): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let sec: i64 = t
        .next()
        .map(|x| x.split('.').next().unwrap_or("0").parse().ok())
        .unwrap_or(Some(0))?;
    let offset = match tz {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            let mut p = tz[1..].split(':');
            let oh: i64 = p.next()?.parse().ok()?;
            let om: i64 = p.next().unwrap_or("0").parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
    };
    Some(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_round_trips() {
        assert_eq!(parse_rfc3339("2024-01-01T09:00:00Z"), Some(1_704_099_600));
        assert_eq!(
            parse_rfc3339("2024-01-01T09:00:00.512Z"),
            Some(1_704_099_600)
        );
        assert_eq!(
            parse_rfc3339("2024-01-01T11:00:00+02:00"),
            Some(1_704_099_600)
        );
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("nope"), None);
        for ts in [0, 951_782_400, 1_704_099_600, 4_102_444_800] {
            let s = date_time(ts).replace(' ', "T").replace(" UTC", "") + ":00Z";
            let s = s.replace("TUTC", "");
            assert_eq!(parse_rfc3339(&s.replace("UTC", "")), Some(ts), "{s}");
        }
    }

    #[test]
    fn dates_and_durations() {
        assert_eq!(date_time(1_704_099_600), "2024-01-01 09:00 UTC");
        assert_eq!(date_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(duration(45), "45 s");
        assert_eq!(duration(600), "10 min");
        assert_eq!(duration(7_800), "2 h 10 min");
    }

    #[test]
    fn encodes_path_segments() {
        assert_eq!(encode("at://did:plc:a/b"), "at%3A%2F%2Fdid%3Aplc%3Aa%2Fb");
        assert_eq!(encode("plain-id_1.x~"), "plain-id_1.x~");
    }

    #[test]
    fn paths_and_lists() {
        let v = json!({"a": {"b": [1, {"c": "x"}]}, "n": null});
        assert_eq!(get(&v, "a.b.1.c").map(text).as_deref(), Some("x"));
        assert!(get(&v, "n").is_none());
        assert!(get(&v, "a.z").is_none());
        let s = |v: &[&str]| list_of(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>(), 2);
        assert_eq!(s(&["A"]), "A");
        assert_eq!(s(&["A", "B"]), "A and B");
        assert_eq!(s(&["A", "B", "C"]), "A, B and 1 others");
    }
}
