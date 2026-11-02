//! Hand-written scanners for the token grammars CommonMark defines with regular expressions
//! (HTML tags, autolinks, HTML block conditions). Each returns the number of bytes matched.

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    i
}

fn tag_name(s: &[u8], mut i: usize) -> Option<usize> {
    if i >= s.len() || !s[i].is_ascii_alphabetic() {
        return None;
    }
    i += 1;
    while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == b'-') {
        i += 1;
    }
    Some(i)
}

fn attribute(s: &[u8], start: usize) -> Option<usize> {
    // whitespace+ name (ws* = ws* value)?
    let mut i = skip_ws(s, start);
    if i == start || i >= s.len() || !(s[i].is_ascii_alphabetic() || s[i] == b'_' || s[i] == b':') {
        return None;
    }
    i += 1;
    while i < s.len() && (s[i].is_ascii_alphanumeric() || matches!(s[i], b'_' | b'.' | b':' | b'-'))
    {
        i += 1;
    }
    let j = skip_ws(s, i);
    if j < s.len() && s[j] == b'=' {
        let mut k = skip_ws(s, j + 1);
        if k >= s.len() {
            return None;
        }
        match s[k] {
            q @ (b'\'' | b'"') => {
                k += 1;
                while k < s.len() && s[k] != q {
                    k += 1;
                }
                if k >= s.len() {
                    return None;
                }
                return Some(k + 1);
            }
            _ => {
                let v = k;
                while k < s.len()
                    && !is_ws(s[k])
                    && !matches!(s[k], b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
                {
                    k += 1;
                }
                if k == v {
                    return None;
                }
                return Some(k);
            }
        }
    }
    Some(i)
}

/// `<tag attr="v" ...>` or `<tag/>`; `s[0] == b'<'`.
pub(crate) fn open_tag(s: &[u8]) -> Option<usize> {
    let mut i = tag_name(s, 1)?;
    while let Some(j) = attribute(s, i) {
        i = j;
    }
    i = skip_ws(s, i);
    if i < s.len() && s[i] == b'/' {
        i += 1;
    }
    (i < s.len() && s[i] == b'>').then_some(i + 1)
}

/// `</tag>`.
pub(crate) fn closing_tag(s: &[u8]) -> Option<usize> {
    if s.len() < 3 || s[1] != b'/' {
        return None;
    }
    let i = skip_ws(s, tag_name(s, 2)?);
    (i < s.len() && s[i] == b'>').then_some(i + 1)
}

fn find(s: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if from > s.len() {
        return None;
    }
    s[from..]
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| from + p)
}

/// Any inline raw HTML construct starting at `s[0] == b'<'`.
pub(crate) fn html_inline(s: &[u8]) -> Option<usize> {
    if s.len() < 2 {
        return None;
    }
    match s[1] {
        b'!' => {
            if s.starts_with(b"<!--") {
                if s.starts_with(b"<!-->") {
                    return Some(5);
                }
                if s.starts_with(b"<!--->") {
                    return Some(6);
                }
                return find(s, 4, b"-->").map(|p| p + 3);
            }
            if s.starts_with(b"<![CDATA[") {
                return find(s, 9, b"]]>").map(|p| p + 3);
            }
            if s.len() > 2 && s[2].is_ascii_alphabetic() {
                return find(s, 3, b">").map(|p| p + 1);
            }
            None
        }
        b'?' => find(s, 2, b"?>").map(|p| p + 2),
        b'/' => closing_tag(s),
        _ => open_tag(s),
    }
}

/// `<scheme:...>` autolink; returns bytes consumed including the angle brackets.
pub(crate) fn autolink_uri(s: &[u8]) -> Option<usize> {
    let mut i = 1;
    if i >= s.len() || !s[i].is_ascii_alphabetic() {
        return None;
    }
    i += 1;
    while i < s.len() && (s[i].is_ascii_alphanumeric() || matches!(s[i], b'+' | b'.' | b'-')) {
        i += 1;
    }
    let scheme_len = i - 1;
    if !(2..=32).contains(&scheme_len) || i >= s.len() || s[i] != b':' {
        return None;
    }
    i += 1;
    while i < s.len() && s[i] > b' ' && s[i] != b'<' && s[i] != b'>' {
        i += 1;
    }
    (i < s.len() && s[i] == b'>').then_some(i + 1)
}

/// `<user@example.com>` autolink.
pub(crate) fn autolink_email(s: &[u8]) -> Option<usize> {
    let mut i = 1;
    let start = i;
    while i < s.len() && (s[i].is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&s[i])) {
        i += 1;
    }
    if i == start || i >= s.len() || s[i] != b'@' {
        return None;
    }
    i += 1;
    loop {
        // label: [a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?
        let ls = i;
        if i >= s.len() || !s[i].is_ascii_alphanumeric() {
            return None;
        }
        while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == b'-') && i - ls < 63 {
            i += 1;
        }
        if s[i - 1] == b'-' {
            return None;
        }
        if i < s.len() && s[i] == b'.' {
            i += 1;
            continue;
        }
        break;
    }
    (i < s.len() && s[i] == b'>').then_some(i + 1)
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

