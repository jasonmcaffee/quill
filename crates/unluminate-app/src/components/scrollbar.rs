//! The thin bar down the right of a document, which says how far through it you are and is dragged
//! to move.
//!
//! `task-1673` asks for one on every document: dark, thin, draggable, and quiet — "more subtle when
//! not hovered over and when not scrolling, slightly more visible when hovering or scrolling". So
//! the bar is always there and it is the **alpha** that changes: an idle thumb is a faint mark that
//! says where you are without competing with the writing, and it comes up to full strength while the
//! page is moving or the pointer is near it, then fades back a moment after the page settles.
//!
//! ## Why it is two calls rather than one
//!
//! Every other control in Unluminate is one function that interacts and draws. This one cannot be,
//! because of the order a frame is settled in:
//!
//! - The **interaction** has to happen straight after the editing area's own, so that the bar wins
//!   the pointer over the text underneath it. egui hands a drag to the last widget that asked for
//!   the point, and the editing area asks for the whole of its rectangle.
//! - The **drawing** has to happen at the end, once the wheel, the caret and the sync between a
//!   source and its preview have all had their say, or the thumb is drawn a frame behind the text it
//!   is describing — which on a fast scroll is plainly visible.
//!
//! So [`grab`] takes the drag and [`paint`] draws, and [`Bar`] is the geometry they share so the two
//! cannot come to different answers about where the thumb is.
//!
//! ## Where it sits
//!
//! [`INSET`] points in from the right of the area it is given, which is exactly what
//! `components::resize_edges` takes from the window's edge and what the activity bar's buttons are
//! inset by. A bar over the window's own resize grip would be a bar that could not be dragged in the
//! one pane that reaches the window's edge; a bar under a pane divider would be the same fault in
//! every other pane. It lands inside the editing area's right hand padding, so no letter is ever
//! drawn underneath it.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Vec2};

use crate::theme::color;

/// How far in from the right edge the bar sits. See the note above about the window's resize grip.
pub const INSET: f32 = 6.0;
/// How wide the invisible strip that takes the pointer is. A thin mark cannot be hit with a mouse,
/// which is the reason `splitter::GRAB` is eight points wide for a one point line.
const GRAB: f32 = 14.0;
/// How wide the thumb is drawn, idle and while it is being pointed at. The second is what "slightly
/// more visible" means besides the alpha: the mark thickens under the pointer the way a divider does.
const THIN: f32 = 5.0;
const THICK: f32 = 8.0;
/// The shortest a thumb may be drawn, so that a very long file still has something to take hold of.
const SHORTEST: f32 = 28.0;
/// How long the bar stays up after the page last moved, and how long it takes to settle back
/// afterwards, in seconds.
const LINGER: f64 = 0.9;
const FADE: f64 = 0.45;

/// Where the bar and its thumb are, worked out from the page and the view.
///
/// Built by [`Bar::new`], which gives `None` when the whole page fits and there is nothing to
/// scroll — that is the one case where no bar should be drawn at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    /// The strip the thumb runs up and down, and the strip that takes the pointer.
    pub track: Rect,
    /// The thumb at the scroll position it was built with.
    pub thumb: Rect,
    /// How much of the page cannot be seen, which is what a full sweep of the track is worth.
    pub overflow: f32,
}

impl Bar {
    /// Where the bar goes for a page `content` tall being looked at through a window `view` tall,
    /// scrolled `scroll` points down. `None` when it all fits.
    pub fn new(area: Rect, scroll: f32, content: f32, view: f32) -> Option<Self> {
        let overflow = content - view;
        if !(overflow > 0.5) || view <= 0.0 || !content.is_finite() {
            return None;
        }
        let track = Rect::from_min_max(
            Pos2::new(area.right() - INSET - GRAB, area.top()),
            Pos2::new(area.right() - INSET, area.bottom()),
        );
        // How tall the thumb is says how much of the page is on the screen, which is the other thing
        // a scrollbar tells you. Never shorter than `SHORTEST`, or a large file gives a thumb too
        // small to take hold of.
        let height = (view / content * track.height()).clamp(SHORTEST.min(track.height()), track.height());
        let travel = (track.height() - height).max(0.0);
        let along = (scroll / overflow).clamp(0.0, 1.0);
        let top = track.top() + travel * along;
        Some(Self {
            track,
            thumb: Rect::from_min_size(Pos2::new(track.left(), top), Vec2::new(track.width(), height)),
            overflow,
        })
    }

