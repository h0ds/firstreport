//! `kagami-mcp`: a Model Context Protocol server over `feed` and
//! `ledger`, so an agent can ask where a story came from and what is
//! on record about an outlet, and get the same sourced answer a person
//! gets from the site.
//!
//! Transport is stdio: one JSON-RPC 2.0 message per line in, one per
//! line out. Every tool is a read of one route of the two local APIs;
//! nothing is written, nothing is fetched from anywhere else, and no
//! tool returns a verdict the APIs do not (`feed/docs/plan.md` rules).
//!
//!   KAGAMI_FEED_URL     default http://127.0.0.1:8080
//!   KAGAMI_LEDGER_URL   default http://127.0.0.1:8081

mod tools;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use tools::{Call, Service};

/// Protocol revisions this server speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Read-only provenance for news stories and outlets. \
Use `lookup` first when you have a URL or a quoted passage. Every fact carries its source, \
licence and date: keep them when you quote. Nothing here rates an outlet or says whether a \
story is true; a miss means only that this system has no record.";

/// Where a tool call is sent. A trait so the protocol can be tested
/// without the two servers running.
pub trait Backend {
    fn get(&self, call: &Call) -> impl std::future::Future<Output = Result<String, String>>;
}

struct Http {
    client: reqwest::Client,
    feed: String,
    ledger: String,
}

impl Backend for Http {
    async fn get(&self, call: &Call) -> Result<String, String> {
        let (base, name, start) = match call.service {
            Service::Feed => (&self.feed, "feed", "kagami run"),
            Service::Ledger => (&self.ledger, "ledger", "kagami run"),
        };
        let url = format!("{}{}", base.trim_end_matches('/'), call.path);
        let mut req = self.client.get(&url).query(&call.query);
        if call.markdown {
            req = req.header("Accept", "text/markdown");
        }
        let resp = req
            .send()
            .await
            .map_err(|_| format!("{name} is not answering at {base}. Start it with `{start}`."))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| format!("{name}: {e}"))?;
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or(body);
            return Err(format!("{name} answered {}: {msg}", status.as_u16()));
        }
        // JSON is re-indented so a model reads it as easily as Markdown.
        Ok(match serde_json::from_str::<Value>(&body) {
            Ok(v) if !call.markdown => serde_json::to_string_pretty(&v).unwrap_or(body),
            _ => body,
        })
    }
}

fn result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// One message in, at most one out. Notifications get no reply.
pub async fn handle(msg: &Value, backend: &impl Backend) -> Option<Value> {
    let method = msg.get("method").and_then(|m| m.as_str());
    let id = match msg.get("id") {
        Some(id) if !id.is_null() => id,
        // A notification, or a response to something we never asked.
        _ => return None,
    };
    let Some(method) = method else {
        return Some(error(id, -32600, "not a request"));
    };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(|v| v.as_str());
            let version = asked
                .filter(|v| VERSIONS.contains(v))
                .unwrap_or(VERSIONS[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "kagami", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({"tools": tools::list()})),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let out = match tools::plan(name, &args) {
                Ok(call) => backend.get(&call).await,
                Err(e) => Err(e),
            };
            // A failed tool is a result the model can read and act on,
            // not a protocol error.
            let (text, is_error) = match out {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            result(
                id,
                json!({"content": [{"type": "text", "text": text}], "isError": is_error}),
            )
        }
        other => error(id, -32601, &format!("unknown method {other:?}")),
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).is_some() {
        eprintln!(
            "usage: kagami-mcp   (no arguments; speaks MCP over stdin and stdout)\n\n\
             KAGAMI_FEED_URL     default http://127.0.0.1:8080\n\
             KAGAMI_LEDGER_URL   default http://127.0.0.1:8081"
        );
        std::process::exit(2);
    }
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let backend = Http {
        client: reqwest::Client::builder()
            .user_agent(concat!("kagami-mcp/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("http client"),
        feed: env("KAGAMI_FEED_URL", "http://127.0.0.1:8080"),
        ledger: env("KAGAMI_LEDGER_URL", "http://127.0.0.1:8081"),
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg, &backend).await,
            Err(_) => Some(error(&Value::Null, -32700, "not JSON")),
        };
        if let Some(r) = reply {
            let mut s = r.to_string();
            s.push('\n');
            if out.write_all(s.as_bytes()).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers with the call it was given, so a test sees the routing.
    struct Echo;
    impl Backend for Echo {
        async fn get(&self, call: &Call) -> Result<String, String> {
            if call.path.ends_with("/404") {
                return Err("feed answered 404: unknown id".into());
            }
            Ok(format!("{:?} {} {:?}", call.service, call.path, call.query))
        }
    }

    async fn ask(msg: Value) -> Option<Value> {
        handle(&msg, &Echo).await
    }

    #[tokio::test]
    async fn handshake_lists_tools_and_ignores_notifications() {
        let r = ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-03-26"}}))
        .await
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "kagami");
        let r = ask(json!({"jsonrpc": "2.0", "id": 2, "method": "initialize",
            "params": {"protocolVersion": "1999-01-01"}}))
        .await
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], VERSIONS[0]);
        assert!(
            ask(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .await
                .is_none()
        );
        let r = ask(json!({"jsonrpc": "2.0", "id": "a", "method": "tools/list"}))
            .await
            .unwrap();
        assert_eq!(r["id"], "a");
        let tools = r["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 6);
        for t in tools {
            assert!(t["name"].is_string() && t["description"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
            assert_eq!(t["annotations"]["readOnlyHint"], true);
        }
        let r = ask(json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}))
            .await
            .unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn calls_route_to_the_right_server_and_errors_are_results() {
        let call = |name: &str, args: Value| {
            ask(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": name, "arguments": args}}))
        };
        let r = call("lookup", json!({"url": "https://example.org/a?b=1"}))
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], false);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Feed /v1/lookup"), "{text}");
        assert!(text.contains("https://example.org/a?b=1"));
        let r = call("outlet", json!({"id": "bbc"})).await.unwrap();
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("Ledger /v1/outlets/bbc"));
        // Bad arguments, an unknown tool and a backend failure all come
        // back as readable tool results.
        for r in [
            call("lookup", json!({})).await.unwrap(),
            call("outlet", json!({"id": "../v1/stats"})).await.unwrap(),
            call("no_such_tool", json!({})).await.unwrap(),
            call("story", json!({"id": "404"})).await.unwrap(),
        ] {
            assert_eq!(r["result"]["isError"], true, "{r}");
            assert!(r.get("error").is_none());
        }
    }
}
