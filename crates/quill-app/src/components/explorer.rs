//! The file explorer down the left of the window.
//!
//! The tree itself lives in `file_tree.rs`; this draws it. It has a heading naming the project, a
//! button that hides the panel, a box that filters the file list, a small coloured square in front of
//! each file saying what kind it is, the open file shown as a filled pill with an amber dot when it
//! has unsaved changes, and a footer counting the files.
//!
//! The heading is the project folder's row: it has no row of its own in the tree, so a right click
//! on the name opens the same menu a folder row does. That is `task-1673`, which also asked for the
//! plus button beside it to go — it meant `New file` and asked the window to save.
//!
//! ## Following the tab
//!
//! `task-1664` asks that the tree agree with the tabs: the file that is showing is selected here, and
//! the list is scrolled far enough for the row to be seen. The pill was already drawn; what the panel
//! does now is scroll to it, when the window says to by passing `reveal`.
//!
//! It scrolls with `scroll_to_rect(row, None)`, and the `None` is the point of it: that means *scroll
//! by the least amount that brings this rectangle into view*, so a row that is already visible does
//! not move and the tree does not jump every time you switch tabs. `Align::Center` would put the row
//! in the middle of the panel and throw away the reader's place in the tree for no reason. `Go to
//! File` and `Find in Files` scroll their own lists with the same call.
//!
//! Opening out the folders above the row is the window's half, because it is the tree that changes
//! rather than the drawing. See `QuillApp::follow_the_open_file`.
//!
//! ## The selection, and the ring round it
//!
//! `task-1681` gave the explorer a cursor of its own, because `Delete` cannot mean "throw this file
//! away" while the editing area has the keys. Two marks, not one: the file that is **showing** keeps
//! its filled pill, and the row the explorer's own cursor is on gains a one point ring — drawn only
//! while the explorer has the keyboard, so there is never a doubt about where a key press is going.
//! A row that is both is a pill with a ring, which is what clicking a file looks like.
//!
//! That is what the paragraph above has always said, and until `task-1693` it was not what the code
//! did: both marks filled the same `SELECTED_ROW` pill, so a right click on a second file left two
//! rows looking equally open — and the cursor's pill stayed after its tab was closed, because a
//! cursor is a different thing from an open file and is not supposed to move when a tab goes. **The
//! pill means the file that is showing and nothing else.** The cursor gets the quiet `CONTROL` fill
//! the hover already uses, plus the ring while the explorer has the keyboard, so the two marks are
//! two appearances rather than one.
//!
//! ## Dragging
//!
//! A row can be carried onto a folder, which is IntelliJ's Move refactoring under the gesture a
//! person reaches for. The component **reports and decides nothing**, which is the rule every
//! component here follows and is the shape `task-1673` gave the tab drag: it collects the rectangle
//! of every row it draws, works out which one the pointer is over once the list is drawn, and says
//! what was dropped where. `QuillApp::move_path` does the rest.
//!
//! Three drops are refused by simply not offering a target — a folder into itself or into anything
//! under it, a path into the folder it is already in, and anything outside the panel, the last so a
//! drag can be thought better of.
//!
//! ## The empty space below the rows
//!
//! It is the project folder, which is the answer the heading already gives: a right click there
//! opens the same menu a folder row opens, so a file or a folder can be made from anywhere in the
//! panel rather than only from a row that happens to be in the right place. `task-1693` asks for it,
//! and asks that the entries which are about a particular file be **greyed out** rather than taken
//! away — which the window does, through `actions::Aim`. The interaction is added after the rows, so
//! a row that is there always wins the point.

use std::path::{Path, PathBuf};

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::services::file_kind::Refusal;
use crate::services::file_tree::FileTree;
use crate::theme::{color, icon, size, file_marker};

/// What a row shows besides its name: the colour git wants it in, and the icon its plugin gives it.
///
/// A component draws and does not reach into the window's state, so it is handed one of these for
/// each row rather than being given the plugins and the repository to look in.
#[derive(Default, Clone)]
pub struct Decoration {
    /// The colour git wants the name in, when git has something to say about the file.
    pub tint: Option<egui::Color32>,
    /// The picture the file's plugin puts in front of it, in place of the coloured square.
    pub icon: Option<egui::TextureHandle>,
}

