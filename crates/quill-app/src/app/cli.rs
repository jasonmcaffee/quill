//! What a command line command means.
//!
//! `QuillApp::run_action` is the one place a menu entry turns into a change, and this is the same
//! rule for the command line: [`QuillApp::run_cli`] is the one place a request turns into a change.
//! A command reaches it from `services::control`, which read it off a socket, and never from
//! anywhere else.
//!
//! ## Where the work is, and where it is not
//!
//! Nothing here decides what a command is called or what it takes — `quill_cli::catalogue` does,
//! and the client parses against the same list, so a command that the CLI will accept is a command
//! the window knows. Nothing here draws, either. Every arm either reads the window's state into a
//! reply or asks the window to change in exactly the way a menu entry or a click would, so a thing
//! done from the command line and the same thing done by hand are the same thing.
//!
//! Wherever there is already a way in, it is used: `run_action` for anything on a menu,
//! `open_path`, `save`, `set_settings`, `set_font_size`, `FileTree::expand`, `Document::apply`. The
//! commands that have no menu entry behind them — reading the text, moving the caret, typing into a
//! modal — are the ones that do anything of their own here.
//!
//! ## Answers that cannot be given at once
//!
//! Four commands are asked on one frame and answered on a later one: a screenshot, because the
//! picture of a frame arrives after that frame has been painted; `terminal read --wait-for`,
//! because it is waiting for a shell; `modal results --wait`, because `Find in Files` reads the
//! project on a thread; and `git action --wait`, because git runs on a thread too. Each one keeps
//! its request in [`Waiting`] and answers it when it is ready or when its time runs out. That is
//! also why every one of them takes a timeout: a command that could wait for ever is a script that
//! hangs.
//!
//! ## Paths
//!
//! One rule, everywhere: **a relative path is relative to the project folder**. Not to wherever the
//! client happened to be run from — that would make the same command mean different things in two
//! terminals, and an agent's working directory is rarely the thing it is editing. A path that must
//! be somewhere else is given in full. Every reply says which absolute path it used, so there is
//! never any doubt about where a file went.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::ViewportCommand;
use serde_json::{json, Map, Value};

use quill_cli::protocol::{code, Reply, Request};

use crate::app::actions::{Action, GitAction, HighlightColor};
use crate::app::{QuillApp, ViewMode};
use quill_core::symbols::Role;

use crate::components::find_in_files::FindInFiles;
use crate::components::go_to_file::GoToFile;
use crate::components::references::{self, References};
use crate::components::modal;
use crate::components::status_bar;
use crate::components::prompt_dialog::{Prompt, Purpose};
use crate::services::control::Pending;
use crate::services::file_kind;
use crate::settings;
use crate::theme::size;

/// How long a command waits when it was not told.
const DEFAULT_WAIT: Duration = Duration::from_millis(10_000);
/// How long a screenshot waits. It is a handful of frames away, so this is only ever the answer when
/// the window has stopped drawing altogether.
const SCREENSHOT_WAIT: Duration = Duration::from_millis(5_000);
/// How long a screenshot lets the window settle before asking for the picture.
///
/// A quarter of a second, which is three times egui's own animation time of a twelfth. It is a
/// **duration rather than a number of frames**, because how many frames a quarter of a second is
/// depends on the machine: six frames was tried first and left a modal at 97 per cent of its fade,
/// which is exactly the sort of nearly-right that a picture is supposed to settle. See
/// [`Waiting::Screenshot`].
const SETTLE: Duration = Duration::from_millis(250);

/// What running a command produced.
pub enum Outcome {
    /// Answer it now.
    Reply(Reply),
    /// Keep it, and answer it when [`Waiting`] says it is ready.
    Hold(Waiting),
}

/// A request that has been accepted and is waiting for something.
pub enum Waiting {
    /// A picture of the window, once it has stopped moving and one has been painted.
    ///
    /// `settle` counts the frames still to be drawn before the picture is asked for. It exists
    /// because a screenshot taken on the frame a command lands catches the window **mid animation**:
    /// egui fades a modal and its backdrop in over about a twelfth of a second, and the first
    /// picture of a newly opened `Settings` showed the editor's text through it, half faded. That is
    /// what the window really looked like at that instant, and it is not what anybody wanted to be
    /// shown. Settling first costs a quarter of a second and makes the picture the answer to "what
    /// does it look like now" rather than "what did it look like on the way there".
    Screenshot { path: PathBuf, until: Instant, settled: Instant, asked: bool },
    /// Some text on the terminal's screen.
    TerminalText { needle: String, lines: Option<usize>, until: Instant },
    /// A search that is still running.
    ModalResults { limit: usize, until: Instant },
    /// The references search, which runs on the same kind of thread.
    ///
    /// It carries what is to be done with the answer, because `editor references` and
    /// `editor rename` are the same search asked for two reasons and a second waiting variant
    /// would be a second place to get the cancellation right.
    References { until: Instant, code_only: bool, rename: Option<CliRename> },
    /// Git, which is on a thread of its own.
    Git { until: Instant },
}

/// A rename a command asked for, waiting for the search that will find what it changes.
pub struct CliRename {
    to: String,
    /// `Some(true)` for this file, `Some(false)` for the project, `None` for whatever the name
    /// resolves to — which is the modal's own default.
    scope: Option<bool>,
    /// `comments`, `strings`, or neither, which is the default: they are textual matches.
    include: Vec<String>,
    /// False when the change set is only to be printed.
    apply: bool,
}

impl Waiting {
    fn until(&self) -> Instant {
        match self {
            Waiting::Screenshot { until, .. }
            | Waiting::TerminalText { until, .. }
            | Waiting::ModalResults { until, .. }
            | Waiting::References { until, .. }
            | Waiting::Git { until } => *until,
        }
    }
}

/// Answer it now, with a sentence and some data.
fn ok(request: &Request, message: impl Into<String>, result: Value) -> Outcome {
    Outcome::Reply(Reply::done(&request.command, message, result))
}

/// Refuse it, with a code a caller can match on and a sentence a person can read.
fn no(request: &Request, code: &str, message: impl Into<String>) -> Outcome {
    Outcome::Reply(Reply::failed(&request.command, code, message))
}

/// Nothing but a sentence, which is what most commands that change something answer with.
fn done(request: &Request, message: impl Into<String>) -> Outcome {
    ok(request, message, Value::Null)
}

/// A reply whose data is a list of lines the client prints as they are.
fn lines(request: &Request, message: impl Into<String>, lines: Vec<String>, extra: Value) -> Outcome {
    let mut result = match extra {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    result.insert("lines".to_owned(), json!(lines));
    ok(request, message, Value::Object(result))
}

impl QuillApp {
    /// Take everything the command channel has, run it, and answer whatever is now ready.
    ///
    /// Called once at the top of a frame, before anything is drawn, so that a command's effect is in
    /// the frame that is about to be painted and therefore in the next screenshot.
    pub fn pump_control(&mut self, ctx: &egui::Context) {
        self.finish_waiting(ctx);
        let arrived = match &self.control {
            Some(server) => server.take(),
            None => Vec::new(),
        };
        for pending in arrived {
            let outcome = self.run_cli(&pending.request, ctx);
            self.settle(pending, outcome);
        }
        if !self.cli_waiting.is_empty() {
            // Something is being waited for, so keep drawing: a window that has gone to sleep is a
            // request that is never answered.
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    /// Answer a request now, or keep it.
    fn settle(&mut self, pending: Pending, outcome: Outcome) {
        match outcome {
            Outcome::Reply(reply) => pending.answer(reply),
            Outcome::Hold(waiting) => self.cli_waiting.push((pending, waiting)),
        }
    }

    /// Look at everything that is waiting, and answer what is ready or out of time.
    fn finish_waiting(&mut self, ctx: &egui::Context) {
        if self.cli_waiting.is_empty() {
            return;
        }
        let picture = screenshot_from(ctx);
        let mut still_waiting = Vec::new();
        for (pending, mut waiting) in std::mem::take(&mut self.cli_waiting) {
            match self.ready(&pending.request, &mut waiting, ctx, picture.as_ref()) {
                Some(reply) => pending.answer(reply),
                None if Instant::now() >= waiting.until() => {
                    pending.answer(self.timed_out(&waiting))
                }
                None => still_waiting.push((pending, waiting)),
            }
        }
        self.cli_waiting = still_waiting;
    }

    /// The reply for something that was waiting, if it is ready.
    fn ready(
        &mut self,
        request: &Request,
        waiting: &mut Waiting,
        ctx: &egui::Context,
        picture: Option<&egui::ColorImage>,
    ) -> Option<Reply> {
        match waiting {
            Waiting::Screenshot { path, settled, asked, .. } => {
                if !*asked {
                    if Instant::now() < *settled {
                        ctx.request_repaint();
                        return None;
                    }
                    ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
                    ctx.request_repaint();
                    *asked = true;
                    return None;
                }
                let picture = picture?;
                Some(match write_png(picture, path) {
                    Ok(()) => Reply::done(
                        &request.command,
                        format!("Wrote {}", path.display()),
                        json!({
                            "path": path.to_string_lossy(),
                            "width": picture.size[0],
                            "height": picture.size[1],
                        }),
                    ),
                    Err(problem) => Reply::failed(
                        &request.command,
                        code::FAILED,
                        format!("Could not write {}: {problem}", path.display()),
                    ),
                })
            }
            Waiting::TerminalText { needle, lines, .. } => {
                let screen = self.terminal_text(*lines)?;
                screen.contains(needle.as_str()).then(|| {
                    Reply::done(
                        &request.command,
                        format!("Found {needle} on the terminal"),
                        json!({ "text": screen, "waitedFor": needle, "found": true }),
                    )
                })
            }
            Waiting::ModalResults { limit, .. } => {
                let find = self.find_in_files.as_ref()?;
                (!find.is_searching()).then(|| self.modal_results_reply(request, *limit))
            }
            Waiting::References { .. } => {
                let modal = self.references.as_ref()?;
                (!modal.is_searching()).then(|| self.references_reply(request, waiting))
            }
            Waiting::Git { .. } => {
                let git = self.git.as_ref()?;
                (git.running().is_none()).then(|| self.git_status_reply(request))
            }
        }
    }

    /// What to say when the time ran out.
    fn timed_out(&mut self, waiting: &Waiting) -> Reply {
        let (command, message) = match waiting {
            Waiting::Screenshot { path, .. } => (
                "window.screenshot",
                format!(
                    "The window did not paint a frame to capture, so nothing was written to {}.",
                    path.display()
                ),
            ),
            Waiting::TerminalText { needle, lines, .. } => {
                let screen = self.terminal_text(*lines).unwrap_or_default();
                return Reply {
                    ok: false,
                    command: "terminal.read".to_owned(),
                    message: format!("{needle} did not appear on the terminal in time."),
                    result: json!({ "text": screen, "waitedFor": needle, "found": false }),
                    error: Some(quill_cli::protocol::Failure {
                        code: code::TIMED_OUT.to_owned(),
                        message: format!("{needle} did not appear on the terminal in time."),
                    }),
                };
            }
            Waiting::ModalResults { .. } => {
                ("modal.results", "The search was still running when the time ran out.".to_owned())
            }
            Waiting::References { rename, .. } => (
                if rename.is_some() { "editor.rename" } else { "editor.references" },
                "The search was still running when the time ran out, so nothing was changed."
                    .to_owned(),
            ),
            Waiting::Git { .. } => {
                ("git.action", "Git was still running when the time ran out.".to_owned())
            }
        };
        Reply::failed(command, code::TIMED_OUT, message)
    }

    /// Run a whole command line, the way `quill-cli` would, and take the answer.
    ///
    /// `line` is what somebody types after the word `quill-cli`. It is parsed against the same
    /// catalogue the client parses against and run through the same [`Self::run_cli`], so a test
    /// driving Quill this way goes down the whole command line path apart from the socket — the
    /// parser, the argument names, the dispatch and the reply are all the real ones.
    ///
    /// `None` for a command that is answered on a later frame: a screenshot, or one of the three
    /// waits. Those need the frame loop, so they belong to the running window rather than to a test
    /// that calls this and looks at the answer.
    pub fn run_command_line(&mut self, line: &str, ctx: &egui::Context) -> Option<Reply> {
        let words = split_line(line);
        let typed = match quill_cli::parse::parse(&words) {
            Ok(typed) => typed,
            Err(problem) => return Some(Reply::failed("", code::USAGE, problem.message)),
        };
        let Some(command) = typed.command else {
            return Some(Reply::failed("", code::USAGE, "no command"));
        };
        let request = Request::new("", &command.wire(), typed.arguments);
        match self.run_cli(&request, ctx) {
            Outcome::Reply(reply) => Some(reply),
            Outcome::Hold(_) => None,
        }
    }

    /// Run a request that has already been built, and take the answer.
    ///
    /// The same as [`Self::run_command_line`] for a caller that has a [`Request`] rather than a line
    /// of text — which is a test walking the whole catalogue and asking the window whether it knows
    /// each command. `None` when the answer belongs to a later frame.
    pub fn run_cli_for_test(&mut self, request: &Request, ctx: &egui::Context) -> Option<Reply> {
        match self.run_cli(request, ctx) {
            Outcome::Reply(reply) => Some(reply),
            Outcome::Hold(_) => None,
        }
    }

    /// Run one command. The single place a command line request turns into a change.
    fn run_cli(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        let (area, verb) = match request.command.split_once('.') {
            Some((area, verb)) => (area, verb),
            None => ("", request.command.as_str()),
        };
        match area {
            "" => self.cli_top(request, verb, ctx),
            "window" => self.cli_window(request, verb, ctx),
            "tab" => self.cli_tab(request, verb),
            "pane" => self.cli_pane(request, verb),
            "editor" => self.cli_editor(request, verb, ctx),
            "highlight" => self.cli_highlight(request, verb),
            "terminal" => self.cli_terminal(request, verb),
            "explorer" => self.cli_explorer(request, verb),
            "modal" => self.cli_modal(request, verb, ctx),
            "settings" => self.cli_settings(request, verb),
            "plugins" => self.cli_plugins(request, verb),
            "git" => self.cli_git(request, verb),
            "action" => self.cli_action(request, verb, ctx),
            "project" => self.cli_project(request, verb),
            "mcp" => self.cli_mcp(request, verb),
            _ => no(
                request,
                code::UNKNOWN_COMMAND,
                format!("There is no command called {}.", request.command),
            ),
        }
    }

    /// A relative path is relative to the project. See the note at the top of this file.
    fn cli_path(&self, text: &str) -> PathBuf {
        let path = PathBuf::from(text);
        if path.is_absolute() {
            path
        } else {
            self.tree.root().join(path)
        }
    }

    /// The path argument called `name`, resolved.
    fn cli_path_argument(&self, request: &Request, name: &str) -> Option<PathBuf> {
        request.text(name).map(|text| self.cli_path(&text))
    }
}

/// Split a command line the way a shell would, honouring quotation marks.
///
/// A shell has already done this by the time `quill-cli` sees its arguments. Anything driving
/// [`QuillApp::run_command_line`] has not had a shell, so it is done here — and it is the same two
/// rules a shell keeps: whitespace separates, and a quoted run is one word however much whitespace
/// is in it.
fn split_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    for character in line.chars() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => word.push(c),
            (None, c @ ('"' | '\'')) => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            (None, c) => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// The screenshot in this frame's input, if one arrived.
fn screenshot_from(ctx: &egui::Context) -> Option<egui::ColorImage> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Screenshot { image, .. } => Some((**image).clone()),
            _ => None,
        })
    })
}

/// Write a captured frame to a PNG.
fn write_png(image: &egui::ColorImage, path: &Path) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        if !folder.as_os_str().is_empty() {
            std::fs::create_dir_all(folder)?;
        }
    }
    let width = image.size[0] as u32;
    let height = image.size[1] as u32;
    // egui holds colour premultiplied by alpha; a PNG holds it straight. Writing the premultiplied
    // bytes as though they were straight ones darkens everything the window lets the desktop through,
    // which is most of it.
    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    let buffer = image::RgbaImage::from_raw(width, height, bytes)
        .ok_or_else(|| std::io::Error::other("the captured frame was the wrong size"))?;
    buffer
        .save(path)
        .map_err(|problem| std::io::Error::other(format!("{problem}")))
}

/// How long a command was told to wait, or the default.
fn waits_for(request: &Request, name: &str, fallback: Duration) -> Instant {
    let milliseconds = request.number(name).filter(|value| *value >= 0.0).map(|value| value as u64);
    Instant::now() + milliseconds.map(Duration::from_millis).unwrap_or(fallback)
}

impl QuillApp {
    // ------------------------------------------------------------------ the CLI and a whole Quill

    fn cli_top(&mut self, request: &Request, verb: &str, ctx: &egui::Context) -> Outcome {
        match verb {
            "status" => ok(request, self.status_sentence(), self.status_value(ctx)),
            "quit" => {
                self.run_action(Action::Quit, ctx);
                done(request, "Quill is closing.")
            }
            _ => no(
                request,
                code::UNKNOWN_COMMAND,
                format!("There is no command called {}.", request.command),
            ),
        }
    }

