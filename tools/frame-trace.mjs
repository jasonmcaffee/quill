#!/usr/bin/env node
// Read a frame trace and print the median of each phase.
//
// `UNLUMINATE_FRAME_TRACE=<file> unluminate` writes one line per frame, which for a window left open
// for a minute is a few thousand lines nobody can read. This turns them into the handful of numbers
// worth reading: how long a frame took, how long between frames, and what each phase of one cost.
//
//   node tools/frame-trace.mjs <file> [--from N] [--json]
//
// `--from N` ignores the first N frames, which is what you want when the interesting part is a
// settled window rather than the first few frames that build every cache the editor has.
//
// It is a reader and not a test. `task-1805` §9 says why there is no threshold here: every number is
// a different number on another machine.

import { readFileSync } from 'node:fs';

const args = process.argv.slice(2);
const path = args.find((argument) => !argument.startsWith('--'));
const json = args.includes('--json');
const fromAt = args.indexOf('--from');
const from = fromAt >= 0 ? Number(args[fromAt + 1]) || 0 : 0;

if (!path) {
  console.error('usage: node tools/frame-trace.mjs <trace file> [--from N] [--json]');
  process.exit(2);
}

const lines = readFileSync(path, 'utf8').split(/\r?\n/);

// `mark <name> <ms since the program started>` — the straight line of starting up.
const marks = [];
// `frame <ms> [outside <ms>] | <phase> <ms> …` — the loop.
const frames = [];

for (const line of lines) {
  const words = line.trim().split(/\s+/);
  if (words[0] === 'mark' && words.length >= 3) {
    marks.push({ name: words[1], at: Number(words[2]) });
    continue;
  }
  if (words[0] !== 'frame') continue;
  const [head, tail = ''] = line.split('|');
  const headWords = head.trim().split(/\s+/);
  const frame = { total: Number(headWords[1]), outside: undefined, phases: {} };
  if (headWords[2] === 'outside') frame.outside = Number(headWords[3]);
  const rest = tail.trim().split(/\s+/).filter(Boolean);
  for (let at = 0; at + 1 < rest.length; at += 2) {
    // A phase drawn twice in one frame is two pieces of work; they are added, because what the
    // reader wants is what that name cost this frame.
    frame.phases[rest[at]] = (frame.phases[rest[at]] ?? 0) + Number(rest[at + 1]);
  }
  frames.push(frame);
}

const kept = frames.slice(from);
if (kept.length === 0) {
  console.error(`no frames in ${path}${from ? ` after skipping ${from}` : ''}`);
  process.exit(1);
}

/** The middle value, which is the honest average for a distribution with a long tail on one side. */
function median(values) {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

const names = [...new Set(kept.flatMap((frame) => Object.keys(frame.phases)))];
const phases = names
  .map((name) => ({
    phase: name,
    median: median(kept.map((frame) => frame.phases[name] ?? 0)),
    worst: Math.max(...kept.map((frame) => frame.phases[name] ?? 0)),
  }))
  .sort((a, b) => b.median - a.median);

const summary = {
  file: path,
  frames: kept.length,
  medianFrameMs: median(kept.map((frame) => frame.total)),
  worstFrameMs: Math.max(...kept.map((frame) => frame.total)),
  medianGapMs: median(kept.map((frame) => frame.outside).filter((gap) => gap !== undefined)),
  marks,
  phases,
};

if (json) {
  console.log(JSON.stringify(summary, null, 2));
  process.exit(0);
}

const round = (value) => value.toFixed(3);

if (marks.length > 0) {
  console.log('starting up — milliseconds from the first mark:');
  let previous = 0;
  for (const mark of marks) {
    console.log(
      `  ${mark.name.padEnd(20)} at ${mark.at.toFixed(1).padStart(9)}   (+${(mark.at - previous).toFixed(1)})`
    );
    previous = mark.at;
  }
  console.log('');
}

console.log(
  `${summary.frames} frames${from ? ` (the first ${from} skipped)` : ''}: ` +
    `median ${round(summary.medianFrameMs)} ms, worst ${round(summary.worstFrameMs)} ms, ` +
    `median gap between frames ${round(summary.medianGapMs)} ms`
);
console.log('');
console.log(`  ${'phase'.padEnd(20)}${'median'.padStart(10)}${'worst'.padStart(10)}`);
for (const phase of phases) {
  console.log(
    `  ${phase.phase.padEnd(20)}${round(phase.median).padStart(10)}${round(phase.worst).padStart(10)}`
  );
}
