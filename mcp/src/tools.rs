//! The tools, as data: each is one read of one route. `plan` turns a
//! tool name and its arguments into that read, and refuses anything
//! that would reach a route the tool does not name.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Feed,
    Ledger,
}

/// One GET against one of the two servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub service: Service,
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Ask for the Markdown twin, which keeps every source and date.
    pub markdown: bool,
}

fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
        "annotations": {"readOnlyHint": true, "openWorldHint": false},
    })
}

pub fn list() -> Vec<Value> {
    let since = json!({"type": "string", "description": "Only records active since this time: RFC 3339, or unix seconds."});
    vec![
        tool(
            "lookup",
            "Look up a URL or a passage",
            "Everything on record about one article URL and/or one piece of text: the item, the story it belongs to, whether it was the first report, near-copies of the text and where the earliest appeared. `matched` says how far the lookup got and `checked` says what was searched, so a miss can be reported as a miss. Start here when you have a link or a quote.",
            json!({
                "url": {"type": "string", "description": "An article URL. Tracking parameters, www., AMP and mobile forms all find the same item."},
                "text": {"type": "string", "description": "A headline or passage to find near-copies of."},
                "since": since,
            }),
            &[],
        ),
        tool(
            "stories",
            "List stories",
            "Stories (clusters of reports about the same event), with how many outlets carried each and which outlet was first. Sort by `recent`, or by how fast outlets are joining (`velocity_1h`, `outlets_6h`).",
            json!({
                "min_outlets": {"type": "integer", "minimum": 1, "description": "Only stories at least this many outlets carried."},
                "sort": {"type": "string", "enum": ["recent", "velocity_1h", "outlets_6h"]},
                "since": since,
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20."},
                "cursor": {"type": "string", "description": "`next` from the previous page (sort `recent` only)."},
            }),
            &[],
        ),
        tool(
            "story",
            "One story",
            "One story in full: every report in the order outlets joined, with times, headlines, which texts are copies of which, and how confident the grouping is.",
            json!({"id": {"type": "string", "description": "Story (cluster) id, decimal or 0x-prefixed hex."}}),
            &["id"],
        ),
        tool(
            "item_revisions",
            "Revisions of one report",
            "How one report's headline and summary changed after publication, oldest first, and whether it was withdrawn.",
            json!({"id": {"type": "string", "description": "Item id, decimal or 0x-prefixed hex."}}),
            &["id"],
        ),
        tool(
            "outlet",
            "Facts about an outlet",
            "Sourced facts about one outlet: ownership, country, founding, third-party ratings, observed publishing behaviour. Each fact names its source, licence and retrieval date; sources that disagree are listed side by side. This system adds no rating of its own.",
            json!({"id": {"type": "string", "description": "Outlet id, e.g. `bbc`. Find one with `find_outlet`."}}),
            &["id"],
        ),
        tool(
            "find_outlet",
            "Find an outlet",
            "Find outlets by name, id, domain or article URL.",
            json!({"q": {"type": "string", "description": "A name, an id, a domain or a URL."}}),
            &["q"],
        ),
        tool(
            "sources",
            "Sources of outlet facts",
            "Every source the outlet facts come from, with its licence and required attribution.",
            json!({}),
            &[],
        ),
    ]
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// A string or a number, as a string: models send ids both ways.
fn scalar(args: &Value, key: &str) -> Option<String> {
    match args.get(key)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A numeric id for a path: digits, or `0x` and hex digits.
fn numeric_id(args: &Value) -> Result<String, String> {
    let id = scalar(args, "id").ok_or("`id` is required")?;
    let ok = match id.strip_prefix("0x") {
        Some(hex) => !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()),
        None => id.chars().all(|c| c.is_ascii_digit()),
    };
    if ok {
        Ok(id)
    } else {
        Err(format!(
            "`id` must be decimal or 0x-prefixed hex, not {id:?}"
        ))
    }
}

pub fn plan(name: &str, args: &Value) -> Result<Call, String> {
    let mut query: Vec<(String, String)> = Vec::new();
    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            query.push((k.to_string(), v));
        }
    };
    let (service, path, markdown) = match name {
        "lookup" => {
            if text(args, "url").is_none() && text(args, "text").is_none() {
                return Err("give `url`, `text`, or both".into());
            }
            put("url", text(args, "url").map(String::from));
            put("text", text(args, "text").map(String::from));
            put("since", scalar(args, "since"));
            (Service::Feed, "/v1/lookup".to_string(), false)
        }
        "stories" => {
            put("min_outlets", scalar(args, "min_outlets"));
            put("sort", text(args, "sort").map(String::from));
            put("since", scalar(args, "since"));
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100);
            put("limit", Some(limit.to_string()));
            put("cursor", text(args, "cursor").map(String::from));
            (Service::Feed, "/v1/clusters".to_string(), false)
        }
        "story" => (
            Service::Feed,
            format!("/v1/clusters/{}", numeric_id(args)?),
            true,
        ),
        "item_revisions" => (
            Service::Feed,
            format!("/v1/items/{}/revisions", numeric_id(args)?),
            false,
        ),
        "outlet" => {
            let id = text(args, "id").ok_or("`id` is required")?;
            if !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                return Err(format!(
                    "{id:?} is not an outlet id; find one with `find_outlet`"
                ));
            }
            (Service::Ledger, format!("/v1/outlets/{id}"), true)
        }
        "find_outlet" => {
            put(
                "q",
                Some(text(args, "q").ok_or("`q` is required")?.to_string()),
            );
            (Service::Ledger, "/v1/search".to_string(), false)
        }
        "sources" => (Service::Ledger, "/v1/sources".to_string(), false),
        other => return Err(format!("unknown tool {other:?}")),
    };
    Ok(Call {
        service,
        path,
        query,
        markdown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_tool_plans() {
        for t in list() {
            let name = t["name"].as_str().unwrap();
            let args = json!({"id": "12", "q": "bbc", "url": "https://example.org/"});
            let c = plan(name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(c.path.starts_with("/v1/"), "{name}");
        }
    }

    #[test]
    fn ids_cannot_leave_their_route() {
        assert_eq!(
            plan("story", &json!({"id": 1064})).unwrap().path,
            "/v1/clusters/1064"
        );
        assert_eq!(
            plan("story", &json!({"id": "0xCAF0"})).unwrap().path,
            "/v1/clusters/0xCAF0"
        );
        for bad in ["1/../../debug", "0x", "", "12?x=1", "abc"] {
            assert!(plan("story", &json!({"id": bad})).is_err(), "{bad}");
            assert!(
                plan("item_revisions", &json!({"id": bad})).is_err(),
                "{bad}"
            );
        }
        for bad in ["a/b", "..", "bbc?x", "bbc/owner"] {
            assert!(plan("outlet", &json!({"id": bad})).is_err(), "{bad}");
        }
    }

    #[test]
    fn stories_limit_is_bounded() {
        let q = |args| plan("stories", &args).unwrap().query;
        assert!(q(json!({})).contains(&("limit".into(), "20".into())));
        assert!(q(json!({"limit": 5000})).contains(&("limit".into(), "100".into())));
        let got = q(json!({"min_outlets": 3, "sort": "outlets_6h"}));
        assert!(got.contains(&("min_outlets".into(), "3".into())));
        assert!(got.contains(&("sort".into(), "outlets_6h".into())));
    }
}
