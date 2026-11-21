//! OpenAPI 3.1 description of the read-only API, served at
//! `/v1/openapi.json` so tool-using agents can call it correctly the
//! first time. Paths are written here next to the routes; the schemas
//! are generated from the serde types in `types.rs`, the store and the
//! route modules, so a struct change moves the document with it. The
//! pipeline test checks every documented path is routed; the test here
//! checks every reference resolves and the schemas match what is
//! serialised.

use schemars::generate::SchemaSettings;
use schemars::JsonSchema;
use serde_json::{json, Map, Value};

use super::routes::{
    ApiError, ClusterOut, EventsPage, FeedOut, LookupOut, OutletOut, Page, RevisionsOut, StatsOut,
    SyndicationOut, TextMatches,
};
use crate::store::reader::SnapshotMeta;
use crate::types::{Cluster, ClusterEvent, Item, Join, SynGroup, Velocity, CONTRACT_VERSION};

/// The `paths` section, built from the same route table the router is
/// built from (`routes::routes`), so a route cannot be served without
/// an entry here: an undocumented one is marked `x-undocumented` and
/// the test refuses it.
fn paths() -> Map<String, Value> {
    super::routes::routes()
        .into_iter()
        .map(|(path, _)| (path.to_string(), operation(path)))
        .collect()
}

