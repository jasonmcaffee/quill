//! Turning a terminal colour into red, green and blue.
//!
//! A cell's colour is one of three things. A named colour, which is one of the sixteen a terminal has had
//! since the nineteen eighties plus the foreground, the background and the cursor. An indexed colour, which
//! is a number from 0 to 255 into the table below. Or a colour given in full, which is what 24 bit colour
//! means and which needs no table at all.
//!
//! `alacritty_terminal` keeps a table of colours a program has changed with an escape sequence and leaves
//! everything else empty, so the defaults are ours to supply. These are Quill's own: the same blue as the
//! accent in the window, the same amber as the unsaved marker, and a background that is the editor's
//! background, so that the terminal belongs to the window rather than looking like another application
//! sitting inside it.

/// A colour, as three bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The same colour mixed towards black, which is what dim text is drawn in.
    pub fn dimmed(self) -> Self {
        Self::new(
            (self.r as f32 * 0.62) as u8,
            (self.g as f32 * 0.62) as u8,
            (self.b as f32 * 0.62) as u8,
        )
    }
}

impl From<alacritty_terminal::vte::ansi::Rgb> for Rgb {
    fn from(value: alacritty_terminal::vte::ansi::Rgb) -> Self {
        Self::new(value.r, value.g, value.b)
    }
}

impl From<Rgb> for alacritty_terminal::vte::ansi::Rgb {
    fn from(value: Rgb) -> Self {
        Self { r: value.r, g: value.g, b: value.b }
    }
}

/// The sixteen named colours, in the order a terminal numbers them: black, red, green, yellow, blue,
/// magenta, cyan, white, and then the eight bright ones.
const NAMED: [Rgb; 16] = [
    Rgb::new(0x24, 0x2A, 0x33), // black, which is a shade lighter than the editor so it can be seen
    Rgb::new(0xE0, 0x4A, 0x4A), // red
    Rgb::new(0x5C, 0xC8, 0x7A), // green
    Rgb::new(0xFE, 0xBC, 0x2E), // yellow, the amber the window marks unsaved changes with
    Rgb::new(0x48, 0x9F, 0xF8), // blue, the window's accent
    Rgb::new(0xC3, 0x7C, 0xF0), // magenta
    Rgb::new(0x4F, 0xC1, 0xC4), // cyan
    Rgb::new(0xC8, 0xCE, 0xDB), // white, the colour a label on a control is drawn in
    Rgb::new(0x53, 0x5C, 0x6B), // bright black, which is what dimmed text and box drawing tend to use
    Rgb::new(0xFF, 0x6E, 0x6E), // bright red
    Rgb::new(0x7E, 0xE7, 0x9B), // bright green
    Rgb::new(0xFF, 0xD5, 0x66), // bright yellow
    Rgb::new(0x7C, 0xBB, 0xFF), // bright blue
    Rgb::new(0xDA, 0x9E, 0xFF), // bright magenta
    Rgb::new(0x74, 0xE0, 0xE3), // bright cyan
    Rgb::new(0xFF, 0xFF, 0xFF), // bright white
];

/// The six values each of red, green and blue takes in the colour cube, which every terminal uses.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The colours a terminal starts with, and the ones a program has changed.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Ordinary text.
    pub foreground: Rgb,
    /// Behind the text. The window's opacity setting is applied to this when it is painted.
    pub background: Rgb,
    /// The block the cursor is drawn as.
    pub cursor: Rgb,
    named: [Rgb; 16],
}

impl Palette {
    pub const fn new() -> Self {
        Self {
            foreground: Rgb::new(0xE8, 0xEB, 0xF1),
            background: Rgb::new(0x1A, 0x1F, 0x26),
            cursor: Rgb::new(0x48, 0x9F, 0xF8),
            named: NAMED,
        }
    }

