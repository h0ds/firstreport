//! BERT WordPiece tokenizer (plan §5.2), implemented in-repo so we don't
//! pull in the `tokenizers` crate. Matches the reference behaviour for
//! uncased models: lowercase, strip accents, split punctuation and CJK
//! characters, greedy longest-match-first subwords with `##` continuations.

use std::collections::HashMap;
use std::path::Path;

pub const CLS: &str = "[CLS]";
pub const SEP: &str = "[SEP]";
pub const UNK: &str = "[UNK]";
pub const PAD: &str = "[PAD]";

/// Longest word (in chars) WordPiece will attempt; longer ones become UNK.
const MAX_WORD_CHARS: usize = 100;

/// Either tokenizer behind one interface; chosen by `embed.tokenizer`.
pub enum AnyTokenizer {
    WordPiece(Tokenizer),
    Unigram(super::unigram::Unigram),
}

impl AnyTokenizer {
    pub fn load(kind: crate::config::TokenizerKind, path: &Path) -> std::io::Result<Self> {
        use crate::config::TokenizerKind;
        Ok(match kind {
            TokenizerKind::WordPiece => AnyTokenizer::WordPiece(Tokenizer::from_vocab_file(path)?),
            TokenizerKind::Unigram => AnyTokenizer::Unigram(super::unigram::Unigram::from_file(
                path,
                super::unigram::IdScheme::XlmRoberta,
            )?),
        })
    }

    pub fn encode(&self, text: &str, max_len: usize) -> Encoded {
        match self {
            AnyTokenizer::WordPiece(t) => t.encode(text, max_len),
            AnyTokenizer::Unigram(t) => t.encode(text, max_len),
        }
    }

    pub fn pad_id(&self) -> u32 {
        match self {
            AnyTokenizer::WordPiece(t) => t.pad_id(),
            AnyTokenizer::Unigram(t) => t.pad_id(),
        }
    }

    pub fn vocab_size(&self) -> usize {
        match self {
            AnyTokenizer::WordPiece(t) => t.vocab_size(),
            AnyTokenizer::Unigram(t) => t.vocab_size(),
        }
    }
}

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    cls: u32,
    sep: u32,
    unk: u32,
    pad: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub ids: Vec<u32>,
    /// 1 for real tokens (including CLS/SEP), 0 for padding.
    pub mask: Vec<u32>,
}

impl Tokenizer {
    pub fn from_vocab_file(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_vocab_text(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn from_vocab_text(text: &str) -> Result<Self, String> {
        let mut vocab = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let tok = line.trim_end_matches(['\r', '\n']);
            if !tok.is_empty() {
                vocab.entry(tok.to_string()).or_insert(i as u32);
            }
        }
        let get = |t: &str| {
            vocab
                .get(t)
                .copied()
                .ok_or_else(|| format!("vocab missing {t}"))
        };
        Ok(Self {
            cls: get(CLS)?,
            sep: get(SEP)?,
            unk: get(UNK)?,
            pad: get(PAD)?,
            vocab,
        })
    }

    pub fn pad_id(&self) -> u32 {
        self.pad
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    /// `[CLS] tokens… [SEP]`, truncated so the total is at most `max_len`.
    pub fn encode(&self, text: &str, max_len: usize) -> Encoded {
        let budget = max_len.saturating_sub(2);
        let mut ids = Vec::with_capacity(max_len.min(64));
        ids.push(self.cls);
        'outer: for word in basic_tokenize(text) {
            for id in self.wordpiece(&word) {
                if ids.len() > budget {
                    break 'outer;
                }
                ids.push(id);
            }
        }
        ids.push(self.sep);
        let mask = vec![1; ids.len()];
        Encoded { ids, mask }
    }

    /// Greedy longest-match-first over a single basic token.
    fn wordpiece(&self, word: &str) -> Vec<u32> {
        let chars: Vec<char> = word.chars().collect();
        if chars.len() > MAX_WORD_CHARS {
            return vec![self.unk];
        }
        let mut out = Vec::new();
        let mut start = 0;
        let mut buf = String::new();
        while start < chars.len() {
            let mut end = chars.len();
            let mut found = None;
            while start < end {
                buf.clear();
                if start > 0 {
                    buf.push_str("##");
                }
                buf.extend(&chars[start..end]);
                if let Some(&id) = self.vocab.get(&buf) {
                    found = Some(id);
                    break;
                }
                end -= 1;
            }
            match found {
                Some(id) => {
                    out.push(id);
                    start = end;
                }
                None => return vec![self.unk],
            }
        }
        out
    }
}

/// BERT "basic" tokenization for uncased models: lowercase, strip
/// accents (NFD-style for the Latin range we care about), split on
/// whitespace, isolate punctuation and CJK characters, drop control chars.
pub fn basic_tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    for c in text.chars() {
        if c == '\0' || c == '\u{FFFD}' || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            flush(&mut cur, &mut out);
            continue;
        }
        if is_punct(c) || is_cjk(c) {
            flush(&mut cur, &mut out);
            out.push(c.to_lowercase().collect());
            continue;
        }
        for lc in c.to_lowercase() {
            if let Some(base) = strip_accent(lc) {
                cur.push(base);
            } else if !is_combining_mark(lc) {
                cur.push(lc);
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn is_punct(c: char) -> bool {
    let cp = c as u32;
    (33..=47).contains(&cp)
        || (58..=64).contains(&cp)
        || (91..=96).contains(&cp)
        || (123..=126).contains(&cp)
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '\u{2013}'
                | '\u{2014}'
                | '\u{2018}'
                | '\u{2019}'
                | '\u{201C}'
                | '\u{201D}'
                | '\u{2026}'
                | '\u{00AB}'
                | '\u{00BB}'
                | '\u{00A1}'
                | '\u{00BF}'
        )
}

fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x20000..=0x2A6DF).contains(&cp)
        || (0x2A700..=0x2CEAF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0x2F800..=0x2FA1F).contains(&cp)
}

