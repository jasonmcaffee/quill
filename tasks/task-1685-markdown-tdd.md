# task-1685 — the Markdown preview

The ticket says four things:

> Quill markdown preview has a few issues. It's not rendering md tables correctly. I can't select
> and copy text from the preview. Code blocks aren't easy to read, etc. Seems like a lot of
> formatting isn't complete or is missing.

Three of those are features that were never written, and the fourth — "a lot of formatting isn't
complete" — is the interesting one, because it is not a list of small omissions. The preview's
parser reads the source **one line at a time and decides what that line is in isolation**, and
inside a line it walks the characters with three booleans for bold, italic and struck. That shape
cannot express most of what Markdown is: a list item holding a paragraph, a quote holding a list, a
fence indented inside a bullet, or the question of whether the `*` in `2 * 3 * 4` opens emphasis.
Every missing feature named below is downstream of that one decision.

So this is a rewrite of `quill_core::markdown` into the two phases every conforming Markdown
implementation uses — a **block parser** that builds a tree, then an **inline parser** with a
delimiter stack — plus three features on top of it: tables, selection, and code that can be read.

---

## 1. What is wrong today, measured against CommonMark 0.31.2 and GFM

The module's own comment is honest about the omissions, which makes the audit short. Everything in
this table was checked against the code in `crates/quill-core/src/markdown.rs` as it stands.

| Markdown | What the preview does now | Why it matters |
|---|---|---|
| Pipe table | Shows the pipes as prose: `\| Name \| Size \|` | The ticket's first complaint |
| Task list `- [ ]` | Bullet, then the literal `[ ]` | Every checklist in the repo |
| Nested quote `>>` | One bar, then a literal `>` | Quoting a quote |
| Quote holding a list | The bar, then `- item` as prose | Very common |
| List item holding a paragraph | The continuation line is a new paragraph at the left margin | Any list with prose in it |
| Fence inside a list item | The fence marks are shown, the code is not | Any tutorial |
| Indented code (four spaces) | Ordinary prose | CommonMark's other code block |
| Setext heading (`===` under a line) | The line, then a horizontal rule | Common in older files |
| Reference link `[a][b]` + `[b]: url` | The brackets are shown and the definition is a paragraph | Long documents use them |
| Footnote `[^1]` | Shown as `[^1]` | Used in this repo's own notes |
| Autolink `<https://…>` / bare `https://…` | Prose | GFM |
| Backslash escape `\*not italic\*` | The backslash is shown and emphasis opens | Escaping is how you show a mark |
| Entity `&amp;` `&nbsp;` | Shown as written | HTML entities are valid Markdown |
| Inline HTML `<br>` `<sub>` | Shown as written | At least the tags should not be shown |
| `` ``code with a ` in it`` `` | The inner backtick ends the span | CommonMark's backtick rule |
| Link title `[a](url "t")` | The title is part of the address, so it is dropped — right by accident | — |
| `[a](<url with spaces>)` | Not read | — |
| Hard break (two trailing spaces) | Nothing; the lines are two paragraphs | Poetry, addresses |
| Lazy continuation | Every wrapped line is its own paragraph | Most hand-written Markdown |
| Rule `- - -`, `***` with spaces | Not a rule | CommonMark allows spaces |
| Heading `## Title ##` | The closing hashes are shown | CommonMark's closing sequence |
| Ordered list `3)` starting at three | Right | — |
| Tight and loose lists | Every item is one paragraph with a blank line between | Spacing looks wrong |
| Emphasis `2 * 3 * 4` | Italic from the first `*` to the second | The toggle has no flanking rule |
| Emphasis `snake_case_word` | Guarded by a special case for `_` only | The general rule covers `*` too |
| Front matter (`---` … `---`) | A horizontal rule, then the YAML as prose | Half the `.md` files anybody writes |

Nothing above is a bug in the sense of a mistake; each is a thing the line-at-a-time parser has no
way to express.

### The three that are not parsing at all

**Tables** need column widths, which need measurement.

