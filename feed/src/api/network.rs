//! `/feeds`: the network dashboard. Every configured feed with its outlet,
//! a status light and label, last fetch and last success as ages, the
//! adapted interval and the error in plain words. Polls `/v1/feeds` and
//! `/v1/outlets`; linked from the feeds count on `/debug`. Styled from the
//! same copy of the design tokens as the debug page (project rule: copy,
//! no shared runtime dependency); the test below rejects raw values.

pub const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Truth Decay feed network</title>
<meta name="description" content="Every feed Truth Decay polls: which are online, which are failing and why, which the steward is holding, and when each was last fetched.">
<link rel="alternate" type="application/json" href="/v1/feeds" title="Feeds (JSON)">
<link rel="alternate" type="text/plain" href="/llms.txt" title="How to read this server">
<style>
:root{--kg-blue:#2563EB;--kg-blue-hover:#1D4ED8;--kg-blue-soft:#DBEAFE;--kg-blue-ink:#1E3A8A;--kg-bg:#FFFFFF;--kg-bg-subtle:#F8FAFC;--kg-line:#E2E8F0;--kg-line-strong:#CBD5E1;--kg-text-muted:#64748B;--kg-text-secondary:#334155;--kg-text:#0F172A;--kg-ok:#059669;--kg-warn:#D97706;--kg-err:#DC2626;--kg-font-serif:"Instrument Serif", "Iowan Old Style", "Times New Roman", serif;--kg-font-sans:Inter, -apple-system, "Segoe UI", system-ui, sans-serif;--kg-font-mono:"JetBrains Mono", "SF Mono", Menlo, Consolas, monospace;--kg-fs-small:12px;--kg-fs-ui:13px;--kg-fs-body:15px;--kg-fs-body-lg:17px;--kg-fs-h3:20px;--kg-fs-h2:28px;--kg-lh-small:16px;--kg-lh-ui:20px;--kg-lh-body:24px;--kg-lh-body-lg:24px;--kg-lh-h3:28px;--kg-lh-h2:32px;--kg-lockup-wordmark:28px;--kg-lockup-kanji:17px;--kg-s1:4px;--kg-s2:8px;--kg-s3:12px;--kg-s4:16px;--kg-s5:24px;--kg-s6:32px;--kg-s7:48px;--kg-s8:64px;--kg-s9:96px;--kg-radius-input:4px;--kg-t-fast:120ms;--kg-t-base:200ms;--kg-ease:cubic-bezier(0.2, 0, 0, 1)}
html{background:var(--kg-bg);color:var(--kg-text)}
body{margin:0;font:400 var(--kg-fs-body)/var(--kg-lh-body) var(--kg-font-sans);font-feature-settings:"tnum","cv11","ss01";-webkit-font-smoothing:antialiased}
p,ul,ol{margin:0}
main{max-width:1200px;margin:0 auto;padding:var(--kg-s6) var(--kg-s5) var(--kg-s9)}
header{display:flex;align-items:baseline;justify-content:space-between;gap:var(--kg-s4);flex-wrap:wrap;padding-bottom:var(--kg-s4);border-bottom:1px solid var(--kg-line)}
.lockup{display:inline-flex;align-items:baseline;gap:var(--kg-s3);font-size:var(--kg-lockup-wordmark);line-height:var(--kg-lh-h2)}
.wordmark{font-family:var(--kg-font-serif);font-style:italic;letter-spacing:-0.01em;color:var(--kg-text)}
.wordmark .kanji{font-family:"Noto Serif JP",serif;font-style:normal;font-size:var(--kg-lockup-kanji);color:var(--kg-text-muted);margin-left:var(--kg-s1)}
.rule{align-self:stretch;width:1px;background:var(--kg-line-strong)}
.tool{font-weight:500;font-size:var(--kg-fs-ui);line-height:var(--kg-lh-ui);color:var(--kg-text-secondary)}
.muted{color:var(--kg-text-muted);font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);letter-spacing:.01em}
.mono{font-family:var(--kg-font-mono);font-size:var(--kg-fs-ui);line-height:var(--kg-lh-ui)}
h2{font-family:var(--kg-font-serif);font-weight:400;font-size:var(--kg-fs-h2);line-height:var(--kg-lh-h2);letter-spacing:-0.01em;margin:var(--kg-s6) 0 var(--kg-s2)}
h3{font-size:var(--kg-fs-h3);line-height:var(--kg-lh-h3);font-weight:600;letter-spacing:-0.01em;margin:var(--kg-s5) 0 var(--kg-s2)}
a{color:var(--kg-blue);text-decoration:none;text-decoration-thickness:1px;text-underline-offset:3px;text-decoration-skip-ink:auto;transition:color var(--kg-t-fast) var(--kg-ease),text-decoration-color var(--kg-t-fast) var(--kg-ease)}
a:hover{text-decoration-line:underline;color:var(--kg-blue-hover)}
.title-link a,a.title-link{color:inherit}
.title-link a:hover,a.title-link:hover{color:var(--kg-blue)}
.quiet a,a.quiet{color:inherit;text-decoration-line:underline;text-decoration-color:var(--kg-line-strong)}
.quiet a:hover,a.quiet:hover{color:var(--kg-blue);text-decoration-color:currentColor}
:focus-visible{outline:2px solid var(--kg-blue);outline-offset:2px}
input[type=search],input[type=number],select{font:400 var(--kg-fs-ui)/var(--kg-lh-ui) var(--kg-font-sans);height:36px;padding:0 var(--kg-s3);border:1px solid var(--kg-line-strong);border-radius:var(--kg-radius-input);background:var(--kg-bg);color:var(--kg-text);box-sizing:border-box}
input[type=search]:focus,input[type=number]:focus,select:focus{outline:2px solid var(--kg-blue);outline-offset:0}
.tag{font-family:var(--kg-font-mono);font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);padding:2px var(--kg-s2);border:1px solid var(--kg-line);border-radius:var(--kg-radius-input);color:var(--kg-text-secondary);white-space:nowrap}
.sep{color:var(--kg-line-strong)}
.status{display:inline-flex;align-items:center;gap:var(--kg-s2);font-size:var(--kg-fs-ui);line-height:var(--kg-lh-ui)}
.status::before{content:"";width:8px;height:8px;border-radius:50%;background:currentColor}
.status.ok{color:var(--kg-ok)}.status.warn{color:var(--kg-warn)}.status.err{color:var(--kg-err)}
.status span{color:var(--kg-text-secondary)}
.empty{color:var(--kg-text-muted);padding:var(--kg-s5) 0}
.foot{margin-top:var(--kg-s7)}
@keyframes kg-fade-in{from{opacity:0}to{opacity:1}}
.enter{animation:kg-fade-in var(--kg-t-base) var(--kg-ease)}
@media (prefers-reduced-motion:reduce){.enter{animation:none}a{transition:none}}
@media (max-width:720px){main{padding:var(--kg-s5) var(--kg-s4) var(--kg-s8)}}
.back{margin-left:var(--kg-s3)}
.stats{display:flex;gap:var(--kg-s5);flex-wrap:wrap;margin-top:var(--kg-s4)}
.stat b{display:block;font-weight:600;font-size:var(--kg-fs-h3);line-height:var(--kg-lh-h3)}
.stat .status{margin-top:var(--kg-s1)}
.strip{display:flex;flex-wrap:wrap;gap:var(--kg-s1);margin:var(--kg-s3) 0 0;padding:0;list-style:none}
.strip li{width:12px;height:12px;border-radius:var(--kg-radius-input);background:var(--kg-line)}
.strip li.ok{background:var(--kg-ok)}.strip li.warn{background:var(--kg-warn)}.strip li.err{background:var(--kg-err)}
.strip li.idle{background:var(--kg-line-strong)}
.strip li:hover{outline:2px solid var(--kg-text);outline-offset:1px}
.controls{display:flex;gap:var(--kg-s3);align-items:center;flex-wrap:wrap;margin-bottom:var(--kg-s3)}
#q{width:240px}
.status.idle{color:var(--kg-line-strong)}
table{width:100%;border-collapse:collapse}
th{font:500 var(--kg-fs-small)/var(--kg-lh-small) var(--kg-font-sans);text-transform:uppercase;letter-spacing:.06em;color:var(--kg-text-muted);text-align:left;padding:var(--kg-s2) var(--kg-s3) var(--kg-s2) 0;border-bottom:1px solid var(--kg-line-strong);white-space:nowrap}
td{padding:var(--kg-s2) var(--kg-s3) var(--kg-s2) 0;border-bottom:1px solid var(--kg-line);font-size:var(--kg-fs-ui);line-height:var(--kg-lh-ui);color:var(--kg-text-secondary);vertical-align:top}
tbody tr:hover td{background:var(--kg-bg-subtle)}
td.num,th.num{text-align:right;font-variant-numeric:tabular-nums}
.outlet{display:inline-flex;align-items:center;gap:var(--kg-s2);color:var(--kg-text);font-weight:500;white-space:nowrap}
.outlet img{width:16px;height:16px;border-radius:var(--kg-radius-input);flex:none}
.feedurl{font-family:var(--kg-font-mono);font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);overflow-wrap:anywhere}
.moved{display:block;color:var(--kg-text-muted)}
.nw{white-space:nowrap}
.err-text{display:block;color:var(--kg-text-muted);font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);margin-top:var(--kg-s1);overflow-wrap:anywhere;max-width:48ch}
@media (max-width:720px){.hide-sm{display:none}}
</style>
</head>
<body>
<main>
<header>
  <span class="lockup"><span class="wordmark">Truth Decay</span><span class="rule"></span><span class="tool">feed</span></span>
  <span class="muted quiet"><span id="updated">loading</span><a class="back" href="/debug">clusters</a></span>
