//! `lens check`: every internal link in every page resolves to a file
//! in the site, and every page has its JSON and Markdown twins.

use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Report {
    pub pages: usize,
    pub links: usize,
    pub problems: Vec<String>,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.file_name().is_some_and(|n| n == "index.html") {
            out.push(p);
        }
    }
    Ok(())
}

/// Site-relative targets of `href="/…"` and `src="/…"` in one page.
fn internal_links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["href=\"/", "src=\"/"] {
        let mut from = 0;
        while let Some(p) = html[from..].find(attr) {
            let start = from + p + attr.len() - 1;
            let end = html[start..]
                .find('"')
                .map(|e| start + e)
                .unwrap_or(html.len());
            let target = &html[start..end];
            if !target.starts_with("//") {
                out.push(target.split('#').next().unwrap_or(target).to_string());
            }
            from = end;
        }
    }
    out
}

fn resolves(site: &Path, target: &str) -> bool {
    let rel = target.trim_start_matches('/');
    let p = site.join(rel);
    if target.ends_with('/') || rel.is_empty() {
        p.join("index.html").is_file()
    } else {
        p.is_file()
    }
}

pub fn run(site: &Path) -> Result<Report, String> {
    let mut pages = Vec::new();
    walk(site, &mut pages)?;
    let mut r = Report::default();
    for page in pages {
        r.pages += 1;
        let dir = page.parent().unwrap();
        let shown = dir.strip_prefix(site).unwrap_or(dir).display();
        for twin in ["index.json", "index.md"] {
            if !dir.join(twin).is_file() {
                r.problems.push(format!("/{shown}/ has no {twin}"));
            }
        }
        let html =
            std::fs::read_to_string(&page).map_err(|e| format!("{}: {e}", page.display()))?;
        for target in internal_links(&html) {
            r.links += 1;
            if !resolves(site, &target) {
                r.problems.push(format!(
                    "/{shown}/ links to {target}, which is not in the site"
                ));
            }
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_internal_links_and_missing_targets() {
        let links = internal_links(
            "<a href=\"/story/1/\">x</a><link href=\"/tokens.css\"><a href=\"https://x/\">y</a><a href=\"//cdn/x\">z</a><a href=\"/outlet/a/#top\">",
        );
        assert_eq!(links, vec!["/story/1/", "/tokens.css", "/outlet/a/"]);
        let site = std::env::temp_dir().join(format!("lens-check-{}", std::process::id()));
        std::fs::create_dir_all(site.join("story/1")).unwrap();
        std::fs::write(site.join("story/1/index.html"), "<a href=\"/story/2/\">").unwrap();
        std::fs::write(site.join("story/1/index.json"), "{}").unwrap();
        let r = run(&site).unwrap();
        assert_eq!(r.pages, 1);
        assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
        assert!(r.problems.iter().any(|p| p.contains("no index.md")));
        assert!(r.problems.iter().any(|p| p.contains("/story/2/")));
        std::fs::remove_dir_all(&site).unwrap();
    }
}