    /// The one line `status` shows when nobody asked for JSON.
    fn status_sentence(&self) -> String {
        format!(
            "{} \u{00B7} {} tab{} \u{00B7} {} \u{00B7} explorer {} \u{00B7} terminal {}",
            self.tree.root().display(),
            self.files.len(),
            if self.files.len() == 1 { "" } else { "s" },
            self.files.active().name(),
            if self.explorer_visible { "shown" } else { "hidden" },
            if self.terminal.visible { "shown" } else { "hidden" },
        )
    }

    /// Everything about the window, in one value.
    ///
    /// One command rather than eight, because the first thing anything driving Quill wants is to
    /// know where it is, and eight round trips to find that out is eight chances to read a window
    /// that changed underneath you.
    fn status_value(&self, ctx: &egui::Context) -> Value {
        let screen = ctx.content_rect();
        json!({
            "version": crate::build_info::VERSION,
            "buildDate": crate::build_info::BUILD_DATE,
            "pid": std::process::id(),
            "port": self.control.as_ref().map(|server| server.port()),
            "project": self.tree.root().to_string_lossy(),
            "window": { "width": screen.width(), "height": screen.height() },
            "tabs": self.tabs_value(),
            "activeTab": self.files.active_index(),
            "panes": self.panes_value(),
            "editor": self.editor_value(),
            "explorer": {
                "visible": self.explorer_visible,
                "width": self.panes.explorer_width,
                "filter": self.filter,
                "rows": self.tree.rows().len(),
                "files": self.tree.all_files().len(),
            },
            "terminal": self.terminal_value(),
            "modal": self.modal_value(ctx),
            "settings": self.settings_value(),
            "git": self.git_value(),
            "message": self.message,
        })
    }

    // --------------------------------------------------------------------------------- the window

    fn cli_window(&mut self, request: &Request, verb: &str, ctx: &egui::Context) -> Outcome {
        match verb {
            "screenshot" => {
                let Some(path) = self.cli_path_argument(request, "file") else {
                    return no(request, code::USAGE, "Say where to write the picture.");
                };
                ctx.request_repaint();
                Outcome::Hold(Waiting::Screenshot {
                    path,
                    until: waits_for(request, "timeout", SCREENSHOT_WAIT),
                    settled: Instant::now() + SETTLE,
                    asked: false,
                })
            }
            "focus" => {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                done(request, "Brought the window to the front.")
            }
            "size" => self.cli_window_size(request, ctx),
            "position" => self.cli_window_position(request, ctx),
            "message" => {
                if request.has("text") {
                    self.message = request.text("text");
                    done(request, format!("Showing {}", self.message.clone().unwrap_or_default()))
                } else if request.arguments.contains_key("text") {
                    self.message = None;
                    done(request, "Cleared the status bar message.")
                } else {
                    ok(
                        request,
                        self.message.clone().unwrap_or_else(|| "The status bar has no message.".to_owned()),
                        json!({ "message": self.message }),
                    )
                }
            }
            _ => unknown(request),
        }
    }

    fn cli_window_size(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        let screen = ctx.content_rect();
        let width = request.number("width").map(|value| value as f32);
        let height = request.number("height").map(|value| value as f32);
        if width.is_none() && height.is_none() {
            return ok(
                request,
                format!("{} by {} points", screen.width(), screen.height()),
                json!({ "width": screen.width(), "height": screen.height() }),
            );
        }
        let wanted = egui::Vec2::new(
            width.unwrap_or(screen.width()).max(320.0),
            height.unwrap_or(screen.height()).max(240.0),
        );
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(wanted));
        ok(
            request,
            format!("Set the window to {} by {} points", wanted.x, wanted.y),
            json!({ "width": wanted.x, "height": wanted.y }),
        )
    }

    fn cli_window_position(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        let outer = ctx.input(|input| input.viewport().outer_rect);
        let x = request.number("x").map(|value| value as f32);
        let y = request.number("y").map(|value| value as f32);
        if x.is_none() && y.is_none() {
            let at = outer.map(|rect| rect.min).unwrap_or(egui::Pos2::ZERO);
            return ok(
                request,
                format!("The window is at {}, {}", at.x, at.y),
                json!({ "x": at.x, "y": at.y, "known": outer.is_some() }),
            );
        }
        let at = outer.map(|rect| rect.min).unwrap_or(egui::Pos2::ZERO);
        let wanted = egui::Pos2::new(x.unwrap_or(at.x), y.unwrap_or(at.y));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(wanted));
        ok(
            request,
            format!("Moved the window to {}, {}", wanted.x, wanted.y),
            json!({ "x": wanted.x, "y": wanted.y }),
        )
    }

    // ----------------------------------------------------------------------------------- the tabs

    fn cli_tab(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "open" => self.cli_tab_open(request),
            "list" => {
                let rows: Vec<String> = self
                    .files
                    .iter()
                    .enumerate()
                    .map(|(at, file)| {
                        format!(
                            "{}{at:<3} {}{}",
                            if at == self.files.active_index() { "*" } else { " " },
                            file.name(),
                            if file.document.is_modified() { " (unsaved)" } else { "" }
                        )
                    })
                    .collect();
                lines(
                    request,
                    format!("{} open", self.files.len()),
                    rows,
                    json!({ "tabs": self.tabs_value(), "activeTab": self.files.active_index() }),
                )
            }
            "show" => match self.cli_find_tab(request, "tab") {
                Ok(index) => {
                    self.show_tab(index);
                    done(request, format!("Showing {}", self.files.active().name()))
                }
                Err(outcome) => outcome,
            },
            "close" => self.cli_tab_close(request),
            "next" => {
                self.files.next();
                self.forget_layout();
                done(request, format!("Showing {}", self.files.active().name()))
            }
            "previous" => {
                self.files.previous();
                self.forget_layout();
                done(request, format!("Showing {}", self.files.active().name()))
            }
            "move" => self.cli_tab_move(request),
            "save" => self.cli_tab_save(request),
            "save-as" => self.cli_tab_save_as(request),
            "reload" => self.cli_tab_reload(request),
            _ => unknown(request),
        }
    }

    /// `quill-cli tab move <position> [--tab] [--pane]` — what dragging a tab does.
    ///
    /// It goes through `OpenFiles::drag_tab`, which is the same call the drag makes, so a
    /// rearrangement made from a script and one made with the pointer are the same rearrangement —
    /// including what `position` counts, which is the target pane's tabs as they are on the screen.
    fn cli_tab_move(&mut self, request: &Request) -> Outcome {
        let Some(position) = request.whole("position") else {
            return no(request, code::USAGE, "Say where it goes, counting from 0.");
        };
        let index = if request.has("tab") {
            match self.cli_find_tab(request, "tab") {
                Ok(index) => index,
                Err(outcome) => return outcome,
            }
        } else {
            self.files.active_index()
        };
        let pane = request.whole("pane").unwrap_or_else(|| self.files.pane_of(index));
        if pane >= self.files.pane_count() {
            return no(
                request,
                code::NOT_FOUND,
                format!("There is no pane {pane}; there are {}.", self.files.pane_count()),
            );
        }
        let name = self.files.at(index).name();
        if !self.files.drag_tab(index, pane, position) {
            return no(request, code::NOT_APPLICABLE, format!("{name} could not be moved there."));
        }
        self.forget_layout();
        let landed = self.files.active_index();
        ok(
            request,
            format!(
                "{name} is tab {} of pane {}",
                self.files.tabs_in(self.files.pane_of(landed)).iter().position(|at| *at == landed).unwrap_or(0),
                self.files.pane_of(landed)
            ),
            self.panes_value(),
        )
    }

    // ---------------------------------------------------------------- the panes

    /// `quill-cli pane ...` — the editing area split into panes.
    ///
    /// Every verb goes through the action or the `OpenFiles` method the menus go through, so a split
    /// made from a script and a split made by right clicking a tab are the same split.
    fn cli_pane(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "list" => {
                let rows: Vec<String> = (0..self.files.pane_count())
                    .map(|pane| {
                        let showing = self
                            .files
                            .showing_in(pane)
                            .map(|index| self.files.at(index).name())
                            .unwrap_or_default();
                        format!(
                            "{}{pane:<3} {:>2} tab{}  showing {showing}",
                            if pane == self.files.focused_pane() { "*" } else { " " },
                            self.files.tabs_in(pane).len(),
                            if self.files.tabs_in(pane).len() == 1 { " " } else { "s" },
                        )
                    })
                    .collect();
                let panes = self.files.pane_count();
                lines(
                    request,
                    format!("{panes} pane{}", if panes == 1 { "" } else { "s" }),
                    rows,
                    self.panes_value(),
                )
            }
            "split" => {
                self.files.split_right();
                ok(
                    request,
                    format!(
                        "Split into {} panes, showing {}",
                        self.files.pane_count(),
                        self.files.active().name()
                    ),
                    self.panes_value(),
                )
            }
            "move" => {
                let Some(direction) = request.text("direction") else {
                    return no(request, code::USAGE, "Say which way: left or right.");
                };
                let right = match direction.trim().to_ascii_lowercase().as_str() {
                    "right" => true,
                    "left" => false,
                    other => {
                        return no(
                            request,
                            code::USAGE,
                            format!("{other} is not a direction. Say left or right."),
                        )
                    }
                };
                if !self.files.move_tab(right) {
                    return no(
                        request,
                        code::NOT_APPLICABLE,
                        format!("There is no pane to the {direction} of this one."),
                    );
                }
                ok(
                    request,
                    format!(
                        "{} is in pane {}",
                        self.files.active().name(),
                        self.files.focused_pane()
                    ),
                    self.panes_value(),
                )
            }
            "focus" => {
                let Some(pane) = request.number("pane") else {
                    return no(request, code::USAGE, "Say which pane, counting from 0.");
                };
                let pane = pane.max(0.0) as usize;
                if !self.files.focus_pane(pane) {
                    return no(
                        request,
                        code::NOT_FOUND,
                        format!("There is no pane {pane}. There are {}.", self.files.pane_count()),
                    );
                }
                self.focus = crate::app::Focus::Editor;
                ok(
                    request,
                    format!("Pane {pane} has the keyboard, showing {}", self.files.active().name()),
                    self.panes_value(),
                )
            }
            "width" => {
                let (Some(pane), Some(fraction)) = (request.number("pane"), request.number("fraction"))
                else {
                    return no(request, code::USAGE, "Say which pane and what share of the width.");
                };
                let pane = pane.max(0.0) as usize;
                if !self.files.set_pane_width(pane, fraction as f32) {
                    return no(
                        request,
                        code::NOT_APPLICABLE,
                        format!(
                            "There is no pane {pane} to widen. There are {}.",
                            self.files.pane_count()
                        ),
                    );
                }
                ok(request, format!("Pane {pane} is {fraction} of the editing area"), self.panes_value())
            }
            "unsplit" | "unsplit-all" => {
                let all = verb == "unsplit-all";
                let done_it = if all { self.files.unsplit_all() } else { self.files.unsplit() };
                if !done_it {
                    return no(request, code::NOT_APPLICABLE, "The editing area is not split.");
                }
                ok(
                    request,
                    format!("{} pane{} left", self.files.pane_count(), if self.files.pane_count() == 1 { "" } else { "s" }),
                    self.panes_value(),
                )
            }
            _ => unknown(request),
        }
    }

    /// The panes, for `pane list` and for `status`.
    fn panes_value(&self) -> Value {
        json!({
            "count": self.files.pane_count(),
            "focused": self.files.focused_pane(),
            "panes": (0..self.files.pane_count())
                .map(|pane| json!({
                    "pane": pane,
                    "width": self.files.pane_widths().get(pane).copied().unwrap_or(0.0),
                    "tabs": self.files.tabs_in(pane),
                    "showing": self.files.showing_in(pane),
                    "name": self
                        .files
                        .showing_in(pane)
                        .map(|index| self.files.at(index).name()),
                }))
                .collect::<Vec<Value>>(),
        })
    }

    fn cli_tab_open(&mut self, request: &Request) -> Outcome {
        let Some(path) = self.cli_path_argument(request, "path") else {
            return no(request, code::USAGE, "Say which file to open.");
        };
        if !path.exists() {
            return no(request, code::NOT_FOUND, format!("There is no file at {}", path.display()));
        }
        if path.is_dir() {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!("{} is a folder. `project open` shows a folder.", path.display()),
            );
        }
        if let Err(refusal) = file_kind::openable(&path) {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!("Quill cannot open {}: {}", path.display(), refusal.reason()),
            );
        }
        if request.switch("permanent") {
            self.open_path_permanently(&path);
        } else {
            self.open_path(&path);
        }
        ok(
            request,
            format!("Opened {} in tab {}", path.display(), self.files.active_index()),
            json!({
                "tab": self.files.active_index(),
                "path": path.to_string_lossy(),
                "picture": self.files.active().is_picture(),
            }),
        )
    }

    fn cli_tab_close(&mut self, request: &Request) -> Outcome {
        let index = if request.has("tab") {
            match self.cli_find_tab(request, "tab") {
                Ok(index) => index,
                Err(outcome) => return outcome,
            }
        } else {
            self.files.active_index()
        };
        let name = self.files.get(index).map(|file| file.name()).unwrap_or_default();
        self.close_tab(index);
        ok(
            request,
            format!("Closed {name}"),
            json!({ "closed": name, "tabs": self.files.len() }),
        )
    }

    fn cli_tab_save(&mut self, request: &Request) -> Outcome {
        if self.files.active().is_picture() {
            return no(request, code::NOT_APPLICABLE, "A picture cannot be edited, so there is nothing to save.");
        }
        if self.files.active().path().is_none() {
            return no(
                request,
                code::NOT_APPLICABLE,
                "This tab has never been saved. Use `tab save-as <path>`.",
            );
        }
        self.save();
        match self.files.active().document.is_modified() {
            false => {
                let path = self.files.active().path().map(|p| p.display().to_string()).unwrap_or_default();
                ok(request, format!("Saved {path}"), json!({ "path": path }))
            }
            true => no(request, code::FAILED, "Quill could not write the file. The status bar says why."),
        }
    }

    fn cli_tab_save_as(&mut self, request: &Request) -> Outcome {
        let Some(path) = self.cli_path_argument(request, "path") else {
            return no(request, code::USAGE, "Say where to write it.");
        };
        if self.files.active().is_picture() {
            return no(request, code::NOT_APPLICABLE, "A picture cannot be edited, so there is nothing to save.");
        }
        match self.files.active_mut().document.save_as(&path) {
            Ok(()) => {
                self.tree.reload();
                ok(request, format!("Saved {}", path.display()), json!({ "path": path.to_string_lossy() }))
            }
            Err(problem) => no(
                request,
                code::FAILED,
                format!("Could not write {}: {problem}", path.display()),
            ),
        }
    }

    fn cli_tab_reload(&mut self, request: &Request) -> Outcome {
        let Some(path) = self.files.active().path().map(Path::to_path_buf) else {
            return no(request, code::NOT_APPLICABLE, "This tab has never been saved.");
        };
        let discard = request.switch("discard");
        if !discard && self.document().is_modified() {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!(
                    "{} has unsaved changes, so it was not reloaded. Save it first, or say \
                     --discard to throw them away.",
                    path.display()
                ),
            );
        }
        if self.reload_from_disk(&path, discard) {
            ok(
                request,
                format!("Read {} again", path.display()),
                json!({ "path": path.to_string_lossy(), "discarded": discard }),
            )
        } else {
            no(
                request,
                code::FAILED,
                self.message.clone().unwrap_or_else(|| format!("Could not reload {}", path.display())),
            )
        }
    }

    /// The tab an argument names: its number, its name, or its path.
    fn cli_find_tab(&self, request: &Request, name: &str) -> Result<usize, Outcome> {
        let Some(text) = request.text(name) else {
            return Err(no(request, code::USAGE, "Say which tab."));
        };
        if let Ok(index) = text.trim().parse::<usize>() {
            return if index < self.files.len() {
                Ok(index)
            } else {
                Err(no(
                    request,
                    code::NOT_FOUND,
                    format!("There is no tab {index}; there are {}.", self.files.len()),
                ))
            };
        }
        let wanted = self.cli_path(&text);
        let found = self
            .files
            .iter()
            .position(|file| file.path() == Some(wanted.as_path()) || file.name() == text);
        found.ok_or_else(|| no(request, code::NOT_FOUND, format!("No tab is showing {text}.")))
    }

    fn tabs_value(&self) -> Value {
        json!(self
            .files
            .iter()
            .enumerate()
            .map(|(at, file)| json!({
                "index": at,
                "name": file.name(),
                "path": file.path().map(|path| path.to_string_lossy()),
                "modified": file.document.is_modified(),
                "picture": file.is_picture(),
                "transient": file.transient,
                "viewMode": view_mode_name(file.view_mode),
                "pane": file.pane,
            }))
            .collect::<Vec<Value>>())
    }
}

