# Kagami 鏡

An open-source alternative to a news aggregator, built as a disinformation-analysis
network: where did a story originate, how fast did it spread, who amplified it,
was it syndicated or independently reported.

The project is split into two layers.

## Intelligence layer

Rust, single binary per tool, runs entirely locally. Three pillars, built in order:

| Tool | Pillar | Job |
|---|---|---|
| [`feed/`](feed/) | 1 | Ingest headlines, cluster them into stories, record propagation. Neutral by construction. |
| [`ledger/`](ledger/) | 2 | Sourced facts about outlets keyed by outlet id: ownership, country, founding, third-party ratings, each with its source and licence. Never a score. |
| `lens/` | 3 | Presentation over `feed` and `ledger`. Not started. |
| [`reply/`](reply/) | 4 | A social account that answers "prove it" with sourced provenance from `feed` and `ledger`. Planned. |

`feed/docs/plan.md` is the full plan for pillar 1, including the project rules
that apply to every pillar. `ledger/docs/plan.md` is pillar 2's, and `reply/docs/plan.md` is pillar 4's.

## Visual layer

[`design/`](design/) holds the aesthetics document, the CSS tokens, and a
component preview page. Each tool copies `tokens.css` rather than depending
on it at runtime. The `feed` debug page is the first consumer.

## Getting started

```
cd feed
cargo run -- run
```

Requires a Rust toolchain. No Docker, no managed database, no cloud APIs.

## Status

M3 cluster. `feed run` polls the seed feeds, archives every non-304 body
as a zstd snapshot, parses and canonicalizes items, dedupes them, fills
the SQLite store, embeds each item (MiniLM via ONNX Runtime once
`feed fetch-model` has run), and clusters embedded items into stories
with an incremental cosine assignment over an in-memory active set that
is checkpointed to disk. `/v1/clusters`, `/v1/events` and `/debug` are
live alongside the M1 routes, and `feed replay` runs recorded fixtures
through the whole pipeline offline. M4 added revision and withdrawal
tracking from snapshot diffs, `/v1/items/{id}/revisions`,
`/v1/syndication`, and spread curves on the debug page. The ingress
hardening pass added adaptive polling with jitter, Retry-After and
permanent-redirect handling, a User-Agent fallback, `/v1/feeds` health,
OPML import and feed autodiscovery. M5 added the merge pass, a golden
pair set with `feed eval`, and `feed/docs/clustering.md` for tuning.
M6 added NDJSON and Parquet export, `prune`, the nightly job, and the
contract and API docs. `feed` is at v0.1.0 with the data contract
frozen at v1; it is complete and verified offline, and has not yet run
against live feeds. See `feed/docs/plan.md` §11.

`ledger` is at L2e: file-backed claim store with validation, refresh
tracking, read-only API with search and a `/lookup` page, export, and
importers for Wikidata, Wikipedia's perennial sources list and domain
registration data via RDAP, the outlet's own site, plus feed behaviour measured from `feed`'s
own export. See
`ledger/docs/plan.md` §8.
