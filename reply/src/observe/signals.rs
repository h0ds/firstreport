//! The four signals (plan §7), pure functions over sightings. Every
//! observation carries its window, count and denominator; what it means
//! is the reader's call.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::json;

use crate::model::Observation;

use super::simhash::hamming;
use super::{Sighting, Thresholds};

/// A burst as found: the subject, and its accounts in order of first post.
#[derive(Debug, Clone, PartialEq)]
pub struct Burst {
    pub subject: String,
    pub accounts_in_order: Vec<String>,
    pub first: i64,
    pub last: i64,
}

/// Run every signal. `now` is the clock, passed in for determinism.
pub fn run(sightings: &[Sighting], t: Thresholds, now: i64) -> (Vec<Observation>, Vec<Burst>) {
    let mut out = Vec::new();
    let mut by_subject: BTreeMap<String, Vec<&Sighting>> = BTreeMap::new();
    for s in sightings {
        if let Some(sub) = s.subject() {
            by_subject.entry(sub).or_default().push(s);
        }
    }
    let baseline = cohort_share(&all_known_ages(sightings), t.cohort_span_days);
    let mut bursts = Vec::new();
    for (subject, mut posts) in by_subject {
        posts.sort_by_key(|s| s.at);
        let Some((obs, burst)) = burst(&subject, &posts, t, now) else {
            continue;
        };
        out.push(obs);
        if let Some(o) = syndicated_text(&subject, &posts, now) {
            out.push(o);
        }
        if let Some(o) = cohort(&burst, &posts, t, baseline, now) {
            out.push(o);
        }
        bursts.push(burst);
    }
    out.extend(first_movers(&bursts, t, now));
    (out, bursts)
}

/// Distinct accounts posting the subject inside one window of
/// `burst_window_secs`; the densest such window wins.
fn burst(
    subject: &str,
    posts: &[&Sighting],
    t: Thresholds,
    now: i64,
) -> Option<(Observation, Burst)> {
    let total: HashSet<&str> = posts.iter().map(|s| s.account.as_str()).collect();
    if total.len() < t.burst_min_accounts {
        return None;
    }
    let mut best: Option<(usize, usize, usize)> = None; // (count, i, j)
    let mut j = 0;
    for i in 0..posts.len() {
        while j < posts.len() && posts[j].at - posts[i].at <= t.burst_window_secs {
            j += 1;
        }
        let distinct: HashSet<&str> = posts[i..j].iter().map(|s| s.account.as_str()).collect();
        if best.is_none_or(|(c, _, _)| distinct.len() > c) {
            best = Some((distinct.len(), i, j));
        }
    }
    let (count, i, j) = best?;
    if count < t.burst_min_accounts {
        return None;
    }
    let window = &posts[i..j];
    let mut seen = HashSet::new();
    let accounts_in_order: Vec<String> = window
        .iter()
        .filter(|s| seen.insert(s.account.clone()))
        .map(|s| s.account.clone())
        .collect();
    let mut gaps: Vec<i64> = window.windows(2).map(|w| w[1].at - w[0].at).collect();
    gaps.sort_unstable();
    let median_gap = gaps.get(gaps.len() / 2).copied().unwrap_or(0);
    let (first, last) = (window[0].at, window[window.len() - 1].at);
    let obs = Observation {
        kind: "burst".into(),
        subject: subject.to_string(),
        window_secs: t.burst_window_secs as u32,
        count: count as u32,
        denominator: total.len() as u32,
        detail: json!({
            "first": first,
            "last": last,
            "span_secs": last - first,
            "median_gap_secs": median_gap,
            "posts_in_window": window.len(),
            "posts_total": posts.len(),
        }),
        computed_at: now,
    };
    Some((
        obs,
        Burst {
            subject: subject.to_string(),
            accounts_in_order,
            first,
            last,
        },
    ))
}

