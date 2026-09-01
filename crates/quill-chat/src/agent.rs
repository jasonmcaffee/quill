//! Talking to a command-line agent: `claude` and `codex`, run as a child process.
//!
//! **This is the transport the ticket asks for.** *"connection to Claude and codex etc through
//! cli"* — so the pane runs the agent that is already installed and already signed in rather than
//! sending to the API behind it. Four things follow from that, and each of them is a reason this is
//! the better half of the feature rather than a cheaper one:
//!
//! - **Quill holds no key.** There is nothing to put in a settings file, nothing to read out of an
//!   environment variable, nothing to redact out of an error message. The agent's own credentials
//!   are the agent's own business.
//! - **The agent brings its own tools**, its own sandbox and its own permission model. Quill does not
//!   hand it a tool catalogue and does not run anything on its behalf, so the whole question of what
//!   a model may do to this machine is answered by the program a person already trusts with it.
//! - **It reads the project.** Started in the folder the window has open, `claude` finds that
//!   project's `CLAUDE.md` and `codex` its `AGENTS.md`, so the answer is about the code in front of
//!   you without a word of it being uploaded by Quill.
//! - **The conversation is the agent's.** A second question is `--resume <session>` rather than the
//!   whole transcript sent again, so the context the agent has built is the context it keeps.
//!
//! ## Two shapes, and one of them is already understood
//!
//! `claude -p --output-format stream-json --include-partial-messages` emits one JSON object a line,
//! and the interesting ones carry `event`, which is **the Anthropic wire verbatim** — the same
//! `content_block_delta`, `input_json_delta` and `message_delta` that `/v1/messages` sends. So the
//! decoder that already reads that API reads this too, one level down, and thinking blocks, tool
//! calls and token deltas all arrive with no new code. What is added on top is the envelope:
//! `system` with the session id, `user` carrying the results of the tools the agent ran itself, and
//! `result` ending the turn with what it cost.
//!
//! `codex exec --json` is a different model and gets its own reading: a thread of **items** that are
//! started, updated and completed, where a shell command the agent ran is an item beside the words
//! it said. There are no token deltas — an item arrives whole — so an answer appears a paragraph at
//! a time rather than a word at a time, which is the agent's own shape and not something to paper
//! over.
//!
//! Both are read into the same [`crate::wire::Reply`] values, so the pane has never heard of either.
//!
//! ## The prompt goes down standard input
//!
//! Both agents read their instructions from standard input when none is given as an argument, and
//! that is how Quill sends one — not as the last word of the command line. **Because a batch file
//! cannot take a multi-line argument**: npm installs `codex` on Windows as `codex.cmd`, and Rust's
//! own `Command` refuses to spawn a batch file with an argument it cannot safely escape, answering
//! *"batch file arguments are invalid"*. Quill's first line tells the agent it is answering in a
//! pane, so every prompt has a blank line in it and every `codex` turn failed. Measured on this
//! machine, after the run before it had failed for a different reason in the same place.
//!
//! It is better on every platform besides: a command line has a length limit — 32,767 characters on
//! Windows — and a person pasting a stack trace into the composer would reach it.
//!
//! ## The child is killed, not asked to stop — and the kill comes from outside the reader
//!
//! Stopping is what the composer's stop button does, and an agent in the middle of a turn has no
//! protocol message for it. The process is killed and its pipes are dropped, which is the same thing
//! `quill_terminal::Session::kill` does and for the same reason: the alternative is a window that
//! goes on being billed for an answer nobody will read.
//!
//! **The kill cannot come from the thread that is reading it.** That thread is asleep inside a read
//! on the agent's standard output and will not look at a flag until the next line arrives — which,
//! for an agent thinking about a long answer, is not soon. Measured: `stop` put the pane back to
//! `finished` at once, kept what had arrived, and left the child running. So the [`Running`] handle
//! is shared with whoever started the turn, and stopping kills through it; the blocked read then
//! ends because the pipe has closed, which is what unwedges the thread as well.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::model::Conversation;
use crate::provider::{Provider, Wire};
use crate::wire::Reply;

/// The child of a turn that is running, so that stopping can reach it.
///
/// Empty before the agent has been started and after it has ended, which is what makes
/// [`Running::stop`] safe to call at any moment — including on a turn that never started, which is
/// what the stop button does when somebody presses it twice.
#[derive(Default)]
pub struct Running(Mutex<Option<Child>>);

impl Running {
    /// Kill whatever is running, if anything is.
    ///
    /// The pipes go with it, so a thread asleep in a read on the agent's output wakes at once with
    /// the end of its input — which is the half that matters, and the reason this is not simply a
    /// flag.
    pub fn stop(&self) {
        if let Ok(mut held) = self.0.lock() {
            if let Some(child) = held.as_mut() {
                let _ = child.kill();
            }
        }
    }

    fn hold(&self, child: Child) {
        if let Ok(mut held) = self.0.lock() {
            *held = Some(child);
        }
    }

    fn take(&self) -> Option<Child> {
        self.0.lock().ok().and_then(|mut held| held.take())
    }
}