/// What the window tells the explorer about itself.
///
/// A struct rather than six more arguments, because the list had reached the length at which a
/// caller starts passing them in the wrong order.
#[derive(Debug, Clone, Copy, Default)]
pub struct View<'a> {
    /// The file showing in the pane with the keyboard, drawn as a filled pill.
    pub current: Option<&'a Path>,
    /// The row the explorer's own cursor is on.
    pub selected: Option<&'a Path>,
    /// Whether the explorer has the keyboard, which is what the ring says.
    pub keyboard: bool,
    /// Whether the file showing has changes that have not been written.
    pub unsaved: bool,
    /// True on the frames the list should scroll to `current`.
    pub reveal: bool,
    /// True on the frames the list should scroll to `selected`, which is what the arrow keys ask
    /// for: they move the explorer's own cursor without opening anything, so nothing else would.
    pub reveal_selected: bool,
    pub opacity: f32,
    /// How much bigger or smaller than its usual size the panel draws everything in it.
    ///
    /// `task-1771`: every pane is zoomable with `Ctrl`/`Cmd` and the wheel, and the explorer has no font
    /// size of its own to walk - its rows, its indents and its lettering are the style guide's numbers - so
    /// its zoom is a multiplier over all of them. One number reaches every measurement in this file through
    /// [`View::at`], which is what stops half of the panel scaling and half of it staying put.
    pub zoom: f32,
    /// Where to put the list this frame, in points from the top of the rows.
    ///
    /// `None` on nearly every frame. It is set on the frame after a zoom, so that the row the pointer was
    /// over is still under the pointer at the new size - the rule `task-1672` wrote for the editing area,
    /// applied to a list. The window works it out, because the window is what knows where the pointer was
    /// and what the zoom was before it changed; this component only obeys.
    pub scroll_to: Option<f32>,
}

impl View<'_> {
    /// `points` at this panel's zoom.
    fn at(&self, points: f32) -> f32 {
        points * self.zoom
    }
}

/// What the user did in the explorer.
#[derive(Debug, Default)]
pub struct ExplorerOutcome {
    /// A folder to open or close.
    pub toggle: Option<PathBuf>,
    /// A file to load into the editor, in the tab a single click reuses.
    pub open: Option<PathBuf>,
    /// A file that was double clicked, which opens it in a tab of its own.
    pub open_permanently: Option<PathBuf>,
    /// A row was right clicked: where the pointer was, and what it was over.
    pub context_menu: Option<(Pos2, PathBuf, bool)>,
    /// True when that right click was in the **empty space below the rows** rather than on a row.
    ///
    /// The path in `context_menu` is then the project folder, because that is what the empty space
    /// under a project's files is. The window uses this to dim the entries that are about a
    /// particular file — `task-1693`, and see `actions::Aim`.
    pub menu_over_empty_space: bool,
    /// A row was clicked, so it becomes the explorer's selection.
    pub select: Option<PathBuf>,
    /// Something was clicked in the panel, so the explorer should take the keyboard.
    pub focus: bool,
    /// A row was let go over a folder: what was carried, and the folder it landed in.
    pub moved: Option<(PathBuf, PathBuf)>,
    /// The button that hides the panel was pressed.
    pub hide: bool,
    /// True while a row is in the air, which is the one moment the window must not read the tree
    /// again — the entries under the drag would be rebuilt out from under it.
    pub dragging: bool,
    /// How far down the rows the list is, in points, after this frame.
    ///
    /// Read back so the window can work out where to put it after a zoom - see `View::scroll_to`.
    pub scroll: f32,
    /// The panel itself is being carried to another edge of the window, or its heading was right
    /// clicked — `task-1697`. The heading is the handle, which is what the ask calls "the top bar".
    pub grab: crate::components::dock::Grab,
}

/// One row as it was drawn, which is what the drop target is worked out from.
struct Drawn {
    rect: Rect,
    path: PathBuf,
    directory: bool,
}

