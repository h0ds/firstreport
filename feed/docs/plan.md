# feed — Aggregation & Story Clustering Layer

Pillar 1 of an open-source news comparison site. `feed` ingests headlines from many outlets, clusters them into stories, and exposes the result through a stable data contract. It knows nothing about bias, factuality, or ownership. Those belong to `ledger` (pillar 2). Presentation belongs to `lens` (pillar 3).

---

## 1. Project rules

These apply to every pillar and override anything below that conflicts.

1. **Most performant stack.** Rust. No interpreted runtime anywhere in the hot path.
2. **Runs entirely locally in dev.** `cargo run` and it works. No Docker, no managed DB, no cloud APIs, no GPU requirement. Network access is only to the feeds themselves.
3. **Bespoke over off-the-shelf.** Custom implementations for anything performance-relevant (vector search, clustering, storage layout). Third-party crates are allowed only for commodity plumbing (HTTP client, feed parsing, async runtime, ONNX inference) and only where writing our own gains nothing.
4. **Deterministic.** Same feed snapshots in, same clusters out. Everything is reproducible and testable offline from recorded fixtures.
5. **Data contract is the product.** Internals can be rewritten freely; the emitted schema is versioned and stable.
6. **Neutral by construction.** `feed` records what was published, by whom, and when. It makes no judgments and applies no filtering by outlet quality. Fringe and mainstream sources are ingested identically. Characterization is `ledger`'s job. If this layer is ever seen as editorial, the whole stack loses credibility.

---

## 1a. Why this exists

`feed` is the first tool of a larger disinformation-analysis network. Almost every question in that domain — where did a claim originate, how fast did it spread, who amplified it, was it syndicated or independently reported — is answered by propagation data over story clusters. This plan therefore treats **provenance and propagation as first-class outputs**, not as an aggregator's side effect.

Claim-level tracking (extracting and following specific assertions within stories) is a separate, later pillar. `feed` operates at story granularity only.

---

## 2. Scope

### In scope
- Polling RSS / Atom / JSON Feed sources on a schedule
- URL canonicalization and item deduplication
- Local sentence embedding of headline + summary
- Incremental (online) story clustering with time decay
- Periodic cluster merge pass
- Per-cluster propagation record: join order, join timestamps, velocity
- Syndication fingerprinting (near-identical copy across outlets)
- Raw feed snapshot archival (compressed) for upstream edit/deletion evidence
- Durable storage with crash-safe checkpointing
- Versioned JSON HTTP API
- Bulk export (NDJSON + Parquet)
- Minimal debug web page for watching clusters form
- Outlet registry (domain → outlet ID) as the single seam with `ledger`

### Out of scope (deliberately)
- Bias, factuality, ownership ratings
- Claim extraction or claim-level tracking (future pillar)
- Any full-text article fetching or storage (raw *feed* XML is archived; article pages are not)
- User accounts, personalization, notifications
- Any UI beyond the debug page
- Distributed operation, multi-node, queues

---

## 3. Architecture

Single Rust binary, single process, `tokio` runtime. Four long-running tasks communicate through bounded channels:

```
┌──────────┐   items   ┌──────────┐  embedded  ┌───────────┐  events  ┌────────┐
│  Poller  │ ────────▶ │ Embedder │ ─────────▶ │ Clusterer │ ───────▶ │  Store │
└──────────┘           └──────────┘            └───────────┘          └────────┘
      ▲                                              │                     │
      │ schedule                                     │ read-only view      │ read
      │                                        ┌─────▼─────────────────────▼──┐
      │                                        │           API / Export        │
      └────────────────────────────────────────┴───────────────────────────────┘
```

- **Poller** — owns the feed list and per-feed state; fetches, parses, canonicalizes, dedupes, emits `Item`.
- **Embedder** — batches items, runs the ONNX model, attaches a 384-dim f32 embedding.
- **Clusterer** — owns the active cluster set in memory; assigns each item to a cluster or creates one; emits `ClusterEvent`s.
- **Store** — persists items, clusters, and events; checkpoints clusterer state; serves reads.
- **API** — read-only over the store plus a live view of the active cluster set; also runs the export job.

