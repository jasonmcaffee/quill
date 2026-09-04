# task-1694 — an HTML plugin, and the markup a tokeniser could not read

> create a new plugin for html in unluminate.
>
> look at our other plugins, search the web to research more, and then create a tdd, then fully implement.

`task-1671` asked the same question about CSS and its answer was three small, data-driven additions
to `unluminate_core::syntax` with the manifest written against them. This is that ticket again with a
harder subject, and the shape of the answer is the same: HTML needs something the tokeniser has
never had, the thing it needs is a rule rather than a list, and it is a key in the manifest that is
off unless a language asks for it.

The difference is which rule. CSS needed the tokeniser to change its mind about what a *word* is.
HTML needs it to change its mind about whether a word means anything **at all**, and where.

---

## 1. Why the half hour of typing would have produced an unusable plugin

A Unluminate plugin is data: a `plugin.conf` naming a language's extensions, its comments, its strings,
some lists of words and a colour a token. Five of those manifests exist. Writing a sixth for HTML by
filling in the same keys takes half an hour, and this is what it produces:

```html
<p>The body of the table is important, and a small form of it is a
   good option for the header — see the summary in section 4.</p>
```

Fourteen words of ordinary prose. **Nine of them are HTML element names**: `body`, `table`, `small`,
`form`, `option`, `header`, `summary`, `section`, `p`. With the element names in
`language.keywords`, every one of them is drawn in Dracula's pink, and a paragraph of English reads
like a stylesheet having a seizure.

This is not a limitation to write into `plugin.limitations` beside "a regular expression literal is
coloured as division". It is the plugin being wrong about the majority of the bytes in the majority
of the files. Here is the whole list of element names that are also ordinary English words, taken
from MDN's element reference:

> `a`, `abbr`, `address`, `area`, `article`, `aside`, `audio`, `b`, `base`, `big`, `body`, `button`,
> `canvas`, `caption`, `center`, `cite`, `code`, `col`, `data`, `details`, `dialog`, `dir`, `embed`,
> `fieldset`, `figure`, `font`, `footer`, `form`, `frame`, `head`, `header`, `i`, `image`, `input`,
> `label`, `legend`, `link`, `main`, `map`, `mark`, `menu`, `meta`, `meter`, `nav`, `object`,
> `option`, `output`, `p`, `param`, `picture`, `pre`, `progress`, `q`, `s`, `samp`, `script`,
> `search`, `section`, `select`, `slot`, `small`, `source`, `span`, `strike`, `strong`, `style`,
> `sub`, `summary`, `sup`, `table`, `template`, `time`, `title`, `track`, `u`, `var`, `video`

Seventy-six of them. There is no clever ordering of the word lists that rescues this, because the
CSS rule — *a word that is both a property and a value is coloured whichever way it is written more
often* — assumes both readings are inside the language. Here one of the readings is **not in the
language at all**: it is English, and there is more of it in an HTML file than there is markup.

So the question a plugin for HTML actually asks is not "which words" but "**where does the language
apply**", and that is a question no manifest key could answer, because the tokeniser had no idea
that a language might apply in one part of a file and not another.

### The four other shapes that fall through

Once that is said, four smaller things fall out of the same fact, and they are worth naming because
each one is a rule and not a list:

| The HTML | What the tokeniser did with it |
|---|---|
| `<p>It's fine</p>` | the `'` opens a string, and the rest of the line is yellow |
| `<p>5 &lt; 3 is true, and 5 < 3 is too</p>` | `<` is an operator in prose, and `&lt;` is an ampersand and a word |
| `<script>if (a < b) {}</script>` | the `<` reads as markup, inside a language that is not markup |
| `<div class="a">` and `class` in prose | the same word, and only one of the two is an attribute |

The apostrophe is the one that decides the design on its own. `language.strings = ", '` is
unavoidable — HTML attribute values are written with both quotes — and prose is full of apostrophes.
Any answer that reads a string wherever it finds a quote makes every contraction in the document
yellow to the end of its line.

---

## 2. What the other editors do, and which tier this is

Every serious highlighter of HTML is a **state machine**, and the reason is exactly the paragraph
above: a token's meaning depends on whether the reader is in text, in a tag, in an attribute value,
in a comment, or inside a raw-text element.

