// What the palette's contrast ratios really are, against WCAG 2.2.
//
// `task-1804` §3.4: *"there is no mention of contrast, colour-blindness or WCAG in the 458-line style
// guide."* This is the missing measurement rather than an opinion about it — the palette is a closed
// list in `crates/unluminate-app/src/theme/mod.rs`, so the ratios can be computed from the source of
// truth rather than sampled off a screenshot, and they can be computed again the day a colour moves.
//
// ## What the numbers mean
//
// WCAG 2.2 asks for **4.5:1** for ordinary text, **3:1** for text at 18pt or 14pt bold, and **3:1**
// for the boundary of a control somebody has to find. A ratio is between two *rendered* colours, so
// every pair below names the ground it was measured against: `text_dim` on `editor` and `text_dim`
// on `status_bar` are different numbers and the palette has to pass both.
//
// **It is measured at full opacity**, and that is a limit worth stating: Unluminate's background is
// translucent, so what is really behind the text is the desktop at `1 - opacity`. A ratio that
// passes here can fail on a pale wallpaper at 40 per cent. Text is painted at full alpha and the
// grounds are not, which is why the honest measurement is of the palette and the honest statement is
// that a person choosing a low opacity is choosing lower contrast.
//
//   node tools/contrast.mjs            the table
//   node tools/contrast.mjs --check    exit 1 if any ordinary-text pair is under 4.5:1

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = join(dirname(fileURLToPath(import.meta.url)), '..')
const source = readFileSync(join(repo, 'crates/unluminate-app/src/theme/mod.rs'), 'utf8')

/** Every `name = Color32::from_rgb(r, g, b);` in the palette macro's own list. */
function palette() {
  const found = new Map()
  const pattern = /^\s{4}([a-z_]+) = Color32::from_rgb\(0x([0-9A-Fa-f]{2}), 0x([0-9A-Fa-f]{2}), 0x([0-9A-Fa-f]{2})\);$/gm
  for (const [, name, r, g, b] of source.matchAll(pattern)) {
    found.set(name, [parseInt(r, 16), parseInt(g, 16), parseInt(b, 16)])
  }
  return found
}

/** WCAG relative luminance. */
function luminance([r, g, b]) {
  const channel = value => {
    const v = value / 255
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/** The contrast ratio between two colours, 1:1 to 21:1. */
function ratio(a, b) {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (high + 0.05) / (low + 0.05)
}

// Every pair the window really draws: what is written, and what it is written on. Taken from the
// components rather than from every combination, because a ratio between two colours that never
// meet is a number about nothing.
const pairs = [
  ['text', 'editor', 'body text in the editing area', 4.5],
  ['text_strong', 'editor', 'the file name, and a value in a dialog', 4.5],
  ['text_control', 'menu', 'a menu entry', 4.5],
  ['text_control', 'control', 'the word on a button', 4.5],
  ['text_control', 'title_bar', 'the project name in the title bar', 4.5],
  ['text_control', 'explorer', 'a file name in the explorer', 4.5],
  ['text_control', 'status_bar', 'the file name in the status bar', 4.5],
  ['text_dim', 'editor', 'the line numbers, and a shortcut beside a menu entry', 4.5],
  ['text_dim', 'status_bar', 'the kind, the line ending and the caret position', 4.5],
  ['text_dim', 'explorer', 'a folder that is not open', 4.5],
  ['text_dim', 'menu', 'a menu entry that cannot be used just now', 3],
  ['text_faint', 'editor', 'a placeholder, and the match count on the Find bar', 4.5],
  ['text_faint', 'explorer_footer', 'the file count under the explorer', 4.5],
  ['text_strong', 'accent', 'the word on the button that does the thing', 4.5],
  ['text_strong', 'selected_row', 'the file that is open, in the explorer', 4.5],
  ['accent', 'editor', 'the caret, and the underline under a linked word', 3],
  ['control_border', 'editor', 'the edge of a control, which has to be findable', 3],
  ['divider', 'editor', 'the line between two panels', 3],
  ['unsaved', 'status_bar', 'the dot that says there are unsaved changes', 3],
  ['git_added', 'editor', 'an added line in a diff', 3],
  ['git_modified', 'editor', 'a changed line in a diff', 3],
  ['git_untracked', 'explorer', 'a file git has never seen', 3],
  ['file_markdown', 'explorer', 'the mark beside a Markdown file', 3],
  ['file_text', 'explorer', 'the mark beside a plain file', 3],
  ['icon', 'title_bar', 'a drawn icon in the rail', 3],
  ['close', 'title_bar', 'the close button', 3],
  ['maximise', 'title_bar', 'the maximise button', 3],
  ['agent', 'editor', "an agent's name on a ticket", 3],
]

const colours = palette()
const rows = pairs.map(([front, back, what, needs]) => {
  const a = colours.get(front)
  const b = colours.get(back)
  if (!a || !b) return { front, back, what, needs, got: 0, missing: true }
  return { front, back, what, needs, got: ratio(a, b), missing: false }
})

const failing = rows.filter(row => row.missing || row.got < row.needs)

if (process.argv.includes('--check')) {
  for (const row of failing) {
    console.error(`${row.front} on ${row.back}: ${row.got.toFixed(2)}:1, needs ${row.needs}:1 — ${row.what}`)
  }
  if (failing.length > 0) {
    console.error(`\n${failing.length} of ${rows.length} pairs are under WCAG 2.2.`)
    process.exit(1)
  }
  console.log(`All ${rows.length} pairs meet WCAG 2.2.`)
} else {
  console.log('| what is drawn | ratio | needs | |')
  console.log('|---|---|---|---|')
  for (const row of rows) {
    const mark = row.missing ? 'no such colour' : row.got >= row.needs ? 'passes' : '**FAILS**'
    console.log(`| ${row.what} (\`${row.front}\` on \`${row.back}\`) | ${row.got.toFixed(2)}:1 | ${row.needs}:1 | ${mark} |`)
  }
  console.log(`\n${rows.length - failing.length} of ${rows.length} pairs meet WCAG 2.2.`)
}
