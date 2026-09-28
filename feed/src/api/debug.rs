//! `/debug`: one self-contained HTML page that polls `/v1/clusters` and
//! `/v1/stats` and shows clusters forming. Styled from a copy of the
//! tokens in `design/tokens.css` (project rule: copy, no shared runtime
//! dependency). System sans for body text; the serif is only the lockup.

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
:root{--kg-blue:#2563EB;--kg-blue-hover:#1D4ED8;--kg-blue-soft:#DBEAFE;--kg-blue-ink:#1E3A8A;--kg-bg:#FFFFFF;--kg-bg-subtle:#F8FAFC;--kg-line:#E2E8F0;--kg-line-strong:#CBD5E1;--kg-text-muted:#64748B;--kg-text-secondary:#334155;--kg-text:#0F172A;--kg-ok:#059669;--kg-warn:#D97706;--kg-err:#DC2626;--kg-font-serif:"Instrument Serif","Iowan Old Style","Times New Roman",serif;--kg-font-sans:Inter,-apple-system,"Segoe UI",system-ui,sans-serif;--kg-font-mono:"JetBrains Mono","SF Mono",Menlo,Consolas,monospace;--kg-t-base:200ms;--kg-ease:cubic-bezier(0.2,0,0,1)}
html{background:var(--kg-bg);color:var(--kg-text)}
body{margin:0;font:15px/1.6 var(--kg-font-sans);font-feature-settings:"tnum","cv11","ss01";-webkit-font-smoothing:antialiased}
main{max-width:1200px;margin:0 auto;padding:32px 16px 96px}
header{display:flex;align-items:baseline;justify-content:space-between;gap:16px;flex-wrap:wrap;padding-bottom:16px;border-bottom:1px solid var(--kg-line)}
.lockup{display:inline-flex;align-items:baseline;gap:12px;font-size:32px}
.wordmark{font-family:var(--kg-font-serif);font-style:italic;letter-spacing:-0.01em}
.wordmark .kanji{font-family:"Noto Serif JP",serif;font-style:normal;font-size:.6em;color:var(--kg-text-muted);margin-left:.25em}
.rule{align-self:stretch;width:1px;background:var(--kg-line-strong)}
.tool{font-weight:500;font-size:16px}
.muted{color:var(--kg-text-muted);font-size:12px;letter-spacing:.01em}
.mono{font-family:var(--kg-font-mono);font-size:13px}
.stats{display:flex;gap:24px;flex-wrap:wrap;margin:16px 0 32px}
.stat b{display:block;font-weight:600;font-size:20px;line-height:1.2}
.status{display:inline-flex;align-items:center;gap:8px;font-size:13px}
.status::before{content:"";width:8px;height:8px;border-radius:50%;background:currentColor}
.status.ok{color:var(--kg-ok)}.status.warn{color:var(--kg-warn)}.status.err{color:var(--kg-err)}
.status span{color:var(--kg-text-secondary)}
h2{font-family:var(--kg-font-serif);font-weight:400;font-size:28px;line-height:1.15;margin:0 0 8px}
.controls{display:flex;gap:12px;align-items:center;flex-wrap:wrap;margin-bottom:8px}
select,input{font:400 14px var(--kg-font-sans);height:36px;padding:0 12px;border:1px solid var(--kg-line-strong);border-radius:4px;background:var(--kg-bg);color:var(--kg-text)}
select:focus,input:focus{outline:2px solid var(--kg-blue);outline-offset:0}
a{color:var(--kg-blue);text-decoration:none;text-underline-offset:3px}a:hover{text-decoration:underline}
.cluster{padding:16px 0;border-bottom:1px solid var(--kg-line)}
.cluster .title{font-size:17px;font-weight:600;line-height:1.35;margin:0 0 4px}
.cluster .meta{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.cluster .sep{color:var(--kg-line-strong)}
.origin{display:inline-flex;align-items:center;gap:6px;font-size:12px;font-weight:500;padding:1px 8px 1px 6px;border:1px solid var(--kg-line);border-radius:999px;color:var(--kg-text-secondary)}
.origin img,.outlet img{width:14px;height:14px;border-radius:3px;flex:none}
.stack{display:inline-flex}.stack:empty{display:none}.stack img{box-shadow:0 0 0 2px var(--kg-bg);background:var(--kg-bg)}.stack img+img{margin-left:-4px}
.stack img.first{box-shadow:0 0 0 2px var(--kg-bg),0 0 0 3.5px var(--kg-blue);position:relative;z-index:1;margin-right:3px}
.broke{color:var(--kg-text);font-weight:600}.first-label{font-size:10px;text-transform:uppercase;letter-spacing:.06em;color:var(--kg-blue);margin-right:2px}
.outlet{display:inline-flex;align-items:center;gap:6px;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.outlet a{color:inherit}
.spark{width:80px;height:20px;display:block}
.spark path{fill:none;stroke:var(--kg-blue);stroke-width:1.5;stroke-linejoin:round;stroke-linecap:round}
.spark .area{fill:var(--kg-blue-soft);fill-opacity:.4;stroke:none}
.items{margin:8px 0 0;padding:0;list-style:none;display:none}
.curve{display:none;margin:12px 0 4px;font-family:var(--kg-font-sans)}
.cluster.open .items,.cluster.open .curve{display:block}
.items li{display:grid;grid-template-columns:56px 160px 1fr;gap:12px;padding:8px 0;color:var(--kg-text-secondary);font-size:13px;border-top:1px solid var(--kg-bg-subtle);align-items:baseline}
.items .hl{color:var(--kg-text);font-size:14px;line-height:1.4}.items .hl a{color:inherit}
.items .alt{display:block;font-size:12px;color:var(--kg-text-muted);margin-top:2px}.items .alt a{color:inherit}
.items li.first .when{color:var(--kg-blue)}
.empty{color:var(--kg-text-muted);padding:24px 0}
@keyframes kg-fade-in{from{opacity:0}to{opacity:1}}
.enter{animation:kg-fade-in var(--kg-t-base) var(--kg-ease)}
@media (prefers-reduced-motion:reduce){.enter{animation:none}}
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
  <input id="min" type="number" min="1" value="2" style="width:80px" title="Minimum outlets">
  <span class="muted">min outlets · refreshes every 5 s ·</span>
  <select id="tz" title="Time zone for all times on this page"><option value="local">Local time</option><option value="utc">UTC</option></select>
</div>
<div id="clusters"><div class="empty">No clusters yet.</div></div>
<p class="muted" style="margin-top:48px">Machine readers: the same data is at <a href="/v1/clusters">/v1/clusters</a>, and each cluster answers <span class="mono">Accept: text/markdown</span>. See <a href="/llms.txt">/llms.txt</a>.</p>
</main>
<script>
const $ = s => document.querySelector(s);
const seen = new Set();
// Times: one zone for the whole page (local by default, UTC on request, remembered
// per browser). Short form carries the zone label and a date when not today;
// tooltips always give the full timestamp in both zones.
let tz = 'local';
try { tz = localStorage.getItem('kg-tz') || 'local'; } catch {}
const tzOpt = () => tz === 'utc' ? {timeZone: 'UTC'} : {};
const fmtT = ts => {
  if (!ts) return '—';
  const d = new Date(ts*1000), o = tzOpt();
  const day = x => x.toLocaleDateString('en-CA', o);
  const hm = d.toLocaleTimeString([], {...o, hour: '2-digit', minute: '2-digit', hourCycle: 'h23', timeZoneName: 'short'});
  return day(d) === day(new Date()) ? hm : d.toLocaleDateString([], {...o, month: 'short', day: 'numeric'}) + ' ' + hm;
};
const fullT = ts => ts ? `${new Date(ts*1000).toLocaleString([], {dateStyle: 'medium', timeStyle: 'long'})} · ${new Date(ts*1000).toISOString().replace('.000Z','Z')}` : '';
const tsSpan = ts => `<time datetime="${ts ? new Date(ts*1000).toISOString() : ''}" title="${esc(fullT(ts))}">${fmtT(ts)}</time>`;
const age = (ts, now) => { const s = Math.max(0, now - ts); return s < 3600 ? Math.round(s/60)+' min' : s < 86400 ? Math.round(s/3600)+' h' : Math.round(s/86400)+' d'; };
function spark(tl, now){
  if (!tl || tl.length < 2) return '';
  const t0 = tl[0].joined_at, span = Math.max(1, now - t0);
  const pts = tl.map((j,i)=>[80*(j.joined_at-t0)/span, 20-19*((i+1)/tl.length)]);
  const line = pts.map((p,i)=>(i?'L':'M')+p[0].toFixed(1)+','+p[1].toFixed(1)).join(' ');
  return `<svg class="spark" viewBox="0 0 80 20" preserveAspectRatio="none" aria-hidden="true"><path class="area" d="${line} L80,20 L0,20 Z"/><path d="${line}"/></svg>`;
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
  return `<svg class="curve" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" aria-label="spread curve">
    <line x1="${L}" y1="${H-B}" x2="${W}" y2="${H-B}" stroke="var(--kg-line)"/>
    <line x1="${L}" y1="4" x2="${L}" y2="${H-B}" stroke="var(--kg-line)"/>
    <path d="${step(1)}" fill="none" stroke="var(--kg-line-strong)" stroke-width="1.5"/>
    <path d="${step(2)}" fill="none" stroke="var(--kg-blue)" stroke-width="1.5"/>
    <text x="${L-4}" y="8" text-anchor="end" font-size="11" fill="var(--kg-text-muted)">${maxI}</text>
    <text x="${L-4}" y="${H-B}" text-anchor="end" font-size="11" fill="var(--kg-text-muted)">0</text>
    <text x="${W}" y="${H-2}" text-anchor="end" font-size="11" fill="var(--kg-text-muted)">${label} · ${o} outlets, ${maxI} items</text>
  </svg>`;
}
function render(list, now){
  const root = $('#clusters');
  if (!list.length) { root.innerHTML = '<div class="empty">No clusters yet.</div>'; return; }
  const open = new Set([...root.querySelectorAll('.cluster.open')].map(e=>e.dataset.id));
  root.innerHTML = list.map(c => {
    const v = c.velocity || {};
    const fresh = !seen.has(c.id);
    seen.add(c.id);
    const items = compare(c);
    return `<article class="cluster${fresh?' enter':''}${open.has(String(c.id))?' open':''}" data-id="${c.id}">
      <p class="title"><a href="/v1/clusters/${c.id}" onclick="this.closest('.cluster').classList.toggle('open');return false">${esc(c.primary_title)}</a></p>
      <div class="meta muted">
        ${outletStack(c)}
        ${[
          c.item_count > c.outlet_count ? n(c.item_count,'item') : '',
          v.items_1h && v.items_1h < c.item_count ? `${v.items_1h} new in the last hour` : '',
          c.syndication && c.syndication.length ? n(c.syndication.length,'syndication group') : '',
          c.last_active_at - c.created_at > 60 ? `since ${tsSpan(c.created_at)}, updated <span title="${esc(fullT(c.last_active_at))}">${age(c.last_active_at, now)} ago</span>` : tsSpan(c.created_at),
        ].filter(Boolean).map(t=>`<span class="sep">·</span><span>${t}</span>`).join('')}
        ${spark(c.timeline, now)}
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
const outletTag = (id, cls) => {
  const o = outlets[id] || {};
  const icon = o.domain ? `<img src="https://icons.duckduckgo.com/ip3/${encodeURIComponent(o.domain)}.ico" alt="" loading="lazy" onerror="this.remove()">` : '';
  const nm = o.ledger ? `<a href="${esc(o.ledger)}" title="Sourced facts about ${esc(o.name || id)} in the ledger">${esc(o.name || id)}</a>` : esc(o.name || id);
  return `<span class="${cls}" title="${esc(o.domain || id)}" data-outlet="${esc(id)}">${icon}${nm}</span>`;
};
// Every outlet in the cluster, origin first: icons overlap, first two named, rest as +N.
const outletStack = c => {
  const ids = [...new Set([c.origin_outlet_id, ...[...(c.items||[])].sort((a,b)=>at(a)-at(b)).map(i=>i.outlet_id)])];
  const icons = ids.slice(0, 8).map((id, k) => { const o = outlets[id] || {};
    return o.domain ? `<img${k ? '' : ' class="first"'} src="https://icons.duckduckgo.com/ip3/${encodeURIComponent(o.domain)}.ico" alt="" title="${esc(o.name||id)}${k ? '' : ' · broke the story'}" loading="lazy" onerror="this.remove()">` : ''; }).join('');
  const names = ids.slice(0, 2).map((id, k) => { const nm = esc((outlets[id]||{}).name || id);
    return k ? nm : `<b class="broke">${nm}</b> <span class="first-label">first</span>`; }).join('<span class="sep">·</span>');
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
  return [...by].map(([id, v], k) => {
    const d = at(v[0]) - t0;
    const when = k === 0 ? 'first' : '+' + (d < 3600 ? Math.round(d/60) + 'm' : (d/3600).toFixed(1) + 'h');
    const alts = v.slice(1).filter(i => i.title !== v[0].title).map(i => `<span class="alt">also: ${link(i)}</span>`).join('');
    return `<li${k ? '' : ' class="first"'}><span class="mono when" title="${esc(`Picked up ${fullT(at(v[0]))}` + (v[0].published_at ? `\nOutlet says published ${fullT(v[0].published_at)}` : ''))}">${when}</span>${outletTag(id,'outlet')}<span class="hl">${link(v[0])}${alts}</span></li>`;
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
      ['feeds', `${s.feeds_total} <span class="muted">${s.feeds_dead} dead</span>`],
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
