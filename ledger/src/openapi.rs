//! OpenAPI 3.1 description of the read-only API, served at
//! `/v1/openapi.json` so tool-using agents can call it correctly the
//! first time. Built in code next to the routes; the test checks every
//! documented path is routed.

use serde_json::{json, Value};

use crate::model::CONTRACT_VERSION;

pub fn spec() -> Value {
    let claim_props = json!({
        "outlet_id": {"type": "string", "description": "Kagami outlet id, shared with feed."},
        "field": {"type": "string", "description": "What the claim is about: owner, country, founded, bias, reliability, domain_registered, site_https, posting_cadence, … Open set; see ledger/docs/plan.md §3."},
        "value": {"type": "string", "description": "The source's wording, never Kagami's."},
        "data": {"type": "object", "additionalProperties": true, "description": "Structured form when one exists: qid, scope, window and counts for observed sources, page URLs for site checks."},
        "source_id": {"type": "string"},
        "ref_url": {"type": "string", "format": "uri", "description": "The page that states this."},
        "retrieved_at": {"type": "integer", "description": "Unix seconds when Kagami read it."},
        "as_of": {"type": "string", "description": "Date the source says the fact holds, if it says."},
        "note": {"type": "string"}
    });
    let mut claim_view_props = claim_props.clone();
    claim_view_props["age_days"] =
        json!({"type": "integer", "description": "Days since retrieved."});
    claim_view_props["stale"] = json!({"type": "boolean", "description": "Older than the source's refresh interval; the value may have changed since."});
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
            "schemas": {
                "Error": {"type": "object", "properties": {"error": {"type": "string"}}, "required": ["error"]},
                "Claim": {"type": "object", "properties": claim_props, "required": ["outlet_id", "field", "value", "source_id", "ref_url", "retrieved_at"]},
                "ClaimView": {"type": "object", "description": "A claim plus its freshness.", "properties": claim_view_props, "required": ["outlet_id", "field", "value", "source_id", "ref_url", "retrieved_at", "age_days", "stale"]},
                "OutletView": {"type": "object", "properties": {
                    "id": {"type": "string"},
                    "fields": {"type": "object", "additionalProperties": {"type": "array", "items": {"$ref": "#/components/schemas/ClaimView"}}, "description": "Field name to its claims from every source, side by side."},
                    "claims": {"type": "integer"},
                    "stale": {"type": "integer"}
                }, "required": ["id", "fields", "claims", "stale"]},
                "OutletSummary": {"type": "object", "properties": {
                    "id": {"type": "string"}, "claims": {"type": "integer"},
                    "fields": {"type": "array", "items": {"type": "string"}},
                    "sources": {"type": "array", "items": {"type": "string"}}
                }, "required": ["id", "claims", "fields", "sources"]},
                "SearchHit": {"type": "object", "properties": {
                    "id": {"type": "string"}, "name": {"type": "string"},
                    "domains": {"type": "array", "items": {"type": "string"}},
                    "claims": {"type": "integer", "description": "0 means in the registry but not assessed yet."},
                    "stale": {"type": "integer"},
                    "matched": {"type": "string", "enum": ["id", "name", "domain"]}
                }, "required": ["id", "name", "domains", "claims", "stale", "matched"]},
                "Source": {"type": "object", "properties": {
                    "id": {"type": "string"}, "name": {"type": "string"}, "url": {"type": "string"},
                    "licence": {"type": "string", "description": "SPDX id or a short name."},
                    "kind": {"type": "string", "enum": ["database", "community", "rating", "observed"]},
                    "notes": {"type": "string"},
                    "no_redistribution": {"type": "boolean", "description": "Terms forbid redistribution; such claims are never served."},
                    "refresh_days": {"type": "integer", "description": "0 means no schedule."}
                }, "required": ["id", "name", "url", "licence", "kind"]},
                "Stats": {"type": "object", "properties": {
                    "contract_version": {"type": "integer"}, "outlets": {"type": "integer"}, "claims": {"type": "integer"}, "sources": {"type": "integer"},
                    "claims_by_source": {"type": "object", "additionalProperties": {"type": "integer"}},
                    "claims_by_field": {"type": "object", "additionalProperties": {"type": "integer"}}
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
        .map(|p| p.replace("{id}", "bbc").replace("{field}", "country"))
        .collect()
}
