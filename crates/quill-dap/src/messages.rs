//! The messages Quill sends and the ones it reads, as Rust types with the JSON written by hand.
//!
//! **Only what Quill uses is modelled.** The protocol is large and deliberately additive — an
//! adapter may put anything it likes in a body and a later version of the specification will — so a
//! field this file does not name is a field nothing here looks at, which is the behaviour a reader
//! would expect from `serde` and is what this gets by reading values rather than deriving structs.
//!
//! The conversions are hand-written for the reason the workspace's comment on `serde_json` gives:
//! nothing here is a Rust type going over a wire, it is an open protocol other people's programs
//! speak, so what crosses is a value. `quill-cli/src/protocol.rs` made the same choice about Quill's
//! own control channel.
//!
//! Three kinds of message, each carrying a `seq`:
//!
//! - a **request** from the client, which the adapter answers;
//! - a **response**, repeating its request's seq as `request_seq` and saying whether it worked;
//! - an **event**, which the adapter sends when it likes and nothing answers.
//!
//! And one reverse request — `runInTerminal` — which is the adapter asking the *client* to do
//! something, and is therefore a request the client answers with a response of its own.

use serde_json::{json, Map, Value};

/// What the client asks for.
///
/// Every variant is one DAP request, and [`Request::command`] and [`Request::arguments`] are what
/// turn it into the wire. A request with nothing to say sends no `arguments` at all rather than an
/// empty object, because that is what the specification's own examples do.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// The handshake. The adapter answers with its capabilities, which is the whole reason a client
    /// never has to guess what an adapter can do.
    Initialize { client_id: String, lines_start_at_one: bool },
    /// Every breakpoint in one file, by **full replacement** — the protocol has no "add one".
    SetBreakpoints { path: String, breakpoints: Vec<SourceBreakpoint> },
    /// Which of the adapter's own exception filters are switched on.
    SetExceptionBreakpoints { filters: Vec<String> },
    /// The breakpoints are sent; start the program.
    ConfigurationDone,
    /// Start the debuggee. The body is the adapter's own shape, built by the registry entry, because
    /// each adapter names the program, the arguments and the folder slightly differently.
    Launch(Value),
    /// Let go of the debuggee, killing it or leaving it running as `terminate_debuggee` says.
    Disconnect { terminate_debuggee: bool },
    /// Ask the debuggee to stop politely, which an adapter that can honours.
    Terminate,
    Threads,
    StackTrace { thread: i64, levels: usize },
    Scopes { frame: i64 },
    Variables { reference: i64 },
    SetVariable { reference: i64, name: String, value: String },
    /// Evaluate an expression in a frame. `context` is `watch` for the watch list and `repl` for the
    /// expression box, which is the distinction the specification draws and adapters act on.
    Evaluate { expression: String, frame: Option<i64>, context: String },
    Continue { thread: i64 },
    Next { thread: i64 },
    StepIn { thread: i64 },
    StepOut { thread: i64 },
    Pause { thread: i64 },
}

