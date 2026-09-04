# task-1671 — a CSS plugin, and a terminal that can be interrupted

Two things were asked for:

> create a new plugin, similar to our typescript, rust, etc plugins, for css.
>
> research online, then create a tdd, the fully implement.
>
> also, we need ctrl+C to work in the terminal, so we can do things like cancel, stop claude code, etc.

They are unrelated, so they are written up separately. Sections 1 to 7 are the plugin; section 8 is
the terminal.

## 1. What a CSS plugin has to get right, and what the tokeniser could not do

An Unluminous plugin is data: a `plugin.conf` naming a language's extensions, its comments, its strings,
two lists of words and a colour a token. Three of those manifests already exist, and writing a fourth
for CSS is half an hour's typing. The half hour would have produced a bad plugin, and it is worth
being precise about why, because the answer is what most of this ticket is.

CSS was written by people who had no reason to care what a tokeniser finds easy. Four of its
commonest shapes fall straight through the rules in `unluminous_core::syntax`:

| The CSS | What the tokeniser did with it |
|---|---|
| `background-color: red;` | `background`, `-`, `color` — three tokens, and the hyphen drawn as an operator between two halves of one name |
| `--brand: #ff79c6;` | `-`, `-`, `brand` — and the colour is `#` and then a word |
| `#ff0000` and `#00ff00` | one is a word and the other is a number, because `number()` needs a digit first |
| `@media (min-width: 40rem)` | `@` and then `media`, so `@media` cannot be one keyword |

The first is the important one. **A hyphen is a letter in CSS.** Nearly every property name has one
in it, every custom property starts with two, and every vendor prefix is one. A tokeniser that breaks
a word at a hyphen cannot recognise a single CSS property by name, so the whole point of the plugin —
"colour the properties" — is unreachable.

There is a fifth, subtler one. CSS has **three** kinds of word worth telling apart, and an Unluminous
grammar had two lists:

```css
@media screen {          /* an at-rule            */
  .card { display: flex; /* a property, a value   */ }
}
```

`display` and `flex` in one colour is a readable file. It is also flatter than any editor a person is
used to: every one of them separates the property from the value, because that is the one distinction
a stylesheet is made of.

So three small, data-driven additions to `unluminous-core` come first, and the manifest is written against
them. Each is a key in the manifest, off unless a language asks for it, so no existing plugin changes
by a pixel.

## 2. `language.word_characters` — a hyphen is a letter here

```
language.word_characters = -, @
```

Characters that count as part of a word **anywhere in it, including the first position**. With `-`
and `@`, `background-color`, `--brand-hue`, `-webkit-box-shadow`, `nth-child` and `@font-face` are
each one word, which is what a person reading the file sees them as.

*Continue-only was considered and rejected.* Letting the extra characters continue a word but not
begin one keeps `-8px` as an operator and a number, which is a small gain, and costs `@media` (two
tokens again), `--brand` (three) and `-webkit-anything` (two). The rule is also longer to say. One
sentence — "these characters are part of a word" — is worth more than the negative-number case,
which is written into the plugin's own `limitations` instead: **`-8px` is a word, so it is drawn in
the plain text colour rather than as a number.**

*Why not a regular expression per token, as TextMate grammars use?* Because that is a different
tokeniser, not a bigger list of characters — `syntax.rs` is one linear pass with no dependency and no
backtracking, which is what keeps `highlight` cheap enough to run on every text revision of a large
file (`task-1666` §4). A `Vec<char>` tested with `contains` costs nothing measurable next to the
`is_alphanumeric` already there.

## 3. `language.types` — the third list

```
language.types = flex, grid, none, auto, absolute, solid, bold, ...
```

A third list of words, alongside `language.keywords` and `language.builtins`, producing
`Token::Type`. `classify` tries keyword, then builtin, then type, then the two heuristics that were
already there.

It generalises past CSS: `Token::Type` has until now been reachable only by the "starts with a
capital letter" heuristic, which is silent in any language that does not capitalise its type names.
Every existing manifest leaves the key out and gets an empty list, so nothing that ships today
changes.

Which CSS word goes in which list is section 5.

## 4. `language.hex_colors` — `#ff0000` is a number

```
language.hex_colors = true
```

A `#` followed by exactly **3, 4, 6 or 8** hexadecimal digits, and then something that is not a word
character, is a number. That is the CSS colour grammar exactly, and the lengths are what stop an id
selector being mistaken for one: `#header` is not hex, `#abcde` is five digits, and both fall through
to being a word.

`#face` and `#dad` **are** hex-shaped and are drawn as colours even when they are id selectors, which
cannot be helped without knowing whether the reader is in a selector or a declaration — the one thing
a token-at-a-time pass does not know. It is in the plugin's `limitations`.

