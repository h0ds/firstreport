//! The embedder task (plan §5.2): receives `Item`s from the poller, batches
//! up to `batch_size` or `batch_wait_ms`, runs the model on a blocking
//! thread, writes embeddings to the store, and forwards `(Item, vector)`
//! to the clusterer (M3).

pub mod model;
pub mod tokenizer;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::stats::Stats;
use crate::store::writer::Writer;
use crate::store::WriteMsg;
use crate::types::Item;

pub use model::Model;

#[allow(dead_code)] // consumed by the clusterer (M3)
pub struct Embedded {
    pub item: Item,
    pub vector: Vec<f32>,
}

/// Text fed to the model: title, a separator, and the summary.
pub fn input_text(item: &Item) -> String {
    if item.summary.is_empty() {
        item.title.clone()
    } else {
        format!("{} . {}", item.title, item.summary)
    }
}

pub fn load_from_config(cfg: &Config) -> Result<Model, model::ModelError> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    Model::load(
        &cfg.embed.runtime_lib,
        &cfg.embed.model_path,
        &cfg.embed.vocab_path,
        cfg.embed.max_tokens,
        threads,
    )
}

pub async fn run(
    cfg: Arc<Config>,
    model: Model,
    mut rx: mpsc::Receiver<Item>,
    out: mpsc::Sender<Embedded>,
    writer: Writer,
    stats: Arc<Stats>,
) {
    let batch_size = cfg.embed.batch_size.max(1);
    let wait = Duration::from_millis(cfg.embed.batch_wait_ms);
    let model = Arc::new(std::sync::Mutex::new(model));
    info!(
        batch_size,
        wait_ms = cfg.embed.batch_wait_ms,
        "embedder started"
    );

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
        let started = Instant::now();
        let n = batch.len();
        let result = tokio::task::spawn_blocking(move || {
            let texts: Vec<String> = batch.iter().map(input_text).collect();
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
                    if out.try_send(Embedded { item, vector }).is_err() {
                        warn!("clusterer backlog full; dropping embedded item");
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
