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

/// `feed icons`: each outlet's icon from its home page into `dir`,
/// through the poller's client rules (robots.txt, opt-out list). Outlets
/// with an icon already are skipped unless `refresh`. Prints one line per
/// outlet; a missing icon is not an error.
pub async fn icons(
    config_dir: &Path,
    outlets: &Outlets,
    dir: &Path,
    refresh: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::fetch::icons;
    std::fs::create_dir_all(dir)?;
    let client = Client::new(8, 2)?.with_opt_outs(crate::fetch::opt_out::load(config_dir)?);
    let (mut got, mut kept, mut missing) = (0, 0, 0);
    for o in &outlets.outlets {
        if !refresh && icons::existing(dir, &o.id).is_some() {
            kept += 1;
            continue;
        }
        let Some(domain) = o.domains.first() else { continue };
        match icons::fetch_one(&client, domain).await {
            Ok((bytes, ext)) => {
                if let Some(old) = icons::existing(dir, &o.id) {
                    let _ = std::fs::remove_file(old);
                }
                std::fs::write(dir.join(format!("{}.{ext}", o.id)), &bytes)?;
                println!("{}	{ext}	{} bytes", o.id, bytes.len());
                got += 1;
            }
            Err(e) => {
                println!("{}	none	{e}", o.id);
                missing += 1;
            }
        }
    }
    println!("# {got} fetched, {kept} already present, {missing} without an icon; in {}", dir.display());
    Ok(())
}

pub async fn discover(
    config_dir: &Path,
    page: &str,
    feeds: &Feeds,
    outlets: &Outlets,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(4, 2)?.with_opt_outs(crate::fetch::opt_out::load(config_dir)?);
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
    let client = Client::new(8, 2)?.with_opt_outs(crate::fetch::opt_out::load(config_dir)?);
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

// ---- `feed feeds`: the failing feeds, and fixing their URLs

/// A feed the poller could not read: failing now, or declared dead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failing {
    pub url: String,
    pub outlet_id: String,
    pub fail_count: u32,
    pub dead: bool,
    pub last_status: Option<u16>,
    pub last_error: Option<String>,
}

/// Feeds whose last attempt failed, from the store's feed table. A feed
/// that has never been fetched is not failing yet.
pub fn failing_feeds(states: &[crate::store::writer::FeedState]) -> Vec<Failing> {
    let mut out: Vec<Failing> = states
        .iter()
        .filter(|s| s.last_fetch_at.is_some() && (s.dead || s.fail_count > 0))
        .map(|s| Failing {
            url: s.url.clone(),
            outlet_id: s.outlet_id.clone(),
            fail_count: s.fail_count,
            dead: s.dead,
            last_status: s.last_status,
            last_error: s.last_error.clone(),
        })
        .collect();
    out.sort_by(|a, b| {
        b.dead
            .cmp(&a.dead)
            .then(b.fail_count.cmp(&a.fail_count))
            .then(a.url.cmp(&b.url))
    });
    out
}

/// `feeds.toml` with one entry's `url` replaced, everything else (order,
/// comments, other keys) untouched. None when `old` is not an entry's
/// url. Textual on purpose: the file is hand-edited and reviewed in git.
pub fn replace_feed_url(toml_text: &str, old: &str, new: &str) -> Option<String> {
    let needle_dq = format!("url = \"{old}\"");
    let needle_sq = format!("url = '{old}'");
    let mut out = String::with_capacity(toml_text.len() + 64);
    let mut replaced = false;
    for line in toml_text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let body = trimmed.trim_end_matches(['\r', '\n']);
        let eol = &trimmed[body.len()..];
        if !replaced && (body == needle_dq || body == needle_sq) {
            out.push_str(&format!("{indent}url = \"{new}\"{eol}"));
            replaced = true;
        } else {
            out.push_str(line);
        }
    }
    replaced.then_some(out)
}

/// A feed the steward is holding (docs/steward.md §2): it fetches and
/// parses, but its items are not ingested until `until`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quarantined {
    pub url: String,
    pub outlet_id: String,
    pub reason: String,
    pub until: i64,
    pub timestamps_unreliable: bool,
}

