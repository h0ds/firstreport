//! Deterministic feature-hashing embedder: bag of unigrams and bigrams over
//! the simhash normalization, hashed into `dim` buckets with signed
//! weights, L2 normalized. No model, no network, identical on every
//! machine. Its similarity is lexical, so it is a stand-in for replay and
//! CI, not for production clustering.

use xxhash_rust::xxh64::xxh64;

use crate::parse::simhash::normalize;

pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(8) }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn embed(&self, text: &str) -> Vec<f32> {
        let words = normalize(text);
        let mut v = vec![0f32; self.dim];
        let mut add = |s: &str, w: f32| {
            let h = xxh64(s.as_bytes(), 0);
            let bucket = (h % self.dim as u64) as usize;
            let sign = if (h >> 63) == 1 { -1.0 } else { 1.0 };
            v[bucket] += sign * w;
        };
        for w in &words {
            add(w, 1.0);
        }
        for pair in words.windows(2) {
            add(&format!("{} {}", pair[0], pair[1]), 0.5);
        }
        super::model::normalize(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_unit_length_and_lexically_sensible() {
        let h = HashEmbedder::new(64);
        let a = h.embed("Central bank holds rates steady");
        let b = h.embed("central bank holds rates steady!");
        let c = h.embed("Coastal flooding closes highways");
        assert_eq!(a, b);
        let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
        let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
        assert!(dot(&a, &c) < 0.5);
        let d = h.embed("Central bank keeps rates steady");
        assert!(dot(&a, &d) > 0.5);
    }
}
