//! `/debug`: one self-contained HTML page that polls `/v1/clusters` and
//! `/v1/stats` and shows clusters forming. Styled from a copy of the
//! tokens in `design/tokens.css` (project rule: copy, no shared runtime
//! dependency). System sans for body text; the serif is only the lockup.
//! Sizes, line heights and spacing come only from the tokens
//! (design/DESIGN.md §3, §4); the test below rejects raw values.

pub const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kagami feed debug</title>
<meta name="description" content="Stories forming from news feeds: which outlet published first, who joined when, what was syndicated. No outlet is rated.">
<link rel="alternate" type="application/json" href="/v1/clusters" title="Clusters (JSON)">
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
.stats{display:flex;gap:var(--kg-s5);flex-wrap:wrap;margin-top:var(--kg-s4)}
.stat b{display:block;font-weight:600;font-size:var(--kg-fs-h3);line-height:var(--kg-lh-h3)}
.controls{display:flex;gap:var(--kg-s3);align-items:center;flex-wrap:wrap;margin-bottom:var(--kg-s2)}
#min{width:80px}
.cluster{padding:var(--kg-s4) 0;border-bottom:1px solid var(--kg-line)}
.cluster .title{font-size:var(--kg-fs-body-lg);line-height:var(--kg-lh-body-lg);font-weight:600;letter-spacing:-0.005em;margin:0 0 var(--kg-s1)}
.cluster .meta{display:flex;gap:var(--kg-s2);align-items:center;flex-wrap:wrap}
.origin{display:inline-flex;align-items:center;gap:var(--kg-s2);color:var(--kg-text-secondary)}
.origin img,.outlet img{width:16px;height:16px;border-radius:var(--kg-radius-input);flex:none}
.stack{display:inline-flex;gap:var(--kg-s1)}.stack:empty{display:none}
.broke{color:var(--kg-text);font-weight:600}
/* origin ring (design §10): around the first outlet's icon, or its name when it has none */
.kg-origin{border-radius:var(--kg-radius-input);box-shadow:0 0 0 1.5px var(--kg-bg),0 0 0 3px var(--kg-text)}
.kg-origin-name{border-radius:var(--kg-radius-input);box-shadow:0 0 0 1.5px var(--kg-text);padding:0 var(--kg-s1)}
.kg-sr{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap}
.stack{margin-left:var(--kg-s1)}
.outlet{display:inline-flex;align-items:center;gap:var(--kg-s2);min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.spark-wrap{display:inline-flex;align-items:center;gap:var(--kg-s2)}
.spark{width:80px;height:20px;display:block;overflow:visible}
.spark path,.spark line,.spark circle,.curve path,.curve line{vector-effect:non-scaling-stroke}
.spark .line{fill:none;stroke:var(--kg-blue);stroke-width:1.5;stroke-linejoin:round;stroke-linecap:round}
.spark .area{fill:var(--kg-blue-soft);fill-opacity:.4;stroke:none}
.spark .base{stroke:var(--kg-line);stroke-width:1}
.spark .end{fill:var(--kg-blue);stroke:none}
.spark-n{font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);color:var(--kg-text-secondary);font-variant-numeric:tabular-nums;min-width:2ch}
.curve{display:none;margin:var(--kg-s3) 0 var(--kg-s1)}
.curve .axis{stroke:var(--kg-line);stroke-width:1}
.curve .items-line{fill:none;stroke:var(--kg-line-strong);stroke-width:1.5}
.curve .outlets-line{fill:none;stroke:var(--kg-blue);stroke-width:1.5}
.curve .lbl{font-family:var(--kg-font-sans);font-size:var(--kg-fs-small);fill:var(--kg-text-muted)}
.items{margin:var(--kg-s2) 0 0;padding:0;list-style:none;display:none}
.cluster.open .items,.cluster.open .curve{display:block}
.items li{display:grid;grid-template-columns:var(--kg-s8) 160px 1fr;gap:var(--kg-s3);padding:var(--kg-s2) 0;color:var(--kg-text-secondary);font-size:var(--kg-fs-ui);line-height:var(--kg-lh-body);border-top:1px solid var(--kg-line);align-items:baseline}
.items .when{line-height:var(--kg-lh-body)}
.items li.first .when{color:var(--kg-blue)}
.items .hl{color:var(--kg-text);font-size:var(--kg-fs-body);line-height:var(--kg-lh-body)}
.items .hl>a{color:inherit}.items .hl>a:hover{color:var(--kg-blue)}
.items .alt{display:block;font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);color:var(--kg-text-muted);margin-top:var(--kg-s1)}
@media (max-width:720px){.items li{grid-template-columns:1fr;gap:var(--kg-s1)}}
</style>
</head>
<body>
<main>
<header>
  <span class="lockup"><span class="wordmark">Kagami<span class="kanji">鏡</span></span><span class="rule"></span><span class="tool">feed</span></span>
  <span id="health" class="status ok"><span>connecting</span></span>