| | How it reads HTML | What it costs |
|---|---|---|
| **TextMate / VS Code / Sublime** | a `.tmLanguage` grammar: nested `begin`/`end` rules with a pushdown stack of scopes, so `meta.tag` is a state and `entity.name.tag` only exists inside it. `<script>` and `<style>` bodies `include` `source.js` and `source.css` — a second grammar, embedded. | a regular-expression engine, a grammar format, and a stack |
| **tree-sitter** | a real incremental parser producing a syntax tree; `tree-sitter-html` has an external scanner written in C purely to handle raw-text elements, because they cannot be expressed in its grammar | a parser generator, a compiled grammar per language, a C scanner |
| **The Language Server Protocol** | `vscode-html-languageservice` — a full HTML parser behind a process | a program per language, found on the machine |
| **`highlight.js` / `Pygments`** | a hand-written state machine over regular expressions, with sub-languages for script and style | a regular-expression engine |

Every one of them is the same shape underneath: **a small number of states, and a rule for what a
character means in each.** The differences are in the machinery used to express the states, not in
the states themselves. HTML has few enough of them to write out by hand — `task-1675` §2 already
refused a language server and `task-1680` and `task-1686` refused one again, and the argument holds
here without modification: a colour that is silently absent because no language server happened to
be installed looks like a fault rather than a missing feature.

So this is the **syntactic tier**, for the fourth time, and the state machine is written in Rust in
`unluminate_core::syntax` where it can be tested with no window. What follows is the design of the states
and of the one manifest key that turns them on.

The **scope names** the survey settles are worth keeping, because they decide the colours in §5.
Sublime's own scope-naming document — which is the descendant of TextMate's and is what VS Code's
grammars are written against — names three things: a tag name is `entity.name.tag`, an attribute
name is `entity.other.attribute-name`, and the whole tag including its punctuation is `meta.tag`.
A character reference is `constant.character.entity`, which sits under `constant` beside
`constant.numeric`.

---

## 3. `language.markup` — the key, and the states behind it

```
language.markup = true
```

One key, off unless a manifest asks for it, which is the rule every key added since `task-1671` has
followed and which `the_older_plugins_ask_for_none_of_what_the_markup_added` keeps. With it off,
`syntax::scan` is byte for byte the function it was, and no plugin that shipped before this changes
by a pixel.

With it on, `scan` runs five states. They are the states, and everything else in this document is a
consequence of them:

```
                  ┌──────────────────────────────────────────────┐
                  │                                              │
   ┌──────────┐   │ `<` + letter, `/`, `!` or `?`   ┌──────────┐ │  `>`
   │   Text   │───┴───────────────────────────────▶ │ Tag name │─┴──────▶
   │          │                                     └────┬─────┘
   │ prose,   │                                          │ space
   │ entities │                                     ┌────▼─────┐
   └──────────┘                                     │Attribute │◀────┐
        ▲                                           └────┬─────┘     │
        │                                                │ `=`       │
        │                                           ┌────▼─────┐     │
        │  `</name`                                 │  Value   │─────┘
   ┌────┴─────┐                                     └──────────┘
   │ Raw text │◀── a start tag the language calls raw text
   └──────────┘
```

### 3.1 Text — and `<` is only sometimes a tag

Outside a tag everything is `Token::Text`. Not "everything not in a word list": **everything**. The
operators are not drawn, the strings are not read, the numbers are not read. A quote is a quote, an
apostrophe is an apostrophe, `2026` is prose and `5 < 3` is arithmetic.

`<` opens a tag **only when the next character is an ASCII letter, `/`, `!` or `?`**. That is the
HTML Standard's own tag-open state, taken verbatim: a less-than sign begins a tag when followed by
a tag name (which starts with an ASCII letter), a solidus for an end tag, an exclamation mark for a
comment or a doctype, or a question mark for a processing instruction. Anything else — a space, a
digit, another `<` — and the browser treats it as literal text, so Unluminate does too. This is why
`5 < 3` stays prose while `<p>` does not, and it costs one comparison.

The one thing that **is** read in text is a **character reference**: `&amp;`, `&#8212;`,
`&#x1F600;`. Named, decimal and hexadecimal, each ending at a `;`, which is the spec's own three
forms. It is `Token::Number`, for the reason §5 gives.

### 3.2 Tag name