    /// The scroll position a thumb whose top edge is at `y` describes.
    fn scroll_at(&self, y: f32) -> f32 {
        let travel = (self.track.height() - self.thumb.height()).max(0.0);
        if travel <= 0.0 {
            return 0.0;
        }
        (((y - self.track.top()) / travel).clamp(0.0, 1.0)) * self.overflow
    }
}

/// What the bar was asked to do this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Grab {
    /// Where the page should be scrolled to, when the bar was dragged or its track was clicked.
    pub scroll: Option<f32>,
    /// The pointer is on the bar or the thumb is being dragged, which is what brings it up to full
    /// strength and what tells the editing area that a wheel over the bar is still about the page.
    pub active: bool,
}

/// Take the drag. Called where the editing area takes its own pointer, so the bar wins the point.
///
/// `id` separates one bar from another in egui's bookkeeping and is what the bar is **named** after,
/// so it has to be unique across the whole window: the caller passes the file's name, and the
/// preview's carries the word after it. Two panes cannot be showing one file, so that is enough —
/// the same answer the gutter's blame cells and a drawn diagram already reached.
pub fn grab(ui: &mut egui::Ui, bar: &Bar, id: &str) -> Grab {
    let name = format!("Scroll {id}");
    let response = ui.interact(bar.track, ui.id().with(("scrollbar", id)), Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, ui.is_enabled(), name.clone())
    });
    let mut outcome = Grab { scroll: None, active: response.hovered() || response.dragged() };
    let held = ui.id().with(("scrollbar-held", id));

    if response.drag_started() {
        // Where on the thumb it was taken hold of, so the thumb does not jump under the pointer.
        // Grabbing the track rather than the thumb takes hold of the middle of it, which is what
        // dragging from a click on the track means everywhere else.
        let offset = response
            .interact_pointer_pos()
            .map(|at| {
                if bar.thumb.contains(at) {
                    at.y - bar.thumb.top()
                } else {
                    bar.thumb.height() / 2.0
                }
            })
            .unwrap_or(0.0);
        ui.memory_mut(|memory| memory.data.insert_temp(held, offset));
    }
    if response.dragged() {
        let offset: f32 = ui.memory(|memory| memory.data.get_temp(held).unwrap_or(0.0));
        if let Some(at) = response.interact_pointer_pos() {
            outcome.scroll = Some(bar.scroll_at(at.y - offset));
        }
    } else if response.clicked() {
        // A click on the track alone jumps there. A click on the thumb is the end of a drag that
        // moved nothing, so it must not move anything either.
        if let Some(at) = response.interact_pointer_pos() {
            if !bar.thumb.contains(at) {
                outcome.scroll = Some(bar.scroll_at(at.y - bar.thumb.height() / 2.0));
            }
        }
    }
    outcome
}

/// Draw the thumb, after the text, at the position the frame settled on.
///
/// `active` is [`Grab::active`] or the page having moved this frame — either is what "hovering or
/// scrolling" means in the ask.
pub fn paint(ui: &egui::Ui, bar: &Bar, id: &str, active: bool) {
    let strength = strength(ui, id, active);
    let painter = ui.painter();
    // The track behind the thumb, which fades in with it. Left there when the bar is quiet it would
    // be a second line down the right hand edge of every document for no reason.
    if strength > 0.01 {
        painter.rect_filled(
            bar.track.shrink2(Vec2::new((GRAB - THICK) / 2.0, 0.0)),
            CornerRadius::same(4),
            color::divider().gamma_multiply(strength * 0.6),
        );
    }
    // Quiet, the thumb is a control's grey and as thin as a mark can be and still be seen; used, it
    // is the colour of a heading in the explorer and a little wider. The two ends are palette
    // colours rather than a shade of their own, and everything between them is the fade.
    let thumb = Rect::from_center_size(
        bar.thumb.center(),
        Vec2::new(THIN + (THICK - THIN) * strength, bar.thumb.height()),
    );
    painter.rect_filled(thumb, CornerRadius::same(4), mix(color::control(), color::text_dim(), strength));
}

