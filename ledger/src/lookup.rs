//! `/lookup`: type an outlet, see every claim about it grouped by field
//! with its source, licence, retrieval date and a stale marker. Built
//! from a copy of the design tokens (project rule: copy, no shared
//! runtime dependency). Not part of the API contract; it moves into
//! `lens` later.

pub const PAGE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Kagami ledger lookup</title>
<style>
:root{--kg-blue:#2563EB;--kg-blue-hover:#1D4ED8;--kg-blue-soft:#DBEAFE;--kg-blue-ink:#1E3A8A;--kg-bg:#FFFFFF;--kg-bg-subtle:#F8FAFC;--kg-line:#E2E8F0;--kg-line-strong:#CBD5E1;--kg-text-muted:#64748B;--kg-text-secondary:#334155;--kg-text:#0F172A;--kg-ok:#059669;--kg-warn:#D97706;--kg-err:#DC2626;--kg-font-serif:"Instrument Serif","Iowan Old Style","Times New Roman",serif;--kg-font-sans:Inter,-apple-system,"Segoe UI",system-ui,sans-serif;--kg-font-mono:"JetBrains Mono","SF Mono",Menlo,Consolas,monospace;--kg-t-base:200ms;--kg-ease:cubic-bezier(0.2,0,0,1)}
html{background:var(--kg-bg);color:var(--kg-text)}
body{margin:0;font:15px/1.6 var(--kg-font-sans);font-feature-settings:"tnum","cv11","ss01";-webkit-font-smoothing:antialiased}
main{max-width:720px;margin:0 auto;padding:32px 16px 96px}
header{display:flex;align-items:baseline;justify-content:space-between;gap:16px;flex-wrap:wrap;padding-bottom:16px;border-bottom:1px solid var(--kg-line)}
.lockup{display:inline-flex;align-items:baseline;gap:12px;font-size:32px}
.wordmark{font-family:var(--kg-font-serif);font-style:italic;letter-spacing:-0.01em}
.wordmark .kanji{font-family:"Noto Serif JP",serif;font-style:normal;font-size:.6em;color:var(--kg-text-muted);margin-left:.25em}
.rule{align-self:stretch;width:1px;background:var(--kg-line-strong)}
.tool{font-weight:500;font-size:16px}
.muted{color:var(--kg-text-muted);font-size:12px;letter-spacing:.01em}
.mono{font-family:var(--kg-font-mono);font-size:13px}
h2{font-family:var(--kg-font-serif);font-weight:400;font-size:28px;line-height:1.15;margin:32px 0 4px}
h3{font-size:20px;font-weight:600;margin:24px 0 8px}
input{font:400 14px var(--kg-font-sans);height:36px;padding:0 12px;border:1px solid var(--kg-line-strong);border-radius:4px;background:var(--kg-bg);color:var(--kg-text);width:100%;box-sizing:border-box;margin-top:24px}
input:focus{outline:2px solid var(--kg-blue);outline-offset:0}
a{color:var(--kg-blue);text-decoration:none;text-underline-offset:3px}a:hover{text-decoration:underline}
.hits{list-style:none;margin:8px 0 0;padding:0}
.hits li{display:flex;gap:12px;align-items:baseline;padding:8px 0;border-bottom:1px solid var(--kg-line)}
.hits li a{font-weight:600}
.hits .dom{color:var(--kg-text-muted);font-size:13px}
.hits .n{margin-left:auto;font-size:12px;color:var(--kg-text-muted);white-space:nowrap}
.claim{padding:12px 0;border-bottom:1px solid var(--kg-line)}
.claim .value{font-size:17px;font-weight:600;line-height:1.35}
.claim .scope{color:var(--kg-text-secondary);font-size:13px;margin-top:2px}
.claim .meta{display:flex;gap:8px;flex-wrap:wrap;align-items:center;margin-top:4px}
.claim .sep{color:var(--kg-line-strong)}
.tag{font-family:var(--kg-font-mono);font-size:12px;padding:1px 6px;border:1px solid var(--kg-line);border-radius:4px;color:var(--kg-text-secondary)}
.status{display:inline-flex;align-items:center;gap:8px;font-size:12px}
.status::before{content:"";width:8px;height:8px;border-radius:50%;background:currentColor}
.status.ok{color:var(--kg-ok)}.status.warn{color:var(--kg-warn)}
.status span{color:var(--kg-text-secondary)}
.summary{color:var(--kg-text-secondary);font-size:13px;margin-top:6px}
.empty{color:var(--kg-text-muted);padding:24px 0}
.enter{animation:kg-fade-in var(--kg-t-base) var(--kg-ease)}
@keyframes kg-fade-in{from{opacity:0}to{opacity:1}}
@media (prefers-reduced-motion:reduce){.enter{animation:none}}
</style>
</head>
<body>
<main>
<header>
  <span class="lockup"><span class="wordmark">Kagami<span class="kanji">鏡</span></span><span class="rule"></span><span class="tool">ledger</span></span>
  <span class="muted" id="counts"></span>
</header>
<input id="q" type="search" placeholder="Outlet name, id or domain" autocomplete="off" autofocus>
<div id="hits"></div>
<div id="outlet"></div>
</main>
<script>
const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const date = ts => new Date(ts*1000).toISOString().slice(0,10);
const n = (k, w) => `${k} ${w}${k === 1 ? '' : 's'}`;
let sources = {};
const FIELD_ORDER = ['reliability','bias','owner','parent','legal_form','country','founded','website','domain_registered','registrar','posting_cadence','origin_share','syndication_share','revision_rate','withdrawal_rate','wikipedia','name','wikidata_id'];
const FIELD_LABEL = {reliability:'Reliability', bias:'Bias', owner:'Owner', parent:'Parent organisation', legal_form:'Legal form', country:'Country', founded:'Founded', website:'Website', domain_registered:'Domain registered', registrar:'Registrar', posting_cadence:'Posting cadence', origin_share:'First to shared stories', syndication_share:'Syndication', revision_rate:'Revisions', withdrawal_rate:'Withdrawals', wikipedia:'Wikipedia', name:'Name in source', wikidata_id:'Wikidata'};

async function init(){
  const [src, st] = await Promise.all([fetch('/v1/sources').then(r=>r.json()), fetch('/v1/stats').then(r=>r.json())]);
  for (const s of src) sources[s.id] = s;
  $('#counts').textContent = `${n(st.outlets,'outlet')} with claims · ${n(st.claims,'claim')} · ${n(st.sources,'source')}`;
  const q = new URLSearchParams(location.search).get('q');
  if (q) { $('#q').value = q; search(q); }
}
let t;
$('#q').addEventListener('input', e => { clearTimeout(t); t = setTimeout(() => search(e.target.value), 150); });
$('#q').addEventListener('keydown', e => { if (e.key === 'Enter') { const a = $('#hits a'); if (a) a.click(); } });

async function search(q){
  $('#outlet').innerHTML = '';
  if (!q.trim()) { $('#hits').innerHTML = ''; return; }
  history.replaceState(null, '', '?q=' + encodeURIComponent(q));
  const hits = await fetch('/v1/search?q=' + encodeURIComponent(q)).then(r=>r.json());
  if (!hits.length) { $('#hits').innerHTML = '<div class="empty">No outlet in the registry matches that. Add it to feed\'s outlets.toml first.</div>'; return; }
  $('#hits').innerHTML = '<ul class="hits">' + hits.map(h => `<li class="enter"><a href="#" data-id="${esc(h.id)}">${esc(h.name || h.id)}</a><span class="dom">${esc(h.domains.join(', '))}</span><span class="n">${h.claims ? n(h.claims,'claim') + (h.stale ? `, ${h.stale} stale` : '') : 'not assessed yet'}</span></li>`).join('') + '</ul>';
  for (const a of document.querySelectorAll('#hits a')) a.onclick = e => { e.preventDefault(); show(a.dataset.id, a.textContent); };
  if (hits.length === 1) show(hits[0].id, hits[0].name || hits[0].id);
}

async function show(id, name){
  const r = await fetch('/v1/outlets/' + encodeURIComponent(id));
  if (r.status === 404) { $('#outlet').innerHTML = `<h2>${esc(name)}</h2><div class="empty">No claims yet. Run <span class="mono">ledger import wikidata</span> and <span class="mono">ledger import wikipedia_rsp</span> with this outlet in the registry.</div>`; return; }
  const o = await r.json();
  const fields = Object.keys(o.fields).sort((a,b) => (FIELD_ORDER.indexOf(a)+1||99) - (FIELD_ORDER.indexOf(b)+1||99));
  let html = `<h2>${esc(name)}</h2><div class="muted">${n(o.claims,'claim')}${o.stale ? ` · ${o.stale} past their source's refresh interval` : ' · all within their refresh intervals'} · every value is a source's, never ours</div>`;
  for (const f of fields) {
    html += `<h3>${esc(FIELD_LABEL[f] || f)}</h3>`;
    for (const c of o.fields[f]) {
      const s = sources[c.source_id] || {name: c.source_id, licence: ''};
      const d = c.data || {};
      html += `<div class="claim enter">
        <div class="value">${esc(c.value)}</div>
        ${d.scope && d.scope !== c.value ? `<div class="scope">${esc(d.scope)}</div>` : ''}
        <div class="meta muted">
          <a href="${esc(c.ref_url)}" rel="noopener">${esc(s.name)}</a><span class="sep">·</span>
          <span class="tag">${esc(s.licence)}</span><span class="sep">·</span>
          <span>retrieved ${date(c.retrieved_at)}</span>
          ${c.as_of ? `<span class="sep">·</span><span>as of ${esc(c.as_of)}</span>` : ''}
          <span class="sep">·</span>
          <span class="status ${c.stale ? 'warn' : 'ok'}"><span>${c.stale ? `stale, ${c.age_days} days old` : `${c.age_days} days old`}</span></span>
        </div>
        ${d.summary ? `<div class="summary">${esc(d.summary)}</div>` : ''}
        ${d.window_days ? `<div class="summary">Measured over ${d.window_days} days to ${esc(d.window_end)}, ${n(d.items,'item')}. A measurement, not a verdict.</div>` : ''}
        ${d.domain ? `<div class="summary">${esc(d.domain)}${d.expires ? `, expires ${esc(d.expires)}` : ''}${Array.isArray(d.status) && d.status.length ? `, ${esc(d.status.join(', '))}` : ''}</div>` : ''}
        ${c.note ? `<div class="summary">Note: ${esc(c.note)}</div>` : ''}
        ${Array.isArray(d.discussions) && d.discussions.length ? `<div class="summary">Discussions: ${d.discussions.map((u,i)=>`<a href="${esc(u)}" rel="noopener">${i+1}</a>`).join(' ')}</div>` : ''}
      </div>`;
    }
  }
  $('#outlet').innerHTML = html;
}
init();
</script>
</body>
</html>
"##;
