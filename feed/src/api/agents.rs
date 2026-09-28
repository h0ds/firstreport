//! What a machine reader gets (design §11): `/llms.txt` saying what
//! this server is and how to use it, `/robots.txt` allowing everything,
//! and a Markdown rendering of a cluster for clients that ask for
//! `text/markdown`. Same facts, same order as the debug page.

use std::collections::HashMap;

use crate::types::{Cluster, Item, CONTRACT_VERSION};

pub const ROBOTS_TXT: &str = "User-agent: *\nAllow: /\n";

pub fn llms_txt() -> String {
    format!(
        "# Kagami feed

> Headlines from a fixed set of news outlets, clustered into stories, with the
> record of how each story spread: which outlet published first, who joined
> when, which items are the same text syndicated rather than independently
> written, and how each item's text changed after publication. Kagami attaches
> no rating, lean or quality signal to any outlet or story, and never will.

Contract version {CONTRACT_VERSION}; object shapes are in feed/docs/data-contract.md. Times are
unix seconds UTC. Ids are unsigned 64-bit; read them as strings if your
language lacks u64. Outlet ids are shared with Kagami ledger, which holds the
sourced facts about each outlet.

## Routes

- `GET /v1/clusters?since=&min_outlets=&sort=recent|velocity_1h|outlets_6h&limit=&cursor=&include=items` - stories
- `GET /v1/clusters/{{id}}` - one story with its items. Send `Accept: text/markdown` for a Markdown rendering.
- `GET /v1/clusters/{{id}}/timeline` - who joined when
- `GET /v1/items?since=&outlet=&limit=&cursor=` and `GET /v1/items/{{id}}` - headlines
- `GET /v1/items/{{id}}/revisions` - earlier versions of an item's text
- `GET /v1/syndication?since=` - groups of items that share the same text across outlets
- `GET /v1/events?after_seq=` - the cluster event log, a cursor for followers
- `GET /v1/snapshots`, `GET /v1/snapshots/{{id}}/body` - the raw feed fetches
- `GET /v1/outlets`, `GET /v1/feeds`, `GET /v1/stats`
- `GET /debug` - the same data as a page for people

## Rules of use

- A cluster is a claim that these items are about the same story, made by a
  similarity threshold, not by a person. Treat membership as evidence.
- `origin_outlet_id` is the earliest published member as the feeds reported
  it. Feeds lie about times; `joined_at` is what Kagami saw.
- Syndication groups mean the same copy, not the same story.
- Nothing here says whether an outlet is reliable. For sourced facts about an
  outlet, ask Kagami ledger by the same outlet id.
"
    )
}

fn hhmm(ts: i64) -> String {
    let s = ts.rem_euclid(86_400);
    format!("{:02}:{:02} UTC", s / 3600, (s % 3600) / 60)
}

