// Measure how well a local model drives Unluminous when it is given only the CLI documentation.
//
//   node unluminous-cli/agent-assessment/run.js --url http://localhost:8087/v1/chat/completions \
//        --model qwen3.8-27b --out _agent_output/task-1661-unluminous-cli/assessment
//
// What it does, per task:
//
//   1. puts the window back into a known state, and runs the task's own setup;
//   2. asks the model, giving it the system prompt, the whole of `docs/commands.md`, and the
//      instruction phrased the way a person would say it;
//   3. reads the command lines out of the answer;
//   4. runs them against the live Unluminous;
//   5. checks the window really ended up as the instruction asked, or — for a task that only asks
//      for information — checks that the right command was chosen, by running it under `--dry-run`.
//
// A task passes only if every step does. Nothing is graded by a model: every check is a predicate
// over what the window actually reported afterwards.

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const { tasks, fresh } = require('./tasks.js');
const project = require('./project.js');

const REPO = path.resolve(__dirname, '..', '..');
const DOCS = path.join(REPO, 'unluminous-cli', 'docs', 'commands.md');
const CLI = process.env.UNLUMINOUS_CLI || path.join(REPO, 'target', 'release', 'unluminous-cli.exe');

/** The system prompt: what the model is, and the shape its answer must take. */
function systemPrompt(documentation) {
  return `You drive the Unluminous text editor from a command line tool called unluminous-cli.

A person tells you what they want in plain English. You answer with the unluminous-cli command line
that does it, and with nothing else.

Rules for your answer:
- Output ONLY command lines, one per line. No prose, no explanation, no markdown, no code fences.
- Every command begins with the word unluminous-cli.
- Add --json to every command.
- Use one command where one will do. Use several lines only when the task genuinely needs them.
- Never invent a command, a flag or a setting name. Everything you may use is in the documentation
  below. If you are unsure which command it is, re-read the reference section for that area.
- Paths are relative to the project folder unless they are absolute.

Here is the complete documentation for unluminous-cli.

${documentation}`;
}

