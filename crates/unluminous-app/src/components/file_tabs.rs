//! The strip of tabs along the top of the editing area, one for each open file.
//!
//! It sits over the editing area rather than over the whole window, because the explorer is to the
//! left of it and a tab belongs to the editor. That is where the reference editor puts it and where
//! `tasks/unluminous-ide-tdd.md` says it goes.
//!
//! The tab that is showing is a filled row with a two point accent line along its bottom edge, which
//! is the underline `task-1649` asks for. A file with unsaved changes shows the amber dot Unluminous
//! already uses for that, in place of the close cross until the pointer is over the tab. A transient
//! tab — the one a single click reuses — has its name in italic, which is how a person can tell that
//! clicking another file will take it away.
//!
//! When the tabs do not fit, the strip is shifted so the one that is showing is on screen, rather
//! than the tabs being squeezed to unreadable stubs. There is no scroll area, because everything in
//! Unluminous is painted at an absolute position and one container that is not would behave differently
//! from the rest of the window.
//!
//! ## One strip a pane
//!
//! Since `task-1664` the editing area can be split into panes, and each pane has a strip of its own
//! holding its own tabs. Two things follow, and both are parameters rather than something the strip
//! works out.
//!
//! Every control's id carries the **pane number**, because egui identifies a widget by its id and two
//! strips whose second tab shared an id would hand one click to both of them.
//!
//! And the strip is told whether its pane has the **keyboard**. The accent line under the tab that is
//! showing is drawn in the quiet colour when it does not, which is how a person sees at a glance
//! which of four panes their typing is going to.

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
    /// A tab was right clicked: which one, and where the pointer was.
    pub menu: Option<(usize, Pos2)>,
    /// A tab is being dragged: which one, and where the pointer is now.
    ///
    /// Reported every frame the drag is held, and again with [`Self::dropped`] set on the frame it
    /// is let go. Where it lands is not the strip's business: the pointer may be over a strip
    /// belonging to another pane, which this one has never heard of, so the window works it out once
    /// every pane has said where its own tabs are. See [`Strip`].
    pub dragging: Option<(usize, Pos2)>,
    /// The drag ended on this frame.
    pub dropped: bool,
    /// The empty part of the strip, past the last tab, was double clicked.
    ///
    /// `task-1771` asks that two presses at the top of a pane fill the window with it. Every other pane in
    /// Unluminous has a header to press; the editing area has this strip, and the part of it no tab wanted is
    /// exactly what a panel's own drag handle is left with. See `components::dock::handle`.
    pub twice_on_the_empty_part: bool,
    /// Where this strip drew itself and each of its tabs, so the window can say which strip a
    /// pointer is over and where between two tabs it fell.
    pub strip: Strip,
}

/// Where a strip and its tabs ended up on the screen.
///
/// A component draws and does not decide, so this is what the strip reports rather than a conclusion
/// about it. It is the only way a tab can be dragged from one pane into another: each pane draws its
/// own strip and knows nothing of the others, and a drag started in one of them carries a pointer
/// that ends up over a different one.
#[derive(Debug, Clone, PartialEq)]
pub struct Strip {
    /// The whole strip.
    pub area: Rect,
    /// One rectangle a tab, in the order they are drawn — including the ones scrolled off the end,
    /// so dropping a tab past the visible ones still lands somewhere sensible.
    pub tabs: Vec<Rect>,
}

impl Default for Strip {
    /// A strip that has not been drawn: nowhere, holding nothing. `Rect` has no `Default` of its
    /// own, deliberately, because there is no obvious empty rectangle; `NOTHING` is the one egui
    /// offers for a rectangle that has not been decided yet.
    fn default() -> Self {
        Self { area: Rect::NOTHING, tabs: Vec::new() }
    }
}

impl Strip {
    /// Where in this strip a tab dropped at `x` belongs: how many tabs it goes after.
    ///
    /// A tab goes after every tab whose middle the pointer has passed, which is what makes a
    /// rearrangement follow the pointer rather than jump when it crosses an edge.
    pub fn position_at(&self, x: f32) -> usize {
        self.tabs.iter().filter(|rect| rect.center().x < x).count()
    }
}

