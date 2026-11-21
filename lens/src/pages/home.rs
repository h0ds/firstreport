//! `/` (plan §3.1). The N1 form: stories from the window as cluster
//! cards ordered by reach (docs/algorithms.md §4) then recency, and the
//! outlet list. The "spreading now" list and the search page are N2.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::derive::{cluster_facts, HOUR};
use crate::input::Input;
use crate::render::{cluster_card, datetime, esc, html, n, Page};

/// Stories per page of the story list; page 1 is `/`, then `/page/N/`.
pub const PAGE_SIZE: usize = 50;

type Ranked<'a> = Vec<(&'a crate::input::Cluster, crate::derive::ClusterFacts)>;

/// The window's stories in list order: ranked by reach, then single
/// reports newest first (docs/algorithms.md §4).
fn ranked(input: &Input, window_hours: i64) -> (Ranked<'_>, Ranked<'_>) {
    let now = input.export_time;
    let since = now - window_hours * HOUR;
    let mut recent: Vec<_> = input
        .clusters
        .values()
        .filter(|c| c.last_active_at >= since && !c.timeline.is_empty())
        .map(|c| {
            (
                c,
                cluster_facts(c, &input.items, &input.events, now, window_hours * HOUR),
            )
        })
        .collect();
    // Reach order (docs/algorithms.md §4): independent reports plus
    // languages minus one, then outlets in six hours, then recency. A
    // story with at most one independent report is listed apart, in
    // recency order, so single reports are shown but not mixed in.
    recent.sort_by(|a, b| {
        b.1.reach
            .cmp(&a.1.reach)
            .then(b.1.outlets_6h.cmp(&a.1.outlets_6h))
            .then(b.0.last_active_at.cmp(&a.0.last_active_at))
            .then(a.0.id.cmp(&b.0.id))
    });
    let (recent, single): (Vec<_>, Vec<_>) = recent.into_iter().partition(|(_, f)| f.reports > 1);
    let mut single = single;
    single.sort_by(|a, b| {
        b.0.last_active_at
            .cmp(&a.0.last_active_at)
            .then(a.0.id.cmp(&b.0.id))
    });
    (recent, single)
}

/// Number of pages the story list takes.
pub fn page_count(input: &Input, window_hours: i64) -> usize {
    let (r, s) = ranked(input, window_hours);
    (r.len() + s.len()).div_ceil(PAGE_SIZE).max(1)
}

fn page_path(page: usize) -> String {
    if page <= 1 {
        "/".into()
    } else {
        format!("/page/{page}/")
    }
}

/// "last 24 hours", or "last 7 days" for whole days past two.
fn window_label(hours: i64) -> String {
    if hours > 48 && hours % 24 == 0 {
        format!("last {} days", hours / 24)
    } else {
        format!("last {hours} hours")
    }
}

/// The story list's pager: the shared one, 50 to a page.
fn pager(page: usize, pages: usize, total: usize) -> String {
    crate::render::pager(page, pages, total, PAGE_SIZE, "Story pages", &page_path)
}

/// heading where that part of the list begins (or continues).
fn page_cards(input: &Input, recent: &Ranked<'_>, single: &Ranked<'_>, page: usize) -> String {
    let scale = crate::render::spark_scale(
        recent
            .iter()
            .chain(single.iter())
            .map(|(_, f)| f.spark.as_slice()),
    );
    let start = (page.max(1) - 1) * PAGE_SIZE;
    // The card's pictures, explained once rather than on every card.
    let mut b = String::from("<p class=\"kg-muted kg-legend\"><span class=\"key spark\"></span>outlets carrying the story over its first day <span class=\"sep\">·</span> <span class=\"key lean\"><i class=\"k1\"></i><i class=\"k3\"></i><i class=\"k5\"></i></span>where each outlet's audience leans, left to right, from US sharing data (<a class=\"kg-link-quiet\" href=\"https://dataverse.harvard.edu/dataset.xhtml?persistentId=doi:10.7910/DVN/QAN5VX\" rel=\"noopener\">Robertson et al. 2018</a>); hollow dots are outlets it does not score</p>");
    for (k, (c, f)) in recent
        .iter()
        .chain(single.iter())
        .enumerate()
        .skip(start)
        .take(PAGE_SIZE)
    {
        if k >= recent.len() && (k == recent.len() || k == start) {
            let _ = write!(
                b,
                "<h2>Single reports{}</h2><p class=\"kg-muted\">{} told by one outlet so far, or only as copies of one text. Newest first.</p>",
                if k == recent.len() { "" } else { ", continued" },
                n(single.len(), "story")
            );
        }
        b.push_str(&cluster_card(
            input,
            c,
            &f.spark,
            scale,
            false,
            false,
            Some(f),
        ));
    }
    b
}

