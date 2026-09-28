//! 64-bit simhash over normalized title+summary (plan §5.1). Items within
//! Hamming distance `hamming_threshold` are treated as the same copy.

use xxhash_rust::xxh64::xxh64;

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "at", "for", "with", "by",
    "from", "as", "is", "are", "was", "were", "be", "been", "it", "its", "this", "that", "these",
    "those", "he", "she", "they", "his", "her", "their", "we", "you", "i", "not", "no", "has",
    "have", "had", "will", "would", "can", "could", "says", "said", "after", "over", "into",
    "than", "who", "what", "how", "why", "when", "where",
];

/// Lowercase, strip punctuation, drop stopwords. Public so the same
/// normalization can be reused by tests and by later pillars.
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
    fn identical_text_identical_hash() {
        let a = simhash("Central bank holds rates steady, signals cuts later this year");
        let b = simhash("Central bank holds rates steady, signals cuts later this year");
        assert_eq!(a, b);
    }

    #[test]
    fn punctuation_and_case_do_not_matter() {
        let a = simhash("Central Bank Holds Rates Steady!");
        let b = simhash("central bank holds rates steady");
        assert_eq!(hamming(a, b), 0);
    }

    #[test]
    fn near_copy_is_close_and_unrelated_is_far() {
        let base = "The finance minister announced a new package of measures to support small businesses hit by rising energy costs, including tax deferrals and grants";
        let near = "The finance minister announced a new package of measures to support small businesses hit by rising energy costs, including tax deferrals and loans";
        let far = "Local football club wins regional championship after dramatic penalty shootout in front of record crowd";
        assert!(
            hamming(simhash(base), simhash(near)) <= 8,
            "near copy too far"
        );
        assert!(
            hamming(simhash(base), simhash(far)) >= 16,
            "unrelated too close"
        );
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(simhash(""), 0);
        assert_eq!(simhash("the and of"), 0);
    }
}
