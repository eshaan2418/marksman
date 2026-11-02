//! Arena-allocated document tree with doubly linked siblings, so inline processing can splice
//! nodes (emphasis, links) in O(1).

/// List metadata shared by list and item nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListData {
    pub ordered: bool,
    pub bullet_char: u8,
    pub start: u32,
    pub delimiter: u8,
    pub padding: usize,
    pub marker_offset: usize,
    pub tight: bool,
}

/// Node kinds: blocks first, then inlines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeValue {
    Document,
    BlockQuote,
    List(ListData),
    Item(ListData),
    Paragraph,
    Heading {
        level: u8,
    },
    ThematicBreak,
    CodeBlock {
        fenced: bool,
        fence_char: u8,
        fence_len: usize,
        fence_offset: usize,
        info: String,
        literal: String,
    },
    HtmlBlock {
        kind: u8,
        literal: String,
    },
    Text(String),
    SoftBreak,
    LineBreak,
    Code(String),
    HtmlInline(String),
    Emph,
    Strong,
    Link {
        dest: String,
        title: String,
    },
    Image {
        dest: String,
        title: String,
    },
}

pub(crate) type NodeId = usize;

#[derive(Debug)]
pub(crate) struct Node {
    pub value: NodeValue,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
    pub open: bool,
    pub last_line_blank: bool,
    pub start_line: usize,
    pub content: String, // raw text of leaf blocks during the block phase
}

#[derive(Debug, Default)]
pub(crate) struct Arena {
    pub nodes: Vec<Node>,
}

impl Arena {
    pub fn add(&mut self, value: NodeValue, start_line: usize) -> NodeId {
        self.nodes.push(Node {
            value,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
            open: true,
            last_line_blank: false,
            start_line,
            content: String::new(),
        });
        self.nodes.len() - 1
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.unlink(child);
        self.nodes[child].parent = Some(parent);
        match self.nodes[parent].last_child {
            Some(last) => {
                self.nodes[last].next = Some(child);
                self.nodes[child].prev = Some(last);
            }
            None => self.nodes[parent].first_child = Some(child),
        }
        self.nodes[parent].last_child = Some(child);
    }

    pub fn insert_after(&mut self, node: NodeId, sibling: NodeId) {
        self.unlink(sibling);
        let next = self.nodes[node].next;
        let parent = self.nodes[node].parent;
        self.nodes[sibling].next = next;
        self.nodes[sibling].prev = Some(node);
        self.nodes[sibling].parent = parent;
        self.nodes[node].next = Some(sibling);
        match next {
            Some(n) => self.nodes[n].prev = Some(sibling),
            None => {
                if let Some(p) = parent {
                    self.nodes[p].last_child = Some(sibling);
                }
            }
        }
    }

    pub fn unlink(&mut self, node: NodeId) {
        let (prev, next, parent) = (
            self.nodes[node].prev,
            self.nodes[node].next,
            self.nodes[node].parent,
        );
        match prev {
            Some(p) => self.nodes[p].next = next,
            None => {
                if let Some(par) = parent {
                    self.nodes[par].first_child = next;
                }
            }
        }
        match next {
            Some(n) => self.nodes[n].prev = prev,
            None => {
                if let Some(par) = parent {
                    self.nodes[par].last_child = prev;
                }
            }
        }
        let n = &mut self.nodes[node];
        n.prev = None;
        n.next = None;
        n.parent = None;
    }

    pub fn children(&self, node: NodeId) -> Children<'_> {
        Children {
            arena: self,
            next: self.nodes[node].first_child,
        }
    }
}

pub(crate) struct Children<'a> {
    arena: &'a Arena,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.arena.nodes[cur].next;
        Some(cur)
    }
}
