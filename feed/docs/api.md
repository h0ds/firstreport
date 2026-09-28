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
| `GET /v1/outlets` | | outlets with domains and feeds |
| `GET /v1/feeds` | `filter=all\|failing\|dead\|stale` | per-feed poller state, failures first |
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
```