function ask(url, model, documentation, say, temperature) {
  const body = JSON.stringify({
    model,
    messages: [
      { role: 'system', content: systemPrompt(documentation) },
      { role: 'user', content: say },
    ],
    temperature,
    top_p: temperature > 0 ? 0.95 : 1,
    max_tokens: 900,
  });
  const out = execFileSync(
    'curl',
    ['-s', '--max-time', '300', url, '-H', 'Content-Type: application/json', '--data-binary', '@-'],
    { input: body, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
  );
  let reply;
  try {
    reply = JSON.parse(out);
  } catch (problem) {
    throw new Error(`the model did not answer with JSON: ${out.slice(0, 300)}`);
  }
  const choice = reply.choices && reply.choices[0];
  if (!choice) throw new Error(`no answer: ${out.slice(0, 300)}`);
  return (choice.message.content || '').trim();
}

/** The command lines in an answer, with fences and stray prose stripped. */
function commandsIn(answer) {
  return answer
    .split('\n')
    .map((line) => line.trim())
    .map((line) => line.replace(/^```.*$/, ''))
    .map((line) => line.replace(/^\$\s*/, ''))
    .map((line) => line.replace(/^[-*]\s+/, ''))
    .filter((line) => line.startsWith('unluminous-cli'))
    .map((line) => line.slice('unluminous-cli'.length).trim());
}

/** Split a command line the way a shell would, honouring quotes. */
function words(line) {
  const out = [];
  let word = '';
  let quote = null;
  for (const character of line) {
    if (quote) {
      if (character === quote) quote = null;
      else word += character;
    } else if (character === '"' || character === "'") {
      quote = character;
    } else if (/\s/.test(character)) {
      if (word) {
        out.push(word);
        word = '';
      }
    } else {
      word += character;
    }
  }
  if (word) out.push(word);
  return out;
}

/** Run unluminous-cli and return the parsed reply, whatever the exit code. */
function cli(argv, { quiet = false } = {}) {
  const full = argv.includes('--json') ? argv : argv.concat(['--json']);
  try {
    const out = execFileSync(CLI, full, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
    return { code: 0, reply: JSON.parse(out) };
  } catch (problem) {
    const text = (problem.stdout || '') + (problem.stderr || '');
    let reply = null;
    try {
      reply = JSON.parse(text);
    } catch (_) {
      /* the client refused before it could answer in JSON */
    }
    if (!quiet && !reply) {
      return { code: problem.status === undefined ? -1 : problem.status, reply: null, text };
    }
    return { code: problem.status === undefined ? -1 : problem.status, reply, text };
  }
}

function reset(task, folder) {
  // Every terminal tab goes first. Two tasks each start one and nothing closes them, so without
  // this a later task lands in whichever shell was left over — including the one a task deliberately
  // left with a half-typed command on its prompt.
  for (let guard = 0; guard < 12; guard += 1) {
    const { reply } = cli(['terminal', 'list'], { quiet: true });
    if (!reply || !reply.ok || !reply.result.count) break;
    cli(['terminal', 'close'], { quiet: true });
  }
  for (const argv of fresh(folder)) cli(argv, { quiet: true });
  for (const argv of (task.setup || [])) cli(argv, { quiet: true });
}

function matchesExpected(expected, wire) {
  const wanted = Array.isArray(expected) ? expected : [expected];
  return wanted.includes(wire);
}

function grade(task, lines) {
  if (lines.length === 0) {
    return { pass: false, why: 'the model produced no command' };
  }
  if (!task.allowMany && !task.check && lines.length > 3) {
    return { pass: false, why: `${lines.length} commands for a task that needs one` };
  }

  // What each line would send, without sending it. This is how a task that only asks for
  // information is checked: there is no state to look at afterwards, so what is checked is that
  // the right command was chosen with the right arguments.
  const planned = lines.map((line) => {
    const { reply } = cli(words(line).concat(['--dry-run']), { quiet: true });
    return reply && reply.ok ? reply : null;
  });
  if (planned.some((plan) => plan === null)) {
    const at = planned.findIndex((plan) => plan === null);
    return { pass: false, why: `line ${at + 1} is not a valid command: unluminous-cli ${lines[at]}` };
  }

  if (task.expectCommand) {
    const found = planned.find((plan) => matchesExpected(task.expectCommand, plan.command));
    if (!found) {
      return {
        pass: false,
        why: `chose ${planned.map((p) => p.command).join(', ')}, wanted ${
          Array.isArray(task.expectCommand) ? task.expectCommand.join(' or ') : task.expectCommand
        }`,
      };
    }
    if (task.expectArguments && !task.expectArguments(found.arguments || {})) {
      return {
        pass: false,
        why: `${found.command} was right but its arguments were ${JSON.stringify(found.arguments)}`,
      };
    }
  }

  // Now run them for real.
  for (const line of lines) {
    const { code, reply } = cli(words(line), { quiet: true });
    if (code !== 0) {
      const why = reply && reply.error ? reply.error.message : `exit ${code}`;
      return { pass: false, why: `unluminous-cli ${line} failed: ${why}` };
    }
  }

  if (task.check) {
    const { reply } = cli(task.check.run, { quiet: true });
    if (!reply || !reply.ok) {
      return { pass: false, why: `the check command itself failed: ${JSON.stringify(reply)}` };
    }
    let passed = false;
    try {
      passed = !!task.check.test(reply.result);
    } catch (problem) {
      return { pass: false, why: `the check threw: ${problem.message}` };
    }
    if (!passed) {
      return {
        pass: false,
        why: `the window did not end up as asked: ${JSON.stringify(reply.result).slice(0, 400)}`,
      };
    }
  }
  return { pass: true, why: '' };
}

function main() {
  const argv = process.argv.slice(2);
  const flag = (name, fallback) => {
    const at = argv.indexOf(`--${name}`);
    return at >= 0 ? argv[at + 1] : fallback;
  };
  const url = flag('url', 'http://localhost:8087/v1/chat/completions');
  const model = flag('model', 'qwen3.8-27b');
  const out = flag('out', path.join(REPO, '_agent_output', 'task-1661-unluminous-cli', 'assessment'));
  const only = flag('only', null);
  const folder = flag(
    'project',
    path.join(REPO, '_agent_output', 'task-1661-unluminous-cli', 'assessment-project'),
  );
  const label = flag('label', 'run');
  const temperature = Number(flag('temperature', '0'));
  fs.mkdirSync(out, { recursive: true });

  // The sample project is written out again first, so a task that edited or added a file cannot
  // change what the next round is measuring.
  console.log(`sample project: ${project.write(folder)} files in ${folder}`);
  const { reply: rooted } = cli(['project', 'open', folder], { quiet: true });
  if (!rooted || !rooted.ok) {
    console.error('could not point the running Unluminous at the sample project; is one running?');
    process.exit(2);
  }

  // The control. A score only means something if the test can fail, so `--no-docs` runs the same 64
  // instructions with the reference taken away and nothing but the tool's name left. What survives
  // is what the model already knew or could guess; the difference between the two numbers is what
  // the documentation is worth.
  const withoutDocs = argv.includes('--no-docs');
  const documentation = withoutDocs
    ? 'There is no documentation available. Work out the commands yourself.'
    : fs.readFileSync(DOCS, 'utf8');
  const chosen = only ? tasks.filter((t) => t.id === only) : tasks;
  const results = [];
  let passed = 0;

  console.log(`${chosen.length} tasks, model ${model} at ${url}, temperature ${temperature}`);
  console.log(`documentation: ${documentation.length} characters\n`);

  for (const [at, task] of chosen.entries()) {
    reset(task, folder);
    let answer = '';
    let outcome;
    const began = Date.now();
    try {
      answer = ask(url, model, documentation, task.say, temperature);
      outcome = grade(task, commandsIn(answer));
    } catch (problem) {
      outcome = { pass: false, why: `asking the model failed: ${problem.message}` };
    }
    const seconds = ((Date.now() - began) / 1000).toFixed(1);
    if (outcome.pass) passed += 1;
    results.push({
      id: task.id,
      say: task.say,
      answer,
      commands: commandsIn(answer),
      pass: outcome.pass,
      why: outcome.why,
      seconds: Number(seconds),
    });
    const mark = outcome.pass ? 'PASS' : 'FAIL';
    console.log(
      `${String(at + 1).padStart(2)}/${chosen.length} ${mark} ${task.id.padEnd(28)} ${seconds}s ${
        outcome.pass ? '' : '\n        ' + outcome.why
      }`,
    );
    if (!outcome.pass) {
      console.log('        answered: ' + answer.replace(/\n/g, ' | ').slice(0, 220));
    }
  }

  const score = (passed / chosen.length) * 100;
  const summary = {
    model,
    url,
    label,
    temperature,
    tasks: chosen.length,
    passed,
    failed: chosen.length - passed,
    score: Number(score.toFixed(2)),
    documentationCharacters: documentation.length,
    withoutDocs,
    results,
  };
  const file = path.join(out, `${label}.json`);
  fs.writeFileSync(file, JSON.stringify(summary, null, 2), 'utf8');
  console.log(`\n${passed}/${chosen.length} = ${score.toFixed(2)}%   ->  ${file}`);
  process.exit(score >= 97 ? 0 : 1);
}

main();
