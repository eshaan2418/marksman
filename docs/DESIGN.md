# marksman design

## Data structures

- **Arena** (`node.rs`): `Vec<Node>` indexed by `NodeId`. Each node has parent, first and last
  child, and prev and next sibling links, plus `open`, `last_line_blank`, its start line, and a
  raw `content` buffer used by leaf blocks during phase 1. `append_child`, `insert_after`, and
  `unlink` are O(1).
- **NodeValue**: blocks (document, block quote, list and item with `ListData`, paragraph,
  heading, thematic break, code block, HTML block) and inlines (text, soft and hard break, code,
  raw HTML, emphasis, strong, link, image).

## Phase 1: blocks (`blocks.rs`)

The per-line state is `offset` (bytes), `column` (tab-expanded), `next_nonspace` (and its
column), `indent`, `blank`, and `partially_consumed_tab`. A cache of the line's first non-space
position keeps the per-container indentation checks O(1), which keeps deep lists linear. Each
line:

1. **Continue** open containers (`continue_block`). The result is matched, not matched, or line
   consumed (a closing code fence).
2. **Start** new blocks (`try_starts`), in the spec's precedence order. A container start
   loops; a leaf start ends the loop.
3. **Lazy continuation**, or **close** unmatched blocks and **add** the line to the tip.

`finalize` closes a block:
- **paragraphs** extract leading link reference definitions, and are removed if nothing else
  remains;
- **fenced code** splits off the info string (unescaped);
- **indented code** drops trailing blank lines;
- **HTML blocks** drop trailing blank lines;
- **lists** compute tightness from `last_line_blank`.

HTML blocks use seven start conditions and five end conditions (`scanners.rs`). Kinds 6 and 7
end at a blank line. Kind 7, a complete arbitrary tag alone on its line, cannot interrupt a
paragraph.

## Phase 2: inlines (`inlines.rs`)

Each paragraph and heading's content is trimmed and scanned once. The handlers are:

| byte | handler |
|---|---|
| `\n` | soft break, or a hard break after two trailing spaces |
| `\` | escape, or a hard break before a newline |
| `` ` `` | code span: a closing run of the same length; one surrounding space stripped |
| `*` `_` | delimiter run, pushed on the delimiter stack with can-open and can-close |
| `[` `![` | bracket opener, pushed on the bracket stack |
| `]` | link or image resolution (inline, then full, collapsed, or shortcut reference) |
| `<` | autolink (URI or email), else raw HTML |
| `&` | entity or numeric reference |
| other | text up to the next special byte |

After the scan, `process_emphasis(None)` resolves the remaining delimiters. Link resolution
calls `process_emphasis` on the link's own delimiters first. Two memos keep adversarial input
linear: `dest_fails_from` for link destinations and `ticks_memo` for code spans.

## Rendering (`html.rs`)

An explicit-stack traversal with enter and exit hooks. It reproduces commonmark.js's output
rules: `cr()` emits a newline only if the output does not already end with one; tight lists omit
`<p>`; image `alt` text renders only the text of its descendants (`disable_tags`); and link
destinations are percent-encoded at parse time and HTML-escaped at render time, exactly once
each.
