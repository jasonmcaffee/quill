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
pub mod completion;
pub mod debug;
pub mod files;
pub mod folding;
pub mod git;
pub mod symbols;

use std::collections::HashMap;

use crate::services::symbol_index::Indexer;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Rect, Vec2};
use quill_core::{layout, relayout, Command, Document, Highlights, Layout, Rgba};

use crate::components::about_dialog::{self, About};
use crate::components::activity_bar;
use crate::components::context_menu;
use crate::components::debug_dialogs::{self, BreakpointDialog, EvaluateDialog};
use crate::components::debug_panel::{self, DebugPanel};
use crate::components::diagram_view;
use crate::components::editor_view;
use crate::components::explorer;
use crate::components::file_tabs::{self, TabView};
use crate::components::find_in_files::{self, FindInFiles};
use crate::components::references::References;
use crate::components::git_dialogs::{self, Dialog};
use crate::components::git_panel;
use crate::components::go_to_file::{self, GoToFile};
use crate::components::gutter::{self, Gutter};
use crate::components::picture_view;
use crate::components::prompt_dialog::{self, Prompt, Purpose};
use crate::components::resize_edges;
use crate::components::run_dialog::{self, RunDialog};
use crate::components::run_panel::{self, RunPanel};
use crate::components::run_widget;
use crate::components::scrollbar;
use crate::components::settings_dialog::{self, SettingsWindow};
use crate::components::splitter;
use crate::components::status_bar;
use crate::components::terminal_panel::{self, TerminalPanel};
use crate::components::text_menu;
use crate::components::text_tools;
use crate::components::title_bar::{self, MenuPlacement};
use crate::app::debug::{Built, DebugState, PendingBuild};
use crate::services::breakpoint_store::BreakpointStore;
use crate::services::debuggers;
use crate::services::file_kind;
use crate::services::file_marks::FileMarks;
use crate::services::file_move;
use crate::services::imports;
use crate::services::recycle;
use crate::services::run_configurations::{self, Configuration, Origin, RunConfigurations};
use crate::services::file_tree::FileTree;
use crate::services::file_clipboard::FileClipboard;
use crate::services::launcher;
use crate::services::locators;
use crate::services;
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

use actions::{Action, DebugAction, GitAction, MenuState, RunAction};
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

/// How far a code panel is drawn outside the text it sits behind.
const PANEL_PADDING: f32 = 6.0;

/// How much air is left under a picture in the Markdown preview.
///
/// The same idea as the space between two paragraphs: a picture with the next line of prose against
/// its bottom edge reads as part of the picture.
const PICTURE_GAP: f32 = 14.0;

/// A question with two answers, and what to do when it is answered.
///
/// The dialog knows nothing about what it is asking. What is held is the [`Answer`] — the thing to
/// do when the button is pressed — so a seventh question can be added without the one confirmation
/// dialog learning a seventh thing.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirmation {
    pub title: String,
    pub note: String,
    /// The word on the button that does it.
    pub button: String,
    pub answer: Answer,
}

/// What confirming a question does.
///
/// Two, because everything Quill used to ask about first was something git could not undo, and
/// `task-1681` added the one thing that is not: throwing a file away.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// Send this to the git thread.
    Git(quill_git::worker::Request),
    /// Delete this path, wherever `services::recycle` puts a deleted file on this platform.
    Delete(PathBuf),
    /// Stop this run configuration and take it away, which is what `Remove` in the run dialog does
    /// to one that is still running. The question is asked because silently killing a server
    /// somebody is watching is worse than one extra click.
    RemoveRun(String),
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
    /// The explorer. The arrow keys walk the tree and `Delete` throws a file away.
    ///
    /// `task-1681` added it, because `Delete` cannot mean "throw this file away" while the editing
    /// area has the keys — there it means "take away the letter in front of the caret". A single
    /// click on a row leaves the keyboard here, which is what VS Code does and what makes `Down`
    /// `Down` `Down` a way to look through a folder; a double click hands it to the editor.
    Explorer,
    /// The terminal. Typing goes to the program running in it, and Tab and Escape go with it.
    Terminal,
}

/// A tab being carried by the pointer.
///
/// `task-1673` asks for IntelliJ's two: rearranging the tabs in a pane, and dragging a tab from one
/// pane into another. Both are one gesture, and where it ends is not a question a strip of tabs can
/// answer — each pane draws its own strip and has never heard of the others, while the pointer
/// wanders freely between them. So the strip reports **that** a tab is being carried and where the
/// pointer is, and `QuillApp::settle_the_tab_drag` decides where it landed once every pane has said
/// where its own tabs are.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TabDrag {
    /// Which open file is in the air, as an index into `files`.
    file: usize,
    /// Where the pointer is now.
    at: Pos2,
    /// It was let go on this frame.
    dropped: bool,
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

/// True while a modal is open, which is what makes a key press the modal's rather than a pane's.
///
/// The other half of the same problem [`text_box_has_the_keyboard`] answers, and it needed its own
/// answer once `task-1682` gave Enter a meaning in every modal: a confirmation, an about box and
/// most of the git dialogs have no field in them, so nothing had the focus and the editing area,
/// the terminal and the explorer all went on reading the frame's keys behind the dialog. `Enter` in
/// the delete confirmation would then have deleted the file **and** opened the row under the
/// explorer's cursor.
///
/// egui's own modal layer is what is asked, rather than a list of Quill's dialogs, so a modal added
/// later is covered without being added anywhere. It is the layer as it stood at the **end of the
/// last frame**, which is the honest answer at the point in a frame these three read the keyboard —
/// before anything is drawn, and so before this frame's modal has said it is there.
pub fn a_modal_has_the_keyboard(ctx: &egui::Context) -> bool {
    ctx.memory(|memory| memory.top_modal_layer().is_some())
}

/// The name of the widget that holds egui's keyboard focus while a pane of Quill's own is being typed
/// into.
pub const KEYBOARD_HOLDER: &str = "quill-keyboard-holder";

/// How long the window will sleep before it wakes itself up, whether or not anything has asked it to.
///
/// Quill draws only when something happens: a key press, the pointer, a program writing to a
/// terminal, a thread finishing its work. When none of those is happening it asks for no frame and
/// the operating system puts the main thread to sleep until an event arrives. Everything that wakes
/// it from another thread — the command line, the git worker, the symbol indexer, every terminal —
/// does so through egui's `request_repaint`, which on macOS signals a source on the main run loop.
///
/// A window was found in the state where that wake had stopped arriving. It was visible on the
/// screen, it was using no processor time, its main thread was asleep waiting for an event, no lock
/// was held by anybody and nothing was deadlocked, a command from the command line was queued with
/// its connection still waiting for the answer, and not one frame had been drawn in three seconds.
/// It could not be typed into and it could not be dragged. It drew a single frame each time the
/// operating system pushed something at it, such as being activated or the desktop it was on being
/// switched to. Nothing had crashed; the window was asleep and the wake never came.
///
/// The window therefore no longer depends on being woken. Every frame asks for another one half a
/// second later, so there is always a timer pending, and a wake that goes missing costs half a second
/// instead of the window. The cost of that is two frames a second while nothing is happening.
///
/// The wake this schedules is a different mechanism from the one that went missing. Asking for a frame
/// after a delay makes winit wait on a timer, which the run loop fires from inside itself; waking from
/// another thread signals a source and hopes the sleep breaks. The timer is the one that can be
/// counted on, which is why the heartbeat is a delay rather than a thread calling `request_repaint`.
pub const HEARTBEAT: std::time::Duration = std::time::Duration::from_millis(500);

/// How long an adapter search is believed before it is done again.
///
/// It exists because the commonest thing to happen next, when there is no adapter, is an install
/// running in the tile beside the message that offered it — so the message has to notice. Five
/// seconds is short enough that a finished install is reflected while somebody is still looking at
/// it, and long enough that a few directory reads are not part of drawing a frame.
pub const ADAPTER_SEARCH_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// How often the folders that are showing in the explorer are asked whether they have changed.
///
/// `task-1693` reported that a file an agent made never appeared. Three quarters of a second is
/// quick enough that a file written by a program in the terminal tile below is in the tree before
/// anybody has looked away, and slow enough that the handful of `metadata` calls it costs are a few
/// dozen a second. See `FileTree::changed_on_disk`, which is where the argument for asking rather
/// than watching is written down.
///
/// It is asked at the top of a frame like everything else, and `HEARTBEAT` is what guarantees there
/// is a frame: an idle window still draws twice a second.
pub const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

