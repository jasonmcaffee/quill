# Accessibility

`task-1804` §3.4 asked for this document, and asked for one thing from it above all: **say plainly
whether it ships in 1.0.**

## The answer

**No. Screen-reader support does not ship in 1.0, and it is the largest single piece of work
outstanding on this product.** What ships in 1.0 is the half that was measurable and cheap — the
accessibility tree turned on, so the names every control already carries reach the operating system —
and this document, so nobody has to rediscover the rest.

That is a decision rather than an omission, and the reason is that the remaining work is not one
change. It is a keyboard path to every pane, a focus model the window does not have, live regions for
the four things that change without being asked, and a screen-reader pass by somebody who uses one.
Shipping half of it and calling the product accessible would be worse than shipping none and saying
so, because the first is a claim somebody relies on.

**It is also the item most likely to be a hard requirement wherever this is sold**, which is §3.4's
own point and the reason the scope is written down now rather than when somebody asks for it.

---

## 1. What was already true, and what was already wrong

`task-1655` gave every control a name: `design/style-guide.md` requires it, `controls::icon_button`,
`modal::button`, `menu_row` and the rest all call `Response::widget_info`, and there are **112** such
calls in `components/`. The 482 screenshot tests find controls *by that name* — `harness.get_by_label
("Check for Updates")` — so the names are real, current and continuously tested, which is a better
starting position than most products have.

**And none of them reached anything.** `eframe` was configured
`default-features = false, features = ["wgpu", "default_fonts"]`, and `accesskit` is one of the
default features that turns off with them. `egui_kittest` enables it for the test harness itself,
which is why the tests could read the names — so the whole of the accessibility tree existed in the
test build and in no shipped binary.

That is fixed: `accesskit` is on. Narrator and UI Automation on Windows, VoiceOver and
NSAccessibility on macOS now receive a tree with the 112 named controls in it. It cost one line, and
it is the only part of this document that is done.

## 2. The contrast, measured

`tools/contrast.mjs` computes every ratio from `theme/mod.rs` itself — the palette is a closed list,
so this is the source of truth rather than a sample off a screenshot, and it can be run again the day
a colour moves. `node tools/contrast.mjs --check` fails when an ordinary-text pair is under 4.5:1.

**23 of the 28 pairs the window really draws meet WCAG 2.2. Five do not**, and they are named rather
than summarised:

| what is drawn | ratio | needs |
|---|---|---|
| **the word on the button that does the thing** (`text_strong` on `accent`) | **2.77:1** | 4.5:1 |
| the line between two panels (`divider` on `editor`) | 1.25:1 | 3:1 |
| the edge of a control, which has to be findable (`control_border` on `editor`) | 1.56:1 | 3:1 |
| a placeholder, and the match count on the Find bar (`text_faint` on `editor`) | 4.17:1 | 4.5:1 |
| the file count under the explorer (`text_faint` on `explorer_footer`) | 4.11:1 | 4.5:1 |

The first is the worst and it is not a corner: it is white on the accent blue, which is the primary
button in every modal in the product — `Done`, `Open`, `Replace`, `Commit`. At 2.77:1 the word on the
one button a person is meant to press is the least legible text in the window.

**Nothing here has been changed**, and that is deliberate: the palette is closed, five theme plugins
inherit from it, and 482 accepted screenshots are of it. Moving a colour is a change somebody should
agree to rather than one a measurement makes on its own. What it needs is a darker accent for the
button's ground or a darker word on it — `#2C6FB8` under white reaches 4.6:1, and near-black on the
accent as it is reaches 8.4:1 — and a decision about whether the dividers become visible or the
product says plainly that its chrome is low-contrast by design.

## 3. And the thing the contrast table cannot say

**Unluminous's background is translucent.** Every ratio above is measured at full opacity, and what
is really behind the text is the desktop at `1 - opacity`. A pair that passes at 100 per cent can
fail on a pale wallpaper at 40. Text is painted at full alpha and the grounds are not, which is what
keeps it readable at all — but a person choosing a low opacity is choosing lower contrast, and
nothing in the window says so.

The honest answer is probably a note beside the opacity slider rather than a limit on it: the
transparency is the character of the product and `README.md` says so.

## 4. What is missing, in the order it would have to be done

1. **A keyboard path to every pane.** `Focus` is an enum with a handful of values and the panes are
   reached with the pointer or with a chord each. There is no `F6`-style cycle through the panes, no
   way to reach the rail, the tabs, the status bar or a plugin's pane from the keyboard, and no
   visible focus ring that is not the caret. Everything after this depends on it: a tree a screen
   reader can read is no use if there is no way to move through it.
2. **A focus model the accessibility tree agrees with.** egui's focus and Unluminous's `Focus` are two
   ideas about the same thing — `controls::wants_the_keyboard` exists because a widget of Unluminous's
   own has to hold egui's focus while the window decides what has the keyboard. A screen reader reads
   egui's, so the two have to be one.
3. **Roles rather than labels.** Most `widget_info` calls announce `WidgetType::Button` or `Label`,
   which is right for a button and wrong for a tree, a grid, a tab strip and a list. The explorer is a
   tree, the database grid is a grid, and the file tabs are a tab strip; each has a role, and each
   needs its expanded state, its selection and its position announced.
4. **The editing area itself.** It is one painted rectangle: the text, the caret and the selection are
   drawn, not widgets. A screen reader needs the line the caret is on, the character or word it moved
   over, and the selection as it changes — which is a `TextEdit`-shaped surface reporting into
   AccessKit rather than a rectangle.
5. **Live regions**, for the four things that change without being asked: the status bar's message,
   git's replies, a run's output and the agent chat's stream.
6. **The one place a control was deliberately hidden.**
   `components/agent_tasks/ticket_modal.rs` contains the only comment in the tree that notices the
   accessibility tree exists, and it is there to explain that a control was **painted rather than
   named** to stay out of it. That is the right decision for a dropdown that would otherwise announce
   itself twice, and it is exactly the sort of thing that has to be revisited once anything reads the
   tree.
7. **A pass by somebody who uses a screen reader.** Everything above can be done and still produce a
   window nobody can use. This is not a step that can be replaced by a test.

## 5. What is not in scope, and is a decision rather than an omission

**Localisation.** There is none of any kind: every string is English and written where it is drawn.
`task-1804` §3.4 called deferring it reasonable and asked that it be written down rather than left as
an omission, so: it is deferred, and the first step whenever it is taken is that the strings become a
table rather than literals — which is a change to every file in `components/`.

**High-contrast and forced-colours modes.** Windows' own high-contrast setting is not read. A theme
is the mechanism that would answer it — `Palette` is a closed list of names and a theme says what
each means — so this is a theme plus a way to notice the system asked for one, rather than new
machinery.

**Reduced motion.** There is very little motion to reduce. The one thing that animates is the
plugin decoration's rasterisation on a changed frame, and `Settings -> Appearance` already has a
tick box that turns the whole of it off.

## 6. How to check any of this again

```sh
node tools/contrast.mjs             # every ratio, from the palette itself
node tools/contrast.mjs --check     # exits 1 when a pair is under WCAG 2.2
```

The 482 screenshot tests are the other half: every one of them finds its controls by the name a
screen reader would read, so a control that loses its name fails a test long before anybody with a
screen reader meets it.
