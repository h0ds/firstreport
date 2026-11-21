//! robots.txt (RFC 9309), the publisher's rules for automated access.
//! Every fetch the poller and discovery make goes through a check:
//! a disallowed path is never requested, and a `Crawl-delay` stretches
//! the spacing between requests to that host. Feeds are published to
//! be polled and sitemaps to be crawled, so these rules rarely bite;
//! when they do, the publisher has said no and that is the answer.
//!
//! Group selection follows the RFC: the group whose `User-agent` token
//! is a case-insensitive prefix of ours (`firstreport-feed`) wins, else the
//! `*` group, else everything is allowed. Within a group the longest
//! matching rule wins and `Allow` beats `Disallow` at equal length.
//! `*` and `$` in rule paths are honoured.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    pub agents: Vec<String>,
    /// (allow, path pattern), in file order.
    pub rules: Vec<(bool, String)>,
    pub crawl_delay: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Robots {
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub allowed: bool,
    pub crawl_delay: Option<u64>,
}

/// Our token as it appears before the version in the User-Agent.
pub const OUR_TOKEN: &str = "firstreport-feed";

pub fn parse(text: &str) -> Robots {
    let mut groups: Vec<Group> = Vec::new();
    let mut current: Option<Group> = None;
    let mut last_was_agent = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "user-agent" => {
                if !last_was_agent {
                    if let Some(g) = current.take() {
                        groups.push(g);
                    }
                    current = Some(Group::default());
                }
                if let Some(g) = current.as_mut() {
                    g.agents.push(value.to_ascii_lowercase());
                }
                last_was_agent = true;
            }
            "allow" | "disallow" => {
                if let Some(g) = current.as_mut() {
                    if !value.is_empty() || key == "disallow" {
                        g.rules.push((key == "allow", value.to_string()));
                    }
                }
                last_was_agent = false;
            }
            "crawl-delay" => {
                if let Some(g) = current.as_mut() {
                    g.crawl_delay = value.parse::<f64>().ok().map(|d| d.ceil() as u64);
                }
                last_was_agent = false;
            }
            _ => {
                last_was_agent = false;
            }
        }
    }
    if let Some(g) = current.take() {
        groups.push(g);
    }
    Robots { groups }
}

/// Glob match with `*` (any run) and a trailing `$` (end anchor).
fn matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pattern, false),
    };
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else {
            match path[pos..].find(part) {
                Some(p) => pos += p + part.len(),
                None => return false,
            }
        }
    }
    if anchored {
        // the last part must reach the end
        parts.len() == 1 && pos == path.len() || parts.len() > 1 && pos == path.len()
    } else {
        true
    }
}

impl Robots {
    fn group_for(&self, token: &str) -> Option<&Group> {
        let token = token.to_ascii_lowercase();
        self.groups
            .iter()
            .find(|g| {
                g.agents
                    .iter()
                    .any(|a| a != "*" && token.starts_with(a.as_str()))
            })
            .or_else(|| {
                self.groups
                    .iter()
                    .find(|g| g.agents.iter().any(|a| a == "*"))
            })
    }

    /// Whether `path` (path plus query) may be fetched by `token`, and
    /// the group's crawl delay if any.
    pub fn check(&self, token: &str, path: &str) -> Decision {
        let Some(g) = self.group_for(token) else {
            return Decision {
                allowed: true,
                crawl_delay: None,
            };
        };
        let mut best: Option<(usize, bool)> = None;
        for (allow, pat) in &g.rules {
            if pat.is_empty() {
                continue; // `Disallow:` with nothing allows everything
            }
            if matches(pat, path) {
                let len = pat.len();
                match best {
                    Some((l, a)) if l > len || (l == len && a) => {}
                    _ => best = Some((len, *allow)),
                }
            }
        }
        Decision {
            allowed: best.map(|(_, a)| a).unwrap_or(true),
            crawl_delay: g.crawl_delay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# publisher rules
User-agent: *
Disallow: /admin/
Disallow: /search
Allow: /search/feed.xml
Crawl-delay: 2

User-agent: firstreport-feed
User-agent: other-bot
Disallow: /arc/outboundfeeds/
Allow: /arc/outboundfeeds/news-sitemap/

User-agent: badbot
Disallow: /
";

    #[test]
    fn groups_and_longest_match() {
        let r = parse(SAMPLE);
        assert_eq!(r.groups.len(), 3);
        // our own group wins over *
        assert!(
            !r.check("firstreport-feed", "/arc/outboundfeeds/rss/")
                .allowed
        );
        assert!(
            r.check(
                "firstreport-feed",
                "/arc/outboundfeeds/news-sitemap/?outputType=xml"
            )
            .allowed,
            "longer Allow wins"
        );
        assert_eq!(r.check("firstreport-feed", "/").crawl_delay, None);
        // an unknown bot gets the * group
        let d = r.check("somebody-else", "/search/feed.xml");
        assert!(d.allowed && d.crawl_delay == Some(2));
        assert!(!r.check("somebody-else", "/search?q=x").allowed);
        assert!(!r.check("somebody-else", "/admin/x").allowed);
        assert!(r.check("somebody-else", "/rss.xml").allowed);
        assert!(!r.check("badbot", "/anything").allowed);
        // no groups at all: everything allowed
        assert!(parse("").check("firstreport-feed", "/x").allowed);
        // an empty Disallow allows everything
        assert!(
            parse("User-agent: *\nDisallow:\n")
                .check("firstreport-feed", "/x")
                .allowed
        );
    }

    #[test]
    fn wildcards_and_anchors() {
        assert!(matches("/a/*/c", "/a/b/c"));
        assert!(matches("/a/*/c", "/a/b/x/cd"));
        assert!(!matches("/a/*/c", "/a/b"));
        assert!(matches("/*.pdf$", "/x/y.pdf"));
        assert!(!matches("/*.pdf$", "/x/y.pdf?x"));
        assert!(matches("/feed$", "/feed"));
        assert!(!matches("/feed$", "/feeds"));
        assert!(matches("/", "/anything"));
        let r = parse("User-agent: *\nDisallow: /*?utm_\n");
        assert!(!r.check("firstreport-feed", "/x?utm_source=a").allowed);
        assert!(r.check("firstreport-feed", "/x?q=a").allowed);
    }

    #[test]
    fn allow_beats_disallow_at_equal_length_and_delay_is_ceiled() {
        let r = parse("User-agent: *\nDisallow: /p\nAllow: /p\nCrawl-delay: 0.5\n");
        let d = r.check("firstreport-feed", "/p/x");
        assert!(d.allowed);
        assert_eq!(d.crawl_delay, Some(1));
    }
}
