# lens — Presentation Layer

Pillar 3. `lens` is what a person or a bot visits. It composes what
`feed` records (stories, who published first, who joined when, what
was copied) with what `ledger` holds (sourced facts about each outlet)
into pages. It adds nothing of its own: no score, no lean colour, no
"trending". Where a news aggregator shows a story with coverage grouped by
lean, `lens` shows the story with coverage grouped by what a named
source says, and shows the source.

The project rules in `feed/docs/plan.md` §1 apply. Rule 6 restated for
this layer:

> `lens` arranges. It does not rank outlets, weigh sources, or
> summarise in its own voice. Every number on a page is a count from
> `feed`; every label is a claim from `ledger` with its provenance
> line. If two sources disagree, both are on the page.

The design rules in `design/DESIGN.md` apply in full, §10 (one
language across surfaces) and §11 (machine readers) especially.

## 1. Why a static site

`lens` is a generator, not a third server. `lens build` reads the
exports the other two tools already produce and writes a directory of
HTML, JSON and Markdown files. Reasons:

- **Cheapest possible to serve.** Static files cost nothing on any
  host, cache perfectly, and need no process (INFRA-PLAN Tier 3 is a
  static frontend plus a tunnel to the API; this removes the tunnel
  for the pages).
- **Best shape for machine readers.** Every page's JSON and Markdown
  twin is a file next to it, addressable without negotiation, and the
  whole site is one `sitemap.xml` away.
- **Deterministic.** Same exports in, same site out (rule 4). The site
  can be tested offline from the same recorded fixtures as `feed`.
- **Keeps the live servers on loopback.** The APIs stay where the
  infra plan wants them.

The nightly job already exports; `lens build` runs after it. A story
page is therefore up to a day old. That is acceptable for the first
version: the propagation record of a story is the product, and it is
most useful after the story has spread. A live mode (a small server
that renders the same templates from the APIs) is deferred to N4 and
only if the daily cadence turns out to matter.

## 2. Inputs

From `feed export --format ndjson`: `items`, `clusters` (with
`timeline`, `syndication`, `expired`), `events`, `outlets`, and
`revisions` when present (older exports lack it; the story page then
says the earlier text is not in the export).

From `ledger export`: `claims`, `sources`, `outlets`.

The join is the outlet id, the one thing the two share. `lens` reads
both directories and nothing else. It never fetches the network; the
favicons the debug page pulls from a third party are dropped here
(design §10: outlets shown by name, never by an icon that could carry
a rating, and a build must not depend on a third-party service).

## 3. Pages

Five page kinds. Each has a JSON twin (`<page>.json`) and a Markdown
twin (`<page>.md`) written beside it, linked from `<head>` as
alternates, with the same facts in the same order.

### 3.1 Front page `/`

Stories from the last 24 hours (configurable), each a cluster card:
primary title, outlet count, item count, "first: <outlet>", first-seen
time, sparkline of joins, and the counts that placed it. Ordered by
reach (`docs/algorithms.md` §4): independent reports in the window plus
languages minus one, then outlets in the last 6 hours, then recency.
Stories with one independent report are listed apart, newest first. A
second list, "spreading now", ordered by outlets in the last 6 hours,
shown only when it differs from the top. No other ranking. A line at
the top states the window, the outlet pool size, the export time and
the rule, so the page says what it covers and how it is ordered.

### 3.2 Story page `/story/<cluster id>/`

The cluster card, then:

- **Timeline.** Every member in join order: outlet, headline, published
  time (as the outlet claimed) and picked-up time (as `feed` saw). The
  first is marked. Later rewrites from the same outlet sit under its
  first headline, so framings can be compared side by side.
- **Copies.** Syndication groups: which outlets carried the same text
  and who had it first. Labelled as copies, not coverage.
- **Changes.** Revisions and withdrawals from the event log, with the
  before and after text.
