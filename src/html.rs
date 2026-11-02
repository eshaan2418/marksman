//! HTML renderer matching the reference implementations' output byte for byte.

use crate::node::{Arena, NodeId, NodeValue};
use crate::util::escape_html;

struct Renderer<'a> {
    arena: &'a Arena,
    out: String,
    disable_tags: usize, // > 0 inside image alt text: emit text only
}

impl Renderer<'_> {
    fn lit(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn cr(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn tag(&mut self, s: &str) {
        if self.disable_tags == 0 {
            self.out.push_str(s);
        }
    }

    fn text(&mut self, s: &str) {
        escape_html(&mut self.out, s);
    }

    fn in_tight_list(&self, para: NodeId) -> bool {
        let grand = self.arena.nodes[para]
            .parent
            .and_then(|p| self.arena.nodes[p].parent);
        matches!(grand.map(|g| &self.arena.nodes[g].value), Some(NodeValue::List(d)) if d.tight)
    }

    /// Opening part of a node (leaf nodes are rendered completely here).
    fn enter(&mut self, n: NodeId) {
        match &self.arena.nodes[n].value {
            NodeValue::Document => {}
            NodeValue::BlockQuote => {
                self.cr();
                self.tag("<blockquote>");
                self.cr();
            }
            NodeValue::List(d) => {
                let d = *d;
                self.cr();
                if d.ordered {
                    if d.start == 1 {
                        self.tag("<ol>");
                    } else {
                        self.tag(&format!("<ol start=\"{}\">", d.start));
                    }
                } else {
                    self.tag("<ul>");
                }
                self.cr();
            }
            NodeValue::Item(_) => self.tag("<li>"),
            NodeValue::Paragraph => {
                if !self.in_tight_list(n) {
                    self.cr();
                    self.tag("<p>");
                }
            }
            NodeValue::Heading { level } => {
                let level = *level;
                self.cr();
                self.tag(&format!("<h{level}>"));
            }
            NodeValue::ThematicBreak => {
                self.cr();
                self.tag("<hr />");
                self.cr();
            }
            NodeValue::CodeBlock { info, literal, .. } => {
                self.cr();
                self.tag("<pre>");
                let lang = info.split([' ', '\t', '\n']).next().unwrap_or("");
                if lang.is_empty() {
                    self.tag("<code>");
                } else {
                    let mut t = String::from("<code class=\"language-");
                    escape_html(&mut t, lang);
                    t.push_str("\">");
                    self.tag(&t);
                }
                let l = literal.clone();
                self.text(&l);
                self.tag("</code></pre>");
                self.cr();
            }
            NodeValue::HtmlBlock { literal, .. } => {
                let l = literal.clone();
                self.cr();
                self.lit(&l);
                self.cr();
            }
            NodeValue::Text(t) => {
                let t = t.clone();
                self.text(&t);
            }
            NodeValue::SoftBreak => self.lit("\n"),
            NodeValue::LineBreak => {
                self.tag("<br />");
                self.cr();
            }
            NodeValue::Code(c) => {
                let c = c.clone();
                self.tag("<code>");
                self.text(&c);
                self.tag("</code>");
            }
            NodeValue::HtmlInline(h) => {
                let h = h.clone();
                self.lit(&h);
            }
            NodeValue::Emph => self.tag("<em>"),
            NodeValue::Strong => self.tag("<strong>"),
            NodeValue::Link { dest, title } => {
                let mut t = String::from("<a href=\"");
                escape_html(&mut t, dest);
                t.push('"');
                if !title.is_empty() {
                    t.push_str(" title=\"");
                    escape_html(&mut t, title);
                    t.push('"');
                }
                t.push('>');
                self.tag(&t);
            }
            NodeValue::Image { dest, .. } => {
                let dest = dest.clone();
                if self.disable_tags == 0 {
                    self.lit("<img src=\"");
                    self.text(&dest);
                    self.lit("\" alt=\"");
                }
                self.disable_tags += 1;
            }
        }
    }

    fn exit(&mut self, n: NodeId) {
        match &self.arena.nodes[n].value {
            NodeValue::BlockQuote => {
                self.cr();
                self.tag("</blockquote>");
                self.cr();
            }
            NodeValue::List(d) => {
                let ordered = d.ordered;
                self.cr();
                self.tag(if ordered { "</ol>" } else { "</ul>" });
                self.cr();
            }
            NodeValue::Item(_) => {
                self.tag("</li>");
                self.cr();
            }
            NodeValue::Paragraph => {
                if !self.in_tight_list(n) {
                    self.tag("</p>");
                    self.cr();
                }
            }
            NodeValue::Heading { level } => {
                let level = *level;
                self.tag(&format!("</h{level}>"));
                self.cr();
            }
            NodeValue::Emph => self.tag("</em>"),
            NodeValue::Strong => self.tag("</strong>"),
            NodeValue::Link { .. } => self.tag("</a>"),
            NodeValue::Image { title, .. } => {
                let title = title.clone();
                self.disable_tags -= 1;
                if self.disable_tags == 0 {
                    if !title.is_empty() {
                        self.lit("\" title=\"");
                        self.text(&title);
                    }
                    self.lit("\" />");
                }
            }
            _ => {}
        }
    }

    /// Iterative depth-first traversal, so deeply nested input cannot overflow the stack.
    fn run(&mut self, root: NodeId) {
        let mut stack: Vec<(NodeId, bool)> = vec![(root, true)];
        while let Some((n, entering)) = stack.pop() {
            if !entering {
                self.exit(n);
                continue;
            }
            self.enter(n);
            stack.push((n, false));
            let mut c = self.arena.nodes[n].last_child;
            while let Some(k) = c {
                stack.push((k, true));
                c = self.arena.nodes[k].prev;
            }
        }
    }
}

pub(crate) fn render(arena: &Arena, root: NodeId) -> String {
    let mut r = Renderer {
        arena,
        out: String::new(),
        disable_tags: 0,
    };
    r.run(root);
    r.out
}
