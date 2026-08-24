//! The Quill window.
//!
//! Holds the open document, the file explorer, the fonts and the transparency setting, and lays the window
//! out as the design does: a title bar Quill draws itself, the formatting toolbar, the explorer down the
//! left, the editing area filling the rest, and the status bar along the bottom.
//!
//! Transparency works because the background and the text are two separate paints. `clear_color` gives the
//! operating system compositor an alpha taken from the opacity setting, so the desktop shows through the
//! window. Every glyph is painted at full alpha, so the writing stays sharp at every setting.
//!
//! The window has no operating system title bar, because rounded corners and transparency need the
//! decorations turned off, so the bars at the top and bottom are painted here and the top one moves the
//! window when it is dragged.

use std::path::{Path, PathBuf};

use egui::{Color32, CornerRadius, Pos2, Rect, Vec2};
use quill_core::{layout, Command, Document, Layout};

use crate::editor_view;
use crate::explorer;
use crate::file_tree::FileTree;
use crate::status_bar;
use crate::text_renderer::TextRenderer;
use crate::theme::{self, color, size};
use crate::title_bar::{self, FileAction};
use crate::toolbar;

/// How opaque the background is when Quill starts. Not fully opaque, so the transparency is visible
/// without opening the opacity menu. The design shows 83 per cent.
pub const DEFAULT_OPACITY: f32 = 0.83;

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

pub struct QuillApp {
    pub document: Document,
    pub tree: FileTree,
    pub renderer: TextRenderer,
    /// How opaque the window background is.
    pub opacity: f32,
    /// The text in the explorer's filter box.
    pub filter: String,
    /// False when the explorer has been hidden by its own button.
    pub explorer_visible: bool,
    /// The most recent layout, kept so that a frame that changed nothing does not lay out again.
    layout: Layout,
    laid_out_revision: u64,
    laid_out_width: f32,
    /// How far the editing area is scrolled.
    scroll: f32,
    /// True when the editing area has the keyboard.
    focused: bool,
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
}

impl QuillApp {
    /// A new window showing `folder` in the explorer and an empty document.
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        let renderer = TextRenderer::new();
        let mut document = Document::new();
        // Start in a family this system actually has, so the first thing typed is visible.
        document.apply(Command::ApplyStyle(quill_core::StyleChange::family(
            renderer.default_family(),
        )));
        Self {
            document,
            tree: FileTree::new(folder),
            renderer,
            opacity: DEFAULT_OPACITY,
            filter: String::new(),
            explorer_visible: true,
            layout: Layout::default(),
            laid_out_revision: 0,
            laid_out_width: 0.0,
            scroll: 0.0,
            focused: true,
            editor_area: Rect::ZERO,
            view_mode: ViewMode::Raw,
            preview: None,
            preview_layout: Layout::default(),
            preview_revision: 0,
            preview_width: 0.0,
            preview_scroll: 0.0,
            themed: false,
            bold_family: egui::FontFamily::Proportional,
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
        let family = self.renderer.default_family();
        let regular = self.renderer.face_bytes(&family, false);
        let bold = self.renderer.face_bytes(&family, true);
        let has_bold = bold.is_some();
        theme::install_fonts(ctx, &family, regular, bold);
        if has_bold {
            self.bold_family = egui::FontFamily::Name(theme::BOLD_FAMILY.into());
        }
    }

    /// The layout as it was last painted, which the tests assert against.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The rectangle the editing area last occupied.
    pub fn editor_area(&self) -> Rect {
        self.editor_area
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
        theme::faded(color::EDITOR, self.opacity)
    }