- **Coverage by source label.** For each `ledger` field that a source
  provides for enough of the outlets present (reliability from
  Wikipedia's perennial list, bias from AllSides if its terms allow),
  a table of that source's labels against the outlets in the story,
  headed with the source's name and licence. Outlets the source has no
  label for are listed as "not rated by <source>". This is the
  blindspot view done honestly: it is the source's map, and we say so.
- **Method.** `feed`'s own judgement of the story as counts
  (`docs/steward.md` §3): threshold, weakest join and margin, unscored
  members, merged, languages. Absent confidence is said, not zeroed.
- **About the outlets.** One claim card per outlet for its most useful
  facts (owner, country, founded, domain registered), each with its
  provenance line, linking to the outlet page.

### 3.3 Outlet page `/outlet/<id>/`

The ledger's claims grouped by field, every claim with its provenance
line (the lookup page's rendering, made static), then the outlet's
stories: recent clusters it was in, how many it was first to, how many
of its items were copies of other outlets' text and how many were
copied. Those are counts from `feed` with the window stated, not a
rating.

### 3.4 Search `/search/`

A single page with a prebuilt index (outlet names, ids, domains; story
titles for the window) queried in the browser. No server. The index is
a JSON file, itself a machine-readable listing.

### 3.5 About `/about/`

What Kagami is, what each number means, what `lens` does not do, the
list of sources with licences (from `ledger`'s registry), the contract
version, and the rules of use for machine readers. Generated, so it is
never out of date with the data.

## 4. Derived at build time

Everything `lens` computes is a count or a join. Named here so nothing
else creeps in:

- Per cluster: outlets in the last 6 h and 24 h (from the timeline),
  join sparkline points, syndication group membership, revision and
  withdrawal events.
- Per outlet: clusters in the window, first-to count, copy counts in
  both directions. The same definitions as `ledger`'s
  `feed_behaviour` importer, so the numbers agree.
- Per story and source: the label table in §3.2.
- Search index.

Not derived, ever: any per-outlet number that combines sources, any
ordering of outlets other than alphabetical or by join time, any
summary text.

## 5. Machine readers

Design §11 applied to a static site:

- `/llms.txt` describing the site, the page kinds, the twins, and the
  rules of use.
- `/robots.txt` allowing everything; `/sitemap.xml` listing every
  page.
- Twins as files: `/story/<id>/index.json`, `/story/<id>/index.md`,
  and the same for outlets and the front page.
- JSON-LD in each outlet page: `schema.org/Organization` with `name`,
  `url`, and `sameAs` pointing at the Wikidata entity and Wikipedia
  article when `ledger` has them. No `Rating` type, ever. Story pages
  carry an `ItemList` of the items' URLs with `datePublished`; `lens`
  does not host article text, so no `NewsArticle`.
- Stable addresses: cluster ids and outlet ids are the contract;
  `/story/<id>/` keeps working across rebuilds while the cluster is in
  the export. Merged clusters redirect to the survivor via a stub page.
- `data-id`, `data-outlet`, `data-source` attributes on cards, `time`
  elements for every timestamp, text labels for every status.

## 6. Repository layout

```
lens/
  Cargo.toml
  docs/plan.md
  src/
    main.rs        CLI: build | serve | check
    input.rs       read the two exports, join on outlet id
    derive.rs      §4, with tests against feed's replay fixtures
    pages/         one module per page kind; each writes html, json, md
    render.rs      the HTML skeleton, the two components, the twins
    tokens.css     copy of design/tokens.css (project rule: copy)
    llms.rs        llms.txt, robots.txt, sitemap.xml
  fixtures/
    feed_export/   a recorded feed export (shared with ledger's)
    ledger_export/ a recorded ledger export
    expected/      golden site for the fixtures, diffed in tests
```

No templating crate: pages are Rust functions building strings, the
way the debug and lookup pages already are. No JavaScript except the
search page's query over its index and the time-zone control.

## 7. CLI

```
lens build --feed-export <dir> --ledger-export <dir> --out <dir> [--window <hours>] [--base-url <url>]
lens serve --out <dir> [--bind]        static file server for local review
lens check --out <dir>                 every internal link resolves, every page has its twins
```

`build` is idempotent and writes atomically (new directory, then
rename), so a half-built site is never served. `--base-url` is only
for `sitemap.xml` and JSON-LD; pages use relative links.

## 8. Design

Two objects, from `design/DESIGN.md` §5: the cluster card and the
claim card with its provenance line. `lens` adds three layouts, not
three objects: the timeline list, the label table, and the outlet
story list. The chrome is the shared lockup, `Kagami` │ lens. Fonts as
in `design/fonts/README.md`, self-hosted; until they are fetched the
fallbacks apply.

## 9. Milestones

- **N0** crate, `input.rs`, `derive.rs` against the recorded exports.
  Done: the join and the counts, pinned to ledger's feed_behaviour
  numbers.
- **N1** Story page and outlet page, with twins; `lens check`,
  `lens serve`, a first front page and `llms.txt`. Done: `lens build`
  on the fixtures produces a site that `check` passes and a test diffs
  three twins against golden copies.
- **N2** Front page with the spreading-now list, search, about,
  sitemap, JSON-LD on front and story and outlet pages. Done offline;
  the first real build over a week of live data needs the local
  runbook.
- **N3** Coverage by source label. The label bar is built and shows
  Wikipedia's reliability verdicts on any story where the list labels
  at least two outlets; AllSides bias slots into the same bar once its
  terms are settled. Merged-cluster stubs done: an absorbed id keeps
  its address with a canonical link and refresh to the survivor.
- **N4** Live mode, only if the daily cadence is a problem in
  practice.

N0 and N1 can be built and verified offline now. N2 needs real data to
be judged, not to be built.

## 10. What it needs from the other pillars

- `feed`: nothing new for the static site; the export carries
  everything. Live mode would want a stories-by-outlet route.
- `ledger`: `export` includes the source registry; its outlet list has
  ids and claim counts only, so names and domains come from `feed`'s
  export, which is the registry. Ownership
  chains (ledger plan §9) are not needed for N1; the outlet page shows
  the direct owner claim and links to the owner's outlet page when the
  owner is itself an outlet in the registry.
- `design`: a "label table" component once N3 is designed; until then
  the plain table style.

## 11. Open questions

- **Coverage counting** (feed plan §12). Count items or syndication
  groups per outlet? `lens` shows both numbers on the story page and
  ranks the front page by outlets, which sidesteps it. Revisit when
  live data shows how much syndication inflates item counts.
- **Publish-date trust.** The timeline shows both times. The "first"
  marker follows `feed`'s origin (published time). If live data shows
  feeds lying badly, the story page should say when the two disagree
  by more than an hour rather than pick a side.
- **Blindspot without lean labels.** Until AllSides is settled the
  label table has one source, Wikipedia's reliability list, which
  covers a few hundred outlets. Whether a one-source table is useful
  or misleading is a judgement to make on real pages.
- **Window.** 24 hours on the front page and 30 days of stories on
  outlet pages are guesses. Tune on real data.
- **Site size.** A year of clusters is hundreds of thousands of story
  pages. Keep the last 30 days as pages and older stories as JSON only,
  or let the site grow? Decide when it is measured.