</header>
<div class="stats" id="stats"></div>
<ul class="strip" id="strip" aria-label="One light per feed"></ul>
<h2>Network</h2>
<div class="controls">
  <select id="show" title="Which feeds to list"><option value="all">All feeds</option><option value="problem">Needing attention</option><option value="ok">Healthy</option><option value="err">Failing or dead</option></select>
  <input id="q" type="search" placeholder="Outlet or feed address" autocomplete="off">
  <span class="muted" id="count"></span>
</div>
<table>
  <thead><tr><th>Status</th><th>Outlet</th><th class="hide-sm">Feed</th><th>Last fetch</th><th class="hide-sm">Last success</th><th class="hide-sm">Every</th><th class="num">Items</th><th class="num hide-sm">Fails</th></tr></thead>
  <tbody id="rows"><tr><td colspan="8" class="empty">Loading feeds.</td></tr></tbody>
</table>
<p class="muted quiet foot">Machine readers: the same data is at <a href="/v1/feeds">/v1/feeds</a> (<a href="/v1/feeds?filter=history">with removed feeds</a>). See <a href="/llms.txt">/llms.txt</a>.</p>
</main>
<script>
const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
// Ages, never full timestamps on the page; the full time is on hover.
const dur = s => s < 60 ? `${s} s` : s < 3600 ? `${Math.round(s/60)} min` : s < 86400 ? `${Math.round(s/3600)} h` : `${Math.round(s/86400)} d`;
const full = ts => ts ? new Date(ts*1000).toISOString().replace('.000Z', 'Z').replace('T', ' ') : '';
const ago = (ts, now) => ts ? `<time datetime="${new Date(ts*1000).toISOString()}" title="${full(ts)}">${dur(Math.max(0, now - ts))} ago</time>` : '<span class="muted">never</span>';
// One status per feed, most serious first. Semantic colours are for system
// status only (design §2), and every light has a text label beside it.
function status(f, now){
  const e = f.last_error || '';
  if (f.dead) return {k: 'err', label: 'dead', rank: 0};
  const q = f.health && f.health.quarantine;
  if (q && now < q.until) return {k: 'warn', label: 'quarantined', rank: 3, note: `${q.reason} Released in ${dur(q.until - now)}.`};
  if (/robots\.txt/.test(e)) return {k: 'warn', label: 'skipped: robots.txt', rank: 3};
  if (f.fail_count > 0) return {k: 'err', label: f.fail_count > 2 ? 'failing' : 'backing off', rank: f.fail_count > 2 ? 1 : 2};
  if (!f.last_fetch_at) return {k: 'idle', label: 'waiting', rank: 5};
  if (!f.last_ok_at || now - f.last_ok_at > 86400) return {k: 'warn', label: 'stale', rank: 4};
  // Steward flag (docs/steward.md §2 rule 4): the feed works, but its
  // claimed publish times are days off; pick-up times are what Truth Decay saw.
  if (f.health && f.health.timestamps_unreliable) return {k: 'ok', label: 'online', rank: 6, note: 'timestamps unreliable: claimed publish times are days from pick-up; the timeline shows both.'};
  return {k: 'ok', label: 'online', rank: 6};
}
let outlets = {}, feeds = [];
// Update only what changed, so the 10 s refresh never reloads icons or
// flashes the table: markup is set when it differs, and each row keeps its
// node (keyed by feed URL), replaced only when its own markup changed.
const setHtml = (el, html) => { if (el.innerHTML !== html) el.innerHTML = html; };
const rowHtml = new Map();
function patchRows(body, rows, empty){
  if (!rows.length) { rowHtml.clear(); setHtml(body, empty); return; }
  body.querySelector('td.empty')?.closest('tr')?.remove();
  const nodes = new Map([...body.querySelectorAll('tr[data-key]')].map(e => [e.dataset.key, e]));
  const order = rows.map(([key, html]) => {
    let el = nodes.get(key);
    if (!el || rowHtml.get(key) !== html) {
      const t = document.createElement('template');
      t.innerHTML = `<table><tbody>${html}</tbody></table>`;
      const fresh = t.content.querySelector('tr');
      fresh.dataset.key = key;
      if (el) el.replaceWith(fresh);
      el = fresh;
      rowHtml.set(key, html);
    }
    nodes.delete(key);
    return el;
  });
  for (const [key, el] of nodes) { el.remove(); rowHtml.delete(key); }
  order.forEach((el, i) => { if (body.children[i] !== el) body.insertBefore(el, body.children[i] || null); });
}
const host = u => { try { return new URL(u).host.replace(/^www\./, ''); } catch { return u; } };
const shortErr = e => e.replace(/^error sending request for url \([^)]*\): /, '').replace(/^client error \((\w+)\): /, '$1: ');
function outletCell(id){
  const o = outlets[id] || {};
  const icon = o.domain ? `<img src="https://icons.duckduckgo.com/ip3/${encodeURIComponent(o.domain)}.ico" alt="" loading="lazy" onerror="this.remove()">` : '';
  const nm = esc(o.name || id);
  return `<span class="outlet quiet">${icon}${o.ledger ? `<a href="${esc(o.ledger)}" title="Sourced facts about ${nm}">${nm}</a>` : nm}</span>`;
}
function render(){
  const now = Math.floor(Date.now()/1000);
  const rows = feeds.map(f => ({f, s: status(f, now)}));
  const count = k => rows.filter(r => r.s.label === k).length;
  const tiles = [
    ['feeds', rows.length, ''], ['online', count('online'), 'ok'], ['backing off', count('backing off'), 'err'],
    ['failing', count('failing'), 'err'], ['dead', count('dead'), 'err'], ['stale', count('stale'), 'warn'],
    ['quarantined', count('quarantined'), 'warn'], ['skipped: robots.txt', count('skipped: robots.txt'), 'warn'], ['waiting', count('waiting'), 'idle'],
    ['timestamps unreliable', rows.filter(r => r.f.health && r.f.health.timestamps_unreliable).length, 'warn'],
  ].filter(([k, v], i) => i < 2 || v > 0);
  setHtml($('#stats'), tiles.map(([k, v, c]) => `<div class="stat"><b>${v}</b>${c ? `<span class="status ${c}"><span class="muted">${k}</span></span>` : `<span class="muted">${k}</span>`}</div>`).join(''));
  const sorted = [...rows].sort((a, b) => a.s.rank - b.s.rank || (outlets[a.f.outlet_id]?.name || a.f.outlet_id).localeCompare(outlets[b.f.outlet_id]?.name || b.f.outlet_id));
  setHtml($('#strip'), sorted.map(r => `<li class="${r.s.k}" title="${esc((outlets[r.f.outlet_id]||{}).name || r.f.outlet_id)}: ${r.s.label}" aria-label="${esc(r.f.outlet_id)} ${r.s.label}"></li>`).join(''));
  const show = $('#show').value, q = $('#q').value.trim().toLowerCase();
  const list = sorted.filter(r => (show === 'all' || (show === 'ok' ? r.s.k === 'ok' : show === 'err' ? r.s.k === 'err' : r.s.k !== 'ok'))
    && (!q || (r.f.outlet_id + ' ' + ((outlets[r.f.outlet_id]||{}).name || '') + ' ' + r.f.url).toLowerCase().includes(q)));
  $('#count').textContent = `${list.length} of ${rows.length}`;
  const trs = list.map(({f, s}) => [f.url, `<tr data-outlet="${esc(f.outlet_id)}">
    <td><span class="status ${s.k}"><span>${s.label}</span></span>${s.note ? `<span class="err-text">${esc(s.note)}</span>` : f.last_error ? `<span class="err-text" title="${esc(f.last_error)}">${esc(shortErr(f.last_error))}</span>` : ''}</td>
    <td>${outletCell(f.outlet_id)}</td>
    <td class="hide-sm quiet"><a class="feedurl" href="${esc(f.url)}" rel="noopener">${esc(host(f.url))}${esc(new URL(f.url, 'http://x').pathname.length > 1 ? new URL(f.url, 'http://x').pathname : '')}</a>${f.resolved_url ? `<span class="feedurl moved">moved to ${esc(f.resolved_url)}</span>` : ''}</td>
    <td class="nw">${ago(f.last_fetch_at, now)}${f.last_status ? ` <span class="muted">${f.last_status}</span>` : ''}</td>
    <td class="hide-sm nw">${ago(f.last_ok_at, now)}</td>
    <td class="hide-sm nw" title="configured every ${dur(f.base_interval_secs)}">${dur(f.interval_secs)}</td>
    <td class="num">${f.last_items ?? '—'}</td>
    <td class="num hide-sm">${f.fail_count || ''}</td></tr>`]);
  patchRows($('#rows'), trs, '<tr><td colspan="8" class="empty">No feed matches.</td></tr>');
  $('#updated').textContent = `updated ${new Date().toLocaleTimeString([], {hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23'})} · refreshes every 10 s`;
}
async function tick(){
  try {
    const [fs, os] = await Promise.all([fetch('/v1/feeds').then(r => r.json()), Object.keys(outlets).length ? null : fetch('/v1/outlets').then(r => r.json())]);
    if (os) for (const o of os) outlets[o.id] = {name: o.name, domain: (o.domains||[])[0], ledger: o.ledger_lookup};
    feeds = fs; render();
  } catch (e) { $('#updated').textContent = 'api unreachable'; }
}
$('#show').onchange = render; $('#q').oninput = render;
tick(); setInterval(tick, 10000);
</script>
</body>
</html>
"##;