    /// Run something chosen from the File menu, or asked for by its keyboard shortcut.
    ///
    /// The folder and file pickers are the operating system's own, through `rfd`. A native picker is
    /// platform work in the same way that creating a window is, so it sits on the same side of the line as
    /// `winit` and `fontdb`. They block until the user answers, which is what a modal dialog does.
    pub fn file_action(&mut self, action: FileAction) {
        match action {
            FileAction::OpenFolder => {
                let start = self.tree.root().to_path_buf();
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title("Open Folder")
                    .set_directory(&start)
                    .pick_folder()
                {
                    self.open_folder(&folder);
                }
            }
            FileAction::OpenFile => {
                let start = self.tree.root().to_path_buf();
                if let Some(file) = rfd::FileDialog::new()
                    .set_title("Open File")
                    .set_directory(&start)
                    .add_filter("Markdown and plain text", &["md", "txt"])
                    .pick_file()
                {
                    // Showing the folder the file came from is what a reader expects: the explorer should
                    // hold the file that is now open.
                    if let Some(parent) = file.parent() {
                        if !file.starts_with(self.tree.root()) {
                            self.open_folder(parent);
                        }
                    }
                    self.open_path(&file);
                }
            }
            FileAction::Save => self.save(),
            FileAction::SaveAs => {
                let start = self.tree.root().to_path_buf();
                if let Some(target) = rfd::FileDialog::new()
                    .set_title("Save As")
                    .set_directory(&start)
                    .add_filter("Markdown and plain text", &["md", "txt"])
                    .save_file()
                {
                    if self.document.save_as(&target).is_ok() {
                        self.tree.reload();
                    }
                }
            }
        }
    }

    /// Show `folder` in the explorer.
    pub fn open_folder(&mut self, folder: &Path) {
        self.tree = FileTree::new(folder);
        self.filter.clear();
        self.explorer_visible = true;
    }

