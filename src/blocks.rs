//! Phase 1: block structure. Each line is matched against the open container blocks, new
//! block starts are tried on the remainder, and the rest of the line is added to the innermost
//! open block (or lazily continues a paragraph). This follows the parsing strategy in the
//! appendix of the CommonMark spec (the same algorithm as commonmark.js and cmark).

use std::collections::HashMap;

use crate::inlines::{self, RefMap};
use crate::node::{Arena, ListData, NodeId, NodeValue};
use crate::scanners;
use crate::util;

const CODE_INDENT: usize = 4;

#[derive(PartialEq, Eq)]
enum Cont {
    Matched,
    NotMatched,
    LineConsumed, // a closing code fence finished the block and the line
}

pub(crate) struct BlockParser<'a> {
    pub arena: Arena,
    pub doc: NodeId,
    tip: NodeId,
    oldtip: NodeId,
    line: &'a str,
    line_number: usize,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
    all_closed: bool,
    last_matched_container: NodeId,
    /// First non-space/tab byte at or after the start of the last whitespace scan on this line,
    /// so the per-container rescans of deep indentation are O(1) (nested lists stay linear).
    nn_cache: (usize, usize, bool), // (line number, position, scanned region contained a tab)
    pub refmap: RefMap,
}

/// Length of `s` after removing a trailing run of `\n` + spaces groups (the regex `(\n *)+$`).
fn strip_trailing_blank_lines(s: &str) -> usize {
    let mut end = s.len();
    loop {
        let t = s[..end].trim_end_matches(' ');
        if t.ends_with('\n') {
            end = t.len() - 1;
        } else {
            return end;
        }
    }
}

fn is_space_or_tab(b: Option<u8>) -> bool {
    matches!(b, Some(b' ' | b'\t'))
}

impl<'a> BlockParser<'a> {
    pub fn new() -> Self {
        let mut arena = Arena::default();
        let doc = arena.add(NodeValue::Document, 0);
        BlockParser {
            arena,
            doc,
            tip: doc,
            oldtip: doc,
            line: "",
            line_number: 0,
            offset: 0,
            column: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            partially_consumed_tab: false,
            all_closed: true,
            last_matched_container: doc,
            nn_cache: (0, 0, true),
            refmap: HashMap::new(),
        }
    }

