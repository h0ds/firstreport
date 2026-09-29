//! Templates (plan §8): TOML files, one sentence each, with the facts the
//! sentence needs, an optional condition, and the cite it emits, in one
//! directory per language. Layouts give the sentence order and length
//! rules per platform and apply to every language. A language is usable
//! only when it has every template a layout names plus `trailer`; English
//! is the fallback.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::facts;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Template {
    #[serde(skip)]
    pub id: String,
    /// Paths into the view that must resolve, or the template is skipped.
    #[serde(default)]
    pub needs: Vec<String>,
    /// `path op literal`; op is one of == != >= <= > <. Skipped when false.
    #[serde(default)]
    pub when: Option<String>,
    /// The sentence, with `{path}` placeholders.
    pub text: String,
    /// The cite, with placeholders; see `compose::cite_url` for the forms.
    pub cite: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Layout {
    #[serde(skip)]
    pub platform: String,
    pub max_chars: usize,
    /// Template ids in the order their sentences appear. The closing
    /// sentence is the `trailer` template of the language, always last.
    pub order: Vec<String>,
}

pub const FALLBACK_LANG: &str = "en";
pub const TRAILER: &str = "trailer";

#[derive(Debug, Default)]
pub struct Set {
    /// Language → template id → template.
    pub langs: BTreeMap<String, BTreeMap<String, Template>>,
    pub layouts: BTreeMap<String, Layout>,
}

/// `templates/<lang>/*.toml` (each file holds `[id]` tables) for every
/// two-letter directory, and `templates/layout/<platform>.toml`.
pub fn load(dir: &Path) -> Result<Set, String> {
    let mut set = Set::default();
    let mut lang_dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                n.len() == 2 && n.to_string_lossy().chars().all(|c| c.is_ascii_lowercase())
            })
        })
        .collect();
    lang_dirs.sort();
    for ld in lang_dirs {
        let lang = ld.file_name().unwrap().to_string_lossy().to_string();
        let mut templates = BTreeMap::new();
        for path in toml_files(&ld)? {
            let s =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let tables: BTreeMap<String, Template> =
                toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))?;
            for (id, mut t) in tables {
                if templates.contains_key(&id) {
                    return Err(format!("{}: template {id} defined twice", path.display()));
                }
                t.id = id.clone();
                templates.insert(id, t);
            }
        }
        set.langs.insert(lang, templates);
    }
    for path in toml_files(&dir.join("layout"))? {
        let s = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut l: Layout = toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))?;
        l.platform = path.file_stem().unwrap().to_string_lossy().to_string();
        set.layouts.insert(l.platform.clone(), l);
    }
    if set.langs.get(FALLBACK_LANG).is_none_or(BTreeMap::is_empty) {
        return Err(format!("{}: no {FALLBACK_LANG} templates", dir.display()));
    }
    if set.layouts.is_empty() {
        return Err(format!("{}: no layouts", dir.display()));
    }
    Ok(set)
}

fn toml_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    v.sort();
    Ok(v)
}

impl Set {
    pub fn layout(&self, platform: &str) -> Result<&Layout, String> {
        self.layouts
            .get(platform)
            .ok_or_else(|| format!("no layout for platform {platform}"))
    }

    /// The English templates: the reference set every layout is written against.
    pub fn fallback(&self) -> &BTreeMap<String, Template> {
        &self.langs[FALLBACK_LANG]
    }

    /// Whether `lang` has every template `layout` names, and the trailer.
    pub fn complete(&self, lang: &str, layout: &Layout) -> bool {
        self.langs.get(lang).is_some_and(|t| {
            t.contains_key(TRAILER) && layout.order.iter().all(|id| t.contains_key(id))
        })
    }

    /// The language to answer in: the mention's, when complete; else English.
    pub fn choose(&self, wanted: Option<&str>, layout: &Layout) -> (String, bool) {
        match wanted {
            Some(l) if self.complete(l, layout) => (l.to_string(), false),
            Some(l) if l != FALLBACK_LANG => (FALLBACK_LANG.to_string(), true),
            _ => (FALLBACK_LANG.to_string(), false),
        }
    }

