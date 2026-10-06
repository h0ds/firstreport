//! ONNX Runtime session wrapper: tokenized batch in, pooled and
//! L2-normalized sentence embeddings out. The runtime library is loaded
//! at startup from a configured path (`load-dynamic`); nothing is fetched
//! at build time.

use std::path::Path;
use std::sync::OnceLock;

use ort::session::Session;
use ort::value::Tensor;

use super::tokenizer::{AnyTokenizer, Encoded};
use crate::config::Pooling;

static RUNTIME: OnceLock<Result<(), String>> = OnceLock::new();

/// Load the ONNX Runtime shared library once per process.
pub fn init_runtime(lib: &Path) -> Result<(), String> {
    RUNTIME
        .get_or_init(|| {
            let env = ort::init_from(lib).map_err(|e| format!("loading {}: {e}", lib.display()))?;
            env.with_name("truthdecay-feed").commit();
            Ok(())
        })
        .clone()
}

pub struct Model {
    session: Session,
    tokenizer: AnyTokenizer,
    /// Whether the graph declares a `token_type_ids` input (BERT-family
    /// models do, XLM-RoBERTa-family models do not).
    wants_token_types: bool,
    max_tokens: usize,
    pooling: Pooling,
    dim: Option<usize>,
}

#[derive(Debug)]
pub struct ModelError(pub String);
impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ModelError {}

impl<T> From<ort::Error<T>> for ModelError {
    fn from(e: ort::Error<T>) -> Self {
        ModelError(e.to_string())
    }
}

impl Model {
    pub fn load(
        runtime_lib: &Path,
        model: &Path,
        vocab: &Path,
        tokenizer_kind: crate::config::TokenizerKind,
        max_tokens: usize,
        pooling: Pooling,
        threads: usize,
    ) -> Result<Self, ModelError> {
        init_runtime(runtime_lib).map_err(ModelError)?;
        let tokenizer = AnyTokenizer::load(tokenizer_kind, vocab)
            .map_err(|e| ModelError(format!("tokenizer {}: {e}", vocab.display())))?;
        let session = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_intra_threads(threads.max(1))?
            .commit_from_file(model)
            .map_err(|e| ModelError(format!("model {}: {e}", model.display())))?;
        let wants_token_types = session
            .inputs()
            .iter()
            .any(|i| i.name() == "token_type_ids");
        tracing::info!(
            tokenizer = ?tokenizer_kind,
            vocab = tokenizer.vocab_size(),
            token_type_ids = wants_token_types,
            "model loaded"
        );
        Ok(Self {
            session,
            tokenizer,
            wants_token_types,
            max_tokens,
            pooling,
            dim: None,
        })
    }

    /// Embedding width, known after the first batch.
    #[allow(dead_code)] // the clusterer sizes its centroids from this (M3)
    pub fn dim(&self) -> Option<usize> {
        self.dim
    }

    /// Embed a batch of texts. Output vectors are L2-normalized.
    pub fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, ModelError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let encoded: Vec<Encoded> = texts
            .iter()
            .map(|t| self.tokenizer.encode(t, self.max_tokens))
            .collect();
        let seq = encoded.iter().map(|e| e.ids.len()).max().unwrap_or(1);
        let batch = encoded.len();
        let pad = self.tokenizer.pad_id() as i64;

