//! The palette, the measurements and the drawn icons.
//!
//! Every colour in [`Palette::QUILL_DARK`] was read out of `design/intial-design-screenshot.png` rather than
//! chosen by eye. The example at `examples/sample_design.rs` reports, for each region of that image, the
//! colour covering most of it and the most saturated colour in it, which is how the accents were found. Run
//! it with `cargo run --example sample_design` to check any of these against the design again.
//!
//! ## A colour is a question now, and the list of names is still closed
//!
//! `task-1776` asks for themes, and a theme is the answer to "what does `EDITOR` mean". Until it, every
//! name here was a `const` read at 689 places in 56 files, and a constant cannot be themed. So each name is
//! a function over the **active theme** — `color::editor()` rather than `color::EDITOR` — and everything
//! the style guide says about the palette is still true: this module is the whole list of colours Quill
//! draws with, a new one is added here with a comment saying where it was read from, and
//! `Color32::from_rgb` at the point of use is still how a window comes to have four slightly different
//! greys.
//!
//! The list lives once, in the [`palette!`] invocation below, and the struct, the default theme, the names
//! a manifest may set, the reader, the writer and the forty accessor functions are all generated from it.
//! Writing them out would be five places to forget a name.
//!
//! ## The active theme is thread-local
//!
//! A window is one thread, and a second window is a second **process** — `services::launcher::open_window`
//! runs `current_exe` — so nothing in the shipped binary wants two themes at once, and a process-global
//! would have been correct for the product. It would have been wrong for the tests:
//! `crates/quill-app/tests/screenshots.rs` holds 169 accepted pictures, cargo runs them in parallel in one
//! process, and a test that switched a global theme would recolour whatever else was mid-frame. Held per
//! thread, a theme chosen in one test cannot reach another's picture, and no test needs a lock or an
//! ordering.
//!
//! It is also the cheapest of the three shapes. A colour is read thousands of times a frame; a `RefCell`
//! borrow returning one `Color32` is a counter check and four bytes, where a `RwLock` is an atomic pair and
//! a `Cell<Palette>` copies all forty colours to read one.
//!
//! What it asks is that nothing paints off the thread that drew: the background workers — `quill_git`, the
//! text search, the symbol index and the debug adapter — hold no painter and name no colour, and the other
//! place a colour is read, `run_cli`, runs inside `pump_control` at the top of a frame.

use std::cell::RefCell;

use egui::{Color32, CornerRadius, Stroke, Vec2};

pub mod icon;

