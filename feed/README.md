# feed

Kagami pillar 1: ingest headlines from many outlets, cluster them into
stories, record how each story spread. The full plan is in `docs/plan.md`.

```
cargo run -- run                     # poll config/feeds.toml, serve on 127.0.0.1:8080
cargo run -- --bind 0.0.0.0:8080 run # expose beyond loopback
RUST_LOG=feed=debug cargo run -- run # per-fetch logging
cargo test
```

Runtime state lives in `data/` (gitignored): `feed.db` and `snapshots/`.

## Sources and the publisher's rules

`config/feeds.toml` lists RSS, Atom and JSON feeds and, for publishers
with no feed, Google News sitemaps, each with the outlet it belongs to.
Every fetch honours the origin's robots.txt and Crawl-delay, sends
conditional requests, identifies the project in its User-Agent, and
never requests an article page. `../docs/publisher-rules.md` states
exactly what is fetched and kept.

After a live run, `cargo run -- feeds` lists the feeds that failed,
`cargo run -- feeds --fix` asks each outlet's home page for a
replacement and confirms it fetches as a feed, and `--write` rewrites
that one line in `config/feeds.toml` for review. The same command lists
feeds the steward is holding (`../docs/steward.md`): a feed whose
bodies stop parsing, that republishes its own items under new URLs, or
that floods is quarantined for a day with the reason on its row, and
lifts itself when the next fetch judges clean.

## Embedding model

```
cargo run -- fetch-model
```

downloads three files into `models/` (gitignored): the int8 ONNX export of
the configured preset (`all-MiniLM-L6-v2` by default, or
`paraphrase-multilingual-MiniLM-L12-v2` for English and Japanese
together), its tokenizer file, and the ONNX Runtime shared library. The
commented block under `[embed]` in `config/feed.toml` shows the
multilingual settings. Nothing is fetched at build time; the runtime is loaded at startup
from `embed.runtime_lib`. Without these files `feed run` still ingests and
logs a warning; items are embedded from the backlog once they appear. Sources
are configurable under `[embed]` in `config/feed.toml`, and the embedding
width is taken from the model, not hardcoded.

Embed latency (amortized per item, p50 and p99) is reported in `/v1/stats`.

## API (v1, read-only)

| Route | Notes |
|---|---|
| `GET /v1/items?since=&outlet=&limit=&cursor=` | newest first; `next` carries the cursor |
| `GET /v1/items/{id}` | latest text |
| `GET /v1/items/{id}/revisions` | text history from snapshot diffs; empty until the first change |
| `GET /v1/syndication?text_hash=&since=&limit=` | items across all clusters within the simhash threshold of `text_hash`, scanned over the last 30 days by default |
| `GET /v1/lookup?url=&text=&since=` | everything known about one URL or text in one call: item, cluster, origin, outlet with ledger links, or near-copies by simhash; `matched` and `checked` say how far it got |
| `GET /v1/snapshots?feed=&since=&limit=&cursor=` | raw feed fetch log |
| `GET /v1/snapshots/{id}` | |
| `GET /v1/snapshots/{id}/body` | decompressed feed body as fetched |
| `GET /v1/clusters?since=&min_outlets=&sort=&limit=&cursor=&include=items` | `sort` is `recent` (cursor-paged), `velocity_1h` or `outlets_6h` (top-N) |
| `GET /v1/clusters/{id}` | contract `Cluster` plus its items |
| `GET /v1/clusters/{id}/timeline` | ordered joins only |
| `GET /v1/events?after_seq=&limit=` | cluster event log for incremental sync |
| `GET /v1/feeds?filter=all\|failing\|dead\|stale\|quarantined\|history` | per-feed health: last status and error, adapted interval, fail count, permanent moves, the steward's signals and quarantine; configured feeds only unless `history` |
| `GET /feeds` | network dashboard: every feed with its outlet, status light and last fetch, linked from the feeds count on `/debug` |
| `GET /v1/outlets` | registry with domains and feeds |
| `GET /v1/stats` | process counters, store totals, embed and assign latency |
| `GET /debug` | live page showing clusters form, styled from `design/tokens.css` |

Ids are u64 and accepted as decimal or `0x`-prefixed hex.

## Ingress behaviour

- **Conditional requests** with ETag and Last-Modified; gzip and brotli.
- **Adaptive polling.** A fetch that yields new items shortens the feed's
  interval by a third, an unchanged one lengthens it by a third, bounded
  by `poll.min_interval_secs` and `poll.max_interval_secs`. Every wait
  carries deterministic ±`poll.jitter` so feeds on the same interval
  drift apart instead of bursting.
- **Unreachable is retried soon.** A fetch that makes no connection (DNS or
  connect failure) is retried after 2, 4 and 8 minutes before normal
  backoff applies, so a blip in the machine's own DNS or network does not
  leave healthy feeds backed off for half an hour. It still counts as a
  failure.
- **Backoff** doubles from the configured interval on each failure, capped
  at `poll.max_backoff_secs`; `Retry-After` on 429/503 is honoured when it
  is longer. Seven days of continuous failure marks a feed dead.
