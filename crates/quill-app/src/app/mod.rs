//! The Quill window.
//!
//! Holds the open document, the file explorer, the terminal, the fonts and the settings, and lays the
//! window out: a title bar Quill draws itself, which carries the menus, the project's name and the text
//! tools; a thin rail of pane buttons down the far left; the explorer beside it; the editing area filling
//! the rest; the terminal along the bottom when it is showing; and the status bar.
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

pub mod action_names;
pub mod actions;
pub mod cli;
pub mod files;
pub mod git;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Rect, Vec2};
use quill_core::{layout, relayout, Command, Document, Highlights, Layout, Rgba};

use crate::components::about_dialog::{self, About};
use crate::components::activity_bar;
use crate::components::context_menu;
use crate::components::diagram_view;
use crate::components::editor_view;
use crate::components::explorer;
use crate::components::file_tabs::{self, TabView};
use crate::components::find_in_files::{self, FindInFiles};
use crate::components::git_dialogs::{self, Dialog};
use crate::components::git_panel;
use crate::components::go_to_file::{self, GoToFile};
use crate::components::gutter::{self, Gutter};
use crate::components::picture_view;
use crate::components::prompt_dialog::{self, Prompt, Purpose};
use crate::components::resize_edges;
use crate::components::settings_dialog::{self, SettingsWindow};
use crate::components::splitter;
use crate::components::status_bar;
use crate::components::terminal_panel::{self, TerminalPanel};
use crate::components::text_menu;
use crate::components::text_tools;
use crate::components::title_bar::{self, MenuPlacement};
use crate::services::file_kind;
use crate::services::file_marks::FileMarks;
use crate::services::file_tree::FileTree;
use crate::services::file_clipboard::FileClipboard;
use crate::services::launcher;
use crate::services::icons::Icons;
use crate::services::plugins::Plugins;
use crate::services::mermaid_scene::MermaidScenes;
use crate::services::preview_images::PreviewImages;
use crate::services::project_state::{self, ProjectState};
use crate::services::native_menu::NativeMenu;
use crate::services::control;
use crate::services::store::Store;
use crate::services::text_renderer::TextRenderer;
use crate::settings::{self, Panes, Settings};
use crate::theme::{self, color, size};

use actions::{Action, GitAction, MenuState};
use git::GitState;
use files::OpenFiles;

/// How opaque the background is when Quill starts.
pub const DEFAULT_OPACITY: f32 = settings::DEFAULT_OPACITY;

/// How much bigger a pinch has to ask for before the editor's font moves a size.
///
/// The smallest gap between two of the sizes `Edit -> Settings -> Appearance -> Font` offers, which
/// is 11 to 13. A gesture asking for that much gets one size; one asking for twice as much gets two.
/// A ratio rather than a number of points, because what one notch of a wheel is worth in points is
/// a platform's business — measured on this machine it is about 55, which is a third bigger than the
/// number egui's own default assumes — and the ratio a gesture is asking for is the same everywhere.
const ZOOM_STEP: f32 = 1.18;

/// How tall the panel is that stands in for a diagram that could not be drawn.
///
/// Enough for the reason, the line it was on, and a few lines of the source under it. A fixed height
/// rather than one worked out from the source, because a document being typed into would otherwise
/// jump about as the panel grew and shrank with every keystroke.
const PROBLEM_HEIGHT: f32 = 160.0;