/// Generate the palette from one list.
///
/// Each entry becomes a field on [`Palette`], its value in [`Palette::QUILL_DARK`], a name in
/// [`Palette::NAMES`], an arm of [`Palette::get`] and [`Palette::set`], and a function in [`color`].
/// The doc comment written once reaches the field and the function.
macro_rules! palette {
    ($( $(#[$note:meta])* $name:ident = $default:expr; )*) => {
        /// The colours one theme is made of.
        ///
        /// `Copy`, so a theme is a value a test can assert on with no window — the seam
        /// `quill_core::mermaid::Scene` and `services::vello_canvas::Decor` already are.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Palette {
            $( $(#[$note])* pub $name: Color32, )*
        }

        impl Palette {
            /// The palette Quill shipped with, and the one every theme inherits the names it does not set.
            pub const QUILL_DARK: Palette = Palette { $( $name: $default, )* };

            /// Every role's name, in the order this file lists them.
            ///
            /// What a manifest may set, what `theme show` walks, and what the Settings page reads. One
            /// list, so a name cannot exist in one of the three and not the others.
            pub const NAMES: &'static [&'static str] = &[ $( stringify!($name), )* ];

            /// One role by name, or nothing when this version has no such role.
            pub fn get(&self, name: &str) -> Option<Color32> {
                match name { $( stringify!($name) => Some(self.$name), )* _ => None }
            }

            /// Set one role by name. False when the name is not one of [`Palette::NAMES`], which is what
            /// lets a manifest be refused with the list rather than loading with a colour it thought it
            /// had set.
            pub fn set(&mut self, name: &str, colour: Color32) -> bool {
                match name { $( stringify!($name) => { self.$name = colour; true } )* _ => false }
            }
        }

        /// The colours, by the names the style guide lists.
        pub mod color {
            use super::{with, Color32};
            pub use super::derived::*;
            $( $(#[$note])* pub fn $name() -> Color32 { with(|palette| palette.$name) } )*
        }
    };
}

palette! {
    /// Behind the text. The window's alpha is applied to this by the opacity setting.
    editor = Color32::from_rgb(0x1A, 0x1F, 0x26);
    /// The bar along the top holding the window buttons and the file name.
    title_bar = Color32::from_rgb(0x2A, 0x31, 0x3D);
    /// The bar holding the formatting controls.
    toolbar = Color32::from_rgb(0x1E, 0x22, 0x2A);
    /// Behind the file explorer.
    explorer = Color32::from_rgb(0x1F, 0x23, 0x2A);
    /// The strip at the bottom of the explorer counting the files.
    explorer_footer = Color32::from_rgb(0x1C, 0x20, 0x26);
    /// The bar along the very bottom of the window.
    status_bar = Color32::from_rgb(0x10, 0x15, 0x19);
    /// Inside a dropdown or a button that is not active.
    control = Color32::from_rgb(0x35, 0x3B, 0x46);
    /// Inside the box that filters the file list.
    field = Color32::from_rgb(0x1D, 0x21, 0x2A);
    /// Round the edge of a control.
    control_border = Color32::from_rgb(0x38, 0x3F, 0x4B);
    /// Between the panels.
    divider = Color32::from_rgb(0x2A, 0x30, 0x3B);
    /// Behind a menu. Darker than a control so that a control drawn on top of it stands out.
    menu = Color32::from_rgb(0x26, 0x2C, 0x36);

    /// Anything switched on: an active button, the caret, the row of the open file.
    accent = Color32::from_rgb(0x48, 0x9F, 0xF8);
    /// Behind the name of the file that is open.
    selected_row = Color32::from_rgb(0x30, 0x43, 0x61);
    /// There are changes that have not been saved.
    unsaved = Color32::from_rgb(0xFE, 0xBC, 0x2E);
    /// Behind selected text.
    text_selection = Color32::from_rgb(0x30, 0x43, 0x61);
    /// Behind a code block, a table and the front matter in the Markdown preview.
    ///
    /// A step up from `editor` rather than a colour of its own, so the block reads as a panel on the
    /// page rather than as a second surface. `task-1685` added it: a fence with no ground under it
    /// is the whole of what "code blocks aren't easy to read" meant.
    code_panel = Color32::from_rgb(0x23, 0x29, 0x33);
    /// Behind one piece of inline code, which is the same idea at the size of a word.
    code_chip = Color32::from_rgb(0x28, 0x2F, 0x3A);

    /// A heading in the editor, and the file name in the title bar.
    text_strong = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    /// Ordinary text in the editor.
    text = Color32::from_rgb(0xE8, 0xEB, 0xF1);
    /// A label on a control, and a name in the file list.
    text_control = Color32::from_rgb(0xC8, 0xCE, 0xDB);
    /// A heading in the explorer, the counts in its footer, and the status bar.
    text_dim = Color32::from_rgb(0x8B, 0x93, 0xA3);
    /// The words inside the filter box before anything is typed.
    text_faint = Color32::from_rgb(0x78, 0x80, 0x8F);

    /// The square in front of a Markdown file.
    file_markdown = Color32::from_rgb(0x41, 0x8C, 0xD9);
    /// The square in front of a plain text file.
    file_text = Color32::from_rgb(0x7E, 0x87, 0x95);

    /// The oldest commit in a file, in the blame column beside the line numbers.
    ///
    /// This pair is the one part of the palette not read out of `design/intial-design-screenshot.png`,
    /// because the design has no gutter in it. They were measured out of the capture the ask came with,
    /// `tasks/quill-ide-tdd.md` section 2, in the same way: the two colours covering the annotation
    /// column of that image.
    blame_old = Color32::from_rgb(0x3C, 0x7D, 0x64);
    /// The newest commit in a file. Everything between is interpolated by rank.
    blame_new = Color32::from_rgb(0xB4, 0x58, 0x8C);

    /// A file, or a line, that git does not have yet. Measured from the commit panel in the same
    /// capture, where it is the colour of the `added` count.
    git_added = Color32::from_rgb(0x7F, 0xCA, 0x98);
    /// A file, or a line, that differs from the version git has. The `modified` count in that capture.
    git_modified = Color32::from_rgb(0x4D, 0x9D, 0xC3);
    /// A file git is not tracking at all.
    git_untracked = Color32::from_rgb(0x9A, 0x8C, 0x5A);

    /// The blue a plugin's own page is built on, when that page is a copy of somebody else's.
    ///
    /// **This is the one place a second blue is right, and it took two reviews to be sure of it.** Quill's
    /// [`color::accent`] is an azure and the page the Agent-Tasks board is measured against is built on a
    /// periwinkle; the first pass kept the azure, on the grounds that a plugin should look like the rest of
    /// the window. `task-1765` asked for a board that looks *nearly identical to a picture*, and the
    /// reviewer named this as the most obvious mismatch in it, twice. Between a rule about how a plugin
    /// should look and an instruction about how this one must look, the instruction wins.
    ///
    /// It is contained: it reaches `plugin_ui::Palette` as `board_accent` and only the board reads it.
    /// Quill's own accent still means *this is where the keyboard is* everywhere, the board included, so
    /// the two never say the same thing in two colours.
    board_accent = Color32::from_rgb(0x4C, 0x6E, 0xF5);

    /// The colour an agent wears: the round badge on a card, and the dot on the `AGENT DONE` lane.
    ///
    /// The palette is closed and this does not open it, for the reason [`derived::breakpoint`] records
    /// beside itself. Quill's palette has a red, an amber, a green, two blues and a pink, and the board
    /// needs the four lanes to be four colours a person can tell apart at nine points across — grey, red,
    /// blue and this. It is the violet of the picture the board is measured against,
    /// `_agent_output/task-1765-vello-board/reference-board.png`, and it is used nowhere else.
    agent = Color32::from_rgb(0x9B, 0x7C, 0xF6);

    /// The three window buttons.
    close = Color32::from_rgb(0xFF, 0x5F, 0x57);
    minimise = Color32::from_rgb(0xFE, 0xBC, 0x2E);
    maximise = Color32::from_rgb(0x28, 0xC8, 0x40);

    /// A drawn icon sitting there: a rail button whose pane is put away, the explorer's arrow.
    ///
    /// The five roles from here down are `task-1776`, and each is **the colour that was already being
    /// passed** at the point of use, so nothing moved when they were added. They exist because Material
    /// Theme UI's own theme files carry exactly this — `Actions.Grey`, `Objects.Blue`,
    /// `Checkbox.Focus.Wide` — and because without them a theme could recolour the whole window and leave
    /// every icon in the greys of the theme before it.
    icon = Color32::from_rgb(0x8B, 0x93, 0xA3);
    /// The same icon when what it opens is open, or its button is on.
    icon_active = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    /// The same icon when it cannot be used — git outside a repository.
    icon_disabled = Color32::from_rgb(0x78, 0x80, 0x8F);
    /// A folder's arrow, and the folder mark the `material` icon set draws in front of its name.
    folder = Color32::from_rgb(0x8B, 0x93, 0xA3);
    /// The same folder when it is open. Atom Material Icons' one loud move, and the reason
    /// `folder` and `icon` are two roles rather than one.
    folder_open = Color32::from_rgb(0x48, 0x9F, 0xF8);
}

/// Colours that are another colour, and the marks a person makes on their own text.
///
/// Re-exported from [`color`], so `color::breakpoint()` and `color::HIGHLIGHT_YELLOW` read the way every
/// other name in the palette does. They are here rather than in the [`palette!`] list because each is
/// **defined as** something else, and a theme that could set them separately could make a breakpoint a
/// different red from the one the close button is.
pub mod derived {
    use super::{color, Color32};
    use quill_core::Rgba;

    /// The four colours a passage can be marked in, on the editor's right click menu.
    ///
    /// The palette is closed and these do not open it: they are the accents Quill shipped with — the
    /// unsaved amber, the accent blue, git's added green and blame's newest pink — each at the same alpha,
    /// which is low enough that the writing over them stays readable at every window opacity. A colour
    /// chosen in the wheel is somebody's own mark on their own text, which is the exception the style
    /// guide records beside a syntax theme's token colours.
    ///
    /// They are `quill_core::Rgba` rather than `Color32` because a highlight is a value that is written to
    /// a file and sent over the command line's wire as well as painted, and because egui's own alpha is
    /// premultiplied while a colour a person chose is not. [`super::color32`] is the one place the two
    /// meet.
    ///
    /// **A theme does not change them**, and that is `task-1776`'s decision rather than an omission. A
    /// mark carries the colour it was made in, in a file beside the project; if the four defaults moved
    /// with the theme, a document marked under one theme and read under another would show four colours
    /// the menu no longer offers.
    pub const HIGHLIGHT_ALPHA: u8 = 0x59;
    pub const HIGHLIGHT_YELLOW: Rgba = Rgba::new(0xFE, 0xBC, 0x2E, HIGHLIGHT_ALPHA);
    pub const HIGHLIGHT_GREEN: Rgba = Rgba::new(0x7F, 0xCA, 0x98, HIGHLIGHT_ALPHA);
    pub const HIGHLIGHT_BLUE: Rgba = Rgba::new(0x48, 0x9F, 0xF8, HIGHLIGHT_ALPHA);
    pub const HIGHLIGHT_PINK: Rgba = Rgba::new(0xB4, 0x58, 0x8C, HIGHLIGHT_ALPHA);

    /// How opaque the band behind the line a program is stopped on is, and how much of the accent's
    /// brightness it keeps.
    ///
    /// Fitted to the colour the design shipped with: `#1C3C5E` at `0x9E`, premultiplied, is the accent at
    /// a shade over 61 per cent of its brightness. The three channels want 0.628, 0.609 and 0.612, because
    /// the original was sampled off the design rather than computed from the accent, so one number
    /// reproduces it to **within one unit a channel** — which is a difference no eye has ever seen at an
    /// alpha of 158, and worth far less than a band that follows the accent under a pink theme.
    const EXECUTION_ALPHA: u8 = 0x9E;
    const EXECUTION_BRIGHTNESS: f32 = 0.615;

    /// The band behind the line the program is stopped on.
    ///
    /// The accent, at an alpha of its own so it cannot be mistaken for a passage somebody marked: the four
    /// highlight colours are all at [`HIGHLIGHT_ALPHA`] and this is deliberately not one of them. It is
    /// painted under the glyphs, where `paint_highlights` paints.
    ///
    /// **Derived rather than a role a manifest can set**, and that is a trap avoided rather than a
    /// simplification. Every other colour in the palette is opaque and is written `#RRGGBB`; this one is
    /// the only one whose alpha carries meaning, so a theme that set it in the same three bytes as the
    /// rest would paint an opaque band over the line the debugger stopped on and hide the code under it.
    /// Following the accent is also what a person means by choosing a pink theme.
    pub fn execution_point() -> Color32 {
        let accent = color::accent();
        let dim = |channel: u8| (channel as f32 * EXECUTION_BRIGHTNESS).round() as u8;
        Color32::from_rgba_unmultiplied(
            dim(accent.r()),
            dim(accent.g()),
            dim(accent.b()),
            EXECUTION_ALPHA,
        )
    }

    /// The breakpoint dot in the gutter.
    ///
    /// The palette is closed and this does not open it: it is the close button's red, which is the one red
    /// the design already has, and a breakpoint is red in every editor there has ever been. A breakpoint
    /// that is switched off, or one the adapter could not bind, is drawn as a ring in the same colour
    /// rather than in a second one — what is different about it is that it is hollow, not that it is
    /// another colour.
    pub fn breakpoint() -> Color32 {
        color::close()
    }

    /// A value painted at the end of a line while the program is paused.
    ///
    /// The faintest text, because an inline value is decoration over somebody's code and must never be
    /// mistaken for text in the document.
    pub fn inline_value() -> Color32 {
        color::text_faint()
    }

    /// The tint a variable that has just changed wears: the unsaved amber, which is what stepping is for
    /// and is the one thing on the tree worth looking at twice.
    pub fn value_changed() -> Color32 {
        color::unsaved()
    }

    /// The ring round the badge of a ticket whose agent is running in this window.
    ///
    /// Git's added green rather than a colour of its own: it is the green Quill already means "there is
    /// something here that was not here before" by, which is what an attached terminal is.
    pub fn attached() -> Color32 {
        color::git_added()
    }
}

/// Which drawn set an icon comes from.
///
/// The sixth registry of the shape `plugins::RENDERERS` started: a name checked against a list, so a
/// manifest asking for a set this version has not got is refused with the list rather than loading as a
/// theme whose buttons are drawn as nothing. See `theme::icon` for what each set covers and what falls
/// through to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconSet {
    /// The marks Quill shipped with: a solid disclosure triangle, stroked rail buttons, no folder mark.
    ///
    /// Kept and selectable rather than deleted, which is what makes this a **set** rather than a
    /// redrawing: One Dark names it, because One Dark's own IntelliJ icons are the IDE's rather than
    /// Material's, and anybody who preferred the triangles has them back in one line.
    Classic,
    /// Heavier, rounder and filled where the classic one is a stroke, in the manner of Atom Material
    /// Icons: a chevron for a disclosure, and a folder mark in front of a folder's name.
    ///
    /// **The default**, because `task-1776` asks for the marks on the rail and the explorer's arrow to be
    /// *improved* — not merely to become choosable. A seam that left the default where it was would have
    /// answered half the ticket.
    #[default]
    Material,
}

impl IconSet {
    /// The word a manifest, the settings file and the command line call it.
    pub fn name(self) -> &'static str {
        match self {
            IconSet::Classic => "classic",
            IconSet::Material => "material",
        }
    }

    pub fn parse(name: &str) -> Option<IconSet> {
        match name.trim().to_lowercase().as_str() {
            "classic" => Some(IconSet::Classic),
            "material" => Some(IconSet::Material),
            _ => None,
        }
    }

    /// Both of them, for the Settings page's dropdown and for `plugins::ICON_SETS`.
    pub const ALL: [IconSet; 2] = [IconSet::Material, IconSet::Classic];
}

/// One theme: what every name in the palette means, what the tokens are coloured, and which icons are
/// drawn.
///
/// Read from a `plugin.kind = theme` manifest, or [`Theme::quill_dark`], which is the one built in.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// `quill/dark`, `themes-bundle-1/dracula` — the plugin and the theme, as a contributed pane is named.
    pub key: String,
    /// What a person reads in the Settings list.
    pub name: String,
    /// Which plugin it came from, for `theme list`. `quill` for the built-in one.
    pub plugin: String,
    /// False is refused when a manifest is read — see `services::plugins::theme_from`. Kept on the value
    /// so the seam is visible and so `theme list` can answer the question.
    pub dark: bool,
    pub palette: Palette,
    /// The nine token colours, or none — in which case each language plugin's own are used, which is what
    /// every plugin that shipped before `task-1776` carries.
    pub syntax: Option<crate::services::plugins::SyntaxTheme>,
    pub icons: IconSet,
}

impl Theme {
    /// The theme Quill shipped with: the design's own numbers, and **no syntax colours at all**.
    ///
    /// Naming none is what makes the default build pixel-identical to the one before themes existed. Every
    /// language plugin goes on colouring its own files with the scheme in its own manifest until a theme
    /// that names the nine is chosen, and then that one theme recolours all of them at once — which is
    /// what a colour scheme is for and why five copies of Dracula were the wrong shape.
    pub fn quill_dark() -> Theme {
        Theme {
            key: "quill/dark".to_owned(),
            name: "Quill Dark".to_owned(),
            plugin: "quill".to_owned(),
            dark: true,
            palette: Palette::QUILL_DARK,
            syntax: None,
            icons: IconSet::default(),
        }
    }

    /// The same theme with one colour used for everything the accent means.
    ///
    /// Material Theme UI's best known setting, and the one thing on its configuration page somebody
    /// changes twice a year. It reaches the two roles that **are** the accent rather than merely being
    /// blue: the accent itself and an open folder's mark. The wash behind the line a program is stopped on
    /// follows it without being named here, because `derived::execution_point` is worked out from the
    /// accent rather than stored.
    pub fn with_accent(mut self, accent: Color32) -> Theme {
        self.palette.folder_open = accent;
        self.palette.accent = accent;
        self
    }
}

thread_local! {
    /// The theme this thread paints in. See the note at the top of this file for why it is not global.
    static ACTIVE: RefCell<Theme> = RefCell::new(Theme::quill_dark());
}

/// Read one colour out of the active theme.
///
/// The whole of what an accessor in [`color`] does. One `RefCell` borrow and a four byte copy, which is
/// what makes reading a colour thousands of times a frame cost nothing worth measuring.
fn with<T>(read: impl FnOnce(&Palette) -> T) -> T {
    ACTIVE.with_borrow(|theme| read(&theme.palette))
}

/// Paint in this theme from now on, on this thread.
///
/// `apply` still has to be called with a context afterwards, because egui keeps its own copy of the
/// colours in its style and has to be told. The window does both in one place —
/// `QuillApp::apply_the_theme` — so the two can never be half done.
pub fn activate(theme: Theme) {
    ACTIVE.with_borrow_mut(|active| *active = theme);
}

/// The theme this thread is painting in.
pub fn active() -> Theme {
    ACTIVE.with_borrow(Theme::clone)
}

/// The active theme's palette, which is what a plugin's provider is handed.
pub fn palette() -> Palette {
    ACTIVE.with_borrow(|theme| theme.palette)
}

/// Which icons the active theme draws with.
pub fn icons() -> IconSet {
    ACTIVE.with_borrow(|theme| theme.icons)
}

/// The nine token colours the active theme names, if it names them.
///
/// Asked at the moment a file is coloured, exactly as `Plugins::renders` is asked before a diagram is
/// drawn, so choosing a theme recolours every open file in the same frame rather than at the next restart.
pub fn syntax() -> Option<crate::services::plugins::SyntaxTheme> {
    ACTIVE.with_borrow(|theme| theme.syntax.clone())
}

/// Measurements taken from the design.
pub mod size {
    /// Height of the bar holding the window buttons and the file name.
    pub const TITLE_BAR: f32 = 50.0;
    /// Width of the rail of pane buttons down the far left of the window.
    ///
    /// Narrower than IntelliJ's, which is about forty points, because `task-1658` asks for that and
    /// because Quill's holds three buttons rather than a dozen. Twenty four for the button, six either
    /// side — and the six on the left is exactly what `components::resize_edges` takes, so a button and
    /// the window's own left grip never fight over the same point.
    pub const ACTIVITY_BAR: f32 = 36.0;
    /// Height of the bar along the bottom of the window.
    pub const STATUS_BAR: f32 = 32.0;
    /// Width of the file explorer.
    pub const EXPLORER: f32 = 248.0;
    /// Height of the strip counting the files.
    pub const EXPLORER_FOOTER: f32 = 28.0;
    /// One row in the file list.
    pub const ROW: f32 = 28.0;
    /// How far one level of nesting indents.
    pub const INDENT: f32 = 18.0;
    /// Space between the text and the left edge of the editing area.
    pub const EDITOR_PADDING_X: f32 = 43.0;
    /// Space between the text and the top of the editing area.
    pub const EDITOR_PADDING_Y: f32 = 36.0;
    /// The narrowest an editing pane may be dragged.
    ///
    /// Wide enough to still be an editor rather than a stripe: the gutter, the padding either side
    /// and enough room for a line of text. A divider that could be dragged past it would be a way of
    /// losing a pane off the side of the window.
    pub const EDITOR_PANE_MIN: f32 = 160.0;
    /// The window's rounded corner.
    pub const WINDOW_CORNER: u8 = 12;
    /// A control's rounded corner.
    pub const CONTROL_CORNER: u8 = 6;
}

/// A highlight's colour as egui wants it.
///
/// The one place the two spellings of a colour meet. `quill_core::Rgba` is what a mark is stored,
/// written and sent as — four plain bytes with the alpha kept separate — and egui's `Color32` keeps
/// its alpha premultiplied. Converting in one function is what stops a highlight coming out too
/// bright in one place and right in another.
pub fn color32(color: quill_core::Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

/// Set up egui so that the ordinary controls come out looking like the design, rather than restyling each
/// one where it is used.
///
/// Called again whenever the theme changes, because egui keeps its own copy of these colours in its style.
/// `interface_scale` is the interface font size as a multiple of egui's own default — `appearance.ui.font.size`
/// divided by the size egui sets a body of text in — which is IntelliJ's `Use custom font` and is 1.0 until
/// somebody asks for something else.
pub fn apply(ctx: &egui::Context) {
    apply_scaled(ctx, 1.0);
}

/// The same, with the interface set larger or smaller.
pub fn apply_scaled(ctx: &egui::Context, interface_scale: f32) {
    // egui scales its whole interface — every menu, the explorer, the status bar — when command and
    // plus is pressed. That is a browser's zoom, and it is not what an editor's zoom means: Quill's
    // command and plus changes the size the *document* is set in and leaves the window alone. With
    // egui's own left on, one press would do both.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    let scale = interface_scale.clamp(0.6, 2.0);
    ctx.all_styles_mut(|style| {
        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.panel_fill = color::toolbar();
        visuals.window_fill = color::menu();
        visuals.extreme_bg_color = color::field();
        visuals.faint_bg_color = color::explorer_footer();
        visuals.window_corner_radius = CornerRadius::same(size::CONTROL_CORNER);
        visuals.window_stroke = Stroke::new(1.0, color::control_border());
        visuals.selection.bg_fill = color::accent();
        visuals.selection.stroke = Stroke::new(1.0, color::text_strong());

        let corner = CornerRadius::same(size::CONTROL_CORNER);
        // Not interactive: labels and separators.
        visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
        visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color::divider());
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, color::text_control());
        visuals.widgets.noninteractive.corner_radius = corner;
        // Sitting there, not being pointed at.
        visuals.widgets.inactive.bg_fill = color::control();
        visuals.widgets.inactive.weak_bg_fill = color::control();
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, color::control_border());
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, color::text_control());
        visuals.widgets.inactive.corner_radius = corner;
        // Being pointed at.
        visuals.widgets.hovered.bg_fill = color::control().gamma_multiply(1.25);
        visuals.widgets.hovered.weak_bg_fill = color::control().gamma_multiply(1.25);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, color::accent().gamma_multiply(0.6));
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, color::text_strong());
        visuals.widgets.hovered.corner_radius = corner;
        // Being pressed.
        visuals.widgets.active.bg_fill = color::accent();
        visuals.widgets.active.weak_bg_fill = color::accent();
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, color::accent());
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, color::text_strong());
        visuals.widgets.active.corner_radius = corner;
        // A dropdown that is open.
        visuals.widgets.open.bg_fill = color::control();
        visuals.widgets.open.weak_bg_fill = color::control();
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, color::accent());
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, color::text_strong());
        visuals.widgets.open.corner_radius = corner;

        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.menu_margin = egui::Margin::same(6);

        // The interface's own font size, which is IntelliJ's Appearance -> Use custom font.
        //
        // **Set from egui's own defaults rather than multiplied in place**, because this function is
        // called again every time the theme changes: scaling what is already there would compound, so
        // choosing 16 points twice would land on 20. Reading the defaults each time makes it absolute,
        // and at a scale of one it writes back exactly what egui had, so a Quill that names no size in
        // its settings file is drawn exactly as it always was.
        let defaults = egui::Style::default();
        for (kind, font) in style.text_styles.iter_mut() {
            if let Some(default) = defaults.text_styles.get(kind) {
                font.size = (default.size * scale).round();
            }
        }
    });
}

