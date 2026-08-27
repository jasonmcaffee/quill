//! The window's debug state: the adapter, the session, and what the tile has to draw between one
//! message and the next.
//!
//! Nothing here draws and nothing here speaks the protocol. The protocol is `quill_dap`, the drawing
//! is `components::debug_panel` and `components::gutter`, and this is what sits between them —
//! exactly the place `app::git::GitState` sits between `quill_git::Worker` and `components::git_panel`.
//!
//! **One session per window.** IntelliJ runs several; the first version of this does not, and
//! everything is simpler for it: the state below is one `Option<DebugState>` on `QuillApp` rather
//! than a collection, and no pane of the tile needs a session chooser above it.
//!
//! ## What is cached and what is thrown away
//!
//! Every `variablesReference` is valid only while the program stays paused, so [`DebugState::fetched`]
//! is cleared on every resume — the specification's own rule, written down in
//! `quill_dap::Request::resumes` and acted on here. What is **not** thrown away is which rows were
//! open: [`DebugState::opened`] is keyed by the row's **path of names** rather than by its reference,
//! so stepping through a loop does not re-collapse the structure being watched. The values are always
//! refetched, because the references they came from are gone.
//!
//! ## The breakpoints are the document's, and this holds what the adapter said about them
//!
//! Quill draws the adapter's answer rather than its own hope. [`DebugState::answered`] holds, per
//! file, what came back from `setBreakpoints` — in the order the offsets were sent, which is why
//! [`DebugState::sent`] keeps that order. A breakpoint the adapter could not bind stays hollow for
//! the life of the session, and one it moved is drawn where it really landed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use quill_dap::messages::{Frame, OutputKind, Scope, SourceBreakpoint, Thread, Variable, VerifiedBreakpoint};
use quill_dap::session::{Event, Outcome, Session, State, Step};
use quill_dap::{AdapterCommand, Client, Reply};

use crate::services::run_configurations::Configuration;

/// A build that has to finish before there is a program to debug, and the thread running it.
///
/// `task-1692`: `cargo run` is a build tool, so debugging it means asking cargo what it built first.
/// The build runs **on a thread**, the way `quill-git` runs git, because a cold `cargo build` of this
/// repository is minutes and the window draws throughout it. The reply arrives on a channel and is
/// taken in `QuillApp::take_the_debug_replies`, beside every other thread's replies.
///
/// Reading structured output out of the run tile instead was considered and rejected in the TDD: the
/// tile is a ConPTY, and JSON that has been through a terminal's line wrapping is not JSON.
pub struct PendingBuild {
    /// The configuration this is a build *of*, which is what the session will be named after and what
    /// keeps its own command line — the derived binary is the launch request's business only.
    pub configuration: Configuration,
    /// The debugger the built program will be given to.
    pub adapter: String,
    /// The file the session was started for, when it was `Debug Current File`.
    pub for_file: Option<PathBuf>,
    /// The `--bin` name, which picks one artifact out of a workspace that built several.
    pub wanted: Option<String>,
    /// The debuggee's own arguments, which never reached the build command.
    pub program_args: Vec<String>,
    /// What the tile says while it runs — `Building quill`.
    pub what: String,
    /// The command line, so the tile and the output can say what was really run.
    pub command: String,
    /// When it started, which is what the tile counts up from.
    pub started: std::time::Instant,
    /// The build's answer, once there is one.
    pub replies: std::sync::mpsc::Receiver<Built>,
}

/// What a build came back with.
pub enum Built {
    /// The program cargo said it made.
    Program(PathBuf),
    /// The build failed, and this is what the compiler said — its own words, verbatim, which is the
    /// rule `quill-git` already follows about git's standard error.
    Failed(String),
    /// The build worked and produced no program at all, which is a library crate.
    Nothing,
}

/// How many lines of the adapter's own output are kept when it is not going to the run tile.
///
/// A fallback rather than the usual path — §7.2 says the debuggee runs in the run tile — so this is a
/// buffer for an adapter that would not ask, and a bound on it so a program in a loop cannot fill the
/// window's memory.
const OUTPUT_LIMIT: usize = 2_000;

/// One row of the variables tree, flattened for drawing.
///
/// The tree is rebuilt from [`DebugState::fetched`] and [`DebugState::opened`] whenever either moves,
/// rather than being a tree of nodes kept in step with them: there is one authority for what is open
/// and one for what has been read, and a third structure agreeing with both would be a third chance
/// to disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The path of names down to this row — `Locals/items/0` — which is what remembers that it was
    /// open across a step. Stable across references, which is the whole point of it.
    pub key: String,
    pub depth: usize,
    pub name: String,
    pub value: String,
    pub kind: Option<String>,
    /// Non-zero when the row has children to fetch.
    pub reference: i64,
    /// The reference this row is *inside*, which is what `setVariable` names it by.
    pub container: i64,
    pub expanded: bool,
    /// True when the value is not what it was at the last stop, which is what stepping is for.
    pub changed: bool,
    /// True for a scope's own row — Locals, Arguments — which is drawn as a heading rather than as a
    /// value and can never be assigned to.
    pub is_scope: bool,
}

impl Row {
    pub fn has_children(&self) -> bool {
        self.reference != 0
    }
}

/// One watched expression: what was typed, and what the debugger last said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watch {
    /// What labels the question, so an answer that arrives after another was asked still lands in
    /// the right row.
    pub id: u64,
    pub expression: String,
    /// The value, the debugger's refusal, or nothing while the program is running.
    pub result: Option<Result<Variable, String>>,
}

/// A value tooltip's question, and the tree it turned into.
///
/// `task-1696`. It is deliberately **not** a second watch: a watch is durable and is asked again at
/// every stop, and this is about a moment — the pointer is resting somewhere now. So it is thrown
/// away when the program resumes rather than re-asked, which is what IntelliJ's own tooltip does.
///
/// Its rows are the tile's [`Row`], built by the same walk over the same [`DebugState::fetched`]
/// map, because a `variablesReference` is global to the stop: a structure opened in the tile and
/// then pointed at is already read, and the popup opens on it instantly.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverValue {
    /// What labels the question, so an answer arriving after the pointer has moved on lands nowhere
    /// rather than in a popup about something else.
    pub id: u64,
    /// The expression as `quill_core::expressions::at` read it, which is also the root row's key.
    pub expression: String,
    /// The debugger's answer, its refusal, or nothing while it is still being asked.
    pub answer: Option<Result<Variable, String>>,
    /// Which rows are open, by their path of names below the root.
    ///
    /// Its own set rather than [`DebugState::opened`]: the two trees have different roots, so a key
    /// like `items/0` would mean two things at once.
    opened: HashSet<String>,
    /// The flattened tree, rebuilt whenever what it is built from moves.
    pub rows: Vec<Row>,
}

impl HoverValue {
    fn new(id: u64, expression: &str) -> Self {
        Self {
            id,
            expression: expression.to_owned(),
            answer: None,
            opened: HashSet::new(),
            rows: Vec::new(),
        }
    }

    /// True while the debugger has not answered yet, which is what the popup says instead of a value.
    pub fn is_waiting(&self) -> bool {
        self.answer.is_none()
    }

    /// The debugger's refusal, when that is what came back.
    pub fn refusal(&self) -> Option<&str> {
        match self.answer.as_ref() {
            Some(Err(said)) => Some(said.as_str()),
            _ => None,
        }
    }
}