All hot state (embeddings of active items, cluster centroids) lives in process memory. Disk is for durability and cold history only.

---

## 4. Data contract (v1)

Emitted schema. Field additions are non-breaking; removals or type changes bump the version.

```
Outlet {
  id:           string      // stable slug, e.g. "reuters"
  name:         string
  domains:      [string]    // canonical host names
  feeds:        [FeedRef]   // { url, kind: rss|atom|json }
}

Item {
  id:           u64         // xxhash64 of canonical_url
  outlet_id:    string
  canonical_url: string
  title:        string
  summary:      string      // ≤ 512 chars, plain text, may be empty
  published_at: i64         // unix seconds, from feed; 0 if absent
  fetched_at:   i64         // unix seconds
  lang:         string      // ISO 639-1, detected; "und" if unknown
  text_hash:    u64         // simhash of normalized title+summary; syndication signal
  cluster_id:   u64 | null
  cluster_rank: u32 | null  // 0 = first item in its cluster (origin), 1 = second, …
  joined_at:    i64 | null  // when this item was assigned to its cluster
  snapshot_id:  u64         // raw feed snapshot this item was parsed from
}

Cluster {
  id:            u64        // id of the first item in the cluster
  created_at:    i64
  last_active_at: i64
  item_count:    u32
  outlet_count:  u32
  primary_title: string     // title of the item nearest the centroid
  origin_item_id: u64       // first item (by published_at, tie-break fetched_at)
  origin_outlet_id: string
  timeline:      [Join]     // ordered; Join { item_id, outlet_id, published_at, joined_at }
  syndication:   [SynGroup] // groups of items with near-identical text_hash
                            // SynGroup { text_hash, item_ids[], first_outlet_id, spread_secs }
  velocity: {
    items_1h: u32, items_6h: u32, items_24h: u32,
    outlets_1h: u32, outlets_6h: u32, outlets_24h: u32,
    time_to_5_outlets_secs: u32 | null,
    time_to_20_outlets_secs: u32 | null
  }
  // centroid is internal; not emitted over API, included in Parquet dump
}

Snapshot {
  id:          u64          // xxhash64(feed_url + fetched_at)
  feed_url:    string
  fetched_at:  i64
  status:      u16
  body_hash:   u64
  // body stored zstd-compressed on disk, retrievable via API
}

ClusterEvent {
  ts:      i64
  kind:    "created" | "item_added" | "merged" | "expired"
  cluster_id: u64
  item_id:    u64 | null
  merged_into: u64 | null
}
```

Embeddings are never emitted over the JSON API (size). They are included in the Parquet export for downstream research use.

---

## 5. Pipeline stages

### 5.1 Poller

