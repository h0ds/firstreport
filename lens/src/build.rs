//! Write the site. Into a fresh directory beside the target, then a
//! rename, so a half-built site is never served (plan §7).

use std::path::{Path, PathBuf};

use crate::input::{Input, Outlet};
use crate::pages::Twins;

fn write_twins(dir: &Path, t: &Twins) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let w = |name: &str, body: &str| {
        std::fs::write(dir.join(name), body)
            .map_err(|e| format!("{}: {e}", dir.join(name).display()))
    };
    w("index.html", &t.html)?;
    w(
        "index.json",
        &serde_json::to_string_pretty(&t.json).map_err(|e| e.to_string())?,
    )?;
    w("index.md", &t.md)
}

/// Outlet pages cover the window ledger's feed_behaviour importer uses.
pub const OUTLET_WINDOW_DAYS: i64 = 30;

/// One `<url>` per page, in build order. `base_url` may be empty, in
/// which case locations are site-relative, which most crawlers accept
/// from a same-host sitemap.
pub fn sitemap(pages: &[String], base_url: &str, export_time: i64) -> String {
    let base = base_url.trim_end_matches('/');
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    let lastmod = crate::render::date(export_time);
    for p in pages {
        s.push_str(&format!(
            "  <url><loc>{}{}</loc><lastmod>{lastmod}</lastmod></url>\n",
            crate::render::esc(base),
            crate::render::esc(p)
        ));
    }
    s.push_str("</urlset>\n");
    s
}

/// Where the self-hosted woff2 files live: `KAGAMI_FONTS`, else the
/// checkout's `design/fonts/` (populated by `design/fonts/fetch.sh`).
fn fonts_dir() -> PathBuf {
    std::env::var_os("KAGAMI_FONTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../design/fonts"))
}

