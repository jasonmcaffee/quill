//! Telling a program about the mouse.
//!
//! A program that has asked for mouse reporting, which `claude` does, expects a click inside it to arrive
//! as an escape sequence rather than selecting text. Two encodings are in use and both are here, because a
//! program chooses which it wants: the older one from X10, which cannot describe a column past 223, and
//! SGR, which can and which every recent program asks for.
//!
//! Holding shift always selects locally instead of reporting, which is the convention every terminal
//! follows and the only way to copy out of a program that has taken over the mouse. That decision is the
//! window's; this module only encodes.

/// What the program has asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseMode {
    /// Report a press and a release.
    pub report_click: bool,
    /// Report movement while a button is held.
    pub drag: bool,
    /// Report movement even with no button held.
    pub motion: bool,
    /// Use the SGR encoding rather than the older one.
    pub sgr: bool,
}

impl MouseMode {
    /// True when a click should be reported rather than starting a selection.
    pub fn reports_clicks(&self) -> bool {
        self.report_click || self.drag || self.motion
    }
}

/// Which button, as a terminal numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

impl Button {
    fn number(self) -> u8 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
            // The wheel is reported as buttons 64 and 65.
            Button::WheelUp => 64,
            Button::WheelDown => 65,
        }
    }
}

/// The modifiers a mouse report carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

impl Modifiers {
    fn bits(&self) -> u8 {
        let mut bits = 0;
        if self.shift {
            bits |= 4;
        }
        if self.alt {
            bits |= 8;
        }
        if self.control {
            bits |= 16;
        }
        bits
    }
}

/// A press, a release, or movement with a button held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Press,
    Release,
    Drag,
}

/// The bytes that tell the program about one mouse event.
///
/// `row` and `column` are counted from zero, as the grid counts them; the sequences count from one.
/// Returns nothing when the program has not asked to be told, or when the position cannot be expressed in
/// the encoding it asked for.
pub fn report(
    mode: MouseMode,
    kind: Kind,
    button: Button,
    row: usize,
    column: usize,
    modifiers: Modifiers,
) -> Option<Vec<u8>> {
    if !mode.reports_clicks() {
        return None;
    }
    if kind == Kind::Drag && !(mode.drag || mode.motion) {
        return None;
    }
    let wheel = matches!(button, Button::WheelUp | Button::WheelDown);
    // The wheel has no release, and reporting one would look like a second turn.
    if wheel && kind != Kind::Press {
        return None;
    }
    let mut code = button.number() + modifiers.bits();
    if kind == Kind::Drag {
        code += 32;
    }
    let (row, column) = (row + 1, column + 1);

    if mode.sgr {
        let last = if kind == Kind::Release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{column};{row}{last}").into_bytes());
    }

    // The older encoding adds 32 to everything and puts it in one byte each, so a column past 223 cannot be
    // said at all. Nothing is sent rather than a position that is wrong.
    if row + 32 > 255 || column + 32 > 255 {
        return None;
    }
    let button_byte = if kind == Kind::Release { 3 + modifiers.bits() } else { code };
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + button_byte,
        (32 + column) as u8,
        (32 + row) as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sgr() -> MouseMode {
        MouseMode { report_click: true, drag: true, motion: false, sgr: true }
    }

    fn text(bytes: Vec<u8>) -> String {
        String::from_utf8_lossy(&bytes).replace('\x1b', "ESC ")
    }

    #[test]
    fn nothing_is_sent_when_the_program_has_not_asked() {
        let quiet = MouseMode::default();
        assert_eq!(
            report(quiet, Kind::Press, Button::Left, 0, 0, Modifiers::default()),
            None,
            "a click in a program that has not asked about the mouse is for selecting text"
        );
    }

    #[test]
    fn a_press_and_a_release_are_told_apart_by_the_last_letter() {
        let press = report(sgr(), Kind::Press, Button::Left, 4, 9, Modifiers::default()).expect("a press");
        assert_eq!(text(press), "ESC [<0;10;5M");
        let release =
            report(sgr(), Kind::Release, Button::Left, 4, 9, Modifiers::default()).expect("a release");
        assert_eq!(text(release), "ESC [<0;10;5m", "a small m is a release");
    }

    #[test]
    fn the_three_buttons_are_numbered_from_zero() {
        let at = |button| {
            text(report(sgr(), Kind::Press, button, 0, 0, Modifiers::default()).expect("a press"))
        };
        assert_eq!(at(Button::Left), "ESC [<0;1;1M");
        assert_eq!(at(Button::Middle), "ESC [<1;1;1M");
        assert_eq!(at(Button::Right), "ESC [<2;1;1M");
    }

    #[test]
    fn the_wheel_is_reported_as_the_high_numbered_buttons_and_has_no_release() {
        let up = report(sgr(), Kind::Press, Button::WheelUp, 0, 0, Modifiers::default()).expect("a turn");
        assert_eq!(text(up), "ESC [<64;1;1M");
        let down =
            report(sgr(), Kind::Press, Button::WheelDown, 0, 0, Modifiers::default()).expect("a turn");
        assert_eq!(text(down), "ESC [<65;1;1M");
        assert_eq!(
            report(sgr(), Kind::Release, Button::WheelUp, 0, 0, Modifiers::default()),
            None,
            "a wheel release would look like a second turn"
        );
    }

    #[test]
    fn a_drag_adds_thirty_two_and_is_only_sent_when_the_program_asked_for_movement() {
        let dragging = report(sgr(), Kind::Drag, Button::Left, 1, 1, Modifiers::default()).expect("a drag");
        assert_eq!(text(dragging), "ESC [<32;2;2M");
        let clicks_only = MouseMode { report_click: true, drag: false, motion: false, sgr: true };
        assert_eq!(report(clicks_only, Kind::Drag, Button::Left, 1, 1, Modifiers::default()), None);
    }

    #[test]
    fn the_modifiers_are_added_to_the_button() {
        let control = Modifiers { control: true, ..Modifiers::default() };
        let bytes = report(sgr(), Kind::Press, Button::Left, 0, 0, control).expect("a press");
        assert_eq!(text(bytes), "ESC [<16;1;1M", "control adds sixteen");
        let both = Modifiers { control: true, shift: true, alt: false };
        let bytes = report(sgr(), Kind::Press, Button::Left, 0, 0, both).expect("a press");
        assert_eq!(text(bytes), "ESC [<20;1;1M");
    }

    #[test]
    fn the_older_encoding_puts_the_position_in_one_byte_each() {
        let old = MouseMode { report_click: true, drag: false, motion: false, sgr: false };
        let bytes = report(old, Kind::Press, Button::Left, 4, 9, Modifiers::default()).expect("a press");
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 32, 32 + 10, 32 + 5]);
    }

    #[test]
    fn the_older_encoding_says_nothing_rather_than_something_wrong_past_its_limit() {
        let old = MouseMode { report_click: true, drag: false, motion: false, sgr: false };
        assert_eq!(report(old, Kind::Press, Button::Left, 0, 300, Modifiers::default()), None);
        // The same click in SGR is fine, because it counts in numbers rather than in bytes.
        let bytes = report(sgr(), Kind::Press, Button::Left, 0, 300, Modifiers::default()).expect("a press");
        assert_eq!(text(bytes), "ESC [<0;301;1M");
    }
}
