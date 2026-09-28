//! OpenAPI 3.1 description of the read-only API, served at
//! `/v1/openapi.json` so tool-using agents can call it correctly the
//! first time. Built in code next to the routes; the pipeline test
//! checks every documented path is routed. Object shapes follow
//! `docs/data-contract.md`.

use serde_json::{json, Value};

use crate::types::CONTRACT_VERSION;

fn u64_schema(desc: &str) -> Value {
    json!({"type": "integer", "format": "uint64", "description": format!("{desc} Unsigned 64-bit; read as a string in languages without u64. Paths accept decimal or 0x-hex.")})
}

fn ts(desc: &str) -> Value {
    json!({"type": "integer", "description": format!("{desc} Unix seconds UTC; 0 means not supplied by the feed.")})
}

fn page_of(schema_ref: &str) -> Value {
    json!({"type": "object", "properties": {
        "items": {"type": "array", "items": {"$ref": schema_ref}},
        "next": {"type": ["string", "null"], "description": "Pass back as cursor= for the next page."}
    }, "required": ["items", "next"]})
}

fn q(name: &str, schema: Value, desc: &str) -> Value {
    json!({"name": name, "in": "query", "schema": schema, "description": desc})
}

fn id_param() -> Value {
    json!({"name": "id", "in": "path", "required": true, "schema": {"type": "string"}, "description": "Decimal or 0x-prefixed hex u64."})
}

fn ok(schema: Value) -> Value {
    json!({"200": {"description": "OK", "content": {"application/json": {"schema": schema}}}})
}

fn ok_or_404(schema: Value) -> Value {
    let mut r = ok(schema);
    r["404"] = json!({"$ref": "#/components/responses/NotFound"});
    r
}