impl std::fmt::Debug for Running {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("Running")
    }
}

/// What one turn asks a command-line agent.
///
/// Built by the caller because it knows things this crate does not: which folder the window has
/// open, and what the person actually typed.
#[derive(Debug, Clone, Default)]
pub struct Ask {
    /// What to say. One turn's worth: the transcript is the agent's own.
    pub prompt: String,
    /// The folder the agent runs in, which is the project the window has open.
    pub folder: Option<std::path::PathBuf>,
    /// The agent's own session, to carry the conversation on. Empty for the first turn.
    pub session: String,
    /// Pictures written to files, because both agents take an attachment by path.
    pub pictures: Vec<std::path::PathBuf>,
    /// How much the agent may do without being asked.
    pub permission: Permission,
}

/// How much a command-line agent may do to this machine without asking.
///
/// **The one setting that matters, and it is deliberately the person's.** An agent run with
/// `--print` cannot stop and ask a question, so what it may do has to be decided before it starts.
/// Three values rather than each agent's own vocabulary, because a person choosing between "read
/// only" and "may edit" should not have to know that one of them calls it a sandbox and the other a
/// permission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Permission {
    /// Read and think, change nothing. Both agents' own safest setting, and the default.
    #[default]
    Read,
    /// May edit files in the project it was started in.
    Edit,
    /// May do anything, including run commands with no sandbox.
    Full,
}