    /// Static problems: empty text or cite, a layout naming an unknown
    /// template, an English template no layout uses, a bad `when`, a
    /// language missing a template a layout names or with one English
    /// lacks. Translations are all-or-nothing: a partial language is a
    /// problem, not a fallback.
    pub fn check(&self) -> Vec<String> {
        let mut out = Vec::new();
        let en = self.fallback();
        for (lang, templates) in &self.langs {
            for t in templates.values() {
                if t.text.trim().is_empty() {
                    out.push(format!("{lang}/{}: empty text", t.id));
                }
                if t.cite.trim().is_empty() {
                    out.push(format!("{lang}/{}: no cite; rule 6", t.id));
                }
                if let Some(w) = &t.when {
                    if parse_when(w).is_none() {
                        out.push(format!("{lang}/{}: cannot parse when {w:?}", t.id));
                    }
                }
                if lang == FALLBACK_LANG
                    && t.id != TRAILER
                    && !self.layouts.values().any(|l| l.order.contains(&t.id))
                {
                    out.push(format!("{lang}/{}: no layout uses it", t.id));
                }
                if lang != FALLBACK_LANG && !en.contains_key(&t.id) {
                    out.push(format!("{lang}/{}: no such English template", t.id));
                }
            }
            if !templates.contains_key(TRAILER) {
                out.push(format!("{lang}: no trailer template"));
            }
            for id in en.keys() {
                if !templates.contains_key(id) {
                    out.push(format!("{lang}: missing {id}"));
                }
            }
        }
        for l in self.layouts.values() {
            for id in &l.order {
                if !en.contains_key(id) {
                    out.push(format!("layout {}: unknown template {id}", l.platform));
                }
            }
        }
        out
    }
}

/// Whether `t` fires on `view`.
pub fn applies(t: &Template, view: &Value) -> bool {
    t.needs.iter().all(|p| facts::get(view, p).is_some())
        && t.when.as_deref().is_none_or(|w| eval_when(w, view))
}

#[derive(Debug, PartialEq)]
struct When<'a> {
    path: &'a str,
    op: &'a str,
    lit: &'a str,
}

fn parse_when(w: &str) -> Option<When<'_>> {
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some((a, b)) = w.split_once(op) {
            let path = a.trim();
            let lit = b.trim().trim_matches('"');
            if !path.is_empty() {
                return Some(When { path, op, lit });
            }
        }
    }
    None
}

fn eval_when(w: &str, view: &Value) -> bool {
    let Some(c) = parse_when(w) else { return false };
    let Some(v) = facts::get(view, c.path) else {
        return false;
    };
    if let (Some(a), Ok(b)) = (v.as_f64(), c.lit.parse::<f64>()) {
        return match c.op {
            "==" => a == b,
            "!=" => a != b,
            ">=" => a >= b,
            "<=" => a <= b,
            ">" => a > b,
            _ => a < b,
        };
    }
    let a = facts::text(v);
    match c.op {
        "==" => a == c.lit,
        "!=" => a != c.lit,
        _ => false,
    }
}

/// Substitute `{path}` placeholders. A placeholder that does not resolve
/// is an error: a sentence never goes out with a hole in it.
pub fn render(s: &str, view: &Value) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('}') else {
            return Err(format!("unclosed placeholder in {s:?}"));
        };
        let path = &after[..j];
        match facts::get(view, path) {
            Some(v) => out.push_str(&facts::text(v)),
            None => return Err(format!("{path} is missing for {s:?}")),
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn when_and_render() {
        let v = json!({"d": {"share": 0.7, "m": "item"}});
        assert!(eval_when("d.share >= 0.5", &v));
        assert!(!eval_when("d.share > 0.7", &v));
        assert!(eval_when("d.m == item", &v));
        assert!(eval_when("d.m == \"item\"", &v));
        assert!(!eval_when("d.missing == x", &v));
        assert!(parse_when("nonsense").is_none());
        assert_eq!(render("{d.m}: {d.share}", &v).unwrap(), "item: 0.7");
        assert!(render("{d.nope}", &v).is_err());
        assert!(render("{d.m", &v).is_err());
    }
}
