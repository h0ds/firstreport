//! SentencePiece unigram tokenizer, implemented in-repo like the
//! WordPiece one so no tokenizer crate enters the hot path. Reads the
//! model's own `.model` file (a protobuf, decoded here with a minimal
//! wire reader), normalises with NFKC plus SentencePiece's whitespace
//! rules, and segments with Viterbi over the piece scores. Verified
//! against the reference implementation's ids on recorded English and
//! Japanese models (`fixtures/models/unigram-reference.json`).
//!
//! Output ids follow the XLM-RoBERTa convention that the multilingual
//! sentence models were trained with: `<s>`=0, `<pad>`=1, `</s>`=2,
//! `<unk>`=3, and every other piece at its SentencePiece id plus one.
//! The raw SentencePiece ids are available for the tests and for any
//! model that uses them directly.

// Wired into the embedder in the next commit.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;

use unicode_normalization::UnicodeNormalization;

use super::tokenizer::Encoded;

const SPACE: char = '\u{2581}'; // ▁
/// SentencePiece's penalty below the lowest piece score for unknown characters.
const UNK_PENALTY: f32 = 10.0;

/// Piece types from the model proto.
const TYPE_NORMAL: u64 = 1;
const TYPE_UNKNOWN: u64 = 2;
const TYPE_CONTROL: u64 = 3;
const TYPE_USER_DEFINED: u64 = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub text: String,
    pub score: f32,
    pub kind: u64,
}

/// How output ids map from SentencePiece ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdScheme {
    /// Ids as SentencePiece assigns them; sequence wrapped in the model's
    /// own `<s>` and `</s>` when it has them.
    Native,
    /// XLM-RoBERTa: `<s>`=0, `<pad>`=1, `</s>`=2, `<unk>`=3, pieces +1.
    XlmRoberta,
}

pub struct Unigram {
    pieces: Vec<Piece>,
    lookup: HashMap<String, u32>,
    /// Longest piece in chars, bounding the lattice fan-out.
    max_piece_chars: usize,
    unk_id: u32,
    bos_id: Option<u32>,
    eos_id: Option<u32>,
    min_score: f32,
    add_dummy_prefix: bool,
    scheme: IdScheme,
}

// ---- protobuf wire reader, just enough for ModelProto

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let byte = *self.b.get(self.i).ok_or("truncated varint")?;
            self.i += 1;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
            if shift > 63 {
                return Err("varint too long".into());
            }
        }
    }
    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let n = self.varint()? as usize;
        let s = self.b.get(self.i..self.i + n).ok_or("truncated field")?;
        self.i += n;
        Ok(s)
    }
    fn skip(&mut self, wire: u64) -> Result<(), String> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.i += 8,
            2 => {
                self.bytes()?;
            }
            5 => self.i += 4,
            w => return Err(format!("unsupported wire type {w}")),
        }
        Ok(())
    }
    fn done(&self) -> bool {
        self.i >= self.b.len()
    }
}

struct Spec {
    pieces: Vec<Piece>,
    add_dummy_prefix: bool,
}

fn parse_piece(b: &[u8]) -> Result<Piece, String> {
    let mut r = Reader { b, i: 0 };
    let mut p = Piece {
        text: String::new(),
        score: 0.0,
        kind: TYPE_NORMAL,
    };
    while !r.done() {
        let key = r.varint()?;
        match (key >> 3, key & 7) {
            (1, 2) => p.text = String::from_utf8_lossy(r.bytes()?).into_owned(),
            (2, 5) => {
                let s = r.b.get(r.i..r.i + 4).ok_or("truncated score")?;
                p.score = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                r.i += 4;
            }
            (3, 0) => p.kind = r.varint()?,
            (_, w) => r.skip(w)?,
        }
    }
    Ok(p)
}

fn parse_model(b: &[u8]) -> Result<Spec, String> {
    let mut r = Reader { b, i: 0 };
    let mut spec = Spec {
        pieces: Vec::new(),
        add_dummy_prefix: true,
    };
    while !r.done() {
        let key = r.varint()?;
        match (key >> 3, key & 7) {
            (1, 2) => spec.pieces.push(parse_piece(r.bytes()?)?),
            (3, 2) => {
                // NormalizerSpec: 3 = add_dummy_prefix
                let mut n = Reader {
                    b: r.bytes()?,
                    i: 0,
                };
                while !n.done() {
                    let k = n.varint()?;
                    match (k >> 3, k & 7) {
                        (3, 0) => spec.add_dummy_prefix = n.varint()? != 0,
                        (_, w) => n.skip(w)?,
                    }
                }
            }
            (_, w) => r.skip(w)?,
        }
    }
    if spec.pieces.is_empty() {
        return Err("no pieces in model".into());
    }
    Ok(spec)
}