fn starts_with_ci(s: &[u8], pat: &str) -> bool {
    s.len() >= pat.len() && s[..pat.len()].eq_ignore_ascii_case(pat.as_bytes())
}

/// HTML block start condition (1-7) for a line whose first non-space character is `<`.
pub(crate) fn html_block_start(s: &[u8], can_interrupt_paragraph: bool) -> Option<u8> {
    if s.first() != Some(&b'<') {
        return None;
    }
    for t in ["script", "pre", "textarea", "style"] {
        if starts_with_ci(&s[1..], t) {
            let after = s.get(1 + t.len()).copied();
            if after.is_none() || matches!(after, Some(b' ' | b'\t' | b'\n' | b'\r' | b'>')) {
                return Some(1);
            }
        }
    }
    if s.starts_with(b"<!--") {
        return Some(2);
    }
    if s.starts_with(b"<?") {
        return Some(3);
    }
    if s.len() > 2 && s[1] == b'!' && s[2].is_ascii_alphabetic() {
        return Some(4);
    }
    if s.starts_with(b"<![CDATA[") {
        return Some(5);
    }
    let name_start = if s.get(1) == Some(&b'/') { 2 } else { 1 };
    if let Some(end) = tag_name(s, name_start) {
        let name = std::str::from_utf8(&s[name_start..end])
            .unwrap_or("")
            .to_ascii_lowercase();
        if BLOCK_TAGS.contains(&name.as_str()) {
            let after = s.get(end).copied();
            let ok = match after {
                None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'>') => true,
                Some(b'/') => s.get(end + 1) == Some(&b'>'),
                _ => false,
            };
            if ok {
                return Some(6);
            }
        }
    }
    if !can_interrupt_paragraph {
        let tag = if name_start == 2 {
            closing_tag(s)
        } else {
            open_tag(s)
        };
        if let Some(n) = tag {
            let name_end = tag_name(s, name_start).unwrap_or(name_start);
            let name = std::str::from_utf8(&s[name_start..name_end])
                .unwrap_or("")
                .to_ascii_lowercase();
            if !["script", "style", "pre", "textarea"].contains(&name.as_str())
                && s[n..]
                    .iter()
                    .all(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
            {
                return Some(7);
            }
        }
    }
    None
}

/// Does this line end an HTML block of kind 1-5? (Kinds 6 and 7 end at a blank line.)
pub(crate) fn html_block_end(kind: u8, line: &[u8]) -> bool {
    let contains_ci = |pat: &str| {
        line.windows(pat.len())
            .any(|w| w.eq_ignore_ascii_case(pat.as_bytes()))
    };
    match kind {
        1 => {
            contains_ci("</script>")
                || contains_ci("</pre>")
                || contains_ci("</textarea>")
                || contains_ci("</style>")
        }
        2 => find(line, 0, b"-->").is_some(),
        3 => find(line, 0, b"?>").is_some(),
        4 => line.contains(&b'>'),
        5 => find(line, 0, b"]]>").is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(
            open_tag(b"<a href=\"x\" title='y' data-x=z disabled>rest"),
            Some(40)
        );
        assert_eq!(open_tag(b"<br/>"), Some(5));
        assert_eq!(open_tag(b"<a h*#ref=\"hi\">"), None);
        assert_eq!(closing_tag(b"</div  >"), Some(8));
        assert_eq!(html_inline(b"<!-- a -- b -->x"), Some(15));
        assert_eq!(html_inline(b"<!-->"), Some(5));
        assert_eq!(html_inline(b"<?php echo ?>"), Some(13));
        assert_eq!(html_inline(b"<!DOCTYPE html>"), Some(15));
    }

    #[test]
    fn autolinks() {
        assert_eq!(autolink_uri(b"<http://foo.bar/baz>"), Some(20));
        assert_eq!(autolink_uri(b"<m:abc>"), None); // scheme too short
        assert_eq!(autolink_uri(b"<http://foo bar>"), None);
        assert_eq!(autolink_email(b"<foo@bar.example.com>"), Some(21));
        assert_eq!(autolink_email(b"<foo@-bar.com>"), None);
    }

    #[test]
    fn html_blocks() {
        assert_eq!(html_block_start(b"<div class=x>", true), Some(6));
        assert_eq!(html_block_start(b"<script>", true), Some(1));
        assert_eq!(html_block_start(b"<custom-tag>", false), Some(7));
        assert_eq!(html_block_start(b"<custom-tag>", true), None);
        assert!(html_block_end(1, b"x </SCRIPT> y"));
    }
}
