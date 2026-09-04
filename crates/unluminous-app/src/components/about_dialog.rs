//! The About box: who wrote Unluminous, what version this is, and when it was built.
//!
//! `Unluminous -> About Unluminous` used to write one line into the status bar, which is gone as soon as
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

/// The size the box asks for. Four lines and two buttons need no more, and the height is what leaves
/// the same room under the last line as `modal::body` leaves over the first: a modal that is mostly
/// empty looks like one that failed to load.
const WIDTH: f32 = 440.0;
const HEIGHT: f32 = 234.0;

/// How far apart the lines sit.
const LINE: f32 = 26.0;

/// What the About box says. Text rather than the constants themselves, so a test can fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct About {
    pub developer: String,
    pub version: String,
    pub built: String,
    /// What the last check for a newer release came to, when one has been asked for.
    ///
    /// **The box does not ask on its own**, which is `services::update`'s rule: it shows what a
    /// check found if there has been one, and offers the button either way. `task-1804` §6.
    pub update: Option<String>,
}

impl About {
    /// What this build of Unluminous is, from the one place either fact is read.
    pub fn current() -> Self {
        Self {
            developer: build_info::DEVELOPER.to_owned(),
            version: build_info::VERSION.to_owned(),
            built: build_info::BUILD_DATE.to_owned(),
            update: None,
        }
    }

    /// The same, saying what a check found.
    pub fn with_update(self, update: Option<String>) -> Self {
        Self { update, ..self }
    }

    /// The lines, in the order they are read. The first has no label of its own, which is why this
    /// is a list of pairs rather than a table with a heading.
    ///
    /// The fourth is there only once a check has been made, because a row reading `Updates: —` says
    /// nothing and a row that is absent says the same thing more quietly.
    fn lines(&self) -> Vec<(&'static str, &str)> {
        let mut lines = vec![
            ("Developed by", self.developer.as_str()),
            ("Version:", self.version.as_str()),
            ("Build Date:", self.built.as_str()),
        ];
        if let Some(update) = self.update.as_deref() {
            lines.push(("Updates:", update));
        }
        lines
    }
}

/// Draw the About box. Returns true when it was closed, by the button, the cross or Escape.
///
/// The caller owns whether there is one at all, which is the shape `go_to_file` and `find_in_files`
/// already have: the window takes it, draws it, and puts it back unless it closed.
pub fn show(ctx: &egui::Context, about: &About) -> Outcome {
    let mut outcome = Outcome::default();
    let (closing, closed) = modal::show(ctx, "unluminous-about", WIDTH, HEIGHT, |ui, area| {
        let crossed = modal::header(ui, area, "About Unluminous");
        let body = modal::body(area);
        for (index, (label, value)) in about.lines().iter().enumerate() {
            line(ui, body, index, label, value);
        }
        // `Done` rather than `Close`, for the reason the Settings window's footer says `Done`: the
        // header already draws a `Close` cross, and `design/style-guide.md` forbids two controls in
        // one window sharing a name.
        //
        // `Check for Updates` is beside it rather than on a line of its own, because it is the one
        // thing this box *does* and the footer is where a modal keeps what it does. It is the same
        // entry as the one on the application menu, reaching the same code -- there is one place a
        // check starts, which is `UnluminousApp::check_for_updates`.
        // **`Done` last**, because `modal::footer` gives the keyboard to the last button -- *"the
        // one that does the thing"* -- and the thing an About box does is close. Putting the check
        // last took Enter away from Done, which is what `enter_presses_the_button_that_does_the_thing`
        // is there to notice, and it noticed.
        match modal::footer(ui, area, &[("Check for Updates", true), ("Done", true)]) {
            Some(0) => {
                outcome.check = true;
                false
            }
            Some(1) => true,
            _ => crossed,
        }
    });
    outcome.close = closing || closed;
    outcome
}

/// What the About box asked for this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Put the box away.
    pub close: bool,
    /// Ask the releases page whether there is a newer Unluminous.
    pub check: bool,
}

/// One line of the box: its label in the ordinary control colour, its value in the strong one.
///
/// The whole line is a named control rather than two, because what a person reads is the sentence
/// and it is the sentence a test should be able to ask for. `Sense::hover` because there is nothing
/// to click: the name is for the screenshot tests and for assistive technology, and every control in
/// Unluminous has one.
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

    fn an_about(update: Option<&str>) -> About {
        About {
            developer: "Jason McAffee".to_owned(),
            version: "0.2.0".to_owned(),
            built: "2026-08-25 10:45pm".to_owned(),
            update: update.map(str::to_owned),
        }
    }

    fn read(about: &About) -> Vec<String> {
        about.lines().iter().map(|(label, value)| format!("{label} {value}")).collect()
    }

    #[test]
    fn the_three_lines_are_the_ones_the_ticket_asks_for() {
        assert_eq!(
            read(&an_about(None)),
            vec![
                "Developed by Jason McAffee".to_owned(),
                "Version: 0.2.0".to_owned(),
                "Build Date: 2026-08-25 10:45pm".to_owned(),
            ]
        );
    }

    /// The fourth line arrives only once a check has been made, and never before.
    ///
    /// `task-1804` §6: the box does not ask on its own, and a row reading `Updates:` with
    /// nothing after it would say it had asked and found nothing.
    #[test]
    fn the_update_line_is_absent_until_something_has_been_asked() {
        assert_eq!(read(&an_about(None)).len(), 3, "nothing has been asked");
        assert_eq!(
            read(&an_about(Some("0.35.0 is available"))),
            vec![
                "Developed by Jason McAffee".to_owned(),
                "Version: 0.2.0".to_owned(),
                "Build Date: 2026-08-25 10:45pm".to_owned(),
                "Updates: 0.35.0 is available".to_owned(),
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
