//! The embedder task (plan §5.2): receives `Item`s from the poller, batches
//! up to `batch_size` or `batch_wait_ms`, runs the model on a blocking
//! thread, writes embeddings to the store, and forwards `(Item, vector)`
//! to the clusterer.

pub mod hash;
pub mod model;
pub mod tokenizer;
pub mod unigram;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::{Config, EmbedBackend};
use crate::stats::Stats;
use crate::store::writer::Writer;
use crate::store::WriteMsg;
use crate::types::Item;

pub use model::Model;

/// Either the ONNX model or the deterministic feature-hashing stand-in.
pub enum Embedder {
    Onnx(Model),
    Hash(hash::HashEmbedder),
}

impl Embedder {
    pub fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, model::ModelError> {
        match self {
            Embedder::Onnx(m) => m.embed(texts),
            Embedder::Hash(h) => Ok(texts.iter().map(|t| h.embed(t)).collect()),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Embedder::Onnx(_) => "onnx".into(),
            Embedder::Hash(h) => format!("hash(dim={})", h.dim()),
        }
    }
}

pub struct Embedded {
    pub item: Item,
    pub vector: Vec<f32>,
}

/// Text fed to the model, from `embed.input_template`. When the summary is
/// empty, everything after `{title}` is dropped so no dangling separator
/// is embedded.
pub fn input_text(template: &str, item: &Item) -> String {
    let t = if item.summary.is_empty() {
        match template.find("{title}") {
            Some(i) => &template[..i + "{title}".len()],
            None => template,
        }
    } else {
        template
    };
    t.replace("{title}", &item.title)
        .replace("{summary}", &item.summary)
        .trim()
        .to_string()
}

/// Whether the configured backend can be loaded without further setup.
pub fn available(cfg: &Config) -> bool {
    match cfg.embed.backend {
        EmbedBackend::Hash => true,
        EmbedBackend::Onnx => cfg.embed.model_path.exists() && cfg.embed.runtime_lib.exists(),
    }
}

/// `threads`: `None` uses all cores; replay passes `Some(1)` so results
/// are identical across machines.
pub fn load_from_config(
    cfg: &Config,
    threads: Option<usize>,
) -> Result<Embedder, model::ModelError> {
    match cfg.embed.backend {
        EmbedBackend::Hash => Ok(Embedder::Hash(hash::HashEmbedder::new(cfg.embed.hash_dim))),
        EmbedBackend::Onnx => {
            let threads = threads.unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4)
            });
            Model::load(
                &cfg.embed.runtime_lib,
                &cfg.embed.model_path,
                &cfg.embed.vocab_path,
                cfg.embed.max_tokens,
                cfg.embed.pooling,
                threads,
            )
            .map(Embedder::Onnx)
        }
    }
}

pub async fn run(
    cfg: Arc<Config>,
    model: Embedder,
    mut rx: mpsc::Receiver<Item>,
    out: mpsc::Sender<Embedded>,
    writer: Writer,
    stats: Arc<Stats>,
) {
    let batch_size = cfg.embed.batch_size.max(1);
    let wait = Duration::from_millis(cfg.embed.batch_wait_ms);
    info!(backend = %model.describe(), batch_size, wait_ms = cfg.embed.batch_wait_ms, "embedder started");
    let model = Arc::new(std::sync::Mutex::new(model));

    loop {
        let Some(first) = rx.recv().await else { break };
        let mut batch = vec![first];
        let deadline = tokio::time::sleep(wait);
        tokio::pin!(deadline);
        while batch.len() < batch_size {
            tokio::select! {
                more = rx.recv() => match more {
                    Some(it) => batch.push(it),
                    None => break,
                },
                _ = &mut deadline => break,
            }
        }

        let model = model.clone();
        let template = cfg.embed.input_template.clone();
        let started = Instant::now();
        let n = batch.len();
        let result = tokio::task::spawn_blocking(move || {
            let texts: Vec<String> = batch.iter().map(|it| input_text(&template, it)).collect();
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            let mut m = model.lock().unwrap_or_else(|p| p.into_inner());
            m.embed(&refs).map(|vs| (batch, vs))
        })
        .await;
        let elapsed = started.elapsed();
        stats.record_embed(n, elapsed);

        match result {
            Ok(Ok((items, vectors))) => {
                let mut rows = Vec::with_capacity(items.len());
                for (item, vector) in items.into_iter().zip(vectors) {
                    rows.push((item.id, vector.clone()));
                    // Back-pressure rather than drop: the clusterer is
                    // single-threaded and cheap, so this rarely waits.
                    if out.send(Embedded { item, vector }).await.is_err() {
                        warn!("clusterer gone; embeddings still stored");
                    }
                }
                writer.send(WriteMsg::Embeddings(rows)).await;
            }
            Ok(Err(e)) => error!("embedding batch of {n} failed: {e}"),
            Err(e) => error!("embedding task panicked: {e}"),
        }
    }
    info!("embedder stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, summary: &str) -> Item {
        Item {
            id: 1,
            outlet_id: "o".into(),
            canonical_url: String::new(),
            title: title.into(),
            summary: summary.into(),
            published_at: 0,
            fetched_at: 0,
            lang: "en".into(),
            text_hash: 0,
            cluster_id: None,
            cluster_rank: None,
            joined_at: None,
            snapshot_id: 0,
        }
    }

    #[test]
    fn template_substitutes_and_drops_empty_summary_tail() {
        assert_eq!(input_text("{title} . {summary}", &item("A", "B")), "A . B");
        assert_eq!(input_text("{title} . {summary}", &item("A", "")), "A");
        assert_eq!(input_text("{title}", &item("A", "B")), "A");
        assert_eq!(
            input_text("{title} {title} {summary}", &item("A", "B")),
            "A A B"
        );
    }
}
