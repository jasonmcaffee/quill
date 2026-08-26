//! The Settings window, opened from `Edit -> Settings`.
//!
//! It is laid out the way `tasks/img.png` shows IntelliJ's: a search box and a list of pages down the
//! left grouped under headings, a breadcrumb across the top of the right hand side saying where you are,
//! and the chosen page's sections under it. It is a modal, so the rest of the window is dimmed and does
//! not take clicks while it is open, which is what `tasks/improvements.md` asks for.
//!
//! Every change takes effect as it is made rather than when the window is closed, so there is one button
//! and it says `Close`. A dialog with `Apply` has to hold a second copy of every setting and decide what
//! to do when the two disagree; showing the change straight away needs neither.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::components::controls;
use crate::components::modal;
use crate::components::plugins_page::{self, PluginsOutcome, PluginsState};
use crate::services::plugins::Plugins;
use crate::settings::{Page, Settings, FONT_SIZES, MIN_OPACITY, TERMINAL_FONT_SIZES};
use crate::theme::{color, icon, size};

/// How large the window is, before it is shrunk to fit a small Quill window.
const WIDTH: f32 = 900.0;
const HEIGHT: f32 = 560.0;
/// How wide the list of pages is.
const LIST_WIDTH: f32 = 258.0;
const HEADER: f32 = 46.0;
const FOOTER: f32 = 52.0;

/// Which page is showing, and what has been typed in the search box. Lives in the window's state, so it
/// is still there when the settings are opened again.
#[derive(Debug, Clone, Default)]
pub struct SettingsWindow {
    pub open: bool,
    pub page: Page,
    pub search: String,
    /// What the Plugins page is showing.
    pub plugins: PluginsState,
}

impl SettingsWindow {
    pub fn open(&mut self) {
        self.open = true;
    }
}

/// What happened in the Settings window this frame.
#[derive(Debug, Default, PartialEq)]
pub struct SettingsOutcome {
    /// A setting was changed, so the window applies it and writes the settings file.
    pub changed: bool,
    /// The window was closed.
    pub closed: bool,
    /// What the Plugins page asked for.
    pub plugins: PluginsOutcome,
}