impl Request {
    /// The `command` this goes over the wire as.
    pub fn command(&self) -> &'static str {
        match self {
            Request::Initialize { .. } => "initialize",
            Request::SetBreakpoints { .. } => "setBreakpoints",
            Request::SetExceptionBreakpoints { .. } => "setExceptionBreakpoints",
            Request::ConfigurationDone => "configurationDone",
            Request::Launch(_) => "launch",
            Request::Disconnect { .. } => "disconnect",
            Request::Terminate => "terminate",
            Request::Threads => "threads",
            Request::StackTrace { .. } => "stackTrace",
            Request::Scopes { .. } => "scopes",
            Request::Variables { .. } => "variables",
            Request::SetVariable { .. } => "setVariable",
            Request::Evaluate { .. } => "evaluate",
            Request::Continue { .. } => "continue",
            Request::Next { .. } => "next",
            Request::StepIn { .. } => "stepIn",
            Request::StepOut { .. } => "stepOut",
            Request::Pause { .. } => "pause",
        }
    }

    /// The `arguments` object, or nothing for a request that takes none.
    pub fn arguments(&self) -> Option<Value> {
        Some(match self {
            Request::Initialize { client_id, lines_start_at_one } => json!({
                "clientID": client_id,
                "clientName": "Quill",
                "adapterID": "quill",
                "locale": "en",
                "linesStartAt1": lines_start_at_one,
                "columnsStartAt1": true,
                "pathFormat": "path",
                // Asked for rather than assumed: an adapter that can run the debuggee in the
                // client's own terminal is what puts a real ConPTY behind the program, which is the
                // whole of §7.2. One that cannot simply never sends the reverse request.
                "supportsRunInTerminalRequest": true,
                "supportsVariableType": true,
                "supportsProgressReporting": false,
                "supportsMemoryReferences": false,
            }),
            Request::SetBreakpoints { path, breakpoints } => json!({
                "source": { "path": path, "name": file_name(path) },
                "breakpoints": breakpoints.iter().map(SourceBreakpoint::to_value).collect::<Vec<_>>(),
                "lines": breakpoints.iter().map(|one| one.line).collect::<Vec<_>>(),
                "sourceModified": false,
            }),
            Request::SetExceptionBreakpoints { filters } => json!({ "filters": filters }),
            Request::ConfigurationDone | Request::Terminate | Request::Threads => return None,
            Request::Launch(body) => body.clone(),
            Request::Disconnect { terminate_debuggee } => {
                json!({ "terminateDebuggee": terminate_debuggee, "restart": false })
            }
            Request::StackTrace { thread, levels } => {
                json!({ "threadId": thread, "startFrame": 0, "levels": levels })
            }
            Request::Scopes { frame } => json!({ "frameId": frame }),
            Request::Variables { reference } => json!({ "variablesReference": reference }),
            Request::SetVariable { reference, name, value } => {
                json!({ "variablesReference": reference, "name": name, "value": value })
            }
            Request::Evaluate { expression, frame, context } => {
                let mut body = Map::new();
                body.insert("expression".to_owned(), json!(expression));
                body.insert("context".to_owned(), json!(context));
                if let Some(frame) = frame {
                    body.insert("frameId".to_owned(), json!(frame));
                }
                Value::Object(body)
            }
            Request::Continue { thread } => json!({ "threadId": thread }),
            Request::Next { thread } | Request::StepIn { thread } | Request::StepOut { thread } => {
                json!({ "threadId": thread, "granularity": "statement" })
            }
            Request::Pause { thread } => json!({ "threadId": thread }),
        })
    }

    /// The whole frame body, ready for [`crate::codec::encode`].
    pub fn to_value(&self, seq: i64) -> Value {
        let mut message = Map::new();
        message.insert("seq".to_owned(), json!(seq));
        message.insert("type".to_owned(), json!("request"));
        message.insert("command".to_owned(), json!(self.command()));
        if let Some(arguments) = self.arguments() {
            message.insert("arguments".to_owned(), arguments);
        }
        Value::Object(message)
    }

    /// True when this request invalidates every `variablesReference` the client is holding.
    ///
    /// The protocol says a reference lives only while the program stays paused, so resuming in any
    /// of its five spellings throws the cache away. Zed's invalidation rule, and the specification's
    /// own — and stating it here rather than at each of the five call sites is what stops the sixth,
    /// added later, being the one that forgot.
    pub fn resumes(&self) -> bool {
        matches!(
            self,
            Request::Continue { .. }
                | Request::Next { .. }
                | Request::StepIn { .. }
                | Request::StepOut { .. }
        )
    }
}

/// One breakpoint as the adapter is told about it: a line, and the two optional strings that make it
/// conditional or a logpoint.
///
/// **The adapter does the evaluating and the logging.** A condition is a string the debugger
/// compiles in the debuggee's own language, and a log message is a string it formats and prints —
/// so Quill's whole cost for two features IntelliJ has is two `Option<String>`s and the modal that
/// edits them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceBreakpoint {
    /// One-based, which is what the protocol takes and what the gutter draws.
    pub line: usize,
    pub condition: Option<String>,
    pub log_message: Option<String>,
}

impl SourceBreakpoint {
    pub fn at(line: usize) -> Self {
        Self { line, ..Self::default() }
    }

    fn to_value(&self) -> Value {
        let mut body = Map::new();
        body.insert("line".to_owned(), json!(self.line));
        if let Some(condition) = self.condition.as_ref().filter(|text| !text.trim().is_empty()) {
            body.insert("condition".to_owned(), json!(condition));
        }
        if let Some(message) = self.log_message.as_ref().filter(|text| !text.trim().is_empty()) {
            body.insert("logMessage".to_owned(), json!(message));
        }
        Value::Object(body)
    }
}