/// The family name egui uses for the interface's bold text.
pub const BOLD_FAMILY: &str = "quill-bold";

/// Set the interface in a real font, so that the toolbar's bold B is actually bold.
///
/// egui's built in fonts have no bold face, so its `strong` styling only brightens the colour. The design
/// shows a genuinely bold B, so the family Quill is using is handed to egui as well, with its bold face
/// under a name the toolbar can ask for. egui's own fonts stay in the list behind ours, because they carry
/// symbols such as the triangles in front of a folder that a text face does not have.
pub fn install_fonts(ctx: &egui::Context, family: &str, regular: Option<Vec<u8>>, bold: Option<Vec<u8>>) {
    let mut fonts = egui::FontDefinitions::default();
    let mut bold_stack = Vec::new();
    if let Some(bytes) = bold {
        fonts.font_data.insert("quill-ui-bold".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        bold_stack.push("quill-ui-bold".to_owned());
    }
    if let Some(bytes) = regular {
        fonts.font_data.insert("quill-ui".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.insert(0, "quill-ui".to_owned());
        }
        bold_stack.push("quill-ui".to_owned());
    }
    // Whatever egui already had stays behind ours as a fallback for symbols.
    if let Some(defaults) = fonts.families.get(&egui::FontFamily::Proportional) {
        for name in defaults.clone() {
            if !bold_stack.contains(&name) {
                bold_stack.push(name);
            }
        }
    }
    if !bold_stack.is_empty() {
        fonts.families.insert(egui::FontFamily::Name(BOLD_FAMILY.into()), bold_stack);
    }
    let _ = family;
    ctx.set_fonts(fonts);
}

/// Apply the opacity setting to a background colour.
///
/// Only backgrounds go through this. Text, icons and the caret are always drawn at full alpha, which is
/// what lets the desktop show through the window without making the writing hard to read.
pub fn faded(base: Color32, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The colour of the square in front of a file, by what kind of file it is.
///
/// Markdown gets the blue square because Quill treats it differently, having a preview for it. Every
/// other kind of text gets the grey one, whether Quill knows the extension or not. What the status bar
/// calls the file is decided by `services::file_kind::kind_name`, not here.
pub fn file_marker(path: &std::path::Path) -> Color32 {
    if crate::services::file_kind::is_markdown(Some(path)) {
        color::file_markdown()
    } else {
        color::file_text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test that makes the 689-site rewrite safe: every colour Quill shipped with, still there.
    #[test]
    fn quill_dark_is_exactly_what_shipped() {
        activate(Theme::quill_dark());
        assert_eq!(color::editor(), Color32::from_rgb(0x1A, 0x1F, 0x26));
        assert_eq!(color::title_bar(), Color32::from_rgb(0x2A, 0x31, 0x3D));
        assert_eq!(color::accent(), Color32::from_rgb(0x48, 0x9F, 0xF8));
        assert_eq!(color::text_strong(), Color32::WHITE);
        assert_eq!(color::text_faint(), Color32::from_rgb(0x78, 0x80, 0x8F));
        assert_eq!(color::git_added(), Color32::from_rgb(0x7F, 0xCA, 0x98));
        assert_eq!(color::board_accent(), Color32::from_rgb(0x4C, 0x6E, 0xF5));
        assert_eq!(color::close(), Color32::from_rgb(0xFF, 0x5F, 0x57));
        // The five icon roles are the colours that were being passed before they existed.
        assert_eq!(color::icon(), color::text_dim(), "an icon sitting there was TEXT_DIM");
        assert_eq!(color::icon_active(), color::text_strong());
        assert_eq!(color::icon_disabled(), color::text_faint());
        assert_eq!(color::folder(), color::text_dim(), "the explorer's arrow was TEXT_DIM");
    }

    #[test]
    fn a_derived_colour_follows_the_one_it_is_defined_as() {
        activate(Theme::quill_dark());
        assert_eq!(color::breakpoint(), color::close());
        assert_eq!(color::inline_value(), color::text_faint());
        assert_eq!(color::value_changed(), color::unsaved());
        assert_eq!(color::attached(), color::git_added());

        let mut theme = Theme::quill_dark();
        theme.palette.close = Color32::from_rgb(0xFF, 0x61, 0x88);
        activate(theme);
        assert_eq!(color::breakpoint(), Color32::from_rgb(0xFF, 0x61, 0x88), "and it followed");
        activate(Theme::quill_dark());
    }

    #[test]
    fn a_theme_reaches_every_accessor() {
        let mut theme = Theme::quill_dark();
        theme.key = "test/monokai".to_owned();
        theme.palette.editor = Color32::from_rgb(0x2D, 0x2A, 0x2E);
        theme.palette.accent = Color32::from_rgb(0xFF, 0xD8, 0x66);
        activate(theme);
        assert_eq!(color::editor(), Color32::from_rgb(0x2D, 0x2A, 0x2E));
        assert_eq!(color::accent(), Color32::from_rgb(0xFF, 0xD8, 0x66));
        assert_eq!(active().key, "test/monokai");

        activate(Theme::quill_dark());
        assert_eq!(color::editor(), Color32::from_rgb(0x1A, 0x1F, 0x26), "and back");
    }

    #[test]
    fn every_role_is_readable_and_writable_by_name() {
        let mut palette = Palette::QUILL_DARK;
        for name in Palette::NAMES {
            assert!(palette.get(name).is_some(), "{name} can be read");
            assert!(palette.set(name, Color32::RED), "{name} can be set");
        }
        assert_eq!(palette.editor, Color32::RED);
        assert!(palette.get("editor_background").is_none(), "a name Quill has not got");
        assert!(!palette.set("editor_background", Color32::RED));
    }

    #[test]
    fn an_accent_reaches_everything_that_means_the_accent() {
        activate(Theme::quill_dark());
        // What the design shipped, to within one unit a channel — see `EXECUTION_BRIGHTNESS`.
        let shipped = Color32::from_rgba_premultiplied(0x1C, 0x3C, 0x5E, 0x9E);
        let derived = color::execution_point();
        assert_eq!(derived.a(), shipped.a());
        for (was, now) in shipped.to_array().iter().zip(derived.to_array()) {
            assert!(was.abs_diff(now) <= 1, "{shipped:?} against {derived:?}");
        }

        activate(Theme::quill_dark().with_accent(Color32::from_rgb(0xFF, 0x79, 0xC6)));
        assert_eq!(color::accent(), Color32::from_rgb(0xFF, 0x79, 0xC6));
        assert_eq!(color::folder_open(), Color32::from_rgb(0xFF, 0x79, 0xC6));
        let wash = color::execution_point();
        assert_eq!(wash.a(), 0x9E, "the wash keeps its own alpha, not a highlight's");
        assert!(wash.r() > wash.b(), "and it followed the accent into the pink");
        activate(Theme::quill_dark());
    }

    /// Why the active theme is thread-local rather than global — see the note at the top of this file.
    #[test]
    fn the_active_theme_does_not_leak_between_threads() {
        activate(Theme::quill_dark());
        let elsewhere = std::thread::spawn(|| {
            let mut theme = Theme::quill_dark();
            theme.palette.editor = Color32::from_rgb(0x0F, 0x11, 0x1A);
            activate(theme);
            color::editor()
        })
        .join()
        .expect("the other thread finished");
        assert_eq!(elsewhere, Color32::from_rgb(0x0F, 0x11, 0x1A), "it got its own");
        assert_eq!(color::editor(), Color32::from_rgb(0x1A, 0x1F, 0x26), "and this one is untouched");
    }

    /// The interface size is set from egui's defaults rather than multiplied into what is there.
    ///
    /// `apply_scaled` runs again on every theme change, so a scale applied in place would compound:
    /// choosing sixteen points and then choosing a theme would land on twenty.
    #[test]
    fn the_interface_size_is_absolute_rather_than_compounding() {
        let context = egui::Context::default();
        let size_of = |context: &egui::Context| {
            context.style_of(egui::Theme::Dark).text_styles.get(&egui::TextStyle::Body).map(|font| font.size)
        };
        apply(&context);
        let plain = size_of(&context).expect("egui has a body style");

        apply_scaled(&context, 1.6);
        let once = size_of(&context).expect("still there");
        assert!(once > plain, "asking for a larger interface makes it larger");
        apply_scaled(&context, 1.6);
        assert_eq!(size_of(&context), Some(once), "and asking twice is asking once");

        apply_scaled(&context, 1.0);
        assert_eq!(size_of(&context), Some(plain), "and one is exactly what egui had");
    }

    #[test]
    fn an_icon_set_is_named_the_way_the_settings_file_writes_it() {
        for set in IconSet::ALL {
            assert_eq!(IconSet::parse(set.name()), Some(set));
        }
        assert_eq!(IconSet::parse("MATERIAL"), Some(IconSet::Material));
        assert_eq!(IconSet::default(), IconSet::Material, "the improved marks are what a window comes up in");
        assert_eq!(IconSet::parse("atom"), None);
    }
}