fn operation(path: &str) -> Value {
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
    match path {
        "/v1/items" => {
            json!({"get": {"operationId": "listItems", "summary": "Headlines, newest fetched first",
                "parameters": [since.clone(), q("outlet", json!({"type": "string"}), "Outlet id."), limit.clone(), cursor.clone()],
                "responses": ok(page_of("ItemPage"))}})
        }
        "/v1/items/{id}" => {
            json!({"get": {"operationId": "getItem", "summary": "One item, latest text", "parameters": [id_param()],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Item"}))}})
        }
        "/v1/items/{id}/revisions" => {
            json!({"get": {"operationId": "itemRevisions", "summary": "Earlier versions of an item's text, oldest first", "parameters": [id_param()],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Revisions"}))}})
        }
        "/v1/syndication" => {
            json!({"get": {"operationId": "syndication", "summary": "Items sharing one text hash across outlets, earliest published first",
                "parameters": [q("text_hash", json!({"type": "string"}), "Required. The simhash from an item, decimal or hex."), since.clone(), limit.clone()],
                "responses": ok(json!({"$ref": "#/components/schemas/Syndication"}))}})
        }
        "/v1/lookup" => {
            json!({"get": {"operationId": "lookup", "summary": "Everything known about one URL or one piece of text, in one call",
                "description": "matched says how far the lookup got: item (the URL is an item, with its cluster, is_origin and outlet), text (near-copies of the text by simhash), outlet (unseen URL on an outlet feed follows), or none. checked lists what was searched.",
                "parameters": [q("url", json!({"type": "string"}), "An absolute http(s) URL; canonicalized as the poller does."), q("text", json!({"type": "string"}), "Text to match by simhash over title and summary."), since.clone()],
                "responses": ok(json!({"$ref": "#/components/schemas/Lookup"}))}})
        }
        "/v1/snapshots" => {
            json!({"get": {"operationId": "listSnapshots", "summary": "Raw feed fetches",
                "parameters": [q("feed", json!({"type": "string"}), "Feed URL."), since.clone(), limit.clone(), cursor.clone()],
                "responses": ok(page_of("SnapshotPage"))}})
        }
        "/v1/snapshots/{id}" => {
            json!({"get": {"operationId": "getSnapshot", "parameters": [id_param()],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Snapshot"}))}})
        }
        "/v1/snapshots/{id}/body" => {
            json!({"get": {"operationId": "snapshotBody", "summary": "The fetched feed body, decompressed", "parameters": [id_param()],
                "responses": {"200": {"description": "OK", "content": {"application/xml": {}, "text/plain": {}}}, "404": {"$ref": "#/components/responses/NotFound"}}}})
        }
        "/v1/clusters" => json!({"get": {"operationId": "listClusters", "summary": "Stories",
                "parameters": [since.clone(),
                    q("min_outlets", json!({"type": "integer", "default": 1}), "Only clusters with at least this many outlets."),
                    q("sort", json!({"type": "string", "enum": ["recent", "velocity_1h", "outlets_6h"], "default": "recent"}), "recent is cursor-paged by last_active_at; the others rank by joins in the trailing window and have no next."),
                    limit.clone(), cursor.clone(),
                    q("include", json!({"type": "string", "enum": ["items"]}), "Inline each cluster's items.")],
                "responses": ok(page_of("ClusterPage"))}}),
        "/v1/clusters/{id}" => {
            json!({"get": {"operationId": "getCluster", "summary": "One story with its items",
                "description": "Send Accept: text/markdown for a Markdown rendering: title, metadata, items in join order with the first marked, syndication, velocity.",
                "parameters": [id_param()],
                "responses": {
                    "200": {"description": "OK", "content": {
                        "application/json": {"schema": {"$ref": "#/components/schemas/Cluster"}},
                        "text/markdown": {"schema": {"type": "string"}}}},
                    "404": {"$ref": "#/components/responses/NotFound"}}}})
        }
        "/v1/clusters/{id}/timeline" => {
            json!({"get": {"operationId": "clusterTimeline", "summary": "Who joined when; cheap for spread curves", "parameters": [id_param()],
                "responses": ok_or_404(json!({"type": "array", "items": {"$ref": "#/components/schemas/Join"}}))}})
        }
        "/v1/events" => {
            json!({"get": {"operationId": "events", "summary": "The cluster event log; a cursor for followers",
                "parameters": [q("after_seq", json!({"type": "integer"}), "Return events with seq greater than this."), limit.clone()],
                "responses": ok(json!({"$ref": "#/components/schemas/Events"}))}})
        }
        "/v1/feeds" => {
            json!({"get": {"operationId": "feeds", "summary": "Per-feed poller state, failures first. Operational; not part of the analytic contract.",
                "parameters": [q("filter", json!({"type": "string", "enum": ["all", "failing", "dead", "stale", "quarantined", "history"], "default": "all"}), "stale means no success in 24 h; quarantined means the steward is holding the feed's items, with the reason and release time in health.quarantine. Only feeds in the loaded feeds.toml unless history, which adds rows for removed or replaced feeds (configured: false).")],
                "responses": ok(json!({"type": "array", "items": {"$ref": "#/components/schemas/Feed"}}))}})
        }
        "/v1/outlets" => {
            json!({"get": {"operationId": "listOutlets", "summary": "Outlets with domains, feeds and their ledger links",
                "responses": ok(json!({"type": "array", "items": {"$ref": "#/components/schemas/Outlet"}}))}})
        }
        "/v1/outlets/{id}" => {
            json!({"get": {"operationId": "getOutlet", "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}, "description": "Outlet id, shared with First Report ledger."}],
                "responses": ok_or_404(json!({"$ref": "#/components/schemas/Outlet"}))}})
        }
        "/v1/stats" => json!({"get": {"operationId": "stats",
                "responses": ok(json!({"$ref": "#/components/schemas/Stats"}))}}),
        "/v1/openapi.json" => {
            json!({"get": {"operationId": "openapi", "responses": {"200": {"description": "This document"}}}})
        }
        "/llms.txt" => {
            json!({"get": {"operationId": "llms", "summary": "What this server is and its rules of use, for machine readers", "responses": {"200": {"description": "OK", "content": {"text/plain": {}}}}}})
        }
        "/debug" => {
            json!({"get": {"operationId": "debugPage", "summary": "The same data as a page for people", "responses": {"200": {"description": "OK", "content": {"text/html": {}}}}}})
        }
        "/feeds" => {
            json!({"get": {"operationId": "networkPage", "summary": "Every feed and its health as a page for people; the data is /v1/feeds", "responses": {"200": {"description": "OK", "content": {"text/html": {}}}}}})
        }
        "/robots.txt" => {
            json!({"get": {"operationId": "robots", "summary": "Allows everything", "responses": {"200": {"description": "OK", "content": {"text/plain": {}}}}}})
        }
        other => json!({"get": {"operationId": other, "x-undocumented": true}}),
    }
}

/// `components.schemas`, generated. Each named type's root schema plus
/// whatever it references, with `$ref`s pointing into this map.
pub fn schemas() -> Map<String, Value> {
    // OpenAPI 3.1 takes JSON Schema 2020-12 as is.
    let mut settings = SchemaSettings::draft2020_12();
    settings.definitions_path = "/components/schemas".into();
    let mut g = settings.into_generator();
    let mut out = Map::new();
    fn add<T: JsonSchema>(
        g: &mut schemars::SchemaGenerator,
        out: &mut Map<String, Value>,
        name: &str,
    ) {
        let mut v = g.root_schema_for::<T>().to_value();
        if let Some(o) = v.as_object_mut() {
            o.remove("$schema");
            o.remove("title");
        }
        out.insert(name.to_string(), v);
    }
    add::<ApiError>(&mut g, &mut out, "Error");
    add::<Item>(&mut g, &mut out, "Item");
    add::<Join>(&mut g, &mut out, "Join");
    add::<SynGroup>(&mut g, &mut out, "SynGroup");
    add::<Velocity>(&mut g, &mut out, "Velocity");
    add::<Cluster>(&mut g, &mut out, "ClusterCore");
    add::<ClusterOut>(&mut g, &mut out, "Cluster");
    add::<SnapshotMeta>(&mut g, &mut out, "Snapshot");
    add::<ClusterEvent>(&mut g, &mut out, "ClusterEventCore");
    add::<RevisionsOut>(&mut g, &mut out, "Revisions");
    add::<SyndicationOut>(&mut g, &mut out, "Syndication");
    add::<TextMatches>(&mut g, &mut out, "TextMatches");
    add::<LookupOut>(&mut g, &mut out, "Lookup");
    add::<EventsPage>(&mut g, &mut out, "Events");
    add::<FeedOut>(&mut g, &mut out, "Feed");
    add::<OutletOut>(&mut g, &mut out, "Outlet");
    add::<StatsOut>(&mut g, &mut out, "Stats");
    add::<Page<Item>>(&mut g, &mut out, "ItemPage");
    add::<Page<ClusterOut>>(&mut g, &mut out, "ClusterPage");
    add::<Page<SnapshotMeta>>(&mut g, &mut out, "SnapshotPage");
    for (k, v) in g.take_definitions(true) {
        out.entry(k).or_insert(v);
    }
    out
}

fn page_of(name: &str) -> Value {
    json!({"$ref": format!("#/components/schemas/{name}")})
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
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "First Report feed",
            "version": format!("contract-{CONTRACT_VERSION}"),
            "summary": "Headlines clustered into stories, with the record of how each spread. No outlet is rated.",
            "description": "A cluster is a claim that its items are about the same story, made by a similarity threshold, not a person; treat membership as evidence. origin_outlet_id is the earliest published member as the feeds reported it; feeds lie about times, joined_at is what First Report saw. Syndication groups mean the same copy, not the same story. Sourced facts about each outlet are in First Report ledger under the same outlet id; see the ledger links on outlets. Rules of use are in /llms.txt; object shapes in feed/docs/data-contract.md."
        },
        "servers": [{"url": "/"}],
        "paths": paths(),
        "components": {
            "responses": {"NotFound": {"description": "Unknown id", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}}},
            "schemas": schemas()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(o) => {
                if let Some(Value::String(r)) = o.get("$ref") {
                    out.push(r.clone());
                }
                o.values().for_each(|x| refs(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }

    #[test]
    fn every_route_is_documented_once() {
        let routes = super::super::routes::routes();
        let mut paths: Vec<&str> = routes.iter().map(|(p, _)| *p).collect();
        paths.sort();
        let n = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), n, "duplicate route");
        for (p, op) in spec()["paths"].as_object().unwrap() {
            assert!(
                op["get"]["x-undocumented"].is_null(),
                "route {p} has no OpenAPI operation"
            );
        }
    }

    #[test]
    fn schemas_match_the_serialised_types() {
        let doc = spec();
        let schemas = doc["components"]["schemas"].as_object().unwrap();
        let mut r = Vec::new();
        refs(&doc, &mut r);
        assert!(!r.is_empty());
        for x in &r {
            let name = x.strip_prefix("#/components/").unwrap_or(x);
            let (kind, name) = name.split_once('/').unwrap();
            let ok = match kind {
                "schemas" => schemas.contains_key(name),
                "responses" => doc["components"]["responses"].get(name).is_some(),
                _ => false,
            };
            assert!(ok, "dangling reference {x}");
        }
        let item = Item {
            id: 1,
            outlet_id: "wire".into(),
            canonical_url: "https://wire.example/1".into(),
            title: "t".into(),
            summary: String::new(),
            published_at: 1,
            fetched_at: 2,
            lang: "en".into(),
            text_hash: 3,
            cluster_id: Some(1),
            cluster_rank: Some(0),
            joined_at: Some(2),
            join_similarity: Some(0.8),
            snapshot_id: 4,
        };
        let json = serde_json::to_value(&item).unwrap();
        let props = schemas["Item"]["properties"].as_object().unwrap();
        for k in json.as_object().unwrap().keys() {
            assert!(
                props.contains_key(k),
                "serialised field {k} missing from schema"
            );
        }
        assert_eq!(props.len(), json.as_object().unwrap().len());
        assert_eq!(props["id"]["format"], "uint64");
        assert_eq!(props["cluster_id"]["type"], json!(["integer", "null"]));
        assert_eq!(
            props["id"]["description"], "xxhash64 of `canonical_url`.",
            "doc comments become descriptions"
        );
        let cluster = schemas["Cluster"]["properties"].as_object().unwrap();
        assert!(
            cluster.contains_key("timeline") && cluster.contains_key("items"),
            "flatten kept"
        );
        assert!(
            !schemas["Snapshot"]["properties"]
                .as_object()
                .unwrap()
                .contains_key("path"),
            "serde(skip) honoured"
        );
        assert_eq!(
            schemas["ItemPage"]["properties"]["items"]["items"]["$ref"],
            "#/components/schemas/Item"
        );
    }
}
