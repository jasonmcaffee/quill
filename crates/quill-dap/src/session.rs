//! The session: a small state machine that knows what to ask for next, and nothing else.
//!
//! **Nothing here does any input or output.** A [`Session`] is given messages and answers with the
//! frames to write and the events the window should act on, which is what makes every case in §12
//! of `tasks/task-1687-debugging-tdd.md` a test with no process behind it: the happy lifecycle, an
//! adapter that dies in the middle of one, a breakpoint that could not be bound, a `stopped` that
//! arrives before `configurationDone`, and a request that is never sent because the adapter did not
//! say it could do it. `crate::client::Client` is the half that owns a pipe.
//!
//! ## The lifecycle
//!
//! [`State::Starting`] — `initialize` has been sent. Its answer carries the [`Capabilities`], and
//! **every optional feature asks them first**, so Quill never sends what an adapter did not offer.
//! Then `launch`, whose body the caller built, because each adapter names the program and its
//! arguments slightly differently and that knowledge is Quill's rather than the protocol's.
//!
//! [`State::Configuring`] — the `initialized` event has arrived, which is the adapter saying it is
//! ready to be told where to stop. Every file that has a breakpoint is sent, then the exception
//! filters, then `configurationDone`.
//!
//! The order is the protocol's own rather than the TDD's §2.2 summary, which lists `setBreakpoints`
//! before `launch`. It has to be: the `initialized` event is what says the adapter will accept
//! breakpoints, and no adapter sends it until `launch` has been received. Sending them earlier gets
//! them refused by lldb-dap and dropped by js-debug.
//!
//! [`State::Running`] and [`State::Paused`] — a `stopped` event names a thread, and the four
//! requests every stop needs are made unprompted: `threads`, `stackTrace`, the top frame's `scopes`
//! and the first scope's `variables`. Everything deeper waits until a row is opened.
//!
//! [`State::Ended`] — `terminated`, `exited`, or the adapter itself has gone.
//!
//! ## References die on resume
//!
//! Every `variablesReference` is valid only while the program stays paused. The cache of fetched
//! rows is therefore thrown away on `continued` and on **every stepping request**, which is the
//! specification's own rule and Zed's invalidation rule. [`crate::Request::resumes`] is where the
//! five spellings of "the program is going again" are written down once.

use serde_json::Value;

use crate::messages::{
    self, Capabilities, Frame, Message, OutputKind, Request, Scope, SourceBreakpoint, Stopped,
    Thread, Variable, VerifiedBreakpoint,
};

/// How many frames of the stack are asked for at a stop.
///
/// A stack can be thousands deep in a recursion that went wrong, and the pane shows a few dozen.
/// Two hundred is `quill_app::git::HISTORY_LIMIT`'s reasoning applied to a stack: far more than
/// anybody reads, and bounded so that a runaway recursion is a long list rather than a hung window.
pub const STACK_LIMIT: usize = 200;

/// Where a session is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The adapter has been started and `initialize` sent.
    Starting,
    /// The adapter said `initialized`; the breakpoints are going out.
    Configuring,
    /// The program is going.
    Running,
    /// The program has stopped somewhere.
    Paused,
    /// The program, or the adapter, has gone.
    Ended,
}

impl State {
    /// The word the tile's header, the status bar and `debug status` all use, so the three cannot
    /// come to different answers. [`Session::where_it_is`] adds the location to it.
    pub fn label(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Configuring => "configuring",
            State::Running => "running",
            State::Paused => "paused",
            State::Ended => "ended",
        }
    }

    pub fn is_paused(self) -> bool {
        self == State::Paused
    }

    /// True while there is a debuggee to act on at all, which is what dims the stepping buttons and
    /// the stop button rather than removing them.
    pub fn is_alive(self) -> bool {
        self != State::Ended
    }
}

/// One of the five stepping requests, named once so the menu, the keyboard and the command line
/// cannot disagree about which is which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// `continue` — run to the next breakpoint. The reference editor's `F9`.
    Resume,
    /// `next` — run this line and stop on the next. `F8`.
    Over,
    /// `stepIn` — go into the call on this line. `F7`.
    Into,
    /// `stepOut` — finish this function and stop in the caller. `Shift+F8`.
    Out,
    /// `pause` — stop a program that is running.
    Pause,
}

impl Step {
    pub fn name(self) -> &'static str {
        match self {
            Step::Resume => "continue",
            Step::Over => "step-over",
            Step::Into => "step-into",
            Step::Out => "step-out",
            Step::Pause => "pause",
        }
    }

    /// What the status bar says while it happens.
    pub fn label(self) -> &'static str {
        match self {
            Step::Resume => "Resuming",
            Step::Over => "Stepping over",
            Step::Into => "Stepping into",
            Step::Out => "Stepping out",
            Step::Pause => "Pausing",
        }
    }

    fn request(self, thread: i64) -> Request {
        match self {
            Step::Resume => Request::Continue { thread },
            Step::Over => Request::Next { thread },
            Step::Into => Request::StepIn { thread },
            Step::Out => Request::StepOut { thread },
            Step::Pause => Request::Pause { thread },
        }
    }
}

/// What the session asks the window to do about something that happened.
///
/// It draws none of it and reads no file: an event says what is now true, and `app::debug` decides
/// what the window does about it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The adapter answered `initialize`. Its capabilities are on the session.
    Ready,
    /// The program is going.
    Running,
    /// It stopped. The frames follow in their own event, because they are a second request.
    Stopped(Stopped),
    /// The call stack of the stopped thread.
    Frames(Vec<Frame>),
    /// The threads the adapter knows about, which is what puts a chooser above the frames when
    /// there is more than one.
    Threads(Vec<Thread>),
    /// The groups of variables in one frame.
    Scopes { frame: i64, scopes: Vec<Scope> },
    /// The children of one reference, which is either a scope or an opened row.
    Variables { reference: i64, variables: Vec<Variable> },
    /// What the adapter said about the breakpoints in one file — where each really landed and
    /// whether it is bound. Drawn as given.
    Breakpoints { path: String, answered: Vec<VerifiedBreakpoint> },
    /// One breakpoint changed after the fact, which is how a breakpoint in a library binds once the
    /// library loads.
    BreakpointChanged(VerifiedBreakpoint),
    /// The debuggee, or the adapter, said something.
    Output { kind: OutputKind, text: String },
    /// A watch or the expression box was answered. `id` is what the caller labelled the question
    /// with, so an answer that arrives after another was asked can still be put in the right row.
    Evaluated { id: u64, result: Result<Variable, String> },
    /// A value was changed in the running program, and this is the value **as the debugger now sees
    /// it** rather than what was typed.
    VariableSet { reference: i64, name: String, result: Result<Variable, String> },
    /// The same, for a value that was named by an **expression** rather than by a row that had
    /// already been read. `task-1696`: this is what changing the root of a value tooltip answers
    /// with, since that row has no container reference for `setVariable` to name it by.
    ExpressionSet { expression: String, result: Result<Variable, String> },
    /// The adapter refused something. Its own message, never one of Quill's.
    Failed { command: String, message: String },
    /// Run this in the client's own terminal and answer with the process id. See §7.2: this is what
    /// puts a real ConPTY behind the debuggee.
    RunInTerminal {
        seq: i64,
        title: String,
        cwd: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// The adapter has put the program in a session of its own and is asking the client to open it.
    ///
    /// js-debug's model, and the reason a `node` configuration used to connect and then do nothing:
    /// the parent session it left behind has no threads and never stops, and its breakpoints stay
    /// `provisional`. The client answers this by dialling the same adapter a second time and running
    /// the handshake again with `configuration` — which carries js-debug's own `__pendingTargetId`,
    /// the only thing that ties the new connection to the program that is already running.
    StartDebugging { seq: i64, request: String, configuration: serde_json::Value },
    /// The session is over, with the code the program chose if it chose one.
    Ended { code: Option<i32> },
}

/// The frames to write and the events to act on.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outcome {
    pub frames: Vec<Value>,
    pub events: Vec<Event>,
}

impl Outcome {
    /// True when there is nothing to write and nothing to act on, which is what a message the
    /// session deliberately ignores produces.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty() && self.events.is_empty()
    }

    fn absorb(&mut self, other: Outcome) {
        self.frames.extend(other.frames);
        self.events.extend(other.events);
    }
}