impl Unigram {
    pub fn from_file(path: &Path, scheme: IdScheme) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes, scheme)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn from_bytes(bytes: &[u8], scheme: IdScheme) -> Result<Self, String> {
        let spec = parse_model(bytes)?;
        let mut lookup = HashMap::with_capacity(spec.pieces.len());
        let mut unk_id = None;
        let mut bos_id = None;
        let mut eos_id = None;
        let mut min_score = f32::MAX;
        let mut max_piece_chars = 1;
        for (i, p) in spec.pieces.iter().enumerate() {
            match p.kind {
                TYPE_UNKNOWN => unk_id = Some(i as u32),
                TYPE_CONTROL => match p.text.as_str() {
                    "<s>" => bos_id = Some(i as u32),
                    "</s>" => eos_id = Some(i as u32),
                    _ => {}
                },
                TYPE_NORMAL | TYPE_USER_DEFINED => {
                    lookup.insert(p.text.clone(), i as u32);
                    min_score = min_score.min(p.score);
                    max_piece_chars = max_piece_chars.max(p.text.chars().count());
                }
                _ => {}
            }
        }
        Ok(Self {
            unk_id: unk_id.ok_or("model has no <unk> piece")?,
            bos_id,
            eos_id,
            min_score,
            max_piece_chars,
            lookup,
            add_dummy_prefix: spec.add_dummy_prefix,
            pieces: spec.pieces,
            scheme,
        })
    }

    pub fn vocab_size(&self) -> usize {
        self.pieces.len()
    }

    /// NFKC, control characters dropped, whitespace runs collapsed to one
    /// space and trimmed, then SentencePiece's `▁` for spaces with the
    /// dummy prefix.
    pub fn normalize(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 2);
        let mut pending_space = false;
        let mut started = false;
        for c in text.nfkc() {
            if c.is_whitespace() {
                pending_space = true;
                continue;
            }
            if c.is_control() || c == '\u{feff}' {
                continue;
            }
            if pending_space && started {
                out.push(SPACE);
            }
            pending_space = false;
            if !started {
                started = true;
                if self.add_dummy_prefix {
                    out.push(SPACE);
                }
            }
            out.push(c);
        }
        out
    }

    /// Raw SentencePiece ids for a text, no specials.
    pub fn piece_ids(&self, text: &str) -> Vec<u32> {
        let norm = self.normalize(text);
        let chars: Vec<char> = norm.chars().collect();
        let n = chars.len();
        if n == 0 {
            return vec![];
        }
        // Viterbi: best[i] = best score for chars[..i]; back[i] = (start, id).
        let unk_score = self.min_score - UNK_PENALTY;
        let mut best = vec![f32::NEG_INFINITY; n + 1];
        let mut back: Vec<(usize, u32)> = vec![(0, self.unk_id); n + 1];
        best[0] = 0.0;
        let mut buf = String::new();
        for start in 0..n {
            if best[start] == f32::NEG_INFINITY {
                continue;
            }
            let mut any = false;
            buf.clear();
            for end in start + 1..=n.min(start + self.max_piece_chars) {
                buf.push(chars[end - 1]);
                if let Some(&id) = self.lookup.get(&buf) {
                    any = true;
                    let s = best[start] + self.pieces[id as usize].score;
                    if s > best[end] {
                        best[end] = s;
                        back[end] = (start, id);
                    }
                }
            }
            if !any || !self.lookup.contains_key(&chars[start].to_string()) {
                // The single character is not a piece: an unknown node.
                let s = best[start] + unk_score;
                if s > best[start + 1] {
                    best[start + 1] = s;
                    back[start + 1] = (start, self.unk_id);
                }
            }
        }
        let mut ids = Vec::new();
        let mut i = n;
        while i > 0 {
            let (start, id) = back[i];
            ids.push(id);
            i = start;
        }
        ids.reverse();
        // Consecutive unknowns are one <unk>, as in the reference.
        ids.dedup_by(|a, b| *a == self.unk_id && *b == self.unk_id);
        ids
    }

    fn map(&self, spm_id: u32) -> u32 {
        match self.scheme {
            IdScheme::Native => spm_id,
            IdScheme::XlmRoberta => {
                if spm_id == self.unk_id {
                    3
                } else {
                    spm_id + 1
                }
            }
        }
    }

    pub fn pad_id(&self) -> u32 {
        match self.scheme {
            IdScheme::Native => 0,
            IdScheme::XlmRoberta => 1,
        }
    }

    /// `<s> pieces… </s>`, truncated so the total is at most `max_len`.
    pub fn encode(&self, text: &str, max_len: usize) -> Encoded {
        let (bos, eos) = match self.scheme {
            IdScheme::Native => (self.bos_id, self.eos_id),
            IdScheme::XlmRoberta => (Some(0), Some(2)),
        };
        let budget = max_len.saturating_sub(bos.is_some() as usize + eos.is_some() as usize);
        let mut ids = Vec::with_capacity(max_len.min(64));
        if let Some(b) = bos {
            ids.push(b);
        }
        for id in self.piece_ids(text).into_iter().take(budget) {
            ids.push(self.map(id));
        }
        if let Some(e) = eos {
            ids.push(e);
        }
        let mask = vec![1; ids.len()];
        Encoded { ids, mask }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/models")
            .join(name)
    }

    fn reference() -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(fixture("unigram-reference.json")).unwrap())
            .unwrap()
    }

    #[test]
    fn reads_the_model_proto() {
        let t = Unigram::from_file(&fixture("unigram-en-test.model"), IdScheme::Native).unwrap();
        assert_eq!(t.vocab_size(), 1000);
        assert_eq!(t.unk_id, 0);
        assert_eq!((t.bos_id, t.eos_id), (Some(1), Some(2)));
        assert!(t.add_dummy_prefix);
        assert_eq!(t.pieces[4].text, "▁");
        assert!(t.min_score < -3.0);
        let ja = Unigram::from_file(&fixture("unigram-ja-test.model"), IdScheme::Native).unwrap();
        assert_eq!(ja.vocab_size(), 8000);
        assert!(Unigram::from_bytes(b"not a model", IdScheme::Native).is_err());
    }

    #[test]
    fn normalizes_like_sentencepiece() {
        let t = Unigram::from_file(&fixture("unigram-en-test.model"), IdScheme::Native).unwrap();
        assert_eq!(
            t.normalize("  leading and   multiple   spaces "),
            "▁leading▁and▁multiple▁spaces"
        );
        assert_eq!(
            t.normalize("Ｆｕｌｌ ＡＳＣＩＩ ｶﾀｶﾅ"),
            "▁Full▁ASCII▁カタカナ"
        );
        assert_eq!(t.normalize(""), "");
    }

    #[test]
    fn matches_the_reference_ids_on_english_and_japanese_models() {
        let r = reference();
        for (name, file) in [
            ("en", "unigram-en-test.model"),
            ("ja", "unigram-ja-test.model"),
        ] {
            let t = Unigram::from_file(&fixture(file), IdScheme::Native).unwrap();
            let mut mismatches = Vec::new();
            for case in r[name]["cases"].as_array().unwrap() {
                let text = case["text"].as_str().unwrap();
                let want: Vec<u32> = case["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect();
                let got = t.piece_ids(text);
                if got != want {
                    mismatches.push(format!("{name} {text:?}: got {got:?} want {want:?}"));
                }
            }
            assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        }
    }

    #[test]
    fn xlm_roberta_scheme_wraps_and_shifts() {
        let t =
            Unigram::from_file(&fixture("unigram-ja-test.model"), IdScheme::XlmRoberta).unwrap();
        let native =
            Unigram::from_file(&fixture("unigram-ja-test.model"), IdScheme::Native).unwrap();
        let e = t.encode("日銀、政策金利を据え置き", 128);
        let raw = native.piece_ids("日銀、政策金利を据え置き");
        assert_eq!(e.ids[0], 0);
        assert_eq!(*e.ids.last().unwrap(), 2);
        assert_eq!(e.ids.len(), raw.len() + 2);
        for (a, b) in e.ids[1..e.ids.len() - 1].iter().zip(&raw) {
            assert_eq!(*a, b + 1);
        }
        assert_eq!(e.mask.len(), e.ids.len());
        assert_eq!(t.pad_id(), 1);
        let unk = t.encode("\u{1F6A8}", 8);
        assert!(unk.ids.contains(&3), "unknown maps to 3: {:?}", unk.ids);
        let short = t.encode("沿岸部で洪水、夜間の暴風雨で高速道路2本が通行止めに", 6);
        assert_eq!(short.ids.len(), 6);
        let n = native.encode("日銀", 128);
        assert_eq!((n.ids[0], *n.ids.last().unwrap()), (1, 2));
    }
}
