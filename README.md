# First Report

An open-source news comparison site, built as a disinformation-analysis
network: where did a story originate, how fast did it spread, who amplified it,
was it syndicated or independently reported.

The project is split into two layers.

## Intelligence layer

Rust, one binary per tool, runs entirely locally. Four pillars, built in
order, and one command over them:

| Tool | Pillar | Job |
|---|---|---|
| [`feed/`](feed/) | 1 | Ingest headlines, cluster them into stories, record propagation. Neutral by construction. |
| [`ledger/`](ledger/) | 2 | Sourced facts about outlets keyed by outlet id: ownership, country, founding, third-party ratings, observed behaviour, each with its source and licence. Never a score. |
| [`lens/`](lens/) | 3 | Static site generated from `feed` and `ledger` exports: stories, outlets, coverage by a named source's labels. |
| [`reply/`](reply/) | 4 | A social account that answers "prove it" with sourced provenance from `feed` and `ledger`, never a verdict. |
| [`firstreport/`](firstreport/) | | One command over the pillars: `run`, `site`, `doctor`. |
| [`mcp/`](mcp/) | | A Model Context Protocol server over `feed` and `ledger`, so an agent can ask for provenance directly. |

Each pillar has its plan under `<tool>/docs/plan.md`; `feed`'s carries the
project rules that apply to every pillar. `docs/local-runbook.md` lists
everything that has not yet run outside the sandbox it was built in, with
the commands to do it. `docs/steward.md` is the design for judging the
data pipe, our own algorithms and our own conduct, and never the outlets.
`docs/algorithms.md` is the public register of every rule First Report runs to
group, order, hold or flag anything; a rule not in it does not run.

## Visual layer

[`design/`](design/) holds the aesthetics document, the CSS tokens, and a
component preview page. Each tool copies `tokens.css` rather than depending
on it at runtime, and a test in `lens` fails if its copy drifts. The `lens`
site, the `feed` debug page and the `ledger` lookup page are its consumers.

## For publishers

First Report reads feeds and news sitemaps, keeps headline, link, summary
and time, never fetches article pages, honours robots.txt and
Crawl-delay on every request, and names this repository in its
User-Agent. `docs/publisher-rules.md` states exactly what is fetched
and the rules the fetcher enforces.

## For agents and bots

First Report is built to be read by machines as easily as by people, because
the disinformation it documents is spread by machines. Every server
publishes `/llms.txt` describing itself and its rules of use, allows
everything in `/robots.txt`, serves JSON by default, and answers
`Accept: text/markdown` on its main object with a rendering that keeps
every source, licence and date. Each server also serves an OpenAPI 3.1
document at `/v1/openapi.json`, with schemas generated from its types,
so a tool-using agent can call it correctly the first time. Every page
for people links its JSON twin from `<head>`. `design/DESIGN.md` §11 has the rules.

[`deploy/`](deploy/) holds what puts the site on the internet: the
Cloudflare Worker config, the archive Worker, systemd units and the
setup script. `docs/deploy.md` is the plan they implement.

## Getting started

```
cargo build --workspace
./target/debug/firstreport doctor    # what is in place, what to fetch or import
./target/debug/firstreport run       # feed on :8080, ledger on :8081, Ctrl-C stops both
./target/debug/firstreport site      # export both, build the lens site into ./site, check it
```

Requires a Rust toolchain. No Docker, no managed database, no cloud APIs.
Each pillar is still its own binary with its own commands (`feed`,
`ledger`, `lens`); `firstreport` only drives them from a checkout.

## Status

Everything below is verified offline against recorded fixtures. The
first live runs (feeds, model download, ledger imports) are the local
runbook's job; `firstreport doctor` reports what is still missing.

- **feed** is at v0.1.0 with the data contract frozen at v1: polling
  with adaptive intervals and snapshot archival, canonical URLs and
  dedup, embedding through ONNX Runtime with in-repo WordPiece and
  SentencePiece unigram tokenizers (presets for English MiniLM-L6 and
  the multilingual MiniLM-L12 that covers Japanese), incremental
  clustering with a merge pass, revision and withdrawal tracking,
  syndication groups, the read-only API with Markdown twins, OpenAPI
  and `llms.txt`, NDJSON and Parquet export including revisions, a
  nightly job, feed discovery and OPML import, `feed eval` over three
  golden pair sets, the steward's feed health (five stated rules that
  quarantine a broken feed for a day with the reason shown), clustering
  confidence on every story, request and disk budgets and the opt-out
  list (`docs/steward.md` S1, S2 and S4).
  See `feed/docs/plan.md` §11.
- **ledger** is at L2e: file-backed claim store with validation and
  refresh tracking, importers for Wikidata, Wikipedia's perennial
  sources list, domain registration via RDAP, the outlet's own site and
  feed behaviour from `feed`'s export, a read-only API with search,
  Markdown twins, OpenAPI and a `/lookup` page, and the steward's
  change guard and conflict list (`docs/steward.md` S3). See
  `ledger/docs/plan.md` §8.
- **lens** is at N3: front, story, outlet, search and about pages with
  JSON and Markdown twins, `llms.txt` and a sitemap, coverage by a named
  source's labels on story pages, `check` and `serve`. See
  `lens/docs/plan.md` §9.
- **reply** is built through R6: templates with the cite check, the
  resolver over `feed` and `ledger`, a Bluesky listener that drafts
  mentions into a queue, a review page and poster with a public log,
  on-demand observation with the four coordination signals, English and
  Japanese templates. Nothing has run against a live account. See
  `reply/docs/plan.md` §16.
- **firstreport** runs `feed` and `ledger` together, builds and checks the
  site, and reports what is in place.

## Licence

The code is licensed under either of [Apache License 2.0](LICENSE-APACHE)
or [MIT](LICENSE-MIT), at your option. Outlet facts in `ledger/data/` keep
the licence of the source each one names.