**Selection** was never written: `show_markdown_preview` calls `editor_view::paint_text`, whose own
comment says "with no selection and no caret", and senses `egui::Sense::hover()`. There is no
pointer handling, so there is nothing to copy. The text is real text in a real `Layout` — it is
only that nobody ever asked it what byte was under the pointer.

**Code blocks** are one colour — `#7ED39B` for every character — at 95% of body size, on the same
background as the prose, with no border, no gutter and no indication of where they start and stop.
That is what "not easy to read" means, and the fix is the same as the editor's: colour it with the
grammar the plugin already supplies, and put it on a panel of its own.

---

## 2. Where the work goes, and why not a crate

`pulldown-cmark` is the obvious answer and is the wrong one here, for the reason
`tasks/quill-mermaid-plugin-tdd.md` §2 gives about `mermaid.js` and `task-1675` §2 gives about a
language server: it would answer a different question from the one Quill is asking.

A Markdown crate produces **events for HTML** — `Start(Tag::Table)`, `Start(Tag::TableCell)`,
`Html`, `SoftBreak`. Quill has no HTML, no box model and no inline layout: it has a rope, a list of
character spans, one paragraph style a line, and a layout engine that places glyphs left to right.
So the crate's output would have to be walked and re-expressed as those four things anyway, which is
the whole of the work; what the crate would save is the tokenising, which is the part with the tests
in it. It would also cost the thing that matters most about the current design — that the preview
**is** a document, so the ordinary layout, the ordinary painter, the ordinary scrollbar, the
ordinary hit testing and now the ordinary selection all work on it with no second code path.

Against that: writing it means owning the conformance. That is accepted, and §9 is how it is held
down — a battery of source-to-preview cases taken from the CommonMark specification's own examples,
run with no window.

**What is deliberately not implemented**, each with its reason, is in §10.

---

## 3. The block parser

`markdown/blocks.rs`. A tree, because Markdown is one — which is the only shape that reads a list
item holding a quote holding a fence.

```
Block
├── Document
├── Heading { level, setext }
├── Paragraph
├── Quote
├── List { ordered, start, tight }
├── Item { marker, task: Option<bool> }
├── Code { language, fenced }
├── Table { alignments }
├── Rule
├── FrontMatter
└── Diagram { source }          // a mermaid fence, kept whole
```

It is built **recursively rather than incrementally**: a quote's lines have one `>` taken off them
and are parsed again, and a list item's lines have its indent taken off them and are parsed again.
That is a different shape from CommonMark's reference implementation, which keeps a stack of open
containers and walks the document once — and it gives the same answers for everything short of the
pathological cases, at a fraction of the reading. A preview is worked out again on every keystroke,
so being easy to follow is worth more here than parsing a document nobody would write. Two rules
matter enough to name:

**Lazy continuation.** A plain line following a paragraph continues that paragraph even when the
quote or list prefixes are missing, which is what makes hand-wrapped prose come out as one
paragraph instead of five. It is the single most visible fix in the whole ticket, because nearly
every Markdown file anybody has hand-written is wrapped.

**Tight and loose.** A list is loose when any of its items is separated from the next by a blank
line, or when an item holds two blocks. A tight list draws its items one under the other; a loose
one gets a blank line between them. That is the whole difference, and it is why lists in the
preview look too airy today: every item is followed by a blank paragraph unconditionally.

### Front matter

A file whose very first line is `---` and which has a later `---` before any other content has
**front matter**: the block is kept, and drawn as a quiet, monospaced, dimmed panel at the top
rather than as a rule followed by a paragraph of YAML. It is not part of CommonMark, but it is part
of every Markdown file written for a static site, and rendering it as a horizontal rule and some
stray prose is the single worst-looking thing the current preview does.

---

## 4. The inline parser

`markdown/inline.rs`. CommonMark's delimiter-stack algorithm, which is what makes emphasis right
rather than nearly right.

1. Walk the text, emitting plain runs and pushing a **delimiter** for every run of `*`, `_` or `~`,
   recording its length and whether it **can open** and **can close**.