/// `/outlets/`: every outlet in the pool with its domains and how much
/// ledger knows about it. Its own page, so the front page is stories.
pub fn outlets_page(input: &Input) -> Twins {
    let now = input.export_time;
    let mut b = format!(
        "<h1>Outlets</h1><p class=\"kg-muted\">{} in the pool. Each outlet's page has what First Report ledger records about it, with sources.</p><ul class=\"stories\">",
        n(input.outlets.len(), "outlet")
    );
    let mut md = format!(
        "# Outlets\n\n{} in the pool.\n\n",
        n(input.outlets.len(), "outlet")
    );
    for o in input.outlets.values() {
        let claims = input.claims.get(&o.id).map(|v| v.len()).unwrap_or(0);
        let _ = write!(
            b,
            "<li>{}<span class=\"kg-muted\">{}</span><span class=\"n kg-muted\">{}</span></li>",
            crate::render::outlet_badge(input, &o.id),
            esc(&o.domains.join(", ")),
            if claims == 0 {
                "not assessed yet".to_string()
            } else {
                n(claims, "fact")
            }
        );
        let _ = writeln!(
            md,
            "- [{}](/outlet/{}/) · {}",
            input.outlet_name(&o.id),
            o.id,
            o.domains.join(", ")
        );
    }
    b.push_str("</ul>");
    let html_page = html(&Page {
        title: "Outlets",
        description:
            "Every news outlet in First Report's pool, with its domains. No outlet is rated.",
        path: "/outlets/",
        wide: false,
        head: "",
        body: &b,
        status: &format!("Updated {}", datetime(now)),
    });
    let json = json!({"export_time": now, "outlets": input.outlets.values().collect::<Vec<_>>()});
    Twins {
        html: html_page,
        json,
        md,
    }
}

/// Page `page` (2 or more) of the story list, at `/page/N/`. Its twins
/// carry that page's stories; the full list stays at `/index.json`.
pub fn story_page(input: &Input, window_hours: i64, page: usize) -> Twins {
    let now = input.export_time;
    let (recent, single) = ranked(input, window_hours);
    let pages = page_count(input, window_hours);
    let path = page_path(page);
    let mut b = format!(
        "<h1>Stories</h1><p class=\"kg-muted\">Page {page} of {pages} · {} stories · {} · ranked by <a class=\"kg-link-quiet\" href=\"/about/#reach\">reach</a></p>",
        crate::render::thousands(recent.len() + single.len()),
        window_label(window_hours)
    );
    b.push_str(&pager(page, pages, recent.len() + single.len()));
    b.push_str(&page_cards(input, &recent, &single, page));
    b.push_str(&pager(page, pages, recent.len() + single.len()));
    let ids: Vec<_> = recent
        .iter()
        .chain(single.iter())
        .skip((page - 1) * PAGE_SIZE)
        .take(PAGE_SIZE)
        .collect();
    let title = format!("Stories, page {page}");
    let html_page = html(&Page {
        title: &title,
        description: "Stories forming across news outlets, ordered by reach. No outlet is rated.",
        path: &path,
        wide: false,
        head: "",
        body: &b,
        status: &format!("Updated {}", datetime(now)),
    });
    let json = json!({
        "page": page, "pages": pages, "page_size": PAGE_SIZE, "export_time": now,
        "stories": ids.iter().map(|(c, f)| json!({"id": c.id, "url": input.story_url(c), "title": c.primary_title, "outlet_count": c.outlet_count, "reach": f.reach, "reports": f.reports})).collect::<Vec<_>>(),
        "full_list": "/index.json",
    });
    let mut md = format!("# Stories, page {page} of {pages}\n\nOrdered by reach then recency; the full list is /index.md.\n\n");
    for (c, f) in &ids {
        let _ = writeln!(
            md,
            "- [{}]({}) · {} · reach {}",
            c.primary_title,
            input.story_url(c),
            n(c.outlet_count as usize, "outlet"),
            f.reach
        );
    }
    Twins {
        html: html_page,
        json,
        md,
    }
}