- Feed list from `config/feeds.toml`. Each entry: `url`, `outlet`, optional `interval_secs` (default 900), optional `kind` override.
- Scheduler: min-heap keyed on `next_fetch_at`. One task pops due feeds and dispatches fetches into a semaphore-bounded pool (global cap ~64, per-host cap 2).
- Conditional requests: store `etag` and `last_modified` per feed; send `If-None-Match` / `If-Modified-Since`. 304 → reschedule, no parse.
- Backoff: on error, `interval * 2^n` capped at 6h. On success, reset. `Retry-After` on 429/503 is honoured when longer. Feeds failing continuously for 7 days are flagged `dead` in the store and skipped until manually re-enabled.
- Adaptive intervals (hardening pass): new items shorten a feed's interval by a third, an unchanged body or 304 lengthens it by a third, bounded by `min_interval_secs`/`max_interval_secs`. This is the main lever on `joined_at` precision and therefore on velocity and time-to-N-outlets. Every wait carries deterministic jitter (hash of feed and time) so feeds sharing an interval drift apart.
- Permanent redirects are followed and recorded as `resolved_url`; the configured URL remains the feed's identity. A host answering 403 to the product User-Agent is retried once with the `Mozilla/5.0 (compatible; kagami-feed/…)` form and remembered.
- Per-feed health (last status, last error, adapted interval, moves) is persisted on the feed table and served at `/v1/feeds`; `feed feeds` lists the failing ones offline and `feed feeds --fix` proposes replacement URLs by discovery, applied to `feeds.toml` with `--write`.
- **Steward** (`docs/steward.md` §2): over each feed's last twenty fetches with a body the poller keeps parse failures, duplicate rate (a new URL carrying text already in the feed's previous listing), median publish-to-fetch skew, clamped future dates, burst against the feed's median and empty runs. Five one-sentence rules quarantine a feed for a day (parse, duplicates, flood) or flag it `timestamps_unreliable`. A quarantined feed keeps polling at its maximum interval and keeps its snapshots, but its items are held (`items_held` in stats) and its listing is not evidence of revisions. The reason and release time are on the feed row, `/v1/feeds?filter=quarantined`, `feed feeds` and `kagami doctor`. This judges the feed as a pipe and never knows the outlet.
- **Snapshot archival:** every non-304 response body is written to `data/snapshots/<yyyy>/<mm>/<dd>/<snapshot_id>.zst` (zstd level 3) before parsing, with a row in the `snapshot` table. Parse failures still keep the snapshot. If an item later disappears from a feed or its title changes between snapshots, a `revised` / `withdrawn` event is emitted (§5.3a). Storage cost: ~3k feeds × 96 polls/day × ~5 KB compressed ≈ 1.5 GB/day worst case; in practice most polls are 304s and the real figure is a fraction of that. Retention configurable, default 180 days.
- **The publisher's rules come first.** Every request goes through the shared client, which checks `config/opt-out.toml` before anything else (a listed host or subdomain gets no request at all), reads each origin's robots.txt daily, never requests a disallowed path (the feed fails with that reason and is rechecked when the file changes), spaces requests to a host by its `Crawl-delay`, and keeps under the hourly request budgets in `[poll]` (a request over budget waits; `/v1/stats` counts the waits). `docs/publisher-rules.md` states everything Kagami fetches and keeps.
- Parse with `feed-rs`. Extract title, link, summary/description (HTML stripped to text, truncated to 512 chars), published date, language hint.
- **News sitemaps** for publishers with no feed (Reuters, most large papers): a fetched body whose root is `<urlset>` is parsed as a Google News sitemap into the same entries (URL, headline, publication time, language; no summary). Entries without a `news:` block are skipped; a sitemap index is refused, the config points at the news sitemap itself.
- **Text hash:** compute a 64-bit simhash over the normalized title+summary (lowercased, punctuation and stopwords stripped, word 2-shingles). Items whose hashes are within Hamming distance ≤ 3 are treated as syndicated copies of the same text. This is independent of the embedding and much stricter: it detects *same copy*, not *same story*.
- Canonicalize URL:
  - lowercase scheme + host, strip `www.`, `amp.`, `m.`, `mobile.`; strip `/amp` path suffixes and AMP query toggles
  - strip fragment
  - strip known tracking params (`utm_*`, `fbclid`, `gclid`, `ref`, `source`, and a maintained list)
  - sort remaining query params
  - resolve relative links against feed URL
- Item ID = xxhash64(canonical_url). Dedupe against a bloom filter of recent IDs (rebuilt from store on boot, ~7 days window) then confirmed against the store on hit.
- Resolve `outlet_id` from host via `config/outlets.toml` (exact host match, then suffix match). Unknown host → outlet `unknown:<host>`, logged, still ingested.
- Detect language with `whatlang` (small, no network). Non-target languages are still stored but can be excluded from clustering via config.
- Publish dates more than an hour after the fetch are replaced with the fetch time (feeds do emit them, and a future date would fall outside every cluster window). Counted in `/v1/stats`.
- Feed list growth: `feed import --opml` and `feed discover <url>` print `feeds.toml`/`outlets.toml` entries and append them with `--write`.

### 5.2 Embedder