/// What the adapter said about a breakpoint after it was set.
///
/// **Quill draws this rather than its own hope.** An adapter that moved a breakpoint to the next
/// statement is telling the truth about where the program will stop, and one that could not bind a
/// breakpoint at all says `verified: false` — which is drawn hollow rather than pretended about.
/// That is `task-1675`'s honesty rule applied to a protocol that was designed for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifiedBreakpoint {
    /// The adapter's own id, when it gave one. Carried so a later `breakpoint` event can be matched
    /// to the row it is about.
    pub id: Option<i64>,
    pub verified: bool,
    /// Where the breakpoint really landed, when the adapter said.
    pub line: Option<usize>,
    /// Why it could not be bound, when the adapter said. Shown as it was written.
    pub message: Option<String>,
}

impl VerifiedBreakpoint {
    fn read(value: &Value) -> Self {
        Self {
            id: value.get("id").and_then(Value::as_i64),
            verified: value.get("verified").and_then(Value::as_bool).unwrap_or(false),
            line: value.get("line").and_then(Value::as_u64).map(|line| line as usize),
            message: text(value, "message"),
        }
    }
}

/// What an adapter said it can do, out of the `initialize` response.
///
/// **Every optional feature asks this first**, so Quill never sends what an adapter did not offer
/// and a control whose capability is absent is absent — the rule the `F` button already follows.
/// A capability this file does not name is one nothing in Quill gates on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub configuration_done: bool,
    pub set_variable: bool,
    pub conditional_breakpoints: bool,
    pub log_points: bool,
    pub terminate_request: bool,
    pub evaluate_for_hovers: bool,
    /// The named exception filters this adapter offers, as `(filter, label, default)`. Quill holds
    /// no list of its own: an adapter that offers none gets no control.
    pub exception_filters: Vec<ExceptionFilter>,
}

impl Capabilities {
    fn read(body: &Value) -> Self {
        let flag = |name: &str| body.get(name).and_then(Value::as_bool).unwrap_or(false);
        let filters = body
            .get("exceptionBreakpointFilters")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().map(ExceptionFilter::read).collect())
            .unwrap_or_default();
        Self {
            configuration_done: flag("supportsConfigurationDoneRequest"),
            set_variable: flag("supportsSetVariable"),
            conditional_breakpoints: flag("supportsConditionalBreakpoints"),
            log_points: flag("supportsLogPoints"),
            terminate_request: flag("supportsTerminateRequest"),
            evaluate_for_hovers: flag("supportsEvaluateForHovers"),
            exception_filters: filters,
        }
    }
}

/// One of the adapter's own exception filters — debugpy's raised and uncaught, lldb's throw and
/// catch. Named by the adapter, ticked by the person, and sent straight back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExceptionFilter {
    pub filter: String,
    pub label: String,
    pub default: bool,
}

impl ExceptionFilter {
    fn read(value: &Value) -> Self {
        Self {
            filter: text(value, "filter").unwrap_or_default(),
            label: text(value, "label").unwrap_or_default(),
            default: value.get("default").and_then(Value::as_bool).unwrap_or(false),
        }
    }
}

/// One thread, as the adapter names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

/// One frame of the call stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    /// The file this frame is in, when the adapter knows one. A frame inside a library often has
    /// none, and is listed without one rather than hidden.
    pub path: Option<String>,
    pub line: usize,
    /// True when the adapter marked the frame `subtle` — library internals. **Listed rather than
    /// hidden**, in the quiet colour, which is the comments-and-strings rule from the references
    /// list.
    pub subtle: bool,
}

/// One group of variables: Locals, Arguments, Registers — whatever the adapter calls them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub name: String,
    pub reference: i64,
    /// True when the adapter says this scope is expensive to fetch, which is what stops Registers
    /// being opened unasked.
    pub expensive: bool,
}

/// One row of the variables tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: String,
    /// The type, when the adapter gives one. Never invented.
    pub kind: Option<String>,
    /// Non-zero when this row has children, which is the whole of the lazy model: nothing deeper is
    /// ever fetched until somebody opens the row.
    pub reference: i64,
    /// What to send back to `setVariable`/`evaluate` to name this row, when the adapter gave one.
    pub evaluate_name: Option<String>,
}