impl Permission {
    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Full => "full",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "read" => Some(Self::Read),
            "edit" => Some(Self::Edit),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// Every value [`Permission`] has, for a settings page and for a configuration that names one.
pub const PERMISSIONS: &[&str] = &["read", "edit", "full"];

/// The command line one turn is, as the program and its arguments.
///
/// Split out from [`run`] so a test can assert on what would be run without running it — which is
/// the only way to test the flags at all, since a real agent's answer is not something a test can
/// know. The arguments are **built here rather than configured**, so a row in a settings file cannot
/// turn into a command line that means something else.
pub fn command_line(provider: &Provider, ask: &Ask) -> Vec<String> {
    let mut out = Vec::new();
    match provider.wire {
        Wire::ClaudeCli => {
            out.push("--print".to_owned());
            out.push("--output-format".to_owned());
            out.push("stream-json".to_owned());
            // Without it an answer arrives whole, once, when the turn ends — which is the one thing
            // the ticket asks for by name.
            out.push("--include-partial-messages".to_owned());
            // `stream-json` refuses to run without it.
            out.push("--verbose".to_owned());
            out.push("--permission-mode".to_owned());
            out.push(
                match ask.permission {
                    // Its own name for "ask me", which in `--print` means "do not do it".
                    Permission::Read => "manual",
                    Permission::Edit => "acceptEdits",
                    Permission::Full => "bypassPermissions",
                }
                .to_owned(),
            );
            if !provider.model.trim().is_empty() {
                out.push("--model".to_owned());
                out.push(provider.model.trim().to_owned());
            }
            if !ask.session.trim().is_empty() {
                out.push("--resume".to_owned());
                out.push(ask.session.trim().to_owned());
            }
        }
        Wire::CodexCli => {
            out.push("exec".to_owned());
            let resuming = !ask.session.trim().is_empty();
            if resuming {
                out.push("resume".to_owned());
                out.push(ask.session.trim().to_owned());
            }
            out.push("--json".to_owned());
            // A pane opened on a folder that is not a repository is an ordinary thing, and codex
            // refuses to start in one without this.
            out.push("--skip-git-repo-check".to_owned());
            match ask.permission {
                // **`codex exec resume` has no `--sandbox`**, though `codex exec` does — measured,
                // after every second question failed with *"unexpected argument '--sandbox'"*. What
                // resume does take is `-c`, its own configuration override, so the sandbox is named
                // that way there. The same value either way, said in the two vocabularies one
                // program has for it.
                Permission::Read | Permission::Edit => {
                    let mode = match ask.permission {
                        Permission::Edit => "workspace-write",
                        _ => "read-only",
                    };
                    match resuming {
                        true => {
                            out.push("-c".to_owned());
                            out.push(format!("sandbox_mode=\"{mode}\""));
                        }
                        false => {
                            out.push("--sandbox".to_owned());
                            out.push(mode.to_owned());
                        }
                    }
                }
                // Its own name, and it is a long one on purpose. Both forms take this one.
                Permission::Full => out.push("--dangerously-bypass-approvals-and-sandbox".to_owned()),
            }
            if !provider.model.trim().is_empty() {
                out.push("--model".to_owned());
                out.push(provider.model.trim().to_owned());
            }
            for picture in &ask.pictures {
                out.push("--image".to_owned());
                out.push(picture.display().to_string());
            }
        }
        // Not a program. A caller that asks anyway gets an empty command line rather than a panic,
        // and `run` refuses it by name.
        Wire::OpenAi | Wire::Anthropic | Wire::Responses => {}
    }
    out
}

/// What is written to the agent's standard input: the words, and any picture it takes no other way.
///
/// Claude Code has no `--image`: it reads a picture when it is given the path of one, which is what
/// a person does in its own prompt box. So the paths go in the words rather than being dropped, and
/// they are named as attachments so the agent is not left guessing why a path is there. Codex has
/// `--image`, so its paths are already on the command line and are not repeated here.
pub fn prompt_for(provider: &Provider, ask: &Ask) -> String {
    if ask.pictures.is_empty() || provider.wire == Wire::CodexCli {
        return ask.prompt.clone();
    }
    let named: Vec<String> = ask
        .pictures
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    match ask.prompt.trim().is_empty() {
        true => format!("Look at these images: {}", named.join(", ")),
        false => format!("{}\n\nAttached images: {}", ask.prompt, named.join(", ")),
    }
}

/// Run one turn against a command-line agent, handing each reply to `on_reply`.
///
/// Answers `false` from `on_reply` to stop, which is what a request the pane has moved on from does
/// — the same shape [`crate::client::run`] uses for an HTTP one, so `Client` does not care which
/// transport a provider has.
pub fn run(
    provider: &Provider,
    ask: &Ask,
    stopping: &AtomicBool,
    running: &Running,
    on_reply: &dyn Fn(Reply) -> bool,
) {
    let Some(program) = provider.program_path() else {
        on_reply(Reply::Failed(
            provider
                .why_not()
                .unwrap_or_else(|| format!("`{}` could not be run.", provider.command)),
        ));
        return;
    };
    let arguments = command_line(provider, ask);
    let mut command = Command::new(&program);
    command.args(&arguments);
    if let Some(folder) = &ask.folder {
        command.current_dir(folder);
    }
    // **The prompt goes down standard input**, for the reason at the top of this file: a batch file
    // cannot take a multi-line argument, and `codex` is a batch file on Windows. Inheriting the
    // window's own would be worse than either, since there is nothing on the other end of it.
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // No console window, which is what `quill_git` does for the same reason: a flashing black
        // rectangle every time somebody asks a question is not a feature.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(problem) => {
            on_reply(Reply::Failed(format!(
                "{} could not be started: {problem}",
                program.display()
            )));
            return;
        }
    };
    // Written and then **closed**, which is the half that matters: both agents read until the end
    // of their input, so a pipe left open is an agent that waits for ever.
    if let Some(mut input) = child.stdin.take() {
        use std::io::Write;
        let prompt = prompt_for(provider, ask);
        if let Err(problem) = input.write_all(prompt.as_bytes()).and_then(|()| input.flush()) {
            end(&mut child);
            on_reply(Reply::Failed(format!(
                "{} would not take the question: {problem}",
                provider.command.trim()
            )));
            return;
        }
    }
    let Some(output) = child.stdout.take() else {
        let _ = child.kill();
        on_reply(Reply::Failed("the agent gave nothing to read.".to_owned()));
        return;
    };
    // **Handed over before a byte is read**, so a stop that arrives while the first line is still
    // coming has something to kill. `stopping` was already set by then, so a turn stopped in that
    // instant is killed here as well rather than being started and left.
    if stopping.load(Ordering::Relaxed) {
        end(&mut child);
        return;
    }
    // **Read on a thread of its own**, because a program that says nothing on standard error must
    // not fill a pipe nobody is emptying and stop writing to the one that is being read. What it
    // says is kept for the refusal, which is `quill-git`'s rule about git's own stderr.
    let said = child.stderr.take().map(|errors| {
        std::thread::spawn(move || {
            let mut all = String::new();
            for line in BufReader::new(errors).lines().map_while(Result::ok) {
                if all.len() < 8000 {
                    all.push_str(&line);
                    all.push('\n');
                }
            }
            all
        })
    });

    let said = said;
    let mut decoder = Decoder::new(provider.wire);
    let mut carried_on = true;
    running.hold(child);
    for line in BufReader::new(output).lines() {
        if stopping.load(Ordering::Relaxed) {
            carried_on = false;
            break;
        }
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        for reply in decoder.line(line) {
            if !on_reply(reply) {
                carried_on = false;
                break;
            }
        }
        if !carried_on {
            break;
        }
    }
    let mut child = match running.take() {
        Some(child) => child,
        // Killed and taken by `Running::stop` — which is exactly what stopping looks like from here.
        None => return,
    };
    if !carried_on {
        end(&mut child);
        return;
    }
    let status = child.wait();
    let errors = said.and_then(|thread| thread.join().ok()).unwrap_or_default();
    // **A turn that ended cleanly has already said so**, through its own last event. What is left
    // here is a program that stopped without one, and the honest thing to report is what it said on
    // standard error — which is the only place `claude` and `codex` put a start-up failure at all.
    for reply in decoder.finish() {
        if !on_reply(reply) {
            return;
        }
    }
    if !decoder.ended {
        let ended = match status {
            Ok(status) if status.success() => "the agent stopped without finishing its answer.".to_owned(),
            Ok(status) => format!(
                "{} stopped with {status}.",
                provider.command.trim()
            ),
            Err(problem) => format!("{} could not be waited for: {problem}", provider.command.trim()),
        };
        let detail = errors.trim();
        on_reply(Reply::Failed(match detail.is_empty() {
            true => ended,
            false => format!("{ended}\n\n{detail}"),
        }));
    }
}

