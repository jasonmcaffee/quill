//! The editing area's own right click menu, which is where a passage is marked.
//!
//! Quill had no menu on the editing area at all before `task-1663`. This is it: the ordinary
//! clipboard rows, drawn by the same [`controls::menu_rows`] the bar's menus and the other two
//! context menus use so the row height, the dimming and the shortcut column cannot drift; then a row
//! of four colour blocks with the colour wheel's icon at the end of it; then the two ways of taking
//! a mark away.
//!
//! Whether it is open is the window's state rather than egui's memory, which is the rule the
//! gutter's menu already follows: **a screenshot test cannot press the right mouse button**, and a
//! menu that can only be reached by pressing it cannot be looked at.
//!
//! The colour wheel is drawn **inside this popup** rather than in a second one over it. egui keeps
//! at most one popup open at a time, so opening a second would shut this one — the same rule that
//! turned the three line spacings in the text options panel from a dropdown into three buttons.
//! Pressing the wheel's icon makes the menu taller.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use quill_core::Rgba;

use crate::app::actions::{Action, Entry, HighlightColor};
use crate::components::color_wheel;
use crate::components::controls;
use crate::theme::{color, icon};

/// How wide the menu is. Wider than the explorer's, because the colour wheel has to fit in it.
const WIDTH: f32 = 300.0;
/// One colour block.
const BLOCK: f32 = 34.0;
/// The gap between two blocks.
const GAP: f32 = 8.0;
/// How tall the row holding the blocks is.
const BLOCK_ROW: f32 = 40.0;

/// The editing area's menu, while it is open.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMenu {
    /// The top left corner, which is where the pointer was.
    pub at: Pos2,
    /// Where in the document the pointer was, which is what decides whether there is a mark to
    /// clear and which one it is.
    pub offset: usize,
    /// The colour being chosen, while the wheel is showing. `None` is the wheel put away.
    pub wheel: Option<Rgba>,
}

impl TextMenu {
    pub fn new(at: Pos2, offset: usize) -> Self {
        Self { at, offset, wheel: None }
    }
}

/// What happened in the menu this frame.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// A plain row was chosen.
    pub chosen: Option<Action>,
    /// A colour was chosen: one of the four blocks, or `Apply highlight` in the wheel.
    pub highlight: Option<Rgba>,
    /// The wheel was opened, moved or put away. The outer `Some` is "this changed"; the inner is
    /// the wheel's new state.
    pub wheel: Option<Option<Rgba>>,
    /// The menu should be put away.
    pub close: bool,
}

/// Draw the menu.
///
/// `above` is the rows over the colours — the clipboard and Select All — and `below` is the rows
/// under them, which is the two ways of clearing. Both are ordinary [`Entry`] lists built in
/// `app::actions`, so an entry added to either is an [`Action`] with a name and is on the command
/// line the same day.
pub fn show(
    ui: &mut egui::Ui,
    menu: &TextMenu,
    above: &[Entry],
    below: &[Entry],
    has_selection: bool,
    last: Rgba,
) -> Outcome {
    let mut outcome = Outcome::default();
    let popup = egui::Popup::new(
        egui::Id::new("quill-text-menu"),
        ui.ctx().clone(),
        menu.at,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .frame(
        egui::Frame::popup(ui.style())
            .fill(color::menu())
            .stroke(Stroke::new(1.0, color::control_border()))
            .inner_margin(6),
    )
    .width(WIDTH);

    if let Some(response) = popup.show(|ui| {
        let mut inner =
            Outcome { chosen: controls::menu_rows(ui, above, 0.0), ..Outcome::default() };
        ui.separator();
        controls::menu_heading(ui, "Highlight", 0.0);
        blocks(ui, menu, has_selection, last, &mut inner);
        if let Some(colour) = menu.wheel {
            let (area, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), color_wheel::HEIGHT), Sense::hover());
            let wheel = color_wheel::show(ui, area, colour);
            if let Some(chosen) = wheel.chosen {
                inner.wheel = Some(Some(chosen));
            }
            if let Some(applied) = wheel.applied {
                inner.highlight = Some(applied);
            }
        }
        ui.separator();
        if let Some(action) = controls::menu_rows(ui, below, 0.0) {
            inner.chosen = Some(action);
        }
        inner
    }) {
        let inner = response.inner;
        outcome.chosen = inner.chosen;
        outcome.highlight = inner.highlight;
        outcome.wheel = inner.wheel;
        outcome.close = response.response.should_close();
    }
    if outcome.chosen.is_some() || outcome.highlight.is_some() {
        outcome.close = true;
    }
    // Escape puts a menu away wherever it is, which is the one thing every menu on every platform
    // agrees about.
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        outcome.close = true;
    }
    outcome
}

/// The row of four colour blocks, with the wheel's icon at the end of it.
///
/// A block rather than a row with a word on it, because the ask is for blocks and because the whole
/// question a person is answering here is "which colour", which a colour answers better than its
/// name does. Each one still has its name for a test and for assistive technology.
fn blocks(
    ui: &mut egui::Ui,
    menu: &TextMenu,
    has_selection: bool,
    last: Rgba,
    outcome: &mut Outcome,
) {
    let (row, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), BLOCK_ROW), Sense::hover());
    let mut left = row.left() + 8.0;
    for colour in HighlightColor::ALL {
        let area = Rect::from_min_size(
            Pos2::new(left, row.center().y - BLOCK / 2.0),
            Vec2::splat(BLOCK),
        );
        if block(ui, area, colour, has_selection) {
            outcome.highlight = Some(colour.rgba());
        }
        left += BLOCK + GAP;
    }
    // The wheel's icon sits at the right hand end, apart from the four, because it is not a fifth
    // colour: it is the way to any colour at all.
    let area = Rect::from_min_size(
        Pos2::new(row.right() - 8.0 - BLOCK, row.center().y - BLOCK / 2.0),
        Vec2::splat(BLOCK),
    );
    let open = menu.wheel.is_some();
    let response = ui.interact(area, ui.id().with("highlight-wheel"), Sense::click());
    let painter = ui.painter();
    painter.rect(
        area,
        CornerRadius::same(6),
        if open { color::control() } else { egui::Color32::TRANSPARENT },
        Stroke::new(1.0, if open { color::accent() } else { color::control_border() }),
        egui::StrokeKind::Inside,
    );
    icon::color_wheel(painter, area.center(), color::text_control());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, "Choose a colour")
    });
    if response.clicked() {
        // It opens on the colour that was last marked in, so choosing a shade of it again is a drag
        // rather than a hunt. The window holds that; the menu is given it.
        outcome.wheel = Some(if open { None } else { Some(menu.wheel.unwrap_or(last)) });
    }
}

/// One colour block: the colour itself, over the editor's own background so that what is drawn is
/// what the passage will look like rather than the colour at full strength.
fn block(ui: &mut egui::Ui, area: Rect, colour: HighlightColor, enabled: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let name = format!("Highlight {}", colour.label().to_ascii_lowercase());
    let response = ui.interact(area, ui.id().with(("highlight-block", colour.label())), sense);
    let painter = ui.painter();
    painter.rect_filled(area, CornerRadius::same(6), color::editor());
    let paint = if enabled { colour.color() } else { colour.color().gamma_multiply(0.35) };
    painter.rect_filled(area, CornerRadius::same(6), paint);
    let edge = if response.hovered() && enabled { color::text_strong() } else { color::control_border() };
    painter.rect_stroke(area, CornerRadius::same(6), Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    response.clicked()
}
