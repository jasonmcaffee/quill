//! One terminal: a shell in a pseudoterminal, and the emulator that reads what it writes.
//!
//! This is the one file that knows `alacritty_terminal` exists. Everything above it works on [`Screen`],
//! so a change to that crate's interface is a change to this file and nothing else.
//!
//! How the pieces fit, and where each one runs:
//!
//! - `tty::new` opens the pseudoterminal and starts the shell.
//! - `Term` holds the grid, the scrollback, the alternate screen and the modes. It lives behind a
//!   `FairMutex` because two threads reach it.
//! - `EventLoop` runs on a thread of its own, reads the pseudoterminal, parses the escape sequences and
//!   updates `Term`. Quill's own code never touches that thread.
//! - `Notifier` writes bytes to the shell and tells it when the size changed.
//! - Anything the emulation cannot deal with on its own, such as a program asking for the window title or
//!   writing to the clipboard, arrives as an event on a channel and is dealt with in [`Session::pump`] on
//!   the window's thread.
//!
//! A session can also be built with no pseudoterminal at all, with [`Session::detached`], and fed bytes
//! directly with [`Session::feed`]. That is what makes the screen testable: a test writes
//! `ESC [ 31 m hello` and asserts that the first five cells are red, with no shell, no thread and no
//! waiting.

use std::sync::mpsc::Receiver;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape as VteCursorShape, Processor};

use crate::keys::Mode;
use crate::mouse::MouseMode;
use crate::palette::Palette;
use crate::screen::{Cursor, CursorShape, Screen, ScreenCell};

/// How many lines of output are kept above the top of the screen.
pub const SCROLLBACK: usize = 10_000;

