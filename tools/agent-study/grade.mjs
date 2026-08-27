// Reads the sessions a run produced and prints the numbers the study is judged on:
// how much of the work went through Quill rather than round it, and every refusal.
//
//   node tools/agent-study/grade.mjs
import fs from 'node:fs';
import path from 'node:path';

const REPO = path.resolve(import.meta.dirname, '..', '..');
const OUT = process.env.STUDY_OUT ?? path.join(REPO, '_agent_output', 'agent-study');
const dir = path.join(OUT, 'sessions');
if (!fs.existsSync(dir)) { console.log(`no sessions in ${dir} — run run-all.mjs first`); process.exit(1); }

let total = 0, viaQuill = 0, viaOwn = 0, refused = 0, scenarios = 0, bypassed = 0;
const refusals = [], ownTools = {};
for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.json') && !x.startsWith('_')).sort()) {
  const r = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
  const calls = r.turns.flatMap(t => t.steps.filter(s => s.kind === 'tool'));
  const own = calls.filter(s => !/quill/i.test(s.tool ?? ''));
  scenarios += 1;
  total += calls.length;
  viaQuill += calls.length - own.length;
  viaOwn += own.length;
  if (own.length) { bypassed += 1; ownTools[r.id] = [...new Set(own.map(s => s.tool))]; }
  for (const s of calls.filter(s => s.error)) {
    refused += 1;
    refusals.push([r.id, s.tool, JSON.stringify(s.input).slice(0, 90), s.error.replace(/\s+/g, ' ').slice(0, 130)]);
  }
}

const pct = n => total ? `${(n / total * 100).toFixed(0)}%` : '—';
console.log(`scenarios            ${scenarios}`);
console.log(`tool calls           ${total}`);
console.log(`  through Quill      ${viaQuill}  ${pct(viaQuill)}`);
console.log(`  the agent's own    ${viaOwn}  ${pct(viaOwn)}   <- the number to drive down`);
console.log(`refused calls        ${refused}`);
console.log(`scenarios that went round Quill   ${bypassed} of ${scenarios}`);
if (Object.keys(ownTools).length) {
  console.log('\nwhere it went round:');
  for (const [k, v] of Object.entries(ownTools)) console.log(`  ${k.padEnd(18)} ${v.join(', ')}`);
}
if (refusals.length) {
  console.log('\nrefusals:');
  for (const [id, tool, input, msg] of refusals) console.log(`  ${id} | ${tool} ${input}\n      ${msg}`);
}
