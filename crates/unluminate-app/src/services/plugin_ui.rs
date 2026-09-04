//! The code half of the UI plugins: what a provider is, what it is handed, and what it may ask for.
//!
//! `services::plugins` reads the manifest and says *what* a plugin contributes and *where*. This file
//! is the other half: the trait the contributed pane, tab and settings page are filled by, and the two
//! values that cross the boundary in each direction. `tasks/ui-plugin-architecture.md` is the design.
//!
//! ## A provider is code that shipped in the binary
//!
//! `plugins::UI_PROVIDERS` is the fourth registry of its shape, after the renderers, the project
//! detectors and the debuggers. A manifest names a provider, the name is checked, and the drawing
//! shipped with Unluminate. Nothing in a plugin folder is executed, which is the property the whole plugin
//! system has had since it was written and which this addition does not give up.
//!
//! The three alternatives were weighed and each is the right answer to a question this is not asking.
//! A dynamic library means a Rust type crossing a `dlopen` boundary, which is undefined behaviour
//! unless both sides were built by the same compiler with the same flags, and `egui::Ui` is generic
//! and closure heavy, which is exactly the shape that cannot cross. WebAssembly copies its arguments
//! by value and cannot pass an object graph, so drawing through it means designing a widget protocol
//! first. A separate process is what Unluminate already does for git, for debug adapters and for terminals,
//! and it is the named route for a third party plugin; it needs the same widget protocol.
//!
//! So the first UI plugin draws with `egui`, through the same `components::controls` and
//! `components::modal` every other part of the window uses, and the widget protocol is left for the
//! day something needs it.
//!
//! ## Drawing returns requests rather than doing things
//!
//! A provider that wanted to open a file would have to reach the window's `OpenFiles`, and then two
//! things would own the tab strip. So a provider returns [`Request`]s and `UnluminateApp` acts on them once
//! the pane has been drawn. That is the rule `components::activity_bar` already keeps: nothing there
//! changes anything, and each button reports the `Action` it stands for.
//!
//! ## A provider cannot read a setting it was not given
//!
//! [`Look`] is handed over rather than reached for. It carries the palette, the fonts, the row heights
//! and the opacity, all of them the window's own.
//!
//! **The claim that is actually true is about the manifest**, and it is worth stating exactly, because the
//! stronger version is false: a provider is ordinary Rust compiled into Unluminate and can of course write
//! `Color32::from_rgb`. What a **plugin folder** cannot do is name a colour — there is no manifest key that
//! sets one, and there never will be, which is what stops somebody's `plugin.conf` deciding what the window
//! looks like. Inside the binary the rule is a convention that a review keeps, exactly as it is for every
//! other component in `components/`.
//! That is the rule the syntax themes already keep — Dracula's own background is deliberately unused,
//! because a scheme that repainted the editing area would take the transparency away — and the rule a
//! Mermaid diagram's own `style` directive already meets, which is read and ignored.

use std::path::PathBuf;

use egui::Color32;

use crate::services::vello_canvas::Chrome;
use crate::settings::Settings;
use crate::theme::{color, size};

/// What a row in a menu is, from `design/style-guide.md`: 24 points against a list row's 28.
///
/// Named here rather than in `theme::size` because the menus draw their own rows and nothing else in
/// the window needed the number until a plugin did.
pub const MENU_ROW: f32 = 24.0;

