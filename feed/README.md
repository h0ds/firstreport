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
| `GET /v1/outlets` | registry with domains and feeds |
| `GET /v1/stats` | process counters and store totals |

Ids are u64 and accepted as decimal or `0x`-prefixed hex.

## Offline testing

`fixtures/feeds/` holds recorded feed bodies. `src/pipeline_test.rs` drives
one through the poller, store and API without touching the network; the M3
replay harness generalizes this.
