//! The Quill window.
//!
//! Holds the open document, the file explorer, the terminal, the fonts and the settings, and lays the
//! window out: a title bar Quill draws itself, the formatting toolbar, the explorer down the left, the
//! editing area filling the rest, the terminal along the bottom when it is showing, and the status bar.
//!
//! Transparency works because the background and the text are two separate paints. `clear_color` gives the
//! operating system compositor an alpha taken from the opacity setting, so the desktop shows through the
//! window. Every glyph is painted at full alpha, so the writing stays sharp at every setting.
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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Rect, Vec2};
use quill_core::{layout, Command, Document, Layout};

use crate::components::editor_view;
use crate::components::explorer;
use crate::components::settings_dialog::{self, SettingsWindow};
use crate::components::splitter;
use crate::components::status_bar;
use crate::components::terminal_panel::{self, TerminalPanel};
use crate::components::title_bar::{self, MenuPlacement};
use crate::components::toolbar;
use crate::services::file_kind;
use crate::services::file_tree::FileTree;
use crate::services::launcher;
use crate::services::native_menu::NativeMenu;
use crate::services::store::Store;
use crate::services::text_renderer::TextRenderer;
use crate::settings::{self, Panes, Settings};
use crate::theme::{self, color, size};

use actions::{Action, MenuState};

/// How opaque the background is when Quill starts.
pub const DEFAULT_OPACITY: f32 = settings::DEFAULT_OPACITY;

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