/// Everything a provider needs to look like the rest of the window.
///
/// Built once a frame from the settings and the theme. A pane that ignored `opacity` would be the one
/// opaque rectangle in a window whose transparency is the whole character of the product, so it is
/// here rather than left to each provider to remember.
#[derive(Clone)]
pub struct Look<'a> {
    /// The fonts, for a provider that draws a character grid.
    ///
    /// A borrow of Unluminate's own renderer, which is the one thing here that is machinery rather than a value.
    /// It is what lets a provider draw a **real terminal** — `components::terminal_panel::grid`, the same one
    /// the terminal tile and the run tile share, with its keyboard, its selection, its mouse reports and its
    /// resize — instead of painting a picture of one. A provider that is not in the binary could not be given
    /// this, which is one more thing the widget tree of `tasks/ui-plugin-architecture.md` §10 would have to
    /// answer for.
    pub renderer: &'a crate::services::text_renderer::TextRenderer,
    /// The family the editor sets text in, as chosen in `Settings -> Appearance`.
    pub font_family: String,
    /// The point size the editor sets text in.
    pub font_size: f32,
    /// The point size a character grid is set in, which is the terminal's own setting.
    pub monospace_size: f32,
    /// How opaque the window background is.
    ///
    /// The window paints a provider's ground with this applied — see [`UiProvider::pane`] — and a provider
    /// paints anything of its own the same way, through [`Look::ground`], so a pane is as transparent as
    /// the editing area rather than the one opaque rectangle in the window.
    pub opacity: f32,
    pub palette: Palette,
    /// What a row in a list is, from `design/style-guide.md`.
    pub row_height: f32,
    /// What a row in a menu is.
    pub menu_row_height: f32,
    pub corner_radius: f32,
    /// Where a provider puts the decoration `egui` cannot draw.
    ///
    /// **Depth is handed over for the same reason colour is.** A provider says what kind of surface it is
    /// drawing — `raised`, `sunken`, `glow` — and the elevation recipe is the window's, in
    /// `services::vello_canvas::Lift`, so two panels cannot disagree about how far off the page a card
    /// stands. There is no way to record a shadow of your own, which is the rule the closed palette keeps
    /// for colour.
    ///
    /// [`Chrome::off`] unless the window put a canvas behind this pane, so a provider always has one to
    /// draw into and a test that has no canvas draws into one that records nothing. A provider asks
    /// `chrome.is_recording()` when it has a flat form to fall back to.
    pub chrome: &'a Chrome,
    /// How a fenced code block in a plugin's markdown is coloured.
    ///
    /// **Handed over for the same reason the palette is.** `unluminate-core` holds no plugin registry, so it asks
    /// through `CodeHighlighter` and the window answers with the same two calls `colour_the_file` makes for a
    /// source file — which is what makes a fence of Rust in an answer look like a `.rs` file. A provider could
    /// not build one: it cannot reach `services::plugins`, and a plugin that could would be a plugin deciding
    /// which languages exist.
    ///
    /// `None` in a test and in any window with no plugins loaded, where a fence keeps the one code colour it
    /// always had — which is exactly what the preview did before `task-1685`, so it can never be worse.
    pub highlighter: Option<&'a dyn unluminate_core::CodeHighlighter>,
    /// Whether this plugin holds the keyboard this frame.
    ///
    /// **What stops a plugin reading a key press meant for something else.** Unluminate has one value that says who
    /// has the keys, and a plugin drawing a pane cannot see it, so the window tells it here. Agent-Tasks reads it
    /// before taking an arrow key: without it, moving the caret in the editor would also move the chosen card on
    /// a board nobody was looking at.
    pub has_the_keyboard: bool,
    /// How much bigger or smaller this pane is drawn than the settings alone would make it.
    ///
    /// `task-1771`'s per-pane zoom, from `settings::Panes::zoom_of`. It is already multiplied into
    /// `font_size`, `monospace_size` and the two row heights by [`Look::zoomed_by`], and it is kept here
    /// because [`Look::scale`] is what every fixed measurement on a board is multiplied by and that has to
    /// follow the zoom **down** as well as up — which the font size on its own cannot do, since it is
    /// floored at one.
    zoom: f32,
}

impl std::fmt::Debug for Look<'_> {
    /// Written by hand because a `TextRenderer` holds a font database and a glyph atlas and has no `Debug`.
    /// What is printed is what a test wants to see when an assertion about the look fails.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Look")
            .field("font_family", &self.font_family)
            .field("font_size", &self.font_size)
            .field("monospace_size", &self.monospace_size)
            .field("opacity", &self.opacity)
            .field("chrome", &self.chrome.is_recording())
            .finish()
    }
}

/// The chrome a `Look` has when nothing put a canvas behind it.
///
/// A `static` rather than an argument, so that the hundred existing callers of [`Look::of`] — nearly all of
/// them tests — go on building a `Look` with no canvas, no graphics card and no fonts behind it. It records
/// nothing however much is drawn into it, so there is nothing to accumulate and nothing to reset.
static NO_CHROME: Chrome = Chrome::off();

impl<'a> Look<'a> {
    /// The look of this window, this frame.
    pub fn of(
        settings: &Settings,
        renderer: &'a crate::services::text_renderer::TextRenderer,
    ) -> Self {
        Self {
            renderer,
            font_family: settings.font_family.clone(),
            font_size: settings.font_size,
            monospace_size: settings.terminal_font_size,
            opacity: settings.opacity,
            palette: Palette::active(),
            chrome: &NO_CHROME,
            highlighter: None,
            row_height: size::ROW,
            menu_row_height: MENU_ROW,
            corner_radius: f32::from(size::CONTROL_CORNER),
            // False unless the window says otherwise, because that is the safe answer: a plugin that was told it
            // had the keys when it did not would take them from the editor.
            has_the_keyboard: false,
            zoom: crate::settings::DEFAULT_ZOOM,
        }
    }

