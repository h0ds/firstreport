//! Small HTML helpers shared by the importers that read pages:
//! element scan, attribute lookup, entity decode, text extraction. Not
//! a parser; enough for the tables and link lists we need.

pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" | "#160" => Some(' '),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tags out, entities decoded, whitespace collapsed.
pub fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                in_tag = true;
                // block-ish tags separate words
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
        let _ = chars.peek();
    }
    decode(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `(tag inner html)` for each `<tag …>…</tag>` at the top level of
/// `html`, tolerating nesting of the same tag.
pub fn elements<'a>(html: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = lower[i..].find(&open) {
        let start = i + p;
        // must be `<tag>` or `<tag ` not `<table` when looking for `<t`
        let after = lower.as_bytes().get(start + open.len()).copied();
        if !matches!(
            after,
            Some(b' ') | Some(b'>') | Some(b'\n') | Some(b'\t') | Some(b'/')
        ) {
            i = start + open.len();
            continue;
        }
        let Some(gt) = lower[start..].find('>') else {
            break;
        };
        let attrs_start = start + open.len();
        let inner_start = start + gt + 1;
        // find the matching close, counting nested opens
        let mut depth = 1;
        let mut j = inner_start;
        let mut end = None;
        while j < lower.len() {
            let next_open = lower[j..].find(&open).map(|x| j + x);
            let next_close = lower[j..].find(&close).map(|x| j + x);
            match (next_open, next_close) {
                (_, None) => break,
                (Some(o), Some(c)) if o < c => {
                    let a = lower.as_bytes().get(o + open.len()).copied();
                    if matches!(a, Some(b' ') | Some(b'>') | Some(b'\n') | Some(b'/')) {
                        depth += 1;
                    }
                    j = o + open.len();
                }
                (_, Some(c)) => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(c);
                        break;
                    }
                    j = c + close.len();
                }
            }
        }
        let Some(e) = end else { break };
        out.push((&html[attrs_start..start + gt], &html[inner_start..e]));
        i = e + close.len();
    }
    out
}

pub fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find(name) {
        let at = from + p;
        let prev = if at == 0 {
            b' '
        } else {
            lower.as_bytes()[at - 1]
        };
        let before_ok =
            !(prev.is_ascii_alphanumeric() || prev == b'-' || prev == b'_' || prev == b':');
        let rest = &attrs[at + name.len()..];
        let rest_t = rest.trim_start();
        if before_ok && rest_t.starts_with('=') {
            let v = rest_t[1..].trim_start();
            let q = v.chars().next()?;
            if q == '"' || q == '\'' {
                let end = v[1..].find(q)?;
                return Some(decode(&v[1..1 + end]));
            }
            let end = v.find(|c: char| c.is_whitespace()).unwrap_or(v.len());
            return Some(decode(&v[..end]));
        }
        from = at + name.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(
            text_of("<b>Fox</b> News&#39;s <a href='x'>c</a>"),
            "Fox News's c"
        );
        assert_eq!(decode("a &amp; b &lt;c&gt; &#x41;"), "a & b <c> A");
        assert_eq!(
            attr(" id=\"BBC\" class='x y'", "id").as_deref(),
            Some("BBC")
        );
        assert_eq!(attr(" data-id=\"z\" id=BBC ", "id").as_deref(), Some("BBC"));
        let e = elements("<td>a<td>b</td></td><td>c</td>", "td");
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].1, "c");
    }
}
