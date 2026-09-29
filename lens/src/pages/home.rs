//! `/` (plan §3.1). The N1 form: stories from the window as cluster
//! cards ordered by outlets in the last 24 hours then recency, and the
//! outlet list. The "spreading now" list and the search page are N2.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::derive::{cluster_facts, HOUR};
use crate::input::Input;
use crate::render::{cluster_card, datetime, esc, html, n, outlet_link, time_el, Page};

pub fn home(input: &Input, window_hours: i64) -> Twins {
    let now = input.export_time;
    let since = now - window_hours * HOUR;
    let mut recent: Vec<_> = input
        .clusters
        .values()
        .filter(|c| c.last_active_at >= since && !c.timeline.is_empty())
        .map(|c| (c, cluster_facts(c, &input.events, now)))
        .collect();
    recent.sort_by(|a, b| {
        b.1.outlets_24h
            .cmp(&a.1.outlets_24h)
            .then(b.0.last_active_at.cmp(&a.0.last_active_at))
            .then(a.0.id.cmp(&b.0.id))
    });

    let mut b = String::new();
    let _ = write!(
        b,
        "<h1>Stories</h1><p class=\"kg-muted\">{} active in the {} hours to {}, from {} in the pool. Ordered by outlets joining in the last day, then by recency. No other ranking.</p>",
        n(recent.len(), "story"),
        window_hours,
        time_el(now),
        n(input.outlets.len(), "outlet")
    );
    if recent.is_empty() {
        b.push_str("<div class=\"empty\">No stories in the window. Run feed for a while, export, and build again.</div>");
    }
    for (c, f) in &recent {
        b.push_str(&cluster_card(input, c, &f.spark, false));
    }
    b.push_str("<h2>Outlets</h2><ul class=\"stories\">");
    for o in input.outlets.values() {
        let claims = input.claims.get(&o.id).map(|v| v.len()).unwrap_or(0);
        let _ = write!(
            b,
            "<li>{}<span class=\"kg-muted\">{}</span><span class=\"n kg-muted\">{}</span></li>",
            outlet_link(input, &o.id),
            esc(&o.domains.join(", ")),
            if claims == 0 {
                "not assessed yet".to_string()
            } else {
                n(claims, "claim")
            }
        );
    }
    b.push_str("</ul>");

    let page_html = html(&Page {
        title: "Stories",
        description: "Stories forming across news outlets: who published first, who joined when, what was copied. No outlet is rated.",
        path: "/",
        wide: false,
        head: "",
        body: &b,
        status: &format!("built from an export at {}", datetime(now)),
    });
    let page_json = json!({
        "window_hours": window_hours,
        "export_time": now,
        "stories": recent.iter().map(|(c, f)| json!({
            "id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count, "item_count": c.item_count,
            "origin_outlet_id": c.origin_outlet_id, "created_at": c.created_at, "last_active_at": c.last_active_at,
            "outlets_24h": f.outlets_24h, "outlets_6h": f.outlets_6h,
        })).collect::<Vec<_>>(),
        "outlets": input.outlets.values().collect::<Vec<_>>(),
    });
    let mut md = format!(
        "# Stories\n\n{} active in the {} hours to {}, from {} in the pool. Ordered by outlets joining in the last day, then by recency. No other ranking; no outlet is rated.\n\n",
        n(recent.len(), "story"),
        window_hours,
        datetime(now),
        n(input.outlets.len(), "outlet")
    );
    for (c, f) in &recent {
        let _ = writeln!(
            md,
            "- [{}](/story/{}/) · {} · {} · first: {} · {} in the last day",
            c.primary_title,
            c.id,
            n(c.outlet_count as usize, "outlet"),
            n(c.item_count as usize, "item"),
            input.outlet_name(&c.origin_outlet_id),
            n(f.outlets_24h as usize, "outlet")
        );
    }
    md.push_str("\n## Outlets\n\n");
    for o in input.outlets.values() {
        let _ = writeln!(md, "- [{}](/outlet/{}/)", input.outlet_name(&o.id), o.id);
    }
    Twins {
        html: page_html,
        json: page_json,
        md,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn home_lists_recent_stories_and_every_outlet() {
        let i = fixture_input();
        let t = home(&i, 24);
        assert!(t.html.contains("<h1>Stories</h1>"));
        assert!(
            t.html.contains("/outlet/blog/"),
            "unassessed outlets are listed"
        );
        assert!(t.html.contains("not assessed yet"));
        let all = home(&i, 24 * 90);
        assert_eq!(all.json["stories"].as_array().unwrap().len(), 61);
        let first = &all.json["stories"][0];
        assert!(
            first["outlets_24h"].as_u64().unwrap()
                >= all.json["stories"][60]["outlets_24h"].as_u64().unwrap()
        );
        assert!(all.md.contains("- [Story 1001](/story/1001/)"));
    }
}
