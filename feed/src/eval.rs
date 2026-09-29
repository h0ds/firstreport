//! `feed eval`: precision and recall of the assignment step on a labelled
//! pair set (plan §9). Embeds both sides of every pair with the configured
//! backend, computes cosine similarity, and sweeps thresholds. The output
//! is what `docs/clustering.md` records when defaults are chosen.

use std::path::Path;

use serde::Serialize;

use crate::cluster::simd::dot;
use crate::config::Config;
use crate::embed;

#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub same: bool,
    pub a: String,
    pub b: String,
}

pub fn parse_pairs(text: &str) -> Result<Vec<Pair>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 3 {
            return Err(format!(
                "line {}: expected 3 tab-separated columns, got {}",
                n + 1,
                cols.len()
            ));
        }
        let same = match cols[0].trim() {
            "1" => true,
            "0" => false,
            other => {
                return Err(format!(
                    "line {}: label must be 0 or 1, got {other:?}",
                    n + 1
                ))
            }
        };
        out.push(Pair {
            same,
            a: cols[1].trim().to_string(),
            b: cols[2].trim().to_string(),
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Metrics {
    pub threshold: f32,
    pub tp: usize,
    pub fp: usize,
    pub fn_: usize,
    pub tn: usize,
    pub precision: f32,
    pub recall: f32,
    pub f1: f32,
}

/// Metrics at one threshold over `(same, similarity)` observations.
pub fn metrics_at(obs: &[(bool, f32)], threshold: f32) -> Metrics {
    let (mut tp, mut fp, mut fn_, mut tn) = (0, 0, 0, 0);
    for &(same, s) in obs {
        match (same, s >= threshold) {
            (true, true) => tp += 1,
            (false, true) => fp += 1,
            (true, false) => fn_ += 1,
            (false, false) => tn += 1,
        }
    }
    let precision = if tp + fp > 0 {
        tp as f32 / (tp + fp) as f32
    } else {
        0.0
    };
    let recall = if tp + fn_ > 0 {
        tp as f32 / (tp + fn_) as f32
    } else {
        0.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    Metrics {
        threshold,
        tp,
        fp,
        fn_,
        tn,
        precision,
        recall,
        f1,
    }
}

#[derive(Debug, Serialize)]
pub struct Misclassified {
    pub same: bool,
    pub similarity: f32,
    pub a: String,
    pub b: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub backend: String,
    pub pairs: usize,
    pub positives: usize,
    pub sweep: Vec<Metrics>,
    pub best: Metrics,
    /// Metrics at the configured `cluster.join_threshold`.
    pub configured: Metrics,
    /// Pairs the configured threshold gets wrong, worst first.
    pub misclassified: Vec<Misclassified>,
}

pub fn sweep_thresholds() -> Vec<f32> {
    (30..=98).step_by(2).map(|t| t as f32 / 100.0).collect()
}

pub fn evaluate(cfg: &Config, pairs: &[Pair]) -> Result<Report, Box<dyn std::error::Error>> {
    let mut e = embed::load_from_config(cfg, Some(1))?;
    let backend = e.describe();
    let texts: Vec<&str> = pairs
        .iter()
        .flat_map(|p| [p.a.as_str(), p.b.as_str()])
        .collect();
    let mut vectors = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(cfg.embed.batch_size.max(1)) {
        vectors.extend(e.embed(chunk)?);
    }
    let obs: Vec<(bool, f32)> = pairs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.same, dot(&vectors[2 * i], &vectors[2 * i + 1])))
        .collect();

    let sweep: Vec<Metrics> = sweep_thresholds()
        .into_iter()
        .map(|t| metrics_at(&obs, t))
        .collect();
    // Best F1; on ties prefer the higher threshold (fewer false merges).
    let best = sweep
        .iter()
        .cloned()
        .fold(None::<Metrics>, |acc, m| match acc {
            Some(b) if b.f1 > m.f1 || (b.f1 == m.f1 && b.threshold > m.threshold) => Some(b),
            _ => Some(m),
        })
        .expect("sweep is non-empty");
    let configured = metrics_at(&obs, cfg.cluster.join_threshold);
    let mut misclassified: Vec<Misclassified> = pairs
        .iter()
        .zip(&obs)
        .filter(|(_, (same, s))| *same != (*s >= cfg.cluster.join_threshold))
        .map(|(p, (same, s))| Misclassified {
            same: *same,
            similarity: *s,
            a: p.a.clone(),
            b: p.b.clone(),
        })
        .collect();
    misclassified.sort_by(|x, y| {
        let dx = (x.similarity - cfg.cluster.join_threshold).abs();
        let dy = (y.similarity - cfg.cluster.join_threshold).abs();
        dy.partial_cmp(&dx).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(Report {
        backend,
        pairs: pairs.len(),
        positives: pairs.iter().filter(|p| p.same).count(),
        sweep,
        best,
        configured,
        misclassified,
    })
}

pub fn render(r: &Report) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "backend {}  pairs {}  positives {}\n\n",
        r.backend, r.pairs, r.positives
    ));
    s.push_str("threshold  precision  recall  f1     tp  fp  fn  tn\n");
    for m in &r.sweep {
        s.push_str(&format!(
            "{:>9.2}  {:>9.3}  {:>6.3}  {:>5.3}  {:>2}  {:>2}  {:>2}  {:>2}\n",
            m.threshold, m.precision, m.recall, m.f1, m.tp, m.fp, m.fn_, m.tn
        ));
    }
    s.push_str(&format!(
        "\nbest f1 {:.3} at threshold {:.2} (precision {:.3}, recall {:.3})\n",
        r.best.f1, r.best.threshold, r.best.precision, r.best.recall
    ));
    s.push_str(&format!(
        "configured join_threshold {:.2}: f1 {:.3} (precision {:.3}, recall {:.3}), {} misclassified\n",
        r.configured.threshold,
        r.configured.f1,
        r.configured.precision,
        r.configured.recall,
        r.misclassified.len()
    ));
    for m in r.misclassified.iter().take(10) {
        let kind = if m.same {
            "missed same-story"
        } else {
            "false merge"
        };
        let cut = |t: &str| {
            t.split(" . ")
                .next()
                .unwrap_or(t)
                .chars()
                .take(70)
                .collect::<String>()
        };
        s.push_str(&format!(
            "  {kind} ({:.3})\n    {}\n    {}\n",
            m.similarity,
            cut(&m.a),
            cut(&m.b)
        ));
    }
    s
}

