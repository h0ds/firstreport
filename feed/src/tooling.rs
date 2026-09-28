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

/// Feeds advertised by `page`, or the page itself if it is a feed, or
/// the conventional paths if nothing is advertised.
async fn discover_one(client: &Client, page: &str) -> Result<Vec<discover::Discovered>, String> {
    let base = Url::parse(page).map_err(|e| e.to_string())?;
    let fetched = client
        .fetch(page, None, None)
        .await
        .map_err(|e| e.to_string())?;
    let body = fetched.body.unwrap_or_default();
    if discover::looks_like_feed(&body) {
        return Ok(vec![discover::Discovered {
            url: page.to_string(),
            kind: "feed".into(),
            title: None,
        }]);
    }
    let mut found = discover::from_html(&String::from_utf8_lossy(&body), &base);
    if found.is_empty() {
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
                    break; // one conventional hit is enough
                }
            }
        }
    }
    Ok(found)
}

/// Keep at most `max` feeds per page: RSS and Atom first, JSON after,
/// document order otherwise. Outlets advertise comment feeds, per-section
/// feeds and podcasts; the first few are almost always the main ones.
pub fn pick_feeds(found: &[discover::Discovered], max: usize) -> Vec<discover::Discovered> {
    let rank = |d: &discover::Discovered| match d.kind.as_str() {
        "application/rss+xml" | "application/atom+xml" | "feed" => 0,
        _ => 1,
    };
    let mut v: Vec<(usize, usize, discover::Discovered)> = found
        .iter()
        .enumerate()
        .map(|(i, d)| (rank(d), i, d.clone()))
        .collect();
    v.sort_by_key(|(r, i, _)| (*r, *i));
    v.into_iter()
        .filter(|(_, _, d)| !d.url.to_ascii_lowercase().contains("comment"))
        .take(max)
        .map(|(_, _, d)| d)
        .collect()
}

pub async fn discover(
    config_dir: &Path,
    page: &str,
    feeds: &Feeds,
    outlets: &Outlets,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(4, 2)?;
    let found = discover_one(&client, page).await?;
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

/// One host per line. Tolerates `#` comments and the tab-separated
/// `aggregator-domains.txt` format (host first, counts after).
pub fn parse_hosts(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let host = line
            .split(['\t', ' ', ','])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let host = host
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        if !host.is_empty() && host.contains('.') && !out.contains(&host) {
            out.push(host);
        }
    }
    out
}

/// Walk a host list with bounded concurrency and propose feeds for all of
/// them at once. Hosts that yield nothing are listed at the end so they
/// can be retried or dropped by hand.
pub async fn discover_hosts(
    config_dir: &Path,
    hosts_path: &Path,
    feeds: &Feeds,
    outlets: &Outlets,
    write: bool,
    limit: Option<usize>,
    max_per_host: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut hosts = parse_hosts(&std::fs::read_to_string(hosts_path)?);
    if let Some(n) = limit {
        hosts.truncate(n);
    }
    let client = Client::new(8, 2)?;
    let total = hosts.len();
    eprintln!("discovering feeds for {total} hosts");

    // Bounded fan-out: the client's own per-host and global caps hold the
    // request rate; this bounds how many hosts are in flight.
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    for host in hosts {
        let client = client.clone();
        let sem = sem.clone();
        tasks.spawn(async move {
            let _p = sem.acquire().await.expect("semaphore");
            let page = format!("https://{host}/");
            let r = discover_one(&client, &page).await;
            (host, r)
        });
    }
    let mut results: Vec<(String, Result<Vec<discover::Discovered>, String>)> =
        Vec::with_capacity(total);
    let mut done = 0;
    while let Some(joined) = tasks.join_next().await {
        if let Ok(r) = joined {
            done += 1;
            if done % 25 == 0 || done == total {
                eprintln!("  {done}/{total}");
            }
            results.push(r);
        }
    }
    results.sort_by(|a, b| a.0.cmp(&b.0));

    let mut cands = Vec::new();
    let mut empty = Vec::new();
    let mut failed = Vec::new();
    for (host, r) in &results {
        match r {
            Ok(found) if !found.is_empty() => {
                for d in pick_feeds(found, max_per_host) {
                    if let Some(c) = candidate(&d.url, d.title.as_deref(), outlets) {
                        cands.push(c);
                    }
                }
            }
            Ok(_) => empty.push(host.clone()),
            Err(e) => failed.push(format!("{host}: {e}")),
        }
    }
    eprintln!(
        "{} feeds from {} hosts; {} hosts advertised nothing; {} failed",
        cands.len(),
        total - empty.len() - failed.len(),
        empty.len(),
        failed.len()
    );
    let (f, o) = render(&cands, feeds, outlets);
    emit(config_dir, write, &f, &o)?;
    if !empty.is_empty() {
        eprintln!("\n# no feeds found (try the outlet's own feed page by hand):");
        for h in &empty {
            eprintln!("#   {h}");
        }
    }
    if !failed.is_empty() {
        eprintln!("\n# failed:");
        for h in failed.iter().take(50) {
            eprintln!("#   {h}");
        }
    }
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
    fn hosts_file_formats() {
        let text = "# host\tarticles\tstories\napnews.com\t25\t25\nhttps://www.thehill.com/\n\nbad\napnews.com\t1\t1\n";
        assert_eq!(parse_hosts(text), vec!["apnews.com", "www.thehill.com"]);
    }

    #[test]
    fn pick_prefers_rss_atom_and_skips_comment_feeds() {
        let d = |url: &str, kind: &str| crate::fetch::discover::Discovered {
            url: url.into(),
            kind: kind.into(),
            title: None,
        };
        let found = vec![
            d("https://x/feed.json", "application/json"),
            d("https://x/comments/feed", "application/rss+xml"),
            d("https://x/rss", "application/rss+xml"),
            d("https://x/atom", "application/atom+xml"),
            d("https://x/sport/rss", "application/rss+xml"),
        ];
        let picked: Vec<String> = pick_feeds(&found, 2).into_iter().map(|d| d.url).collect();
        assert_eq!(picked, vec!["https://x/rss", "https://x/atom"]);
        assert_eq!(pick_feeds(&found, 10).len(), 4, "comment feed dropped");
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
