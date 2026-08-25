//! The Quill window.
//!
//! Holds the open document, the file explorer, the terminal, the fonts and the settings, and lays the
//! window out: a title bar Quill draws itself, the formatting toolbar, the explorer down the left, the
//! editing area filling the rest, the terminal along the bottom when it is showing, and the status bar.
//!
//! Transparency works because the background and the text are two separate paints. `clear_color` gives the
//! operating system compositor an alpha taken from the opacity setting, so the desktop shows through the
//! window. Every glyph is painted at full alpha, so the writing stays sharp at every setting. That is the
//! whole of it on macOS; on Windows the compositor has to be talked into honouring the alpha at all, which
//! is `services::windows_transparency` and is the one platform call this file makes.
//!
//! The window has no operating system title bar, because rounded corners and transparency need the
//! decorations turned off, so the bars at the top and bottom are painted here and the top one moves the
//! window when it is dragged.
//!
//! Two rules this file keeps to, and a later change should keep to as well.
//!
//! Every pane is resized by dragging its edge, through `components::splitter`. The explorer, the split
//! between the source and the preview, and the terminal all use it, and a new pane must use it too rather
//! than growing a divider of its own.
//!
//! Everything a menu or a keyboard shortcut can ask for is an `actions::Action`, and [`QuillApp::run_action`]
//! is the only place an action turns into a change. The menu bar inside the window, the macOS menu bar and a
//! test all go down that one path.

pub mod actions;
pub mod files;
pub mod git;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Rect, Vec2};
use quill_core::{layout, Command, Document, Layout};

use crate::components::context_menu;
use crate::components::editor_view;
use crate::components::explorer;
use crate::components::file_tabs::{self, TabView};
use crate::components::git_dialogs::{self, Dialog};
use crate::components::git_panel;
use crate::components::gutter::{self, Gutter};
use crate::components::prompt_dialog::{self, Prompt, Purpose};
use crate::components::settings_dialog::{self, SettingsWindow};
use crate::components::splitter;
use crate::components::status_bar;
use crate::components::terminal_panel::{self, TerminalPanel};
use crate::components::title_bar::{self, MenuPlacement};
use crate::components::toolbar;
use crate::services::file_kind;
use crate::services::file_tree::FileTree;
use crate::services::file_clipboard::FileClipboard;
use crate::services::launcher;
use crate::services::icons::Icons;
use crate::services::plugins::Plugins;
use crate::services::native_menu::NativeMenu;
use crate::services::store::Store;
use crate::services::text_renderer::TextRenderer;
use crate::settings::{self, Panes, Settings};
use crate::theme::{self, color, size};

use actions::{Action, GitAction, MenuState};
use git::GitState;
use files::OpenFiles;

/// How opaque the background is when Quill starts.
pub const DEFAULT_OPACITY: f32 = settings::DEFAULT_OPACITY;

/// A question with two answers, and what to do when it is answered.
///
/// Everything Quill asks about first is something git cannot undo, so what is held is the request
/// itself and confirming simply sends it. That is what keeps the dialog from having to know what
/// any of them mean.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirmation {
    pub title: String,
    pub note: String,
    /// The word on the button that does it.
    pub button: String,
    pub request: quill_git::worker::Request,
}

/// Which of the three ways of looking at a Markdown file is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// The Markdown source, as it is on disk. This is the only mode that can be typed into.
    #[default]
    Raw,
    /// The source on the left and the preview on the right.
    SideBySide,
    /// The preview filling the editing area.
    Preview,
}

impl ViewMode {
    pub const ALL: [ViewMode; 3] = [ViewMode::Raw, ViewMode::SideBySide, ViewMode::Preview];

    /// The name a test asks for the button by, and what assistive technology reads out.
    pub fn label(&self) -> &'static str {
        match self {
            ViewMode::Raw => "Raw Markdown",
            ViewMode::SideBySide => "Side by side",
            ViewMode::Preview => "Markdown preview",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ViewMode::Raw => "Raw Markdown: the source as it is on disk",
            ViewMode::SideBySide => "Side by side: the source on the left, the preview on the right",
            ViewMode::Preview => "Markdown preview: the rendered document",
        }
    }

    /// True when the source is shown, which is the only time there is anything to type into.
    pub fn shows_source(&self) -> bool {
        matches!(self, ViewMode::Raw | ViewMode::SideBySide)
    }

    pub fn shows_preview(&self) -> bool {
        matches!(self, ViewMode::SideBySide | ViewMode::Preview)
    }
}

/// What the keyboard is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The document. Typing edits the file.
    #[default]
    Editor,
    /// The terminal. Typing goes to the program running in it, and Tab and Escape go with it.
    Terminal,
}

/// True when one of the window's text boxes has the keyboard: the explorer's filter, the commit
/// message, the rename prompt, the plugin search or the settings search.
///
/// This is the other half of what [`Focus`] means. `Focus` says whether the editing area or the
/// terminal is the one being typed into; this says whether either of them is being typed into at
/// all. Both have to be asked, because egui does **not** take the events a `TextEdit` consumed out
/// of `input.events` — the list is the frame's input and every reader sees all of it. The editing
/// area and the terminal read it directly, so without this question they take the same key presses
/// the box has just taken, and typing a filter also types into the file behind it.
///
/// `text_edit_focused` is asked rather than `egui_wants_keyboard_input`, which is
/// `memory.focused().is_some()` and is true of **any** focusable widget. Every control Quill draws
/// with `Sense::click` is focusable, so the broader question would stop the document being typed
/// into after a button was reached with Tab. Only a box that takes text should take the keyboard
/// away, and in egui only `TextEdit` and `DragValue` ask for focus when they are clicked.
pub fn text_box_has_the_keyboard(ctx: &egui::Context) -> bool {
    ctx.text_edit_focused()
}

pub struct QuillApp {
    /// The files that are open, one to a tab, and which of them is showing.
    pub files: OpenFiles,
    pub tree: FileTree,
    pub renderer: TextRenderer,
    /// The font and the background, as chosen in `Edit -> Settings`.
    pub settings: Settings,
    /// Where the draggable dividers were left.
    pub panes: Panes,
    /// The text in the explorer's filter box.
    pub filter: String,
    /// False when the explorer has been hidden.
    pub explorer_visible: bool,
    /// The Settings modal.
    pub settings_window: SettingsWindow,
    /// The terminal along the bottom.
    pub terminal: TerminalPanel,
    /// Where this window's menus are drawn.
    pub menu_placement: MenuPlacement,
    /// The projects that have been open, newest first.
    pub recent: Vec<PathBuf>,
    /// What the keyboard is talking to.
    pub focus: Focus,
    /// Something to say in the status bar, such as what version this is.
    pub message: Option<String>,
    /// Set when the window has been asked to close, which a test can check instead of the window going.
    pub closing: bool,
    /// Where the settings are kept. Absent until [`Self::load_settings`] is called, which the released
    /// binary does and the tests do not, so a test never reads or writes the settings of the person running
    /// it.
    store: Option<Store>,
    /// Set when a setting or a pane size changed and has not been written yet.
    unsaved_settings: bool,
    /// The macOS menu bar, once it has been installed.
    native_menu: Option<NativeMenu>,
    /// The most recent layout, kept so that a frame that changed nothing does not lay out again.
    layout: Layout,
    laid_out_revision: u64,
    laid_out_width: f32,
    /// Set when the layout has to be worked out again whatever the revision says.
    ///
    /// The revision counts changes to one document, so it starts again at one for the next document that is
    /// opened. Two documents can therefore be at the same revision, and comparing revisions alone would keep
    /// the layout of the file that was open before. That is a real fault, found by looking at a screenshot
    /// where a file had been opened and the editing area was empty.
    layout_stale: bool,
    /// The rectangle the editing area last occupied, so a test can measure the document's own text without
    /// also measuring the bars round it.
    editor_area: Rect,
    /// The Markdown preview, worked out from the source and kept until the source changes.
    preview: Option<quill_core::Preview>,
    preview_layout: Layout,
    preview_revision: u64,
    preview_width: f32,
    /// Set when the theme has been applied, which has to happen once the context exists.
    themed: bool,
    /// The family the toolbar uses for its bold B. It is the real bold face once [`Self::prepare`] has
    /// installed it, and the ordinary one before that, because asking egui for a family it has not been
    /// given panics.
    bold_family: egui::FontFamily,
    /// A context to wake the window with when the terminal has something new to draw.
    context: Option<egui::Context>,
    /// What had the keyboard on the last frame, so that the terminal can be told when it gains or loses it.
    last_focus: Focus,
    /// The plugins that are installed, which is what decides how a file is coloured and what icon
    /// it has.
    pub plugins: Plugins,
    /// The revision the open file was last coloured at, so it is not coloured again for nothing.
    coloured_revision: Option<u64>,
    /// The plugins' icons, decoded once each.
    icons: Icons,
    /// The repository the project is in, when it is in one, and the thread that runs git.
    pub git: Option<GitState>,
    /// Set once the folder has been looked at, so a folder that is not in a repository is not
    /// looked at again on every frame.
    git_looked: bool,
    /// A question with two answers, and the request to send when it is answered.
    ///
    /// Everything that asks first is something git cannot undo — a rollback, a hard reset, dropping
    /// a stash — so what is held is the request, and confirming sends it.
    pub confirmation: Option<Confirmation>,
    /// What was cut or copied in the explorer, waiting to be pasted.
    pub clipboard: FileClipboard,
    /// Where the explorer's own menu is open, and what it is about.
    pub explorer_menu: Option<(Pos2, PathBuf, bool)>,
    /// The text prompt, when one is open.
    pub prompt: Option<Prompt>,
    /// Where the gutter's own menu is open, when it is. Held here rather than in egui's memory so
    /// that a test can open it: a screenshot test cannot press the right mouse button.
    pub gutter_menu: Option<Pos2>,
}