    /// The same look, `factor` bigger.
    ///
    /// Every size a provider reads is multiplied here rather than at each of its own call sites, so a pane
    /// that already scaled with the editor's font zooms with no change to the provider at all. `task-1771`.
    pub fn zoomed_by(self, factor: f32) -> Self {
        let factor = factor.clamp(crate::settings::MIN_ZOOM, crate::settings::MAX_ZOOM);
        Self {
            font_size: self.font_size * factor,
            monospace_size: self.monospace_size * factor,
            row_height: self.row_height * factor,
            menu_row_height: self.menu_row_height * factor,
            zoom: self.zoom * factor,
            ..self
        }
    }

    /// The same look, colouring fenced code with the window's own plugins.
    pub fn colouring_with<'b: 'c, 'c>(
        self,
        highlighter: &'b dyn unluminate_core::CodeHighlighter,
    ) -> Look<'c>
    where
        'a: 'c,
    {
        Look { highlighter: Some(highlighter), ..self }
    }

    /// The same look, with the keyboard given to the plugin about to be drawn.
    pub fn holding_the_keyboard(mut self, holding: bool) -> Self {
        self.has_the_keyboard = holding;
        self
    }

    /// The same look, recording its decoration into this pane's canvas.
    ///
    /// The lifetime shrinks to the chrome's, which is what makes a chrome created for one pane inside the
    /// pane loop usable by a `Look` built once outside it.
    pub fn drawing_into<'b: 'c, 'c>(self, chrome: &'b Chrome) -> Look<'c>
    where
        'a: 'c,
    {
        Look { chrome, ..self }
    }

    /// How much bigger everything drawn at a fixed size has to be, from the font the editor is set in.
    ///
    /// **Because Unluminate offers 48 and 64 point text.** Every height on the board was written for the default 16
    /// points — a card 84 points tall, a lane heading 34, the board's own header 66 — so choosing a large font
    /// scaled the words and left the boxes, and a card title overlapped its own footer while a lane heading
    /// overlapped the first card. One number rather than a separate rule per box, so the proportions the design
    /// settled on are kept whatever size the text is.
    ///
    /// Floored rather than allowed below one: a person who chooses 9 point text wants small text, not cards too
    /// short to press, and a tick box that shrinks with the font stops being a target.
    /// The zoom is applied **after** the floor rather than inside it, so a pane zoomed out really does get
    /// smaller: the floor exists so that 9 point text does not shrink a tick box below a target, and a person
    /// holding `Ctrl` and turning the wheel the other way is asking for exactly that.
    pub fn scale(&self) -> f32 {
        let from_the_font = self.font_size / self.zoom / crate::settings::DEFAULT_FONT_SIZE;
        from_the_font.max(1.0) * self.zoom
    }

    /// A colour with the window's opacity applied, which is what a provider paints a ground with.
    ///
    /// One function rather than each provider multiplying an alpha, because the arithmetic is the
    /// window's and a provider that got it slightly wrong would show as a seam down the edge of its
    /// pane.
    pub fn ground(&self, colour: Color32) -> Color32 {
        let alpha = (self.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), alpha)
    }
}

/// The colours a provider draws with.
///
/// `theme::color` passed through as a value, so a plugin's rows, fields, buttons and chosen row are
/// the ones every list in Unluminate draws. There is deliberately no way for a **manifest** to add one: a
/// manifest key that set a colour would undo the reason the palette is closed. A provider is Rust and
/// can reach `theme::color` directly — Agent-Tasks does, for the one red the design already has — and
/// what stops that being a second palette is the same thing that stops it in every other component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub editor: Color32,
    pub panel: Color32,
    pub panel_footer: Color32,
    pub control: Color32,
    pub control_border: Color32,
    pub field: Color32,
    pub divider: Color32,
    pub menu: Color32,
    pub accent: Color32,
    pub selected_row: Color32,
    pub unsaved: Color32,
    pub text_strong: Color32,
    pub text: Color32,
    pub text_control: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub added: Color32,
    pub modified: Color32,
    /// The four surfaces a board is built from: the page behind it, a lane, a card, and a well.
    ///
    /// Named rather than new. The picture the board is measured against uses `#181D24`, `#1C222A`,
    /// `#20252E` and `#1B2026`, and Unluminate's own `EDITOR`, `EXPLORER`, `CODE_PANEL` and `FIELD` are each
    /// within five units a channel of one of them — `CODE_PANEL`'s own comment even says it is "a step up
    /// from `EDITOR` … so the block reads as a panel on the page", which is what a card is. So the board
    /// gains the ladder it needs and the palette gains no colour.
    pub board_page: Color32,
    pub board_lane: Color32,
    pub board_card: Color32,
    pub board_well: Color32,
    /// The violet a card's agent badge is, and the green ring it wears while its terminal is running.
    pub agent: Color32,
    pub attached: Color32,
    /// The blue a board's own buttons are, which is the picture's rather than Unluminate's — see `color::board_accent()`.
    pub board_accent: Color32,
}