/// Everything the window knows about the session it is running.
pub struct DebugState {
    client: Client,
    session: Session,
    /// How the adapter was started, kept so a **child session** can dial the same server again. See
    /// [`DebugState::adopt_child`], which is what makes js-debug work at all.
    command: AdapterCommand,
    /// What wakes the window, kept for the same reason: a child session's reader is a second thread
    /// and it has to be able to draw what it reads.
    waker: quill_dap::Waker,
    /// True once [`Self::adopt_child`] has moved the session onto a child connection, which is what
    /// makes the parent's messages something to ignore rather than something to act on.
    child_open: bool,
    /// A snapshot of the configuration this was started from, for the same reason `Run` keeps one:
    /// editing a configuration mid-session must change what the next session does and never what
    /// this one says about itself.
    pub configuration: Configuration,
    /// The registry entry's name — `lldb`, `node` — for the tile's header and `debug status`.
    pub adapter: String,
    /// The adapter that was started, as it was handed over.
    pub described: String,
    /// What the adapter warned about when the session began, shown once. Empty for one with nothing
    /// to say.
    pub caveat: &'static str,
    pub threads: Vec<Thread>,
    pub frames: Vec<Frame>,
    /// Which frame's variables are showing. The top one at each stop, and whatever was clicked after
    /// that.
    pub frame: Option<i64>,
    scopes: Vec<Scope>,
    /// The children of every reference that has been read, while the program stays paused.
    fetched: HashMap<i64, Vec<Variable>>,
    /// Which rows are open, by their path of names, so a step does not re-collapse the structure
    /// being watched.
    opened: HashSet<String>,
    /// What each row's value was at the last stop, so a change can be marked.
    previous: HashMap<String, String>,
    /// The flattened tree, rebuilt whenever what it is built from moves.
    pub rows: Vec<Row>,
    pub watches: Vec<Watch>,
    /// The next label for a question, so two answers can never be confused.
    next_question: u64,
    /// The one-off expression asked from `Evaluate Expression`, and its answer.
    pub evaluated: Option<(u64, String, Option<Result<Variable, String>>)>,
    /// The value tooltip's question, while there is one. `task-1696`.
    pub hover: Option<HoverValue>,
    /// The offsets sent for each file, in the order they were sent, so the adapter's answers — which
    /// come back as a list in that order — can be matched to lines.
    sent: HashMap<PathBuf, Vec<usize>>,
    /// What the adapter said about each file's breakpoints.
    answered: HashMap<PathBuf, Vec<VerifiedBreakpoint>>,
    /// The exception filters that are switched on, out of what the adapter offered.
    pub filters: Vec<String>,
    /// What the adapter or the debuggee said, when it is not going to the run tile.
    pub output: Vec<String>,
    /// The last thing worth saying in the status bar.
    pub message: Option<String>,
    /// Set once the stop button has been pressed, so a second press is the hard one.
    stopping: bool,
    /// How many times what the window draws a value from has changed.
    ///
    /// `task-1696`: the inline values are worked out once and cached, and until now the key was the
    /// text revision and the frame — neither of which moves when the **variables** arrive, which is a
    /// round trip after the stop. So the first ask cached an empty answer and nothing ever asked
    /// again. It only appeared to work because the execution-point jump was running every frame and
    /// re-colouring the file, which moves the text revision as a side effect.
    reads: u64,
    /// How many times this session has stopped.
    ///
    /// `task-1696`: what makes "jump to where the program stopped" happen **once a stop** rather than
    /// once a frame. Being paused is a state that lasts, so a window that acted on it every frame put
    /// the caret back on the stopped line sixty times a second — which is a caret nobody can move
    /// while a program is paused, and it is why `Debug -> Show Value` could never find a word.
    stops: u64,
}

impl DebugState {
    /// Start an adapter and open a session on it.
    ///
    /// The reply is the state, or the reason nothing could be started — which the caller puts in the
    /// status bar. **Nothing is invented and nothing is fetched.**
    pub fn start(
        adapter: &str,
        command: &AdapterCommand,
        launch: serde_json::Value,
        caveat: &'static str,
        configuration: Configuration,
        waker: quill_dap::Waker,
    ) -> Result<Self, String> {
        let mut client = Client::start(command, waker.clone())?;
        let described = client.described().to_owned();
        let mut session = Session::new(launch);
        let opening = session.begin();
        if !client.write_all(&opening.frames) {
            return Err(format!("{described} would not take the first request."));
        }
        Ok(Self {
            client,
            session,
            command: command.clone(),
            waker,
            child_open: false,
            configuration,
            adapter: adapter.to_owned(),
            described,
            caveat,
            threads: Vec::new(),
            frames: Vec::new(),
            frame: None,
            scopes: Vec::new(),
            fetched: HashMap::new(),
            opened: HashSet::new(),
            previous: HashMap::new(),
            rows: Vec::new(),
            watches: Vec::new(),
            next_question: 1,
            evaluated: None,
            hover: None,
            sent: HashMap::new(),
            answered: HashMap::new(),
            filters: Vec::new(),
            output: Vec::new(),
            message: None,
            stopping: false,
            reads: 0,
            stops: 0,
        })
    }

    /// A session with no adapter behind it, fed messages directly.
    ///
    /// The other half of [`quill_dap::Client::detached`], and it exists for the same reason
    /// `RunPanel::start_detached` does: a picture of a paused debugger cannot be taken of a real one
    /// without waiting for a real program, and what a test waits for is the one thing a screenshot
    /// test may never do. It runs the whole state machine — so what it draws is what a real adapter
    /// sending those messages would have drawn.
    pub fn detached(adapter: &str, configuration: Configuration) -> Self {
        Self {
            client: Client::detached(),
            session: Session::new(serde_json::Value::Null),
            command: AdapterCommand::stdio("a scripted adapter", Vec::new()),
            waker: std::sync::Arc::new(|| {}),
            child_open: false,
            configuration,
            adapter: adapter.to_owned(),
            described: "a scripted adapter".to_owned(),
            caveat: "",
            threads: Vec::new(),
            frames: Vec::new(),
            frame: None,
            scopes: Vec::new(),
            fetched: HashMap::new(),
            opened: HashSet::new(),
            previous: HashMap::new(),
            rows: Vec::new(),
            watches: Vec::new(),
            next_question: 1,
            evaluated: None,
            hover: None,
            sent: HashMap::new(),
            answered: HashMap::new(),
            filters: Vec::new(),
            output: Vec::new(),
            message: None,
            stopping: false,
            reads: 0,
            stops: 0,
        }
    }

    /// Open a detached session, which is what a real one does when its adapter starts.
    pub fn begin(&mut self) {
        let opening = self.session.begin();
        self.send(opening);
    }

    /// Hand one message to a session, as though the adapter had sent it.
    ///
    /// A detached session's own requests are recorded rather than written, so a test reads what it
    /// really asked for — with the seq it really used — and answers *that*, exactly as an adapter
    /// would. Nothing about the order or the numbering is assumed.
    pub fn feed(&mut self, message: quill_dap::Message) {
        let outcome = self.session.on_message(message);
        self.client.write_all(&outcome.frames);
        for event in outcome.events {
            self.absorb(event);
        }
    }

