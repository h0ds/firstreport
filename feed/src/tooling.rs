//! Operator commands that grow the feed list: `import --opml` and
//! `discover <url>`. Both print TOML for `feeds.toml` and `outlets.toml`
//! and only touch the config files with `--write`, so the operator sees
//! what would change first.

use std::collections::BTreeMap;
use std::path::Path;

use url::Url;

use crate::config::{Feeds, Outlets};
use crate::fetch::{client::Client, discover, opml};

/// An outlet id guessed from a host: the registrable label, slugified.
/// `news.bbc.co.uk` → `bbc`, `www.theguardian.com` → `theguardian`.
pub fn outlet_slug(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    let labels: Vec<&str> = host
        .trim_start_matches("www.")
        .split('.')
        .filter(|l| !l.is_empty())
        .collect();
    let second_level = [
        "co", "com", "org", "net", "gov", "ac", "edu", "or", "ne", "go",
    ];
    let pick = match labels.len() {
        0 => "unknown",
        1 => labels[0],
        2 => labels[0],
        n => {
            let tld = labels[n - 1];
            let sld = labels[n - 2];
            if tld.len() == 2 && second_level.contains(&sld) {
                labels[n - 3]
            } else {
                sld
            }
        }
    };
    let slug: String = pick
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slug.trim_matches('-').to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub url: String,
    pub outlet_id: String,
    pub outlet_name: String,
    pub host: String,
}

fn candidate(url: &str, title: Option<&str>, existing: &Outlets) -> Option<Candidate> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.trim_start_matches("www.").to_string();
    let resolver = crate::parse::outlet::Resolver::from_config(existing);
    let resolved = resolver.resolve(&host);
    let (outlet_id, outlet_name) = if let Some(id) = resolved.strip_prefix("unknown:") {
        let _ = id;
        let slug = outlet_slug(&host);
        (slug.clone(), title.map(str::to_string).unwrap_or(slug))
    } else {
        let name = existing
            .outlets
            .iter()
            .find(|o| o.id == resolved)
            .map(|o| o.name.clone())
            .unwrap_or_default();
        (resolved, name)
    };
    Some(Candidate {
        url: url.to_string(),
        outlet_id,
        outlet_name,
        host,
    })
}