impl QuillApp {
    // ---------------------------------------------------------------- the marked passages

    /// `quill-cli highlight ...` — the passages marked in the project's files.
    ///
    /// Every one of these works on a file whether it is open or not, which is the point of the bulk
    /// commands: an agent that has worked out twenty places worth marking marks them in one call and
    /// none of the files has to be opened first. [`QuillApp::change_highlights`] is where the choice
    /// between the open document and the store is made, so nothing here has to think about it.
    fn cli_highlight(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "list" => self.cli_highlight_list(request),
            "add" => self.cli_highlight_add(request),
            "clear" => self.cli_highlight_clear(request),
            "apply" => self.cli_highlight_apply(request),
            _ => unknown(request),
        }
    }

    fn cli_highlight_list(&mut self, request: &Request) -> Outcome {
        let paths: Vec<PathBuf> = if request.switch("all") {
            let mut paths: Vec<PathBuf> =
                self.marks.files().iter().map(|(path, _)| (*path).clone()).collect();
            for path in self.files.paths() {
                if !self.highlights_of(&path).is_empty() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
            paths.sort();
            paths
        } else {
            match self.highlight_target(request) {
                Ok(path) => vec![path],
                Err(refusal) => return no(request, code::USAGE, refusal),
            }
        };
        let mut rows: Vec<String> = Vec::new();
        let mut listed: Vec<Value> = Vec::new();
        for path in paths {
            let marks = self.highlights_of(&path);
            if marks.is_empty() {
                continue;
            }
            let text = self.highlight_text(&path).ok();
            for mark in marks.iter() {
                let (from, to) = match &text {
                    Some(text) => (
                        status_bar::position_of(text, mark.range.start),
                        status_bar::position_of(text, mark.range.end),
                    ),
                    None => (
                        status_bar::Position { line: 0, column: 0 },
                        status_bar::Position { line: 0, column: 0 },
                    ),
                };
                let words = text
                    .as_ref()
                    .map(|text| {
                        text.byte_slice(mark.range.start..mark.range.end.min(text.len_bytes()))
                    })
                    .unwrap_or_default();
                rows.push(format!(
                    "{:<10} {:>4}:{:<3} {:>4}:{:<3} {}",
                    mark.color.to_hex(),
                    from.line,
                    from.column,
                    to.line,
                    to.column,
                    self.tree_relative(&path)
                ));
                listed.push(json!({
                    "path": path.to_string_lossy(),
                    "start": mark.range.start,
                    "end": mark.range.end,
                    "fromLine": from.line,
                    "fromColumn": from.column,
                    "toLine": to.line,
                    "toColumn": to.column,
                    "color": mark.color.to_hex(),
                    "text": words,
                }));
            }
        }
        lines(
            request,
            format!("{} highlight{}", listed.len(), if listed.len() == 1 { "" } else { "s" }),
            rows,
            json!({ "highlights": listed }),
        )
    }

    fn cli_highlight_add(&mut self, request: &Request) -> Outcome {
        let path = match self.highlight_target(request) {
            Ok(path) => path,
            Err(refusal) => return no(request, code::USAGE, refusal),
        };
        let color = match request.text("color") {
            Some(name) => match HighlightColor::parse(&name) {
                Some(color) => color,
                None => {
                    return no(
                        request,
                        code::USAGE,
                        format!(
                            "{name} is not a colour. Say one of {}, or #rrggbb or #rrggbbaa.",
                            HighlightColor::names()
                        ),
                    )
                }
            },
            None => HighlightColor::Yellow.rgba(),
        };
        let text = match self.highlight_text(&path) {
            Ok(text) => text,
            Err(refusal) => return no(request, code::NOT_FOUND, refusal),
        };
        let ranges = match self.highlight_ranges(request, &text) {
            Ok(ranges) => ranges,
            Err(refusal) => return no(request, code::USAGE, refusal),
        };
        if ranges.is_empty() {
            return no(
                request,
                code::NOT_FOUND,
                "Nothing in that file matched, so nothing was marked.",
            );
        }
        let marked = ranges.len();
        self.change_highlights(&path, |marks| {
            for range in &ranges {
                marks.add(range.clone(), color);
            }
        });
        self.forget_layout();
        ok(
            request,
            format!(
                "Marked {marked} passage{} in {}",
                if marked == 1 { "" } else { "s" },
                self.tree_relative(&path)
            ),
            json!({
                "path": path.to_string_lossy(),
                "marked": marked,
                "color": color.to_hex(),
                "highlights": self.highlights_of(&path).len(),
            }),
        )
    }

    fn cli_highlight_clear(&mut self, request: &Request) -> Outcome {
        if request.switch("all") {
            // Counted before anything is taken away, and each file counted once. A file that is open
            // is in the store as well — the window pushes it there every frame — so adding the two
            // totals together would report twice as many as there were.
            let open: Vec<PathBuf> = self.files.paths();
            let mut cleared: usize =
                (0..self.files.len()).map(|index| self.files.at(index).document.highlights().len()).sum();
            cleared += self
                .marks
                .files()
                .iter()
                .filter(|(path, _)| !open.contains(path))
                .map(|(_, marks)| marks.len())
                .sum::<usize>();
            self.marks.clear_all();
            for index in 0..self.files.len() {
                self.files.at_mut(index).document.clear_highlights();
            }
            self.forget_layout();
            return ok(
                request,
                format!("Cleared {cleared} highlight{}", if cleared == 1 { "" } else { "s" }),
                json!({ "cleared": cleared }),
            );
        }
        let path = match self.highlight_target(request) {
            Ok(path) => path,
            Err(refusal) => return no(request, code::USAGE, refusal),
        };
        let before = self.highlights_of(&path).len();
        match request.whole("from-line") {
            None => {
                self.change_highlights(&path, |marks| {
                    marks.clear_all();
                });
            }
            Some(from_line) => {
                let text = match self.highlight_text(&path) {
                    Ok(text) => text,
                    Err(refusal) => return no(request, code::NOT_FOUND, refusal),
                };
                let from = offset_at(&text, from_line, 1);
                let to =
                    offset_at(&text, request.whole("to-line").unwrap_or(from_line), usize::MAX);
                self.change_highlights(&path, |marks| {
                    marks.clear(from..to);
                });
            }
        }
        self.forget_layout();
        let after = self.highlights_of(&path).len();
        ok(
            request,
            format!(
                "{} highlight{} left in {}",
                after,
                if after == 1 { "" } else { "s" },
                self.tree_relative(&path)
            ),
            json!({
                "path": path.to_string_lossy(),
                "cleared": before.saturating_sub(after),
                "highlights": after,
            }),
        )
    }

    fn cli_highlight_apply(&mut self, request: &Request) -> Outcome {
        let text = match (request.text("from-file"), request.text("json-text")) {
            (Some(named), _) => {
                let path = self.cli_path(&named);
                match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(problem) => {
                        return no(
                            request,
                            code::NOT_FOUND,
                            format!("Could not read {}: {problem}", path.display()),
                        )
                    }
                }
            }
            (None, Some(inline)) => inline,
            (None, None) => {
                return no(request, code::USAGE, "Say --from-file <path> or --json-text <json>.")
            }
        };
        let wanted: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(problem) => return no(request, code::USAGE, format!("That is not JSON: {problem}")),
        };
        let Some(list) = wanted.as_array() else {
            return no(
                request,
                code::USAGE,
                "The JSON should be an array of {path, fromLine, toLine, color} objects.",
            );
        };
        if request.switch("replace") {
            self.marks.clear_all();
            for index in 0..self.files.len() {
                self.files.at_mut(index).document.clear_highlights();
            }
        }
        let mut marked = 0;
        let mut touched: Vec<String> = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        for (at, entry) in list.iter().enumerate() {
            match self.apply_one_highlight(entry) {
                Ok(path) => {
                    marked += 1;
                    let shown = self.tree_relative(&path);
                    if !touched.contains(&shown) {
                        touched.push(shown);
                    }
                }
                Err(refusal) => refused.push(format!("{}: {refusal}", at + 1)),
            }
        }
        self.forget_layout();
        let message = format!(
            "Marked {marked} passage{} across {} file{}{}",
            if marked == 1 { "" } else { "s" },
            touched.len(),
            if touched.len() == 1 { "" } else { "s" },
            if refused.is_empty() {
                String::new()
            } else {
                format!(", and refused {}", refused.len())
            }
        );
        lines(
            request,
            message,
            refused.clone(),
            json!({ "marked": marked, "files": touched, "refused": refused }),
        )
    }

    /// One entry of a bulk request. The error is what is reported against that entry's number, so a
    /// list with one bad row in it still applies the rest — which is what an agent wants of a batch.
    fn apply_one_highlight(&mut self, entry: &Value) -> Result<PathBuf, String> {
        let Some(named) = entry.get("path").and_then(Value::as_str) else {
            return Err("no path".to_owned());
        };
        let path = self.cli_path(named);
        let color = match entry.get("color").and_then(Value::as_str) {
            Some(name) => {
                HighlightColor::parse(name).ok_or_else(|| format!("{name} is not a colour"))?
            }
            None => HighlightColor::Yellow.rgba(),
        };
        let text = self.highlight_text(&path)?;
        let whole = |name: &str| entry.get(name).and_then(Value::as_u64).map(|n| n as usize);
        let from_line = whole("fromLine").ok_or_else(|| "no fromLine".to_owned())?;
        let from = offset_at(&text, from_line, whole("fromColumn").unwrap_or(1));
        let to_line = whole("toLine").unwrap_or(from_line);
        let to = offset_at(&text, to_line, whole("toColumn").unwrap_or(usize::MAX));
        if from >= to {
            return Err("the passage ends before it starts".to_owned());
        }
        self.change_highlights(&path, |marks| marks.add(from..to, color));
        Ok(path)
    }

    /// The file a highlight command is about: the one named, or the tab that is showing.
    fn highlight_target(&self, request: &Request) -> Result<PathBuf, String> {
        if let Some(path) = self.cli_path_argument(request, "path") {
            return Ok(path);
        }
        self.document()
            .path()
            .map(Path::to_path_buf)
            .ok_or_else(|| "This tab has never been saved, so name the file to mark.".to_owned())
    }

    /// The text of a file, whether it is open or not.
    ///
    /// An open file's document rather than what is on the disk, because what has been typed and not
    /// yet saved is what the person is looking at and is what the offsets have to be against.
    fn highlight_text(&self, path: &Path) -> Result<quill_core::Rope, String> {
        if let Some(index) = self.files.index_of(path) {
            return Ok(self.files.at(index).document.text().clone());
        }
        std::fs::read_to_string(path)
            .map(|text| quill_core::Rope::from_str(&text.replace("\r\n", "\n")))
            .map_err(|problem| format!("Could not read {}: {problem}", path.display()))
    }

    /// The ranges one `highlight add` asks for: a passage, or every occurrence of some words.
    fn highlight_ranges(
        &self,
        request: &Request,
        text: &quill_core::Rope,
    ) -> Result<Vec<std::ops::Range<usize>>, String> {
        if let Some(needle) = request.text("text") {
            let needle = unescape(&needle);
            if needle.is_empty() {
                return Err("Say what words to mark.".to_owned());
            }
            let whole = text.to_string();
            let mut out = Vec::new();
            let mut at = 0;
            while let Some(found) = whole[at..].find(&needle) {
                let start = at + found;
                out.push(start..start + needle.len());
                at = start + needle.len();
            }
            return Ok(out);
        }
        let Some(from_line) = request.whole("from-line") else {
            return Err("Say --from-line and --to-line, or --text.".to_owned());
        };
        let from = offset_at(text, from_line, request.whole("from-column").unwrap_or(1));
        let to_line = request.whole("to-line").unwrap_or(from_line);
        let to = offset_at(text, to_line, request.whole("to-column").unwrap_or(usize::MAX));
        if from >= to {
            return Err("The passage ends before it starts.".to_owned());
        }
        // Collected from one item rather than written as `vec![from..to]`, which clippy reads as
        // somebody who meant a list of every number between the two and warns about every build.
        Ok(std::iter::once(from..to).collect())
    }

    /// A path as it is worth showing: relative to the project when it is inside it.
    fn tree_relative(&self, path: &Path) -> String {
        crate::services::project_state::relative(self.tree.root(), path).display().to_string()
    }
}

fn unknown(request: &Request) -> Outcome {
    no(
        request,
        code::UNKNOWN_COMMAND,
        format!(
            "There is no command called {}. `quill-cli commands` lists them.",
            request.command
        ),
    )
}

fn view_mode_name(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Raw => "raw",
        ViewMode::SideBySide => "side",
        ViewMode::Preview => "preview",
    }
}

/// Undo the escapes a shell will not: `\n` and `\t` typed as two characters.
///
/// A command line cannot carry a real new line through every shell there is, and typing two lines
/// into a document is an ordinary thing to want, so the two escapes every language spells the same
/// way are understood. `\\` is a backslash, so a literal `\n` is still reachable.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Where in the document a line and a column are, both counting from one.
///
/// Past the end of a line lands at the end of that line, and past the end of the document lands at
/// the end of the document, so a caller that guessed too far still lands somewhere sensible rather
/// than being refused.
pub fn offset_at(text: &quill_core::Rope, line: usize, column: usize) -> usize {
    let line = line.saturating_sub(1).min(text.len_lines().saturating_sub(1));
    let range = text.line_range(line);
    let body = text.byte_slice(range.clone());
    let body = body.trim_end_matches('\n').trim_end_matches('\r');
    let wanted = column.saturating_sub(1);
    let mut at = range.start;
    for (count, (offset, character)) in body.char_indices().enumerate() {
        if count == wanted {
            return range.start + offset;
        }
        at = range.start + offset + character.len_utf8();
    }
    at.max(range.start)
}

impl QuillApp {
    // --------------------------------------------------------------------------------- the editor

    fn cli_editor(&mut self, request: &Request, verb: &str, ctx: &egui::Context) -> Outcome {
        match verb {
            "status" => ok(request, self.editor_sentence(), self.editor_value()),
            "text" => self.cli_editor_text(request),
            "set-text" => self.cli_editor_set_text(request),
            "insert" => self.cli_editor_insert(request),
            "caret" => self.cli_editor_caret(request),
            "select" => self.cli_editor_select(request),
            "undo" => self.cli_editor_history(request, true),
            "redo" => self.cli_editor_history(request, false),
            "view" => self.cli_editor_view(request, ctx),
            "scroll" => self.cli_editor_scroll(request),
            "preview" => self.cli_editor_preview(request, ctx),
            "definition" => self.cli_editor_definition(request),
            "references" => self.cli_editor_references(request),
            "rename" => self.cli_editor_rename(request),
            "complete" => self.cli_editor_complete(request),
            "navigate-back" => self.cli_navigate(request, true),
            "navigate-forward" => self.cli_navigate(request, false),
            _ => unknown(request),
        }
    }

    /// `quill-cli editor navigate-back` and its mirror, through the same stack the menu walks.
    fn cli_navigate(&mut self, request: &Request, back: bool) -> Outcome {
        self.message = None;
        self.navigate(back);
        match self.message.clone() {
            // `navigate` says so in the status bar when there is nowhere to go, and a command that
            // did nothing should say so rather than report success.
            Some(problem) if problem.starts_with("There is nowhere") => {
                no(request, code::NOT_APPLICABLE, problem)
            }
            _ => ok(
                request,
                format!("{} \u{00B7} {}", self.files.active().name(), self.caret_position().line),
                json!({
                    "path": self.files.active().path().map(|path| path.to_string_lossy()),
                    "offset": self.caret_offset(),
                    "back": self.back.len(),
                    "forward": self.forward.len(),
                }),
            ),
        }
    }

    /// `quill-cli editor scroll` — how far through the file the view is, and moving it.
    ///
    /// The page it measures against is the one the window laid out on the last frame it drew, which
    /// is the same page the wheel and the scrollbar move. In side by side the other half follows,
    /// through the same `follow_the_other_half` a wheel goes through — the frame's own rule cannot
    /// notice this one, because a command is applied before the frame draws anything and so there is
    /// nothing for its before-and-after comparison to see.
    fn cli_editor_scroll(&mut self, request: &Request) -> Outcome {
        if self.files.active().is_picture() {
            return no(request, code::NOT_APPLICABLE, "This tab holds a picture, which is panned rather than scrolled.");
        }
        let preview = request.switch("preview");
        let room = (self.editor_area.height() - size::EDITOR_PADDING_Y * 2.0).max(0.0);
        let page = if preview { self.preview_layout() } else { self.layout() };
        let height = page.height;
        let overflow = (height - room).max(0.0);
        let wanted = if request.switch("top") {
            Some(0.0)
        } else if request.switch("bottom") {
            Some(overflow)
        } else if let Some(line) = request.whole("line") {
            // Counting from one, as the status bar and `editor caret` do.
            let paragraph = line.max(1) - 1;
            Some(page.paragraph_band(paragraph).map(|(top, _)| top).unwrap_or(overflow))
        } else {
            request.number("to").map(|points| points as f32)
        };
        if let Some(to) = wanted {
            let to = to.clamp(0.0, overflow);
            let file = self.files.active_mut();
            if preview {
                file.preview_scroll = to;
            } else {
                file.scroll = to;
            }
            if self.view_mode() == ViewMode::SideBySide {
                self.follow_the_other_half(!preview, room);
            }
        }
        let file = self.files.active();
        ok(
            request,
            format!(
                "{} \u{00B7} source {:.0} of {:.0} \u{00B7} preview {:.0} of {:.0}",
                file.name(),
                file.scroll,
                file.cached.layout.height,
                file.preview_scroll,
                file.cached.preview_layout.height,
            ),
            json!({
                "tab": self.files.active_index(),
                "name": file.name(),
                "view": room,
                "source": { "scroll": file.scroll, "height": file.cached.layout.height },
                "preview": {
                    "scroll": file.preview_scroll,
                    "height": file.cached.preview_layout.height,
                },
                "viewMode": view_mode_name(self.view_mode()),
            }),
        )
    }