    /// The colour at an index from 0 to 255: the sixteen named colours, then the colour cube, then the
    /// grey ramp.
    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.named[index as usize],
            16..=231 => {
                let index = index - 16;
                Rgb::new(
                    CUBE[(index / 36) as usize],
                    CUBE[((index % 36) / 6) as usize],
                    CUBE[(index % 6) as usize],
                )
            }
            232..=255 => {
                let step = 8 + 10 * (index as u16 - 232);
                let level = step.min(255) as u8;
                Rgb::new(level, level, level)
            }
        }
    }

    /// The colour a cell asked for.
    ///
    /// `bright` says whether bold text should use the bright variant of a named colour, which is what every
    /// terminal does and what makes a heading in `claude` stand out. `overrides` is the table
    /// `alacritty_terminal` keeps of colours a program has changed with an escape sequence; a colour set
    /// there wins over the default.
    pub fn resolve(
        &self,
        colour: alacritty_terminal::vte::ansi::Color,
        bright: bool,
        overrides: &alacritty_terminal::term::color::Colors,
    ) -> Rgb {
        use alacritty_terminal::vte::ansi::{Color, NamedColor};
        match colour {
            Color::Spec(rgb) => rgb.into(),
            Color::Indexed(index) => {
                let index = if bright && index < 8 { index + 8 } else { index };
                match overrides[index as usize] {
                    Some(rgb) => rgb.into(),
                    None => self.indexed(index),
                }
            }
            Color::Named(named) => {
                let named = if bright { named.to_bright() } else { named };
                if let Some(rgb) = overrides[named] {
                    return rgb.into();
                }
                match named {
                    NamedColor::Foreground => self.foreground,
                    NamedColor::BrightForeground => Rgb::new(0xFF, 0xFF, 0xFF),
                    NamedColor::DimForeground => self.foreground.dimmed(),
                    NamedColor::Background => self.background,
                    NamedColor::Cursor => self.cursor,
                    // The dim colours are the named ones mixed towards black, which is what a terminal
                    // without a dim table of its own does.
                    NamedColor::DimBlack
                    | NamedColor::DimRed
                    | NamedColor::DimGreen
                    | NamedColor::DimYellow
                    | NamedColor::DimBlue
                    | NamedColor::DimMagenta
                    | NamedColor::DimCyan
                    | NamedColor::DimWhite => {
                        let ordinary = named as usize - NamedColor::DimBlack as usize;
                        self.named[ordinary].dimmed()
                    }
                    other => self.named[(other as usize).min(15)],
                }
            }
        }
    }

    /// The table to hand `alacritty_terminal` so that a program asking what colour something is gets an
    /// answer, and so that resetting a colour puts Quill's own back rather than nothing.
    pub fn as_colors(&self) -> alacritty_terminal::term::color::Colors {
        use alacritty_terminal::vte::ansi::NamedColor;
        let mut colors = alacritty_terminal::term::color::Colors::default();
        for index in 0..=255_u8 {
            colors[index as usize] = Some(self.indexed(index).into());
        }
        colors[NamedColor::Foreground] = Some(self.foreground.into());
        colors[NamedColor::Background] = Some(self.background.into());
        colors[NamedColor::Cursor] = Some(self.cursor.into());
        colors
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term::color::Colors;
    use alacritty_terminal::vte::ansi::{Color, NamedColor};

    #[test]
    fn the_sixteen_named_colours_are_all_different() {
        let palette = Palette::new();
        for first in 0..16_u8 {
            for second in first + 1..16 {
                assert_ne!(
                    palette.indexed(first),
                    palette.indexed(second),
                    "colour {first} and colour {second} are the same, so text in them cannot be told apart"
                );
            }
        }
    }

    #[test]
    fn the_colour_cube_is_the_arithmetic_every_terminal_uses() {
        let palette = Palette::new();
        // 16 is the first cube entry, which is black; 231 is the last, which is white.
        assert_eq!(palette.indexed(16), Rgb::new(0, 0, 0));
        assert_eq!(palette.indexed(231), Rgb::new(255, 255, 255));
        // 196 is the pure red every terminal shows for `\x1b[38;5;196m`.
        assert_eq!(palette.indexed(196), Rgb::new(255, 0, 0));
        // 46 is pure green and 21 is pure blue.
        assert_eq!(palette.indexed(46), Rgb::new(0, 255, 0));
        assert_eq!(palette.indexed(21), Rgb::new(0, 0, 255));
    }

    #[test]
    fn the_grey_ramp_climbs_from_dark_to_light() {
        let palette = Palette::new();
        assert_eq!(palette.indexed(232), Rgb::new(8, 8, 8));
        assert_eq!(palette.indexed(255), Rgb::new(238, 238, 238));
        for index in 232..255_u8 {
            assert!(
                palette.indexed(index).r < palette.indexed(index + 1).r,
                "grey {index} should be darker than grey {}",
                index + 1
            );
        }
    }

    #[test]
    fn a_colour_given_in_full_is_used_as_it_is() {
        let palette = Palette::new();
        let colour = Color::Spec(alacritty_terminal::vte::ansi::Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(palette.resolve(colour, false, &Colors::default()), Rgb::new(1, 2, 3));
    }

    #[test]
    fn bold_text_in_a_named_colour_is_drawn_in_the_bright_one() {
        let palette = Palette::new();
        let plain = palette.resolve(Color::Named(NamedColor::Red), false, &Colors::default());
        let bold = palette.resolve(Color::Named(NamedColor::Red), true, &Colors::default());
        assert_eq!(plain, palette.indexed(1));
        assert_eq!(bold, palette.indexed(9), "bold red is bright red");
    }

    #[test]
    fn a_colour_a_program_has_changed_wins_over_the_default() {
        let palette = Palette::new();
        let mut overrides = Colors::default();
        overrides[1_usize] = Some(alacritty_terminal::vte::ansi::Rgb { r: 9, g: 9, b: 9 });
        assert_eq!(
            palette.resolve(Color::Indexed(1), false, &overrides),
            Rgb::new(9, 9, 9),
            "the program asked for a different red, so it gets one"
        );
    }

    #[test]
    fn dim_text_is_darker_than_ordinary_text() {
        let palette = Palette::new();
        let ordinary = palette.resolve(Color::Named(NamedColor::Foreground), false, &Colors::default());
        let dim = palette.resolve(Color::Named(NamedColor::DimForeground), false, &Colors::default());
        assert!(dim.r < ordinary.r && dim.g < ordinary.g && dim.b < ordinary.b);
    }

    #[test]
    fn the_table_handed_over_holds_every_colour() {
        let palette = Palette::new();
        let colors = palette.as_colors();
        for index in 0..=255_usize {
            assert!(colors[index].is_some(), "colour {index} should have a value");
        }
        assert_eq!(colors[NamedColor::Background], Some(palette.background.into()));
    }
}
