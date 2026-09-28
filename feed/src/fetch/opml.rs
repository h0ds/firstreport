//! OPML import: pull `xmlUrl` outlines out of an OPML document. A small
//! tolerant attribute scanner rather than an XML parser, because OPML in
//! the wild is frequently not well-formed.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpmlFeed {
    pub url: String,
    pub title: Option<String>,
    pub html_url: Option<String>,
}

/// Attributes of one tag body (everything between `<outline` and `>`).
pub fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        let start = i;
        while i < b.len()
            && b[i] != b'='
            && !(b[i] as char).is_whitespace()
            && b[i] != b'/'
            && b[i] != b'>'
        {
            i += 1;
        }
        let name = tag[start..i].to_string();
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && (b[i] as char).is_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                let value = decode(&tag[vs..i.min(b.len())]);
                i += 1;
                if !name.is_empty() {
                    out.push((name.to_ascii_lowercase(), value));
                }
                continue;
            }
        }
        if name.is_empty() {
            i += 1;
        }
    }
    out
}

fn decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

pub fn parse(text: &str) -> Vec<OpmlFeed> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("<outline") {
        let start = from + pos + "<outline".len();
        let Some(end_rel) = text[start..].find('>') else {
            break;
        };
        let body = &text[start..start + end_rel];
        from = start + end_rel + 1;
        let a = attrs(body);
        let get = |k: &str| {
            a.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .filter(|v| !v.is_empty())
        };
        if let Some(url) = get("xmlurl") {
            out.push(OpmlFeed {
                url,
                title: get("title").or_else(|| get("text")),
                html_url: get("htmlurl"),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_feeds_from_messy_opml() {
        let doc = r#"<?xml version="1.0"?><opml version="2.0"><head><title>Subs</title></head><body>
          <outline text="News">
            <outline type="rss" text="BBC &amp; more" title="BBC" xmlUrl="https://feeds.bbci.co.uk/news/rss.xml" htmlUrl="https://bbc.co.uk"/>
            <outline TYPE='rss' text='Wire' XMLURL='https://wire.example/rss?a=1&amp;b=2'>
          </outline>
          <outline text="no url here"></outline>
        </body></opml>"#;
        let f = parse(doc);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].url, "https://feeds.bbci.co.uk/news/rss.xml");
        assert_eq!(f[0].title.as_deref(), Some("BBC"));
        assert_eq!(f[0].html_url.as_deref(), Some("https://bbc.co.uk"));
        assert_eq!(f[1].url, "https://wire.example/rss?a=1&b=2");
        assert_eq!(f[1].title.as_deref(), Some("Wire"));
    }
}
