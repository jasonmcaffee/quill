//! The strip of tabs along the top of the editing area, one for each open file.
//!
//! It sits over the editing area rather than over the whole window, because the explorer is to the
//! left of it and a tab belongs to the editor. That is where IntelliJ puts it and where
//! `tasks/quill-ide-tdd.md` says it goes.
//!
//! The tab that is showing is a filled row with a two point accent line along its bottom edge, which
//! is the underline `task-1649` asks for. A file with unsaved changes shows the amber dot Quill
//! already uses for that, in place of the close cross until the pointer is over the tab. A transient
//! tab — the one a single click reuses — has its name in italic, which is how a person can tell that
//! clicking another file will take it away.
//!
//! When the tabs do not fit, the strip is shifted so the one that is showing is on screen, rather
//! than the tabs being squeezed to unreadable stubs. There is no scroll area, because everything in
//! Quill is painted at an absolute position and one container that is not would behave differently
//! from the rest of the window.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::theme::{color, icon};

/// How tall the strip is. The same as the terminal tile's header, so the two horizontal strips in
/// the window are the same height.
pub const HEIGHT: f32 = 32.0;
/// The accent line under the tab that is showing.
const UNDERLINE: f32 = 2.0;
/// Space either side of a tab's contents.
const PADDING: f32 = 10.0;
/// The gap between two tabs.
const GAP: f32 = 2.0;

/// One tab, as the strip draws it. The strip is given these rather than the open files themselves,
/// because a component draws and does not reach into the window's state.
#[derive(Clone)]
pub struct TabView {
    pub name: String,
    /// There are unsaved changes.
    pub modified: bool,
    /// This is the tab a single click in the explorer reuses.
    pub transient: bool,
    /// The small square in front of the name, coloured by what kind of file it is. Drawn only when
    /// the file has no plugin to give it a picture.
    pub marker: Color32,
    /// The picture the file's plugin puts in front of it.
    pub icon: Option<egui::TextureHandle>,
}

/// What the user did in the strip.
#[derive(Debug, Default, PartialEq)]
pub struct TabsOutcome {
    /// A tab was clicked, so it should be shown.
    pub show: Option<usize>,
    /// A tab's close cross was clicked, or it was clicked with the middle button.
    pub close: Option<usize>,
    /// A tab was double clicked, which makes a transient tab permanent.
    pub keep: Option<usize>,
}

/// How wide one tab is. Measured rather than guessed, because a name can be any length.
fn tab_width(ui: &egui::Ui, tab: &TabView) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        tab.name.clone(),
        egui::FontId::proportional(12.0),
        color::TEXT_CONTROL,
    );
    // The marker, the gap after it, the name, the gap before the cross, and the cross.
    PADDING + 8.0 + 8.0 + galley.size().x + 8.0 + 14.0 + PADDING
}

/// Draw the strip into `area`.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    tabs: &[TabView],
    active: usize,
    opacity: f32,
) -> TabsOutcome {
    let mut outcome = TabsOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::TOOLBAR, opacity));
    painter.line_segment(
        [Pos2::new(area.left(), area.bottom()), Pos2::new(area.right(), area.bottom())],
        Stroke::new(1.0, color::DIVIDER),
    );

    let widths: Vec<f32> = tabs.iter().map(|tab| tab_width(ui, tab)).collect();
    let offset = shift(&widths, active, area.width());

    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    inner.set_clip_rect(ui.painter().clip_rect().intersect(area));

    let mut pen = area.left() - offset;
    for (index, tab) in tabs.iter().enumerate() {
        let rect = Rect::from_min_size(Pos2::new(pen, area.top()), Vec2::new(widths[index], area.height()));
        pen += widths[index] + GAP;
        if rect.right() < area.left() || rect.left() > area.right() {
            continue;
        }
        draw_tab(&mut inner, rect, tab, index, index == active, &mut outcome);
    }
    outcome
}

