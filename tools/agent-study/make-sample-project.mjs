// Writes the small project the scenarios are run against: a Rust binary with a module,
// a node program, a stylesheet, and Markdown with a table and a Mermaid diagram — enough
// shape for folding, symbols, the preview, both debuggers and git to have something real
// to work on. It is its own git repository with one commit, one modified file and one
// untracked file, because the git scenario asks about all three.
//
//   node tools/agent-study/make-sample-project.mjs
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const REPO = path.resolve(import.meta.dirname, '..', '..');
const OUT = process.env.STUDY_OUT ?? path.join(REPO, '_agent_output', 'agent-study');
const P = process.env.STUDY_PROJECT ?? path.join(OUT, 'scratch-project');

const files = {
  // `[workspace]` keeps it out of Unluminous's own workspace when it is written under the repo,
  // which otherwise makes every cargo command in it fail before it starts.
  'Cargo.toml': `[workspace]\n\n[package]\nname = "scratch"\nversion = "0.1.0"\nedition = "2021"\n\n[[bin]]\nname = "scratch"\npath = "src/main.rs"\n`,
  '.gitignore': `target/\n`,
  'src/main.rs': `// A small program with enough shape to fold, navigate and debug.
mod shapes;

use shapes::{Rect, area_of};

/// Adds up the areas of a handful of rectangles.
fn total_area(rects: &[Rect]) -> f64 {
    let mut total = 0.0;
    for r in rects {
        if r.width > 0.0 && r.height > 0.0 {
            total += area_of(r);
        }
    }
    total
}

/// The largest rectangle by area, or none when the list is empty.
fn largest(rects: &[Rect]) -> Option<&Rect> {
    let mut best: Option<&Rect> = None;
    for r in rects {
        match best {
            None => best = Some(r),
            Some(b) if area_of(r) > area_of(b) => best = Some(r),
            _ => {}
        }
    }
    best
}

fn main() {
    let rects = vec![
        Rect { name: "a".into(), width: 3.0, height: 4.0 },
        Rect { name: "b".into(), width: 5.0, height: 2.0 },
        Rect { name: "c".into(), width: 1.5, height: 9.0 },
    ];
    let total = total_area(&rects);
    println!("total area {total}");
    if let Some(big) = largest(&rects) {
        println!("largest is {} at {}", big.name, area_of(big));
    }
}
`,
  'src/shapes.rs': `/// A rectangle with a name on it.
pub struct Rect {
    pub name: String,
    pub width: f64,
    pub height: f64,
}

/// How much room a rectangle takes up.
pub fn area_of(r: &Rect) -> f64 {
    r.width * r.height
}

/// Whether two rectangles are the same size.
pub fn same_size(a: &Rect, b: &Rect) -> bool {
    area_of(a) == area_of(b)
}
`,
  'web/app.js': `// A small node program, so the node debugger has something to stop in.
function collatzLength(n) {
  let steps = 0;
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = 3 * n + 1;
    }
    steps += 1;
  }
  return steps;
}

function longestUnder(limit) {
  let best = 1;
  let bestSteps = 0;
  for (let i = 1; i < limit; i += 1) {
    const steps = collatzLength(i);
    if (steps > bestSteps) {
      bestSteps = steps;
      best = i;
    }
  }
  return { best, bestSteps };
}

const answer = longestUnder(2000);
console.log(\`longest chain under 2000 starts at \${answer.best} (\${answer.bestSteps} steps)\`);
`,
  'web/styles.css': `:root {
  --brand-hue: 280;
  --text: #e8e8ef;
}

.panel {
  display: flex;
  background: #12121a;
  color: var(--text);
  padding: 12px 16px;
}

.panel--wide {
  max-width: 100%;
}
`,
  'README.md': `# Scratch

A little project that exists so an agent has something real to open, fold, navigate and debug.

## What is in it

| Path | What it is |
|---|---|
| \`src/main.rs\` | The Rust program |
| \`src/shapes.rs\` | The \`Rect\` type and the two functions on it |
| \`web/app.js\` | A node program |
| \`web/styles.css\` | A stylesheet |

## A diagram

\`\`\`mermaid
flowchart LR
  main --> total_area --> area_of
  main --> largest --> area_of
\`\`\`
`,
  // A page for `s24-browser`. A browser tab renders HTML over the `unluminous://` project origin,
  // so the scenario needs one: the first run of that scenario asked for README.md and was refused,
  // correctly, with "is not an HTML file". `task-1804`.
  'web/index.html': `<!doctype html>
<html>
  <head><title>Scratch</title><link rel="stylesheet" href="styles.css"></head>
  <body><h1>Scratch</h1><p>A page for the browser tab to render.</p><script src="app.js"></script></body>
</html>
`,
  'docs/notes.md': `# Notes\n\nSome prose to preview.\n\n- one\n- two\n`,
};

fs.mkdirSync(P, { recursive: true });
for (const [rel, body] of Object.entries(files)) {
  const f = path.join(P, rel);
  fs.mkdirSync(path.dirname(f), { recursive: true });
  fs.writeFileSync(f, body);
}

// A small SQLite database for `s25-database`, which asks an agent to add a data source, list its
// tables and say how many rows are in the biggest one. `task-1804` §4.2: the Database plugin
// shipped in `task-1777` and no scenario had ever watched an agent use it, which is the state
// `CLAUDE.md` says a feature must not be left in.
//
// Written as SQL and turned into a file by `sqlite3` when this machine has one; the scenario says so
// itself when it does not, rather than the run failing somewhere further in. Not committed, because
// nothing binary in this repository is.
const database = path.join(P, 'data', 'sample.db');
fs.mkdirSync(path.dirname(database), { recursive: true });
if (!fs.existsSync(database)) {
  const schema = [
    'CREATE TABLE artist (id INTEGER PRIMARY KEY, name TEXT NOT NULL);',
    'CREATE TABLE album (id INTEGER PRIMARY KEY, title TEXT NOT NULL, artist_id INTEGER REFERENCES artist(id));',
    'CREATE VIEW album_by_artist AS SELECT artist.name, album.title FROM album JOIN artist ON artist.id = album.artist_id;',
    "INSERT INTO artist (id, name) VALUES (1, 'Miles Davis'), (2, 'Bill Evans'), (3, 'John Coltrane');",
    "INSERT INTO album (id, title, artist_id) VALUES (1, 'Kind of Blue', 1), (2, 'Milestones', 1), (3, 'Waltz for Debby', 2), (4, 'Sunday at the Village Vanguard', 2), (5, 'A Love Supreme', 3), (6, 'Giant Steps', 3), (7, 'Blue Train', 3);",
  ].join('\n');
  const made = spawnSync('sqlite3', [database], { input: schema, encoding: 'utf8' });
  if (made.error || made.status !== 0) {
    console.log('no sqlite3 on this machine, so data/sample.db was not made: s25-database will have nothing to open.');
  }
}

const git = (...a) => spawnSync('git', a, { cwd: P, encoding: 'utf8' });
if (!fs.existsSync(path.join(P, '.git'))) git('init', '-q');
git('add', '-A');
git('-c', 'user.name=Scratch', '-c', 'user.email=scratch@example.com', 'commit', '-qm', 'first commit');
// One tracked change and one untracked file, so `git status` has all three states to report.
fs.appendFileSync(path.join(P, 'src/shapes.rs'), '// a change nobody has committed\n');
fs.writeFileSync(path.join(P, 'NOTES.txt'), 'untracked\n');

console.log(`sample project written to ${P}`);
console.log(`open it with:  unluminous-cli launch "${P}" --no-wait`);
