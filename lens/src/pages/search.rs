//! `/search/` (plan §3.4): one page, a prebuilt index, the query runs
//! in the browser. The index is `index.json` beside the page, itself a
//! machine-readable listing of outlets and stories in the window.

use std::fmt::Write as _;

use serde_json::json;

use super::Twins;
use crate::input::Input;
use std::collections::HashMap;

use crate::render::{datetime, esc, html, n, Page};

/// `/search.js`: a file of its own, so the site's Content-Security-Policy
/// can forbid inline scripts (`docs/deploy.md` §4.2).
pub const SCRIPT: &str = r#"const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const n = (k, w) => `${k.toLocaleString('en')} ${k === 1 ? w : w.endsWith('y') ? w.slice(0, -1) + 'ies' : w + 's'}`;
let idx = null;
async function load() { if (!idx) idx = await fetch('/search/index.json').then(r => r.json()); return idx; }
const words = q => q.toLowerCase().split(/\s+/).filter(Boolean);
// Every word must appear. A whole word counts most, then the start of a
// word, then the inside of one; an exact match outranks all.
function score(ws, text) {
  const t = String(text ?? '').toLowerCase();
  let s = 0;
  for (const w of ws) {
    let best = 0;
    for (let i = t.indexOf(w); i >= 0; i = t.indexOf(w, i + 1)) {
      const a = i === 0 || /[^\p{L}\p{N}]/u.test(t[i - 1]);
      const b = i + w.length === t.length || /[^\p{L}\p{N}]/u.test(t[i + w.length]);
      best = Math.max(best, a && b ? 4 : a ? 2 : 1);
    }
    if (!best) return 0;
    s += best;
  }
  return t === ws.join(' ') ? s + 10 : s;
}
function mark(text, ws) {
  const re = new RegExp('(' + ws.map(w => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|') + ')', 'gi');
  return String(text ?? '').split(re).map((p, i) => i % 2 ? '<mark>' + esc(p) + '</mark>' : esc(p)).join('');
}
async function search(raw) {
  const q = raw.trim();
  const ws = words(q);
  history.replaceState(null, '', q ? '?q=' + encodeURIComponent(q) : location.pathname);
  $('#start').hidden = !!q;
  if (!ws.length) { $('#summary').textContent = ''; $('#outlets').innerHTML = ''; $('#stories').innerHTML = ''; return; }
  const d = await load();
  const outlets = d.outlets.map(o => ({o, s: Math.max(score(ws, o.id), score(ws, o.name), score(ws, o.domains.join(' ')))})).filter(x => x.s).sort((a, b) => b.s - a.s || a.o.name.localeCompare(b.o.name));
  const stories = d.stories.map(c => ({c, s: score(ws, c.title)})).filter(x => x.s).sort((a, b) => b.s - a.s || b.c.outlet_count - a.c.outlet_count || b.c.created_at - a.c.created_at);
  $('#summary').innerHTML = outlets.length || stories.length
    ? `${[stories.length && n(stories.length, 'story'), outlets.length && n(outlets.length, 'outlet')].filter(Boolean).join(' and ')} match${stories.length + outlets.length === 1 ? 'es' : ''} <q>${esc(q)}</q>${stories.length > 50 ? '; the 50 best stories are shown' : ''}.`
    : `Nothing in the index matches <q>${esc(q)}</q>. Try fewer words, or part of a word.`;
  $('#outlets').innerHTML = outlets.length ? '<h2>Outlets</h2><ul class="kg-results">' + outlets.slice(0, 12).map(({o}) =>
    `<li><a href="/outlet/${esc(o.id)}/" data-outlet="${esc(o.id)}">${mark(o.name || o.id, ws)}</a><span class="meta">${mark(o.domains.join(' · '), ws)}<span class="sep">·</span>${o.claims ? n(o.claims, 'claim') : 'not assessed yet'}</span></li>`).join('') + '</ul>' : '';
  $('#stories').innerHTML = stories.length ? '<h2>Stories</h2><ul class="kg-results">' + stories.slice(0, 50).map(({c}) =>
    `<li data-id="${c.id}"><a href="${esc(c.url)}"${c.url.startsWith('/') ? '' : ' rel="noopener"'}>${mark(c.title, ws)}</a><span class="meta"><span class="kg-count">${n(c.outlet_count, 'outlet')}</span>${c.origin ? `<span class="sep">·</span>first: ${esc(c.origin)}` : ''}<span class="sep">·</span>${esc(c.when || c.created)}</span></li>`).join('') + '</ul>' : '';
}
let t;
$('#q').addEventListener('input', e => { clearTimeout(t); t = setTimeout(() => search(e.target.value), 120); });
for (const a of document.querySelectorAll('[data-q]')) a.addEventListener('click', e => { e.preventDefault(); $('#q').value = a.dataset.q; search(a.dataset.q); $('#q').focus(); });
document.addEventListener('keydown', e => { if (e.key === '/' && document.activeElement !== $('#q')) { e.preventDefault(); $('#q').focus(); } });
const q0 = new URLSearchParams(location.search).get('q');
if (q0) { $('#q').value = q0; search(q0); }
"#;

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
            "id": c.id, "url": input.story_url(c), "title": c.primary_title, "outlet_count": c.outlet_count,
            "created_at": c.created_at, "created": datetime(c.created_at),
            "when": crate::render::short_time(c.created_at, input.export_time),
            "origin": input.outlet_name(&c.origin_outlet_id),
        })).collect::<Vec<_>>(),
    });
    // Outlets with the most stories in the window, as starting points.
    let mut busy: HashMap<&str, usize> = HashMap::new();
    for c in &stories {
        let mut seen = std::collections::HashSet::new();
        for j in &c.timeline {
            if seen.insert(j.outlet_id.as_str()) && input.outlets.contains_key(&j.outlet_id) {
                *busy.entry(j.outlet_id.as_str()).or_default() += 1;
            }
        }
    }
    let mut busy: Vec<_> = busy.into_iter().collect();
    busy.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let tries: String = busy
        .iter()
        .take(8)
        .map(|(id, _)| {
            let name = input.outlet_name(id);
            format!(
                "<a class=\"kg-chip\" href=\"/search/?q={}\" data-q=\"{}\">{}</a>",
                esc(&name.to_lowercase().replace(' ', "+")),
                esc(&name.to_lowercase()),
                esc(name)
            )
        })
        .collect();
    let try_line = if tries.is_empty() {
        String::new()
    } else {
        format!("<p class=\"kg-try\"><span class=\"kg-muted\">Busiest outlets in the window:</span> {tries}</p>")
    };
    let mut b = String::new();
    let _ = write!(
        b,
        "<h1>Search</h1><p class=\"kg-muted\">{} and {} stories from the {} hours to {} UTC.</p><div class=\"kg-searchbox\"><input id=\"q\" class=\"kg-input\" type=\"search\" placeholder=\"Search outlets and stories\" aria-label=\"Search outlets and stories\" autocomplete=\"off\" autofocus><kbd>/</kbd></div><p id=\"summary\" class=\"kg-muted\" aria-live=\"polite\"></p><section id=\"start\"><p class=\"kg-muted\">Every word must appear, in any order: <q>iran war</q> finds <q>Iran says the war…</q>. Outlets match by name, id or domain; stories by headline. Results are ordered by how well they match, then by how many outlets carry the story. Nothing else is ranked.</p>{}</section><noscript><p class=\"kg-muted\">Search runs in the browser over <a href=\"/search/index.json\">index.json</a>; without scripts, read that file or the <a href=\"/\">front page</a>.</p></noscript><div class=\"kg-search-results\"><section id=\"stories\"></section><aside id=\"outlets\"></aside></div><script src=\"/search.js\" defer></script>",
        n(input.outlets.len(), "outlet"),
        crate::render::thousands(stories.len()),
        window_hours,
        crate::render::short_time(input.export_time, 0),
        try_line
    );
    let page_html = html(&Page {
        title: "Search",
        description: "Find an outlet by name, id or domain, or a story by title.",
        path: "/search/",
        wide: true,
        head: "",
        body: &b,
        status: &crate::render::updated(input.export_time),
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
        // The script is a file, so the page carries no inline script.
        assert!(t
            .html
            .contains("<script src=\"/search.js\" defer></script>"));
        assert!(!t.html.contains("<script>"));
        assert!(SCRIPT.contains("fetch('/search/index.json')"));
        // Plurals of "story" are "stories"; busiest outlets offered as chips.
        assert!(SCRIPT.contains("w.slice(0, -1) + 'ies'"));
        assert!(t.html.contains("class=\"kg-chip\" href=\"/search/?q="));
        assert!(t.json["stories"][0]["origin"].is_string());
        assert_eq!(t.json["outlets"].as_array().unwrap().len(), 3);
        assert_eq!(t.json["stories"].as_array().unwrap().len(), 61);
        assert_eq!(t.json["stories"][0]["id"], 1060, "newest first");
        assert!(t.md.contains("- [Wire](/outlet/wire/) · wire.example"));
    }
}
