//! The opt-out list (docs/steward.md §5, docs/publisher-rules.md): hosts
//! that asked to be left alone, honoured before robots.txt is even read.
//! Read from `config/opt-out.toml`; a missing file is an empty list.

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptOut {
    #[serde(default, rename = "host")]
    pub hosts: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub host: String,
    pub reason: String,
    pub since: String,
}

pub const FILE: &str = "opt-out.toml";

/// Load the list from the config directory. Missing file: empty. A file
/// that does not parse is an error, since a silent empty list would
/// mean requests a publisher asked us not to make.
pub fn load(config_dir: &Path) -> Result<Vec<String>, String> {
    let path = config_dir.join(FILE);
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let parsed: OptOut = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(parsed
        .hosts
        .into_iter()
        .map(|e| {
            tracing::info!(host = %e.host, reason = %e.reason, since = %e.since, "opted out");
            normalise(&e.host)
        })
        .collect())
}

fn normalise(host: &str) -> String {
    host.trim()
        .trim_start_matches("*.")
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// Whether `host` is listed, itself or as a subdomain of a listed host.
pub fn covers(list: &[String], host: &str) -> bool {
    let h = normalise(host);
    list.iter()
        .any(|o| !o.is_empty() && (h == *o || h.ends_with(&format!(".{o}"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomains_are_covered_and_lookalikes_are_not() {
        let list = vec!["example.com".to_string(), "news.co.jp".to_string()];
        assert!(covers(&list, "example.com"));
        assert!(covers(&list, "feeds.example.com"));
        assert!(covers(&list, "FEEDS.Example.com."));
        assert!(!covers(&list, "notexample.com"));
        assert!(!covers(&list, "example.com.evil.net"));
        assert!(covers(&list, "www.news.co.jp"));
        assert!(!covers(&[], "example.com"));
    }

    #[test]
    fn shipped_file_parses_and_a_bad_one_is_an_error() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config");
        let list = load(&dir).unwrap();
        assert!(list.is_empty(), "no one has asked yet: {list:?}");
        let tmp = std::env::temp_dir().join(format!("kg-optout-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join(FILE),
            "[[host]]\nhost = \"*.Example.com\"\nreason = \"asked\"\nsince = \"2026-01-01\"\n",
        )
        .unwrap();
        assert_eq!(load(&tmp).unwrap(), vec!["example.com".to_string()]);
        std::fs::write(tmp.join(FILE), "[[host]]\nhost = \"x\"\n").unwrap();
        assert!(load(&tmp).is_err(), "an entry without a reason and date");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
