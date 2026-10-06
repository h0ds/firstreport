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
<title>Truth Decay ledger lookup</title>
<meta name="description" content="Sourced facts about news outlets, each with its source, licence and date. Never a score.">
<link rel="alternate" type="application/json" href="/v1/outlets" title="Outlets with claims (JSON)">
<link rel="alternate" type="text/plain" href="/llms.txt" title="How to read this server">
<style>
:root{--kg-blue:#2563EB;--kg-blue-hover:#1D4ED8;--kg-blue-soft:#DBEAFE;--kg-blue-ink:#1E3A8A;--kg-bg:#FFFFFF;--kg-bg-subtle:#F8FAFC;--kg-line:#E2E8F0;--kg-line-strong:#CBD5E1;--kg-text-muted:#64748B;--kg-text-secondary:#334155;--kg-text:#0F172A;--kg-ok:#059669;--kg-warn:#D97706;--kg-err:#DC2626;--kg-font-serif:"Instrument Serif", "Iowan Old Style", "Times New Roman", serif;--kg-font-sans:Inter, -apple-system, "Segoe UI", system-ui, sans-serif;--kg-font-mono:"JetBrains Mono", "SF Mono", Menlo, Consolas, monospace;--kg-fs-small:12px;--kg-fs-ui:13px;--kg-fs-body:15px;--kg-fs-body-lg:17px;--kg-fs-h3:20px;--kg-fs-h2:28px;--kg-lh-small:16px;--kg-lh-ui:20px;--kg-lh-body:24px;--kg-lh-body-lg:24px;--kg-lh-h3:28px;--kg-lh-h2:32px;--kg-lockup-wordmark:28px;--kg-lockup-kanji:17px;--kg-s1:4px;--kg-s2:8px;--kg-s3:12px;--kg-s4:16px;--kg-s5:24px;--kg-s6:32px;--kg-s7:48px;--kg-s8:64px;--kg-s9:96px;--kg-radius-input:4px;--kg-t-fast:120ms;--kg-t-base:200ms;--kg-ease:cubic-bezier(0.2, 0, 0, 1)}
html{background:var(--kg-bg);color:var(--kg-text)}
body{margin:0;font:400 var(--kg-fs-body)/var(--kg-lh-body) var(--kg-font-sans);font-feature-settings:"tnum","cv11","ss01";-webkit-font-smoothing:antialiased}
p,ul,ol{margin:0}
main{max-width:720px;margin:0 auto;padding:var(--kg-s6) var(--kg-s5) var(--kg-s9)}
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
#q{width:100%;margin-top:var(--kg-s5)}
.hits{list-style:none;margin:var(--kg-s2) 0 0;padding:0}
.hits li{display:flex;gap:var(--kg-s3);align-items:baseline;padding:var(--kg-s2) 0;border-bottom:1px solid var(--kg-line)}
.hits li>a{font-weight:600;color:var(--kg-text)}.hits li>a:hover{color:var(--kg-blue)}
.hits .dom{color:var(--kg-text-muted);font-size:var(--kg-fs-small);line-height:var(--kg-lh-small)}
.hits .n{margin-left:auto;font-size:var(--kg-fs-small);line-height:var(--kg-lh-small);color:var(--kg-text-muted);white-space:nowrap}
.field{display:grid;grid-template-columns:176px 1fr;gap:var(--kg-s4);padding:var(--kg-s3) 0;border-bottom:1px solid var(--kg-line)}
.field .fname{font-size:var(--kg-fs-ui);line-height:var(--kg-lh-body);font-weight:500;color:var(--kg-text-secondary)}
.claim+.claim{margin-top:var(--kg-s3)}
.claim .value{font-size:var(--kg-fs-body);line-height:var(--kg-lh-body);font-weight:600;color:var(--kg-text);overflow-wrap:anywhere}
.claim .value .v+.v::before{content:" · ";color:var(--kg-text-muted);font-weight:400}
@media (max-width:720px){.field{grid-template-columns:1fr;gap:var(--kg-s1)}}
.claim .scope{color:var(--kg-text-secondary);font-size:var(--kg-fs-ui);line-height:var(--kg-lh-ui);margin-top:var(--kg-s1)}
.claim .provenance{display:flex;gap:var(--kg-s2);flex-wrap:wrap;align-items:center;margin-top:var(--kg-s1)}
.summary{color:var(--kg-text-secondary);font-size:var(--kg-fs-ui);line-height:var(--kg-lh-ui);margin-top:var(--kg-s1)}
</style>
</head>
<body>
<main>
<header>
  <span class="lockup"><span class="wordmark">Truth Decay</span><span class="rule"></span><span class="tool">ledger</span></span>
  <span class="muted" id="counts"></span>
</header>
<input id="q" type="search" placeholder="Outlet name, id or domain" autocomplete="off" autofocus>
<div id="hits"></div>
<div id="outlet"></div>
<p class="muted quiet foot">Machine readers: the same data is at <a href="/v1/outlets">/v1/outlets</a>, and each outlet answers <span class="mono">Accept: text/markdown</span>. See <a href="/llms.txt">/llms.txt</a>.</p>
</main>
<script>
const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const date = ts => new Date(ts*1000).toISOString().slice(0,10);
const n = (k, w) => `${k} ${w}${k === 1 ? '' : 's'}`;
let sources = {};
const FIELD_ORDER = ['reliability','bias','owner','parent','legal_form','country','founded','website','domain_registered','registrar','site_https','site_pages','posting_cadence','origin_share','syndication_share','revision_rate','withdrawal_rate','wikipedia','name','wikidata_id'];
const FIELD_LABEL = {reliability:'Reliability', bias:'Bias', owner:'Owner', parent:'Parent organisation', legal_form:'Legal form', country:'Country', founded:'Founded', website:'Website', domain_registered:'Domain registered', registrar:'Registrar', site_https:'HTTPS', site_pages:'Accountability pages', posting_cadence:'Posting cadence', origin_share:'First to shared stories', syndication_share:'Syndication', revision_rate:'Revisions', withdrawal_rate:'Withdrawals', wikipedia:'Wikipedia', name:'Name in source', wikidata_id:'Wikidata'};

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
  let html = `<h2 data-id="${esc(id)}">${esc(name)}</h2><div class="muted">${n(o.claims,'claim')}${o.stale ? ` · ${o.stale} past their source's refresh interval` : ' · all within their refresh intervals'} · every value is a source's, never ours</div>`;
  // One row per field. Claims in a field whose provenance is identical
  // (same source, page, retrieval day, as-of and age) share one
  // provenance line and list their values together; claims from
  // different sources are never merged (design §5).
  // A note that applies to several fields (which Wikidata item matched,
  // say) is shown at its first appearance only.
  const seenNotes = new Set();
  for (const f of fields) {
    const groups = new Map();
    for (const c of o.fields[f]) {
      const k = [c.source_id, c.ref_url, date(c.retrieved_at), c.as_of || '', c.stale, c.age_days].join('|');
      if (!groups.has(k)) groups.set(k, []);
      groups.get(k).push(c);
    }
    html += `<section class="field enter" data-field="${esc(f)}"><div class="fname">${esc(FIELD_LABEL[f] || f)}</div><div>`;
    for (const cs of groups.values()) {
      const c = cs[0];
      const s = sources[c.source_id] || {name: c.source_id, licence: ''};
      const extra = cs.map(c => {
        const d = c.data || {};
        return [
          d.scope && d.scope !== c.value ? `<div class="scope">${esc(d.scope)}</div>` : '',
          d.summary ? `<div class="summary">${esc(d.summary)}</div>` : '',
          d.window_days ? `<div class="summary">Measured over ${d.window_days} days to ${esc(d.window_end)}, ${n(d.items,'item')}. A measurement, not a verdict.</div>` : '',
          d.domain ? `<div class="summary">${esc(d.domain)}${d.expires ? `, expires ${esc(d.expires)}` : ''}${Array.isArray(d.status) && d.status.length ? `, ${esc(d.status.join(', '))}` : ''}</div>` : '',
          c.field === 'site_pages' ? `<div class="summary quiet">${['about','contact','privacy','corrections','ethics','masthead'].filter(k=>d[k]).map(k=>`<a href="${esc(d[k])}" rel="noopener">${k}</a>`).join(' · ') || 'none linked'}</div>` : '',
          c.note && !seenNotes.has(c.note) && seenNotes.add(c.note) ? `<div class="summary">Note: ${esc(c.note)}</div>` : '',
          Array.isArray(d.discussions) && d.discussions.length ? `<div class="summary quiet">Discussions: ${d.discussions.map((u,i)=>`<a href="${esc(u)}" rel="noopener">${i+1}</a>`).join(' ')}</div>` : '',
        ].join('');
      });
      // Identical notes under a shared line are said once.
      const notes = [...new Set(extra.filter(Boolean))].join('');
      html += `<article class="claim" data-outlet="${esc(id)}" data-field="${esc(f)}" data-source="${esc(c.source_id)}">
        <div class="value">${cs.map(c => `<span class="v">${esc(c.value)}</span>`).join('')}</div>
        <div class="provenance muted quiet">
          <a href="${esc(c.ref_url)}" rel="noopener">${esc(s.name)}</a><span class="sep">·</span>
          <span class="tag">${esc(s.licence)}</span><span class="sep">·</span>
          <span>retrieved <time datetime="${date(c.retrieved_at)}">${date(c.retrieved_at)}</time></span>
          ${c.as_of ? `<span class="sep">·</span><span>as of ${esc(c.as_of)}</span>` : ''}
          <span class="sep">·</span>
          <span class="status ${c.stale ? 'warn' : 'ok'}"><span>${c.stale ? `stale, ${c.age_days} days old` : `${c.age_days} days old`}</span></span>
        </div>${notes}
      </article>`;
    }
    html += `</div></section>`;
  }
  $('#outlet').innerHTML = html;
}
init();
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