pub fn home(input: &Input, window_hours: i64) -> Twins {
    let now = input.export_time;
    let (recent, single) = ranked(input, window_hours);
    let pages = page_count(input, window_hours);
    let active = recent.len() + single.len();

    let mut b = String::new();
    let _ = write!(
        b,
        "<h1>Stories</h1><p class=\"kg-muted\">{} stories from <a class=\"kg-link-quiet\" href=\"/outlets/\">{} outlets</a> · {} · ranked by <a class=\"kg-link-quiet\" href=\"/about/#reach\">reach</a></p>",
        crate::render::thousands(active),
        input.outlets.len(),
        window_label(window_hours)
    );
    if active == 0 {
        b.push_str("<div class=\"empty\">No stories in the window. Run feed for a while, export, and build again.</div>");
    }
    // Spreading now: outlets joining in the last six hours, most first.
    // Only stories with any; the list is absent when nothing is moving.
    // Drawn from every active story, single reports included: what is
    // moving now is a different question from what has reached far.
    let mut spreading: Vec<&(&crate::input::Cluster, crate::derive::ClusterFacts)> = recent
        .iter()
        .chain(single.iter())
        .filter(|(_, f)| f.outlets_6h > 0)
        .collect();
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
        .zip(recent.iter().chain(single.iter()))
        .all(|(a, b)| a.0.id == b.0.id);
    if same_as_top {
        spreading.clear();
    }
    if !spreading.is_empty() {
        b.push_str("<section class=\"panel\"><h2>Spreading now</h2><p class=\"kg-muted\">Outlets that joined in the last six hours.</p><ul class=\"stories\">");
        for (c, f) in &spreading {
            let _ = write!(
                b,
                // Headline, then its chip and first outlet on the line below.
                "<li class=\"spread\" data-id=\"{}\"><a {}>{}</a><span class=\"n kg-muted\"><span class=\"kg-count\">{} in 6 h</span><span class=\"sep\">·</span>{}</span></li>",
                c.id,
                input.story_href(c),
                esc(&c.primary_title),
                n(f.outlets_6h as usize, "outlet"),
                crate::render::origin_link(input, &c.origin_outlet_id)
            );
        }
        b.push_str("</ul></section><h2>All stories in the window</h2>");
    }
    b.push_str(&pager(1, pages, active));
    b.push_str(&page_cards(input, &recent, &single, 1));
    b.push_str(&pager(1, pages, active));
    let head = format!(
        "<script type=\"application/ld+json\">{}</script>\n",
        json!({
            "@context": "https://schema.org",
            "@type": "ItemList",
            "name": "Stories",
            "description": "Stories forming across news outlets, ordered by reach: independent reports in the window plus languages. No outlet is rated.",
            "numberOfItems": active,
            // Only the stories on this page: listing the whole export made
            // the front page several hundred kilobytes (`docs/deploy.md` §4.2).
            "itemListElement": recent.iter().chain(single.iter()).take(PAGE_SIZE).enumerate().map(|(k, (c, _))| json!({
                "@type": "ListItem",
                "position": k + 1,
                "url": input.story_url(c),
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
        status: &format!("Updated {}", datetime(now)),
    });
    let page_json = json!({
        "window_hours": window_hours,
        "export_time": now,
        "order": "reach = reports + languages - 1 over reach_window_secs, then outlets_6h, then last_active_at; stories with reports <= 1 are listed under single_reports by last_active_at (docs/algorithms.md §4)",
        "spreading_now": spreading.iter().map(|(c, f)| json!({"id": c.id, "title": c.primary_title, "outlets_6h": f.outlets_6h})).collect::<Vec<_>>(),
        "stories": recent.iter().map(|(c, f)| json!({
            "id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count, "item_count": c.item_count,
            "origin_outlet_id": c.origin_outlet_id, "created_at": c.created_at, "last_active_at": c.last_active_at,
            "outlets_24h": f.outlets_24h, "outlets_6h": f.outlets_6h,
            "reach_window_secs": f.reach_window_secs, "reports": f.reports, "languages": f.languages, "reach": f.reach,
        })).collect::<Vec<_>>(),
        "single_reports": single.iter().map(|(c, f)| json!({
            "id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count, "item_count": c.item_count,
            "origin_outlet_id": c.origin_outlet_id, "created_at": c.created_at, "last_active_at": c.last_active_at,
            "outlets_24h": f.outlets_24h, "outlets_6h": f.outlets_6h,
            "reach_window_secs": f.reach_window_secs, "reports": f.reports, "languages": f.languages, "reach": f.reach,
        })).collect::<Vec<_>>(),
        "outlets": input.outlets.values().collect::<Vec<_>>(),
    });
    let mut md = format!(
        "# Stories\n\n{} active in the {} hours to {}, from {} in the pool. Ordered by reach: independent reports in the window plus languages, then recency (see /about/#reach); no outlet is rated.\n\n",
        n(active, "story"),
        window_hours,
        datetime(now),
        n(input.outlets.len(), "outlet")
    );
    if !spreading.is_empty() {
        md.push_str("## Spreading now\n\nOutlets that joined in the last six hours.\n\n");
        for (c, f) in &spreading {
            let _ = writeln!(
                md,
                "- [{}]({}) · {} in 6 h{} · first: {}",
                c.primary_title,
                input.story_url(c),
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
    let md_line = |md: &mut String, c: &crate::input::Cluster, f: &crate::derive::ClusterFacts| {
        let _ = writeln!(
            md,
            "- [{}]({}) · {} · {} · first: {} · reach {}: {}, {}",
            c.primary_title,
            input.story_url(c),
            n(c.outlet_count as usize, "outlet"),
            n(c.item_count as usize, "item"),
            input.outlet_name(&c.origin_outlet_id),
            f.reach,
            n(f.reports as usize, "independent report"),
            n(f.languages as usize, "language")
        );
    };
    for (c, f) in &recent {
        md_line(&mut md, c, f);
    }
    if !single.is_empty() {
        md.push_str("\n## Single reports\n\nTold by one outlet so far, or only as copies of one text. Newest first.\n\n");
        for (c, f) in &single {
            md_line(&mut md, c, f);
        }
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
            t.html.contains("href=\"/outlets/\""),
            "the front page links the outlet list"
        );
        assert!(
            !t.html.contains("<h2>Outlets</h2>"),
            "the list is its own page"
        );
        let o = outlets_page(&i);
        assert!(
            o.html.contains("/outlet/blog/"),
            "unassessed outlets are listed"
        );
        assert!(o.html.contains("not assessed yet"));
        let all = home(&i, 24 * 90);
        let stories = all.json["stories"].as_array().unwrap();
        let single = all.json["single_reports"].as_array().unwrap();
        assert_eq!(stories.len() + single.len(), 61);
        // Reach order (docs/algorithms.md §4): the counts are on every
        // entry, the top has the most, and single reports are apart.
        let reach = |v: &serde_json::Value| v["reach"].as_u64().unwrap();
        assert!(stories.windows(2).all(|w| reach(&w[0]) >= reach(&w[1])));
        assert!(stories.iter().all(|s| s["reports"].as_u64().unwrap() > 1));
        assert!(single.iter().all(|s| s["reports"].as_u64().unwrap() <= 1));
        assert_eq!(stories[0]["reach_window_secs"], 90 * 24 * HOUR);
        assert!(all.html.contains("<h2>Single reports</h2>"));
        assert!(all.html.contains("data-reach=\""));
        assert!(all.md.contains("## Single reports"));
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
        // Two independent reports: the gazette's item is its own text.
        i.clusters.get_mut(&1059).unwrap().syndication.clear();
        for j in &mut i.clusters.get_mut(&1060).unwrap().timeline {
            j.joined_at = now - HOUR;
        }
        let t = home(&i, 24);
        assert!(t.html.contains("<h2>Spreading now</h2>"), "{}", t.html);
        assert_eq!(t.json["spreading_now"][0]["id"], 1060);
        assert_eq!(t.json["stories"][0]["id"], 1059);
        assert_eq!(t.json["stories"][0]["reach"], 2);
        assert_eq!(t.json["single_reports"][0]["id"], 1060);
        assert_eq!(t.json["single_reports"][0]["reports"], 1);
        assert!(
            t.html
                .contains("<span class=\"kg-count\">1 outlet in 6 h</span><span class=\"sep\">·</span><span class=\"kg-outlet\"><span class=\"kg-icon kg-letter kg-origin\""),
            "equal counts shown once"
        );
        assert!(t.md.contains("## Spreading now"));
    }

    #[test]
    fn thousands_separators() {
        use crate::render::thousands;
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(2219), "2,219");
        assert_eq!(thousands(1234567), "1,234,567");
    }

    #[test]
    fn story_list_is_paged_fifty_to_a_page() {
        let i = fixture_input();
        let pages = page_count(&i, 24 * 60);
        assert!(pages >= 2, "fixture has more than one page: {pages}");
        let home_page = home(&i, 24 * 60);
        assert_eq!(
            home_page.html.matches("class=\"kg-cluster").count(),
            PAGE_SIZE
        );
        assert!(home_page
            .html
            .contains("<a class=\"step\" rel=\"next\" href=\"/page/2/\">Older →</a>"));
        assert!(home_page
            .html
            .contains(&format!("<span class=\"range\">1–{PAGE_SIZE} of ")));
        let last = story_page(&i, 24 * 60, pages);
        assert!(last
            .html
            .contains("<span class=\"step off\">Older →</span>"));
        assert!(last
            .html
            .contains(&format!("aria-current=\"page\">{pages}</span>")));
        assert_eq!(last.json["page"], pages);
        // The machine twin of page 1 keeps the whole list.
        let all = home_page.json["stories"].as_array().unwrap().len()
            + home_page.json["single_reports"].as_array().unwrap().len();
        assert!(all > PAGE_SIZE);
    }
}