impl Variable {
    pub fn has_children(&self) -> bool {
        self.reference != 0
    }

    fn read(value: &Value) -> Self {
        Self {
            name: text(value, "name").unwrap_or_default(),
            value: text(value, "value").unwrap_or_default(),
            kind: text(value, "type"),
            reference: value.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
            evaluate_name: text(value, "evaluateName"),
        }
    }
}

/// Why the program stopped, as the adapter said it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// `breakpoint`, `step`, `exception`, `pause` — the adapter's own word, shown as it was given.
    pub reason: String,
    pub thread: Option<i64>,
    /// The sentence an adapter adds for an exception, which is better than anything Quill could say.
    pub description: Option<String>,
    pub text: Option<String>,
    /// True when the adapter says every thread stopped, which is the usual case.
    pub all_threads: bool,
}

/// Which stream an `output` event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// The debuggee's own standard output.
    Stdout,
    /// Its standard error.
    Stderr,
    /// The adapter talking about itself — what it is loading, what it could not find.
    Console,
}

/// Everything an adapter can say that Quill reads.
///
/// A message that is none of these is a [`Message::Other`], carried rather than dropped so that the
/// session can say what it saw and a later version can grow a variant without this becoming a place
/// that silently swallowed something.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// An answer to a request, matched by `request_seq`.
    Response {
        seq: i64,
        request_seq: i64,
        command: String,
        success: bool,
        /// The adapter's own message when it refused. **Never replaced with one of Quill's**: a
        /// debugger explains itself better than an editor could, which is git's rule kept here.
        message: Option<String>,
        body: Value,
    },
    /// The adapter is ready to be told about breakpoints.
    Initialized,
    Stopped(Stopped),
    /// The program is going again. Sent by some adapters and not by others, so nothing depends on
    /// it: the state machine also moves on the response to a stepping request.
    Continued { thread: Option<i64>, all_threads: bool },
    Output { kind: OutputKind, text: String },
    /// A breakpoint changed after it was set — bound once the library holding it loaded, most often.
    BreakpointChanged(VerifiedBreakpoint),
    /// The debuggee has gone.
    Terminated,
    Exited { code: i32 },
    /// The reverse request: run this command in the client's own terminal and say what its process
    /// id was. Quill answers it with the run tile.
    RunInTerminal { seq: i64, kind: String, title: String, cwd: String, args: Vec<String>, env: Vec<(String, String)> },
    /// Anything else the adapter sent.
    Other { kind: String, name: String },
}

impl Message {
    /// Read one frame.
    ///
    /// Anything unrecognised becomes [`Message::Other`] rather than an error: the protocol is
    /// additive by design and an adapter sending a `progressStart` is behaving correctly.
    pub fn read(value: &Value) -> Message {
        let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "response" => Message::Response {
                seq: value.get("seq").and_then(Value::as_i64).unwrap_or(0),
                request_seq: value.get("request_seq").and_then(Value::as_i64).unwrap_or(0),
                command: text(value, "command").unwrap_or_default(),
                success: value.get("success").and_then(Value::as_bool).unwrap_or(false),
                message: text(value, "message"),
                body: value.get("body").cloned().unwrap_or(Value::Null),
            },
            "event" => read_event(value),
            "request" => read_reverse_request(value),
            other => Message::Other { kind: other.to_owned(), name: String::new() },
        }
    }
}

fn read_event(value: &Value) -> Message {
    let name = text(value, "event").unwrap_or_default();
    let body = value.get("body").cloned().unwrap_or(Value::Null);
    match name.as_str() {
        "initialized" => Message::Initialized,
        "stopped" => Message::Stopped(Stopped {
            reason: text(&body, "reason").unwrap_or_else(|| "stopped".to_owned()),
            thread: body.get("threadId").and_then(Value::as_i64),
            description: text(&body, "description"),
            text: text(&body, "text"),
            all_threads: body
                .get("allThreadsStopped")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        }),
        "continued" => Message::Continued {
            thread: body.get("threadId").and_then(Value::as_i64),
            all_threads: body
                .get("allThreadsContinued")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        },
        "output" => Message::Output {
            kind: match text(&body, "category").as_deref() {
                Some("stderr") => OutputKind::Stderr,
                Some("stdout") => OutputKind::Stdout,
                // `console`, `important`, `telemetry` and anything else are the adapter talking
                // rather than the program, which is the distinction that matters here.
                _ => OutputKind::Console,
            },
            text: text(&body, "output").unwrap_or_default(),
        },
        "breakpoint" => Message::BreakpointChanged(VerifiedBreakpoint::read(
            body.get("breakpoint").unwrap_or(&Value::Null),
        )),
        "terminated" => Message::Terminated,
        "exited" => Message::Exited {
            code: body.get("exitCode").and_then(Value::as_i64).unwrap_or(0) as i32,
        },
        _ => Message::Other { kind: "event".to_owned(), name },
    }
}