/// Draw the explorer into `area`.
///
/// `filter` is the text in the filter box and `view` is everything the window knows that changes
/// how a row is drawn.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    tree: &FileTree,
    filter: &mut String,
    view: View,
    decorate: &dyn Fn(&std::path::Path) -> Decoration,
) -> ExplorerOutcome {
    let mut outcome = ExplorerOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::EXPLORER, view.opacity));

    // The heading strip is the handle this panel is carried to another edge by — `task-1697`, and
    // it is what the ask means by "the top bar". Added **first**, so the project's own row and the
    // hide button, which are added after it, take the points they cover: egui gives a pointer to the
    // last widget that asked for it. See `components::dock`.
    outcome.grab = crate::components::dock::handle(
        ui,
        Rect::from_min_max(area.min, Pos2::new(area.right(), area.top() + view.at(36.0))),
        crate::app::dock::Panel::Explorer,
    );

    // The heading: the folder's name in small letter spaced capitals, then the button that hides the panel.
    let heading_y = area.top() + view.at(22.0);
    let name = tree
        .root()
        .file_name()
        .map(|name| name.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| tree.root().display().to_string());
    // Letters are spaced out by hand, because egui has no letter spacing setting.
    let spaced: String = name.chars().flat_map(|c| [c, ' ']).collect();
    let font = egui::FontId::proportional(view.at(10.5));
    // The heading has to stop before the button on the right. A long folder name is cut short with an
    // ellipsis rather than run underneath it.
    let available = area.width() - view.at(16.0) - view.at(46.0);
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
        Pos2::new(area.left() + view.at(16.0), heading_y - galley.size().y / 2.0),
        galley,
        color::TEXT_DIM,
    );

    // The project's name is a row like any other row in the tree, so it takes a right click and
    // opens the same menu a folder does — `task-1673` asks for that, and the project folder is the
    // one folder in the tree that has no row of its own to right click. It takes no left click:
    // there is nothing to open or close about the root, which is always shown. It is a drop target,
    // though, because moving something back to the top of the project has to be possible.
    let heading_hit = Rect::from_min_max(
        Pos2::new(area.left(), area.top() + view.at(8.0)),
        Pos2::new(area.right() - view.at(34.0), area.top() + view.at(36.0)),
    );
    let heading_response =
        ui.interact(heading_hit, ui.id().with("explorer-heading"), Sense::click());
    if heading_response.secondary_clicked() {
        if let Some(at) = heading_response.interact_pointer_pos().or_else(|| heading_response.hover_pos()) {
            outcome.context_menu = Some((at, tree.root().to_path_buf(), true));
            outcome.select = Some(tree.root().to_path_buf());
            outcome.focus = true;
        }
    }
    let project = tree
        .root()
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| tree.root().display().to_string());
    // **The project's own row answers the double click too.** `task-1771` asks for two presses "anywhere
    // in the top of a pane", and the heading is added *after* the drag handle - so egui gives it the
    // pointer over the words, and the handle is left with a sliver at the very top and the space beside the
    // button. Reported into the same `Grab` the handle fills, so the window still has one thing to read and
    // there is no second path for it to disagree with.
    if heading_response.double_clicked() {
        outcome.grab.twice = true;
    }
    heading_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &project)
    });

    // The one button: hiding the panel. There used to be a plus beside it that meant `New file`,
    // which never opened anything — it asked the window to save — and `task-1673` asks for it to go.
    // Making a file is on the right click menu, which the project's name now opens too.
    {
        let name = "Hide the explorer";
        let centre = Pos2::new(area.right() - view.at(18.0), heading_y);
        let hit = Rect::from_center_size(centre, Vec2::splat(view.at(22.0)));
        let response =
            ui.interact(hit, ui.id().with(("explorer-button", name)), Sense::click()).on_hover_text(name);
        if response.hovered() {
            painter.rect_filled(hit, CornerRadius::same(4), color::CONTROL);
        }
        icon::collapse_at(&painter, centre, color::TEXT_DIM, view.zoom);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name)
        });
        if response.clicked() {
            outcome.hide = true;
        }
    }

    // The filter box.
    let filter_rect = Rect::from_min_size(
        Pos2::new(area.left() + view.at(12.0), area.top() + view.at(36.0)),
        Vec2::new(area.width() - view.at(24.0), view.at(24.0)),
    );
    painter.rect(
        filter_rect,
        CornerRadius::same(size::CONTROL_CORNER),
        color::FIELD,
        Stroke::new(1.0, color::DIVIDER),
        egui::StrokeKind::Inside,
    );
    icon::magnifier_at(
        &painter,
        Pos2::new(filter_rect.left() + view.at(13.0), filter_rect.center().y),
        color::TEXT_FAINT,
        view.zoom,
    );
    let text_rect = crate::components::controls::field_text_rect(ui, filter_rect, view.at(26.0));
    // The size the box would set text in, zoomed. Asked of the style rather than written down, so at a
    // zoom of one the filter box is exactly the box it was before `task-1771` — which is a promise a
    // screenshot test keeps and which a number chosen here would have quietly broken.
    let typed = ui
        .style()
        .text_styles
        .get(&egui::TextStyle::Body)
        .map(|font| font.size)
        .unwrap_or(12.0)
        * view.zoom;
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = field.add(
        egui::TextEdit::singleline(filter)
            .hint_text(egui::RichText::new("Filter files").color(color::TEXT_FAINT).size(typed))
            .font(egui::FontId::proportional(typed))
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .text_color(color::TEXT_CONTROL),
    );
    // Named, because a test finds a control by its name and egui names a text box after what is typed in it.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Filter files")
    });

    // The rows: either the tree, or a flat list of what matches the filter.
    let list_top = filter_rect.bottom() + view.at(12.0);
    let footer_top = area.bottom() - view.at(size::EXPLORER_FOOTER);
    let list_rect = Rect::from_min_max(Pos2::new(area.left(), list_top), Pos2::new(area.right(), footer_top));
    let filtering = !filter.trim().is_empty();

    // What was drawn, and what is in the air. Both filled in by the loop below and read once it has
    // finished, which is the first moment anything knows where every row ended up.
    let mut drawn: Vec<Drawn> = Vec::new();
    let mut carried: Option<PathBuf> = None;
    let mut released = false;

    let mut list = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
    list.set_clip_rect(list_rect);
    let mut rows_area = egui::ScrollArea::vertical().id_salt("explorer-rows");
    if let Some(offset) = view.scroll_to {
        rows_area = rows_area.vertical_scroll_offset(offset.max(0.0));
    }
    let scrolled = rows_area.show(&mut list, |ui| {
        if filtering {
            let matches = tree.matching(filter);
            if matches.is_empty() {
                ui.add_space(view.at(6.0));
                ui.label(
                    egui::RichText::new("  No file matches")
                        .size(view.at(11.5))
                        .color(color::TEXT_FAINT),
                );
            }
            for path in matches {
                let depth = tree.depth_of(path);
                // **Asked by name.** `task-28`: this was `file_kind::openable`, which reads a file whose
                // extension it does not know, so filtering a large project opened and read up to
                // `SEARCH_LIMIT` files **every frame** while somebody was typing. `all_files` holds only
                // regular files, so the kind is known without asking; the size is not, and the tab is
                // where a file too large to open says so.
                let refusal = crate::services::file_kind::openable_in_a_listing(
                    path,
                    crate::services::file_kind::Kind::File,
                    None,
                )
                .err();
                let row = file_row(ui, path, depth, view, refusal, decorate(path));
                row.collect(path, false, &mut drawn, &mut carried, &mut released);
                row.apply(&mut outcome, path, false);
            }
        } else {
            // **Only the rows on screen.** `task-28`: every row in the tree was drawn every frame, and each
            // one allocates a rectangle and interacts, so a folder of twenty thousand files opened out was
            // twenty thousand widgets a frame and the window could not be used until it was closed again.
            //
            // Every row is exactly `size::ROW` tall, so which rows are visible is arithmetic rather than a
            // measurement — the same reasoning `editor_view::visible_lines` uses about a document. The
            // space above the first drawn row and below the last is added back, so the scroll bar still
            // describes the whole tree.
            let rows = tree.rows();
            let visible = visible_rows(ui, rows.len(), view.at(size::ROW));
            // A reveal has to work for a row that is not being drawn, which is the one thing
            // virtualisation takes away: `folder_row` and `file_row` scroll to their own rectangle, and a
            // row that was never drawn has none. So the scroll is asked for here as well, from the row's
            // place in the list, which is known whether or not it is on screen.
            if let Some(index) = revealed_row(&rows, view) {
                let top = ui.cursor().top() + index as f32 * view.at(size::ROW);
                ui.scroll_to_rect(
                    Rect::from_min_size(
                        Pos2::new(ui.clip_rect().left(), top),
                        Vec2::new(1.0, view.at(size::ROW)),
                    ),
                    None,
                );
            }
            ui.add_space(visible.start as f32 * view.at(size::ROW));
            for row in &rows[visible.clone()] {
                if row.entry.is_directory {
                    let clicked = folder_row(ui, &row.entry, row.depth, view);
                    if clicked.open {
                        outcome.toggle = Some(row.entry.path.clone());
                    }
                    clicked.collect(&row.entry.path, true, &mut drawn, &mut carried, &mut released);
                    clicked.apply(&mut outcome, &row.entry.path, true);
                } else {
                    let clicked = file_row(
                        ui,
                        &row.entry.path,
                        row.depth,
                        view,
                        row.entry.refusal,
                        decorate(&row.entry.path),
                    );
                    clicked.collect(
                        &row.entry.path,
                        false,
                        &mut drawn,
                        &mut carried,
                        &mut released,
                    );
                    clicked.apply(&mut outcome, &row.entry.path, false);
                }
            }
            ui.add_space((rows.len() - visible.end) as f32 * view.at(size::ROW));
        }
        if let Some(error) = &tree.last_error {
            ui.add_space(view.at(6.0));
            ui.label(egui::RichText::new(error).size(view.at(11.0)).color(color::CLOSE));
        }
    });
    outcome.scroll = scrolled.state.offset.y;

    // The empty space below the last row. `task-1693` asks that a right click there open the same
    // menu, so that a file or a folder can be made from anywhere in the panel rather than only from
    // a row that happens to be in the right place. It is the project folder's menu, which is the
    // answer the heading already gives — with everything that is about a particular file dimmed,
    // because nothing was clicked.
    //
    // Added **after** the rows, so a row that is there wins the point, and only over what is left of
    // the list once they have been drawn.
    let used = drawn.iter().map(|row| row.rect.bottom()).fold(list_rect.top(), f32::max);
    let empty = Rect::from_min_max(Pos2::new(list_rect.left(), used), list_rect.max);
    if empty.height() > 1.0 {
        let response = ui.interact(empty, ui.id().with("explorer-empty"), Sense::click());
        if response.secondary_clicked() {
            if let Some(at) =
                response.interact_pointer_pos().or_else(|| response.hover_pos())
            {
                outcome.context_menu = Some((at, tree.root().to_path_buf(), true));
                outcome.menu_over_empty_space = true;
                outcome.focus = true;
            }
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), "Explorer background")
        });
    }

    // Where a row being carried would land. After the loop, for the reason `settle_the_tab_drag`
    // runs after the pane loop: this is the earliest moment anything knows where every row is.
    outcome.dragging = carried.is_some();
    if let Some(source) = &carried {
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let target = pointer.and_then(|at| drop_target(&drawn, tree.root(), &heading_hit, at, source));
        if let Some(folder) = &target {
            let painter = ui.painter_at(area);
            if let Some(row) = drawn.iter().find(|row| row.directory && row.path == *folder) {
                painter.rect(
                    row.rect.shrink2(Vec2::new(8.0, 1.0)),
                    CornerRadius::same(5),
                    color::CONTROL,
                    Stroke::new(1.0, color::ACCENT),
                    egui::StrokeKind::Inside,
                );
            } else if *folder == *tree.root() {
                painter.rect(
                    heading_hit,
                    CornerRadius::same(5),
                    egui::Color32::TRANSPARENT,
                    Stroke::new(1.0, color::ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let Some(at) = pointer {
            carried_name(ui, area, source, at, target.is_some());
        }
        if released {
            if let Some(folder) = target {
                outcome.moved = Some((source.clone(), folder));
            }
        }
    }

    // The footer, counting the files and how many are unsaved.
    let footer = Rect::from_min_max(Pos2::new(area.left(), footer_top), area.right_bottom());
    painter.rect_filled(footer, CornerRadius::ZERO, crate::theme::faded(color::EXPLORER_FOOTER, view.opacity));
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
    if view.unsaved {
        text = format!("{text}  \u{00B7}  1 unsaved");
    }
    let galley =
        painter.layout_no_wrap(text, egui::FontId::proportional(view.at(10.5)), color::TEXT_DIM);
    painter.galley(
        Pos2::new(footer.left() + view.at(16.0), footer.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_DIM,
    );

    outcome
}

/// The folder a drop at `at` would land in, or nothing when the drop is refused.
///
/// A folder row is that folder and a file row is the folder the file is in, which is what IntelliJ
/// does and is what somebody aiming at a crowded folder means. Three things answer with nothing: a
/// folder dropped into itself or into anything under it, a path dropped into the folder it is
/// already in, and a pointer over no row at all.
fn drop_target(
    drawn: &[Drawn],
    root: &Path,
    heading: &Rect,
    at: Pos2,
    source: &Path,
) -> Option<PathBuf> {
    let folder = match drawn.iter().find(|row| row.rect.contains(at)) {
        Some(row) if row.directory => row.path.clone(),
        Some(row) => row.path.parent()?.to_path_buf(),
        None if heading.contains(at) => root.to_path_buf(),
        None => return None,
    };
    if folder == source || folder.starts_with(source) {
        return None;
    }
    if source.parent() == Some(folder.as_path()) {
        return None;
    }
    Some(folder)
}

/// The name of what is being carried, drawn under the pointer.
fn carried_name(ui: &egui::Ui, area: Rect, source: &Path, at: Pos2, welcome: bool) {
    let name = source.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    let painter = ui.painter_at(area.expand(4.0));
    let tint = if welcome { color::TEXT_STRONG } else { color::TEXT_FAINT };
    let galley = painter.layout_no_wrap(name, egui::FontId::proportional(12.0), tint);
    let box_rect = Rect::from_min_size(
        at + Vec2::new(12.0, 6.0),
        galley.size() + Vec2::new(12.0, 6.0),
    );
    painter.rect(
        box_rect,
        CornerRadius::same(4),
        color::MENU,
        Stroke::new(1.0, if welcome { color::ACCENT } else { color::CONTROL_BORDER }),
        egui::StrokeKind::Inside,
    );
    painter.galley(box_rect.min + Vec2::new(6.0, 3.0), galley, tint);
}

/// What happened to one row, before the caller knows which path it was.
///
/// A row cannot decide what a click means — that is the window's business — so it reports the
/// things that can happen to it and the caller turns them into an outcome.
#[derive(Debug)]
struct RowClick {
    /// Clicked once: open the file, or open or close the folder.
    open: bool,
    /// Clicked twice: open the file in a tab of its own.
    twice: bool,
    /// Right clicked, at this position.
    menu: Option<Pos2>,
    /// Where the row was drawn, which is what a drop is worked out against.
    rect: Rect,
    /// True while this row is the one being carried.
    dragged: bool,
    /// True on the frame it was let go.
    dropped: bool,
    /// Clicked at all, whether or not the file can be opened. What the selection follows, because
    /// a file Quill cannot show the text of can still be the one you meant to delete.
    picked: bool,
}

impl RowClick {
    fn apply(&self, outcome: &mut ExplorerOutcome, path: &std::path::Path, directory: bool) {
        if let Some(at) = self.menu {
            outcome.context_menu = Some((at, path.to_path_buf(), directory));
            outcome.select = Some(path.to_path_buf());
            outcome.focus = true;
        }
        if self.picked {
            outcome.select = Some(path.to_path_buf());
            outcome.focus = true;
        }
        if directory {
            return;
        }
        if self.twice {
            outcome.open_permanently = Some(path.to_path_buf());
        } else if self.open {
            outcome.open = Some(path.to_path_buf());
        }
    }

    /// Remember where this row was drawn, and whether it is the one in the air.
    fn collect(
        &self,
        path: &std::path::Path,
        directory: bool,
        drawn: &mut Vec<Drawn>,
        carried: &mut Option<PathBuf>,
        released: &mut bool,
    ) {
        drawn.push(Drawn { rect: self.rect, path: path.to_path_buf(), directory });
        if self.dragged || self.dropped {
            *carried = Some(path.to_path_buf());
        }
        if self.dropped {
            *released = true;
        }
    }

    /// Read a response into a click. Right clicking is separate from left clicking, so a menu can
    /// be opened over a row without also opening the file.
    fn from(response: &egui::Response) -> Self {
        Self {
            open: response.clicked(),
            twice: response.double_clicked(),
            menu: response
                .secondary_clicked()
                .then(|| response.interact_pointer_pos().or_else(|| response.hover_pos()))
                .flatten(),
            rect: response.rect,
            dragged: response.dragged(),
            dropped: response.drag_stopped(),
            picked: response.clicked() || response.double_clicked(),
        }
    }
}

/// A folder row: a triangle that points down when open, then the name.
fn folder_row(
    ui: &mut egui::Ui,
    entry: &crate::services::file_tree::Entry,
    depth: usize,
    view: View,
) -> RowClick {
    let name = &entry.name;
    let row = allocate_row(ui, view.at(size::ROW));
    let response =
        ui.interact(row, ui.id().with(("folder", name, depth)), Sense::click_and_drag());
    let pill = row.shrink2(Vec2::new(view.at(8.0), 1.0));
    let selected = view.selected == Some(entry.path.as_path());
    if selected && view.reveal_selected {
        ui.scroll_to_rect(row, None);
    }
    // A folder is never the file that is showing, so it has only the cursor's quiet mark. See the
    // note at the top of this file.
    if selected || response.hovered() {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::CONTROL);
    }
    if selected && view.keyboard {
        ui.painter().rect_stroke(
            pill,
            CornerRadius::same(5),
            Stroke::new(1.0, color::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    let x = row.left() + view.at(16.0) + depth as f32 * view.at(size::INDENT);
    icon::disclosure_at(
        ui.painter(),
        Pos2::new(x, row.center().y),
        entry.expanded,
        color::TEXT_DIM,
        view.zoom,
    );
    let galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(view.at(12.5)),
        color::TEXT_CONTROL,
    );
    ui.painter().galley(
        Pos2::new(x + view.at(12.0), row.center().y - galley.size().y / 2.0),
        galley,
        color::TEXT_CONTROL,
    );
    // The accessible name is the folder's name, so a test can ask for it by name.
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), entry.expanded, name)
    });
    RowClick::from(&response)
}

/// A file row: a small coloured square saying what kind of file it is, then the name. The file that is open
/// is drawn as a filled pill, with an amber dot on the right when it has unsaved changes.
fn file_row(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    depth: usize,
    view: View,
    refusal: Option<Refusal>,
    decoration: Decoration,
) -> RowClick {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let row = allocate_row(ui, view.at(size::ROW));
    // A file Quill cannot open is drawn dimmed and does not open on a click, so the tree says what is
    // in the folder without pretending everything in it can be opened. It still takes a right click
    // and can still be carried: renaming a picture, or moving one, has nothing to do with whether
    // Quill can show its text.
    let openable = refusal.is_none();
    let response = ui.interact(row, ui.id().with(("file", path)), Sense::click_and_drag());
    if let Some(refusal) = refusal {
        // The row says which of the two reasons it is: the file is not text, or it is too large.
        response.clone().on_hover_text(refusal.reason());
    }
    let open = view.current == Some(path);
    let selected = view.selected == Some(path);
    if (open && view.reveal) || (selected && view.reveal_selected) {
        // The least scrolling that brings the row into view, so a row already on the screen does not
        // move. See the note at the top of this file.
        ui.scroll_to_rect(row, None);
    }
    let pill = row.shrink2(Vec2::new(view.at(8.0), 1.0));
    // The pill is the file that is **showing**. The explorer's own cursor gets the quieter fill the
    // hover already uses, so two rows are never drawn as though both were open — `task-1693`, and
    // the note at the top of this file.
    if open {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::SELECTED_ROW);
    } else if selected || (response.hovered() && openable) {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::CONTROL);
    }
    // The ring says where the keyboard is, which is the whole reason the explorer has a selection of
    // its own. Without it a person could not tell whether Delete would take a letter or a file.
    if selected && view.keyboard {
        ui.painter().rect_stroke(
            pill,
            CornerRadius::same(5),
            Stroke::new(1.0, color::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    let x = row.left() + view.at(16.0) + depth as f32 * view.at(size::INDENT);
    match &decoration.icon {
        // A file whose plugin gives it a picture gets the picture in place of the square.
        Some(icon) => crate::services::icons::draw(
            ui.painter(),
            Pos2::new(x + view.at(4.0), row.center().y),
            icon,
        ),
        None => {
            let marker =
                if openable { file_marker(path) } else { color::TEXT_FAINT.gamma_multiply(0.45) };
            ui.painter().rect_filled(
                Rect::from_center_size(
                    Pos2::new(x + view.at(4.0), row.center().y),
                    Vec2::splat(view.at(8.0)),
                ),
                CornerRadius::same(2),
                marker,
            );
        }
    }
    // A file git has something to say about is drawn in git's colour for it, which is what IntelliJ
    // does and is the cheapest way to see at a glance what a commit would hold.
    let tint = if open {
        color::TEXT_STRONG
    } else if let Some(mark) = decoration.tint {
        mark
    } else if openable {
        color::TEXT_CONTROL
    } else {
        color::TEXT_FAINT.gamma_multiply(0.7)
    };
    let galley =
        ui.painter().layout_no_wrap(name.clone(), egui::FontId::proportional(view.at(12.5)), tint);
    ui.painter().galley(
        Pos2::new(x + view.at(16.0), row.center().y - galley.size().y / 2.0),
        galley,
        tint,
    );
    if open && view.unsaved {
        ui.painter().circle_filled(
            Pos2::new(pill.right() - view.at(12.0), row.center().y),
            view.at(3.5),
            color::UNSAVED,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), open, &name)
    });
    let mut click = RowClick::from(&response);
    if !openable {
        // It cannot be opened, so a click on it opens nothing. The menu, the selection and carrying
        // it still work.
        click.open = false;
        click.twice = false;
    }
    click
}