fn is_combining_mark(c: char) -> bool {
    let cp = c as u32;
    (0x0300..=0x036F).contains(&cp)
        || (0x1AB0..=0x1AFF).contains(&cp)
        || (0x1DC0..=0x1DFF).contains(&cp)
        || (0x20D0..=0x20FF).contains(&cp)
}

/// Map precomposed Latin letters with diacritics to their base letter, as
/// BERT's `_run_strip_accents` does via NFD. Covers Latin-1 Supplement and
/// Latin Extended-A, which is what news text contains.
fn strip_accent(c: char) -> Option<char> {
    let cp = c as u32;
    if !(0x00C0..=0x017F).contains(&cp) {
        return None;
    }
    Some(match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' => 't',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOCAB: &str = include_str!("../../fixtures/models/tiny-vocab.txt");

    fn tok() -> Tokenizer {
        Tokenizer::from_vocab_text(VOCAB).unwrap()
    }

    fn id(t: &Tokenizer, s: &str) -> u32 {
        t.vocab[s]
    }

    #[test]
    fn basic_tokenization_matches_bert_rules() {
        assert_eq!(
            basic_tokenize("Hello, World!"),
            ["hello", ",", "world", "!"]
        );
        assert_eq!(basic_tokenize("  café  naïve "), ["cafe", "naive"]);
        assert_eq!(basic_tokenize("鏡 kagami"), ["鏡", "kagami"]);
        assert_eq!(basic_tokenize("don't"), ["don", "'", "t"]);
        assert_eq!(basic_tokenize("a\u{0}b"), ["ab"]);
    }

    #[test]
    fn wordpiece_greedy_longest_match() {
        let t = tok();
        let e = t.encode("Central bank holds rates", 128);
        let want = vec![
            id(&t, "[CLS]"),
            id(&t, "central"),
            id(&t, "bank"),
            id(&t, "hold"),
            id(&t, "##s"),
            id(&t, "rate"),
            id(&t, "##s"),
            id(&t, "[SEP]"),
        ];
        assert_eq!(e.ids, want);
        assert_eq!(e.mask, vec![1; 8]);
    }

    #[test]
    fn unknown_word_is_unk_and_truncation_keeps_sep() {
        let t = tok();
        let e = t.encode("bank zzzzzz", 128);
        assert_eq!(e.ids[2], id(&t, "[UNK]"));
        let e = t.encode("the bank the bank the bank the bank", 5);
        assert_eq!(e.ids.len(), 5);
        assert_eq!(*e.ids.last().unwrap(), id(&t, "[SEP]"));
        assert_eq!(e.ids[0], id(&t, "[CLS]"));
    }

    #[test]
    fn unaffordable_splits_to_pieces() {
        let t = tok();
        let e = t.encode("unaffordable", 128);
        assert_eq!(
            &e.ids[1..4],
            &[id(&t, "un"), id(&t, "##aff"), id(&t, "##ord")]
        );
        assert_eq!(e.ids[4], id(&t, "##able"));
    }
}
