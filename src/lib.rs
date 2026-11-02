//! marksman: a CommonMark 0.31.2 parser and HTML renderer with no dependencies.
//!
//! ```
//! assert_eq!(marksman::to_html("# Hi *there*\n"), "<h1>Hi <em>there</em></h1>\n");
//! ```

mod blocks;
mod entities;
mod html;
mod inlines;
mod node;
mod scanners;
mod unicode;
mod util;

pub use node::{ListData, NodeValue};

use std::borrow::Cow;

/// Render CommonMark to HTML.
pub fn to_html(markdown: &str) -> String {
    let doc = Document::parse(markdown);
    doc.to_html()
}

/// A parsed document. Walk it with [`Document::walk`] or render it with [`Document::to_html`].
pub struct Document {
    arena: node::Arena,
    root: node::NodeId,
}

/// A node visit during [`Document::walk`].
pub enum Event<'a> {
    Enter(&'a NodeValue),
    Exit(&'a NodeValue),
}

impl Document {
    /// Parse a document. Never fails: every input is valid CommonMark.
    pub fn parse(markdown: &str) -> Document {
        // NUL is replaced with U+FFFD, as the spec requires for security.
        let input: Cow<'_, str> = if markdown.contains('\0') {
            Cow::Owned(markdown.replace('\0', "\u{FFFD}"))
        } else {
            Cow::Borrowed(markdown)
        };
        let (arena, root, _refs) = blocks::BlockParser::new().parse(&input);
        Document { arena, root }
    }

    pub fn to_html(&self) -> String {
        html::render(&self.arena, self.root)
    }

    /// Depth-first traversal with enter/exit events.
    pub fn walk(&self, mut f: impl FnMut(Event<'_>)) {
        fn go(a: &node::Arena, n: node::NodeId, f: &mut impl FnMut(Event<'_>)) {
            f(Event::Enter(&a.nodes[n].value));
            let mut c = a.nodes[n].first_child;
            while let Some(k) = c {
                go(a, k, f);
                c = a.nodes[k].next;
            }
            f(Event::Exit(&a.nodes[n].value));
        }
        go(&self.arena, self.root, &mut f);
    }
}