/// Which rows fall inside what is being drawn into, given how many there are.
///
/// One row either side of the visible band, so a row half off the top or the bottom edge is drawn rather
/// than appearing as the list is scrolled.
fn visible_rows(ui: &egui::Ui, total: usize, row: f32) -> std::ops::Range<usize> {
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let row = row.max(1.0);
    let first = ((clip.top() - top) / row).floor().max(0.0) as usize;
    let first = first.saturating_sub(1).min(total);
    let count = (clip.height() / row).ceil() as usize + 2;
    first..(first + count).min(total)
}

/// Where in the list the row the window asked to be scrolled to is, if it asked for one.
///
/// `reveal` is about the file that is showing and `reveal_selected` about the explorer's own cursor, which
/// is the same pair `file_row` and `folder_row` read. Both are one frame long, so this answers `None` on
/// nearly every frame.
fn revealed_row(
    rows: &[crate::services::file_tree::Row<'_>],
    view: View<'_>,
) -> Option<usize> {
    let wanted = match (view.reveal, view.reveal_selected) {
        (_, true) => view.selected?,
        (true, false) => view.current?,
        (false, false) => return None,
    };
    rows.iter().position(|row| row.entry.path == wanted)
}

fn allocate_row(ui: &mut egui::Ui, height: f32) -> Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    rect
}