- **Permanent moves.** 301/308 chains are followed and the final URL is
  recorded as `resolved_url`, which is fetched from then on while the
  configured URL stays the feed's identity. The log says when to update
  `feeds.toml`. Temporary redirects are followed silently.
- **User-Agent fallback.** The product token is sent first. A host that
  answers 403 is retried once with the `Mozilla/5.0 (compatible; …)` form,
  which still names the project, and remembered if that worked.
- **Canonicalization** strips `www.`, `amp.`, `m.` and `mobile.` hosts,
  `/amp` path suffixes, AMP query toggles and tracking parameters, so one
  article is one item however a feed links it.
- **Date sanity.** A publish date more than an hour in the future is
  replaced with the fetch time and counted in `/v1/stats`.

## Growing the feed list

```
cargo run -- import --opml subscriptions.opml          # print TOML for feeds and new outlets
cargo run -- import --opml subscriptions.opml --write  # append it to config/
cargo run -- discover https://www.example.com          # feeds the page advertises (or conventional paths)
cargo run -- discover https://www.example.com --write
cargo run -- discover --hosts fixtures/golden/aggregator/aggregator-domains.txt --limit 100
cargo run -- discover --hosts hosts.txt --max-per-host 2 --write
```

`--hosts` walks a host list (one per line; the aggregator domains file
works as is) with bounded concurrency, keeps up to `--max-per-host`
feeds per site preferring RSS and Atom and skipping comment feeds, and
lists the hosts that advertised nothing so they can be handled by hand.

Outlet ids for unknown hosts are guessed from the registrable domain
(`news.bbc.co.uk` → `bbc`); edit the appended entries if the guess is
wrong. Nothing is written without `--write`.

## Revision tracking

After each parse the feed's listing is diffed against its previous one
(kept in memory, or rebuilt from the latest archived snapshot after a
restart). A changed title or summary records both the original and the
new text in `item_revision`, updates the item to the latest text, and
emits a `revised` event; the embedding is deliberately not recomputed.
An item that disappears while older items are still listed emits a
`withdrawn` event. Both counts appear in `/v1/stats`.

## Clustering

Each embedded item is compared against every active cluster centroid
(flat SIMD dot product, AVX2 or NEON with a scalar fallback) inside a
time window, joins the best match above `cluster.join_threshold` or
seeds a new cluster, and the cluster's timeline, origin, velocity and
syndication groups are derived from the item table on read. Idle
clusters expire on the item clock. The active set is checkpointed to
`data/clusterer.ckpt` every `store.checkpoint_secs` and on shutdown;
a missing or corrupt checkpoint triggers a rebuild from embedded items
in the window.

## Export, prune, nightly

```
cargo run -- export --format ndjson --out data/export/today
cargo run --features parquet -- export --format parquet --out data/export/today
cargo run -- prune --older-than 90d --embeddings
```

Export writes `items`, `clusters`, `events` and `outlets`. NDJSON omits
embeddings and centroids; Parquet includes them as `list<float32>`.
This is the bulk interface for `ledger` and `lens`; the live API is for
incremental updates. `prune --older-than` deletes rows past the cutoff
(active clusters excepted), `--embeddings` nulls embeddings outside the
clustering window, and snapshot retention is applied on every run,
then the disk budget (`snapshots.max_bytes`, whole days oldest first).
With `export.nightly = true`, `feed run` does an export into
`out_dir/<yyyy-mm-dd>` at `export.hour_utc` daily and applies snapshot
retention.

Docs: `docs/data-contract.md` (the v1 schema, frozen), `docs/api.md`,
`docs/clustering.md`, `docs/plan.md`.

## Tuning

```
cargo run -- eval --pairs fixtures/golden/pairs.tsv
cargo run -- eval --pairs fixtures/golden/ja-pairs.tsv     # Japanese and cross-language; needs the multilingual preset
```

reports precision, recall and F1 of the assignment step across
thresholds on a labelled pair set, plus the pairs the configured
`cluster.join_threshold` gets wrong. `fixtures/golden/aggregator/`
holds a thousand-pair set derived from a news aggregator's real story groupings
and a spectrum-spanning list of 888 outlet hosts for `feed discover`. `docs/clustering.md` explains what
the pair set needs to contain, records the current numbers, and lists
the model and input-template alternatives to compare.

## Offline replay

```
cargo run -- replay --fixtures fixtures/replay
```

runs recorded snapshots (`fixtures/replay/manifest.toml`) through the
full pipeline with no network, single-threaded inference, and the item
clock driving expiry, then prints the resulting clusters as JSON. The
test `replay::fixtures_match_golden_clusters` diffs that output against
`fixtures/expected/clusters.json` using the `hash` embedding backend, so
determinism is checked in CI without a model. After an intentional
change, regenerate with `KAGAMI_UPDATE_EXPECTED=1 cargo test`.

The `hash` backend (`embed.backend = "hash"`) is a lexical feature-hashing
stand-in for the model. It is for replay and tests, not production
clustering quality.
