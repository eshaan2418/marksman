//! String helpers shared by the parser and the renderer.

use crate::entities;

/// Append `s` HTML-escaped (`& < > "`).
pub(crate) fn escape_html(out: &mut String, s: &str) {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let rep = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            _ => continue,
        };
        out.push_str(&s[last..i]);
        out.push_str(rep);
        last = i + 1;
    }
    out.push_str(&s[last..]);
}

/// Percent-encode a link destination the way commonmark.js (mdurl) does: keep ASCII
/// alphanumerics, `;/?:@&=+$,-_.!~*'()#`, and existing `%XX` escapes; encode everything else
/// as UTF-8 bytes.
pub(crate) fn normalize_uri(out: &mut String, s: &str) {
    const KEEP: &[u8] = b";/?:@&=+$,-_.!~*'()#"; // HTML escaping happens in the renderer
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || KEEP.contains(&b) {
            out.push(b as char);
        } else if b == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&s[i..i + 3]);
            i += 3;
            continue;
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        i += 1;
    }
}

/// Is `b` an ASCII punctuation character (CommonMark's definition, used for backslash escapes)?
pub(crate) fn is_ascii_punct(b: u8) -> bool {
    b.is_ascii_punctuation()
}

/// Decode an entity or numeric character reference at the start of `s` (which begins with `&`).
/// Returns the decoded text and the number of bytes consumed.
pub(crate) fn decode_entity(s: &str) -> Option<(String, usize)> {
    let b = s.as_bytes();
    if b.len() < 3 || b[0] != b'&' {
        return None;
    }
    if b[1] == b'#' {
        let (hex, start) = if b.len() > 2 && (b[2] == b'x' || b[2] == b'X') {
            (true, 3)
        } else {
            (false, 2)
        };
        let mut end = start;
        while end < b.len()
            && (if hex {
                b[end].is_ascii_hexdigit()
            } else {
                b[end].is_ascii_digit()
            })
        {
            end += 1;
        }
        let digits = end - start;
        if digits == 0 || digits > (if hex { 6 } else { 7 }) || end >= b.len() || b[end] != b';' {
            return None;
        }
        let v = u32::from_str_radix(&s[start..end], if hex { 16 } else { 10 }).ok()?;
        let c = if v == 0 {
            '\u{FFFD}'
        } else {
            char::from_u32(v).unwrap_or('\u{FFFD}')
        };
        return Some((c.to_string(), end + 1));
    }
    let mut end = 1;
    while end < b.len() && end <= 32 && b[end].is_ascii_alphanumeric() {
        end += 1;
    }
    if end == 1 || end >= b.len() || b[end] != b';' {
        return None;
    }
    entities::lookup(&s[1..end]).map(|v| (v.to_string(), end + 1))
}

/// Process backslash escapes and entity references (for link destinations, titles, info strings).
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('\\') && !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && is_ascii_punct(b[i + 1]) {
            out.push_str(&s[last..i]);
            out.push(b[i + 1] as char);
            i += 2;
            last = i;
        } else if b[i] == b'&' {
            if let Some((txt, n)) = decode_entity(&s[i..]) {
                out.push_str(&s[last..i]);
                out.push_str(&txt);
                i += n;
                last = i;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

/// Normalise a link label for matching: trim, collapse internal whitespace, Unicode case fold.
/// commonmark.js folds with toLowerCase().toUpperCase(); the same here handles e.g. "ẞ" vs "SS".
pub(crate) fn normalize_label(s: &str) -> String {
    let mut collapsed = String::with_capacity(s.len());
    let mut space = false;
    for c in s
        .trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r')
        .chars()
    {
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
            space = true;
        } else {
            if space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            space = false;
            collapsed.push(c);
        }
    }
    collapsed.to_lowercase().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_and_numeric_refs() {
        assert_eq!(decode_entity("&amp;x").unwrap(), ("&".to_string(), 5));
        assert_eq!(decode_entity("&#35;").unwrap().0, "#");
        assert_eq!(decode_entity("&#X22;").unwrap().0, "\"");
        assert_eq!(decode_entity("&#0;").unwrap().0, "\u{FFFD}");
        assert!(decode_entity("&#87654321;").is_none());
        assert!(decode_entity("&nosuch;").is_none());
        assert!(decode_entity("&copy").is_none());
    }

    #[test]
    fn uri_encoding() {
        let mut s = String::new();
        normalize_uri(&mut s, "/föö bar%20?a=1&b=2");
        assert_eq!(s, "/f%C3%B6%C3%B6%20bar%20?a=1&b=2");
    }

    #[test]
    fn labels() {
        assert_eq!(normalize_label("  Foo \n  BAR "), "FOO BAR");
        assert_eq!(normalize_label("ẞ"), normalize_label("SS"));
    }
}
