# marksman deep dive

### 1. Why two phases?

Block structure must be known before inline structure. `*a` on one line and `b*` on the next
are one paragraph, so the emphasis can span them, unless the second line is a list item. And a
line can only be judged once its containers are known: whether `> foo` continues a quote, or
whether four leading spaces mean code. Phase 1 builds the block tree line by line, without
looking at inline syntax. Phase 2 parses each paragraph's and heading's text as a whole. The
one inline construct phase 1 needs, link reference definitions, is extracted when a paragraph
closes, because definitions can be used before they appear.

### 2. Walk through one line of phase 1.

1. Set `container` to the document. While its last child is open, descend and try to continue
   that block. A block quote needs `>`, an item needs enough indentation, a paragraph needs a
   non-blank line, and fenced code checks for its closing fence. Stop at the first block that
   does not continue.
2. On what remains of the line, try the block starts in order: block quote, ATX heading,
   fence, HTML block, setext underline, thematic break, list item, indented code. Each start
   that opens a container repeats the step.
3. If unmatched blocks remain and the line is non-blank text while the tip is a paragraph,
   it is a **lazy continuation**: it joins the paragraph without closing anything. Otherwise
   close the unmatched blocks and add the rest of the line to the right leaf, or start a new
   paragraph.

### 3. How are tabs handled?

Tabs are not expanded in the text. They count as advancing to the next multiple of 4 columns,
but only when indentation matters. `advance_offset(count, columns)` can consume part of a tab
(`partially_consumed_tab`). When the remaining text is then added to a block, the unconsumed
columns of that tab are written out as spaces. This is how `-\t\tfoo` produces an indented code
block inside a list item with the right number of leading spaces (spec "Tabs" examples).

### 4. How does list tightness work?

A list is loose if any constituent item ends with a blank line before another item, or if any
item's direct children are separated by blank lines. Blank-line status propagates up the open
containers on every blank line (`last_line_blank`). Fenced code, block quotes, and the first
line of an empty item are excluded, because a blank there does not separate blocks.
`ends_with_blank_line` follows the last children down through nested lists and items. A tight
list renders its paragraphs without `<p>`.

### 5. What makes emphasis hard?

The rules depend on what surrounds a delimiter run:
- **Flanking:** left-flanking and right-flanking are defined using Unicode whitespace and
  Unicode punctuation (categories P and S in 0.31).
- **Underscore:** `_` has extra intraword restrictions.
- **Multiples of 3:** if one delimiter run can both open and close, a match is rejected when
  the two runs' total length is a multiple of 3 but neither is.

`process_emphasis` walks closers left to right, searches back for a compatible opener, and
wraps the nodes between them in `<em>` or `<strong>`, using 2 delimiters when both sides have at
least 2. Delimiters between the opener and closer are discarded. An `openers_bottom` array,
indexed by delimiter character, can-open, and length mod 3, records where a failed search
stopped. A later closer of the same class never rescans that region, which keeps the algorithm
linear (`nested strong/emph` and `mismatched openers` in the pathological test).

### 6. How do links interact with emphasis?

`[` and `![` are pushed on a bracket stack along with a pointer into the delimiter stack.
On `]`, the parser tries, in order:
- an inline link `(dest "title")`;
- a full reference `[label]`;
- a collapsed `[]` or shortcut reference.

On success, everything after the opener becomes the link's children, and emphasis is processed
*only inside the link* (down to the saved delimiter pointer). Links cannot contain links, so
earlier `[` openers are deactivated; image openers are not. This is why `*[foo*](url)` produces
a link containing `*foo*` as literal text: the emphasis cannot cross the link boundary.

### 7. How are reference labels matched?

Labels are compared after trimming, collapsing internal whitespace, and Unicode case folding
(implemented as lowercase-then-uppercase, which maps `ẞ` and `SS` together, as commonmark.js
does). The first definition of a label wins, so later duplicates are ignored. Labels may not
contain unescaped brackets or exceed 999 characters.

### 8. Which quadratic paths did the pathological tests find?

Four, each now O(1) per attempt:
1. **Text merging.** To decide whether it could merge into the previous text node,
   `append_text` scanned the whole delimiter and bracket stacks. Now it merges only into the
   node it created itself (`mergeable`). Nested emphasis went from 2.7 s at 20k to 21 ms.
2. **Unclosed link destinations** (`[a](b[a](b…`). Every `](` scanned to the end of the
   paragraph. A destination scan that reaches the end without seeing any `)` proves that every
   later scan fails too, because a later start can never land inside an escape pair (the byte
   before it is always `(`). `dest_fails_from` records it: 4.3 s at 80k became 20 ms.
3. **Unmatched backtick runs.** After one full scan that found no closer, a map from run length
   to its last start position answers "is there a closer of length n after here?" directly.
4. **Many reference definitions.** Draining each definition from the paragraph string shifted
   the rest every time. Definitions are now consumed by offset and sliced once: 121 ms at 80k
   became 8 ms.

A fifth, structural issue was deep nesting, which overflowed the stack in the recursive
renderer. Rendering and `walk` are now iterative.

### 9. Why are the generated tables correct?

Unicode punctuation for flanking is defined by general category (P and S since 0.31).
`scripts/gen_unicode.py` derives the ranges from Python's `unicodedata`, which is Unicode 15.0 on
3.12. The HTML5 entity names come from WHATWG's `entities.json`; only names ending in `;` are
kept, because CommonMark requires the semicolon. Both tables are sorted and binary-searched. The
spec examples that exercise them (`&HilbertSpace;`, `&ngE;`, punctuation like `€` adjacent to
`*`) all pass.

### 10. Why is pulldown-cmark faster?

It never materialises a tree. Its first pass builds a compact block tree of byte ranges into
the source; inline parsing then emits borrowed `&str` events that the HTML writer consumes
directly. marksman allocates a `String` per text node and clones literals during rendering.
Those are the costs a borrowed-slice representation would remove, and the roadmap item. The
arena tree is what allows `Document::walk` and later tree transformations.

### 11. What does the fuzzer check?

That any valid UTF-8 input renders without panicking (no out-of-range slicing, no
arithmetic overflow, no unwrap failure), and that rendering is deterministic. The corpus starts
from the 652 spec examples, so mutations explore the edges of every construct. 3 million runs
found nothing; the pathological tests cover the performance side, which fuzzing does not.