/// How much air is left under a picture in the Markdown preview.
///
/// The same idea as the space between two paragraphs: a picture with the next line of prose against
/// its bottom edge reads as part of the picture.
const PICTURE_GAP: f32 = 14.0;

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
    ///
    /// Markdown's wording, which is what it has always been and what every existing test asks for.
    /// A file whose preview is a diagram uses [`Self::label_for`] instead.
    pub fn label(&self) -> &'static str {
        self.label_for(file_kind::PreviewKind::Markdown)
    }

    /// The name, by what kind of preview the open file has.
    ///
    /// `task-1660` gives a `.mmd` file the same three modes a `.md` file has, and a button over a
    /// Mermaid diagram that said `Markdown preview` would be a small wrongness a reader notices at
    /// once. `Side by side` is the same word in both, which is not two controls sharing a name:
    /// only one file is open at a time, so the two are never on the screen together.
    pub fn label_for(&self, kind: file_kind::PreviewKind) -> &'static str {
        match (self, kind) {
            (ViewMode::Raw, file_kind::PreviewKind::Markdown) => "Raw Markdown",
            (ViewMode::Raw, file_kind::PreviewKind::Mermaid) => "Raw Mermaid",
            (ViewMode::SideBySide, _) => "Side by side",
            (ViewMode::Preview, file_kind::PreviewKind::Markdown) => "Markdown preview",
            (ViewMode::Preview, file_kind::PreviewKind::Mermaid) => "Mermaid diagram",
        }
    }

    pub fn description(&self) -> &'static str {
        self.description_for(file_kind::PreviewKind::Markdown)
    }

    /// What the pointer resting on the button says, by what kind of preview the file has.
    pub fn description_for(&self, kind: file_kind::PreviewKind) -> &'static str {
        match (self, kind) {
            (ViewMode::Raw, file_kind::PreviewKind::Markdown) => {
                "Raw Markdown: the source as it is on disk"
            }
            (ViewMode::Raw, file_kind::PreviewKind::Mermaid) => {
                "Raw Mermaid: the source as it is on disk"
            }
            (ViewMode::SideBySide, file_kind::PreviewKind::Markdown) => {
                "Side by side: the source on the left, the preview on the right"
            }
            (ViewMode::SideBySide, file_kind::PreviewKind::Mermaid) => {
                "Side by side: the source on the left, the diagram on the right"
            }
            (ViewMode::Preview, file_kind::PreviewKind::Markdown) => {
                "Markdown preview: the rendered document"
            }
            (ViewMode::Preview, file_kind::PreviewKind::Mermaid) => {
                "Mermaid diagram: the drawn diagram"
            }
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

/// One picture in the Markdown preview, ready to draw.
///
/// The preview is laid out by the ordinary layout engine, which knows about glyphs and not about
/// pictures. So a picture is a paragraph with no text in it that has been asked to be at least as
/// tall as the picture is drawn, and this is what the window paints into that room. Held between
/// frames because a preview is redrawn sixty times a second and a photograph is decoded once.
pub struct PlacedPicture {
    /// Which paragraph of the preview it belongs to, which is what says where it goes.
    pub paragraph: usize,
    /// How large it is drawn, in points.
    pub size: Vec2,
    /// The picture, or `None` when it could not be read — in which case the alt text is drawn.
    pub texture: Option<egui::TextureHandle>,
    pub alt: String,
}

/// One diagram in the Markdown preview, laid out and ready to draw.
///
/// The same shape as [`PlacedPicture`] and for the same reason: `quill_core::markdown` says which
/// paragraph stands in for the diagram, and this says how large it came out and what it holds. Held
/// between frames because a preview is redrawn sixty times a second and a diagram is laid out once.
pub struct PlacedDiagram {
    /// Which paragraph of the preview it belongs to.
    pub paragraph: usize,
    /// How large it is drawn, in points.
    pub size: Vec2,
    /// The diagram, or the reason it could not be drawn — which is shown in its place, because a
    /// mistake in one diagram must not take the rest of the document away.
    pub laid: crate::services::mermaid_scene::Laid,
    /// What was between the fences, so a problem can show it under the reason.
    pub source: String,
}

/// How many frames the explorer scrolls to the file that is showing after it changes. See
/// `QuillApp::reveal_in_explorer` for why it is not one.
const REVEAL_FRAMES: u8 = 2;

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
    /// What was last written to the project's own `.quill` folder, so it is written again only when
    /// something has changed. `None` while this window is not remembering the project at all, which is
    /// every window a test builds: the released binary turns it on by calling
    /// [`QuillApp::restore_project`], so a test neither reads nor writes a `.quill` folder.
    written_project: Option<ProjectState>,
    /// The macOS menu bar, once it has been installed.
    native_menu: Option<NativeMenu>,
    /// What each pane has asked for the explorer to scroll to, and what it last scrolled to.
    ///
    /// The window remembers the path it last revealed and compares it against the file showing in
    /// the pane with the keyboard, rather than each of the eleven places a tab can change calling a
    /// reveal. The twelfth, added next month, would be the one that forgot. See
    /// `Self::follow_the_open_file`.
    revealed: Option<PathBuf>,
    /// How many more frames the explorer should scroll to the file that is showing.
    ///
    /// Two rather than one, and the reason is worth keeping. Revealing a file usually **opens folders
    /// out in the same frame**, so the list can grow by forty rows between one frame and the next —
    /// and egui clamps a scroll target against the content size it measured on the *previous* frame.
    /// The first frame therefore scrolls as far as the old, shorter list allowed and stops short of
    /// the row; the second, by which time the list has been measured, reaches it. Measured on a real
    /// window: opening a file three folders down left its row just below the fold until a second
    /// frame was drawn.
    reveal_in_explorer: u8,
    /// The rectangle the editing area last occupied, so a test can measure the document's own text without
    /// also measuring the bars round it.
    editor_area: Rect,
    /// What a pinch has asked for that has not been given to it yet.
    ///
    /// A pinch arrives as a great many small multipliers, one a frame. Multiplying the size by each
    /// one and rounding to a whole point would round every one of them away and nothing would ever
    /// move, so what the gesture has asked for is kept here between frames and the setting changes
    /// only when it has asked for a whole point.
    zoom_pending: f32,
    /// Set once a pane has taken this frame's zoom gesture.
    ///
    /// A gesture belongs to the window rather than to a pane, because the size is one setting for
    /// the whole window, and the pointer says which pane it is about. Every pane used to ask the
    /// same `zoom_delta` for itself, so with the editing area split the size stepped once for
    /// each
    /// pane: one notch of the wheel took sixteen points to thirty two.
    zoom_taken: bool,
    /// Set when the pane with the keyboard is willing to take the gesture but has no pointer over
    /// it, which is settled at the end of the frame — a pinch with the pointer over the explorer or
    /// the terminal still zooms the pane a person is typing into.
    zoom_offered_to_the_keyboard: bool,
    /// The pictures in the preview, decoded and kept between frames.
    ///
    /// One of the three caches that stay on the window rather than moving onto the tab with the rest
    /// of `OpenFile::cached`: it is keyed on a path rather than on a document, so panes drawing two
    /// files share it correctly and it costs nothing to keep shared.
    preview_images: PreviewImages,
    /// Every diagram that has been laid out, kept so a preview lays each one out once. Keyed on the
    /// source text, so it is shared between panes for the same reason.
    mermaid_scenes: MermaidScenes,
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
    /// The `Go to File` modal, when it is open.
    pub go_to_file: Option<GoToFile>,
    /// The `Find in Files` modal, when it is open. It holds the thread the searching runs on, so
    /// shutting the modal is what stops that thread.
    pub find_in_files: Option<FindInFiles>,
    /// The About box, when it is open. It holds the version and the build date as text rather than
    /// reading them when it draws, so that a screenshot test can fix them; `About::current` is what
    /// `Action::About` puts here.
    pub about: Option<About>,
    /// Set when something outside the editing area moved the caret — opening a `Find in Files`
    /// result is the only one — so the next frame scrolls the file to show it.
    reveal_caret: bool,
    /// Where the gutter's own menu is open, when it is. Held here rather than in egui's memory so
    /// that a test can open it: a screenshot test cannot press the right mouse button.
    pub gutter_menu: Option<Pos2>,
    /// Where a tab's own menu is open, and which pane's strip it was opened on. Held here for the
    /// same reason the gutter's is.
    ///
    /// The entries in it all act on "the tab that is showing", which is what makes them ordinary
    /// parameterless actions the View menu and the command line can ask for too — so opening the
    /// menu shows the tab it was opened on first. The editing area's own menu already sets that
    /// precedent: a right click outside the selection puts the caret there before opening.
    pub tab_menu: Option<(Pos2, usize)>,
    /// The editing area's own menu, when it is open. Held here for the same reason the gutter's is,
    /// and it carries the colour wheel with it.
    pub text_menu: Option<text_menu::TextMenu>,
    /// The colour the wheel was last left on, so opening it again starts where it was left rather
    /// than back at the first block. In memory only: it is a habit within one sitting rather than a
    /// setting, and the four blocks are what a lasting preference looks like.
    pub last_highlight: Rgba,
    /// The passages marked in every file of this project. The authority for every file that is not
    /// open; a file that **is** open is owned by its document and pushed in here whenever it
    /// changes. See `services::file_marks`.
    pub marks: FileMarks,
    /// The command channel, once it has been opened. `None` in every window a test builds, and in a
    /// released Quill started with `--control off`: a window with no channel is an ordinary window.
    pub(crate) control: Option<control::Server>,
    /// Commands that have been accepted and are waiting for something — a painted frame, a shell, a
    /// search, git. See `app::cli`.
    pub(crate) cli_waiting: Vec<(control::Pending, cli::Waiting)>,
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
            written_project: None,
            native_menu: None,
            revealed: None,
            reveal_in_explorer: 0,
            editor_area: Rect::ZERO,
            zoom_pending: 1.0,
            zoom_taken: false,
            zoom_offered_to_the_keyboard: false,
            preview_images: PreviewImages::new(),
            mermaid_scenes: MermaidScenes::new(),
            themed: false,
            bold_family: egui::FontFamily::Proportional,
            context: None,
            last_focus: Focus::Editor,
            plugins: Plugins::load(None).0,
            icons: Icons::new(),
            git: None,
            git_looked: false,
            confirmation: None,
            clipboard: FileClipboard::new(),
            explorer_menu: None,
            prompt: None,
            go_to_file: None,
            find_in_files: None,
            about: None,
            reveal_caret: false,
            gutter_menu: None,
            text_menu: None,
            tab_menu: None,
            last_highlight: theme::color::HIGHLIGHT_YELLOW,
            marks: FileMarks::new(),
            control: None,
            cli_waiting: Vec::new(),
        }
    }

    /// A window whose document already holds `text`, which the screenshot tests use to set a scene.
    pub fn with_text(folder: impl Into<PathBuf>, text: &str) -> Self {
        let mut app = Self::new(folder);
        app.document_mut().apply(Command::Insert(text.to_owned()));
        app.document_mut().apply(Command::MoveDocumentStart { extend: false });
        app
    }

    /// Open the command channel, so that `quill-cli` can drive this window.
    ///
    /// Called from `main.rs` and nowhere else, exactly as [`Self::load_settings`] and
    /// [`Self::restore_project`] are, and for the same reason: a test must not open a port, write an
    /// instance file into the person's settings folder, or leave a listener behind when it ends. A
    /// window with no channel is an ordinary window in every other respect.
    pub fn open_control_channel(&mut self, ctx: &egui::Context) {
        let folder = self.tree.root().to_path_buf();
        // The context rather than `thread_waker`, because this is called before the first frame and
        // the window has not yet been given one to wake.
        let context = ctx.clone();
        self.control =
            control::Server::start(folder, Arc::new(move || context.request_repaint()));
        if let Some(server) = &self.control {
            self.message = Some(format!(
                "Quill {} \u{00B7} the command line is listening on port {}",
                crate::build_info::VERSION,
                server.port()
            ));
        }
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
        self.set_the_font_everywhere();
    }

    /// Read what was left open in this project, put it back, and remember it from then on.
    ///
    /// Called by the released binary only, and for the same reason [`Self::load_settings`] is: a test
    /// must not read or write anything belonging to the person running it, and a `.quill` folder written
    /// into a test's own sample project would change what the explorer draws in the middle of a
    /// screenshot test.
    ///
    /// The files are opened permanently rather than transiently, because a tab that was there when the
    /// window closed is not a file somebody is glancing at.
    pub fn restore_project(&mut self) {
        // Read before the files are opened, so each tab comes up with its passages already marked
        // rather than having them appear a frame later.
        self.marks = FileMarks::load(self.tree.root());
        let state = project_state::load(self.tree.root());
        for folder in &state.expanded_folders {
            self.tree.expand(folder);
        }
        for path in &state.open_files {
            self.open_path_permanently(path);
        }
        // The panes after the tabs, because a tab has to exist before it can be put in one. The tabs
        // are opened in the order they were written, so the list lines up with them — and anything in
        // it that would break an invariant is corrected rather than refused. See
        // `OpenFiles::restore_panes`.
        if !state.file_panes.is_empty() {
            self.files.restore_panes(&state.file_panes, &state.pane_widths, state.active_pane);
        }
        if let Some(path) = state.open_files.get(state.active_file) {
            if let Some(index) = self.files.index_of(path) {
                self.show_tab(index);
            }
        }
        self.explorer_visible = state.explorer_visible;
        // The shells themselves cannot be brought back, so the same number of fresh ones are started in
        // the project's own folder, which is what a person means by "my terminals were there".
        if state.terminal_visible && state.terminal_tabs > 0 {
            self.terminal.visible = true;
            for _ in 0..state.terminal_tabs {
                self.new_terminal_tab();
            }
        }
        self.written_project = Some(self.project_state());
    }

    /// What is open in this project now.
    fn project_state(&self) -> ProjectState {
        let open_files = self.files.paths();
        let active = self.files.active().path().and_then(|path| {
            open_files.iter().position(|known| known == path)
        });
        ProjectState {
            open_files,
            active_file: active.unwrap_or(0),
            file_panes: self.files.panes_of_tabs(),
            pane_widths: self.files.pane_widths().to_vec(),
            active_pane: self.files.focused_pane(),
            expanded_folders: self.tree.expanded_folders(),
            explorer_visible: self.explorer_visible,
            terminal_visible: self.terminal.visible,
            terminal_tabs: self.terminal.tabs.count(),
        }
    }

    /// Write what is open down, if it has changed since it was last written.
    ///
    /// Called every frame and writes almost never: the comparison is against what is on disk, so
    /// nothing is written until a tab, a folder or a pane actually changed.
    fn remember_the_project(&mut self) {
        if self.written_project.is_none() {
            return;
        }
        let now = self.project_state();
        if self.written_project.as_ref() == Some(&now) {
            return;
        }
        project_state::save(self.tree.root(), &now);
        self.written_project = Some(now);
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
        &self.files.active().cached.layout
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

    // ------------------------------------------------------------------- the marked passages

    /// Mark the selected passage in the file that is showing.
    ///
    /// The one place a passage is marked by hand: the four blocks, the four menu entries and the
    /// colour wheel all come here, so a colour chosen one way and the same colour chosen another
    /// are the same change. Nothing happens when there is no selection, and the status bar says so
    /// rather than leaving a click looking as though it did nothing.
    pub fn highlight_selection(&mut self, color: Rgba) -> bool {
        let range = self.document().selection().range();
        if range.is_empty() {
            self.message = Some("Select some text to highlight it.".to_owned());
            return false;
        }
        self.last_highlight = color;
        let marked = self.document_mut().highlight(range, color);
        if marked {
            self.message = None;
        }
        marked
    }

    /// True when there is a mark the selection touches, or one under the caret when nothing is
    /// selected. What decides whether `Clear Highlight` can be used, and what it will act on.
    pub fn marks_under_the_caret(&self) -> bool {
        let selection = self.document().selection();
        if selection.is_empty() {
            self.document().highlights().at(selection.head).is_some()
        } else {
            !self.document().highlights().overlapping(selection.range()).is_empty()
        }
    }

    /// Take away the marks the selection touches, or the one under the caret when nothing is
    /// selected.
    ///
    /// One rule rather than two, so that `Clear Highlight` means the same thing on the Edit menu, on
    /// the right click menu and from the command line. A right click outside a selection puts the
    /// caret where it was clicked before the menu opens, which is what makes "the one under the
    /// caret" the one under the pointer.
    pub fn clear_highlight_here(&mut self) -> bool {
        let selection = self.document().selection();
        if selection.is_empty() {
            self.document_mut().clear_highlight_at(selection.head)
        } else {
            self.document_mut().clear_highlight(selection.range())
        }
    }

    /// Take away every mark in the file that is showing.
    pub fn clear_highlights_here(&mut self) -> bool {
        self.document_mut().clear_highlights()
    }

    /// Change what is marked in any file of this project, whether it is open or not.
    ///
    /// The one place that choice is made, so no caller has to think about it: a file that is open is
    /// owned by its document, and every other file is owned by `services::file_marks`. Anything
    /// changed in a document is pushed into the store by [`Self::remember_the_marks`] on the same
    /// frame, so the two cannot come to disagree.
    pub fn change_highlights(
        &mut self,
        path: &Path,
        change: impl FnOnce(&mut Highlights),
    ) -> bool {
        if let Some(index) = self.files.index_of(path) {
            let mut marks = self.files.at(index).document.highlights().clone();
            let before = marks.clone();
            change(&mut marks);
            if before == marks {
                return false;
            }
            self.files.at_mut(index).document.set_highlights(marks);
            return true;
        }
        self.marks.change(path, change)
    }

    /// What is marked in one file, whether it is open or not.
    pub fn highlights_of(&self, path: &Path) -> Highlights {
        if let Some(index) = self.files.index_of(path) {
            return self.files.at(index).document.highlights().clone();
        }
        self.marks.highlights(path).cloned().unwrap_or_default()
    }

    /// Push what every open document holds into the store, and write the store if it changed.
    ///
    /// Called every frame and does almost nothing: an integer comparison for each open tab, because
    /// a document that has not changed since it was last pushed cannot have new marks in it. Writing
    /// is on the same terms as the project state — only when something changed, and only once the
    /// pointer is up, so dragging a selection never writes.
    fn remember_the_marks(&mut self, settled: bool) {
        for index in 0..self.files.len() {
            let Some(path) = self.files.at(index).path().map(Path::to_path_buf) else {
                continue;
            };
            let revision = self.files.at(index).document.revision();
            if self.files.at(index).marked_revision == Some(revision) {
                continue;
            }
            let marks = self.files.at(index).document.highlights().clone();
            self.marks.set(&path, marks);
            self.files.at_mut(index).marked_revision = Some(revision);
        }
        if self.remembers_this_project() && settled {
            let root = self.tree.root().to_path_buf();
            self.marks.save(&root);
        }
    }

    /// True when this window is the one allowed to read and write the project's own `.quill` folder.
    ///
    /// Which is the released binary and nothing else: `restore_project` is what turns it on, and a
    /// test neither reads nor writes a person's files.
    fn remembers_this_project(&self) -> bool {
        self.written_project.is_some()
    }

    /// What the menus need to know about the window.
    pub fn menu_state(&self) -> MenuState {
        MenuState {
            can_undo: self.document().can_undo(),
            can_redo: self.document().can_redo(),
            has_selection: !self.document().selection().is_empty(),
            recent: self.recent.clone(),
            view_mode: self.view_mode(),
            can_preview: file_kind::preview_applies(self.document().path()),
            preview_kind: file_kind::preview_kind(self.document().path()),
            explorer_visible: self.explorer_visible,
            line_numbers: self.settings.line_numbers,
            terminal_visible: self.terminal.visible,
            terminal_tabs: self.terminal.tabs.count(),
            open_files: self.files.len(),
            panes: self.files.pane_count(),
            pane: self.files.focused_pane(),
            tabs_in_pane: self.files.tabs_in(self.files.focused_pane()).len(),
            in_repository: self.git.is_some(),
            has_file: self.document().path().is_some(),
            annotated: self.files.active().blame.is_some(),
            unfinished: self.git.as_ref().and_then(|git| git.snapshot.in_progress),
            highlights: self.document().highlights().len(),
            on_a_highlight: self.marks_under_the_caret(),
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
                    // A window of its own, which is what `Recent Projects` already did and what
                    // `task-1658` asks for: a project is a window, so opening a second one keeps the
                    // first. Only if a second process cannot be started does the folder take this
                    // window, which is better than the entry doing nothing at all.
                    if !launcher::open_window(&folder) {
                        self.open_folder(&folder);
                    }
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
            Action::GoToFile => {
                // The folder is read again first, so a file made since the window opened is in the
                // list. It is one walk of the project, on a key press rather than on every frame,
                // and a finder that cannot find a file you made a minute ago is not a finder.
                self.tree.reload();
                self.go_to_file = Some(GoToFile::default());
            }
            Action::FindInFiles => {
                // The folder is read again first, for the reason `Go to File` reads it: a file made
                // since the window opened is part of this project and has to be searched.
                self.tree.reload();
                self.find_in_files = Some(FindInFiles::open(self.thread_waker()));
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
            Action::SaveAs if self.files.active().is_picture() => {
                self.message =
                    Some("A picture cannot be edited, so there is nothing to save.".to_owned());
            }
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
                self.remember_the_project();
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
            Action::ChangeFontSize { larger } => {
                // On a tab showing a picture the same keys zoom the picture. `task-1658` asks for
                // control and plus to zoom an image, and one shortcut meaning "make what I am looking
                // at bigger" is what a person expects of it.
                let area = self.editor_area.size();
                match self.files.active_mut().picture.as_mut() {
                    Some(picture) => picture.step_zoom(larger, area),
                    None => {
                        // About the caret, which is what a person zooming with the keyboard is
                        // looking at. `task-1672`.
                        self.anchor_the_view_at_the_caret();
                        self.set_font_size(settings::step_font_size(self.settings.font_size, larger))
                    }
                }
            }
            Action::ResetFontSize => match self.files.active_mut().picture.as_mut() {
                Some(picture) => picture.fit(),
                None => {
                    self.anchor_the_view_at_the_caret();
                    self.set_font_size(settings::DEFAULT_FONT_SIZE)
                }
            },
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
            // The panes. Each is one call on `OpenFiles`, which is where the rules about panes live,
            // so a split made from a menu and a split made from the command line are the same split.
            Action::SplitRight => {
                self.files.split_right();
                self.focus = Focus::Editor;
            }
            Action::MoveTabRight => {
                if !self.files.move_tab(true) {
                    self.message = Some("There is no pane to the right of this one.".to_owned());
                }
            }
            Action::MoveTabLeft => {
                if !self.files.move_tab(false) {
                    self.message = Some("There is no pane to the left of this one.".to_owned());
                }
            }
            Action::Unsplit => {
                if !self.files.unsplit() {
                    self.message = Some("The editing area is not split.".to_owned());
                }
            }
            Action::UnsplitAll => {
                if !self.files.unsplit_all() {
                    self.message = Some("The editing area is not split.".to_owned());
                }
            }
            Action::NextPane => {
                self.files.next_pane();
                self.focus = Focus::Editor;
            }
            Action::PreviousPane => {
                self.files.previous_pane();
                self.focus = Focus::Editor;
            }
            Action::SelectOpenFile => self.select_the_open_file(),
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
            Action::ReloadPath(path) => {
                self.reload_from_disk(&path, false);
            }
            Action::Git(what) => self.run_git(what),
            Action::Highlight(colour) => {
                self.highlight_selection(colour.rgba());
            }
            Action::ClearHighlight => {
                if !self.clear_highlight_here() {
                    self.message = Some("There is no highlight at the caret.".to_owned());
                }
            }
            Action::ClearHighlights => {
                let cleared = self.document().highlights().len();
                if self.clear_highlights_here() {
                    self.message = Some(format!(
                        "Cleared {cleared} highlight{}",
                        if cleared == 1 { "" } else { "s" }
                    ));
                }
            }
            Action::About => {
                // One modal at a time, which is what opening any of the others does.
                self.close_every_modal();
                self.about = Some(About::current());
            }
        }
    }

    /// Open a terminal tab if there is not one already, which is what showing the tile does.
    fn open_terminal_tab(&mut self) {
        if self.terminal.tabs.is_empty() {
            self.new_terminal_tab();
        }
    }

    /// Start another terminal, in the folder the explorer is showing, running the shell the settings
    /// name — or this machine's own when they name none.
    pub fn new_terminal_tab(&mut self) {
        let rows = self.terminal_rows();
        let cell = self.renderer.cell_metrics(self.settings.terminal_font_size);
        let size = quill_terminal::session::Size::new(rows, 80).with_cell(cell.width, cell.height);
        self.terminal.tabs.settings.shell = self.settings.shell();
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
    ///
    /// What was open in the project being left is written down first, because after this the window no
    /// longer knows which project its tabs belonged to. What the *new* project had open is deliberately
    /// not restored here: this is also the path `Open File` takes when the file chosen is outside the
    /// folder that is open, and quietly closing somebody's tabs and opening a different set because they
    /// opened one file elsewhere would be a surprise. A project's state is restored when a window opens
    /// on it, which is what `File -> Open Folder` now does.
    pub fn open_folder(&mut self, folder: &Path) {
        self.remember_the_project();
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

    /// The one place a file is loaded, whether it is text or a picture. `permanent` decides whether it
    /// takes a tab of its own or reuses the transient one; [`files::OpenFiles::open`] decides what that
    /// means.
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
        // A picture is a tab of its own kind. It is read here rather than in `files`, so that the one
        // place a file is opened stays the one place a file is opened.
        if file_kind::is_image(path) {
            self.files.open_file(files::OpenFile::picture(path), permanent);
            self.message = None;
            self.forget_layout();
            return;
        }
        match Document::open(path) {
            Ok(mut document) => {
                document.apply(Command::MoveDocumentStart { extend: false });
                self.files.open(document, permanent);
                let change = self.settings.as_style_change();
                self.document_mut().set_base_style(change);
                // Whatever was marked in this file last time. The document clamps the ranges to the
                // text it has just read, so a file that changed on the disk since is a mark in the
                // wrong place rather than a range past the end of the rope.
                if let Some(marks) = self.marks.highlights(path).cloned() {
                    self.document_mut().set_highlights(marks);
                }
                let revision = self.document().revision();
                self.files.active_mut().marked_revision = Some(revision);
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

    /// Open a file at a match found by `Find in Files`, with the match itself selected.
    ///
    /// Selecting it rather than only putting the caret there is what the ticket asks for when it
    /// says the result should highlight the matching spot in the document: a selection is how a
    /// document shows a piece of itself, and it is the same highlight a search inside a file leaves.
    fn open_the_match(&mut self, path: &Path, range: std::ops::Range<usize>) {
        // A tab of its own, because choosing a line out of a list of matches is not glancing.
        self.open_path_permanently(path);
        if self.files.active().path() != Some(path) {
            return; // it would not open, and `open_path_permanently` has already said why
        }
        // The offsets came from the file on disk. A tab that was already open and has been edited
        // since is a different text, so a range that runs past its end is left alone rather than
        // selecting the wrong thing.
        let length = self.document().text().len_bytes();
        if range.end > length {
            self.message = Some(format!(
                "{} has changed since it was searched, so the match could not be shown.",
                path.display()
            ));
            return;
        }
        self.document_mut().apply(Command::PlaceCaret { offset: range.start, extend: false });
        self.document_mut().apply(Command::PlaceCaret { offset: range.end, extend: true });
        // The file is nearly always taller than the editing area, so the match has to be scrolled to
        // as well as selected.
        self.reveal_caret = true;
        self.focus = Focus::Editor;
    }

    /// Start working on the repository the project is in, if it is in one.
    ///
    /// Called when a window opens and again when it is pointed at another folder, because the second
    /// folder may be a different repository, or none.
    pub fn open_repository(&mut self) {
        let waker = self.thread_waker();
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
        for pane in 0..self.files.pane_count() {
            if let Some(index) = self.files.showing_in(pane) {
                self.colour_the_file(index);
            }
        }
    }

    /// Colour one file. One in each pane is asked about every frame, because every pane is drawing.
    fn colour_the_file(&mut self, index: usize) {
        // The text revision. Keyed on the revision, this re-tokenised the whole file and rebuilt
        // every style span on every frame in which the caret moved, which is every frame of dragging
        // a selection. See `tasks/task-1666-performance-tdd.md` section 2.
        let revision = self.files.at(index).document.text_revision();
        if self.files.at(index).coloured_revision == Some(revision) {
            return;
        }
        let Some(path) = self.files.at(index).path().map(Path::to_path_buf) else {
            self.files.at_mut(index).coloured_revision = Some(revision);
            return;
        };
        let Some(plugin) = self.plugins.for_path(&path) else {
            self.files.at_mut(index).coloured_revision = Some(revision);
            return;
        };
        let base = quill_core::Color::rgb(color::TEXT.r(), color::TEXT.g(), color::TEXT.b());
        let text = self.files.at(index).document.text().to_string();
        if text.len() > Self::COLOUR_LIMIT {
            self.message = Some(format!(
                "{} is too large to colour, so it is shown as plain text.",
                path.display()
            ));
            self.files.at_mut(index).coloured_revision = Some(revision);
            return;
        }
        let theme = plugin.theme.clone();
        let spans: Vec<(std::ops::Range<usize>, quill_core::Color)> =
            quill_core::syntax::highlight(&text, &plugin.grammar)
                .into_iter()
                .filter_map(|(range, token)| theme.colour(token).map(|colour| (range, colour)))
                .collect();
        let file = self.files.at_mut(index);
        file.document.set_syntax(base, &spans);
        // `set_syntax` bumps the revision, so what is remembered is the revision *after* it, or the
        // next frame would colour it all over again for ever.
        file.coloured_revision = Some(file.document.text_revision());
        file.cached.stale = true;
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
                for file in self.files.iter_mut() {
                    file.coloured_revision = None;
                }
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

    /// What a thread calls to have the window drawn again when it has an answer.
    ///
    /// Two threads use it: the one that runs git, and the one `Find in Files` searches on. A reply
    /// arriving while the window is idle has to ask for the frame itself, or it sits unseen until
    /// the pointer next moves.
    fn thread_waker(&self) -> std::sync::Arc<dyn Fn() + Send + Sync> {
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
                // The same entry shuts it again, because the rail's git button is this action and a
                // button that only ever opens something is a button you can press once.
                if git.panel.open {
                    git.panel.open = false;
                } else {
                    git.panel.open();
                    git.send(Request::Log { path: None, limit: git::HISTORY_LIMIT });
                }
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
    pub fn reload_from_disk(&mut self, path: &Path, discard: bool) -> bool {
        self.tree.reload();
        let Some(index) = self.files.index_of(path) else {
            self.message = Some(format!("Reloaded {}", path.display()));
            return true;
        };
        // A tab with unsaved changes is not reloaded, because reading the file again would throw
        // them away and there is no undo for that. The explorer's own `Reload from Disk` never
        // discards; the command line can ask to, because a script that means it has no way to say
        // so through a menu.
        if !discard && self.files.get(index).is_some_and(|file| file.document.is_modified()) {
            self.message =
                Some(format!("{} has unsaved changes, so it was not reloaded", path.display()));
            return false;
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
                    file.forget_where_it_was_being_read();
                }
                if index == self.files.active_index() {
                    self.forget_layout();
                }
                self.message = Some(format!("Reloaded {}", path.display()));
                true
            }
            Err(problem) => {
                self.message =
                    Some(format!("Quill could not reload {}: {problem}", path.display()));
                false
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

    /// Throw away what was laid out for the tab that is showing, because a different document has
    /// taken it.
    fn forget_layout(&mut self) {
        let file = self.files.active_mut();
        file.forget_what_was_worked_out();
        file.preview_scroll = 0.0;
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
        // A tab showing a picture holds an empty document over the picture's path, so saving it would
        // write nothing over the file. There is nothing in a picture Quill can change, so there is
        // nothing to save.
        if self.files.active().is_picture() {
            self.message = Some("A picture cannot be edited, so there is nothing to save.".to_owned());
            return;
        }
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
        &self.files.active().cached.preview_layout
    }

    /// The preview's text, for a test that wants to check what the parser produced.
    pub fn preview_text(&self) -> String {
        self.files.active().cached.preview.as_ref().map(|p| p.text.to_string()).unwrap_or_default()
    }

    /// The pictures the preview is drawing, for a test.
    pub fn preview_pictures(&self) -> &[PlacedPicture] {
        &self.files.active().cached.preview_pictures
    }

    /// Work the preview out again if the source or the width changed.
    ///
    /// The preview is produced by `quill_core::markdown`, which turns the source into the same three
    /// things a document holds, so the ordinary layout engine and the ordinary painter draw it. Nothing
    /// here knows how to render Markdown.
    ///
    /// Pictures are the one thing that takes two passes. `markdown` says which paragraph stands in
    /// for a picture and what file it names, but how tall that paragraph has to be depends on how
    /// wide the pane is and on how large the picture turns out to be — neither of which that crate
    /// can know, because it has no window and cannot decode an image. So the pictures are read here,
    /// each one asks its paragraph to be at least as tall as it is drawn, and only then is the
    /// preview laid out.
    fn refresh_preview(&mut self, ctx: &egui::Context, width: f32) {
        // The text revision, for the reason `refresh_layout` records: the preview is built from the
        // source, and moving the caret does not change the source.
        let revision = self.document().text_revision();
        let cached = &self.files.active().cached;
        if cached.preview.is_some()
            && !cached.stale
            && revision == cached.preview_revision
            && (width - cached.preview_width).abs() < 0.5
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
        let mut preview = quill_core::markdown::render(
            &self.document().text().to_string(),
            &base,
            colors,
            self.renderer.monospaced_family(),
        );
        let pictures = self.read_the_pictures(ctx, &mut preview, width);
        let diagrams = self.lay_the_diagrams_out(ctx, &mut preview, width);
        let laid = layout(
            &preview.text,
            &preview.chars,
            &preview.paragraphs,
            &self.renderer,
            width,
        );
        let cached = &mut self.files.active_mut().cached;
        cached.preview_pictures = pictures;
        cached.preview_diagrams = diagrams;
        cached.preview_layout = laid;
        cached.preview = Some(preview);
        cached.preview_revision = revision;
        cached.preview_width = width;
    }

    /// Read every picture the preview names, and give each one's paragraph the room it needs.
    ///
    /// A picture is drawn at its own size, or scaled down to the width of the pane when it is wider
    /// than that — never blown up, which is what `services::picture` decided for a picture in a tab
    /// and is what "fit" means to anybody. A picture that will not decode leaves its paragraph the
    /// height of a line of text and its alt text is drawn there instead, which is what the preview
    /// did before there were pictures at all.
    fn read_the_pictures(
        &mut self,
        ctx: &egui::Context,
        preview: &mut quill_core::Preview,
        width: f32,
    ) -> Vec<PlacedPicture> {
        let folder = self
            .document()
            .path()
            .and_then(|path| path.parent())
            .map(std::path::Path::to_path_buf);
        let mut placed = Vec::new();
        for image in preview.images.clone() {
            let ready = self.preview_images.ready(ctx, folder.as_deref(), &image.source);
            let size = match &ready {
                Some(ready) => {
                    let (pixels_across, pixels_down) =
                        (ready.size[0] as f32, ready.size[1] as f32);
                    let scale = if pixels_across > 0.0 { (width / pixels_across).min(1.0) } else { 1.0 };
                    Vec2::new(pixels_across * scale, pixels_down * scale)
                }
                None => Vec2::ZERO,
            };
            if size.y > 0.0 {
                let room = size.y + PICTURE_GAP;
                preview
                    .paragraphs
                    .set(image.paragraph..image.paragraph + 1, |style| style.min_height = room);
            }
            placed.push(PlacedPicture {
                paragraph: image.paragraph,
                size,
                texture: ready.map(|ready| ready.texture),
                alt: image.alt.clone(),
            });
        }
        placed
    }

    /// Lay every diagram the preview names out, and give each one's paragraph the room it needs.
    ///
    /// Exactly the two passes the pictures take, and for the same reason: `quill_core::markdown`
    /// cannot know how wide the pane is, so it says where a diagram goes and this works out how tall
    /// it turns out to be. A diagram wider than the pane is scaled down to fit — never blown up —
    /// which is what `fit` means everywhere else in Quill.
    ///
    /// **A diagram that will not draw keeps its room and says why.** Losing the whole document
    /// because one fence has a typo in it would be far worse than a panel where a picture should be,
    /// and the panel names the line so the typo can be found.
    fn lay_the_diagrams_out(
        &mut self,
        ctx: &egui::Context,
        preview: &mut quill_core::Preview,
        width: f32,
    ) -> Vec<PlacedDiagram> {
        // The plugin decides whether a diagram is drawn at all. With it switched off, a mermaid
        // fence stays the code it was before `task-1660`, in the same frame.
        if preview.diagrams.is_empty() || !self.mermaid_is_enabled() {
            return Vec::new();
        }
        let base = self.diagram_style();
        let theme = crate::services::mermaid_scene::theme();
        let metrics = crate::services::mermaid_scene::EguiMetrics::new(ctx, self.bold_family.clone());
        let mut placed = Vec::with_capacity(preview.diagrams.len());
        for diagram in preview.diagrams.clone() {
            let laid = self.mermaid_scenes.scene(&diagram.source, &base, &metrics, &theme);
            let size = match &laid {
                Ok(scene) if scene.size.width > 0.0 => {
                    let scale = (width / scene.size.width).min(1.0);
                    Vec2::new(scene.size.width * scale, scene.size.height * scale)
                }
                // A problem panel takes a fixed height: enough for the reason and a few lines of the
                // source under it.
                _ => Vec2::new(width, PROBLEM_HEIGHT),
            };
            if size.y > 0.0 {
                let room = size.y + PICTURE_GAP;
                preview
                    .paragraphs
                    .set(diagram.paragraph..diagram.paragraph + 1, |style| style.min_height = room);
            }
            placed.push(PlacedDiagram {
                paragraph: diagram.paragraph,
                size,
                laid,
                source: diagram.source.clone(),
            });
        }
        placed
    }

    /// The family and the size a diagram's text is set in.
    ///
    /// The **size** follows the editor's, so a diagram grows and shrinks with command and plus
    /// exactly as the Markdown preview does.
    ///
    /// The **family** is the one `theme::install_fonts` put into egui, which is not necessarily the
    /// editor's. A diagram is the one thing in the window that is measured by `quill-core` and drawn
    /// by `egui`, and those two have to be looking at the same face or a box comes out the wrong size
    /// for the words in it. Measuring in the settings font while drawing in egui's left a requirement
    /// diagram's fields hanging over the right edge of their boxes, which is what the screenshot
    /// showed and what no assertion about the scene could have caught.
    fn diagram_style(&self) -> quill_core::CharStyle {
        quill_core::CharStyle {
            family: self.renderer.default_family(),
            size: self.document().active_style().size * 0.9,
            ..quill_core::CharStyle::default()
        }
    }

    /// Switch a plugin on or off, and undo whatever it was doing to the window.
    ///
    /// The one place it happens, so the two things that have to follow always do: the open file may
    /// have just gained or lost its colours, and it may have just gained or lost its diagram. Doing
    /// them here rather than in the settings dialog is what makes `quill-cli plugins disable` and
    /// the tick box in `Plugins` mean exactly the same thing.
    pub fn set_plugin_enabled(&mut self, id: &str, on: bool) {
        self.plugins.set_enabled(self.store.as_ref(), id, on);
        // Every open file, not only the one showing: a plugin is a setting for the window, and with
        // panes there is more than one file being drawn.
        for file in self.files.iter_mut() {
            file.coloured_revision = None;
            // The preview is thrown away rather than kept, because whether a mermaid fence is a
            // picture or a piece of code has just changed and the preview is built from that answer.
            file.cached.preview = None;
            file.cached.preview_diagrams.clear();
        }
        self.mermaid_scenes.forget();
    }

    /// How many diagrams have been laid out and kept, for a test.
    pub fn mermaid_scene_count(&self) -> usize {
        self.mermaid_scenes.len()
    }

    /// Whether the Mermaid plugin is switched on.
    ///
    /// Asked before a diagram is laid out and before a `.mmd` file is drawn as one, so switching the
    /// plugin off in `Plugins` withdraws every diagram in the window in the same frame. That is what
    /// makes it a plugin rather than a feature with a plugin painted on it.
    pub fn mermaid_is_enabled(&self) -> bool {
        self.plugins.renders("mermaid")
    }

    /// The diagrams the preview is drawing, for a test.
    pub fn preview_diagrams(&self) -> &[PlacedDiagram] {
        &self.files.active().cached.preview_diagrams
    }

    /// Lay the file that is showing out, if the text, the formatting or the width changed since the
    /// last time.
    ///
    /// What was worked out is kept on the tab rather than on the window, so each pane's file is laid
    /// out at that pane's width and nothing is laid out twice a frame. See `files::Cached`.
    fn refresh_layout(&mut self, width: f32) {
        // The **text** revision, not the revision. Moving the caret bumps the revision, so keying
        // this on it laid the whole document out again on every frame of dragging a selection — 82 ms
        // a frame on a file the size of `app/mod.rs`. See `tasks/task-1666-performance-tdd.md`
        // section 2.
        let revision = self.document().text_revision();
        let cached = &self.files.active().cached;
        if !cached.stale
            && revision == cached.laid_out_revision
            && (width - cached.laid_out_width).abs() < 0.5
        {
            return;
        }
        // What was laid out last time is handed over rather than thrown away: `relayout` keeps every
        // paragraph whose text and formatting are unchanged, so typing a letter costs the paragraph
        // it was typed into instead of the file.
        let previous = std::mem::take(&mut self.files.active_mut().cached.layout);
        let laid = relayout(
            previous,
            self.document().text(),
            self.document().chars(),
            self.document().paragraphs(),
            &self.renderer,
            width,
        );
        let cached = &mut self.files.active_mut().cached;
        cached.stale = false;
        cached.layout = laid;
        cached.laid_out_revision = revision;
        cached.laid_out_width = width;
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
        self.zoom_taken = false;
        self.zoom_offered_to_the_keyboard = false;
        // Before anything is drawn, so that what a command asked for is in the frame about to be
        // painted and therefore in the next screenshot.
        self.pump_control(ui.ctx());
        self.ask_git_about_the_open_file();
        self.colour_the_open_file();
        // Before the explorer is drawn, so the folders it needs are already open on this frame.
        self.follow_the_open_file();
        let full = ui.max_rect();

        // The window is one painted surface with rounded corners, because it has no operating system
        // title bar. Everything else is drawn on top of it.
        ui.painter().rect_filled(
            full,
            CornerRadius::same(size::WINDOW_CORNER),
            theme::faded(color::EDITOR, self.settings.opacity),
        );

        let title_rect = Rect::from_min_size(full.min, Vec2::new(full.width(), size::TITLE_BAR));
        // The text tools live at the right hand end of the title bar rather than in a strip of their
        // own. How much room they want depends on the open file, and the title bar leaves exactly that
        // much clear — but the bar's own height never changes, so switching from a `.md` file to a
        // `.rs` one no longer moves the tabs and the editing area up and down by forty four points.
        let tools_width = text_tools::width(self.document().path());
        let tools_rect = title_bar::tools_rect(title_rect, self.menu_placement, tools_width);
        let status_rect = Rect::from_min_size(
            Pos2::new(full.left(), full.bottom() - size::STATUS_BAR),
            Vec2::new(full.width(), size::STATUS_BAR),
        );
        let body = Rect::from_min_max(
            Pos2::new(full.left(), title_rect.bottom()),
            Pos2::new(full.right(), status_rect.top()),
        );

        // The rail of pane buttons takes the far left of the body, the whole way down, so the terminal
        // button sits at the bottom left corner of the window as `task-1658`'s capture shows it.
        let rail_rect =
            Rect::from_min_size(body.min, Vec2::new(size::ACTIVITY_BAR, body.height()));
        let panes = Rect::from_min_max(Pos2::new(rail_rect.right(), body.top()), body.max);

        // The terminal takes the bottom of the panes across their whole width, as it does in IntelliJ,
        // and the explorer and the editing area share what is left.
        let terminal_height = if self.terminal.visible {
            self.panes
                .terminal_height
                .clamp(settings::TERMINAL_MIN, (panes.height() - 120.0).max(settings::TERMINAL_MIN))
        } else {
            0.0
        };
        let upper = Rect::from_min_max(
            panes.min,
            Pos2::new(panes.right(), panes.bottom() - terminal_height),
        );
        let terminal_rect =
            Rect::from_min_max(Pos2::new(panes.left(), upper.bottom()), panes.max);

        let explorer_width = if self.explorer_visible {
            self.panes.explorer_width.clamp(settings::EXPLORER_MIN, settings::EXPLORER_MAX)
        } else {
            0.0
        };
        let explorer_rect =
            Rect::from_min_size(upper.min, Vec2::new(explorer_width, upper.height()));
        let editing_area =
            Rect::from_min_max(Pos2::new(upper.left() + explorer_width, upper.top()), upper.max);

        // The menus, which the title bar draws when they are not in the screen's own bar.
        let menus = actions::menus(&self.menu_state());
        let mut action = None;

        // The title bar.
        let outcome = title_bar::show(
            ui,
            title_rect,
            self.folder_name().as_deref(),
            self.settings.opacity,
            self.menu_placement,
            &menus,
            tools_width,
        );
        if outcome.close {
            self.closing = true;
            self.write_settings();
            self.remember_the_project();
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

        // The text tools, drawn over the right hand end of the title bar. After the bar rather than
        // before it, because the bar takes drags over the room between the menus and the buttons to move
        // the window, and a control added earlier would sit underneath that and never be pressed.
        if tools_width > 0.0 {
            let tools_outcome = {
                let mut tools_ui = ui.new_child(egui::UiBuilder::new().max_rect(tools_rect));
                text_tools::show(
                    &mut tools_ui,
                    tools_rect,
                    self.document(),
                    &self.bold_family,
                    self.view_mode(),
                )
            };
            for command in tools_outcome.commands {
                self.document_mut().apply(command);
            }
            if let Some(mode) = tools_outcome.view_mode {
                self.set_view_mode(mode);
            }
        }

        // The rail of pane buttons down the far left.
        {
            let state = activity_bar::RailState {
                explorer_visible: self.explorer_visible,
                git_open: self.git.as_ref().is_some_and(|git| git.panel.open),
                in_repository: self.git.is_some(),
                terminal_visible: self.terminal.visible,
            };
            let opacity = self.settings.opacity;
            let chosen = {
                let mut rail_ui = ui.new_child(egui::UiBuilder::new().max_rect(rail_rect));
                activity_bar::show(&mut rail_ui, rail_rect, state, opacity)
            };
            if let Some(chosen) = chosen {
                action = Some(chosen);
            }
        }

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
                // True for the two frames after the file that is showing changed, which is when the
                // list scrolls to it. Counted down rather than left on, because it is a one shot: a
                // person who closed the folder holding the open file closed it deliberately.
                let reveal = self.reveal_in_explorer > 0;
                self.reveal_in_explorer = self.reveal_in_explorer.saturating_sub(1);
                if self.reveal_in_explorer > 0 {
                    // The second frame has to actually happen, and an idle window draws nothing.
                    ui.ctx().request_repaint();
                }
                // Worked out for every row before the explorer is drawn, because decoding an icon
                // needs the context mutably and the explorer already has the window borrowed.
                let rows: Vec<PathBuf> = if self.filter.trim().is_empty() {
                    self.tree.rows().iter().map(|row| row.entry.path.clone()).collect()
                } else {
                    self.tree.matching(&self.filter).iter().map(|path| path.to_path_buf()).collect()
                };
                // A map rather than a list. It used to be searched for each row as the row was
                // drawn, comparing paths, so a project with four hundred rows open did a hundred and
                // sixty thousand path comparisons every frame.
                let decorations: std::collections::HashMap<PathBuf, explorer::Decoration> = rows
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
                    decorations.get(path).cloned().unwrap_or_default()
                };
                let mut explorer_ui = ui.new_child(egui::UiBuilder::new().max_rect(explorer_rect));
                explorer::show(
                    &mut explorer_ui,
                    explorer_rect,
                    &self.tree,
                    &mut self.filter,
                    open.as_deref(),
                    unsaved,
                    reveal,
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

        // The panes: a strip of tabs and an editing area each, left to right. One pane is the
        // ordinary case and takes the same path as any other number.
        //
        // The loop **borrows the focus**: `files.active()` answers with the pane being drawn for as
        // long as it is being drawn, which is what the four hundred lines of `show_editor` and
        // `show_preview` already mean by it, so nothing had to have a pane index threaded through
        // it. Two things must not follow the borrowed focus and are passed in instead — the keyboard,
        // or every pane would take the same key presses and draw a caret, and `editor_area`, which
        // the status bar reads on the frame after. See `tasks/task-1664-split-view-tdd.md` §6.
        let pane_rects = self.pane_rects(editing_area);
        let had_the_keyboard = self.files.focused_pane();
        let mut keyboard = had_the_keyboard;
        // A close can remove a pane and renumber the ones after it, so it is done once the loop has
        // finished rather than underneath itself.
        let mut close: Option<usize> = None;
        for (pane, rect) in pane_rects.iter().copied().enumerate() {
            self.files.focus_pane(pane);
            if self.show_pane(ui, pane, pane == had_the_keyboard, rect, &mut close) {
                keyboard = pane;
            }
        }
        self.files.focus_pane(keyboard);
        if let Some(index) = close {
            self.close_tab(index);
        }
        // A gesture nobody was pointing at belongs to the pane being typed into. Here rather than
        // in the loop, because a pane earlier in the row must not take a gesture aimed at one
        // later in it, and which pane the pointer is over is not known until they are all drawn.
        if !self.zoom_taken && self.zoom_offered_to_the_keyboard {
            self.zoom_taken = true;
            self.zoom_the_text(ui, 0.0);
        }

        // The dividers between the panes, added after every pane for the reason
        // `components::splitter` records: the editing area takes drags over the whole of its
        // rectangle, so a divider added earlier sits underneath one and never sees the pointer.
        for pane in 0..pane_rects.len().saturating_sub(1) {
            let edge = Rect::from_min_size(
                Pos2::new(pane_rects[pane].right(), pane_rects[pane].top()),
                Vec2::new(1.0, pane_rects[pane].height()),
            );
            let name = format!("pane-{pane}");
            let drag = splitter::show(ui, edge, &name, splitter::Axis::Upright);
            if drag.delta != 0.0 && editing_area.width() > 1.0 {
                let smallest = (size::EDITOR_PANE_MIN / editing_area.width()).min(0.45);
                self.files.move_divider(pane, drag.delta / editing_area.width(), smallest);
            }
            if drag.reset {
                self.files.reset_pane_widths();
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

        // A tab's own menu, drawn after the panes so it sits over them rather than under one.
        if let Some((at, _)) = self.tab_menu {
            let entries = actions::tab_menu(&self.menu_state());
            let outcome = context_menu::show(ui, "tab", at, &entries);
            if let Some(chosen) = outcome.chosen {
                action = Some(chosen);
            }
            if outcome.close {
                self.tab_menu = None;
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

        // The editing area's own menu, which is where a passage is marked. Drawn after the editing
        // area for the same reason the other two are: it has to sit over what it was opened on.
        if let Some(menu) = self.text_menu.clone() {
            let state = self.menu_state();
            let above = actions::text_menu(&state);
            let below = actions::clear_highlight_menu(&state);
            let last = self.last_highlight;
            let outcome =
                text_menu::show(ui, &menu, &above, &below, state.has_selection, last);
            if let Some(chosen) = outcome.chosen {
                action = Some(chosen);
            }
            if let Some(color) = outcome.highlight {
                self.highlight_selection(color);
            }
            if let Some(wheel) = outcome.wheel {
                if let Some(menu) = self.text_menu.as_mut() {
                    menu.wheel = wheel;
                }
                if let Some(color) = wheel {
                    self.last_highlight = color;
                }
            }
            if outcome.close {
                self.text_menu = None;
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

        // The status bar. A picture has no caret and no font, so it says how big it is and how far it
        // is zoomed instead.
        let style = self.document().active_style();
        let branch = self.git.as_ref().and_then(|git| git.status_label());
        let picture = self.editor_area.size();
        let (position, detail) = match self.files.active().picture.as_ref() {
            Some(picture_in_the_tab) => (None, picture_in_the_tab.description(picture)),
            None => (
                Some(self.caret_position()),
                format!("{} \u{00B7} {:.0} pt", style.family, style.size),
            ),
        };
        status_bar::show(
            ui,
            status_rect,
            &status_bar::Status {
                name: &self.file_name(),
                unsaved: self.document().is_modified(),
                kind: file_kind::kind_name(self.document().path()),
                position,
                detail: &detail,
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

        // `Go to File`, drawn after the prompt for the same reason the prompt is drawn after the git
        // windows: the newest thing a person asked for belongs on top of the older ones.
        if let Some(mut finder) = self.go_to_file.take() {
            finder.refresh(self.tree.root(), self.tree.all_files());
            let outcome = go_to_file::show(ui.ctx(), &mut finder);
            if let Some(path) = outcome.open {
                // A tab of its own, not the transient one: choosing a file out of a list of file
                // names is not glancing at it.
                self.open_path_permanently(&path);
                self.focus = Focus::Editor;
            }
            if !outcome.close {
                self.go_to_file = Some(finder);
            }
        }

        // `Find in Files`, which is drawn beside `Go to File` because they are the same kind of
        // thing: a question about the project, asked over the top of it.
        if let Some(mut find) = self.find_in_files.take() {
            find.pump(self.tree.all_files());
            let outcome = find_in_files::show(ui.ctx(), &mut find, self.panes.find_split);
            if outcome.drag != 0.0 {
                // The divider is dragged in points and the split is a fraction, because the modal
                // can be resized: a fraction keeps the two panes in proportion when it is.
                let height = outcome.panes_height.max(1.0);
                self.panes.find_split = (self.panes.find_split + outcome.drag / height)
                    .clamp(find_in_files::SPLIT_MIN, find_in_files::SPLIT_MAX);
                self.unsaved_settings = true;
            }
            if outcome.reset_split {
                self.panes.find_split = find_in_files::SPLIT;
                self.unsaved_settings = true;
            }
            if let Some((path, range)) = outcome.open {
                self.open_the_match(&path, range);
            }
            if !outcome.close {
                self.find_in_files = Some(find);
            }
        }

        // The About box. Only one modal is open at a time — `Action::About` shuts whatever was —
        // so where it is drawn among the others decides nothing; it is here because it is the same
        // kind of thing as the two above, a small window over the project rather than about a file.
        if let Some(about) = self.about.take() {
            if !about_dialog::show(ui.ctx(), &about) {
                self.about = Some(about);
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
            self.set_plugin_enabled(&id, on);
        }
        if settings_outcome.changed || self.settings != before {
            self.apply_settings(&before);
        }

        // The eight places the window itself is resized from, added last so they sit over every pane:
        // the editing area, the explorer and the status bar all take drags over the whole of their
        // rectangles, and a grip added earlier would never see a pointer. See `components::resize_edges`
        // for why they exist at all, which is that Quill's window has no operating system frame.
        if let Some(direction) = resize_edges::show(ui, full) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }

        if let Some(chosen) = action {
            self.run_action(chosen, ui.ctx());
        }

        // What is open in this project is written down for next time, on the same terms as the
        // settings: once the pointer is up, and only when something has actually changed.
        self.remember_the_project();
        // And what is marked in its files, on exactly the same terms.
        let settled = !ui.input(|input| input.pointer.any_down());
        self.remember_the_marks(settled);

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
            self.set_the_font_everywhere();
        }
        self.unsaved_settings = true;
    }

    /// A pinch on the trackpad, or the wheel with the zoom modifier held, over the editing area.
    ///
    /// `zoom_delta` reports both as one multiplier, which is IntelliJ's control and mouse wheel for
    /// nothing, and egui holds the scroll back while the modifier is down so the document does not
    /// slide about while it is being zoomed.
    ///
    /// It walks the same sizes the Settings window offers and the keyboard steps through, rather
    /// than setting whatever size the multiplier works out to. Two reasons. A size the dialog cannot
    /// show is a size a person cannot get back to, and one step per notch of a wheel is what every
    /// other editor does.
    ///
    /// The gesture is accumulated rather than applied a frame at a time, because it arrives as a
    /// stream of multipliers a fraction over one: a step is taken each time what has been asked for
    /// reaches [`ZOOM_STEP`], and the remainder is carried into the next frame, so a slow pinch and
    /// a fast one both end up where the fingers say. Nothing here needs to know what one notch of a
    /// wheel is worth in points, which is a platform's business and differs between mice.
    ///
    /// `above` is how far below the top of the view the point the gesture is about sits: where the
    /// pointer is, or the top of the view when the gesture arrived with the pointer somewhere else.
    /// Whatever text is there is what the zoom is not allowed to move, which is what `task-1672`
    /// asks for — a person zooming in is zooming in on the line they are looking at, and having to
    /// scroll back to it afterwards is the whole complaint.
    ///
    /// What comes out is written to the settings file once the pointer is up rather than on every
    /// frame, by the rule `ui` already keeps for a dragged divider.
    fn zoom_the_text(&mut self, ui: &egui::Ui, above: f32) {
        let gesture = ui.input(|input| input.zoom_delta());
        if (gesture - 1.0).abs() < f32::EPSILON {
            return;
        }
        self.zoom_pending *= gesture;
        let mut steps = 0i32;
        while self.zoom_pending >= ZOOM_STEP {
            self.zoom_pending /= ZOOM_STEP;
            steps += 1;
        }
        while self.zoom_pending <= 1.0 / ZOOM_STEP {
            self.zoom_pending *= ZOOM_STEP;
            steps -= 1;
        }
        if steps == 0 {
            return;
        }
        // Counted first and taken afterwards, so the point that is to stay put is read off the
        // layout as it is now — before `set_font_size` marks it stale — however many sizes one
        // frame of the gesture turns out to be worth.
        self.anchor_the_view(above);
        for _ in 0..steps.abs() {
            self.set_font_size(settings::step_font_size(self.settings.font_size, steps > 0));
        }
    }

    /// Remember the text `above` points below the top of the editing area, so the zoom can put it
    /// back there once the file has been laid out at its new size.
    ///
    /// Set before the size changes, and before [`Self::set_the_font_everywhere`] takes the top of
    /// the view for every other file, so this one — the pane being zoomed — keeps the point the
    /// person is actually looking at.
    fn anchor_the_view(&mut self, above: f32) {
        let file = self.files.active_mut();
        let at = file.cached.layout.anchor_at_y(file.scroll + above);
        file.zoom_anchor = Some(files::ViewAnchor { at, above });
    }

    /// The same, for a zoom from the keyboard, which has no pointer to be about.
    ///
    /// The caret is what a person is working on, so that is the point kept still — clamped into the
    /// view, so a caret that is off the top or the bottom of the window anchors the edge nearest it
    /// rather than scrolling the file to somewhere nobody asked to be.
    fn anchor_the_view_at_the_caret(&mut self) {
        let view_height = (self.editor_area.height() - size::EDITOR_PADDING_Y * 2.0).max(0.0);
        let file = self.files.active();
        let caret = file.cached.layout.caret_at(file.document.selection().head);
        let above = (caret.y - file.scroll).clamp(0.0, view_height);
        self.anchor_the_view(above);
    }

    /// Scroll a view back to the place it was anchored at before the font changed.
    ///
    /// Called on the frame the file is laid out again, after `refresh_layout` and before the scroll
    /// position is read for anything else, so the wheel and the caret still have the last word in
    /// the ordinary way. Clamped to what there is to scroll, because a larger font can leave a file
    /// that overflowed the window no longer overflowing it.
    fn keep_the_place_through_a_zoom(&mut self, view_height: f32) {
        let file = self.files.active_mut();
        let Some(anchor) = file.zoom_anchor.take() else {
            return;
        };
        let overflow = (file.cached.layout.height - view_height).max(0.0);
        file.scroll =
            (file.cached.layout.y_of_anchor(anchor.at) - anchor.above).clamp(0.0, overflow);
    }

    /// The same for the Markdown preview, which scrolls on its own and is laid out from the same
    /// base style, so a change of size moves it in exactly the same way.
    fn keep_the_previews_place_through_a_zoom(&mut self, view_height: f32) {
        let file = self.files.active_mut();
        let Some(anchor) = file.preview_anchor.take() else {
            return;
        };
        let overflow = (file.cached.preview_layout.height - view_height).max(0.0);
        file.preview_scroll =
            (file.cached.preview_layout.y_of_anchor(anchor.at) - anchor.above).clamp(0.0, overflow);
    }

    /// Set the editor's font size, from the keyboard or from a pinch.
    ///
    /// The one setting the Settings window holds, so a size reached with the keyboard is the size
    /// the dialog shows, reaches every open tab, and is written to the settings file and still there
    /// next time Quill starts — which is what a person means by zooming an editor rather than
    /// zooming a view of it.
    ///
    /// Public so a test can drive it without pressing keys.
    pub fn set_font_size(&mut self, size: f32) {
        let size = size.clamp(settings::MIN_FONT_SIZE, settings::MAX_FONT_SIZE);
        if (self.settings.font_size - size).abs() < 0.01 {
            return;
        }
        self.settings.font_size = size;
        self.set_the_font_everywhere();
        self.unsaved_settings = true;
    }

    /// Show every open file in the font the settings name.
    ///
    /// The editor's font is one setting for the whole window, the way IntelliJ has one editor font,
    /// so a change reaches every tab rather than only the one that happens to be showing. It used to
    /// reach `document_mut()` alone, which meant that opening three files and then changing the font
    /// left two of them in the old one until Quill was restarted, and left the Markdown preview in
    /// it as well, because the preview is laid out from the source's own base style.
    ///
    /// This is not an edit: it pushes nothing onto any document's undo history and marks no file as
    /// having unsaved changes, because what Quill saves is plain text and carries no formatting.
    fn set_the_font_everywhere(&mut self) {
        let change = self.settings.as_style_change();
        // Before anything is changed, because an anchor describes the layout the reader can still
        // see. Every file rather than the one showing, for the same reason the font itself reaches
        // every file: the other tabs are laid out again too, and a tab that came back scrolled
        // somewhere else would be a tab that had moved while nobody was looking at it.
        for file in self.files.iter_mut() {
            file.anchor_the_views();
        }
        for file in self.files.iter_mut() {
            file.document.set_base_style(change.clone());
        }
        // Every file has to be laid out again, and every preview thrown away so it is built from
        // the new base style rather than the one it was made with. Every file rather than only the
        // one showing, which is what this used to do: with panes there is more than one on the
        // screen, and a cache now belongs to the tab it describes.
        for file in self.files.iter_mut() {
            file.cached.stale = true;
            file.cached.preview = None;
        }
        // The frame that puts each view back where it was anchored is the frame after this one, and
        // an idle window draws nothing — the last notch of a gesture would otherwise be left
        // showing the text at its new size in the old place until something else woke the window.
        if let Some(context) = &self.context {
            context.request_repaint();
        }
    }

    /// Where each pane goes, left to right, from the shares the panes were left at.
    ///
    /// The last pane is taken to the right hand edge rather than measured, so rounding cannot leave a
    /// hairline of the window showing down the far side.
    fn pane_rects(&self, area: Rect) -> Vec<Rect> {
        let widths = self.files.pane_widths().to_vec();
        let mut out = Vec::with_capacity(widths.len());
        let mut left = area.left();
        for (at, share) in widths.iter().enumerate() {
            let right = if at + 1 == widths.len() {
                area.right()
            } else {
                (left + area.width() * share).floor()
            };
            out.push(Rect::from_min_max(
                Pos2::new(left, area.top()),
                Pos2::new(right.max(left), area.bottom()),
            ));
            left = right;
        }
        if out.is_empty() {
            out.push(area);
        }
        out
    }

    /// Draw one pane: its strip of tabs, and the editing area under it.
    ///
    /// `focused` says whether this is the pane with the keyboard, which is **not** the same question
    /// as which pane `files` is focused on while this runs — see the note in `ui`. Returns true when
    /// the pane was clicked in, which is what moves the keyboard to it.
    ///
    /// A close is reported rather than done, because closing a tab can empty a pane and renumber the
    /// ones after it, and the loop this is called from is walking those numbers.
    fn show_pane(
        &mut self,
        ui: &mut egui::Ui,
        pane: usize,
        focused: bool,
        area: Rect,
        close: &mut Option<usize>,
    ) -> bool {
        let tabs_rect =
            Rect::from_min_size(area.min, Vec2::new(area.width(), file_tabs::HEIGHT));
        let editor_rect =
            Rect::from_min_max(Pos2::new(area.left(), tabs_rect.bottom()), area.max);
        let mut took_the_keyboard = false;
        // Everything in the pane is drawn into a `Ui` of its own, carrying the pane's number as its
        // id salt. egui identifies a widget by its id, and every control inside asks for one with
        // `ui.id().with(...)` — so without this the gutters of two panes, or their editing areas, or
        // two previews, would be one widget as far as egui is concerned and one click would reach
        // both. The alternative was passing a pane number into five components; one salt does it for
        // everything, including whatever is added to a pane later.
        let ui = &mut ui.new_child(
            egui::UiBuilder::new().max_rect(area).id_salt(("editor-pane", pane)),
        );

        // The tabs in this pane, in the order they are drawn. The strip counts within itself, so what
        // it reports is turned back into an index into the open files here.
        let indices = self.files.tabs_in(pane);
        let icons: Vec<Option<egui::TextureHandle>> = indices
            .iter()
            .map(|index| self.files.at(*index).path().map(std::path::Path::to_path_buf))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|path| self.plugin_icon(ui.ctx(), path.as_deref()))
            .collect();
        let tabs: Vec<TabView> = indices
            .iter()
            .zip(icons)
            .map(|(index, icon)| {
                let file = self.files.at(*index);
                TabView {
                    name: file.name(),
                    modified: file.document.is_modified(),
                    transient: file.transient,
                    marker: file.path().map(theme::file_marker).unwrap_or(color::FILE_TEXT),
                    icon,
                }
            })
            .collect();
        let active = self
            .files
            .showing_in(pane)
            .and_then(|showing| indices.iter().position(|index| *index == showing))
            .unwrap_or(0);
        let opacity = self.settings.opacity;
        let outcome = {
            let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect));
            file_tabs::show(&mut tabs_ui, tabs_rect, &tabs, active, pane, focused, opacity)
        };
        let at = |within: usize| indices.get(within).copied();
        if let Some(index) = outcome.show.and_then(at) {
            self.show_tab(index);
            self.focus = Focus::Editor;
            took_the_keyboard = true;
        }
        if let Some(index) = outcome.keep.and_then(at) {
            self.show_tab(index);
            self.files.make_permanent(index);
            self.focus = Focus::Editor;
            took_the_keyboard = true;
        }
        if let Some(index) = outcome.close.and_then(at) {
            *close = Some(index);
        }
        if let Some((within, where_)) = outcome.menu {
            // The tab is shown first, so every entry in the menu can be about "the tab that is
            // showing" and so be an action with no argument. See `actions::tab_menu`.
            if let Some(index) = at(within) {
                self.show_tab(index);
                self.focus = Focus::Editor;
                took_the_keyboard = true;
            }
            self.tab_menu = Some((where_, pane));
        }

        // The editing area: the picture, the source, the preview, or both side by side.
        took_the_keyboard |= self.show_editing_area(ui, editor_rect, focused);
        took_the_keyboard
    }

    /// Keep the explorer's selection on the file that is showing.
    ///
    /// Derived from the state rather than fired from each of the places a tab can change — there are
    /// eleven of those today and the twelfth, added next month, would be the one that forgot. It
    /// costs one comparison a frame.
    fn follow_the_open_file(&mut self) {
        let showing = self.files.active().path().map(Path::to_path_buf);
        if showing == self.revealed {
            return;
        }
        self.revealed = showing.clone();
        let Some(path) = showing else {
            return;
        };
        // Opening out the folders above it, so there is a row to select at all. `expand` walks the
        // components and opens each folder; the file itself is not a folder, so it is left alone.
        self.tree.expand(&path);
        self.reveal_in_explorer = REVEAL_FRAMES;
    }

    /// Show the explorer, scrolled to the file that is showing. `View -> Select Opened File`.
    fn select_the_open_file(&mut self) {
        self.explorer_visible = true;
        if let Some(path) = self.files.active().path().map(Path::to_path_buf) {
            self.tree.expand(&path);
        }
        // The filter box is what the explorer draws instead of the tree, and a file that does not
        // match it has no row to scroll to, so asking to be shown where a file is clears it.
        self.filter.clear();
        self.reveal_in_explorer = REVEAL_FRAMES;
    }

    /// Draw whatever the open tab holds into `area`.
    ///
    /// A picture, the Markdown source, the preview, or the source and the preview side by side with a
    /// draggable divider between them. Split out of [`Self::ui`] because it is the one place the four
    /// answers are chosen between, and `ui` has enough to do laying the window out.
    fn show_editing_area(&mut self, ui: &mut egui::Ui, area: Rect, focused: bool) -> bool {
        if self.files.active().is_picture() {
            if focused {
                self.editor_area = area;
            }
            return self.show_picture(ui, area);
        }
        match self.view_mode() {
            ViewMode::Raw => return self.show_editor(ui, area, focused),
            ViewMode::Preview => {
                if focused {
                    self.editor_area = area;
                }
                self.show_preview(ui, area);
            }
            ViewMode::SideBySide => {
                let fraction = self.panes.preview_fraction.clamp(0.15, 0.85);
                let split = (area.width() * fraction).floor();
                let left = Rect::from_min_size(area.min, Vec2::new(split, area.height()));
                let right = Rect::from_min_max(
                    Pos2::new(area.left() + split, area.top()),
                    area.max,
                );
                let took = self.show_editor(ui, left, focused);
                self.show_preview(ui, right);
                // The split between the source and the preview is a pane like any other, so it is dragged.
                let edge = Rect::from_min_size(
                    Pos2::new(right.left(), right.top()),
                    Vec2::new(1.0, right.height()),
                );
                let drag = splitter::show(ui, edge, "preview", splitter::Axis::Upright);
                if drag.delta != 0.0 && area.width() > 0.0 {
                    self.panes.preview_fraction =
                        (fraction + drag.delta / area.width()).clamp(0.15, 0.85);
                    self.unsaved_settings = true;
                }
                if drag.reset {
                    self.panes.preview_fraction = 0.5;
                    self.unsaved_settings = true;
                }
                return took;
            }
        }
        false
    }

    /// Draw the picture the open tab holds, and take the gestures that move and zoom it.
    fn show_picture(&mut self, ui: &mut egui::Ui, area: Rect) -> bool {
        let name = self.files.active().name();
        let Some(picture) = self.files.active_mut().picture.as_mut() else {
            return false;
        };
        let outcome = picture_view::show(ui, area, picture, &name);
        if outcome.take_focus {
            self.focus = Focus::Editor;
        }
        outcome.take_focus
    }

    /// Draw whatever the open file's preview is: a Markdown page, or a drawn diagram.
    ///
    /// One function rather than branches spread through `show_editing_area`, so that the three view
    /// modes are the same three modes whichever kind of file is open and `SideBySide` needs no
    /// special case of its own at all.
    fn show_preview(&mut self, ui: &mut egui::Ui, area: Rect) {
        if file_kind::is_mermaid(self.document().path()) {
            self.show_diagram(ui, area);
            return;
        }
        self.show_markdown_preview(ui, area);
    }

    /// Draw the whole file as one diagram, which is what a `.mmd` file's preview is.
    fn show_diagram(&mut self, ui: &mut egui::Ui, area: Rect) {
        if !self.mermaid_is_enabled() {
            let problem = quill_core::mermaid::Problem::whole(
                "The Mermaid plugin is switched off, so this file is not drawn as a diagram. Switch it on in Plugins.",
            );
            diagram_view::show_problem(ui, area, &problem, "");
            return;
        }
        let source = self.document().text().to_string();
        let base = self.diagram_style();
        let theme = crate::services::mermaid_scene::theme();
        let metrics =
            crate::services::mermaid_scene::EguiMetrics::new(ui.ctx(), self.bold_family.clone());
        let laid = self.mermaid_scenes.scene(&source, &base, &metrics, &theme);
        let name = self.files.active().name();
        match laid {
            Ok(scene) => {
                // Taken apart by field, because the view has to be borrowed mutably while the window
                // is still needed for the focus that follows.
                let Self { files, .. } = self;
                let view = &mut files.active_mut().diagram;
                let outcome = diagram_view::show(ui, area, &scene, view, &name);
                if outcome.take_focus {
                    self.focus = Focus::Editor;
                }
            }
            Err(problem) => diagram_view::show_problem(ui, area, &problem, &source),
        }
    }

    /// Draw the Markdown preview into `area`. It is read only, so it has no caret and no selection: there
    /// is nothing to type into, because what is shown is worked out from the source.
    fn show_markdown_preview(&mut self, ui: &mut egui::Ui, area: Rect) {
        let response = ui.interact(area, ui.id().with("preview"), egui::Sense::hover());
        let text_width = (area.width() - size::EDITOR_PADDING_X * 2.0).max(50.0);
        let ctx = ui.ctx().clone();
        self.refresh_preview(&ctx, text_width);
        self.keep_the_previews_place_through_a_zoom(area.height() - size::EDITOR_PADDING_Y * 2.0);

        // The preview scrolls on its own, so reading the rendered page does not move the caret.
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        if wheel != 0.0 && response.hovered() {
            self.files.active_mut().preview_scroll -= wheel;
        }
        let overflow =
            (self.preview_layout().height - (area.height() - size::EDITOR_PADDING_Y * 2.0)).max(0.0);
        let scroll = self.files.active().preview_scroll.clamp(0.0, overflow);
        self.files.active_mut().preview_scroll = scroll;

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - scroll,
        );
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
        editor_view::paint_text(&painter_ui, &self.renderer, self.preview_layout(), origin);
        self.paint_the_pictures(&painter_ui, origin);
        self.paint_the_diagrams(&painter_ui, origin, text_width);
    }

    /// Draw the pictures into the room their paragraphs were given.
    ///
    /// Drawn after the text rather than before it, so a picture cannot be hidden behind the letters
    /// of the empty line it sits on, and at the left of the text where every other block starts.
    fn paint_the_pictures(&self, ui: &egui::Ui, origin: Pos2) {
        let painter = ui.painter();
        for picture in self.preview_pictures() {
            let Some(line) =
                self.preview_layout().lines.iter().find(|line| line.paragraph == picture.paragraph)
            else {
                continue;
            };
            let at = Pos2::new(origin.x, origin.y + line.y);
            match &picture.texture {
                Some(texture) => {
                    let rect = Rect::from_min_size(at, picture.size);
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    // Nothing to draw, so say what should have been there. The same words the
                    // preview showed for a picture before it could draw one.
                    let words = if picture.alt.trim().is_empty() {
                        "a picture that could not be read".to_owned()
                    } else {
                        picture.alt.clone()
                    };
                    let galley = painter.layout_no_wrap(
                        words,
                        egui::FontId::proportional(13.0),
                        color::TEXT_DIM,
                    );
                    painter.galley(at, galley, color::TEXT_DIM);
                }
            }
        }
    }

    /// Draw the diagrams into the room their paragraphs were given.
    ///
    /// After the text, like the pictures, so a diagram cannot be hidden behind the letters of the
    /// empty line it sits on.
    fn paint_the_diagrams(&self, ui: &egui::Ui, origin: Pos2, width: f32) {
        for diagram in self.preview_diagrams() {
            let Some(line) =
                self.preview_layout().lines.iter().find(|line| line.paragraph == diagram.paragraph)
            else {
                continue;
            };
            let at = Pos2::new(origin.x, origin.y + line.y);
            match &diagram.laid {
                Ok(scene) => {
                    let scale =
                        if scene.size.width > 0.0 { (width / scene.size.width).min(1.0) } else { 1.0 };
                    diagram_view::paint(ui, scene, at, scale);
                }
                Err(problem) => {
                    let panel = Rect::from_min_size(at, Vec2::new(width, diagram.size.y));
                    diagram_view::show_problem(ui, panel, problem, &diagram.source);
                }
            }
        }
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

    /// Draw the source of the file that is showing into `area`.
    ///
    /// `focused` is whether this pane has the keyboard. Only that pane draws a caret and only that
    /// pane reads the keyboard, or every pane would take the same key presses. Returns true when the
    /// pane was clicked in, which is what moves the keyboard to it.
    fn show_editor(&mut self, ui: &mut egui::Ui, area: Rect, focused: bool) -> bool {
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

        if focused {
            // Only the focused pane, because this is what the status bar and `editor status` read on
            // the frame after and they mean the pane that is being typed into.
            self.editor_area = area;
        }
        let response = ui.interact(area, ui.id().with("editor"), egui::Sense::click_and_drag());
        let mut took_the_keyboard = false;
        if response.clicked() || response.drag_started() || response.secondary_clicked() {
            self.focus = Focus::Editor;
            took_the_keyboard = true;
        }
        // Over the writing, the pointer is a vertical bar rather than an arrow, which is what it is in
        // every editor and what `task-1658` asks for. Only over the text itself: the gutter is a
        // rectangle of its own and the divider beside it sets its own pointer, and both are drawn after
        // this, so the last one to speak wins where they overlap.
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        let has_keyboard = self.focus == Focus::Editor && focused;
        let text_width = (area.width() - padding - size::EDITOR_PADDING_X).max(50.0);
        self.refresh_layout(text_width);
        // Straight after the layout, so the rest of the frame — the wheel, the caret, the painter
        // — sees the scroll position the zoom asked for rather than the one it was left at.
        self.keep_the_place_through_a_zoom(area.height() - size::EDITOR_PADDING_Y * 2.0);

        let scroll = self.files.active().scroll;
        let origin = Pos2::new(area.left() + padding, area.top() + size::EDITOR_PADDING_Y - scroll);
        // Taken apart by field, because the input handlers want the document mutably while the
        // layout they measure against is borrowed at the same time, and a method on `self` would
        // borrow the whole window. Both now live on the same tab, and the two are separate fields of
        // it, which is a borrow the compiler allows through one reference.
        let file = self.files.active_mut();
        let laid = &file.cached.layout;
        let document = &mut file.document;
        let pointer_changed = editor_view::handle_pointer(&response, document, laid, origin);
        let outcome = editor_view::handle_input(ui, document, laid, has_keyboard);
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

        // A right click opens the editing area's own menu. Inside a selection it leaves the
        // selection alone — a menu that opened with nothing selected would be a menu with nothing
        // to mark in it, which is the whole point of it — and anywhere else it puts the caret there
        // first, which is what every editor does.
        if response.secondary_clicked() {
            if let Some(at) = response.interact_pointer_pos() {
                let local = at - origin;
                let offset = self.layout().offset_at(local.x, local.y);
                let selection = self.document().selection().range();
                if !selection.contains(&offset) {
                    self.document_mut().apply(Command::PlaceCaret { offset, extend: false });
                }
                self.text_menu = Some(text_menu::TextMenu::new(at, offset));
                self.focus = Focus::Editor;
            }
        }

        // A pinch, or the wheel with the zoom modifier held, changes the size of the text that is
        // being worked on: either the pointer is demonstrably over this pane, or this is the pane
        // with the keyboard and no pane has the pointer.
        //
        // Neither of those is `response.hovered()`, and it took measuring the real window to find
        // out why it must not be. A two notch gesture produced thirty eight frames, eleven of them
        // carrying a zoom — and on every one of those eleven `hovered()` was false and
        // `pointer.hover_pos()` was `None`, because egui reports no pointer at all on a frame whose
        // only input is a wheel event. Gating on either alone threw the whole gesture away and the
        // text never moved, which is exactly what the first version of this did. So the last place
        // the pointer was seen is asked for as well, which is what `latest_pos` is, and it is what
        // says which pane a gesture with no pointer on this frame is still about.
        let pointer = ui
            .input(|input| input.pointer.hover_pos().or_else(|| input.pointer.latest_pos()))
            .filter(|at| area.contains(*at));
        if !self.zoom_taken {
            match pointer {
                // Over this pane, so the gesture is this pane's, about the text it is over.
                Some(at) => {
                    self.zoom_taken = true;
                    let top = area.top() + size::EDITOR_PADDING_Y;
                    self.zoom_the_text(ui, (at.y - top).max(0.0));
                }
                // Not over this pane. The pane with the keyboard takes it at the end of the frame
                // if no pane turns out to have the pointer, keeping the top of its view still,
                // because there is then no point on the screen for the gesture to be about.
                None if has_keyboard => self.zoom_offered_to_the_keyboard = true,
                None => {}
            }
        }

        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        let mut scroll = self.files.active().scroll;
        if wheel != 0.0 && response.hovered() {
            scroll -= wheel;
        }
        if outcome.scroll_to_caret || (self.reveal_caret && focused) {
            let caret = self.layout().caret_at(self.document().selection().head);
            let view_height = area.height() - size::EDITOR_PADDING_Y * 2.0;
            if caret.y < scroll {
                scroll = caret.y;
            } else if caret.y + caret.height > scroll + view_height {
                scroll = caret.y + caret.height - view_height;
            }
        }
        if focused {
            self.reveal_caret = false;
        }
        let overflow =
            (self.layout().height - (area.height() - size::EDITOR_PADDING_Y * 2.0)).max(0.0);
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
                self.layout(),
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
            self.layout(),
            origin,
            editor_view::PaintStyle {
                selection: color::TEXT_SELECTION,
                caret: color::ACCENT,
                show_caret: has_keyboard,
            },
        );
        took_the_keyboard
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

    /// Write the settings, the pane sizes and what was open in the project before the window goes.
    fn on_exit(&mut self) {
        self.write_settings();
        self.remember_the_project();
    }
}