/// How wide and tall the grid is, in cells and in points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub rows: usize,
    pub columns: usize,
    /// The width of one cell in whole points, which the shell needs so that a program drawing a picture
    /// knows how large the area is.
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Size {
    pub fn new(rows: usize, columns: usize) -> Self {
        Self { rows: rows.max(1), columns: columns.max(1), cell_width: 8, cell_height: 16 }
    }

    pub fn with_cell(mut self, width: f32, height: f32) -> Self {
        self.cell_width = width.round().max(1.0) as u16;
        self.cell_height = height.round().max(1.0) as u16;
        self
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

impl From<Size> for WindowSize {
    fn from(size: Size) -> Self {
        WindowSize {
            num_lines: size.rows as u16,
            num_cols: size.columns as u16,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
        }
    }
}

/// What to run, and where.
#[derive(Debug, Clone, Default)]
pub struct SessionSettings {
    /// The program to run. `None` uses the shell the operating system says the user has.
    pub shell: Option<String>,
    pub args: Vec<String>,
    /// The folder the program starts in, which is the project the window has open.
    pub working_directory: Option<std::path::PathBuf>,
}

/// Told when something happened that the window has to redraw for.
///
/// This is how the terminal wakes the window without knowing what the window is: the caller hands over a
/// function, and the caller's function is the one that knows about egui.
pub type Waker = Arc<dyn Fn() + Send + Sync + 'static>;

/// Passes the emulator's events to the window's thread, and wakes it.
#[derive(Clone)]
struct Proxy {
    events: std::sync::mpsc::Sender<Event>,
    waker: Waker,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        // A send that fails means the window has gone, which is not worth reporting: the session is about
        // to be dropped.
        let _ = self.events.send(event);
        (self.waker)();
    }
}

/// One terminal.
pub struct Session {
    term: Arc<FairMutex<Term<Proxy>>>,
    events: Receiver<Event>,
    /// Writes to the shell. Absent in a detached session, which has no shell.
    notifier: Option<Notifier>,
    /// Parses bytes given to [`Session::feed`]. Only a detached session has one; a session with a shell
    /// has the reader thread's parser instead.
    parser: Option<Processor>,
    size: Size,
    palette: Palette,
    /// The title the program set, which is what the tab is named after.
    title: String,
    /// What the program asked to be put on the clipboard, for the window to hand over.
    clipboard: Option<String>,
    /// False once the shell has stopped.
    running: bool,
    /// How many times the program rang the bell, which a test can check.
    bells: usize,
    /// The name shown on the tab before the program sets a title of its own.
    name: String,
    /// The name a person typed for this tab, which beats both of the two above.
    ///
    /// Empty until somebody renames the tab. A name that was asked for by hand is the one thing in
    /// the tab's name a program must not be able to take away again, so it is held apart from
    /// `title` rather than written into it: `claude` sets a title on every prompt, and a rename
    /// written into `title` would last until the next one.
    given: String,
}

impl Session {
    /// Start a shell in a pseudoterminal.
    pub fn spawn(settings: &SessionSettings, size: Size, waker: Waker) -> std::io::Result<Self> {
        // `TERM` and `COLORTERM` tell the program what it is talking to. Without them a program either
        // draws nothing clever or draws it wrongly.
        alacritty_terminal::tty::setup_env();

        let shell = settings.shell.clone().unwrap_or_else(default_shell);
        let name = program_name(&shell);
        let options = alacritty_terminal::tty::Options {
            shell: Some(alacritty_terminal::tty::Shell::new(shell, settings.args.clone())),
            // Through `paths::plain`, because a verbatim Windows path is a path `cmd.exe` will not
            // start in: it says so and starts in `C:\Windows` instead, which is a terminal that opens,
            // works, and is quietly in the wrong folder. That module says where such a path comes from.
            working_directory: settings.working_directory.as_deref().map(crate::paths::plain),
            drain_on_exit: false,
            env: Default::default(),
            #[cfg(target_os = "windows")]
            escape_args: true,
        };
        let (sender, events) = std::sync::mpsc::channel();
        let proxy = Proxy { events: sender, waker };
        let palette = Palette::new();
        let mut term = Term::new(config(), &size, proxy.clone());
        term.colors_mut(&palette);
        let term = Arc::new(FairMutex::new(term));

        // A window identifier of zero: Quill has one window in a process, so there is nothing to tell
        // apart. It reaches the shell as `WINDOWID`.
        let pty = alacritty_terminal::tty::new(&options, size.into(), 0)?;
        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();

        Ok(Self {
            term,
            events,
            notifier: Some(notifier),
            parser: None,
            size,
            palette,
            title: String::new(),
            clipboard: None,
            running: true,
            bells: 0,
            name,
            given: String::new(),
        })
    }

    /// A terminal with no shell behind it, fed by [`Session::feed`].
    ///
    /// This is what the tests and the screenshot tests use. It runs the same emulator over the same bytes,
    /// so what it draws is what a real shell writing those bytes would draw, and it is the same on every
    /// run because nothing is waited for.
    pub fn detached(size: Size) -> Self {
        let (sender, events) = std::sync::mpsc::channel();
        let waker: Waker = Arc::new(|| {});
        let proxy = Proxy { events: sender, waker };
        let palette = Palette::new();
        let mut term = Term::new(config(), &size, proxy);
        term.colors_mut(&palette);
        Self {
            term: Arc::new(FairMutex::new(term)),
            events,
            notifier: None,
            parser: Some(Processor::new()),
            size,
            palette,
            title: String::new(),
            clipboard: None,
            running: true,
            bells: 0,
            name: "detached".to_owned(),
            given: String::new(),
        }
    }

    /// Run `bytes` through the emulator as if the shell had written them. Only a detached session.
    pub fn feed(&mut self, bytes: &[u8]) {
        let Some(parser) = self.parser.as_mut() else {
            return;
        };
        let mut term = self.term.lock();
        parser.advance(&mut *term, bytes);
        drop(term);
        self.pump();
    }

    /// The name for the tab: the title the program set, or the program's own name.
    ///
    /// A title that says no more than where the program was started from is not a name, so the program's
    /// own name is used instead. `cmd.exe` sets the console title to its own full path, which put
    /// `C:\Windows\system32\cmd.exe` on a Windows tab where the same tab on macOS read `zsh`. `zsh` sets
    /// no title at all, so the fallback below did all the work there and the fault could not be seen
    /// until Quill ran on Windows.
    pub fn name(&self) -> &str {
        if !self.given.is_empty() {
            return &self.given;
        }
        if self.title.is_empty() || title_is_only_the_program(&self.title, &self.name) {
            &self.name
        } else {
            &self.title
        }
    }

    /// The name a person typed for this tab, or `None` while it is still named after its program.
    pub fn given_name(&self) -> Option<&str> {
        (!self.given.is_empty()).then_some(self.given.as_str())
    }

    /// Call this tab something else, which is what `task-1682` asks a right click to offer.
    ///
    /// An empty name puts it back to what the program calls it, so there is one way to undo a
    /// rename rather than a second command that means "forget the name I gave".
    pub fn rename(&mut self, name: &str) {
        self.given = name.trim().to_owned();
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn bells(&self) -> usize {
        self.bells
    }

    /// What the program asked to be put on the clipboard, taken out so it is only handed over once.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// Deal with everything the emulation could not deal with itself.
    ///
    /// Called once a frame, and after feeding a detached session. Some of these carry a function that
    /// formats the answer the program is waiting for, which is written straight back to it.
    pub fn pump(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Title(title) => self.title = title,
                Event::ResetTitle => self.title.clear(),
                Event::PtyWrite(text) => self.send(text.into_bytes()),
                Event::ColorRequest(index, formatter) => {
                    let colour = self.palette.indexed(index.min(255) as u8);
                    self.send(formatter(colour.into()).into_bytes());
                }
                Event::TextAreaSizeRequest(formatter) => {
                    let answer = formatter(self.size.into());
                    self.send(answer.into_bytes());
                }
                Event::ClipboardStore(_, text) => self.clipboard = Some(text),
                // Reading the clipboard on a program's say so is not answered. A program that could read
                // the clipboard could read a password out of it, and nothing Quill runs needs to.
                Event::ClipboardLoad(_, _) => {}
                Event::Bell => self.bells += 1,
                Event::ChildExit(_) | Event::Exit => self.running = false,
                Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            }
        }
    }

    /// Write bytes to the shell.
    pub fn send(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(notifier) = &self.notifier {
            notifier.notify(bytes);
        }
    }

    /// What the program has asked for, which decides what some keys send.
    pub fn mode(&self) -> Mode {
        let term = self.term.lock();
        let mode = *term.mode();
        Mode {
            application_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }

    /// True when the program is drawing a screen of its own, which is what `claude` and a text editor in a
    /// terminal do. There is no scrollback on that screen.
    pub fn on_alternate_screen(&self) -> bool {
        self.term.lock().mode().contains(TermMode::ALT_SCREEN)
    }

    /// True when the program has asked to be told about clicks.
    pub fn wants_mouse(&self) -> bool {
        self.mouse_mode().reports_clicks()
    }

    /// What the program has asked to be told about the mouse, and in which encoding.
    pub fn mouse_mode(&self) -> MouseMode {
        let term = self.term.lock();
        let mode = *term.mode();
        MouseMode {
            report_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            drag: mode.contains(TermMode::MOUSE_DRAG),
            motion: mode.contains(TermMode::MOUSE_MOTION),
            sgr: mode.contains(TermMode::SGR_MOUSE),
        }
    }

    /// True when the program is drawing its own screen and has asked for the wheel to arrive as arrow
    /// keys, which is what makes scrolling work in a program that has no scrollback of its own.
    pub fn alternate_scroll(&self) -> bool {
        let term = self.term.lock();
        let mode = *term.mode();
        mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL)
    }

    /// True when the program has asked to be told when the terminal gains or loses the keyboard.
    pub fn wants_focus_reports(&self) -> bool {
        self.term.lock().mode().contains(TermMode::FOCUS_IN_OUT)
    }

    /// Change the size, telling the emulator and the program on the far side together.
    ///
    /// Both have to be told, and they have to be told the same thing: the emulator so the grid is the right
    /// shape, and the program through `SIGWINCH` so it draws in the right places. Telling only one is the
    /// fault that leaves a full screen program drawing into the wrong half of the tile.
    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.term.lock().resize(size);
        if let Some(notifier) = self.notifier.as_mut() {
            notifier.on_resize(size.into());
        }
    }

    /// Move the view through the history. A positive delta goes back towards older output.
    pub fn scroll(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    /// Put the view back at the newest output, which typing does.
    pub fn scroll_to_bottom(&mut self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Start a selection at a cell. `line` is a row of the screen, counted from the top.
    pub fn begin_selection(&mut self, row: usize, column: usize, kind: SelectionKind) {
        let point = self.point_at(row, column);
        let side = Side::Left;
        let mut term = self.term.lock();
        let selection = match kind {
            SelectionKind::Simple => Selection::new(SelectionType::Simple, point, side),
            SelectionKind::Word => Selection::new(SelectionType::Semantic, point, side),
            SelectionKind::Line => Selection::new(SelectionType::Lines, point, side),
        };
        term.selection = Some(selection);
    }

    /// Drag a selection out to a cell.
    pub fn extend_selection(&mut self, row: usize, column: usize) {
        let point = self.point_at(row, column);
        let mut term = self.term.lock();
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, Side::Right);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.lock().selection = None;
    }

    /// What is selected, as text, ready for the clipboard.
    pub fn selected_text(&self) -> Option<String> {
        self.term.lock().selection_to_string().filter(|text| !text.is_empty())
    }

    /// A point in the grid from a row on the screen, taking the scrollback into account.
    fn point_at(&self, row: usize, column: usize) -> Point {
        let term = self.term.lock();
        let offset = term.grid().display_offset();
        let columns = term.columns();
        let line = Line(row as i32 - offset as i32);
        Point::new(line, Column(column.min(columns.saturating_sub(1))))
    }

    /// Everything the painter needs, copied out under the lock.
    ///
    /// The lock is held for this copy and not for the drawing. See the file's own documentation for why.
    pub fn snapshot(&self) -> Screen {
        let term = self.term.lock();
        let rows = term.screen_lines();
        let columns = term.columns();
        let mut screen =
            Screen::empty(rows, columns, self.palette.foreground, self.palette.background);
        screen.history = term.history_size();

        let content = term.renderable_content();
        screen.scrollback = content.display_offset;
        let colours = content.colors;
        for indexed in content.display_iter {
            let row = (indexed.point.line.0 + content.display_offset as i32) as usize;
            let column = indexed.point.column.0;
            if row >= rows || column >= columns {
                continue;
            }
            let cell = indexed.cell;
            let flags = cell.flags;
            let bold = flags.contains(Flags::BOLD) || flags.contains(Flags::DIM_BOLD);
            let dim = flags.contains(Flags::DIM);

            // Bold text in a named colour is drawn in the bright variant, which is what every terminal
            // does. Dim text is drawn in the darker one.
            let mut foreground = self.palette.resolve(cell.fg, bold && !dim, colours);
            if dim {
                foreground = foreground.dimmed();
            }
            let mut background = self.palette.resolve(cell.bg, false, colours);
            // Inverse video swaps the two, which is how a program marks a line without changing its
            // colours. Resolved here so the painter has one rule: draw the background, draw the letter.
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }

            screen.cells[row * columns + column] = ScreenCell {
                character: cell.c,
                marks: cell.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
                foreground,
                background,
                bold,
                italic: flags.contains(Flags::ITALIC) || flags.contains(Flags::BOLD_ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikethrough: flags.contains(Flags::STRIKEOUT),
                wide: flags.contains(Flags::WIDE_CHAR),
                spacer: flags.contains(Flags::WIDE_CHAR_SPACER)
                    || flags.contains(Flags::LEADING_WIDE_CHAR_SPACER),
                hidden: flags.contains(Flags::HIDDEN),
            };
        }

        // The cursor. Hidden while the view is scrolled back, because it is not on the screen being looked
        // at, and hidden when the program asked for it to be.
        let cursor = content.cursor;
        if cursor.shape != VteCursorShape::Hidden && content.display_offset == 0 {
            let row = (cursor.point.line.0 + content.display_offset as i32).max(0) as usize;
            if row < rows {
                screen.cursor = Some(Cursor {
                    row,
                    column: cursor.point.column.0.min(columns.saturating_sub(1)),
                    shape: match cursor.shape {
                        VteCursorShape::Beam => CursorShape::Beam,
                        VteCursorShape::Underline => CursorShape::Underline,
                        _ => CursorShape::Block,
                    },
                });
            }
        }

        if let Some(range) = content.selection {
            let start = range.start;
            let end = range.end;
            let first = (start.line.0 + content.display_offset as i32).max(0) as usize;
            let last = (end.line.0 + content.display_offset as i32).max(0) as usize;
            if first < rows {
                let from = first * columns + start.column.0.min(columns - 1);
                let to = (last.min(rows - 1) * columns + end.column.0.min(columns - 1) + 1)
                    .min(screen.cells.len());
                if to > from {
                    screen.selection = Some(from..to);
                }
            }
        }

        screen.title = self.name().to_owned();
        screen
    }
}