/// Feeds under quarantine at `now`, and feeds flagged for unreliable
/// timestamps, alphabetical.
pub fn quarantined_feeds(states: &[crate::store::writer::FeedState], now: i64) -> Vec<Quarantined> {
    let mut out: Vec<Quarantined> = states
        .iter()
        .filter_map(|s| {
            let h = s.health.as_ref()?;
            let q = h.quarantine.as_ref().filter(|q| now < q.until);
            if q.is_none() && !h.timestamps_unreliable {
                return None;
            }
            Some(Quarantined {
                url: s.url.clone(),
                outlet_id: s.outlet_id.clone(),
                reason: q.map(|q| q.reason.clone()).unwrap_or_default(),
                until: q.map(|q| q.until).unwrap_or(0),
                timestamps_unreliable: h.timestamps_unreliable,
            })
        })
        .collect();
    out.sort_by(|a, b| a.url.cmp(&b.url));
    out
}

/// Print the quarantined and flagged feeds, one per line. Nothing is
/// printed when there are none: `feed feeds` is about problems.
pub fn print_quarantined(q: &[Quarantined], now: i64) {
    for f in q {
        let mut what = Vec::new();
        if !f.reason.is_empty() {
            what.push(format!(
                "quarantined, {} ({} h left)",
                f.reason,
                ((f.until - now).max(0) + 1_800) / 3_600
            ));
        }
        if f.timestamps_unreliable {
            what.push("timestamps unreliable".to_string());
        }
        println!("{}\t{}\t{}", f.outlet_id, f.url, what.join("; "));
    }
    if !q.is_empty() {
        println!(
            "# {} held or flagged by the steward (docs/steward.md §2); nothing to fix by hand, a quarantine lifts itself after a day",
            q.len()
        );
    }
}

/// Print the failing feeds from the store, one per line.
pub fn print_failing(failing: &[Failing]) {
    if failing.is_empty() {
        println!("no failing feeds");
        return;
    }
    for f in failing {
        println!(
            "{}\t{}\t{}{}\t{}",
            f.outlet_id,
            f.url,
            if f.dead { "dead, " } else { "" },
            match f.last_status {
                Some(s) => format!("http {s}, {} failures", f.fail_count),
                None => format!("{} failures", f.fail_count),
            },
            f.last_error.as_deref().unwrap_or("")
        );
    }
    println!(
        "# {} failing; `feed feeds --fix` looks for replacements, `--write` applies them",
        failing.len()
    );
}

#[cfg(test)]
mod feeds_tests {
    use super::*;
    use crate::store::writer::FeedState;

    fn state(url: &str, fails: u32, dead: bool, status: Option<u16>) -> FeedState {
        FeedState {
            url: url.into(),
            outlet_id: "o".into(),
            kind: None,
            base_interval_secs: 900,
            interval_secs: 900,
            etag: None,
            last_modified: None,
            next_fetch_at: 0,
            fail_count: fails,
            last_ok_at: None,
            dead,
            resolved_url: None,
            last_fetch_at: Some(1),
            last_status: status,
            last_error: status.map(|s| format!("http {s}")),
            last_items: 0,
            health: None,
        }
    }