2. A delimiter run is *left-flanking* when it is not followed by whitespace and either not followed
   by punctuation or preceded by whitespace or punctuation; *right-flanking* is the mirror. `*` can
   open when left-flanking; `_` can open when left-flanking and either not right-flanking or
   preceded by punctuation. That last clause is what makes `snake_case` a word rather than emphasis
   — the general rule, in place of the special case the current code carries.
3. Then run CommonMark's *process emphasis*: for each closer, look back for the nearest opener of
   the same character that may pair with it, honouring the rule of three (if either delimiter can
   both open and close, their combined length must not be a multiple of three unless both are).
4. Anything left on the stack when the text runs out is literal text. That is what makes
   `2 * 3 * 4` come out as `2 * 3 * 4` — neither `*` is left-flanking, so neither can open.

Everything that is not a delimiter is handled by a scanner in front of the stack, in this order,
because the earlier ones win: **backslash escape**, **entity**, **code span**, **autolink**,
**raw HTML tag**, **image**, **link** (inline, then reference, then footnote), then the delimiters.

**Code spans** follow the backtick rule properly: a run of *n* backticks is closed by the next run
of exactly *n*, and one leading and one trailing space are stripped when the content has a
non-space in it. That is what makes `` `` `code` `` `` work.

**Entities** — `&amp;`, `&#39;`, `&#x2014;` and the two hundred or so named ones anybody uses — are
decoded to their character. The table is small and static; the full HTML5 list of 2,231 is not worth
the two hundred kilobytes.

**Raw HTML** is *removed* rather than shown. Quill cannot render it, and a reader seeing `<sub>`
in the middle of a sentence is worse off than a reader seeing neither. Four tags are given a meaning
because they are the ones that appear in prose and have an obvious equivalent in styled text:
`<br>` is a line break, `<b>`/`<strong>` is bold, `<i>`/`<em>` is italic, `<code>` is code. A block
of HTML on lines of its own is dropped whole.

**Reference links and footnotes** need a pass before the inline pass, since a definition may come
after its use. The block parser collects `[label]: destination "title"` lines and footnote
definitions into a map, and the inline parser looks up as it goes. A reference with no definition is
left as the literal text it was written as, which is what CommonMark says and is also what a reader
wants: it makes the broken link visible.

---

## 5. Tables

### The shape of the answer

A table needs columns of equal width, and Quill's layout engine places one glyph after another with
no notion of a column. Three ways to give it one were weighed.

**Teach the layout engine tab stops.** A paragraph would carry a list of column positions and an
alignment for each, and a `\t` would advance to the next. It is the prettiest — proportional text,
real rules drawn as rectangles — and it costs the most: `ParagraphStyle` is `Copy` and twelve bytes,
one per line of every document, so a variable-length list in it is either a heap allocation per line
or a fixed array that inflates a hundred-thousand-line file by eleven megabytes. And a cell whose
text is longer than its column has to wrap, which makes one visual line hold pieces of several
cells — while `PlacedLine::bytes` is a single `Range`, on which the hit testing, the selection
rectangles and the incremental relayout fingerprint all depend. That is a rewrite of the layout
engine for one feature.

**Draw the table as a picture**, the way a Mermaid diagram is drawn: an empty paragraph and a
`Scene`. This is cheap and it is wrong, because the ticket's second complaint is that text in the
preview cannot be selected, and a picture of a table is a table nobody can copy out of.

**Set the table in the monospaced font and draw its rules with box-drawing characters.** Chosen.
Every cell is padded with spaces to its column's width, so the columns line up exactly and by
construction rather than by measurement; the rules are `─ │ ┌ ┬ ┐ ├ ┼ ┤ └ ┴ ┘`, which every
monospaced font on both platforms has; the whole table is ordinary text in the ordinary layout, so
it selects, copies, scrolls and hit-tests with no new code at all; and what lands on the clipboard
is a table a person can paste anywhere.

It is also what the field does. `glamour`, the renderer behind `glow`, draws exactly this, and so do
`rich`, `mdcat` and `bat`. The precedent is inside Quill too: the horizontal rule has been drawn as
forty-eight `─` since the preview was written, for the same reason — the layout engine places
glyphs, so a line that is not text is a line made of text.

