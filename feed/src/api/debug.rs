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
.origin{font-family:var(--kg-font-mono);font-size:12px;padding:1px 6px;border:1px solid var(--kg-line);border-radius:4px;color:var(--kg-text-secondary)}
.spark{width:80px;height:20px;display:block}
.spark path{fill:none;stroke:var(--kg-blue);stroke-width:1.5;stroke-linejoin:round;stroke-linecap:round}
.spark .area{fill:var(--kg-blue-soft);fill-opacity:.4;stroke:none}
.items{margin:8px 0 0;padding:0;list-style:none;display:none}
.curve{display:none;margin:12px 0 4px;font-family:var(--kg-font-sans)}
.cluster.open .items,.cluster.open .curve{display:block}
.items li{display:grid;grid-template-columns:72px 110px 1fr;gap:12px;padding:4px 0;color:var(--kg-text-secondary);font-size:13px;border-top:1px solid var(--kg-bg-subtle)}
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
  <input id="min" type="number" min="1" value="1" style="width:80px" title="Minimum outlets">
  <span class="muted">min outlets · refreshes every 5 s · times in UTC</span>
</div>
<div id="clusters"><div class="empty">No clusters yet.</div></div>
</main>
<script>
const $ = s => document.querySelector(s);
const seen = new Set();
const fmtT = ts => ts ? new Date(ts*1000).toISOString().slice(11,16) + ' UTC' : '—';
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
    const items = (c.items||[]).map(i=>`<li><span class="mono">${fmtT(i.published_at)}</span><span class="mono">${i.outlet_id}</span><span>${esc(i.title)}</span></li>`).join('');
    return `<article class="cluster${fresh?' enter':''}${open.has(String(c.id))?' open':''}" data-id="${c.id}">
      <p class="title"><a href="/v1/clusters/${c.id}" onclick="this.closest('.cluster').classList.toggle('open');return false">${esc(c.primary_title)}</a></p>
      <div class="meta muted">
        <span>${n(c.outlet_count,'outlet')}</span><span class="sep">·</span>
        <span>${n(c.item_count,'item')}</span><span class="sep">·</span>
        <span>${v.items_1h||0} in the last hour</span><span class="sep">·</span>
        ${c.syndication && c.syndication.length ? `<span>${c.syndication.length} syndication groups</span><span class="sep">·</span>` : ''}
        <span>first seen ${fmtT(c.created_at)}</span><span class="sep">·</span>
        <span>active ${age(c.last_active_at, now)} ago</span><span class="sep">·</span>
        <span class="origin">${esc(c.origin_outlet_id)}</span>
        ${spark(c.timeline, now)}
      </div>
      ${curve(c.timeline, now)}
      <ul class="items">${items}</ul>
    </article>`;
  }).join('');
}
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const n = (k, word) => `${k} ${word}${k === 1 ? '' : 's'}`;
async function tick(){
  try {
    const sort = $('#sort').value, min = $('#min').value || 1;
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
</script>
</body>
</html>
"##;