- Model: quantized MiniLM-L6-class sentence transformer, ONNX, int8, ~22 MB. Stored under `models/`. Downloaded once by `cargo run -- fetch-model` if absent; never at build time.
- Runtime: `ort` (ONNX Runtime) CPU execution provider, intra-op threads = physical cores. The `ort` crate's default feature downloads the runtime library during `cargo build`, which violates rule 2; we use `load-dynamic` instead and `fetch-model` provisions the shared library next to the model. Loaded at startup from `embed.runtime_lib`.
- Tokenizers implemented in-repo, chosen by `embed.tokenizer`: WordPiece for BERT-family models (`vocab.txt`) and SentencePiece unigram for XLM-RoBERTa-family models (`sentencepiece.bpe.model`, read directly with a minimal protobuf reader; NFKC normalisation; Viterbi segmentation; XLM-RoBERTa's id convention). The unigram tokenizer is verified against the reference implementation's ids on recorded English and Japanese models. Neither pulls in the `tokenizers` crate.
- Presets: `minilm-l6` (English, the default) and `multilingual-minilm-l12` (`paraphrase-multilingual-MiniLM-L12-v2`, 50 languages including Japanese, same 384 width). The multilingual model has no `token_type_ids` input; the embedder reads the graph's inputs and feeds only what it declares.
- Input text: `title + " . " + summary`, truncated to 128 tokens.
- Batching: collect up to 64 items or 200 ms, whichever first. Mean-pool, L2-normalize. Output width is read from the model (384 for MiniLM-L6) rather than assumed, so swapping models is a config change.
- The embedder is optional at runtime: without a model, `feed run` ingests and warns. On boot, items inside `cluster.window_secs` with no embedding are queued first, so a restart or a late-installed model catches up. `item.embedding` is therefore nullable.
- Target: < 2 ms per item amortized on a modern laptop CPU.

### 5.3 Clusterer

Online, incremental, single-threaded over an in-memory active set. No batch reclustering of history.

**State**
- `active: Vec<ActiveCluster>` where `ActiveCluster { id, centroid: [f32;384], sum: [f32;384], n: u32, last_active_at, item_ids: Vec<u64>, outlet_ids: SmallSet }`
- Centroid = L2-normalized `sum / n`.
- Clusters leave the active set after `expiry_secs` (default 48h) of no new items → `expired` event, persisted, dropped from memory.

**Clock.** Assignment, window checks and expiry run on the item clock (the latest `fetched_at` seen), never the wall clock. A fixture replay therefore expires clusters exactly where a live run would have, and the golden test never depends on when it ran.

**Assignment (per item)**
1. Compute cosine similarity against every active centroid. Flat SIMD dot product (`std::simd` or hand-unrolled AVX2/NEON via `std::arch` with a scalar fallback). At ≤ 10k active clusters this is ~1–3 ms and beats any index; revisit only if active set exceeds ~100k.
2. Restrict candidates to clusters whose `last_active_at` is within `window_secs` (default 72h) of the item's `published_at`. Prevents old stories re-absorbing new ones with similar phrasing.
3. Best match `s_max`:
   - `s_max ≥ join_threshold` (default 0.72) → add item, update sum/centroid, emit `item_added`.
   - else → new cluster with this item as seed, emit `created`.
4. Same-outlet guard: if the best cluster already contains ≥ 3 items from this outlet and `s_max < join_threshold + 0.08`, treat as no match. Reduces one outlet's live-blog updates collapsing unrelated stories.

**Propagation record** (maintained on every assignment)
- Append `Join { item_id, outlet_id, published_at, joined_at }` to the cluster's ordered timeline. `cluster_rank` is the item's index in that timeline.
- Origin = timeline entry with the earliest `published_at` (tie-break earliest `fetched_at`). Note that origin can change if a late-fetched item has an earlier `published_at`; emit an `origin_changed` event when it does. Feeds lie about publish dates, so `lens` should treat origin as evidence, not proof.
- Velocity counters, the timeline, origin and syndication groups are all recomputed on read from the item table (`cluster_rank`, `joined_at`, `text_hash`); the cluster row stores only counts, the primary title and the centroid.
- Syndication groups: on join, compare the item's `text_hash` against those of existing cluster members (Hamming ≤ 3). Match → add to that `SynGroup`; else start a new one. `spread_secs` = last − first `published_at` within the group.

**Merge pass** (every `merge_interval_secs`, default 600)
- Pairwise cosine over active centroids (n² but n is small). Any pair with similarity ≥ `merge_threshold` (default 0.80) and overlapping time windows → merge smaller into larger, emit `merged`, rewrite `cluster_id` on affected items.
- Guard: never merge if the union would exceed `max_cluster_size` (default 500).

**Thresholds are config values** and the primary tuning surface. Ship with defaults tuned on a recorded fixture set (see §9).

### 5.3a Revision tracking

Runs inside the poller after parsing. For each feed, diff the parsed item set against the previous snapshot's item set (by canonical URL):

- Title or summary changed → emit `ClusterEvent { kind: "revised", item_id, snapshot_id }` and store the new text in `item_revision`. On the first change the original text is also recorded, keyed by the previous snapshot, so history starts from what was first seen. The item's embedding and `text_hash` are **not** recomputed (cluster stability, and the syndication signal should reflect the copy as first published), but the latest text is what the API returns, with revision history available.
- Item present in previous snapshot, absent now, and not simply aged out of the feed's window (heuristic: it is newer than the oldest item still present; undated items are never called withdrawn) → emit `withdrawn`. An empty listing emits nothing: that is a feed failure, not a hundred withdrawals.
- The previous listing lives in memory per feed; after a restart it is parsed from the latest archived snapshot on the feed's first fetch, so a restart never masks a revision.

Cheap, and it's the kind of evidence that is impossible to recover after the fact.

**Checkpointing**
- Every `checkpoint_secs` (default 60) and on shutdown: serialize `active` to `data/clusterer.ckpt` (custom binary format, length-prefixed, versioned header, xxhash checksum). On boot, load if valid; else rebuild from store by replaying items from the last `window_secs`.

### 5.4 Store

SQLite via `rusqlite`, WAL mode, `synchronous=NORMAL`, single writer task receiving events over a channel. All writes batched in transactions of up to 256 rows or 500 ms.

```sql
CREATE TABLE outlet (
  id TEXT PRIMARY KEY, name TEXT NOT NULL
);
CREATE TABLE outlet_domain (
  host TEXT PRIMARY KEY, outlet_id TEXT NOT NULL REFERENCES outlet(id)
);
CREATE TABLE feed (
  url TEXT PRIMARY KEY, outlet_id TEXT NOT NULL, kind TEXT,
  interval_secs INTEGER NOT NULL, etag TEXT, last_modified TEXT,
  next_fetch_at INTEGER NOT NULL, fail_count INTEGER NOT NULL DEFAULT 0,
  last_ok_at INTEGER, dead INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE item (
  id INTEGER PRIMARY KEY,           -- xxhash64 as i64
  outlet_id TEXT NOT NULL, canonical_url TEXT NOT NULL,
  title TEXT NOT NULL, summary TEXT NOT NULL,
  published_at INTEGER NOT NULL, fetched_at INTEGER NOT NULL,
  lang TEXT NOT NULL, text_hash INTEGER NOT NULL,
  cluster_id INTEGER, cluster_rank INTEGER, joined_at INTEGER,
  snapshot_id INTEGER NOT NULL,
  embedding BLOB                    -- dim × f32 little-endian; NULL until embedded
);
CREATE INDEX item_cluster ON item(cluster_id, cluster_rank);
CREATE INDEX item_published ON item(published_at);
CREATE INDEX item_text_hash ON item(text_hash);
CREATE TABLE item_revision (
  item_id INTEGER NOT NULL, snapshot_id INTEGER NOT NULL,
  seen_at INTEGER NOT NULL, title TEXT NOT NULL, summary TEXT NOT NULL,
  PRIMARY KEY (item_id, snapshot_id)
);
CREATE TABLE snapshot (
  id INTEGER PRIMARY KEY, feed_url TEXT NOT NULL, fetched_at INTEGER NOT NULL,
  status INTEGER NOT NULL, body_hash INTEGER NOT NULL, path TEXT NOT NULL,
  item_count INTEGER NOT NULL
);
CREATE INDEX snapshot_feed ON snapshot(feed_url, fetched_at);
CREATE TABLE cluster (
  id INTEGER PRIMARY KEY, created_at INTEGER NOT NULL,
  last_active_at INTEGER NOT NULL, item_count INTEGER NOT NULL,
  outlet_count INTEGER NOT NULL, primary_title TEXT NOT NULL,
  centroid BLOB NOT NULL, expired INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE cluster_event (
  seq INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL,
  kind TEXT NOT NULL,               -- created|item_added|merged|expired|origin_changed|revised|withdrawn
  cluster_id INTEGER, item_id INTEGER, merged_into INTEGER, snapshot_id INTEGER
);
```

Why SQLite and not a custom store: it's the one place where off-the-shelf costs nothing. Reads for the API are index hits; writes are batched and sequential. If profiling ever shows it as a bottleneck, the item table is a natural candidate for a custom append-only segment format — the schema above is already shaped for that.

Retention: items and clusters kept forever by default; `feed prune --older-than 90d` available.

### 5.5 API

`axum` on `localhost:8080`. Read-only. All responses `application/json`, versioned under `/v1`.

```
GET /v1/clusters?since=<ts>&limit=<n>&min_outlets=<n>&lang=<code>
    → active + recently expired clusters, newest first
GET /v1/clusters/{id}
    → cluster with full item list, timeline, syndication groups, velocity
GET /v1/clusters/{id}/timeline
    → ordered joins only; cheap endpoint for spread-curve rendering
GET /v1/clusters?sort=velocity_1h|outlets_6h&...
    → same listing, ranked by propagation speed
GET /v1/items/{id}
GET /v1/items/{id}/revisions
GET /v1/syndication?text_hash=<h>
    → all items across all clusters sharing a near-identical text
GET /v1/snapshots?feed=<url>&since=<ts>
GET /v1/snapshots/{id}/body
    → decompressed raw feed XML
GET /v1/outlets
GET /v1/events?after_seq=<n>&limit=<n>
    → cluster event log, for downstream incremental sync
GET /v1/stats
    → feeds polled, items/hour, active clusters, embed latency p50/p99
GET /debug
    → single HTML page, auto-refreshing, shows clusters forming live
```

Cursor pagination everywhere (`next` field). No auth in v1; bind to loopback by default, `--bind 0.0.0.0` to expose.

### 5.6 Export

`feed export --format ndjson|parquet --out <dir>` and a built-in nightly job (config-gated). Emits `items`, `clusters`, `events`, `outlets` and `revisions` in both formats. Parquet includes embeddings and centroids; NDJSON omits them. This is the interface `ledger` and `lens` are expected to consume in bulk; the live API is for incremental updates.

---

## 6. Configuration

`config/feed.toml`

```toml
[poll]
default_interval_secs = 900
global_concurrency = 64
per_host_concurrency = 2
max_backoff_secs = 21600

[embed]
model_path = "models/minilm-l6-int8.onnx"
vocab_path = "models/vocab.txt"
max_tokens = 128
batch_size = 64
batch_wait_ms = 200

[cluster]
join_threshold = 0.72
merge_threshold = 0.80
window_secs = 259200
expiry_secs = 172800
merge_interval_secs = 600
max_cluster_size = 500
same_outlet_guard = true
languages = ["en"]          # empty = all

[store]
path = "data/feed.db"
checkpoint_secs = 60

[snapshots]
enabled = true
dir = "data/snapshots"
zstd_level = 3
retention_days = 180

[syndication]
hamming_threshold = 3

[api]
bind = "127.0.0.1:8080"

[export]
nightly = false
out_dir = "data/export"
```

`config/feeds.toml`

```toml
[[feed]]
url = "https://www.reuters.com/rssfeed/topnews"
outlet = "reuters"

[[feed]]
url = "https://feeds.bbci.co.uk/news/rss.xml"
outlet = "bbc"
interval_secs = 600
```

`config/outlets.toml` — the only file `ledger` also reads. IDs only, no ratings. The seed list should span the full spectrum from wire services to fringe and partisan sites; the network's value comes from seeing where claims originate, and that is rarely in the mainstream list. No outlet is excluded on quality grounds — that filtering, if any, happens in `lens` using `ledger` data.

```toml
[[outlet]]
id = "reuters"
name = "Reuters"
domains = ["reuters.com"]

[[outlet]]
id = "bbc"
name = "BBC"
domains = ["bbc.co.uk", "bbc.com"]
```

---

## 7. Repository layout

```
feed/
  Cargo.toml
  README.md
  docs/
    plan.md              this document
    data-contract.md     §4 expanded, with examples
    clustering.md        §5.3 expanded, tuning notes, fixture results
    api.md               §5.5 expanded
  config/
    feed.toml
    feeds.toml           seed list, ~30 feeds across regions
    outlets.toml
  models/                gitignored except README with fetch instructions
  fixtures/
    feeds/               recorded feed XML snapshots for offline tests
    expected/            golden cluster assignments
  data/                  gitignored runtime dir
  src/
    main.rs              CLI: run | fetch-model | export | prune | replay
    config.rs
    types.rs             Item, Cluster, Outlet, ClusterEvent
    fetch/
      mod.rs
      scheduler.rs       min-heap, backoff
      client.rs          reqwest, conditional GET, per-host limits
    parse/
      mod.rs
      feed.rs            feed-rs wrapper, html strip
      url.rs             canonicalization, tracking param list
      outlet.rs          host → outlet resolution
      simhash.rs         text_hash
      revision.rs        snapshot-to-snapshot diff, revised/withdrawn events
    snapshot/
      mod.rs             zstd write/read, path layout, retention
    embed/
      mod.rs
      tokenizer.rs       WordPiece
      model.rs           ort session, batching, pooling
    cluster/
      mod.rs
      active.rs          ActiveCluster, centroid math
      propagation.rs     timeline, origin, syndication groups, velocity
      simd.rs            dot product kernels + scalar fallback
      assign.rs          assignment logic
      merge.rs           merge pass
      checkpoint.rs      binary ckpt format
    store/
      mod.rs
      schema.rs
      writer.rs          batched write task
      reader.rs
    api/
      mod.rs
      routes.rs
      debug.rs           inline HTML page
      export.rs
  tests/
    replay.rs            fixtures → expected clusters
    url_canon.rs
    tokenizer.rs
```

---

## 8. Dependencies (allowed list)

Commodity only. Anything not listed needs a justification in the PR.

| Crate | Purpose |
|---|---|
| `tokio` | async runtime |
| `reqwest` (rustls) | HTTP client |
| `feed-rs` | RSS/Atom/JSON Feed parsing |
| `ort` | ONNX Runtime bindings |
| `rusqlite` (bundled) | SQLite |
| `axum` | HTTP server |
| `serde`, `serde_json`, `toml` | config + API serialization |
| `xxhash-rust` | item IDs, checksums |
| `whatlang` | language detection |
| `zstd` | snapshot compression |
| `arrow` / `parquet` | export only, behind a feature flag |
| `tracing` | structured logs |

Explicitly **not** used: `tokenizers`, `hnsw`/`faiss` bindings, any ORM, any web framework beyond `axum`.

---

## 9. Testing & tuning

- **Fixture replay**: `fixtures/feeds/` holds ~200 recorded feed snapshots over a simulated 3-day window. `feed replay --fixtures fixtures/feeds` runs the full pipeline offline (no network) and writes clusters. `tests/replay.rs` diffs against `fixtures/expected/`. This is the determinism guarantee and the tuning harness.
- **Golden set**: hand-labelled item pairs (same story / different story) in `fixtures/golden/pairs.tsv`; `feed eval --pairs` reports precision/recall/F1 across thresholds and at the configured one. Sixty written pairs ship as a smoke set; grow it to a few hundred from real clusters once feed has run live. Tracked in `docs/clustering.md`.
- **Unit tests**: URL canonicalization table-driven; tokenizer against reference outputs from the Python tokenizer; SIMD kernels against scalar.
- **Benchmarks** (`cargo bench`): embed throughput, assignment latency vs active set size, store write throughput.

---

## 10. Performance targets (laptop, 8-core CPU, no GPU)

| Metric | Target |
|---|---|
| Feeds polled | 3,000 at 15 min intervals |
| Ingest rate | ≥ 50 items/s sustained |
| Embed latency | ≤ 2 ms/item amortized |
| Assignment latency | ≤ 3 ms/item at 10k active clusters |
| Memory (steady state) | ≤ 500 MB |
| API p99 | ≤ 20 ms for `/v1/clusters` |
| Cold boot with checkpoint | ≤ 2 s |
| Cold boot without checkpoint | ≤ 60 s (replay 72h from store) |

---

## 11. Milestones

Each milestone is independently runnable and leaves `cargo test` green.

**M0 — Skeleton** (½ day)
Cargo project, config loading, types, CLI stub, `tracing`. `cargo run` starts and exits cleanly.

**M1 — Ingest** (2 days)
Poller, parser, canonicalization, outlet resolution, dedupe, simhash, snapshot archival, SQLite store. `feed run` polls seed feeds and fills the `item` and `snapshot` tables. `/v1/items`, `/v1/snapshots`, `/v1/stats` work.

**M2 — Embed** (1 day)
Tokenizer, ONNX session, batching. Embeddings stored on items. Bench meets target.

**M3 — Cluster** (2 days)
Active set, SIMD kernels, assignment, expiry, checkpoint. `/v1/clusters` and `/debug` live. Fixture replay harness in place.

**M4 — Propagation** (1–2 days)
Timeline, origin tracking, syndication groups, velocity, revision diffing. `/v1/clusters/{id}/timeline`, `/v1/syndication`, `/v1/items/{id}/revisions`. Debug page shows spread curves.

**M5 — Merge & tune** (1–2 days)
Merge pass, same-outlet guard, golden set evaluation, threshold tuning documented.

**M6 — Export & polish** (1 day)
NDJSON/Parquet export, `prune`, `README`, docs finalized. Tag `v0.1.0`. Data contract frozen at v1.

Total: roughly 9–11 working days to a usable v0.1.

### Status at v0.1.0

All six milestones plus an ingress hardening pass are implemented and
covered by 82 tests, including an offline end-to-end replay with a
golden output. What has not happened yet is a run against live feeds
with the real model: the environment this was built in could not reach
the feed hosts or the model download. The first live run decides the
tuning (`docs/clustering.md`) and will surface ingress failure modes
that only real feeds produce. Treat v0.1.0 as "complete and verified
offline", not "proven in production".

---

## 12. Open questions

- **Seed feed list.** Hand-curate ~30 for dev, or script an initial ~1,000 from a public outlet list? Recommend hand-curating for M1–M4, scaling at M5 once tuning is stable.
- **Multilingual.** MiniLM-L6 is English-only. A multilingual model (e.g. paraphrase-multilingual-MiniLM-L12) is ~3× slower and 118 MB. Recommend English-only for v0.1 with the model path configurable so swapping is trivial.
- **Coverage counting.** With syndication groups in the contract, `lens` can count either raw items or distinct syndication groups per cluster. `feed` emits both and takes no position.
- **Publish-date trust.** Feeds misreport `published_at` routinely. Should origin use `published_at` (what the outlet claims) or `fetched_at` (what we observed)? Current design: `published_at` with `fetched_at` tie-break, and both are in the timeline so `lens` can choose. Revisit once real data shows how often they disagree badly.
- **Checkpoint format vs. just rebuilding from SQLite.** Resolved at M3: both. The checkpoint is the fast path (custom binary, checksummed, atomic write); rebuild from embedded items in the window is the fallback when it is missing, corrupt, empty beside a backlog, or from a different embedding width.
- **Embedding backend for tests.** Resolved at M3: `embed.backend = "hash"` is a deterministic lexical stand-in so the replay golden test runs without a model or network. Production always uses `onnx`.
