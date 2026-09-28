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

## Embedding model

```
cargo run -- fetch-model
```

downloads three files into `models/` (gitignored): the int8 ONNX export of
`all-MiniLM-L6-v2`, its WordPiece vocab, and the ONNX Runtime shared
library. Nothing is fetched at build time; the runtime is loaded at startup
from `embed.runtime_lib`. Without these files `feed run` still ingests and
logs a warning; items are embedded from the backlog once they appear. Sources
are configurable under `[embed]` in `config/feed.toml`, and the embedding
width is taken from the model, not hardcoded.

Embed latency (amortized per item, p50 and p99) is reported in `/v1/stats`.

## API (v1, read-only)

| Route | Notes |
|---|---|
| `GET /v1/items?since=&outlet=&limit=&cursor=` | newest first; `next` carries the cursor |
| `GET /v1/items/{id}` | |
| `GET /v1/snapshots?feed=&since=&limit=&cursor=` | raw feed fetch log |
| `GET /v1/snapshots/{id}` | |
| `GET /v1/snapshots/{id}/body` | decompressed feed body as fetched |
| `GET /v1/clusters?since=&min_outlets=&sort=&limit=&cursor=&include=items` | `sort` is `recent` (cursor-paged), `velocity_1h` or `outlets_6h` (top-N) |
| `GET /v1/clusters/{id}` | contract `Cluster` plus its items |
| `GET /v1/clusters/{id}/timeline` | ordered joins only |
| `GET /v1/events?after_seq=&limit=` | cluster event log for incremental sync |
| `GET /v1/outlets` | registry with domains and feeds |
| `GET /v1/stats` | process counters, store totals, embed and assign latency |
| `GET /debug` | live page showing clusters form, styled from `design/tokens.css` |

Ids are u64 and accepted as decimal or `0x`-prefixed hex.

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
