//! Configuration loading for `feed`.
//!
//! Three files under `config/`: `feed.toml` (runtime settings), `feeds.toml`
//! (what to poll) and `outlets.toml` (host → outlet registry, shared with
//! `ledger`). All are plain TOML with serde defaults matching plan §6.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
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
    /// From `config/opt-out.toml`, not `feed.toml`: hosts never requested.
    #[serde(skip)]
    pub opt_out_hosts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Poll {
    pub default_interval_secs: u64,
    pub global_concurrency: usize,
    pub per_host_concurrency: usize,
    pub max_backoff_secs: u64,
    /// Shorten a feed's interval when it yields new items, lengthen it
    /// when it doesn't, within the bounds below.
    pub adaptive: bool,
    pub min_interval_secs: u64,
    pub max_interval_secs: u64,
    /// Fraction of each wait randomized (deterministically) so feeds on
    /// the same interval don't burst together. 0 disables.
    pub jitter: f64,
    /// Request budgets (docs/steward.md §5): feed requests per host per
    /// hour and in total; a request over budget waits. 0 disables one.
    pub per_host_requests_per_hour: u32,
    pub global_requests_per_hour: u32,
}

impl Default for Poll {
    fn default() -> Self {
        Self {
            default_interval_secs: 900,
            global_concurrency: 64,
            per_host_concurrency: 2,
            max_backoff_secs: 21_600,
            adaptive: true,
            min_interval_secs: 300,
            max_interval_secs: 3_600,
            jitter: 0.1,
            per_host_requests_per_hour: 240,
            global_requests_per_hour: 20_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Embed {
    /// `onnx` runs the configured model; `hash` is a deterministic
    /// feature-hashing embedder with no model, for offline replay and CI.
    pub backend: EmbedBackend,
    pub model_path: PathBuf,
    pub vocab_path: PathBuf,
    /// ONNX Runtime shared library, loaded at startup. Never fetched at
    /// build time. Default depends on the OS (`.so` or `.dylib`).
    pub runtime_lib: PathBuf,
    pub max_tokens: usize,
    pub batch_size: usize,
    pub batch_wait_ms: u64,
    /// `mean` (MiniLM, gte, e5) or `cls` (bge).
    pub pooling: Pooling,
    /// `wordpiece` (BERT-family models, `vocab.txt`) or `unigram`
    /// (XLM-RoBERTa-family models, `sentencepiece.bpe.model`).
    pub tokenizer: TokenizerKind,
    /// Which published model `fetch-model` downloads when no URL is
    /// set; also the documented pairing of tokenizer and pooling.
    pub preset: ModelPreset,
    /// Text handed to the model. `{title}` and `{summary}` are substituted;
    /// a summary-only segment is dropped when the summary is empty.
    pub input_template: String,
    /// Width of the `hash` backend's vectors.
    pub hash_dim: usize,
    /// Sources for `feed fetch-model`. Empty means "pick by target triple".
    pub model_url: String,
    pub vocab_url: String,
    /// Tarball containing the runtime library under `lib/`.
    pub runtime_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenizerKind {
    WordPiece,
    Unigram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ModelPreset {
    /// `all-MiniLM-L6-v2`: English, 384-wide, WordPiece, ~22 MB int8.
    #[serde(rename = "minilm-l6")]
    MinilmL6,
    /// `paraphrase-multilingual-MiniLM-L12-v2`: 50 languages including
    /// Japanese, 384-wide, XLM-RoBERTa unigram tokenizer, ~120 MB int8.
    #[serde(rename = "multilingual-minilm-l12")]
    MultilingualMinilmL12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbedBackend {
    Onnx,
    Hash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pooling {
    Mean,
    Cls,
}

impl Default for Embed {
    fn default() -> Self {
        let lib = if cfg!(target_os = "macos") {
            "models/libonnxruntime.dylib"
        } else if cfg!(windows) {
            "models/onnxruntime.dll"
        } else {
            "models/libonnxruntime.so"
        };
        Self {
            backend: EmbedBackend::Onnx,
            model_path: "models/minilm-l6-int8.onnx".into(),
            vocab_path: "models/vocab.txt".into(),
            runtime_lib: lib.into(),
            max_tokens: 128,
            batch_size: 64,
            batch_wait_ms: 200,
            pooling: Pooling::Mean,
            tokenizer: TokenizerKind::WordPiece,
            preset: ModelPreset::MinilmL6,
            input_template: "{title} . {summary}".into(),
            hash_dim: 256,
            model_url: String::new(),
            vocab_url: String::new(),
            runtime_url: String::new(),
        }
    }
}

/// ONNX Runtime release used by `fetch-model` when no URL is configured.
pub const ORT_VERSION: &str = "1.27.1";
const MINILM: &str = "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/";
const MULTILINGUAL: &str =
    "https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2/resolve/main/";

impl Embed {
    /// Model export for this machine: int8 AVX2 on x86_64, int8 ARM64 on
    /// aarch64, the f32 export anywhere else.
    pub fn model_url_for(&self, os: &str, arch: &str) -> String {
        if !self.model_url.is_empty() {
            return self.model_url.clone();
        }
        let file = match arch {
            "x86_64" => "model_quint8_avx2.onnx",
            "aarch64" => "model_qint8_arm64.onnx",
            _ => "model.onnx",
        };
        let _ = os;
        format!("{}onnx/{file}", self.preset_base())
    }

    fn preset_base(&self) -> &'static str {
        match self.preset {
            ModelPreset::MinilmL6 => MINILM,
            ModelPreset::MultilingualMinilmL12 => MULTILINGUAL,
        }
    }

    /// The tokenizer file for the preset: WordPiece vocab or the
    /// SentencePiece model.
    pub fn vocab_url_for(&self) -> String {
        if !self.vocab_url.is_empty() {
            return self.vocab_url.clone();
        }
        let file = match self.preset {
            ModelPreset::MinilmL6 => "vocab.txt",
            ModelPreset::MultilingualMinilmL12 => "sentencepiece.bpe.model",
        };
        format!("{}{file}", self.preset_base())
    }

    /// The preset's tokenizer, so a mismatch in config is caught at
    /// startup rather than as garbage embeddings.
    pub fn check_preset(&self) -> Result<(), String> {
        let want = match self.preset {
            ModelPreset::MinilmL6 => TokenizerKind::WordPiece,
            ModelPreset::MultilingualMinilmL12 => TokenizerKind::Unigram,
        };
        if self.tokenizer != want && self.model_url.is_empty() {
            return Err(format!(
                "embed.preset {:?} uses the {:?} tokenizer but embed.tokenizer is {:?}; set both, or set model_url and vocab_url for a custom model",
                self.preset, want, self.tokenizer
            ));
        }
        Ok(())
    }

    /// Runtime tarball for this machine, or an error naming the triple.
    pub fn runtime_url_for(&self, os: &str, arch: &str) -> Result<String, String> {
        if !self.runtime_url.is_empty() {
            return Ok(self.runtime_url.clone());
        }
        let name = match (os, arch) {
            ("linux", "x86_64") => "linux-x64",
            ("linux", "aarch64") => "linux-aarch64",
            ("macos", "x86_64") => "osx-x86_64",
            ("macos", "aarch64") => "osx-arm64",
            ("windows", "x86_64") => "win-x64",
            ("windows", "aarch64") => "win-arm64",
            _ => return Err(format!("no prebuilt ONNX Runtime for {os}/{arch}; set embed.runtime_url and embed.runtime_lib by hand")),
        };
        let ext = if os == "windows" { "zip" } else { "tgz" };
        Ok(format!("https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/onnxruntime-{name}-{ORT_VERSION}.{ext}"))
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
    /// Disk budget (docs/steward.md §5): whole days are dropped, oldest
    /// first, to stay under it; 0 means none. Applied by prune and the
    /// nightly job, after retention.
    pub max_bytes: u64,
}

impl Default for Snapshots {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: "data/snapshots".into(),
            zstd_level: 3,
            retention_days: 180,
            max_bytes: 0,
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
    /// Base URL of the First Report ledger that holds sourced facts about the
    /// same outlet ids. Empty disables the links.
    pub ledger_url: String,
}

impl Default for Api {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".into(),
            ledger_url: "http://127.0.0.1:8081".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Export {
    /// Run the export once a day inside `feed run`.
    pub nightly: bool,
    pub out_dir: PathBuf,
    /// `ndjson`, or `parquet` when built with that feature.
    pub format: String,
    /// UTC hour (0-23) the nightly job runs at.
    pub hour_utc: u8,
}

impl Default for Export {
    fn default() -> Self {
        Self {
            nightly: false,
            out_dir: "data/export".into(),
            format: "ndjson".into(),
            hour_utc: 2,
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
    UnknownOutlet {
        feed_url: String,
        outlet: String,
    },
    /// `opt-out.toml` present but unreadable or malformed.
    OptOut(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(p, e) => write!(f, "reading {}: {e}", p.display()),
            ConfigError::Parse(p, e) => write!(f, "parsing {}: {e}", p.display()),
            ConfigError::UnknownOutlet { feed_url, outlet } => {
                write!(f, "feed {feed_url} references unknown outlet {outlet:?}")
            }
            ConfigError::OptOut(e) => write!(f, "opt-out list: {e}"),
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
    let mut config = config;
    config.opt_out_hosts = crate::fetch::opt_out::load(dir).map_err(ConfigError::OptOut)?;

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
        assert_eq!(c.api.ledger_url, "http://127.0.0.1:8081");
    }

    #[test]
    fn shipped_config_dir_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("config");
        let loaded = load(&dir).expect("shipped config must load");
        assert!(!loaded.feeds.feeds.is_empty());
        assert!(!loaded.outlets.outlets.is_empty());
    }

    #[test]
    fn fetch_sources_follow_target_triple() {
        let e = Embed::default();
        assert!(e
            .model_url_for("linux", "x86_64")
            .ends_with("model_quint8_avx2.onnx"));
        assert!(e
            .model_url_for("macos", "aarch64")
            .ends_with("model_qint8_arm64.onnx"));
        assert!(e
            .model_url_for("linux", "riscv64")
            .ends_with("/onnx/model.onnx"));
        assert!(e
            .vocab_url_for()
            .ends_with("all-MiniLM-L6-v2/resolve/main/vocab.txt"));
        assert!(e.check_preset().is_ok());
        let m = Embed {
            preset: ModelPreset::MultilingualMinilmL12,
            tokenizer: TokenizerKind::Unigram,
            ..Default::default()
        };
        assert!(m.model_url_for("linux", "x86_64").contains(
            "paraphrase-multilingual-MiniLM-L12-v2/resolve/main/onnx/model_quint8_avx2.onnx"
        ));
        assert!(m.vocab_url_for().ends_with("/sentencepiece.bpe.model"));
        assert!(m.check_preset().is_ok());
        let wrong = Embed {
            preset: ModelPreset::MultilingualMinilmL12,
            ..Default::default()
        };
        assert!(
            wrong.check_preset().is_err(),
            "preset and tokenizer must agree"
        );
        let parsed: Embed =
            toml::from_str("preset = \"multilingual-minilm-l12\"\ntokenizer = \"unigram\"")
                .unwrap();
        assert_eq!(parsed.tokenizer, TokenizerKind::Unigram);
        assert!(e
            .runtime_url_for("linux", "aarch64")
            .unwrap()
            .contains("onnxruntime-linux-aarch64-"));
        assert!(e
            .runtime_url_for("macos", "aarch64")
            .unwrap()
            .contains("osx-arm64"));
        assert!(e
            .runtime_url_for("windows", "x86_64")
            .unwrap()
            .ends_with("onnxruntime-win-x64-1.27.1.zip"));
        assert!(e.runtime_url_for("freebsd", "x86_64").is_err());
        let e = Embed {
            runtime_url: "file:///x.tgz".into(),
            ..Default::default()
        };
        assert_eq!(
            e.runtime_url_for("freebsd", "x86_64").unwrap(),
            "file:///x.tgz"
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let err = toml::from_str::<Config>("[poll]\nnope = 1\n").unwrap_err();
        assert!(err.to_string().contains("nope"));
    }
}