    fn editor_sentence(&self) -> String {
        let at = self.caret_position();
        format!(
            "{} \u{00B7} {} lines \u{00B7} line {} column {}{}",
            self.files.active().name(),
            self.document().text().len_lines(),
            at.line,
            at.column,
            if self.document().is_modified() { " \u{00B7} unsaved" } else { "" }
        )
    }

    fn editor_value(&self) -> Value {
        let file = self.files.active();
        let at = self.caret_position();
        let selection = self.document().selection();
        json!({
            "tab": self.files.active_index(),
            "name": file.name(),
            "path": file.path().map(|path| path.to_string_lossy()),
            "picture": file.is_picture(),
            "modified": self.document().is_modified(),
            "lines": self.document().text().len_lines(),
            "characters": self.document().text().len_chars(),
            "caret": { "line": at.line, "column": at.column, "offset": selection.head },
            "selection": {
                "empty": selection.is_empty(),
                "start": selection.start(),
                "end": selection.end(),
                "text": self.document().selected_text(),
            },
            "viewMode": view_mode_name(self.view_mode()),
            "canUndo": self.document().can_undo(),
            "canRedo": self.document().can_redo(),
            "previewApplies": file_kind::preview_applies(file.path()),
            "kind": file_kind::kind_name(file.path()),
        })
    }

    fn cli_editor_text(&mut self, request: &Request) -> Outcome {
        if self.files.active().is_picture() {
            return no(request, code::NOT_APPLICABLE, "This tab holds a picture rather than text.");
        }
        let whole = self.document().text().to_string();
        let from = request.whole("from-line").unwrap_or(1).max(1);
        let to = request.whole("to-line");
        let text = if from == 1 && to.is_none() {
            whole
        } else {
            let all: Vec<&str> = whole.split_inclusive('\n').collect();
            let last = to.unwrap_or(all.len()).min(all.len());
            if from > last {
                String::new()
            } else {
                all[from - 1..last].concat()
            }
        };
        ok(
            request,
            String::new(),
            json!({ "text": text, "fromLine": from, "toLine": to }),
        )
    }