impl Palette {
    /// The palette of the theme this window is painting in.
    ///
    /// A function rather than the `const UNLUMINATE` it was until `task-1776`: a colour is read out of the
    /// active theme now, so there is no constant left to be. What is unchanged is the thing the note above
    /// is about — a provider is handed **one** palette, worked out here, and a manifest still has no way
    /// to add a colour to it.
    pub fn active() -> Palette {
        Palette {
            editor: color::editor(),
            panel: color::explorer(),
            panel_footer: color::explorer_footer(),
            control: color::control(),
            control_border: color::control_border(),
            field: color::field(),
            divider: color::divider(),
            menu: color::menu(),
            accent: color::accent(),
            selected_row: color::selected_row(),
            unsaved: color::unsaved(),
            text_strong: color::text_strong(),
            text: color::text(),
            text_control: color::text_control(),
            text_dim: color::text_dim(),
            text_faint: color::text_faint(),
            added: color::git_added(),
            modified: color::git_modified(),
            board_page: color::editor(),
            board_lane: color::explorer(),
            board_card: color::code_panel(),
            board_well: color::field(),
            agent: color::agent(),
            attached: color::attached(),
            board_accent: color::board_accent(),
        }
    }
}

/// Where a provider's modal reserved room for the decoration behind it.
///
/// Four values that travel together, so a provider hands back one thing rather than four. See
/// [`UiProvider::take_the_modals_canvas`].
pub struct ChromeSlot {
    /// What the canvas is cached under, so a modal that did not change is not rasterised again.
    pub id: egui::Id,
    /// The rectangle the decoration covers.
    pub area: egui::Rect,
    /// The painter of the **modal's own layer**, which is the only one that can fill the slot in.
    pub painter: egui::Painter,
    pub shape: egui::layers::ShapeIdx,
}

impl std::fmt::Debug for ChromeSlot {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("ChromeSlot").field("id", &self.id).field("area", &self.area).finish()
    }
}

/// What a provider asks the window to do, having drawn.
///
/// Every variant is something only the window can do. A provider does none of them itself, so there is
/// one owner of the tab strip, one owner of the status bar and one place a file is opened.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Open a file in a tab, which is what a provider does when a ticket names one.
    OpenFile(PathBuf),
    /// Say something in the status bar, which is where every honest miss in Unluminate is reported.
    Message(String),
    /// Show this plugin's own tab in the editing area.
    ShowTab,
    /// Show or hide this plugin's pane.
    ShowPane(bool),
    /// Draw again next frame even if nothing was touched, which is what a provider with a terminal in
    /// it needs while that terminal is printing.
    Repaint,
    /// Take the keyboard, or give it back.
    ///
    /// A plugin with a terminal in it needs the keys, and the window is the only thing that can say who has them:
    /// `UnluminateApp::focus` is one value and the editing area, the explorer and the terminal tile already share it
    /// through that one value. A provider that decided for itself would be a second owner of the keyboard, and
    /// both would read the same key press.
    TakeTheKeyboard(bool),
    /// Show this file or folder in the platform's own file manager.
    ///
    /// The window's business rather than a provider's, for the reason the clipboard is: opening a folder is asking
    /// the operating system to do something, and `services::launcher` is the one place Unluminate does that.
    Reveal(PathBuf),
    /// Put this on the clipboard.
    ///
    /// The clipboard is the platform's and the window owns the one handle to it, which is why a provider asks
    /// rather than reaching: two owners of a clipboard is two programs fighting over one selection.
    Copy(String),
    /// Run one of Unluminate's own commands and hand the answer back through [`UiProvider::answered`].
    ///
    /// **The one path a command becomes a change is `UnluminateApp::run_cli`**, and this is how a provider
    /// reaches it. So a thing a plugin asks for and the same thing typed at `unluminate-cli` are the same
    /// thing, rather than a second dispatcher a plugin brought with it. Agent-Chat asks, because the
    /// tools it offers a model are the catalogue's own commands.
    ///
    /// `command` is the wire name — `tab.open` — and `arguments` is exactly what goes in the request.
    /// `id` is the provider's own, echoed back with the answer, because more than one may be
    /// outstanding and they do not necessarily finish in order.
    RunCommand { id: String, command: String, arguments: serde_json::Map<String, serde_json::Value> },
    /// The picture on the clipboard, if there is one, answered through [`UiProvider::answered`].
    ///
    /// **The window owns the one handle to the clipboard**, which is why [`Request::Copy`] exists and
    /// is the same reason this does: two owners of a clipboard are two programs fighting over one
    /// selection. The answer is `{ "media": …, "name": …, "data": <base64> }`, or a refusal saying
    /// there was no picture on it.
    ClipboardPicture { id: String },
}