/// Among posts of the subject with text, those whose text is within three
/// bits of another account's. The earliest such post is the copy's origin.
fn syndicated_text(subject: &str, posts: &[&Sighting], now: i64) -> Option<Observation> {
    let with: Vec<&&Sighting> = posts.iter().filter(|s| s.text_hash.is_some()).collect();
    if with.len() < 2 {
        return None;
    }
    let mut copies = 0u32;
    let mut earliest: Option<i64> = None;
    for (i, a) in with.iter().enumerate() {
        let ha = a.text_hash.unwrap();
        let near = with.iter().enumerate().any(|(k, b)| {
            k != i && b.account != a.account && hamming(ha, b.text_hash.unwrap()) <= 3
        });
        if near {
            copies += 1;
            earliest = Some(earliest.map_or(a.at, |e| e.min(a.at)));
        }
    }
    if copies < 2 {
        return None;
    }
    Some(Observation {
        kind: "syndicated_text".into(),
        subject: subject.to_string(),
        window_secs: (posts.last()?.at - posts.first()?.at).max(0) as u32,
        count: copies,
        denominator: with.len() as u32,
        detail: json!({
            "share": copies as f64 / with.len() as f64,
            "earliest": earliest,
            "hamming_threshold": 3,
        }),
        computed_at: now,
    })
}

/// Creation days of every account with a known age, deduped.
fn all_known_ages(sightings: &[Sighting]) -> Vec<u32> {
    let mut m: HashMap<&str, u32> = HashMap::new();
    for s in sightings {
        if let Some(a) = s.account_age_days {
            m.insert(&s.account, a);
        }
    }
    m.into_values().collect()
}

/// The largest share of these accounts created within `span_days` of
/// each other. Ages are days-old, so a span of creation dates is a span
/// of ages.
pub fn cohort_share(ages: &[u32], span_days: u32) -> Option<f64> {
    if ages.is_empty() {
        return None;
    }
    let mut a = ages.to_vec();
    a.sort_unstable();
    let mut best = 0;
    let mut j = 0;
    for i in 0..a.len() {
        while j < a.len() && a[j] - a[i] <= span_days {
            j += 1;
        }
        best = best.max(j - i);
    }
    Some(best as f64 / a.len() as f64)
}

fn cohort(
    burst: &Burst,
    posts: &[&Sighting],
    t: Thresholds,
    baseline: Option<f64>,
    now: i64,
) -> Option<Observation> {
    let members: HashSet<&str> = burst.accounts_in_order.iter().map(String::as_str).collect();
    let mut ages: HashMap<&str, u32> = HashMap::new();
    for s in posts {
        if members.contains(s.account.as_str()) {
            if let Some(a) = s.account_age_days {
                ages.insert(&s.account, a);
            }
        }
    }
    let known: Vec<u32> = ages.values().copied().collect();
    if known.len() < 5 {
        return None;
    }
    let share = cohort_share(&known, t.cohort_span_days)?;
    if share * 100.0 < t.cohort_min_share as f64 {
        return None;
    }
    let in_span = (share * known.len() as f64).round() as u32;
    Some(Observation {
        kind: "cohort".into(),
        subject: burst.subject.clone(),
        window_secs: t.burst_window_secs as u32,
        count: in_span,
        denominator: known.len() as u32,
        detail: json!({
            "span_days": t.cohort_span_days,
            "share": share,
            "baseline_share": baseline,
            "accounts_in_burst": members.len(),
            "accounts_with_known_age": known.len(),
        }),
        computed_at: now,
    })
}