    fn cli_editor_set_text(&mut self, request: &Request) -> Outcome {
        if self.files.active().is_picture() {
            return no(request, code::NOT_APPLICABLE, "This tab holds a picture rather than text.");
        }
        let text = match request.text("from-file") {
            Some(named) => {
                let path = self.cli_path(&named);
                match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(problem) => {
                        return no(
                            request,
                            code::NOT_FOUND,
                            format!("Could not read {}: {problem}", path.display()),
                        )
                    }
                }
            }
            None => unescape(&request.text("text").unwrap_or_default()),
        };
        // Selecting everything and typing over it, which is one edit and therefore one undo.
        self.document_mut().apply(quill_core::Command::SelectAll);
        self.document_mut().apply(quill_core::Command::Insert(text.clone()));
        self.forget_layout();
        ok(
            request,
            format!("Replaced the text with {} characters", text.chars().count()),
            json!({ "characters": text.chars().count(), "lines": self.document().text().len_lines() }),
        )
    }

    fn cli_editor_insert(&mut self, request: &Request) -> Outcome {
        if self.files.active().is_picture() {
            return no(request, code::NOT_APPLICABLE, "This tab holds a picture rather than text.");
        }
        let Some(text) = request.text("text") else {
            return no(request, code::USAGE, "Say what to type.");
        };
        let text = unescape(&text);
        self.document_mut().apply(quill_core::Command::Insert(text.clone()));
        self.forget_layout();
        self.reveal_caret = true;
        let at = self.caret_position();
        ok(
            request,
            format!("Typed {} characters", text.chars().count()),
            json!({ "caret": { "line": at.line, "column": at.column } }),
        )
    }

    fn cli_editor_caret(&mut self, request: &Request) -> Outcome {
        let line = request.whole("line");
        let column = request.whole("column");
        if line.is_none() && column.is_none() {
            let at = self.caret_position();
            return ok(
                request,
                format!("Line {} column {}", at.line, at.column),
                json!({ "line": at.line, "column": at.column, "offset": self.document().selection().head }),
            );
        }
        let here = self.caret_position();
        let offset = offset_at(
            self.document().text(),
            line.unwrap_or(here.line),
            column.unwrap_or(1),
        );
        self.document_mut().apply(quill_core::Command::PlaceCaret { offset, extend: false });
        self.reveal_caret = true;
        let at = self.caret_position();
        ok(
            request,
            format!("The caret is at line {} column {}", at.line, at.column),
            json!({ "line": at.line, "column": at.column, "offset": offset }),
        )
    }

    fn cli_editor_select(&mut self, request: &Request) -> Outcome {
        if request.switch("all") {
            self.document_mut().apply(quill_core::Command::SelectAll);
        } else if request.switch("none") {
            let head = self.document().selection().head;
            self.document_mut().apply(quill_core::Command::PlaceCaret { offset: head, extend: false });
        } else {
            let Some(from_line) = request.whole("from-line") else {
                return no(
                    request,
                    code::USAGE,
                    "Say --all, --none, or at least --from-line and --to-line.",
                );
            };
            let text = self.document().text();
            let from = offset_at(text, from_line, request.whole("from-column").unwrap_or(1));
            let to_line = request.whole("to-line").unwrap_or(from_line);
            let to = offset_at(text, to_line, request.whole("to-column").unwrap_or(usize::MAX));
            self.document_mut().apply(quill_core::Command::PlaceCaret { offset: from, extend: false });
            self.document_mut().apply(quill_core::Command::PlaceCaret { offset: to, extend: true });
        }
        self.reveal_caret = true;
        let selection = self.document().selection();
        let chosen = self.document().selected_text();
        ok(
            request,
            format!("{} characters selected", chosen.chars().count()),
            json!({
                "start": selection.start(),
                "end": selection.end(),
                "characters": chosen.chars().count(),
                "text": chosen,
            }),
        )
    }

    fn cli_editor_history(&mut self, request: &Request, undo: bool) -> Outcome {
        let possible =
            if undo { self.document().can_undo() } else { self.document().can_redo() };
        if !possible {
            return no(
                request,
                code::NOT_APPLICABLE,
                if undo { "There is nothing to undo." } else { "There is nothing to redo." },
            );
        }
        let command =
            if undo { quill_core::Command::Undo } else { quill_core::Command::Redo };
        self.document_mut().apply(command);
        self.forget_layout();
        done(request, if undo { "Undone" } else { "Redone" })
    }

    fn cli_editor_view(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        let Some(name) = request.text("mode") else {
            return no(request, code::USAGE, "Say raw, side or preview.");
        };
        let mode = match name.trim() {
            "raw" | "source" => ViewMode::Raw,
            "side" | "side-by-side" => ViewMode::SideBySide,
            "preview" => ViewMode::Preview,
            other => {
                return no(
                    request,
                    code::USAGE,
                    format!("{other} is not a view mode. Say raw, side or preview."),
                )
            }
        };
        if mode != ViewMode::Raw && !file_kind::preview_applies(self.document().path()) {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!(
                    "{} has no preview, so only the raw view applies to it.",
                    self.files.active().name()
                ),
            );
        }
        self.run_action(Action::SetViewMode(mode), ctx);
        ok(
            request,
            format!("Showing the {} view", view_mode_name(mode)),
            json!({ "viewMode": view_mode_name(mode) }),
        )
    }

    /// What `editor preview` answers for a file that is a diagram all the way through.
    ///
    /// The scene's own numbers rather than a picture: how many things were drawn, how large it came
    /// out, and every piece of text in it — which is enough for a script, or an agent, to tell that
    /// the right diagram was drawn without being able to look at it.
    fn cli_editor_diagram(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        let source = self.document().text().to_string();
        let base = self.diagram_style();
        let theme = crate::services::mermaid_scene::theme();
        if !self.mermaid_is_enabled() {
            return no(
                request,
                code::NOT_APPLICABLE,
                "The Mermaid plugin is switched off, so this file is not drawn as a diagram."
                    .to_owned(),
            );
        }
        let kind = quill_core::mermaid::kind(&source).map(|kind| kind.name().to_owned());
        let metrics = crate::services::mermaid_scene::EguiMetrics::new(ctx, self.bold_family.clone());
        match self.mermaid_scenes.scene(&source, &base, &metrics, &theme) {
            Ok(scene) => {
                let texts: Vec<String> =
                    scene.texts().into_iter().map(str::to_owned).collect();
                ok(
                    request,
                    String::new(),
                    json!({
                        "diagram": kind,
                        "width": scene.size.width,
                        "height": scene.size.height,
                        "items": scene.items.len(),
                        "text": texts,
                    }),
                )
            }
            Err(problem) => no(
                request,
                code::NOT_APPLICABLE,
                format!("{} could not be drawn. {}", self.files.active().name(), problem.message()),
            ),
        }
    }
    /// `quill-cli editor definition` — where the word at the caret is defined.
    ///
    /// Through the same functions the menu entry goes through, so a definition found from the
    /// command line and one found by clicking are the same answer. `--open` is the jump itself, and
    /// it goes through `go_to_definition` rather than opening a file directly, so the pivot to the
    /// references on the definition and the back stack both work from a script.
    fn cli_editor_definition(&mut self, request: &Request) -> Outcome {
        if !self.definitions_apply_here() {
            return no(
                request,
                code::NOT_APPLICABLE,
                "This file's language has not said what a definition looks like, so there is none to go to.",
            );
        }
        let offset = match self.cli_offset(request) {
            Ok(offset) => offset,
            Err(problem) => return no(request, code::USAGE, problem),
        };
        let Some(name) = self.symbol_at(offset) else {
            return no(request, code::NOT_APPLICABLE, "There is no symbol at that position.");
        };
        let path = self.files.active().path().map(Path::to_path_buf);
        let candidates = self.candidates_for(&name, path.as_deref(), offset);
        let rows: Vec<Value> = candidates
            .iter()
            .map(|candidate| {
                json!({
                    "path": candidate.path.to_string_lossy(),
                    "offset": candidate.name_range.start,
                    "end": candidate.name_range.end,
                    "kind": candidate.kind.name(),
                    "confidence": match candidate.confidence {
                        quill_core::symbols::Confidence::Sure => "sure",
                        quill_core::symbols::Confidence::Likely => "likely",
                    },
                    "open": candidate.open,
                })
            })
            .collect();
        if request.switch("open") {
            self.go_to_definition(offset);
            let sentence = self
                .message
                .clone()
                .unwrap_or_else(|| format!("Went to '{name}'"));
            return ok(request, sentence, json!({ "name": name, "candidates": rows }));
        }
        let sentence = match rows.len() {
            0 => format!("No definition found for '{name}'"),
            1 => format!("'{name}' is defined once"),
            many => format!("'{name}' has {many} candidate definitions"),
        };
        ok(request, sentence, json!({ "name": name, "candidates": rows }))
    }

    /// `quill-cli editor complete` — the names the word being typed could become.
    ///
    /// It goes through the same two functions the popup does: `completion_at` works out the stem and
    /// the rows, and `--choose` opens the state exactly as `Ctrl+Space` would and then accepts it
    /// exactly as `Enter` would. So a thing done from the command line and the same thing done by
    /// hand really are the same thing, and the list a script reads is the list a person is looking
    /// at rather than a second answer worked out beside it.
    ///
    /// Listing changes nothing at all — not even which row is chosen — because a script asking what
    /// is on offer must not move a popup somebody is steering.
    fn cli_editor_complete(&mut self, request: &Request) -> Outcome {
        if !self.completion_applies_here() {
            return no(
                request,
                code::NOT_APPLICABLE,
                "No plugin claims this file, so Quill has no words to offer.",
            );
        }
        let offset = match self.cli_offset(request) {
            Ok(offset) => offset,
            Err(problem) => return no(request, code::USAGE, problem),
        };
        if let Some(name) = request.text("choose") {
            return self.cli_editor_complete_choose(request, offset, name.trim());
        }
        let (stem, word, rows) = self.completion_at(offset);
        if word.is_empty() {
            return no(request, code::NOT_APPLICABLE, "There is nothing to complete here.");
        }
        let limit = request.whole("limit").unwrap_or(rows.len());
        let shown: Vec<&quill_core::completion::Row> = rows.iter().take(limit).collect();
        let lines_of_it: Vec<String> = shown
            .iter()
            .map(|row| {
                format!(
                    "{:<32}{:<10}{:<10}{}",
                    row.name,
                    row.kind.map_or("", |kind| kind.name()),
                    row.source.name(),
                    row.detail
                )
            })
            .collect();
        let value: Vec<Value> = shown
            .iter()
            .map(|row| {
                json!({
                    "name": row.name,
                    "kind": row.kind.map(|kind| kind.name()),
                    "source": row.source.name(),
                    "detail": row.detail,
                    "matched": row.matched,
                })
            })
            .collect();
        lines(
            request,
            match rows.len() {
                0 => format!("Nothing completes '{word}'"),
                1 => format!("1 completion for '{word}'"),
                many => format!("{many} completions for '{word}', {} shown", shown.len()),
            },
            lines_of_it,
            json!({
                "stem": word,
                "offset": stem.start,
                "end": stem.end,
                "total": rows.len(),
                "rows": value,
            }),
        )
    }

    /// `--choose`: apply one of the offered rows, as pressing `Enter` on it would.
    ///
    /// The caret is moved to the point the question was asked about first, so that `--offset` and
    /// `--line` mean the same thing here as they do when the rows are being listed, and so that the
    /// edit lands where the caret is — which is the one place an accept can land.
    fn cli_editor_complete_choose(
        &mut self,
        request: &Request,
        offset: usize,
        name: &str,
    ) -> Outcome {
        if name.is_empty() {
            return no(request, code::USAGE, "Say which completion to take.");
        }
        self.document_mut().apply(quill_core::Command::PlaceCaret { offset, extend: false });
        self.complete_word();
        let Some(state) = self.completion.as_ref() else {
            return no(
                request,
                code::NOT_APPLICABLE,
                self.message.clone().unwrap_or_else(|| "There is nothing to complete here.".to_owned()),
            );
        };
        let stem = self.document().text().byte_slice(state.stem.clone());
        if !self.choose_the_completion(name) {
            let offered: Vec<String> = self
                .completion
                .as_ref()
                .map(|state| state.rows.iter().take(8).map(|row| row.name.clone()).collect())
                .unwrap_or_default();
            self.close_the_completion();
            return no(
                request,
                code::NOT_FOUND,
                format!("'{name}' is not one of the completions for '{stem}'. These are: {}.", offered.join(", ")),
            );
        }
        if !self.accept_the_completion(false) {
            return no(request, code::FAILED, format!("'{name}' could not be applied."));
        }
        ok(
            request,
            format!("Completed '{stem}' to '{name}'"),
            json!({ "stem": stem, "name": name, "caret": self.caret_offset() }),
        )
    }

    /// `quill-cli editor references` — every place a name is used.
    ///
    /// The modal is opened and waited for rather than a second search being run beside it: the
    /// modal *is* the search, so what a script reads is exactly what a person would be looking at.
    fn cli_editor_references(&mut self, request: &Request) -> Outcome {
        let name = match request.text("name").as_deref() {
            Some(name) if !name.trim().is_empty() => name.trim().to_owned(),
            _ => {
                if !self.symbols_apply_here() {
                    return no(
                        request,
                        code::NOT_APPLICABLE,
                        "No plugin claims this file, so Quill cannot tell one of its words from another.",
                    );
                }
                let offset = self.caret_offset();
                match self.symbol_at(offset) {
                    Some(name) => name,
                    None => {
                        return no(
                            request,
                            code::NOT_APPLICABLE,
                            "There is no symbol at the caret, so name what to look for.",
                        )
                    }
                }
            }
        };
        self.tree.reload();
        let waker = self.thread_waker();
        self.references =
            Some(References::open(references::Purpose::References, &name, waker));
        Outcome::Hold(Waiting::References {
            until: Instant::now() + self.cli_timeout(request),
            code_only: request.switch("code-only"),
            rename: None,
        })
    }

    /// `quill-cli editor rename` — the change set, and applying it.
    ///
    /// The scope and the roles are the modal's own default-tick rules as flags, which is what makes
    /// twenty renames across a project scriptable the way `highlight apply` already is. Without
    /// `--apply` nothing is edited and the change set is printed, because a rename is exactly the
    /// sort of thing a script should be able to look at before it leaps.
    fn cli_editor_rename(&mut self, request: &Request) -> Outcome {
        let Some(to) = request.text("new-name") else {
            return no(request, code::USAGE, "Say what to call it.");
        };
        if !self.symbols_apply_here() {
            return no(
                request,
                code::NOT_APPLICABLE,
                "No plugin claims this file, so Quill cannot tell one of its words from another.",
            );
        }
        let offset = self.caret_offset();
        let from = match request.text("name") {
            Some(name) if !name.trim().is_empty() => name.trim().to_owned(),
            _ => match self.symbol_at(offset) {
                Some(name) => name,
                None => {
                    return no(
                        request,
                        code::NOT_APPLICABLE,
                        "There is no symbol at the caret, so name what to rename.",
                    )
                }
            },
        };
        let grammar = self.grammar_for(self.files.active().path()).cloned().unwrap_or_default();
        if let Err(reason) = quill_core::symbols::check_name(to.trim(), &grammar) {
            return no(request, code::USAGE, reason);
        }
        let scope = match request.text("scope").as_deref() {
            None => None,
            Some("file") => Some(true),
            Some("project") => Some(false),
            Some(other) => {
                return no(
                    request,
                    code::USAGE,
                    format!("`{other}` is not a scope. It is `file` or `project`."),
                )
            }
        };
        let include: Vec<String> = request
            .text("include")
            .unwrap_or_default()
            .split(',')
            .map(|part| part.trim().to_lowercase())
            .filter(|part| !part.is_empty())
            .collect();
        for named in &include {
            if named != "comments" && named != "strings" {
                return no(
                    request,
                    code::USAGE,
                    format!("`{named}` is not something to include. It is `comments` or `strings`."),
                );
            }
        }
        // The same resolution the modal does, so the default scope is the same one a person sees.
        let path = self.files.active().path().map(Path::to_path_buf);
        let candidates = self.candidates_for(&from, path.as_deref(), offset);
        self.rename_kind = candidates.first().map(|candidate| candidate.kind);
        self.rename_here = path;
        self.rename_ticked_up_to = 0;
        self.tree.reload();
        let waker = self.thread_waker();
        self.references = Some(References::open(references::Purpose::Rename, &from, waker));
        if let Some(modal) = self.references.as_mut() {
            modal.new_name = to.trim().to_owned();
        }
        Outcome::Hold(Waiting::References {
            until: Instant::now() + self.cli_timeout(request),
            code_only: false,
            rename: Some(CliRename { to: to.trim().to_owned(), scope, include, apply: request.switch("apply") }),
        })
    }

    /// The position a symbol command is about: the caret, an offset, or a line and column.
    ///
    /// A line and a column go through the same `offset_at` `editor caret` and `editor select`
    /// already use, so all three mean the same thing by line 42 column 9 — including in a file whose
    /// letters are wider than one byte.
    fn cli_offset(&mut self, request: &Request) -> Result<usize, String> {
        if let Some(offset) = request.whole("offset") {
            let length = self.document().text().len_bytes();
            if offset > length {
                return Err(format!(
                    "This file is {length} bytes long, so there is no byte {offset}."
                ));
            }
            return Ok(offset);
        }
        let Some(line) = request.whole("line") else {
            return Ok(self.caret_offset());
        };
        let text = self.document().text();
        if line == 0 || line > text.len_lines() {
            return Err(format!(
                "This file has {} lines, so there is no line {line}.",
                text.len_lines()
            ));
        }
        Ok(offset_at(text, line, request.whole("column").unwrap_or(1)))
    }

    /// How long a symbol command waits for its search.
    fn cli_timeout(&self, request: &Request) -> Duration {
        request
            .whole("timeout")
            .map(|milliseconds| Duration::from_millis(milliseconds as u64))
            .unwrap_or(DEFAULT_WAIT)
    }

    /// The word at an offset in the tab that is showing.
    fn symbol_at(&mut self, offset: usize) -> Option<String> {
        let index = self.files.active_index();
        let word = self.tab_symbols(index).read.identifier_at(offset)?;
        Some(self.files.at(index).document.text().byte_slice(word))
    }

    /// What `editor references` and `editor rename` answer once the search has finished.
    fn references_reply(&mut self, request: &Request, waiting: &Waiting) -> Reply {
        let Waiting::References { code_only, rename, .. } = waiting else {
            return Reply::failed(&request.command, code::NOT_APPLICABLE, "Nothing was waiting.");
        };
        let Some(modal) = self.references.as_ref() else {
            return Reply::failed(
                &request.command,
                code::NOT_APPLICABLE,
                "The modal was shut before the search finished.",
            );
        };
        let name = modal.name.clone();
        let capped = modal.is_capped();
        let rows: Vec<Value> = modal
            .hits()
            .iter()
            .filter(|hit| !*code_only || hit.role == Role::Code)
            .map(|hit| {
                json!({
                    "path": hit.path.to_string_lossy(),
                    "line": hit.line,
                    "column": hit.range.start + 1,
                    "offset": hit.offset.start,
                    "role": hit.role.name(),
                    "text": hit.text,
                })
            })
            .collect();
        let Some(rename) = rename else {
            let sentence = match rows.len() {
                0 => format!("Nothing in this project uses '{name}'"),
                1 => format!("'{name}' is used once"),
                many => format!("'{name}' is used in {many} places"),
            };
            self.references = None;
            return Reply::done(
                &request.command,
                sentence,
                json!({ "name": name, "references": rows, "capped": capped }),
            );
        };
        self.tick_for_the_command_line(rename);
        let Some(modal) = self.references.as_ref() else {
            return Reply::failed(&request.command, code::NOT_APPLICABLE, "The modal was shut.");
        };
        let change = crate::app::symbols::RenameChange {
            from: name.clone(),
            to: rename.to.clone(),
            by_file: modal.change(),
        };
        let listed: Vec<Value> = change
            .by_file
            .iter()
            .map(|(path, ranges)| {
                json!({ "path": path.to_string_lossy(), "places": ranges.len() })
            })
            .collect();
        if !rename.apply {
            self.references = None;
            return Reply::done(
                &request.command,
                format!(
                    "{} places in {} files would be renamed from '{name}' to '{}'",
                    change.count(),
                    change.by_file.len(),
                    rename.to
                ),
                json!({ "from": name, "to": rename.to, "files": listed, "references": rows }),
            );
        }
        let report = self.apply_rename(&change);
        self.references = None;
        let sentence = report.sentence(&rename.to);
        self.message = Some(sentence.clone());
        Reply::done(
            &request.command,
            sentence,
            json!({
                "from": name,
                "to": rename.to,
                "changed": report.changed,
                "files": listed,
                "openTabs": report.open.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
                "skipped": report
                    .skipped
                    .iter()
                    .map(|(path, reason)| json!({ "path": path.to_string_lossy(), "reason": reason }))
                    .collect::<Vec<_>>(),
            }),
        )
    }

    /// Tick the rows a command line rename asks for: the same default rules, with the flags on top.
    fn tick_for_the_command_line(&mut self, rename: &CliRename) {
        let kind = self.rename_kind;
        let here = self.rename_here.clone();
        let Some(modal) = self.references.as_mut() else {
            return;
        };
        let ticks: Vec<bool> = modal
            .hits()
            .iter()
            .map(|hit| {
                let same_file = here.as_deref() == Some(hit.path.as_path());
                let by_role = match hit.role {
                    Role::Code => true,
                    Role::Comment => rename.include.iter().any(|part| part == "comments"),
                    Role::String => rename.include.iter().any(|part| part == "strings"),
                };
                if !by_role {
                    return false;
                }
                match rename.scope {
                    Some(true) => same_file,
                    Some(false) => true,
                    None => crate::app::symbols::ticked_by_default(hit.role, kind, same_file)
                        || (hit.role != Role::Code && by_role),
                }
            })
            .collect();
        modal.set_ticks(ticks);
    }


    fn cli_editor_preview(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        if !file_kind::preview_applies(self.document().path()) {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!("{} has no preview.", self.files.active().name()),
            );
        }
        // A Mermaid file's preview is a picture, not text, so what is read back is the diagram: what
        // kind it is and how large it came out, or the reason it could not be drawn. Reading a
        // picture out as words is what a caller is really asking for here.
        if file_kind::is_mermaid(self.document().path()) {
            return self.cli_editor_diagram(request, ctx);
        }
        // The preview is normally built when it is about to be drawn. Asked for from the command
        // line it may never have been drawn, so it is built here at the width the editing area has,
        // or at a sensible width if the window has not laid one out yet.
        let width = if self.editor_area.width() > 1.0 {
            self.editor_area.width()
        } else {
            ctx.content_rect().width().max(400.0)
        };
        self.refresh_preview(ctx, width);
        // What the parser found is the source of each picture; what the window read is whether it
        // could be drawn and how large. They are matched up by the paragraph both of them name.
        let sources = self
            .files
            .active()
            .cached
            .preview
            .as_ref()
            .map(|preview| preview.images.clone())
            .unwrap_or_default();
        let pictures: Vec<Value> = self
            .preview_pictures()
            .iter()
            .map(|placed| {
                let source = sources
                    .iter()
                    .find(|image| image.paragraph == placed.paragraph)
                    .map(|image| image.source.clone());
                json!({
                    "paragraph": placed.paragraph,
                    "source": source,
                    "alt": placed.alt,
                    "width": placed.size.x,
                    "height": placed.size.y,
                    "drawn": placed.texture.is_some(),
                })
            })
            .collect();
        // The diagrams are reported the same way the pictures are: what the parser found beside what
        // the window made of it, matched up by the paragraph both of them name.
        let diagrams: Vec<Value> = self
            .preview_diagrams()
            .iter()
            .map(|placed| {
                json!({
                    "paragraph": placed.paragraph,
                    "diagram": quill_core::mermaid::kind(&placed.source).map(|kind| kind.name()),
                    "width": placed.size.x,
                    "height": placed.size.y,
                    "drawn": placed.laid.is_ok(),
                    "problem": placed.laid.as_ref().err().map(|problem| problem.message()),
                })
            })
            .collect();
        ok(
            request,
            String::new(),
            json!({ "text": self.preview_text(), "pictures": pictures, "diagrams": diagrams }),
        )
    }

    // ------------------------------------------------------------------------------- the terminal

    fn cli_terminal(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "show" => {
                self.terminal.visible = true;
                if self.terminal.tabs.is_empty() {
                    self.new_terminal_tab();
                }
                self.focus = crate::app::Focus::Terminal;
                ok(request, "The terminal is showing.", self.terminal_value())
            }
            "hide" => {
                self.terminal.visible = false;
                self.focus = crate::app::Focus::Editor;
                ok(request, "The terminal is hidden.", self.terminal_value())
            }
            "toggle" => {
                let visible = !self.terminal.visible;
                let verb = if visible { "show" } else { "hide" };
                self.cli_terminal(request, verb)
            }
            "new" => {
                self.terminal.visible = true;
                self.new_terminal_tab();
                self.focus = crate::app::Focus::Terminal;
                ok(
                    request,
                    format!("Started terminal tab {}", self.terminal.tabs.active_index()),
                    self.terminal_value(),
                )
            }
            "list" => {
                let names = self.terminal.tabs.names();
                let rows: Vec<String> = names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| {
                        format!(
                            "{}{at:<3} {name}",
                            if at == self.terminal.tabs.active_index() { "*" } else { " " }
                        )
                    })
                    .collect();
                lines(request, format!("{} terminal tabs", names.len()), rows, self.terminal_value())
            }
            "select" => self.cli_terminal_select(request),
            "close" => self.cli_terminal_close(request),
            "send" => self.cli_terminal_send(request),
            "read" => self.cli_terminal_read(request),
            "height" => self.cli_terminal_height(request),
            _ => unknown(request),
        }
    }

    fn cli_terminal_select(&mut self, request: &Request) -> Outcome {
        let Some(index) = request.whole("index") else {
            return no(request, code::USAGE, "Say which terminal tab, counting from 0.");
        };
        if index >= self.terminal.tabs.count() {
            return no(
                request,
                code::NOT_FOUND,
                format!("There is no terminal tab {index}; there are {}.", self.terminal.tabs.count()),
            );
        }
        self.terminal.tabs.show(index);
        ok(request, format!("Showing terminal tab {index}"), self.terminal_value())
    }

    fn cli_terminal_close(&mut self, request: &Request) -> Outcome {
        if self.terminal.tabs.is_empty() {
            return no(request, code::NOT_APPLICABLE, "There is no terminal tab to close.");
        }
        let index = request.whole("index").unwrap_or_else(|| self.terminal.tabs.active_index());
        if index >= self.terminal.tabs.count() {
            return no(request, code::NOT_FOUND, format!("There is no terminal tab {index}."));
        }
        self.terminal.tabs.close(index);
        if self.terminal.tabs.is_empty() {
            self.terminal.visible = false;
            self.focus = crate::app::Focus::Editor;
        }
        ok(request, format!("Closed terminal tab {index}"), self.terminal_value())
    }

    fn cli_terminal_send(&mut self, request: &Request) -> Outcome {
        if self.terminal.tabs.active().is_none() {
            return no(
                request,
                code::NOT_APPLICABLE,
                "There is no terminal running. `terminal show` starts one.",
            );
        }
        let mode = self.terminal.tabs.active().map(|session| session.mode()).unwrap_or_default();
        let mut bytes: Vec<u8> = Vec::new();
        let mut said = String::new();
        if let Some(name) = request.text("key") {
            let Some(press) = key_named(&name) else {
                return no(
                    request,
                    code::USAGE,
                    format!("{name} is not a key this understands. `quill-cli commands \"terminal send\"` lists them."),
                );
            };
            match quill_terminal::keys::encode(press, mode) {
                Some(encoded) => bytes.extend(encoded),
                None => return no(request, code::USAGE, format!("{name} sends nothing to a shell.")),
            }
            said = format!("Sent {name}");
        }
        if let Some(text) = request.text("text") {
            let text = unescape(&text);
            bytes.extend(text.as_bytes());
            if !request.switch("no-enter") {
                bytes.push(b'\r');
            }
            said = format!(
                "Sent `{text}`{}",
                if request.switch("no-enter") { " without pressing Enter" } else { "" }
            );
        }
        if bytes.is_empty() {
            return no(request, code::USAGE, "Say what to send: some text, or --key.");
        }
        if let Some(session) = self.terminal.tabs.active() {
            session.send(bytes.clone());
        }
        self.terminal.visible = true;
        ok(request, said, json!({ "bytes": bytes.len(), "tab": self.terminal.tabs.active_index() }))
    }

    fn cli_terminal_read(&mut self, request: &Request) -> Outcome {
        // Take in whatever the shell has written since the last frame, so a read straight after a
        // send is not looking at the screen as it was before the command ran.
        self.terminal.tabs.pump();
        let count = request.whole("lines");
        let Some(text) = self.terminal_text(count) else {
            return no(
                request,
                code::NOT_APPLICABLE,
                "There is no terminal running. `terminal show` starts one.",
            );
        };
        match request.text("wait-for") {
            Some(needle) if !text.contains(&needle) => Outcome::Hold(Waiting::TerminalText {
                needle,
                lines: count,
                until: waits_for(request, "timeout", DEFAULT_WAIT),
            }),
            Some(needle) => ok(
                request,
                String::new(),
                json!({ "text": text, "waitedFor": needle, "found": true }),
            ),
            None => ok(request, String::new(), json!({ "text": text })),
        }
    }

    fn cli_terminal_height(&mut self, request: &Request) -> Outcome {
        if let Some(points) = request.number("points") {
            self.panes.terminal_height = (points as f32).max(settings::TERMINAL_MIN);
            self.unsaved_settings = true;
        }
        ok(
            request,
            format!("The terminal is {} points tall", self.panes.terminal_height),
            json!({ "height": self.panes.terminal_height }),
        )
    }

    /// What the terminal tab that is showing has on its screen, with the blank lines under it
    /// trimmed away and at most `count` lines kept.
    fn terminal_text(&self, count: Option<usize>) -> Option<String> {
        let session = self.terminal.tabs.active()?;
        let whole = session.snapshot().text();
        let mut rows: Vec<&str> = whole.lines().collect();
        while rows.last().is_some_and(|row| row.trim().is_empty()) {
            rows.pop();
        }
        if let Some(count) = count {
            let from = rows.len().saturating_sub(count);
            rows = rows[from..].to_vec();
        }
        Some(rows.join("\n"))
    }

    fn terminal_value(&self) -> Value {
        json!({
            "visible": self.terminal.visible,
            "height": self.panes.terminal_height,
            "tabs": self.terminal.tabs.names(),
            "activeTab": self.terminal.tabs.active_index(),
            "count": self.terminal.tabs.count(),
            "focused": self.focus == crate::app::Focus::Terminal,
        })
    }

    // ------------------------------------------------------------------------------- the explorer

    fn cli_explorer(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "show" | "hide" | "toggle" => {
                self.explorer_visible = match verb {
                    "show" => true,
                    "hide" => false,
                    _ => !self.explorer_visible,
                };
                ok(
                    request,
                    if self.explorer_visible {
                        "The explorer is showing."
                    } else {
                        "The explorer is hidden."
                    },
                    json!({ "visible": self.explorer_visible, "width": self.panes.explorer_width }),
                )
            }
            "width" => {
                if let Some(points) = request.number("points") {
                    self.panes.explorer_width =
                        (points as f32).clamp(settings::EXPLORER_MIN, settings::EXPLORER_MAX);
                    self.unsaved_settings = true;
                }
                ok(
                    request,
                    format!("The explorer is {} points wide", self.panes.explorer_width),
                    json!({ "width": self.panes.explorer_width }),
                )
            }
            "filter" => {
                self.filter = request.text("text").unwrap_or_default();
                let matched = self.tree.matching(&self.filter).len();
                ok(
                    request,
                    if self.filter.is_empty() {
                        "The filter box is empty.".to_owned()
                    } else {
                        format!("{matched} files match {}", self.filter)
                    },
                    json!({ "filter": self.filter, "matches": matched }),
                )
            }
            "select-open-file" => {
                self.select_the_open_file();
                match self.files.active().path() {
                    Some(path) => ok(
                        request,
                        format!("The explorer is showing {}", path.display()),
                        json!({ "path": path.to_string_lossy(), "visible": self.explorer_visible }),
                    ),
                    None => no(
                        request,
                        code::NOT_APPLICABLE,
                        "The tab that is showing has never been saved, so there is no row to select.",
                    ),
                }
            }
            "expand" => self.cli_explorer_expand(request),
            "collapse" => self.cli_explorer_collapse(request),
            "tree" => self.cli_explorer_tree(request),
            "files" => self.cli_explorer_files(request),
            "reveal" => match self.cli_path_argument(request, "path") {
                Some(path) if path.exists() => {
                    crate::services::launcher::reveal(&path);
                    done(request, format!("Showing {} in the file manager", path.display()))
                }
                Some(path) => {
                    no(request, code::NOT_FOUND, format!("There is nothing at {}", path.display()))
                }
                None => no(request, code::USAGE, "Say which path to show."),
            },
            _ => unknown(request),
        }
    }

    fn cli_explorer_expand(&mut self, request: &Request) -> Outcome {
        let Some(path) = self.cli_path_argument(request, "path") else {
            return no(request, code::USAGE, "Say which folder to open.");
        };
        if !path.is_dir() {
            return no(request, code::NOT_FOUND, format!("{} is not a folder.", path.display()));
        }
        self.tree.expand(&path);
        self.explorer_visible = true;
        ok(
            request,
            format!("Opened {} in the tree", path.display()),
            json!({ "rows": self.tree.rows().len() }),
        )
    }

    fn cli_explorer_collapse(&mut self, request: &Request) -> Outcome {
        match self.cli_path_argument(request, "path") {
            Some(path) => {
                if self.tree.find(&path).is_none() {
                    return no(
                        request,
                        code::NOT_FOUND,
                        format!("{} is not in the tree.", path.display()),
                    );
                }
                if self.tree.find(&path).is_some_and(|entry| entry.expanded) {
                    self.tree.toggle(&path);
                }
                ok(
                    request,
                    format!("Shut {}", path.display()),
                    json!({ "rows": self.tree.rows().len() }),
                )
            }
            None => {
                // Deepest first, so shutting one does not take the next out of the tree before it
                // has been shut.
                let mut open = self.tree.expanded_folders();
                open.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
                let shut = open.len();
                for path in open {
                    self.tree.toggle(&path);
                }
                ok(
                    request,
                    format!("Shut {shut} folders"),
                    json!({ "closed": shut, "rows": self.tree.rows().len() }),
                )
            }
        }
    }

    fn cli_explorer_tree(&mut self, request: &Request) -> Outcome {
        let limit = request.whole("limit").unwrap_or(200);
        let rows = self.tree.rows();
        let shown: Vec<Value> = rows
            .iter()
            .take(limit)
            .map(|row| {
                json!({
                    "name": row.entry.name,
                    "path": row.entry.path.to_string_lossy(),
                    "depth": row.depth,
                    "folder": row.entry.is_directory,
                    "expanded": row.entry.expanded,
                    "openable": row.entry.openable,
                })
            })
            .collect();
        let printed: Vec<String> = rows
            .iter()
            .take(limit)
            .map(|row| {
                format!(
                    "{:indent$}{}{}",
                    "",
                    row.entry.name,
                    if row.entry.is_directory { "/" } else { "" },
                    indent = row.depth * 2
                )
            })
            .collect();
        lines(
            request,
            format!("{} rows, {} shown", rows.len(), shown.len()),
            printed,
            json!({ "rows": shown, "total": rows.len() }),
        )
    }

    fn cli_explorer_files(&mut self, request: &Request) -> Outcome {
        let limit = request.whole("limit").unwrap_or(500);
        let all = self.tree.all_files();
        let shown: Vec<String> =
            all.iter().take(limit).map(|path| path.to_string_lossy().to_string()).collect();
        lines(
            request,
            format!("{} files, {} shown", all.len(), shown.len()),
            shown.clone(),
            json!({ "files": shown, "total": all.len() }),
        )
    }

    // --------------------------------------------------------------------------------- the modals

    fn cli_modal(&mut self, request: &Request, verb: &str, ctx: &egui::Context) -> Outcome {
        match verb {
            "list" => {
                let open = self.open_modal();
                lines(
                    request,
                    match &open {
                        Some(name) => format!("{name} is open"),
                        None => "No modal is open".to_owned(),
                    },
                    MODALS.iter().map(|(name, what)| format!("{name:<16}{what}")).collect(),
                    json!({
                        "open": open,
                        "modals": MODALS.iter().map(|(name, what)| json!({ "name": name, "summary": what })).collect::<Vec<Value>>(),
                    }),
                )
            }
            "open" => self.cli_modal_open(request),
            "state" => ok(request, self.modal_sentence(), self.modal_value(ctx)),
            "type" => self.cli_modal_type(request),
            "results" => self.cli_modal_results(request),
            "choose" => self.cli_modal_choose(request),
            "accept" => self.cli_modal_accept(request, ctx),
            "cancel" => self.cli_modal_cancel(request),
            "move" | "size" | "reset" => self.cli_modal_geometry(request, verb, ctx),
            _ => unknown(request),
        }
    }

    /// Which modal is open, by the name `modal open` takes.
    fn open_modal(&self) -> Option<String> {
        if self.go_to_file.is_some() {
            return Some("go-to-file".to_owned());
        }
        if self.find_in_files.is_some() {
            return Some("find-in-files".to_owned());
        }
        if self.settings_window.open {
            return Some("settings".to_owned());
        }
        if self.about.is_some() {
            return Some("about".to_owned());
        }
        if let Some(prompt) = &self.prompt {
            return Some(prompt_name(prompt).to_owned());
        }
        if self.confirmation.is_some() {
            return Some("confirmation".to_owned());
        }
        if self.git.as_ref().is_some_and(|git| git.panel.open) {
            return Some("commit".to_owned());
        }
        if self.git.as_ref().is_some_and(|git| git.dialogs.open.is_some()) {
            return Some("git-dialog".to_owned());
        }
        None
    }

    fn modal_sentence(&self) -> String {
        match self.open_modal() {
            Some(name) => format!("{name} is open"),
            None => "No modal is open".to_owned(),
        }
    }

    fn modal_value(&self, ctx: &egui::Context) -> Value {
        let Some(name) = self.open_modal() else {
            return json!({ "open": Value::Null });
        };
        let id = modal_id(&name);
        let rect = id.and_then(|id| modal::drawn(ctx, id));
        let mut value = json!({
            "open": name,
            "rect": rect.map(|rect| json!({
                "x": rect.min.x, "y": rect.min.y, "width": rect.width(), "height": rect.height(),
            })),
        });
        let map = value.as_object_mut().expect("an object");
        if let Some(go) = &self.go_to_file {
            map.insert("query".to_owned(), json!(go.query));
            map.insert("results".to_owned(), json!(go.results().len()));
            map.insert("chosen".to_owned(), json!(go.chosen));
        }
        if let Some(find) = &self.find_in_files {
            map.insert("query".to_owned(), json!(find.query));
            map.insert("matchCase".to_owned(), json!(find.match_case));
            map.insert("results".to_owned(), json!(find.hits().len()));
            map.insert("chosen".to_owned(), json!(find.chosen));
            map.insert("searching".to_owned(), json!(find.is_searching()));
        }
        if let Some(about) = &self.about {
            map.insert("developer".to_owned(), json!(about.developer));
            map.insert("version".to_owned(), json!(about.version));
            map.insert("buildDate".to_owned(), json!(about.built));
        }
        if self.settings_window.open {
            map.insert("page".to_owned(), json!(self.settings_window.page.title()));
            map.insert("search".to_owned(), json!(self.settings_window.search));
        }
        if let Some(prompt) = &self.prompt {
            map.insert("title".to_owned(), json!(prompt.title));
            map.insert("note".to_owned(), json!(prompt.note));
            map.insert("query".to_owned(), json!(prompt.value));
            map.insert("confirm".to_owned(), json!(prompt.confirm));
        }
        if let Some(question) = &self.confirmation {
            map.insert("title".to_owned(), json!(question.title));
            map.insert("note".to_owned(), json!(question.note));
            map.insert("confirm".to_owned(), json!(question.button));
        }
        value
    }

    fn cli_modal_open(&mut self, request: &Request) -> Outcome {
        let Some(name) = request.text("name") else {
            return no(request, code::USAGE, "Say which modal to open.");
        };
        let query = request.text("query").unwrap_or_default();
        match name.as_str() {
            "go-to-file" => {
                self.close_every_modal();
                self.tree.reload();
                let mut go = GoToFile::default();
                go.query = query;
                let root = self.tree.root().to_path_buf();
                let files = self.tree.all_files().to_vec();
                go.refresh(&root, &files);
                let found = go.results().len();
                self.go_to_file = Some(go);
                ok(
                    request,
                    format!("Go to File is open with {found} results"),
                    json!({ "open": "go-to-file", "results": found }),
                )
            }
            "find-in-files" => {
                self.close_every_modal();
                self.tree.reload();
                let mut find = FindInFiles::open(self.thread_waker());
                find.query = query;
                find.match_case = request.switch("match-case");
                let files = self.tree.all_files().to_vec();
                find.pump(&files);
                self.find_in_files = Some(find);
                ok(
                    request,
                    "Find in Files is open. `modal results --wait 5000` waits for the search.",
                    json!({ "open": "find-in-files" }),
                )
            }
            "settings" => {
                self.close_every_modal();
                self.settings_window.open();
                if let Some(page) = request.text("page") {
                    let Some(chosen) = settings_page(&page) else {
                        return no(
                            request,
                            code::USAGE,
                            format!("{page} is not a Settings page. Say appearance, editor, plugins, terminal or mcp."),
                        );
                    };
                    self.settings_window.page = chosen;
                }
                ok(
                    request,
                    format!("Settings is open at {}", self.settings_window.page.title()),
                    json!({ "open": "settings", "page": self.settings_window.page.title() }),
                )
            }
            "about" => {
                self.close_every_modal();
                let about = crate::components::about_dialog::About::current();
                let answer = json!({
                    "open": "about",
                    "developer": about.developer,
                    "version": about.version,
                    "buildDate": about.built,
                });
                self.about = Some(about);
                ok(request, "About Quill is open", answer)
            }
            "new-file" | "rename" => self.cli_modal_open_prompt(request, &name, &query),
            other => no(
                request,
                code::USAGE,
                format!("There is no modal called {other}. `modal list` names them."),
            ),
        }
    }

    fn cli_modal_open_prompt(&mut self, request: &Request, name: &str, query: &str) -> Outcome {
        let path = match self.cli_path_argument(request, "path") {
            Some(path) => path,
            None if name == "rename" => match self.document().path() {
                Some(path) => path.to_path_buf(),
                None => {
                    return no(request, code::USAGE, "Say --path: which file to rename.")
                }
            },
            None => self.tree.root().to_path_buf(),
        };
        if !path.exists() {
            return no(request, code::NOT_FOUND, format!("There is nothing at {}", path.display()));
        }
        self.close_every_modal();
        let mut prompt = match name {
            "new-file" => {
                if !path.is_dir() {
                    return no(
                        request,
                        code::USAGE,
                        format!("{} is not a folder; a new file goes in one.", path.display()),
                    );
                }
                Prompt::new(
                    "New File",
                    &format!(
                        "A new, empty file in {}. Any extension: example.txt, test.json, main.rs.",
                        path.display()
                    ),
                    "example.txt",
                    "Create",
                    Purpose::NewFile(path),
                )
            }
            _ => {
                let existing = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                Prompt::new(
                    "Rename",
                    &format!("Rename {}.", path.display()),
                    &existing,
                    "Rename",
                    Purpose::Rename(path),
                )
            }
        };
        if !query.is_empty() {
            prompt.value = query.to_owned();
        }
        let title = prompt.title.clone();
        let value = prompt.value.clone();
        self.prompt = Some(prompt);
        ok(
            request,
            format!("{title} is open with {value} in its box"),
            json!({ "open": name, "query": value }),
        )
    }

    fn cli_modal_type(&mut self, request: &Request) -> Outcome {
        let text = request.text("text").unwrap_or_default();
        if let Some(go) = &mut self.go_to_file {
            go.query = text.clone();
            let root = self.tree.root().to_path_buf();
            let files = self.tree.all_files().to_vec();
            go.refresh(&root, &files);
            let found = go.results().len();
            return ok(
                request,
                format!("{found} files match {text}"),
                json!({ "query": text, "results": found }),
            );
        }
        if let Some(find) = &mut self.find_in_files {
            find.query = text.clone();
            if request.switch("match-case") {
                find.match_case = true;
            }
            let files = self.tree.all_files().to_vec();
            find.pump(&files);
            return ok(
                request,
                format!("Searching for {text}"),
                json!({ "query": text, "searching": true }),
            );
        }
        if self.settings_window.open {
            self.settings_window.search = text.clone();
            return ok(request, format!("Searching the settings for {text}"), json!({ "query": text }));
        }
        if let Some(prompt) = &mut self.prompt {
            prompt.value = text.clone();
            return ok(request, format!("Put {text} in the box"), json!({ "query": text }));
        }
        no(request, code::NOT_APPLICABLE, "No modal with a box in it is open.")
    }

    fn cli_modal_results(&mut self, request: &Request) -> Outcome {
        let limit = request.whole("limit").unwrap_or(50);
        if let Some(find) = &mut self.find_in_files {
            let files = self.tree.all_files().to_vec();
            find.pump(&files);
            if find.is_searching() && request.has("wait") {
                return Outcome::Hold(Waiting::ModalResults {
                    limit,
                    until: waits_for(request, "wait", DEFAULT_WAIT),
                });
            }
        }
        Outcome::Reply(self.modal_results_reply(request, limit))
    }

    /// What `modal results` answers, once there is something to answer with.
    fn modal_results_reply(&self, request: &Request, limit: usize) -> Reply {
        if let Some(go) = &self.go_to_file {
            let rows: Vec<Value> = go
                .results()
                .iter()
                .take(limit)
                .enumerate()
                .map(|(at, found)| {
                    json!({
                        "index": at,
                        "name": found.name,
                        "folder": found.folder,
                        "path": found.path.to_string_lossy(),
                        "score": found.score,
                    })
                })
                .collect();
            let printed: Vec<String> = go
                .results()
                .iter()
                .take(limit)
                .enumerate()
                .map(|(at, found)| format!("{at:<4}{:<32}{}", found.name, found.folder))
                .collect();
            return Reply::done(
                &request.command,
                format!("{} files match {}", go.results().len(), go.query),
                json!({ "results": rows, "total": go.results().len(), "chosen": go.chosen, "lines": printed }),
            );
        }
        if let Some(find) = &self.find_in_files {
            let rows: Vec<Value> = find
                .hits()
                .iter()
                .take(limit)
                .enumerate()
                .map(|(at, hit)| {
                    json!({
                        "index": at,
                        "path": hit.path.to_string_lossy(),
                        "line": hit.line,
                        "text": hit.text,
                    })
                })
                .collect();
            let printed: Vec<String> = find
                .hits()
                .iter()
                .take(limit)
                .enumerate()
                .map(|(at, hit)| {
                    format!("{at:<4}{}:{} {}", hit.path.display(), hit.line, hit.text.trim())
                })
                .collect();
            return Reply::done(
                &request.command,
                format!(
                    "{} matches for {}{}",
                    find.hits().len(),
                    find.query,
                    if find.is_searching() { ", still searching" } else { "" }
                ),
                json!({
                    "results": rows,
                    "total": find.hits().len(),
                    "chosen": find.chosen,
                    "searching": find.is_searching(),
                    "lines": printed,
                }),
            );
        }
        Reply::failed(
            &request.command,
            code::NOT_APPLICABLE,
            "No modal with results in it is open.",
        )
    }

    fn cli_modal_choose(&mut self, request: &Request) -> Outcome {
        let Some(index) = request.whole("index") else {
            return no(request, code::USAGE, "Say which row, counting from 0.");
        };
        if let Some(go) = &mut self.go_to_file {
            if index >= go.results().len() {
                return no(
                    request,
                    code::NOT_FOUND,
                    format!("There is no row {index}; there are {}.", go.results().len()),
                );
            }
            go.chosen = index;
            let path = go.chosen_path().map(|path| path.to_string_lossy().to_string());
            return ok(request, format!("Chose row {index}"), json!({ "chosen": index, "path": path }));
        }
        if let Some(find) = &mut self.find_in_files {
            if index >= find.hits().len() {
                return no(
                    request,
                    code::NOT_FOUND,
                    format!("There is no row {index}; there are {}.", find.hits().len()),
                );
            }
            find.chosen = index;
            return ok(request, format!("Chose row {index}"), json!({ "chosen": index }));
        }
        no(request, code::NOT_APPLICABLE, "No modal with a list in it is open.")
    }

    fn cli_modal_accept(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        if request.has("index") {
            if let Outcome::Reply(reply) = self.cli_modal_choose(request) {
                if !reply.ok {
                    return Outcome::Reply(reply);
                }
            }
        }
        if let Some(go) = self.go_to_file.take() {
            let Some(path) = go.chosen_path() else {
                self.go_to_file = Some(go);
                return no(request, code::NOT_FOUND, "Nothing is chosen, so there is nothing to open.");
            };
            self.open_path_permanently(&path);
            return ok(
                request,
                format!("Opened {}", path.display()),
                json!({ "path": path.to_string_lossy(), "tab": self.files.active_index() }),
            );
        }
        if let Some(find) = self.find_in_files.take() {
            let Some(hit) = find.chosen_hit().cloned() else {
                self.find_in_files = Some(find);
                return no(request, code::NOT_FOUND, "Nothing is chosen, so there is nothing to open.");
            };
            self.open_the_match(&hit.path, hit.offset.clone());
            let at = self.caret_position();
            return ok(
                request,
                format!("Opened {} at line {}", hit.path.display(), hit.line),
                json!({
                    "path": hit.path.to_string_lossy(),
                    "line": hit.line,
                    "caret": { "line": at.line, "column": at.column },
                }),
            );
        }
        if self.settings_window.open {
            self.settings_window.open = false;
            return done(request, "Closed Settings.");
        }
        if self.about.take().is_some() {
            // There is nothing to accept in the About box, so its one button and `modal accept` do
            // the same thing the Close button does.
            return done(request, "Closed About Quill.");
        }
        if let Some(prompt) = self.prompt.take() {
            let value = prompt.value.clone();
            self.run_prompt(prompt);
            return ok(
                request,
                self.message.clone().unwrap_or_else(|| format!("Confirmed with {value}")),
                json!({ "value": value, "message": self.message }),
            );
        }
        if let Some(question) = self.confirmation.take() {
            let label = question.request.label();
            self.send_git(question.request);
            return ok(request, format!("Confirmed: {label}"), json!({ "confirmed": label }));
        }
        if self.git.as_ref().is_some_and(|git| git.panel.open) {
            return no(
                request,
                code::NOT_APPLICABLE,
                "The commit panel needs a message and files chosen; drive it with `git action` instead.",
            );
        }
        let _ = ctx;
        no(request, code::NOT_APPLICABLE, "No modal is open.")
    }

    fn cli_modal_cancel(&mut self, request: &Request) -> Outcome {
        let Some(name) = self.open_modal() else {
            return no(request, code::NOT_APPLICABLE, "No modal is open.");
        };
        self.close_every_modal();
        ok(request, format!("Shut {name}"), json!({ "closed": name }))
    }

    /// Shut whatever is open, which is what opening another one does and what Escape does.
    ///
    /// `pub(super)` because `run_action` is in `app` rather than here, and `About Quill` shuts the
    /// others exactly as `modal open` does.
    pub(super) fn close_every_modal(&mut self) {
        self.go_to_file = None;
        self.find_in_files = None;
        self.about = None;
        self.settings_window.open = false;
        self.prompt = None;
        self.confirmation = None;
        if let Some(git) = self.git.as_mut() {
            git.panel.open = false;
            git.dialogs.open = None;
        }
    }

    fn cli_modal_geometry(&mut self, request: &Request, verb: &str, ctx: &egui::Context) -> Outcome {
        let Some(name) = self.open_modal() else {
            return no(request, code::NOT_APPLICABLE, "No modal is open.");
        };
        let Some(id) = modal_id(&name) else {
            return no(request, code::NOT_APPLICABLE, format!("{name} cannot be moved."));
        };
        let Some(rect) = modal::drawn(ctx, id) else {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!("{name} has not been drawn yet, so there is nothing to move."),
            );
        };
        if verb == "reset" {
            modal::reset_placement(ctx, id);
            return done(request, format!("Put {name} back in the middle."));
        }
        let mut placement = modal::placement(ctx, id);
        let message = if verb == "move" {
            let x = request.number("x").map(|value| value as f32).unwrap_or(rect.min.x);
            let y = request.number("y").map(|value| value as f32).unwrap_or(rect.min.y);
            placement.offset += egui::Pos2::new(x, y) - rect.min;
            format!("Moved {name} to {x}, {y}")
        } else {
            let width = request.number("width").map(|value| value as f32).unwrap_or(rect.width());
            let height = request.number("height").map(|value| value as f32).unwrap_or(rect.height());
            placement.grown += egui::Vec2::new(width, height) - rect.size();
            format!("Made {name} {width} by {height}")
        };
        modal::set_placement(ctx, id, placement);
        ctx.request_repaint();
        done(request, message)
    }

    // ------------------------------------------------------------------------------- the settings

    fn cli_settings(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "list" => {
                let rows: Vec<String> = SETTINGS
                    .iter()
                    .map(|key| format!("{:<34}{:<16}{}", key.name, self.setting_text(key.name), key.help))
                    .collect();
                lines(request, format!("{} settings", SETTINGS.len()), rows, self.settings_value())
            }
            "get" => {
                let Some(name) = request.text("key") else {
                    return no(request, code::USAGE, "Say which setting.");
                };
                match SETTINGS.iter().find(|key| key.name == name) {
                    Some(key) => ok(
                        request,
                        self.setting_text(key.name),
                        json!({ "key": key.name, "value": self.setting_text(key.name), "accepts": key.accepts }),
                    ),
                    None => no(request, code::NOT_FOUND, unknown_setting(&name)),
                }
            }
            "set" => self.cli_settings_set(request),
            "reset" => self.cli_settings_reset(request),
            "fonts" => {
                let limit = request.whole("limit").unwrap_or(100);
                let families: Vec<String> =
                    self.renderer.families().iter().take(limit).cloned().collect();
                lines(
                    request,
                    format!("{} families, {} shown", self.renderer.families().len(), families.len()),
                    families.clone(),
                    json!({ "families": families, "total": self.renderer.families().len() }),
                )
            }
            _ => unknown(request),
        }
    }

    /// `mcp status`: what this window is doing about the Model Context Protocol.
    ///
    /// The window is the only thing that knows, which is why it is the one `mcp` command that is not
    /// answered by the client. It reports what the settings say **and** what is actually happening,
    /// because those two come apart in the two cases that matter: a port another Quill is holding,
    /// and a window started with `--control off`.
    fn cli_mcp(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "status" => {
                let state = self
                    .mcp
                    .as_ref()
                    .map(|hosted| hosted.state().clone())
                    .unwrap_or(crate::services::mcp::State::Off);
                let shape = self.settings.mcp_tools;
                let tools = quill_cli::mcp::tools::tools(shape).len();
                let endpoint = state.port().map(quill_cli::mcp::endpoint);
                ok(
                    request,
                    state.message(),
                    json!({
                        "state": state.name(),
                        "enabled": self.settings.mcp_enabled,
                        "port": self.settings.mcp_port,
                        "endpoint": endpoint,
                        "tools": { "shape": shape.name(), "count": tools },
                        "controlChannel": self.control.is_some(),
                        // What an agent that launches the server itself should be told to run. It is
                        // the answer whether or not anything is listening, which is the point of it.
                        "stdio": {
                            "command": quill_cli::mcp::install::quill_cli_program().to_string_lossy(),
                            "arguments": ["mcp", "serve"],
                        },
                    }),
                )
            }
            _ => unknown(request),
        }
    }

    fn cli_settings_set(&mut self, request: &Request) -> Outcome {
        let (Some(name), Some(value)) = (request.text("key"), request.text("value")) else {
            return no(request, code::USAGE, "Say a setting and a value.");
        };
        if !SETTINGS.iter().any(|key| key.name == name) {
            return no(request, code::NOT_FOUND, unknown_setting(&name));
        }
        let before = self.setting_text(&name);
        if let Err(problem) = self.apply_setting(&name, value.trim()) {
            return no(request, code::USAGE, problem);
        }
        let after = self.setting_text(&name);
        ok(
            request,
            format!("{name} is now {after} (was {before})"),
            json!({ "key": name, "value": after, "was": before }),
        )
    }

    fn cli_settings_reset(&mut self, request: &Request) -> Outcome {
        let fresh = crate::settings::Settings {
            // The family a fresh Quill has is decided by what this machine has installed, so it is
            // the renderer's answer rather than an empty string, which would show as nothing.
            font_family: self.renderer.default_family(),
            ..crate::settings::Settings::new()
        };
        match request.text("key") {
            Some(name) => {
                if !SETTINGS.iter().any(|key| key.name == name) {
                    return no(request, code::NOT_FOUND, unknown_setting(&name));
                }
                let value = fresh_value(&name, &fresh);
                if let Err(problem) = self.apply_setting(&name, &value) {
                    return no(request, code::FAILED, problem);
                }
                ok(
                    request,
                    format!("{name} is back to {value}"),
                    json!({ "key": name, "value": value }),
                )
            }
            None => {
                self.set_settings(fresh);
                self.panes = crate::settings::Panes::new();
                self.unsaved_settings = true;
                ok(request, "Every setting is back to what a new Quill has.", self.settings_value())
            }
        }
    }

    /// One setting as text, which is how `settings get` and `settings list` show it and how the
    /// settings file spells it.
    fn setting_text(&self, name: &str) -> String {
        match name {
            "appearance.font.family" => self.settings.font_family.clone(),
            "appearance.font.size" => format!("{:.0}", self.settings.font_size),
            "appearance.background.opacity" => format!("{:.3}", self.settings.opacity),
            "terminal.font.size" => format!("{:.0}", self.settings.terminal_font_size),
            "terminal.shell" => self.settings.terminal_shell.clone(),
            "editor.line_numbers" => self.settings.line_numbers.to_string(),
            "editor.suggestions" => self.settings.suggestions.name().to_owned(),
            "mcp.enabled" => self.settings.mcp_enabled.to_string(),
            "mcp.port" => self.settings.mcp_port.to_string(),
            "mcp.tools" => self.settings.mcp_tools.name().to_owned(),
            "panes.explorer.width" => format!("{:.0}", self.panes.explorer_width),
            "panes.terminal.height" => format!("{:.0}", self.panes.terminal_height),
            "panes.preview.fraction" => format!("{:.3}", self.panes.preview_fraction),
            "panes.find.split" => format!("{:.3}", self.panes.find_split),
            _ => String::new(),
        }
    }

    /// Put a setting into effect, or say why the value will not do.
    ///
    /// Every value is brought inside its limits rather than refused, which is what the Settings
    /// window does with a slider and what `Settings::read_from` does with a hand edited file. What
    /// is refused is a value that is not a number at all, because that is a mistake rather than an
    /// extreme.
    fn apply_setting(&mut self, name: &str, value: &str) -> Result<(), String> {
        let number = || {
            value
                .parse::<f32>()
                .map_err(|_| format!("{name} wants a number, and {value} is not one."))
        };
        let flag = || match value.to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(true),
            "false" | "no" | "off" | "0" => Ok(false),
            _ => Err(format!("{name} wants true or false, and {value} is neither.")),
        };
        let mut settings = self.settings.clone();
        match name {
            "appearance.font.family" => {
                if !self.renderer.families().iter().any(|family| family == value) {
                    return Err(format!(
                        "This machine has no font called {value}. `settings fonts` lists them."
                    ));
                }
                settings.font_family = value.to_owned();
            }
            "appearance.font.size" => {
                settings.font_size =
                    number()?.clamp(settings::MIN_FONT_SIZE, settings::MAX_FONT_SIZE)
            }
            "appearance.background.opacity" => {
                settings.opacity = number()?.clamp(settings::MIN_OPACITY, 1.0)
            }
            "terminal.font.size" => settings.terminal_font_size = number()?.clamp(6.0, 48.0),
            // Not checked against the machine the way a font family is: a shell may be a bare name to
            // be found on the path, an absolute path, or something installed a moment from now. When
            // it is wrong the tile says so in the shell's own words, which is `Tabs::open`'s answer
            // and is a better message than one made up here.
            "terminal.shell" => settings.terminal_shell = value.trim().to_owned(),
            "editor.line_numbers" => settings.line_numbers = flag()?,
            "editor.suggestions" => {
                settings.suggestions = crate::settings::Suggestions::parse(value).ok_or_else(|| {
                    format!("{name} wants automatic or manual, and {value} is neither.")
                })?
            }
            "mcp.enabled" => settings.mcp_enabled = flag()?,
            "mcp.port" => settings.mcp_port = crate::settings::clamp_port(number()?),
            "mcp.tools" => {
                settings.mcp_tools = quill_cli::mcp::Shape::parse(value).ok_or_else(|| {
                    format!("{name} wants grouped or every, and {value} is neither.")
                })?
            }
            "panes.explorer.width" => {
                self.panes.explorer_width =
                    number()?.clamp(settings::EXPLORER_MIN, settings::EXPLORER_MAX);
                self.unsaved_settings = true;
                return Ok(());
            }
            "panes.terminal.height" => {
                self.panes.terminal_height = number()?.max(settings::TERMINAL_MIN);
                self.unsaved_settings = true;
                return Ok(());
            }
            "panes.preview.fraction" => {
                self.panes.preview_fraction = number()?.clamp(0.15, 0.85);
                self.unsaved_settings = true;
                return Ok(());
            }
            "panes.find.split" => {
                self.panes.find_split = number()?.clamp(
                    crate::components::find_in_files::SPLIT_MIN,
                    crate::components::find_in_files::SPLIT_MAX,
                );
                self.unsaved_settings = true;
                return Ok(());
            }
            _ => return Err(unknown_setting(name)),
        }
        self.set_settings(settings);
        Ok(())
    }

    fn settings_value(&self) -> Value {
        let mut map = Map::new();
        for key in SETTINGS {
            map.insert(
                key.name.to_owned(),
                json!({ "value": self.setting_text(key.name), "accepts": key.accepts, "help": key.help }),
            );
        }
        Value::Object(map)
    }

    // --------------------------------------------------------------------------------- the plugins

    fn cli_plugins(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "list" => {
                let rows: Vec<String> = self
                    .plugins
                    .all()
                    .iter()
                    .map(|plugin| {
                        format!(
                            "{}{:<14}{:<10}{}",
                            if plugin.enabled { "*" } else { " " },
                            plugin.id,
                            plugin.version,
                            plugin.name
                        )
                    })
                    .collect();
                let value: Vec<Value> = self
                    .plugins
                    .all()
                    .iter()
                    .map(|plugin| {
                        json!({
                            "id": plugin.id,
                            "name": plugin.name,
                            "version": plugin.version,
                            "vendor": plugin.vendor,
                            "enabled": plugin.enabled,
                            "bundled": plugin.bundled,
                            "extensions": plugin.extensions,
                        })
                    })
                    .collect();
                lines(
                    request,
                    format!("{} plugins, {} switched on", self.plugins.all().len(), self.plugins.enabled_count()),
                    rows,
                    json!({ "plugins": value }),
                )
            }
            "install" => match request.text("id") {
                Some(id) if self.plugins.get(&id).is_some() => {
                    self.install_plugin(&id);
                    ok(
                        request,
                        self.message.clone().unwrap_or_else(|| format!("Installed {id}")),
                        json!({ "id": id }),
                    )
                }
                Some(id) => no(request, code::NOT_FOUND, format!("There is no plugin called {id}.")),
                None => no(request, code::USAGE, "Say which plugin."),
            },
            "enable" | "disable" => {
                let Some(id) = request.text("id") else {
                    return no(request, code::USAGE, "Say which plugin.");
                };
                if self.plugins.get(&id).is_none() {
                    return no(request, code::NOT_FOUND, format!("There is no plugin called {id}."));
                }
                let on = verb == "enable";
                // Through the window's own way in, so switching a plugin off from the command line
                // and switching it off in the Plugins page are the same thing.
                self.set_plugin_enabled(&id, on);
                ok(
                    request,
                    format!("{id} is switched {}", if on { "on" } else { "off" }),
                    json!({ "id": id, "enabled": on }),
                )
            }
            _ => unknown(request),
        }
    }

    // ------------------------------------------------------------------------------------ the git

    fn cli_git(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "status" => Outcome::Reply(self.git_status_reply(request)),
            "actions" => {
                let rows: Vec<String> =
                    GitAction::ALL.iter().map(|what| what.name().to_owned()).collect();
                lines(
                    request,
                    format!("{} entries on the Git menu", rows.len()),
                    rows.clone(),
                    json!({ "actions": rows }),
                )
            }
            "action" => self.cli_git_action(request),
            _ => unknown(request),
        }
    }

    fn cli_git_action(&mut self, request: &Request) -> Outcome {
        let Some(name) = request.text("name") else {
            return no(request, code::USAGE, "Say which entry on the Git menu.");
        };
        let path = self.cli_path_argument(request, "path");
        let Some(what) = GitAction::from_name(&name, path) else {
            return no(
                request,
                code::NOT_FOUND,
                format!("There is no Git entry called {name}. `git actions` lists them."),
            );
        };
        if self.git.is_none() && what != GitAction::Clone {
            return no(
                request,
                code::NOT_APPLICABLE,
                "This folder is not in a git repository.",
            );
        }
        self.run_git(what);
        if request.has("wait") {
            return Outcome::Hold(Waiting::Git { until: waits_for(request, "wait", DEFAULT_WAIT) });
        }
        ok(
            request,
            format!("Asked git for {name}. `git status` says what came back."),
            json!({ "asked": name }),
        )
    }

    /// What git has to say, which is also what a `git action --wait` is waiting for.
    fn git_status_reply(&self, request: &Request) -> Reply {
        let Some(git) = &self.git else {
            return Reply::failed(
                &request.command,
                code::NOT_APPLICABLE,
                "This folder is not in a git repository.",
            );
        };
        let status = &git.snapshot.status;
        let changed: Vec<Value> = status
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "path": entry.path,
                    "index": entry.index.letter().trim(),
                    "worktree": entry.worktree.letter().trim(),
                    "staged": entry.staged(),
                    "untracked": entry.untracked(),
                })
            })
            .collect();
        let printed: Vec<String> = status
            .entries
            .iter()
            .map(|entry| format!("{}{} {}", entry.index.letter(), entry.worktree.letter(), entry.path))
            .collect();
        let mut result = json!({
            "root": git.repository.root().to_string_lossy(),
            "branch": status.branch,
            "upstream": status.upstream,
            "ahead": status.ahead,
            "behind": status.behind,
            "changed": changed,
            "unfinished": git.snapshot.in_progress,
            "branches": git.snapshot.branches.iter().map(|branch| branch.name.clone()).collect::<Vec<String>>(),
            "running": git.running(),
            "message": git.message,
            "lines": printed,
        });
        if let Some(map) = result.as_object_mut() {
            map.insert("annotated".to_owned(), json!(self.files.active().blame.is_some()));
        }
        Reply::done(
            &request.command,
            format!(
                "{} \u{00B7} {} changed{}",
                status.branch.clone().unwrap_or_else(|| "detached".to_owned()),
                status.entries.len(),
                git.message.as_ref().map(|text| format!(" \u{00B7} {text}")).unwrap_or_default()
            ),
            result,
        )
    }

    fn git_value(&self) -> Value {
        match &self.git {
            Some(git) => json!({
                "repository": true,
                "root": git.repository.root().to_string_lossy(),
                "branch": git.snapshot.status.branch,
                "changed": git.snapshot.status.entries.len(),
                "unfinished": git.snapshot.in_progress,
                "running": git.running(),
                "message": git.message,
            }),
            None => json!({ "repository": false }),
        }
    }

    // ---------------------------------------------------------------------- every menu entry there is

    fn cli_action(&mut self, request: &Request, verb: &str, ctx: &egui::Context) -> Outcome {
        match verb {
            "list" => {
                let listed = self.every_menu_entry();
                let rows: Vec<String> = listed
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}{:<24}{:<18}{}",
                            if entry.enabled { " " } else { "-" },
                            entry.name,
                            entry.shortcut,
                            entry.menu
                        )
                    })
                    .collect();
                let value: Vec<Value> = listed
                    .iter()
                    .map(|entry| {
                        json!({
                            "name": entry.name,
                            "menu": entry.menu,
                            "label": entry.label,
                            "shortcut": entry.shortcut,
                            "enabled": entry.enabled,
                            "checked": entry.checked,
                        })
                    })
                    .collect();
                lines(
                    request,
                    format!("{} menu entries", listed.len()),
                    rows,
                    json!({ "actions": value }),
                )
            }
            "run" => self.cli_action_run(request, ctx),
            _ => unknown(request),
        }
    }

    fn cli_action_run(&mut self, request: &Request, ctx: &egui::Context) -> Outcome {
        let Some(name) = request.text("name") else {
            return no(request, code::USAGE, "Say which menu entry.");
        };
        if let Some(instead) = Action::instead_of_a_file_chooser(&name) {
            return no(
                request,
                code::NOT_APPLICABLE,
                format!(
                    "{name} opens the platform's file chooser, which nobody can click from a script. \
                     Use `quill-cli {instead}` instead."
                ),
            );
        }
        let path = self.cli_path_argument(request, "path");
        if Action::wants_a_path(&name) && path.is_none() {
            return no(request, code::USAGE, format!("{name} needs --path."));
        }
        let Some(action) = Action::from_name(&name, path) else {
            return no(
                request,
                code::NOT_FOUND,
                format!("There is no menu entry called {name}. `action list` names them all."),
            );
        };
        self.run_action(action, ctx);
        ok(
            request,
            self.message.clone().unwrap_or_else(|| format!("Ran {name}")),
            json!({ "ran": name, "message": self.message }),
        )
    }

    /// Every entry on every menu, as it stands right now.
    ///
    /// Built by walking the real menus rather than from a list of its own, which is the point: a
    /// menu entry added later is on the command line the day it is added.
    fn every_menu_entry(&self) -> Vec<MenuEntry> {
        use crate::app::actions::{self, Entry};
        fn walk(entries: &[Entry], menu: &str, out: &mut Vec<MenuEntry>) {
            for entry in entries {
                match entry {
                    Entry::Item { name, action, shortcut, enabled, checked, .. } => {
                        out.push(MenuEntry {
                            name: action.name(),
                            menu: menu.to_owned(),
                            label: name.clone(),
                            shortcut: shortcut.map(|keys| keys.label()).unwrap_or_default(),
                            enabled: *enabled,
                            checked: *checked,
                        });
                    }
                    Entry::Submenu { name, entries } => walk(entries, name, out),
                    Entry::Separator => {}
                }
            }
        }
        let mut out = Vec::new();
        for menu in actions::menus(&self.menu_state()) {
            walk(&menu.entries, &menu.name, &mut out);
        }
        out
    }

    // -------------------------------------------------------------------------------- the project

    fn cli_project(&mut self, request: &Request, verb: &str) -> Outcome {
        match verb {
            "open" => {
                let Some(folder) = self.cli_path_argument(request, "folder") else {
                    return no(request, code::USAGE, "Say which folder to show.");
                };
                if !folder.is_dir() {
                    return no(
                        request,
                        code::NOT_FOUND,
                        format!("{} is not a folder.", folder.display()),
                    );
                }
                self.open_folder(&folder);
                ok(
                    request,
                    format!("Showing {}", folder.display()),
                    json!({ "project": folder.to_string_lossy(), "files": self.tree.all_files().len() }),
                )
            }
            "recent" => {
                let rows: Vec<String> =
                    self.recent.iter().map(|path| path.display().to_string()).collect();
                lines(
                    request,
                    format!("{} recent projects", rows.len()),
                    rows.clone(),
                    json!({ "recent": rows }),
                )
            }
            _ => unknown(request),
        }
    }
}