/// What a command answered.
///
/// A string for a person and a value for a machine, because the same command is run from a menu entry
/// and from `unluminate-cli plugin run`, and the command line's caller is often an agent that wants the
/// number rather than the sentence.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answer {
    /// One line for the status bar. Empty when there is nothing to say.
    pub message: String,
    /// The same answer as data, for the command line and for a test.
    pub value: serde_json::Value,
}

impl Answer {
    pub fn said(message: impl Into<String>) -> Self {
        Self { message: message.into(), value: serde_json::Value::Null }
    }

    pub fn with(mut self, value: serde_json::Value) -> Self {
        self.value = value;
        self
    }

    pub fn nothing() -> Self {
        Self::default()
    }
}

/// What a provider is told about the window when it is opened.
///
/// Handed in on [`UiProvider::open`] rather than read from a global, so a test opens a provider on a
/// temporary folder and the released binary opens it on the person's own.
#[derive(Clone, Default)]
pub struct Context {
    /// The folder this window has open, when it has one.
    pub project: Option<PathBuf>,
    /// The file showing in the window when this provider was opened.
    ///
    /// [`UiProvider::showing`] is told when it *changes*, and the window compares before it tells —
    /// so a provider opened after that comparison had settled was never told at all. This is the
    /// other half: what it was when the provider opened. It is **which file**, never its text, for
    /// the reason written on that method.
    pub showing: Option<PathBuf>,
    /// The folders this machine has had open, newest first, which is the list `File -> Open Recent` draws.
    ///
    /// **Handed over rather than reached for**, which is the rule the project folder and the opacity already
    /// follow: a provider cannot read `services::store`, and a plugin that could would be a plugin deciding
    /// what a window knows. Agent-Tasks offers these as the choices in a ticket's `Project` dropdown, because a
    /// folder somebody has opened is a folder they might point a ticket at.
    pub recent_projects: Vec<PathBuf>,
    /// The folder this plugin keeps its own files in: `<settings folder>/plugins/<plugin id>`.
    ///
    /// `None` in a test that has no store, which is what stops a test reading or writing the settings
    /// of the person running it — the rule `UnluminateApp::load_settings` already keeps.
    pub folder: Option<PathBuf>,
    /// How to ask the window to draw again from another thread.
    ///
    /// A provider that owns a process needs this and one that only draws does not. Without it, a terminal that
    /// printed while nobody was pointing at the window would be a terminal nobody saw until the pointer moved,
    /// and the alternative — asking for a frame on a timer — keeps the graphics card busy while an agent sits
    /// at its prompt. `unluminate_git::Worker` and the terminal tile already work this way.
    pub wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Context")
            .field("project", &self.project)
            .field("recent_projects", &self.recent_projects.len())
            .field("folder", &self.folder)
            .field("wake", &self.wake.is_some())
            .finish()
    }
}

/// What a plugin's code is asked to do. One implementation per name in `plugins::UI_PROVIDERS`.
pub trait UiProvider: std::fmt::Debug {
    /// The name in the manifest, which is the name in the registry.
    fn id(&self) -> &'static str;

    /// Called the first time this plugin's pane, tab or settings page is shown, and never before.
    ///
    /// This is what makes a plugin lazy, and it is the reference editor's own arrangement: its documented reason
    /// for calling `createToolWindowContent` on the first click is that "if a user does not interact
    /// with the tool window, no plugin code will be loaded or executed". For Agent-Tasks it is the
    /// difference between opening a database file at startup and opening it when the board is first
    /// looked at.
    fn open(&mut self, context: &Context) -> Result<(), String>;

    /// True once [`Self::open`] has succeeded, so the window can tell "not opened yet" from "opened".
    fn is_open(&self) -> bool;

    /// Whether this provider draws decoration that needs a canvas behind it.
    ///
    /// False by default, so a provider that draws only with `egui` costs no pixmap, no rasterisation and no
    /// texture — the rule that a control which cannot apply is absent, applied to a renderer. Agent-Tasks
    /// answers true, and the window checks the manifest's `ui.chrome` and the `plugins.chrome` setting as
    /// well, so there are three ways to say no and one to say yes.
    fn draws_chrome(&self) -> bool {
        false
    }

