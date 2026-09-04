# task-1707 — Opening a whole block at once

## 1. What was asked for

> From the task-1695 observation study.
>
> Asked to "fold up all the functions so I can just see the shape of the file, then open `total_area`
> back up so I can read it", the agent collapsed everything (correctly, one call) and then expanded
> line 7 — and the function was still unreadable, because the `for` at line 9 and the `if` at line 10
> were still collapsed inside it. It took three calls, found by trial:
>
> ```
> fold expand --line 7   ->  8 of 9 blocks collapsed
> fold expand --line 9   ->  7 of 9 blocks collapsed
> fold expand --line 10  ->  6 of 9 blocks collapsed
> ```
>
> There is `--all` (the whole file) and `--line` (exactly one region) and nothing in between. "Open
> that function" is what a person means and there is no way to say it — for an agent or from the
> keyboard. IntelliJ has expand-recursively on a chord of its own for exactly this.

So: a way to say *this block, and everything inside it* — collapse or expand — from the keyboard, the
menu and the command line. `task-1686` deliberately left this out (its §13: "Recursive collapse and
expand-to-level … `Collapse All` and `Expand All` cover the ask; the nesting level is a dial with no
reader."). The study found the ask is real: a person who has collapsed everything to see the shape of
the file wants to open one function back up, whole, in one move.

The scope, from the ticket:

1. `fold expand --line N --recursive` — the region at that line and every region inside it.
2. The matching collapse, if the TDD finds it worth having.
3. A keyboard chord and a `View -> Folding` entry, because `task-1686` put the four fold commands in
   all three places and `action list` is built by walking the real menus.
4. Tests over a nested file, asserting the hidden set rather than the call count.

## 2. What other editors do

### 2.1 IntelliJ IDEA

Both exist, on chords of their own. The official keymap reference (2026.2), Code folding section:

| What | Windows / Linux | macOS |
|---|---|---|
| Collapse (the block at the caret) | `Ctrl+NumPad -` | `⌘ NumPad -` |
| Expand | `Ctrl+NumPad +` | `⌘ NumPad +` |
| **Collapse Recursively** | `Ctrl+Alt+NumPad -` | `⌘ ⌥ NumPad -` |
| **Expand Recursively** | `Ctrl+Alt+NumPad +` | `⌘ ⌥ NumPad +` |
| Collapse All | `Ctrl+Shift+NumPad -` | `⌘ ⇧ NumPad -` |
| Expand All | `Ctrl+Shift+NumPad +` | `⌘ ⇧ NumPad +` |

The docs say it in one sentence: "To collapse or expand code recursively, press Ctrl+Alt+NumPad -/+.
IntelliJ IDEA collapses or expands the current fragment and all its subordinate regions within that
fragment." The menu labels are "Collapse Recursively" and "Expand Recursively", and the mouse has its
own gesture: **Alt+click on the folding arrow** folds or unfolds recursively.

The action source settles the semantics. `CollapseRegionRecursivelyAction` finds the region at the
caret — `BaseFoldingHandler.getFoldRegionsForCaret` — **plus every region contained in it**, and sets
each `setExpanded(false)`; `ExpandRegionRecursivelyAction` does the same with `setExpanded(true)`. A
recursive expand therefore **opens the whole subtree**: a child that was independently collapsed is
opened too. There is no "keep the children as they were".

Sources:

- `https://www.jetbrains.com/help/idea/code-folding.html`
- `https://www.jetbrains.com/help/idea/reference-keymap-win-default.html`
- `https://www.jetbrains.com/help/idea/reference-keymap-mac-default.html`
- `https://www.jetbrains.com/help/rider/Code_Folding.html` — the platform's own menu table
- `https://github.com/JetBrains/intellij-community/blob/master/platform/foldings/src/com/intellij/codeInsight/folding/impl/actions/ExpandRegionRecursivelyAction.java`

### 2.2 Visual Studio Code

Both exist, as `editor.foldRecursively` and `editor.unfoldRecursively`, on `Ctrl+K Ctrl+[` and
`Ctrl+K Ctrl+]` (⌘K ⌘[ / ⌘K ⌘] on macOS). The docs: "Fold Recursively … folds the innermost uncollapsed
region at the cursor and all regions inside that region." "Unfold Recursively … unfolds the region at
the cursor and all regions inside that region."

The source (`src/vs/editor/contrib/folding/browser/folding.ts`) settles the semantics. Both call
`setCollapseStateLevelsDown(model, doCollapse, Number.MAX_VALUE, selectedLines)`, which toggles every
region inside the one at the cursor whose state differs from the target. A recursive unfold therefore
**opens the whole subtree**: every collapsed child is opened. Same as IntelliJ.

There is also `editor.toggleFoldRecursively` (`Ctrl+K Ctrl+Shift+L`), added in 1.128.0 after a user
request (issue #88915) — a recursive *toggle*, which the other two editors do not ship as a day-one
command. The mouse: **Shift+click or middle-click on the fold icon** is the recursive gesture
(Alt+click is the sibling/surrounding one).

Sources:

- `https://code.visualstudio.com/docs/editing/codebasics#_folding`
- `https://github.com/microsoft/vscode/blob/main/src/vs/editor/contrib/folding/browser/folding.ts`
- `https://github.com/microsoft/vscode/blob/main/src/vs/editor/contrib/folding/browser/foldingModel.ts`
- `https://github.com/microsoft/vscode/issues/88915`

### 2.3 CodeMirror 6

**No recursive command.** `@codemirror/fold` has `foldCode` (one level, the range at the caret),
`unfoldCode` (one level, the outermost range at the caret), `foldAll` (top-level ranges only — it
jumps past each fold it makes, so it is not recursive) and `unfoldAll` (every level). There is no
`unfoldRecursively`.

The data model is the interesting part, because it is the same shape Unluminous's is. `foldState` is a
`StateField<DecorationSet>` — a set of folded ranges. `unfoldEffect` removes **only the
exactly-matching range**; a child's fold is a separate entry and is untouched. So in CodeMirror,
unfolding a parent **preserves** the children's own fold state: a child that was independently folded
is still folded after the parent opens. (The parent's `Decoration.replace` hides the children while it
is closed, but their entries stay in the set.)

Sources:

- `https://github.com/codemirror/fold/blob/main/src/fold.ts`
- `https://codemirror.net/docs/ref/#fold`

## 3. What "recursive" means here

The two full editors agree, and they agree with the ask: **a recursive expand opens the whole subtree,
and a recursive collapse closes the whole subtree.** A child that was independently folded is opened by
a recursive expand; a child that was open is closed by a recursive collapse. There is no "keep the
children as they were" mode.

That is the one place Unluminous's data model has to be *deliberate* rather than *lazy*. Unluminous's fold state
is a set of collapsed head offsets inside the `Document` — the CodeMirror shape. Removing only the
parent's offset would *preserve* the children (CodeMirror's behaviour), which is the wrong answer for
this feature. So a recursive expand must remove every offset inside the target, and a recursive
collapse must add every head inside the target, and that is written as two small functions in
`unluminous_core::folding` rather than left to the set's natural behaviour.

### 3.1 The recursive set

One question, asked one way: **given a region, which regions are it and everything inside it?**

```rust
/// True when `other` is a region nested inside this one.
pub fn contains_region(&self, other: &Region) -> bool;

/// The region headed by `line`, and every region nested inside it, in the order the regions are.
/// `None` when nothing is headed by `line`.
pub fn region_tree(regions: &[Region], line: usize) -> Option<Vec<&Region>>;
```

`contains_region` is `self.head < other.head && other.last() <= self.last()`. The regions are sorted by
head and nest properly — a bracket, a tag, an indent and a heading each close before their parent does
— so a region is inside another exactly when its head is below the parent's and its last line is at or
above the parent's last. `region_tree` is the root plus every region that passes that test; a
grandchild passes it too, because the test is against the root, not against the parent.

The root is the region **headed by** `line`, which is what `fold expand --line N` already means (the
block that *starts* on line N), and it keeps the recursive and the plain commands aimed at the same
block. The keyboard asks the same question of the caret: the innermost region the caret is in, which is
`region_at`, and then its tree.

## 4. The commands

### 4.1 The command line

`--recursive` is a switch on the two commands that already take `--line`:

```
unluminous-cli fold collapse [--line <number>] [--all] [--recursive] [--regions]
unluminous-cli fold expand   [--line <number>] [--all] [--recursive] [--regions]
```

`--recursive` needs `--line`: it is "this block and everything in it", and a block is named by its
line. `--recursive --all` is a usage error, because `--all` already covers the whole file and there is
no block to be recursive about. With `--line`, the answer is the same as every other fold command —
how many blocks are collapsed, and `--regions` adds the list — so an agent that has just opened a
function can read the shape of what is left without a second call.

The no-op rule carries over: `fold collapse --line 7 --recursive` twice is a no-op the second time,
because the set is already what it asks for, and `set_folds` says so.

### 4.2 The keyboard and the menu

Two new entries on `View -> Folding`, beside the four that are there:

| Entry | Key |
|---|---|
| Collapse Recursively | `Ctrl/Cmd+Alt+Shift+Period` |
| Expand Recursively | `Ctrl/Cmd+Alt+Shift+Comma` |

The four existing chords are `Ctrl+.`, `Ctrl+Shift+.`, `Ctrl+Shift+,` and `Ctrl+Alt+.`, built around
the full stop and the comma beside it. The recursive pair is the same two keys with the one remaining
modifier added, which is what makes six of them a set rather than a list: the more modifiers, the wider
the reach. `action_names` fails the build if two entries claim one key, so the pairing is checked
rather than believed.

Both act on the block the caret is in — the innermost region at the caret, the same block `Collapse or
Expand Block` acts on — so the menu entry and the chord do what the person under the pointer means, and
the command line's `--line` names the same block by its line.

They are **absent for a file that cannot fold**, which `folding_applies` already answers for the whole
menu, and dimmed when there is nothing to do: `Collapse Recursively` when the file has no foldable
block, `Expand Recursively` when nothing is collapsed.

## 5. Tests

**`unluminous-core`, with no window.** A nested file — a function holding a `for` holding an `if`, the
study's own shape — and:

- `region_tree` at the function's head returns all three, in order.
- `region_tree` at the `for`'s head returns the `for` and the `if`, not the function.
- `region_tree` at a line that heads nothing returns `None`.
- `contains_region` for a grandchild, a sibling (false) and the region itself (false).

**`unluminous-app`, with no window.** Over a nested document:

- Collapse recursively at the function's head: the function, the `for` and the `if` are all collapsed;
  a block outside the function keeps whatever state it had.
- Expand recursively at the function's head when the `for` and the `if` are already collapsed: all
  three open — the children's collapsed state is destroyed, which is the decision of §3.
- Collapse recursively twice is a no-op the second time.
- The caret is brought out of a block that is collapsed recursively.

**Screenshot tests.** A nested file with the outer block collapsed recursively — the arrow, the badge,
and the line numbers jumping past the whole function. **Look at the images.**

**The command line.** `fold list`, then `fold collapse --line N --recursive`, then
`fold expand --line N --recursive` over a nested file, asserting the hidden set the reply reports
rather than the call count.

## 6. Deliberately not here

- **A recursive toggle.** VS Code added `toggleFoldRecursively` in 1.128.0, after a user asked;
  IntelliJ has no toggle at all — its two recursive commands are separate. Two commands are the ask and
  the precedent; a third is a follow-up, not a day-one requirement.
- **Expand to a level.** IntelliJ's `Ctrl+NumPad * 1..5` and VS Code's `editor.foldLevel1..7`.
  `task-1686` §13 already put this out: "the nesting level is a dial with no reader." Recursive is the
  ask; a level dial is a different feature.
- **Preserving the children's state on expand.** CodeMirror does it, by accident of its set model. The
  two full editors both destroy it, and the ask — "open that function so I can read it" — wants the
  whole thing open. A "keep the children" mode is a setting nobody asked for.