    /// Everything a detached session has asked for since this was last called. Empty for a real one.
    pub fn requested(&mut self) -> Vec<serde_json::Value> {
        self.client.take_written()
    }

    pub fn state(&self) -> State {
        self.session.state()
    }

    /// One sentence saying where the session is, which the tile's header, the status bar and
    /// `debug status` all use.
    pub fn where_it_is(&self) -> String {
        self.session.where_it_is()
    }

    /// How many times the program has stopped. See the field.
    pub fn stops(&self) -> u64 {
        self.stops
    }

    /// How many times what a value is read from has changed. See the field.
    pub fn reads(&self) -> u64 {
        self.reads
    }

    pub fn is_paused(&self) -> bool {
        self.session.state().is_paused()
    }

    /// True when the program is stopped **and everything the stop asked for has come back**, which
    /// is what a script means by "stopped".
    ///
    /// The two are not the same instant. `stopped` is an event and the four requests it causes are
    /// another thing, so there is a window of a few round trips in which the session is paused and
    /// knows nothing about where or what. A `--wait-for-pause` that answered then would hand a script
    /// a stop it could not read a frame or a variable out of, which is the whole point of waiting.
    ///
    /// Measured against a real CodeLLDB, in that window `debug status` answers with a null line and
    /// `debug variables` with an empty list — both of which look exactly like a program that stopped
    /// somewhere uninteresting.
    ///
    /// Counting **the stop's own requests** rather than every outstanding one answers it for every
    /// shape of frame — one with no locals, one whose scopes are all expensive — without being wedged
    /// by an adapter that never answers something. That is not hypothetical: CodeLLDB 1.12.3 drops an
    /// `evaluate` of a name that does not resolve, with a Python traceback on its standard error and
    /// no response at all, so one bad watch expression would otherwise have made every later stop
    /// look unread for the rest of the session.
    pub fn is_ready(&self) -> bool {
        self.is_paused() && self.session.has_read_the_stop()
    }

    pub fn is_alive(&self) -> bool {
        self.session.state().is_alive()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.session.exit_code()
    }

    /// Where the program is stopped, as a path and a **one-based** line.
    pub fn location(&self) -> Option<(PathBuf, usize)> {
        self.session.location().map(|(path, line)| (PathBuf::from(path), line))
    }

    /// True when the debuggee's output is going to the run tile rather than into [`Self::output`].
    pub fn runs_in_terminal(&self) -> bool {
        self.session.runs_in_terminal()
    }

    /// What the adapter said it can do, which is what every optional control asks before drawing
    /// itself.
    pub fn capabilities(&self) -> &quill_dap::Capabilities {
        self.session.capabilities()
    }

    /// The scopes of the frame that is showing.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// Send whatever a session operation produced, and say whether the adapter took it.
    fn send(&mut self, outcome: Outcome) -> bool {
        let sent = self.client.write_all(&outcome.frames);
        // The events an operation produces are about what Quill has just decided rather than about
        // what the adapter said, so they are acted on here rather than being handed back.
        for event in outcome.events {
            self.absorb(event);
        }
        sent
    }

    /// Tell the adapter what a file's breakpoints are now.
    ///
    /// Full replacement per file, which is what the protocol's `setBreakpoints` is. The offsets are
    /// remembered in the order they were sent so the answer, which comes back as a list in that
    /// order, can be matched back to lines.
    pub fn set_breakpoints(&mut self, path: &Path, lines: Vec<(usize, SourceBreakpoint)>) {
        let offsets: Vec<usize> = lines.iter().map(|(offset, _)| *offset).collect();
        let breakpoints: Vec<SourceBreakpoint> =
            lines.into_iter().map(|(_, breakpoint)| breakpoint).collect();
        self.sent.insert(path.to_path_buf(), offsets);
        let outcome = self.session.set_breakpoints(&path.to_string_lossy(), breakpoints);
        self.send(outcome);
    }

    /// Which of the adapter's exception filters are on.
    pub fn set_filters(&mut self, filters: Vec<String>) {
        self.filters = filters.clone();
        let outcome = self.session.set_exception_filters(filters);
        self.send(outcome);
    }

    /// What the adapter said about the breakpoint at `offset` in `path`, if it has said anything.
    ///
    /// `None` means it has not been answered about — a session that has not started, or a file the
    /// adapter has not seen — which the gutter draws as an ordinary breakpoint rather than as an
    /// unverified one: an unbound breakpoint is a thing a debugger says, and with no debugger there
    /// is nobody to have said it.
    pub fn verified(&self, path: &Path, offset: usize) -> Option<&VerifiedBreakpoint> {
        let sent = self.sent.get(path)?;
        let answered = self.answered.get(path)?;
        let at = sent.iter().position(|known| *known == offset)?;
        answered.get(at)
    }

    /// One of the five stepping requests.
    pub fn step(&mut self, step: Step) -> Result<(), String> {
        let outcome = self.session.step(step)?;
        self.message = Some(format!("{}\u{2026}", step.label()));
        self.send(outcome);
        Ok(())
    }

    /// Open or close a row of the variables tree.
    ///
    /// Opening one that has never been read asks for its children; opening one that has been read
    /// already costs nothing at all, which is what makes stepping through a loop cheap.
    pub fn toggle_row(&mut self, key: &str) {
        let Some(row) = self.rows.iter().find(|row| row.key == key).cloned() else {
            return;
        };
        if !row.has_children() {
            return;
        }
        if self.opened.contains(key) {
            self.opened.remove(key);
        } else {
            self.opened.insert(key.to_owned());
            if !self.fetched.contains_key(&row.reference) {
                let outcome = self.session.expand(row.reference);
                self.send(outcome);
            }
        }
        self.rebuild_rows();
    }

    /// Show another frame's variables, without resuming the program.
    pub fn show_frame(&mut self, frame: i64) {
        self.frame = Some(frame);
        // The references belonged to the frame that was showing, so what was read of it means
        // nothing here. What was *open* is kept, because a person looking at the same structure one
        // frame up wants it open there too.
        self.fetched.clear();
        self.scopes.clear();
        self.rows.clear();
        let outcome = self.session.show_frame(frame);
        self.send(outcome);
        // The watches are about a frame, so they are asked again in this one.
        self.ask_the_watches();
    }

    /// Add an expression to the watch list, and ask it now if the program is stopped.
    pub fn add_watch(&mut self, expression: &str) {
        let expression = expression.trim();
        if expression.is_empty() || self.watches.iter().any(|watch| watch.expression == expression) {
            return;
        }
        let id = self.take_question();
        self.watches.push(Watch { id, expression: expression.to_owned(), result: None });
        if self.is_paused() {
            let outcome = self.session.evaluate(id, expression, "watch");
            self.send(outcome);
        }
    }

    /// Take one off the list. True when there was one to take.
    pub fn remove_watch(&mut self, expression: &str) -> bool {
        let before = self.watches.len();
        self.watches.retain(|watch| watch.expression != expression);
        self.watches.len() != before
    }