/// Keep egui's keyboard focus on a widget of Quill's own, so that pressing `Tab` or an arrow key
/// cannot hand the keyboard to a button.
///
/// **This is what a report of Quill crashing while somebody typed turned out to be.** egui moves
/// keyboard focus when a bare `Tab` or a bare arrow key is pressed, and it moves it to the next
/// widget that can take focus — every control Quill draws with `Sense::click` can. The editing area,
/// the explorer and the terminal are not egui widgets and never held egui's focus, so the focus
/// walked out of the document and onto the first button in the window, which is `Close` in the title
/// bar. A button with the keyboard is pressed by `Space` and by `Enter`. One arrow key and a space
/// therefore closed the window, or minimised it if the focus had reached `Minimise` instead. Nothing
/// panicked and nothing was written down, because the window was asked to close in the ordinary way.
///
/// egui's answer to this is a focused widget that says which keys are its own, which is how a
/// `TextEdit` keeps `Tab` from moving the focus out of a text box. Quill has three surfaces that read
/// the keyboard themselves rather than through a widget, so one focusable widget stands for all three:
/// while it holds the focus, `Tab` and the arrows are claimed and egui moves the focus nowhere.
///
/// It senses no clicks, so `Space` and `Enter` cannot press it, and it has no size, so there is
/// nothing to draw and nothing to click on.
///
/// It gives the focus up while a text box or a modal has the keyboard, because there the keys really
/// are the widget's: a `Tab` in the commit message is the box's, and a modal's buttons are the only
/// things a key should reach while it is open. Anything else holding the focus — a button in the
/// toolbar, a row in the explorer, a file tab — is a focus that arrived by wandering, and the holder
/// takes it back on the frame after. That leaves the one frame between the `Tab` that moved the focus
/// and the holder taking it back, and a person's next key press is frames later.
///
/// A click on a text box is never stolen: on the frame it is clicked the holder already has the focus
/// and so asks for nothing, and the box's own request is the one that takes effect.
pub fn hold_the_keyboard(ui: &mut egui::Ui) {
    let id = egui::Id::new(KEYBOARD_HOLDER);
    if text_box_has_the_keyboard(ui.ctx()) || a_modal_has_the_keyboard(ui.ctx()) {
        ui.memory_mut(|memory| memory.surrender_focus(id));
        return;
    }
    let response = ui.interact(Rect::ZERO, id, egui::Sense::focusable_noninteractive());
    if ui.memory(|memory| memory.focused()) != Some(id) {
        response.request_focus();
    }
    // Claimed every frame rather than once: egui only lets a widget claim keys on a frame where it
    // both had the focus last frame and holds it now, so the frame it takes the focus on is a frame it
    // cannot yet claim anything on.
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            id,
            egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                // Escape is left alone: it hands the keyboard back, and the focus is taken again on
                // the frame after by the request above.
                escape: false,
            },
        );
    });
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
    /// The run tile along the bottom, which is the terminal tile's sibling: the window shows one of
    /// the two and never both, because two grids stacked take the editing area below the fold.
    pub run: RunPanel,
    /// The project's run configurations, as `.quill/run-configurations.conf` holds them plus
    /// whatever has been run without being kept. See `services::run_configurations`.
    pub run_configurations: RunConfigurations,
    /// The name the run widget has chosen, which is what `Run` with no name means everywhere.
    ///
    /// `None` until something is chosen or a project that remembered one is opened. It is
    /// per-person, so it lives in `workspace.conf` beside the terminal's flags rather than in the
    /// file the project shares.
    pub run_selected: Option<String>,
    /// The `Run Configurations` modal.
    pub run_dialog: RunDialog,
    /// Set when a configuration was added, edited or removed and the file has not been written yet.
    /// Written on the same terms as the settings: once the pointer is up.
    unsaved_run_configurations: bool,
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
    /// True while the Markdown preview holds the selection that `Copy` means.
    ///
    /// The preview must never take the keyboard: in the side-by-side view the source is being typed
    /// into and the preview is being read, and a click in the preview that stopped the caret working
    /// would be worse than having no selection at all. So `Focus` is left alone and this one flag
    /// says which of the two a copy is about — set by a press in a preview and cleared by a press in
    /// an editing area, which is what "the pane the pointer last pressed in" means.
    reading_preview: bool,
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
    /// The debug tile along the bottom, which is the run tile's sibling as the run tile is the
    /// terminal's: the window shows **one** of the three and never two, because two grids stacked
    /// take the editing area below the fold.
    pub debug_panel: DebugPanel,
    /// The session that is running, when one is. **One at most**: IntelliJ runs several and the
    /// first version of this does not, which is what keeps every pane of the tile free of a session
    /// chooser above it. See `app::debug`.
    pub debug: Option<DebugState>,
    /// The build that has to finish before a session can start, when there is one. `task-1692`:
    /// `cargo run` names a build tool, so Debug asks cargo what it built before it debugs anything.
    pub debug_build: Option<PendingBuild>,
    /// What was found the last time each adapter was looked for, and when. The search reads
    /// directories and a frame may not, so it is cached here — and because an install running in the
    /// tile beside it changes the answer, the cache **goes stale on its own** after
    /// [`ADAPTER_SEARCH_TTL`] rather than waiting to be told.
    pub debug_adapters: HashMap<String, (std::time::Instant, debuggers::Report)>,
    /// What was said about debugging while there was no session to hold it — which in practice is a
    /// failed build's compiler errors. `debug output` reads it before the session's own output, so
    /// the reason a session never started is in the place somebody would look for it.
    pub debug_output: Vec<String>,
    /// The project's breakpoints, as `.quill/breakpoints.conf` holds them. The authority for every
    /// file that is **not** open; a file that is open is owned by its document and pushed in here
    /// whenever it changes — the highlights' rule, unchanged. See `services::breakpoint_store`.
    pub breakpoints: BreakpointStore,
    /// The `Evaluate Expression` modal, when it is open.
    pub evaluate: Option<EvaluateDialog>,
    /// The `Edit Breakpoint` modal, when it is open.
    pub breakpoint_dialog: Option<BreakpointDialog>,
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
    /// Where the explorer's own menu is open, what it is about, and whether it was aimed at a row.
    ///
    /// The last of the four is `actions::Aim`: a right click in the empty space below the rows opens
    /// the project folder's menu with everything that is about a particular file dimmed.
    pub explorer_menu: Option<(Pos2, PathBuf, bool, actions::Aim)>,
    /// The row the explorer's own cursor is on, which is what `Delete` is about.
    ///
    /// Separate from the file that is showing, because the two are different questions: a click
    /// selects a row here and opens it there, and the arrow keys move this one without opening
    /// anything at all.
    pub selected: Option<PathBuf>,
    /// How many more frames the explorer should scroll to its own selection.
    reveal_selection: u8,
    /// When the folders that are showing were last asked whether they had changed on disk.
    ///
    /// See [`WATCH_INTERVAL`] and `QuillApp::notice_what_changed_on_disk`.
    last_watched: std::time::Instant,
    /// True while a row in the explorer is being carried, which is the one moment the tree must not
    /// be read again underneath the drag.
    dragging_a_row: bool,
    /// Where the window is now, read from egui once a frame.
    ///
    /// `None` until a frame has been drawn, so a window that has not been on the screen yet never
    /// writes a geometry over the one it was opened with. See `project_state`.
    window_place: Option<project_state::WindowPlace>,
    /// The text prompt, when one is open.
    pub prompt: Option<Prompt>,
    /// The `Go to File` modal, when it is open.
    pub go_to_file: Option<GoToFile>,
    /// The `Find in Files` modal, when it is open. It holds the thread the searching runs on, so
    /// shutting the modal is what stops that thread.
    pub find_in_files: Option<FindInFiles>,
    /// The references, candidates or rename modal, when one is open. Like `Find in Files` it holds
    /// the thread it searches on, so shutting it is what stops that thread.
    pub references: Option<References>,
    /// The project's definitions, and the thread they are read on.
    ///
    /// `None` until something asks a question about a symbol, so a window a unit test builds has no
    /// thread reading a folder behind it. See `app::symbols`.
    pub(crate) symbols: Option<Indexer>,
    /// What the index was last asked about: the project, how many files were in it, and how many
    /// plugins were switched on. A change to any of the three is what asks for another read.
    pub(crate) symbols_asked: Option<(PathBuf, usize, usize)>,
    /// Set when a file on the disk changed under the index — a save, a rename, a reload.
    pub(crate) symbols_stale: bool,
    /// The word under the pointer while the modifier is held, and where a click on it would go.
    /// Cached against the text revision and the word, so a resting pointer costs one comparison.
    pub(crate) hover: Option<symbols::Hover>,
    /// What the name being renamed resolved to, which is what decides how widely the change set is
    /// ticked by default. Taken when the modal opens, while the caret is still where the question
    /// was asked from.
    pub(crate) rename_kind: Option<quill_core::symbols::SymbolKind>,
    /// The file the rename was asked in, which is what "this file only" means.
    pub(crate) rename_here: Option<PathBuf>,
    /// How many of the rename's rows have had a default tick worked out for them.
    ///
    /// The search streams, so this counts up as batches land: what is ticked by default is the tail
    /// that has just arrived, and everything above it is left as it is — which after the first
    /// batch means it belongs to whoever has been clicking the boxes.
    pub(crate) rename_ticked_up_to: usize,
    /// Where the caret has been, so `Navigate Back` can go there. Travel history rather than state:
    /// bounded, and not written to disk.
    pub(crate) back: Vec<symbols::Place>,
    /// The mirror stack, pushed by `Navigate Back` and cleared by any new jump.
    pub(crate) forward: Vec<symbols::Place>,
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
    /// Which paragraph that menu was opened over, so its breakpoint entries are about the row under
    /// the pointer rather than about the caret — the rule the text menu and the terminal tab menu
    /// already follow. `None` means the caret's line, which is what the keyboard and the command
    /// line mean.
    pub gutter_menu_line: Option<usize>,
    /// The values painted at the ends of lines, worked out once per stop. See
    /// [`QuillApp::inline_values`].
    pub(crate) inline_cache: Option<InlineValues>,
    /// The temporary breakpoint `Run to Cursor` made, and where. Taken away at the next stop.
    ///
    /// Held here rather than on the session because it is a **breakpoint**, and every breakpoint in
    /// Quill lives where the text is — this is only the note that says which one to take back.
    pub(crate) run_to: Option<(PathBuf, usize)>,
    /// Where a tab's own menu is open, and which pane's strip it was opened on. Held here for the
    /// same reason the gutter's is.
    ///
    /// The entries in it all act on "the tab that is showing", which is what makes them ordinary
    /// parameterless actions the View menu and the command line can ask for too — so opening the
    /// menu shows the tab it was opened on first. The editing area's own menu already sets that
    /// precedent: a right click outside the selection puts the caret there before opening.
    pub tab_menu: Option<(Pos2, usize)>,
    /// Where a terminal tab's own menu is open, and which tab it was opened on. Held here for the
    /// same reason the other three are: a screenshot test cannot press the right mouse button.
    pub terminal_menu: Option<(Pos2, usize)>,
    /// The tab being carried by the pointer, while one is. Frame local: the strip reports the drag
    /// every frame it is held and the window settles it once every pane has drawn, because a tab
    /// picked up in one pane is very often dropped on another and no one strip can see them all.
    tab_drag: Option<TabDrag>,
    /// Where each pane's strip of tabs drew itself and its tabs, in pane order. Frame local, and
    /// rebuilt by the pane loop, which is the only thing that can know it.
    tab_strips: Vec<file_tabs::Strip>,
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
    /// The MCP endpoint this window hosts, when `mcp.enabled` is on. `None` in every window a test
    /// builds, exactly as the command channel is: a test must not open a port either.
    pub(crate) mcp: Option<services::mcp::Hosted>,
    /// Commands that have been accepted and are waiting for something — a painted frame, a shell, a
    /// search, git. See `app::cli`.
    pub(crate) cli_waiting: Vec<(control::Pending, cli::Waiting)>,
    /// The completion popup, when one is open. One at most, because it belongs to the pane with the
    /// keyboard — the same reasoning as the one `hover` and the one `references` modal. See
    /// `app::completion`.
    pub(crate) completion: Option<completion::CompletionState>,
    /// Where the popup hangs. Frame local, recorded by the pane that has the keyboard as it draws
    /// its caret, because the pane loop borrows the focus and no one else can know where that caret
    /// ended up on the screen. Read after the loop, which is where the popup is drawn.
    completion_anchor: Option<completion::CompletionAnchor>,
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
            run: RunPanel::new(),
            run_configurations: RunConfigurations::new(),
            run_selected: None,
            run_dialog: RunDialog::default(),
            unsaved_run_configurations: false,
            debug_panel: DebugPanel::new(),
            debug: None,
            debug_build: None,
            debug_output: Vec::new(),
            debug_adapters: HashMap::new(),
            breakpoints: BreakpointStore::new(),
            evaluate: None,
            breakpoint_dialog: None,
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
            selected: None,
            reveal_selection: 0,
            last_watched: std::time::Instant::now(),
            dragging_a_row: false,
            window_place: None,
            reveal_in_explorer: 0,
            editor_area: Rect::ZERO,
            zoom_pending: 1.0,
            zoom_taken: false,
            zoom_offered_to_the_keyboard: false,
            reading_preview: false,
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
            references: None,
            symbols: None,
            symbols_asked: None,
            symbols_stale: false,
            hover: None,
            rename_kind: None,
            rename_here: None,
            rename_ticked_up_to: 0,
            back: Vec::new(),
            forward: Vec::new(),
            about: None,
            reveal_caret: false,
            gutter_menu: None,
            gutter_menu_line: None,
            inline_cache: None,
            run_to: None,
            text_menu: None,
            tab_menu: None,
            terminal_menu: None,
            tab_drag: None,
            tab_strips: Vec::new(),
            last_highlight: theme::color::HIGHLIGHT_YELLOW,
            marks: FileMarks::new(),
            control: None,
            mcp: None,
            cli_waiting: Vec::new(),
            completion: None,
            completion_anchor: None,
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
        // The MCP endpoint is opened from here for the same reason the command channel is: a test
        // must not open a port or leave a listener behind when it ends. A window that never calls
        // this has no endpoint and is an ordinary window in every other respect.
        self.mcp = Some(services::mcp::Hosted::new());
        self.reconcile_mcp();
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
        // And that this project has a window open, which is what `task-1693` asks Quill to bring
        // back next time. A window that is already in the list writes nothing — `Store::open_windows`
        // records why.
        store.remember_open_window(self.tree.root());
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
        // And where the program is to stop, on exactly the same terms and for the same reason: a
        // breakpoint that appeared a frame after its file did would be a dot that arrived late.
        self.breakpoints = BreakpointStore::load(self.tree.root());
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
        // Where each tab was left. After the panes, because a tab has to be in its pane before the
        // place it was being read at means anything, and before the tab that was showing is chosen,
        // because showing one throws away what was laid out for it.
        //
        // A caret past the end of a file that has changed since is **clamped rather than refused**,
        // which is the rule this whole feature keeps: a project that opens with part of its state is
        // better than one that will not open.
        for (index, path) in state.open_files.iter().enumerate() {
            let Some(tab) = self.files.index_of(path) else {
                continue;
            };
            let caret = state.file_carets.get(index).copied().unwrap_or(0);
            let scroll = state.file_scrolls.get(index).copied().unwrap_or(0.0);
            let file = self.files.at_mut(tab);
            let end = file.document.text().len_bytes();
            file.document.apply(Command::PlaceCaret { offset: caret.min(end), extend: false });
            file.scroll = scroll.max(0.0);
        }
        if let Some(path) = state.open_files.get(state.active_file) {
            if let Some(index) = self.files.index_of(path) {
                self.show_tab(index);
            }
        }
        self.explorer_visible = state.explorer_visible;
        // The project's run configurations, and which of them the widget had chosen. **No run is
        // started**: unlike a terminal, which is a place to type, a run is something that was
        // started deliberately, and restarting somebody's dev server because they closed the window
        // would be a surprise. The tile comes back up holding nothing, which is what it says.
        self.run_configurations = run_configurations::load(self.tree.root());
        self.run.visible = state.run_visible;
        // The remembered choice is only adopted if something still answers to it. A temporary is
        // not written down, so the commonest thing a project remembers having chosen is a name
        // that has gone with the window — and a widget offering to run something that is not there
        // is worse than one offering nothing. The detectors count, which is why this is asked
        // after the plugins have been read.
        if !state.run_selected.is_empty()
            && self.configuration_named(Some(&state.run_selected)).is_some()
        {
            self.run_selected = Some(state.run_selected.clone());
        }
        // The shells themselves cannot be brought back, so the same number of fresh ones are started in
        // the project's own folder, which is what a person means by "my terminals were there".
        if state.terminal_visible && state.terminal_tabs > 0 {
            self.terminal.visible = true;
            self.run.visible = false;
            for _ in 0..state.terminal_tabs {
                self.new_terminal_tab();
            }
            // A name somebody typed is the one thing about a terminal that survives its shell, so
            // it is put back. A blank leaves the tab named after whatever program it is running,
            // which is what `Session::rename` already means by an empty name.
            for (index, name) in state.terminal_tab_names.iter().enumerate() {
                if !name.is_empty() {
                    self.terminal.tabs.rename(index, name);
                }
            }
        }
        self.written_project = Some(self.project_state());
    }

    /// What is open in this project now.
    fn project_state(&self) -> ProjectState {
        // One pass over the tabs that have a path, so the four parallel lists are the same length
        // by construction. They were built from two different walks before — `paths()`, which drops
        // a tab that has never been saved, and `panes_of_tabs()`, which does not — so an untitled
        // tab slid every pane number along by one.
        let mut open_files = Vec::new();
        let mut file_panes = Vec::new();
        let mut file_scrolls = Vec::new();
        let mut file_carets = Vec::new();
        for file in self.files.iter() {
            let Some(path) = file.path() else {
                continue;
            };
            open_files.push(path.to_path_buf());
            file_panes.push(file.pane);
            // Where each tab was left, so a project opens at the line it was being read at rather
            // than at the top of every file — `task-1693`.
            file_scrolls.push(file.scroll);
            file_carets.push(file.document.selection().head);
        }
        let active = self.files.active().path().and_then(|path| {
            open_files.iter().position(|known| known == path)
        });
        ProjectState {
            open_files,
            active_file: active.unwrap_or(0),
            file_panes,
            file_scrolls,
            file_carets,
            pane_widths: self.files.pane_widths().to_vec(),
            active_pane: self.files.focused_pane(),
            expanded_folders: self.tree.expanded_folders(),
            explorer_visible: self.explorer_visible,
            terminal_visible: self.terminal.visible,
            terminal_tabs: self.terminal.tabs.count(),
            // The names a person typed, and nothing else: `Tabs::names` would give back
            // `powershell.exe 2` for a tab nobody has named, which is a name the next run would
            // restore as though somebody had chosen it.
            terminal_tab_names: self
                .terminal
                .tabs
                .sessions()
                .iter()
                .map(|session| session.given_name().unwrap_or_default().to_owned())
                .collect(),
            run_visible: self.run.visible,
            run_selected: self.run_selected.clone().unwrap_or_default(),
            // Where the window is, which is the other half of "in the same location and state". It
            // is read from egui once a frame and is `None` until there has been one, so a window
            // that has not drawn yet never writes a geometry over the one it was opened with.
            window: self.window_place,
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
            folding_applies: file_kind::folding_applies(self.document().path()),
            foldable: self.fold_counts().0,
            folded: self.fold_counts().1,
            definitions_apply: self.definitions_apply_here(),
            symbols_apply: self.symbols_apply_here(),
            completion_applies: self.completion_applies_here(),
            can_go_back: !self.back.is_empty(),
            can_go_forward: !self.forward.is_empty(),
            run_selected: self.run_selected.clone(),
            run_names: self.run_rows().into_iter().map(|row| row.name).collect(),
            run_active: self
                .run_selected
                .as_deref()
                .and_then(|name| self.run.index_of(name))
                .and_then(|at| self.run.at(at))
                .is_some_and(run_panel::Run::is_running),
            run_file_applies: self.run_file_template().is_some(),
            run_tile_visible: self.run.visible,
            debug_applies: self.debug_applies_here(),
            debug_active: self.debug.is_some(),
            debug_paused: self.debug.as_ref().is_some_and(DebugState::is_paused),
            debug_tile_visible: self.debug_panel.visible,
            on_a_breakpoint: self.breakpoint_in_question().is_some(),
            breakpoint_enabled: self
                .breakpoint_in_question()
                .is_none_or(|breakpoint| breakpoint.enabled),
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
            Action::GoToDefinition => {
                let offset = self.caret_offset();
                self.go_to_definition(offset);
            }
            Action::FindReferences => {
                let offset = self.caret_offset();
                self.find_references(offset);
            }
            Action::RenameSymbol => {
                let offset = self.caret_offset();
                self.rename_symbol(offset);
            }
            // Only while the editing area has the keyboard. The menu's keyboard watcher does not
            // care what has the focus and does not consume the press, and `Ctrl+Space` is a key a
            // terminal sends — as a NUL byte — so without this guard one press would both open a
            // list over a file nobody was typing into and reach the program in the terminal.
            Action::CompleteWord if self.focus == Focus::Editor => self.complete_word(),
            Action::CompleteWord => {}
            Action::NavigateBack => self.navigate(true),
            Action::NavigateForward => self.navigate(false),
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
                } else if self.preview_holds_the_selection() {
                    if let Some(text) = self.preview_selected_text() {
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
                if self.reading_preview && self.view_mode().shows_preview() {
                    self.select_the_whole_preview();
                } else {
                    self.document_mut().apply(Command::SelectAll);
                }
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
                let showing = !self.terminal.visible;
                self.show_the_terminal_tile(showing);
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
                self.show_the_terminal_tile(true);
                self.new_terminal_tab();
            }
            Action::RenameTerminalTab => {
                let index = self.terminal.tabs.active_index();
                match self.terminal.tabs.names().get(index) {
                    Some(name) => {
                        self.prompt = Some(Prompt::new(
                            "Rename Terminal Tab",
                            "What this tab is called in the strip. The name stays put when the program in it sets a title of its own.",
                            name,
                            "Rename",
                            Purpose::RenameTerminalTab(index),
                        ));
                    }
                    None => self.message = Some("There is no terminal tab to rename.".to_owned()),
                }
            }
            Action::ToggleDebugTile => {
                let showing = self.debug_panel.visible;
                self.show_the_debug_tile(!showing);
            }
            Action::Debug(what) => self.debug_a_configuration(what),
            Action::ToggleRunTile => {
                let showing = !self.run.visible;
                self.show_the_run_tile(showing);
            }
            // The reason is already in the status bar, which is the whole of what a menu can say
            // about a run that would not start. It is the command line that needed it as a value,
            // and `cli_run_do` is what takes it.
            Action::Run(what) => {
                let _ = self.run_a_configuration(what);
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
            Action::NewFolder(folder) => {
                self.prompt = Some(Prompt::new(
                    "New Folder",
                    &format!("A new folder inside {}.", folder.display()),
                    "folder",
                    "Create",
                    Purpose::NewFolder(folder),
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
            Action::DeletePath(path) => self.ask_before_deleting(&path),
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
            Action::Fold(what) => {
                use crate::app::actions::FoldAction;
                match what {
                    FoldAction::Toggle => self.toggle_fold_at_caret(),
                    FoldAction::All => self.collapse_all_folds(),
                    FoldAction::None_ => self.expand_all_folds(),
                    FoldAction::Others => self.collapse_all_but_marked(),
                };
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

    // ------------------------------------------------------------------------ running

    /// Do what the `Run` menu, the run widget, the keyboard or the command line asked for.
    ///
    /// Split out of [`Self::run_action`] rather than written into it, for the reason `run_git` is
    /// split out: seven arms about one subject read better together, and the arm in `run_action`
    /// stays one line. It is still the one place a run action turns into a change.
    /// The reply is the reason it could not be done, or nothing when it was.
    ///
    /// It used to say nothing at all and leave the reason in the status bar, which the command line
    /// then read back and reported as a **success**: `task-1691` measured `run start` on a
    /// configuration whose program was not on the `PATH` coming back with `isError` false, `started`
    /// false and no reason anywhere. The one place a run action turns into a change is also the one
    /// place that knows whether it did, so it is what says so.
    pub fn run_a_configuration(&mut self, what: RunAction) -> Result<(), String> {
        match what {
            RunAction::Start(named) => match self.configuration_named(named.as_deref()) {
                Some(configuration) => self.start_a_run(configuration),
                None => Err(self.no_such_configuration(named.as_deref())),
            },
            // Rerun and start are the same thing, because starting one that is already running
            // stops it and starts it again — §5.2. Two entries because two words are what a person
            // reaches for, one path because they mean the same.
            RunAction::Rerun(named) => match self.configuration_named(named.as_deref()) {
                Some(configuration) => self.start_a_run(configuration),
                None => Err(self.no_such_configuration(named.as_deref())),
            },
            RunAction::Stop(named) => {
                let name = named.or_else(|| self.run_selected.clone());
                match name.as_deref().and_then(|name| self.run.index_of(name)) {
                    Some(at) => {
                        let name = self.run.at(at).map(|run| run.name().to_owned()).unwrap_or_default();
                        self.run.stop(at);
                        self.message = Some(format!("Stopping {name}"));
                        Ok(())
                    }
                    None => {
                        // Nothing to stop is a refusal rather than a quiet nothing, for the same
                        // reason a run that would not start is: a caller that is told it worked
                        // cannot tell it from a program that stopped by itself a moment ago.
                        let problem = match name {
                            Some(name) => format!("{name} is not running."),
                            None => "Nothing is running.".to_owned(),
                        };
                        self.message = Some(problem.clone());
                        Err(problem)
                    }
                }
            }
            RunAction::Select(name) => match self.configuration_named(Some(&name)) {
                Some(_) => {
                    self.run_selected = Some(name);
                    Ok(())
                }
                None => Err(self.no_such_configuration(Some(&name))),
            },
            RunAction::CurrentFile => self.run_the_current_file(),
            RunAction::Edit => {
                self.close_every_modal();
                let chosen = self.run_selected.clone();
                self.run_dialog.open(chosen);
                Ok(())
            }
        }
    }

    /// Say that a configuration of this name is not there, in the words that fit which question was
    /// asked: naming one that is not there and having chosen nothing at all are two different
    /// things to be told.
    fn no_such_configuration(&mut self, named: Option<&str>) -> String {
        let problem = match named {
            Some(name) => format!("There is no run configuration called {name}."),
            None => "No run configuration is chosen. Press the play button to make one.".to_owned(),
        };
        self.message = Some(problem.clone());
        problem
    }

    /// The configuration of this name, or the one the widget has chosen when no name is given.
    ///
    /// A **suggestion** counts: running one is how it becomes a temporary, which is what makes the
    /// detectors worth having. Everything that runs something comes through here, so the widget,
    /// the menu and the command line cannot come to different answers about what a name means.
    pub fn configuration_named(&self, named: Option<&str>) -> Option<Configuration> {
        let name = match named {
            Some(name) => name.to_owned(),
            None => self.run_selected.clone()?,
        };
        if let Some((_, configuration)) = self.run_configurations.find(&name) {
            return Some(configuration.clone());
        }
        self.suggestions().into_iter().find(|configuration| configuration.name == name)
    }

    /// What the built-in detectors offer for this project, given the plugins that are switched on.
    ///
    /// Worked out at the moment of use rather than held, which is the rule `Plugins::renders`
    /// keeps: switching the JavaScript plugin off withdraws the npm suggestions in the same frame.
    pub fn suggestions(&self) -> Vec<Configuration> {
        let runners = self.plugins.project_runners();
        let offered = run_configurations::detect(self.tree.root(), &runners);
        // A suggestion whose name is already a configuration is not offered twice: once somebody
        // has kept `cargo run`, the detector has nothing left to say about it.
        offered
            .into_iter()
            .filter(|configuration| self.run_configurations.find(&configuration.name).is_none())
            .collect()
    }

    /// Every configuration the widget's flyout and the `Run` menu list, in that order.
    pub fn run_rows(&self) -> Vec<run_widget::Row> {
        let mut rows: Vec<run_widget::Row> = self
            .run_configurations
            .listed()
            .into_iter()
            .map(|(origin, configuration)| run_widget::Row {
                name: configuration.name.clone(),
                origin,
                running: self
                    .run
                    .index_of(&configuration.name)
                    .and_then(|at| self.run.at(at))
                    .is_some_and(run_panel::Run::is_running),
            })
            .collect();
        rows.extend(self.suggestions().into_iter().map(|configuration| run_widget::Row {
            name: configuration.name,
            origin: Origin::Suggested,
            running: false,
        }));
        rows
    }

    /// Start a configuration, showing the tile and the run it made.
    ///
    /// Running a suggestion or a file makes a **temporary**, which is what puts it in the list so
    /// it can be run again and kept. Running something that is already permanent adds nothing.
    fn start_a_run(&mut self, configuration: Configuration) -> Result<(), String> {
        let root = self.tree.root().to_path_buf();
        let size = self.run_grid_size();
        let waker = self.waker();
        let name = configuration.name.clone();
        // Remembered before it is started, so a program that will not start still leaves the thing
        // that was tried in the list rather than vanishing with the error message.
        if self.run_configurations.find(&name).is_none() {
            self.run_configurations.add_temporary(configuration.clone());
        }
        self.run_selected = Some(name.clone());
        match self.run.start(configuration, &root, size, waker) {
            Ok(_) => {
                self.show_the_run_tile(true);
                self.focus = Focus::Terminal;
                self.message = Some(format!("Running {name}"));
                Ok(())
            }
            Err(problem) => {
                self.message = Some(problem.clone());
                Err(problem)
            }
        }
    }

    /// `Run Current File`: the open file's language's own command, with `{file}` replaced.
    fn run_the_current_file(&mut self) -> Result<(), String> {
        let Some(template) = self.run_file_template() else {
            let problem = "This file's language has not said how one file of it is run.".to_owned();
            self.message = Some(problem.clone());
            return Err(problem);
        };
        let Some(path) = self.document().path().map(Path::to_path_buf) else {
            // A document that has never been saved has no path, so there is nothing to run.
            let problem = "Save the file first, so there is something to run.".to_owned();
            self.message = Some(problem.clone());
            return Err(problem);
        };
        let root = self.tree.root().to_path_buf();
        let configuration = run_configurations::for_file(&template, &root, &path);
        self.start_a_run(configuration)
    }

    /// The open file's `run.file`, when a plugin that is switched on claims it and the file has
    /// been saved somewhere.
    ///
    /// What decides whether `Run Current File` is on the menu and in the flyout at all — absent,
    /// not dimmed, which is the rule the three code-navigation entries already follow.
    pub fn run_file_template(&self) -> Option<String> {
        let path = self.document().path()?;
        self.plugins.run_file(path).map(str::to_owned)
    }

    // ------------------------------------------------------------------------------- debugging

    /// Everything on the Run menu's debug half, and the tile's own buttons.
    ///
    /// **The one place a debug action turns into a change**, which is `run_action`'s rule applied
    /// within the family: the menu, the keyboard, the tile and the command line all come through
    /// here, so a thing done from a script and the same thing done by hand are the same thing,
    /// including what it says about it.
    pub fn debug_a_configuration(&mut self, what: DebugAction) {
        match what {
            DebugAction::Start(named) => match self.configuration_named(named.as_deref()) {
                Some(configuration) => self.start_debugging(configuration, None),
                // The sentence goes into the status bar, which is where every other refusal in
                // this family goes; `cli_debug_start` reads back whether a session exists rather
                // than what was said, so it needs no value here.
                None => {
                    self.no_such_configuration(named.as_deref());
                }
            },
            DebugAction::CurrentFile => self.debug_the_current_file(),
            DebugAction::Stop => match self.debug.as_mut() {
                Some(debug) => {
                    debug.stop();
                    self.message = debug.message.clone();
                }
                None => self.message = Some("Nothing is being debugged.".to_owned()),
            },
            DebugAction::Resume => self.step(quill_dap::Step::Resume),
            DebugAction::StepOver => self.step(quill_dap::Step::Over),
            DebugAction::StepInto => self.step(quill_dap::Step::Into),
            DebugAction::StepOut => self.step(quill_dap::Step::Out),
            DebugAction::Pause => self.step(quill_dap::Step::Pause),
            DebugAction::RunToCursor => self.run_to_cursor(),
            DebugAction::ToggleBreakpoint => self.toggle_breakpoint_here(),
            DebugAction::EditBreakpoint => self.open_the_breakpoint_dialog(),
            DebugAction::ToggleBreakpointEnabled => self.toggle_the_breakpoint_enabled(),
            DebugAction::EvaluateExpression => self.open_the_expression_box(),
            DebugAction::ToggleTile => {
                let showing = self.debug_panel.visible;
                self.show_the_debug_tile(!showing);
            }
            DebugAction::InstallAdapter(adapter) => self.install_an_adapter(&adapter),
        }
    }

    /// Install a debug adapter, by running its own install command in the run tile.
    ///
    /// **The editor still fetches nothing.** What runs is a package manager or an editor's extension
    /// installer, named by the registry entry, in a visible terminal with a program in it that can be
    /// watched, read with `run output` and stopped — which is every other run's rules, applied to
    /// this one. `task-1692` §7.1, and `tools/release.ps1` installing `gh` with winget is the same
    /// move made a year earlier.
    ///
    /// The configuration it makes is a temporary, so it is offered again if it has to be run again
    /// and is never written into the project's own file. What was selected before is put back
    /// afterwards: installing something is not choosing what the play button does.
    fn install_an_adapter(&mut self, adapter: &str) {
        let adapter = match adapter.trim().is_empty() {
            // An empty name means "the one that could not start", which is the adapter the file that
            // is showing would use — the same question the refusal itself asked.
            true => match self.document().path().and_then(|path| self.plugins.debugger_for(path)) {
                Some(named) => named.to_owned(),
                None => {
                    self.message = Some("Say which debugger to install.".to_owned());
                    return;
                }
            },
            false => adapter.trim().to_owned(),
        };
        let Some(entry) = debuggers::find(&adapter) else {
            self.message = Some(format!("This version of Quill does not know a debugger called {adapter}."));
            return;
        };
        let command = entry.install_command();
        if command.is_empty() {
            self.message = Some(format!(
                "Quill has no way to install {adapter} here. {}. Set {} to it once you have one.",
                entry.comes_from,
                format_args!("debug.{adapter}")
            ));
            return;
        }
        let was_selected = self.run_selected.clone();
        self.forget_the_adapter_search();
        let configuration = Configuration::new(&format!("Install {adapter}"), &command);
        match self.start_a_run(configuration) {
            Ok(()) => self.message = Some(format!("Installing {adapter}: {command}")),
            Err(problem) => self.message = Some(problem),
        }
        self.run_selected = was_selected;
    }

    /// Start a configuration under its debugger.
    ///
    /// **Debug is Run, under a debugger** — the same `Configuration` the play button starts, same
    /// command, same folder, same environment, which is IntelliJ's own model. A second session
    /// replaces the first, because there is one at a time.
    ///
    /// `for_file` names the file the session was started for, which is what decides which language's
    /// debugger to use when the configuration is a temporary made from `run.file`.
    fn start_debugging(&mut self, configuration: Configuration, for_file: Option<PathBuf>) {
        let Some(adapter) = self.adapter_for(&configuration, for_file.as_deref()) else {
            self.message = Some(
                "Nothing has said which debugger to use for this configuration, so there is nothing to start."
                    .to_owned(),
            );
            return;
        };
        // A configuration that names a build tool is built first and debugged second — `cargo run`
        // is the commonest configuration there is, and refusing it was `task-1692`'s second sentence.
        if let Some(build) = locators::locate(&configuration.command) {
            self.begin_a_build(build, configuration, adapter, for_file);
            return;
        }
        self.launch_a_session(configuration, adapter, None);
    }

    /// What the debug tile says when there is no session: a build in flight, a debugger this machine
    /// has not got, or the invitation to press Debug.
    ///
    /// Worked out each frame from a **cached** search, because looking for an adapter reads
    /// directories — the extension folders, LLVM's install locations — and a frame may not.
    /// [`Self::forget_the_adapter_search`] is what makes an install take effect.
    pub(crate) fn debug_idle(&mut self) -> debug_panel::Idle {
        if let Some(pending) = self.debug_build.as_ref() {
            return debug_panel::Idle::Building {
                what: pending.what.clone(),
                seconds: pending.started.elapsed().as_secs(),
            };
        }
        let ready = debug_panel::Idle::Ready(
            "Nothing is being debugged. Press the bug button in the title bar, or set a breakpoint and press Shift+F9."
                .to_owned(),
        );
        // Which adapter this project would use, asked of the configuration the buttons would start.
        let Some(configuration) =
            self.configuration_named(None).or_else(|| self.suggestions().into_iter().next())
        else {
            return ready;
        };
        let Some(adapter) = self.adapter_for(&configuration, None) else {
            return ready;
        };
        let report = self.adapter_report(&adapter);
        if report.is_found() {
            return ready;
        }
        debug_panel::Idle::Missing(debug_panel::Missing {
            sentence: format!(
                "Debugging {} needs {}. {}.",
                report.name,
                report.programs.join(" or "),
                report.comes_from
            ),
            adapter: report.name.to_owned(),
            install: report.install,
        })
    }

    /// What was found the last time this adapter was looked for.
    ///
    /// The search is cached because it reads directories and the tile asks every frame; it is
    /// forgotten whenever something could have changed it, which is an install finishing or the
    /// settings being written.
    pub(crate) fn adapter_report(&mut self, adapter: &str) -> debuggers::Report {
        if let Some((looked, known)) = self.debug_adapters.get(adapter) {
            if looked.elapsed() < ADAPTER_SEARCH_TTL {
                return known.clone();
            }
        }
        let Some(entry) = debuggers::find(adapter) else {
            return debuggers::Report {
                name: "",
                found: None,
                configured: false,
                programs: Vec::new(),
                languages: Vec::new(),
                comes_from: "this version of Quill does not know it",
                install: String::new(),
                settings_key: format!("debug.{adapter}"),
                caveat: "",
            };
        };
        let override_path = self.settings.debug_adapter(adapter).map(str::to_owned);
        let mut report = debuggers::report(entry, override_path.as_deref());
        report.languages = self.plugins.languages_debugged_by(adapter);
        self.debug_adapters.insert(adapter.to_owned(), (std::time::Instant::now(), report.clone()));
        report
    }

    /// Look for the adapters again next time somebody asks, because something that could have
    /// changed the answer has happened — an install has finished, or the settings have moved.
    pub(crate) fn forget_the_adapter_search(&mut self) {
        self.debug_adapters.clear();
    }

    /// Which debugger a configuration is given to.
    ///
    /// **The configuration first, the open file only as a fallback.** Asking the open file first is
    /// what made debugging a Node server while reading `README.md` answer that the file's language
    /// had named no debugger — a refusal about the wrong thing. `debuggers::adapter_for` reads the
    /// command line, the plugins answer for a program whose extension one of them claims, and the
    /// file that is showing is what is left.
    pub(crate) fn adapter_for(
        &self,
        configuration: &Configuration,
        for_file: Option<&Path>,
    ) -> Option<String> {
        if let Some((program, _)) = configuration.program_and_arguments() {
            if let Some(named) = debuggers::adapter_for(&program) {
                return Some(named.to_owned());
            }
            if let Some(named) = self.plugins.debugger_for(Path::new(&program)) {
                return Some(named.to_owned());
            }
        }
        let path = for_file.map(Path::to_path_buf).or_else(|| self.document().path().map(Path::to_path_buf));
        path.as_deref().and_then(|path| self.plugins.debugger_for(path)).map(str::to_owned)
    }

    /// Start the build a locator asked for, on a thread.
    ///
    /// The window is woken when it finishes, exactly as the git worker wakes it, and nothing else
    /// waits: the editor draws, the tile counts the seconds, and pressing Debug again replaces the
    /// build the way starting a second session replaces the first.
    fn begin_a_build(
        &mut self,
        build: locators::Build,
        configuration: Configuration,
        adapter: String,
        for_file: Option<PathBuf>,
    ) {
        let root = self.tree.root().to_path_buf();
        let folder = configuration.working_directory(&root);
        let (sender, replies) = std::sync::mpsc::channel();
        let waker = self.waker();
        let command = build.command();
        let wanted = build.wanted.clone();
        let program = build.program.clone();
        let args = build.args.clone();
        std::thread::spawn(move || {
            let answer = match std::process::Command::new(&program)
                .args(&args)
                .current_dir(&folder)
                .output()
            {
                Ok(output) if output.status.success() => {
                    let printed = String::from_utf8_lossy(&output.stdout);
                    match locators::executable(&printed, wanted.as_deref()) {
                        Some(program) => Built::Program(program),
                        None => Built::Nothing,
                    }
                }
                // The compiler's own words, which is what `--message-format=json-render-diagnostics`
                // puts on standard error and is more use than anything Quill could write instead.
                Ok(output) => Built::Failed(String::from_utf8_lossy(&output.stderr).to_string()),
                Err(problem) => Built::Failed(format!("{program} would not start: {problem}")),
            };
            let _ = sender.send(answer);
            waker();
        });
        // A build replaces whatever was being debugged, because Debug always means "this, now".
        self.stop_debugging();
        self.debug_output.clear();
        self.message = Some(format!("{}\u{2026}", build.what));
        self.debug_build = Some(PendingBuild {
            configuration,
            adapter,
            for_file,
            wanted: build.wanted,
            program_args: build.program_args,
            what: build.what,
            command,
            started: std::time::Instant::now(),
            replies,
        });
        self.show_the_debug_tile(true);
    }

    /// Take the build's answer, once there is one, and start the session it was for.
    ///
    /// Called once a frame from [`Self::take_the_debug_replies`], which is where every other thread's
    /// replies are already taken.
    fn take_the_build(&mut self) {
        let Some(pending) = self.debug_build.as_ref() else {
            return;
        };
        let Ok(answer) = pending.replies.try_recv() else {
            return;
        };
        let pending = self.debug_build.take().expect("just looked at it");
        match answer {
            Built::Program(program) => {
                let built = (program.to_string_lossy().to_string(), pending.program_args);
                self.launch_a_session(pending.configuration, pending.adapter, Some(built));
            }
            Built::Failed(said) => {
                // The compiler's words go where an adapter's words go, so `debug output` carries
                // them and nothing has to be read off a terminal.
                self.debug_output.extend(said.lines().map(str::to_owned));
                self.message = Some("The build failed \u{2014} see the debug output.".to_owned());
            }
            Built::Nothing => {
                self.message =
                    Some(format!("Nothing to debug: `{}` built no program.", pending.command));
            }
        }
    }

    /// Start the adapter and open the session, once there is a program to give it.
    ///
    /// `built` is what a locator produced, and it stands in for the configuration's command line in
    /// the launch request **only** — the configuration keeps its own command, so the play button
    /// still runs `cargo run` and the tile still says so.
    fn launch_a_session(
        &mut self,
        configuration: Configuration,
        adapter: String,
        built: Option<(String, Vec<String>)>,
    ) {
        let root = self.tree.root().to_path_buf();
        let override_path = self.settings.debug_adapter(&adapter).map(str::to_owned);
        // The refusal is one sentence naming what was looked for, where it comes from and the
        // command that installs it, built by the registry entry that knew — never an error dialog
        // and never a dead button.
        let prepared = match debuggers::prepare(
            &adapter,
            &configuration,
            &root,
            override_path.as_deref(),
            built,
        ) {
            Ok(prepared) => prepared,
            Err(refusal) => {
                self.message = Some(refusal.message());
                // And the tile comes up saying it, with the Install button under it. A sentence in
                // the status bar is what a person misses; `task-1692`'s whole complaint is that
                // pressing Debug looked like nothing happening.
                //
                // A session that has already ended is thrown away first, because the tile draws the
                // offer only where there is no session at all — and the empty panes of a program
                // that finished a minute ago are worth less than the reason this one never started.
                if self.debug.as_ref().is_some_and(|debug| !debug.is_alive()) {
                    self.debug = None;
                }
                self.forget_the_adapter_search();
                self.show_the_debug_tile(true);
                return;
            }
        };
        // The one that was there is stopped and thrown away first, so its adapter does not outlive
        // the session that replaced it. Nothing ever orphans a child on purpose.
        self.stop_debugging();
        let name = configuration.name.clone();
        if self.run_configurations.find(&name).is_none() {
            self.run_configurations.add_temporary(configuration.clone());
        }
        self.run_selected = Some(name.clone());
        let waker = self.waker();
        match DebugState::start(
            &adapter,
            &prepared.adapter,
            prepared.body,
            prepared.caveat,
            configuration,
            waker,
        ) {
            Ok(state) => {
                self.debug = Some(state);
                self.send_every_breakpoint();
                self.show_the_debug_tile(true);
                self.message = Some(match prepared.caveat.is_empty() {
                    true => format!("Debugging {name}"),
                    // The adapter's own limits reach the person rather than being discovered as
                    // wrong-looking values later. §5.3.
                    false => format!("Debugging {name}. {}", prepared.caveat),
                });
            }
            Err(problem) => self.message = Some(problem),
        }
    }

    /// `Debug Current File`: the open file's language's own command, under its own debugger.
    ///
    /// It exists exactly where `Run Current File` exists **and** the language names an adapter, so a
    /// `.rs` file offers neither — Rust deliberately has no `run.file`, because running one file of
    /// a Cargo project is not a thing cargo does — and a `.css` file offers nothing.
    fn debug_the_current_file(&mut self) {
        let Some(template) = self.run_file_template() else {
            self.message =
                Some("This file's language has not said how one file of it is run.".to_owned());
            return;
        };
        let Some(path) = self.document().path().map(Path::to_path_buf) else {
            self.message = Some("Save the file first, so there is something to debug.".to_owned());
            return;
        };
        let root = self.tree.root().to_path_buf();
        let configuration = run_configurations::for_file(&template, &root, &path);
        self.start_debugging(configuration, Some(path));
    }

    /// One of the five stepping requests, with the adapter's own refusal if it will not.
    fn step(&mut self, step: quill_dap::Step) {
        let Some(debug) = self.debug.as_mut() else {
            self.message = Some("Nothing is being debugged.".to_owned());
            return;
        };
        // What every row showed, remembered before the program goes on, so the next stop can mark
        // what moved: "changed" means "different from the last time you looked".
        debug.remember_the_values();
        match debug.step(step) {
            Ok(()) => self.message = debug.message.clone(),
            Err(problem) => self.message = Some(problem),
        }
        self.debug_panel.stop_editing();
    }

    /// `Run to Cursor`: a breakpoint on the caret's line for the length of one resume.
    ///
    /// DAP has no request for it and every client builds it the same way — a temporary breakpoint,
    /// a `continue`, and the breakpoint taken away at the next stop. It is done through the ordinary
    /// breakpoint path rather than a second one, so the dot is real while it lasts and the adapter is
    /// told about it exactly as it is told about any other.
    fn run_to_cursor(&mut self) {
        if self.debug.as_ref().is_none_or(|debug| !debug.is_paused()) {
            self.message = Some("The program is not stopped.".to_owned());
            return;
        }
        let Some(path) = self.document().path().map(Path::to_path_buf) else {
            self.message = Some("Save the file first, so there is a line to run to.".to_owned());
            return;
        };
        let caret = self.document().selection().head;
        let line = self.document_mut().line_start_of(caret);
        // A line that already has one needs no temporary: resuming will stop there anyway.
        let temporary = self.document().breakpoints().at(line).is_none();
        if temporary {
            self.document_mut().toggle_breakpoint(line);
            self.run_to = Some((path.clone(), line));
            self.send_the_breakpoints_of(&path);
        }
        self.step(quill_dap::Step::Resume);
    }

    /// Take away the temporary breakpoint `Run to Cursor` made, once the program has stopped again.
    fn clear_the_run_to_breakpoint(&mut self) {
        let Some((path, offset)) = self.run_to.take() else {
            return;
        };
        self.change_breakpoints(&path, |breakpoints| {
            breakpoints.remove_at(offset);
        });
        self.send_the_breakpoints_of(&path);
    }

    /// Put a breakpoint on the caret's line, or take away the one that is there.
    fn toggle_breakpoint_here(&mut self) {
        let caret = self.document().selection().head;
        let line = self.document_mut().line_start_of(caret);
        self.toggle_breakpoint_at_offset(line);
    }

    /// The same, from a click in the gutter, which names a paragraph rather than an offset.
    fn toggle_breakpoint_at_line(&mut self, paragraph: usize) {
        let offset = self.document().text().line_to_byte(paragraph);
        self.toggle_breakpoint_at_offset(offset);
    }

    /// The one place a breakpoint is put on or taken off the file that is showing.
    ///
    /// **Absent rather than refused** for a file whose language names no debugger: the gutter takes
    /// no click there at all, and this says so for the keyboard and the menu, which can still ask.
    fn toggle_breakpoint_at_offset(&mut self, offset: usize) {
        if !self.debug_applies_here() {
            self.message =
                Some("This file's language has not said which debugger to use.".to_owned());
            return;
        }
        let Some(path) = self.document().path().map(Path::to_path_buf) else {
            self.message = Some("Save the file first, so a breakpoint has somewhere to live.".to_owned());
            return;
        };
        let now = self.document_mut().toggle_breakpoint(offset);
        let line = self.document().line_number_of(offset);
        self.message = Some(match now {
            true => format!("Breakpoint on line {line}"),
            false => format!("Breakpoint removed from line {line}"),
        });
        self.send_the_breakpoints_of(&path);
    }

    /// Which line the gutter's menu and the breakpoint entries are about: the row it was opened
    /// over, or the caret's line when the question came from the keyboard or the command line.
    fn breakpoint_line_in_question(&self) -> usize {
        match self.gutter_menu_line {
            Some(paragraph) => self.document().text().line_to_byte(paragraph),
            None => {
                let caret = self.document().selection().head;
                self.document().line_start_of(caret)
            }
        }
    }

    /// The breakpoint on that line, if there is one. What the gutter's menu asks to decide whether
    /// it offers to set one or to remove one.
    fn breakpoint_in_question(&self) -> Option<&quill_core::Breakpoint> {
        self.document().breakpoints().at(self.breakpoint_line_in_question())
    }

    /// Switch the breakpoint in question off without taking it away, or back on again.
    ///
    /// A disabled breakpoint keeps its condition and its log message and is drawn hollow; it is
    /// simply not sent to the adapter, because `enabled` is Quill's own idea and the protocol has no
    /// field for it.
    fn toggle_the_breakpoint_enabled(&mut self) {
        let Some(path) = self.document().path().map(Path::to_path_buf) else {
            return;
        };
        let offset = self.breakpoint_line_in_question();
        let Some(was) = self.document().breakpoints().at(offset).map(|one| one.enabled) else {
            self.message = Some("There is no breakpoint on that line.".to_owned());
            return;
        };
        self.document_mut().change_breakpoint(offset, |breakpoint| breakpoint.enabled = !was);
        let line = self.document().line_number_of(offset);
        self.message = Some(match was {
            true => format!("Breakpoint on line {line} disabled"),
            false => format!("Breakpoint on line {line} enabled"),
        });
        self.send_the_breakpoints_of(&path);
    }

    /// Open `Edit Breakpoint...` on the line in question, putting one there if there is none.
    fn open_the_breakpoint_dialog(&mut self) {
        if !self.debug_applies_here() {
            self.message =
                Some("This file's language has not said which debugger to use.".to_owned());
            return;
        }
        let Some(path) = self.document().path().map(Path::to_path_buf) else {
            self.message = Some("Save the file first, so a breakpoint has somewhere to live.".to_owned());
            return;
        };
        let offset = self.breakpoint_line_in_question();
        let created = self.document().breakpoints().at(offset).is_none();
        if created {
            self.document_mut().toggle_breakpoint(offset);
        }
        let breakpoint = self.document().breakpoints().at(offset).cloned().unwrap_or_default();
        // A field whose capability is absent is absent. With **no session running** both are offered,
        // because a breakpoint edited now is one a debugger will be asked about later and refusing to
        // let somebody type a condition before they have pressed Debug would be absurd.
        let (conditions, log_points) = match self.debug.as_ref() {
            Some(debug) => (
                debug.capabilities().conditional_breakpoints,
                debug.capabilities().log_points,
            ),
            None => (true, true),
        };
        self.close_every_modal();
        self.breakpoint_dialog = Some(BreakpointDialog {
            path,
            offset,
            line: self.document().line_number_of(offset),
            enabled: breakpoint.enabled,
            condition: breakpoint.condition.clone().unwrap_or_default(),
            log_message: breakpoint.log_message.clone().unwrap_or_default(),
            conditions,
            log_points,
            created,
        });
    }

    /// Open `Evaluate Expression`, seeded with the selection when there is one.
    fn open_the_expression_box(&mut self) {
        let seed = match self.document().selection().is_empty() {
            true => String::new(),
            false => {
                let range = self.document().selection().range();
                self.document().text().byte_slice(range)
            }
        };
        self.close_every_modal();
        self.evaluate = Some(EvaluateDialog {
            expression: seed.trim().to_owned(),
            result: None,
            asking: false,
        });
    }

    /// End the session, killing the adapter. What closing the window, closing the project and
    /// starting a second session all do.
    pub fn stop_debugging(&mut self) {
        if let Some(mut debug) = self.debug.take() {
            debug.kill();
        }
        self.debug_panel.stop_editing();
        self.run_to = None;
    }

    /// Change what is set in any file of this project, whether it is open or not.
    ///
    /// **The one place that choice is made**, so no caller has to think about it: a file that is
    /// open is owned by its `Document`, and every other file is owned by `services::breakpoint_store`.
    /// It is `change_highlights` with one word changed, deliberately — the rule is the same rule and
    /// a second answer to it would be a second thing to keep in step.
    pub fn change_breakpoints(
        &mut self,
        path: &Path,
        change: impl FnOnce(&mut quill_core::Breakpoints),
    ) -> bool {
        if let Some(index) = self.files.index_of(path) {
            let mut breakpoints = self.files.at(index).document.breakpoints().clone();
            let before = breakpoints.clone();
            change(&mut breakpoints);
            if before == breakpoints {
                return false;
            }
            self.files.at_mut(index).document.set_breakpoints(breakpoints);
            return true;
        }
        self.breakpoints.change(path, change)
    }

    /// What is set in one file, whether it is open or not.
    pub fn breakpoints_of(&self, path: &Path) -> quill_core::Breakpoints {
        if let Some(index) = self.files.index_of(path) {
            return self.files.at(index).document.breakpoints().clone();
        }
        self.breakpoints.breakpoints(path).cloned().unwrap_or_default()
    }

    /// Every file in this project that has a breakpoint in it, open or not, in path order.
    pub fn every_breakpoint(&self) -> Vec<(PathBuf, quill_core::Breakpoints)> {
        let mut files: Vec<(PathBuf, quill_core::Breakpoints)> = Vec::new();
        for (path, breakpoints) in self.breakpoints.files() {
            files.push((path.clone(), breakpoints.clone()));
        }
        for index in 0..self.files.len() {
            let Some(path) = self.files.at(index).path().map(Path::to_path_buf) else {
                continue;
            };
            let breakpoints = self.files.at(index).document.breakpoints().clone();
            // The document owns an open file, so its set replaces whatever the store had.
            files.retain(|(known, _)| *known != path);
            if !breakpoints.is_empty() {
                files.push((path, breakpoints));
            }
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    /// Push what every open document holds into the store, and write the store if it changed.
    ///
    /// Called every frame and does almost nothing: an integer comparison for each open tab, because
    /// a document that has not changed since it was last pushed cannot have new breakpoints in it.
    /// `remember_the_marks`'s arrangement exactly, keyed on the same revision.
    fn remember_the_breakpoints(&mut self, settled: bool) {
        for index in 0..self.files.len() {
            let Some(path) = self.files.at(index).path().map(Path::to_path_buf) else {
                continue;
            };
            let revision = self.files.at(index).document.revision();
            if self.files.at(index).breakpoints_at == Some(revision) {
                continue;
            }
            let breakpoints = self.files.at(index).document.breakpoints().clone();
            self.breakpoints.set(&path, breakpoints);
            self.files.at_mut(index).breakpoints_at = Some(revision);
        }
        if self.remembers_this_project() && settled {
            let root = self.tree.root().to_path_buf();
            self.breakpoints.save(&root);
        }
    }

    /// Tell the adapter what one file's breakpoints are now.
    ///
    /// A disabled breakpoint is **not sent**: `enabled` is Quill's own idea and the protocol has no
    /// field for it, so switching one off means taking it out of the set the adapter holds. The dot
    /// stays, drawn hollow.
    fn send_the_breakpoints_of(&mut self, path: &Path) {
        let breakpoints = self.breakpoints_of(path);
        let (conditions, logs) = match self.debug.as_ref() {
            Some(debug) => (
                debug.capabilities().conditional_breakpoints,
                debug.capabilities().log_points,
            ),
            None => return,
        };
        let document_index = self.files.index_of(path);
        let lines: Vec<(usize, quill_dap::SourceBreakpoint)> = breakpoints
            .iter()
            .filter(|breakpoint| breakpoint.enabled)
            .map(|breakpoint| {
                let line = match document_index {
                    Some(index) => {
                        self.files.at(index).document.line_number_of(breakpoint.offset)
                    }
                    // A file that is not open has no laid-out text to count lines in, so its own
                    // bytes are read at the moment of use — the ownership rule's disk half, and the
                    // same "re-read rather than watch" `open_the_match` already does.
                    None => line_number_in_file(path, breakpoint.offset),
                };
                let carried = quill_dap::SourceBreakpoint {
                    line,
                    // Never sent to an adapter that did not offer it, which is the rule every
                    // optional feature here follows.
                    condition: conditions.then(|| breakpoint.condition.clone()).flatten(),
                    log_message: logs.then(|| breakpoint.log_message.clone()).flatten(),
                };
                (breakpoint.offset, carried)
            })
            .collect();
        if let Some(debug) = self.debug.as_mut() {
            debug.set_breakpoints(path, lines);
        }
    }

    /// Tell the adapter about every file that has one, which is what a session start does.
    fn send_every_breakpoint(&mut self) {
        for (path, _) in self.every_breakpoint() {
            self.send_the_breakpoints_of(&path);
        }
    }

    /// Everything the debug tile reported this frame.
    ///
    /// One function rather than twelve arms in the middle of `ui`, for the reason the run tile's
    /// outcome is settled in one place: the tile decides nothing and this decides everything, so
    /// pressing `Step Over` in the tile and pressing `F8` are the same call.
    fn act_on_the_debug_tile(&mut self, outcome: debug_panel::DebugOutcome, ctx: &egui::Context) {
        if outcome.hide {
            self.show_the_debug_tile(false);
        }
        if let Some(adapter) = outcome.install {
            self.debug_a_configuration(DebugAction::InstallAdapter(adapter));
        }
        if let Some(command) = outcome.copy {
            ctx.copy_text(command.clone());
            self.message = Some(format!("Copied: {command}"));
        }
        if outcome.console {
            // One press, both directions: the debuggee's terminal is the run tile, and two grids
            // cannot show at once.
            self.show_the_run_tile(true);
        }
        if let Some(step) = outcome.step {
            self.step(step);
        }
        if outcome.stop {
            self.debug_a_configuration(DebugAction::Stop);
        }
        if let Some(frame) = outcome.show_frame {
            if let Some(debug) = self.debug.as_mut() {
                debug.show_frame(frame);
            }
            // Clicking a frame moves the execution point to it without resuming, which is
            // IntelliJ's own behaviour.
            self.follow_the_execution_point();
        }
        if let Some(key) = outcome.toggle_row {
            if let Some(debug) = self.debug.as_mut() {
                debug.toggle_row(&key);
            }
        }
        if let Some((key, value)) = outcome.set_value {
            if let Some(debug) = self.debug.as_mut() {
                if let Err(problem) = debug.set_value(&key, &value) {
                    self.message = Some(problem);
                }
            }
        }
        if let Some(expression) = outcome.add_watch {
            if let Some(debug) = self.debug.as_mut() {
                debug.add_watch(&expression);
            }
        }
        if let Some(expression) = outcome.remove_watch {
            if let Some(debug) = self.debug.as_mut() {
                debug.remove_watch(&expression);
            }
        }
        if let Some(filters) = outcome.filters {
            if let Some(debug) = self.debug.as_mut() {
                debug.set_filters(filters);
            }
        }
    }

    /// The two debug modals, drawn after the panes so they sit over everything.
    fn show_the_debug_modals(&mut self, ctx: &egui::Context) {
        if let Some(mut dialog) = self.breakpoint_dialog.take() {
            let outcome = debug_dialogs::breakpoint(ctx, &mut dialog);
            match (outcome.confirmed, outcome.removed, outcome.cancelled) {
                (true, _, _) => {
                    let condition = dialog.condition();
                    let log = dialog.log();
                    let enabled = dialog.enabled;
                    let path = dialog.path.clone();
                    self.change_breakpoints(&path, |breakpoints| {
                        if let Some(breakpoint) = breakpoints.at_mut(dialog.offset) {
                            breakpoint.enabled = enabled;
                            breakpoint.condition = condition;
                            breakpoint.log_message = log;
                        }
                    });
                    self.send_the_breakpoints_of(&path);
                    self.message = Some(format!("Breakpoint on line {} saved", dialog.line));
                }
                (_, true, _) => {
                    let path = dialog.path.clone();
                    self.change_breakpoints(&path, |breakpoints| {
                        breakpoints.remove_at(dialog.offset);
                    });
                    self.send_the_breakpoints_of(&path);
                    self.message = Some(format!("Breakpoint removed from line {}", dialog.line));
                }
                // Cancelling takes back only a breakpoint this modal put there: somebody who right
                // clicked an empty line, chose `Add Conditional Breakpoint...` and then thought
                // better of it has not asked for a plain one. One that was already there is left
                // exactly as it was, which is what Cancel means everywhere else.
                (_, _, true) if dialog.created => {
                    let path = dialog.path.clone();
                    self.change_breakpoints(&path, |breakpoints| {
                        breakpoints.remove_at(dialog.offset);
                    });
                    self.send_the_breakpoints_of(&path);
                }
                _ => {}
            }
            if !outcome.confirmed && !outcome.removed && !outcome.cancelled {
                self.breakpoint_dialog = Some(dialog);
            }
        }
        if let Some(mut dialog) = self.evaluate.take() {
            let paused = self.debug.as_ref().is_some_and(DebugState::is_paused);
            let outcome = debug_dialogs::evaluate(ctx, &mut dialog, paused);
            if outcome.confirmed {
                let expression = dialog.expression.clone();
                dialog.asking = true;
                dialog.result = None;
                if let Some(debug) = self.debug.as_mut() {
                    debug.evaluate(&expression);
                }
            }
            // The answer, once the adapter has sent one. Read here rather than pushed, because a
            // modal is drawn every frame anyway and one place reading is one place to get right.
            if let Some((_, _, Some(answer))) =
                self.debug.as_ref().and_then(|debug| debug.evaluated.clone())
            {
                dialog.asking = false;
                dialog.result = Some(match answer {
                    Ok(value) => Ok(value.value),
                    Err(problem) => Err(problem),
                });
            }
            if !outcome.cancelled {
                self.evaluate = Some(dialog);
            }
        }
    }

    /// Take everything the adapter has said and act on it.
    ///
    /// Called once a frame, beside the git worker's own poll and the run tile's `settle`, which is
    /// where every other thread's replies are already taken.
    fn take_the_debug_replies(&mut self, ctx: &egui::Context) {
        // A build that has finished is what starts the session, so it is asked first — and it is
        // asked whether or not a session exists, which the early return below would have skipped.
        self.take_the_build();
        let Some(debug) = self.debug.as_mut() else {
            return;
        };
        let asked = debug.take_replies();
        let paused = debug.is_paused();
        let ended = !debug.is_alive();
        if let Some(said) = debug.message.take() {
            self.message = Some(said);
        }
        for event in asked {
            match event {
                quill_dap::Event::RunInTerminal { seq, title, cwd, args, env } => {
                    self.run_the_debuggee(seq, &title, &cwd, args, env);
                }
                // js-debug's child session. Opening it is the session's own business; re-sending the
                // breakpoints is the window's, because the child has never been told about any of
                // them and the store is what knows them all. `task-1692`.
                quill_dap::Event::StartDebugging { seq, request, configuration } => {
                    let opened = self
                        .debug
                        .as_mut()
                        .map(|debug| debug.adopt_child(seq, &request, configuration));
                    match opened {
                        Some(Ok(())) => self.send_every_breakpoint(),
                        Some(Err(problem)) => self.message = Some(problem),
                        None => {}
                    }
                }
                _ => {}
            }
        }
        if paused {
            // The temporary breakpoint `Run to Cursor` made has done its job.
            self.clear_the_run_to_breakpoint();
            self.follow_the_execution_point();
        }
        if ended {
            self.debug_panel.stop_editing();
        }
        // A polite stop has to actually run out, and an idle window draws nothing — so it is woken
        // once, when the grace ends, rather than kept drawing for the whole two seconds.
        if let Some(left) = self.debug.as_ref().and_then(DebugState::stopping_in) {
            ctx.request_repaint_after(left);
        }
    }

    /// Answer the adapter's `runInTerminal` by starting the command in the run tile.
    ///
    /// **This is what puts a real ConPTY behind the debuggee** — its colours, its interactivity, and
    /// the run tile's own rules about opening at final size and never resizing while starting. It
    /// *is* a run, so it goes through `RunPanel::start` rather than through a second path.
    fn run_the_debuggee(
        &mut self,
        seq: i64,
        title: &str,
        cwd: &str,
        args: Vec<String>,
        env: Vec<(String, String)>,
    ) {
        let Some((program, rest)) = args.split_first() else {
            if let Some(debug) = self.debug.as_mut() {
                debug.answer_run_in_terminal(seq, false, None);
            }
            return;
        };
        let name = self
            .debug
            .as_ref()
            .map(|debug| debug.configuration.name.clone())
            .unwrap_or_else(|| title.to_owned());
        // A configuration built from what the adapter asked for rather than from what was chosen:
        // the adapter often wraps the program in one of its own, which is exactly how lldb-dap's
        // comm-file scheme works, and running the chosen command instead would run the wrong thing.
        let configuration = Configuration {
            name,
            command: run_configurations::join_command(program, rest),
            directory: String::new(),
            env: env
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<String>>()
                .join("; "),
        };
        let folder = match cwd.trim().is_empty() {
            true => self.tree.root().to_path_buf(),
            false => PathBuf::from(cwd),
        };
        let size = self.run_grid_size();
        let waker = self.waker();
        let started = self.run.start(configuration, &folder, size, waker);
        if let Some(problem) = started.as_ref().err() {
            self.message = Some(problem.clone());
        }
        if let Some(debug) = self.debug.as_mut() {
            // No process id: a pseudoconsole hands back a console rather than a child, and the
            // specification makes the id optional for exactly this reason. `started` is what the
            // adapter needs to know.
            debug.answer_run_in_terminal(seq, started.is_ok(), None);
        }
    }

    /// Open the file the program stopped in, scroll the least amount that shows the line, and put
    /// the caret on it.
    ///
    /// `open_the_match`'s path, which is what `Find in Files` and `Go to Definition` already use, so
    /// a jump from a stop and a jump from a search are the same jump.
    fn follow_the_execution_point(&mut self) {
        let Some(debug) = self.debug.as_ref() else {
            return;
        };
        let Some((path, line)) = debug.location() else {
            return;
        };
        if !path.exists() {
            // A frame in a library Quill has no source for. The tile still lists it; there is simply
            // nowhere to jump to, and saying nothing is better than saying something wrong.
            return;
        }
        self.open_path_permanently(&path);
        let offset = self.document().offset_of_line_number(line);
        // The caret is **placed** rather than the line selected, which is what `open_the_match` does
        // for a search hit. A stop already marks its line with a band across the whole width of the
        // pane, and a selection over the same line would be two decorations saying one thing — and
        // the wrong one of the two, since nothing about a stop is selected. IntelliJ places the
        // caret here too.
        self.document_mut().apply(Command::PlaceCaret { offset, extend: false });
        self.reveal_caret = true;
        // The layout may not have been worked out at this width yet, so the scroll is asked for on
        // the next frame rather than computed here — `reveal_caret`'s own arrangement.
        self.files.at_mut(self.files.active_index()).forget_what_was_worked_out();
    }

    /// Show the run tile, or put it away.
    ///
    /// **The bottom of the window holds one tile**, so showing this one puts the other two away —
    /// and its two siblings do the same in the other directions. Every path that shows any of the
    /// three goes through one of them, which is what stops them drifting apart: `terminal show` from
    /// the command line used to leave both `visible`, and the two grids were then drawn into the
    /// same rectangle, one over the other. `task-1687` made the pair a trio, on the same terms.
    pub fn show_the_run_tile(&mut self, showing: bool) {
        self.run.visible = showing;
        if showing {
            self.terminal.visible = false;
            self.debug_panel.visible = false;
            self.focus = Focus::Terminal;
        } else if self.focus == Focus::Terminal {
            self.focus = Focus::Editor;
        }
    }

    /// Show the terminal tile, or put it away, opening a shell if there is not one already.
    ///
    /// The second of the three; see [`Self::show_the_run_tile`] for why there is a function at all.
    pub fn show_the_terminal_tile(&mut self, showing: bool) {
        self.terminal.visible = showing;
        if showing {
            self.run.visible = false;
            self.debug_panel.visible = false;
            self.open_terminal_tab();
            self.focus = Focus::Terminal;
        } else if self.focus == Focus::Terminal {
            self.focus = Focus::Editor;
        }
    }

    /// Show the debug tile, or put it away.
    ///
    /// The third of the three. It does **not** take the keyboard: the tile holds a list and a tree
    /// rather than a grid a program is being typed into, and taking the keyboard away from the
    /// editor to look at a variable would mean pressing `F8` moved the caret rather than the
    /// program. The stepping keys work wherever the keyboard is, because they are menu entries.
    pub fn show_the_debug_tile(&mut self, showing: bool) {
        self.debug_panel.visible = showing;
        if showing {
            self.run.visible = false;
            self.terminal.visible = false;
            if self.focus == Focus::Terminal {
                self.focus = Focus::Editor;
            }
        }
    }

    /// How large the run tile's grid is going to be, so a program is opened at that size and is
    /// **never resized**.
    ///
    /// This is not a nicety. A pseudoconsole resized while its child is writing its first line
    /// loses that line — measured six times out of six on `cmd /c echo something`, which writes and
    /// exits inside a millisecond and was therefore always still starting when the tile drew its
    /// first frame and told it the real size. An empty tab for a program that plainly printed
    /// something is the one thing a run tile must not do, because the whole point of it is that the
    /// evidence outlives the process.
    ///
    /// So the size is worked out from the rectangle the tile really has — `RunPanel::tile`, which
    /// the window records every frame whether the tile is showing or not — through the same
    /// function the tile itself uses, and the two agree exactly. The guess underneath is only ever
    /// reached before the window has drawn a frame at all.
    fn run_grid_size(&self) -> quill_terminal::session::Size {
        let cell = self.renderer.cell_metrics(self.settings.terminal_font_size);
        let tile = match self.run.tile.width() > 1.0 && self.run.tile.height() > 1.0 {
            true => self.run.tile.size(),
            false => Vec2::new(self.editor_area.width().max(600.0), self.panes.run_height),
        };
        run_panel::grid_size(tile, cell)
    }

    /// Write the project's run configurations down, if this window is the one that may.
    fn remember_the_run_configurations(&mut self) {
        if !self.unsaved_run_configurations {
            return;
        }
        self.unsaved_run_configurations = false;
        if !self.remembers_this_project() {
            return;
        }
        run_configurations::save(self.tree.root(), &self.run_configurations);
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

    /// A run with no program behind it, fed bytes directly.
    ///
    /// What the screenshot tests use, exactly as [`Self::new_detached_terminal_tab`] is what the
    /// terminal's use and for the same reason: when a real program answers is not something a test
    /// can know, so a picture of a run is taken of an emulator that was handed fixed bytes.
    ///
    /// The configuration is kept as a temporary as well, so the widget and the menu list it — which
    /// is what a real run would have done.
    pub fn new_detached_run(&mut self, configuration: Configuration, rows: usize, columns: usize) -> usize {
        let cell = self.renderer.cell_metrics(self.settings.terminal_font_size);
        let size =
            quill_terminal::session::Size::new(rows, columns).with_cell(cell.width, cell.height);
        if self.run_configurations.find(&configuration.name).is_none() {
            self.run_configurations.add_temporary(configuration.clone());
        }
        self.run_selected = Some(configuration.name.clone());
        self.terminal.visible = false;
        self.run.visible = true;
        self.run.start_detached(configuration, size)
    }

    /// A debug session with no adapter behind it, fed messages directly.
    ///
    /// The third of the family, and it exists for the same reason [`Self::new_detached_run`] and
    /// [`Self::new_detached_terminal_tab`] do: when a real debugger answers is not something a test
    /// can know, so a picture of a paused program is taken of a session that was handed fixed
    /// messages. It goes through the same path a real session does — `begin`, and then every
    /// breakpoint in the project — so what a test drives is what the window really does.
    pub fn new_detached_debug_session(&mut self, adapter: &str, configuration: Configuration) {
        self.stop_debugging();
        if self.run_configurations.find(&configuration.name).is_none() {
            self.run_configurations.add_temporary(configuration.clone());
        }
        self.run_selected = Some(configuration.name.clone());
        let mut state = DebugState::detached(adapter, configuration);
        state.begin();
        self.debug = Some(state);
        self.send_every_breakpoint();
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
                // And wherever it was to stop. The document clamps these too, so a file rewritten
                // outside Quill gives a dot on the wrong line — which the adapter's `verified`
                // answer then says so about — rather than a panic in the layout engine.
                if let Some(breakpoints) = self.breakpoints.breakpoints(path).cloned() {
                    self.document_mut().set_breakpoints(breakpoints);
                }
                let revision = self.document().revision();
                self.files.active_mut().marked_revision = Some(revision);
                self.files.active_mut().breakpoints_at = Some(revision);
                // What the file looked like at the moment it was read, so a later read can tell that
                // something else has changed it since.
                self.files.active_mut().note_what_is_on_disk();
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
    pub(crate) const COLOUR_LIMIT: usize = 2 * 1024 * 1024;

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
        // **One** reading of the file, answering two questions. Colouring it and reading it for the
        // blocks that could be collapsed both want `syntax::scan` over the same text at the same
        // revision, and a second pass was worth 2.5 ms a keystroke on the largest file in this
        // repository. `quill_core::folding::Tokens` is the second answer, kept beside the first.
        let mut spans: Vec<(std::ops::Range<usize>, quill_core::Color)> = Vec::new();
        let mut tokens = quill_core::folding::Tokens::default();
        quill_core::syntax::scan(&text, &plugin.grammar, |range, token| {
            match token {
                quill_core::Token::Comment => tokens.note(range.clone(), true),
                quill_core::Token::String => tokens.note(range.clone(), false),
                _ => {}
            }
            if token != quill_core::Token::Text {
                if let Some(colour) = theme.colour(token) {
                    spans.push((range, colour));
                }
            }
        });
        let file = self.files.at_mut(index);
        file.document.set_syntax(base, &spans);
        // `set_syntax` bumps the revision, so what is remembered is the revision *after* it, or the
        // next frame would colour it all over again for ever. The tokens are keyed on that same
        // number, which is what lets `fold_regions` use them instead of reading the file again.
        let now = file.document.text_revision();
        file.coloured_revision = Some(now);
        file.cached.fold_tokens = Some((now, tokens));
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
                        answer: Answer::Git(Request::Rollback(vec![path])),
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
                answer: Answer::Git(Request::DropStash(name)),
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
                    answer: Answer::Git(Request::Reset { revision, mode }),
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
                answer: Answer::Git(Request::DeleteBranch { name, force: false }),
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

    /// Draw the one confirmation, and do what it asks when it is answered.
    ///
    /// Drawn before every other modal, because it is asked *over* whatever asked it, and drawn on
    /// its own rather than inside `show_git_windows`, because `task-1681` gave it a second kind of
    /// answer and a window with no repository behind it can now ask a question.
    fn show_the_confirmation(&mut self, ctx: &egui::Context) {
        let Some(question) = self.confirmation.clone() else {
            return;
        };
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
            self.confirmation = None;
            self.answer_the_question(question.answer);
        } else if outcome.cancelled {
            self.confirmation = None;
        }
    }

    /// Do what confirming a question does, and say what was done.
    ///
    /// One place rather than two, because the dialog is not the only thing that answers one:
    /// `quill-cli modal accept` presses the same button, and two arms that agreed today would be
    /// two arms that did not agree the day a fourth question was added. The sentence it returns is
    /// what the command line reports; the dialog throws it away, having already put anything worth
    /// saying in the status bar.
    pub fn answer_the_question(&mut self, answer: Answer) -> String {
        match answer {
            Answer::Git(request) => {
                let label = request.label();
                self.send_git(request);
                label
            }
            Answer::Delete(path) => {
                let name = path.display().to_string();
                self.delete_path(&path);
                format!("delete {name}")
            }
            Answer::RemoveRun(name) => {
                if let Some(at) = self.run.index_of(&name) {
                    self.run.close(at);
                }
                self.run_configurations.remove(&name);
                if self.run_selected.as_deref() == Some(name.as_str()) {
                    self.run_selected = None;
                }
                self.unsaved_run_configurations = true;
                self.message = Some(format!("Removed {name}"));
                format!("remove {name}")
            }
        }
    }

    /// Ask before throwing a file away.
    ///
    /// The question names what is about to go and where it is going, and for a folder it counts
    /// what is inside — the count is the fact that changes the answer. Where a deleted file goes is
    /// `services::recycle`'s to say, so the sentence is derived from it rather than written twice.
    pub fn ask_before_deleting(&mut self, path: &Path) {
        if !path.exists() {
            self.message = Some(format!("{} is not there.", path.display()));
            return;
        }
        if path == self.tree.root() {
            self.message = Some("The project folder itself cannot be deleted from here.".to_owned());
            return;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let what = if path.is_dir() {
            let inside = recycle::count_inside(path, 10_000);
            match inside {
                0 => format!("Delete {name}, which is empty."),
                1 => format!("Delete {name} and the 1 file in it."),
                _ => format!("Delete {name} and the {inside} files in it."),
            }
        } else {
            format!("Delete {name}.")
        };
        self.close_every_modal();
        // Nothing has happened yet, so whatever the status bar was saying about the last thing that
        // did is now misleading — and `quill-cli action run delete-path` answers with it, which
        // made asking the question report a deletion that had not happened.
        self.message = None;
        self.confirmation = Some(Confirmation {
            title: "Delete".to_owned(),
            note: format!("{what} {}", recycle::destination().reassurance()),
            button: "DELETE".to_owned(),
            answer: Answer::Delete(path.to_path_buf()),
        });
    }

    /// Throw a file or a folder away, and tidy up after it.
    ///
    /// Every tab on the file — or on anything under the folder — is closed **without** the save
    /// `close_tab` does, because writing a file in order to throw it away is not a thing to do. The
    /// project's marks for those paths go with them, and the index is told the project changed.
    pub fn delete_path(&mut self, path: &Path) {
        match recycle::delete(path) {
            Ok(()) => {
                let gone: Vec<PathBuf> = self
                    .files
                    .paths()
                    .into_iter()
                    .filter(|open| open == path || open.starts_with(path))
                    .collect();
                for open in &gone {
                    if let Some(index) = self.files.index_of(open) {
                        self.close_tab_without_saving(index);
                    }
                    self.marks.forget(open);
                }
                if self.selected.as_deref() == Some(path) {
                    self.selected = path.parent().map(Path::to_path_buf);
                }
                self.tree.reload();
                self.the_project_changed_on_disk();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                self.message = Some(format!("Deleted {name} to {}", recycle::destination().name()));
            }
            Err(problem) => {
                self.message =
                    Some(format!("Quill could not delete {}: {problem}", path.display()))
            }
        }
    }

    /// Move a file or a folder, and take the code that names it with it.
    ///
    /// `to` is where the thing itself lands, not the folder it was dropped into, because a name
    /// already taken in the destination has to be settled before anything is planned.
    ///
    /// The order matters. The plan is worked out **first**, against the project as it is, because
    /// every specifier in it is resolved against files that are still where they were. Then the
    /// bytes move. Then the edits are applied, following `task-1675`'s ownership rule: an open file
    /// is edited as a document and left modified, and a closed file is read, checked and written
    /// once.
    pub fn move_path(&mut self, from: &Path, to: &Path, refactor: bool) -> bool {
        if from == to {
            return false;
        }
        if to.exists() {
            self.message = Some(format!("{} is already there", to.display()));
            return false;
        }
        let plan = match refactor {
            true => self.plan_a_move(from, to),
            false => file_move::Plan::default(),
        };
        if let Some(folder) = to.parent() {
            if let Err(problem) = std::fs::create_dir_all(folder) {
                self.message = Some(format!("Quill could not make {}: {problem}", folder.display()));
                return false;
            }
        }
        if let Err(problem) = move_the_bytes(from, to) {
            self.message = Some(format!("Quill could not move {}: {problem}", from.display()));
            return false;
        }
        // The tabs follow the file before anything is written, so a tab on a moved file is edited
        // at its new path rather than at one with nothing behind it.
        self.retarget_the_tabs(&plan.moved);
        self.marks.moved(&plan.moved);
        let report = self.apply_a_move(&plan);
        self.tree.reload();
        if let Some(folder) = to.parent() {
            self.tree.expand(folder);
        }
        self.the_project_changed_on_disk();
        self.selected = Some(to.to_path_buf());
        let name = from
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| from.display().to_string());
        let where_to = to
            .parent()
            .and_then(|folder| folder.strip_prefix(self.tree.root()).ok())
            .map(|folder| folder.display().to_string())
            .filter(|folder| !folder.is_empty())
            .unwrap_or_else(|| "the project".to_owned());
        let mut said = format!("Moved {name} to {where_to} \u{00B7} {}", plan.sentence());
        for note in plan.notes.iter().chain(report.iter()) {
            said.push_str(&format!(" \u{00B7} {note}"));
        }
        self.message = Some(said);
        true
    }

    /// Work out what a move would change, without changing anything.
    ///
    /// The reader it hands the planner is where the ownership rule enters: a file that is open
    /// answers with the text in its tab, and every other file answers with what is on the disk.
    pub fn plan_a_move(&self, from: &Path, to: &Path) -> file_move::Plan {
        let files = self.tree.all_files().to_vec();
        let project = imports::Project { root: self.tree.root(), files: &files };
        let open: Vec<(PathBuf, String)> = self
            .files
            .iter()
            .filter_map(|file| {
                file.path().map(|path| (path.to_path_buf(), file.document.text().to_string()))
            })
            .collect();
        let read = |path: &Path| -> Option<String> {
            if let Some((_, text)) = open.iter().find(|(known, _)| known == path) {
                return Some(text.clone());
            }
            std::fs::read_to_string(path).ok()
        };
        file_move::plan(&project, &self.plugins.grammars(), from, to, &read)
    }

    /// Point every tab that was on a moved file at where the file went.
    fn retarget_the_tabs(&mut self, moved: &[(PathBuf, PathBuf)]) {
        for (old, new) in moved {
            let Some(index) = self.files.index_of(old) else {
                continue;
            };
            self.files.at_mut(index).document.set_path(new.clone());
            self.files.at_mut(index).forget_what_was_worked_out();
        }
    }

    /// Apply a plan's edits, and say what could not be applied.
    ///
    /// An open file is one `Command::ReplaceMany`, which is one undo step, and is left **modified
    /// rather than written**: a refactor must never silently write a buffer somebody was editing.
    /// A closed file is read, every range is checked to still hold what the plan expected, and only
    /// then is it written once — and a file that changed underneath the plan is skipped whole and
    /// named rather than patched on faith.
    fn apply_a_move(&mut self, plan: &file_move::Plan) -> Vec<String> {
        let mut skipped = Vec::new();
        for file in &plan.files {
            if file.edits.is_empty() {
                continue;
            }
            match self.files.index_of(&file.path) {
                Some(index) => {
                    let edits = file.edits.clone();
                    self.files.at_mut(index).document.apply(Command::ReplaceMany(edits));
                }
                None => {
                    if let Err(reason) = write_the_edits(&file.path, &file.edits) {
                        let name = file
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_default();
                        skipped.push(format!("{name} was left alone: {reason}"));
                    }
                }
            }
        }
        skipped
    }

    /// The keys the explorer takes, and only while it has the keyboard.
    ///
    /// `Up` and `Down` walk the rows that are showing, so a row inside a shut folder is never
    /// stepped onto; `Right` opens a folder and `Left` shuts it or steps to its parent; `Enter`
    /// opens the file permanently and hands the keyboard to the editor; `Escape` hands it back
    /// without opening anything; and `Delete` — or `Backspace`, which is the key a Mac keyboard has
    /// — asks the question.
    fn route_the_explorer_keys(&mut self, ui: &egui::Ui) -> Option<Action> {
        if self.focus != Focus::Explorer || !self.explorer_visible {
            return None;
        }
        // A modal is open: its keys are its own. Without this, `Delete` in the explorer opens the
        // confirmation and the `Enter` that answers it also opens the row the cursor is on.
        if a_modal_has_the_keyboard(ui.ctx()) {
            return None;
        }
        // A field with the keyboard is typed into, not navigated with. The filter box is the only
        // one in the panel, and while it has the focus its own arrow keys move the caret.
        //
        // The question is whether a **text box** has the keyboard, not whether anything at all has
        // the focus: `hold_the_keyboard` keeps the focus on a widget of Quill's own the rest of the
        // time, and the broader question would leave the tree unable to be walked at all.
        if text_box_has_the_keyboard(ui.ctx()) {
            return None;
        }
        // A letter typed while the tree has the keyboard belongs to the **editor**. The explorer has
        // no use for one, and "click a file in the tree and start typing" has to go on working
        // exactly as it did — the keyboard is handed over here, before any pane reads the frame's
        // input, so the letter that caused it lands in the document.
        if ui.input(|input| input.events.iter().any(|event| matches!(event, egui::Event::Text(_))))
        {
            self.focus = Focus::Editor;
            return None;
        }
        let key = ui.input(|input| {
            [
                egui::Key::ArrowDown,
                egui::Key::ArrowUp,
                egui::Key::ArrowRight,
                egui::Key::ArrowLeft,
                egui::Key::Enter,
                egui::Key::Escape,
                egui::Key::Delete,
            ]
            .into_iter()
            .find(|key| input.key_pressed(*key))
        })
        .or_else(|| {
            // The Mac keyboard has no `Delete`, and `Backspace` on its own is far too close to what
            // somebody who has just clicked a file is about to type. IntelliJ's own answer on macOS
            // is the command key with it, and that is unambiguous on every platform.
            ui.input(|input| {
                input.key_pressed(egui::Key::Backspace) && input.modifiers.command
            })
            .then_some(egui::Key::Delete)
        })?;
        match key {
            egui::Key::ArrowDown => self.step_the_selection(1),
            egui::Key::ArrowUp => self.step_the_selection(-1),
            egui::Key::ArrowRight => self.open_the_selected_folder(true),
            egui::Key::ArrowLeft => self.open_the_selected_folder(false),
            egui::Key::Enter => {
                let path = self.selected.clone()?;
                if path.is_dir() {
                    self.tree.toggle(&path);
                } else {
                    self.open_path_permanently(&path);
                    self.focus = Focus::Editor;
                }
            }
            egui::Key::Escape => self.focus = Focus::Editor,
            egui::Key::Delete => {
                return self.selected.clone().map(Action::DeletePath);
            }
            _ => {}
        }
        None
    }

    /// Move the explorer's cursor by `step` rows, through the rows that are showing.
    fn step_the_selection(&mut self, step: isize) {
        let rows: Vec<PathBuf> = self.explorer_rows();
        if rows.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|path| rows.iter().position(|row| row == path))
            .map(|at| (at as isize + step).clamp(0, rows.len() as isize - 1) as usize)
            .unwrap_or(if step > 0 { 0 } else { rows.len() - 1 });
        self.selected = Some(rows[at].clone());
        self.reveal_selection = REVEAL_FRAMES;
    }

    /// `Right` opens the folder the cursor is on; `Left` shuts it, or steps to the folder above.
    fn open_the_selected_folder(&mut self, open: bool) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let showing = self.tree.find(&path).map(|entry| entry.expanded).unwrap_or(false);
        if path.is_dir() && showing != open {
            self.tree.toggle(&path);
            return;
        }
        if !open {
            if let Some(folder) = path.parent() {
                if folder.starts_with(self.tree.root()) && folder != self.tree.root() {
                    self.selected = Some(folder.to_path_buf());
                    self.reveal_selection = REVEAL_FRAMES;
                }
            }
        }
    }

    /// The rows the explorer is showing, in order — the same list it draws.
    fn explorer_rows(&self) -> Vec<PathBuf> {
        if self.filter.trim().is_empty() {
            self.tree.rows().iter().map(|row| row.entry.path.clone()).collect()
        } else {
            self.tree.matching(&self.filter).iter().map(|path| path.to_path_buf()).collect()
        }
    }

    /// Read a path again from disk: the folder it is in, and the file itself if it is open.
    ///
    /// Unsaved changes are kept rather than thrown away. A person asking to reload has asked for
    /// what is on disk, but nothing in the entry says "and lose what I typed", and quietly losing an
    /// edit is not a thing an editor should do without asking. So a file with unsaved changes says
    /// so and is left alone.
    /// Read the tab that is showing again when its file has changed underneath it.
    ///
    /// Quill watches nothing and a tab is owned by its `Document`, which is the right rule while
    /// Quill is the only thing writing. It is the wrong answer the moment something else does: the tab
    /// went on showing text that was no longer in the file, and `editor text` answered with it — so a
    /// caller that wrote a file and read it back through the window was handed what it had replaced.
    ///
    /// Checked at the moment of use rather than watched, which is the rule
    /// `services::symbol_index` already follows for a closed file. The cost is one `metadata` call on
    /// the command that reads, and nothing at all on a frame.
    ///
    /// A tab with unsaved changes is never touched. Those belong to the person, and throwing them
    /// away has no undo — `tab reload --discard` is how somebody says they mean it.
    pub fn reread_if_the_file_changed(&mut self) {
        if !self.files.active().the_file_changed_underneath() {
            return;
        }
        let Some(path) = self.document().path().map(Path::to_path_buf) else { return };
        self.reload_from_disk(&path, false);
    }

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
                    file.note_what_is_on_disk();
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
            Purpose::NewFolder(folder) => {
                // The same shape `NewFile` has, with `create_dir_all` in place of writing an empty
                // file. There is nothing to open in a tab, so what it does instead is open the new
                // folder out in the tree and put the explorer's cursor on it — which is where
                // somebody who has just made a folder is about to make a file.
                let target = crate::services::file_clipboard::free_name(&folder, &name);
                match std::fs::create_dir_all(&target) {
                    Ok(()) => {
                        self.tree.reload();
                        self.tree.expand(&target);
                        self.selected = Some(target.clone());
                        self.reveal_selection = REVEAL_FRAMES;
                        self.message = Some(format!("Made {}", target.display()));
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
                // A rename **is** a move to a new name, so it goes through the same function a drag
                // does and the code that names the file follows it. A rename that updated no
                // references while a drag did would be two answers to one question.
                if self.move_path(&path, &target, true) {
                    let said = self.message.take().unwrap_or_default();
                    let rest = said.split_once('\u{00B7}').map(|(_, rest)| rest).unwrap_or("");
                    self.message = Some(match rest.trim().is_empty() {
                        true => format!("Renamed to {name}"),
                        false => format!("Renamed to {name} \u{00B7} {}", rest.trim()),
                    });
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
            Purpose::RenameTerminalTab(index) => {
                if self.terminal.tabs.rename(index, &name) {
                    self.message = Some(format!("Terminal tab {index} is called {name}"));
                }
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
    /// Close the tab at `index`, and show whatever is left.
    ///
    /// **A tab with unsaved changes is written first**, which is what `task-1681` asks for: *"If I
    /// close a tab that has been edited but not saved, it should save and close."* Every other
    /// editor puts a three-answer dialog here; Quill can give the simpler answer because it saves
    /// plain text and nothing else, so writing the buffer to the file it came from is exactly what
    /// was typed. There is no format conversion to get wrong and no decision for a dialog to ask
    /// about.
    ///
    /// This is the one place a tab is closed — the cross on the tab, `Ctrl+W`, the tab's own menu
    /// and `quill-cli tab close` all reach it — so it is one change in one function.
    pub fn close_tab(&mut self, index: usize) {
        self.save_before_closing(index);
        self.files.close(index);
        self.forget_layout();
    }

    /// Write a tab that is about to be closed, if it has unsaved changes and somewhere to put them.
    ///
    /// Two tabs are deliberately left alone. **A picture** holds an empty document over the
    /// picture's path, so writing it would put nothing over the file — `save` already refuses for
    /// this reason. **A tab with no path** has nowhere to be written, and choosing one is a dialog,
    /// which is the thing this is removing; it says so rather than writing `untitled.md` into
    /// somebody's project because they shut a scratch buffer.
    fn save_before_closing(&mut self, index: usize) {
        let Some(file) = self.files.get(index) else {
            return;
        };
        if file.is_picture() || !file.document.is_modified() {
            return;
        }
        let Some(path) = file.path().map(Path::to_path_buf) else {
            self.message = Some(
                "That tab has no file to save to, so it was closed without saving.".to_owned(),
            );
            return;
        };
        let name = file.name();
        match self.files.at_mut(index).document.save() {
            Ok(()) => {
                self.files.at_mut(index).note_what_is_on_disk();
                self.message = Some(format!("Saved {name}"));
                // The disk is what the index holds for every file that is not open, and this one is
                // about to stop being open.
                self.the_project_changed_on_disk();
            }
            Err(problem) => {
                self.message =
                    Some(format!("Quill could not save {}: {problem}", path.display()))
            }
        }
    }

    /// Close a tab without writing it, which is what deleting its file means and what
    /// `quill-cli tab close --discard` asks for.
    pub fn close_tab_without_saving(&mut self, index: usize) {
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
                self.files.active_mut().note_what_is_on_disk();
                self.tree.reload();
                self.the_project_changed_on_disk();
            }
            return;
        }
        let _ = self.document_mut().save();
        // Written by this tab, so this tab and the file agree again: without this the tab's own write
        // would look like somebody else's change and the next read would re-read what it just wrote.
        self.files.active_mut().note_what_is_on_disk();
        // The file on the disk is what the index holds for every file that is not open, and this
        // one is about to stop being open one day. Reading the project again is tens of
        // milliseconds on a thread, and saving is not something anybody does sixty times a second.
        self.the_project_changed_on_disk();
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

    /// Which line of the source each line of the preview came from, for a test. See
    /// `quill_core::Preview::source_lines`, which is what the two halves of the side by side view
    /// are scrolled together through.
    pub fn preview_source_lines(&self) -> Vec<usize> {
        self.files
            .active()
            .cached
            .preview
            .as_ref()
            .map(|preview| preview.source_lines.clone())
            .unwrap_or_default()
    }

    /// The style covering one byte of the preview's text, for a test.
    pub fn preview_style_at(&self, at: usize) -> quill_core::CharStyle {
        self.files
            .active()
            .cached
            .preview
            .as_ref()
            .map(|preview| preview.chars.style_at(at).clone())
            .unwrap_or_default()
    }

    /// The panels the preview asked for, for a test.
    pub fn preview_panels(&self) -> Vec<quill_core::PreviewPanel> {
        self.files
            .active()
            .cached
            .preview
            .as_ref()
            .map(|preview| preview.panels.clone())
            .unwrap_or_default()
    }

    /// Where the inline code is, for a test.
    pub fn preview_code_spans(&self) -> Vec<std::ops::Range<usize>> {
        self.files
            .active()
            .cached
            .preview
            .as_ref()
            .map(|preview| preview.code_spans.clone())
            .unwrap_or_default()
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
        let mono = self.renderer.monospaced_family();
        // How many characters of the code font fit across the pane, which is the one measurement a
        // table takes. Everything else about a table is integer arithmetic over characters, which is
        // what `markdown::table` is for and why it is testable with no fonts.
        let mut preview = {
            let highlighter = PluginHighlighter { plugins: &self.plugins };
            let code = quill_core::CharStyle {
                family: mono.clone().unwrap_or_else(|| base.family.clone()),
                size: base.size * 0.95,
                ..quill_core::CharStyle::default()
            };
            let advance = quill_core::FontMetrics::advance(&self.renderer, "M", &code).max(1.0);
            let options = quill_core::PreviewOptions {
                base: base.clone(),
                colors,
                mono,
                columns: (width / advance).floor().max(16.0) as usize,
                highlighter: Some(&highlighter),
            };
            quill_core::markdown::render(&self.document().text().to_string(), &options)
        };
        let pictures = self.read_the_pictures(ctx, &mut preview, width);
        let diagrams = self.lay_the_diagrams_out(ctx, &mut preview, width);
        let laid = layout(
            &preview.text,
            &preview.chars,
            &preview.paragraphs,
            &self.renderer,
            width,
        );
        // A byte range into text that has been rebuilt means nothing, so a selection in the preview
        // does not survive an edit. It does survive a scroll and a resize, which is where a person
        // actually loses one — and the clamp is what makes a resize safe, since a table laid out at
        // a new width is a different number of bytes.
        let rebuilt = revision != self.files.active().cached.preview_revision;
        let length = preview.text.len_bytes();
        let file = self.files.active_mut();
        if rebuilt {
            file.preview_selection = quill_core::Selection::caret(0);
        } else {
            file.preview_selection.anchor = file.preview_selection.anchor.min(length);
            file.preview_selection.head = file.preview_selection.head.min(length);
        }
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
        // And the fold revision beside it, because collapsing a block changes the layout without
        // changing a byte of the text. It is a counter of its own so that a fold does not re-colour
        // the file or rebuild the preview — `tasks/task-1686-folding-tdd.md` section 5.1.
        let folded = self.document().fold_revision();
        let cached = &self.files.active().cached;
        if !cached.stale
            && revision == cached.laid_out_revision
            && folded == cached.laid_out_folds
            && (width - cached.laid_out_width).abs() < 0.5
        {
            return;
        }
        let index = self.files.active_index();
        let hidden = self.hidden_paragraphs(index);
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
            &hidden,
        );
        let cached = &mut self.files.active_mut().cached;
        cached.stale = false;
        cached.layout = laid;
        cached.laid_out_revision = revision;
        cached.laid_out_folds = folded;
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
        // Everything the adapter has said since the last frame, taken beside git's own replies
        // because it is the same kind of thing: a thread has answered and the window has to draw it.
        self.take_the_debug_replies(ui.ctx());
        self.colour_the_open_file();
        // The project's definitions, read on a thread. Beside the colouring because it is the same
        // kind of thing — what the files say, worked out from what they hold — and because both are
        // keyed on something cheap enough to ask about every frame.
        self.keep_the_symbol_index_fresh();
        // Before the explorer is drawn, so a file another program has just made is in the tree on
        // this frame rather than the next one.
        self.notice_what_changed_on_disk();
        // Where the window is, so the project can be opened here again next time — `task-1693`.
        self.note_where_the_window_is(ui.ctx());
        // Before the explorer is drawn, so the folders it needs are already open on this frame.
        self.follow_the_open_file();
        // Before any button is drawn, so that on the very first frame the focus is here and not on the
        // first thing in the title bar.
        hold_the_keyboard(ui);
        // Always ask to be woken again. See `HEARTBEAT`.
        ui.ctx().request_repaint_after(HEARTBEAT);
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
        // The run widget takes the right hand end and the text tools sit in front of it, so the play
        // and the bug are in the same place whatever file is open — `task-1693`. How much room each
        // wants is worked out first, because the tools have to know where the run widget starts.
        let run_state = self.run_widget_state();
        let run_width = run_widget::width(&run_state);
        let tools_rect =
            title_bar::tools_rect(title_rect, self.menu_placement, tools_width, run_width);
        let run_rect = title_bar::run_rect(title_rect, self.menu_placement, run_width);
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

        // One tile takes the bottom of the panes across their whole width, as the terminal does in
        // IntelliJ, and the explorer and the editing area share what is left. **One**: the terminal
        // tile or the run tile, never both stacked, because two grids stacked take the editing area
        // below the fold of anything.
        let tile_height = if self.terminal.visible {
            self.panes
                .terminal_height
                .clamp(settings::TERMINAL_MIN, (panes.height() - 120.0).max(settings::TERMINAL_MIN))
        } else if self.run.visible {
            self.panes
                .run_height
                .clamp(settings::RUN_MIN, (panes.height() - 120.0).max(settings::RUN_MIN))
        } else if self.debug_panel.visible {
            self.panes
                .debug_height
                .clamp(settings::DEBUG_MIN, (panes.height() - 120.0).max(settings::DEBUG_MIN))
        } else {
            0.0
        };
        let upper =
            Rect::from_min_max(panes.min, Pos2::new(panes.right(), panes.bottom() - tile_height));
        let terminal_rect =
            Rect::from_min_max(Pos2::new(panes.left(), upper.bottom()), panes.max);
        // Where the run tile is, recorded whether it is showing or not, so that a run started while
        // it is put away is still opened at the size it will be drawn at. See `run_grid_size`.
        self.run.tile = match self.run.visible {
            true => terminal_rect,
            // The rectangle it *would* have, which is what it will be given the moment something is
            // run: showing the tile is what starting a run does.
            false => Rect::from_min_max(
                Pos2::new(
                    panes.left(),
                    panes.bottom()
                        - self
                            .panes
                            .run_height
                            .clamp(settings::RUN_MIN, (panes.height() - 120.0).max(settings::RUN_MIN)),
                ),
                panes.max,
            ),
        };

        // Where the debug tile is, recorded whether it is showing or not, for the reason the run
        // tile's rectangle is recorded: a session started while it is put away still has to know how
        // much room its tree will have.
        self.debug_panel.tile = match self.debug_panel.visible {
            true => terminal_rect,
            false => Rect::from_min_max(
                Pos2::new(
                    panes.left(),
                    panes.bottom()
                        - self.panes.debug_height.clamp(
                            settings::DEBUG_MIN,
                            (panes.height() - 120.0).max(settings::DEBUG_MIN),
                        ),
                ),
                panes.max,
            ),
        };

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
            run_width,
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

        // The run widget, drawn over the title bar after the bar for the reason the text tools are:
        // the bar takes drags over the room between the menus and the buttons to move the window,
        // and a control added earlier would sit underneath that and never be pressed.
        if run_width > 0.0 {
            let chosen = {
                let mut run_ui = ui.new_child(egui::UiBuilder::new().max_rect(run_rect));
                run_widget::show(&mut run_ui, run_rect, &run_state)
            };
            if let Some(chosen) = chosen {
                action = Some(chosen);
            }
        }

        // The rail of pane buttons down the far left.
        {
            let state = activity_bar::RailState {
                explorer_visible: self.explorer_visible,
                git_open: self.git.as_ref().is_some_and(|git| git.panel.open),
                in_repository: self.git.is_some(),
                terminal_visible: self.terminal.visible,
                run_visible: self.run.visible,
                debug_visible: self.debug_panel.visible,
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
                // The same one shot for the explorer's own cursor, which the arrow keys move
                // without opening anything, so nothing else would scroll to it.
                let reveal_selected = self.reveal_selection > 0;
                self.reveal_selection = self.reveal_selection.saturating_sub(1);
                let selected = self.selected.clone();
                if self.reveal_selection > 0 {
                    ui.ctx().request_repaint();
                }
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
                    explorer::View {
                        current: open.as_deref(),
                        selected: selected.as_deref(),
                        keyboard: self.focus == Focus::Explorer,
                        unsaved,
                        reveal,
                        reveal_selected,
                        opacity: self.settings.opacity,
                    },
                    &decorate,
                )
            };
            if let Some(path) = explorer_outcome.select {
                self.selected = Some(path);
            }
            if explorer_outcome.focus {
                self.focus = Focus::Explorer;
            }
            if let Some(path) = explorer_outcome.toggle {
                self.tree.toggle(&path);
            }
            // A single click opens the file and leaves the keyboard here, which is VS Code's own
            // behaviour and is what makes `Down` `Down` `Down` a way to look through a folder. A
            // double click is somebody going to the editor, so the keyboard goes with them.
            if let Some(path) = explorer_outcome.open {
                self.open_path(&path);
            }
            if let Some(path) = explorer_outcome.open_permanently {
                self.open_path_permanently(&path);
                self.focus = Focus::Editor;
            }
            if let Some((source, folder)) = explorer_outcome.moved {
                let name = source
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                let target = folder.join(name);
                self.move_path(&source, &target, true);
            }
            if explorer_outcome.hide {
                self.explorer_visible = false;
            }
            self.dragging_a_row = explorer_outcome.dragging;
            if let Some((at, path, directory)) = explorer_outcome.context_menu {
                let aimed = match explorer_outcome.menu_over_empty_space {
                    true => actions::Aim::AtEmptySpace,
                    false => actions::Aim::AtARow,
                };
                self.explorer_menu = Some((at, path, directory, aimed));
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
        // The completion popup's five keys, taken out of the frame's input **before** any pane reads
        // it, which is the one-frame ordering `Find in Files` and `Go to File` already rely on: a
        // key the popup takes never reaches `editor_view::handle_input`. Everything else flows
        // through untouched.
        self.route_the_completion_keys(ui);
        // The explorer's own keys, for the same reason and in the same place: they are read before
        // any pane is drawn, and only while the explorer has the keyboard, so `Delete` can never
        // mean two things at once.
        if let Some(chosen) = self.route_the_explorer_keys(ui) {
            action = Some(chosen);
        }
        // And the copy, when what is selected is in a preview rather than in a document. Before the
        // panes for the same reason again: in the side-by-side view the source is drawn first and
        // would otherwise take the event and copy its own selection instead.
        self.route_the_preview_copy(ui);
        let pane_rects = self.pane_rects(editing_area);
        let had_the_keyboard = self.files.focused_pane();
        let mut keyboard = had_the_keyboard;
        self.tab_strips.clear();
        self.tab_drag = None;
        // Rebuilt by the pane that has the keyboard, which is the only thing that can know it.
        self.completion_anchor = None;
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
        // Where a tab being carried would land, and where it did. After the loop, because a tab
        // picked up in one pane is dropped on another as often as not.
        self.settle_the_tab_drag(ui, &pane_rects);
        // The completion popup, drawn from the geometry the pane with the keyboard recorded. After
        // the loop for the reason the tab drag is settled after it: this is the first moment
        // anything knows where that pane's caret ended up, and one popup drawn here can never be
        // underneath a divider or drawn twice in a split view.
        self.show_the_completion(ui);
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
        if let Some((at, path, directory, aimed)) = self.explorer_menu.clone() {
            let entries = actions::explorer_menu_with_git(
                &self.menu_state(),
                &path,
                directory,
                !self.clipboard.is_empty(),
                aimed,
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
            let mut below = actions::clear_highlight_menu(&state);
            // The ticket's own two rows, under the marks they are about: `Collapse All But
            // Highlighted` is worth something only to somebody who has just marked a passage, and
            // this is where they are already pointing.
            let folding = actions::folding_here_menu(&state);
            if !folding.is_empty() {
                below.push(actions::Entry::Separator);
                below.extend(folding);
            }
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
            if let Some((index, at)) = panel_outcome.menu {
                self.terminal_menu = Some((at, index));
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
        // The run tile, in the same place and never at the same time.
        if self.run.visible {
            let panel_outcome = {
                let mut panel_ui = ui.new_child(egui::UiBuilder::new().max_rect(terminal_rect));
                panel_ui.set_clip_rect(terminal_rect);
                let font_size = self.settings.terminal_font_size;
                run_panel::show(
                    &mut panel_ui,
                    terminal_rect,
                    &mut self.run,
                    &self.renderer,
                    font_size,
                    self.settings.opacity,
                )
            };
            if panel_outcome.drag != 0.0 {
                let limit = (body.height() - 120.0).max(settings::RUN_MIN);
                self.panes.run_height =
                    (self.panes.run_height - panel_outcome.drag).clamp(settings::RUN_MIN, limit);
                self.unsaved_settings = true;
            }
            if panel_outcome.reset_height {
                self.panes.run_height = settings::RUN_HEIGHT;
                self.unsaved_settings = true;
            }
            if panel_outcome.take_focus {
                self.focus = Focus::Terminal;
                // Clicking a run's tab is choosing it, so the widget and the tile agree about what
                // `Run` with no name means.
                if let Some(run) = self.run.active() {
                    self.run_selected = Some(run.name().to_owned());
                }
            }
            if let Some(text) = panel_outcome.copy {
                ui.ctx().copy_text(text);
            }
            if panel_outcome.stop {
                self.message =
                    self.run.active().map(|run| format!("Stopping {}", run.name()));
            }
            if panel_outcome.rerun {
                let name = self.run.active().map(|run| run.name().to_owned());
                if let Some(name) = name {
                    action = Some(Action::Run(RunAction::Rerun(Some(name))));
                }
            }
            if panel_outcome.hide {
                self.show_the_run_tile(false);
            }
        }
        // The debug tile, in the same place and never at the same time as either of the other two.
        if self.debug_panel.visible {
            // Worked out before the tile is drawn, because it is what the tile says when there is no
            // session and because the search behind it is a cache the window owns.
            let idle = self.debug_idle();
            let outcome = {
                let mut panel_ui = ui.new_child(egui::UiBuilder::new().max_rect(terminal_rect));
                panel_ui.set_clip_rect(terminal_rect);
                let opacity = self.settings.opacity;
                // The panel and the session are borrowed apart, which is what lets a component take
                // its own state mutably and what it draws immutably — the shape every component in
                // Quill has.
                let DebugSplit { panel, debug } = split_the_debug(self);
                debug_panel::show(&mut panel_ui, terminal_rect, panel, debug, &idle, opacity)
            };
            if outcome.drag != 0.0 {
                let limit = (body.height() - 120.0).max(settings::DEBUG_MIN);
                self.panes.debug_height =
                    (self.panes.debug_height - outcome.drag).clamp(settings::DEBUG_MIN, limit);
                self.unsaved_settings = true;
            }
            if outcome.reset_height {
                self.panes.debug_height = settings::DEBUG_HEIGHT;
                self.unsaved_settings = true;
            }
            self.act_on_the_debug_tile(outcome, ui.ctx());
        }
        // What every program has said since the last frame, and the hard kill that follows a polite
        // stop nobody answered. Outside the `visible` test on purpose: a program that is running
        // has to be read whether or not anybody is looking at it, or its output would arrive in a
        // rush the moment the tile came back up.
        if self.run.settle() {
            ui.ctx().request_repaint();
        }
        if let Some(left) = self.run.stopping_in() {
            // An idle window draws nothing, and the grace has to actually run out — so the window
            // is woken once, when it does, rather than kept awake for the whole two seconds.
            ui.ctx().request_repaint_after(left);
        }
        self.run.focused = self.focus == Focus::Terminal && self.run.visible;

        // A terminal tab's own menu, drawn after the tile so it sits over it rather than under.
        if let Some((at, _)) = self.terminal_menu {
            let entries = actions::terminal_tab_menu();
            let outcome = context_menu::show(ui, "terminal-tab", at, &entries);
            if let Some(chosen) = outcome.chosen {
                action = Some(chosen);
            }
            if outcome.close {
                self.terminal_menu = None;
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
        // The one confirmation, drawn over whatever asked it and before every other modal.
        self.show_the_confirmation(ui.ctx());
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

        // The references, the candidate list and the rename, which are one modal wearing three
        // faces and are drawn beside the two above for the same reason: a question about the
        // project, asked over the top of it.
        self.show_the_references(ui);

        // The About box. Only one modal is open at a time — `Action::About` shuts whatever was —
        // so where it is drawn among the others decides nothing; it is here because it is the same
        // kind of thing as the two above, a small window over the project rather than about a file.
        if let Some(about) = self.about.take() {
            if !about_dialog::show(ui.ctx(), &about) {
                self.about = Some(about);
            }
        }

        // The two debug modals, beside the other project-wide dialogs and for the same reason: only
        // one is ever open, so where they are drawn among the others decides nothing.
        self.show_the_debug_modals(ui.ctx());

        // The `Run Configurations` modal, drawn beside the other project-wide dialogs.
        {
            let running: Vec<String> = self
                .run
                .runs()
                .iter()
                .filter(|run| run.is_running())
                .map(|run| run.name().to_owned())
                .collect();
            let mut dialog = std::mem::take(&mut self.run_dialog);
            let outcome =
                run_dialog::show(ui.ctx(), &mut dialog, &mut self.run_configurations, &running);
            self.run_dialog = dialog;
            if outcome.changed {
                self.unsaved_run_configurations = true;
            }
            if let Some(name) = outcome.remove {
                self.run_configurations.remove(&name);
                if self.run_selected.as_deref() == Some(name.as_str()) {
                    self.run_selected = None;
                }
                self.unsaved_run_configurations = true;
            }
            if let Some(name) = outcome.confirm_removal {
                // The same furniture the git dialogs use, because silently killing a server
                // somebody is watching is worse than one extra click.
                self.confirmation = Some(Confirmation {
                    title: "Remove".to_owned(),
                    note: format!("{name} is running. Removing it stops the program first."),
                    button: "REMOVE".to_owned(),
                    answer: Answer::RemoveRun(name),
                });
            }
            if outcome.closed {
                self.unsaved_run_configurations = true;
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
            // What the MCP page's status line reads. A window that never opened an endpoint — every
            // window a test builds — reads as off, which is what it is.
            self.mcp.as_ref().map(|hosted| hosted.state()).unwrap_or(&services::mcp::State::Off),
            &quill_cli::mcp::install::quill_cli_program(),
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
        // Nothing is added at all while the window is maximised, which is Quill's rule for a
        // control that cannot apply and — the part that matters — is what stops a request the window
        // manager will refuse ever being sent. `components::resize_edges` records what one of those
        // costs: it wedges every later move and resize as well.
        let maximized = ui.ctx().input(|input| input.viewport().maximized.unwrap_or(false));
        if let Some(direction) = resize_edges::show(ui, full, maximized) {
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
        // And where it stops, on the same terms again.
        self.remember_the_breakpoints(settled);

        // Settings are written once the pointer is up, so that dragging a divider or a slider writes the
        // file once at the end rather than on every frame of the drag.
        if self.unsaved_settings && !ui.input(|input| input.pointer.any_down()) {
            self.write_settings();
        }
        // And the project's run configurations, on exactly the same terms: typing into a field in
        // the dialog would otherwise write the file on every keystroke.
        if settled {
            self.remember_the_run_configurations();
        }
    }

    /// What the run widget in the title bar needs to know to draw itself.
    ///
    /// Worked out here rather than in the widget for the reason every component in Quill decides
    /// nothing: the widget draws what it is handed and reports what was pressed.
    fn run_widget_state(&self) -> run_widget::WidgetState {
        let rows = self.run_rows();
        let running = self
            .run_selected
            .as_deref()
            .and_then(|name| self.run.index_of(name))
            .and_then(|at| self.run.at(at))
            .is_some_and(run_panel::Run::is_running);
        // `Run Current File` names the file it would run, so the row says what pressing it will do.
        let current_file = self.run_file_template().and_then(|_| {
            self.document()
                .path()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
        });
        // The bug button is there when the configuration the play button would start resolves to a
        // debugger — asked of the thing the button acts on rather than of whichever tab is focused,
        // so it does not come and go as tabs are switched. `task-1692` §4.
        let debuggable = self
            .configuration_named(None)
            .or_else(|| self.suggestions().into_iter().next())
            .and_then(|configuration| {
                let adapter = self.adapter_for(&configuration, None)?;
                debuggers::can_debug(&adapter, &configuration.command).then_some(())
            })
            .is_some()
            || self.debug_applies_here();
        run_widget::WidgetState {
            selected: self.run_selected.clone(),
            rows,
            running,
            debuggable,
            current_file,
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
        // A `debug.<name>` may have moved, and the search that found the old one is now a lie.
        self.forget_the_adapter_search();
        if self.settings.font_family != before.font_family || self.settings.font_size != before.font_size
        {
            self.set_the_font_everywhere();
        }
        // The MCP endpoint follows its three settings. It is asked on every settings change rather
        // than only when one of the three moved, because `Hosted::reconcile` answers "nothing
        // changed" in two comparisons and a list of the settings that have to remember to tell it
        // is a list whose next entry will be the one that forgot.
        self.reconcile_mcp();
        self.unsaved_settings = true;
    }

    /// Whether this window is hosting an MCP endpoint right now.
    ///
    /// False in every window a test builds, because a test never opens one — the same rule the
    /// command channel keeps. It is here so a test can say so rather than reaching into the field.
    pub fn is_serving_mcp(&self) -> bool {
        self.mcp.as_ref().and_then(|hosted| hosted.state().port()).is_some()
    }

    /// Bring the MCP endpoint into line with the settings.
    ///
    /// Does nothing in a window that never opened one, which is every window a test builds.
    pub(crate) fn reconcile_mcp(&mut self) {
        let has_channel = self.control.is_some();
        let (enabled, port, shape) =
            (self.settings.mcp_enabled, self.settings.mcp_port, self.settings.mcp_tools);
        let folder = self.tree.root().to_path_buf();
        if let Some(hosted) = &mut self.mcp {
            hosted.reconcile(enabled, port, shape, has_channel, &folder);
        }
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
        // Where this strip drew itself, for the drag to be settled against once every pane has been
        // drawn. Pushed in pane order, because the loop walks the panes left to right.
        self.tab_strips.push(outcome.strip);
        if let Some((within, pointer)) = outcome.dragging {
            if let Some(file) = at(within) {
                self.tab_drag = Some(TabDrag { file, at: pointer, dropped: outcome.dropped });
            }
        }
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

    /// Work out where the tab being carried would land, draw the mark that says so, and move it when
    /// it is let go.
    ///
    /// Called once the whole row of panes has been drawn, which is the earliest moment anything
    /// knows where every strip is. A tab may be dropped **anywhere in a pane** rather than on its
    /// strip alone, which is what IntelliJ does and is what a person dragging a file into the pane
    /// beside them is aiming at; where along the strip it goes is read from the pointer's x.
    ///
    /// Dropped outside every pane — over the explorer, the terminal, the status bar — nothing
    /// happens and no mark is drawn, so a drag can be thought better of.
    fn settle_the_tab_drag(&mut self, ui: &mut egui::Ui, pane_rects: &[Rect]) {
        let Some(drag) = self.tab_drag else {
            return;
        };
        let Some(pane) = pane_rects.iter().position(|rect| rect.contains(drag.at)) else {
            return;
        };
        let Some(strip) = self.tab_strips.get(pane) else {
            return;
        };
        let position = strip.position_at(drag.at.x);
        if drag.dropped {
            self.files.drag_tab(drag.file, pane, position);
            self.focus = Focus::Editor;
            return;
        }
        // The mark goes over the strip it is about, so it is drawn into the window's own painter
        // rather than the pane's: the pane was drawn already and a mark added to it would be under
        // the strip it is meant to be over.
        file_tabs::insertion_mark(ui.painter(), strip, position);
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
                let before = self.where_both_halves_are();
                let took = self.show_editor(ui, left, focused);
                self.show_preview(ui, right);
                self.scroll_the_two_halves_together(before, area);
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

    /// How far each half of the side by side view is scrolled, taken before either is drawn.
    fn where_both_halves_are(&self) -> (f32, f32) {
        let file = self.files.active();
        (file.scroll, file.preview_scroll)
    }

    /// Scroll the half that was not moved to show what the half that was moved is showing.
    ///
    /// `task-1673` asks that the source and the preview scroll together. The two pages are nothing
    /// like the same height — a heading is one line of source and three times a line on the page,
    /// and a fence's backticks are two lines of source and nothing at all — so the crossing is done
    /// through the text rather than through a proportion of the height. `quill_core::scroll_sync`
    /// is the arithmetic, and `Preview::source_lines` is what makes it possible.
    ///
    /// **Which half drives is decided by which one moved**, compared against where they both were
    /// before the frame drew anything. That is what stops the two of them chasing each other: the
    /// crossing snaps to a paragraph, so a position taken across and back is not quite the position
    /// it started at, and a rule that moved both halves every frame would creep down the file on its
    /// own. Only one half is written to, and only when the other actually moved.
    ///
    /// The follower is settled after both halves are drawn, so it lands on the next frame. egui
    /// paints continuously while a wheel is turning or a thumb is being dragged, so that frame is
    /// sixteen milliseconds later and nobody can see it.
    fn scroll_the_two_halves_together(&mut self, before: (f32, f32), area: Rect) {
        let (was_source, was_preview) = before;
        let file = self.files.active();
        let (source_moved, preview_moved) =
            ((file.scroll - was_source).abs() > 0.01, (file.preview_scroll - was_preview).abs() > 0.01);
        if source_moved == preview_moved {
            // Neither moved, or a change of font size moved both. Nothing to follow either way.
            return;
        }
        self.follow_the_other_half(source_moved, (area.height() - size::EDITOR_PADDING_Y * 2.0).max(0.0));
    }

    /// Move the half of the side by side view that was not scrolled so that it shows what the other
    /// half is showing. `source_drives` says which way round; `room` is how tall each half is, which
    /// is the same for both because they stand side by side.
    ///
    /// Split from [`Self::scroll_the_two_halves_together`] so the command line can ask for it: a
    /// scroll set by `quill-cli editor scroll` is applied before the frame draws anything, so the
    /// frame's own before-and-after comparison would see nothing move and the other half would sit
    /// where it was.
    pub fn follow_the_other_half(&mut self, source_drives: bool, room: f32) {
        let file = self.files.active();
        let Some(preview) = file.cached.preview.as_ref() else {
            return;
        };
        let source_page = &file.cached.layout;
        let preview_page = &file.cached.preview_layout;
        let map = &preview.source_lines;
        let (scroll, preview_scroll) = if source_drives {
            let to =
                quill_core::preview_y_for_source_y(source_page, preview_page, map, file.scroll);
            (file.scroll, to.clamp(0.0, (preview_page.height - room).max(0.0)))
        } else {
            let to = quill_core::source_y_for_preview_y(
                source_page,
                preview_page,
                map,
                file.preview_scroll,
            );
            (to.clamp(0.0, (source_page.height - room).max(0.0)), file.preview_scroll)
        };
        let file = self.files.active_mut();
        file.scroll = scroll;
        file.preview_scroll = preview_scroll;
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
        // `click_and_drag` rather than `hover`: the preview is read only, but reading includes
        // taking a copy of what you are reading. See `QuillApp::reading_preview`.
        let response = ui.interact(area, ui.id().with("preview"), egui::Sense::click_and_drag());
        let text_width = (area.width() - size::EDITOR_PADDING_X * 2.0).max(50.0);
        let ctx = ui.ctx().clone();
        self.refresh_preview(&ctx, text_width);
        let view_height = area.height() - size::EDITOR_PADDING_Y * 2.0;
        self.keep_the_previews_place_through_a_zoom(view_height);

        let was = self.files.active().preview_scroll;
        // The bar down the right, taken hold of before the wheel is read so that it wins the pointer
        // over the page underneath it. See `components::scrollbar`.
        let bar_name = format!("{} preview", self.files.active().name());
        let bar = scrollbar::Bar::new(area, was, self.preview_layout().height, view_height);
        let grab = match &bar {
            Some(bar) => scrollbar::grab(ui, bar, &bar_name),
            None => scrollbar::Grab::default(),
        };
        if let Some(to) = grab.scroll {
            self.files.active_mut().preview_scroll = to;
        }

        // The preview scrolls on its own, so reading the rendered page does not move the caret.
        let wheel = ui.input(|input| input.smooth_scroll_delta.y);
        // `contains_pointer` rather than `hovered`, because the scrollbar is a widget over this one
        // and takes the hover from it: a wheel turned with the pointer resting on the bar is still
        // plainly about the page the bar belongs to.
        if wheel != 0.0 && response.contains_pointer() {
            self.files.active_mut().preview_scroll -= wheel;
        }
        let overflow = (self.preview_layout().height - view_height).max(0.0);
        let scroll = self.files.active().preview_scroll.clamp(0.0, overflow);
        self.files.active_mut().preview_scroll = scroll;

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - scroll,
        );
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
        painter_ui.set_clip_rect(ui.painter().clip_rect().intersect(area));
        // The pointer is read before anything is drawn, so the selection painted below is the one
        // this frame's drag made rather than the one it started with.
        self.select_in_the_preview(&response, origin);
        if response.hovered() {
            painter_ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        self.paint_the_panels(&painter_ui, origin, text_width);
        self.paint_the_code_chips(&painter_ui, origin);
        editor_view::paint_behind(
            &painter_ui,
            self.preview_layout(),
            origin,
            self.files.active().preview_selection.range(),
            color::TEXT_SELECTION,
            2.0,
        );
        editor_view::paint_text(&painter_ui, &self.renderer, self.preview_layout(), origin);
        self.paint_the_pictures(&painter_ui, origin);
        self.paint_the_diagrams(&painter_ui, origin, text_width);
        // Drawn last, at the position the frame settled on rather than the one it opened with.
        if let Some(bar) = scrollbar::Bar::new(area, scroll, self.preview_layout().height, view_height) {
            scrollbar::paint(ui, &bar, &bar_name, grab.active || (scroll - was).abs() > 0.01);
        }
    }

    /// Take `Ctrl/Cmd+C` for the preview when the preview is what is being read.
    ///
    /// egui delivers a copy as an `Event::Copy` rather than as a key press — which is why `Copy` is
    /// marked in `actions::menus` as not coming from the keyboard — so the event is what has to be
    /// claimed. Removing it from the frame's input is what stops the source pane copying its own
    /// selection a moment later.
    fn route_the_preview_copy(&mut self, ui: &egui::Ui) {
        if !self.view_mode().shows_preview() || !self.preview_holds_the_selection() {
            return;
        }
        let took = ui.input_mut(|input| {
            let before = input.events.len();
            input.events.retain(|event| !matches!(event, egui::Event::Copy));
            before != input.events.len()
        });
        if took {
            if let Some(text) = self.preview_selected_text() {
                ui.ctx().copy_text(text);
            }
        }
    }

    /// Read the pointer in the preview and keep what it selected on the tab.
    ///
    /// The component works out the selection and changes nothing, which is the rule every component
    /// in Quill follows; the choice made here is that a press claims the copy for the preview
    /// without taking the keyboard from the source beside it.
    fn select_in_the_preview(&mut self, response: &egui::Response, origin: Pos2) {
        let was = self.files.active().preview_selection;
        let Some(preview) = self.files.active().cached.preview.as_ref() else { return };
        let text = preview.text.clone();
        let selection =
            editor_view::read_pointer(response, self.preview_layout(), &text, origin, was);
        if let Some(selection) = selection {
            self.files.active_mut().preview_selection = selection;
            self.reading_preview = true;
        }
    }

    /// The text the preview has selected, which is what `Copy` means while the preview is being read.
    pub fn preview_selected_text(&self) -> Option<String> {
        let file = self.files.active();
        let range = file.preview_selection.range();
        if range.is_empty() {
            return None;
        }
        let preview = file.cached.preview.as_ref()?;
        Some(preview.text.byte_slice(range))
    }

    /// True when a copy would take what the preview has selected rather than what the document has.
    pub fn preview_holds_the_selection(&self) -> bool {
        self.reading_preview && !self.files.active().preview_selection.is_empty()
    }

    /// Select the whole of the preview, which is what `Select All` means while it is being read.
    pub fn select_the_whole_preview(&mut self) {
        let length = self
            .files
            .active()
            .cached
            .preview
            .as_ref()
            .map(|preview| preview.text.len_bytes())
            .unwrap_or(0);
        self.files.active_mut().preview_selection = quill_core::Selection::new(0, length);
        self.reading_preview = true;
    }

    /// Draw a panel behind the code blocks, the tables and the front matter.
    ///
    /// `quill-core` says which paragraphs want a ground under them and this decides what one looks
    /// like, which is the same seam the pictures and the diagrams already sit on. The panel is drawn
    /// the whole width of the text rather than the width of the words, because a block of code is a
    /// block: a ragged right edge would read as a quotation.
    fn paint_the_panels(&self, ui: &egui::Ui, origin: Pos2, width: f32) {
        let Some(preview) = self.files.active().cached.preview.as_ref() else { return };
        if preview.panels.is_empty() {
            return;
        }
        let layout = self.preview_layout();
        let clip = ui.painter().clip_rect();
        for panel in &preview.panels {
            let Some((top, _)) = layout.paragraph_band(panel.paragraphs.start) else { continue };
            let Some((last, height)) = layout.paragraph_band(panel.paragraphs.end - 1) else {
                continue;
            };
            let rect = Rect::from_min_max(
                Pos2::new(origin.x - PANEL_PADDING, origin.y + top - PANEL_PADDING),
                Pos2::new(origin.x + width, origin.y + last + height + PANEL_PADDING),
            );
            if rect.bottom() < clip.top() || rect.top() > clip.bottom() {
                continue;
            }
            ui.painter().rect_filled(rect, 4.0, color::CODE_PANEL);
        }
    }

    /// Draw a chip behind each piece of inline code, which is what makes it read as a thing rather
    /// than as green prose.
    ///
    /// Only the pieces on the screen are asked for. The ranges are in order, so finding them is a
    /// pair of binary searches — the rule `task-1666` set for anything that runs once a frame.
    fn paint_the_code_chips(&self, ui: &egui::Ui, origin: Pos2) {
        let Some(preview) = self.files.active().cached.preview.as_ref() else { return };
        if preview.code_spans.is_empty() {
            return;
        }
        let layout = self.preview_layout();
        let clip = ui.painter().clip_rect();
        let bytes = layout.visible_bytes(clip.top() - origin.y, clip.bottom() - origin.y);
        let first = preview.code_spans.partition_point(|span| span.end <= bytes.start);
        for span in &preview.code_spans[first..] {
            if span.start >= bytes.end {
                break;
            }
            editor_view::paint_behind(ui, layout, origin, span.clone(), color::CODE_CHIP, 3.0);
        }
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
    fn gutter<'a>(
        &'a self,
        folds: &'a [(usize, bool)],
        breakpoints: &'a [(usize, gutter::BreakpointMark)],
    ) -> Gutter<'a> {
        let file = self.files.active();
        Gutter {
            numbers: self.settings.line_numbers,
            blame: file.blame.as_deref(),
            changes: &file.line_changes,
            // Worked out before this is called rather than here, because reading a file for what it
            // could fold wants `&mut self` for the cache and a component is handed what it draws.
            folds,
            breakpoints,
            can_debug: self.debug_applies_to(file.path()),
            execution_point: self.execution_paragraph(file.path()),
            // What the gutter's own type follows, so the numbers grow with the text rather than
            // staying a fixed eleven and a half points beside forty point letters.
            font_size: self.settings.font_size,
        }
    }

    /// Remember where the window is, which is the other half of "in the same location and state".
    ///
    /// Read from egui once a frame rather than reported by whatever moved the window, which is
    /// `follow_the_open_file`'s rule: a list of the places that have to say "I moved it" is a list
    /// whose next entry will be the one that forgot, and there are four of them here — the title
    /// bar's drag, the resize grips, the platform's own snap, and `quill-cli window position`.
    ///
    /// The **position** comes from the outer rectangle and the **size** from the inner one, because
    /// those are the two the `ViewportBuilder` takes back. A maximised window records its geometry
    /// as it is, so that a window restored from maximised is the size it was before — but it is the
    /// `maximised` flag that decides how it opens.
    fn note_where_the_window_is(&mut self, ctx: &egui::Context) {
        let place = ctx.input(|input| {
            let viewport = input.viewport();
            let outer = viewport.outer_rect?;
            let inner = viewport.inner_rect.unwrap_or(outer);
            Some(project_state::WindowPlace {
                x: outer.min.x,
                y: outer.min.y,
                width: inner.width(),
                height: inner.height(),
                maximised: viewport.maximized.unwrap_or(false),
            })
        });
        // A maximised window's own geometry is the whole screen, and remembering that as the size to
        // restore to would mean a window that could never be made small again. So the size is kept
        // as it was before it was maximised, and only the flag moves.
        match (place, self.window_place) {
            (Some(now), Some(before)) if now.maximised && !before.maximised => {
                self.window_place = Some(project_state::WindowPlace { maximised: true, ..before });
            }
            (Some(now), _) if now.is_sensible() => self.window_place = Some(now),
            _ => {}
        }
    }

    /// Read the tree again when a folder that is showing has been written to since it was last read.
    ///
    /// `task-1693`: a file or a folder made by anything other than Quill — an agent with its own
    /// tools, a build, a command in the terminal tile — never appeared in the explorer, because the
    /// tree is only read when Quill is told to read it. The folders that are **showing** are asked,
    /// on a timer, which is a handful of `metadata` calls; `FileTree::changed_on_disk` records why
    /// that is the right shape rather than a watcher.
    ///
    /// **Not while a row is being carried.** Reloading rebuilds the entries under the drag, and a
    /// drop that landed on a folder which had just been replaced would be a move somebody did not
    /// ask for.
    fn notice_what_changed_on_disk(&mut self) {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_watched) < WATCH_INTERVAL {
            return;
        }
        self.last_watched = now;
        if self.dragging_a_row {
            return;
        }
        if self.tree.changed_on_disk() {
            self.tree.reload();
        }
    }

    /// True when the file at `path` has a language that names a debugger, which is what decides
    /// whether the gutter takes a click at all.
    ///
    /// **The one question the menus, the title bar, the gutter and the command line all ask**, so
    /// none of them can disagree about whether a file can be debugged — `Plugins::debugger_for`, one
    /// reading, exactly as `file_kind::definitions_apply` is one reading.
    pub(crate) fn debug_applies_to(&self, path: Option<&Path>) -> bool {
        path.is_some_and(|path| self.plugins.debugger_for(path).is_some())
    }

    /// The same question about the file that is showing.
    pub(crate) fn debug_applies_here(&self) -> bool {
        self.debug_applies_to(self.document().path())
    }

    /// Which paragraph of `path` the program is stopped on, when it is stopped in that file.
    ///
    /// Zero-based, which is what `quill-core` calls a source line everywhere and is one less than
    /// the number the gutter draws — the adapter's answer is one-based, so the conversion happens
    /// here rather than at each of the places that read it.
    fn execution_paragraph(&self, path: Option<&Path>) -> Option<usize> {
        let (stopped_in, line) = self.debug.as_ref()?.location()?;
        let path = path?;
        (same_file(path, &stopped_in)).then(|| line.saturating_sub(1))
    }

    /// The values to paint at the ends of the lines that name them, while the program is paused.
    ///
    /// **DAP has no request for this; it is the client matching names**, and Quill already owns both
    /// halves of the machinery: `FileSymbols` has the file's identifiers sorted by position, and the
    /// paused frame's first level of variables has already been fetched because the tile shows it.
    /// So the match is a walk over the file's words against one map.
    ///
    /// Worked out **once per stop** and cached, keyed on the text revision and the frame that is
    /// showing, which is `symbols::Hover`'s own key made once more: a frame in which neither moved
    /// costs two comparisons. It is only ever computed for the file the program is stopped in, so a
    /// project with fifty tabs open pays for one of them.
    fn inline_values(&mut self, index: usize) -> Vec<(usize, String)> {
        let Some(debug) = self.debug.as_ref() else {
            return Vec::new();
        };
        if !debug.is_paused() {
            return Vec::new();
        }
        let Some(path) = self.files.at(index).path().map(Path::to_path_buf) else {
            return Vec::new();
        };
        // Only the file the program is stopped in: a local of the paused frame means nothing at the
        // end of a line of another file that happens to use the same word.
        let stopped_in = debug.location().map(|(path, _)| path);
        if !stopped_in.is_some_and(|stopped| same_file(&path, &stopped)) {
            return Vec::new();
        }
        let revision = self.files.at(index).document.text_revision();
        let frame = debug.frame;
        if let Some(cached) = self.inline_cache.as_ref() {
            if cached.revision == revision && cached.frame == frame && cached.path == path {
                return cached.values.clone();
            }
        }
        let values = debug.top_frame_values();
        let text = self.files.at(index).document.text().to_string();
        let read = &self.tab_symbols(index).read;
        let mut rows: Vec<(usize, String)> = Vec::new();
        for word in read.word_ranges() {
            let Some(value) = values.get(&text[word.clone()]) else {
                continue;
            };
            let paragraph = text[..word.start].bytes().filter(|byte| *byte == b'\n').count();
            // One value a line, the **first** name on it: a line that names three locals would
            // otherwise carry three values and be unreadable, and the first is the one the line is
            // most about.
            if rows.last().is_some_and(|(known, _)| *known == paragraph) {
                continue;
            }
            // A value the debugger could not read is **not painted**. It is still in the tree, in the
            // debugger's own words, which is where the honest full answer belongs — but at the end of
            // a line of code `step = <variable not available>` is the debugger declining to answer
            // dressed as information, and IntelliJ paints nothing there either. Seen on the released
            // 0.14.0 build against a real CodeLLDB: two of three inline values on a seven-line program
            // were this.
            if is_unreadable(value) {
                continue;
            }
            rows.push((paragraph, format!("{} = {}", &text[word.clone()], elide_value(value))));
        }
        self.inline_cache = Some(InlineValues { revision, frame, path, values: rows.clone() });
        rows
    }

    /// The inline values of the file that is showing, for a test to look at without drawing a frame.
    pub fn inline_values_for_test(&mut self) -> Vec<(usize, String)> {
        let index = self.files.active_index();
        self.inline_cache = None;
        self.inline_values(index)
    }

    /// What the gutter draws for each breakpoint in one file: which paragraph it is on, and how.
    ///
    /// The document is the authority for **where** they are and the session for **whether the
    /// debugger agreed to stop there**, which is the two-authority split §6.3 asks for: Quill draws
    /// the adapter's answer rather than its own hope. With no session running there is nobody to have
    /// said a breakpoint is unbound, so every one of them is drawn solid.
    pub fn breakpoint_marks(&self, index: usize) -> Vec<(usize, gutter::BreakpointMark)> {
        let file = self.files.at(index);
        let document = &file.document;
        if document.breakpoints().is_empty() {
            return Vec::new();
        }
        let path = file.path().map(Path::to_path_buf);
        let mut rows: Vec<(usize, gutter::BreakpointMark)> = document
            .breakpoints()
            .iter()
            .map(|breakpoint| {
                let answered = path
                    .as_deref()
                    .and_then(|path| self.debug.as_ref()?.verified(path, breakpoint.offset));
                let mark = gutter::BreakpointMark {
                    enabled: breakpoint.enabled,
                    verified: answered.map(|answer| answer.verified).unwrap_or(true),
                    conditional: breakpoint.is_conditional(),
                };
                // The adapter's own line wins where it gave one: a breakpoint it moved to the next
                // statement is drawn where the program will really stop, for the life of the session.
                let paragraph = match answered.and_then(|answer| answer.line) {
                    Some(line) => line.saturating_sub(1),
                    None => document.text().byte_to_line(breakpoint.offset),
                };
                (paragraph, mark)
            })
            .collect();
        // The gutter binary searches this list, so it has to be sorted — and it may not be, because
        // an adapter is free to move a breakpoint to the next statement and two that were in offset
        // order can come back out of it. Two on one line would break the search as well, so the
        // later of them gives way, which is the rule `Breakpoints` itself keeps about two at one
        // offset.
        rows.sort_by_key(|(paragraph, _)| *paragraph);
        rows.dedup_by_key(|(paragraph, _)| *paragraph);
        rows
    }

    /// Draw the source of the file that is showing into `area`.
    ///
    /// `focused` is whether this pane has the keyboard. Only that pane draws a caret and only that
    /// pane reads the keyboard, or every pane would take the same key presses. Returns true when the
    /// pane was clicked in, which is what moves the keyboard to it.
    /// What the pointer is over in this pane while the platform's modifier is held.
    ///
    /// `Modifiers::command` is the Apple key on macOS and the control key on Windows, which is what
    /// a person means by the modifier in either place. Nothing at all is worked out while it is not
    /// held, and letting go of it forgets what was: an underline that outlived the modifier would be
    /// an affordance promising something the next click would not do.
    ///
    /// Only the pane with the keyboard is asked. Two panes each resolving a word under one pointer
    /// would be two underlines, and the pointer is only ever over one of them anyway.
    fn symbol_under_the_pointer(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        origin: Pos2,
        focused: bool,
    ) -> editor_view::SymbolPointer {
        let held = ui.input(|input| input.modifiers.command);
        if !held || !focused || !self.definitions_apply_here() {
            self.forget_the_hover();
            return editor_view::SymbolPointer::default();
        }
        let Some(at) = response.hover_pos() else {
            return editor_view::SymbolPointer::default();
        };
        let local = at - origin;
        let offset = self.layout().offset_at(local.x, local.y);
        let hover = self.resolve_under_the_pointer(offset);
        editor_view::SymbolPointer { word: hover.map(|hover| hover.word) }
    }

    fn show_editor(&mut self, ui: &mut egui::Ui, area: Rect, focused: bool) -> bool {
        // What this file could fold and what of it is folded, read before anything is drawn: the
        // cache wants `&mut self` and every component here is handed what it draws.
        let index = self.files.active_index();
        let fold_marks: Vec<(usize, bool)> = self.fold_marks(index).to_vec();
        // Set by an arrow in the gutter or a badge in the text, and acted on at the end of the
        // frame — a fold changes the layout, and changing it half way through drawing this pane
        // would leave the rest of the frame drawing from a layout that no longer matches.
        let mut folded: Option<usize> = None;
        // What the gutter draws for each breakpoint, read here for the reason the folds are: it asks
        // the session what the adapter said, and a component is handed what it draws.
        let breakpoint_marks = self.breakpoint_marks(index);
        // The line the program is stopped on, and the values to paint at the ends of the lines that
        // bind them. Both worked out before anything is drawn, for the same reason again.
        let execution_point = self.execution_paragraph(self.files.at(index).path());
        let inline_values = self.inline_values(index);
        // Set by a click in the gutter's breakpoint column, and acted on at the end of the frame for
        // the reason a fold is: changing the document half way through drawing this pane would leave
        // the rest of the frame drawing from a layout that no longer matches.
        let mut toggled_breakpoint: Option<usize> = None;
        // The gutter takes the left of the editing area, and the text starts after it. With no
        // gutter the text keeps the padding it always had, so putting the numbers away leaves the
        // window looking exactly as it did before there were any.
        let gutter = self.gutter(&fold_marks, &breakpoint_marks);
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
            // A press in the source takes the copy back from the preview beside it. See
            // `QuillApp::reading_preview`.
            self.reading_preview = false;
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
        // A caret is never inside a hidden paragraph. `reveal_caret` is set by everything that puts
        // the caret somewhere without a click — a jump to a definition, a search hit, `quill-cli
        // editor caret --line`, `Navigate Back` — so asking here is one place rather than one per
        // jump, which is `follow_the_open_file`'s rule: the next jump added would be the one that
        // forgot. Before the layout, so the frame that reveals it is the frame that draws it.
        if self.reveal_caret && focused {
            self.reveal_the_caret_from_a_fold();
        }
        self.refresh_layout(text_width);
        let view_height = area.height() - size::EDITOR_PADDING_Y * 2.0;
        // Straight after the layout, so the rest of the frame — the wheel, the caret, the painter
        // — sees the scroll position the zoom asked for rather than the one it was left at.
        self.keep_the_place_through_a_zoom(view_height);

        // The bar down the right hand edge, taken hold of here rather than at the end of the frame:
        // the editing area asks for drags over the whole of its rectangle and egui hands a point to
        // the last widget that asked for it, so a bar added after the text is a bar that can be
        // dragged. It is drawn at the end, once the wheel and the caret have had their say. See
        // `components::scrollbar`.
        let was = self.files.active().scroll;
        // Named after the file rather than after the half, because two panes each have one and two
        // controls must not share a name — the same reason the gutter's blame cells and a diagram
        // carry the file's name. Two panes cannot be showing one file, so the name is unique.
        let bar_name = self.files.active().name();
        let bar = scrollbar::Bar::new(area, was, self.layout().height, view_height);
        let grab = match &bar {
            Some(bar) => scrollbar::grab(ui, bar, &bar_name),
            None => scrollbar::Grab::default(),
        };
        if let Some(to) = grab.scroll {
            self.files.active_mut().scroll = to;
        }

        let scroll = self.files.active().scroll;
        let origin = Pos2::new(area.left() + padding, area.top() + size::EDITOR_PADDING_Y - scroll);

        // What the pointer is over while the modifier is held, worked out **before** the click and
        // cached against the text revision and the word. Resolve first is VS Code's model and it is
        // what makes the click feel instantaneous: the answer is already in hand, and only a word
        // that really has somewhere to go is underlined.
        let symbol = self.symbol_under_the_pointer(ui, &response, origin, focused);
        if symbol.resolved() {
            // A hand rather than the writing bar, which is what says the word is a link.
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let formatting = file_kind::formatting_applies(self.files.active().path());

        // Taken apart by field, because the input handlers want the document mutably while the
        // layout they measure against is borrowed at the same time, and a method on `self` would
        // borrow the whole window. Both now live on the same tab, and the two are separate fields of
        // it, which is a borrow the compiler allows through one reference.
        // Whether a character reached the document this frame, which is the one thing the automatic
        // trigger fires on. Read before the input is handled, because handling it is what consumes
        // the events. A paste, an undo and a command line edit are all deliberately not typing.
        let typed = has_keyboard
            && ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, egui::Event::Text(text) if !text.chars().any(char::is_control))
                })
            });
        let file = self.files.active_mut();
        let laid = &file.cached.layout;
        let document = &mut file.document;
        let pointer = editor_view::handle_pointer(&response, document, laid, origin, &symbol);
        let pointer_changed = pointer.changed;
        let outcome = editor_view::handle_input(ui, document, laid, has_keyboard, formatting);
        // The window decides what a jump means, which is the rule every component follows.
        if let Some(offset) = pointer.jump {
            self.focus = Focus::Editor;
            self.go_to_definition(offset);
            return true;
        }
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

        // Open, refilter or close the completion popup, now that the letter just typed is in the
        // file. Only the pane with the keyboard, because there is one popup and it belongs to
        // whichever pane is being typed into.
        if focused {
            self.keep_the_completion_fresh(typed);
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
        // `contains_pointer` rather than `hovered`: the scrollbar is a widget over this one and
        // takes the hover from it, and a wheel turned with the pointer resting on the bar is still
        // about the page the bar belongs to.
        if wheel != 0.0 && response.contains_pointer() {
            scroll -= wheel;
        }
        if outcome.scroll_to_caret || (self.reveal_caret && focused) {
            let caret = self.layout().caret_at(self.document().selection().head);
            if caret.y < scroll {
                scroll = caret.y;
            } else if caret.y + caret.height > scroll + view_height {
                scroll = caret.y + caret.height - view_height;
            }
        }
        if focused {
            self.reveal_caret = false;
        }
        let overflow = (self.layout().height - view_height).max(0.0);
        let scroll = scroll.clamp(0.0, overflow);
        self.files.active_mut().scroll = scroll;

        let origin = Pos2::new(area.left() + padding, area.top() + size::EDITOR_PADDING_Y - scroll);

        // Where the completion popup hangs, worked out from the caret's own box at the position the
        // frame settled on. Recorded rather than drawn here: the window draws it after the whole row
        // of panes, so it sits over the dividers rather than under one.
        if focused {
            self.remember_where_the_completion_hangs(origin, area);
        }

        // The gutter is drawn from the same origin as the text, so a number cannot drift away from
        // the line it belongs to.
        if gutter_width > 0.0 {
            let outcome = gutter::show(
                ui,
                gutter_rect,
                &self.gutter(&fold_marks, &breakpoint_marks),
                self.layout(),
                origin.y,
                self.document().text().byte_to_line(self.document().selection().head),
            );
            if let Some(at) = outcome.context_menu {
                self.gutter_menu = Some(at);
                // Which row it was over, so the menu's breakpoint entries are about the line under
                // the pointer rather than about the caret — the rule the text menu already follows.
                self.gutter_menu_line = outcome.menu_paragraph;
            }
            if let Some(line) = outcome.toggle_fold {
                folded = Some(line);
            }
            if let Some(line) = outcome.toggle_breakpoint {
                toggled_breakpoint = Some(line);
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
                underline: symbol.word.clone(),
                execution_point,
                inline_values: &inline_values,
            },
        );
        // The badge standing for each collapsed block, over the text and after the end of its head
        // line. It takes clicks, so it is added after the editing area rather than before it: egui
        // hands a point to the last widget that asked for it and the editing area asks for all of
        // its rectangle, which is the same ordering the scrollbar and the pane dividers follow.
        let visible = editor_view::visible_lines(&painter_ui, self.layout(), origin);
        if let Some(line) =
            editor_view::fold_badges(&mut painter_ui, self.layout(), origin, &fold_marks, visible)
        {
            folded = Some(line);
        }
        // Drawn last, at the position the frame settled on rather than the one it opened with, or
        // the thumb is a frame behind the writing — which on a fast scroll can be seen.
        if let Some(bar) = scrollbar::Bar::new(area, scroll, self.layout().height, view_height) {
            scrollbar::paint(ui, &bar, &bar_name, grab.active || (scroll - was).abs() > 0.01);
        }
        if let Some(line) = folded {
            self.toggle_fold_at_line(line);
        }
        if let Some(line) = toggled_breakpoint {
            self.toggle_breakpoint_at_line(line);
        }
        took_the_keyboard
    }
}

/// Move a file or a folder on the disk.
///
/// A rename first, which is one operation and keeps the file's own history where the platform has
/// one; a copy and a delete when that fails, which is what happens across volumes and is what
/// `services::file_clipboard` already does for a paste.
fn move_the_bytes(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if from.is_dir() {
        copy_the_folder(from, to)?;
        std::fs::remove_dir_all(from)
    } else {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    }
}

/// Copy a folder and everything under it.
fn copy_the_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let here = entry.path();
        let there = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_the_folder(&here, &there)?;
        } else {
            std::fs::copy(&here, &there)?;
        }
    }
    Ok(())
}

/// Write a closed file's share of a move, having checked it is still the file the plan was made
/// against.
///
/// The check is what makes this safe on a syntactic tier: every range is compared against the
/// length of the text it is supposed to be inside, and a file that has changed since the plan was
/// made is refused whole rather than patched on faith. Bytes outside the ranges are untouched, so
/// encodings, line endings and trailing whitespace survive byte for byte.
fn write_the_edits(path: &Path, edits: &[(std::ops::Range<usize>, String)]) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|problem| problem.to_string())?;
    if edits.iter().any(|(range, _)| range.end > text.len() || !text.is_char_boundary(range.start))
    {
        return Err("it has changed since the move was worked out".to_owned());
    }
    let after = file_move::applied(&text, edits);
    std::fs::write(path, after).map_err(|problem| problem.to_string())
}

/// The colour a file is drawn in for what git thinks of it.
///
/// The same three colours the change bars in the gutter and the markers in the commit panel use, so
/// a modified file is one colour wherever it is shown.
/// The inline values worked out for one file at one stop, and what they were worked out from.
///
/// `symbols::Hover`'s key made once more: a frame in which the text has not changed and the frame
/// that is showing has not changed costs two comparisons rather than a walk of the file's words.
pub(crate) struct InlineValues {
    revision: u64,
    frame: Option<i64>,
    path: PathBuf,
    values: Vec<(usize, String)>,
}

/// True when a value is the debugger saying it has nothing to say.
///
/// Adapters spell this a dozen ways — `<variable not available>`, `<optimized out>`, `<not
/// available>`, `<error: ...>` — and what they share is the angle brackets: a value a program really
/// holds is a number, a string or a structure, and none of those is written that way. So the shape is
/// the test rather than a list of an adapter's own wordings, which would be a list that is wrong for
/// the next adapter.
fn is_unreadable(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('<') && value.ends_with('>')
}

/// A value painted at the end of a line, cut to something a line can hold.
///
/// Far shorter than the tile's own limit: a value at the end of a line of code is a glance, and one
/// that ran off the edge of the pane would be worse than none. The tile shows the whole of it.
fn elide_value(value: &str) -> String {
    const LIMIT: usize = 48;
    let flat = value.replace('\n', " ");
    match flat.chars().count() > LIMIT {
        true => format!("{}\u{2026}", flat.chars().take(LIMIT).collect::<String>()),
        false => flat,
    }
}

/// The two halves of the debug tile's state, borrowed apart.
///
/// A component takes its own state mutably and what it draws immutably, which is the shape every
/// component in Quill has — and the borrow checker will not hand out both halves of one `&mut self`
/// through two method calls. One struct with two fields is what makes the split explicit rather than
/// something the caller has to spell out at each site.
struct DebugSplit<'a> {
    panel: &'a mut DebugPanel,
    debug: Option<&'a DebugState>,
}

fn split_the_debug(app: &mut QuillApp) -> DebugSplit<'_> {
    DebugSplit { panel: &mut app.debug_panel, debug: app.debug.as_ref() }
}

/// Whether two paths name the same file.
///
/// An adapter answers with whatever spelling the debug information holds, which on Windows is very
/// often a different case from the one the explorer opened — and a comparison that missed that would
/// leave the execution point invisible in a file that is plainly open. So the names are compared
/// without case there and exactly everywhere else, which is what each platform's file system means.
/// `canonicalize` is deliberately not used: it touches the disk, this is asked at every stop, and a
/// verbatim path is a thing `paths::plain` exists to stop travelling.
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    // Through `paths::plain` first, because a path Quill wrote down may be **verbatim** —
    // `\\?\C:\jason\dev\quill` — while the one an adapter answers with never is. That module records
    // where a verbatim path comes from and why one must not be allowed to travel; this is the second
    // place two of them have to be compared.
    let left = quill_terminal::paths::plain(left);
    let right = quill_terminal::paths::plain(right);
    if left == right {
        return true;
    }
    match cfg!(windows) {
        true => {
            let flatten = |path: &Path| path.to_string_lossy().to_lowercase().replace('/', "\\");
            flatten(&left) == flatten(&right)
        }
        false => false,
    }
}

/// Which **one-based** line an offset is on in a file that is not open.
///
/// The ownership rule's disk half: a file that is open is owned by its `Document` and every other
/// file is owned by the store, so a closed file's line numbers come from its own bytes, read at the
/// moment of use rather than watched — which is what `open_the_match` already does before jumping
/// into one. A file that cannot be read answers line one, which the adapter will then decline to
/// bind and say so about.
fn line_number_in_file(path: &Path, offset: usize) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 1;
    };
    text.as_bytes()[..offset.min(text.len())].iter().filter(|byte| **byte == b'\n').count() + 1
}

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

    /// Write the settings, the pane sizes and what was open in the project before the window goes,
    /// and stop everything that is running.
    ///
    /// Nothing ever orphans a child on purpose: `Session`'s own drop shuts a pseudoterminal down,
    /// and this is the same path taken deliberately so it happens while the window is still here to
    /// wait for it.
    fn on_exit(&mut self) {
        self.run.kill_everything();
        self.write_settings();
        self.remember_the_project();
    }
}

/// Colouring a fenced code block with the grammar a plugin already supplies.
///
/// `quill-core` holds no plugin registry and must not learn about one, so it asks a question through
/// `CodeHighlighter` and this answers it — with exactly the two calls `colour_the_file` makes for a
/// source file, so a fence of Rust inside a document is coloured as a `.rs` file is. A language
/// nothing claims answers with nothing, and the block keeps the one code colour it always had.
struct PluginHighlighter<'a> {
    plugins: &'a crate::services::plugins::Plugins,
}

impl quill_core::CodeHighlighter for PluginHighlighter<'_> {
    fn colour(
        &self,
        language: &str,
        code: &str,
    ) -> Vec<(std::ops::Range<usize>, quill_core::Color)> {
        let Some(plugin) = self.plugins.for_language(language) else {
            return Vec::new();
        };
        quill_core::syntax::highlight(code, &plugin.grammar)
            .into_iter()
            .filter_map(|(range, token)| plugin.theme.colour(token).map(|colour| (range, colour)))
            .collect()
    }
}