/// One row of `action list`.
struct MenuEntry {
    name: String,
    menu: String,
    label: String,
    shortcut: String,
    enabled: bool,
    checked: bool,
}

/// The modals `modal open` knows, and what each one is.
const MODALS: &[(&str, &str)] = &[
    ("go-to-file", "Find a file in the project by part of its name and open it."),
    ("find-in-files", "Search every file's text, with the chosen file shown underneath."),
    ("settings", "Edit -> Settings: the font, the background, the gutter, the plugins, the terminal."),
    ("about", "Who wrote Quill, what version this is and when it was built."),
    ("new-file", "Make an empty file in a folder. Takes --path."),
    ("rename", "Rename a file or a folder. Takes --path."),
];

/// The egui id a modal is drawn under, which is what its placement is remembered against.
fn modal_id(name: &str) -> Option<&'static str> {
    Some(match name {
        "go-to-file" => "quill-go-to-file",
        "find-in-files" => "quill-find-in-files",
        "settings" => "quill-settings",
        "about" => "quill-about",
        "new-file" | "rename" | "prompt" => "quill-prompt",
        "confirmation" => "quill-confirmation",
        "commit" => "quill-commit",
        "git-dialog" => "quill-git-dialog",
        _ => return None,
    })
}

/// Which of the two prompts is open, by the name `modal open` takes.
fn prompt_name(prompt: &Prompt) -> &'static str {
    match prompt.purpose {
        Purpose::NewFile(_) => "new-file",
        Purpose::Rename(_) => "rename",
        _ => "prompt",
    }
}

