//! Phase 2: inline parsing of paragraph and heading text: code spans, emphasis (the
//! delimiter-run algorithm), links and images (the bracket stack), autolinks, raw HTML,
//! entities, and line breaks. Mirrors the "Phase 2: inline structure" algorithm in the spec.

use std::collections::HashMap;

use crate::node::{Arena, NodeId, NodeValue};
use crate::scanners;
use crate::unicode;
use crate::util;

/// Normalised label -> (destination, title).
pub(crate) type RefMap = HashMap<String, (String, String)>;

struct Delim {
    cc: u8,
    numdelims: usize,
    origdelims: usize,
    node: NodeId,
    can_open: bool,
    can_close: bool,
    previous: Option<usize>,
    next: Option<usize>,
    active: bool, // removed from the stack
}

struct Bracket {
    node: NodeId,
    previous_delimiter: Option<usize>,
    index: usize,
    image: bool,
    active: bool,
    bracket_after: bool,
}

struct Inline<'a> {
    arena: &'a mut Arena,
    refmap: &'a RefMap,
    subject: String,
    pos: usize,
    delims: Vec<Delim>,
    top: Option<usize>, // most recent live delimiter
    brackets: Vec<Bracket>,
}

/// Parse inline content of every paragraph and heading under `root`.
pub(crate) fn process(arena: &mut Arena, root: NodeId, refmap: &RefMap) {
    let mut stack = vec![root];
    let mut leaves = Vec::new();
    while let Some(n) = stack.pop() {
        match arena.nodes[n].value {
            NodeValue::Paragraph | NodeValue::Heading { .. } => leaves.push(n),
            _ => {
                let kids: Vec<NodeId> = arena.children(n).collect();
                stack.extend(kids.into_iter().rev());
            }
        }
    }
    for leaf in leaves {
        let content = std::mem::take(&mut arena.nodes[leaf].content);
        let subject = content
            .trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r')
            .to_string();
        let mut p = Inline {
            arena,
            refmap,
            subject,
            pos: 0,
            delims: Vec::new(),
            top: None,
            brackets: Vec::new(),
        };
        while p.parse_inline(leaf) {}
        p.process_emphasis(None);
    }
}

fn text(arena: &mut Arena, s: &str) -> NodeId {
    arena.add(NodeValue::Text(s.to_string()), 0)
}

