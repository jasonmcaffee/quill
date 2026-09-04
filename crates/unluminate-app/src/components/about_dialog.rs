//! The About box: who wrote Unluminate, what version this is, and when it was built.
//!
//! `Unluminate -> About Unluminate` used to write one line into the status bar, which is gone as soon as
//! anything else writes there and carried no build date. `task-1667` asks for the window every other
//! application answers that menu entry with, and for the build date on it, so that two builds of one
//! version can be told apart — which during a week of tasks is the ordinary case.
//!
//! It is built from `components::modal` like the other nine, so it is dragged by its header, resized
//! by its eight grips, put back in the middle by a double click and closed by Escape without any of
//! that being written here.
//!
//! **The two values are passed in rather than read from `build_info` inside the drawing.** A
//! component that reached for the compiled-in stamp could not be screenshot tested: the picture
//! would differ from the accepted one every time the binary was rebuilt, which is a test that fails
//! for a reason that is not a fault. [`About::current`] is what the window calls; a test passes a
//! date of its own.

use egui::{Pos2, Rect, Sense, Vec2};

use crate::build_info;
use crate::components::modal;
use crate::theme::color;

/// The size the box asks for. Three lines and a button need no more, and the height is what leaves
/// the same room under the last line as `modal::body` leaves over the first: a modal that is mostly
/// empty looks like one that failed to load.
const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 208.0;

/// How far apart the three lines sit.
const LINE: f32 = 26.0;

/// What the About box says. Text rather than the constants themselves, so a test can fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct About {
    pub developer: String,
    pub version: String,
    pub built: String,
}

impl About {
    /// What this build of Unluminate is, from the one place either fact is read.
    pub fn current() -> Self {
        Self {
            developer: build_info::DEVELOPER.to_owned(),
            version: build_info::VERSION.to_owned(),
            built: build_info::BUILD_DATE.to_owned(),
        }
    }

    /// The three lines, in the order they are read. The first has no label of its own, which is why
    /// this is a list of pairs rather than a table with a heading.
    fn lines(&self) -> [(&'static str, &str); 3] {
        [("Developed by", self.developer.as_str()), ("Version:", &self.version), ("Build Date:", &self.built)]
    }
}

/// Draw the About box. Returns true when it was closed, by the button, the cross or Escape.
///
/// The caller owns whether there is one at all, which is the shape `go_to_file` and `find_in_files`
/// already have: the window takes it, draws it, and puts it back unless it closed.
pub fn show(ctx: &egui::Context, about: &About) -> bool {
    let (pressed, closed) = modal::show(ctx, "unluminate-about", WIDTH, HEIGHT, |ui, area| {
        let crossed = modal::header(ui, area, "About Unluminate");
        let body = modal::body(area);
        for (index, (label, value)) in about.lines().iter().enumerate() {
            line(ui, body, index, label, value);
        }
        // `Done` rather than `Close`, for the reason the Settings window's footer says `Done`: the
        // header already draws a `Close` cross, and `design/style-guide.md` forbids two controls in
        // one window sharing a name.
        let pressed = modal::footer(ui, area, &[("Done", true)]).is_some();
        pressed || crossed
    });
    pressed || closed
}

/// One line of the box: its label in the ordinary control colour, its value in the strong one.
///
/// The whole line is a named control rather than two, because what a person reads is the sentence
/// and it is the sentence a test should be able to ask for. `Sense::hover` because there is nothing
/// to click: the name is for the screenshot tests and for assistive technology, and every control in
/// Unluminate has one.
fn line(ui: &mut egui::Ui, body: Rect, index: usize, label: &str, value: &str) {
    let row = Rect::from_min_size(
        Pos2::new(body.left(), body.top() + index as f32 * LINE),
        Vec2::new(body.width(), LINE),
    );
    let after = modal::label(ui.painter(), row, row.left(), label, color::text_control(), 12.5);
    // Nothing follows `Developed by` but the name, so the two are one sentence with one space in it;
    // the other two have a colon of their own and read as a label and a value.
    modal::label(ui.painter(), row, after + 6.0, value, color::text_strong(), 12.5);

    let name = format!("{label} {value}");
    let response = ui.interact(row, ui.id().with(("about-line", index)), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), name.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_lines_are_the_ones_the_ticket_asks_for() {
        let about = About {
            developer: "Jason McAffee".to_owned(),
            version: "0.2.0".to_owned(),
            built: "2026-08-25 10:45pm".to_owned(),
        };
        let read: Vec<String> =
            about.lines().iter().map(|(label, value)| format!("{label} {value}")).collect();
        assert_eq!(
            read,
            vec![
                "Developed by Jason McAffee".to_owned(),
                "Version: 0.2.0".to_owned(),
                "Build Date: 2026-08-25 10:45pm".to_owned(),
            ]
        );
    }

    #[test]
    fn what_the_window_shows_comes_from_the_build() {
        let about = About::current();
        assert_eq!(about.version, build_info::VERSION);
        assert_eq!(about.built, build_info::BUILD_DATE);
        assert_eq!(about.developer, "Jason McAffee");
    }
}
