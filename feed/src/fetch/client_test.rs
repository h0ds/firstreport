//! The client against a throwaway local HTTP server, so redirect
//! tracking, rate-limit handling and the User-Agent fallback are checked
//! without the network. The server is a few lines of std, no framework.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::client::{Client, FetchError};

const RSS: &str =
    "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title></channel></rss>";

struct Req {
    path: String,
    ua: String,
    etag: Option<String>,
}

fn read_req(stream: &mut std::net::TcpStream) -> Req {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut ua = String::new();
    let mut etag = None;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        if h.trim().is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("user-agent:") {
            ua = v.trim().to_string();
        }
        if let Some(v) = lower.strip_prefix("if-none-match:") {
            etag = Some(v.trim().to_string());
        }
    }
    Req { path, ua, etag }
}

fn respond(stream: &mut std::net::TcpStream, status: &str, headers: &[(&str, &str)], body: &str) {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

/// Serves until dropped. Returns the base URL and a request counter.
fn serve() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            h.fetch_add(1, Ordering::SeqCst);
            let req = read_req(&mut s);
            match req.path.as_str() {
                // Our group may fetch anything but /private and must wait a
                // second between requests; every other bot is kept out.
                "/robots.txt" => respond(
                    &mut s,
                    "200 OK",
                    &[],
                    "User-agent: truthdecay-feed\nDisallow: /private\nCrawl-delay: 1\n\nUser-agent: *\nDisallow: /\n",
                ),
                "/private" => respond(&mut s, "200 OK", &[], RSS),
                "/feed" => respond(&mut s, "200 OK", &[("ETag", "\"v1\"")], RSS),
                "/conditional" if req.etag.as_deref() == Some("\"v1\"") => {
                    respond(&mut s, "304 Not Modified", &[], "")
                }
                "/conditional" => respond(&mut s, "200 OK", &[("ETag", "\"v1\"")], RSS),
                "/moved" => respond(
                    &mut s,
                    "301 Moved Permanently",
                    &[("Location", "/feed")],
                    "",
                ),
                "/moved-twice" => respond(
                    &mut s,
                    "308 Permanent Redirect",
                    &[("Location", "/moved")],
                    "",
                ),
                "/temp" => respond(&mut s, "302 Found", &[("Location", "/feed")], ""),
                "/temp-then-perm" => respond(&mut s, "302 Found", &[("Location", "/moved")], ""),
                "/loop" => respond(
                    &mut s,
                    "301 Moved Permanently",
                    &[("Location", "/loop")],
                    "",
                ),
                "/limited" => respond(
                    &mut s,
                    "429 Too Many Requests",
                    &[("Retry-After", "90")],
                    "",
                ),
                "/down" => respond(&mut s, "503 Service Unavailable", &[], ""),
                "/picky" if req.ua.starts_with("mozilla/5.0 (compatible; truthdecay-feed/") => {
                    respond(&mut s, "200 OK", &[], RSS)
                }
                "/picky" => respond(&mut s, "403 Forbidden", &[], ""),
                "/gone" => respond(&mut s, "404 Not Found", &[], ""),
                _ => respond(&mut s, "404 Not Found", &[], ""),
            }
            let _ = s.read(&mut [0u8; 1]);
        }
    });
    (base, hits)
}

#[tokio::test]
async fn permanent_redirects_are_reported_temporary_ones_are_not() {
    let (base, _) = serve();
    let c = Client::new(4, 2).unwrap();

    let f = c.fetch(&format!("{base}/moved"), None, None).await.unwrap();
    assert_eq!(f.status, 200);
    assert_eq!(
        f.resolved_url.as_deref(),
        Some(format!("{base}/feed").as_str())
    );
    assert_eq!(f.body.unwrap().as_ref(), RSS.as_bytes());

    let f = c
        .fetch(&format!("{base}/moved-twice"), None, None)
        .await
        .unwrap();
    assert_eq!(
        f.resolved_url.as_deref(),
        Some(format!("{base}/feed").as_str()),
        "a chain of permanent hops resolves to the end"
    );

    let f = c.fetch(&format!("{base}/temp"), None, None).await.unwrap();
    assert_eq!(f.status, 200);
    assert_eq!(
        f.resolved_url, None,
        "temporary redirects leave the configured URL alone"
    );

    let f = c
        .fetch(&format!("{base}/temp-then-perm"), None, None)
        .await
        .unwrap();
    assert_eq!(
        f.resolved_url, None,
        "a temporary first hop means nothing permanent is known"
    );

    let e = c
        .fetch(&format!("{base}/loop"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(e, FetchError::Redirect(_)), "{e}");
}

#[tokio::test]
async fn conditional_requests_rate_limits_and_statuses() {
    let (base, _) = serve();
    let c = Client::new(4, 2).unwrap();

    let f = c
        .fetch(&format!("{base}/conditional"), None, None)
        .await
        .unwrap();
    assert_eq!(f.etag.as_deref(), Some("\"v1\""));
    let f = c
        .fetch(&format!("{base}/conditional"), Some("\"v1\""), None)
        .await
        .unwrap();
    assert_eq!(f.status, 304);
    assert!(f.body.is_none());

    let e = c
        .fetch(&format!("{base}/limited"), None, None)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Some(429));
    assert_eq!(e.retry_after(), Some(90));

    let e = c
        .fetch(&format!("{base}/down"), None, None)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Some(503));
    assert_eq!(e.retry_after(), None);

    let e = c
        .fetch(&format!("{base}/gone"), None, None)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Some(404));
}