/// The one request an adapter makes of the client.
fn read_reverse_request(value: &Value) -> Message {
    let name = text(value, "command").unwrap_or_default();
    if name != "runInTerminal" {
        return Message::Other { kind: "request".to_owned(), name };
    }
    let arguments = value.get("arguments").cloned().unwrap_or(Value::Null);
    let args = arguments
        .get("args")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let env = arguments
        .get("env")
        .and_then(Value::as_object)
        .map(|pairs| {
            pairs
                .iter()
                // A null value means "take this one out of the environment", which is not
                // something the run tile's `SessionSettings` can express, so it is dropped rather
                // than being turned into an empty string that means something different.
                .filter_map(|(name, value)| {
                    value.as_str().map(|value| (name.clone(), value.to_owned()))
                })
                .collect::<Vec<(String, String)>>()
        })
        .unwrap_or_default();
    Message::RunInTerminal {
        seq: value.get("seq").and_then(Value::as_i64).unwrap_or(0),
        // `integrated` or `external`. Quill has one terminal and runs both in it, which is what
        // every editor with one console does, and it says which it was asked for nowhere because
        // nobody could act on the difference.
        kind: text(&arguments, "kind").unwrap_or_else(|| "integrated".to_owned()),
        title: text(&arguments, "title").unwrap_or_else(|| "Debug".to_owned()),
        cwd: text(&arguments, "cwd").unwrap_or_default(),
        args,
        env,
    }
}

/// The client's answer to `runInTerminal`: a response to the adapter's own request.
///
/// `started` says whether the command really began. `process` is the debuggee's process id **when the
/// client can supply one**, and the specification makes it optional for exactly this reason: a
/// pseudoconsole hands back a console rather than a child, so a client that runs the program through
/// one often cannot say which process it became. Quill is one of those — `quill_terminal::Session`
/// owns a ConPTY and alacritty's pty layer does not surface the child — so it answers `success: true`
/// with no id, which is what lldb-dap's own comm-file scheme and js-debug both expect.
///
/// A command that could not be started is `success: false` with a sentence, and the adapter then
/// falls back to running the program itself and sending its output as `output` events.
pub fn run_in_terminal_response(
    seq: i64,
    request_seq: i64,
    started: bool,
    process: Option<u32>,
) -> Value {
    let mut message = Map::new();
    message.insert("seq".to_owned(), json!(seq));
    message.insert("type".to_owned(), json!("response"));
    message.insert("request_seq".to_owned(), json!(request_seq));
    message.insert("command".to_owned(), json!("runInTerminal"));
    message.insert("success".to_owned(), json!(started));
    match (started, process) {
        (true, Some(id)) => {
            message.insert("body".to_owned(), json!({ "processId": id }));
        }
        (true, None) => {
            message.insert("body".to_owned(), json!({}));
        }
        (false, _) => {
            message.insert(
                "message".to_owned(),
                json!("Quill could not start the program in its run tile."),
            );
        }
    }
    Value::Object(message)
}

