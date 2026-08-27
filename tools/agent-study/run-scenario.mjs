// Drives an agent through one scenario of user-phrased instructions against a real Quill
// window, capturing every tool call, argument and refusal so the session can be graded.
//
// The point is that nothing here believes what the agent said it did: each scenario names
// `quill-cli` commands to run before and after, and those answers are what the grading reads.
//
// See README.md in this folder for how to run it and what has to be standing up first.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const REPO = path.resolve(import.meta.dirname, '..', '..');
const OUT = process.env.STUDY_OUT ?? path.join(REPO, '_agent_output', 'agent-study');
const PROJECT = process.env.STUDY_PROJECT ?? path.join(OUT, 'scratch-project');
const MODEL = process.env.STUDY_MODEL ?? 'qwen38-study/qwen38-27b';
const CONFIG = process.env.OPENCODE_CONFIG ?? path.join(OUT, 'study-opencode.json');
const CLI = process.env.QUILL_CLI ?? path.join(REPO, 'target', 'release', 'quill-cli.exe');
const TURN_LIMIT_MS = Number(process.env.STUDY_TURN_LIMIT ?? 900000);

/** Runs one turn of the conversation and returns the raw event stream. */
function turn(message, sessionId) {
  return new Promise((resolve) => {
    const args = ['run', '--dir', PROJECT, '-m', MODEL, '--format', 'json', '--auto'];
    if (sessionId) args.push('-s', sessionId);
    args.push(message);
    // stdin is ignored rather than piped: opencode's `run` waits on stdin for EOF, and a pipe
    // nobody writes to never gives it one. The study's own config file is passed rather than
    // the machine's being edited, so equipping and unequipping the MCP changes nothing on disk.
    const p = spawn('opencode', args, {
      shell: true, cwd: PROJECT, stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, OPENCODE_CONFIG: CONFIG },
    });
    let out = '', err = '';
    const timer = setTimeout(() => { try { p.kill(); } catch {} }, TURN_LIMIT_MS);
    p.stdout.on('data', d => out += d);
    p.stderr.on('data', d => err += d);
    // Wait on exit, not close: the agent leaves its MCP subprocess holding the inherited stdout
    // pipe, so close never fires. A short drain catches the tail of the stream.
    p.on('exit', () => { clearTimeout(timer); setTimeout(() => resolve({ out, err }), 800); });
  });
}

/** Keeps the few things worth grading out of the agent's event stream. */
export function distil(raw) {
  const events = [];
  for (const line of raw.split('\n')) {
    const t = line.trim();
    if (!t.startsWith('{')) continue;
    try { events.push(JSON.parse(t)); } catch {}
  }
  const steps = [];
  for (const e of events) {
    if (e.type === 'tool_use') {
      const s = e.part?.state ?? {};
      steps.push({
        kind: 'tool', tool: e.part?.tool, status: s.status, input: s.input,
        output: typeof s.output === 'string' ? s.output : null,
        // A refused call carries its message in `error` and nothing in `output`. What the agent
        // was actually told is the whole point of grading, so it is kept as text.
        error: typeof s.error === 'string' ? s.error : (s.error ? JSON.stringify(s.error) : null),
      });
    } else if (e.type === 'text' && e.part?.text) steps.push({ kind: 'text', text: e.part.text });
    else if (e.type === 'error') steps.push({ kind: 'error', detail: JSON.stringify(e.error) });
  }
  return {
    sessionID: events.find(e => e.sessionID)?.sessionID ?? null,
    steps,
    tokens: events.filter(e => e.type === 'step_finish').pop()?.part?.tokens ?? null,
  };
}

/** Asks Quill what is really true, so a scenario is checked against the window rather than the agent. */
function quill(cmd) {
  return new Promise((resolve) => {
    const p = spawn(CLI, [...cmd, '--json'], { shell: false });
    let out = '';
    p.stdout.on('data', d => out += d);
    p.stderr.on('data', d => out += d);
    p.on('close', () => resolve(out.trim()));
  });
}

/** The readable transcript, which is what a person or a grader actually reads. */
export function transcript(rec) {
  let md = `# ${rec.id} — ${rec.name}\n\nArea: **${rec.area}**\n\n`;
  if (rec.expect) md += `Expected: ${rec.expect}\n\n`;
  for (const [k, v] of Object.entries(rec.before ?? {})) md += `### before: ${k}\n\`\`\`\n${String(v).slice(0, 1200)}\n\`\`\`\n\n`;
  for (const t of rec.turns) {
    md += `---\n\n## USER (${t.seconds}s)\n\n> ${t.user}\n\n`;
    for (const s of t.steps) {
      if (s.kind === 'tool') {
        md += `**TOOL** \`${s.tool}\` [${s.status}]\ninput: \`${JSON.stringify(s.input)}\`\n`;
        if (s.output) md += `output:\n\`\`\`\n${s.output.slice(0, 1800)}\n\`\`\`\n`;
        if (s.error) md += `**REFUSED**: ${s.error.slice(0, 900)}\n`;
        md += `\n`;
      } else if (s.kind === 'text') md += `**AGENT**: ${s.text}\n\n`;
      else md += `**STREAM ERROR**: ${s.detail}\n\n`;
    }
  }
  for (const [k, v] of Object.entries(rec.after ?? {})) md += `### after: ${k}\n\`\`\`\n${String(v).slice(0, 2000)}\n\`\`\`\n\n`;
  return md;
}

if (process.argv[2]) {
  const scenario = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  const dir = path.join(OUT, 'sessions');
  fs.mkdirSync(dir, { recursive: true });

  const rec = { id: scenario.id, area: scenario.area, name: scenario.name, expect: scenario.expect, turns: [], before: {}, after: {} };
  for (const [k, c] of Object.entries(scenario.before ?? {})) rec.before[k] = await quill(c);

  let sid = null;
  for (const message of scenario.prompts) {
    const started = Date.now();
    const { out, err } = await turn(message, sid);
    const d = distil(out);
    sid = sid ?? d.sessionID;
    rec.turns.push({ user: message, seconds: Math.round((Date.now() - started) / 1000), ...d, stderr: err.slice(-2000) });
    fs.writeFileSync(path.join(dir, `${scenario.id}.raw.jsonl`), out);
  }
  for (const [k, c] of Object.entries(scenario.after ?? {})) rec.after[k] = await quill(c);

  fs.writeFileSync(path.join(dir, `${scenario.id}.json`), JSON.stringify(rec, null, 1));
  fs.writeFileSync(path.join(dir, `${scenario.id}.md`), transcript(rec));
  const calls = rec.turns.reduce((a, t) => a + t.steps.filter(s => s.kind === 'tool').length, 0);
  const refused = rec.turns.reduce((a, t) => a + t.steps.filter(s => s.error).length, 0);
  console.log(`${scenario.id}: ${rec.turns.length} turn(s), ${calls} tool calls, ${refused} refused, ${rec.turns.reduce((a, t) => a + t.seconds, 0)}s`);
}