#[cfg(test)]
mod design_tests {
    //! Design conformance (design/DESIGN.md §3, §4, §12), modelled on lens's
    //! `stylesheets_use_only_scale_values`. Copied per crate, not shared
    //! (project rule).

    const PAGE_HTML: &str = super::PAGE;

    fn style_block() -> &'static str {
        let a = PAGE_HTML.find("<style>").expect("<style>") + "<style>".len();
        let b = PAGE_HTML.find("</style>").expect("</style>");
        &PAGE_HTML[a..b]
    }

    /// `(selector, property, value)` for every declaration, including
    /// inline `style=""` attributes (selector `style=`).
    fn declarations() -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for block in style_block().split('}') {
            let Some((sel, body)) = block.rsplit_once('{') else {
                continue;
            };
            let sel = sel.rsplit('{').next().unwrap_or(sel).trim().to_string();
            for d in body.split(';') {
                if let Some((p, v)) = d.split_once(':') {
                    out.push((sel.clone(), p.trim().to_string(), v.trim().to_string()));
                }
            }
        }
        for part in PAGE_HTML.split("style=\"").skip(1) {
            let body = part.split('"').next().unwrap_or("");
            for d in body.split(';') {
                if let Some((p, v)) = d.split_once(':') {
                    out.push(("style=".into(), p.trim().to_string(), v.trim().to_string()));
                }
            }
        }
        out
    }

    #[test]
    fn embedded_css_uses_only_scale_values() {
        let spacing = [
            "4px", "8px", "12px", "16px", "24px", "32px", "48px", "64px", "96px",
        ];
        for (sel, prop, val) in declarations() {
            if sel == ":root" || prop.starts_with("--") {
                continue; // token definitions, checked against design/ below
            }
            let at = format!("`{sel} {{ {prop}: {val} }}`");
            if prop == "font-size" {
                assert!(
                    val.starts_with("var(--kg-fs-") || val.starts_with("var(--kg-lockup-"),
                    "raw font size {at}"
                );
            }
            if prop == "font" {
                assert!(
                    val.contains("var(--kg-fs-") && !val.contains("px"),
                    "font shorthand off the scale {at}"
                );
            }
            if prop == "line-height" {
                assert!(
                    val.starts_with("var(--kg-lh-"),
                    "line height not a grid token {at}"
                );
            }
            if prop.starts_with("margin") || prop.starts_with("padding") || prop == "gap" {
                for tok in val.split_whitespace() {
                    let ok = tok == "0"
                        || tok == "auto"
                        || tok.starts_with("var(--kg-s")
                        || spacing.contains(&tok)
                        || (tok == "2px" && sel == ".tag");
                    assert!(ok, "spacing off the 4px scale {at}");
                }
            }
        }
        assert!(
            !PAGE_HTML.contains("font-size=\""),
            "SVG text sizes come from a class on the scale, not a font-size attribute"
        );
        assert!(
            !PAGE_HTML.contains("preserveAspectRatio=\"none\""),
            "charts are drawn at size, never stretched"
        );
    }

    #[test]
    fn embedded_tokens_match_design() {
        let design = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/tokens.css"),
        )
        .expect("design/tokens.css");
        let defs = |css: &str| -> Vec<(String, String)> {
            css.split([';', '{', '}', '\n'])
                .filter_map(|d| {
                    let d = d.trim();
                    let d = d.split("/*").next().unwrap_or(d).trim();
                    d.strip_prefix("--kg-")
                        .and_then(|r| r.split_once(':'))
                        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                })
                .collect()
        };
        let want: std::collections::HashMap<String, String> = defs(&design).into_iter().collect();
        let root = style_block()
            .split(":root{")
            .nth(1)
            .expect(":root")
            .split('}')
            .next()
            .unwrap();
        let have = defs(&format!("{{{root}}}"));
        assert!(have.len() > 20, "tokens block present");
        for (k, v) in have {
            let w = want
                .get(&k)
                .unwrap_or_else(|| panic!("--kg-{k} is not a design token"));
            assert_eq!(&v, w, "--kg-{k} differs from design/tokens.css");
        }
    }
}