/// The `threads` answer.
pub fn read_threads(body: &Value) -> Vec<Thread> {
    body.get("threads")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| Thread {
                    id: row.get("id").and_then(Value::as_i64).unwrap_or(0),
                    name: text(row, "name").unwrap_or_else(|| "thread".to_owned()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `stackTrace` answer.
pub fn read_frames(body: &Value) -> Vec<Frame> {
    body.get("stackFrames")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| Frame {
                    id: row.get("id").and_then(Value::as_i64).unwrap_or(0),
                    name: text(row, "name").unwrap_or_default(),
                    path: row.get("source").and_then(|source| text(source, "path")),
                    line: row.get("line").and_then(Value::as_u64).unwrap_or(0) as usize,
                    subtle: text(row, "presentationHint").as_deref() == Some("subtle"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `scopes` answer.
pub fn read_scopes(body: &Value) -> Vec<Scope> {
    body.get("scopes")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| Scope {
                    name: text(row, "name").unwrap_or_default(),
                    reference: row.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
                    expensive: row.get("expensive").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `variables` answer.
pub fn read_variables(body: &Value) -> Vec<Variable> {
    body.get("variables")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().map(Variable::read).collect())
        .unwrap_or_default()
}

/// The `setBreakpoints` answer: one row per breakpoint sent, in the order they were sent.
pub fn read_breakpoints(body: &Value) -> Vec<VerifiedBreakpoint> {
    body.get("breakpoints")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().map(VerifiedBreakpoint::read).collect())
        .unwrap_or_default()
}

/// The `setVariable` answer, which is the value **as the debugger now sees it** rather than what was
/// typed — a debugger that rounded a float or interned a string is telling the truth about what the
/// program holds.
pub fn read_set_variable(body: &Value) -> Variable {
    Variable {
        name: String::new(),
        value: text(body, "value").unwrap_or_default(),
        kind: text(body, "type"),
        reference: body.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
        evaluate_name: None,
    }
}

/// The `evaluate` answer. It carries a `variablesReference` like anything else, which is why a watch
/// that answers with a structure expands.
pub fn read_evaluate(body: &Value) -> Variable {
    Variable {
        name: String::new(),
        value: text(body, "result").unwrap_or_default(),
        kind: text(body, "type"),
        reference: body.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
        evaluate_name: None,
    }
}

/// The `initialize` answer.
pub fn read_capabilities(body: &Value) -> Capabilities {
    Capabilities::read(body)
}

/// A string field, absent rather than empty when it is not there.
fn text(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_owned)
}

/// The last part of a path, which is what a `source.name` is.
fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_with_no_arguments_sends_none() {
        assert!(Request::ConfigurationDone.arguments().is_none());
        assert!(Request::Threads.arguments().is_none());
        let frame = Request::ConfigurationDone.to_value(4);
        assert_eq!(frame["command"], "configurationDone");
        assert_eq!(frame["seq"], 4);
        assert!(frame.get("arguments").is_none(), "an empty object would be noise on the wire");
    }

    #[test]
    fn a_breakpoint_with_nothing_extra_carries_only_its_line() {
        let request = Request::SetBreakpoints {
            path: "C:\\p\\src\\main.rs".to_owned(),
            breakpoints: vec![SourceBreakpoint::at(14)],
        };
        let arguments = request.arguments().expect("setBreakpoints takes arguments");
        assert_eq!(arguments["source"]["path"], "C:\\p\\src\\main.rs");
        assert_eq!(arguments["source"]["name"], "main.rs", "the last part, on either separator");
        assert_eq!(arguments["breakpoints"][0]["line"], 14);
        assert!(arguments["breakpoints"][0].get("condition").is_none());
        assert!(arguments["breakpoints"][0].get("logMessage").is_none());
    }

    /// A condition and a log message are data in the request: the adapter evaluates and the adapter
    /// logs, so Quill's whole cost for two of IntelliJ's features is two optional strings.
    #[test]
    fn a_condition_and_a_log_message_are_sent_as_they_were_written() {
        let breakpoint = SourceBreakpoint {
            line: 9,
            condition: Some("attempts > 3".to_owned()),
            log_message: Some("at {attempts}".to_owned()),
        };
        let request =
            Request::SetBreakpoints { path: "a.js".to_owned(), breakpoints: vec![breakpoint] };
        let arguments = request.arguments().expect("arguments");
        assert_eq!(arguments["breakpoints"][0]["condition"], "attempts > 3");
        assert_eq!(arguments["breakpoints"][0]["logMessage"], "at {attempts}");
    }

    /// Blank is the same as absent. A person who opened the modal, thought better of it and left the
    /// field empty has not asked for a condition of `""`, which some adapters treat as false.
    #[test]
    fn a_blank_condition_is_not_sent_at_all() {
        let breakpoint = SourceBreakpoint {
            line: 2,
            condition: Some("   ".to_owned()),
            log_message: Some(String::new()),
        };
        let request =
            Request::SetBreakpoints { path: "a.js".to_owned(), breakpoints: vec![breakpoint] };
        let arguments = request.arguments().expect("arguments");
        assert!(arguments["breakpoints"][0].get("condition").is_none());
        assert!(arguments["breakpoints"][0].get("logMessage").is_none());
    }

    #[test]
    fn the_four_stepping_requests_invalidate_the_references_and_the_reading_ones_do_not() {
        assert!(Request::Continue { thread: 1 }.resumes());
        assert!(Request::Next { thread: 1 }.resumes());
        assert!(Request::StepIn { thread: 1 }.resumes());
        assert!(Request::StepOut { thread: 1 }.resumes());
        assert!(!Request::Variables { reference: 5 }.resumes());
        assert!(!Request::Pause { thread: 1 }.resumes(), "pausing stops rather than resumes");
    }

    #[test]
    fn a_response_is_read_with_the_seq_it_answers() {
        let value = serde_json::json!({
            "seq": 5, "type": "response", "request_seq": 2, "command": "threads",
            "success": true, "body": { "threads": [{ "id": 1, "name": "main" }] }
        });
        let Message::Response { request_seq, command, success, body, .. } = Message::read(&value)
        else {
            panic!("a response");
        };
        assert_eq!(request_seq, 2);
        assert_eq!(command, "threads");
        assert!(success);
        assert_eq!(read_threads(&body), vec![Thread { id: 1, name: "main".to_owned() }]);
    }

    /// The adapter's own refusal is carried whole, because nothing in Quill could say it better.
    #[test]
    fn a_refusal_carries_the_adapters_own_message() {
        let value = serde_json::json!({
            "seq": 6, "type": "response", "request_seq": 3, "command": "setVariable",
            "success": false, "message": "cannot assign to a constant"
        });
        let Message::Response { success, message, .. } = Message::read(&value) else {
            panic!("a response");
        };
        assert!(!success);
        assert_eq!(message.as_deref(), Some("cannot assign to a constant"));
    }

    #[test]
    fn the_events_quill_reads_are_read() {
        let event = |name: &str, body: serde_json::Value| {
            Message::read(&serde_json::json!({ "seq": 1, "type": "event", "event": name, "body": body }))
        };
        assert_eq!(event("initialized", Value::Null), Message::Initialized);
        assert_eq!(event("terminated", Value::Null), Message::Terminated);
        assert_eq!(event("exited", serde_json::json!({ "exitCode": 101 })), Message::Exited { code: 101 });
        let Message::Stopped(stopped) =
            event("stopped", serde_json::json!({ "reason": "breakpoint", "threadId": 1 }))
        else {
            panic!("a stop");
        };
        assert_eq!(stopped.reason, "breakpoint");
        assert_eq!(stopped.thread, Some(1));
        let Message::Output { kind, text } =
            event("output", serde_json::json!({ "category": "stderr", "output": "boom\n" }))
        else {
            panic!("output");
        };
        assert_eq!(kind, OutputKind::Stderr);
        assert_eq!(text, "boom\n");
    }

    /// Output with no category is the adapter talking about itself, which is what the specification
    /// says `console` means and what every adapter that omits it intends.
    #[test]
    fn output_with_no_category_is_the_adapter_talking() {
        let value = serde_json::json!({
            "seq": 1, "type": "event", "event": "output", "body": { "output": "loading\n" }
        });
        let Message::Output { kind, .. } = Message::read(&value) else { panic!("output") };
        assert_eq!(kind, OutputKind::Console);
    }

    #[test]
    fn an_event_quill_does_not_read_is_carried_rather_than_dropped() {
        let value = serde_json::json!({ "seq": 1, "type": "event", "event": "progressStart" });
        assert_eq!(
            Message::read(&value),
            Message::Other { kind: "event".to_owned(), name: "progressStart".to_owned() }
        );
    }

    #[test]
    fn the_reverse_request_is_read_with_its_command_and_folder() {
        let value = serde_json::json!({
            "seq": 12, "type": "request", "command": "runInTerminal",
            "arguments": {
                "kind": "integrated", "title": "Debug", "cwd": "C:\\p",
                "args": ["C:\\p\\target\\debug\\app.exe", "--fast"],
                "env": { "RUST_LOG": "debug", "GONE": null }
            }
        });
        let Message::RunInTerminal { seq, cwd, args, env, .. } = Message::read(&value) else {
            panic!("the reverse request");
        };
        assert_eq!(seq, 12);
        assert_eq!(cwd, "C:\\p");
        assert_eq!(args, vec!["C:\\p\\target\\debug\\app.exe", "--fast"]);
        assert_eq!(env, vec![("RUST_LOG".to_owned(), "debug".to_owned())]);
    }

    #[test]
    fn the_answer_to_the_reverse_request_carries_the_process_id() {
        let yes = run_in_terminal_response(20, 12, true, Some(4242));
        assert_eq!(yes["success"], true);
        assert_eq!(yes["body"]["processId"], 4242);
        assert_eq!(yes["request_seq"], 12);
        // The id is optional in the specification, and a client running the program through a
        // pseudoconsole often has none to give. Started is what matters.
        let pidless = run_in_terminal_response(20, 12, true, None);
        assert_eq!(pidless["success"], true);
        assert!(pidless["body"].get("processId").is_none());
        let no = run_in_terminal_response(20, 12, false, None);
        assert_eq!(no["success"], false);
        assert!(no["message"].as_str().is_some(), "a refusal says why");
    }

    #[test]
    fn capabilities_that_were_not_offered_are_false_rather_than_assumed() {
        let body = serde_json::json!({ "supportsSetVariable": true });
        let capabilities = read_capabilities(&body);
        assert!(capabilities.set_variable);
        assert!(!capabilities.conditional_breakpoints, "not offered means not there");
        assert!(!capabilities.log_points);
        assert!(capabilities.exception_filters.is_empty());
    }

    #[test]
    fn the_exception_filters_are_the_adapters_own() {
        let body = serde_json::json!({
            "exceptionBreakpointFilters": [
                { "filter": "raised", "label": "Raised Exceptions", "default": false },
                { "filter": "uncaught", "label": "Uncaught Exceptions", "default": true }
            ]
        });
        let capabilities = read_capabilities(&body);
        assert_eq!(capabilities.exception_filters.len(), 2);
        assert_eq!(capabilities.exception_filters[1].filter, "uncaught");
        assert!(capabilities.exception_filters[1].default);
    }

    #[test]
    fn a_frame_the_adapter_marked_subtle_stays_marked() {
        let body = serde_json::json!({
            "stackFrames": [
                { "id": 1, "name": "main", "line": 4, "source": { "path": "a.rs" } },
                { "id": 2, "name": "core::ops", "line": 9, "presentationHint": "subtle" }
            ]
        });
        let frames = read_frames(&body);
        assert_eq!(frames[0].path.as_deref(), Some("a.rs"));
        assert!(!frames[0].subtle);
        assert!(frames[1].subtle, "library internals stay marked all the way to the screen");
        assert!(frames[1].path.is_none(), "a frame with no file is listed rather than hidden");
    }

    #[test]
    fn a_variable_with_children_says_so_and_one_without_does_not() {
        let body = serde_json::json!({
            "variables": [
                { "name": "count", "value": "3", "type": "i32", "variablesReference": 0 },
                { "name": "items", "value": "Vec(2)", "variablesReference": 17, "evaluateName": "items" }
            ]
        });
        let variables = read_variables(&body);
        assert!(!variables[0].has_children());
        assert_eq!(variables[0].kind.as_deref(), Some("i32"));
        assert!(variables[1].has_children());
        assert_eq!(variables[1].reference, 17);
        assert_eq!(variables[1].evaluate_name.as_deref(), Some("items"));
    }

    #[test]
    fn a_breakpoint_the_adapter_moved_is_read_where_it_really_landed() {
        let body = serde_json::json!({
            "breakpoints": [
                { "id": 1, "verified": true, "line": 15 },
                { "verified": false, "message": "no code on that line" }
            ]
        });
        let answered = read_breakpoints(&body);
        assert_eq!(answered[0].line, Some(15));
        assert!(answered[0].verified);
        assert!(!answered[1].verified);
        assert_eq!(answered[1].message.as_deref(), Some("no code on that line"));
        assert!(answered[1].line.is_none());
    }

    #[test]
    fn set_variable_answers_with_the_value_the_debugger_now_holds() {
        let body = serde_json::json!({ "value": "42", "type": "i32" });
        let answered = read_set_variable(&body);
        assert_eq!(answered.value, "42");
        assert_eq!(answered.kind.as_deref(), Some("i32"));
    }
}