The alternative was to leave the `#` as an operator, and it is worse than it sounds: `#00ff00` would
be drawn as a number (its first digit is a digit) and `#ff0000` as plain text (its first digit is a
letter), so half the colours in a file would be coloured and the other half not, apparently at
random. A rule that is sometimes wrong beats one that is arbitrarily inconsistent.

## 5. Which word goes in which list

The colours are Dracula's, as the other three bundled plugins are, and **the mapping is Unluminous's
per-token one rather than Dracula's CSS-specific one**. Dracula's specification asks for a green
property, a pink selector and a purple value; Unluminous's token kinds are shared across every language,
`theme.builtin` is purple in the JavaScript, TypeScript and Rust manifests, and a `.css` file sitting
in a tab next to a `.ts` file should be coloured by the same scheme rather than by a second one that
almost agrees with it. So:

| List | Token | Dracula | What is in it |
|---|---|---|---|
| `language.keywords` | Keyword | pink `#FF79C6` | the at-rules `@media`, `@import`, `@keyframes`, …; the words that structure one — `and`, `not`, `only`, `from`, `to`, `important`; the element selectors `div`, `a`, `button`, …; the pseudo-classes and pseudo-elements `hover`, `nth-child`, `before`, `selection`, … |
| `language.builtins` | Builtin | purple `#BD93F9` | the property names, hyphens and all: `background-color`, `grid-template-columns`, `z-index` |
| `language.types` | Type | cyan `#8BE9FD` | the value keywords: `flex`, `absolute`, `ellipsis`, `sans-serif`, `ease-in-out` |
| — | Function | green `#50FA7B` | anything directly before `(`, which is `var`, `calc`, `url`, `rgb`, `min`, `clamp`, `linear-gradient` |
| — | Number | orange `#FFB86C` | `16px`, `1.5rem`, `#ff79c6` |
| — | String | yellow `#F1FA8C` | `"Segoe UI"`, `'…'` |
| — | Comment | grey `#6272A4` | `/* … */` |

A property is a **builtin** because that is what `builtin` means in every other Unluminous manifest — a
name the language itself provides. A value keyword is a **type** because `flex`, `grid`, `absolute`
and `ellipsis` name a kind of thing.

Three consequences, all of them in `plugin.limitations` rather than left to be discovered:

- **A word that is both a property and a value is drawn the same way in both places**, because the
  lists are tried in order and nothing here knows whether it is left or right of the colon. The rule
  for deciding which list it goes in is *whichever reading is written more often*: `inset`, `left`,
  `top`, `bottom`, `content` and `border` are properties, so `border-style: inset` is drawn as a
  property; `flex`, `grid` and `all` are values, because `display: flex` and `transition: all` are
  written far more often than the shorthand properties of the same name.
- **A selector is only as coloured as its parts.** `.card` is a pink dot and a plain word, `#main` a
  pink hash and a plain word; a class name is somebody's own name, and Unluminous has nothing to say about
  it. `div`, `a:hover` and `::before` are coloured, because those words are the language's.
- **A custom property is plain text.** `--brand-hue` is a name its author chose, which is what
  Dracula's own specification says a variable should look like.

## 6. What is *not* in the plugin

**`.scss`, `.sass` and `.less` are not claimed.** They are different languages — nesting, variables,
mixins — and one of the differences is fatal here: they take `//` as a line comment, and CSS does
not. A grammar has one line comment, so claiming both would mean either colouring half of every
`url(https://…)` as a comment in a `.css` file, or leaving `// a note` uncoloured in a `.scss` one.
They are worth a plugin each, written against the same three additions this ticket makes; they are
not worth breaking `.css` for.

**Nothing is fetched, parsed or executed**, which is the rule the plugin system already keeps. There
is no `@import` following, no colour swatch in the gutter, no completion of property names. Those
want a language server, and `services/plugins.rs` records why that seam is named and left empty.

## 7. How it is checked

- `unluminous-core` gains a test per addition — hyphenated words, the three-list order, and the four hex
  lengths with `#header` and `#abcde` failing — and its existing tests must all still pass, which is
  what proves the additions are off by default.
- `services::plugins` gains the CSS plugin to the list every bundled manifest is checked against: it
  parses, it claims `.css` and nothing else, it has an icon, a description and a colour scheme.
- `crates/unluminous-app/tests/screenshots.rs` gains `a_css_file_is_coloured_by_its_plugin`, which opens a
  real stylesheet and asserts the colour of a property, a value, an at-rule, a hex colour and a
  comment by asking the document's own spans, then takes the picture. `plugins_page` is re-accepted
  with a fourth row on it.
- `cargo run --example frame_cost` is not needed: nothing here runs more often than the tokeniser
  already did, and the two new checks are a `contains` over a list of two characters and a
  `strip_prefix`.

## 8. Ctrl+C in the terminal

### What was wrong