/// Accounts that were first or second into many distinct bursts.
fn first_movers(bursts: &[Burst], t: Thresholds, now: i64) -> Vec<Observation> {
    let recent: Vec<&Burst> = bursts
        .iter()
        .filter(|b| now - b.first <= t.movers_window_days as i64 * 86_400)
        .collect();
    let mut led: HashMap<&str, u32> = HashMap::new();
    for b in &recent {
        for a in b.accounts_in_order.iter().take(2) {
            *led.entry(a).or_default() += 1;
        }
    }
    let mut out: Vec<Observation> = led
        .into_iter()
        .filter(|(_, n)| *n as usize >= t.movers_min_bursts)
        .map(|(account, n)| Observation {
            kind: "first_movers".into(),
            subject: account.to_string(),
            window_secs: t.movers_window_days * 86_400,
            count: n,
            denominator: recent.len() as u32,
            detail: json!({ "share": n as f64 / recent.len().max(1) as f64 }),
            computed_at: now,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.subject.cmp(&b.subject)));
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::observe::simhash::simhash;

    /// Deterministic sightings: background traffic on many links, one
    /// planted burst of near-copies by young accounts, one signup wave in
    /// the background so the baseline has something to absorb, and a pair
    /// of accounts that lead five bursts each.
    pub fn planted(now: i64) -> Vec<Sighting> {
        let mut v = Vec::new();
        let s = |at: i64, acc: &str, url: &str, text: &str, age: Option<u32>| Sighting {
            platform: "t".into(),
            account: acc.into(),
            account_age_days: age,
            at,
            url: Some(url.into()),
            text_hash: Some(simhash(text)),
        };
        // Background: 200 accounts, spread over 3 days, own wording each,
        // ages spread over 3 years except a wave of 60 created 400-410 days ago.
        for i in 0..200u32 {
            let age = if i % 10 < 3 {
                400 + (i % 10) * 3
            } else {
                30 + (i * 37) % 1000
            };
            let text = format!("post number {i} about topic {} with some words", i % 7);
            v.push(s(
                now - 3 * 86_400 + i as i64 * 1200,
                &format!("bg{i}"),
                &format!("https://news.example/{}", i % 40),
                &text,
                Some(age),
            ));
        }
        // Planted burst: 30 accounts within 5 minutes, same wording, 24 of
        // them created 3-9 days ago.
        for i in 0..30u32 {
            let age = if i < 24 { 3 + i % 7 } else { 500 + i };
            v.push(s(
                now - 3600 + i as i64 * 10,
                &format!("burst{i}"),
                "https://truth-wire.example/x",
                "BREAKING the bank did the thing and nobody is talking about it",
                Some(age),
            ));
        }
        // Two leaders: first and second into 5 other bursts of 20.
        for b in 0..5u32 {
            let url = format!("https://led.example/{b}");
            let t0 = now - 2 * 86_400 + b as i64 * 7200;
            v.push(s(
                t0,
                "lead1",
                &url,
                &format!("story {b} take one"),
                Some(800),
            ));
            v.push(s(
                t0 + 5,
                "lead2",
                &url,
                &format!("story {b} take two"),
                Some(900),
            ));
            for i in 0..20u32 {
                v.push(s(
                    t0 + 30 + i as i64 * 15,
                    &format!("f{b}_{i}"),
                    &url,
                    &format!("story {b} reaction {i} with different words each"),
                    Some(100 + i * 40),
                ));
            }
        }
        v.sort_by_key(|s| s.at);
        v
    }

    #[test]
    fn finds_the_planted_burst_and_nothing_in_the_background() {
        let now = 1_704_200_000;
        let v = planted(now);
        let t = Thresholds::default();
        let (obs, bursts) = run(&v, t, now);
        let kinds = |k: &str| obs.iter().filter(|o| o.kind == k).collect::<Vec<_>>();

        let b = kinds("burst");
        assert_eq!(
            b.len(),
            6,
            "{:?}",
            b.iter().map(|o| &o.subject).collect::<Vec<_>>()
        );
        let planted_burst = b
            .iter()
            .find(|o| o.subject == "https://truth-wire.example/x")
            .unwrap();
        assert_eq!((planted_burst.count, planted_burst.denominator), (30, 30));
        assert_eq!(planted_burst.detail["span_secs"], 290);
        assert!(bursts
            .iter()
            .all(|b| !b.subject.starts_with("https://news.example/")));

        let syn = kinds("syndicated_text");
        assert_eq!(syn.len(), 1);
        assert_eq!((syn[0].count, syn[0].denominator), (30, 30));

        let c = kinds("cohort");
        assert_eq!(c.len(), 1, "only the planted burst is a cohort: {c:?}");
        assert_eq!((c[0].count, c[0].denominator), (24, 30));
        let baseline = c[0].detail["baseline_share"].as_f64().unwrap();
        assert!(
            baseline < 0.5,
            "the background signup wave stays under the threshold: {baseline}"
        );

        let m = kinds("first_movers");
        assert_eq!(
            m.iter().map(|o| o.subject.as_str()).collect::<Vec<_>>(),
            vec!["lead1", "lead2"]
        );
        assert_eq!((m[0].count, m[0].denominator), (5, 6));
    }

    #[test]
    fn cohort_share_is_the_densest_span() {
        assert_eq!(cohort_share(&[1, 2, 3, 100, 200], 5), Some(0.6));
        assert_eq!(cohort_share(&[], 5), None);
        assert_eq!(cohort_share(&[10], 5), Some(1.0));
    }
}