    /// Draw the pane. Called once a frame while the pane is showing.
    ///
    /// **The ground is already painted**, in `look.palette.editor` with the window's opacity applied, and a
    /// provider must not paint its own. The decoration a provider records goes into a slot reserved between
    /// that ground and everything drawn here, and egui hands a layer's shapes to the tessellator in the order
    /// they arrive — so a second ground painted here would be painted over the decoration.
    fn pane(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request>;

    /// Draw the tab in the editing area. Called once a frame while that tab is showing.
    ///
    /// The default is the pane, because a plugin whose tab and pane show the same thing is the common
    /// case and a provider should not have to write it twice. Agent-Tasks overrides it, because a
    /// whole editing area holds the board and the ticket side by side where a 420 point column cannot.
    fn tab(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request> {
        self.pane(ui, look)
    }

    /// Draw the Settings page inside the rectangle every page gets.
    fn settings(&mut self, ui: &mut egui::Ui, look: &Look<'_>) -> Vec<Request>;

    /// Draw whatever modal this plugin has open, if it has one.
    ///
    /// A modal is drawn from the context rather than into a rectangle, because `components::modal` places it,
    /// drags it and resizes it, and it has to be **above** the panes — including the plugin's own. So it is a
    /// method of its own rather than something the pane draws, and the window calls it once a frame after every
    /// pane, which is where every other modal in Unluminate is drawn.
    ///
    /// Answers what it asked for and whether it closed; the default has no modal and says so.
    fn modal(&mut self, ctx: &egui::Context, look: &Look<'_>) -> (Vec<Request>, bool) {
        let _ = (ctx, look);
        (Vec::new(), false)
    }

    /// Where the modal just drawn left room for its decoration, if it draws any.
    ///
    /// **A modal is on a layer of its own, and that is the whole reason this exists.** A pane's decoration is
    /// rasterised into a slot the *window* reserved, because the window draws the pane's ground; a modal is
    /// drawn by `egui::Modal` on a foreground layer the window never touches, so the slot has to be reserved
    /// from inside — and `egui::Painter::set` writes to the layer its own painter belongs to, so the painter
    /// has to come back with it.
    ///
    /// Taken rather than read, once, on the frame the modal was drawn: a slot belongs to one frame's shape
    /// list and is meaningless on the next. `None` from a provider whose modal has no depth, which is every
    /// provider but one.
    fn take_the_modals_canvas(&mut self) -> Option<ChromeSlot> {
        None
    }

    /// Answer a command from the menu, from a button in the pane, or from the command line.
    ///
    /// The one path a change goes down, which is what `UnluminateApp::run_action` is for the window and
    /// `UnluminateApp::run_cli` is for the command line. A thing done by hand and the same thing done by an
    /// agent are therefore the same thing rather than two paths that agree today.
    fn command(&mut self, command: &str, arguments: &[String]) -> Result<Answer, String>;

    /// Every command this provider answers, with one line each, for `unluminate-cli plugin show`.
    fn commands(&self) -> Vec<(&'static str, &'static str)>;

    /// What the pane is showing, as data.
    ///
    /// Not optional. Unluminate's rule is that everything a person can do in the window an agent can do
    /// too, through the same code, and both are covered by tests. A pane drawn with `egui` is
    /// invisible to a test and to an agent unless it can be read, and a screenshot is not an answer to
    /// "how many tickets are in progress". This is what `unluminate-cli plugin view` prints and what a unit
    /// test asserts against, built from the same reads the drawing uses.
    fn view(&self) -> serde_json::Value;

    /// Told when this plugin gains or loses the keyboard.
    ///
    /// The window owns `Focus`, so it is the window that says so. A provider with a terminal in it draws that
    /// terminal as focused only while this is true, which is what stops the terminal reading a key the board's own
    /// fields were being typed into.
    fn keyboard(&mut self, has_it: bool) {
        let _ = has_it;
    }

    /// Catch up with whatever this plugin owns that is not drawing. Called once a frame while it is open.
    ///
    /// Answers whether something is still running, so the window knows to keep drawing. The default does
    /// nothing and answers no, which is right for a plugin that only draws: this exists for the one that
    /// owns processes. It must be cheap, because it runs on every frame — Agent-Tasks compares two integers
    /// per terminal and does nothing when neither moved.
    fn catch_up(&mut self) -> bool {
        false
    }

    /// What this provider wants the window to do, asked for **outside** a draw.
    ///
    /// [`Self::pane`] answers with requests because it is drawing; this is the same answer for a
    /// provider that decided something while nobody was looking at it. Agent-Chat needs it because a
    /// model's tool call arrives on a worker thread and has to be run whether or not the pane is
    /// showing — a turn that stalled because a pane was put away would be a fault nothing on the
    /// screen could explain. Drained once a frame beside [`Self::catch_up`], which is the one place a
    /// plugin is let to do something between frames.
    fn asking(&mut self) -> Vec<Request> {
        Vec::new()
    }

    /// The answer to a [`Request::RunCommand`], echoed back with the `id` that asked for it.
    ///
    /// `Ok` carries the command's own `result`; `Err` carries the sentence it refused with, which is
    /// the server's-own-words rule applied to Unluminate's own commands.
    fn answered(&mut self, id: &str, answer: Result<serde_json::Value, String>) {
        let _ = (id, answer);
    }

    /// Told that this pane was just zoomed, so it can keep the point under the pointer still.
    ///
    /// `ratio` is how much bigger the pane became - 1.15 for a notch out, its reciprocal for a notch in -
    /// and `above` is how far below the top of the pane's **body** the pointer was.
    ///
    /// **A provider corrects its own scroll because the window cannot reach it.** The rows the explorer
    /// draws hang off a scroll position the window keeps, and `task-1672`'s rule is applied there; a
    /// provider's scrolling belongs to the provider - the chat's conversation is an `egui::ScrollArea` and
    /// the board's listings are a number on the board - so it is told what happened and does the same
    /// arithmetic. Content laid out at a scale has a height proportional to it, so the point at
    /// `offset + above` lands at `(offset + above) * ratio` and putting it back is one subtraction.
    ///
    /// Nothing by default, which is right for a provider whose pane does not scroll.
    fn zoomed(&mut self, ratio: f32, above: f32) {
        let _ = (ratio, above);
    }

    /// Told what the window is showing, when it changes.
    ///
    /// The window owns this the way it owns the keyboard, so it is the window that says so — the
    /// shape [`Self::keyboard`] already set. Agent-Chat tells the model which project is open and
    /// which file is showing, because a chat in an editor that does not know what you are looking at
    /// is a browser tab. It is **which file**, never the file's text: a pane that quietly uploaded
    /// whatever was on the screen is a pane nobody could use on anything confidential.
    fn showing(&mut self, project: Option<&std::path::Path>, file: Option<&std::path::Path>) {
        let _ = (project, file);
    }

    /// This provider as its own type, for a caller that has to reach past the trait.
    ///
    /// Two callers need it, and both are inside Unluminate: a test driving the provider's own functions the way its
    /// buttons do, and the drawing, which is the provider's own code. A provider that is not in the binary
    /// could not offer this and would not be asked for it. It is `Option` rather than required so that a
    /// provider which has no reason to offer it says so by answering nothing.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Called when the plugin is switched off, when the project changes, or when the window closes.
    fn close(&mut self);
}

/// Build the provider named `name`, or `None` when this version has no such name.
///
/// The one place a name in `plugins::UI_PROVIDERS` becomes an object, so the registry and the code
/// cannot disagree about which names exist — `every_registered_provider_can_be_built` is the test that
/// keeps them in step.
pub fn provider(name: &str) -> Option<Box<dyn UiProvider>> {
    match name {
        "agent-tasks" => Some(Box::new(crate::services::agent_tasks::AgentTasks::new())),
        "agent-chat" => Some(Box::new(crate::services::agent_chat::AgentChat::new())),
        "database" => Some(Box::new(crate::services::database::DatabaseExplorer::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::UI_PROVIDERS;

    #[test]
    fn every_registered_provider_can_be_built() {
        // The registry is a list of names and this is the code behind them. A name with no code would
        // load a manifest whose pane is permanently empty, which is the exact outcome checking the
        // name against a registry exists to prevent.
        for name in UI_PROVIDERS {
            let built = provider(name).unwrap_or_else(|| panic!("{name} is registered with no code"));
            assert_eq!(built.id(), *name, "a provider should know its own name");
            assert!(!built.is_open(), "a provider is not open until it has been opened");
            assert!(!built.commands().is_empty(), "{name} answers no commands");
        }
        assert!(provider("nothing-like-this").is_none());
    }

    #[test]
    fn a_look_records_no_decoration_until_the_window_gives_it_a_canvas() {
        // `task-1765`. Every existing caller of `Look::of` — nearly all of them tests — goes on building a
        // look with no canvas behind it, and what it hands out records nothing however much is drawn into
        // it. That is what stops a test with no window paying for a rasteriser.
        let settings = Settings::new();
        let renderer = crate::services::text_renderer::TextRenderer::new();
        let look = Look::of(&settings, &renderer);
        assert!(!look.chrome.is_recording(), "a look built with no canvas records nothing");
        look.chrome.raised(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(10.0)),
            4.0,
            crate::services::vello_canvas::Fill::Solid(Color32::RED),
            crate::services::vello_canvas::Lift::Small,
        );
        assert!(look.chrome.take().is_empty());

        // And the same look drawing into a real one records.
        let chrome = crate::services::vello_canvas::Chrome::recording();
        let drawing = Look::of(&settings, &renderer).drawing_into(&chrome);
        assert!(drawing.chrome.is_recording());
        drawing.chrome.raised(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(10.0)),
            4.0,
            crate::services::vello_canvas::Fill::Solid(Color32::RED),
            crate::services::vello_canvas::Lift::Small,
        );
        // A band to draw the shadows in, the two shadows, the unclip and the surface — `Chrome::raised`.
        assert_eq!(chrome.take().len(), 5);
    }

    #[test]
    fn the_boards_four_surfaces_are_colours_unluminate_already_had() {
        // The palette is closed, and the board's ladder does not open it: the page, a lane, a card and a
        // well are `EDITOR`, `EXPLORER`, `CODE_PANEL` and `FIELD`, each within a few units a channel of the
        // picture the board is measured against. A test rather than a comment, because a later change that
        // reached for a colour of its own would otherwise pass quietly.
        let palette = Palette::active();
        assert_eq!(palette.board_page, color::editor());
        assert_eq!(palette.board_lane, color::explorer());
        assert_eq!(palette.board_card, color::code_panel());
        assert_eq!(palette.board_well, color::field());
        assert_eq!(palette.attached, color::git_added());
        // And the ladder really is a ladder: each step is lighter than the one behind it, or a card drawn
        // on a lane drawn on the page would be three rectangles nobody could tell apart.
        let brightness = |colour: Color32| u32::from(colour.r()) + u32::from(colour.g()) + u32::from(colour.b());
        assert!(brightness(palette.board_page) < brightness(palette.board_lane));
        assert!(brightness(palette.board_lane) < brightness(palette.board_card));
        assert!(brightness(palette.board_well) < brightness(palette.board_lane));
    }

    #[test]
    fn a_provider_is_handed_the_windows_own_look_and_cannot_name_a_colour() {
        let mut settings = Settings::new();
        settings.font_size = 17.0;
        settings.opacity = 0.5;
        // A real renderer, because `Look` carries one so that a provider can draw a character grid. Building
        // one loads the system fonts, which this test does not read: it is here so the value can exist.
        let renderer = crate::services::text_renderer::TextRenderer::new();
        let look = Look::of(&settings, &renderer);
        assert_eq!(look.font_size, 17.0);
        assert_eq!(look.opacity, 0.5);
        assert_eq!(look.palette, Palette::active(), "there is one palette and a plugin gets it");
        // The opacity setting reaches the ground a provider paints, so a pane is as transparent as the
        // editing area rather than the one opaque rectangle in the window.
        let ground = look.ground(look.palette.editor);
        assert_eq!(ground.a(), 128, "half opacity is half alpha");
        settings.opacity = 1.0;
        assert_eq!(Look::of(&settings, &renderer).ground(look.palette.editor).a(), 255);
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn every_fixed_height_on_a_board_follows_the_font_the_editor_is_set_in() {
        // Unluminate's font size control offers 48 and 64 point text. Every height on the board was written for the
        // default 16 — a card 84 points tall, a lane heading 34, the board's header 66 — so a large font scaled
        // the words and left the boxes, and a card title overlapped its own footer.
        let renderer = crate::services::text_renderer::TextRenderer::new();
        let mut settings = Settings::default();
        settings.font_size = crate::settings::DEFAULT_FONT_SIZE;
        let normal = Look::of(&settings, &renderer);
        assert_eq!(normal.scale(), 1.0, "the default font changes nothing");

        settings.font_size = 48.0;
        let large = Look::of(&settings, &renderer);
        assert_eq!(large.scale(), 3.0, "48 point text is three times the default");

        // Small text does not shrink the boxes: somebody who chooses 9 point wants small words, not cards too
        // short to press and tick boxes too small to hit.
        settings.font_size = 9.0;
        let small = Look::of(&settings, &renderer);
        assert_eq!(small.scale(), 1.0, "a small font leaves the boxes alone");

        for size in crate::settings::FONT_SIZES {
            settings.font_size = *size;
            let look = Look::of(&settings, &renderer);
            let card = crate::components::agent_tasks::card::height(&look);
            // What a card really has to hold, in the order `card::show` draws it: eight points of padding, a
            // title of two wrapped lines, and the 22 point band at the bottom that the footer and the epic chip
            // share. Whatever the size.
            let needed = 8.0 + look.font_size * 2.7 + 22.0;
            assert!(
                card >= needed,
                "at {size} point a card is {card} points and needs at least {needed}"
            );
        }
    }
}
