//! `/search/` (plan §3.4): one page, a prebuilt index, the query runs
//! in the browser. The index is `index.json` beside the page, itself a
//! machine-readable listing of outlets and stories in the window.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::input::Input;
use crate::render::{datetime, html, n, Page};

const SCRIPT: &str = r#"<script>
const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const n = (k, w) => `${k} ${w}${k === 1 ? '' : (w.endsWith('y') ? 'ies' : 's')}`.replace(/ys$/, 'ies');
let idx = null;
async function load() { if (!idx) idx = await fetch('/search/index.json').then(r => r.json()); return idx; }
function score(q, s) { s = s.toLowerCase(); return s === q ? 3 : s.startsWith(q) ? 2 : s.includes(q) ? 1 : 0; }
async function search(q) {
  q = q.trim().toLowerCase();
  history.replaceState(null, '', q ? '?q=' + encodeURIComponent(q) : location.pathname);
  if (!q) { $('#outlets').innerHTML = ''; $('#stories').innerHTML = ''; return; }
  const d = await load();
  const outlets = d.outlets.map(o => ({o, s: Math.max(score(q, o.id), score(q, o.name), ...o.domains.map(x => score(q, x)))})).filter(x => x.s).sort((a, b) => b.s - a.s || a.o.name.localeCompare(b.o.name)).slice(0, 20);
  const stories = d.stories.map(c => ({c, s: score(q, c.title)})).filter(x => x.s).sort((a, b) => b.s - a.s || b.c.created_at - a.c.created_at).slice(0, 50);
  $('#outlets').innerHTML = (outlets.length ? '<h2>Outlets</h2><ul class="stories">' + outlets.map(({o}) => `<li><a href="/outlet/${esc(o.id)}/" data-outlet="${esc(o.id)}">${esc(o.name || o.id)}</a><span class="kg-muted">${esc(o.domains.join(', '))}</span><span class="n kg-muted">${o.claims ? n(o.claims, 'claim') : 'not assessed yet'}</span></li>`).join('') + '</ul>' : '');
  $('#stories').innerHTML = (stories.length ? '<h2>Stories</h2><ul class="stories">' + stories.map(({c}) => `<li data-id="${c.id}"><a href="/story/${c.id}/">${esc(c.title)}</a><span class="n kg-muted">${n(c.outlet_count, 'outlet')} · ${esc(c.created)}</span></li>`).join('') + '</ul>' : '');
  if (!outlets.length && !stories.length) $('#stories').innerHTML = '<div class="empty">Nothing in the index matches that.</div>';
}
let t;
$('#q').addEventListener('input', e => { clearTimeout(t); t = setTimeout(() => search(e.target.value), 120); });
const q0 = new URLSearchParams(location.search).get('q');
if (q0) { $('#q').value = q0; search(q0); }
</script>"#;

pub fn search(input: &Input, window_hours: i64) -> Twins {
    let since = input.export_time - window_hours * crate::derive::HOUR;
    let mut stories: Vec<_> = input
        .clusters
        .values()
        .filter(|c| c.last_active_at >= since && !c.timeline.is_empty())
        .collect();
    stories.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    let index = json!({
        "export_time": input.export_time,
        "window_hours": window_hours,
        "outlets": input.outlets.values().map(|o| json!({
            "id": o.id, "name": o.name, "domains": o.domains,
            "claims": input.claims.get(&o.id).map(|v| v.len()).unwrap_or(0),
        })).collect::<Vec<_>>(),
        "stories": stories.iter().map(|c| json!({
            "id": c.id, "title": c.primary_title, "outlet_count": c.outlet_count,
            "created_at": c.created_at, "created": datetime(c.created_at),
        })).collect::<Vec<_>>(),
    });
    let mut b = String::new();
    let _ = write!(
        b,
        "<h1>Search</h1><p class=\"kg-muted\">{} and {} from the {} hours to {}. Matches by name, id, domain or title; nothing is ranked beyond that.</p><input id=\"q\" class=\"kg-input wide\" type=\"search\" placeholder=\"Outlet name, id or domain; story title\" autocomplete=\"off\" autofocus><noscript><p class=\"kg-muted\">Search runs in the browser over <a href=\"/search/index.json\">index.json</a>; without scripts, read that file or the <a href=\"/\">front page</a>.</p></noscript><div id=\"outlets\"></div><div id=\"stories\"></div>{SCRIPT}",
        n(input.outlets.len(), "outlet"),
        n(stories.len(), "story"),
        window_hours,
        datetime(input.export_time)
    );
    let page_html = html(&Page {
        title: "Search",
        description: "Find an outlet by name, id or domain, or a story by title.",
        path: "/search/",
        wide: false,
        head: "",
        body: &b,
        status: &format!(
            "Updated {}",
            datetime(input.export_time)
        ),
    });
    let mut md = format!(
        "# Search\n\nThe search index is `/search/index.json`: {} with id, name, domains and claim count; {} from the {} hours to {} with id, title, outlet count and creation time. Query it however you like; the page matches by substring and ranks exact, prefix, then contains, nothing more.\n\n## Outlets\n\n",
        n(input.outlets.len(), "outlet"),
        n(stories.len(), "story"),
        window_hours,
        datetime(input.export_time)
    );
    for o in input.outlets.values() {
        let _ = writeln!(
            md,
            "- [{}](/outlet/{}/) · {}",
            input.outlet_name(&o.id),
            o.id,
            o.domains.join(", ")
        );
    }
    Twins {
        html: page_html,
        json: index,
        md,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn search_page_ships_its_index() {
        let i = fixture_input();
        let t = search(&i, 24 * 90);
        assert!(t.html.contains("id=\"q\""));
        assert!(t.html.contains("fetch('/search/index.json')"));
        assert_eq!(t.json["outlets"].as_array().unwrap().len(), 3);
        assert_eq!(t.json["stories"].as_array().unwrap().len(), 61);
        assert_eq!(t.json["stories"][0]["id"], 1060, "newest first");
        assert!(t.md.contains("- [Wire](/outlet/wire/) · wire.example"));
    }
}