    pub fn parse(mut self, input: &'a str) -> (Arena, NodeId, RefMap) {
        let mut lines: Vec<&str> = Vec::new();
        let b = input.as_bytes();
        let mut start = 0;
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\n' || b[i] == b'\r' {
                lines.push(&input[start..i]);
                if b[i] == b'\r' && i + 1 < b.len() && b[i + 1] == b'\n' {
                    i += 1;
                }
                start = i + 1;
            }
            i += 1;
        }
        if start < b.len() {
            lines.push(&input[start..]);
        }
        let n = lines.len();
        for l in lines {
            self.incorporate_line(l);
        }
        while self.tip != self.doc {
            self.finalize(self.tip, n);
        }
        self.finalize(self.doc, n);
        inlines::process(&mut self.arena, self.doc, &self.refmap);
        (self.arena, self.doc, self.refmap)
    }

    fn peek(&self, pos: usize) -> Option<u8> {
        self.line.as_bytes().get(pos).copied()
    }

    fn find_next_nonspace(&mut self) {
        let b = self.line.as_bytes();
        let (cached_line, cached_pos, cached_tab) = self.nn_cache;
        let (i, cols) =
            if cached_line == self.line_number && self.offset <= cached_pos && !cached_tab {
                // Everything in [offset, cached_pos) is spaces: no need to rescan.
                (cached_pos, self.column + (cached_pos - self.offset))
            } else {
                let mut i = self.offset;
                let mut cols = self.column;
                let mut tab = false;
                while i < b.len() {
                    match b[i] {
                        b' ' => {
                            i += 1;
                            cols += 1;
                        }
                        b'\t' => {
                            i += 1;
                            cols += 4 - (cols % 4);
                            tab = true;
                        }
                        _ => break,
                    }
                }
                self.nn_cache = (self.line_number, i, tab);
                (i, cols)
            };
        self.blank = i >= b.len();
        self.next_nonspace = i;
        self.next_nonspace_column = cols;
        self.indent = self.next_nonspace_column - self.column;
        self.indented = self.indent >= CODE_INDENT;
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        let b = self.line.as_bytes();
        while count > 0 && self.offset < b.len() {
            if b[self.offset] == b'\t' {
                let to_tab = 4 - (self.column % 4);
                if columns {
                    self.partially_consumed_tab = to_tab > count;
                    let adv = to_tab.min(count);
                    self.column += adv;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= adv;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += 1;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn add_line(&mut self) {
        if self.partially_consumed_tab {
            self.offset += 1;
            let to_tab = 4 - (self.column % 4);
            for _ in 0..to_tab {
                self.arena.nodes[self.tip].content.push(' ');
            }
        }
        let rest = &self.line[self.offset.min(self.line.len())..];
        let c = &mut self.arena.nodes[self.tip].content;
        c.push_str(rest);
        c.push('\n');
    }

    fn can_contain(&self, parent: NodeId, child: &NodeValue) -> bool {
        match &self.arena.nodes[parent].value {
            NodeValue::Document | NodeValue::BlockQuote | NodeValue::Item(_) => {
                !matches!(child, NodeValue::Item(_))
            }
            NodeValue::List(_) => matches!(child, NodeValue::Item(_)),
            _ => false,
        }
    }

    fn accepts_lines(v: &NodeValue) -> bool {
        matches!(
            v,
            NodeValue::Paragraph | NodeValue::CodeBlock { .. } | NodeValue::HtmlBlock { .. }
        )
    }

    fn add_child(&mut self, value: NodeValue) -> NodeId {
        while !self.can_contain(self.tip, &value) {
            self.finalize(self.tip, self.line_number - 1);
        }
        let n = self.arena.add(value, self.line_number);
        self.arena.append_child(self.tip, n);
        self.tip = n;
        n
    }

    fn close_unmatched_blocks(&mut self) {
        if !self.all_closed {
            while self.oldtip != self.last_matched_container {
                let parent = self.arena.nodes[self.oldtip]
                    .parent
                    .expect("open block has a parent");
                self.finalize(self.oldtip, self.line_number - 1);
                self.oldtip = parent;
            }
            self.all_closed = true;
        }
    }

    fn continue_block(&mut self, container: NodeId) -> Cont {
        match self.arena.nodes[container].value.clone() {
            NodeValue::Document | NodeValue::List(_) => Cont::Matched,
            NodeValue::BlockQuote => {
                if !self.indented && self.peek(self.next_nonspace) == Some(b'>') {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if is_space_or_tab(self.peek(self.offset)) {
                        self.advance_offset(1, true);
                    }
                    Cont::Matched
                } else {
                    Cont::NotMatched
                }
            }
            NodeValue::Item(d) => {
                if self.blank {
                    if self.arena.nodes[container].first_child.is_none() {
                        return Cont::NotMatched; // a list item can begin with at most one blank line
                    }
                    self.advance_next_nonspace();
                } else if self.indent >= d.marker_offset + d.padding {
                    self.advance_offset(d.marker_offset + d.padding, true);
                } else {
                    return Cont::NotMatched;
                }
                Cont::Matched
            }
            NodeValue::Heading { .. } | NodeValue::ThematicBreak => Cont::NotMatched,
            NodeValue::CodeBlock {
                fenced: true,
                fence_char,
                fence_len,
                fence_offset,
                ..
            } => {
                let rest = &self.line.as_bytes()[self.next_nonspace.min(self.line.len())..];
                if self.indent <= 3 && rest.first() == Some(&fence_char) {
                    let n = rest.iter().take_while(|&&b| b == fence_char).count();
                    if n >= fence_len && rest[n..].iter().all(|&b| b == b' ' || b == b'\t') {
                        self.finalize(container, self.line_number);
                        return Cont::LineConsumed;
                    }
                }
                let mut i = fence_offset;
                while i > 0 && is_space_or_tab(self.peek(self.offset)) {
                    self.advance_offset(1, true);
                    i -= 1;
                }
                Cont::Matched
            }
            NodeValue::CodeBlock { .. } => {
                if self.indent >= CODE_INDENT {
                    self.advance_offset(CODE_INDENT, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return Cont::NotMatched;
                }
                Cont::Matched
            }
            NodeValue::HtmlBlock { kind, .. } => {
                if self.blank && (kind == 6 || kind == 7) {
                    Cont::NotMatched
                } else {
                    Cont::Matched
                }
            }
            NodeValue::Paragraph => {
                if self.blank {
                    Cont::NotMatched
                } else {
                    Cont::Matched
                }
            }
            _ => Cont::NotMatched,
        }
    }

    fn finalize(&mut self, block: NodeId, _line: usize) {
        let above = self.arena.nodes[block].parent;
        self.arena.nodes[block].open = false;
        let value = std::mem::replace(&mut self.arena.nodes[block].value, NodeValue::Document);
        let new_value = match value {
            NodeValue::Paragraph => {
                let mut content = std::mem::take(&mut self.arena.nodes[block].content);
                let mut had_refs = false;
                // Consume definitions by offset: draining each one would shift the rest of the
                // paragraph every time (quadratic in the number of definitions).
                let mut off = 0;
                while content[off..].starts_with('[') {
                    match inlines::parse_reference(&content[off..], &mut self.refmap) {
                        Some(n) => {
                            off += n;
                            had_refs = true;
                        }
                        None => break,
                    }
                }
                content.drain(..off);
                let empty = content
                    .bytes()
                    .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
                self.arena.nodes[block].content = content;
                if had_refs && empty {
                    self.arena.nodes[block].value = NodeValue::Paragraph;
                    self.arena.unlink(block);
                    self.tip = above.unwrap_or(self.doc);
                    return;
                }
                NodeValue::Paragraph
            }
            NodeValue::CodeBlock {
                fenced,
                fence_char,
                fence_len,
                fence_offset,
                ..
            } => {
                let content = std::mem::take(&mut self.arena.nodes[block].content);
                if fenced {
                    let nl = content.find('\n').unwrap_or(content.len());
                    let info =
                        util::unescape(content[..nl].trim_matches(|c: char| c == ' ' || c == '\t'));
                    let literal = if nl < content.len() {
                        content[nl + 1..].to_string()
                    } else {
                        String::new()
                    };
                    NodeValue::CodeBlock {
                        fenced,
                        fence_char,
                        fence_len,
                        fence_offset,
                        info,
                        literal,
                    }
                } else {
                    // /(\n *)+$/ -> "\n": drop trailing blank lines, keep one final newline.
                    let mut literal = content[..strip_trailing_blank_lines(&content)].to_string();
                    literal.push('\n');
                    NodeValue::CodeBlock {
                        fenced,
                        fence_char,
                        fence_len,
                        fence_offset,
                        info: String::new(),
                        literal,
                    }
                }
            }
            NodeValue::HtmlBlock { kind, .. } => {
                let content = std::mem::take(&mut self.arena.nodes[block].content);
                let end = strip_trailing_blank_lines(&content); // /(\n *)+$/ -> ""
                NodeValue::HtmlBlock {
                    kind,
                    literal: content[..end].to_string(),
                }
            }
            NodeValue::List(mut d) => {
                d.tight = true;
                let items: Vec<NodeId> = self.arena.children(block).collect();
                'outer: for (k, &item) in items.iter().enumerate() {
                    let has_next_item = k + 1 < items.len();
                    if has_next_item && self.ends_with_blank_line(item) {
                        d.tight = false;
                        break;
                    }
                    let subs: Vec<NodeId> = self.arena.children(item).collect();
                    for (j, &sub) in subs.iter().enumerate() {
                        if (has_next_item || j + 1 < subs.len()) && self.ends_with_blank_line(sub) {
                            d.tight = false;
                            break 'outer;
                        }
                    }
                }
                NodeValue::List(d)
            }
            other => other,
        };
        self.arena.nodes[block].value = new_value;
        self.tip = above.unwrap_or(self.doc);
    }

    fn ends_with_blank_line(&self, mut block: NodeId) -> bool {
        loop {
            if self.arena.nodes[block].last_line_blank {
                return true;
            }
            match self.arena.nodes[block].value {
                NodeValue::List(_) | NodeValue::Item(_) => match self.arena.nodes[block].last_child
                {
                    Some(c) => block = c,
                    None => return false,
                },
                _ => return false,
            }
        }
    }

    // --- block starts -------------------------------------------------------------------

    /// Try every block start. Returns 0 (none), 1 (container started), 2 (leaf started).
    fn try_starts(&mut self, container: NodeId) -> u8 {
        let rest = &self.line.as_bytes()[self.next_nonspace.min(self.line.len())..];
        let first = rest.first().copied();
        let tip_is_para = matches!(self.arena.nodes[container].value, NodeValue::Paragraph);

        // Block quote
        if !self.indented && first == Some(b'>') {
            self.advance_next_nonspace();
            self.advance_offset(1, false);
            if is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            self.close_unmatched_blocks();
            self.add_child(NodeValue::BlockQuote);
            return 1;
        }
        // ATX heading: /^#{1,6}(?:[ \t]+|$)/
        if !self.indented && first == Some(b'#') {
            let n = rest.iter().take_while(|&&b| b == b'#').count();
            if n <= 6 && (n == rest.len() || rest[n] == b' ' || rest[n] == b'\t') {
                self.advance_next_nonspace();
                self.advance_offset(n, false);
                self.close_unmatched_blocks();
                let h = self.add_child(NodeValue::Heading { level: n as u8 });
                let mut text = self.line[self.offset.min(self.line.len())..].to_string();
                // Remove a closing sequence: /^[ \t]*#+[ \t]*$/ or /[ \t]+#+[ \t]*$/
                let t = text.trim_end_matches([' ', '\t']);
                let without = t.trim_end_matches('#');
                if without.len() < t.len() {
                    if without.trim_matches([' ', '\t']).is_empty() {
                        text = String::new();
                    } else if without.ends_with([' ', '\t']) {
                        text = without.to_string();
                    }
                }
                self.arena.nodes[h].content = text;
                self.advance_offset(self.line.len() - self.offset, false);
                return 2;
            }
        }
        // Fenced code: /^`{3,}(?!.*`)|^~{3,}/
        if !self.indented && (first == Some(b'`') || first == Some(b'~')) {
            let c = first.unwrap_or(b'`');
            let n = rest.iter().take_while(|&&b| b == c).count();
            if n >= 3 && (c == b'~' || !rest[n..].contains(&b'`')) {
                self.close_unmatched_blocks();
                let fence_offset = self.indent;
                self.add_child(NodeValue::CodeBlock {
                    fenced: true,
                    fence_char: c,
                    fence_len: n,
                    fence_offset,
                    info: String::new(),
                    literal: String::new(),
                });
                self.advance_next_nonspace();
                self.advance_offset(n, false);
                return 2;
            }
        }
        // HTML block
        if !self.indented && first == Some(b'<') {
            let lazy_para = !self.all_closed
                && !self.blank
                && matches!(self.arena.nodes[self.tip].value, NodeValue::Paragraph);
            let can_interrupt = tip_is_para || lazy_para;
            if let Some(kind) = scanners::html_block_start(rest, can_interrupt) {
                self.close_unmatched_blocks();
                self.add_child(NodeValue::HtmlBlock {
                    kind,
                    literal: String::new(),
                });
                return 2;
            }
        }
        // Setext heading underline: /^(?:=+|-+)[ \t]*$/
        if !self.indented && tip_is_para && (first == Some(b'=') || first == Some(b'-')) {
            let c = first.unwrap_or(b'=');
            let n = rest.iter().take_while(|&&b| b == c).count();
            if rest[n..].iter().all(|&b| b == b' ' || b == b'\t') {
                self.close_unmatched_blocks();
                let mut content = std::mem::take(&mut self.arena.nodes[container].content);
                let mut off = 0;
                while content[off..].starts_with('[') {
                    match inlines::parse_reference(&content[off..], &mut self.refmap) {
                        Some(k) => off += k,
                        None => break,
                    }
                }
                content.drain(..off);
                if !content.is_empty() {
                    let heading = self.arena.add(
                        NodeValue::Heading {
                            level: if c == b'=' { 1 } else { 2 },
                        },
                        self.arena.nodes[container].start_line,
                    );
                    self.arena.nodes[heading].content = content;
                    self.arena.insert_after(container, heading);
                    self.arena.unlink(container);
                    self.tip = heading;
                    self.advance_offset(self.line.len() - self.offset, false);
                    return 2;
                }
                self.arena.nodes[container].content = content;
            }
        }
        // Thematic break: three or more of the same of * - _, optionally separated by spaces/tabs
        if !self.indented && matches!(first, Some(b'*' | b'-' | b'_')) {
            let c = first.unwrap_or(b'*');
            let mut count = 0;
            let mut ok = true;
            for &b in rest {
                if b == c {
                    count += 1;
                } else if b != b' ' && b != b'\t' {
                    ok = false;
                    break;
                }
            }
            if ok && count >= 3 {
                self.close_unmatched_blocks();
                self.add_child(NodeValue::ThematicBreak);
                self.advance_offset(self.line.len() - self.offset, false);
                return 2;
            }
        }
        // List item
        let container_is_list = matches!(self.arena.nodes[container].value, NodeValue::List(_));
        if (!self.indented || container_is_list)
            && let Some(data) = self.parse_list_marker(container)
        {
            self.close_unmatched_blocks();
            let matches = match self.arena.nodes[self.tip].value {
                NodeValue::List(l) => {
                    l.ordered == data.ordered
                        && l.delimiter == data.delimiter
                        && l.bullet_char == data.bullet_char
                }
                _ => false,
            };
            if !matches {
                self.add_child(NodeValue::List(data));
            }
            self.add_child(NodeValue::Item(data));
            return 1;
        }
        // Indented code
        if self.indented
            && !matches!(self.arena.nodes[self.tip].value, NodeValue::Paragraph)
            && !self.blank
        {
            self.advance_offset(CODE_INDENT, true);
            self.close_unmatched_blocks();
            self.add_child(NodeValue::CodeBlock {
                fenced: false,
                fence_char: 0,
                fence_len: 0,
                fence_offset: 0,
                info: String::new(),
                literal: String::new(),
            });
            return 2;
        }
        0
    }

    fn parse_list_marker(&mut self, container: NodeId) -> Option<ListData> {
        if self.indent >= 4 {
            return None;
        }
        let rest = &self.line.as_bytes()[self.next_nonspace.min(self.line.len())..];
        let interrupts_para = matches!(self.arena.nodes[container].value, NodeValue::Paragraph);
        let mut data = ListData {
            ordered: false,
            bullet_char: 0,
            start: 1,
            delimiter: 0,
            padding: 0,
            marker_offset: self.indent,
            tight: true,
        };

        let marker_len = match rest.first() {
            Some(&c @ (b'*' | b'+' | b'-')) => {
                data.bullet_char = c;
                1
            }
            Some(c) if c.is_ascii_digit() => {
                let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
                if digits > 9
                    || digits >= rest.len()
                    || !(rest[digits] == b'.' || rest[digits] == b')')
                {
                    return None;
                }
                let start: u32 = std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()?;
                if interrupts_para && start != 1 {
                    return None;
                }
                data.ordered = true;
                data.start = start;
                data.delimiter = rest[digits];
                digits + 1
            }
            _ => return None,
        };
        let next = rest.get(marker_len).copied();
        if !(next.is_none() || next == Some(b' ') || next == Some(b'\t')) {
            return None;
        }
        if interrupts_para && rest[marker_len..].iter().all(|&b| b == b' ' || b == b'\t') {
            return None; // an empty item cannot interrupt a paragraph
        }
        self.advance_next_nonspace();
        self.advance_offset(marker_len, true);
        let spaces_start_col = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            let nc = self.peek(self.offset);
            if !(self.column - spaces_start_col < 5 && is_space_or_tab(nc)) {
                break;
            }
        }
        let blank_item = self.peek(self.offset).is_none();
        let spaces_after = self.column - spaces_start_col;
        if !(1..5).contains(&spaces_after) || blank_item {
            data.padding = marker_len + 1;
            self.column = spaces_start_col;
            self.offset = spaces_start_offset;
            if is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
        } else {
            data.padding = marker_len + spaces_after;
        }
        Some(data)
    }

    // --- per line ---------------------------------------------------------------------

    fn incorporate_line(&mut self, ln: &'a str) {
        let mut container = self.doc;
        self.oldtip = self.tip;
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.line_number += 1;
        self.line = ln;

        let mut all_matched = true;
        while let Some(last) = self.arena.nodes[container].last_child {
            if !self.arena.nodes[last].open {
                break;
            }
            container = last;
            self.find_next_nonspace();
            match self.continue_block(container) {
                Cont::Matched => {}
                Cont::NotMatched => all_matched = false,
                Cont::LineConsumed => return,
            }
            if !all_matched {
                container = self.arena.nodes[container]
                    .parent
                    .expect("container has a parent");
                break;
            }
        }

        self.all_closed = container == self.oldtip;
        self.last_matched_container = container;

        let mut matched_leaf = !matches!(self.arena.nodes[container].value, NodeValue::Paragraph)
            && Self::accepts_lines(&self.arena.nodes[container].value);
        while !matched_leaf {
            self.find_next_nonspace();
            let c = self.peek(self.next_nonspace);
            let maybe_special = matches!(
                c,
                Some(b'#' | b'`' | b'~' | b'*' | b'+' | b'_' | b'=' | b'<' | b'>' | b'-')
            ) || c.is_some_and(|b| b.is_ascii_digit());
            if !self.indented && !maybe_special {
                self.advance_next_nonspace();
                break;
            }
            match self.try_starts(container) {
                1 => container = self.tip,
                2 => {
                    container = self.tip;
                    matched_leaf = true;
                }
                _ => {
                    self.advance_next_nonspace();
                    break;
                }
            }
        }

        // Lazy paragraph continuation?
        if !self.all_closed
            && !self.blank
            && matches!(self.arena.nodes[self.tip].value, NodeValue::Paragraph)
        {
            self.add_line();
            return;
        }
        self.close_unmatched_blocks();
        if self.blank
            && let Some(lc) = self.arena.nodes[container].last_child
        {
            self.arena.nodes[lc].last_line_blank = true;
        }
        let value = &self.arena.nodes[container].value;
        let last_line_blank = self.blank
            && !(matches!(value, NodeValue::BlockQuote)
                || matches!(value, NodeValue::CodeBlock { fenced: true, .. })
                || (matches!(value, NodeValue::Item(_))
                    && self.arena.nodes[container].first_child.is_none()
                    && self.arena.nodes[container].start_line == self.line_number));
        let mut cont = Some(container);
        while let Some(c) = cont {
            self.arena.nodes[c].last_line_blank = last_line_blank;
            cont = self.arena.nodes[c].parent;
        }
        if Self::accepts_lines(&self.arena.nodes[container].value) {
            self.add_line();
            if let NodeValue::HtmlBlock { kind, .. } = self.arena.nodes[container].value
                && (1..=5).contains(&kind)
                && scanners::html_block_end(
                    kind,
                    &self.line.as_bytes()[self.offset.min(self.line.len())..],
                )
            {
                self.finalize(container, self.line_number);
            }
        } else if self.offset < self.line.len() && !self.blank {
            self.add_child(NodeValue::Paragraph);
            self.advance_next_nonspace();
            self.add_line();
        }
    }
}