/// How wide one tab is. Measured rather than guessed, because a name can be any length.
fn tab_width(ui: &egui::Ui, tab: &TabView) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        tab.name.clone(),
        egui::FontId::proportional(12.0),
        color::text_control(),
    );
    // The marker, the gap after it, the name, the gap before the cross, and the cross.
    PADDING + 8.0 + 8.0 + galley.size().x + 8.0 + 14.0 + PADDING
}

/// Draw the strip into `area`.
///
/// `active` is which of `tabs` is showing, counting within this strip. `pane` is which pane the strip
/// belongs to, which keeps the ids of two strips apart, and `focused` says whether that pane has the
/// keyboard.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    tabs: &[TabView],
    active: usize,
    pane: usize,
    focused: bool,
    opacity: f32,
) -> TabsOutcome {
    let mut outcome = TabsOutcome::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, CornerRadius::ZERO, crate::theme::faded(color::toolbar(), opacity));
    painter.line_segment(
        [Pos2::new(area.left(), area.bottom()), Pos2::new(area.right(), area.bottom())],
        Stroke::new(1.0, color::divider()),
    );

    let widths: Vec<f32> = tabs.iter().map(|tab| tab_width(ui, tab)).collect();
    let offset = shift(&widths, active, area.width());

    // **Over the whole strip, and added first, so it is left with exactly the part no tab wanted.** egui
    // gives a pointer to the *last* widget that asked for it, so a widget added before the tabs is one the
    // tabs take back wherever they are — which is the order `components::dock::handle` documents and the
    // reason a panel's own header can be both a handle and a row of buttons. Two presses here fill the
    // window with the editing area: `task-1771` asks for that at the top of every pane, and this strip is
    // what the editing area has instead of a header.
    let empty = ui.interact(area, ui.id().with(("tab-strip-empty", pane)), Sense::CLICK);
    outcome.twice_on_the_empty_part = empty.double_clicked();

    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    inner.set_clip_rect(ui.painter().clip_rect().intersect(area));

    outcome.strip.area = area;
    let mut pen = area.left() - offset;
    for (index, tab) in tabs.iter().enumerate() {
        let rect = Rect::from_min_size(Pos2::new(pen, area.top()), Vec2::new(widths[index], area.height()));
        pen += widths[index] + GAP;
        outcome.strip.tabs.push(rect);
        if rect.right() < area.left() || rect.left() > area.right() {
            continue;
        }
        draw_tab(&mut inner, rect, tab, Where { pane, index, focused }, index == active, &mut outcome);
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

/// Where one tab is: which strip it is in, where in that strip, and whether the strip has the
/// keyboard. Three values that travel together, so they are one argument rather than three.
#[derive(Clone, Copy)]
struct Where {
    pane: usize,
    index: usize,
    focused: bool,
}

/// One tab: the marker, the name, and either the unsaved dot or the close cross.
fn draw_tab(
    ui: &mut egui::Ui,
    rect: Rect,
    tab: &TabView,
    at: Where,
    active: bool,
    outcome: &mut TabsOutcome,
) {
    let index = at.index;
    let name = format!("Tab: {}", tab.name);
    // A tab senses a drag as well as a click, which is how it is rearranged. egui only calls a press
    // a drag once the pointer has moved far enough, so a click is still a click.
    let response = ui
        .interact(rect, ui.id().with(("file-tab", at.pane, index)), Sense::click_and_drag())
        .on_hover_text(&name);
    if response.dragged() || response.drag_stopped() {
        if let Some(pointer) = response.interact_pointer_pos() {
            outcome.dragging = Some((index, pointer));
            outcome.dropped = response.drag_stopped();
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, CornerRadius::ZERO, color::selected_row());
        // The accent line, quiet in a pane that has not got the keyboard, so which pane is being
        // typed into can be seen at a glance.
        let line = if at.focused { color::accent() } else { color::accent().gamma_multiply(0.35) };
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.bottom() - UNDERLINE),
                Vec2::new(rect.width(), UNDERLINE),
            ),
            CornerRadius::ZERO,
            line,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, color::control());
    }
    // A tab being carried is outlined, so it is clear which one is in the air. The insertion mark
    // that says where it would land is drawn by the window, because only the window knows which
    // strip the pointer has ended up over.
    if response.dragged() {
        painter.rect_filled(rect, CornerRadius::ZERO, color::control());
        painter.rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(3),
            Stroke::new(1.0, color::accent()),
            egui::StrokeKind::Inside,
        );
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

    let tint = if active { color::text_strong() } else { color::text_control() };
    // A transient tab is drawn faintly rather than in italic: egui has no italic face for the
    // family Unluminous installs, and a fake slant is worse than a change of weight.
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
        painter.circle_filled(shut.center(), 3.5, color::unsaved());
    } else {
        let shut_name = format!("Close {}", tab.name);
        let shut_response = ui
            .interact(shut, ui.id().with(("file-tab-close", at.pane, index)), Sense::click())
            .on_hover_text(&shut_name);
        icon::cross(&ui.painter(), shut.center(), color::text_dim());
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
    // A right click opens the tab's own menu. The window shows the tab first, so that every entry in
    // the menu can be about "the tab that is showing" and so be an ordinary action with no argument.
    if response.secondary_clicked() {
        if let Some(at) = response.interact_pointer_pos().or_else(|| response.hover_pos()) {
            outcome.menu = Some((index, at));
        }
    }
}