    #[test]
    fn failing_feeds_are_the_failed_and_dead_ones_worst_first() {
        let mut never = state("https://x/never", 0, false, None);
        never.last_fetch_at = None;
        let states = vec![
            state("https://x/ok", 0, false, Some(200)),
            state("https://x/gone", 3, false, Some(404)),
            state("https://x/dead", 40, true, Some(403)),
            never,
        ];
        let f = failing_feeds(&states);
        let urls: Vec<&str> = f.iter().map(|x| x.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x/dead", "https://x/gone"]);
        assert!(f[0].dead && f[1].last_status == Some(404));
    }

    #[test]
    fn quarantined_feeds_are_the_held_and_flagged_ones() {
        use crate::steward::{Health, Quarantine};
        let mut held = state("https://x/held", 0, false, Some(200));
        held.health = Some(Health {
            quarantine: Some(Quarantine {
                reason: "flood".into(),
                since: 0,
                until: 86_400,
            }),
            ..Default::default()
        });
        let mut lifted = state("https://x/lifted", 0, false, Some(200));
        lifted.health = Some(Health {
            quarantine: Some(Quarantine {
                reason: "old".into(),
                since: 0,
                until: 10,
            }),
            ..Default::default()
        });
        let mut flagged = state("https://x/flagged", 0, false, Some(200));
        flagged.health = Some(Health {
            timestamps_unreliable: true,
            ..Default::default()
        });
        let states = vec![
            state("https://x/ok", 0, false, Some(200)),
            held,
            lifted,
            flagged,
        ];
        let q = quarantined_feeds(&states, 100);
        let urls: Vec<&str> = q.iter().map(|x| x.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x/flagged", "https://x/held"]);
        assert!(q[0].reason.is_empty() && q[0].timestamps_unreliable);
        assert_eq!(q[1].reason, "flood");
        assert_eq!(q[1].until, 86_400);
    }

    #[test]
    fn replace_keeps_everything_else() {
        let toml = "# seed\n\n[[feed]]\nurl = \"https://a/old\"\noutlet = \"a\"\ninterval_secs = 600\n\n[[feed]]\nurl = \"https://b/feed\"\noutlet = \"b\"\n";
        let out = replace_feed_url(toml, "https://a/old", "https://a/new").unwrap();
        assert_eq!(out, "# seed\n\n[[feed]]\nurl = \"https://a/new\"\noutlet = \"a\"\ninterval_secs = 600\n\n[[feed]]\nurl = \"https://b/feed\"\noutlet = \"b\"\n");
        assert!(replace_feed_url(toml, "https://a/missing", "x").is_none());
        let crlf = "[[feed]]\r\nurl = \"https://a/old\"\r\noutlet = \"a\"\r\n";
        assert_eq!(
            replace_feed_url(crlf, "https://a/old", "https://a/new").unwrap(),
            "[[feed]]\r\nurl = \"https://a/new\"\r\noutlet = \"a\"\r\n"
        );
    }
}

/// For each failing feed, ask the outlet's home page which feeds it
/// advertises, verify the first candidate by fetching it, and propose
/// it as the replacement. Prints old → new; `write` rewrites the
/// entry in feeds.toml. A feed with no candidate is listed as such.
pub async fn fix_feeds(
    config_dir: &Path,
    outlets: &Outlets,
    failing: &[Failing],
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if failing.is_empty() {
        println!("no failing feeds");
        return Ok(());
    }
    let client = Client::new(4, 2)?.with_opt_outs(crate::fetch::opt_out::load(config_dir)?);
    let path = config_dir.join("feeds.toml");
    let mut text = std::fs::read_to_string(&path)?;
    let mut fixed = 0;
    let mut unfixed = Vec::new();
    for f in failing {
        let Some(outlet) = outlets.outlets.iter().find(|o| o.id == f.outlet_id) else {
            unfixed.push((f, "outlet not in outlets.toml".to_string()));
            continue;
        };
        let Some(domain) = outlet.domains.first() else {
            unfixed.push((f, "outlet has no domain".to_string()));
            continue;
        };
        let home = format!("https://{domain}/");
        let found = match discover_one(&client, &home).await {
            Ok(v) => v,
            Err(e) => {
                unfixed.push((f, format!("{home}: {e}")));
                continue;
            }
        };
        let candidates: Vec<discover::Discovered> = pick_feeds(&found, 5)
            .into_iter()
            .filter(|d| d.url != f.url)
            .collect();
        let mut chosen = None;
        for d in &candidates {
            match client.fetch(&d.url, None, None).await {
                Ok(r)
                    if r.body
                        .as_deref()
                        .map(discover::looks_like_feed)
                        .unwrap_or(false) =>
                {
                    chosen = Some(d.url.clone());
                    break;
                }
                _ => continue,
            }
        }
        match chosen {
            Some(new) => {
                println!("{}\t{}\t-> {}", f.outlet_id, f.url, new);
                if write {
                    match replace_feed_url(&text, &f.url, &new) {
                        Some(t) => {
                            text = t;
                            fixed += 1;
                        }
                        None => {
                            unfixed.push((f, "entry not found in feeds.toml (moved feed?)".into()))
                        }
                    }
                } else {
                    fixed += 1;
                }
            }
            None => unfixed.push((
                f,
                if found.is_empty() {
                    format!("{home} advertises no feed")
                } else {
                    "advertised feeds did not fetch as feeds".to_string()
                },
            )),
        }
    }
    for (f, why) in &unfixed {
        println!("{}\t{}\tno replacement: {why}", f.outlet_id, f.url);
    }
    if write {
        std::fs::write(&path, &text)?;
        println!(
            "# {fixed} replaced in {}, {} left; review the diff, then restart feed",
            path.display(),
            unfixed.len()
        );
    } else {
        println!(
            "# {fixed} replacements found, {} without; re-run with --write to apply",
            unfixed.len()
        );
    }
    Ok(())
}