pub struct QuillApp {
    pub document: Document,
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
    /// How far the editing area is scrolled.
    scroll: f32,
    /// The rectangle the editing area last occupied, so a test can measure the document's own text without
    /// also measuring the bars round it.
    editor_area: Rect,
    /// Which of the three ways of looking at the file is showing.
    pub view_mode: ViewMode,
    /// The Markdown preview, worked out from the source and kept until the source changes.
    preview: Option<quill_core::Preview>,
    preview_layout: Layout,
    preview_revision: u64,
    preview_width: f32,
    /// How far the preview is scrolled, which is separate from the editor's own scrolling.
    preview_scroll: f32,
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
            document,
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
            scroll: 0.0,
            editor_area: Rect::ZERO,
            view_mode: ViewMode::Raw,
            preview: None,
            preview_layout: Layout::default(),
            preview_revision: 0,
            preview_width: 0.0,
            preview_scroll: 0.0,
            themed: false,
            bold_family: egui::FontFamily::Proportional,
            context: None,
            last_focus: Focus::Editor,
        }
    }

    /// A window whose document already holds `text`, which the screenshot tests use to set a scene.
    pub fn with_text(folder: impl Into<PathBuf>, text: &str) -> Self {
        let mut app = Self::new(folder);
        app.document.apply(Command::Insert(text.to_owned()));
        app.document.apply(Command::MoveDocumentStart { extend: false });
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
        // On the first run there is no settings file. One is written straight away, holding the defaults,
        // so that a person looking for it finds it and can see what its names are rather than having to
        // change a setting first to make it appear.
        let first_run = !store.settings_path().is_file();
        self.store = Some(store);
        if first_run {
            self.write_settings();
        }
        self.document.set_base_style(self.settings.as_style_change());
    }

    /// Build the macOS menu bar. Called by the released binary only: a test has no application to attach a
    /// menu bar to, and the bar drawn inside the window is what the tests exercise.
    pub fn install_native_menu(&mut self) {
        let menus = actions::menus(&self.menu_state());
        self.native_menu = Some(NativeMenu::install(&menus, self.context.as_ref()));
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
        status_bar::position_of(self.document.text(), self.document.selection().head)
    }

    /// Run a command, as the toolbar or a test would.
    pub fn command(&mut self, command: Command) {
        self.document.apply(command);
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
            can_undo: self.document.can_undo(),
            can_redo: self.document.can_redo(),
            has_selection: !self.document.selection().is_empty(),
            recent: self.recent.clone(),
            view_mode: self.view_mode,
            explorer_visible: self.explorer_visible,
            terminal_visible: self.terminal.visible,
            terminal_tabs: self.terminal.tabs.count(),
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
                    if self.document.save_as(&target).is_ok() {
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
                self.document.apply(Command::Undo);
            }
            Action::Redo => {
                self.document.apply(Command::Redo);
            }
            Action::Cut => {
                if self.focus == Focus::Terminal {
                    if let Some(text) = self.terminal.tabs.active().and_then(|s| s.selected_text()) {
                        ctx.copy_text(text);
                    }
                } else if !self.document.selection().is_empty() {
                    ctx.copy_text(self.document.selected_text());
                    self.document.apply(Command::DeleteBackward);
                }
            }
            Action::Copy => {
                if self.focus == Focus::Terminal {
                    if let Some(text) = self.terminal.tabs.active().and_then(|s| s.selected_text()) {
                        ctx.copy_text(text);
                    }
                } else if !self.document.selection().is_empty() {
                    ctx.copy_text(self.document.selected_text());
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
                            self.document.apply(Command::Insert(text));
                        }
                    }
                    Ok(_) => {}
                    Err(problem) => eprintln!("Quill could not read the clipboard: {problem}"),
                }
            }
            Action::SelectAll => {
                self.document.apply(Command::SelectAll);
            }
            Action::SetViewMode(mode) => self.view_mode = mode,
            Action::ToggleExplorer => self.explorer_visible = !self.explorer_visible,
            Action::ToggleTerminal => {
                self.terminal.visible = !self.terminal.visible;
                if self.terminal.visible {
                    self.open_terminal_tab();
                    self.focus = Focus::Terminal;
                } else {
                    self.focus = Focus::Editor;
                }
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
    }

    /// Open a file into the editor.
    ///
    /// Any file holding text opens. A `.md` file is Markdown, which means the preview button does something
    /// with it; everything else opens as plain text, which is what `tasks/improvements.md` asks for.
    pub fn open_path(&mut self, path: &Path) {
        if let Err(refusal) = file_kind::openable(path) {
            self.message = Some(format!("{}: {}", path.display(), refusal.reason()));
            return;
        }
        match Document::open(path) {
            Ok(mut document) => {
                document.apply(Command::MoveDocumentStart { extend: false });
                self.document = document;
                self.document.set_base_style(self.settings.as_style_change());
                self.scroll = 0.0;
                self.message = None;
                // The new document counts its revisions from the beginning, so what was laid out for the last
                // one has to be thrown away rather than compared against.
                self.forget_layout();
                // A file that is not Markdown has nothing to preview, so the raw source is shown.
                if !file_kind::is_markdown(Some(path)) {
                    self.view_mode = ViewMode::Raw;
                }
            }
            Err(error) => {
                // Nothing is thrown away: the document that was open stays open, and the reason is said in
                // the status bar rather than only on the error output.
                self.message = Some(format!("Quill could not open {}: {error}", path.display()));
                eprintln!("Quill could not open {}: {error}", path.display());
            }
        }
    }

    /// Throw away what was laid out, because the document it belonged to has gone.
    fn forget_layout(&mut self) {
        self.layout_stale = true;
        self.preview = None;
        self.preview_scroll = 0.0;
    }

    /// The name shown in the title bar and the status bar.
    fn file_name(&self) -> String {
        self.document
            .path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_owned())
    }

    /// The folder shown after the file name in the title bar.
    fn folder_name(&self) -> Option<String> {
        self.tree.root().file_name().map(|name| name.to_string_lossy().to_string())
    }

    fn save(&mut self) {
        if self.document.path().is_none() {
            // With no file to save to, write into the folder the explorer is showing rather than silently
            // doing nothing.
            let target = self.tree.root().join("untitled.md");
            if self.document.save_as(&target).is_ok() {
                self.tree.reload();
            }
            return;
        }
        let _ = self.document.save();
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
        let revision = self.document.revision();
        if self.preview.is_some()
            && !self.layout_stale
            && revision == self.preview_revision
            && (width - self.preview_width).abs() < 0.5
        {
            return;
        }
        let base = quill_core::CharStyle {
            family: self.settings.font_family.clone(),
            size: self.document.active_style().size,
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
            &self.document.text().to_string(),
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
        let revision = self.document.revision();
        if !self.layout_stale
            && revision == self.laid_out_revision
            && (width - self.laid_out_width).abs() < 0.5
        {
            return;
        }
        self.layout_stale = false;
        self.layout = layout(
            self.document.text(),
            self.document.chars(),
            self.document.paragraphs(),
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
        let editor_rect =
            Rect::from_min_max(Pos2::new(upper.left() + explorer_width, upper.top()), upper.max);

        // The menus, which the title bar draws when they are not in the screen's own bar.
        let menus = actions::menus(&self.menu_state());
        let mut action = None;

        // The title bar.
        let outcome = title_bar::show(
            ui,
            title_rect,
            &self.file_name(),
            self.folder_name().as_deref(),
            self.document.is_modified(),
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
                &self.document,
                self.settings.opacity,
                &self.bold_family,
                self.view_mode,
            )
        };
        for command in toolbar_outcome.commands {
            self.document.apply(command);
        }
        if let Some(mode) = toolbar_outcome.view_mode {
            self.view_mode = mode;
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
            action = ui.input(|input| {
                let mut found = None;
                for event in &input.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                        if let Some(chosen) = actions::action_for_key(&state, *key, modifiers) {
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
                let mut explorer_ui = ui.new_child(egui::UiBuilder::new().max_rect(explorer_rect));
                explorer::show(
                    &mut explorer_ui,
                    explorer_rect,
                    &self.tree,
                    &mut self.filter,
                    self.document.path(),
                    self.document.is_modified(),
                    self.settings.opacity,
                )
            };
            if let Some(path) = explorer_outcome.toggle {
                self.tree.toggle(&path);
            }
            if let Some(path) = explorer_outcome.open {
                self.open_path(&path);
                self.focus = Focus::Editor;
            }
            if explorer_outcome.hide {
                self.explorer_visible = false;
            }
            if explorer_outcome.add {
                self.save();
            }
        }

        // The editing area, split according to the view mode.
        match self.view_mode {
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
        let style = self.document.active_style();
        status_bar::show(
            ui,
            status_rect,
            &status_bar::Status {
                name: &self.file_name(),
                unsaved: self.document.is_modified(),
                kind: file_kind::kind_name(self.document.path()),
                position: self.caret_position(),
                family: &style.family,
                font_size: style.size,
                message: self.message.as_deref(),
            },
            self.settings.opacity,
        );
        title_bar::divider(
            ui.painter(),
            Pos2::new(status_rect.left(), status_rect.top()),
            Pos2::new(status_rect.right(), status_rect.top()),
        );

        // The Settings window, drawn last because it is a modal and sits over everything.
        let before = self.settings.clone();
        let project = self.folder_name().unwrap_or_default();
        let families: Vec<String> = self.renderer.families().to_vec();
        let settings_outcome = settings_dialog::show(
            ui.ctx(),
            &mut self.settings_window,
            &mut self.settings,
            &families,
            &project,
        );
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
            self.document.set_base_style(self.settings.as_style_change());
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
            self.preview_scroll -= wheel;
        }
        let overflow =
            (self.preview_layout.height - (area.height() - size::EDITOR_PADDING_Y * 2.0)).max(0.0);
        self.preview_scroll = self.preview_scroll.clamp(0.0, overflow);

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - self.preview_scroll,
        );
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
        editor_view::paint_text(&painter_ui, &self.renderer, &self.preview_layout, origin);
    }

    fn show_editor(&mut self, ui: &mut egui::Ui, area: Rect) {
        self.editor_area = area;
        let response = ui.interact(area, ui.id().with("editor"), egui::Sense::click_and_drag());
        if response.clicked() || response.drag_started() {
            self.focus = Focus::Editor;
        }
        let has_keyboard = self.focus == Focus::Editor;
        let text_width = (area.width() - size::EDITOR_PADDING_X * 2.0).max(50.0);
        self.refresh_layout(text_width);

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - self.scroll,
        );
        let pointer_changed =
            editor_view::handle_pointer(&response, &mut self.document, &self.layout, origin);
        let outcome =
            editor_view::handle_input(ui, &mut self.document, &self.layout, has_keyboard);
        if let Some(text) = outcome.copy {
            ui.ctx().copy_text(text);
        }
        if outcome.changed || pointer_changed {
            self.refresh_layout(text_width);
        }

        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 && response.hovered() {
            self.scroll -= wheel;
        }
        if outcome.scroll_to_caret {
            let caret = self.layout.caret_at(self.document.selection().head);
            let view_height = area.height() - size::EDITOR_PADDING_Y * 2.0;
            if caret.y < self.scroll {
                self.scroll = caret.y;
            } else if caret.y + caret.height > self.scroll + view_height {
                self.scroll = caret.y + caret.height - view_height;
            }
        }
        let overflow = (self.layout.height - (area.height() - size::EDITOR_PADDING_Y * 2.0)).max(0.0);
        self.scroll = self.scroll.clamp(0.0, overflow);

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - self.scroll,
        );
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
        editor_view::paint(
            &painter_ui,
            &self.renderer,
            &self.document,
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

impl eframe::App for QuillApp {
    /// The window background. eframe asks for this every frame, so changing the opacity takes effect at
    /// once.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Fully transparent, because the window's own rounded rectangle is painted in `ui`. Anything
        // painted here would show outside the rounded corners.
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        QuillApp::ui(self, ui);
    }

    /// Write the settings and the pane sizes before the window goes.
    fn on_exit(&mut self) {
        self.write_settings();
    }
}