impl Inline<'_> {
    fn peek(&self) -> Option<u8> {
        self.subject.as_bytes().get(self.pos).copied()
    }

    fn append(&mut self, block: NodeId, value: NodeValue) -> NodeId {
        let n = self.arena.add(value, 0);
        self.arena.append_child(block, n);
        n
    }

    fn append_text(&mut self, block: NodeId, s: &str) -> NodeId {
        // Merge with a preceding text node to keep the tree small.
        if let Some(last) = self.arena.nodes[block].last_child
            && let NodeValue::Text(t) = &mut self.arena.nodes[last].value
            && !self.delims.iter().any(|d| d.active && d.node == last)
            && !self.brackets.iter().any(|b| b.node == last)
        {
            t.push_str(s);
            return last;
        }
        let n = text(self.arena, s);
        self.arena.append_child(block, n);
        n
    }

    fn parse_inline(&mut self, block: NodeId) -> bool {
        let Some(c) = self.peek() else { return false };
        let handled = match c {
            b'\n' => self.parse_newline(block),
            b'\\' => self.parse_backslash(block),
            b'`' => self.parse_backticks(block),
            b'*' | b'_' => self.handle_delim(c, block),
            b'[' => {
                let start = self.pos;
                self.pos += 1;
                let n = text(self.arena, "[");
                self.arena.append_child(block, n);
                self.add_bracket(n, start, false);
                true
            }
            b'!' => {
                let start = self.pos;
                self.pos += 1;
                if self.peek() == Some(b'[') {
                    self.pos += 1;
                    let n = text(self.arena, "![");
                    self.arena.append_child(block, n);
                    self.add_bracket(n, start + 1, true);
                } else {
                    self.append_text(block, "!");
                }
                true
            }
            b']' => self.parse_close_bracket(block),
            b'<' => self.parse_autolink(block) || self.parse_html_tag(block),
            b'&' => self.parse_entity(block),
            _ => self.parse_string(block),
        };
        if !handled {
            self.pos += 1;
            let ch = (c as char).to_string();
            self.append_text(block, &ch);
        }
        true
    }

    fn parse_string(&mut self, block: NodeId) -> bool {
        let b = self.subject.as_bytes();
        let start = self.pos;
        let mut i = start;
        while i < b.len()
            && !matches!(
                b[i],
                b'\n' | b'`' | b'[' | b']' | b'\\' | b'!' | b'<' | b'&' | b'*' | b'_'
            )
        {
            i += 1;
        }
        if i == start {
            return false;
        }
        self.pos = i;
        let s = self.subject[start..i].to_string();
        self.append_text(block, &s);
        true
    }

    fn parse_newline(&mut self, block: NodeId) -> bool {
        self.pos += 1;
        let mut hard = false;
        if let Some(last) = self.arena.nodes[block].last_child
            && let NodeValue::Text(t) = &mut self.arena.nodes[last].value
            && t.ends_with(' ')
        {
            hard = t.ends_with("  ");
            let trimmed = t.trim_end_matches(' ').len();
            t.truncate(trimmed);
        }
        self.append(
            block,
            if hard {
                NodeValue::LineBreak
            } else {
                NodeValue::SoftBreak
            },
        );
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
        true
    }

    fn parse_backslash(&mut self, block: NodeId) -> bool {
        self.pos += 1;
        match self.peek() {
            Some(b'\n') => {
                self.pos += 1;
                self.append(block, NodeValue::LineBreak);
            }
            Some(c) if util::is_ascii_punct(c) => {
                self.pos += 1;
                let s = (c as char).to_string();
                // An escaped character is never a delimiter: give it its own text node.
                let n = text(self.arena, &s);
                self.arena.append_child(block, n);
            }
            _ => {
                self.append_text(block, "\\");
            }
        }
        true
    }

    fn parse_backticks(&mut self, block: NodeId) -> bool {
        let b = self.subject.as_bytes();
        let start = self.pos;
        let mut i = start;
        while i < b.len() && b[i] == b'`' {
            i += 1;
        }
        let n = i - start;
        let after_open = i;
        // Find a closing run of exactly n backticks.
        let mut j = after_open;
        while j < b.len() {
            if b[j] == b'`' {
                let run_start = j;
                while j < b.len() && b[j] == b'`' {
                    j += 1;
                }
                if j - run_start == n {
                    let mut contents = self.subject[after_open..run_start].replace('\n', " ");
                    if contents.len() >= 2
                        && contents.starts_with(' ')
                        && contents.ends_with(' ')
                        && contents.bytes().any(|c| c != b' ')
                    {
                        contents = contents[1..contents.len() - 1].to_string();
                    }
                    self.pos = j;
                    self.append(block, NodeValue::Code(contents));
                    return true;
                }
            } else {
                j += 1;
            }
        }
        self.pos = after_open;
        let ticks = self.subject[start..after_open].to_string();
        self.append_text(block, &ticks);
        true
    }

    fn char_before(&self, pos: usize) -> char {
        if pos == 0 {
            return '\n';
        }
        self.subject[..pos].chars().next_back().unwrap_or('\n')
    }

    fn char_at(&self, pos: usize) -> char {
        self.subject[pos..].chars().next().unwrap_or('\n')
    }

    fn scan_delims(&self, cc: u8) -> (usize, bool, bool) {
        let b = self.subject.as_bytes();
        let mut n = 0;
        while self.pos + n < b.len() && b[self.pos + n] == cc {
            n += 1;
        }
        let before = self.char_before(self.pos);
        let after = if self.pos + n < b.len() {
            self.char_at(self.pos + n)
        } else {
            '\n'
        };
        let after_ws = unicode::is_whitespace(after);
        let after_p = unicode::is_punctuation(after);
        let before_ws = unicode::is_whitespace(before);
        let before_p = unicode::is_punctuation(before);
        let left = !after_ws && (!after_p || before_ws || before_p);
        let right = !before_ws && (!before_p || after_ws || after_p);
        let (can_open, can_close) = if cc == b'_' {
            (left && (!right || before_p), right && (!left || after_p))
        } else {
            (left, right)
        };
        (n, can_open, can_close)
    }

    fn handle_delim(&mut self, cc: u8, block: NodeId) -> bool {
        let (n, can_open, can_close) = self.scan_delims(cc);
        if n == 0 {
            return false;
        }
        let start = self.pos;
        self.pos += n;
        let s = self.subject[start..self.pos].to_string();
        let node = text(self.arena, &s);
        self.arena.append_child(block, node);
        if can_open || can_close {
            let idx = self.delims.len();
            self.delims.push(Delim {
                cc,
                numdelims: n,
                origdelims: n,
                node,
                can_open,
                can_close,
                previous: self.top,
                next: None,
                active: true,
            });
            if let Some(t) = self.top {
                self.delims[t].next = Some(idx);
            }
            self.top = Some(idx);
        }
        true
    }

    fn remove_delim(&mut self, d: usize) {
        let (prev, next) = (self.delims[d].previous, self.delims[d].next);
        if let Some(p) = prev {
            self.delims[p].next = next;
        }
        match next {
            Some(nx) => self.delims[nx].previous = prev,
            None => self.top = prev,
        }
        self.delims[d].active = false;
    }

    fn process_emphasis(&mut self, stack_bottom: Option<usize>) {
        let mut openers_bottom: [Option<usize>; 14] = [stack_bottom; 14];
        // First closer above stack_bottom.
        let mut closer = self.top;
        while let Some(c) = closer {
            if self.delims[c].previous == stack_bottom {
                break;
            }
            closer = self.delims[c].previous;
        }
        while let Some(c) = closer {
            if !self.delims[c].can_close {
                closer = self.delims[c].next;
                continue;
            }
            let cc = self.delims[c].cc;
            let bottom_index = (if cc == b'_' { 2 } else { 8 })
                + if self.delims[c].can_open { 3 } else { 0 }
                + self.delims[c].origdelims % 3;
            let mut opener = self.delims[c].previous;
            let mut found = false;
            while let Some(o) = opener {
                if Some(o) == stack_bottom || Some(o) == openers_bottom[bottom_index] {
                    break;
                }
                let odd_match = (self.delims[c].can_open || self.delims[o].can_close)
                    && self.delims[c].origdelims % 3 != 0
                    && (self.delims[o].origdelims + self.delims[c].origdelims) % 3 == 0;
                if self.delims[o].cc == cc && self.delims[o].can_open && !odd_match {
                    found = true;
                    break;
                }
                opener = self.delims[o].previous;
            }
            let old_closer = c;
            if found {
                let o = opener.expect("found implies opener");
                let use_delims = if self.delims[c].numdelims >= 2 && self.delims[o].numdelims >= 2 {
                    2
                } else {
                    1
                };
                let (on, cn) = (self.delims[o].node, self.delims[c].node);
                self.delims[o].numdelims -= use_delims;
                self.delims[c].numdelims -= use_delims;
                for n in [on, cn] {
                    if let NodeValue::Text(t) = &mut self.arena.nodes[n].value {
                        let len = t.len() - use_delims;
                        t.truncate(len);
                    }
                }
                let emph = self.arena.add(
                    if use_delims == 1 {
                        NodeValue::Emph
                    } else {
                        NodeValue::Strong
                    },
                    0,
                );
                let mut tmp = self.arena.nodes[on].next;
                while let Some(t) = tmp {
                    if t == cn {
                        break;
                    }
                    let next = self.arena.nodes[t].next;
                    self.arena.append_child(emph, t);
                    tmp = next;
                }
                self.arena.insert_after(on, emph);
                // Remove delimiters between opener and closer.
                let mut d = self.delims[c].previous;
                while let Some(x) = d {
                    if x == o {
                        break;
                    }
                    let prev = self.delims[x].previous;
                    self.remove_delim(x);
                    d = prev;
                }
                if self.delims[o].numdelims == 0 {
                    self.arena.unlink(on);
                    self.remove_delim(o);
                }
                if self.delims[c].numdelims == 0 {
                    self.arena.unlink(cn);
                    let next = self.delims[c].next;
                    self.remove_delim(c);
                    closer = next;
                }
                // else: keep processing the same closer
            } else {
                closer = self.delims[c].next;
                openers_bottom[bottom_index] = self.delims[old_closer].previous;
                if !self.delims[old_closer].can_open {
                    self.remove_delim(old_closer);
                }
            }
        }
        while let Some(t) = self.top {
            if Some(t) == stack_bottom {
                break;
            }
            self.remove_delim(t);
        }
    }

    fn add_bracket(&mut self, node: NodeId, index: usize, image: bool) {
        if let Some(last) = self.brackets.last_mut() {
            last.bracket_after = true;
        }
        self.brackets.push(Bracket {
            node,
            previous_delimiter: self.top,
            index,
            image,
            active: true,
            bracket_after: false,
        });
    }

    fn spnl(&mut self) {
        let b = self.subject.as_bytes();
        while self.pos < b.len() && (b[self.pos] == b' ' || b[self.pos] == b'\t') {
            self.pos += 1;
        }
        if self.pos < b.len() && b[self.pos] == b'\n' {
            self.pos += 1;
            while self.pos < b.len() && (b[self.pos] == b' ' || b[self.pos] == b'\t') {
                self.pos += 1;
            }
        }
    }

    fn parse_close_bracket(&mut self, block: NodeId) -> bool {
        self.pos += 1;
        let startpos = self.pos;
        let Some(opener) = self.brackets.last() else {
            self.append_text(block, "]");
            return true;
        };
        if !opener.active {
            self.append_text(block, "]");
            self.brackets.pop();
            return true;
        }
        let is_image = opener.image;
        let opener_index = opener.index;
        let opener_bracket_after = opener.bracket_after;
        let savepos = self.pos;
        let mut dest = String::new();
        let mut title = String::new();
        let mut matched = false;

        if self.peek() == Some(b'(') {
            self.pos += 1;
            self.spnl();
            if let Some(d) = parse_link_destination(&self.subject, &mut self.pos) {
                let before_title = self.pos;
                self.spnl();
                let ws_before = self.pos > before_title
                    && self.subject.as_bytes()[self.pos - 1].is_ascii_whitespace();
                if ws_before && let Some(t) = parse_link_title(&self.subject, &mut self.pos) {
                    title = t;
                }
                self.spnl();
                if self.peek() == Some(b')') {
                    self.pos += 1;
                    dest = d;
                    matched = true;
                }
            }
            if !matched {
                title.clear();
                self.pos = savepos;
            }
        }
        if !matched {
            let before_label = self.pos;
            let n = parse_link_label(&self.subject, self.pos);
            let reflabel = if n > 2 {
                Some(self.subject[before_label..before_label + n].to_string())
            } else if !opener_bracket_after {
                Some(self.subject[opener_index..startpos].to_string())
            } else {
                None
            };
            if n == 0 {
                self.pos = savepos;
            } else {
                self.pos += n;
            }
            if let Some(label) = reflabel {
                let inner = &label[1..label.len() - 1];
                if let Some((d, t)) = self.refmap.get(&util::normalize_label(inner)) {
                    dest = d.clone();
                    title = t.clone();
                    matched = true;
                }
            }
            if !matched {
                self.pos = savepos;
            }
        }
        if matched {
            let opener = self.brackets.pop().expect("checked above");
            let node = self.arena.add(
                if is_image {
                    NodeValue::Image { dest, title }
                } else {
                    NodeValue::Link { dest, title }
                },
                0,
            );
            let mut tmp = self.arena.nodes[opener.node].next;
            while let Some(t) = tmp {
                let next = self.arena.nodes[t].next;
                self.arena.append_child(node, t);
                tmp = next;
            }
            self.arena.append_child(block, node);
            self.process_emphasis(opener.previous_delimiter);
            self.arena.unlink(opener.node);
            if !is_image {
                for b in &mut self.brackets {
                    if !b.image {
                        b.active = false; // no links inside links
                    }
                }
            }
        } else {
            self.brackets.pop();
            self.pos = startpos;
            self.append_text(block, "]");
        }
        true
    }

    fn parse_autolink(&mut self, block: NodeId) -> bool {
        let rest = &self.subject.as_bytes()[self.pos..];
        let (len, email) = if let Some(n) = scanners::autolink_email(rest) {
            (n, true)
        } else if let Some(n) = scanners::autolink_uri(rest) {
            (n, false)
        } else {
            return false;
        };
        let inner = self.subject[self.pos + 1..self.pos + len - 1].to_string();
        self.pos += len;
        let mut dest = String::new();
        util::normalize_uri(
            &mut dest,
            &if email {
                format!("mailto:{inner}")
            } else {
                inner.clone()
            },
        );
        let link = self.append(
            block,
            NodeValue::Link {
                dest,
                title: String::new(),
            },
        );
        let t = text(self.arena, &inner);
        self.arena.append_child(link, t);
        true
    }

    fn parse_html_tag(&mut self, block: NodeId) -> bool {
        let rest = &self.subject.as_bytes()[self.pos..];
        let Some(n) = scanners::html_inline(rest) else {
            return false;
        };
        let s = self.subject[self.pos..self.pos + n].to_string();
        self.pos += n;
        self.append(block, NodeValue::HtmlInline(s));
        true
    }

    fn parse_entity(&mut self, block: NodeId) -> bool {
        let Some((txt, n)) = util::decode_entity(&self.subject[self.pos..]) else {
            return false;
        };
        self.pos += n;
        let node = text(self.arena, &txt);
        self.arena.append_child(block, node);
        true
    }
}

