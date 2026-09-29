//! Templates + facts → `Draft` (plan §8). A pure function of its inputs.

use serde_json::Value;

use crate::facts;
use crate::model::{check_cites, Cite, Draft, FactSet, CONTRACT_VERSION};
use crate::template::{applies, render, Layout, Set, Template, TRAILER};

/// Where cites resolve to. Empty means the reference is left as a URL-less
/// cite, which the cite check rejects: every deployment names its servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Urls {
    pub feed: String,
    pub ledger: String,
    pub log: String,
}

impl Default for Urls {
    fn default() -> Self {
        Self {
            feed: "http://127.0.0.1:8080".into(),
            ledger: "http://127.0.0.1:8081".into(),
            log: "http://127.0.0.1:8082".into(),
        }
    }
}

/// `feed:item:ID` → `/v1/items/ID`; `feed:cluster:ID[/part]` → `/v1/clusters/ID`;
/// `feed:outlet:ID` → `/v1/outlets/ID`;
/// `ledger:OUTLET:FIELD` → `/v1/outlets/OUTLET/FIELD`; `reply:lookup:MENTION`
/// → `/v1/log/MENTION`; `observation:KIND:SUBJECT` → `/v1/observations?...`.
pub fn cite_url(reference: &str, urls: &Urls) -> Result<(String, String), String> {
    let mut parts = reference.splitn(3, ':');
    let (Some(kind), Some(a), Some(b)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("bad cite {reference:?}"));
    };
    let base = |s: &str| s.trim_end_matches('/').to_string();
    let url = match (kind, a) {
        ("feed", "item") => format!("{}/v1/items/{}", base(&urls.feed), facts::encode(b)),
        ("feed", "outlet") => format!("{}/v1/outlets/{}", base(&urls.feed), facts::encode(b)),
        ("feed", "cluster") => {
            let id = b.split('/').next().unwrap_or(b);
            format!("{}/v1/clusters/{}", base(&urls.feed), facts::encode(id))
        }
        ("ledger", outlet) => format!(
            "{}/v1/outlets/{}/{}",
            base(&urls.ledger),
            facts::encode(outlet),
            facts::encode(b)
        ),
        ("reply", "lookup") => format!("{}/v1/log/{}", base(&urls.log), facts::encode(b)),
        ("observation", k) => format!(
            "{}/v1/observations?kind={}&subject={}",
            base(&urls.log),
            facts::encode(k),
            facts::encode(b)
        ),
        _ => return Err(format!("unknown cite kind in {reference:?}")),
    };
    let kind = if kind == "reply" { "log" } else { kind };
    Ok((kind.to_string(), url))
}

pub fn compose(fs: &FactSet, set: &Set, layout: &Layout, urls: &Urls) -> Result<Draft, String> {
    let (lang, lang_fallback) = set.choose(fs.mention.lang.as_deref(), layout);
    let templates = &set.langs[&lang];
    let mut view = facts::view(fs, &lang);
    if let Some(d) = view.get_mut("d").and_then(Value::as_object_mut) {
        d.insert(
            "log_url".into(),
            Value::String(format!(
                "{}/v1/log/{}",
                urls.log.trim_end_matches('/'),
                facts::encode(&fs.mention.id)
            )),
        );
    }
    let mut draft = Draft {
        contract_version: CONTRACT_VERSION,
        mention_id: fs.mention.id.clone(),
        platform: layout.platform.clone(),
        template_ids: vec![],
        sentences: vec![],
        posts: vec![],
        max_chars: layout.max_chars,
        lang: lang.clone(),
        lang_fallback,
        cites: vec![],
        autonomous: false,
    };
    for id in &layout.order {
        let t = templates
            .get(id)
            .ok_or_else(|| format!("layout {} names unknown template {id}", layout.platform))?;
        if applies(t, &view) {
            push(&mut draft, t, &view, urls)?;
        }
    }
    let trailer = templates
        .get(TRAILER)
        .ok_or_else(|| format!("{lang}: no trailer template"))?;
    push(&mut draft, trailer, &view, urls)?;
    draft.posts = split(
        &draft.sentences,
        layout.max_chars,
        facts::words(&lang).sentence_sep,
    )?;
    check_cites(&draft)?;
    Ok(draft)
}

