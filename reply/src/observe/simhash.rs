//! 64-bit simhash over normalized text: a copy of `feed`'s, so a text
//! hash here means the same thing as `feed`'s `text_hash` (project rule:
//! copy, don't share).

use xxhash_rust::xxh64::xxh64;

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "at", "for", "with", "by",
    "from", "as", "is", "are", "was", "were", "be", "been", "it", "its", "this", "that", "these",
    "those", "he", "she", "they", "his", "her", "their", "we", "you", "i", "not", "no", "has",
    "have", "had", "will", "would", "can", "could", "says", "said", "after", "over", "into",
    "than", "who", "what", "how", "why", "when", "where",
];

pub fn normalize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

pub fn simhash(text: &str) -> u64 {
    let words = normalize(text);
    if words.is_empty() {
        return 0;
    }
    let mut v = [0i32; 64];
    let mut feed = |h: u64| {
        for (i, slot) in v.iter_mut().enumerate() {
            if (h >> i) & 1 == 1 {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
    };
    if words.len() == 1 {
        feed(xxh64(words[0].as_bytes(), 0));
    } else {
        let mut buf = String::new();
        for pair in words.windows(2) {
            buf.clear();
            buf.push_str(&pair[0]);
            buf.push(' ');
            buf.push_str(&pair[1]);
            feed(xxh64(buf.as_bytes(), 0));
        }
    }
    let mut out = 0u64;
    for (i, s) in v.iter().enumerate() {
        if *s > 0 {
            out |= 1 << i;
        }
    }
    out
}

#[inline]
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_copies_are_near() {
        let a = simhash("Central bank holds rates, signals cuts may come later in the year");
        let b = simhash("Central bank holds rates and signals cuts may come later in the year");
        let c = simhash("Regional airline adds winter routes to three cities");
        assert!(hamming(a, b) <= 3, "{}", hamming(a, b));
        assert!(hamming(a, c) > 10);
        assert_eq!(simhash(""), 0);
    }
}