/// Draw the mark that says where a dragged tab would land: a two point accent line down the gap
/// between two tabs.
///
/// Drawn by the window after every pane, rather than by the strip, for the reason
/// `components::splitter` records about dividers — and because the strip a tab is dropped on is
/// often not the strip it was picked up from, so no one strip can decide where the mark goes.
pub fn insertion_mark(painter: &egui::Painter, strip: &Strip, position: usize) {
    let x = match (strip.tabs.get(position.wrapping_sub(1)), strip.tabs.get(position)) {
        // Between two tabs: in the gap.
        (Some(before), Some(_)) => before.right() + GAP / 2.0,
        // After the last tab.
        (Some(before), None) => before.right() + GAP / 2.0,
        // Before the first, or an empty strip.
        (None, Some(after)) => after.left() - GAP / 2.0,
        (None, None) => strip.area.left() + 2.0,
    };
    let x = x.clamp(strip.area.left() + 1.0, strip.area.right() - 2.0);
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(x - 1.0, strip.area.top() + 3.0), Vec2::new(2.0, strip.area.height() - 6.0)),
        CornerRadius::same(1),
        color::accent(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three 100 point tabs starting at zero, which is what the strip lays out.
    fn strip_of_three() -> Strip {
        Strip {
            area: Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(400.0, HEIGHT)),
            tabs: (0..3)
                .map(|index| {
                    Rect::from_min_size(
                        Pos2::new(index as f32 * 102.0, 0.0),
                        Vec2::new(100.0, HEIGHT),
                    )
                })
                .collect(),
        }
    }

    /// **A tab goes after every tab whose middle the pointer has passed.** That is what makes a
    /// rearrangement follow the pointer instead of jumping when it crosses an edge.
    #[test]
    fn a_drop_lands_after_every_tab_it_has_passed_the_middle_of() {
        let strip = strip_of_three();
        assert_eq!(strip.position_at(0.0), 0, "before the first tab");
        assert_eq!(strip.position_at(49.0), 0, "the left half of the first tab");
        assert_eq!(strip.position_at(51.0), 1, "the right half of the first tab");
        assert_eq!(strip.position_at(153.0), 2, "the right half of the second");
        assert_eq!(strip.position_at(399.0), 3, "past the end of them all");
    }

    /// A strip with nothing in it takes a tab at position zero, which is what dropping into an empty
    /// pane has to mean.
    #[test]
    fn an_empty_strip_takes_a_tab_at_the_start() {
        let empty = Strip { area: strip_of_three().area, tabs: Vec::new() };
        assert_eq!(empty.position_at(200.0), 0);
    }

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