fn settings_page(name: &str) -> Option<crate::settings::Page> {
    use crate::settings::Page;
    Some(match name.trim().to_lowercase().as_str() {
        "appearance" => Page::Appearance,
        "editor" => Page::Editor,
        "plugins" => Page::Plugins,
        "terminal" => Page::Terminal,
        "mcp" => Page::Mcp,
        _ => return None,
    })
}

/// One setting the command line can read and change.
struct SettingKey {
    name: &'static str,
    accepts: &'static str,
    help: &'static str,
}

/// Every setting, by the name it has in Quill's own settings file.
///
/// The same names, deliberately. Somebody who has looked in `settings.conf` already knows them, and
/// a second vocabulary for the same nine values would be a second thing to learn and a second thing
/// to keep in step.
const SETTINGS: &[SettingKey] = &[
    SettingKey {
        name: "appearance.font.family",
        accepts: "a family this machine has; `settings fonts` lists them",
        help: "The family the editor sets text in.",
    },
    SettingKey {
        name: "appearance.font.size",
        accepts: "6 to 144",
        help: "The point size the editor sets text in, in every tab.",
    },
    SettingKey {
        name: "appearance.background.opacity",
        accepts: "0.05 to 1.0",
        help: "How opaque the window is. Below 1 the desktop shows through.",
    },
    SettingKey {
        name: "terminal.font.size",
        accepts: "6 to 48",
        help: "The point size the terminal sets its grid in.",
    },
    SettingKey {
        name: "terminal.shell",
        accepts: "a program, or empty for this machine's own",
        help: "What each terminal tab runs. Empty means PowerShell on Windows and $SHELL elsewhere.",
    },
    SettingKey {
        name: "editor.line_numbers",
        accepts: "true or false",
        help: "Whether the editing area has a column of line numbers.",
    },
    SettingKey {
        name: "editor.suggestions",
        accepts: "automatic or manual",
        help: "Whether the completion popup arrives as you type. Ctrl+Space works either way.",
    },
    SettingKey {
        name: "mcp.enabled",
        accepts: "true or false",
        help: "Whether this Quill serves MCP over HTTP. An agent that launches the server itself needs neither this nor a port.",
    },
    SettingKey {
        name: "mcp.port",
        accepts: "1024 to 65535",
        help: "The port it serves on when it does.",
    },
    SettingKey {
        name: "mcp.tools",
        accepts: "grouped or every",
        help: "One tool an area, or one tool a command. `mcp tools --count` says what each costs.",
    },
    SettingKey {
        name: "panes.explorer.width",
        accepts: "150 to 620",
        help: "How wide the file explorer is.",
    },
    SettingKey {
        name: "panes.terminal.height",
        accepts: "90 upwards",
        help: "How tall the terminal tile is.",
    },
    SettingKey {
        name: "panes.preview.fraction",
        accepts: "0.15 to 0.85",
        help: "How much of the side by side view the source takes.",
    },
    SettingKey {
        name: "panes.find.split",
        accepts: "0.15 to 0.85",
        help: "How much of Find in Files the results take.",
    },
];

