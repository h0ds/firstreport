//! `lens serve`: a static file server for looking at a build locally.
//! Bespoke and small: GET only, one request per connection, no
//! directory listings. Not for production; the site is files.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("json") => "application/json",
        Some("md") => "text/markdown; charset=utf-8",
        Some("css") => "text/css",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Map a request path to a file under `site`, refusing anything that
/// would escape it.
pub fn resolve(site: &Path, req_path: &str) -> Option<std::path::PathBuf> {
    let path = req_path.split('?').next().unwrap_or("/");
    if path.split('/').any(|seg| seg == "..") {
        return None;
    }
    let rel = path.trim_start_matches('/');
    let mut p = site.join(rel);
    if path.ends_with('/') || rel.is_empty() {
        p = p.join("index.html");
    }
    p.is_file().then_some(p)
}

pub fn run(site: &Path, bind: &str) -> Result<(), String> {
    let listener = TcpListener::bind(bind).map_err(|e| format!("bind {bind}: {e}"))?;
    println!("lens serving {} on http://{bind}/", site.display());
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        let mut buf = [0u8; 4096];
        let n = s.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("/")
            .to_string();
        let (status, ctype, body) = match resolve(site, &path) {
            Some(p) => (
                "200 OK",
                content_type(&p),
                std::fs::read(&p).unwrap_or_default(),
            ),
            None => (
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"not in the site\n".to_vec(),
            ),
        };
        let _ = write!(
            s,
            "HTTP/1.0 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = s.write_all(&body);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inside_the_site_only() {
        let site = std::env::temp_dir().join(format!("lens-serve-{}", std::process::id()));
        std::fs::create_dir_all(site.join("story/1")).unwrap();
        std::fs::write(site.join("story/1/index.html"), "x").unwrap();
        std::fs::write(site.join("tokens.css"), "x").unwrap();
        assert!(resolve(&site, "/story/1/").is_some());
        assert!(resolve(&site, "/story/1/index.html?x=1").is_some());
        assert!(resolve(&site, "/tokens.css").is_some());
        assert!(resolve(&site, "/story/2/").is_none());
        assert!(resolve(&site, "/../etc/passwd").is_none());
        assert_eq!(
            content_type(Path::new("a.md")),
            "text/markdown; charset=utf-8"
        );
        std::fs::remove_dir_all(&site).unwrap();
    }
}