        let mut ids = vec![pad; batch * seq];
        let mut mask = vec![0i64; batch * seq];
        let types = vec![0i64; batch * seq];
        for (b, e) in encoded.iter().enumerate() {
            for (t, (&id, &m)) in e.ids.iter().zip(&e.mask).enumerate() {
                ids[b * seq + t] = id as i64;
                mask[b * seq + t] = m as i64;
            }
        }
        let shape = [batch as i64, seq as i64];
        let outputs = if self.wants_token_types {
            self.session.run(ort::inputs![
                "input_ids" => Tensor::from_array((shape, ids))?,
                "attention_mask" => Tensor::from_array((shape, mask.clone()))?,
                "token_type_ids" => Tensor::from_array((shape, types))?,
            ])?
        } else {
            self.session.run(ort::inputs![
                "input_ids" => Tensor::from_array((shape, ids))?,
                "attention_mask" => Tensor::from_array((shape, mask.clone()))?,
            ])?
        };
        let (out_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        let dims: Vec<i64> = out_shape.iter().copied().collect();
        if dims.len() != 3 || dims[0] as usize != batch || dims[1] as usize != seq {
            return Err(ModelError(format!(
                "unexpected output shape {dims:?} for batch {batch}×{seq}"
            )));
        }
        let dim = dims[2] as usize;
        match self.dim {
            None => self.dim = Some(dim),
            Some(d) if d != dim => {
                return Err(ModelError(format!(
                    "embedding dim changed from {d} to {dim}"
                )))
            }
            _ => {}
        }

        Ok((0..batch)
            .map(|b| {
                let hidden = &data[b * seq * dim..(b + 1) * seq * dim];
                let m = &mask[b * seq..(b + 1) * seq];
                match self.pooling {
                    Pooling::Mean => mean_pool_normalize(hidden, m, dim),
                    Pooling::Cls => cls_pool_normalize(hidden, dim),
                }
            })
            .collect())
    }
}

pub fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for o in &mut v {
            *o /= norm;
        }
    }
    v
}

/// Masked mean over the sequence axis, then L2 normalize.
pub fn mean_pool_normalize(hidden: &[f32], mask: &[i64], dim: usize) -> Vec<f32> {
    let mut out = vec![0f32; dim];
    let mut n = 0f32;
    for (t, &m) in mask.iter().enumerate() {
        if m == 0 {
            continue;
        }
        n += 1.0;
        let row = &hidden[t * dim..(t + 1) * dim];
        for (o, &h) in out.iter_mut().zip(row) {
            *o += h;
        }
    }
    if n > 0.0 {
        for o in &mut out {
            *o /= n;
        }
    }
    normalize(out)
}

/// First token ([CLS]) vector, L2 normalized. What bge models expect.
pub fn cls_pool_normalize(hidden: &[f32], dim: usize) -> Vec<f32> {
    normalize(hidden[..dim].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooling_ignores_padding_and_normalizes() {
        // two real tokens, one padded; dim 2
        let hidden = [1.0, 0.0, 0.0, 1.0, 100.0, 100.0];
        let v = mean_pool_normalize(&hidden, &[1, 1, 0], 2);
        let r = 1.0 / 2f32.sqrt();
        assert!((v[0] - r).abs() < 1e-6 && (v[1] - r).abs() < 1e-6);
        assert_eq!(mean_pool_normalize(&[0.0, 0.0], &[1], 2), vec![0.0, 0.0]);
        let c = cls_pool_normalize(&[3.0, 4.0, 9.0, 9.0], 2);
        assert!((c[0] - 0.6).abs() < 1e-6 && (c[1] - 0.8).abs() < 1e-6);
    }

    /// Runs only when an ONNX Runtime library is available, e.g.
    /// `KAGAMI_ORT_LIB=/path/libonnxruntime.so cargo test`.
    #[test]
    fn tiny_model_end_to_end() {
        let Ok(lib) = std::env::var("KAGAMI_ORT_LIB") else {
            eprintln!("KAGAMI_ORT_LIB unset; skipping");
            return;
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/models");
        let mut m = Model::load(
            Path::new(&lib),
            &root.join("tiny.onnx"),
            &root.join("tiny-vocab.txt"),
            crate::config::TokenizerKind::WordPiece,
            16,
            Pooling::Mean,
            1,
        )
        .unwrap();
        let v = m
            .embed(&["central bank", "central bank", "flood storm"])
            .unwrap();
        assert_eq!(m.dim(), Some(16));
        assert_eq!(v.len(), 3);
        for e in &v {
            let norm = e.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-5, "not unit length: {norm}");
        }
        assert_eq!(v[0], v[1], "deterministic");
        let cos: f32 = v[0].iter().zip(&v[2]).map(|(a, b)| a * b).sum();
        assert!(cos < 0.999, "different texts should differ: {cos}");
        // Batch of one must equal the same row from a padded batch.
        let single = m.embed(&["flood storm"]).unwrap();
        let diff: f32 = single[0]
            .iter()
            .zip(&v[2])
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff < 1e-5, "padding leaked into the pooled vector: {diff}");
    }
}
