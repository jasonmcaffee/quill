//! The file explorer down the left of the window.
//!
//! The tree itself lives in `file_tree.rs`; this draws it. The design gives it a heading with an add
//! button and a button that hides the panel, a box that filters the file list, a small coloured square in
//! front of each file saying what kind it is, the open file shown as a filled pill with an amber dot when
//! it has unsaved changes, and a footer counting the files.

use std::path::PathBuf;

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::services::file_kind::Refusal;
use crate::services::file_tree::FileTree;
use crate::theme::{color, icon, size, file_marker};

/// What the user did in the explorer.
#[derive(Debug, Default)]
pub struct ExplorerOutcome {
    /// A folder to open or close.
    pub toggle: Option<PathBuf>,
    /// A file to load into the editor.
    pub open: Option<PathBuf>,
    /// The button that hides the panel was pressed.
    pub hide: bool,
    /// The add button was pressed.
    pub add: bool,
}

/// Draw the explorer into `area`.
///
/// `filter` is the text in the filter box, `current` is the file that is open and `unsaved` says whether it
/// has changes that have not been written.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    tree: &FileTree,
    filter: &mut String,
    current: Option<&std::path::Path>,
    unsaved: bool,
    opacity: f32,
) -> ExplorerOutcome {
    let mut outcome = ExplorerOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::EXPLORER, opacity));

    // The heading: the folder's name in small letter spaced capitals, then the two small buttons.
    let heading_y = area.top() + 22.0;
    let name = tree
        .root()
        .file_name()
        .map(|name| name.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| tree.root().display().to_string());
    // Letters are spaced out by hand, because egui has no letter spacing setting.
    let spaced: String = name.chars().flat_map(|c| [c, ' ']).collect();
    let font = egui::FontId::proportional(10.5);
    // The heading has to stop before the two buttons on the right. A long folder name is cut short with an
    // ellipsis rather than run underneath them.
    let available = area.width() - 16.0 - 66.0;
    let mut heading = spaced.trim_end().to_owned();
    let mut galley = painter.layout_no_wrap(heading.clone(), font.clone(), color::TEXT_DIM);
    while galley.size().x > available && heading.chars().count() > 1 {
        // Two characters at a time, because each letter of the name was followed by a space.
        heading.pop();
        heading.pop();
        galley = painter.layout_no_wrap(
            format!("{}\u{2026}", heading.trim_end()),
            font.clone(),
            color::TEXT_DIM,
        );
    }
    painter.galley(
        Pos2::new(area.left() + 16.0, heading_y - galley.size().y / 2.0),
        galley,
        color::TEXT_DIM,
    );

    for (index, (name, draw)) in [
        ("New file", icon::plus as fn(&egui::Painter, Pos2, egui::Color32)),
        ("Hide the explorer", icon::collapse as fn(&egui::Painter, Pos2, egui::Color32)),
    ]
    .into_iter()
    .enumerate()
    {
        let centre = Pos2::new(area.right() - 46.0 + index as f32 * 28.0, heading_y);
        let hit = Rect::from_center_size(centre, Vec2::splat(22.0));
        let response =
            ui.interact(hit, ui.id().with(("explorer-button", name)), Sense::click()).on_hover_text(name);
        if response.hovered() {
            painter.rect_filled(hit, CornerRadius::same(4), color::CONTROL);
        }
        draw(&painter, centre, color::TEXT_DIM);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
        });
        if response.clicked() {
            match index {
                0 => outcome.add = true,
                _ => outcome.hide = true,
            }
        }
    }

    // The filter box.
    let filter_rect = Rect::from_min_size(
        Pos2::new(area.left() + 12.0, area.top() + 36.0),
        Vec2::new(area.width() - 24.0, 24.0),
    );
    painter.rect(
        filter_rect,
        CornerRadius::same(size::CONTROL_CORNER),
        color::FIELD,
        Stroke::new(1.0, color::DIVIDER),
        egui::StrokeKind::Inside,
    );
    icon::magnifier(&painter, Pos2::new(filter_rect.left() + 13.0, filter_rect.center().y), color::TEXT_FAINT);
    let text_rect = Rect::from_min_max(
        Pos2::new(filter_rect.left() + 26.0, filter_rect.top()),
        filter_rect.right_bottom(),
    );
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = field.add(
        egui::TextEdit::singleline(filter)
            .hint_text(egui::RichText::new("Filter files").color(color::TEXT_FAINT))
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .text_color(color::TEXT_CONTROL),
    );
    // Named, because a test finds a control by its name and egui names a text box after what is typed in it.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Filter files")
    });

    // The rows: either the tree, or a flat list of what matches the filter.
    let list_top = filter_rect.bottom() + 12.0;
    let footer_top = area.bottom() - size::EXPLORER_FOOTER;
    let list_rect = Rect::from_min_max(Pos2::new(area.left(), list_top), Pos2::new(area.right(), footer_top));
    let filtering = !filter.trim().is_empty();

    let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
    list.set_clip_rect(list_rect);
    egui::ScrollArea::vertical().id_salt("explorer-rows").show(&mut list, |ui| {
        if filtering {
            let matches = tree.matching(filter);
            if matches.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("  No file matches").size(11.5).color(color::TEXT_FAINT),
                );
            }
            for path in matches {
                let depth = tree.depth_of(path);
                let refusal = crate::services::file_kind::openable(path).err();
                if let Some(clicked) = file_row(ui, path, depth, current, unsaved, refusal) {
                    outcome.open = Some(clicked);
                }
            }
        } else {
            for row in tree.rows() {
                if row.entry.is_directory {
                    if folder_row(ui, &row.entry.name, row.depth, row.entry.expanded) {
                        outcome.toggle = Some(row.entry.path.clone());
                    }
                } else if let Some(clicked) =
                    file_row(ui, &row.entry.path, row.depth, current, unsaved, row.entry.refusal)
                {
                    outcome.open = Some(clicked);
                }
            }
        }
        if let Some(error) = &tree.last_error {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(error).size(11.0).color(color::CLOSE));
        }
    });

    // The footer, counting the files and how many are unsaved.
    let footer = Rect::from_min_max(Pos2::new(area.left(), footer_top), area.right_bottom());
    painter.rect_filled(footer, CornerRadius::ZERO, crate::theme::faded(color::EXPLORER_FOOTER, opacity));
    painter.line_segment(
        [Pos2::new(footer.left(), footer.top()), Pos2::new(footer.right(), footer.top())],
        Stroke::new(1.0, color::DIVIDER),
    );
    let count = tree.file_count();
    let files = if count == 1 { "1 file".to_owned() } else { format!("{count} files") };
    let openable = tree.openable_count();
    let mut text = files;
    if openable < count {
        text = format!("{text}  \u{00B7}  {openable} can be opened");
    }
    if unsaved {
        text = format!("{text}  \u{00B7}  1 unsaved");
    }
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(10.5), color::TEXT_DIM);
    painter.galley(
        Pos2::new(footer.left() + 16.0, footer.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_DIM,
    );

    outcome
}

