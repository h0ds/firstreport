# HTTP API v1

Read-only, JSON, on `api.bind` (default `127.0.0.1:8080`). No
authentication: bind to loopback, or put it behind a tunnel. Object
shapes are in `data-contract.md`.

## Pagination

List endpoints return `{ "items": [...], "next": "<cursor>" | null }`.
Pass `next` back as `cursor=` for the following page. Cursors are
`<sort_key>:<id>` and stay valid as data changes. `limit` defaults to
50, maximum 500.

Ids in paths accept decimal or `0x`-prefixed hex.

## Machine readers

`GET /v1/openapi.json` is an OpenAPI 3.1 description of every route,
parameter and response schema. The schemas are generated from the
serde types, so the document follows the code.
`GET /llms.txt` describes the server, its routes and rules of use for
agents; `GET /robots.txt` allows everything. `GET /v1/clusters/{id}`
with `Accept: text/markdown` returns a Markdown rendering of the
cluster with the same facts in the same order as the debug page. The
debug page links `/v1/clusters` and `/llms.txt` as alternates from its
`<head>`. See `design/DESIGN.md` §11.

## Errors

`{ "error": "message" }` with 400 for bad input, 404 for unknown ids,
500 for store failures.

## Endpoints

### Items

| route | query | returns |
|---|---|---|
| `GET /v1/items` | `since`, `outlet`, `limit`, `cursor` | items, newest fetched first |
| `GET /v1/items/{id}` | | one item, latest text |
| `GET /v1/items/{id}/revisions` | | `{ item_id, current, revisions: [Revision] }` |

### Clusters

| route | query | returns |
|---|---|---|
| `GET /v1/clusters` | `since`, `min_outlets`, `sort`, `limit`, `cursor`, `include=items` | clusters |
| `GET /v1/clusters/{id}` | | cluster with `items` |
| `GET /v1/clusters/{id}/timeline` | | `[Join]` only; cheap for spread curves |

`sort` is `recent` (default, by `last_active_at`, cursor-paged),
`velocity_1h` or `outlets_6h` (ranked by joins in the trailing window;
top-N, no cursor). `since` filters on `last_active_at`.

### Syndication

| route | query | returns |
|---|---|---|
| `GET /v1/syndication` | `text_hash` (required), `since`, `limit` | `{ text_hash, hamming_threshold, items }` across all clusters, earliest published first |

Scans items fetched since `since` (default 30 days). Not indexed; keep
`since` tight when calling often.

### Lookup

| route | query | returns |
|---|---|---|
| `GET /v1/lookup` | `url` and/or `text`, `since` | everything known about one URL or one piece of text |

`url` is canonicalized the way the poller does it, so tracking
parameters, `www.`, AMP and mobile forms all find the same item.
`matched` says how far the lookup got: `item` (the URL is an item, with
its `cluster`, `is_origin` and `outlet`), `text` (no item, but `text`
has near-copies by simhash over title and summary, fetched since
`since` (default 30 days ago), in `text_matches`,
and the earliest copy's cluster), `outlet` (no item, but the host
belongs to an outlet feed follows), or `none`. `checked` lists what was
searched in words, so a miss can be quoted as honestly as a hit. This
is the one call the reply layer builds on (`reply/docs/plan.md` §3).

### Snapshots

| route | query | returns |
|---|---|---|
| `GET /v1/snapshots` | `feed`, `since`, `limit`, `cursor` | fetch log |
| `GET /v1/snapshots/{id}` | | one row |
| `GET /v1/snapshots/{id}/body` | | raw feed body, `application/xml` |

### Events

| route | query | returns |
|---|---|---|
| `GET /v1/events` | `after_seq`, `limit` | `{ items: [ClusterEvent], last_seq }` |

Poll with `after_seq=<last_seq>` for incremental sync.

### Registry and health

| route | query | returns |
|---|---|---|
| `GET /v1/outlets` | | outlets with domains, feeds and their ledger links |
| `GET /v1/outlets/{id}` | | one outlet with `ledger` and `ledger_lookup` URLs, from `api.ledger_url` |
| `GET /v1/feeds` | `filter=all\|failing\|dead\|stale\|history` | per-feed poller state, failures first; feeds in the loaded `feeds.toml` only, unless `history` adds removed or replaced ones (`configured: false`) |
| `GET /v1/stats` | | `{ contract_version, process: {...}, store: {...} }` |

`process` counters include fetches, 304s, errors, items seen and new,
embed and assign latency percentiles, active clusters, merges,
revisions, withdrawals and clamped dates. `store` totals include items,
embedded items, snapshots, feeds, dead feeds, outlets and clusters.

### Debug page

`GET /debug` is a self-contained HTML page that polls `/v1/clusters` and
`/v1/stats` every five seconds. Not part of the API contract.

## Examples

```
curl 'localhost:8080/v1/clusters?min_outlets=3&sort=outlets_6h&limit=10'
curl 'localhost:8080/v1/clusters/0xcaf0a1b2c3d4e5f6/timeline'
curl 'localhost:8080/v1/events?after_seq=0&limit=500'
curl 'localhost:8080/v1/feeds?filter=failing'
curl 'localhost:8080/v1/lookup?url=https://www.example.com/story?utm_source=x'
```