/// A colour part of the way between two, which is how the thumb fades.
fn mix(from: Color32, to: Color32, along: f32) -> Color32 {
    let along = along.clamp(0.0, 1.0);
    let each = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * along).round() as u8;
    Color32::from_rgb(each(from.r(), to.r()), each(from.g(), to.g()), each(from.b(), to.b()))
}

/// How far up the bar is: 1 while it is being used, 0 once it has settled, and on its way between
/// the two for [`FADE`] seconds after [`LINGER`] has passed.
///
/// The moment the bar was last used lives in egui's memory under the bar's id, which is where
/// `components::modal` already keeps where a dialog was dragged to: it is a fact about how this
/// window is being used at this moment rather than anything the document or the settings should
/// carry, and nothing is written to disk for it.
fn strength(ui: &egui::Ui, id: &str, active: bool) -> f32 {
    let key = ui.id().with(("scrollbar-seen", id));
    let now = ui.input(|input| input.time);
    let until = if active {
        let until = now + LINGER;
        ui.memory_mut(|memory| memory.data.insert_temp(key, until));
        until
    } else {
        ui.memory(|memory| memory.data.get_temp::<f64>(key).unwrap_or(f64::NEG_INFINITY))
    };
    if now <= until {
        return 1.0;
    }
    let over = now - until;
    if over >= FADE {
        return 0.0;
    }
    // An idle window draws nothing, so the last of the fade would sit there unfinished until
    // something else woke the window. The same repaint `set_the_font_everywhere` asks for.
    ui.ctx().request_repaint();
    1.0 - (over / FADE) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(400.0, 200.0))
    }

    /// **Nothing to scroll, nothing to draw.** A page that fits has no bar at all.
    #[test]
    fn a_page_that_fits_has_no_bar() {
        assert_eq!(Bar::new(area(), 0.0, 150.0, 200.0), None);
        assert_eq!(Bar::new(area(), 0.0, 200.0, 200.0), None);
    }

    /// The bar sits inside the right edge by exactly what the window's own resize grip takes, so the
    /// two never want the same point.
    #[test]
    fn the_bar_clears_the_windows_resize_grip() {
        let bar = Bar::new(area(), 0.0, 1000.0, 200.0).expect("there is something to scroll");
        assert_eq!(bar.track.right(), area().right() - INSET);
        assert_eq!(INSET, crate::components::resize_edges::EDGE);
    }

    /// The thumb starts at the top, ends at the bottom, and says how much of the page is showing.
    #[test]
    fn the_thumb_runs_the_whole_track() {
        let top = Bar::new(area(), 0.0, 1000.0, 200.0).expect("a bar");
        assert_eq!(top.thumb.top(), top.track.top());
        let bottom = Bar::new(area(), 800.0, 1000.0, 200.0).expect("a bar");
        assert!(
            (bottom.thumb.bottom() - bottom.track.bottom()).abs() < 0.01,
            "scrolled to the end the thumb should reach the bottom, it is at {}",
            bottom.thumb.bottom()
        );
        // A fifth of the page is showing, so the thumb is a fifth of the track.
        assert!((top.thumb.height() - 40.0).abs() < 0.01, "{}", top.thumb.height());
    }

    /// A very long file still gives a thumb that can be taken hold of.
    #[test]
    fn a_very_long_file_still_has_a_thumb_to_hold() {
        let bar = Bar::new(area(), 0.0, 500_000.0, 200.0).expect("a bar");
        assert!(bar.thumb.height() >= SHORTEST, "{}", bar.thumb.height());
    }

    /// Dragging the thumb to a position and reading the scroll back gives the same place.
    #[test]
    fn the_thumb_and_the_scroll_position_agree() {
        for scroll in [0.0, 100.0, 400.0, 800.0] {
            let bar = Bar::new(area(), scroll, 1000.0, 200.0).expect("a bar");
            let back = bar.scroll_at(bar.thumb.top());
            assert!((back - scroll).abs() < 0.01, "{scroll} came back as {back}");
        }
    }

    /// Past either end it is clamped rather than run off the page.
    #[test]
    fn a_thumb_dragged_past_the_end_is_clamped() {
        let bar = Bar::new(area(), 0.0, 1000.0, 200.0).expect("a bar");
        assert_eq!(bar.scroll_at(-500.0), 0.0);
        assert_eq!(bar.scroll_at(5000.0), bar.overflow);
    }
}
