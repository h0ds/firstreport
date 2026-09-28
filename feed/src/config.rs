//! Configuration loading for `feed`.
//!
//! Three files under `config/`: `feed.toml` (runtime settings), `feeds.toml`
//! (what to poll) and `outlets.toml` (host → outlet registry, shared with
//! `ledger`). All are plain TOML with serde defaults matching plan §6.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub poll: Poll,
    pub embed: Embed,
    pub cluster: Cluster,
    pub store: Store,
    pub snapshots: Snapshots,
    pub syndication: Syndication,
    pub api: Api,
    pub export: Export,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll: Poll::default(),
            embed: Embed::default(),
            cluster: Cluster::default(),
            store: Store::default(),
            snapshots: Snapshots::default(),
            syndication: Syndication::default(),
            api: Api::default(),
            export: Export::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Poll {
    pub default_interval_secs: u64,
    pub global_concurrency: usize,
    pub per_host_concurrency: usize,
    pub max_backoff_secs: u64,
}

impl Default for Poll {
    fn default() -> Self {
        Self {
            default_interval_secs: 900,
            global_concurrency: 64,
            per_host_concurrency: 2,
            max_backoff_secs: 21_600,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Embed {
    pub model_path: PathBuf,
    pub vocab_path: PathBuf,
    pub max_tokens: usize,
    pub batch_size: usize,
    pub batch_wait_ms: u64,
}

impl Default for Embed {
    fn default() -> Self {
        Self {
            model_path: "models/minilm-l6-int8.onnx".into(),
            vocab_path: "models/vocab.txt".into(),
            max_tokens: 128,
            batch_size: 64,
            batch_wait_ms: 200,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cluster {
    pub join_threshold: f32,
    pub merge_threshold: f32,
    pub window_secs: i64,
    pub expiry_secs: i64,
    pub merge_interval_secs: u64,
    pub max_cluster_size: u32,
    pub same_outlet_guard: bool,
    /// ISO 639-1 codes admitted to clustering. Empty means all.
    pub languages: Vec<String>,
}

impl Default for Cluster {
    fn default() -> Self {
        Self {
            join_threshold: 0.72,
            merge_threshold: 0.80,
            window_secs: 259_200,
            expiry_secs: 172_800,
            merge_interval_secs: 600,
            max_cluster_size: 500,
            same_outlet_guard: true,
            languages: vec!["en".into()],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Store {
    pub path: PathBuf,
    pub checkpoint_secs: u64,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            path: "data/feed.db".into(),
            checkpoint_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Snapshots {
    pub enabled: bool,
    pub dir: PathBuf,
    pub zstd_level: i32,
    pub retention_days: u32,
}

impl Default for Snapshots {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: "data/snapshots".into(),
            zstd_level: 3,
            retention_days: 180,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Syndication {
    pub hamming_threshold: u32,
}

impl Default for Syndication {
    fn default() -> Self {
        Self {
            hamming_threshold: 3,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Api {
    pub bind: String,
}

impl Default for Api {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Export {
    pub nightly: bool,
    pub out_dir: PathBuf,
}

impl Default for Export {
    fn default() -> Self {
        Self {
            nightly: false,
            out_dir: "data/export".into(),
        }
    }
}

/// One entry in `feeds.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedEntry {
    pub url: String,
    pub outlet: String,
    pub interval_secs: Option<u64>,
    pub kind: Option<crate::types::FeedKind>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Feeds {
    #[serde(default, rename = "feed")]
    pub feeds: Vec<FeedEntry>,
}

/// One entry in `outlets.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutletEntry {
    pub id: String,
    pub name: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Outlets {
    #[serde(default, rename = "outlet")]
    pub outlets: Vec<OutletEntry>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    /// A feed references an outlet id that is not in `outlets.toml`.
    UnknownOutlet { feed_url: String, outlet: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(p, e) => write!(f, "reading {}: {e}", p.display()),
            ConfigError::Parse(p, e) => write!(f, "parsing {}: {e}", p.display()),
            ConfigError::UnknownOutlet { feed_url, outlet } => {
                write!(f, "feed {feed_url} references unknown outlet {outlet:?}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(path.into(), e))?;
    toml::from_str(&text).map_err(|e| ConfigError::Parse(path.into(), e))
}

/// Everything loaded from the config directory, cross-validated.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    pub feeds: Feeds,
    pub outlets: Outlets,
}

/// Load `feed.toml`, `feeds.toml` and `outlets.toml` from `dir`. A missing
/// `feed.toml` falls back to defaults; the other two are required.
pub fn load(dir: &Path) -> Result<Loaded, ConfigError> {
    let feed_toml = dir.join("feed.toml");
    let config: Config = if feed_toml.exists() {
        read_toml(&feed_toml)?
    } else {
        Config::default()
    };
    let feeds: Feeds = read_toml(&dir.join("feeds.toml"))?;
    let outlets: Outlets = read_toml(&dir.join("outlets.toml"))?;

    for f in &feeds.feeds {
        if !outlets.outlets.iter().any(|o| o.id == f.outlet) {
            return Err(ConfigError::UnknownOutlet {
                feed_url: f.url.clone(),
                outlet: f.outlet.clone(),
            });
        }
    }

    Ok(Loaded {
        config,
        feeds,
        outlets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_plan() {
        let c = Config::default();
        assert_eq!(c.cluster.join_threshold, 0.72);
        assert_eq!(c.cluster.merge_threshold, 0.80);
        assert_eq!(c.poll.default_interval_secs, 900);
        assert_eq!(c.api.bind, "127.0.0.1:8080");
    }

    #[test]
    fn shipped_config_dir_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("config");
        let loaded = load(&dir).expect("shipped config must load");
        assert!(!loaded.feeds.feeds.is_empty());
        assert!(!loaded.outlets.outlets.is_empty());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let err = toml::from_str::<Config>("[poll]\nnope = 1\n").unwrap_err();
        assert!(err.to_string().contains("nope"));
    }
}