The word directly after the `<`, past a `/` for an end tag or a `!` for a declaration. Its position
is certain — there is exactly one tag name per tag and it is the first word in it — so this is the
one place in HTML where Unluminate knows what a word is without having to be told.

- A name the language names is `Token::Keyword`.
- **A name the language does not name is `Token::Type`**, not plain text.

That second rule is a departure from what the CSS manifest says about a class selector ("a name its
author chose is plain text"), and the reason it is a departure is that the CSS case has no
positional certainty and this one does. Unluminate's honesty rule is to show what is known and not guess
at what is not; here it is *known* that the word is a tag name, and only unknown whether the
language defines it. `<my-widget>`, `<sl-button>`, `<MyPanel>` and `<Foo>` are custom elements and
framework components, which are a large fraction of the HTML anybody writes now, and drawing them
grey would be throwing away something the reader is certain of.

### 3.3 Attribute

Every other word inside the tag.

- A name the language names is `Token::Builtin` — `class`, `href`, `aria-label`.
- A name in `language.types` is `Token::Type`, so the third list CSS added is available to a markup
  language that wants a third class of attribute. HTML names none.
- **Anything else is plain text**, and here that *is* the CSS rule, deliberately. There is one tag
  name in a tag and there may be a dozen attributes, and the ones that are not in the list —
  `data-track-id`, `hx-get`, `v-if`, `x-on:click`, `:class`, `@submit` — are names their author
  chose, exactly as a class selector is. A rule that coloured every one of them would make a tag a
  wall of colour and would say something untrue about names Unluminate has never heard of.

### 3.4 Value

After an `=` inside a tag. A quoted value is a `Token::String` — the ordinary string rule, which is
finally allowed to run because the reader is inside a tag. An **unquoted** value is a `Token::String`
too, read to the next whitespace, `>` or `/`, which is the spec's unquoted-attribute-value rule, so
`<input type=text>` and `<td colspan=2>` colour the value rather than mistaking it for another
attribute.

### 3.5 Raw text — and why `title` and `textarea` differ from `script` and `style`

```
language.raw_text = script=javascript, style=css, textarea, title
```

An element whose body is **not markup**. After its start tag closes, everything up to `</name` is
read as one stretch, and nothing inside it opens a tag — which is what makes `if (a < b)` inside a
`<script>` survive.

The four names are the HTML Standard's own two categories: `script` and `style` are **raw text
elements**, and `textarea` and `title` are **escapable raw text elements**, the difference being
that character references are still decoded in the second pair. That distinction is not declared in
the manifest — it is *derived*, and this is the nicest thing in the key: **an entry that names a
language is raw text, and an entry that names none is escapable raw text**, so `&amp;` inside
`<title>` is still coloured and `&amp;` inside `<script>` is not, exactly as a browser reads them.
One key, two facts, and neither of them written down twice.

The list is data, not a list of element names inside Unluminate, for the reason `language.definers`
exists: a list of languages inside Unluminate is a list a plugin somebody writes later can never join. A
manifest for a template language that has its own raw-text element gets the behaviour by naming it.

**The right-hand side is a language name, and it is not checked at parse time.** That is deliberate
and it is the one registry-shaped key in Unluminate that is not validated against a list — because the
name is resolved by `Plugins::for_language`, the same function a ```` ```rust ```` fence in a
Markdown document is resolved by, which already documents that a language nothing claims answers
with nothing. Checking it would mean a plugin refusing to load because *another* plugin is switched
off, which is a worse outcome than a `<style>` block that is drawn in one colour.

---

## 4. The embedded language, and the seam that already existed

A `<style>` block drawn as one flat stretch of text is the difference between an HTML plugin and a
good one. Unluminate already has the seam for this and it is `CodeHighlighter`: *`unluminate-core` holds no
plugin registry and must not learn about one, so it asks and the window answers.*

The shape here is the mirror of that one. `scan` cannot colour a `<script>` body, so it **says where
it is and what language it names** and colours nothing:

```rust
/// A stretch of a markup document written in another language, and which one.
pub struct Embedded { pub range: Range<usize>, pub language: String }

pub fn scan_with_embedded(text, grammar, embedded: &mut Vec<Embedded>, report: impl FnMut(Range<usize>, Token))
pub fn scan(text, grammar, report)  // the same call, throwing the list away
```

`scan` keeps its signature, so `symbols`, `imports`, `file_move` and `folding` are untouched, and
the `Vec` never allocates for a language that names no raw text. The window's `colour_the_file` —
which is already the one place a source file is coloured — asks `Plugins::for_language` for each
region, runs the ordinary scan over that slice with that grammar, and offsets the ranges into the
file. The same function serves the Markdown preview's fences through `PluginHighlighter`, so a
```` ```html ```` fence holding a `<style>` block is coloured the same way a `.html` file is.

Two consequences, both of them wanted:

- **Switching the CSS plugin off withdraws CSS colouring from inside `<style>` in the same frame**,
  because `for_language` is asked at the moment of use — the rule `Plugins::renders` set and
  `run_file` and `debugger_for` have kept since.
- **It is one level deep.** The embedded scan's own `Embedded` list is discarded, so a manifest that
  embedded markup inside markup gets one level and no recursion. Nobody has asked for two and the
  bound is worth having for free.

Two smaller things have to be got right and both are silent wrong answers if they are missed:

- `StyleSpans::set_many` **requires its changes in order and non-overlapping** — it skips anything
  whose start is before the previous end. The embedded spans are produced after the outer pass, so
  the list is sorted before it is applied. Only for a file that had an embedded region at all.
- `folding::Tokens` is collected in the same pass, and a `<style>` block's own comments and strings
  have to be in it or a `}` inside a CSS string would fold. They are noted from the embedded scan
  too, and `Tokens::put_in_order` puts them back in position order, because `Tokens::covers` is a
  binary search.

---

## 5. The colours, and why an entity is a number

Dracula, like the other five, because a folder of mixed files should be read in one scheme rather
than six that almost agree. The mapping is **Unluminate's per-token one rather than Dracula's
HTML-specific one**, which is the sentence the CSS manifest already writes and the reason is the
same: `theme.builtin` is purple in every other manifest here, and an HTML file in the tab beside a
TypeScript file should be read in the same scheme.

| What | Token | Dracula | Why |
|---|---|---|---|
| element name | `Keyword` | `#FF79C6` | `entity.name.tag`; and Dracula's own HTML tags are pink, so the two agree |
| custom element | `Type` | `#8BE9FD` | it is known to be a tag name and known not to be one of HTML's |
| attribute name | `Builtin` | `#BD93F9` | `entity.other.attribute-name`; purple is what `builtin` is everywhere else in Unluminate |
| attribute value | `String` | `#F1FA8C` | it is a string |
| `&amp;` `&#8212;` | `Number` | `#FFB86C` | `constant.character.entity` sits under `constant` beside `constant.numeric`, so this is the conventional mapping rather than a convenience |
| `<` `>` `/` `=` `!` | `Operator` | `#FF79C6` | the punctuation of `meta.tag` |
| `<!-- -->` | `Comment` | `#6272A4` | |

**`language.numbers = false`**, and it matters. A number in an HTML file is nearly always in prose —
"In 2026 we shipped" — and orange digits scattered through a paragraph is exactly the noise this
whole design exists to avoid. The one place a digit run means something is inside a numeric character
reference, and that is read by the entity rule rather than by the number rule. `language.hex_colors`
is off for the same reason: `#` in HTML is `href="#anchor"` and prose.

---

## 6. What the plugin says beyond colours

### 6.1 `word_characters = -`

`aria-label`, `data-track-id`, `http-equiv`, `accept-charset` and `my-widget` are each one word.
Without it every hyphenated attribute is two tokens with an operator between them, which is
`task-1671`'s first finding restated. `:` is deliberately **not** a word character, so `xml:lang` is
two words with the colon between them: it would join words across prose (`Note:this`) and it would
change what `Rename Symbol` thinks a word is, which is too high a price for one attribute.

### 6.2 Imports — `src=` and `href=` complete the project's files

```
language.imports               = quoted
language.import_keywords       = src, href, srcset, data, poster, action, formaction
language.import_extensions     = .html, .htm, .css, .js, .mjs, .png, .jpg, ...
language.import_omit_extension = false
```

This falls out of `task-1680` with no new code at all. `imports::context_at` asks two questions of
the quoted family — is the caret inside a string, and does an import keyword appear as a whole word
between the start of the statement and that string — and `<script src="│">` answers yes to both.
So typing inside `src=""` offers the project's own files, `<link href="./│">` offers its
stylesheets, and what is inserted is a relative path with its extension written out, because HTML
names the file it is pointing at exactly as `@import 'theme.css'` does.

`export_keyword` is not named: HTML declares nothing, so it hides nothing, and there is no list of a
document's exports to offer.

### 6.3 No definers, and what that withdraws

`language.definers` is empty and `language.brace_definitions` is off, so `Go to Definition` is
**absent** for a `.html` file — Unluminate's rule for a control that can never apply, and the same
decision CSS made for the same reason. `id="header"` does name something, but it names it by
*position* rather than by keyword, and a rule that read `=` as a definer would call every attribute
a definition.

`Find References` and `Rename Symbol` still apply, because neither needs a definition — and renaming
a class name across the HTML and the CSS that styles it is exactly what somebody wants from this.

### 6.4 Completion knows where the caret is

The four sources `task-1678` gathers are right in prose and wrong inside a tag, and the reverse. So
`syntax::markup_position` answers where the caret is — in text, in a tag name, in an attribute, in a
value, or in raw text — by walking **backwards from the caret**, which is what `imports::context_at`
and `symbols::identifier_at` already do and for the same reason: a few hundred bytes of scanning a
keystroke rather than a reading of the file.

What it decides is one thing, in one place: **which of the language's own word lists is offered.**

| Where the caret is | What the language offers |
|---|---|
| a tag name — `<ta│` | the element names, and nothing else |
| an attribute — `<div cl│` | the attribute names, and nothing else |
| prose, a value, raw text | none of them |

The other three sources — this file's words, the other tabs' definitions, the project's index — are
untouched everywhere, so completing a word from the document's own prose goes on working. Without
this, typing an ordinary sentence in an HTML file pops a list of element names under the caret,
which is the same fault as colouring the prose, wearing a different hat.

### 6.5 Folding by tag, not by bracket

`folding::regions` reads brackets that span lines, and an HTML file has almost none. Well-indented
HTML folds correctly today by the indentation fallback and badly-indented HTML does not fold at all,
which is the wrong way round — the badly-indented file is the one somebody needs to fold.

So for a markup grammar, `regions_from` adds **tag regions**: a start tag opens, and the matching end
tag closes. `syntax::tags` reports them, running the same state machine, so a `<` inside a comment,
inside an attribute value or inside a `<script>` body is not a tag and cannot open one.

**There is no list of void elements**, and not needing one is worth stating. `<br>` and `<img>` never
close, so a naive stack would be left holding them for ever. The rule that removes the need is the
one `block_regions` already uses for a stray closing brace: an end tag pops back to **the nearest
matching name on the stack**, discarding whatever was above it. `<br>` is pushed, and discarded when
its parent closes. A self-closing `<img />` is not pushed at all.

Brackets are still read as well, because a `<style>` block's rules and a `<script>` block's functions
are genuinely worth folding, and `tidy` already keeps one arrow per head line.

---

## 7. Which word goes in which list

**`language.keywords` is the element names** — MDN's element reference, the deprecated ones included,
because a plugin's job is to read the HTML that exists rather than the HTML that should. `DOCTYPE`
and `doctype` are in it, so `<!DOCTYPE html>` colours; the `html` after it is an element name and
colours too, which is right by accident and right.

**`language.builtins` is the attribute names** — MDN's attribute reference, the global attributes,
the `aria-*` names that are a fixed list, and the event handler attributes. `data-*` is not a name,
it is a shape, so it is not in the list and `data-track-id` is plain text by §3.3's rule.

Six words are in both lists — `cite`, `data`, `form`, `label`, `slot`, `span`, `style`, `summary`,
`title` — and here, unlike in CSS, that costs nothing at all: the reader knows which position it is
in, so `<title>` is an element and `title="…"` is an attribute, and the same word is drawn two ways
in the same file correctly. That is the payoff for the state machine stated as plainly as it can be.

**`language.types` is empty.** The third list is available to a markup language that has a third
class of attribute and HTML does not; the `Type` colour is reached by §3.2's rule instead.

---

## 8. What is deliberately not in it

Each of these was considered and each has a reason, so that nobody has to rediscover it.

- **A rendered preview.** `language.renders = html` would mean a box model, an inline layout engine,
  a cascade and a network story, which is a browser. `unluminate-core` draws Mermaid because a diagram is
  arithmetic; a web page is not. The three view mode buttons are absent for a `.html` file, which is
  Unluminate's rule for a control that can never apply.
- **`.xml` and `.svg`.** Both are markup and would colour tolerably with these rules, and neither is
  HTML: an XML document has no HTML element names in it, has `<?xml?>` and `CDATA`, and calling its
  files HTML in the status bar would be a lie. An XML plugin is a manifest somebody can write in half
  an hour now that `language.markup` exists — which is the point of the key being data.
- **`.vue`, `.svelte`, `.astro`, `.jsx`.** Each is a language with a template in it, not a template.
  Their script blocks are the majority of the file and are not HTML.
- **`run.file`.** Opening a page in a browser is `start`, `open` or `xdg-open` depending on the
  machine, and `run.file` is one command line for every platform. A `browser` entry in
  `PROJECT_RUNNERS`' shape would be the honest answer and it is a ticket of its own.
- **`debug.adapter`.** Debugging a page means a browser and a DOM, which is a different adapter from
  the `node` one and a different design from `task-1687`'s.
- **Tag-aware editing** — closing a tag as it is typed, renaming the matching tag, selecting the
  enclosing element. All three are real IntelliJ features and all three are editing rather than
  reading; they need the tag tree `syntax::tags` now produces, so they are cheap follow-ups, and
  they are not what "a plugin for html" asked for.
- **Attribute values that hold another language.** `style="color: red"` is CSS and `onclick="f()"` is
  JavaScript. Both are strings and are drawn as strings. Embedding inside a string is a different
  mechanism from embedding inside an element and buys much less.
- **Case-insensitive element names.** `<DIV>` is valid HTML and is drawn as a custom element here,
  because a case-insensitive match would need a second lookup path in a function that runs over every
  byte of every file in Unluminate. Nobody has written uppercase tags since 1999 except in `<!DOCTYPE>`,
  which is in the list twice.

---

## 9. How it is checked

Four layers, as always, and the point of listing them is that each catches something the others
cannot.

1. **`unluminate-core`, no window.** The state machine's own tests: `5 < 3` in prose is prose, an
   apostrophe in prose is not a string, `<p>` is a tag and `< p>` is not, `if (a < b)` inside a
   script opens no tag, `&amp;` is a number in text and in `<title>` and is not in `<script>`, a tag
   name is a keyword and an unknown one is a type, `<title>` and `title="x"` in one file are drawn
   two ways, an unquoted value is a string, and a `<style>` body is reported as embedded CSS.
2. **`unluminate-app` unit tests.** The manifest parses, every key reaches the grammar, the older plugins
   ask for none of what this added, and switching the CSS plugin off withdraws the colouring inside
   `<style>` in the same frame.
3. **A screenshot test.** `syntax_html` opens a page with a doctype, a comment, a `<style>` block, a
   `<script>` block, custom elements, entities and a paragraph of prose with element names in it, and
   the picture is what proves the prose is not pink. `plugins_page` gains a row.
4. **The real window, through `unluminate-cli`.** Open a real `.html` file, read `editor text`, take a
   screenshot, fold a tag, ask for completion inside a tag and in prose.

And the cost is measured rather than asserted: `syntax::tags` is a second pass over a markup file
for folding, and `examples/folding_cost.rs` is what says what it costs.

---

## 10. Everything that changes

| Where | What |
|---|---|
| `unluminate-core/src/syntax.rs` | `Grammar::markup`, `Grammar::raw_text`, the five states in `scan`, `Embedded`, `scan_with_embedded`, `tags`, `markup_position` |
| `unluminate-core/src/folding.rs` | `tag_regions` for a markup grammar, `Tokens::put_in_order` |
| `unluminate-app/src/services/plugins.rs` | `language.markup` and `language.raw_text` read into the grammar |
| `unluminate-app/src/app/mod.rs` | `colour_the_file` and `PluginHighlighter` colour the embedded regions |
| `unluminate-app/src/app/completion.rs` | the language's words are offered by position in a markup file |
| `unluminate-app/plugins/html/` | the manifest, the icon and `icon.md` |
| `crates/unluminate-app/tests/` | the new screenshot and the accepted `plugins_page` |
