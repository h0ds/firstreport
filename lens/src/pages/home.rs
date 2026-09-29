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
        "<h1>Stories</h1><p class=\"kg-muted\">{} active in the {} hours to {}, from {} in the pool. Ordered by outlets joining in the last day, then by recency.</p>",
        n(recent.len(), "story"),
        window_hours,
        time_el(now, now),
        n(input.outlets.len(), "outlet")
    );
    if recent.is_empty() {
        b.push_str("<div class=\"empty\">No stories in the window. Run feed for a while, export, and build again.</div>");
    }
    // Spreading now: outlets joining in the last six hours, most first.
    // Only stories with any; the list is absent when nothing is moving.
    let mut spreading: Vec<&(&crate::input::Cluster, crate::derive::ClusterFacts)> =
        recent.iter().filter(|(_, f)| f.outlets_6h > 0).collect();
    spreading.sort_by(|a, b| {
        b.1.outlets_6h
            .cmp(&a.1.outlets_6h)
            .then(b.0.last_active_at.cmp(&a.0.last_active_at))
            .then(a.0.id.cmp(&b.0.id))
    });
    spreading.truncate(10);
    // On a first day every story is "spreading now" in the same order as
    // the main list; the list earns its place only when it reorders.
    let same_as_top = spreading
        .iter()
        .zip(recent.iter())
        .all(|(a, b)| a.0.id == b.0.id);
    if same_as_top {
        spreading.clear();
    }
    if !spreading.is_empty() {
        b.push_str("<h2>Spreading now</h2><p class=\"kg-muted\">Outlets that joined in the last six hours.</p><ul class=\"stories\">");
        for (c, f) in &spreading {
            let _ = write!(
                b,
                "<li data-id=\"{}\"><a href=\"/story/{}/\">{}</a><span class=\"n kg-muted\">{} in 6 h{} · {}</span></li>",
                c.id,
                c.id,
                esc(&c.primary_title),
                n(f.outlets_6h as usize, "outlet"),
                if c.outlet_count != f.outlets_6h {
                    format!(" · {} in all", n(c.outlet_count as usize, "outlet"))
                } else {
                    String::new()
                },
                crate::render::origin_link(input, &c.origin_outlet_id)
            );
        }
        b.push_str("</ul><h2>All stories in the window</h2>");
    }
    let scale = crate::render::spark_scale(recent.iter().map(|(_, f)| f.spark.as_slice()));
    for (c, f) in &recent {
        b.push_str(&cluster_card(input, c, &f.spark, scale, false, false));
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

    let head = format!(
        "<script type=\"application/ld+json\">{}</script>\n",
        json!({
            "@context": "https://schema.org",
            "@type": "ItemList",
            "name": "Stories",
            "description": "Stories forming across news outlets, ordered by outlets joining in the last day. No outlet is rated.",
            "numberOfItems": recent.len(),
            "itemListElement": recent.iter().enumerate().map(|(k, (c, _))| json!({
                "@type": "ListItem",
                "position": k + 1,
                "url": format!("/story/{}/", c.id),
                "name": c.primary_title,
            })).collect::<Vec<_>>()
        })
    );
    let page_html = html(&Page {
        title: "Stories",
        description: "Stories forming across news outlets: who published first, who joined when, what was copied. No outlet is rated.",
        path: "/",
        wide: false,
        head: &head,
        body: &b,
        status: &format!("built from an export at {}", datetime(now)),
    });
    let page_json = json!({
        "window_hours": window_hours,
        "export_time": now,
        "spreading_now": spreading.iter().map(|(c, f)| json!({"id": c.id, "title": c.primary_title, "outlets_6h": f.outlets_6h})).collect::<Vec<_>>(),
        "stories": recent.iter().map(|(c, f)| json!({
            "id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count, "item_count": c.item_count,
            "origin_outlet_id": c.origin_outlet_id, "created_at": c.created_at, "last_active_at": c.last_active_at,
            "outlets_24h": f.outlets_24h, "outlets_6h": f.outlets_6h,
        })).collect::<Vec<_>>(),
        "outlets": input.outlets.values().collect::<Vec<_>>(),
    });
    let mut md = format!(
        "# Stories\n\n{} active in the {} hours to {}, from {} in the pool. Ordered by outlets joining in the last day, then by recency; no outlet is rated.\n\n",
        n(recent.len(), "story"),
        window_hours,
        datetime(now),
        n(input.outlets.len(), "outlet")
    );
    if !spreading.is_empty() {
        md.push_str("## Spreading now\n\nOutlets that joined in the last six hours.\n\n");
        for (c, f) in &spreading {
            let _ = writeln!(
                md,
                "- [{}](/story/{}/) · {} in 6 h{} · first: {}",
                c.primary_title,
                c.id,
                n(f.outlets_6h as usize, "outlet"),
                if c.outlet_count != f.outlets_6h {
                    format!(" · {} in all", n(c.outlet_count as usize, "outlet"))
                } else {
                    String::new()
                },
                input.outlet_name(&c.origin_outlet_id)
            );
        }
        md.push_str("\n## All stories in the window\n\n");
    }
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
        assert!(all.html.contains("\"@type\":\"ItemList\""));
        assert!(!all.html.contains("Rating"));
        // the export's last two hours hold the newest story only
        // Same order as the main list: the spreading list is withheld.
        let t = home(&i, 6);
        assert!(!t.html.contains("<h2>Spreading now</h2>"));
        assert!(t.json["spreading_now"].as_array().unwrap().is_empty());
        assert!(!t.html.contains("No other ranking"));
        // A story moving now but with fewer outlets over the day than
        // another: the lists differ, so the spreading list appears.
        let mut i = fixture_input();
        let now = i.export_time;
        for j in &mut i.clusters.get_mut(&1059).unwrap().timeline {
            j.joined_at = now - 12 * HOUR;
        }
        i.clusters.get_mut(&1059).unwrap().last_active_at = now - 12 * HOUR;
        for j in &mut i.clusters.get_mut(&1060).unwrap().timeline {
            j.joined_at = now - HOUR;
        }
        let t = home(&i, 24);
        assert!(t.html.contains("<h2>Spreading now</h2>"), "{}", t.html);
        assert_eq!(t.json["spreading_now"][0]["id"], 1060);
        assert_eq!(t.json["stories"][0]["id"], 1059);
        assert!(
            t.html.contains("1 outlet in 6 h · <span class=\"kg-origin-name\">"),
            "equal counts shown once"
        );
        assert!(t.md.contains("## Spreading now"));
    }
}