The cost is stated rather than hidden: **a table is set in the code font, not the prose font.** It
is tabular data next to a monospaced code block, and it reads as deliberate rather than as a
mistake; but it is not what a browser does, and if a later version teaches the layout engine about
columns, this is the thing it should replace.

### How wide

Character counts, not points — which is the other reason the monospaced answer is a good one: the
arithmetic is integers and every one of its tests runs with no fonts.

1. Parse the rows. Cells are split on unescaped `|`; a row with fewer cells than the header is
   padded and one with more is truncated, which is GFM's rule. The delimiter row's colons give each
   column its alignment.
2. Each cell's inline content is rendered to styled text, and its **width in characters** is the
   count of grapheme clusters in it.
3. The natural width of a column is the widest cell in it; the natural width of the table is the sum
   plus the rules.
4. `available` is how many characters of the code font fit across the pane —
   `metrics.advance("M", &code_style)` divided into the width, which is the one measurement the
   whole feature takes.
5. If the table is wider than that, columns are shrunk **widest first** until it fits, with a floor
   of five characters, and a cell too long for its column is **wrapped** at word boundaries into as
   many lines as it needs. A row is then as many preview lines as its tallest cell, each of them one
   contiguous run of text — which is the property that keeps hit testing and selection working.
6. If a table still cannot fit — more columns than there is room for five characters each — it is
   drawn at its natural width and the pane's own wrapping takes over. That is ugly and it is
   honest; the alternative is hiding columns.

Alignment inside a cell is padding: left is spaces after, right is spaces before, centre splits the
difference with the odd space on the right, matching every other renderer.

### What it looks like

```
┌──────────┬───────┬──────────┐
│ Crate    │ Lines │    Tests │
├──────────┼───────┼──────────┤
│ core     │ 9,132 │      412 │
│ terminal │ 3,004 │       88 │
└──────────┴───────┴──────────┘
```

The header row is bold. The rules are in the quiet colour, the text in the ordinary one, so the grid
recedes and the data comes forward. That is what is written into the preview's text and what lands on
the clipboard.

### And the rules are painted, not lettered

What is *drawn* is not the glyphs. **A box-drawing glyph cannot tile**, and the first version of this
proved it: a rule came out as a dotted line and a table's column came out as a column of ticks. Three
reasons, all of them structural rather than a fault in any one font.

Its bitmap is a whole pixel wider than its advance — the ink spans the cell and `px_bounds().ceil()`
rounds up — so the last column of every glyph is only partly covered, and every second or third cell
lands a light pixel. Its ink is an em box while the line it sits on is taller than that, because a
line carries the font's leading, so nothing vertical can reach the next row. And the painter snaps
every glyph to a whole pixel, deliberately, so that letters are drawn at exactly the size they were
rasterised at — which is right for a letter and is the thing that makes a fractional advance visible
in a line.

So `components::editor_view::box_rules` says, for each of the **eleven** characters the preview
writes, which part of its cell each of its two strokes covers: `(0.0, 1.0)` right across, `(0.5, 1.0)`
from the middle to the right or bottom edge. That is the whole of what a corner is. The painter fills
those rectangles at whole-pixel edges, and because the bottom of one line is the top of the next by
construction, a bar down a quote and a column of a table are continuous however tall the lines are.
A run of `─` becomes one rectangle rather than forty-eight.

This is `design/style-guide.md`'s own rule about icons — *drawn rather than lettered* — reaching one
more place, and it fixes the horizontal rule `---` has always drawn as well. Only those eleven: a
character with a curve, a double line or a deliberate dash in it is not two rectangles, and is left
to the font.

---

## 6. Code that can be read

Three changes, each small, and together they are the ticket's third complaint.

**A panel behind the block.** `Preview` gains `panels: Vec<PreviewPanel>` — a paragraph range and
what kind of panel it is. The window paints a rounded rectangle behind those paragraphs before it
paints the text, exactly as it already paints a highlight's colour behind a passage, and exactly
where the pictures and the diagrams get their room. `quill-core` still knows nothing about drawing:
it says which paragraphs are code and the window decides what a code background looks like.

