//! Strip HTML to plain text. Feeds put markup in summaries routinely; we
//! only want the words. Bespoke: a small state machine beats pulling in a
//! full HTML parser for this.

/// Remove tags, decode common entities, collapse whitespace, and truncate to
/// `max_chars` characters (on a char boundary).
pub fn strip(input: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(input.len().min(max_chars * 2));
    let mut in_tag = false;
    let mut chars = input.char_indices().peekable();
    let mut pending_space = false;
    let mut last_was_space = true; // suppress leading whitespace

    let push = |out: &mut String, c: char, pending: &mut bool, last: &mut bool| {
        if c.is_whitespace() {
            *pending = true;
            return;
        }
        if *pending && !*last {
            out.push(' ');
        }
        *pending = false;
        *last = false;
        out.push(c);
    };

    while let Some((i, c)) = chars.next() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                pending_space = true; // block tags separate words
            }
            continue;
        }
        match c {
            '<' => in_tag = true,
            '&' => {
                let rest = &input[i + 1..];
                if let Some(end) = rest.find(';').filter(|&e| e <= 10) {
                    let ent = &rest[..end];
                    if let Some(d) = decode_entity(ent) {
                        for _ in 0..=end {
                            chars.next();
                        }
                        push(&mut out, d, &mut pending_space, &mut last_was_space);
                        continue;
                    }
                }
                push(&mut out, '&', &mut pending_space, &mut last_was_space);
            }
            _ => push(&mut out, c, &mut pending_space, &mut last_was_space),
        }
    }

    if out.chars().count() > max_chars {
        let cut = out
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(out.len());
        out.truncate(cut);
        // don't end mid-word if we can help it
        if let Some(sp) = out.rfind(' ') {
            if sp > max_chars / 2 {
                out.truncate(sp);
            }
        }
        out.push('…');
    }
    out
}

fn decode_entity(ent: &str) -> Option<char> {
    Some(match ent {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" | "#39" => '\'',
        "nbsp" => ' ',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        _ => {
            let code = if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                let dec = ent.strip_prefix('#')?;
                dec.parse().ok()?
            };
            char::from_u32(code)?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_entities() {
        assert_eq!(
            strip("<p>Hello &amp; <b>world</b></p>", 512),
            "Hello & world"
        );
        assert_eq!(strip("a<br/>b", 512), "a b");
        assert_eq!(strip("  lots   of\n\nspace ", 512), "lots of space");
        assert_eq!(strip("&#8217;s &#x27;x&#x27;", 512), "’s 'x'");
        assert_eq!(strip("AT&T rocks", 512), "AT&T rocks");
    }

    #[test]
    fn truncates_on_word_boundary() {
        let s = strip("one two three four five six", 12);
        assert_eq!(s, "one two…");
    }

    #[test]
    fn unterminated_tag_is_dropped() {
        assert_eq!(strip("text <img src=x", 512), "text");
    }
}