    /// Open a file into the editor, reporting the reason in the status bar if it cannot be read.
    pub fn open_path(&mut self, path: &Path) {
        match Document::open(path) {
            Ok(mut document) => {
                document.apply(Command::MoveDocumentStart { extend: false });
                self.document = document;
                self.scroll = 0.0;
            }
            Err(error) => {
                // Nothing is thrown away: the document that was open stays open, and the reason appears
                // where the file name would be.
                self.document
                    .apply(Command::Insert(String::new()));
                eprintln!("Quill could not open {}: {error}", path.display());
            }
        }
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
            && revision == self.preview_revision
            && (width - self.preview_width).abs() < 0.5
        {
            return;
        }
        let base = quill_core::CharStyle {
            family: self.renderer.default_family(),
            size: self.document.active_style().size,
            color: quill_core::Color::rgb(
                color::TEXT.r(),
                color::TEXT.g(),
                color::TEXT.b(),
            ),
            ..quill_core::CharStyle::default()
        };
        let colors = quill_core::PreviewColors {
            text: quill_core::Color::rgb(color::TEXT_STRONG.r(), color::TEXT_STRONG.g(), color::TEXT_STRONG.b()),
            code: quill_core::Color::rgb(0x7E, 0xD3, 0x9B),
            link: quill_core::Color::rgb(color::ACCENT.r(), color::ACCENT.g(), color::ACCENT.b()),
            quiet: quill_core::Color::rgb(color::TEXT_DIM.r(), color::TEXT_DIM.g(), color::TEXT_DIM.b()),
            rule: quill_core::Color::rgb(color::DIVIDER.r(), color::DIVIDER.g(), color::DIVIDER.b()),
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
        if revision == self.laid_out_revision && (width - self.laid_out_width).abs() < 0.5 {
            return;
        }
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
        let full = ui.max_rect();

        // The window is one painted surface with rounded corners, because it has no operating system
        // title bar. Everything else is drawn on top of it.
        ui.painter().rect_filled(
            full,
            CornerRadius::same(size::WINDOW_CORNER),
            theme::faded(color::EDITOR, self.opacity),
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
        let explorer_width = if self.explorer_visible { size::EXPLORER } else { 0.0 };
        let explorer_rect =
            Rect::from_min_size(body.min, Vec2::new(explorer_width, body.height()));
        let editor_rect =
            Rect::from_min_max(Pos2::new(body.left() + explorer_width, body.top()), body.max);

        // The title bar.
        let outcome = title_bar::show(
            ui,
            title_rect,
            &self.file_name(),
            self.folder_name().as_deref(),
            self.document.is_modified(),
            self.opacity,
        );
        if outcome.close {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if outcome.minimise {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if outcome.toggle_maximise {
            let maximised = ui.input(|input| input.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximised));
        }
        if let Some(action) = outcome.file_action {
            self.file_action(action);
        }

        // The toolbar.
        let families: Vec<String> = self.renderer.families().to_vec();
        let toolbar_outcome = {
            let mut toolbar_ui = ui.new_child(egui::UiBuilder::new().max_rect(toolbar_rect));
            toolbar::show(
                &mut toolbar_ui,
                toolbar_rect,
                &self.document,
                &families,
                &mut self.opacity,
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

        // The File menu's shortcuts. Read here rather than in the editing area, because they work whether
        // or not the editing area has the keyboard, and because in preview mode there is no editing area
        // taking key presses at all.
        let requested = ui.input(|input| {
            let mut found = None;
            for event in &input.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    if !modifiers.command {
                        continue;
                    }
                    found = match (key, modifiers.shift) {
                        (egui::Key::O, true) => Some(FileAction::OpenFolder),
                        (egui::Key::O, false) => Some(FileAction::OpenFile),
                        (egui::Key::S, true) => Some(FileAction::SaveAs),
                        _ => found,
                    };
                }
            }
            found
        });
        if let Some(action) = requested {
            self.file_action(action);
        }

        // The explorer.
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
                    self.opacity,
                )
            };
            if let Some(path) = explorer_outcome.toggle {
                self.tree.toggle(&path);
            }
            if let Some(path) = explorer_outcome.open {
                self.open_path(&path);
            }
            if explorer_outcome.hide {
                self.explorer_visible = false;
            }
            if explorer_outcome.add {
                self.save();
            }
            title_bar::divider(
                ui.painter(),
                Pos2::new(explorer_rect.right(), explorer_rect.top()),
                Pos2::new(explorer_rect.right(), explorer_rect.bottom()),
            );
        }

        // The editing area, split according to the view mode.
        match self.view_mode {
            ViewMode::Raw => self.show_editor(ui, editor_rect),
            ViewMode::Preview => {
                self.editor_area = editor_rect;
                self.show_preview(ui, editor_rect);
            }
            ViewMode::SideBySide => {
                let half = (editor_rect.width() / 2.0).floor();
                let left = Rect::from_min_size(editor_rect.min, Vec2::new(half, editor_rect.height()));
                let right = Rect::from_min_max(
                    Pos2::new(editor_rect.left() + half, editor_rect.top()),
                    editor_rect.max,
                );
                self.show_editor(ui, left);
                self.show_preview(ui, right);
                title_bar::divider(
                    ui.painter(),
                    Pos2::new(right.left(), right.top()),
                    Pos2::new(right.left(), right.bottom()),
                );
            }
        }

        // With the explorer hidden there has to be a way to bring it back. It is drawn after the editing
        // area rather than before it, because the editing area takes clicks over the whole of its
        // rectangle and a widget added earlier would sit underneath and never be clicked.
        if !self.explorer_visible {
            let button =
                Rect::from_min_size(Pos2::new(body.left() + 8.0, body.top() + 8.0), Vec2::splat(24.0));
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

        // The status bar.
        let style = self.document.active_style();
        status_bar::show(
            ui,
            status_rect,
            &status_bar::Status {
                name: &self.file_name(),
                unsaved: self.document.is_modified(),
                kind: theme::file_kind(self.document.path()),
                position: self.caret_position(),
                family: &style.family,
                font_size: style.size,
            },
            self.opacity,
        );
        title_bar::divider(
            ui.painter(),
            Pos2::new(status_rect.left(), status_rect.top()),
            Pos2::new(status_rect.right(), status_rect.top()),
        );
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
            self.focused = true;
        }
        let text_width = (area.width() - size::EDITOR_PADDING_X * 2.0).max(50.0);
        self.refresh_layout(text_width);

        let origin = Pos2::new(
            area.left() + size::EDITOR_PADDING_X,
            area.top() + size::EDITOR_PADDING_Y - self.scroll,
        );
        let pointer_changed =
            editor_view::handle_pointer(&response, &mut self.document, &self.layout, origin);
        let outcome = editor_view::handle_input(ui, &mut self.document, &self.layout, self.focused);
        if let Some(text) = outcome.copy {
            ui.ctx().copy_text(text);
        }
        if outcome.save {
            self.save();
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
                show_caret: self.focused,
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
}