/// Draw the Settings window. Does nothing when it is not open.
pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsWindow,
    settings: &mut Settings,
    families: &[String],
    project: &str,
    plugins: &Plugins,
    installed_on_disk: &dyn Fn(&str) -> bool,
    icon_for: &dyn Fn(&str) -> Option<egui::TextureHandle>,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::default();
    if !state.open {
        return outcome;
    }

    // The window is drawn into one rectangle, the way every other part of Quill is, rather than
    // through egui's own layout, so the columns line up with the design. `components::modal` is what
    // decides how large that rectangle is and where it sits, which is also what makes the Settings
    // window draggable and resizable along with every other modal.
    let (inner, should_close) = modal::show(ctx, "quill-settings", WIDTH, HEIGHT, |ui, area| {
        contents(ui, area, state, settings, families, project, plugins, installed_on_disk, icon_for)
    });

    outcome.changed = inner.changed;
    let inner_closed = inner.closed;
    outcome.plugins = inner.plugins;
    if inner_closed || should_close {
        state.open = false;
        outcome.closed = true;
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
fn contents(
    ui: &mut egui::Ui,
    area: Rect,
    state: &mut SettingsWindow,
    settings: &mut Settings,
    families: &[String],
    project: &str,
    plugins: &Plugins,
    installed_on_disk: &dyn Fn(&str) -> bool,
    icon_for: &dyn Fn(&str) -> Option<egui::TextureHandle>,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::default();

    // The heading, which names the project the way IntelliJ's does.
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), HEADER));
    let painter = ui.painter_at(area);
    painter.rect_filled(
        header,
        CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 },
        color::TITLE_BAR,
    );
    let title = if project.is_empty() {
        "Settings".to_owned()
    } else {
        format!("Settings \u{2014} {project}")
    };
    let galley =
        painter.layout_no_wrap(title, egui::FontId::proportional(13.0), color::TEXT_STRONG);
    painter.galley(
        Pos2::new(area.left() + 20.0, header.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_STRONG,
    );
    let close = Rect::from_center_size(
        Pos2::new(area.right() - 24.0, header.center().y),
        Vec2::splat(22.0),
    );
    if controls::icon_button(ui, close, "Close settings", icon::cross) {
        outcome.closed = true;
    }
    line(ui, Pos2::new(header.left(), header.bottom()), Pos2::new(header.right(), header.bottom()));

    let body = Rect::from_min_max(
        Pos2::new(area.left(), header.bottom()),
        Pos2::new(area.right(), area.bottom() - FOOTER),
    );
    let list = Rect::from_min_size(body.min, Vec2::new(LIST_WIDTH, body.height()));
    let page_area = Rect::from_min_max(Pos2::new(list.right(), body.top()), body.max);
    ui.painter_at(area).rect_filled(list, CornerRadius::ZERO, color::EXPLORER_FOOTER);
    line(ui, Pos2::new(list.right(), list.top()), Pos2::new(list.right(), list.bottom()));

    show_list(ui, list, state);
    match state.page {
        Page::Appearance => {
            outcome.changed |= appearance_page(ui, page_area, settings, families);
        }
        Page::Editor => {
            outcome.changed |= editor_page(ui, page_area, settings);
        }
        Page::Plugins => {
            outcome.plugins = plugins_page::show(
                ui,
                page_area,
                &mut state.plugins,
                plugins,
                installed_on_disk,
                icon_for,
            );
        }
        Page::Terminal => {
            outcome.changed |= terminal_page(ui, page_area, settings);
        }
    }

    // The footer, holding the one button.
    let footer = Rect::from_min_max(Pos2::new(area.left(), body.bottom()), area.max);
    line(ui, Pos2::new(footer.left(), footer.top()), Pos2::new(footer.right(), footer.top()));
    let button = Rect::from_min_size(
        Pos2::new(footer.right() - 20.0 - 96.0, footer.center().y - 14.0),
        Vec2::new(96.0, 28.0),
    );
    // Named `Done` rather than `Close`, because the window's own close button is called Close and two
    // controls with one name cannot be told apart, by a person reading them out or by a test.
    if wide_button(ui, button, "Done") {
        outcome.closed = true;
    }
    let note = ui.painter_at(area).layout_no_wrap(
        "Changes take effect at once.".to_owned(),
        egui::FontId::proportional(11.0),
        color::TEXT_FAINT,
    );
    ui.painter_at(area).galley(
        Pos2::new(footer.left() + 20.0, footer.center().y - note.size().y / 2.0),
        note,
        color::TEXT_FAINT,
    );

    outcome
}

/// The search box and the list of pages, grouped under their headings.
fn show_list(ui: &mut egui::Ui, area: Rect, state: &mut SettingsWindow) {
    let search = Rect::from_min_size(
        Pos2::new(area.left() + 12.0, area.top() + 12.0),
        Vec2::new(area.width() - 24.0, 26.0),
    );
    let painter = ui.painter_at(area);
    painter.rect(
        search,
        CornerRadius::same(size::CONTROL_CORNER),
        color::FIELD,
        Stroke::new(1.0, color::DIVIDER),
        egui::StrokeKind::Inside,
    );
    icon::magnifier(&painter, Pos2::new(search.left() + 13.0, search.center().y), color::TEXT_FAINT);
    let text_rect = crate::components::controls::field_text_rect(ui, search, 26.0);
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    field.add(
        egui::TextEdit::singleline(&mut state.search)
            .hint_text(egui::RichText::new("Search settings").color(color::TEXT_FAINT))
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .text_color(color::TEXT_CONTROL),
    );

    let mut pen = search.bottom() + 14.0;
    let mut group_drawn: Option<&str> = None;
    let mut any = false;
    for page in Page::ALL {
        if !page.matches(&state.search) {
            continue;
        }
        any = true;
        if !page.group().is_empty() && group_drawn != Some(page.group()) {
            group_drawn = Some(page.group());
            let row = Rect::from_min_size(
                Pos2::new(area.left(), pen),
                Vec2::new(area.width(), size::ROW),
            );
            icon::disclosure(
                &ui.painter_at(area),
                Pos2::new(row.left() + 18.0, row.center().y),
                true,
                color::TEXT_DIM,
            );
            let galley = ui.painter_at(area).layout_no_wrap(
                page.group().to_owned(),
                egui::FontId::proportional(12.5),
                color::TEXT_CONTROL,
            );
            ui.painter_at(area).galley(
                Pos2::new(row.left() + 30.0, row.center().y - galley.size().y / 2.0),
                galley,
                color::TEXT_CONTROL,
            );
            pen += size::ROW;
        }
        let row =
            Rect::from_min_size(Pos2::new(area.left(), pen), Vec2::new(area.width(), size::ROW));
        // A page with no group of its own is not indented under one.
        let indent = if page.group().is_empty() { 16.0 } else { 46.0 };
        if page_row(ui, row, page, state.page == page, indent) {
            state.page = page;
        }
        pen += size::ROW;
    }
    if !any {
        let galley = ui.painter_at(area).layout_no_wrap(
            "No setting matches".to_owned(),
            egui::FontId::proportional(11.5),
            color::TEXT_FAINT,
        );
        ui.painter_at(area).galley(Pos2::new(area.left() + 30.0, pen + 4.0), galley, color::TEXT_FAINT);
    }
}