pub fn run(cfg: &Config, pairs_path: &Path, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let pairs = parse_pairs(&std::fs::read_to_string(pairs_path)?)?;
    if pairs.is_empty() {
        return Err("no pairs".into());
    }
    let report = evaluate(cfg, &pairs)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render(&report));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EmbedBackend;

    #[test]
    fn parses_labelled_tsv_and_rejects_bad_rows() {
        let p = parse_pairs("# c\n1\ta one\tb one\n\n0\ta two\tb two\n").unwrap();
        assert_eq!(p.len(), 2);
        assert!(p[0].same && !p[1].same);
        assert!(parse_pairs("2\ta\tb").is_err());
        assert!(parse_pairs("1\tonly two").is_err());
    }

    #[test]
    fn metrics_are_counted_correctly() {
        let obs = [(true, 0.9), (true, 0.6), (false, 0.8), (false, 0.2)];
        let m = metrics_at(&obs, 0.7);
        assert_eq!((m.tp, m.fp, m.fn_, m.tn), (1, 1, 1, 1));
        assert!(
            (m.precision - 0.5).abs() < 1e-6
                && (m.recall - 0.5).abs() < 1e-6
                && (m.f1 - 0.5).abs() < 1e-6
        );
        let m = metrics_at(&obs, 0.5);
        assert_eq!((m.tp, m.fp, m.fn_, m.tn), (2, 1, 0, 1));
        assert!((m.recall - 1.0).abs() < 1e-6);
        assert_eq!(metrics_at(&[], 0.5).f1, 0.0);
    }

    /// The golden file evaluates end to end with the hash backend. The
    /// numbers themselves are not asserted tightly: a lexical embedder is
    /// a stand-in, and the point here is that the harness runs on the real
    /// fixture and produces a coherent report.
    #[test]
    fn aggregator_pairs_parse_and_are_balanced() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/golden/aggregator/aggregator-pairs.tsv");
        let pairs = parse_pairs(&std::fs::read_to_string(path).unwrap()).unwrap();
        let pos = pairs.iter().filter(|p| p.same).count();
        assert!(pairs.len() >= 900, "{}", pairs.len());
        assert!(
            pos >= 300 && pos * 2 <= pairs.len() + 100,
            "positives {pos} of {}",
            pairs.len()
        );
        assert!(
            pairs.iter().all(|p| p.a != p.b),
            "no pair compares a text with itself"
        );
    }

    #[test]
    fn japanese_pairs_parse_balanced_and_cover_both_languages() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/golden/ja-pairs.tsv");
        let pairs = parse_pairs(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(pairs.len(), 60);
        assert_eq!(pairs.iter().filter(|p| p.same).count(), 30);
        let has_kana = |s: &str| s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
        assert!(
            pairs.iter().all(|p| has_kana(&p.a) || has_kana(&p.b)),
            "every pair has Japanese on at least one side"
        );
        let cross = pairs
            .iter()
            .filter(|p| has_kana(&p.a) != has_kana(&p.b))
            .count();
        assert_eq!(cross, 20, "ten cross-language positives and ten negatives");
        assert!(pairs.iter().all(|p| p.a != p.b), "no identical pairs");
    }

    #[test]
    fn golden_pairs_evaluate_with_hash_backend() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/golden/pairs.tsv");
        let pairs = parse_pairs(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(pairs.len() >= 50, "smoke set should have at least 50 pairs");
        let mut cfg = Config::default();
        cfg.embed.backend = EmbedBackend::Hash;
        let r = evaluate(&cfg, &pairs).unwrap();
        assert_eq!(r.pairs, pairs.len());
        assert_eq!(r.sweep.len(), sweep_thresholds().len());
        assert!(
            r.best.f1 > 0.6,
            "even a lexical embedder should beat chance on paraphrases: {}",
            render(&r)
        );
        assert!(r.best.precision > 0.7, "{}", render(&r));
        let rendered = render(&r);
        assert!(rendered.contains("best f1"));
    }
}
