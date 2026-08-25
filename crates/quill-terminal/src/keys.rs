//! Turning a key press into the bytes a terminal sends.
//!
//! This is the table in section 9 of `tasks/quill-terminal-tdd.md`, and the sequences are the ones xterm
//! defines and every terminal follows. It is written here rather than taken from a crate because it is the
//! part most likely to need changing for Quill, and because a table of expected bytes is the easiest thing
//! in the whole terminal to test.
//!
//! Two modes change what some keys send, and both are set by the program on the far side rather than chosen
//! here. Application cursor keys turn the arrows and Home and End from `ESC [ A` into `ESC O A`, which is
//! what a full screen program asks for so it can tell an arrow key from a program printing the same
//! characters. Bracketed paste wraps pasted text so that a shell does not run each pasted line as it
//! arrives.
//!
//! There is no dependency on egui here. The window turns egui's key and modifier types into [`Key`] and
//! [`Modifiers`], so this crate stays free of any user interface.

/// A key, named the way a terminal thinks of keys rather than the way a keyboard is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// A function key, from 1 to 20.
    Function(u8),
    /// An ordinary key, held as the character it produces with no modifiers, in lower case.
    Character(char),
}

/// Which modifiers were held down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
    /// The Apple key. Not a terminal modifier: it belongs to the application, so a key press holding it
    /// sends nothing.
    pub command: bool,
}

impl Modifiers {
    pub const NONE: Self = Self { shift: false, alt: false, control: false, command: false };

    pub const fn control() -> Self {
        Self { control: true, ..Self::NONE }
    }

    pub const fn shift() -> Self {
        Self { shift: true, ..Self::NONE }
    }

    pub const fn alt() -> Self {
        Self { alt: true, ..Self::NONE }
    }

    /// The number xterm uses for a set of modifiers in a sequence such as `ESC [ 1 ; 5 D`.
    ///
    /// One plus one for shift, two for alt and four for control, which is why shift and control together is
    /// six and all three is eight.
    fn parameter(&self) -> Option<u8> {
        let mut value = 1;
        if self.shift {
            value += 1;
        }
        if self.alt {
            value += 2;
        }
        if self.control {
            value += 4;
        }
        (value > 1).then_some(value)
    }
}

/// What the program on the far side has asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mode {
    /// The arrows and Home and End send `ESC O` sequences instead of `ESC [` ones.
    pub application_cursor: bool,
    /// Pasted text is wrapped in `ESC [ 200 ~` and `ESC [ 201 ~`.
    pub bracketed_paste: bool,
}

/// One key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub modifiers: Modifiers,
}

impl KeyPress {
    pub const fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    pub const fn plain(key: Key) -> Self {
        Self { key, modifiers: Modifiers::NONE }
    }
}