fn date(ts: i64) -> String {
    // civil-from-days (Howard Hinnant)
    let z = ts.div_euclid(86_400) + 719_468;
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

/// Markdown for one cluster: title, the metadata line from the cluster
/// card, then every item in join order with the first marked.
pub fn cluster_markdown(c: &Cluster, items: &[Item], names: &HashMap<String, String>) -> String {
    let name = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", c.primary_title));
    md.push_str(&format!(
        "Cluster `{}`. {} outlets · {} items · first seen {} {} · first published by {}",
        c.id,
        c.outlet_count,
        c.item_count,
        date(c.created_at),
        hhmm(c.created_at),
        name(&c.origin_outlet_id)
    ));
    if !c.syndication.is_empty() {
        md.push_str(&format!(" · {} syndication groups", c.syndication.len()));
    }
    md.push_str("\n\nMembership is by text similarity, not by a person; treat it as evidence. No outlet here carries a rating. Sourced facts about each outlet are in Kagami ledger under the same id.\n\n## Items in the order they joined\n\n");
    let by_id: HashMap<u64, &Item> = items.iter().map(|i| (i.id, i)).collect();
    for (k, j) in c.timeline.iter().enumerate() {
        let Some(it) = by_id.get(&j.item_id) else {
            continue;
        };
        let when = if it.published_at > 0 {
            format!(
                "published {} {}",
                date(it.published_at),
                hhmm(it.published_at)
            )
        } else {
            format!("picked up {} {}", date(j.joined_at), hhmm(j.joined_at))
        };
        md.push_str(&format!(
            "- {}**{}**: [{}]({}) · {}\n",
            if k == 0 { "first · " } else { "" },
            name(&it.outlet_id),
            it.title,
            it.canonical_url,
            when
        ));
    }
    if !c.syndication.is_empty() {
        md.push_str("\n## Same text at more than one outlet\n\n");
        for g in &c.syndication {
            let outlets: Vec<String> = g
                .item_ids
                .iter()
                .filter_map(|id| by_id.get(id).map(|i| name(&i.outlet_id)))
                .collect();
            md.push_str(&format!(
                "- {} · first at {} · spread over {} minutes\n",
                outlets.join(", "),
                name(&g.first_outlet_id),
                g.spread_secs / 60
            ));
        }
    }
    md.push_str(&format!(
        "\n## Velocity\n\nItems in the last hour {}, six hours {}, day {}. Outlets in the last hour {}, six hours {}, day {}.\n",
        c.velocity.items_1h,
        c.velocity.items_6h,
        c.velocity.items_24h,
        c.velocity.outlets_1h,
        c.velocity.outlets_6h,
        c.velocity.outlets_24h
    ));
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Join, SynGroup, Velocity};

    fn item(id: u64, outlet: &str, t: i64, title: &str) -> Item {
        Item {
            id,
            outlet_id: outlet.into(),
            canonical_url: format!("https://{outlet}.example/{id}"),
            title: title.into(),
            summary: String::new(),
            published_at: t,
            fetched_at: t + 60,
            lang: "en".into(),
            text_hash: id,
            cluster_id: Some(1),
            cluster_rank: Some(0),
            joined_at: Some(t + 60),
            snapshot_id: 1,
        }
    }

    #[test]
    fn markdown_lists_items_in_join_order_with_first_marked() {
        let items = vec![
            item(1, "wire", 1_704_098_400, "Bank holds rates"),
            item(2, "gazette", 1_704_100_200, "Bank holds rates"),
        ];
        let c = Cluster {
            id: 1,
            created_at: 1_704_099_600,
            last_active_at: 1_704_101_400,
            item_count: 2,
            outlet_count: 2,
            primary_title: "Bank holds rates".into(),
            origin_item_id: 1,
            origin_outlet_id: "wire".into(),
            timeline: vec![
                Join {
                    item_id: 1,
                    outlet_id: "wire".into(),
                    published_at: 1_704_098_400,
                    joined_at: 1_704_099_600,
                },
                Join {
                    item_id: 2,
                    outlet_id: "gazette".into(),
                    published_at: 1_704_100_200,
                    joined_at: 1_704_101_400,
                },
            ],
            syndication: vec![SynGroup {
                text_hash: 1,
                item_ids: vec![1, 2],
                first_outlet_id: "wire".into(),
                spread_secs: 1800,
            }],
            velocity: Velocity {
                items_24h: 2,
                outlets_24h: 2,
                ..Default::default()
            },
        };
        let names = HashMap::from([("wire".to_string(), "The Wire".to_string())]);
        let md = cluster_markdown(&c, &items, &names);
        assert!(md.starts_with("# Bank holds rates\n"));
        assert!(md.contains("first published by The Wire"));
        let first = md.find("first · **The Wire**").unwrap();
        let second = md.find("**gazette**").unwrap();
        assert!(first < second);
        assert!(md.contains("published 2024-01-01 08:40 UTC"));
        assert!(md.contains("- The Wire, gazette · first at The Wire · spread over 30 minutes"));
        assert!(md.contains("No outlet here carries a rating"));
        assert!(llms_txt().contains("Accept: text/markdown"));
    }
}