/// What one outstanding request was, so its answer can be understood.
///
/// Correlation is by `seq` on this side rather than by the command name, because two `variables`
/// requests for two different references are in flight the moment somebody opens two rows.
#[derive(Debug, Clone, PartialEq)]
enum Awaiting {
    Initialize,
    Launch,
    Breakpoints(String),
    ConfigurationDone,
    Threads,
    StackTrace,
    Scopes { frame: i64 },
    Variables { reference: i64 },
    SetVariable { reference: i64, name: String },
    SetExpression { expression: String },
    Evaluate { id: u64 },
    Stepping(Step),
    Ending,
    /// Something whose answer nothing is waiting for, such as the exception filters.
    Nothing,
}

/// The client side of one debug session.
pub struct Session {
    state: State,
    capabilities: Capabilities,
    /// The next `seq` to send. The protocol says they start at one and go up by one.
    next_seq: i64,
    /// What each outstanding request was. A short list — a handful at the very most, since the
    /// window only asks for what is on the screen — so it is walked rather than indexed.
    awaiting: Vec<(i64, Awaiting)>,
    /// The seqs of the requests **a stop itself made**, which is what [`Session::has_read_the_stop`]
    /// counts. See there for why this is not the whole of `awaiting`.
    reading: Vec<i64>,
    /// The body of the `launch` request, built by the registry entry because each adapter names the
    /// program and its arguments its own way.
    launch: Value,
    /// The breakpoints to send when the adapter says it is ready, by file.
    breakpoints: Vec<(String, Vec<SourceBreakpoint>)>,
    /// The exception filters that are switched on, out of what the adapter offered.
    exception_filters: Vec<String>,
    /// The thread that stopped, which is what every stepping request is about.
    thread: Option<i64>,
    /// Every thread the adapter last reported.
    threads: Vec<Thread>,
    /// The frame whose variables are showing. The top one at each stop, and whatever was clicked
    /// after that.
    frame: Option<i64>,
    /// The top frame's file and line, which is the execution point and what `debug status` reports.
    location: Option<(String, usize)>,
    /// What the debuggee ended with, once it has.
    exit_code: Option<i32>,
    /// True once the adapter has asked to run the debuggee in Quill's terminal, so the session
    /// knows the run tile is where the output is going and the `output` events are the adapter's
    /// own rather than the program's.
    runs_in_terminal: bool,
}

