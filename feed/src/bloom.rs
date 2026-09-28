//! Bloom filter over recent item ids (plan §5.1). A hit is confirmed against
//! the store; a miss is definitive. Bespoke: it's forty lines.

use xxhash_rust::xxh64::xxh64;

pub struct Bloom {
    bits: Vec<u64>,
    k: u32,
    mask: u64,
}

impl Bloom {
    /// Sized for `expected` items at roughly 1% false positives.
    pub fn new(expected: usize) -> Self {
        let bits_needed = (expected.max(1024) as f64 * 9.6).ceil() as u64;
        let words = (bits_needed / 64).next_power_of_two().max(16);
        Self {
            bits: vec![0; words as usize],
            k: 7,
            mask: words * 64 - 1,
        }
    }

    fn positions(&self, id: u64) -> impl Iterator<Item = u64> + '_ {
        // Kirsch–Mitzenmacher double hashing over one xxh64 of the id.
        let h1 = id;
        let h2 = xxh64(&id.to_le_bytes(), 0x9E37_79B9_7F4A_7C15) | 1;
        (0..self.k as u64).map(move |i| h1.wrapping_add(i.wrapping_mul(h2)) & self.mask)
    }

    pub fn insert(&mut self, id: u64) {
        let ps: Vec<u64> = self.positions(id).collect();
        for p in ps {
            self.bits[(p / 64) as usize] |= 1 << (p % 64);
        }
    }

    pub fn might_contain(&self, id: u64) -> bool {
        self.positions(id)
            .all(|p| self.bits[(p / 64) as usize] & (1 << (p % 64)) != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_false_negatives_few_false_positives() {
        let mut b = Bloom::new(10_000);
        for i in 0..10_000u64 {
            b.insert(i.wrapping_mul(0x5851_F42D_4C95_7F2D));
        }
        for i in 0..10_000u64 {
            assert!(b.might_contain(i.wrapping_mul(0x5851_F42D_4C95_7F2D)));
        }
        let fp = (0..10_000u64)
            .filter(|i| b.might_contain(i.wrapping_mul(0x1234_5678_9ABC_DEF1) + 7))
            .count();
        assert!(fp < 300, "false positives {fp}");
    }
}