/// Kill a child and reap it, so a stopped turn leaves no process behind.
fn end(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// One agent's stream, read into [`Reply`] values.
///
/// Holds the state a line on its own does not carry: the session id the turn is to be continued
/// with, and — for Codex, whose items arrive whole rather than as deltas — how much of each item has
/// already been reported, so an item that is updated does not repeat what was already shown.
pub struct Decoder {
    wire: Wire,
    /// The Anthropic decoder, for the events `claude` nests inside its own envelope.
    inner: crate::wire::Decoder,
    /// How much of each Codex item has been sent on, by item id.
    sent: Vec<(String, usize)>,
    /// Whether a `result` or a `turn.completed` has been seen, so a child that stops without one is
    /// reported rather than looking like a clean end.
    pub ended: bool,
    /// The agent's own session, which is what the next turn is resumed with.
    pub session: String,
    started: bool,
}

impl Decoder {
    pub fn new(wire: Wire) -> Self {
        Self {
            wire,
            inner: crate::wire::Decoder::new(Wire::Anthropic),
            sent: Vec::new(),
            ended: false,
            session: String::new(),
            started: false,
        }
    }

    /// One line of the agent's output.
    ///
    /// A line that is not JSON is **ignored rather than reported**: both agents print the occasional
    /// human sentence among their events, and a pane that showed a refusal every time one did would
    /// be a pane that cried wolf.
    pub fn line(&mut self, line: &str) -> Vec<Reply> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return Vec::new();
        };
        match self.wire {
            Wire::ClaudeCli => self.claude(&value),
            Wire::CodexCli => self.codex(&value),
            _ => Vec::new(),
        }
    }

    /// Whatever is still being built when the child has stopped.
    pub fn finish(&mut self) -> Vec<Reply> {
        match self.ended {
            true => Vec::new(),
            // The Anthropic decoder's own rule: a stream that stopped mid-answer keeps what arrived
            // and says the connection ended before the answer did.
            false => self.inner.finish(),
        }
    }

    /// Claude Code's envelope, whose `event` is the Anthropic wire verbatim.
    fn claude(&mut self, value: &serde_json::Value) -> Vec<Reply> {
        let mut out = Vec::new();
        if let Some(session) = value["session_id"].as_str() {
            if !session.is_empty() && session != self.session {
                self.session = session.to_owned();
                out.push(Reply::Session(session.to_owned()));
            }
        }
        match value["type"].as_str().unwrap_or_default() {
            "system" => {
                // `init` names the model the session really started with, which is the honest answer
                // to what is in the header chip — a row that names no model gets the agent's own.
                if value["subtype"] == "init" {
                    if let Some(model) = value["model"].as_str().filter(|one| !one.is_empty()) {
                        if !self.started {
                            self.started = true;
                            out.push(Reply::Started { model: model.to_owned() });
                        }
                    }
                }
            }
            "stream_event" => {
                let event = &value["event"];
                if !self.started {
                    if let Some(model) = event["message"]["model"].as_str() {
                        self.started = true;
                        out.push(Reply::Started { model: model.to_owned() });
                    }
                }
                out.extend(self.inner.event(&crate::sse::Event {
                    name: event["type"].as_str().unwrap_or_default().to_owned(),
                    data: event.to_string(),
                }));
            }
            // **The results of the tools the agent ran itself.** Quill did not run them and does not
            // need to: what it does with them is show them, so the block the pane already draws for
            // a tool call is filled in from here.
            "user" => {
                for block in value["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" {
                        out.push(Reply::ToolAnswer {
                            id: block["tool_use_id"].as_str().unwrap_or_default().to_owned(),
                            answer: text_of(&block["content"]),
                            failed: block["is_error"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
            "result" => {
                self.ended = true;
                let usage = &value["usage"];
                let input = usage["input_tokens"].as_u64().unwrap_or(0)
                    + usage["cache_read_input_tokens"].as_u64().unwrap_or(0)
                    + usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                out.push(Reply::Usage {
                    input,
                    output: usage["output_tokens"].as_u64().unwrap_or(0),
                });
                // **An error is the agent's own words**, which is `quill-git`'s rule: it explains
                // itself better than Quill could.
                match value["is_error"].as_bool().unwrap_or(false) {
                    true => out.push(Reply::Failed(
                        value["result"]
                            .as_str()
                            .filter(|said| !said.is_empty())
                            .unwrap_or("the agent stopped with an error.")
                            .to_owned(),
                    )),
                    false => out.push(Reply::Finished {
                        reason: value["subtype"].as_str().unwrap_or("stop").to_owned(),
                    }),
                }
            }
            _ => {}
        }
        out
    }

    /// Codex's thread of items.
    fn codex(&mut self, value: &serde_json::Value) -> Vec<Reply> {
        let mut out = Vec::new();
        match value["type"].as_str().unwrap_or_default() {
            "thread.started" => {
                if let Some(thread) = value["thread_id"].as_str() {
                    self.session = thread.to_owned();
                    out.push(Reply::Session(thread.to_owned()));
                }
                if !self.started {
                    self.started = true;
                    out.push(Reply::Started {
                        model: String::new(),
                    });
                }
            }
            "item.started" | "item.updated" | "item.completed" => {
                out.extend(self.item(&value["item"], value["type"] == "item.completed"));
            }
            "turn.completed" => {
                self.ended = true;
                let usage = &value["usage"];
                out.push(Reply::Usage {
                    input: usage["input_tokens"].as_u64().unwrap_or(0)
                        + usage["cached_input_tokens"].as_u64().unwrap_or(0),
                    output: usage["output_tokens"].as_u64().unwrap_or(0),
                });
                out.push(Reply::Finished {
                    reason: "stop".to_owned(),
                });
            }
            "turn.failed" | "error" => {
                self.ended = true;
                out.push(Reply::Failed(
                    value["error"]["message"]
                        .as_str()
                        .or_else(|| value["message"].as_str())
                        .unwrap_or("the agent stopped with an error.")
                        .to_owned(),
                ));
            }
            _ => {}
        }
        out
    }

    /// One Codex item, reported as only the part of it that has not been reported already.
    ///
    /// **Because an item arrives whole and then arrives again.** `item.updated` carries the item's
    /// text so far and `item.completed` carries all of it, so sending each one on would show the
    /// answer three times over. What is remembered is how many characters of each item have been
    /// passed on, and only the rest is.
    fn item(&mut self, item: &serde_json::Value, completed: bool) -> Vec<Reply> {
        let id = item["id"].as_str().unwrap_or_default().to_owned();
        match item["type"].as_str().unwrap_or_default() {
            "agent_message" => {
                let text = item["text"].as_str().unwrap_or_default();
                self.rest_of(&id, text).map(Reply::Text).into_iter().collect()
            }
            "reasoning" => {
                let text = item["text"].as_str().unwrap_or_default();
                self.rest_of(&id, text).map(Reply::Thinking).into_iter().collect()
            }
            // **A command the agent ran is a tool call**, which is what the pane already draws: the
            // command is the call and its output is the answer. Codex's own shape, reported as
            // Quill's, so a tool block means one thing whichever agent produced it.
            "command_execution" => {
                let mut out = Vec::new();
                if self.first_time(&id) {
                    out.push(Reply::ToolCall {
                        id: id.clone(),
                        name: "shell".to_owned(),
                        arguments: serde_json::json!({
                            "command": item["command"].as_str().unwrap_or_default(),
                        })
                        .to_string(),
                    });
                }
                if completed {
                    let code = item["exit_code"].as_i64();
                    out.push(Reply::ToolAnswer {
                        id,
                        answer: item["aggregated_output"].as_str().unwrap_or_default().to_owned(),
                        failed: code.is_some_and(|code| code != 0),
                    });
                }
                out
            }
            "file_change" | "mcp_tool_call" | "web_search" => {
                let mut out = Vec::new();
                if self.first_time(&id) {
                    out.push(Reply::ToolCall {
                        id: id.clone(),
                        name: item["type"].as_str().unwrap_or("tool").to_owned(),
                        arguments: item.to_string(),
                    });
                }
                if completed {
                    out.push(Reply::ToolAnswer {
                        id,
                        answer: item["status"].as_str().unwrap_or("done").to_owned(),
                        failed: item["status"] == "failed",
                    });
                }
                out
            }
            "error" => vec![Reply::Failed(
                item["message"]
                    .as_str()
                    .unwrap_or("the agent reported an error.")
                    .to_owned(),
            )],
            _ => Vec::new(),
        }
    }

    /// The part of `text` that has not been reported for this item yet, and nothing when it is all
    /// been.
    fn rest_of(&mut self, id: &str, text: &str) -> Option<String> {
        let already = self
            .sent
            .iter()
            .find(|(one, _)| one == id)
            .map(|(_, at)| *at)
            .unwrap_or(0);
        // Shorter than what was already sent means a different item wearing the same id, which
        // nothing observed does — but taking the whole of it is the answer that cannot panic on a
        // byte index into the middle of a character.
        if text.len() <= already {
            return None;
        }
        let rest = match text.is_char_boundary(already) {
            true => text[already..].to_owned(),
            false => text.to_owned(),
        };
        self.remember(id, text.len());
        Some(rest)
    }

    /// Whether this item has not been seen before, remembering that it has now.
    fn first_time(&mut self, id: &str) -> bool {
        if self.sent.iter().any(|(one, _)| one == id) {
            return false;
        }
        self.remember(id, 0);
        true
    }

    fn remember(&mut self, id: &str, at: usize) {
        match self.sent.iter_mut().find(|(one, _)| one == id) {
            Some(entry) => entry.1 = at,
            None => self.sent.push((id.to_owned(), at)),
        }
    }
}

/// A tool result's content, which is a string in the simple case and a list of blocks otherwise.
fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(said) => said.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<&str>>()
            .join(""),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The session an agent gave for this conversation, so the next turn carries on rather than starting
/// again.
///
/// On the conversation rather than on the provider, because it is *this* conversation the agent is
/// holding — starting a new one in the pane must start a new one in the agent too.
pub fn session_of(chat: &Conversation) -> String {
    chat.session.clone()
}

/// Where a picture is written so an agent can be given its path.
///
/// Both agents take an attachment as a file, and the pane holds one as bytes — so it is written to
/// the temporary folder under a name that says what it is. Written once per turn and left there:
/// deleting it while the agent is still reading it is a race, and the operating system clears its
/// own temporary folder.
pub fn write_a_picture(folder: &Path, name: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
    let safe: String = name
        .chars()
        .map(|one| match one.is_ascii_alphanumeric() || matches!(one, '.' | '-' | '_') {
            true => one,
            false => '-',
        })
        .collect();
    let path = folder.join(format!("quill-chat-{}-{safe}", std::process::id()));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(wire: Wire) -> Provider {
        Provider::defaults()
            .into_iter()
            .find(|one| one.wire == wire)
            .expect("a row of that shape")
    }

    #[test]
    fn the_two_agents_are_asked_in_their_own_words() {
        let ask = Ask {
            prompt: "What does this do?".to_owned(),
            permission: Permission::Read,
            ..Ask::default()
        };
        let claude = command_line(&provider(Wire::ClaudeCli), &ask);
        assert!(claude.contains(&"--print".to_owned()));
        assert!(claude.contains(&"stream-json".to_owned()));
        // Without it the answer arrives once, whole, at the end — and streaming is the thing the
        // ticket asks for by name.
        assert!(claude.contains(&"--include-partial-messages".to_owned()));
        assert!(claude.contains(&"--verbose".to_owned()), "stream-json refuses without it");

        let codex = command_line(&provider(Wire::CodexCli), &ask);
        assert_eq!(codex[0], "exec");
        assert!(codex.contains(&"--json".to_owned()));
        assert!(codex.contains(&"read-only".to_owned()));

        // **The question is on neither command line**, because a batch file cannot take a multi-line
        // argument and `codex` is one on Windows. It goes down standard input instead.
        for line in [&claude, &codex] {
            assert!(
                !line.iter().any(|word| word.contains("What does this do?")),
                "the question reached the command line: {line:?}"
            );
        }
        assert_eq!(prompt_for(&provider(Wire::ClaudeCli), &ask), "What does this do?");

        // And an endpoint is not a program, so it has no command line at all.
        assert!(command_line(&provider(Wire::OpenAi), &ask).is_empty());
    }

    #[test]
    fn a_second_question_carries_the_agents_own_session_rather_than_the_transcript() {
        // Which is the whole reason this transport is better than sending to the API behind it: the
        // context the agent has built is the context it keeps, and Quill sends one turn's words.
        let ask = Ask {
            prompt: "And the other one?".to_owned(),
            session: "29611139-4d2a-495a-b9a9-94a6189e509c".to_owned(),
            ..Ask::default()
        };
        let claude = command_line(&provider(Wire::ClaudeCli), &ask);
        let at = claude.iter().position(|one| one == "--resume").expect("resumed");
        assert_eq!(claude[at + 1], "29611139-4d2a-495a-b9a9-94a6189e509c");

        let codex = command_line(&provider(Wire::CodexCli), &ask);
        assert_eq!(codex[0], "exec");
        assert_eq!(codex[1], "resume", "its own sub-command rather than a flag");
        assert_eq!(codex[2], "29611139-4d2a-495a-b9a9-94a6189e509c");
        // And the sandbox is named the way *resume* names it, which is not the way `exec` does.
        assert!(!codex.contains(&"--sandbox".to_owned()), "{codex:?}");
        let at = codex.iter().position(|one| one == "-c").expect("a config override");
        assert_eq!(codex[at + 1], "sandbox_mode=\"read-only\"");
    }

    #[test]
    fn what_an_agent_may_do_is_said_in_each_agents_own_vocabulary() {
        for (permission, claude_says, codex_says) in [
            (Permission::Read, "manual", "read-only"),
            (Permission::Edit, "acceptEdits", "workspace-write"),
            (Permission::Full, "bypassPermissions", "--dangerously-bypass-approvals-and-sandbox"),
        ] {
            let ask = Ask { permission, ..Ask::default() };
            assert!(
                command_line(&provider(Wire::ClaudeCli), &ask).contains(&claude_says.to_owned()),
                "{permission:?}"
            );
            assert!(
                command_line(&provider(Wire::CodexCli), &ask).contains(&codex_says.to_owned()),
                "{permission:?}"
            );
        }
        // And every value the settings page offers is one the code has.
        for name in PERMISSIONS {
            assert_eq!(
                Permission::from_name(name).expect("registered with no code").name(),
                *name
            );
        }
        assert!(Permission::from_name("whatever").is_none());
    }

    #[test]
    fn a_picture_reaches_each_agent_the_way_that_agent_takes_one() {
        let ask = Ask {
            prompt: "What is wrong with this?".to_owned(),
            pictures: vec![std::path::PathBuf::from("/tmp/shot.png")],
            ..Ask::default()
        };
        // Codex has a flag for it.
        let codex = command_line(&provider(Wire::CodexCli), &ask);
        let at = codex.iter().position(|one| one == "--image").expect("an image flag");
        assert!(codex[at + 1].contains("shot.png"));
        // …so its question does not name the file twice.
        assert_eq!(
            prompt_for(&provider(Wire::CodexCli), &ask),
            "What is wrong with this?"
        );
        // Claude Code has no such flag, and reads a picture it is given the path of — so the path
        // goes in the words rather than the attachment being dropped in silence.
        let prompt = prompt_for(&provider(Wire::ClaudeCli), &ask);
        assert!(prompt.contains("What is wrong with this?"));
        assert!(prompt.contains("shot.png"), "{prompt}");
    }

    #[test]
    fn claude_code_wraps_the_anthropic_wire_and_is_read_by_the_decoder_that_already_reads_it() {
        // Which is the measurement this whole transport rests on: `stream_event.event` is byte for
        // byte what `/v1/messages` sends, so thinking, tool calls and token deltas need no new code.
        let mut decoder = Decoder::new(Wire::ClaudeCli);
        let replies = feed(
            &mut decoder,
            &[
                r#"{"type":"system","subtype":"init","session_id":"s-1","model":"claude-sonnet-5"}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"session_id":"s-1"}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello "}},"session_id":"s-1"}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"there"}},"session_id":"s-1"}"#,
                r#"{"type":"result","subtype":"success","is_error":false,"session_id":"s-1","usage":{"input_tokens":2,"cache_read_input_tokens":90,"output_tokens":7}}"#,
            ],
        );
        assert_eq!(replies[0], Reply::Session("s-1".to_owned()));
        assert_eq!(replies[1], Reply::Started { model: "claude-sonnet-5".to_owned() });
        assert_eq!(replies[2], Reply::Text("hello ".to_owned()));
        assert_eq!(replies[3], Reply::Text("there".to_owned()));
        assert_eq!(replies[4], Reply::Usage { input: 92, output: 7 });
        assert_eq!(replies[5], Reply::Finished { reason: "success".to_owned() });
        assert_eq!(decoder.session, "s-1", "the next turn resumes this one");
        assert!(decoder.ended);
    }

    #[test]
    fn a_tool_the_agent_ran_itself_is_shown_with_the_answer_it_got() {
        // **Quill runs nothing here.** The agent has its own tools and its own permission model, so
        // what the pane does with a tool call is show it — the block it already draws, filled in
        // from the agent's own report of what it did.
        let mut decoder = Decoder::new(Wire::ClaudeCli);
        let replies = feed(
            &mut decoder,
            &[
                r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"Bash","input":{}}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"git status\"}"}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"main","is_error":false}]}}"#,
            ],
        );
        assert_eq!(
            replies[0],
            Reply::ToolCall {
                id: "toolu_1".to_owned(),
                name: "Bash".to_owned(),
                arguments: "{\"command\":\"git status\"}".to_owned(),
            }
        );
        assert_eq!(
            replies[1],
            Reply::ToolAnswer {
                id: "toolu_1".to_owned(),
                answer: "main".to_owned(),
                failed: false,
            }
        );
    }

    #[test]
    fn a_codex_item_that_arrives_twice_is_shown_once() {
        // Its items are not deltas: `item.updated` carries the text so far and `item.completed`
        // carries all of it, so passing each on whole showed the answer twice over.
        let mut decoder = Decoder::new(Wire::CodexCli);
        let replies = feed(
            &mut decoder,
            &[
                r#"{"type":"thread.started","thread_id":"t-1"}"#,
                r#"{"type":"turn.started"}"#,
                r#"{"type":"item.updated","item":{"id":"item_0","type":"agent_message","text":"hello"}}"#,
                r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hello from the cli."}}"#,
                r#"{"type":"turn.completed","usage":{"input_tokens":21429,"cached_input_tokens":11008,"output_tokens":9}}"#,
            ],
        );
        assert_eq!(replies[0], Reply::Session("t-1".to_owned()));
        assert_eq!(replies[1], Reply::Started { model: String::new() });
        assert_eq!(replies[2], Reply::Text("hello".to_owned()));
        assert_eq!(
            replies[3],
            Reply::Text(" from the cli.".to_owned()),
            "only the part that had not been shown"
        );
        assert_eq!(replies[4], Reply::Usage { input: 32437, output: 9 });
        assert_eq!(replies[5], Reply::Finished { reason: "stop".to_owned() });
        assert_eq!(decoder.session, "t-1");
    }

    #[test]
    fn a_command_codex_ran_is_a_tool_block_with_its_output_in_it() {
        let mut decoder = Decoder::new(Wire::CodexCli);
        let replies = feed(
            &mut decoder,
            &[
                r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"git status","status":"in_progress"}}"#,
                r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"git status","aggregated_output":"nothing to commit","exit_code":0,"status":"completed"}}"#,
            ],
        );
        let Reply::ToolCall { id, name, arguments } = &replies[0] else {
            panic!("{replies:?}");
        };
        assert_eq!(id, "item_1");
        assert_eq!(name, "shell");
        assert!(arguments.contains("git status"));
        assert_eq!(
            replies[1],
            Reply::ToolAnswer {
                id: "item_1".to_owned(),
                answer: "nothing to commit".to_owned(),
                failed: false,
            }
        );
        assert_eq!(replies.len(), 2, "the call is announced once: {replies:?}");
    }

    #[test]
    fn a_command_that_failed_says_so_rather_than_looking_like_an_answer() {
        let mut decoder = Decoder::new(Wire::CodexCli);
        let replies = feed(
            &mut decoder,
            &[
                r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"nope","aggregated_output":"not found","exit_code":127,"status":"failed"}}"#,
            ],
        );
        let Reply::ToolAnswer { failed, .. } = replies[1] else {
            panic!("{replies:?}");
        };
        assert!(failed);
    }

    #[test]
    fn a_line_that_is_not_json_is_ignored_and_an_error_is_the_agents_own_words() {
        let mut decoder = Decoder::new(Wire::CodexCli);
        assert!(decoder.line("Reading additional input from stdin...").is_empty());
        assert!(decoder.line("").is_empty());
        let said = decoder.line(r#"{"type":"turn.failed","error":{"message":"the model is overloaded"}}"#);
        assert_eq!(said, vec![Reply::Failed("the model is overloaded".to_owned())]);
        assert!(decoder.ended);

        let mut decoder = Decoder::new(Wire::ClaudeCli);
        let said = decoder.line(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Credit balance is too low"}"#,
        );
        assert!(said.iter().any(|reply| matches!(reply, Reply::Failed(said) if said.contains("Credit balance"))));
    }

    #[test]
    fn stopping_kills_the_child_rather_than_waiting_for_it_to_say_something() {
        // **The measurement this exists for.** The thread reading an agent is asleep inside a read
        // with no timeout, so a flag it will not look at until the next line arrives does not stop
        // anything: `stop` put the pane back to `finished` at once, kept what had arrived, and left
        // the agent running. A long-lived child stands in for one here, because when a real agent
        // next says something is not something a test can know.
        let running = Running::default();
        // Its own name on each platform, and both are always there.
        let mut command = match cfg!(windows) {
            true => {
                let mut one = Command::new("ping");
                one.args(["-n", "60", "127.0.0.1"]);
                one
            }
            false => {
                let mut one = Command::new("sleep");
                one.arg("60");
                one
            }
        };
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("a long-lived child");
        running.hold(child);
        running.stop();
        let mut child = running.take().expect("the child is still held after being killed");
        let ended = child.wait().expect("it can be waited for");
        assert!(!ended.success(), "a killed program does not succeed: {ended}");

        // And stopping a turn that never started is not an error, which is what pressing the stop
        // button twice does.
        let nothing = Running::default();
        nothing.stop();
        assert!(nothing.take().is_none());
    }

    #[test]
    fn a_program_is_found_the_way_a_shell_finds_one() {
        // A name with a separator is a path; a bare name is looked for on `PATH`, with `PATHEXT`
        // tried on Windows because `claude` is a `.cmd` there.
        assert!(crate::provider::program("").is_none());
        assert!(crate::provider::program("quill-no-such-agent-anywhere").is_none());
        assert!(crate::provider::program("/definitely/not/here/claude").is_none());
        let folder = std::env::temp_dir().join(format!("quill-chat-program-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let name = match cfg!(windows) {
            true => "quill-chat-test-agent.exe",
            false => "quill-chat-test-agent",
        };
        let made = folder.join(name);
        std::fs::write(&made, b"").expect("a file that stands in for a program");
        assert_eq!(
            crate::provider::program(&made.to_string_lossy()),
            Some(made.clone()),
            "a path is taken as one"
        );
        let _ = std::fs::remove_file(&made);
    }

    #[test]
    fn an_agent_that_is_not_installed_says_so_before_anything_is_run() {
        let mut provider = provider(Wire::ClaudeCli);
        provider.command = "quill-no-such-agent-anywhere".to_owned();
        let why = provider.why_not().expect("a refusal");
        assert!(why.contains("quill-no-such-agent-anywhere"), "{why}");
        assert!(why.contains("PATH"), "{why}");
        // And it never asks for a key, whatever a settings file says.
        provider.key_env = "ANTHROPIC_API_KEY".to_owned();
        assert!(!provider.wants_a_key());
        assert!(provider.headers().iter().all(|(name, _)| name == "content-type"));
    }

    fn feed(decoder: &mut Decoder, lines: &[&str]) -> Vec<Reply> {
        let mut out = Vec::new();
        for line in lines {
            out.extend(decoder.line(line));
        }
        out
    }
}