/// The bytes a key press sends, or nothing when the key sends nothing.
pub fn encode(press: KeyPress, mode: Mode) -> Option<Vec<u8>> {
    let KeyPress { key, modifiers } = press;
    // The Apple key belongs to the application: command and C copies, command and V pastes, and nothing
    // holding it reaches the shell.
    if modifiers.command {
        return None;
    }
    let parameter = modifiers.parameter();

    let bytes = match key {
        // A carriage return, not a line feed. A terminal sends what the Return key on a serial terminal
        // sent, and the program decides what a line ending means.
        Key::Enter => single(0x0d, modifiers),
        // Delete rather than backspace, which is what every terminal sends and what readline expects.
        Key::Backspace => single(0x7f, modifiers),
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => single(0x09, modifiers),
        Key::Escape => single(0x1b, modifiers),

        Key::Up | Key::Down | Key::Right | Key::Left | Key::Home | Key::End => {
            let last = match key {
                Key::Up => b'A',
                Key::Down => b'B',
                Key::Right => b'C',
                Key::Left => b'D',
                Key::Home => b'H',
                _ => b'F',
            };
            match parameter {
                // A modified key always uses the `ESC [ 1 ; n X` form, whatever the cursor key mode is,
                // because the `ESC O` form has nowhere to put the modifier.
                Some(parameter) => format!("\x1b[1;{parameter}{}", last as char).into_bytes(),
                None if mode.application_cursor => vec![0x1b, b'O', last],
                None => vec![0x1b, b'[', last],
            }
        }

        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => {
            let number = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                _ => 6,
            };
            match parameter {
                Some(parameter) => format!("\x1b[{number};{parameter}~").into_bytes(),
                None => format!("\x1b[{number}~").into_bytes(),
            }
        }

        // The first four function keys are the older `ESC O` form and the rest are numbered, with the gaps
        // at 16, 22, 27 and 30 that the standard leaves.
        Key::Function(number) => {
            let numbered = |value: u8| match parameter {
                Some(parameter) => format!("\x1b[{value};{parameter}~").into_bytes(),
                None => format!("\x1b[{value}~").into_bytes(),
            };
            match number {
                1..=4 => {
                    let last = [b'P', b'Q', b'R', b'S'][number as usize - 1];
                    match parameter {
                        Some(parameter) => {
                            format!("\x1b[1;{parameter}{}", last as char).into_bytes()
                        }
                        None => vec![0x1b, b'O', last],
                    }
                }
                5 => numbered(15),
                6..=10 => numbered(17 + number - 6),
                11..=14 => numbered(23 + number - 11),
                15..=16 => numbered(28 + number - 15),
                17..=20 => numbered(31 + number - 17),
                _ => return None,
            }
        }

        Key::Character(character) => return character_bytes(character, modifiers),
    };
    Some(bytes)
}

/// A control character, with `ESC` in front of it when alt was held, which is how a shell reads alt.
fn single(byte: u8, modifiers: Modifiers) -> Vec<u8> {
    if modifiers.alt {
        vec![0x1b, byte]
    } else {
        vec![byte]
    }
}

/// An ordinary key, which is the interesting case because control turns a letter into a control code.
fn character_bytes(character: char, modifiers: Modifiers) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if modifiers.control {
        let control = control_code(character)?;
        if modifiers.alt {
            out.push(0x1b);
        }
        out.push(control);
        return Some(out);
    }
    // Without control, the character itself. The window sends the text egui reports rather than working the
    // letter out from the key, so that a keyboard layout, a dead key or an input method all work; this path
    // is for a key press with alt held, which egui reports as a key rather than as text.
    if modifiers.alt {
        out.push(0x1b);
    }
    let mut buffer = [0_u8; 4];
    let text = if modifiers.shift {
        character.to_uppercase().next().unwrap_or(character)
    } else {
        character
    };
    out.extend_from_slice(text.encode_utf8(&mut buffer).as_bytes());
    Some(out)
}

/// The control code a letter or a symbol makes when control is held.
///
/// A to Z are 1 to 26, which is why control and C is 3 and control and D is 4. The five symbols after Z
/// carry on from 27, and control and space is a zero byte, which is how a program is sent a null.
fn control_code(character: char) -> Option<u8> {
    let character = character.to_ascii_lowercase();
    match character {
        'a'..='z' => Some(character as u8 - b'a' + 1),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '/' => Some(0x1f),
        ' ' | '2' | '@' => Some(0x00),
        '3' => Some(0x1b),
        '4' => Some(0x1c),
        '5' => Some(0x1d),
        '7' | '8' => Some(0x7f),
        _ => None,
    }
}

/// The bytes for text arriving from the clipboard.
///
/// Wrapped in the bracketed paste sequences when the program asked for them, so that a shell takes several
/// pasted lines as text to edit rather than as commands to run one after another. Carriage returns are
/// turned into line feeds first, because a terminal treats a carriage return as Return being pressed and
/// pasted text with both would run every line twice.
pub fn paste(text: &str, mode: Mode) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.bracketed_paste {
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.into_bytes()
    }
}