/// One page in the list. The chosen one is drawn as a filled row, the way the open file is in the
/// explorer, so the two lists in the application look like each other.
fn page_row(ui: &mut egui::Ui, row: Rect, page: Page, chosen: bool, indent: f32) -> bool {
    let response = ui.interact(row, ui.id().with(("settings-page", page.title())), Sense::click());
    let pill = row.shrink2(Vec2::new(8.0, 1.0));
    if chosen {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::SELECTED_ROW);
    } else if response.hovered() {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::CONTROL);
    }
    let tint = if chosen { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    let galley = ui.painter().layout_no_wrap(
        page.title().to_owned(),
        egui::FontId::proportional(12.5),
        tint,
    );
    ui.painter().galley(
        Pos2::new(row.left() + indent, row.center().y - galley.size().y / 2.0),
        galley,
        tint,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, page.title())
    });
    response.clicked()
}

/// `Appearance & Behavior > Appearance`: the editor's font and the window's background.
fn appearance_page(
    ui: &mut egui::Ui,
    area: Rect,
    settings: &mut Settings,
    families: &[String],
) -> bool {
    let mut changed = false;
    let mut pen = breadcrumb(ui, area, Page::Appearance);

    pen = section(ui, area, pen, "Font");
    let font_row = row_at(area, pen);
    label(ui, area, font_row, "Family:");
    let family = if settings.font_family.is_empty() {
        "System default".to_owned()
    } else {
        settings.font_family.clone()
    };
    if let Some(chosen) = controls::dropdown(
        ui,
        Rect::from_min_size(Pos2::new(area.left() + 130.0, font_row.top()), Vec2::new(240.0, 28.0)),
        &family,
        "Editor font family",
        None,
        |ui| {
            let mut chosen = None;
            for family in families {
                if ui.selectable_label(*family == settings.font_family, family).clicked() {
                    chosen = Some(family.clone());
                }
            }
            chosen
        },
    ) {
        settings.font_family = chosen;
        changed = true;
    }
    pen += 38.0;

    let size_row = row_at(area, pen);
    label(ui, area, size_row, "Size:");
    if let Some(chosen) = controls::dropdown(
        ui,
        Rect::from_min_size(Pos2::new(area.left() + 130.0, size_row.top()), Vec2::new(96.0, 28.0)),
        &format!("{:.0}", settings.font_size),
        "Editor font size",
        None,
        |ui| {
            let mut chosen = None;
            for option in FONT_SIZES {
                let selected = (settings.font_size - option).abs() < 0.01;
                if ui.selectable_label(selected, format!("{option:.0}")).clicked() {
                    chosen = Some(*option);
                }
            }
            chosen
        },
    ) {
        settings.font_size = chosen;
        changed = true;
    }
    pen += 34.0;
    pen = note(
        ui,
        area,
        pen,
        "The font the editor sets every open file in. Bold, italic and colour stay as they were,",
    );
    pen = note(
        ui,
        area,
        pen,
        "and the size is also on the keyboard at command or control with plus and minus.",
    );

    pen = section(ui, area, pen + 10.0, "Background");
    let opacity_row = row_at(area, pen);
    label(ui, area, opacity_row, "Opacity:");
    let slider_rect = Rect::from_min_size(
        Pos2::new(area.left() + 130.0, opacity_row.top() + 2.0),
        Vec2::new(300.0, 24.0),
    );
    let mut slider_ui = ui.new_child(egui::UiBuilder::new().max_rect(slider_rect));
    slider_ui.spacing_mut().slider_width = 220.0;
    let percent = format!("{:.0}%", settings.opacity * 100.0);
    let response = slider_ui.add(
        egui::Slider::new(&mut settings.opacity, MIN_OPACITY..=1.0)
            .show_value(false)
            .text(percent),
    );
    // The slider's own accessible name is the number, so it is named here for a test to find it.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Background opacity")
    });
    changed |= response.changed();
    pen += 34.0;
    note(
        ui,
        area,
        pen,
        "Fades the window so the desktop shows through. Text stays fully solid at every setting.",
    );
    changed
}