/// `[label]` with at most 999 characters, no unescaped brackets. Returns its byte length or 0.
fn parse_link_label(s: &str, start: usize) -> usize {
    let b = s.as_bytes();
    if b.get(start) != Some(&b'[') {
        return 0;
    }
    let mut i = start + 1;
    let mut chars = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() => {
                i += 2;
                chars += 2;
            }
            b'[' => return 0,
            b']' => {
                return if chars > 999 { 0 } else { i + 1 - start };
            }
            _ => {
                i += 1;
                chars += 1;
            }
        }
        if chars > 999 {
            return 0;
        }
    }
    0
}

/// Link destination: `<...>` or a run without spaces/controls and with balanced parentheses.
/// Returns the normalised URL and advances `pos`.
fn parse_link_destination(s: &str, pos: &mut usize) -> Option<String> {
    let b = s.as_bytes();
    let start = *pos;
    let raw = if b.get(start) == Some(&b'<') {
        let mut i = start + 1;
        while i < b.len() {
            match b[i] {
                b'\\' if i + 1 < b.len() && util::is_ascii_punct(b[i + 1]) => i += 2,
                b'>' => break,
                b'<' | b'\n' => return None,
                _ => i += 1,
            }
        }
        if i >= b.len() {
            return None;
        }
        *pos = i + 1;
        &s[start + 1..i]
    } else {
        let mut i = start;
        let mut parens = 0usize;
        while i < b.len() {
            let c = b[i];
            if c == b'\\' && i + 1 < b.len() && util::is_ascii_punct(b[i + 1]) {
                i += 2;
            } else if c == b'(' {
                parens += 1;
                i += 1;
            } else if c == b')' {
                if parens == 0 {
                    break;
                }
                parens -= 1;
                i += 1;
            } else if c <= b' ' || c == 0x7f {
                break;
            } else {
                i += 1;
            }
        }
        if i == start && b.get(i) != Some(&b')') {
            return None;
        }
        if parens != 0 {
            return None;
        }
        *pos = i;
        &s[start..i]
    };
    let mut out = String::new();
    util::normalize_uri(&mut out, &util::unescape(raw));
    Some(out)
}

