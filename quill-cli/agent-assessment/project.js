// Build the small project the assessment drives Quill on.
//
// A project of its own rather than Quill's own repository, for two reasons. The answers have to be
// checkable — "open the file whose name is closest to `notes`" needs one right answer — and an
// assessment whose results move because somebody added a file is not measuring the model.
//
//   node quill-cli/agent-assessment/project.js <folder>

const fs = require('fs');
const path = require('path');

const files = {
  'README.md': `# Sample

A small project the Quill CLI assessment drives.

## Sections

It has prose, a little code, and a picture reference, so that the Markdown
preview has something to show.

![the mark](mark.png)

## The end

Nothing here is real.
`,
  'notes.md': `# Notes

- the first note
- the second note
- the third note
`,
  'todo.txt': `buy milk
write the assessment
read the results
`,
  'src/main.rs': `//! The sample program.

fn main() {
    println!("hello from the sample");
}

fn helper(value: usize) -> usize {
    value * 2
}
`,
  'src/lib.rs': `//! The sample library.

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
`,
  'src/markdown.rs': `//! A file whose name a subsequence search finds from "mdrs".

pub fn render() -> String {
    String::from("rendered")
}
`,
  'docs/guide.md': `# Guide

How to use the sample. There is a needle in this file: PORCUPINE.
`,
  'docs/reference.md': `# Reference

Every knob the sample has. None of them.
`,
};

// Write the project out, putting every file back to what it should be.
//
// Called at the start of every round as well as by hand, so that a task which edited or added a file
// cannot change what the next round measures. The folder itself is not removed and re-made: a Quill
// is open on it, and on Windows a running terminal's working directory cannot be deleted from under
// it. Files the sample does not own are removed one at a time instead, and a file that will not go
// is left rather than made into a failure.
function write(root) {
  const wanted = new Set(Object.keys(files).map((name) => path.normalize(name)));
  for (const [name, body] of Object.entries(files)) {
    const target = path.join(root, name);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, body, 'utf8');
  }
  for (const found of walk(root, root)) {
    if (wanted.has(found)) continue;
    try {
      fs.rmSync(path.join(root, found));
    } catch (_) {
      /* something has it open; the next round can live with it */
    }
  }
  return Object.keys(files).length;
}

// Every file under `root`, as a path relative to it.
function walk(root, here) {
  const out = [];
  for (const entry of fs.readdirSync(here, { withFileTypes: true })) {
    const full = path.join(here, entry.name);
    if (entry.isDirectory()) out.push(...walk(root, full));
    else out.push(path.relative(root, full));
  }
  return out;
}

if (require.main === module) {
  const root = process.argv[2];
  if (!root) {
    console.error('say where to put the project');
    process.exit(2);
  }
  console.log(`wrote ${write(root)} files into ${root}`);
}

module.exports = { write, files };
