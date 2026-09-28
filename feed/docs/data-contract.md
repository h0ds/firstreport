# Data contract v1

The emitted schema is the product. Everything else in `feed` may be
rewritten; this may only grow. Field additions are non-breaking. A
removal, rename or type change bumps `contract_version` (reported by
`GET /v1/stats`) and gets a new document.

Frozen at v1 with the v0.1.0 tag.

## Conventions

- Ids are unsigned 64-bit. JSON carries them as numbers; consumers in
  languages without native u64 should read them as strings or use the
  `0x` hex form the API accepts on input.
- Times are unix seconds, UTC. `0` means "not supplied by the feed".
- Text is plain: HTML stripped, entities decoded, whitespace collapsed.
- The same objects appear over the API (JSON), in NDJSON export, and in
  Parquet export. Parquet additionally carries `embedding` on items and
  `centroid` on clusters, which are never emitted over JSON.

## Outlet

```json
{
  "id": "bbc",
  "name": "BBC",
  "domains": ["bbc.co.uk", "bbc.com"],
  "feeds": ["https://feeds.bbci.co.uk/news/world/rss.xml"]
}
```

`id` is the seam with `ledger`: stable, hand-assigned, never derived
from content. `feed` attaches no rating, lean or quality signal to it,
and never will.

## Item

```json
{
  "id": 14623171176967558549,
  "outlet_id": "wire",
  "canonical_url": "https://wire.example/2024/01/01/central-bank-holds-rates",
  "title": "Central bank holds interest rates steady, signals cuts later this year",
  "summary": "The central bank kept its benchmark interest rate unchanged on Monday …",
  "published_at": 1704098400,
  "fetched_at": 1704099600,
  "lang": "en",
  "text_hash": 1234567890123456789,
  "cluster_id": 14623171176967558549,
  "cluster_rank": 0,
  "joined_at": 1704099600,
  "snapshot_id": 9876543210987654321
}
```

| field | notes |
|---|---|
| `id` | xxhash64 of `canonical_url`. Stable across refetches and revisions. |
| `canonical_url` | lowercase scheme and host; `www.`, `amp.`, `m.`, `mobile.` stripped; fragment, tracking and AMP parameters removed; remaining query sorted. The canonicalization is contract: changing it changes ids. |
| `title`, `summary` | the latest text. `summary` is at most 512 characters. Earlier versions are at `/v1/items/{id}/revisions`. |
| `published_at` | as the feed claimed, clamped to `fetched_at` when more than an hour in the future. Feeds lie; treat as evidence. |
| `lang` | ISO 639-1 or `und`. Feed hint first, detection second. |
| `text_hash` | 64-bit simhash of the normalized text as first seen. Not updated on revision, so syndication reflects first publication. |
| `cluster_id`, `cluster_rank`, `joined_at` | null until clustered. `cluster_rank` 0 is the cluster's seed. |
| `snapshot_id` | the raw feed fetch this item was parsed from; its body is at `/v1/snapshots/{id}/body`. |

## Cluster

```json
{
  "id": 14623171176967558549,
  "created_at": 1704099600,
  "last_active_at": 1704103200,
  "item_count": 3,
  "outlet_count": 3,
  "primary_title": "Central bank holds interest rates steady, signals cuts later this year",
  "origin_item_id": 14623171176967558549,
  "origin_outlet_id": "wire",
  "timeline": [
    { "item_id": 14623171176967558549, "outlet_id": "wire", "published_at": 1704098400, "joined_at": 1704099600 },
    { "item_id": 124067246807116879,   "outlet_id": "gazette", "published_at": 1704100200, "joined_at": 1704101400 },
    { "item_id": 9676264122735658276,  "outlet_id": "herald", "published_at": 1704102300, "joined_at": 1704103200 }
  ],
  "syndication": [
    { "text_hash": 1234567890123456789, "item_ids": [14623171176967558549, 124067246807116879], "first_outlet_id": "wire", "spread_secs": 1800 }
  ],
  "velocity": {
    "items_1h": 0, "items_6h": 3, "items_24h": 3,
    "outlets_1h": 0, "outlets_6h": 3, "outlets_24h": 3,
    "time_to_5_outlets_secs": null, "time_to_20_outlets_secs": null
  }
}
```

| field | notes |
|---|---|
| `id` | the seed item's id. Survives merges: the larger cluster's id is kept. |
| `primary_title` | title of the member nearest the centroid. |
| `origin_item_id`, `origin_outlet_id` | earliest `published_at`, tie-break earliest `fetched_at`. Can change when a late-fetched item claims an earlier date (`origin_changed` event). |
| `timeline` | every member in join order. Both timestamps are present so consumers can choose what to trust. |
| `syndication` | groups of members within `syndication.hamming_threshold` bits of simhash: the same copy, not the same story. Groups of one are omitted. |
| `velocity` | computed at read time from the timeline against the current clock. Never stored. |

Exports add `expired: bool`. `GET /v1/clusters/{id}` and
`?include=items` add `items: [Item]`.

## Snapshot

```json
{ "id": 9876543210987654321, "feed_url": "https://wire.example/rss", "fetched_at": 1704099600, "status": 200, "body_hash": 1111111111111111111, "item_count": 3 }
```

The raw body is at `/v1/snapshots/{id}/body`, zstd-decompressed. A
fetch whose body is identical to the feed's previous one records a row
that shares the previous file.

## Revision

```json
{ "snapshot_id": 9876543210987654321, "seen_at": 1704099600, "title": "…", "summary": "…" }
```

Oldest first. The first entry is the text as originally seen, written
when the first change is detected. An item that never changed has none.

## ClusterEvent

```json
{ "seq": 42, "ts": 1704101400, "kind": "item_added", "cluster_id": 14623171176967558549, "item_id": 124067246807116879, "merged_into": null, "snapshot_id": null }
```

`kind` is one of `created`, `item_added`, `merged`, `expired`,
`origin_changed`, `revised`, `withdrawn`. `seq` is monotonic and is the
cursor for `GET /v1/events?after_seq=`. `merged` sets `merged_into`;
`revised` and `withdrawn` set `snapshot_id` and leave `cluster_id` null.

## Feed (operational, not part of the analytic contract)

`GET /v1/feeds` describes poller state per feed: last status and error,
adapted interval, permanent moves, dead flag. It is for operating `feed`
and may change between versions without a contract bump.
