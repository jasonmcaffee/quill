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

/// Everything the window knows about the session it is running.
pub struct DebugState {
    client: Client,
    session: Session,
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
        let mut client = Client::start(command, waker)?;
        let described = client.described().to_owned();
        let mut session = Session::new(launch);
        let opening = session.begin();
        if !client.write_all(&opening.frames) {
            return Err(format!("{described} would not take the first request."));
        }
        Ok(Self {
            client,
            session,
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
            sent: HashMap::new(),
            answered: HashMap::new(),
            filters: Vec::new(),
            output: Vec::new(),
            message: None,
            stopping: false,
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
            sent: HashMap::new(),
            answered: HashMap::new(),
            filters: Vec::new(),
            output: Vec::new(),
            message: None,
            stopping: false,
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
                Reply::Message(message) => {
                    let outcome = self.session.on_message(*message);
                    self.client.write_all(&outcome.frames);
                    for event in outcome.events {
                        if matches!(event, Event::RunInTerminal { .. }) {
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
                for watch in &mut self.watches {
                    watch.result = None;
                }
            }
            Event::Stopped(stopped) => {
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
                self.fetched.insert(reference, variables);
                self.rebuild_rows();
            }
            Event::Breakpoints { path, answered } => {
                self.answered.insert(PathBuf::from(path), answered);
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
                    self.rebuild_rows();
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
        }
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
                self.push_children(&mut rows, scope.reference, &key, 1);
            }
        }
        // What each row's value was, so the next stop can mark what moved. Written after the tree is
        // built rather than as it is built, or a row would compare against itself.
        self.rows = rows;
    }

    /// The children of one reference, and theirs, as far as they are open.
    fn push_children(&self, rows: &mut Vec<Row>, reference: i64, parent: &str, depth: usize) {
        let Some(children) = self.fetched.get(&reference) else {
            return;
        };
        for child in children {
            let key = format!("{parent}/{}", child.name);
            let expanded = self.opened.contains(&key);
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
                self.push_children(rows, child.reference, &key, depth + 1);
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
}