fn unknown_setting(name: &str) -> String {
    format!("There is no setting called {name}. `settings list` names them all.")
}

/// What a setting is in a Quill that has never been run.
fn fresh_value(name: &str, fresh: &crate::settings::Settings) -> String {
    let panes = crate::settings::Panes::new();
    match name {
        "appearance.font.family" => fresh.font_family.clone(),
        "appearance.font.size" => format!("{:.0}", fresh.font_size),
        "appearance.background.opacity" => format!("{:.3}", fresh.opacity),
        "terminal.font.size" => format!("{:.0}", fresh.terminal_font_size),
        "terminal.shell" => fresh.terminal_shell.clone(),
        "editor.line_numbers" => fresh.line_numbers.to_string(),
        "editor.suggestions" => fresh.suggestions.name().to_owned(),
        "mcp.enabled" => fresh.mcp_enabled.to_string(),
        "mcp.port" => fresh.mcp_port.to_string(),
        "mcp.tools" => fresh.mcp_tools.name().to_owned(),
        "panes.explorer.width" => format!("{:.0}", panes.explorer_width),
        "panes.terminal.height" => format!("{:.0}", panes.terminal_height),
        "panes.preview.fraction" => format!("{:.3}", panes.preview_fraction),
        "panes.find.split" => format!("{:.3}", panes.find_split),
        _ => String::new(),
    }
}

/// The key `terminal send --key` names, as a key press.
///
/// A short list on purpose: the keys somebody driving a shell actually needs. Anything else is
/// text, which `terminal send` already sends.
fn key_named(name: &str) -> Option<quill_terminal::keys::KeyPress> {
    use quill_terminal::keys::{Key, KeyPress, Modifiers};
    let name = name.trim().to_lowercase();
    if let Some(letter) = name.strip_prefix("ctrl-") {
        let character = letter.chars().next()?;
        if letter.chars().count() != 1 {
            return None;
        }
        return Some(KeyPress::new(Key::Character(character), Modifiers::control()));
    }
    Some(KeyPress::plain(match name.as_str() {
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "page-up" => Key::PageUp,
        "page-down" => Key::PageDown,
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_is_split_the_way_a_shell_splits_one() {
        assert_eq!(split_line("tab open README.md"), vec!["tab", "open", "README.md"]);
        assert_eq!(
            split_line("settings set appearance.font.family \"Courier New\""),
            vec!["settings", "set", "appearance.font.family", "Courier New"]
        );
        assert_eq!(split_line("  spaced   out  "), vec!["spaced", "out"]);
        assert!(split_line("").is_empty());
    }

    #[test]
    fn the_two_escapes_a_shell_will_not_carry_are_understood() {
        assert_eq!(unescape("one\\ntwo"), "one\ntwo");
        assert_eq!(unescape("a\\tb"), "a\tb");
        assert_eq!(unescape("back\\\\slash"), "back\\slash");
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape("\\q"), "\\q", "an escape that means nothing is left alone");
    }

    #[test]
    fn a_line_and_a_column_find_the_place_in_the_text() {
        let text = quill_core::Rope::from_str("one\ntwo\nthree\n");
        assert_eq!(offset_at(&text, 1, 1), 0);
        assert_eq!(offset_at(&text, 2, 1), 4);
        assert_eq!(offset_at(&text, 2, 3), 6);
        assert_eq!(offset_at(&text, 3, 1), 8);
    }

    #[test]
    fn a_column_past_the_end_of_the_line_lands_at_the_end_of_it() {
        let text = quill_core::Rope::from_str("one\ntwo\n");
        assert_eq!(offset_at(&text, 1, 99), 3, "the end of `one`, not the next line");
        assert_eq!(offset_at(&text, 99, 1), 8, "past the last line is the end of the text");
    }

    #[test]
    fn a_line_and_a_column_find_the_place_in_text_that_is_not_ascii() {
        let text = quill_core::Rope::from_str("héllo\nwörld\n");
        // The second character is one byte in and two bytes wide, so the third is at three.
        assert_eq!(offset_at(&text, 1, 3), 3);
        assert_eq!(offset_at(&text, 2, 1), 7);
    }
}