impl Drop for Session {
    /// Stop the reader thread and drop the pseudoterminal, so the shell gets a hangup rather than being
    /// left behind with nobody reading it.
    fn drop(&mut self) {
        if let Some(notifier) = &self.notifier {
            let _ = notifier.0.send(Msg::Shutdown);
        }
    }
}

/// What a drag over the grid selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// The cells the pointer went over.
    Simple,
    /// The whole word under the pointer, which a double click selects.
    Word,
    /// The whole line, which a triple click selects.
    Line,
}

fn config() -> Config {
    Config { scrolling_history: SCROLLBACK, ..Config::default() }
}

/// The shell to run when none was chosen.
///
/// Everywhere but Windows this is `SHELL`, which is the shell the person has actually chosen.
///
/// Windows has no `SHELL`, and `COMSPEC` — which is what this used to read — is not the answer to the
/// same question. It names the interpreter that runs a batch file, and on every Windows there has ever
/// been it says `cmd.exe`. So Quill opened a `cmd.exe` while the machine's own commands live in a
/// PowerShell profile, and `task-1670` reported the visible half of that: a function defined in
/// `Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1` cannot exist in `cmd`, so it is
/// `'claude-skip' is not recognized`, in a terminal that looks like every other terminal on the machine.
///
/// PowerShell it is, then, and the newer one when it is installed: `pwsh.exe` and `powershell.exe` read
/// **different** profiles — `Documents\PowerShell` and `Documents\WindowsPowerShell` — so this is not a
/// preference between two spellings of the same shell, it is which set of a person's own commands the
/// terminal comes up holding. Choosing the newest installed is what Windows Terminal does.
///
/// `COMSPEC` is still the last resort, for a Windows with no PowerShell on the path at all.
/// `terminal.shell` in the settings file beats all of it, which is how a person who wants `cmd` back
/// asks for it.
fn default_shell() -> String {
    if cfg!(target_os = "windows") {
        for shell in ["pwsh.exe", "powershell.exe"] {
            if on_the_path(shell) {
                return shell.to_owned();
            }
        }
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
    }
}