</header>
<div class="stats" id="stats"></div>
<h2>Clusters</h2>
<div class="controls">
  <select id="sort"><option value="recent">Most recent</option><option value="velocity_1h">Items, 1h</option><option value="outlets_6h">Outlets, 6h</option></select>
  <input id="min" type="number" min="1" value="2" title="Minimum outlets">
  <span class="muted">min outlets · refreshes every 5 s ·</span>
  <select id="tz" title="Time zone for all times on this page"><option value="local">Local time</option><option value="utc">UTC</option></select>
</div>
<div id="clusters"><div class="empty">No clusters yet.</div></div>
<p class="muted quiet foot">Machine readers: the same data is at <a href="/v1/clusters">/v1/clusters</a>, and each cluster answers <span class="mono">Accept: text/markdown</span>. See <a href="/llms.txt">/llms.txt</a>.</p>
</main>
<script>
const $ = s => document.querySelector(s);
const seen = new Set();
// Times: one zone for the whole page (local by default, UTC on request, remembered
// per browser). Short form is the clock time, with a date when not today;
// tooltips always give the full timestamp in both zones.
let tz = 'local';
try { tz = localStorage.getItem('kg-tz') || 'local'; } catch {}
const tzOpt = () => tz === 'utc' ? {timeZone: 'UTC'} : {};
const fmtT = ts => {
  if (!ts) return '—';
  const d = new Date(ts*1000), o = tzOpt();
  const day = x => x.toLocaleDateString('en-CA', o);
  // The zone is stated once, by the time-zone control; each time is just the clock.
  const hm = d.toLocaleTimeString([], {...o, hour: '2-digit', minute: '2-digit', hourCycle: 'h23'});
  return day(d) === day(new Date()) ? hm : d.toLocaleDateString([], {...o, month: 'short', day: 'numeric'}) + ' ' + hm;
};
const fullT = ts => ts ? `${new Date(ts*1000).toLocaleString([], {dateStyle: 'medium', timeStyle: 'long'})} · ${new Date(ts*1000).toISOString().replace('.000Z','Z')}` : '';
const tsSpan = ts => `<time datetime="${ts ? new Date(ts*1000).toISOString() : ''}" title="${esc(fullT(ts))}">${fmtT(ts)}</time>`;
const age = (ts, now) => { const s = Math.max(0, now - ts); return s < 3600 ? Math.round(s/60)+' min' : s < 86400 ? Math.round(s/3600)+' h' : Math.round(s/86400)+' d'; };
// Sparkline (design §5): cumulative distinct outlets in twelve two-hour
// buckets from the first join, y scaled to `scale` shared by every card on
// the page, drawn at size with a baseline, an end dot and the final count.
const BUCKETS = 12, BUCKET_SECS = 7200;
function sparkPoints(tl){
  if (!tl || !tl.length) return [];
  const t0 = tl[0].joined_at;
  return Array.from({length: BUCKETS}, (_, b) =>
    new Set(tl.filter(j => j.joined_at < t0 + (b + 1) * BUCKET_SECS).map(j => j.outlet_id)).size);
}
function spark(pts, scale){
  if (!pts.length) return '';
  const W = 80, H = 20, top = 2, bottom = H - 2, step = W / (pts.length - 1);
  const xy = pts.map((v, i) => [i * step, bottom - (bottom - top) * v / Math.max(1, scale)]);
  const line = 'M' + xy.map(p => p[0].toFixed(1) + ',' + p[1].toFixed(1)).join(' L');
  const [ex, ey] = xy[xy.length - 1];
  // A single-outlet story draws only the baseline and the end dot.
  const curve = pts.some(v => v > 1) ? `<path class="area" d="${line} L${W},${bottom} L0,${bottom} Z"/><path class="line" d="${line}"/>` : '';
  return `<span class="spark-wrap"><svg class="spark" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="Outlets carrying the story, two-hour steps: ${pts.join(', ')}"><line class="base" x1="0" y1="${bottom}" x2="${W}" y2="${bottom}"/>${curve}<circle class="end" cx="${ex.toFixed(1)}" cy="${ey.toFixed(1)}" r="1.5"/></svg><span class="spark-n">${pts[pts.length - 1]}</span></span>`;
}
// Spread curve for an expanded cluster: cumulative outlets (blue) and
// items (grey) against time since the first join. Axes are the baseline
// and the left edge; labels are the span and the final counts.
function curve(tl, now){
  if (!tl || tl.length < 2) return '';
  const W = 320, H = 80, L = 28, B = 16;
  const t0 = tl[0].joined_at, span = Math.max(60, tl[tl.length-1].joined_at - t0);
  const seen = new Set(); let o = 0;
  const pts = tl.map((j, i) => { if (!seen.has(j.outlet_id)) { seen.add(j.outlet_id); o++; } return [L + (W-L) * (j.joined_at - t0) / span, i + 1, o]; });
  const maxI = tl.length, sy = v => H - B - (H - B - 4) * v / maxI;
  const step = (idx) => pts.map((p, k) => (k ? `L${p[0].toFixed(1)},${sy(pts[k-1][idx]).toFixed(1)} ` : `M${p[0].toFixed(1)},${sy(0).toFixed(1)} `) + `L${p[0].toFixed(1)},${sy(p[idx]).toFixed(1)}`).join(' ');
  const label = span < 3600 ? Math.round(span/60) + ' min' : span < 86400 ? (span/3600).toFixed(1) + ' h' : (span/86400).toFixed(1) + ' d';
  return `<svg class="curve" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="Spread over ${label}: ${o} outlets (blue) and ${maxI} items (grey)">
    <line class="axis" x1="${L}" y1="${H-B}" x2="${W}" y2="${H-B}"/>
    <line class="axis" x1="${L}" y1="4" x2="${L}" y2="${H-B}"/>
    <path class="items-line" d="${step(1)}"/>
    <path class="outlets-line" d="${step(2)}"/>
    <text class="lbl" x="${L-4}" y="12" text-anchor="end">${maxI}</text>
    <text class="lbl" x="${L-4}" y="${H-B}" text-anchor="end">0</text>
    <text class="lbl" x="${W}" y="${H-2}" text-anchor="end">${label} · ${o} outlets, ${maxI} items</text>
  </svg>`;
}
function render(list, now){
  const root = $('#clusters');
  if (!list.length) { root.innerHTML = '<div class="empty">No clusters yet.</div>'; return; }
  const open = new Set([...root.querySelectorAll('.cluster.open')].map(e=>e.dataset.id));
  const pts = new Map(list.map(c => [c.id, sparkPoints(c.timeline)]));
  const scale = Math.max(1, ...[...pts.values()].flat());
  root.innerHTML = list.map(c => {
    const v = c.velocity || {};
    const fresh = !seen.has(c.id);
    seen.add(c.id);
    const items = compare(c);
    return `<article class="cluster${fresh?' enter':''}${open.has(String(c.id))?' open':''}" data-id="${c.id}">
      <p class="title title-link"><a href="/v1/clusters/${c.id}" onclick="this.closest('.cluster').classList.toggle('open');return false">${esc(c.primary_title)}</a></p>
      <div class="meta muted quiet">
        ${outletStack(c)}
        ${[
          c.item_count > c.outlet_count ? n(c.item_count,'item') : '',
          v.items_1h && v.items_1h < c.item_count ? `${v.items_1h} new in the last hour` : '',
          c.syndication && c.syndication.length ? n(c.syndication.length,'syndication group') : '',
          c.last_active_at - c.created_at > 60 ? `since ${tsSpan(c.created_at)}, updated <span title="${esc(fullT(c.last_active_at))}">${age(c.last_active_at, now)} ago</span>` : tsSpan(c.created_at),
        ].filter(Boolean).map(t=>`<span class="sep">·</span><span>${t}</span>`).join('')}
        ${spark(pts.get(c.id), scale)}
      </div>
      ${curve(c.timeline, now)}
      <ul class="items">${items}</ul>
    </article>`;
  }).join('');
}
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
// Outlet id -> {name, domain}, from the registry; ids fall back to themselves.
let outlets = {};
fetch('/v1/outlets').then(r=>r.json()).then(v => { for (const o of v) outlets[o.id] = {name: o.name, domain: (o.domains||[])[0], ledger: o.ledger_lookup}; }).catch(()=>{});
// Outlet name links to the ledger's page for it: the facts about an outlet
// are one click from any story it is in, and never shown here as a colour.
// Moves the origin ring from the outlet's name to its icon once the icon has
// loaded, so the ring never wraps an empty box.
const ringToIcon = "this.classList.add('kg-origin');const n=this.closest('.origin,.outlet').querySelector('.kg-origin-name');if(n)n.classList.remove('kg-origin-name')";
const outletTag = (id, cls, origin) => {
  const o = outlets[id] || {};
  const icon = o.domain ? `<img src="https://icons.duckduckgo.com/ip3/${encodeURIComponent(o.domain)}.ico" alt="" loading="lazy"${origin ? ` onload="${ringToIcon}"` : ''} onerror="this.remove()">` : '';
  const nm = o.ledger ? `<a href="${esc(o.ledger)}" title="Sourced facts about ${esc(o.name || id)} in the ledger">${esc(o.name || id)}</a>` : esc(o.name || id);
  return `<span class="${cls}" title="${esc(o.domain || id)}" data-outlet="${esc(id)}">${icon}${origin ? `<span class="kg-origin-name"><span class="kg-sr">first to publish: </span>${nm}</span>` : nm}</span>`;
};
// Every outlet in the cluster, origin first: icons overlap, first two named, rest as +N.
const outletStack = c => {
  const ids = [...new Set([c.origin_outlet_id, ...[...(c.items||[])].sort((a,b)=>at(a)-at(b)).map(i=>i.outlet_id)])];
  const icons = ids.slice(0, 8).map((id, k) => { const o = outlets[id] || {};
    // The origin ring starts on the first outlet's name and moves to its icon once the icon loads.
    return o.domain ? `<img src="https://icons.duckduckgo.com/ip3/${encodeURIComponent(o.domain)}.ico" alt="" title="${esc(o.name||id)}${k ? '' : ' · first to publish'}" loading="lazy"${k ? '' : ` onload="${ringToIcon}"`} onerror="this.remove()">` : ''; }).join('');
  const names = ids.slice(0, 2).map((id, k) => { const nm = esc((outlets[id]||{}).name || id);
    // No visible "first": the ring says it; screen readers get the words.
    return k ? nm : `<b class="broke kg-origin-name"><span class="kg-sr">first to publish: </span>${nm}</b>`; }).join('<span class="sep">·</span>');
  const total = Math.max(c.outlet_count || 0, ids.length);
  const more = total > 2 ? ` <span class="muted">+${total - 2}</span>` : '';
  return `<span class="origin" title="${esc(ids.map(id => (outlets[id]||{}).name || id).join(', '))}"><span class="stack">${icons}</span>${names}${more}</span>`;
};
// Expanded view: one row per outlet in the order it picked the story up, with
// its headline so framings can be compared side by side. Later rewrites from
// the same outlet sit underneath.
const link = i => /^https?:/.test(i.canonical_url) ? `<a href="${esc(i.canonical_url)}" target="_blank" rel="noopener">${esc(i.title)}</a>` : esc(i.title);
// Ordered by when Kagami saw each item join (matches the backend's origin outlet).
const at = i => i.joined_at ?? i.fetched_at ?? i.published_at;
const compare = c => {
  const by = new Map();
  for (const i of [...(c.items||[])].sort((a,b)=>at(a)-at(b))) {
    if (!by.has(i.outlet_id)) by.set(i.outlet_id, []);
    by.get(i.outlet_id).push(i);
  }
  const t0 = Math.min(...[...by.values()].map(v=>at(v[0])));
  // Headlines are title links; "also:" rewrites and outlet names are quiet.
  return [...by].map(([id, v], k) => {
    const d = at(v[0]) - t0;
    const when = '+' + (d < 3600 ? Math.round(d/60) + 'm' : (d/3600).toFixed(1) + 'h');
    const alts = v.slice(1).filter(i => i.title !== v[0].title).map(i => `<span class="alt">also: ${link(i)}</span>`).join('');
    return `<li${k ? '' : ' class="first"'}><span class="mono when" title="${esc(`Picked up ${fullT(at(v[0]))}` + (v[0].published_at ? `\nOutlet says published ${fullT(v[0].published_at)}` : ''))}">${when}</span>${outletTag(id, 'outlet quiet', k === 0)}<span class="hl">${link(v[0])}<span class="quiet">${alts}</span></span></li>`;
  }).join('');
};
const n = (k, word) => `${k} ${word}${k === 1 ? '' : 's'}`;
async function tick(){
  try {
    const sort = $('#sort').value, min = $('#min').value || 2;
    const [cs, st] = await Promise.all([
      fetch(`/v1/clusters?limit=100&min_outlets=${min}&sort=${sort}&include=items`).then(r=>r.json()),
      fetch('/v1/stats').then(r=>r.json())]);
    const now = Math.floor(Date.now()/1000);
    const p = st.process, s = st.store;
    $('#stats').innerHTML = [
      // The feeds count opens the network dashboard: every feed and its health.
      ['feeds', `<a href="/feeds" title="Every feed and its status">${st.feeds_configured ?? s.feeds_total}</a> <span class="muted">${s.feeds_dead} dead</span>`],
      ['items / hour', s.items_last_hour], ['items total', s.items_total],
      ['embedded', s.items_embedded], ['active clusters', p.active_clusters],
      ['embed p50', (p.embed_latency.p50_us/1000).toFixed(1)+' ms'],
      ['assign p50', (p.assign_latency.p50_us/1000).toFixed(2)+' ms'],
      ['fetch errors', p.fetch_errors]
    ].map(([k,v])=>`<div class="stat"><b>${v}</b><span class="muted">${k}</span></div>`).join('');
    render(cs.items || [], now);
    const h = $('#health');
    if (p.fetch_errors > p.fetches / 2 && p.fetches > 5) { h.className='status warn'; h.innerHTML='<span>most fetches failing</span>'; }
    else { h.className='status ok'; h.innerHTML=`<span>up ${age(now - p.uptime_secs, now)}</span>`; }
  } catch (e) {
    const h = $('#health'); h.className='status err'; h.innerHTML='<span>api unreachable</span>';
  }
}
tick(); setInterval(tick, 5000);
$('#sort').onchange = tick; $('#min').onchange = tick;
$('#tz').value = tz; $('#tz').onchange = e => { tz = e.target.value; try { localStorage.setItem('kg-tz', tz); } catch {} tick(); };
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