pub fn spec() -> Value {
    let limit = q(
        "limit",
        json!({"type": "integer", "default": 50, "maximum": 500}),
        "Page size.",
    );
    let cursor = q(
        "cursor",
        json!({"type": "string"}),
        "From the previous page's next.",
    );
    let since = q(
        "since",
        json!({"type": "integer"}),
        "Unix seconds; only newer.",
    );
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Kagami feed",
            "version": format!("contract-{CONTRACT_VERSION}"),
            "summary": "Headlines clustered into stories, with the record of how each spread. No outlet is rated.",
            "description": "A cluster is a claim that its items are about the same story, made by a similarity threshold, not a person; treat membership as evidence. origin_outlet_id is the earliest published member as the feeds reported it; feeds lie about times, joined_at is what Kagami saw. Syndication groups mean the same copy, not the same story. Sourced facts about each outlet are in Kagami ledger under the same outlet id; see the ledger links on outlets. Rules of use are in /llms.txt; object shapes in feed/docs/data-contract.md."
        },
        "servers": [{"url": "/"}],
        "paths": {
            "/v1/items": {"get": {"operationId": "listItems", "summary": "Headlines, newest fetched first",
                "parameters": [since.clone(), q("outlet", json!({"type": "string"}), "Outlet id."), limit.clone(), cursor.clone()],
                "responses": ok(page_of("#/components/schemas/Item"))}},
            "/v1/items/{id}": {"get": {"operationId": "getItem", "summary": "One item, latest text", "parameters": [id_param()],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Item"}))}},
            "/v1/items/{id}/revisions": {"get": {"operationId": "itemRevisions", "summary": "Earlier versions of an item's text, oldest first", "parameters": [id_param()],
                "responses": ok_or_404(json!({"type": "object", "properties": {
                    "item_id": u64_schema(""), "current": {"$ref": "#/components/schemas/Item"},
                    "revisions": {"type": "array", "items": {"$ref": "#/components/schemas/Revision"}, "description": "Empty means the text never changed."}
                }}))}},
            "/v1/syndication": {"get": {"operationId": "syndication", "summary": "Items sharing one text hash across outlets, earliest published first",
                "parameters": [q("text_hash", json!({"type": "string"}), "Required. The simhash from an item, decimal or hex."), since.clone(), limit.clone()],
                "responses": ok(json!({"type": "object", "properties": {
                    "text_hash": u64_schema(""), "hamming_threshold": {"type": "integer"},
                    "items": {"type": "array", "items": {"$ref": "#/components/schemas/Item"}}
                }}))}},
            "/v1/snapshots": {"get": {"operationId": "listSnapshots", "summary": "Raw feed fetches",
                "parameters": [q("feed", json!({"type": "string"}), "Feed URL."), since.clone(), limit.clone(), cursor.clone()],
                "responses": ok(page_of("#/components/schemas/Snapshot"))}},
            "/v1/snapshots/{id}": {"get": {"operationId": "getSnapshot", "parameters": [id_param()],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Snapshot"}))}},
            "/v1/snapshots/{id}/body": {"get": {"operationId": "snapshotBody", "summary": "The fetched feed body, decompressed", "parameters": [id_param()],
                "responses": {"200": {"description": "OK", "content": {"application/xml": {}, "text/plain": {}}}, "404": {"$ref": "#/components/responses/NotFound"}}}},
            "/v1/clusters": {"get": {"operationId": "listClusters", "summary": "Stories",
                "parameters": [since.clone(),
                    q("min_outlets", json!({"type": "integer", "default": 1}), "Only clusters with at least this many outlets."),
                    q("sort", json!({"type": "string", "enum": ["recent", "velocity_1h", "outlets_6h"], "default": "recent"}), "recent is cursor-paged by last_active_at; the others rank by joins in the trailing window and have no next."),
                    limit.clone(), cursor.clone(),
                    q("include", json!({"type": "string", "enum": ["items"]}), "Inline each cluster's items.")],
                "responses": ok(page_of("#/components/schemas/Cluster"))}},
            "/v1/clusters/{id}": {"get": {"operationId": "getCluster", "summary": "One story with its items",
                "description": "Send Accept: text/markdown for a Markdown rendering: title, metadata, items in join order with the first marked, syndication, velocity.",
                "parameters": [id_param()],
                "responses": {
                    "200": {"description": "OK", "content": {
                        "application/json": {"schema": {"$ref": "#/components/schemas/Cluster"}},
                        "text/markdown": {"schema": {"type": "string"}}}},
                    "404": {"$ref": "#/components/responses/NotFound"}}}},
            "/v1/clusters/{id}/timeline": {"get": {"operationId": "clusterTimeline", "summary": "Who joined when; cheap for spread curves", "parameters": [id_param()],
                "responses": ok_or_404(json!({"type": "array", "items": {"$ref": "#/components/schemas/Join"}}))}},
            "/v1/events": {"get": {"operationId": "events", "summary": "The cluster event log; a cursor for followers",
                "parameters": [q("after_seq", json!({"type": "integer"}), "Return events with seq greater than this."), limit.clone()],
                "responses": ok(json!({"type": "object", "properties": {
                    "items": {"type": "array", "items": {"$ref": "#/components/schemas/ClusterEvent"}},
                    "last_seq": {"type": ["integer", "null"], "description": "Pass back as after_seq."}}}))}},
            "/v1/feeds": {"get": {"operationId": "feeds", "summary": "Per-feed poller state, failures first. Operational; not part of the analytic contract.",
                "parameters": [q("filter", json!({"type": "string", "enum": ["all", "failing", "dead", "stale"], "default": "all"}), "stale means no success in 24 h.")],
                "responses": ok(json!({"type": "array", "items": {"$ref": "#/components/schemas/Feed"}}))}},
            "/v1/outlets": {"get": {"operationId": "listOutlets", "summary": "Outlets with domains, feeds and their ledger links",
                "responses": ok(json!({"type": "array", "items": {"$ref": "#/components/schemas/Outlet"}}))}},
            "/v1/outlets/{id}": {"get": {"operationId": "getOutlet", "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}, "description": "Outlet id, shared with Kagami ledger."}],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Outlet"}))}},
            "/v1/stats": {"get": {"operationId": "stats",
                "responses": ok(json!({"type": "object", "properties": {"contract_version": {"type": "integer"}, "process": {"type": "object", "additionalProperties": true}, "store": {"type": "object", "additionalProperties": {"type": "integer"}}}}))}},
            "/v1/openapi.json": {"get": {"operationId": "openapi", "responses": {"200": {"description": "This document"}}}},
            "/llms.txt": {"get": {"operationId": "llms", "summary": "What this server is and its rules of use, for machine readers", "responses": {"200": {"description": "OK", "content": {"text/plain": {}}}}}},
            "/debug": {"get": {"operationId": "debugPage", "summary": "The same data as a page for people", "responses": {"200": {"description": "OK", "content": {"text/html": {}}}}}}
        },
        "components": {
            "responses": {"NotFound": {"description": "Unknown id", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}}},
            "schemas": {
                "Error": {"type": "object", "properties": {"error": {"type": "string"}}, "required": ["error"]},
                "Outlet": {"type": "object", "properties": {
                    "id": {"type": "string", "description": "Stable, hand-assigned; the seam with ledger."},
                    "name": {"type": "string"},
                    "domains": {"type": "array", "items": {"type": "string"}},
                    "feeds": {"type": "array", "items": {"type": "string", "format": "uri"}},
                    "ledger": {"type": "string", "format": "uri", "description": "Ledger record for this id (JSON; Accept: text/markdown works there too). Absent when no ledger is configured."},
                    "ledger_lookup": {"type": "string", "format": "uri", "description": "The ledger's page for people."}
                }, "required": ["id", "name", "domains", "feeds"]},
                "Item": {"type": "object", "properties": {
                    "id": u64_schema("xxhash64 of canonical_url; stable across refetches and revisions."),
                    "outlet_id": {"type": "string"},
                    "canonical_url": {"type": "string", "format": "uri"},
                    "title": {"type": "string", "description": "Latest text; earlier versions at /v1/items/{id}/revisions."},
                    "summary": {"type": "string", "description": "Plain text, at most 512 characters, may be empty."},
                    "published_at": ts("As the feed claimed, clamped to fetched_at when over an hour in the future."),
                    "fetched_at": ts(""),
                    "lang": {"type": "string", "description": "ISO 639-1 or und."},
                    "text_hash": u64_schema("Simhash of the text as first seen; the syndication signal."),
                    "cluster_id": {"type": ["integer", "null"], "description": "null until clustered."},
                    "cluster_rank": {"type": ["integer", "null"], "description": "0 is the cluster's seed."},
                    "joined_at": {"type": ["integer", "null"]},
                    "snapshot_id": u64_schema("The raw fetch this was parsed from.")
                }, "required": ["id", "outlet_id", "canonical_url", "title", "summary", "published_at", "fetched_at", "lang", "text_hash", "snapshot_id"]},
                "Join": {"type": "object", "properties": {
                    "item_id": u64_schema(""), "outlet_id": {"type": "string"},
                    "published_at": ts("What the outlet claimed."), "joined_at": ts("What Kagami saw.")
                }, "required": ["item_id", "outlet_id", "published_at", "joined_at"]},
                "SynGroup": {"type": "object", "description": "Members within the hamming threshold of one simhash: the same copy, not the same story. Groups of one are omitted.", "properties": {
                    "text_hash": u64_schema(""), "item_ids": {"type": "array", "items": u64_schema("")},
                    "first_outlet_id": {"type": "string"}, "spread_secs": {"type": "integer"}
                }},
                "Velocity": {"type": "object", "description": "Computed at read time against the current clock; never stored.", "properties": {
                    "items_1h": {"type": "integer"}, "items_6h": {"type": "integer"}, "items_24h": {"type": "integer"},
                    "outlets_1h": {"type": "integer"}, "outlets_6h": {"type": "integer"}, "outlets_24h": {"type": "integer"},
                    "time_to_5_outlets_secs": {"type": ["integer", "null"]}, "time_to_20_outlets_secs": {"type": ["integer", "null"]}
                }},
                "Cluster": {"type": "object", "properties": {
                    "id": u64_schema("The seed item's id; survives merges."),
                    "created_at": ts(""), "last_active_at": ts(""),
                    "item_count": {"type": "integer"}, "outlet_count": {"type": "integer"},
                    "primary_title": {"type": "string", "description": "Title of the member nearest the centroid."},
                    "origin_item_id": u64_schema(""),
                    "origin_outlet_id": {"type": "string", "description": "Earliest published_at, tie-break earliest fetched_at. Can change when a late-fetched item claims an earlier date."},
                    "timeline": {"type": "array", "items": {"$ref": "#/components/schemas/Join"}, "description": "Every member in join order."},
                    "syndication": {"type": "array", "items": {"$ref": "#/components/schemas/SynGroup"}},
                    "velocity": {"$ref": "#/components/schemas/Velocity"},
                    "items": {"type": "array", "items": {"$ref": "#/components/schemas/Item"}, "description": "Present on /v1/clusters/{id} and with include=items."}
                }, "required": ["id", "created_at", "last_active_at", "item_count", "outlet_count", "primary_title", "origin_item_id", "origin_outlet_id", "timeline", "syndication", "velocity"]},
                "Snapshot": {"type": "object", "properties": {
                    "id": u64_schema(""), "feed_url": {"type": "string", "format": "uri"}, "fetched_at": ts(""),
                    "status": {"type": "integer"}, "body_hash": u64_schema("Identical bodies share a file."), "item_count": {"type": "integer"}
                }},
                "Revision": {"type": "object", "properties": {
                    "snapshot_id": u64_schema(""), "seen_at": ts(""), "title": {"type": "string"}, "summary": {"type": "string"}
                }},
                "ClusterEvent": {"type": "object", "properties": {
                    "seq": {"type": "integer", "description": "Monotonic; the cursor."}, "ts": ts(""),
                    "kind": {"type": "string", "enum": ["created", "item_added", "merged", "expired", "origin_changed", "revised", "withdrawn"]},
                    "cluster_id": {"type": ["integer", "null"]}, "item_id": {"type": ["integer", "null"]},
                    "merged_into": {"type": ["integer", "null"], "description": "Set on merged."},
                    "snapshot_id": {"type": ["integer", "null"], "description": "Set on revised and withdrawn."}
                }, "required": ["seq", "ts", "kind"]},
                "Feed": {"type": "object", "description": "Poller state; may change between versions without a contract bump.", "properties": {
                    "url": {"type": "string"}, "outlet_id": {"type": "string"}, "kind": {"type": ["string", "null"]},
                    "base_interval_secs": {"type": "integer"}, "interval_secs": {"type": "integer", "description": "After adaptation."},
                    "next_fetch_at": {"type": "integer"}, "fail_count": {"type": "integer"},
                    "last_ok_at": {"type": ["integer", "null"]}, "dead": {"type": "boolean"},
                    "resolved_url": {"type": ["string", "null"], "description": "Permanent move seen."},
                    "last_fetch_at": {"type": ["integer", "null"]}, "last_status": {"type": ["integer", "null"]},
                    "last_error": {"type": ["string", "null"]}, "last_items": {"type": "integer"},
                    "ok_age_secs": {"type": ["integer", "null"], "description": "Seconds since the last success."}
                }}
            }
        }
    })
}

/// Documented paths with a sample substituted for each parameter, for
/// the routing test.
#[cfg(test)]
pub fn sample_paths() -> Vec<String> {
    spec()["paths"]
        .as_object()
        .unwrap()
        .keys()
        .map(|p| p.replace("{id}", "1"))
        .collect()
}
