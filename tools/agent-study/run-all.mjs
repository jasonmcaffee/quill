// Runs every scenario, or the ones named on the command line, resetting Unluminous to a known
// state before each so one cannot contaminate the next.
//
//   node tools/agent-study/run-all.mjs                    # all of them
//   node tools/agent-study/run-all.mjs s08-debug s12-rename
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const HERE = import.meta.dirname;
const REPO = path.resolve(HERE, '..', '..');
const OUT = process.env.STUDY_OUT ?? path.join(REPO, '_agent_output', 'agent-study');
const all = JSON.parse(fs.readFileSync(path.join(HERE, 'scenarios.json'), 'utf8'));
const want = process.argv.slice(2);
const list = want.length ? all.filter(s => want.includes(s.id)) : all;
if (!list.length) { console.log('no scenarios matched'); process.exit(1); }

fs.mkdirSync(OUT, { recursive: true });
for (const s of list) {
  spawnSync('bash', [path.join(HERE, 'reset.sh')], { encoding: 'utf8' });
  const f = path.join(OUT, '_one.json');
  fs.writeFileSync(f, JSON.stringify(s));
  const r = spawnSync('node', [path.join(HERE, 'run-scenario.mjs'), f], { encoding: 'utf8', stdio: 'inherit' });
  if (r.error) console.log(s.id, 'FAILED TO RUN', r.error.message);
}