/// A folder row: a triangle that points down when open, then the name.
fn folder_row(ui: &mut egui::Ui, name: &str, depth: usize, expanded: bool) -> bool {
    let row = allocate_row(ui);
    let response = ui.interact(row, ui.id().with(("folder", name, depth)), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(row.shrink2(Vec2::new(8.0, 1.0)), CornerRadius::same(5), color::CONTROL);
    }
    let x = row.left() + 16.0 + depth as f32 * size::INDENT;
    icon::disclosure(ui.painter(), Pos2::new(x, row.center().y), expanded, color::TEXT_DIM);
    let galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(12.5),
        color::TEXT_CONTROL,
    );
    ui.painter().galley(
        Pos2::new(x + 12.0, row.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
    // The accessible name is the folder's name, so a test can ask for it by name.
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), expanded, name)
    });
    response.clicked()
}

/// A file row: a small coloured square saying what kind of file it is, then the name. The file that is open
/// is drawn as a filled pill, with an amber dot on the right when it has unsaved changes.
fn file_row(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    depth: usize,
    current: Option<&std::path::Path>,
    unsaved: bool,
    refusal: Option<Refusal>,
) -> Option<PathBuf> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let row = allocate_row(ui);
    // A file Quill cannot open is drawn dimmed and does not take clicks, so the tree says what is in the
    // folder without pretending everything in it can be opened.
    let openable = refusal.is_none();
    let sense = if openable { Sense::click() } else { Sense::hover() };
    let response = ui.interact(row, ui.id().with(("file", path)), sense);
    if let Some(refusal) = refusal {
        // The row says which of the two reasons it is: the file is not text, or it is too large.
        response.clone().on_hover_text(refusal.reason());
    }
    let open = current == Some(path);
    let pill = row.shrink2(Vec2::new(8.0, 1.0));
    if open {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::SELECTED_ROW);
    } else if response.hovered() && openable {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::CONTROL);
    }
    let x = row.left() + 16.0 + depth as f32 * size::INDENT;
    let marker = if openable { file_marker(path) } else { color::TEXT_FAINT.gamma_multiply(0.45) };
    ui.painter().rect_filled(
        Rect::from_center_size(Pos2::new(x + 4.0, row.center().y), Vec2::splat(8.0)),
        CornerRadius::same(2),
        marker,
    );
    let tint = if open {
        color::TEXT_STRONG
    } else if openable {
        color::TEXT_CONTROL
    } else {
        color::TEXT_FAINT.gamma_multiply(0.7)
    };
    let galley =
        ui.painter().layout_no_wrap(name.clone(), egui::FontId::proportional(12.5), tint);
    ui.painter().galley(
        Pos2::new(x + 16.0, row.center().y - galley.size().y / 2.0),
        galley,
        tint,
    );
    if open && unsaved {
        ui.painter().circle_filled(Pos2::new(pill.right() - 12.0, row.center().y), 3.5, color::UNSAVED);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), open, &name)
    });
    response.clicked().then(|| path.to_path_buf())
}

fn allocate_row(ui: &mut egui::Ui) -> Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, size::ROW), Sense::hover());
    rect
}