**Colour from the grammar.** The fence's language is looked up in the plugins Quill already has, and
the code inside is coloured with `quill_core::syntax::highlight` and the plugin's own theme — the
same two calls `colour_the_file` makes for a source file, so a fence of Rust in a document is
coloured exactly as a `.rs` file is. The seam is a trait, `CodeHighlighter`, with one method:
`quill-core` holds no plugin registry and must not learn about one, and the window implements the
trait over `Plugins::grammars`. A language nothing claims falls back to today's single colour, which
is why the change can never make anything worse.

`Plugins` gains `for_language`, which matches a fence's word against a plugin's id, its name and
every extension it claims, so ```` ```rs ```` and ```` ```rust ```` are the same request. It is on
`Plugins` rather than on `Grammars` because colouring needs the theme as well as the grammar, and
`Grammars` deliberately carries only the grammars — it is cloned onto a worker thread.

**A chip behind inline code.** `` `like this` `` gets the same treatment at the span level: the
ranges are recorded and the window paints a small rounded rectangle behind each, which is what makes
inline code read as a thing rather than as green prose.

---

## 7. Selecting and copying in the preview

The preview is a `Layout` over a rope. `Layout::offset_at` turns a point into a byte, and
`Layout::selection_rects_in` gives the rectangles for a range — both used by the editing area every
frame. So what is missing is only the state and the input, and both belong on the tab, beside the
scroll position they live with:

```rust
/// What is selected in this tab's Markdown preview, as a range into the preview's own text.
pub preview_selection: Selection,
```

One field, because `quill_core::Selection` is already an anchor and a head and already knows how to
be dragged either way round — the editing area's own selection is one of these, so a preview's is the
same thing rather than a second spelling of it.

Rules:

- The area senses `click_and_drag` instead of `hover`. A press puts the anchor down and clears the
  selection; a drag extends it; a click with nothing dragged clears it.
- Double click selects the word under the pointer, triple click the line. The anchor goes down where
  the **press** was rather than where the pointer is when egui first calls it a drag: egui does not
  call a press a drag until it has moved a few points, so by that frame the pointer has left the
  letter it was put down on. `input.pointer.press_origin()` is the honest answer, and a test that
  drags in one jump is what found it.
- `Ctrl/Cmd+C` copies the selected text. `Ctrl/Cmd+A` selects the whole preview.
- **The preview never takes the keyboard from the editing area.** In the side-by-side view the
  source is being typed into and the preview is being read, and a click in the preview must not stop
  the caret working. So `Focus` is left alone and one flag on the window, `reading_preview`, says which of the two a copy
  is about: set by a press in a preview, cleared by a press in an editing area. That is what "the
  pane the pointer last pressed in" means, and it is one boolean rather than a fourth `Focus`.
- The selection is thrown away when the preview is worked out again, because a byte range into text
  that has been rebuilt means nothing. It survives a scroll, a resize that does not change the
  wrapping, and switching tabs and back.
- The selection is drawn behind the text and **over** the panels, so a selection inside a code block
  is visible; the panels and the inline code chips go down first, then the selection, then the
  glyphs, then the pictures and the diagrams.
- `Edit → Copy` works when the preview holds the selection, because it goes through `run_action`,
  which is the one place a copy happens. `Ctrl/Cmd+C` does not: egui delivers it as an `Event::Copy`
  rather than as a key press, which is why `Copy` is marked in `actions::menus` as not coming from
  the keyboard. So the event is claimed **before the pane loop**, the ordering the completion popup's
  five keys and the explorer's keys already use — the source pane is drawn first and would otherwise
  take it and copy its own selection.

`quill-cli` gets **one** command rather than two — `editor preview-select`, with `--from`, `--to`,
`--all`, `--none` and `--copy` — because "select this" and "copy what is selected" are one question
asked of a page that cannot be edited, and a second verb would have been a second thing to document.
It goes through the same three functions the pointer goes through, so a selection made from the
command line and one made with the mouse are the same thing. `editor preview --json` reports the
panels and the inline code beside the text, so what a person can see is what a script can read.

