//! OpenAPI 3.1 description of the read-only API, served at
//! `/v1/openapi.json` so tool-using agents can call it correctly the
//! first time. Paths are written here next to the routes; the schemas
//! are generated from the serde types, so a struct change moves the
//! document with it. Tests check every documented path is routed and
//! every serialised field is in its schema.

use schemars::generate::SchemaSettings;
use schemars::JsonSchema;
use serde_json::{json, Map, Value};

use crate::api::{ApiError, ClaimView, OutletSummary, OutletView, SearchHit, Stats};
use crate::model::{Claim, Source, CONTRACT_VERSION};

/// `components.schemas`, generated. Each named type's root schema plus
/// whatever it references, with `$ref`s pointing into this map.
pub fn schemas() -> Map<String, Value> {
    // OpenAPI 3.1 takes JSON Schema 2020-12 as is; `openapi3()` would
    // emit 3.0's `nullable` instead of `type: [.., "null"]`.
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
    add::<Claim>(&mut g, &mut out, "Claim");
    add::<ClaimView>(&mut g, &mut out, "ClaimView");
    add::<OutletView>(&mut g, &mut out, "OutletView");
    add::<OutletSummary>(&mut g, &mut out, "OutletSummary");
    add::<SearchHit>(&mut g, &mut out, "SearchHit");
    add::<Source>(&mut g, &mut out, "Source");
    add::<Stats>(&mut g, &mut out, "Stats");
    for (k, v) in g.take_definitions(true) {
        out.entry(k).or_insert(v);
    }
    out
}

pub fn spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Kagami ledger",
            "version": format!("contract-{CONTRACT_VERSION}"),
            "summary": "Sourced facts about news outlets. Never a score.",
            "description": "Every value is a source's, with its licence, retrieval date and the page that states it. Sources that disagree are listed side by side, never merged. Claims from observed sources (domain registry, site checks, feed behaviour) are measurements with their window and counts attached. Rules of use are in /llms.txt.",
            "license": {"name": "See /v1/sources for each source's licence"}
        },
        "servers": [{"url": "/"}],
        "paths": {
            "/v1/outlets": {"get": {
                "operationId": "listOutlets",
                "summary": "Outlets that have claims, with counts, fields and sources",
                "responses": {"200": {"description": "OK", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/OutletSummary"}}}}}}
            }},
            "/v1/outlets/{id}": {"get": {
                "operationId": "getOutlet",
                "summary": "All claims for one outlet, grouped by field",
                "description": "Send Accept: text/markdown for a Markdown rendering with one provenance line per claim.",
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                "responses": {
                    "200": {"description": "OK", "content": {
                        "application/json": {"schema": {"$ref": "#/components/schemas/OutletView"}},
                        "text/markdown": {"schema": {"type": "string"}}
                    }},
                    "404": {"$ref": "#/components/responses/NotFound"}
                }
            }},
            "/v1/outlets/{id}/{field}": {"get": {
                "operationId": "getOutletField",
                "summary": "Claims for one field of one outlet",
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string"}},
                    {"name": "field", "in": "path", "required": true, "schema": {"type": "string"}}
                ],
                "responses": {
                    "200": {"description": "OK", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/ClaimView"}}}}},
                    "404": {"$ref": "#/components/responses/NotFound"}
                }
            }},
            "/v1/search": {"get": {
                "operationId": "searchOutlets",
                "summary": "Find an outlet by id, name or domain",
                "parameters": [
                    {"name": "q", "in": "query", "schema": {"type": "string"}},
                    {"name": "limit", "in": "query", "schema": {"type": "integer", "default": 20, "maximum": 100}}
                ],
                "responses": {"200": {"description": "OK", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/SearchHit"}}}}}}
            }},
            "/v1/sources": {"get": {
                "operationId": "listSources",
                "summary": "The source registry with licences and refresh intervals",
                "responses": {"200": {"description": "OK", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Source"}}}}}}
            }},
            "/v1/stats": {"get": {
                "operationId": "stats",
                "responses": {"200": {"description": "OK", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Stats"}}}}}
            }},
            "/v1/openapi.json": {"get": {"operationId": "openapi", "responses": {"200": {"description": "This document"}}}},
            "/llms.txt": {"get": {"operationId": "llms", "summary": "What this server is and its rules of use, for machine readers", "responses": {"200": {"description": "OK", "content": {"text/plain": {}}}}}},
            "/lookup": {"get": {"operationId": "lookupPage", "summary": "The same data as a page for people", "responses": {"200": {"description": "OK", "content": {"text/html": {}}}}}}
        },
        "components": {
            "responses": {
                "NotFound": {"description": "Unknown outlet or field", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}}
            },
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
        .map(|p| p.replace("{id}", "bbc").replace("{field}", "country"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Every `$ref` in the document points at a schema that exists.
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
    fn schemas_match_the_serialised_types() {
        let doc = spec();
        let schemas = doc["components"]["schemas"].as_object().unwrap();
        let mut r = Vec::new();
        refs(&doc, &mut r);
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
        let claim = Claim {
            outlet_id: "bbc".into(),
            field: "owner".into(),
            value: "BBC".into(),
            data: Some(BTreeMap::new()),
            source_id: "wikidata".into(),
            ref_url: "https://x".into(),
            retrieved_at: 1,
            as_of: Some("2024".into()),
            note: Some("n".into()),
        };
        let json = serde_json::to_value(&claim).unwrap();
        let props = schemas["Claim"]["properties"].as_object().unwrap();
        for k in json.as_object().unwrap().keys() {
            assert!(
                props.contains_key(k),
                "serialised field {k} missing from schema"
            );
        }
        assert_eq!(props.len(), json.as_object().unwrap().len());
        let required: Vec<&str> = schemas["Claim"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"ref_url") && !required.contains(&"note"));
        assert_eq!(
            schemas["Claim"]["properties"]["note"]["type"],
            json!(["string", "null"]),
            "3.1 null union, not 3.0 nullable"
        );
        assert_eq!(
            schemas["Claim"]["properties"]["ref_url"]["description"],
            "The page that states this."
        );
        let view = schemas["ClaimView"]["properties"].as_object().unwrap();
        assert!(
            view.contains_key("value") && view.contains_key("stale"),
            "flatten kept"
        );
        assert!(schemas["Source"]["properties"]["kind"]["$ref"]
            .as_str()
            .unwrap()
            .starts_with("#/components/schemas/"));
    }
}