/// The bytes that tell a program the terminal has gained or lost the keyboard, when it asked to be told.
pub fn focus(gained: bool) -> Vec<u8> {
    if gained {
        b"\x1b[I".to_vec()
    } else {
        b"\x1b[O".to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(key: Key, modifiers: Modifiers, mode: Mode) -> Vec<u8> {
        encode(KeyPress::new(key, modifiers), mode).unwrap_or_else(|| panic!("{key:?} should send something"))
    }

    fn text(key: Key, modifiers: Modifiers, mode: Mode) -> String {
        String::from_utf8_lossy(&bytes(key, modifiers, mode)).replace('\x1b', "ESC ")
    }

    #[test]
    fn return_sends_a_carriage_return_and_backspace_sends_delete() {
        assert_eq!(bytes(Key::Enter, Modifiers::NONE, Mode::default()), vec![0x0d]);
        assert_eq!(bytes(Key::Backspace, Modifiers::NONE, Mode::default()), vec![0x7f]);
        assert_eq!(bytes(Key::Tab, Modifiers::NONE, Mode::default()), vec![0x09]);
        assert_eq!(bytes(Key::Escape, Modifiers::NONE, Mode::default()), vec![0x1b]);
    }

    #[test]
    fn alt_and_backspace_puts_escape_in_front_of_it() {
        assert_eq!(bytes(Key::Backspace, Modifiers::alt(), Mode::default()), vec![0x1b, 0x7f]);
    }

    #[test]
    fn shift_and_tab_sends_a_back_tab() {
        assert_eq!(text(Key::Tab, Modifiers::shift(), Mode::default()), "ESC [Z");
    }

    #[test]
    fn the_arrows_are_the_ordinary_sequences() {
        let mode = Mode::default();
        assert_eq!(text(Key::Up, Modifiers::NONE, mode), "ESC [A");
        assert_eq!(text(Key::Down, Modifiers::NONE, mode), "ESC [B");
        assert_eq!(text(Key::Right, Modifiers::NONE, mode), "ESC [C");
        assert_eq!(text(Key::Left, Modifiers::NONE, mode), "ESC [D");
        assert_eq!(text(Key::Home, Modifiers::NONE, mode), "ESC [H");
        assert_eq!(text(Key::End, Modifiers::NONE, mode), "ESC [F");
    }

    #[test]
    fn application_cursor_keys_turn_the_bracket_into_an_o() {
        let mode = Mode { application_cursor: true, ..Mode::default() };
        assert_eq!(text(Key::Up, Modifiers::NONE, mode), "ESC OA");
        assert_eq!(text(Key::Left, Modifiers::NONE, mode), "ESC OD");
        assert_eq!(text(Key::Home, Modifiers::NONE, mode), "ESC OH");
        assert_eq!(text(Key::End, Modifiers::NONE, mode), "ESC OF");
    }

    #[test]
    fn a_modified_arrow_carries_the_modifier_as_a_number() {
        let mode = Mode::default();
        assert_eq!(text(Key::Left, Modifiers::shift(), mode), "ESC [1;2D");
        assert_eq!(text(Key::Left, Modifiers::alt(), mode), "ESC [1;3D");
        assert_eq!(text(Key::Left, Modifiers::control(), mode), "ESC [1;5D");
        let shift_control = Modifiers { shift: true, control: true, ..Modifiers::NONE };
        assert_eq!(text(Key::Left, shift_control, mode), "ESC [1;6D");
        let all = Modifiers { shift: true, control: true, alt: true, command: false };
        assert_eq!(text(Key::Left, all, mode), "ESC [1;8D");
    }

    #[test]
    fn a_modified_arrow_keeps_the_bracket_form_even_in_application_cursor_keys() {
        // There is nowhere to put the modifier in the `ESC O` form, so it is not used.
        let mode = Mode { application_cursor: true, ..Mode::default() };
        assert_eq!(text(Key::Left, Modifiers::control(), mode), "ESC [1;5D");
    }

    #[test]
    fn the_tilde_keys_are_numbered() {
        let mode = Mode::default();
        assert_eq!(text(Key::Insert, Modifiers::NONE, mode), "ESC [2~");
        assert_eq!(text(Key::Delete, Modifiers::NONE, mode), "ESC [3~");
        assert_eq!(text(Key::PageUp, Modifiers::NONE, mode), "ESC [5~");
        assert_eq!(text(Key::PageDown, Modifiers::NONE, mode), "ESC [6~");
        assert_eq!(text(Key::Delete, Modifiers::control(), mode), "ESC [3;5~");
    }

    #[test]
    fn the_function_keys_follow_the_standard_including_its_gaps() {
        let mode = Mode::default();
        assert_eq!(text(Key::Function(1), Modifiers::NONE, mode), "ESC OP");
        assert_eq!(text(Key::Function(4), Modifiers::NONE, mode), "ESC OS");
        assert_eq!(text(Key::Function(5), Modifiers::NONE, mode), "ESC [15~");
        assert_eq!(text(Key::Function(6), Modifiers::NONE, mode), "ESC [17~");
        assert_eq!(text(Key::Function(10), Modifiers::NONE, mode), "ESC [21~");
        assert_eq!(text(Key::Function(11), Modifiers::NONE, mode), "ESC [23~");
        assert_eq!(text(Key::Function(12), Modifiers::NONE, mode), "ESC [24~");
        assert_eq!(encode(KeyPress::plain(Key::Function(30)), mode), None, "there is no F30");
    }

    #[test]
    fn control_and_a_letter_is_the_control_code() {
        let mode = Mode::default();
        assert_eq!(bytes(Key::Character('c'), Modifiers::control(), mode), vec![0x03], "Control C");
        assert_eq!(bytes(Key::Character('d'), Modifiers::control(), mode), vec![0x04], "Control D");
        assert_eq!(bytes(Key::Character('a'), Modifiers::control(), mode), vec![0x01]);
        assert_eq!(bytes(Key::Character('z'), Modifiers::control(), mode), vec![0x1a]);
        // Held with shift as well, which is what happens on a keyboard with caps lock on.
        let shift_control = Modifiers { shift: true, control: true, ..Modifiers::NONE };
        assert_eq!(bytes(Key::Character('C'), shift_control, mode), vec![0x03]);
    }

    #[test]
    fn control_and_a_symbol_is_the_code_after_z() {
        let mode = Mode::default();
        assert_eq!(bytes(Key::Character('['), Modifiers::control(), mode), vec![0x1b]);
        assert_eq!(bytes(Key::Character('\\'), Modifiers::control(), mode), vec![0x1c]);
        assert_eq!(bytes(Key::Character(']'), Modifiers::control(), mode), vec![0x1d]);
        assert_eq!(bytes(Key::Character(' '), Modifiers::control(), mode), vec![0x00]);
    }

    #[test]
    fn alt_and_a_letter_puts_escape_in_front() {
        let mode = Mode::default();
        assert_eq!(bytes(Key::Character('f'), Modifiers::alt(), mode), vec![0x1b, b'f']);
    }

    #[test]
    fn the_apple_key_sends_nothing_because_it_belongs_to_the_application() {
        let mode = Mode::default();
        let command = Modifiers { command: true, ..Modifiers::NONE };
        assert_eq!(encode(KeyPress::new(Key::Character('c'), command), mode), None);
        assert_eq!(encode(KeyPress::new(Key::Character('v'), command), mode), None);
        assert_eq!(encode(KeyPress::new(Key::Left, command), mode), None);
    }

    #[test]
    fn pasted_text_is_wrapped_only_when_the_program_asked_for_it() {
        let plain = Mode::default();
        assert_eq!(paste("one two", plain), b"one two".to_vec());
        let bracketed = Mode { bracketed_paste: true, ..Mode::default() };
        assert_eq!(
            String::from_utf8_lossy(&paste("one two", bracketed)).replace('\x1b', "ESC "),
            "ESC [200~one twoESC [201~"
        );
    }

    #[test]
    fn pasted_line_breaks_become_carriage_returns() {
        let mode = Mode::default();
        assert_eq!(paste("one\ntwo", mode), b"one\rtwo".to_vec());
        assert_eq!(paste("one\r\ntwo", mode), b"one\rtwo".to_vec(), "and not two of them");
    }

    #[test]
    fn a_focus_report_says_which_way_round_it_is() {
        assert_eq!(String::from_utf8_lossy(&focus(true)).replace('\x1b', "ESC "), "ESC [I");
        assert_eq!(String::from_utf8_lossy(&focus(false)).replace('\x1b', "ESC "), "ESC [O");
    }
}