/// `Editor > Editor`: what the gutter down the left of the editing area shows.
fn editor_page(ui: &mut egui::Ui, area: Rect, settings: &mut Settings) -> bool {
    let mut changed = false;
    let mut pen = breadcrumb(ui, area, Page::Editor);
    pen = section(ui, area, pen, "Gutter");
    let row = row_at(area, pen);
    changed |= checkbox(ui, row, "Show line numbers", &mut settings.line_numbers);
    pen += 32.0;
    note(
        ui,
        area,
        pen,
        "A number against each line of the file. Quill wraps, so a paragraph that runs over several rows is numbered once, against its first row. Right clicking the gutter puts the numbers away and annotates with git blame.",
    );
    changed
}

/// A tick box with its label to the right of it, drawn the way every other control here is.
fn checkbox(ui: &mut egui::Ui, row: Rect, name: &str, value: &mut bool) -> bool {
    let box_rect = Rect::from_min_size(Pos2::new(row.left(), row.center().y - 8.0), Vec2::splat(16.0));
    let response = ui.interact(row, ui.id().with(("settings-check", name)), Sense::click());
    let painter = ui.painter();
    painter.rect(
        box_rect,
        CornerRadius::same(3),
        if *value { color::ACCENT } else { color::FIELD },
        Stroke::new(1.0, if *value { color::ACCENT } else { color::CONTROL_BORDER }),
        egui::StrokeKind::Inside,
    );
    if *value {
        icon::tick(painter, box_rect.center(), color::TEXT_STRONG);
    }
    let galley =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), color::TEXT_CONTROL);
    painter.galley(
        Pos2::new(box_rect.right() + 10.0, row.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, name)
    });
    if response.clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// `Tools > Terminal`.
fn terminal_page(ui: &mut egui::Ui, area: Rect, settings: &mut Settings) -> bool {
    let mut changed = false;
    let mut pen = breadcrumb(ui, area, Page::Terminal);
    pen = section(ui, area, pen, "Font");
    let size_row = row_at(area, pen);
    label(ui, area, size_row, "Size:");
    if let Some(chosen) = controls::dropdown(
        ui,
        Rect::from_min_size(Pos2::new(area.left() + 130.0, size_row.top()), Vec2::new(96.0, 28.0)),
        &format!("{:.0}", settings.terminal_font_size),
        "Terminal font size",
        None,
        |ui| {
            let mut chosen = None;
            for option in TERMINAL_FONT_SIZES {
                let selected = (settings.terminal_font_size - option).abs() < 0.01;
                if ui.selectable_label(selected, format!("{option:.0}")).clicked() {
                    chosen = Some(*option);
                }
            }
            chosen
        },
    ) {
        settings.terminal_font_size = chosen;
        changed = true;
    }
    pen += 34.0;
    pen = note(
        ui,
        area,
        pen,
        "The size of one cell in the terminal grid. Changing it tells the running program the new size.",
    );

    pen = section(ui, area, pen + 12.0, "Shell");
    let shell_row = row_at(area, pen);
    label(ui, area, shell_row, "Program:");
    let before = settings.terminal_shell.clone();
    modal::field(
        ui,
        Rect::from_min_size(
            Pos2::new(area.left() + 130.0, shell_row.top()),
            Vec2::new(240.0, 28.0),
        ),
        "Terminal shell",
        &mut settings.terminal_shell,
    );
    // Compared rather than taken from the field's own `changed`, because a field reports a change on
    // every letter and this is written to disk: what matters is that the setting is not what it was.
    if settings.terminal_shell != before {
        changed = true;
    }
    pen += 34.0;
    pen = note(
        ui,
        area,
        pen,
        "The program each tab runs, started in the folder the explorer is showing.",
    );
    // A note is one line and is not wrapped, so what an empty field means is a note of its own rather
    // than a longer sentence that would run off the end of the page.
    note(ui, area, pen + 8.0, &format!("Leave it empty for {}.", default_shell_name()));
    changed
}

