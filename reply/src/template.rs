//! Templates (plan §8): TOML files, one sentence each, with the facts the
//! sentence needs, an optional condition, and the cite it emits. Layouts
//! give the sentence order and length rules per platform.

use std::collections::BTreeMap;
use std::path::Path;

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
    /// Template ids in the order their sentences appear.
    pub order: Vec<String>,
    /// Closing sentence, always present, linking the full record.
    pub trailer: String,
    pub trailer_cite: String,
}

#[derive(Debug, Default)]
pub struct Set {
    pub templates: BTreeMap<String, Template>,
    pub layouts: BTreeMap<String, Layout>,
}

/// `templates/en/*.toml` (each file holds `[id]` tables) and
/// `templates/layout/<platform>.toml`.
pub fn load(dir: &Path) -> Result<Set, String> {
    let mut set = Set::default();
    for path in toml_files(&dir.join("en"))? {
        let s = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let tables: BTreeMap<String, Template> =
            toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))?;
        for (id, mut t) in tables {
            if set.templates.contains_key(&id) {
                return Err(format!("{}: template {id} defined twice", path.display()));
            }
            t.id = id.clone();
            set.templates.insert(id, t);
        }
    }
    for path in toml_files(&dir.join("layout"))? {
        let s = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut l: Layout = toml::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))?;
        l.platform = path.file_stem().unwrap().to_string_lossy().to_string();
        set.layouts.insert(l.platform.clone(), l);
    }
    if set.templates.is_empty() {
        return Err(format!("{}: no templates", dir.display()));
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

    /// Static problems: empty text or cite, a layout naming an unknown
    /// template, a template no layout uses, a bad `when`.
    pub fn check(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in self.templates.values() {
            if t.text.trim().is_empty() {
                out.push(format!("template {}: empty text", t.id));
            }
            if t.cite.trim().is_empty() {
                out.push(format!("template {}: no cite; rule 6", t.id));
            }
            if let Some(w) = &t.when {
                if parse_when(w).is_none() {
                    out.push(format!("template {}: cannot parse when {w:?}", t.id));
                }
            }
            if !self.layouts.values().any(|l| l.order.contains(&t.id)) {
                out.push(format!("template {}: no layout uses it", t.id));
            }
        }
        for l in self.layouts.values() {
            for id in &l.order {
                if !self.templates.contains_key(id) {
                    out.push(format!("layout {}: unknown template {id}", l.platform));
                }
            }
            if l.trailer_cite.trim().is_empty() {
                out.push(format!("layout {}: trailer has no cite", l.platform));
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