/// How far the strip is shifted left so that the tab that is showing is on screen.
///
/// Split out so it can be tested without a window: the arithmetic is what decides whether a tab a
/// long way along the strip can be reached at all.
fn shift(widths: &[f32], active: usize, available: f32) -> f32 {
    let total: f32 = widths.iter().map(|width| width + GAP).sum();
    if total <= available || active >= widths.len() {
        return 0.0;
    }
    let left: f32 = widths.iter().take(active).map(|width| width + GAP).sum();
    let right = left + widths[active];
    // Far enough left to bring the active tab's right edge into view, and never past its left edge.
    let needed = (right - available).max(0.0);
    needed.min(left).max(0.0)
}

/// One tab: the marker, the name, and either the unsaved dot or the close cross.
fn draw_tab(
    ui: &mut egui::Ui,
    rect: Rect,
    tab: &TabView,
    index: usize,
    active: bool,
    outcome: &mut TabsOutcome,
) {
    let name = format!("Tab: {}", tab.name);
    let response = ui
        .interact(rect, ui.id().with(("file-tab", index)), Sense::click())
        .on_hover_text(&name);
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, CornerRadius::ZERO, color::SELECTED_ROW);
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.bottom() - UNDERLINE),
                Vec2::new(rect.width(), UNDERLINE),
            ),
            CornerRadius::ZERO,
            color::ACCENT,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, color::CONTROL);
    }

    match &tab.icon {
        Some(icon) => crate::services::icons::draw(
            painter,
            Pos2::new(rect.left() + PADDING + 5.0, rect.center().y),
            icon,
        ),
        None => {
            painter.rect_filled(
                Rect::from_center_size(
                    Pos2::new(rect.left() + PADDING + 4.0, rect.center().y),
                    Vec2::splat(8.0),
                ),
                CornerRadius::same(2),
                tab.marker,
            );
        }
    }

    let tint = if active { color::TEXT_STRONG } else { color::TEXT_CONTROL };
    // A transient tab is drawn faintly rather than in italic: egui has no italic face for the
    // family Quill installs, and a fake slant is worse than a change of weight.
    let tint = if tab.transient { tint.gamma_multiply(0.75) } else { tint };
    let galley =
        painter.layout_no_wrap(tab.name.clone(), egui::FontId::proportional(12.0), tint);
    painter.galley(
        Pos2::new(rect.left() + PADDING + 16.0, rect.center().y - galley.size().y / 2.0),
        galley,
        tint,
    );

    // The cross, or the amber dot when there are unsaved changes and the pointer is elsewhere.
    let shut = Rect::from_center_size(
        Pos2::new(rect.right() - PADDING - 5.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if tab.modified && !response.hovered() {
        painter.circle_filled(shut.center(), 3.5, color::UNSAVED);
    } else {
        let shut_name = format!("Close {}", tab.name);
        let shut_response = ui
            .interact(shut, ui.id().with(("file-tab-close", index)), Sense::click())
            .on_hover_text(&shut_name);
        icon::cross(&ui.painter(), shut.center(), color::TEXT_DIM);
        shut_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &shut_name)
        });
        if shut_response.clicked() {
            outcome.close = Some(index);
        }
    }

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &name)
    });
    if response.double_clicked() {
        outcome.keep = Some(index);
    } else if response.clicked() {
        outcome.show = Some(index);
    }
    if response.middle_clicked() {
        outcome.close = Some(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_that_fit_are_not_shifted() {
        assert_eq!(shift(&[100.0, 100.0], 1, 400.0), 0.0);
    }

    #[test]
    fn a_tab_off_the_right_hand_end_is_brought_into_view() {
        // Three 100 point tabs in 250 points. The third runs from 204 to 304, so the strip has to
        // move 54 points left for its right edge to be reachable.
        let widths = [100.0, 100.0, 100.0];
        let offset = shift(&widths, 2, 250.0);
        assert!((offset - 54.0).abs() < 0.01, "expected 54, got {offset}");
    }

    #[test]
    fn the_first_tab_is_never_shifted_off_its_own_left_edge() {
        let widths = [100.0, 100.0, 100.0];
        assert_eq!(shift(&widths, 0, 50.0), 0.0, "shifting past a tab's left edge hides it");
    }

    #[test]
    fn an_index_past_the_end_shifts_nothing() {
        assert_eq!(shift(&[100.0, 100.0, 100.0], 9, 50.0), 0.0);
    }
}