impl QuillApp {
    /// A new window showing `folder` in the explorer and an empty document.
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        let folder = folder.into();
        let renderer = TextRenderer::new();
        let mut document = Document::new();
        let mut settings = Settings::new();
        settings.font_family = renderer.default_family();
        // Start in a family this system actually has, so the first thing typed is visible.
        document.set_base_style(settings.as_style_change());
        Self {
            files: OpenFiles::new(document),
            tree: FileTree::new(&folder),
            renderer,
            settings,
            panes: Panes::new(),
            filter: String::new(),
            explorer_visible: true,
            settings_window: SettingsWindow::default(),
            terminal: TerminalPanel::new(Some(folder)),
            menu_placement: MenuPlacement::for_this_platform(),
            recent: Vec::new(),
            focus: Focus::Editor,
            message: None,
            closing: false,
            store: None,
            unsaved_settings: false,
            native_menu: None,
            layout: Layout::default(),
            laid_out_revision: 0,
            laid_out_width: 0.0,
            layout_stale: true,
            editor_area: Rect::ZERO,
            preview: None,
            preview_layout: Layout::default(),
            preview_revision: 0,
            preview_width: 0.0,
            themed: false,
            bold_family: egui::FontFamily::Proportional,
            context: None,
            last_focus: Focus::Editor,
            plugins: Plugins::load(None).0,
            coloured_revision: None,
            icons: Icons::new(),
            git: None,
            git_looked: false,
            confirmation: None,
            clipboard: FileClipboard::new(),
            explorer_menu: None,
            prompt: None,
            gutter_menu: None,
        }
    }

    /// A window whose document already holds `text`, which the screenshot tests use to set a scene.
    pub fn with_text(folder: impl Into<PathBuf>, text: &str) -> Self {
        let mut app = Self::new(folder);
        app.document_mut().apply(Command::Insert(text.to_owned()));
        app.document_mut().apply(Command::MoveDocumentStart { extend: false });
        app
    }

    /// Set the window's look up before the first frame is drawn.
    ///
    /// This has to happen before the first frame rather than during it, because `Context::set_fonts` takes
    /// effect at the start of the next frame, and asking for a font family that has not been bound yet
    /// panics inside egui.
    pub fn prepare(&mut self, ctx: &egui::Context) {
        theme::apply(ctx);
        self.themed = true;
        self.context = Some(ctx.clone());
        let family = self.renderer.default_family();
        let regular = self.renderer.face_bytes(&family, false);
        let bold = self.renderer.face_bytes(&family, true);
        let has_bold = bold.is_some();
        theme::install_fonts(ctx, &family, regular, bold);
        if has_bold {
            self.bold_family = egui::FontFamily::Name(theme::BOLD_FAMILY.into());
        }
    }

    /// Read the settings and the recent projects from disk, and remember the project that is open.
    ///
    /// The released binary calls this and the tests do not, so a test neither reads nor writes the settings
    /// of the person running it.
    pub fn load_settings(&mut self) {
        self.use_store(Store::open());
    }

    /// The same, against a named folder, which is what a test that wants to check the settings uses.
    pub fn use_store(&mut self, store: Store) {
        let (settings, panes) = settings::load(&store);
        // A settings file written before this system had the family in it, or with no family at all, falls
        // back to one this system has.
        self.settings = settings;
        if self.settings.font_family.is_empty()
            || !self.renderer.families().iter().any(|family| *family == self.settings.font_family)
        {
            self.settings.font_family = self.renderer.default_family();
        }
        self.panes = panes;
        store.remember_project(self.tree.root());
        self.recent = store.recent_projects();
        let (plugins, problems) = Plugins::load(Some(&store));
        self.plugins = plugins;
        // A plugin that will not parse is skipped and said out loud, rather than stopping Quill.
        // The same rule the settings file already keeps for a line it cannot read.
        if let Some(first) = problems.first() {
            self.message = Some(format!("A plugin could not be read \u{2014} {first}"));
        }
        // On the first run there is no settings file. One is written straight away, holding the defaults,
        // so that a person looking for it finds it and can see what its names are rather than having to
        // change a setting first to make it appear.
        let first_run = !store.settings_path().is_file();
        self.store = Some(store);
        if first_run {
            self.write_settings();
        }
        let change = self.settings.as_style_change();
        self.document_mut().set_base_style(change);
    }

    /// Build the macOS menu bar. Called by the released binary only: a test has no application to attach a
    /// menu bar to, and the bar drawn inside the window is what the tests exercise.
    pub fn install_native_menu(&mut self) {
        let menus = actions::menus(&self.menu_state());
        self.native_menu = Some(NativeMenu::install(&menus, self.context.as_ref()));
    }

    /// The document in the tab that is showing.
    ///
    /// A method rather than a field, because which document is the open one is a property of the
    /// tabs. Everything that used to reach for `app.document` now goes through here, so there is one
    /// answer to what "the open file" means.
    pub fn document(&self) -> &Document {
        &self.files.active().document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.files.active_mut().document
    }

    /// Which of the three ways of looking at the open file is showing.
    pub fn view_mode(&self) -> ViewMode {
        self.files.active().view_mode
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.files.active_mut().view_mode = mode;
    }

    /// The layout as it was last painted, which the tests assert against.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The rectangle the editing area last occupied.
    pub fn editor_area(&self) -> Rect {
        self.editor_area
    }

    /// How opaque the window background is.
    pub fn opacity(&self) -> f32 {
        self.settings.opacity
    }

    /// Where the caret is, as the status bar reports it.
    pub fn caret_position(&self) -> status_bar::Position {
        status_bar::position_of(self.document().text(), self.document().selection().head)
    }

    /// Run a command, as the toolbar or a test would.
    pub fn command(&mut self, command: Command) {
        self.document_mut().apply(command);
    }

    /// The colour of the window background at the current opacity setting.
    ///
    /// The alpha is what makes the desktop visible through the window. It is applied to the background
    /// only; text is painted separately at full alpha.
    pub fn background(&self) -> Color32 {
        theme::faded(color::EDITOR, self.settings.opacity)
    }

    /// What the menus need to know about the window.
    pub fn menu_state(&self) -> MenuState {
        MenuState {
            can_undo: self.document().can_undo(),
            can_redo: self.document().can_redo(),
            has_selection: !self.document().selection().is_empty(),
            recent: self.recent.clone(),
            view_mode: self.view_mode(),
            explorer_visible: self.explorer_visible,
            line_numbers: self.settings.line_numbers,
            terminal_visible: self.terminal.visible,
            terminal_tabs: self.terminal.tabs.count(),
            open_files: self.files.len(),
            in_repository: self.git.is_some(),
            has_file: self.document().path().is_some(),
            annotated: self.files.active().blame.is_some(),
            unfinished: self.git.as_ref().and_then(|git| git.snapshot.in_progress),
        }
    }

    /// Do what a menu, a keyboard shortcut or a test asked for.
    ///
    /// This is the only place an action turns into a change, so the two menu bars and the keyboard cannot
    /// disagree about what `Save` means.
    pub fn run_action(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::NewWindow => {
                launcher::open_window(self.tree.root());
            }
            Action::OpenFolder => {
                let start = self.tree.root().to_path_buf();
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Open Folder")
                    .set_directory(&start)
                    .pick_folder()
                {
                    self.open_folder(&folder);
                }
            }
            Action::OpenFile => {
                let start = self.tree.root().to_path_buf();
                // Every file is offered, not only Markdown and plain text, because Quill opens any file
                // holding text. One that turns out not to be text says so rather than opening as nonsense.
                if let Some(file) =
                    rfd::FileDialog::new().set_title("Open File").set_directory(&start).pick_file()
                {
                    if let Some(parent) = file.parent() {
                        if !file.starts_with(self.tree.root()) {
                            self.open_folder(parent);
                        }
                    }
                    self.open_path(&file);
                }
            }
            Action::OpenFolderInNewWindow => {
                let start = self.tree.root().to_path_buf();
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Open Folder in New Window")
                    .set_directory(&start)
                    .pick_folder()
                {
                    // The project that is open here stays open, which is the point of the entry.
                    if !launcher::open_window(&folder) {
                        self.open_folder(&folder);
                    }
                }
            }
            Action::OpenRecent(folder) => {
                // A window of its own, as IntelliJ does it, so the project that is open stays open.
                if !launcher::open_window(&folder) {
                    self.open_folder(&folder);
                }
            }
            Action::ForgetRecent => {
                self.recent.clear();
                if let Some(store) = &self.store {
                    let _ = std::fs::remove_file(store.recent_path());
                }
            }
            Action::Save => self.save(),
            Action::SaveAs => {
                let start = self.tree.root().to_path_buf();
                if let Some(target) =
                    rfd::FileDialog::new().set_title("Save As").set_directory(&start).save_file()
                {
                    if self.document_mut().save_as(&target).is_ok() {
                        self.tree.reload();
                    }
                }
            }
            Action::CloseWindow | Action::Quit => {
                self.closing = true;
                self.write_settings();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::Settings => self.settings_window.open(),
            Action::Undo => {
                self.document_mut().apply(Command::Undo);
            }
            Action::Redo => {
                self.document_mut().apply(Command::Redo);
            }
            Action::Cut => {
                if self.focus == Focus::Terminal {
                    if let Some(text) = self.terminal.tabs.active().and_then(|s| s.selected_text()) {
                        ctx.copy_text(text);
                    }
                } else if !self.document().selection().is_empty() {
                    ctx.copy_text(self.document().selected_text());
                    self.document_mut().apply(Command::DeleteBackward);
                }
            }
            Action::Copy => {
                if self.focus == Focus::Terminal {
                    if let Some(text) = self.terminal.tabs.active().and_then(|s| s.selected_text()) {
                        ctx.copy_text(text);
                    }
                } else if !self.document().selection().is_empty() {
                    ctx.copy_text(self.document().selected_text());
                }
            }
            Action::Paste => {
                // Reading the clipboard needs the operating system's own clipboard, which egui only hands
                // over as a paste event. A menu entry has no event behind it, so the clipboard is read
                // here. Typing the shortcut still goes through the event, which is why the clipboard
                // entries are marked as not coming from the keyboard.
                match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
                    Ok(text) if !text.is_empty() => {
                        if self.focus == Focus::Terminal {
                            let mode =
                                self.terminal.tabs.active().map(|s| s.mode()).unwrap_or_default();
                            let bytes = quill_terminal::keys::paste(&text, mode);
                            if let Some(session) = self.terminal.tabs.active() {
                                session.send(bytes);
                            }
                        } else {
                            let text = text.replace("\r\n", "\n").replace('\r', "\n");
                            self.document_mut().apply(Command::Insert(text));
                        }
                    }
                    Ok(_) => {}
                    Err(problem) => eprintln!("Quill could not read the clipboard: {problem}"),
                }
            }
            Action::SelectAll => {
                self.document_mut().apply(Command::SelectAll);
            }
            Action::SetViewMode(mode) => self.set_view_mode(mode),
            Action::ToggleExplorer => self.explorer_visible = !self.explorer_visible,
            Action::ToggleLineNumbers => {
                self.settings.line_numbers = !self.settings.line_numbers;
                self.unsaved_settings = true;
            }
            Action::ToggleTerminal => {
                self.terminal.visible = !self.terminal.visible;
                if self.terminal.visible {
                    self.open_terminal_tab();
                    self.focus = Focus::Terminal;
                } else {
                    self.focus = Focus::Editor;
                }
            }
            Action::CloseTab => {
                let index = self.files.active_index();
                self.close_tab(index);
            }
            Action::NextTab => {
                self.files.next();
                self.forget_layout();
            }
            Action::PreviousTab => {
                self.files.previous();
                self.forget_layout();
            }
            Action::NewTerminalTab => {
                self.terminal.visible = true;
                self.new_terminal_tab();
                self.focus = Focus::Terminal;
            }
            Action::CloseTerminalTab => {
                let index = self.terminal.tabs.active_index();
                self.terminal.tabs.close(index);
                if self.terminal.tabs.is_empty() {
                    self.terminal.visible = false;
                    self.focus = Focus::Editor;
                }
            }
            Action::NewFile(folder) => {
                self.prompt = Some(Prompt::new(
                    "New File",
                    &format!("A new, empty file in {}. Any extension: example.txt, test.json, main.rs.", folder.display()),
                    "example.txt",
                    "Create",
                    Purpose::NewFile(folder),
                ));
            }
            Action::CutPath(path) => self.clipboard.cut(path),
            Action::CopyPath(path) => self.clipboard.copy(path),
            Action::CopyPathReference(path) => ctx.copy_text(path.display().to_string()),
            Action::PasteInto(folder) => match self.clipboard.paste_into(&folder) {
                Ok(target) => {
                    self.tree.reload();
                    self.message = Some(format!("Pasted {}", target.display()));
                }
                Err(problem) => self.message = Some(format!("Quill could not paste: {problem}")),
            },
            Action::RenamePath(path) => {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.prompt = Some(Prompt::new(
                    "Rename",
                    &format!("Rename {}.", path.display()),
                    &name,
                    "Rename",
                    Purpose::Rename(path),
                ));
            }
            Action::RevealPath(path) => {
                launcher::reveal(&path);
            }
            Action::ReloadPath(path) => self.reload_from_disk(&path),
            Action::Git(what) => self.run_git(what),
            Action::About => {
                self.message = Some(format!(
                    "Quill {} \u{00B7} a text editor written in Rust",
                    env!("CARGO_PKG_VERSION")
                ));
            }
        }
    }

    /// Open a terminal tab if there is not one already, which is what showing the tile does.
    fn open_terminal_tab(&mut self) {
        if self.terminal.tabs.is_empty() {
            self.new_terminal_tab();
        }
    }

    /// Start another terminal, in the folder the explorer is showing.
    pub fn new_terminal_tab(&mut self) {
        let rows = self.terminal_rows();
        let cell = self.renderer.cell_metrics(self.settings.terminal_font_size);
        let size = quill_terminal::session::Size::new(rows, 80).with_cell(cell.width, cell.height);
        self.terminal.tabs.settings.working_directory = Some(self.tree.root().to_path_buf());
        let waker = self.waker();
        self.terminal.tabs.open(size, waker);
    }

    /// A terminal with no shell behind it, which is what the tests and the screenshot tests use so that what
    /// is drawn is the same on every run.
    pub fn new_detached_terminal_tab(&mut self, rows: usize, columns: usize) {
        let cell = self.renderer.cell_metrics(self.settings.terminal_font_size);
        let size =
            quill_terminal::session::Size::new(rows, columns).with_cell(cell.width, cell.height);
        self.terminal.visible = true;
        self.terminal.tabs.open_detached(size);
    }

    /// How many rows the terminal tile holds at its current height.
    fn terminal_rows(&self) -> usize {
        let cell = self.renderer.cell_metrics(self.settings.terminal_font_size);
        (((self.panes.terminal_height - terminal_panel::FURNITURE) / cell.height).floor() as usize)
            .max(1)
    }

    /// What the terminal calls to have the window drawn again when new output arrives.
    ///
    /// The terminal knows nothing about egui: it is given a function, and this is the function.
    fn waker(&self) -> quill_terminal::session::Waker {
        match &self.context {
            Some(context) => {
                let context = context.clone();
                Arc::new(move || context.request_repaint())
            }
            None => Arc::new(|| {}),
        }
    }

    /// Show `folder` in the explorer, and remember it as a recent project.
    pub fn open_folder(&mut self, folder: &Path) {
        self.tree = FileTree::new(folder);
        self.filter.clear();
        self.explorer_visible = true;
        self.terminal.tabs.settings.working_directory = Some(folder.to_path_buf());
        if let Some(store) = &self.store {
            store.remember_project(folder);
            self.recent = store.recent_projects();
        }
        // The second folder may be a different repository, or none at all.
        self.open_repository();
    }

    /// Open a file into the tab that a single click reuses.
    ///
    /// Any file holding text opens. A `.md` file is Markdown, which means the preview button does
    /// something with it; everything else opens as plain text, which is what
    /// `tasks/improvements.md` asks for.
    pub fn open_path(&mut self, path: &Path) {
        self.open_path_in_tab(path, false);
    }

    /// Open a file in a tab of its own, which is what a double click in the explorer does.
    pub fn open_path_permanently(&mut self, path: &Path) {
        self.open_path_in_tab(path, true);
    }

    /// The one place a file is loaded. `permanent` decides whether it takes a tab of its own or
    /// reuses the transient one; [`files::OpenFiles::open`] decides what that means.
    fn open_path_in_tab(&mut self, path: &Path, permanent: bool) {
        if let Err(refusal) = file_kind::openable(path) {
            self.message = Some(format!("{}: {}", path.display(), refusal.reason()));
            return;
        }
        // A file that is already open is shown rather than read from disk again, so switching back
        // to a tab does not throw away what has been typed into it.
        if let Some(index) = self.files.index_of(path) {
            self.show_tab(index);
            if permanent {
                self.files.make_permanent(index);
            }
            return;
        }
        match Document::open(path) {
            Ok(mut document) => {
                document.apply(Command::MoveDocumentStart { extend: false });
                self.files.open(document, permanent);
                let change = self.settings.as_style_change();
                self.document_mut().set_base_style(change);
                self.message = None;
                // A file that is not Markdown has nothing to preview, so the raw source is shown.
                if !file_kind::is_markdown(Some(path)) {
                    self.files.active_mut().view_mode = ViewMode::Raw;
                }
                // The new document counts its revisions from the beginning, so what was laid out for
                // the last one has to be thrown away rather than compared against.
                self.forget_layout();
            }
            Err(error) => {
                // Nothing is thrown away: the document that was open stays open, and the reason is said in
                // the status bar rather than only on the error output.
                self.message = Some(format!("Quill could not open {}: {error}", path.display()));
                eprintln!("Quill could not open {}: {error}", path.display());
            }
        }
    }

    /// Start working on the repository the project is in, if it is in one.
    ///
    /// Called when a window opens and again when it is pointed at another folder, because the second
    /// folder may be a different repository, or none.
    pub fn open_repository(&mut self) {
        let waker = self.git_waker();
        self.git = GitState::open(self.tree.root(), waker);
        self.git_looked = true;
        self.files.forget_git();
    }

    /// The largest file that is coloured.
    ///
    /// Colouring is one linear pass over the text and it runs whenever the text changes, so on a
    /// very large file it is a pause a person can feel while typing. Two megabytes is where it is
    /// switched off, and the status bar says so rather than leaving the colours quietly missing.
    /// It is a number to be measured rather than a law: change it, and change this comment.
    const COLOUR_LIMIT: usize = 2 * 1024 * 1024;

    /// Colour the open file by what its text is, if a plugin claims it.
    ///
    /// This is not an edit. `Document::set_syntax` pushes nothing onto the undo history and does not
    /// mark the file as changed, for the same reasons setting the font does not: what Quill saves is
    /// plain text and carries no formatting.
    fn colour_the_open_file(&mut self) {
        let revision = self.document().revision();
        if self.coloured_revision == Some(revision) {
            return;
        }
        let Some(path) = self.files.active().path().map(Path::to_path_buf) else {
            self.coloured_revision = Some(revision);
            return;
        };
        let Some(plugin) = self.plugins.for_path(&path) else {
            self.coloured_revision = Some(revision);
            return;
        };
        let base = quill_core::Color::rgb(color::TEXT.r(), color::TEXT.g(), color::TEXT.b());
        let text = self.document().text().to_string();
        if text.len() > Self::COLOUR_LIMIT {
            self.message = Some(format!(
                "{} is too large to colour, so it is shown as plain text.",
                path.display()
            ));
            self.coloured_revision = Some(revision);
            return;
        }
        let theme = plugin.theme.clone();
        let spans: Vec<(std::ops::Range<usize>, quill_core::Color)> =
            quill_core::syntax::highlight(&text, &plugin.grammar)
                .into_iter()
                .filter_map(|(range, token)| theme.colour(token).map(|colour| (range, colour)))
                .collect();
        self.document_mut().set_syntax(base, &spans);
        // `set_syntax` bumps the revision, so what is remembered is the revision *after* it, or the
        // next frame would colour it all over again for ever.
        self.coloured_revision = Some(self.document().revision());
        self.layout_stale = true;
    }

    /// Write a bundled plugin out to the settings folder and load it back from there.
    fn install_plugin(&mut self, id: &str) {
        let Some(store) = self.store.clone() else {
            self.message = Some("There is nowhere to install a plugin to.".to_owned());
            return;
        };
        match self.plugins.install(&store, id) {
            Ok(()) => {
                self.message = Some(format!(
                    "Installed {id} into {}",
                    Plugins::folder(&store, id).display()
                ));
                self.coloured_revision = None;
            }
            Err(problem) => self.message = Some(format!("{id} could not be installed: {problem}")),
        }
    }

    /// The picture the plugin that claims `path` puts in front of it, decoded and ready to draw.
    fn plugin_icon(
        &mut self,
        ctx: &egui::Context,
        path: Option<&Path>,
    ) -> Option<egui::TextureHandle> {
        let path = path?;
        let (id, bytes) = {
            let plugin = self.plugins.for_path(path)?;
            (plugin.id.clone(), plugin.icon.clone()?)
        };
        self.icons.texture(ctx, &id, &bytes)
    }

    /// Ask git what it thinks of the file that is showing, once per file.
    ///
    /// Only the change bars are asked for. Blame is not, because annotating is something a person
    /// turns on: reading the whole history of every file that is opened would be work nobody asked
    /// for, on a large file it is slow, and the column takes room the text would rather have.
    fn ask_git_about_the_open_file(&mut self) {
        if self.files.active().git_asked || self.git.is_none() {
            return;
        }
        let Some(path) = self.files.active().path().map(Path::to_path_buf) else {
            return;
        };
        self.files.active_mut().git_asked = true;
        if let Some(git) = self.git.as_mut() {
            if git.relative(&path).is_some() {
                git.send(quill_git::worker::Request::ChangedLines(path));
            }
        }
    }

    /// What the git thread calls to have the window drawn again when a command finishes.
    fn git_waker(&self) -> std::sync::Arc<dyn Fn() + Send + Sync> {
        match &self.context {
            Some(context) => {
                let context = context.clone();
                Arc::new(move || context.request_repaint())
            }
            None => Arc::new(|| {}),
        }
    }

    /// The path a git entry is about: the one the menu named, or the file that is open.
    fn git_target(&self, named: Option<PathBuf>) -> Option<PathBuf> {
        named.or_else(|| self.document().path().map(Path::to_path_buf))
    }

    /// Do what the Git menu asked for.
    ///
    /// Nothing here runs a git command: each arm either opens a dialog or sends a request to the
    /// worker thread, so the window never waits for git.
    fn run_git(&mut self, what: GitAction) {
        use quill_git::worker::Request;
        let target = match &what {
            GitAction::Add(path)
            | GitAction::ShowDiff(path)
            | GitAction::CompareWithRevision(path)
            | GitAction::ShowHistory(path)
            | GitAction::Rollback(path) => self.git_target(path.clone()),
            _ => self.document().path().map(Path::to_path_buf),
        };
        // Clone is the one entry that works with no repository, because it is how you get one.
        if what == GitAction::Clone {
            self.prompt = Some(Prompt::new(
                "Clone",
                &format!("Clone into a folder under {}, and open it in a window of its own.", self.tree.root().display()),
                "",
                "Clone",
                Purpose::Clone,
            ));
            return;
        }
        let Some(git) = self.git.as_mut() else {
            self.message = Some("This folder is not in a git repository.".to_owned());
            return;
        };
        let relative = target.as_deref().and_then(|path| git.relative(path));
        match what {
            GitAction::Commit => {
                git.panel.open();
                git.send(Request::Log { path: None, limit: git::HISTORY_LIMIT });
            }
            GitAction::Add(_) => {
                if let Some(path) = relative {
                    git.send(Request::Add(vec![path]));
                }
            }
            GitAction::ShowDiff(_) => {
                if let Some(path) = target {
                    git.send(Request::Diff { path, staged: false, revision: None });
                }
            }
            GitAction::CompareWithRevision(_) => {
                if let Some(path) = target {
                    self.prompt = Some(Prompt::new(
                        "Compare with Revision",
                        "A commit, a branch or a tag to compare this file against.",
                        "HEAD~1",
                        "Compare",
                        Purpose::CompareWithRevision(path),
                    ));
                }
            }
            GitAction::ShowHistory(path) => {
                let of = path.or(target);
                git.send(Request::Log { path: of, limit: git::HISTORY_LIMIT });
                git.dialogs.open = Some(Dialog::History);
            }
            GitAction::ShowCurrentRevision => git.send(Request::ShowCommit("HEAD".to_owned())),
            GitAction::Annotate => {
                let annotated = self.files.active().blame.is_some();
                if annotated {
                    self.files.active_mut().blame = None;
                } else if let Some(path) = target {
                    git.send(Request::Blame(path));
                }
            }
            GitAction::Rollback(_) => {
                if let Some(path) = relative {
                    self.confirmation = Some(Confirmation {
                        title: "Rollback".to_owned(),
                        note: format!(
                            "Throw away the changes to {path}. They are not in a commit and not in a stash, so this cannot be undone."
                        ),
                        button: "ROLL BACK".to_owned(),
                        request: Request::Rollback(vec![path]),
                    });
                }
            }
            GitAction::Push => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Push, &status, &remotes);
            }
            GitAction::Pull => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Pull, &status, &remotes);
            }
            GitAction::Fetch => git.send(Request::Fetch),
            GitAction::Merge => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Merge { rebase: false }, &status, &remotes);
            }
            GitAction::Rebase => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Merge { rebase: true }, &status, &remotes);
            }
            GitAction::Continue => {
                let request = match git.snapshot.in_progress {
                    Some("Rebasing") => Request::ResumeRebase(quill_git::Resume::Continue),
                    _ => Request::ResumeMerge(quill_git::Resume::Continue),
                };
                git.send(request);
            }
            GitAction::Abort => {
                let request = match git.snapshot.in_progress {
                    Some("Rebasing") => Request::ResumeRebase(quill_git::Resume::Abort),
                    _ => Request::ResumeMerge(quill_git::Resume::Abort),
                };
                git.send(request);
            }
            GitAction::Branches => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Branches, &status, &remotes);
            }
            GitAction::NewBranch => {
                self.prompt = Some(Prompt::new(
                    "New Branch",
                    "Start a branch here and move to it.",
                    "",
                    "Create",
                    Purpose::NewBranch,
                ));
            }
            GitAction::NewTag => {
                self.prompt = Some(Prompt::new(
                    "New Tag",
                    "Tag the commit that is checked out.",
                    "",
                    "Tag",
                    Purpose::NewTag,
                ));
            }
            GitAction::ResetHead => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Reset, &status, &remotes);
                git.send(Request::Log { path: None, limit: git::HISTORY_LIMIT });
            }
            GitAction::Stash => {
                self.prompt = Some(Prompt::new(
                    "Stash Changes",
                    "Put the changes away under a message, leaving the working tree clean. Untracked files go with them.",
                    "",
                    "Stash",
                    Purpose::Stash,
                ));
            }
            GitAction::Unstash => {
                git.panel.open();
                git.panel.tab = git_panel::Tab::Stashes;
            }
            GitAction::Remotes => {
                let (status, remotes) = (git.snapshot.status.clone(), git.snapshot.remotes.clone());
                git.dialogs.open(Dialog::Remotes, &status, &remotes);
            }
            GitAction::Clone => {}
            GitAction::Exclude => {
                let exclude = git.repository.root().join(".git/info/exclude");
                let path = exclude.clone();
                if !path.is_file() {
                    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
                    let _ = std::fs::write(&path, "# Paths listed here are ignored, and this file is not committed.\n");
                }
                self.open_path_permanently(&path);
            }
            GitAction::Refresh => git.send(Request::Refresh),
        }
    }

    /// Draw the commit panel, whichever git dialog is open, and the confirmation.
    ///
    /// Returns an action when one of them asked for something that goes through `run_action`, which
    /// is how a click in the commit panel and an entry on the Git menu end up in the same place.
    fn show_git_windows(&mut self, ctx: &egui::Context) -> Option<Action> {
        use quill_git::worker::Request;
        // The confirmation first: it is drawn over whatever asked the question.
        if let Some(question) = self.confirmation.clone() {
            let outcome = prompt_dialog::confirm(
                ctx,
                &prompt_dialog::Confirmation {
                    title: question.title.clone(),
                    note: question.note.clone(),
                    confirm: question.button.clone(),
                    purpose: String::new(),
                },
            );
            if outcome.confirmed {
                if let Some(git) = self.git.as_mut() {
                    git.send(question.request);
                }
                self.confirmation = None;
            } else if outcome.cancelled {
                self.confirmation = None;
            }
        }
        let git = self.git.as_mut()?;
        let mut action = None;

        // The commit panel.
        let status = git.snapshot.status.clone();
        let stashes = git.snapshot.stashes.clone();
        let repository = git.repository.name();
        let recent = git.recent_messages.clone();
        let outcome =
            git_panel::show(ctx, &mut git.panel, &status, &stashes, &repository, &recent);
        if !outcome.stage.is_empty() {
            git.send(Request::Add(outcome.stage));
        }
        if !outcome.unstage.is_empty() {
            git.send(Request::Unstage(outcome.unstage));
        }
        if let Some(path) = outcome.show {
            git.send(Request::Diff {
                path: git.repository.root().join(&path),
                staged: false,
                revision: None,
            });
        }
        if let Some(push) = outcome.commit {
            let message = git.panel.message.clone();
            let amend = git.panel.amend;
            git.panel.message.clear();
            git.panel.amend = false;
            git.panel.open = false;
            if push {
                let target = quill_git::PushTarget {
                    remote: git
                        .snapshot
                        .remotes
                        .first()
                        .map(|remote| remote.name.clone())
                        .unwrap_or_else(|| "origin".to_owned()),
                    branch: status.branch.clone().unwrap_or_default(),
                    set_upstream: status.upstream.is_none(),
                    force: false,
                    tags: false,
                };
                git.send(Request::CommitAndPush { message, amend, target });
            } else {
                git.send(Request::Commit { message, amend });
            }
        }
        if let Some((name, drop)) = outcome.unstash {
            git.send(Request::Unstash { name, drop });
        }
        if let Some(name) = outcome.drop_stash {
            self.confirmation = Some(Confirmation {
                title: "Drop Stash".to_owned(),
                note: format!("Throw {name} away. What is in it is nowhere else, so this cannot be undone."),
                button: "DROP".to_owned(),
                request: Request::DropStash(name),
            });
            return action;
        }
        if outcome.refresh {
            git.send(Request::Refresh);
        }

        // The dialogs.
        let branches = git.snapshot.branches.clone();
        let remotes = git.snapshot.remotes.clone();
        let history = git.history.clone();
        let outcome =
            git_dialogs::show(ctx, &mut git.dialogs, &status, &branches, &remotes, &history);
        if let Some(target) = outcome.push {
            git.send(Request::Push(target));
            git.dialogs.close();
        }
        if let Some((remote, branch, strategy)) = outcome.pull {
            git.send(Request::Pull { remote, branch, strategy });
            git.dialogs.close();
        }
        if let Some((branch, options)) = outcome.merge {
            git.send(Request::Merge { branch, options });
            git.dialogs.close();
        }
        if let Some(branch) = outcome.rebase {
            git.send(Request::Rebase(branch));
            git.dialogs.close();
        }
        if let Some((revision, mode)) = outcome.reset {
            git.dialogs.close();
            // Only a hard reset throws work away, so only a hard reset asks first.
            if mode == quill_git::ResetMode::Hard {
                self.confirmation = Some(Confirmation {
                    title: "Reset HEAD".to_owned(),
                    note: format!(
                        "Move the branch to {revision} and throw away everything after it, including changes that were never committed. This cannot be undone."
                    ),
                    button: "RESET".to_owned(),
                    request: Request::Reset { revision, mode },
                });
                return action;
            }
            git.send(Request::Reset { revision, mode });
        }
        if let Some(name) = outcome.switch {
            git.send(Request::Switch(name));
            git.dialogs.close();
        }
        if let Some(name) = outcome.delete_branch {
            self.confirmation = Some(Confirmation {
                title: "Delete Branch".to_owned(),
                note: format!("Delete {name}. Git refuses if it holds commits that are nowhere else."),
                button: "DELETE".to_owned(),
                request: Request::DeleteBranch { name, force: false },
            });
            self.git.as_mut()?.dialogs.close();
            return action;
        }
        if let Some(hash) = outcome.show_commit {
            git.send(Request::ShowCommit(hash));
        }
        if let Some((name, url)) = outcome.add_remote {
            git.send(Request::AddRemote { name, url });
            git.dialogs.remote_name.clear();
            git.dialogs.remote_url.clear();
        }
        if let Some(name) = outcome.remove_remote {
            git.send(Request::RemoveRemote(name));
        }
        if action.is_none() && self.confirmation.is_none() {
            action = None;
        }
        action
    }

    /// Read a path again from disk: the folder it is in, and the file itself if it is open.
    ///
    /// Unsaved changes are kept rather than thrown away. A person asking to reload has asked for
    /// what is on disk, but nothing in the entry says "and lose what I typed", and quietly losing an
    /// edit is not a thing an editor should do without asking. So a file with unsaved changes says
    /// so and is left alone.
    pub fn reload_from_disk(&mut self, path: &Path) {
        self.tree.reload();
        let Some(index) = self.files.index_of(path) else {
            self.message = Some(format!("Reloaded {}", path.display()));
            return;
        };
        if self.files.get(index).is_some_and(|file| file.document.is_modified()) {
            self.message =
                Some(format!("{} has unsaved changes, so it was not reloaded", path.display()));
            return;
        }
        match Document::open(path) {
            Ok(mut document) => {
                document.apply(Command::MoveDocumentStart { extend: false });
                let change = self.settings.as_style_change();
                document.set_base_style(change);
                // The tab that holds the file, which is not necessarily the one showing: reloading
                // a file from the explorer must not drag a different tab into view.
                if let Some(file) = self.files.get_mut(index) {
                    file.document = document;
                    file.scroll = 0.0;
                    file.forget_git();
                }
                if index == self.files.active_index() {
                    self.forget_layout();
                }
                self.message = Some(format!("Reloaded {}", path.display()));
            }
            Err(problem) => {
                self.message = Some(format!("Quill could not reload {}: {problem}", path.display()))
            }
        }
    }

    /// Confirm a prompt, which is what pressing its button does.
    ///
    /// Public so a test can drive it: a screenshot test can put a name in the field but cannot press
    /// the button, and a prompt that can only be answered with the mouse cannot be tested.
    pub fn run_prompt_for_test(&mut self, prompt: Prompt) {
        self.run_prompt(prompt);
    }

    /// Do what the text prompt was asking about, now that it has been confirmed.
    ///
    /// The prompt itself knows nothing about files or git; this is where a typed name turns into a
    /// change, which is the same rule every menu entry follows.
    fn run_prompt(&mut self, prompt: Prompt) {
        let name = prompt.value.trim().to_owned();
        if name.is_empty() {
            return;
        }
        match prompt.purpose {
            Purpose::NewFile(folder) => {
                let target = crate::services::file_clipboard::free_name(&folder, &name);
                match std::fs::write(&target, "") {
                    Ok(()) => {
                        self.tree.reload();
                        self.tree.expand(&folder);
                        self.open_path_permanently(&target);
                    }
                    Err(problem) => {
                        self.message =
                            Some(format!("Quill could not make {}: {problem}", target.display()))
                    }
                }
            }
            Purpose::Rename(path) => {
                let Some(folder) = path.parent() else {
                    return;
                };
                let target = folder.join(&name);
                if target == path {
                    return;
                }
                if target.exists() {
                    self.message = Some(format!("{} is already there", target.display()));
                    return;
                }
                match std::fs::rename(&path, &target) {
                    Ok(()) => {
                        self.tree.reload();
                        // A tab on the file that was renamed is now looking at a path with nothing
                        // at it, so it is reopened at the new one rather than left pointing at
                        // nothing.
                        if let Some(index) = self.files.index_of(&path) {
                            self.files.show(index);
                            self.close_tab(index);
                            if target.is_file() {
                                self.open_path_permanently(&target);
                            }
                        }
                        self.message = Some(format!("Renamed to {name}"));
                    }
                    Err(problem) => {
                        self.message = Some(format!("Quill could not rename {}: {problem}", path.display()))
                    }
                }
            }
            Purpose::NewBranch => self.send_git(quill_git::worker::Request::CreateBranch(name)),
            Purpose::NewTag => self.send_git(quill_git::worker::Request::Tag(name)),
            Purpose::Stash => self.send_git(quill_git::worker::Request::Stash {
                message: name,
                include_untracked: true,
            }),
            Purpose::Clone => {
                let parent = self.tree.root().to_path_buf();
                self.send_git(quill_git::worker::Request::Clone { parent, url: name });
            }
            Purpose::CompareWithRevision(path) => {
                self.send_git(quill_git::worker::Request::Diff {
                    path,
                    staged: false,
                    revision: Some(name),
                });
            }
            Purpose::ResetTo(mode) => {
                let _ = mode;
            }
        }
    }

    /// Send a request to the git thread, saying so when there is no repository to send it to.
    fn send_git(&mut self, request: quill_git::worker::Request) {
        match self.git.as_mut() {
            Some(git) => git.send(request),
            None => self.message = Some("This folder is not in a git repository.".to_owned()),
        }
    }

    /// Show the tab at `index`.
    ///
    /// The laid out text is a cache of the tab that is showing, and each document counts its own
    /// revisions from one, so two tabs can be at the same revision. Comparing revisions alone would
    /// therefore keep the layout of the file that was showing before. That is the same fault
    /// [`Self::forget_layout`] exists for, wearing a different hat.
    pub fn show_tab(&mut self, index: usize) {
        if index == self.files.active_index() && index < self.files.len() {
            return;
        }
        self.files.show(index);
        self.forget_layout();
    }

    /// Close the tab at `index`, and show whatever is left.
    pub fn close_tab(&mut self, index: usize) {
        self.files.close(index);
        self.forget_layout();
    }

    /// Throw away what was laid out, because the document it belonged to has gone.
    fn forget_layout(&mut self) {
        self.layout_stale = true;
        self.preview = None;
        // The revision counts changes to one document, so it starts again for the next one and two
        // documents can be at the same number. Comparing revisions alone would leave the new file
        // wearing the last one's colours, or none at all — the same trap `layout_stale` exists for.
        self.coloured_revision = None;
        self.files.active_mut().preview_scroll = 0.0;
    }

    /// The name shown in the title bar and the status bar.
    fn file_name(&self) -> String {
        self.files.active().name()
    }

    /// The folder shown after the file name in the title bar.
    fn folder_name(&self) -> Option<String> {
        self.tree.root().file_name().map(|name| name.to_string_lossy().to_string())
    }

    fn save(&mut self) {
        if self.document().path().is_none() {
            // With no file to save to, write into the folder the explorer is showing rather than silently
            // doing nothing.
            let target = self.tree.root().join("untitled.md");
            if self.document_mut().save_as(&target).is_ok() {
                self.tree.reload();
            }
            return;
        }
        let _ = self.document_mut().save();
    }

    /// Write the settings and the pane sizes, if there is anywhere to write them.
    fn write_settings(&mut self) {
        if let Some(store) = &self.store {
            settings::save(store, &self.settings, &self.panes);
        }
        self.unsaved_settings = false;
    }

    /// The Markdown preview as it was last laid out, which the tests assert against.
    pub fn preview_layout(&self) -> &Layout {
        &self.preview_layout
    }

    /// The preview's text, for a test that wants to check what the parser produced.
    pub fn preview_text(&self) -> String {
        self.preview.as_ref().map(|p| p.text.to_string()).unwrap_or_default()
    }

    /// Work the preview out again if the source or the width changed.
    ///
    /// The preview is produced by `quill_core::markdown`, which turns the source into the same three
    /// things a document holds, so the ordinary layout engine and the ordinary painter draw it. Nothing
    /// here knows how to render Markdown.
    fn refresh_preview(&mut self, width: f32) {
        let revision = self.document().revision();
        if self.preview.is_some()
            && !self.layout_stale
            && revision == self.preview_revision
            && (width - self.preview_width).abs() < 0.5
        {
            return;
        }
        let base = quill_core::CharStyle {
            family: self.settings.font_family.clone(),
            size: self.document().active_style().size,
            color: quill_core::Color::rgb(color::TEXT.r(), color::TEXT.g(), color::TEXT.b()),
            ..quill_core::CharStyle::default()
        };
        let colors = quill_core::PreviewColors {
            text: quill_core::Color::rgb(
                color::TEXT_STRONG.r(),
                color::TEXT_STRONG.g(),
                color::TEXT_STRONG.b(),
            ),
            code: quill_core::Color::rgb(0x7E, 0xD3, 0x9B),
            link: quill_core::Color::rgb(color::ACCENT.r(), color::ACCENT.g(), color::ACCENT.b()),
            quiet: quill_core::Color::rgb(
                color::TEXT_DIM.r(),
                color::TEXT_DIM.g(),
                color::TEXT_DIM.b(),
            ),
            rule: quill_core::Color::rgb(
                color::DIVIDER.r(),
                color::DIVIDER.g(),
                color::DIVIDER.b(),
            ),
        };
        let preview = quill_core::markdown::render(
            &self.document().text().to_string(),
            &base,
            colors,
            self.renderer.monospaced_family(),
        );
        self.preview_layout = layout(
            &preview.text,
            &preview.chars,
            &preview.paragraphs,
            &self.renderer,
            width,
        );
        self.preview = Some(preview);
        self.preview_revision = revision;
        self.preview_width = width;
    }

    /// Lay the document out if the text, the formatting or the width changed since the last time.
    fn refresh_layout(&mut self, width: f32) {
        let revision = self.document().revision();
        if !self.layout_stale
            && revision == self.laid_out_revision
            && (width - self.laid_out_width).abs() < 0.5
        {
            return;
        }
        self.layout_stale = false;
        self.layout = layout(
            self.document().text(),
            self.document().chars(),
            self.document().paragraphs(),
            &self.renderer,
            width,
        );
        self.laid_out_revision = revision;
        self.laid_out_width = width;
    }

    /// Draw the whole window. Split out from the `eframe::App` implementation so the screenshot tests can
    /// drive it without a real window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if !self.themed {
            // Only the palette and spacing. The fonts are installed in `prepare`, before the first frame.
            theme::apply(ui.ctx());
            self.themed = true;
        }
        if self.context.is_none() {
            self.context = Some(ui.ctx().clone());
        }
        // Looked for once, on the first frame, rather than in `new`: a window built by a test has no
        // context to wake and no business starting a thread, and this is the first point at which
        // there is one.
        if !self.git_looked {
            self.open_repository();
        }
        self.ask_git_about_the_open_file();
        self.colour_the_open_file();
        let full = ui.max_rect();

        // The window is one painted surface with rounded corners, because it has no operating system
        // title bar. Everything else is drawn on top of it.
        ui.painter().rect_filled(
            full,
            CornerRadius::same(size::WINDOW_CORNER),
            theme::faded(color::EDITOR, self.settings.opacity),
        );

        let title_rect = Rect::from_min_size(full.min, Vec2::new(full.width(), size::TITLE_BAR));
        let toolbar_rect = Rect::from_min_size(
            Pos2::new(full.left(), title_rect.bottom()),
            Vec2::new(full.width(), size::TOOLBAR),
        );
        let status_rect = Rect::from_min_size(
            Pos2::new(full.left(), full.bottom() - size::STATUS_BAR),
            Vec2::new(full.width(), size::STATUS_BAR),
        );
        let body = Rect::from_min_max(
            Pos2::new(full.left(), toolbar_rect.bottom()),
            Pos2::new(full.right(), status_rect.top()),
        );

        // The terminal takes the bottom of the window across its whole width, as it does in IntelliJ, and
        // the explorer and the editing area share what is left.
        let terminal_height = if self.terminal.visible {
            self.panes
                .terminal_height
                .clamp(settings::TERMINAL_MIN, (body.height() - 120.0).max(settings::TERMINAL_MIN))
        } else {
            0.0
        };
        let upper = Rect::from_min_max(
            body.min,
            Pos2::new(body.right(), body.bottom() - terminal_height),
        );
        let terminal_rect =
            Rect::from_min_max(Pos2::new(body.left(), upper.bottom()), body.max);

        let explorer_width = if self.explorer_visible {
            self.panes.explorer_width.clamp(settings::EXPLORER_MIN, settings::EXPLORER_MAX)
        } else {
            0.0
        };
        let explorer_rect =
            Rect::from_min_size(upper.min, Vec2::new(explorer_width, upper.height()));
        let editing_area =
            Rect::from_min_max(Pos2::new(upper.left() + explorer_width, upper.top()), upper.max);
        // The tabs belong to the editor, so the strip spans the editing area rather than the window:
        // the explorer is to the left of it, which is where IntelliJ puts it too.
        let tabs_rect = Rect::from_min_size(
            editing_area.min,
            Vec2::new(editing_area.width(), file_tabs::HEIGHT),
        );
        let editor_rect = Rect::from_min_max(
            Pos2::new(editing_area.left(), tabs_rect.bottom()),
            editing_area.max,
        );

        // The menus, which the title bar draws when they are not in the screen's own bar.
        let menus = actions::menus(&self.menu_state());
        let mut action = None;

        // The title bar.
        let outcome = title_bar::show(
            ui,
            title_rect,
            &self.file_name(),
            self.folder_name().as_deref(),
            self.document().is_modified(),
            self.settings.opacity,
            self.menu_placement,
            &menus,
        );
        if outcome.close {
            self.closing = true;
            self.write_settings();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if outcome.minimise {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if outcome.toggle_maximise {
            let maximised = ui.input(|input| input.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximised));
        }
        if let Some(chosen) = outcome.action {
            action = Some(chosen);
        }

        // The macOS menu bar, which is built once and rebuilt when what it holds changes.
        if let Some(native) = self.native_menu.as_mut() {
            native.refresh(&menus);
            if let Some(chosen) = native.poll() {
                action = Some(chosen);
            }
        }

        // The toolbar.
        let toolbar_outcome = {
            let mut toolbar_ui = ui.new_child(egui::UiBuilder::new().max_rect(toolbar_rect));
            toolbar::show(
                &mut toolbar_ui,
                toolbar_rect,
                self.document(),
                self.settings.opacity,
                &self.bold_family,
                self.view_mode(),
            )
        };
        for command in toolbar_outcome.commands {
            self.document_mut().apply(command);
        }
        if let Some(mode) = toolbar_outcome.view_mode {
            self.set_view_mode(mode);
        }
        title_bar::divider(
            ui.painter(),
            Pos2::new(toolbar_rect.left(), toolbar_rect.bottom()),
            Pos2::new(toolbar_rect.right(), toolbar_rect.bottom()),
        );

        // The shortcuts belonging to the menus. Read here rather than in the editing area, because they work
        // whether or not the editing area has the keyboard, and because in preview mode there is no editing
        // area taking key presses at all. On macOS these never arrive, because the menu bar takes them
        // first and sends an action instead.
        if action.is_none() {
            let state = self.menu_state();
            // While one of the window's text boxes has the keyboard, undo, redo and select all
            // belong to that box rather than to the document, and it already does all three itself.
            // The rest of the menu is untouched, so control and S in the filter box still saves.
            let in_a_text_box = text_box_has_the_keyboard(ui.ctx());
            action = ui.input(|input| {
                let mut found = None;
                for event in &input.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                        if let Some(chosen) = actions::action_for_key(&state, *key, modifiers) {
                            if in_a_text_box && chosen.belongs_to_a_focused_text_box() {
                                continue;
                            }
                            found = Some(chosen);
                        }
                    }
                }
                found
            });
        }

        // The explorer, and the divider that sets its width.
        if self.explorer_visible {
            let explorer_outcome = {
                let open = self.files.active().path().map(std::path::Path::to_path_buf);
                let unsaved = self.document().is_modified();
                // Worked out for every row before the explorer is drawn, because decoding an icon
                // needs the context mutably and the explorer already has the window borrowed.
                let rows: Vec<PathBuf> = if self.filter.trim().is_empty() {
                    self.tree.rows().iter().map(|row| row.entry.path.clone()).collect()
                } else {
                    self.tree.matching(&self.filter).iter().map(|path| path.to_path_buf()).collect()
                };
                let decorations: Vec<(PathBuf, explorer::Decoration)> = rows
                    .into_iter()
                    .map(|path| {
                        let icon = self.plugin_icon(ui.ctx(), Some(&path));
                        let tint = self
                            .git
                            .as_ref()
                            .and_then(|git| git.state_of(&path))
                            .map(git_colour);
                        (path, explorer::Decoration { tint, icon })
                    })
                    .collect();
                let decorate = move |path: &std::path::Path| -> explorer::Decoration {
                    decorations
                        .iter()
                        .find(|(known, _)| known == path)
                        .map(|(_, decoration)| decoration.clone())
                        .unwrap_or_default()
                };
                let mut explorer_ui = ui.new_child(egui::UiBuilder::new().max_rect(explorer_rect));
                explorer::show(
                    &mut explorer_ui,
                    explorer_rect,
                    &self.tree,
                    &mut self.filter,
                    open.as_deref(),
                    unsaved,
                    self.settings.opacity,
                    &decorate,
                )
            };
            if let Some(path) = explorer_outcome.toggle {
                self.tree.toggle(&path);
            }
            if let Some(path) = explorer_outcome.open {
                self.open_path(&path);
                self.focus = Focus::Editor;
            }
            if let Some(path) = explorer_outcome.open_permanently {
                self.open_path_permanently(&path);
                self.focus = Focus::Editor;
            }
            if explorer_outcome.hide {
                self.explorer_visible = false;
            }
            if explorer_outcome.add {
                self.save();
            }
            if let Some((at, path, directory)) = explorer_outcome.context_menu {
                self.explorer_menu = Some((at, path, directory));
            }
        }

        // The tabs, one for each open file.
        {
            let icons: Vec<Option<egui::TextureHandle>> = self
                .files
                .iter()
                .map(|file| file.path().map(std::path::Path::to_path_buf))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|path| self.plugin_icon(ui.ctx(), path.as_deref()))
                .collect();
            let tabs: Vec<TabView> = self
                .files
                .iter()
                .zip(icons)
                .map(|(file, icon)| TabView {
                    name: file.name(),
                    modified: file.document.is_modified(),
                    transient: file.transient,
                    marker: file
                        .path()
                        .map(theme::file_marker)
                        .unwrap_or(color::FILE_TEXT),
                    icon,
                })
                .collect();
            let active = self.files.active_index();
            let opacity = self.settings.opacity;
            let outcome = {
                let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect));
                file_tabs::show(&mut tabs_ui, tabs_rect, &tabs, active, opacity)
            };
            if let Some(index) = outcome.show {
                self.show_tab(index);
                self.focus = Focus::Editor;
            }
            if let Some(index) = outcome.keep {
                self.show_tab(index);
                self.files.make_permanent(index);
                self.focus = Focus::Editor;
            }
            if let Some(index) = outcome.close {
                self.close_tab(index);
            }
        }

        // The editing area, split according to the view mode.
        match self.view_mode() {
            ViewMode::Raw => self.show_editor(ui, editor_rect),
            ViewMode::Preview => {
                self.editor_area = editor_rect;
                self.show_preview(ui, editor_rect);
            }
            ViewMode::SideBySide => {
                let fraction = self.panes.preview_fraction.clamp(0.15, 0.85);
                let split = (editor_rect.width() * fraction).floor();
                let left =
                    Rect::from_min_size(editor_rect.min, Vec2::new(split, editor_rect.height()));
                let right = Rect::from_min_max(
                    Pos2::new(editor_rect.left() + split, editor_rect.top()),
                    editor_rect.max,
                );
                self.show_editor(ui, left);
                self.show_preview(ui, right);
                // The split between the source and the preview is a pane like any other, so it is dragged.
                let edge = Rect::from_min_size(
                    Pos2::new(right.left(), right.top()),
                    Vec2::new(1.0, right.height()),
                );
                let drag = splitter::show(ui, edge, "preview", splitter::Axis::Upright);
                if drag.delta != 0.0 && editor_rect.width() > 0.0 {
                    self.panes.preview_fraction =
                        (fraction + drag.delta / editor_rect.width()).clamp(0.15, 0.85);
                    self.unsaved_settings = true;
                }
                if drag.reset {
                    self.panes.preview_fraction = 0.5;
                    self.unsaved_settings = true;
                }
            }
        }

        // The divider that sets the explorer's width. Added after the editing area rather than before it,
        // because the editing area takes drags over the whole of its rectangle and the divider overlaps its
        // left edge: a widget added earlier would sit underneath and never be dragged.
        if self.explorer_visible {
            let edge = Rect::from_min_size(
                Pos2::new(explorer_rect.right(), explorer_rect.top()),
                Vec2::new(1.0, explorer_rect.height()),
            );
            let drag = splitter::show(ui, edge, "explorer", splitter::Axis::Upright);
            if drag.delta != 0.0 {
                self.panes.explorer_width = (self.panes.explorer_width + drag.delta)
                    .clamp(settings::EXPLORER_MIN, settings::EXPLORER_MAX);
                self.unsaved_settings = true;
            }
            if drag.reset {
                self.panes.explorer_width = settings::EXPLORER_WIDTH;
                self.unsaved_settings = true;
            }
        }

        // With the explorer hidden there has to be a way to bring it back. It is drawn after the editing
        // area rather than before it, because the editing area takes clicks over the whole of its
        // rectangle and a widget added earlier would sit underneath and never be clicked.
        if !self.explorer_visible {
            let button = Rect::from_min_size(
                Pos2::new(upper.left() + 8.0, upper.top() + 8.0),
                Vec2::splat(24.0),
            );
            let response = ui
                .interact(button, ui.id().with("show-explorer"), egui::Sense::click())
                .on_hover_text("Show the explorer");
            ui.painter().rect_filled(button, CornerRadius::same(4), color::CONTROL);
            theme::icon::disclosure(ui.painter(), button.center(), false, color::TEXT_DIM);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Show the explorer")
            });
            if response.clicked() {
                self.explorer_visible = true;
            }
        }

        // The explorer's own menu, drawn after the explorer and the editing area so it sits over
        // both rather than under either.
        if let Some((at, path, directory)) = self.explorer_menu.clone() {
            let entries = actions::explorer_menu_with_git(
                &self.menu_state(),
                &path,
                directory,
                !self.clipboard.is_empty(),
            );
            let outcome = context_menu::show(ui, "explorer", at, &entries);
            if let Some(chosen) = outcome.chosen {
                action = Some(chosen);
            }
            if outcome.close {
                self.explorer_menu = None;
            }
        }

        // The gutter's own menu, drawn after the editing area so it sits over it rather than under.
        if let Some(at) = self.gutter_menu {
            let entries = actions::gutter_menu(&self.menu_state());
            let outcome = context_menu::show(ui, "gutter", at, &entries);
            if let Some(chosen) = outcome.chosen {
                action = Some(chosen);
            }
            if outcome.close {
                self.gutter_menu = None;
            }
        }

        // The terminal.
        if self.terminal.visible {
            let panel_outcome = {
                let mut panel_ui = ui.new_child(egui::UiBuilder::new().max_rect(terminal_rect));
                panel_ui.set_clip_rect(terminal_rect);
                let font_size = self.settings.terminal_font_size;
                terminal_panel::show(
                    &mut panel_ui,
                    terminal_rect,
                    &mut self.terminal,
                    &self.renderer,
                    font_size,
                    self.settings.opacity,
                )
            };
            if panel_outcome.drag != 0.0 {
                let limit = (body.height() - 120.0).max(settings::TERMINAL_MIN);
                self.panes.terminal_height =
                    (self.panes.terminal_height - panel_outcome.drag).clamp(settings::TERMINAL_MIN, limit);
                self.unsaved_settings = true;
            }
            if panel_outcome.reset_height {
                self.panes.terminal_height = settings::TERMINAL_HEIGHT;
                self.unsaved_settings = true;
            }
            if panel_outcome.take_focus {
                self.focus = Focus::Terminal;
            }
            if let Some(text) = panel_outcome.copy {
                ui.ctx().copy_text(text);
            }
            if panel_outcome.new_tab {
                self.new_terminal_tab();
                self.focus = Focus::Terminal;
            }
            if panel_outcome.hide {
                self.terminal.visible = false;
                self.focus = Focus::Editor;
            }
            // A shell that has stopped, from `exit` or otherwise, closes its tab, and the tile goes with the
            // last of them. A tile that never had a tab is left showing, because that is the one that has a
            // reason to give: the message says why the shell would not start.
            let had_tabs = !self.terminal.tabs.is_empty();
            self.terminal.tabs.pump();
            if had_tabs && self.terminal.tabs.is_empty() {
                self.terminal.visible = false;
                self.focus = Focus::Editor;
            }
        }
        self.terminal.focused = self.focus == Focus::Terminal;

        // A program that asked to be told when the terminal gains or loses the keyboard is told. `claude`
        // asks, and it is how it knows to stop drawing a cursor of its own.
        if self.focus != self.last_focus {
            let gained = self.focus == Focus::Terminal;
            if let Some(session) = self.terminal.tabs.active() {
                if session.wants_focus_reports() {
                    session.send(quill_terminal::keys::focus(gained));
                }
            }
            self.last_focus = self.focus;
        }

        // The status bar.
        let style = self.document().active_style();
        let branch = self.git.as_ref().and_then(|git| git.status_label());
        status_bar::show(
            ui,
            status_rect,
            &status_bar::Status {
                name: &self.file_name(),
                unsaved: self.document().is_modified(),
                kind: file_kind::kind_name(self.document().path()),
                position: self.caret_position(),
                family: &style.family,
                font_size: style.size,
                message: self
                    .git
                    .as_ref()
                    .and_then(|git| git.message.as_deref())
                    .or(self.message.as_deref()),
                git: branch.as_deref(),
            },
            self.settings.opacity,
        );
        title_bar::divider(
            ui.painter(),
            Pos2::new(status_rect.left(), status_rect.top()),
            Pos2::new(status_rect.right(), status_rect.top()),
        );

        // Anything git has answered since the last frame.
        if let Some(git) = self.git.as_mut() {
            if git.take_replies(&mut self.files) {
                ui.ctx().request_repaint();
            }
        }
        if let Some(chosen) = self.show_git_windows(ui.ctx()) {
            action = Some(chosen);
        }

        // The text prompt, drawn before the Settings window because a prompt opened from a menu
        // belongs over the window rather than over the settings.
        if let Some(mut prompt) = self.prompt.take() {
            let outcome = prompt_dialog::show(ui.ctx(), &mut prompt);
            if outcome.confirmed {
                self.run_prompt(prompt);
            } else if !outcome.cancelled {
                self.prompt = Some(prompt);
            }
        }

        // The Settings window, drawn last because it is a modal and sits over everything.
        let before = self.settings.clone();
        let project = self.folder_name().unwrap_or_default();
        let families: Vec<String> = self.renderer.families().to_vec();
        // Worked out before the window is drawn, because decoding an icon needs the context and
        // the settings window already has the plugins borrowed.
        let plugin_icons: Vec<(String, Option<egui::TextureHandle>)> = self
            .plugins
            .all()
            .iter()
            .map(|plugin| (plugin.id.clone(), plugin.icon.clone()))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(id, bytes)| {
                let texture = bytes.and_then(|bytes| self.icons.texture(ui.ctx(), &id, &bytes));
                (id, texture)
            })
            .collect();
        let icon_for = |id: &str| -> Option<egui::TextureHandle> {
            plugin_icons.iter().find(|(known, _)| known == id).and_then(|(_, icon)| icon.clone())
        };
        let store_folder = self.store.as_ref().map(|store| store.folder().to_path_buf());
        let on_disk = |id: &str| -> bool {
            store_folder
                .as_ref()
                .is_some_and(|folder| folder.join("plugins").join(id).join("plugin.conf").is_file())
        };
        let settings_outcome = settings_dialog::show(
            ui.ctx(),
            &mut self.settings_window,
            &mut self.settings,
            &families,
            &project,
            &self.plugins,
            &on_disk,
            &icon_for,
        );
        if let Some(id) = settings_outcome.plugins.install {
            self.install_plugin(&id);
        }
        if let Some((id, on)) = settings_outcome.plugins.set_enabled {
            self.plugins.set_enabled(self.store.as_ref(), &id, on);
            // The open file may have just gained or lost its colours.
            self.coloured_revision = None;
        }
        if settings_outcome.changed || self.settings != before {
            self.apply_settings(&before);
        }

        if let Some(chosen) = action {
            self.run_action(chosen, ui.ctx());
        }

        // Settings are written once the pointer is up, so that dragging a divider or a slider writes the
        // file once at the end rather than on every frame of the drag.
        if self.unsaved_settings && !ui.input(|input| input.pointer.any_down()) {
            self.write_settings();
        }
    }

    /// Change the settings from outside the Settings window, putting the change into effect at once.
    ///
    /// The window itself changes `self.settings` in place and the change is noticed in `ui`; this is for a
    /// caller that has a whole set of settings to hand, which is what a test and the command line have.
    pub fn set_settings(&mut self, settings: Settings) {
        let before = std::mem::replace(&mut self.settings, settings);
        self.apply_settings(&before);
    }

    /// A setting changed, so put it into effect and write it down.
    fn apply_settings(&mut self, before: &Settings) {
        if self.settings.font_family != before.font_family || self.settings.font_size != before.font_size
        {
            // The whole document is shown in the new font. This is not an edit: it pushes nothing onto the
            // undo history and does not mark the file as having unsaved changes, because what Quill saves is
            // plain text and carries no formatting.
            let change = self.settings.as_style_change();
            self.document_mut().set_base_style(change);
            self.preview = None;
        }
        self.unsaved_settings = true;
    }

    /// Draw the Markdown preview into `area`. It is read only, so it has no caret and no selection: there
    /// is nothing to type into, because what is shown is worked out from the source.
    fn show_preview(&mut self, ui: &mut egui::Ui, area: Rect) {
        let response = ui.interact(area, ui.id().with("preview"), egui::Sense::hover());
        let text_width = (area.width() - size::EDITOR_PADDING_X * 2.0).max(50.0);
        self.refresh_preview(text_width);

        // The preview scrolls on its own, so reading the rendered page does not move the caret.
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 && response.hovered() {
            self.files.active_mut().preview_scroll -= wheel;
        }
        let overflow =
            (self.preview_layout.height - (area.height() - size::EDITOR_PADDING_Y * 2.0)).max(0.0);
        let scroll = self.files.active().preview_scroll.clamp(0.0, overflow);
        self.files.active_mut().preview_scroll = scroll;

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - scroll,
        );
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
        editor_view::paint_text(&painter_ui, &self.renderer, &self.preview_layout, origin);
    }

    /// What the gutter is showing for the file that is open.
    fn gutter(&self) -> Gutter<'_> {
        let file = self.files.active();
        Gutter {
            numbers: self.settings.line_numbers,
            blame: file.blame.as_deref(),
            changes: &file.line_changes,
        }
    }

    fn show_editor(&mut self, ui: &mut egui::Ui, area: Rect) {
        // The gutter takes the left of the editing area, and the text starts after it. With no
        // gutter the text keeps the padding it always had, so putting the numbers away leaves the
        // window looking exactly as it did before there were any.
        let gutter = self.gutter();
        let lines = self.document().text().len_lines();
        let gutter_width = gutter::width(ui, &gutter, lines);
        let gutter_rect =
            Rect::from_min_size(area.min, Vec2::new(gutter_width, area.height()));
        let area = Rect::from_min_max(
            Pos2::new(area.left() + gutter_width, area.top()),
            area.max,
        );
        let padding =
            if gutter_width > 0.0 { editor_view::PADDING } else { size::EDITOR_PADDING_X };

        self.editor_area = area;
        let response = ui.interact(area, ui.id().with("editor"), egui::Sense::click_and_drag());
        if response.clicked() || response.drag_started() {
            self.focus = Focus::Editor;
        }
        let has_keyboard = self.focus == Focus::Editor;
        let text_width = (area.width() - padding - size::EDITOR_PADDING_X).max(50.0);
        self.refresh_layout(text_width);

        let scroll = self.files.active().scroll;
        let origin = Pos2::new(area.left() + padding, area.top() + size::EDITOR_PADDING_Y - scroll);
        // Taken apart by field, because the input handlers want the document mutably while the
        // layout they measure against is borrowed at the same time, and a method on `self` would
        // borrow the whole window.
        let Self { files, layout, .. } = self;
        let document = &mut files.active_mut().document;
        let pointer_changed = editor_view::handle_pointer(&response, document, layout, origin);
        let outcome = editor_view::handle_input(ui, document, layout, has_keyboard);
        if let Some(text) = outcome.copy {
            ui.ctx().copy_text(text);
        }
        if outcome.changed {
            // Typing into a file you were only glancing at plainly means you meant to open it, so
            // the transient tab stops being one a single click will take away.
            let active = self.files.active_index();
            self.files.make_permanent(active);
        }
        if outcome.changed || pointer_changed {
            self.refresh_layout(text_width);
        }

        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        let mut scroll = self.files.active().scroll;
        if wheel != 0.0 && response.hovered() {
            scroll -= wheel;
        }
        if outcome.scroll_to_caret {
            let caret = self.layout.caret_at(self.document().selection().head);
            let view_height = area.height() - size::EDITOR_PADDING_Y * 2.0;
            if caret.y < scroll {
                scroll = caret.y;
            } else if caret.y + caret.height > scroll + view_height {
                scroll = caret.y + caret.height - view_height;
            }
        }
        let overflow = (self.layout.height - (area.height() - size::EDITOR_PADDING_Y * 2.0)).max(0.0);
        let scroll = scroll.clamp(0.0, overflow);
        self.files.active_mut().scroll = scroll;

        let origin = Pos2::new(area.left() + padding, area.top() + size::EDITOR_PADDING_Y - scroll);

        // The gutter is drawn from the same origin as the text, so a number cannot drift away from
        // the line it belongs to.
        if gutter_width > 0.0 {
            let outcome = gutter::show(
                ui,
                gutter_rect,
                &self.gutter(),
                &self.layout,
                origin.y,
                self.document().text().byte_to_line(self.document().selection().head),
            );
            if let Some(at) = outcome.context_menu {
                self.gutter_menu = Some(at);
            }
        }

        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
        editor_view::paint(
            &painter_ui,
            &self.renderer,
            self.document(),
            &self.layout,
            origin,
            editor_view::PaintStyle {
                selection: color::TEXT_SELECTION,
                caret: color::ACCENT,
                show_caret: has_keyboard,
            },
        );
    }
}

/// The colour a file is drawn in for what git thinks of it.
///
/// The same three colours the change bars in the gutter and the markers in the commit panel use, so
/// a modified file is one colour wherever it is shown.
fn git_colour(state: quill_git::State) -> Color32 {
    match state {
        quill_git::State::Untracked => color::GIT_UNTRACKED,
        quill_git::State::Added | quill_git::State::Copied => color::GIT_ADDED,
        quill_git::State::Unmerged => color::CLOSE,
        quill_git::State::Ignored | quill_git::State::Unchanged => color::TEXT_FAINT.gamma_multiply(0.7),
        _ => color::GIT_MODIFIED,
    }
}

impl eframe::App for QuillApp {
    /// The window background. eframe asks for this every frame, so changing the opacity takes effect at
    /// once.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Fully transparent, because the window's own rounded rectangle is painted in `ui`. Anything
        // painted here would show outside the rounded corners.
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Nothing on macOS: the compositor there takes the surface's alpha on its own.
        #[cfg(windows)]
        crate::services::windows_transparency::keep_transparent(_frame);
        QuillApp::ui(self, ui);
    }

    /// Write the settings and the pane sizes before the window goes.
    fn on_exit(&mut self) {
        self.write_settings();
    }
}