---

## 8. What changes where

| File | Change |
|---|---|
| `quill-core/src/markdown/mod.rs` | The public shape: `render`, `Options`, `Preview`, `PreviewPanel`, `PreviewTable`, `CodeHighlighter` |
| `quill-core/src/markdown/blocks.rs` | The block parser and its stack |
| `quill-core/src/markdown/inline.rs` | The delimiter stack, code spans, links, autolinks, entities |
| `quill-core/src/markdown/entity.rs` | Named and numeric character references |
| `quill-core/src/markdown/table.rs` | Pipe tables, column fitting, box drawing |
| `quill-core/src/markdown/build.rs` | Blocks to text, spans and paragraph styles |
| `quill-app/src/app/mod.rs` | `refresh_preview` passes metrics and a highlighter; panels and selection are painted; the preview's pointer is read |
| `quill-app/src/app/files.rs` | `preview_selection` on the tab |
| `quill-app/src/services/plugins.rs` | `Plugins::for_language` |
| `quill-app/src/components/editor_view.rs` | Reading the pointer in a page that cannot be typed into, and painting the box-drawing characters rather than lettering them |
| `quill-app/src/app/cli.rs`, `quill-cli` | The two new commands and their documentation |

`Preview` keeps `text`, `chars`, `paragraphs`, `source_lines`, `images` and `diagrams` exactly as
they are, so `scroll_sync`, the pictures, the diagrams and the side-by-side scrolling are untouched.
`source_lines` is extended to the new blocks and its invariant — never going backwards, one entry a
line — is what the existing test already checks.

---

## 9. How it is held down

**A battery in `quill-core`, with no window.** One test per row of the table in §1, asserting on the
preview's text and on the style at a named position — never on numbers a font decides. Plus the
sixty-odd examples from the CommonMark specification that are expressible in styled text, kept as a
source-and-expectation table so a reader can see what is claimed.

**The four properties every preview is held to**, the shape `mermaid::check::properties` already
has, asserted for every case in the battery:

1. `chars.total_len() == text.len_bytes()` — the spans cover the text exactly.
2. `paragraphs.len() == text.len_lines()` — one style a line.
3. `source_lines.len() == text.len_lines()`, and never decreasing.
4. Every paragraph named by an image, a diagram or a panel is inside the text.

**Screenshot tests** for a document holding one of everything, in the preview and side by side, plus
one for a table, one for a coloured fence and one for a selection. Looked at before they are
accepted, as always.

**Every Markdown file in the checkout.** `cargo run --example markdown_check` reads all of them —
fifty-five files and a megabyte at the time of writing — prints what each came to, and checks the four
properties on every one. It is the counterpart of `mermaid_check`, and it is the widest test there is,
because these files were written by hand over months and hold every shape of Markdown anybody here
actually writes. Measured: 116 ms for the lot, nothing broken.

**The real window**: `sample/welcome.md` grows a table, a task list and a coloured fence, so what a
person sees on first opening Quill is what the ticket asked for.

---

## 10. Deliberately not done

- **Nested tables, and block content in a table cell.** GFM forbids both.
- **Inline HTML rendered as HTML.** Quill has no HTML engine and will not grow one; §4 says what
  happens instead.
- **Definition lists, abbreviations, and the rest of the PHP-Markdown-Extra family.** Not in
  CommonMark, not in GFM, and not in anything in this repository.
- **Math.** `$…$` needs a typesetter, which is a whole feature of its own and a different ticket.
- **Following a link.** The preview colours a link and hides its address; opening one is `task-1659`
  territory and is not asked for here.
- **Editing in the preview.** It is worked out from the source, so there is nothing to type into.
  Selection is reading, not writing.
- **Tables in the prose font, with drawn rules.** §5 says what that would cost and what it would buy.
  If the layout engine ever grows columns, that is the change to make.