    /// Ask a one-off expression, which is what `Evaluate Expression` is.
    ///
    /// Context **`watch`**, not `repl`, and that is a decision with a measurement behind it. The
    /// specification says `repl` means "evaluated from the debug console", and CodeLLDB reads that
    /// literally: it runs the text as an **LLDB command line**, so `debug evaluate total` against a
    /// real one answers `'total' is not a valid command`. Quill's box is called `Evaluate Expression`
    /// and IntelliJ's evaluates an expression, so `watch` is the context that means what the control
    /// says. The distinction Quill keeps between the two is IntelliJ's — persistent against one-off —
    /// which is Quill's own bookkeeping rather than anything on the wire.
    pub fn evaluate(&mut self, expression: &str) {
        let id = self.take_question();
        self.evaluated = Some((id, expression.to_owned(), None));
        let outcome = self.session.evaluate(id, expression, "watch");
        self.send(outcome);
    }

    /// Change a value in the running program.
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        let Some(row) = self.rows.iter().find(|row| row.key == key).cloned() else {
            return Err("There is no such row.".to_owned());
        };
        if row.is_scope {
            return Err("A group of variables is not a value.".to_owned());
        }
        let outcome = self.session.set_variable(row.container, &row.name, value)?;
        self.send(outcome);
        Ok(())
    }

    /// Ask what an expression holds, for the value tooltip. `task-1696` §5.4.
    ///
    /// **Context `hover` when the adapter offered it, and `watch` when it did not.** The first is the
    /// context the specification put there for exactly this — adapters read it as permission to
    /// answer cheaply and without side effects. The second is a fallback rather than a refusal,
    /// because `supportsEvaluateForHovers` says *the hover context is meaningful here*, not
    /// *expressions can be evaluated here*, and an adapter that would happily answer should not be
    /// left drawing nothing.
    ///
    /// There is exactly one of these in flight. Asking a second replaces the first, and the first's
    /// answer then lands nowhere, because every answer carries the id it was asked with.
    pub fn ask_the_hover(&mut self, expression: &str) {
        let expression = expression.trim();
        if expression.is_empty() || !self.is_paused() {
            return;
        }
        let id = self.take_question();
        let context = match self.capabilities().evaluate_for_hovers {
            true => "hover",
            false => "watch",
        };
        self.hover = Some(HoverValue::new(id, expression));
        let outcome = self.session.evaluate(id, expression, context);
        self.send(outcome);
    }

    /// True when the value tooltip has everything it was going to be given.
    ///
    /// [`Self::is_ready`]'s argument made once more, for the same reason: an `evaluate` answer and
    /// the `variables` its children come from are two round trips, and a command that reported the
    /// first would hand a script a value it could not read a field out of. So the question is *has
    /// every row that is open had its children answered*, which is right for a value with no
    /// children, one whose children arrived, and one still being read.
    pub fn hover_is_ready(&self) -> bool {
        let Some(hover) = self.hover.as_ref() else {
            return false;
        };
        if hover.answer.is_none() {
            return false;
        }
        hover
            .rows
            .iter()
            .all(|row| !row.expanded || self.fetched.contains_key(&row.reference))
    }

    /// Put the value tooltip away.
    pub fn forget_the_hover(&mut self) {
        self.hover = None;
    }

    /// Open or close a row of the value tooltip, which is [`Self::toggle_row`] for the other tree.
    pub fn toggle_hover_row(&mut self, key: &str) {
        let Some(hover) = self.hover.as_ref() else {
            return;
        };
        let Some(row) = hover.rows.iter().find(|row| row.key == key).cloned() else {
            return;
        };
        if !row.has_children() {
            return;
        }
        let open = !hover.opened.contains(key);
        if let Some(hover) = self.hover.as_mut() {
            match open {
                true => hover.opened.insert(key.to_owned()),
                false => hover.opened.remove(key),
            };
        }
        if open && !self.fetched.contains_key(&row.reference) {
            let outcome = self.session.expand(row.reference);
            self.send(outcome);
        }
        self.rebuild_hover_rows();
    }

    /// Whether the **root** of a value tooltip can be assigned to.
    ///
    /// It came from an `evaluate`, so it has no container reference and `setVariable` cannot name it
    /// by itself. Two ways round that, in order, and the second is why this is not simply one
    /// capability flag:
    ///
    /// 1. **`setExpression`**, which names its target by the expression. The protocol added it for
    ///    exactly this case.
    /// 2. **The scope the name is already in.** Measured: **CodeLLDB 1.12.3 does not offer
    ///    `supportsSetExpression`** — it offers `setVariable` and nothing else — so on the adapter
    ///    Quill's own registry prefers, the first way alone would mean the commonest thing anybody
    ///    wants to change, a bare local, could not be changed from the popup at all. When the
    ///    expression is a name the paused frame's own scopes hold, `setVariable` on that scope is
    ///    not an approximation of the assignment: it is the identical request the debug tile sends
    ///    when the same row is typed over there.
    ///
    /// Anything else — `self.items.count` on an adapter with no `setExpression` — has no field, which
    /// is Quill's rule that a control which can never apply is absent. The children of a tooltip are
    /// unaffected either way, because they came from `variables`.
    pub fn can_set_the_root(&self) -> bool {
        if self.capabilities().set_expression {
            return true;
        }
        if !self.capabilities().set_variable {
            return false;
        }
        self.hover
            .as_ref()
            .is_some_and(|hover| self.scope_holding(&hover.expression).is_some())
    }

    /// The reference of the first scope that has been read and holds a variable of this name.
    ///
    /// Only what has already been fetched, which is what makes it honest: it answers about rows the
    /// debugger has really shown rather than guessing that a name is a local.
    fn scope_holding(&self, name: &str) -> Option<i64> {
        for scope in &self.scopes {
            if let Some(children) = self.fetched.get(&scope.reference) {
                if children.iter().any(|child| child.name == name) {
                    return Some(scope.reference);
                }
            }
        }
        None
    }

    /// Assign to whatever an expression names in the running program.
    ///
    /// The command line's half of the root row's edit, and the one thing `set_value` cannot do: a
    /// row that has never been read has no container reference for `setVariable` to name it by.
    pub fn set_expression(&mut self, expression: &str, value: &str) -> Result<(), String> {
        let outcome = self.assign_to_an_expression(expression, value)?;
        self.send(outcome);
        Ok(())
    }

    /// The request that changes what an expression names — see [`Self::can_set_the_root`] for which
    /// of the two it is and why there are two.
    fn assign_to_an_expression(
        &mut self,
        expression: &str,
        value: &str,
    ) -> Result<Outcome, String> {
        if self.capabilities().set_expression {
            return self.session.set_expression(expression, value);
        }
        match self.scope_holding(expression) {
            Some(reference) => self.session.set_variable(reference, expression, value),
            // The adapter's own limitation, said plainly rather than dressed up: it can change a row
            // it has shown and cannot compile an assignment.
            None => Err(format!(
                "{} cannot assign to {expression}: it changes a variable it has read rather than an expression.",
                self.described
            )),
        }
    }

    /// Change a value from the value tooltip.
    ///
    /// **Two requests, and which one is used is decided by what the row is rather than by a
    /// preference.** The root is named by its expression and needs `setExpression`; everything below
    /// it has a container and a name, which is exactly what `setVariable` takes and what the tile
    /// already sends.
    pub fn set_hover_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        let Some(hover) = self.hover.as_ref() else {
            return Err("There is nothing to change.".to_owned());
        };
        let Some(row) = hover.rows.iter().find(|row| row.key == key).cloned() else {
            return Err("There is no such row.".to_owned());
        };
        let outcome = match row.depth {
            0 => self.assign_to_an_expression(&row.name, value)?,
            _ => self.session.set_variable(row.container, &row.name, value)?,
        };
        self.send(outcome);
        Ok(())
    }

    /// End the session: politely the first time, and for good the second.
    pub fn stop(&mut self) {
        let hard = self.stopping;
        self.stopping = true;
        let outcome = self.session.stop(hard);
        self.send(outcome);
        self.client.stopping_now();
        self.message = Some(match hard {
            true => "Stopping the debugger".to_owned(),
            false => format!("Stopping {}", self.configuration.name),
        });
    }

    /// Kill the adapter outright, which is what closing the window and starting a second session do.
    pub fn kill(&mut self) {
        self.client.kill();
        let gone = self.session.adapter_gone();
        self.absorb_all(gone);
    }

    /// How long until the polite stop's grace runs out, so the window can be woken then rather than
    /// kept drawing — the run tile's rule.
    pub fn stopping_in(&self) -> Option<std::time::Duration> {
        self.client.stopping_in()
    }

    /// Take everything the adapter has said and put it where the window will draw it.
    ///
    /// Returns the reverse requests the window has to act on — a `runInTerminal` needs the run tile,
    /// which this has no business touching. Everything else is settled here.
    pub fn take_replies(&mut self) -> Vec<Event> {
        let mut for_the_window = Vec::new();
        for reply in self.client.poll() {
            match reply {
                // Once a child session is open, the parent is only telemetry — and its sequence
                // numbers are its own, so feeding them to the child's state machine would match a
                // parent's response to whatever the child was waiting for. `task-1692`.
                Reply::Message(_) if self.child_open => continue,
                Reply::Message(message) | Reply::FromChild(message) => {
                    let outcome = self.session.on_message(*message);
                    self.client.write_all(&outcome.frames);
                    for event in outcome.events {
                        if matches!(event, Event::RunInTerminal { .. } | Event::StartDebugging { .. }) {
                            for_the_window.push(event);
                            continue;
                        }
                        self.absorb(event);
                    }
                }
                // The adapter wrote something that is not the protocol. There is no way to know
                // where the next frame starts, so the session ends and says what was seen.
                Reply::Broken(problem) => {
                    self.message = Some(format!("{} sent {problem}", self.described));
                    let gone = self.session.adapter_gone();
                    self.absorb_all(gone);
                }
                Reply::Gone => {
                    let gone = self.session.adapter_gone();
                    self.absorb_all(gone);
                }
            }
        }
        // A polite stop nobody answered, followed up. `RunPanel::stop`'s shape, with the same grace.
        if self.client.grace_ran_out() && self.is_alive() {
            self.client.kill();
            let gone = self.session.adapter_gone();
            self.absorb_all(gone);
        }
        for_the_window
    }

    /// Answer the adapter's `runInTerminal`, once the window has started it.
    pub fn answer_run_in_terminal(&mut self, seq: i64, started: bool, process: Option<u32>) {
        let outcome = self.session.answer_run_in_terminal(seq, started, process);
        self.send(outcome);
    }

    fn absorb_all(&mut self, outcome: Outcome) {
        for event in outcome.events {
            self.absorb(event);
        }
    }

    /// One thing that happened, put where the window will draw it.
    fn absorb(&mut self, event: Event) {
        match event {
            Event::Ready => {
                // The filters the adapter offers with a default of on are on, which is what every
                // client does and what "break on uncaught exception" being the sensible default
                // means. Quill holds no list of its own.
                self.filters = self
                    .session
                    .capabilities()
                    .exception_filters
                    .iter()
                    .filter(|filter| filter.default)
                    .map(|filter| filter.filter.clone())
                    .collect();
            }
            Event::Running => {
                // Every `variablesReference` died the moment the program was told to go on. What was
                // *open* is kept, which is what stops a step re-collapsing the structure being
                // watched.
                self.fetched.clear();
                self.scopes.clear();
                self.rows.clear();
                self.frames.clear();
                self.frame = None;
                // The tooltip is about a moment and the moment has passed — and every reference in
                // it died with the resume. IntelliJ dismisses its own on a step for the same reason.
                self.hover = None;
                for watch in &mut self.watches {
                    watch.result = None;
                }
            }
            Event::Stopped(stopped) => {
                self.stops += 1;
                self.message = Some(match stopped.description.as_deref() {
                    Some(said) => said.to_owned(),
                    None => format!("Stopped on {}", stopped.reason),
                });
            }
            Event::Threads(threads) => self.threads = threads,
            Event::Frames(frames) => {
                self.frame = frames.first().map(|frame| frame.id);
                self.frames = frames;
                self.ask_the_watches();
            }
            Event::Scopes { frame, scopes } => {
                if self.frame != Some(frame) {
                    return;
                }
                self.reads += 1;
                // The first scope that is not expensive is **open**, which is what the pane opens
                // showing and is the one the session fetched unprompted. Registers — which lldb
                // marks expensive — waits to be asked for, and so does a second Locals-like group.
                // Only when nothing has been opened by hand yet: a person who closed Locals and
                // stepped has closed it, and re-opening it under them would be the fault
                // `keep_the_caret_visible` avoids in the other direction.
                if self.opened.is_empty() {
                    if let Some(first) = scopes.iter().find(|scope| !scope.expensive) {
                        self.opened.insert(first.name.clone());
                    }
                }
                self.scopes = scopes;
                self.rebuild_rows();
            }
            Event::Variables { reference, variables } => {
                self.reads += 1;
                self.fetched.insert(reference, variables);
                self.rebuild_rows();
                self.rebuild_hover_rows();
            }
            Event::Breakpoints { path, answered } => {
                let path = PathBuf::from(path);
                // **An answer with a different number of entries than were sent is not an answer
                // about them**, and taking it would throw away the ids the real answer carried —
                // which is what a later `breakpoint` event uses to say one has bound.
                //
                // js-debug sends `initialized` **twice** on a child session, so the breakpoints go
                // out twice, and it answers the second `setBreakpoints` for a file with an empty
                // list. Measured on `task-1692`, where the effect was a breakpoint that stopped the
                // program and was still drawn hollow.
                let expected = self.sent.get(&path).map(Vec::len).unwrap_or_default();
                if answered.len() != expected && self.answered.contains_key(&path) {
                    return;
                }
                self.answered.insert(path, answered);
            }
            Event::BreakpointChanged(changed) => {
                // A breakpoint that bound after the fact — a library that has just loaded. Matched
                // by the adapter's own id, which is the only thing that identifies it across files.
                let Some(id) = changed.id else {
                    return;
                };
                for answered in self.answered.values_mut() {
                    for known in answered.iter_mut() {
                        if known.id == Some(id) {
                            *known = changed.clone();
                        }
                    }
                }
            }
            Event::Output { kind, text } => {
                // The debuggee's output goes to the run tile when the adapter asked for one. What is
                // kept here is what an adapter that did not ask sends, plus the adapter's own
                // console — which is worth reading either way.
                if self.runs_in_terminal() && kind != OutputKind::Console {
                    return;
                }
                for line in text.split_inclusive('\n') {
                    match self.output.last_mut() {
                        Some(last) if !last.ends_with('\n') => last.push_str(line),
                        _ => self.output.push(line.to_owned()),
                    }
                }
                let over = self.output.len().saturating_sub(OUTPUT_LIMIT);
                self.output.drain(..over);
            }
            Event::Evaluated { id, result } => {
                // The tooltip first, because it is the one whose question is replaced rather than
                // kept: an answer whose id is nobody's is an answer to a hover the pointer has
                // already left, and it lands nowhere.
                if self.hover.as_ref().is_some_and(|hover| hover.id == id) {
                    self.take_the_hover_answer(result);
                    return;
                }
                if let Some(watch) = self.watches.iter_mut().find(|watch| watch.id == id) {
                    watch.result = Some(result);
                    return;
                }
                if let Some((asked, _, answer)) = self.evaluated.as_mut() {
                    if *asked == id {
                        *answer = Some(result);
                    }
                }
            }
            Event::VariableSet { reference, name, result } => match result {
                Ok(answered) => {
                    // What the row shows is the value **as the debugger now sees it** rather than
                    // what was typed. A debugger that rounded a float is telling the truth.
                    if let Some(children) = self.fetched.get_mut(&reference) {
                        if let Some(row) = children.iter_mut().find(|row| row.name == name) {
                            row.value = answered.value.clone();
                            row.reference = answered.reference;
                        }
                    }
                    self.reads += 1;
                    // A tooltip's **root** is changed by this request too, on an adapter with no
                    // `setExpression` — and its value comes from the `evaluate` answer rather than
                    // from `fetched`, so it has to be told as well.
                    if let Some(hover) = self.hover.as_mut() {
                        if hover.expression == name {
                            if let Some(Ok(value)) = hover.answer.as_mut() {
                                value.value = answered.value.clone();
                                value.reference = answered.reference;
                            }
                        }
                    }
                    self.rebuild_rows();
                    self.rebuild_hover_rows();
                }
                Err(said) => self.message = Some(said),
            },
            // The other half of the pair above, for a value that was named by an expression rather
            // than by a row that had already been read. Its answer is the value **as the debugger
            // now sees it**, which is the same rule.
            Event::ExpressionSet { expression, result } => match result {
                Ok(answered) => {
                    if let Some(hover) = self.hover.as_mut() {
                        if hover.expression == expression {
                            if let Some(Ok(value)) = hover.answer.as_mut() {
                                value.value = answered.value.clone();
                                value.reference = answered.reference;
                            }
                        }
                    }
                    self.rebuild_hover_rows();
                }
                Err(said) => self.message = Some(said),
            },
            Event::Failed { command, message } => {
                self.message = Some(format!("{command}: {message}"));
            }
            Event::Ended { code } => {
                self.frames.clear();
                self.scopes.clear();
                self.rows.clear();
                self.fetched.clear();
                self.frame = None;
                self.hover = None;
                for watch in &mut self.watches {
                    watch.result = None;
                }
                self.message = Some(match code {
                    Some(0) | None => format!("{} finished", self.configuration.name),
                    Some(code) => format!("{} ended with exit code {code}", self.configuration.name),
                });
            }
            // Answered by the window, which owns the run tile. `take_replies` hands it up rather
            // than letting it reach here.
            Event::RunInTerminal { .. } => {}
            // Both are the window's: one needs the run tile and the other needs the breakpoints
            // re-sent once the child is open, and neither is this type's business.
            Event::StartDebugging { .. } => {}
        }
    }

    /// Open the child session the adapter asked for, and speak to that from now on.
    ///
    /// **js-debug puts the program in a session of its own.** The parent answers `launch` and then
    /// sends `startDebugging`; the program runs under the child, and a client that ignores it is left
    /// with a session that has no threads, never stops, and whose breakpoints answer
    /// `provisionalBreakpoint` for ever — which is exactly what a `node` configuration did before
    /// `task-1692` measured it.
    ///
    /// So: answer the parent, dial the same server again, and run the handshake on the new connection
    /// with the configuration the adapter handed over — it carries `__pendingTargetId`, which is the
    /// only thing tying this connection to the program that is already waiting. The caller re-sends
    /// the breakpoints afterwards, because the child has never been told about any of them.
    pub fn adopt_child(
        &mut self,
        request_seq: i64,
        request: &str,
        configuration: serde_json::Value,
    ) -> Result<(), String> {
        let answer = self.session.answer_start_debugging(request_seq, true);
        self.client.write_to_parent(&answer);
        self.client.adopt_child(&self.command, self.waker.clone())?;
        self.child_open = true;
        let mut body = match configuration {
            serde_json::Value::Object(fields) => fields,
            _ => serde_json::Map::new(),
        };
        body.insert("request".to_owned(), serde_json::json!(request));
        self.session = Session::new(serde_json::Value::Object(body));
        // Everything the parent said about this program was about the parent. The child is a session
        // that has never been asked anything.
        self.threads.clear();
        self.frames.clear();
        self.frame = None;
        self.scopes.clear();
        self.fetched.clear();
        self.rows.clear();
        self.sent.clear();
        self.answered.clear();
        let opening = self.session.begin();
        self.client.write_all(&opening.frames);
        Ok(())
    }

    /// Ask every watch again, which is what a stop and a change of frame both mean.
    fn ask_the_watches(&mut self) {
        if !self.is_paused() {
            return;
        }
        let asking: Vec<(u64, String)> =
            self.watches.iter().map(|watch| (watch.id, watch.expression.clone())).collect();
        for (id, expression) in asking {
            let outcome = self.session.evaluate(id, &expression, "watch");
            self.send(outcome);
        }
    }

    fn take_question(&mut self) -> u64 {
        let id = self.next_question;
        self.next_question += 1;
        id
    }

    /// Flatten the scopes and whatever has been read of them into the rows the pane draws.
    ///
    /// Built rather than kept, for the reason at the top of the file: `fetched` is the authority for
    /// what has been read and `opened` for what is open, and a third structure agreeing with both
    /// would be a third chance to disagree. It costs a walk of what is on the screen, once per
    /// message that changed something, which is not something that happens once a frame.
    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for scope in &self.scopes {
            let key = scope.name.clone();
            let expanded = self.opened.contains(&key);
            rows.push(Row {
                key: key.clone(),
                depth: 0,
                name: scope.name.clone(),
                value: String::new(),
                kind: None,
                reference: scope.reference,
                container: 0,
                expanded,
                changed: false,
                is_scope: true,
            });
            if expanded {
                self.push_children(&mut rows, scope.reference, &key, 1, &self.opened);
            }
        }
        // What each row's value was, so the next stop can mark what moved. Written after the tree is
        // built rather than as it is built, or a row would compare against itself.
        self.rows = rows;
    }

    /// Build the value tooltip's rows: the answer as the root, and whatever is open below it.
    ///
    /// The root is a `Row` like any other — the expression as its name, the debugger's `result` as
    /// its value — and it is **not** a scope, because a scope is a heading and this is a value that
    /// can be assigned to.
    fn rebuild_hover_rows(&mut self) {
        let Some(hover) = self.hover.as_ref() else {
            return;
        };
        let Some(Ok(value)) = hover.answer.as_ref() else {
            if let Some(hover) = self.hover.as_mut() {
                hover.rows.clear();
            }
            return;
        };
        let key = hover.expression.clone();
        let expanded = hover.opened.contains(&key);
        let opened = hover.opened.clone();
        let mut rows = vec![Row {
            key: key.clone(),
            depth: 0,
            name: key.clone(),
            value: value.value.clone(),
            kind: value.kind.clone(),
            reference: value.reference,
            container: 0,
            expanded,
            // A tooltip is about now, so there is no last time to have been different from.
            changed: false,
            is_scope: false,
        }];
        if expanded {
            self.push_children(&mut rows, value.reference, &key, 1, &opened);
        }
        if let Some(hover) = self.hover.as_mut() {
            hover.rows = rows;
        }
    }

    /// Put the debugger's answer into the tooltip, and open the root when it has children.
    ///
    /// **The root opens itself**, which is what a person means by "show me the object": pointing at
    /// a struct shows its fields with no click. Nothing deeper is opened unasked and nothing deeper
    /// is fetched, which is `task-1687` §8.3's lazy model untouched.
    fn take_the_hover_answer(&mut self, result: Result<Variable, String>) {
        let mut expand = 0;
        if let Some(hover) = self.hover.as_mut() {
            if let Ok(value) = &result {
                if value.reference != 0 {
                    hover.opened.insert(hover.expression.clone());
                    expand = value.reference;
                }
            }
            hover.answer = Some(result);
        }
        if expand != 0 && !self.fetched.contains_key(&expand) {
            let outcome = self.session.expand(expand);
            self.send(outcome);
        }
        self.rebuild_hover_rows();
    }

    /// The children of one reference, and theirs, as far as they are open.
    ///
    /// `opened` is passed in rather than read off `self`, because there are two trees now — the
    /// tile's, rooted on the frame's scopes, and the value tooltip's, rooted on an expression — and
    /// they keep separate answers to what is open. Everything else about them is the same, which is
    /// why this is one walk rather than two.
    fn push_children(
        &self,
        rows: &mut Vec<Row>,
        reference: i64,
        parent: &str,
        depth: usize,
        opened: &HashSet<String>,
    ) {
        let Some(children) = self.fetched.get(&reference) else {
            return;
        };
        for child in children {
            let key = format!("{parent}/{}", child.name);
            let expanded = opened.contains(&key);
            let changed = self
                .previous
                .get(&key)
                .is_some_and(|was| *was != child.value);
            rows.push(Row {
                key: key.clone(),
                depth,
                name: child.name.clone(),
                value: child.value.clone(),
                kind: child.kind.clone(),
                reference: child.reference,
                container: reference,
                expanded,
                changed,
                is_scope: false,
            });
            if expanded {
                self.push_children(rows, child.reference, &key, depth + 1, opened);
            }
        }
    }

    /// Remember what every row showed, so the next stop can mark what changed.
    ///
    /// Called when the program resumes rather than when it stops, because "changed" means "different
    /// from the last time you looked" and the last time you looked was before this step.
    pub fn remember_the_values(&mut self) {
        self.previous =
            self.rows.iter().map(|row| (row.key.clone(), row.value.clone())).collect();
    }

    /// The variables of the top frame by name, which is what the inline values are matched against.
    ///
    /// Only the first level of the frame's own scopes: a value painted at the end of a line is a
    /// local, and walking into structures to find one would be both slow and wrong — `items.len` is
    /// not a name that appears in the source.
    pub fn top_frame_values(&self) -> HashMap<String, String> {
        let mut values = HashMap::new();
        for scope in &self.scopes {
            let Some(children) = self.fetched.get(&scope.reference) else {
                continue;
            };
            for child in children {
                values.entry(child.name.clone()).or_insert_with(|| child.value.clone());
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The row keys are what remembers that a row was open across a step, so they have to be built
    /// from names rather than from references — which change at every stop.
    #[test]
    fn a_rows_key_is_its_path_of_names() {
        let row = Row {
            key: "Locals/items/0".to_owned(),
            depth: 2,
            name: "0".to_owned(),
            value: "1".to_owned(),
            kind: None,
            reference: 0,
            container: 8,
            expanded: false,
            changed: false,
            is_scope: false,
        };
        assert!(!row.has_children());
        assert_eq!(row.key.split('/').next(), Some("Locals"));
    }

    /// **An answer with a different number of entries than were sent is not an answer about them.**
    ///
    /// js-debug sends `initialized` twice on a child session and answers the second
    /// `setBreakpoints` for a file with an empty list; taking that threw away the ids the real
    /// answer carried, and a breakpoint that stopped the program went on being drawn hollow.
    /// `task-1692` measured it.
    #[test]
    fn an_answer_that_is_not_one_for_one_with_what_was_sent_is_not_taken() {
        let mut state = DebugState::detached("node", Configuration::new("Test", "node app.js"));
        let path = PathBuf::from(r"C:\p\app.js");
        state.sent.insert(path.clone(), vec![10]);
        state.absorb(Event::Breakpoints {
            path: path.to_string_lossy().to_string(),
            answered: vec![VerifiedBreakpoint {
                id: Some(1),
                verified: false,
                line: Some(3),
                message: Some("breakpoint.provisionalBreakpoint".to_owned()),
            }],
        });
        assert_eq!(state.verified(&path, 10).map(|known| known.id), Some(Some(1)));
        // The empty second answer changes nothing, so the id survives for the `breakpoint` event
        // that says it has bound.
        state.absorb(Event::Breakpoints {
            path: path.to_string_lossy().to_string(),
            answered: Vec::new(),
        });
        assert_eq!(state.verified(&path, 10).map(|known| known.id), Some(Some(1)));
        // And a real answer, one for one with what was sent, still replaces it.
        state.absorb(Event::Breakpoints {
            path: path.to_string_lossy().to_string(),
            answered: vec![VerifiedBreakpoint {
                id: Some(1),
                verified: true,
                line: Some(3),
                message: None,
            }],
        });
        assert!(state.verified(&path, 10).expect("an answer").verified);
    }

    // ------------------------------------------------------------------ the value tooltip, task-1696

    /// What the adapter would have said.
    fn answer(request_seq: i64, command: &str, body: serde_json::Value) -> quill_dap::Message {
        quill_dap::Message::Response {
            seq: 500 + request_seq,
            request_seq,
            command: command.to_owned(),
            success: true,
            message: None,
            body,
        }
    }

    fn seq_of(asked: &[serde_json::Value], command: &str) -> i64 {
        asked
            .iter()
            .find(|frame| frame["command"] == command)
            .and_then(|frame| frame["seq"].as_i64())
            .unwrap_or_else(|| panic!("a {command} was sent, out of {asked:?}"))
    }

    fn capabilities() -> serde_json::Value {
        serde_json::json!({
            "supportsConfigurationDoneRequest": true,
            "supportsSetVariable": true,
            "supportsEvaluateForHovers": true,
            "supportsSetExpression": true,
        })
    }

    /// A detached session driven to a stop, with one frame and nothing read of it yet.
    fn paused_state(capabilities: serde_json::Value) -> DebugState {
        let mut state = DebugState::detached("lldb", Configuration::new("app", "app.exe"));
        state.begin();
        let asked = state.requested();
        state.feed(answer(seq_of(&asked, "initialize"), "initialize", capabilities));
        state.requested();
        state.feed(quill_dap::Message::Initialized);
        let asked = state.requested();
        state.feed(answer(seq_of(&asked, "configurationDone"), "configurationDone", serde_json::Value::Null));
        state.feed(quill_dap::Message::Stopped(quill_dap::Stopped {
            reason: "breakpoint".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }));
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "threads"),
            "threads",
            serde_json::json!({ "threads": [{ "id": 1, "name": "main" }] }),
        ));
        state.feed(answer(
            seq_of(&asked, "stackTrace"),
            "stackTrace",
            serde_json::json!({ "stackFrames": [
                { "id": 1000, "name": "main", "line": 4, "source": { "path": "main.rs" } }
            ]}),
        ));
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "scopes"),
            "scopes",
            serde_json::json!({ "scopes": [{ "name": "Locals", "variablesReference": 7, "expensive": false }] }),
        ));
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "variables"),
            "variables",
            serde_json::json!({ "variables": [
                { "name": "items", "value": "Vec(3)", "type": "Vec<i32>", "variablesReference": 17 }
            ]}),
        ));
        state
    }

    /// The hover context is asked for when the adapter offered it, and `watch` when it did not — a
    /// fallback rather than a refusal, because an adapter that would happily answer should not be
    /// left drawing nothing.
    #[test]
    fn the_hover_is_asked_in_the_context_the_adapter_offered() {
        let mut state = paused_state(capabilities());
        state.ask_the_hover("self.items");
        let asked = state.requested();
        assert_eq!(asked[0]["arguments"]["context"], "hover");

        let mut plain = paused_state(serde_json::json!({ "supportsConfigurationDoneRequest": true }));
        plain.ask_the_hover("self.items");
        let asked = plain.requested();
        assert_eq!(asked[0]["arguments"]["context"], "watch");
    }

    /// **The root opens itself**, which is what a person means by "show me the object": pointing at a
    /// struct shows its fields with no click at all. Nothing deeper is opened unasked.
    #[test]
    fn a_hover_whose_answer_has_children_opens_itself_and_reads_them() {
        let mut state = paused_state(capabilities());
        state.ask_the_hover("items");
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "Vec(3)", "type": "Vec<i32>", "variablesReference": 41 }),
        ));
        // The children were asked for without anything being clicked.
        let asked = state.requested();
        assert_eq!(seq_of(&asked, "variables") > 0, true);
        assert!(!state.hover_is_ready(), "not until the children have come back");
        state.feed(answer(
            seq_of(&asked, "variables"),
            "variables",
            serde_json::json!({ "variables": [
                { "name": "[0]", "value": "1", "type": "i32", "variablesReference": 0 },
                { "name": "[1]", "value": "2", "type": "i32", "variablesReference": 0 }
            ]}),
        ));
        assert!(state.hover_is_ready());
        let hover = state.hover.as_ref().expect("a hover");
        let names: Vec<&str> = hover.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, vec!["items", "[0]", "[1]"]);
        assert_eq!(hover.rows[0].depth, 0);
        assert_eq!(hover.rows[1].key, "items/[0]");
        assert!(!hover.rows[0].is_scope, "a value that can be assigned to, not a heading");
    }

    /// **Two requests, and which one is used is decided by what the row is.** The root came from an
    /// `evaluate` and has no container reference for `setVariable` to name it by; a child came from
    /// `variables` and is set the way the tile sets one.
    #[test]
    fn the_root_is_set_by_an_expression_and_a_child_by_its_container() {
        let mut state = paused_state(capabilities());
        state.ask_the_hover("items");
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "Vec(3)", "type": "Vec<i32>", "variablesReference": 41 }),
        ));
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "variables"),
            "variables",
            serde_json::json!({ "variables": [
                { "name": "[0]", "value": "1", "type": "i32", "variablesReference": 0 }
            ]}),
        ));
        state.requested();

        state.set_hover_value("items", "vec![]").expect("the adapter offers setExpression");
        let asked = state.requested();
        assert_eq!(asked[0]["command"], "setExpression");
        assert_eq!(asked[0]["arguments"]["expression"], "items");

        state.set_hover_value("items/[0]", "9").expect("a child is set by its container");
        let asked = state.requested();
        assert_eq!(asked[0]["command"], "setVariable");
        assert_eq!(asked[0]["arguments"]["variablesReference"], 41);
        assert_eq!(asked[0]["arguments"]["name"], "[0]");
    }

    /// **Measured against CodeLLDB 1.12.3, which does not offer `supportsSetExpression`.** A bare
    /// name the paused frame's own scopes hold is set by `setVariable` on that scope — which is not
    /// an approximation of the assignment but the identical request the tile sends when the same row
    /// is typed over there.
    #[test]
    fn a_bare_name_is_set_through_its_scope_where_the_adapter_has_no_set_expression() {
        let mut state = paused_state(serde_json::json!({
            "supportsConfigurationDoneRequest": true,
            "supportsSetVariable": true,
        }));
        state.ask_the_hover("items");
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "Vec(3)", "type": "Vec<i32>" }),
        ));
        state.requested();
        assert!(state.can_set_the_root(), "Locals holds a variable called `items`");
        state.set_hover_value("items", "vec![]").expect("through the scope it is in");
        let asked = state.requested();
        assert_eq!(asked[0]["command"], "setVariable");
        assert_eq!(asked[0]["arguments"]["variablesReference"], 7, "the Locals scope");
        assert_eq!(asked[0]["arguments"]["name"], "items");
    }

    /// And anything that is not a name the frame holds has no field at all, which is Quill's rule
    /// that a control which can never apply is absent.
    #[test]
    fn a_field_path_cannot_be_set_where_the_adapter_has_no_set_expression() {
        let mut state = paused_state(serde_json::json!({
            "supportsConfigurationDoneRequest": true,
            "supportsSetVariable": true,
        }));
        state.ask_the_hover("basket.label");
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "fruit", "type": "String" }),
        ));
        assert!(!state.can_set_the_root());
        assert!(state.set_hover_value("basket.label", "pear").is_err());
    }

    /// Every `variablesReference` dies on resume, so the tooltip goes with them: a value from the
    /// last stop drawn beside a tree that cannot be opened is worse than nothing. IntelliJ dismisses
    /// its own on a step for the same reason.
    #[test]
    fn resuming_puts_the_tooltip_away() {
        let mut state = paused_state(capabilities());
        state.ask_the_hover("items");
        let asked = state.requested();
        state.feed(answer(
            seq_of(&asked, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "Vec(3)", "variablesReference": 41 }),
        ));
        assert!(state.hover.is_some());
        state.step(quill_dap::Step::Over).expect("stopped, so it can step");
        assert!(state.hover.is_none(), "and every reference in it went with the resume");
    }

    /// An answer that arrives after the pointer has moved on lands nowhere rather than in a popup
    /// about something else. Every question carries the id it was asked with.
    #[test]
    fn an_answer_to_a_question_nobody_is_asking_any_more_lands_nowhere() {
        let mut state = paused_state(capabilities());
        state.ask_the_hover("items");
        let first = state.requested();
        state.ask_the_hover("attempts");
        let second = state.requested();
        // The first question is answered late, after the pointer moved to another name.
        state.feed(answer(
            seq_of(&first, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "Vec(3)" }),
        ));
        assert!(state.hover.as_ref().expect("a hover").is_waiting(), "still about `attempts`");
        state.feed(answer(
            seq_of(&second, "evaluate"),
            "evaluate",
            serde_json::json!({ "result": "3" }),
        ));
        assert_eq!(state.hover.as_ref().expect("a hover").rows[0].value, "3");
    }
}
