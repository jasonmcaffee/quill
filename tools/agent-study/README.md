# Watching an agent drive Unluminate

Unluminate's contract is that everything a person can do, an agent can do too. The tests in the tree prove
an agent **can**. This folder is how you find out whether it **does** — which is a different question,
and the one `task-1695` was filed about after an agent turned out to be doing 24% of its work with
`grep` and `bash` in a window that had a command for every job.

It drives a real agent, against a real window, through instructions phrased the way a person speaks —
*"I want main.rs on the left and shapes.rs on the right"* — and grades what happened by reading
Unluminate's own state back, never by believing what the agent said it did.

## What has to be standing up

1. **A model with tool calling.** The study was run against a local Qwen 3.8 27B on llama.cpp:

   ```sh
   llama-server.exe -m Qwen3.8-27B-IQ4_XS.gguf --mmproj mmproj-F16.gguf \
     -ngl 9999 --host 127.0.0.1 --port 8087 -c 96000 -fa on -ctk q8_0 -ctv q8_0 --jinja
   ```

   Any model `opencode` can reach works; set `STUDY_MODEL` to its `provider/model`.

2. **A config that equips the Unluminate MCP**, pointed at by `OPENCODE_CONFIG`. Use a file of the study's
   own rather than editing the machine's — then equipping and unequipping changes nothing on disk:

   ```json
   {
     "provider": { "qwen38-study": { "npm": "@ai-sdk/openai-compatible",
       "options": { "baseURL": "http://localhost:8087/v1" },
       "models": { "qwen38-27b": { "limit": { "context": 96000, "output": 16000 } } } } },
     "mcp": { "unluminate": { "type": "local", "enabled": true,
       "command": ["<repo>/target/release/unluminate-cli.exe", "mcp", "serve"] } }
   }
   ```

3. **A Unluminate window open on the sample project**, and `unluminate-cli` built:

   ```sh
   cargo build --release -p unluminate-cli -p unluminate-app
   node tools/agent-study/make-sample-project.mjs
   ./target/release/unluminate-cli.exe launch _agent_output/agent-study/scratch-project --no-wait
   ```

## Running it

```sh
node tools/agent-study/run-all.mjs                      # every scenario
node tools/agent-study/run-all.mjs s08-debug s12-rename  # just these
node tools/agent-study/grade.mjs                         # the numbers
```

Everything lands in `_agent_output/agent-study/sessions/` — one `.md` a scenario to read, one `.json`
to grade, and the raw event stream beside them.

## What a scenario is

```json
{ "id": "s08-debug", "area": "debug", "name": "Breakpoint, debug, inspect a variable",
  "expect": "Adds a breakpoint on the println line, starts the debugger, reads `total`.",
  "before": { "breakpoints": ["debug", "breakpoint", "list"] },
  "after":  { "debug": ["debug", "status"] },
  "prompts": ["Put a breakpoint in src/main.rs on the line that prints the total area, start the
               debugger, and when it stops there tell me what the value of total is."] }
```

`before` and `after` are `unluminate-cli` commands run either side of the conversation. They are the point:
they are what makes the transcript a measurement rather than a story. `prompts` may hold several
turns, which are run in one session.

**Add a scenario when you add a feature.** A feature nobody has watched an agent use is a feature
nobody knows is reachable in practice.

## Reading the result

`grade.mjs` prints the number that matters:

```
tool calls           126
  through Unluminate      96  76%
  the agent's own    30  24%   <- the number to drive down
refused calls        9
scenarios that went round Unluminate   13 of 23
```

Two things to look at in the transcripts, both of which the first run found:

- **Every refusal.** Each one is an agent guessing a name Unluminate does not accept. The refusal messages
  are good and it self-corrects, but every one is a wasted round trip that will happen to every agent.
- **Every non-Unluminate tool call.** Each is a job Unluminate either cannot do, or can do and did not get
  asked to. The second kind is the more interesting, and it is usually a naming or a payload problem
  rather than a missing feature.

`tasks/task-1695-agent-study.md` is the first run, with the transcripts quoted and the tickets it
produced.
