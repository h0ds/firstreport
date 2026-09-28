//! Host → outlet id resolution over the registry in `outlets.toml`.
//! Exact host match first, then longest suffix match. Unknown hosts map to
//! `unknown:<host>` and are still ingested (plan §5.1).

use std::collections::HashMap;

use crate::config::Outlets;

#[derive(Debug, Clone, Default)]
pub struct Resolver {
    /// domain (no leading `www.`) → outlet id
    by_domain: HashMap<String, String>,
}

impl Resolver {
    pub fn from_config(outlets: &Outlets) -> Self {
        let mut by_domain = HashMap::new();
        for o in &outlets.outlets {
            for d in &o.domains {
                let d = d.to_ascii_lowercase();
                let d = d.strip_prefix("www.").unwrap_or(&d).to_string();
                by_domain.insert(d, o.id.clone());
            }
        }
        Self { by_domain }
    }

    pub fn resolve(&self, host: &str) -> String {
        let host = host.to_ascii_lowercase();
        let host = host.strip_prefix("www.").unwrap_or(&host);
        if let Some(id) = self.by_domain.get(host) {
            return id.clone();
        }
        // suffix match: news.bbc.co.uk → bbc.co.uk
        let mut idx = 0;
        while let Some(dot) = host[idx..].find('.') {
            idx += dot + 1;
            if let Some(id) = self.by_domain.get(&host[idx..]) {
                return id.clone();
            }
        }
        format!("unknown:{host}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OutletEntry;

    fn r() -> Resolver {
        Resolver::from_config(&Outlets {
            outlets: vec![
                OutletEntry {
                    id: "bbc".into(),
                    name: "BBC".into(),
                    domains: vec!["bbc.co.uk".into(), "bbc.com".into()],
                },
                OutletEntry {
                    id: "guardian".into(),
                    name: "G".into(),
                    domains: vec!["www.theguardian.com".into()],
                },
            ],
        })
    }

    #[test]
    fn exact_suffix_and_unknown() {
        let r = r();
        assert_eq!(r.resolve("bbc.com"), "bbc");
        assert_eq!(r.resolve("www.bbc.co.uk"), "bbc");
        assert_eq!(r.resolve("feeds.bbci.co.uk"), "unknown:feeds.bbci.co.uk");
        assert_eq!(r.resolve("news.bbc.co.uk"), "bbc");
        assert_eq!(r.resolve("theguardian.com"), "guardian");
        assert_eq!(r.resolve("example.org"), "unknown:example.org");
    }
}