impl Session {
    /// A session that has not started yet. `launch` is the body of the launch request.
    pub fn new(launch: Value) -> Self {
        Self {
            state: State::Starting,
            capabilities: Capabilities::default(),
            next_seq: 1,
            awaiting: Vec::new(),
            reading: Vec::new(),
            launch,
            breakpoints: Vec::new(),
            exception_filters: Vec::new(),
            thread: None,
            threads: Vec::new(),
            frame: None,
            location: None,
            exit_code: None,
            runs_in_terminal: false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    pub fn threads(&self) -> &[Thread] {
        &self.threads
    }

    /// True while the adapter has been asked something it has not answered.
    pub fn is_waiting(&self) -> bool {
        !self.awaiting.is_empty()
    }

    /// True once the four requests a stop makes have all come back.
    ///
    /// What "the stop has been read" is really asking. A `stopped` event is one thing and the
    /// requests it causes — `threads`, `stackTrace`, `scopes`, `variables` — are another, so there is
    /// a window of a few round trips in which the session is paused and knows nothing about where.
    ///
    /// **Only those requests are counted**, and that is the whole reason this is not simply
    /// [`Session::is_waiting`]. An adapter that never answers something is a real thing: measured
    /// against CodeLLDB 1.12.3, an `evaluate` of a name that does not resolve gets a Python traceback
    /// on its standard error and **no response at all**. A readiness rule that waited for every
    /// outstanding request would be wedged for the rest of the session by one bad watch expression.
    /// See `quill_app::app::debug::DebugState::is_ready`.
    pub fn has_read_the_stop(&self) -> bool {
        self.reading.is_empty()
    }

    /// Which thread the stepping requests are about.
    pub fn thread(&self) -> Option<i64> {
        self.thread
    }

    /// Which frame the variables are being read from.
    pub fn frame(&self) -> Option<i64> {
        self.frame
    }

    /// Where the program is stopped: the file and the one-based line of the frame that is showing.
    pub fn location(&self) -> Option<(&str, usize)> {
        self.location.as_ref().map(|(path, line)| (path.as_str(), *line))
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn runs_in_terminal(&self) -> bool {
        self.runs_in_terminal
    }

    /// One sentence saying where the session is, which is what the tile's header shows and what
    /// `debug status` prints — one function so the two cannot disagree.
    pub fn where_it_is(&self) -> String {
        match (self.state, &self.location) {
            (State::Paused, Some((path, line))) => {
                format!("paused at {}:{line}", file_name(path))
            }
            (State::Ended, _) => match self.exit_code {
                Some(0) => "ended".to_owned(),
                Some(code) => format!("ended with exit code {code}"),
                None => "ended".to_owned(),
            },
            (state, _) => state.label().to_owned(),
        }
    }

    /// Say what the breakpoints are before the session starts, or replace one file's while it runs.
    ///
    /// **Full replacement per file**, because that is what the protocol's `setBreakpoints` is: there
    /// is no "add one". A file with none left is still sent, with an empty list, or the adapter
    /// would go on stopping at the one that was taken away.
    pub fn set_breakpoints(&mut self, path: &str, breakpoints: Vec<SourceBreakpoint>) -> Outcome {
        match self.breakpoints.iter_mut().find(|(known, _)| known == path) {
            Some(entry) => entry.1 = breakpoints.clone(),
            None => self.breakpoints.push((path.to_owned(), breakpoints.clone())),
        }
        // Before the adapter has said it is ready, they are only remembered: they go out in
        // `Configuring`, all of them, in one pass.
        if matches!(self.state, State::Starting | State::Configuring | State::Ended) {
            return Outcome::default();
        }
        self.ask(
            Request::SetBreakpoints { path: path.to_owned(), breakpoints },
            Awaiting::Breakpoints(path.to_owned()),
        )
    }

    /// Which of the adapter's own exception filters are switched on.
    ///
    /// Quill holds no list of its own — the names come from the capabilities — so an adapter that
    /// offers none gets no request and no control.
    pub fn set_exception_filters(&mut self, filters: Vec<String>) -> Outcome {
        self.exception_filters = filters.clone();
        if self.capabilities.exception_filters.is_empty() || !self.state.is_alive() {
            return Outcome::default();
        }
        if matches!(self.state, State::Starting) {
            return Outcome::default();
        }
        self.ask(Request::SetExceptionBreakpoints { filters }, Awaiting::Nothing)
    }

    /// Start: send `initialize`, and nothing else until it is answered.
    pub fn begin(&mut self) -> Outcome {
        self.ask(
            Request::Initialize {
                client_id: "quill".to_owned(),
                lines_start_at_one: true,
            },
            Awaiting::Initialize,
        )
    }

    /// One of the five stepping requests, about the thread that stopped.
    ///
    /// Refused with a sentence rather than sent into the dark when there is nothing to step: a
    /// session that is running has no stopped thread, and asking it to step over would be a request
    /// no adapter can answer.
    pub fn step(&mut self, step: Step) -> Result<Outcome, String> {
        let Some(thread) = self.thread.or_else(|| self.threads.first().map(|one| one.id)) else {
            return Err("Nothing is stopped, so there is nothing to step.".to_owned());
        };
        match step {
            Step::Pause if self.state != State::Running => {
                return Err("The program is not running.".to_owned())
            }
            Step::Pause => {}
            _ if self.state != State::Paused => {
                return Err("The program is not stopped.".to_owned())
            }
            _ => {}
        }
        let request = step.request(thread);
        // The references died the moment the program was told to go on, so the caller is told to
        // throw its cache away before the answer arrives rather than after.
        if request.resumes() {
            self.state = State::Running;
            self.frame = None;
            self.location = None;
            // What the last stop was still reading is about a frame that has gone.
            self.reading.clear();
        }
        let mut outcome = self.ask(request, Awaiting::Stepping(step));
        if step != Step::Pause {
            outcome.events.push(Event::Running);
        }
        Ok(outcome)
    }

    /// Read the children of one reference, which is what opening a row in the tree means.
    ///
    /// **Nothing is fetched that is not on screen.** That is the frame-cost rule applied to a
    /// protocol that was designed for it: a structure a thousand deep costs the one level somebody
    /// opened.
    pub fn expand(&mut self, reference: i64) -> Outcome {
        if reference == 0 || !self.state.is_paused() {
            return Outcome::default();
        }
        self.ask(Request::Variables { reference }, Awaiting::Variables { reference })
    }

    /// Read one frame's scopes, which is what clicking a frame means.
    ///
    /// The execution point and the variables both move to that frame, and the program stays exactly
    /// where it is — the reference editor's behaviour, and the protocol's: reading a frame is not resuming.
    pub fn show_frame(&mut self, frame: i64) -> Outcome {
        if !self.state.is_paused() {
            return Outcome::default();
        }
        self.frame = Some(frame);
        self.ask(Request::Scopes { frame }, Awaiting::Scopes { frame })
    }

    /// Evaluate an expression. `id` comes back with the answer, so a watch list can put each result
    /// in the row that asked for it.
    ///
    /// `context` is `watch` for the watch list and `repl` for the expression box, which is the
    /// distinction the specification draws and which adapters really act on — a `repl` evaluation is
    /// allowed to have side effects and a `watch` is not.
    pub fn evaluate(&mut self, id: u64, expression: &str, context: &str) -> Outcome {
        if !self.state.is_alive() {
            return Outcome::default();
        }
        self.ask(
            Request::Evaluate {
                expression: expression.to_owned(),
                frame: self.frame,
                context: context.to_owned(),
            },
            Awaiting::Evaluate { id },
        )
    }

    /// Change a variable in the running program.
    ///
    /// **Offered only when the adapter said it can**, which is the rule every optional feature here
    /// follows: a control whose capability is absent is absent, so this is never reached from the
    /// window — and a caller that asks anyway is refused rather than left waiting for an answer that
    /// will never come.
    pub fn set_variable(
        &mut self,
        reference: i64,
        name: &str,
        value: &str,
    ) -> Result<Outcome, String> {
        if !self.capabilities.set_variable {
            return Err("This debugger cannot change a value while the program is running."
                .to_owned());
        }
        if !self.state.is_paused() {
            return Err("The program is not stopped.".to_owned());
        }
        Ok(self.ask(
            Request::SetVariable {
                reference,
                name: name.to_owned(),
                value: value.to_owned(),
            },
            Awaiting::SetVariable { reference, name: name.to_owned() },
        ))
    }

    /// Assign to whatever an expression names in the running program.
    ///
    /// **The other half of [`Session::set_variable`], and each is used where it is the only one that
    /// can do the job.** A row that was read out of `variables` has a container reference and a name
    /// and is set by `setVariable`; a value that was reached by `evaluate` — the root of a value
    /// tooltip, a watch — has neither, and `setExpression` names it by the expression itself.
    ///
    /// Gated on the capability for the reason every optional feature here is: a control whose
    /// capability is absent is absent, so this is not reached from the window at all, and a caller
    /// that asks anyway is refused rather than left waiting for an answer that will never come.
    pub fn set_expression(&mut self, expression: &str, value: &str) -> Result<Outcome, String> {
        if !self.capabilities.set_expression {
            return Err(
                "This debugger cannot assign to an expression while the program is running."
                    .to_owned(),
            );
        }
        if !self.state.is_paused() {
            return Err("The program is not stopped.".to_owned());
        }
        Ok(self.ask(
            Request::SetExpression {
                expression: expression.to_owned(),
                value: value.to_owned(),
                frame: self.frame,
            },
            Awaiting::SetExpression { expression: expression.to_owned() },
        ))
    }

    /// End it, politely.
    ///
    /// `terminate` first when the adapter offered it — the graceful request, which lets a program
    /// tidy up — and `disconnect` otherwise or on the second press. The hard kill of the adapter's
    /// own process is `Client`'s, after the grace, which is the run tile's exact arrangement.
    pub fn stop(&mut self, hard: bool) -> Outcome {
        if !self.state.is_alive() {
            return Outcome::default();
        }
        let request = match self.capabilities.terminate_request && !hard {
            true => Request::Terminate,
            false => Request::Disconnect { terminate_debuggee: true },
        };
        self.ask(request, Awaiting::Ending)
    }

    /// The answer to the adapter's `runInTerminal`: whether the command started, and its process id
    /// if the caller has one to give.
    pub fn answer_run_in_terminal(
        &mut self,
        request_seq: i64,
        started: bool,
        process: Option<u32>,
    ) -> Outcome {
        let seq = self.take_seq();
        Outcome {
            frames: vec![messages::run_in_terminal_response(seq, request_seq, started, process)],
            events: Vec::new(),
        }
    }

    /// The answer to the adapter's `startDebugging`: the client has opened the child session.
    ///
    /// Written on the **parent's** connection, because that is where the request came from — which
    /// is why the client keeps the parent's writer once it has moved to the child.
    pub fn answer_start_debugging(&mut self, request_seq: i64, opened: bool) -> Value {
        let seq = self.take_seq();
        messages::start_debugging_response(seq, request_seq, opened)
    }

    /// The adapter has gone — its process died, or the pipe broke.
    ///
    /// An ending rather than an error, because a debuggee that ran to completion takes its adapter
    /// with it and that is the ordinary way a session finishes.
    pub fn adapter_gone(&mut self) -> Outcome {
        if self.state == State::Ended {
            return Outcome::default();
        }
        self.state = State::Ended;
        self.frame = None;
        self.location = None;
        Outcome { frames: Vec::new(), events: vec![Event::Ended { code: self.exit_code }] }
    }

    /// One message from the adapter.
    pub fn on_message(&mut self, message: Message) -> Outcome {
        match message {
            Message::Response { request_seq, command, success, message, body, .. } => {
                self.on_response(request_seq, &command, success, message, body)
            }
            Message::Initialized => self.on_initialized(),
            Message::Stopped(stopped) => self.on_stopped(stopped),
            Message::Continued { .. } => {
                // Some adapters send this and some do not, so nothing depends on it: the state has
                // usually moved already, on the stepping request itself.
                if self.state == State::Paused {
                    self.state = State::Running;
                    self.frame = None;
                    self.location = None;
                    return Outcome { frames: Vec::new(), events: vec![Event::Running] };
                }
                Outcome::default()
            }
            Message::Output { kind, text } => {
                Outcome { frames: Vec::new(), events: vec![Event::Output { kind, text }] }
            }
            Message::BreakpointChanged(changed) => Outcome {
                frames: Vec::new(),
                events: vec![Event::BreakpointChanged(changed)],
            },
            Message::Exited { code } => {
                self.exit_code = Some(code);
                Outcome::default()
            }
            Message::Terminated => {
                self.state = State::Ended;
                self.frame = None;
                self.location = None;
                Outcome { frames: Vec::new(), events: vec![Event::Ended { code: self.exit_code }] }
            }
            Message::RunInTerminal { seq, title, cwd, args, env, .. } => {
                self.runs_in_terminal = true;
                Outcome {
                    frames: Vec::new(),
                    events: vec![Event::RunInTerminal { seq, title, cwd, args, env }],
                }
            }
            Message::StartDebugging { seq, request, configuration } => Outcome {
                frames: Vec::new(),
                events: vec![Event::StartDebugging { seq, request, configuration }],
            },
            Message::Other { .. } => Outcome::default(),
        }
    }

    /// The adapter is ready to be told where to stop.
    ///
    /// Everything goes out in one pass — every file that has a breakpoint, the exception filters the
    /// adapter offered, then `configurationDone` — because the adapter will not start the program
    /// until the last of them arrives.
    fn on_initialized(&mut self) -> Outcome {
        self.state = State::Configuring;
        let mut outcome = Outcome::default();
        let files = self.breakpoints.clone();
        for (path, breakpoints) in files {
            outcome.absorb(self.ask(
                Request::SetBreakpoints { path: path.clone(), breakpoints },
                Awaiting::Breakpoints(path),
            ));
        }
        if !self.capabilities.exception_filters.is_empty() {
            let filters = self.exception_filters.clone();
            outcome.absorb(
                self.ask(Request::SetExceptionBreakpoints { filters }, Awaiting::Nothing),
            );
        }
        // Asked for only when the adapter said it takes one. An adapter that did not offer it
        // starts the moment the breakpoints are in, which is what the specification says.
        if self.capabilities.configuration_done {
            outcome.absorb(self.ask(Request::ConfigurationDone, Awaiting::ConfigurationDone));
        } else {
            self.state = State::Running;
            outcome.events.push(Event::Running);
        }
        outcome
    }

    /// The program stopped. Ask for the four things every stop needs.
    fn on_stopped(&mut self, stopped: Stopped) -> Outcome {
        self.state = State::Paused;
        self.thread = stopped.thread.or(self.thread);
        self.frame = None;
        self.location = None;
        // A stop that arrives while the last one is still being read starts the count again: what is
        // outstanding belongs to a frame that has gone.
        self.reading.clear();
        let mut outcome =
            Outcome { frames: Vec::new(), events: vec![Event::Stopped(stopped)] };
        outcome.absorb(self.ask_reading(Request::Threads, Awaiting::Threads));
        if let Some(thread) = self.thread {
            outcome.absorb(self.ask_reading(
                Request::StackTrace { thread, levels: STACK_LIMIT },
                Awaiting::StackTrace,
            ));
        }
        outcome
    }

    fn on_response(
        &mut self,
        request_seq: i64,
        command: &str,
        success: bool,
        message: Option<String>,
        body: Value,
    ) -> Outcome {
        let awaiting = self.take_awaiting(request_seq);
        self.reading.retain(|seq| *seq != request_seq);
        if !success {
            return self.on_refusal(command, awaiting, message);
        }
        match awaiting {
            Some(Awaiting::Initialize) => {
                self.capabilities = messages::read_capabilities(&body);
                let mut outcome =
                    Outcome { frames: Vec::new(), events: vec![Event::Ready] };
                let launch = self.launch.clone();
                outcome.absorb(self.ask(Request::Launch(launch), Awaiting::Launch));
                outcome
            }
            // The launch response arrives after `configurationDone` on most adapters and before it
            // on some, so nothing about the state depends on it. What it *does* say is that the
            // program was accepted.
            Some(Awaiting::Launch) => Outcome::default(),
            Some(Awaiting::Breakpoints(path)) => Outcome {
                frames: Vec::new(),
                events: vec![Event::Breakpoints {
                    path,
                    answered: messages::read_breakpoints(&body),
                }],
            },
            Some(Awaiting::ConfigurationDone) => {
                // A `stopped` can arrive before this answer does — a program that hits a breakpoint
                // in the first microsecond of `main` — so the state is only moved on when nothing
                // has already moved it. Without this the session would say "running" while the
                // program was plainly stopped at a breakpoint.
                if self.state == State::Configuring {
                    self.state = State::Running;
                    return Outcome { frames: Vec::new(), events: vec![Event::Running] };
                }
                Outcome::default()
            }
            Some(Awaiting::Threads) => {
                self.threads = messages::read_threads(&body);
                if self.thread.is_none() {
                    self.thread = self.threads.first().map(|one| one.id);
                }
                Outcome {
                    frames: Vec::new(),
                    events: vec![Event::Threads(self.threads.clone())],
                }
            }
            Some(Awaiting::StackTrace) => {
                let frames = messages::read_frames(&body);
                self.location =
                    frames.first().and_then(|top| top.path.clone().map(|path| (path, top.line)));
                let top = frames.first().map(|frame| frame.id);
                let mut outcome =
                    Outcome { frames: Vec::new(), events: vec![Event::Frames(frames)] };
                // The top frame's scopes, unprompted: it is what the pane opens showing, so asking
                // for it here saves a round trip nobody would have chosen to wait for.
                if let Some(top) = top {
                    self.frame = Some(top);
                    outcome.absorb(
                        self.ask_reading(Request::Scopes { frame: top }, Awaiting::Scopes { frame: top }),
                    );
                }
                outcome
            }
            Some(Awaiting::Scopes { frame }) => {
                let scopes = messages::read_scopes(&body);
                let mut outcome = Outcome {
                    frames: Vec::new(),
                    events: vec![Event::Scopes { frame, scopes: scopes.clone() }],
                };
                // The first level of the first scope that is not expensive, and nothing else. That
                // is the one the pane shows open; Registers, which lldb marks expensive, waits to be
                // asked for.
                if let Some(first) = scopes.iter().find(|scope| !scope.expensive) {
                    let reference = first.reference;
                    outcome.absorb(self.ask_reading(
                        Request::Variables { reference },
                        Awaiting::Variables { reference },
                    ));
                }
                outcome
            }
            Some(Awaiting::Variables { reference }) => Outcome {
                frames: Vec::new(),
                events: vec![Event::Variables {
                    reference,
                    variables: messages::read_variables(&body),
                }],
            },
            Some(Awaiting::SetVariable { reference, name }) => {
                let mut answered = messages::read_set_variable(&body);
                answered.name = name.clone();
                Outcome {
                    frames: Vec::new(),
                    events: vec![Event::VariableSet { reference, name, result: Ok(answered) }],
                }
            }
            Some(Awaiting::SetExpression { expression }) => Outcome {
                frames: Vec::new(),
                events: vec![Event::ExpressionSet {
                    expression,
                    result: Ok(messages::read_set_variable(&body)),
                }],
            },
            Some(Awaiting::Evaluate { id }) => Outcome {
                frames: Vec::new(),
                events: vec![Event::Evaluated {
                    id,
                    result: Ok(messages::read_evaluate(&body)),
                }],
            },
            Some(Awaiting::Stepping(_)) | Some(Awaiting::Ending) | Some(Awaiting::Nothing)
            | None => Outcome::default(),
        }
    }

    /// The adapter refused something.
    ///
    /// **Its own message, never one of Quill's** — a debugger explains a bad expression or a
    /// constant that cannot be assigned to far better than an editor could, which is the rule
    /// `quill-git` already keeps about git's standard error.
    fn on_refusal(
        &mut self,
        command: &str,
        awaiting: Option<Awaiting>,
        message: Option<String>,
    ) -> Outcome {
        let said = message.unwrap_or_else(|| format!("The debugger refused {command}."));
        let event = match awaiting {
            Some(Awaiting::Evaluate { id }) => Event::Evaluated { id, result: Err(said) },
            Some(Awaiting::SetVariable { reference, name }) => Event::VariableSet {
                reference,
                name,
                result: Err(said),
            },
            Some(Awaiting::SetExpression { expression }) => {
                Event::ExpressionSet { expression, result: Err(said) }
            }
            _ => Event::Failed { command: command.to_owned(), message: said },
        };
        Outcome { frames: Vec::new(), events: vec![event] }
    }

    /// Number a request, remember what it was, and answer with the frame to write.
    fn ask(&mut self, request: Request, awaiting: Awaiting) -> Outcome {
        self.ask_marked(request, awaiting, false)
    }

    /// The same, for one of the requests a **stop itself** makes. See [`Session::has_read_the_stop`].
    fn ask_reading(&mut self, request: Request, awaiting: Awaiting) -> Outcome {
        self.ask_marked(request, awaiting, true)
    }

    fn ask_marked(&mut self, request: Request, awaiting: Awaiting, reading: bool) -> Outcome {
        let seq = self.take_seq();
        let frame = request.to_value(seq);
        if awaiting != Awaiting::Nothing {
            self.awaiting.push((seq, awaiting));
        }
        if reading {
            self.reading.push(seq);
        }
        Outcome { frames: vec![frame], events: Vec::new() }
    }

    fn take_seq(&mut self) -> i64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    fn take_awaiting(&mut self, seq: i64) -> Option<Awaiting> {
        let at = self.awaiting.iter().position(|(known, _)| *known == seq)?;
        Some(self.awaiting.remove(at).1)
    }
}

/// The last part of a path, which is what a header says rather than the whole of it.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// What the adapter would have said, so a whole lifecycle is a list of values with no process
    /// anywhere near it.
    fn response(request_seq: i64, command: &str, body: Value) -> Message {
        Message::Response {
            seq: 100 + request_seq,
            request_seq,
            command: command.to_owned(),
            success: true,
            message: None,
            body,
        }
    }

    fn refusal(request_seq: i64, command: &str, said: &str) -> Message {
        Message::Response {
            seq: 100 + request_seq,
            request_seq,
            command: command.to_owned(),
            success: false,
            message: Some(said.to_owned()),
            body: Value::Null,
        }
    }

    /// Everything an ordinary adapter offers.
    fn full_capabilities() -> Value {
        json!({
            "supportsConfigurationDoneRequest": true,
            "supportsSetVariable": true,
            "supportsConditionalBreakpoints": true,
            "supportsLogPoints": true,
            "supportsTerminateRequest": true,
            "supportsEvaluateForHovers": true,
            "supportsSetExpression": true,
        })
    }

    /// A paused session whose adapter answered `initialize` with the bare minimum: no
    /// `setExpression`, no `evaluate` for hovers. `task-1696` §7's absent control is tested against
    /// this one.
    fn plain_session() -> Session {
        let mut session = Session::new(json!({ "program": "app.exe" }));
        let opening = session.begin();
        session.on_message(response(
            seq_of(&opening, "initialize"),
            "initialize",
            json!({ "supportsConfigurationDoneRequest": true }),
        ));
        let configuring = session.on_message(Message::Initialized);
        session.on_message(response(
            seq_of(&configuring, "configurationDone"),
            "configurationDone",
            Value::Null,
        ));
        let stopped = session.on_message(Message::Stopped(Stopped {
            reason: "breakpoint".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        session.on_message(response(
            seq_of(&stopped, "threads"),
            "threads",
            json!({ "threads": [{ "id": 1, "name": "main" }] }),
        ));
        session.on_message(response(
            seq_of(&stopped, "stackTrace"),
            "stackTrace",
            json!({ "stackFrames": [
                { "id": 1000, "name": "main", "line": 14, "source": { "path": "src/main.rs" } }
            ]}),
        ));
        session
    }

    fn commands(outcome: &Outcome) -> Vec<String> {
        outcome
            .frames
            .iter()
            .map(|frame| frame["command"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn seq_of(outcome: &Outcome, command: &str) -> i64 {
        outcome
            .frames
            .iter()
            .find(|frame| frame["command"] == command)
            .and_then(|frame| frame["seq"].as_i64())
            .unwrap_or_else(|| panic!("a {command} was sent"))
    }

    /// Drive a session from nothing to a stop at a breakpoint, which is the whole happy lifecycle.
    fn paused_session() -> Session {
        let mut session = Session::new(json!({ "program": "app.exe" }));
        session.set_breakpoints("src/main.rs", vec![SourceBreakpoint::at(14)]);
        let opening = session.begin();
        let initialize = seq_of(&opening, "initialize");
        session.on_message(response(initialize, "initialize", full_capabilities()));
        let configuring = session.on_message(Message::Initialized);
        let done = seq_of(&configuring, "configurationDone");
        let breakpoints = seq_of(&configuring, "setBreakpoints");
        session.on_message(response(
            breakpoints,
            "setBreakpoints",
            json!({ "breakpoints": [{ "verified": true, "line": 14 }] }),
        ));
        session.on_message(response(done, "configurationDone", Value::Null));
        let stopped = session.on_message(Message::Stopped(Stopped {
            reason: "breakpoint".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        let threads = seq_of(&stopped, "threads");
        let stack = seq_of(&stopped, "stackTrace");
        session.on_message(response(
            threads,
            "threads",
            json!({ "threads": [{ "id": 1, "name": "main" }] }),
        ));
        let scopes = session.on_message(response(
            stack,
            "stackTrace",
            json!({ "stackFrames": [
                { "id": 1000, "name": "main", "line": 14, "source": { "path": "src/main.rs" } }
            ]}),
        ));
        let scopes_seq = seq_of(&scopes, "scopes");
        let variables = session.on_message(response(
            scopes_seq,
            "scopes",
            json!({ "scopes": [{ "name": "Locals", "variablesReference": 7, "expensive": false }] }),
        ));
        // And the first scope's own variables, which is the last of the four requests a stop makes —
        // so this really is a settled stop rather than one that is still being read.
        session.on_message(response(
            seq_of(&variables, "variables"),
            "variables",
            json!({ "variables": [{ "name": "count", "value": "3", "type": "i32" }] }),
        ));
        session
    }

    #[test]
    fn a_new_session_starts_by_asking_what_the_adapter_can_do() {
        let mut session = Session::new(Value::Null);
        assert_eq!(session.state(), State::Starting);
        let opening = session.begin();
        assert_eq!(commands(&opening), vec!["initialize"]);
        assert_eq!(opening.frames[0]["seq"], 1, "the protocol says seq starts at one");
        assert_eq!(opening.frames[0]["arguments"]["linesStartAt1"], true);
        assert_eq!(
            opening.frames[0]["arguments"]["supportsRunInTerminalRequest"], true,
            "asked for, which is what puts a real terminal behind the debuggee"
        );
    }

    /// The order is the protocol's: launch, then the `initialized` event, then the breakpoints, then
    /// `configurationDone`. An adapter sees nothing about where to stop until it has said it is
    /// ready.
    #[test]
    fn the_lifecycle_runs_in_the_protocols_own_order() {
        let mut session = Session::new(json!({ "program": "app.exe" }));
        session.set_breakpoints("a.rs", vec![SourceBreakpoint::at(4)]);
        let opening = session.begin();
        assert_eq!(commands(&opening), vec!["initialize"], "nothing else until it is answered");
        let answered =
            session.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        assert_eq!(commands(&answered), vec!["launch"]);
        assert_eq!(answered.events, vec![Event::Ready]);
        assert_eq!(answered.frames[0]["arguments"]["program"], "app.exe", "the caller's own body");
        let configuring = session.on_message(Message::Initialized);
        assert_eq!(session.state(), State::Configuring);
        assert_eq!(commands(&configuring), vec!["setBreakpoints", "configurationDone"]);
        session.on_message(response(seq_of(&configuring, "configurationDone"), "configurationDone", Value::Null));
        assert_eq!(session.state(), State::Running);
    }

    /// An adapter that does not take `configurationDone` is running the moment its breakpoints are
    /// in, which is what the specification says and what a client that always sent it would hang on.
    #[test]
    fn an_adapter_that_takes_no_configuration_done_is_never_sent_one() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(
            seq_of(&opening, "initialize"),
            "initialize",
            json!({ "supportsSetVariable": true }),
        ));
        let configuring = session.on_message(Message::Initialized);
        assert!(
            !commands(&configuring).contains(&"configurationDone".to_owned()),
            "never sent what the adapter did not offer"
        );
        assert_eq!(session.state(), State::Running);
        assert!(configuring.events.contains(&Event::Running));
    }

    /// A program that hits a breakpoint in the first microsecond of `main` can stop before the
    /// `configurationDone` answer arrives. The session must not then say it is running.
    #[test]
    fn a_stop_that_arrives_before_configuration_done_is_not_undone_by_it() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        let configuring = session.on_message(Message::Initialized);
        let done = seq_of(&configuring, "configurationDone");
        session.on_message(Message::Stopped(Stopped {
            reason: "breakpoint".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        assert_eq!(session.state(), State::Paused);
        let late = session.on_message(response(done, "configurationDone", Value::Null));
        assert_eq!(session.state(), State::Paused, "the stop is what is true");
        assert!(!late.events.contains(&Event::Running));
    }

    /// The four requests every stop needs, made without anybody asking for them.
    #[test]
    fn a_stop_reads_the_threads_the_stack_the_top_frames_scopes_and_its_first_variables() {
        let session = paused_session();
        assert_eq!(session.state(), State::Paused);
        assert_eq!(session.thread(), Some(1));
        assert_eq!(session.frame(), Some(1000), "the top frame, chosen for you");
        assert_eq!(session.location(), Some(("src/main.rs", 14)));
        assert_eq!(session.where_it_is(), "paused at main.rs:14");
    }

    #[test]
    fn the_first_scopes_first_level_is_read_and_an_expensive_one_is_left_alone() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        let configuring = session.on_message(Message::Initialized);
        session.on_message(response(seq_of(&configuring, "configurationDone"), "configurationDone", Value::Null));
        let stopped = session.on_message(Message::Stopped(Stopped {
            reason: "step".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        let stack = seq_of(&stopped, "stackTrace");
        let after_stack = session.on_message(response(
            stack,
            "stackTrace",
            json!({ "stackFrames": [{ "id": 5, "name": "f", "line": 2, "source": { "path": "a.rs" } }] }),
        ));
        let scopes = seq_of(&after_stack, "scopes");
        let after_scopes = session.on_message(response(
            scopes,
            "scopes",
            json!({ "scopes": [
                { "name": "Registers", "variablesReference": 3, "expensive": true },
                { "name": "Locals", "variablesReference": 4, "expensive": false }
            ]}),
        ));
        let asked: Vec<i64> = after_scopes
            .frames
            .iter()
            .filter(|frame| frame["command"] == "variables")
            .map(|frame| frame["arguments"]["variablesReference"].as_i64().unwrap_or(0))
            .collect();
        assert_eq!(asked, vec![4], "Registers is expensive, so it waits to be asked for");
    }

    /// Nothing is fetched that is not on screen, which is the whole of the lazy model.
    #[test]
    fn a_row_with_children_is_read_only_when_it_is_opened() {
        let mut session = paused_session();
        let opened = session.expand(17);
        assert_eq!(commands(&opened), vec!["variables"]);
        assert_eq!(opened.frames[0]["arguments"]["variablesReference"], 17);
        let answer = session.on_message(response(
            seq_of(&opened, "variables"),
            "variables",
            json!({ "variables": [{ "name": "x", "value": "1", "variablesReference": 0 }] }),
        ));
        let Event::Variables { reference, variables } = &answer.events[0] else {
            panic!("the children of the row that was opened");
        };
        assert_eq!(*reference, 17, "the answer knows which row asked");
        assert_eq!(variables[0].name, "x");
    }

    #[test]
    fn a_row_with_no_children_is_never_asked_about() {
        let mut session = paused_session();
        assert!(session.expand(0).frames.is_empty(), "a reference of zero means no children");
    }

    #[test]
    fn the_five_steps_send_the_five_requests_they_are_named_after() {
        for (step, command) in [
            (Step::Resume, "continue"),
            (Step::Over, "next"),
            (Step::Into, "stepIn"),
            (Step::Out, "stepOut"),
        ] {
            let mut session = paused_session();
            let outcome = session.step(step).expect("paused, so it can step");
            assert_eq!(commands(&outcome), vec![command.to_owned()]);
            assert_eq!(outcome.frames[0]["arguments"]["threadId"], 1);
            assert_eq!(session.state(), State::Running, "{command} resumes the program");
            assert!(outcome.events.contains(&Event::Running));
        }
    }

    #[test]
    fn stepping_a_program_that_is_not_stopped_is_refused_with_a_sentence() {
        let mut session = paused_session();
        session.step(Step::Resume).expect("the first one works");
        let problem = session.step(Step::Over).expect_err("it is running now");
        assert!(problem.contains("not stopped"), "{problem}");
    }

    #[test]
    fn pausing_is_the_other_way_round() {
        let mut session = paused_session();
        assert!(session.step(Step::Pause).is_err(), "it is already stopped");
        session.step(Step::Resume).expect("resume");
        let paused = session.step(Step::Pause).expect("running, so it can be paused");
        assert_eq!(commands(&paused), vec!["pause"]);
        assert_eq!(session.state(), State::Running, "pausing does not itself stop anything");
    }

    /// Resuming throws away what the client knew about the frame, because every
    /// `variablesReference` it is holding has just stopped meaning anything.
    #[test]
    fn resuming_lets_go_of_the_frame_and_the_execution_point() {
        let mut session = paused_session();
        assert!(session.location().is_some());
        session.step(Step::Over).expect("step");
        assert!(session.frame().is_none());
        assert!(session.location().is_none());
        assert!(session.expand(7).frames.is_empty(), "and nothing deeper is asked for");
    }

    #[test]
    fn a_continued_event_moves_the_state_even_when_no_step_was_asked_for() {
        let mut session = paused_session();
        let going = session.on_message(Message::Continued { thread: Some(1), all_threads: true });
        assert_eq!(session.state(), State::Running);
        assert_eq!(going.events, vec![Event::Running]);
    }

    /// Clicking a frame moves the variables and the execution point without resuming, which is
    /// The reference editor's behaviour.
    #[test]
    fn showing_another_frame_reads_its_scopes_and_does_not_resume() {
        let mut session = paused_session();
        let outcome = session.show_frame(1001);
        assert_eq!(commands(&outcome), vec!["scopes"]);
        assert_eq!(outcome.frames[0]["arguments"]["frameId"], 1001);
        assert_eq!(session.state(), State::Paused);
        assert_eq!(session.frame(), Some(1001));
    }

    /// Full replacement per file is what the protocol's `setBreakpoints` is, so a file with none
    /// left is still sent — with an empty list, which is what takes the old one away.
    #[test]
    fn changing_a_files_breakpoints_while_it_runs_resends_that_file_whole() {
        let mut session = paused_session();
        let outcome = session.set_breakpoints("src/main.rs", Vec::new());
        assert_eq!(commands(&outcome), vec!["setBreakpoints"]);
        assert_eq!(outcome.frames[0]["arguments"]["breakpoints"].as_array().map(Vec::len), Some(0));
        assert_eq!(outcome.frames[0]["arguments"]["source"]["path"], "src/main.rs");
    }

    #[test]
    fn breakpoints_set_before_the_adapter_is_ready_are_only_remembered() {
        let mut session = Session::new(Value::Null);
        assert!(
            session.set_breakpoints("a.rs", vec![SourceBreakpoint::at(1)]).frames.is_empty(),
            "nothing is sent to an adapter that has not said it is ready"
        );
        let opening = session.begin();
        session.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        let configuring = session.on_message(Message::Initialized);
        assert!(commands(&configuring).contains(&"setBreakpoints".to_owned()));
    }

    /// Quill draws the adapter's answer rather than its own hope: an unbound breakpoint stays
    /// unverified, and one the adapter moved is reported where it really landed.
    #[test]
    fn what_the_adapter_says_about_a_breakpoint_reaches_the_caller_unchanged() {
        let mut session = paused_session();
        let asked = session.set_breakpoints("b.rs", vec![SourceBreakpoint::at(3)]);
        let answered = session.on_message(response(
            seq_of(&asked, "setBreakpoints"),
            "setBreakpoints",
            json!({ "breakpoints": [{ "verified": false, "message": "no code on that line" }] }),
        ));
        let Event::Breakpoints { path, answered } = &answered.events[0] else {
            panic!("the answer about one file");
        };
        assert_eq!(path, "b.rs");
        assert!(!answered[0].verified);
        assert_eq!(answered[0].message.as_deref(), Some("no code on that line"));
    }

    /// Capability gating, which is the rule every optional feature here follows.
    #[test]
    fn set_variable_is_never_sent_to_an_adapter_that_did_not_offer_it() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(
            seq_of(&opening, "initialize"),
            "initialize",
            json!({ "supportsConfigurationDoneRequest": true }),
        ));
        let problem = session.set_variable(7, "x", "1").expect_err("not offered");
        assert!(problem.contains("cannot change a value"), "{problem}");
    }

    #[test]
    fn set_variable_answers_with_what_the_debugger_now_holds_rather_than_what_was_typed() {
        let mut session = paused_session();
        let asked = session.set_variable(7, "x", "1.10").expect("offered and paused");
        let answered = session.on_message(response(
            seq_of(&asked, "setVariable"),
            "setVariable",
            json!({ "value": "1.1000000000000001", "type": "f64" }),
        ));
        let Event::VariableSet { reference, name, result } = &answered.events[0] else {
            panic!("the value as the debugger sees it");
        };
        assert_eq!(*reference, 7);
        assert_eq!(name, "x");
        assert_eq!(result.as_ref().expect("it worked").value, "1.1000000000000001");
    }

    /// A refusal is the adapter's own sentence, put on the row that asked.
    #[test]
    fn a_refused_assignment_carries_the_debuggers_own_message_to_the_row() {
        let mut session = paused_session();
        let asked = session.set_variable(7, "PI", "3").expect("offered");
        let answered = session.on_message(refusal(
            seq_of(&asked, "setVariable"),
            "setVariable",
            "cannot assign to a constant",
        ));
        let Event::VariableSet { result, .. } = &answered.events[0] else { panic!("the refusal") };
        assert_eq!(result.as_ref().expect_err("refused"), "cannot assign to a constant");
    }

    #[test]
    fn a_watch_answer_finds_the_row_that_asked_for_it() {
        let mut session = paused_session();
        let first = session.evaluate(1, "count", "watch");
        let second = session.evaluate(2, "items.len()", "watch");
        // Answered out of order, which is what a debugger evaluating two expressions really does.
        let later = session.on_message(response(
            seq_of(&second, "evaluate"),
            "evaluate",
            json!({ "result": "2", "type": "usize" }),
        ));
        assert_eq!(later.events, vec![Event::Evaluated {
            id: 2,
            result: Ok(Variable {
                name: String::new(),
                value: "2".to_owned(),
                kind: Some("usize".to_owned()),
                reference: 0,
                evaluate_name: None,
            }),
        }]);
        let earlier = session.on_message(refusal(
            seq_of(&first, "evaluate"),
            "evaluate",
            "no symbol named count",
        ));
        let Event::Evaluated { id, result } = &earlier.events[0] else { panic!("the first") };
        assert_eq!(*id, 1);
        assert_eq!(result.as_ref().expect_err("refused"), "no symbol named count");
    }

    /// A watch is asked in the frame that is showing, so clicking a frame and looking again is a
    /// different answer — which is what a watch is for.
    #[test]
    fn a_watch_is_evaluated_in_the_frame_that_is_showing() {
        let mut session = paused_session();
        let asked = session.evaluate(1, "x", "watch");
        assert_eq!(asked.frames[0]["arguments"]["frameId"], 1000);
        session.show_frame(1001);
        let again = session.evaluate(2, "x", "watch");
        assert_eq!(again.frames[0]["arguments"]["frameId"], 1001);
    }

    #[test]
    fn stopping_asks_politely_first_and_hard_after() {
        let mut session = paused_session();
        let polite = session.stop(false);
        assert_eq!(commands(&polite), vec!["terminate"], "the adapter offered it");
        let hard = session.stop(true);
        assert_eq!(commands(&hard), vec!["disconnect"]);
        assert_eq!(hard.frames[0]["arguments"]["terminateDebuggee"], true);
    }

    #[test]
    fn an_adapter_that_cannot_terminate_is_disconnected_from_the_first_time() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(
            seq_of(&opening, "initialize"),
            "initialize",
            json!({ "supportsConfigurationDoneRequest": true }),
        ));
        assert_eq!(commands(&session.stop(false)), vec!["disconnect"]);
    }

    #[test]
    fn a_terminated_event_ends_the_session_with_the_code_the_program_chose() {
        let mut session = paused_session();
        session.on_message(Message::Exited { code: 101 });
        let ended = session.on_message(Message::Terminated);
        assert_eq!(session.state(), State::Ended);
        assert_eq!(ended.events, vec![Event::Ended { code: Some(101) }]);
        assert_eq!(session.where_it_is(), "ended with exit code 101");
        assert!(session.location().is_none());
    }

    /// An adapter that dies in the middle of a session is an ending rather than a crash: a debuggee
    /// that ran to completion takes its adapter with it, which is how a session usually finishes.
    #[test]
    fn an_adapter_that_goes_away_ends_the_session_once() {
        let mut session = paused_session();
        let ended = session.adapter_gone();
        assert_eq!(session.state(), State::Ended);
        assert_eq!(ended.events, vec![Event::Ended { code: None }]);
        assert!(session.adapter_gone().is_empty(), "and only once");
    }

    #[test]
    fn nothing_is_asked_of_a_session_that_has_ended() {
        let mut session = paused_session();
        session.adapter_gone();
        assert!(session.stop(false).frames.is_empty());
        assert!(session.expand(7).frames.is_empty());
        assert!(session.evaluate(1, "x", "repl").frames.is_empty());
        assert!(session.step(Step::Resume).is_err());
    }

    #[test]
    fn the_reverse_request_is_reported_and_answered_with_a_process_id() {
        let mut session = paused_session();
        let asked = session.on_message(Message::RunInTerminal {
            seq: 40,
            kind: "integrated".to_owned(),
            title: "Debug".to_owned(),
            cwd: "C:\\p".to_owned(),
            args: vec!["app.exe".to_owned()],
            env: Vec::new(),
        });
        assert!(session.runs_in_terminal(), "the output is going to the run tile now");
        let Event::RunInTerminal { seq, args, .. } = &asked.events[0] else {
            panic!("the reverse request");
        };
        assert_eq!(*seq, 40);
        assert_eq!(args, &vec!["app.exe".to_owned()]);
        let answered = session.answer_run_in_terminal(40, true, Some(1234));
        assert_eq!(answered.frames[0]["type"], "response");
        assert_eq!(answered.frames[0]["request_seq"], 40);
        assert_eq!(answered.frames[0]["body"]["processId"], 1234);
    }

    /// js-debug's second reverse request, and the one that makes node debugging work at all: the
    /// program is in a session of its own and the client is being told to open it.
    #[test]
    fn a_child_session_is_reported_with_the_configuration_that_opens_it() {
        let mut session = paused_session();
        let asked = session.on_message(Message::StartDebugging {
            seq: 7,
            request: "launch".to_owned(),
            configuration: json!({ "type": "pwa-node", "__pendingTargetId": "abc123" }),
        });
        let Event::StartDebugging { seq, request, configuration } = &asked.events[0] else {
            panic!("the child session");
        };
        assert_eq!(*seq, 7);
        assert_eq!(request, "launch");
        // `__pendingTargetId` is the only thing tying the new connection to the program that is
        // already waiting, so it has to reach the client whole.
        assert_eq!(configuration["__pendingTargetId"], "abc123");
        let answered = session.answer_start_debugging(7, true);
        assert_eq!(answered["type"], "response");
        assert_eq!(answered["command"], "startDebugging");
        assert_eq!(answered["request_seq"], 7);
        assert_eq!(answered["success"], true);
    }

    /// The exception filters are the adapter's own, and an adapter that offers none is never sent
    /// the request at all.
    #[test]
    fn exception_filters_are_only_sent_to_an_adapter_that_offered_some() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(
            seq_of(&opening, "initialize"),
            "initialize",
            json!({
                "supportsConfigurationDoneRequest": true,
                "exceptionBreakpointFilters": [
                    { "filter": "uncaught", "label": "Uncaught", "default": true }
                ]
            }),
        ));
        assert_eq!(session.capabilities().exception_filters.len(), 1);
        let configuring = session.on_message(Message::Initialized);
        assert!(commands(&configuring).contains(&"setExceptionBreakpoints".to_owned()));

        let mut bare = Session::new(Value::Null);
        let opening = bare.begin();
        bare.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        let configuring = bare.on_message(Message::Initialized);
        assert!(
            !commands(&configuring).contains(&"setExceptionBreakpoints".to_owned()),
            "an adapter with no filters gets no request and no control"
        );
    }

    #[test]
    fn output_reaches_the_caller_with_the_stream_it_came_from() {
        let mut session = paused_session();
        let said = session.on_message(Message::Output {
            kind: OutputKind::Stderr,
            text: "boom\n".to_owned(),
        });
        assert_eq!(
            said.events,
            vec![Event::Output { kind: OutputKind::Stderr, text: "boom\n".to_owned() }]
        );
    }

    #[test]
    fn a_message_quill_does_not_read_changes_nothing() {
        let mut session = paused_session();
        let before = session.state();
        let nothing = session
            .on_message(Message::Other { kind: "event".to_owned(), name: "progress".to_owned() });
        assert!(nothing.is_empty());
        assert_eq!(session.state(), before);
    }

    /// Two `variables` requests are in flight the moment two rows are opened, so correlation has to
    /// be by seq rather than by command name.
    #[test]
    fn two_requests_of_one_kind_are_told_apart_by_their_seq() {
        let mut session = paused_session();
        let first = session.expand(11);
        let second = session.expand(22);
        let later = session.on_message(response(
            seq_of(&second, "variables"),
            "variables",
            json!({ "variables": [{ "name": "b", "value": "2" }] }),
        ));
        let Event::Variables { reference, .. } = &later.events[0] else { panic!("variables") };
        assert_eq!(*reference, 22);
        let earlier = session.on_message(response(
            seq_of(&first, "variables"),
            "variables",
            json!({ "variables": [{ "name": "a", "value": "1" }] }),
        ));
        let Event::Variables { reference, .. } = &earlier.events[0] else { panic!("variables") };
        assert_eq!(*reference, 11);
    }

    /// A stop is not read the moment it happens: the four requests it makes are round trips, and a
    /// caller that acted on the stop before they came back would find no frame and no variable.
    #[test]
    fn a_stop_is_not_read_until_the_four_requests_it_makes_have_come_back() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        let configuring = session.on_message(Message::Initialized);
        session.on_message(response(seq_of(&configuring, "configurationDone"), "configurationDone", Value::Null));
        let stopped = session.on_message(Message::Stopped(Stopped {
            reason: "breakpoint".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        assert!(session.state().is_paused());
        assert!(!session.has_read_the_stop(), "the threads and the stack are still on their way");

        session.on_message(response(
            seq_of(&stopped, "threads"),
            "threads",
            json!({ "threads": [{ "id": 1, "name": "main" }] }),
        ));
        assert!(!session.has_read_the_stop(), "and the stack still is");
        let after_stack = session.on_message(response(
            seq_of(&stopped, "stackTrace"),
            "stackTrace",
            json!({ "stackFrames": [{ "id": 5, "name": "f", "line": 2, "source": { "path": "a.rs" } }] }),
        ));
        assert!(!session.has_read_the_stop(), "the scopes were asked for by the stack's own answer");
        let after_scopes = session.on_message(response(
            seq_of(&after_stack, "scopes"),
            "scopes",
            json!({ "scopes": [{ "name": "Locals", "variablesReference": 4, "expensive": false }] }),
        ));
        assert!(!session.has_read_the_stop(), "and the variables by the scopes'");
        session.on_message(response(
            seq_of(&after_scopes, "variables"),
            "variables",
            json!({ "variables": [{ "name": "x", "value": "1" }] }),
        ));
        assert!(session.has_read_the_stop(), "now there is something to look at");
    }

    /// **An adapter that never answers must not wedge that**, which is not hypothetical: measured
    /// against CodeLLDB 1.12.3, an `evaluate` of a name that does not resolve gets a Python traceback
    /// on its standard error and no response at all. Only the stop's own requests are counted, so one
    /// bad watch expression does not make every later stop look unread for the rest of the session.
    #[test]
    fn a_request_the_adapter_never_answers_does_not_stop_a_stop_being_read() {
        let mut session = paused_session();
        assert!(session.has_read_the_stop());
        // Asked and never answered, which is what the real adapter does with this.
        session.evaluate(1, "no_such_name", "watch");
        assert!(session.is_waiting(), "the request really is outstanding");
        assert!(
            session.has_read_the_stop(),
            "but it is not one of the stop's own, so the stop is still read"
        );

        // And the next stop is read on its own terms rather than inheriting the stuck one.
        session.step(Step::Over).expect("paused");
        let stopped = session.on_message(Message::Stopped(Stopped {
            reason: "step".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        assert!(!session.has_read_the_stop());
        session.on_message(response(
            seq_of(&stopped, "threads"),
            "threads",
            json!({ "threads": [{ "id": 1, "name": "main" }] }),
        ));
        let after_stack = session.on_message(response(
            seq_of(&stopped, "stackTrace"),
            "stackTrace",
            json!({ "stackFrames": [{ "id": 9, "name": "f", "line": 3, "source": { "path": "a.rs" } }] }),
        ));
        session.on_message(response(
            seq_of(&after_stack, "scopes"),
            "scopes",
            // Every scope expensive, so nothing more is asked for and the stop is read here.
            json!({ "scopes": [{ "name": "Registers", "variablesReference": 3, "expensive": true }] }),
        ));
        assert!(session.has_read_the_stop(), "a frame with nothing cheap to read is still read");
    }

    /// A refusal counts as an answer: an adapter that says "no stack" has answered, and a caller
    /// waiting for the stop to be read must not wait for ever on it.
    #[test]
    fn a_refusal_of_one_of_the_stops_requests_is_still_an_answer() {
        let mut session = Session::new(Value::Null);
        let opening = session.begin();
        session.on_message(response(seq_of(&opening, "initialize"), "initialize", full_capabilities()));
        let configuring = session.on_message(Message::Initialized);
        session.on_message(response(seq_of(&configuring, "configurationDone"), "configurationDone", Value::Null));
        let stopped = session.on_message(Message::Stopped(Stopped {
            reason: "pause".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        session.on_message(refusal(seq_of(&stopped, "threads"), "threads", "no threads"));
        session.on_message(refusal(seq_of(&stopped, "stackTrace"), "stackTrace", "no stack"));
        assert!(session.has_read_the_stop(), "there is nothing more coming");
    }

    #[test]
    fn an_answer_to_something_nobody_is_waiting_for_is_ignored_rather_than_a_panic() {
        let mut session = paused_session();
        assert!(session.on_message(response(9999, "threads", Value::Null)).is_empty());
    }

    #[test]
    fn a_refusal_nothing_was_waiting_on_still_says_what_the_adapter_said() {
        let mut session = paused_session();
        let said = session.on_message(refusal(9999, "stackTrace", "no stack"));
        assert_eq!(
            said.events,
            vec![Event::Failed { command: "stackTrace".to_owned(), message: "no stack".to_owned() }]
        );
    }


    /// The `hover` context reaches the wire as it was given. `task-1696` §5.4: the context the
    /// specification put there for exactly this, which adapters use to answer cheaply and without
    /// side effects.
    #[test]
    fn the_hover_context_reaches_the_wire() {
        let mut session = paused_session();
        let asked = session.evaluate(1, "self.items.count", "hover");
        assert_eq!(commands(&asked), vec!["evaluate"]);
        assert_eq!(asked.frames[0]["arguments"]["context"], "hover");
        assert_eq!(asked.frames[0]["arguments"]["expression"], "self.items.count");
        assert_eq!(
            asked.frames[0]["arguments"]["frameId"], 1000,
            "resolved against the frame that is showing, or it is a different variable"
        );
    }

    /// `setExpression` is shaped as the specification says, and it carries the frame for the same
    /// reason `evaluate` does.
    #[test]
    fn set_expression_names_its_target_by_the_expression() {
        let mut session = paused_session();
        let asked = session.set_expression("self.items.count", "9").expect("offered");
        assert_eq!(commands(&asked), vec!["setExpression"]);
        assert_eq!(asked.frames[0]["arguments"]["expression"], "self.items.count");
        assert_eq!(asked.frames[0]["arguments"]["value"], "9");
        assert_eq!(asked.frames[0]["arguments"]["frameId"], 1000);
        // And the answer is the value **as the debugger now sees it**, which is `setVariable`'s own
        // rule: a debugger that rounded a float is telling the truth.
        let answered = session.on_message(response(
            seq_of(&asked, "setExpression"),
            "setExpression",
            json!({ "value": "9", "type": "usize" }),
        ));
        match answered.events.as_slice() {
            [Event::ExpressionSet { expression, result: Ok(value) }] => {
                assert_eq!(expression, "self.items.count");
                assert_eq!(value.value, "9");
                assert_eq!(value.kind.as_deref(), Some("usize"));
            }
            other => panic!("one ExpressionSet, not {other:?}"),
        }
    }

    /// **A control whose capability is absent is absent**, and a caller that asks anyway is refused
    /// rather than left waiting for an answer that will never come.
    #[test]
    fn an_adapter_that_cannot_set_an_expression_is_never_sent_one() {
        let mut session = plain_session();
        assert!(!session.capabilities().set_expression);
        assert!(!session.capabilities().evaluate_for_hovers);
        let refused = session.set_expression("count", "9");
        assert!(refused.is_err(), "refused rather than sent");
    }

    /// The adapter's own words, never Quill's — the rule `quill-git` keeps about git's stderr.
    #[test]
    fn a_refused_assignment_carries_the_debuggers_own_sentence() {
        let mut session = paused_session();
        let asked = session.set_expression("PI", "3").expect("offered");
        let answered = session.on_message(refusal(
            seq_of(&asked, "setExpression"),
            "setExpression",
            "cannot assign to a constant",
        ));
        match answered.events.as_slice() {
            [Event::ExpressionSet { expression, result: Err(said) }] => {
                assert_eq!(expression, "PI");
                assert_eq!(said, "cannot assign to a constant");
            }
            other => panic!("one refused ExpressionSet, not {other:?}"),
        }
    }
}