`Ctrl+C` in Unluminous's terminal on Windows did nothing to the program. It copied the selection, or, with
nothing selected, copied nothing at all — so there was no way to stop a command, and no way to detach
from `claude`, which is what the ticket is really about.

The encoding was never the problem. `unluminous_terminal::keys` turns `Ctrl+C` into `0x03`, and
`terminal_panel::tests::control_and_a_letter_becomes_a_terminal_key_press` has asserted that since
the terminal was written. **The key press never arrived.** `egui-winit`, before it pushes an
`Event::Key`, asks whether the press is a clipboard command:

```rust
if is_copy_command(self.modifiers, active_key) {
    self.egui_input.events.push(egui::Event::Copy);
    return;                     // <- no Event::Key, and no Event::Text either
}
```

and `is_copy_command` is `modifiers.command && key == C`. On macOS `command` is the Apple key, so
`Ctrl+C` is an ordinary key press and has always worked. **On Windows `command` *is* the control
key**, so every `Ctrl+C` became an `Event::Copy` and the terminal's own handler — which mapped
`Copy | Cut` to "copy the selection" — swallowed it. This is why the fault is Windows-only and why no
test caught it: the tests are of `key_press`, and `key_press` is not on the path.

### The rule

The terminal's `Event::Copy` decides between copying and interrupting the way every terminal on
Windows does:

- **Something is selected** → copy it, and **clear the selection**. Clearing is not tidiness: a
  selection left behind would swallow the next `Ctrl+C` too, and the one after that, so a program
  could become impossible to stop by dragging the mouse once.
- **Nothing is selected** → send `0x03`. The program is interrupted, which is what the key means.
- **Shift is held** → copy, always. `Ctrl+Shift+C` is the copy that never interrupts, which is what
  Windows Terminal, VS Code's terminal and every Linux terminal emulator use it for.

`Event::Cut` — `Ctrl+X` — is passed to the program as `0x18` and never copies. There is nothing in a
terminal that can be cut, and `Ctrl+X` is how a person leaves `nano`.

Both are one small function, `clipboard_key`, taking what is selected and the modifiers and returning
either text to copy or bytes to send, so the rule is unit-tested rather than reasoned about — the
same level `key_press` is tested at, and for the same reason.

*Two things were considered and not done.* Reading the raw `winit` event ahead of egui would mean a
second input path with its own platform code, to recover a key egui deliberately consumed. And making
`Ctrl+C` *always* interrupt would take away the only copy the keyboard has in the terminal on
Windows, where there is no Apple key to fall back to.

`Ctrl+Insert` and `Shift+Delete` reach the window as these same two events and cannot be told from
`Ctrl+C` and `Ctrl+X`, because egui consumed the key that would have said which. Neither holds the
control key, so both take the copying half of the rule — which is what they have always meant on
Windows anyway.

## 9. What was measured, in the real window

Written after the work was done, so it says what happened rather than what was intended.

`cargo test --workspace` is green on Windows: 290 in `unluminous-core`, 205 screenshot tests, and the rest
across the other crates. `syntax_css.png` is a new accepted picture and `plugins_page.png` was
re-accepted with a fourth row on it; both were opened and looked at before being accepted.

**The stylesheet.** `cargo build --release`, the real binary opened on a 98 line stylesheet with the
real fonts of the machine. `unluminous-cli plugins list` answers `5 plugins, 5 switched on`, with `css`
among them. Every kind of token in section 5 comes out as that table says: `@import`, `@font-face`
and `:hover` pink; `background-color`, `font-family` and `line-height` purple; `dark`, `light`,
`column`, `monospace` and `ease-in-out` cyan; `url(`, `format(`, `var(`, `calc(`, `rgba(` and
`translateY(` green — none of them is in a list, so the bracket heuristic finds them; `#282a36`,
`#f8f8f2` and the four digit `#0008` orange; `"Iosevka"` yellow; `--brand-hue` and `.card` plain.

**The interrupt**, driven at the real window with `SendKeys` after taking the foreground with
`AttachThreadInput`, because `SetForegroundWindow` alone is refused to a background process:

| What was done | What the terminal showed |
|---|---|
| `echo hello` typed, then `Ctrl+C` | `echo hello^C` and a fresh prompt — the line was abandoned |
| `ping -t 8.8.8.8` running, then `Ctrl+C` | the statistics, `Control-C`, and a fresh prompt after 111 replies |
| a selection dragged out, then `Ctrl+C` | the clipboard held `illi-seconds:`, the words under the drag |
| `Ctrl+C` again, straight away | the clipboard was **not** written to, and the shell printed `^C` |

The last two rows together are the whole of the selection rule: the first press copies, and because
it lets go of the selection the second press interrupts. Without the clearing, the second press would
have copied the same words again, and every press after it, for as long as the selection stood.