/// Whether `program` is a file in one of the folders on `PATH`.
///
/// Named rather than run: asking a program whether it exists by starting it would put a window on the
/// screen on the one system where that matters, and this is asked once for each terminal opened.
fn on_the_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|folder| folder.join(program).is_file())
}

/// Whether a title says no more than the name of the program that is running.
///
/// The last part of the title is compared, so a title that is the program's whole path counts, and so
/// does one that has something in front of it, which is what a console started with more rights has.
/// The comparison ignores case because Windows paths do.
fn title_is_only_the_program(title: &str, program: &str) -> bool {
    let last = title.rsplit(['\\', '/']).next().unwrap_or(title);
    last.eq_ignore_ascii_case(program)
}

/// The last part of a program's path, which is what a tab is named until the program sets a title.
fn program_name(shell: &str) -> String {
    std::path::Path::new(shell)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.to_owned())
}

/// Give the emulator Quill's colours, so that a program asking what colour something is gets an answer and
/// so that resetting a colour puts Quill's own back.
trait WithColors {
    fn colors_mut(&mut self, palette: &Palette);
}

impl<T: EventListener> WithColors for Term<T> {
    fn colors_mut(&mut self, palette: &Palette) {
        // `set_color` is one of the things an escape sequence can do, so it is on the handler trait rather
        // than on `Term` itself.
        use alacritty_terminal::vte::ansi::Handler as _;
        let colours = palette.as_colors();
        for index in 0..alacritty_terminal::term::color::COUNT {
            if let Some(colour) = colours[index] {
                self.set_color(index, colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sixteen rows of forty columns, which is enough for anything these tests draw.
    fn detached() -> Session {
        Session::detached(Size::new(16, 40))
    }

    #[test]
    fn text_written_to_the_terminal_appears_on_the_screen() {
        let mut session = detached();
        session.feed(b"hello, terminal");
        let screen = session.snapshot();
        assert_eq!(screen.row_text(0), "hello, terminal");
        assert_eq!(screen.rows, 16);
        assert_eq!(screen.columns, 40);
    }

    #[test]
    fn a_line_feed_and_a_carriage_return_move_to_the_next_line() {
        let mut session = detached();
        session.feed(b"first\r\nsecond");
        let screen = session.snapshot();
        assert_eq!(screen.row_text(0), "first");
        assert_eq!(screen.row_text(1), "second");
    }

    #[test]
    fn a_colour_sequence_colours_the_text_it_comes_before() {
        let mut session = detached();
        session.feed(b"\x1b[31mred\x1b[0m plain");
        let screen = session.snapshot();
        let palette = Palette::new();
        assert_eq!(screen.cell(0, 0).expect("a cell").foreground, palette.indexed(1), "red");
        assert_eq!(
            screen.cell(0, 4).expect("a cell").foreground,
            palette.foreground,
            "after the reset the text is the ordinary colour again"
        );
    }

    #[test]
    fn twenty_four_bit_colour_is_used_exactly_as_it_was_given() {
        let mut session = detached();
        session.feed(b"\x1b[38;2;10;20;30mX");
        let cell = session.snapshot().cell(0, 0).cloned().expect("a cell");
        assert_eq!(cell.foreground, crate::palette::Rgb::new(10, 20, 30));
    }

    #[test]
    fn a_background_colour_reaches_the_cell_behind_the_letter() {
        let mut session = detached();
        session.feed(b"\x1b[44mX");
        let cell = session.snapshot().cell(0, 0).cloned().expect("a cell");
        assert_eq!(cell.background, Palette::new().indexed(4), "blue behind it");
    }

    #[test]
    fn inverse_video_swaps_the_two_colours() {
        let mut session = detached();
        session.feed(b"\x1b[7mX");
        let cell = session.snapshot().cell(0, 0).cloned().expect("a cell");
        let palette = Palette::new();
        assert_eq!(cell.foreground, palette.background);
        assert_eq!(cell.background, palette.foreground);
    }

    #[test]
    fn bold_italic_underline_and_strikethrough_all_arrive() {
        let mut session = detached();
        session.feed(b"\x1b[1mB\x1b[0m\x1b[3mI\x1b[0m\x1b[4mU\x1b[0m\x1b[9mS");
        let screen = session.snapshot();
        assert!(screen.cell(0, 0).expect("B").bold);
        assert!(screen.cell(0, 1).expect("I").italic);
        assert!(screen.cell(0, 2).expect("U").underline);
        assert!(screen.cell(0, 3).expect("S").strikethrough);
    }

    #[test]
    fn bold_text_in_a_named_colour_comes_out_bright() {
        let mut session = detached();
        session.feed(b"\x1b[1;31mbold red");
        let cell = session.snapshot().cell(0, 0).cloned().expect("a cell");
        assert_eq!(cell.foreground, Palette::new().indexed(9));
    }

    #[test]
    fn a_wide_character_takes_two_columns_and_the_second_draws_nothing() {
        let mut session = detached();
        // A Japanese character, which every terminal draws two columns wide.
        session.feed("\u{3042}x".as_bytes());
        let screen = session.snapshot();
        assert_eq!(screen.cell(0, 0).expect("the character").character, '\u{3042}');
        assert!(screen.cell(0, 0).expect("the character").wide);
        assert!(screen.cell(0, 1).expect("the spacer").spacer, "the second column is a spacer");
        assert_eq!(screen.cell(0, 2).expect("what follows").character, 'x');
        assert_eq!(screen.row_text(0), "\u{3042}x", "and reading the row back skips the spacer");
    }

    #[test]
    fn a_combining_accent_stays_with_the_letter_it_is_over() {
        let mut session = detached();
        session.feed("e\u{0301}".as_bytes());
        let cell = session.snapshot().cell(0, 0).cloned().expect("a cell");
        assert_eq!(cell.character, 'e');
        assert_eq!(cell.marks, vec!['\u{0301}'], "the accent is drawn over the letter, not after it");
    }

    #[test]
    fn moving_the_cursor_puts_the_next_text_where_it_was_sent() {
        let mut session = detached();
        // Row 5, column 10, counted from one in the sequence and from zero in the grid.
        session.feed(b"\x1b[5;10Hhere");
        let screen = session.snapshot();
        assert_eq!(screen.row_text(4).trim_start(), "here");
        assert_eq!(screen.cell(4, 9).expect("a cell").character, 'h');
        let cursor = screen.cursor.expect("the cursor is showing");
        assert_eq!((cursor.row, cursor.column), (4, 13), "the cursor is after what was written");
    }

    #[test]
    fn clearing_the_screen_leaves_it_empty() {
        let mut session = detached();
        session.feed(b"something to clear\x1b[2J");
        assert_eq!(session.snapshot().text(), "");
    }

    #[test]
    fn hiding_the_cursor_takes_it_off_the_screen_and_showing_it_brings_it_back() {
        let mut session = detached();
        session.feed(b"\x1b[?25l");
        assert!(session.snapshot().cursor.is_none(), "the program hid the cursor");
        session.feed(b"\x1b[?25h");
        assert!(session.snapshot().cursor.is_some());
    }

    #[test]
    fn a_cursor_shape_the_program_asked_for_is_reported() {
        let mut session = detached();
        session.feed(b"\x1b[6 q");
        assert_eq!(session.snapshot().cursor.expect("a cursor").shape, CursorShape::Beam);
        session.feed(b"\x1b[4 q");
        assert_eq!(session.snapshot().cursor.expect("a cursor").shape, CursorShape::Underline);
        session.feed(b"\x1b[2 q");
        assert_eq!(session.snapshot().cursor.expect("a cursor").shape, CursorShape::Block);
    }

    #[test]
    fn the_alternate_screen_leaves_the_ordinary_one_as_it_was() {
        let mut session = detached();
        session.feed(b"ordinary screen");
        assert!(!session.on_alternate_screen());
        // Switching screens saves the cursor where it was rather than moving it, so a program that wants to
        // draw from the top left says so, which is what the `ESC [ H` is here.
        session.feed(b"\x1b[?1049h\x1b[H");
        assert!(session.on_alternate_screen(), "the program is drawing its own screen now");
        session.feed(b"a screen of its own");
        assert_eq!(session.snapshot().row_text(0), "a screen of its own");
        session.feed(b"\x1b[?1049l");
        assert!(!session.on_alternate_screen());
        assert_eq!(
            session.snapshot().row_text(0),
            "ordinary screen",
            "what was there before the program started is back"
        );
    }

    #[test]
    fn application_cursor_keys_and_bracketed_paste_are_reported_when_the_program_asks() {
        let mut session = detached();
        assert_eq!(session.mode(), Mode::default());
        session.feed(b"\x1b[?1h\x1b[?2004h");
        assert_eq!(session.mode(), Mode { application_cursor: true, bracketed_paste: true });
    }

    #[test]
    fn a_program_asking_about_the_mouse_is_reported() {
        let mut session = detached();
        assert!(!session.wants_mouse());
        session.feed(b"\x1b[?1000h");
        assert!(session.wants_mouse(), "the program asked to be told about clicks");
    }

    #[test]
    fn a_title_the_program_sets_becomes_the_name_of_the_tab() {
        let mut session = detached();
        assert_eq!(session.name(), "detached");
        session.feed(b"\x1b]0;claude\x07");
        assert_eq!(session.name(), "claude");
        session.feed(b"\x1b]2;a different title\x07");
        assert_eq!(session.name(), "a different title");
    }

    #[test]
    fn output_that_runs_off_the_top_can_be_scrolled_back_to() {
        let mut session = Session::detached(Size::new(4, 20));
        for line in 0..20 {
            session.feed(format!("line {line}\r\n").as_bytes());
        }
        let screen = session.snapshot();
        assert!(screen.contains("line 19"), "the newest output is showing, got {:?}", screen.text());
        assert!(screen.history > 0, "there should be history to scroll back through");

        session.scroll(10);
        let scrolled = session.snapshot();
        assert!(scrolled.contains("line 9"), "scrolling back should show older output, got {:?}", scrolled.text());
        assert!(scrolled.cursor.is_none(), "the cursor is not on the part being looked at");

        session.scroll_to_bottom();
        assert!(session.snapshot().contains("line 19"));
    }

    #[test]
    fn resizing_changes_the_grid_and_keeps_the_text() {
        let mut session = Session::detached(Size::new(10, 30));
        session.feed(b"a line of text");
        session.resize(Size::new(6, 20));
        let screen = session.snapshot();
        assert_eq!(screen.rows, 6);
        assert_eq!(screen.columns, 20);
        assert_eq!(screen.row_text(0), "a line of text", "the text is still there after the resize");
    }

    #[test]
    fn resizing_narrower_breaks_a_long_line_up_rather_than_losing_it() {
        let mut session = Session::detached(Size::new(6, 30));
        session.feed(b"0123456789012345678901234");
        assert_eq!(session.snapshot().history, 0);
        session.resize(Size::new(6, 10));

        // Twenty five characters at ten columns is three lines. The cursor is at the end of them, so the
        // view is at the bottom and the two lines in front of it are in the history, which is where a
        // terminal puts what has gone off the top.
        let screen = session.snapshot();
        assert_eq!(screen.row_text(0), "01234", "the end of the line is on the screen");
        assert_eq!(screen.history, 2, "and the rest of it went into the history");

        session.scroll(2);
        let scrolled = session.snapshot();
        assert_eq!(scrolled.row_text(0), "0123456789", "which can be scrolled back to");
        assert_eq!(scrolled.row_text(1), "0123456789");
    }

    #[test]
    fn a_selection_can_be_read_back_as_text() {
        let mut session = detached();
        session.feed(b"select these words");
        session.begin_selection(0, 0, SelectionKind::Simple);
        session.extend_selection(0, 5);
        let selected = session.selected_text().expect("something is selected");
        assert_eq!(selected, "select", "the cells dragged over come back as text");
        assert!(session.snapshot().selection.is_some(), "and the painter is told where they are");
        session.clear_selection();
        assert!(session.selected_text().is_none());
    }

    #[test]
    fn a_double_click_selects_a_word() {
        let mut session = detached();
        session.feed(b"one two three");
        session.begin_selection(0, 5, SelectionKind::Word);
        assert_eq!(session.selected_text().as_deref(), Some("two"));
    }

    #[test]
    fn the_bell_is_counted_rather_than_making_a_noise() {
        let mut session = detached();
        session.feed(b"\x07\x07");
        assert_eq!(session.bells(), 2);
    }

    #[test]
    fn a_program_asking_to_copy_hands_the_text_to_the_window() {
        let mut session = detached();
        // OSC 52, which is how a program running over a connection copies to the clipboard: base64 for
        // "hello".
        session.feed(b"\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(session.take_clipboard().as_deref(), Some("hello"));
        assert_eq!(session.take_clipboard(), None, "it is only handed over once");
    }

    /// A shell this machine certainly has, that answers `echo` and leaves on `exit`.
    ///
    /// `/bin/sh` is named rather than whatever `SHELL` says, so the test does not depend on the shell the
    /// person running it happens to use. It is a Unix path though, and there is nothing at it on Windows,
    /// where the same job is done by the program `COMSPEC` names — so on Windows the test asks for that
    /// instead. Both understand `echo` and `exit`, which is all these tests send, and neither reads a
    /// profile, so what they are waiting for is the pseudoterminal rather than somebody's own startup
    /// file. Naming it here rather than calling `default_shell` is deliberate for the same reason: these
    /// tests are about the plumbing, and what the default *is* has a test of its own.
    fn test_shell() -> String {
        if cfg!(target_os = "windows") {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned())
        } else {
            "/bin/sh".to_owned()
        }
    }

    #[test]
    fn a_shell_runs_a_command_and_its_output_appears() {
        // The one test here that starts a real shell in a real pseudoterminal. It is what proves the
        // pseudoterminal, the reader thread, the writing and the waking work together, which a detached
        // session cannot show.
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = woken.clone();
        let waker: Waker = Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let settings = SessionSettings {
            shell: Some(test_shell()),
            args: Vec::new(),
            working_directory: Some(std::env::temp_dir()),
        };
        let mut session =
            Session::spawn(&settings, Size::new(12, 60), waker).expect("start a shell");
        // A carriage return, which is what the Enter key sends and what `keys.rs` puts on the wire. A line
        // feed is enough for a Unix line discipline, but a ConPTY does not take one as the line being
        // finished, so on Windows the command was typed and never run.
        session.send(b"echo quill-terminal-works\r".to_vec());

        // A shell answers when it answers, so this waits for the output rather than assuming it is there.
        // A generous wait, because this runs on whatever machine the tests are run on and a shell answering
        // is not something a test can hurry.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            session.pump();
            if session.snapshot().contains("quill-terminal-works") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the shell did not answer in thirty seconds, the screen holds {:?}",
                session.snapshot().text()
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            woken.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "the window should have been woken to draw the output"
        );
        assert!(session.is_running());
    }

    #[test]
    fn a_shell_that_is_told_to_leave_stops_running() {
        let waker: Waker = Arc::new(|| {});
        let settings = SessionSettings {
            shell: Some(test_shell()),
            args: Vec::new(),
            working_directory: None,
        };
        let mut session = Session::spawn(&settings, Size::new(8, 40), waker).expect("start a shell");
        session.send(b"exit\r".to_vec());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while session.is_running() {
            session.pump();
            assert!(std::time::Instant::now() < deadline, "the shell did not stop in thirty seconds");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn the_default_shell_is_powershell_rather_than_the_batch_interpreter() {
        // `COMSPEC` says `cmd.exe` on every Windows there is, so reading it meant Quill's terminal never
        // held the commands in a person's PowerShell profile — `task-1670`. Either PowerShell counts:
        // which one depends on whether the newer one is installed on the machine running the test.
        let shell = default_shell().to_lowercase();
        assert!(
            shell.ends_with("powershell.exe") || shell.ends_with("pwsh.exe"),
            "the default shell on Windows should be PowerShell, and is {shell:?}"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn a_shell_starts_in_the_folder_it_was_given_even_when_the_path_is_verbatim() {
        // `task-1670`: the folder reached the terminal as `\\?\C:\jason\dev\quill`, because that is what
        // `canonicalize` gives back on Windows and it had been written into the recent projects file.
        // `cmd.exe` reads the two leading backslashes as a network share, says so, and starts in
        // `C:\Windows` instead. So this asks for the folder in exactly that form and checks the shell
        // came up in it.
        let folder = std::env::temp_dir().join("quill-verbatim-working-directory");
        std::fs::create_dir_all(&folder).expect("make the folder");
        let verbatim = std::fs::canonicalize(&folder).expect("resolve the folder");
        assert!(
            verbatim.to_string_lossy().starts_with(r"\\?\"),
            "this test is only worth anything while `canonicalize` gives back a verbatim path, and it gave {verbatim:?}"
        );

        let settings = SessionSettings {
            shell: Some(test_shell()),
            args: Vec::new(),
            working_directory: Some(verbatim),
        };
        let waker: Waker = Arc::new(|| {});
        let mut session =
            Session::spawn(&settings, Size::new(12, 100), waker).expect("start a shell");
        // The shell's own answer to where it is standing, rather than Quill's. In brackets, because
        // when `cmd` refuses the folder it prints the path it was given as part of the complaint — so
        // the path appearing somewhere on the screen proves nothing, and only the path in brackets is
        // the shell saying it started there.
        session.send(b"echo [%CD%]\r".to_vec());

        let wanted = format!("[{}]", folder.to_string_lossy()).to_lowercase();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            session.pump();
            let screen = session.snapshot().text().to_lowercase();
            assert!(
                !screen.contains("unc paths are not supported"),
                "the shell refused the folder it was given: {screen}"
            );
            if screen.contains(&wanted) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the shell did not say where it was in thirty seconds, the screen holds {screen:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}