/// TOML fragments for feeds.toml and outlets.toml. Outlets already in the
/// registry or already emitted are not repeated; feeds already configured
/// are skipped.
pub fn render(cands: &[Candidate], feeds: &Feeds, outlets: &Outlets) -> (String, String) {
    let mut feeds_out = String::new();
    let mut outlets_out = String::new();
    let mut new_outlets: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for c in cands {
        if feeds.feeds.iter().any(|f| f.url == c.url) {
            continue;
        }
        feeds_out.push_str(&format!(
            "\n[[feed]]\nurl = {}\noutlet = {}\n",
            toml_str(&c.url),
            toml_str(&c.outlet_id)
        ));
        if !outlets.outlets.iter().any(|o| o.id == c.outlet_id) {
            let e = new_outlets
                .entry(c.outlet_id.clone())
                .or_insert_with(|| (c.outlet_name.clone(), vec![]));
            if !e.1.contains(&c.host) {
                e.1.push(c.host.clone());
            }
        }
    }
    for (id, (name, domains)) in new_outlets {
        let doms = domains
            .iter()
            .map(|d| toml_str(d))
            .collect::<Vec<_>>()
            .join(", ");
        outlets_out.push_str(&format!(
            "\n[[outlet]]\nid = {}\nname = {}\ndomains = [{doms}]\n",
            toml_str(&id),
            toml_str(&name)
        ));
    }
    (feeds_out, outlets_out)
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn emit(
    config_dir: &Path,
    write: bool,
    feeds_toml: &str,
    outlets_toml: &str,
) -> std::io::Result<()> {
    if feeds_toml.is_empty() {
        println!("# nothing new");
        return Ok(());
    }
    if write {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(config_dir.join("feeds.toml"))?
            .write_all(feeds_toml.as_bytes())?;
        if !outlets_toml.is_empty() {
            std::fs::OpenOptions::new()
                .append(true)
                .open(config_dir.join("outlets.toml"))?
                .write_all(outlets_toml.as_bytes())?;
        }
        println!(
            "appended to {} and {}",
            config_dir.join("feeds.toml").display(),
            config_dir.join("outlets.toml").display()
        );
    } else {
        println!("# feeds.toml{feeds_toml}");
        if !outlets_toml.is_empty() {
            println!("\n# outlets.toml{outlets_toml}");
        }
        println!("\n# re-run with --write to append these");
    }
    Ok(())
}

pub fn import_opml(
    config_dir: &Path,
    opml_path: &Path,
    feeds: &Feeds,
    outlets: &Outlets,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(opml_path)?;
    let entries = opml::parse(&text);
    let cands: Vec<Candidate> = entries
        .iter()
        .filter_map(|e| candidate(&e.url, e.title.as_deref(), outlets))
        .collect();
    eprintln!("{} feeds in {}", entries.len(), opml_path.display());
    let (f, o) = render(&cands, feeds, outlets);
    emit(config_dir, write, &f, &o)?;
    Ok(())
}

pub async fn discover(
    config_dir: &Path,
    page: &str,
    feeds: &Feeds,
    outlets: &Outlets,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Url::parse(page)?;
    let client = Client::new(4, 2)?;
    let fetched = client.fetch(page, None, None).await?;
    let body = fetched.body.unwrap_or_default();
    let mut found: Vec<discover::Discovered> = Vec::new();
    if discover::looks_like_feed(&body) {
        found.push(discover::Discovered {
            url: page.to_string(),
            kind: "feed".into(),
            title: None,
        });
    } else {
        found = discover::from_html(&String::from_utf8_lossy(&body), &base);
        if found.is_empty() {
            // Nothing advertised: probe the conventional paths.
            for p in discover::CONVENTIONAL_PATHS {
                let Ok(u) = base.join(p) else { continue };
                if let Ok(f) = client.fetch(u.as_str(), None, None).await {
                    if f.body
                        .as_deref()
                        .map(discover::looks_like_feed)
                        .unwrap_or(false)
                    {
                        found.push(discover::Discovered {
                            url: u.to_string(),
                            kind: "feed".into(),
                            title: None,
                        });
                    }
                }
            }
        }
    }
    eprintln!("{} feeds found at {page}", found.len());
    for d in &found {
        eprintln!(
            "  {} ({}{})",
            d.url,
            d.kind,
            d.title
                .as_deref()
                .map(|t| format!(", {t}"))
                .unwrap_or_default()
        );
    }
    let cands: Vec<Candidate> = found
        .iter()
        .filter_map(|d| candidate(&d.url, d.title.as_deref(), outlets))
        .collect();
    let (f, o) = render(&cands, feeds, outlets);
    emit(config_dir, write, &f, &o)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FeedEntry, OutletEntry};

    #[test]
    fn slugs() {
        assert_eq!(outlet_slug("news.bbc.co.uk"), "bbc");
        assert_eq!(outlet_slug("www.theguardian.com"), "theguardian");
        assert_eq!(outlet_slug("feeds.npr.org"), "npr");
        assert_eq!(outlet_slug("rss.nytimes.com"), "nytimes");
        assert_eq!(outlet_slug("example"), "example");
        assert_eq!(outlet_slug("abc.net.au"), "abc");
    }

    #[test]
    fn render_skips_known_and_groups_new_outlets() {
        let outlets = Outlets {
            outlets: vec![OutletEntry {
                id: "bbc".into(),
                name: "BBC".into(),
                domains: vec!["bbc.co.uk".into()],
            }],
        };
        let feeds = Feeds {
            feeds: vec![FeedEntry {
                url: "https://feeds.bbci.co.uk/x".into(),
                outlet: "bbc".into(),
                interval_secs: None,
                kind: None,
            }],
        };
        let cands: Vec<Candidate> = [
            ("https://feeds.bbci.co.uk/x", None),
            ("https://news.bbc.co.uk/rss", None),
            ("https://www.example.com/feed", Some("Example News")),
            ("https://cdn.example.com/atom", None),
        ]
        .iter()
        .filter_map(|(u, t)| candidate(u, *t, &outlets))
        .collect();
        assert_eq!(cands[1].outlet_id, "bbc", "resolved through the registry");
        assert_eq!(cands[2].outlet_id, "example");
        let (f, o) = render(&cands, &feeds, &outlets);
        assert!(!f.contains("feeds.bbci.co.uk/x"), "already configured");
        assert!(f.contains("url = \"https://news.bbc.co.uk/rss\"\noutlet = \"bbc\""));
        assert!(o.contains("id = \"example\"\nname = \"Example News\"\ndomains = [\"example.com\", \"cdn.example.com\"]"));
        assert!(!o.contains("id = \"bbc\""));
    }
}
