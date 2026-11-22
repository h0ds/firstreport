//! Outlet icons for lens: each outlet's own mark, fetched once from its
//! home page and self-hosted in the site, so readers' browsers never ask a
//! third party for them. The icon is the outlet's identity, in its own
//! colours; it carries no rating (design §10).
//!
//! `feed icons` fetches through the same client as the poller (robots.txt,
//! opt-out list, per-host limits) and writes `<data>/icons/<outlet>.<ext>`;
//! `feed export` copies them into the export for lens.

use std::path::Path;

use url::Url;

use super::client::Client;
use super::opml::attrs;

/// Largest icon file kept.
pub const MAX_ICON_BYTES: usize = 256 * 1024;

/// Icons a page advertises, best first: apple-touch-icon (large, full
/// colour), then `icon` links by declared size (SVG counts as large), then
/// the conventional `/favicon.ico`.
pub fn candidates(html: &str, base: &Url) -> Vec<Url> {
    let lower = html.to_ascii_lowercase();
    let head_end = lower.find("</head>").unwrap_or(lower.len());
    let mut found: Vec<(i64, Url)> = Vec::new();
    let mut from = 0;
    while let Some(pos) = lower[from..head_end.max(from)].find("<link") {
        let start = from + pos + "<link".len();
        let Some(end_rel) = html[start..].find('>') else {
            break;
        };
        let body = &html[start..start + end_rel];
        from = start + end_rel + 1;
        let a = attrs(body);
        let get = |k: &str| {
            a.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.trim().to_string())
        };
        let rel = get("rel").unwrap_or_default().to_ascii_lowercase();
        let rels: Vec<&str> = rel.split_whitespace().collect();
        let touch = rels.iter().any(|r| r.starts_with("apple-touch-icon"));
        if !touch && !rels.contains(&"icon") {
            continue;
        }
        let Some(href) = get("href") else { continue };
        let Ok(url) = base.join(&href) else { continue };
        let svg = get("type").is_some_and(|t| t.contains("svg")) || url.path().ends_with(".svg");
        // Declared size, e.g. "32x32" or "16x16 32x32"; the largest counts.
        let size = get("sizes")
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|s| s.split(['x', 'X']).next()?.parse::<i64>().ok())
            .max()
            .unwrap_or(if touch {
                180
            } else if svg {
                256
            } else {
                16
            });
        let score = if touch { 10_000 + size } else { size };
        if !found.iter().any(|(_, u)| u == &url) {
            found.push((score, url));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut out: Vec<Url> = found.into_iter().map(|(_, u)| u).collect();
    if let Ok(ico) = base.join("/favicon.ico") {
        if !out.contains(&ico) {
            out.push(ico);
        }
    }
    out
}

/// The file extension for an image, judged by its bytes, not its name:
/// servers often answer an icon URL with an HTML error page.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    let head = &b[..b.len().min(512)];
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if head.starts_with(&[0, 0, 1, 0]) {
        Some("ico")
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if head.starts_with(b"GIF8") {
        Some("gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("webp")
    } else {
        let text = String::from_utf8_lossy(head).to_ascii_lowercase();
        let t = text.trim_start_matches('\u{feff}').trim_start();
        (t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg"))).then_some("svg")
    }
}

/// Existing icon file for an outlet, if any.
pub fn existing(dir: &Path, outlet_id: &str) -> Option<std::path::PathBuf> {
    ["png", "svg", "ico", "jpg", "gif", "webp"]
        .iter()
        .map(|e| dir.join(format!("{outlet_id}.{e}")))
        .find(|p| p.is_file())
}

/// Icon URLs from `config/icons.toml`, by outlet id, for outlets whose
/// home page cannot be read. A missing file means none.
pub fn known_urls(config_dir: &Path) -> Result<std::collections::HashMap<String, String>, String> {
    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        icon: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        outlet: String,
        url: String,
    }
    let path = config_dir.join("icons.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Default::default());
    };
    let f: File = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(f.icon.into_iter().map(|e| (e.outlet, e.url)).collect())
}

/// Fetch an icon from a known URL, judged by its bytes like any other.
pub async fn fetch_url(client: &Client, url: &str) -> Result<(Vec<u8>, &'static str), String> {
    let f = client
        .fetch(url, None, None)
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    let b = f.body.ok_or_else(|| format!("{url}: empty"))?;
    if b.len() > MAX_ICON_BYTES {
        return Err(format!("{url}: {} bytes, over the cap", b.len()));
    }
    sniff(&b)
        .map(|ext| (b.to_vec(), ext))
        .ok_or_else(|| format!("{url}: not an image"))
}

/// Fetch one outlet's icon: its home page, then the advertised icons in
/// order until one is an image. Returns the extension written.
pub async fn fetch_one(client: &Client, domain: &str) -> Result<(Vec<u8>, &'static str), String> {
    // The bare domain first, then www.: some sites answer on only one.
    let bare = domain.trim_start_matches("www.");
    let mut page = None;
    let mut home = String::new();
    let mut first_err = String::new();
    for host in [bare.to_string(), format!("www.{bare}")] {
        home = format!("https://{host}/");
        match client.fetch(&home, None, None).await {
            Ok(p) => {
                page = Some(p);
                break;
            }
            Err(e) if first_err.is_empty() => first_err = format!("home page: {e}"),
            Err(_) => {}
        }
    }
    let page = page.ok_or(first_err)?;
    let base = Url::parse(&home).map_err(|e| e.to_string())?;
    let html = page
        .body
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let base = page
        .resolved_url
        .as_deref()
        .and_then(|u| Url::parse(u).ok())
        .unwrap_or(base);
    let mut last = String::from("no icon advertised");
    for url in candidates(&html, &base) {
        match client.fetch(url.as_str(), None, None).await {
            Ok(f) => {
                let Some(b) = f.body else { continue };
                if b.len() > MAX_ICON_BYTES {
                    last = format!("{url}: {} bytes, over the cap", b.len());
                    continue;
                }
                match sniff(&b) {
                    Some(ext) => return Ok((b.to_vec(), ext)),
                    None => last = format!("{url}: not an image"),
                }
            }
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_icon_first_then_favicon() {
        let base = Url::parse("https://news.example/").unwrap();
        let html = r#"<html><head>
            <link rel="icon" href="/fav-16.png" sizes="16x16">
            <link rel="icon" type="image/png" href="/fav-32.png" sizes="32x32">
            <link rel="apple-touch-icon" href="/touch.png">
            <link rel="stylesheet" href="/s.css">
            </head><body><link rel="icon" href="/ignored-in-body.png"></body></html>"#;
        let got: Vec<String> = candidates(html, &base)
            .iter()
            .map(|u| u.path().to_string())
            .collect();
        assert_eq!(
            got,
            ["/touch.png", "/fav-32.png", "/fav-16.png", "/favicon.ico"]
        );
        // Nothing advertised: the conventional path only.
        let bare: Vec<String> = candidates("<html></html>", &base)
            .iter()
            .map(|u| u.to_string())
            .collect();
        assert_eq!(bare, ["https://news.example/favicon.ico"]);
    }

    #[test]
    fn images_are_judged_by_their_bytes() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(&[0, 0, 1, 0, 1, 0]), Some("ico"));
        assert_eq!(
            sniff(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>"),
            Some("svg")
        );
        assert_eq!(sniff(b"<?xml version='1.0'?><svg></svg>"), Some("svg"));
        assert_eq!(sniff(b"<!doctype html><title>404</title>"), None);
    }
}