/// A reviewer's edit: keep only these sentences, in order. Cites follow
/// their sentences, so rule 6 survives; the closing sentence, which is
/// the last one, cannot be dropped. `keep` empty means keep everything.
pub fn keep_sentences(d: &Draft, keep: &[usize]) -> Result<Draft, String> {
    let last = d.sentences.len().checked_sub(1).ok_or("empty draft")?;
    let mut idx: Vec<usize> = if keep.is_empty() {
        (0..=last).collect()
    } else {
        keep.to_vec()
    };
    idx.sort_unstable();
    idx.dedup();
    if idx.iter().any(|&i| i > last) {
        return Err("keep names a sentence the draft does not have".into());
    }
    if *idx.last().unwrap() != last {
        idx.push(last);
    }
    let mut out = Draft {
        sentences: vec![],
        cites: vec![],
        posts: vec![],
        template_ids: vec![],
        ..d.clone()
    };
    for (n, &i) in idx.iter().enumerate() {
        out.sentences.push(d.sentences[i].clone());
        out.template_ids
            .push(d.template_ids.get(i).cloned().unwrap_or_default());
        for c in d.cites.iter().filter(|c| c.sentence as usize == i) {
            let mut c = c.clone();
            c.sentence = n as u16;
            out.cites.push(c);
        }
    }
    out.posts = split(
        &out.sentences,
        d.max_chars,
        facts::words(&d.lang).sentence_sep,
    )?;
    check_cites(&out)?;
    Ok(out)
}

fn push(d: &mut Draft, t: &Template, view: &Value, urls: &Urls) -> Result<(), String> {
    let text = render(&t.text, view).map_err(|e| format!("template {}: {e}", t.id))?;
    let reference = render(&t.cite, view).map_err(|e| format!("template {}: {e}", t.id))?;
    let (kind, url) = cite_url(&reference, urls)?;
    d.cites.push(Cite {
        sentence: d.sentences.len() as u16,
        reference,
        url,
        kind,
    });
    d.sentences.push(text);
    d.template_ids.push(t.id.clone());
    Ok(())
}

/// Join sentences into posts of at most `max` chars, breaking only between
/// sentences. A single sentence over the limit is an error, caught by
/// `check` before it can reach a platform.
fn split(sentences: &[String], max: usize, sep: &str) -> Result<Vec<String>, String> {
    let sep_n = sep.chars().count();
    let mut posts: Vec<String> = Vec::new();
    for s in sentences {
        let n = s.chars().count();
        if n > max {
            return Err(format!(
                "sentence of {n} chars exceeds the {max} limit: {s:?}"
            ));
        }
        match posts.last_mut() {
            Some(p) if p.chars().count() + sep_n + n <= max => {
                p.push_str(sep);
                p.push_str(s);
            }
            _ => posts.push(s.clone()),
        }
    }
    Ok(posts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cite_forms() {
        let u = Urls::default();
        assert_eq!(
            cite_url("feed:item:12", &u).unwrap().1,
            "http://127.0.0.1:8080/v1/items/12"
        );
        assert_eq!(
            cite_url("feed:cluster:7/syndication", &u).unwrap().1,
            "http://127.0.0.1:8080/v1/clusters/7"
        );
        assert_eq!(
            cite_url("ledger:bbc:domain_registered", &u).unwrap(),
            (
                "ledger".into(),
                "http://127.0.0.1:8081/v1/outlets/bbc/domain_registered".into()
            )
        );
        assert_eq!(
            cite_url("reply:lookup:at://x/y", &u).unwrap(),
            (
                "log".into(),
                "http://127.0.0.1:8082/v1/log/at%3A%2F%2Fx%2Fy".into()
            )
        );
        assert!(cite_url("nope", &u).is_err());
        assert!(cite_url("x:y:z", &u).is_err());
    }

    #[test]
    fn keep_drops_sentences_but_never_the_closing_one() {
        let d = Draft {
            contract_version: 1,
            mention_id: "m".into(),
            platform: "t".into(),
            template_ids: vec!["a".into(), "b".into(), "trailer".into()],
            sentences: vec!["A.".into(), "B.".into(), "T.".into()],
            posts: vec![],
            max_chars: 300,
            lang: "en".into(),
            lang_fallback: false,
            cites: (0..3)
                .map(|i| Cite {
                    sentence: i,
                    reference: "r".into(),
                    url: "u".into(),
                    kind: "k".into(),
                })
                .collect(),
            autonomous: false,
        };
        let k = keep_sentences(&d, &[1]).unwrap();
        assert_eq!(k.sentences, vec!["B.", "T."]);
        assert_eq!(
            k.cites.iter().map(|c| c.sentence).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(k.posts, vec!["B. T."]);
        let all = keep_sentences(&d, &[]).unwrap();
        assert_eq!(
            (all.sentences.clone(), all.cites.clone()),
            (d.sentences.clone(), d.cites.clone())
        );
        assert_eq!(all.posts, vec!["A. B. T."]);
        assert!(keep_sentences(&d, &[7]).is_err());
    }

    #[test]
    fn splits_between_sentences_only() {
        let s: Vec<String> = ["aaaa", "bbbb", "cc"]
            .iter()
            .map(|x| x.to_string())
            .collect();
        assert_eq!(split(&s, 9, " ").unwrap(), vec!["aaaa bbbb", "cc"]);
        assert_eq!(split(&s, 5, " ").unwrap(), vec!["aaaa", "bbbb", "cc"]);
        assert_eq!(split(&s, 8, "").unwrap(), vec!["aaaabbbb", "cc"]);
        assert!(split(&s, 3, " ").is_err());
    }
}