/// What an empty shell setting means on this machine, in words, so the note under the field says the
/// name a person would type rather than `$SHELL`.
fn default_shell_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "PowerShell"
    } else {
        "the shell named in $SHELL"
    }
}

/// `Appearance & Behavior  >  Appearance` across the top of the page, and the line under it.
fn breadcrumb(ui: &mut egui::Ui, area: Rect, page: Page) -> f32 {
    let painter = ui.painter_at(area);
    let y = area.top() + 26.0;
    if page.group().is_empty() {
        let title = painter.layout_no_wrap(
            page.title().to_owned(),
            egui::FontId::proportional(13.5),
            color::TEXT_STRONG,
        );
        painter.galley(Pos2::new(area.left() + 24.0, y - title.size().y / 2.0), title, color::TEXT_STRONG);
        return y + 22.0;
    }
    let group = painter.layout_no_wrap(
        page.group().to_owned(),
        egui::FontId::proportional(13.5),
        color::TEXT_DIM,
    );
    let mut pen = area.left() + 24.0;
    painter.galley(Pos2::new(pen, y - group.size().y / 2.0), group.clone(), color::TEXT_DIM);
    pen += group.size().x + 8.0;
    let arrow = painter.layout_no_wrap(
        "\u{203A}".to_owned(),
        egui::FontId::proportional(13.5),
        color::TEXT_FAINT,
    );
    painter.galley(Pos2::new(pen, y - arrow.size().y / 2.0), arrow.clone(), color::TEXT_FAINT);
    pen += arrow.size().x + 8.0;
    let title = painter.layout_no_wrap(
        page.title().to_owned(),
        egui::FontId::proportional(13.5),
        color::TEXT_STRONG,
    );
    painter.galley(Pos2::new(pen, y - title.size().y / 2.0), title, color::TEXT_STRONG);
    y + 22.0
}

/// A heading inside a page, with a rule running to the right edge, as IntelliJ draws one.
fn section(ui: &mut egui::Ui, area: Rect, top: f32, name: &str) -> f32 {
    let painter = ui.painter_at(area);
    let galley =
        painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(12.5), color::TEXT_STRONG);
    let y = top + 12.0;
    painter.galley(Pos2::new(area.left() + 24.0, y - galley.size().y / 2.0), galley.clone(), color::TEXT_STRONG);
    let from = area.left() + 24.0 + galley.size().x + 12.0;
    painter.line_segment(
        [Pos2::new(from, y), Pos2::new(area.right() - 24.0, y)],
        Stroke::new(1.0, color::DIVIDER),
    );
    y + 20.0
}

fn row_at(area: Rect, top: f32) -> Rect {
    Rect::from_min_size(Pos2::new(area.left() + 24.0, top), Vec2::new(area.width() - 48.0, 28.0))
}

fn label(ui: &mut egui::Ui, area: Rect, row: Rect, text: &str) {
    let painter = ui.painter_at(area);
    let galley =
        painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(12.5), color::TEXT_CONTROL);
    painter.galley(
        Pos2::new(row.left(), row.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
}

/// A line of explanation under a control, in the faintest colour.
fn note(ui: &mut egui::Ui, area: Rect, top: f32, text: &str) -> f32 {
    let painter = ui.painter_at(area);
    let galley = painter.layout(
        text.to_owned(),
        egui::FontId::proportional(11.5),
        color::TEXT_FAINT,
        area.width() - 48.0,
    );
    painter.galley(Pos2::new(area.left() + 24.0, top), galley.clone(), color::TEXT_FAINT);
    top + galley.size().y + 8.0
}

/// A button with a word in it, which the footer uses.
fn wide_button(ui: &mut egui::Ui, area: Rect, name: &str) -> bool {
    let response = ui.interact(area, ui.id().with(("settings-button", name)), Sense::click());
    let fill = if response.hovered() { color::ACCENT } else { color::CONTROL };
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        fill,
        Stroke::new(1.0, color::CONTROL_BORDER),
        egui::StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_STRONG,
    );
    painter.galley(area.center() - galley.size() / 2.0, galley, color::TEXT_STRONG);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
    });
    response.clicked()
}

fn line(ui: &egui::Ui, from: Pos2, to: Pos2) {
    ui.painter().line_segment([from, to], Stroke::new(1.0, color::DIVIDER));
}