#[tokio::test]
async fn user_agent_fallback_is_tried_once_then_remembered() {
    let (base, hits) = serve();
    let c = Client::new(4, 2).unwrap();

    let f = c.fetch(&format!("{base}/picky"), None, None).await.unwrap();
    assert_eq!(f.status, 200);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        3,
        "robots.txt, then the product token, then the compatible form"
    );

    let f = c.fetch(&format!("{base}/picky"), None, None).await.unwrap();
    assert_eq!(f.status, 200);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        4,
        "the host is remembered; no second 403 round trip"
    );

    // A host that still refuses gets exactly one extra attempt and stays failed.
    let (base2, hits2) = serve();
    let e = c
        .fetch(&format!("{base2}/gone"), None, None)
        .await
        .unwrap_err();
    assert_eq!(e.status(), Some(404));
    assert_eq!(
        hits2.load(Ordering::SeqCst),
        2,
        "robots.txt and one request: only 403 triggers the fallback"
    );
}

#[tokio::test]
async fn robots_txt_is_honoured_and_fetched_once() {
    let (base, hits) = serve();
    let c = Client::new(4, 2).unwrap();
    // The disallowed path is never requested: one hit, for robots.txt.
    let r = c.fetch(&format!("{base}/private"), None, None).await;
    assert!(matches!(r, Err(FetchError::Disallowed)), "{r:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    // Allowed paths go through, robots.txt is not fetched again, and
    // the crawl delay spaces the two requests.
    let t0 = std::time::Instant::now();
    c.fetch(&format!("{base}/feed"), None, None).await.unwrap();
    c.fetch(&format!("{base}/feed"), None, None).await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert!(
        t0.elapsed() >= std::time::Duration::from_secs(1),
        "crawl delay honoured"
    );
    let d = c.allowed(&format!("{base}/private")).await.unwrap();
    assert!(!d.allowed && d.crawl_delay == Some(1));
    assert_eq!(
        FetchError::Disallowed.to_string(),
        "disallowed by the host's robots.txt"
    );
}

#[tokio::test]
async fn request_budget_makes_the_third_request_wait() {
    use std::time::{Duration, Instant};
    let (base, hits) = serve();
    let c = Client::new(4, 2)
        .unwrap()
        .with_budgets(super::client::Budgets {
            per_host: 2,
            global: 0,
        })
        .with_budget_window(Duration::from_millis(2_500));
    let url = format!("{base}/feed");
    // The server's robots.txt asks for a second between requests, so the
    // window must outlast two of those for the budget to be what waits.
    let t0 = Instant::now();
    c.fetch(&url, None, None).await.unwrap();
    c.fetch(&url, None, None).await.unwrap();
    assert_eq!(c.budget_waits(), 0, "two fit the budget");
    c.fetch(&url, None, None).await.unwrap();
    assert_eq!(c.budget_waits(), 1, "the third waited for the window");
    assert!(
        t0.elapsed() >= Duration::from_millis(350),
        "{:?}",
        t0.elapsed()
    );
    // robots.txt plus three feed requests reached the server
    assert_eq!(hits.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn an_opted_out_host_is_never_requested() {
    let (base, hits) = serve();
    let c = Client::new(4, 2)
        .unwrap()
        .with_opt_outs(vec!["127.0.0.1".to_string()]);
    let err = c
        .fetch(&format!("{base}/feed"), None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::OptedOut), "{err}");
    assert!(matches!(
        c.allowed(&format!("{base}/robots.txt")).await.unwrap_err(),
        FetchError::OptedOut
    ));
    assert_eq!(hits.load(Ordering::SeqCst), 0, "not even robots.txt");
    assert_eq!(
        err.to_string(),
        "host asked to be left alone (config/opt-out.toml)"
    );
}

#[tokio::test]
async fn no_connection_is_unreachable_an_answer_is_not() {
    // A port nothing listens on: the connect is refused, nothing answered.
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let c = Client::new(4, 2).unwrap();
    let e = c
        .fetch(&format!("http://127.0.0.1:{closed}/feed.xml"), None, None)
        .await
        .unwrap_err();
    assert!(e.is_unreachable(), "refused connect is unreachable: {e}");

    let (base, _) = serve();
    let e = c.fetch(&format!("{base}/missing"), None, None).await.unwrap_err();
    assert!(!e.is_unreachable(), "a 404 is an answer: {e}");
}