/// Copy the woff2 files into `<site>/fonts/`, where `tokens.css` points.
/// Missing fonts are not an error: the stacks fall back to system faces.
fn copy_fonts(site: &Path) -> Result<usize, String> {
    let Ok(entries) = std::fs::read_dir(fonts_dir()) else {
        return Ok(0);
    };
    let dst = site.join("fonts");
    let mut n = 0;
    for p in entries.flatten().map(|e| e.path()) {
        if p.extension().is_some_and(|x| x == "woff2") {
            std::fs::create_dir_all(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
            let to = dst.join(p.file_name().unwrap_or_default());
            std::fs::copy(&p, &to).map_err(|e| format!("{}: {e}", to.display()))?;
            n += 1;
        }
    }
    Ok(n)
}

/// Returns the number of story and outlet pages written.
pub fn write_site(
    input: &Input,
    out: &Path,
    window_hours: i64,
    base_url: &str,
) -> Result<(usize, usize), String> {
    let tmp: PathBuf = out.with_file_name(format!(
        "{}.building-{}",
        out.file_name().and_then(|n| n.to_str()).unwrap_or("site"),
        std::process::id()
    ));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    }
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::write(tmp.join("tokens.css"), crate::render::TOKENS_CSS).map_err(|e| e.to_string())?;
    std::fs::write(tmp.join("lens.css"), crate::render::LENS_CSS).map_err(|e| e.to_string())?;
    copy_fonts(&tmp)?;
    std::fs::write(tmp.join("llms.txt"), crate::llms::llms_txt(input))
        .map_err(|e| e.to_string())?;
    std::fs::write(tmp.join("robots.txt"), crate::llms::ROBOTS_TXT).map_err(|e| e.to_string())?;
    write_twins(&tmp, &crate::pages::home::home(input, window_hours))?;
    write_twins(
        &tmp.join("search"),
        &crate::pages::search::search(input, window_hours),
    )?;
    write_twins(&tmp.join("about"), &crate::pages::about::about(input))?;
    let mut pages: Vec<String> = vec!["/".into(), "/search/".into(), "/about/".into()];
    // The story list, 50 per page: page 1 is `/`, the rest `/page/N/`.
    for p in 2..=crate::pages::home::page_count(input, window_hours) {
        write_twins(
            &tmp.join("page").join(p.to_string()),
            &crate::pages::home::story_page(input, window_hours, p),
        )?;
        pages.push(format!("/page/{p}/"));
    }
    let mut n = 0;
    for c in input.clusters.values() {
        if c.timeline.is_empty() {
            continue;
        }
        let t = crate::pages::story::story(input, c);
        write_twins(&tmp.join("story").join(c.id.to_string()), &t)?;
        pages.push(format!("/story/{}/", c.id));
        n += 1;
    }
    // Absorbed clusters keep their address as a stub to the survivor.
    let mut stubs = 0;
    for (absorbed, survivor) in crate::pages::stub::merges(input) {
        let has_page = input
            .clusters
            .get(&absorbed)
            .is_some_and(|c| !c.timeline.is_empty());
        if has_page || !input.clusters.contains_key(&survivor) {
            continue;
        }
        let t = crate::pages::stub::stub(input, absorbed, survivor);
        write_twins(&tmp.join("story").join(absorbed.to_string()), &t)?;
        pages.push(format!("/story/{absorbed}/"));
        stubs += 1;
    }
    let facts = crate::derive::outlet_facts(input, OUTLET_WINDOW_DAYS * crate::derive::DAY);
    let mut m = 0;
    // Every registry outlet gets a page, and so does any outlet ledger
    // knows that feed does not poll.
    let mut ids: Vec<&String> = input.outlets.keys().chain(input.claims.keys()).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let o = input.outlets.get(id).cloned().unwrap_or_else(|| Outlet {
            id: id.clone(),
            name: String::new(),
            domains: vec![],
        });
        let f = facts.get(id).cloned().unwrap_or_default();
        let t = crate::pages::outlet::outlet(input, &o, &f, OUTLET_WINDOW_DAYS);
        write_twins(&tmp.join("outlet").join(id), &t)?;
        pages.push(format!("/outlet/{id}/"));
        m += 1;
    }
    std::fs::write(
        tmp.join("sitemap.xml"),
        sitemap(&pages, base_url, input.export_time),
    )
    .map_err(|e| e.to_string())?;
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    std::fs::rename(&tmp, out).map_err(|e| format!("rename to {}: {e}", out.display()))?;
    Ok((n + stubs, m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::fixture_input;

    #[test]
    fn builds_every_story_with_its_twins() {
        let i = fixture_input();
        let out = std::env::temp_dir().join(format!("lens-site-{}", std::process::id()));
        let (n, m) = write_site(&i, &out, 24, "https://kagami.example").unwrap();
        assert_eq!((n, m), (63, 3), "61 stories and two merge stubs");
        assert!(out.join("story/900/index.html").exists());
        assert!(std::fs::read_to_string(out.join("story/900/index.json"))
            .unwrap()
            .contains("\"merged_into\": 1001"));
        assert!(out.join("tokens.css").exists());
        assert!(
            out.join("index.html").exists()
                && out.join("llms.txt").exists()
                && out.join("robots.txt").exists()
        );
        assert!(
            out.join("outlet/blog/index.md").exists(),
            "registry outlet without claims still has a page"
        );
        for c in i.clusters.keys() {
            let d = out.join("story").join(c.to_string());
            assert!(
                d.join("index.html").exists()
                    && d.join("index.json").exists()
                    && d.join("index.md").exists()
            );
        }
        // a rebuild replaces, never accumulates
        let again = write_site(&i, &out, 24, "https://kagami.example").unwrap();
        assert_eq!(again, (n, m));
        let r = crate::check::run(&out).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(r.links > 0);
        // golden twins: set LENS_UPDATE_GOLDEN=1 to regenerate after a
        // deliberate change to a page, then review the diff.
        let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/expected");
        for (built, name) in [
            ("story/1001/index.md", "story-1001.md"),
            ("outlet/wire/index.md", "outlet-wire.md"),
            ("outlet/gazette/index.html", "outlet-gazette.html"),
        ] {
            let got = std::fs::read_to_string(out.join(built)).unwrap();
            let want_path = golden.join(name);
            if std::env::var_os("LENS_UPDATE_GOLDEN").is_some() {
                std::fs::create_dir_all(&golden).unwrap();
                std::fs::write(&want_path, &got).unwrap();
            }
            let want = std::fs::read_to_string(&want_path)
                .unwrap_or_else(|_| panic!("missing golden {name}; run with LENS_UPDATE_GOLDEN=1"));
            // Line endings are git's business (a Windows checkout has CRLF).
            let (got, want) = (got.replace("\r\n", "\n"), want.replace("\r\n", "\n"));
            assert_eq!(got, want, "{name} differs from the golden copy");
        }
        std::fs::remove_dir_all(&out).unwrap();
    }
}