/// `"..."`, `'...'`, or `(...)` with backslash escapes; returns the unescaped title.
fn parse_link_title(s: &str, pos: &mut usize) -> Option<String> {
    let b = s.as_bytes();
    let open = *b.get(*pos)?;
    let close = match open {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut i = *pos + 1;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' && i + 1 < b.len() && util::is_ascii_punct(b[i + 1]) {
            i += 2;
            continue;
        }
        if c == close {
            let t = util::unescape(&s[*pos + 1..i]);
            *pos = i + 1;
            return Some(t);
        }
        if open == b'(' && c == b'(' {
            return None;
        }
        i += 1;
    }
    None
}

/// Try to parse a link reference definition at the start of `s`; on success records it in
/// `refmap` (first definition wins) and returns the number of bytes consumed.
pub(crate) fn parse_reference(s: &str, refmap: &mut RefMap) -> Option<usize> {
    let b = s.as_bytes();
    let n = parse_link_label(s, 0);
    if n == 0 {
        return None;
    }
    let raw_label = &s[1..n - 1];
    let mut pos = n;
    if b.get(pos) != Some(&b':') {
        return None;
    }
    pos += 1;
    let spnl = |pos: &mut usize| {
        while *pos < b.len() && (b[*pos] == b' ' || b[*pos] == b'\t') {
            *pos += 1;
        }
        if *pos < b.len() && b[*pos] == b'\n' {
            *pos += 1;
            while *pos < b.len() && (b[*pos] == b' ' || b[*pos] == b'\t') {
                *pos += 1;
            }
        }
    };
    spnl(&mut pos);
    let dest = parse_link_destination(s, &mut pos)?;
    let before_title = pos;
    spnl(&mut pos);
    let mut title = None;
    if pos != before_title {
        title = parse_link_title(s, &mut pos);
    }
    if title.is_none() {
        pos = before_title;
    }
    let at_line_end = |pos: &mut usize| {
        let mut i = *pos;
        while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if i == b.len() || b[i] == b'\n' {
            *pos = if i < b.len() { i + 1 } else { i };
            true
        } else {
            false
        }
    };
    if !at_line_end(&mut pos) {
        if title.is_none() {
            return None;
        }
        // The title is not followed by the end of the line: drop it and retry without it.
        title = None;
        pos = before_title;
        if !at_line_end(&mut pos) {
            return None;
        }
    }
    let label = util::normalize_label(raw_label);
    if label.is_empty() {
        return None;
    }
    refmap
        .entry(label)
        .or_insert((dest, title.unwrap_or_default()));
    Some(pos)
}
