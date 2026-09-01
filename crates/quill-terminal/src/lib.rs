//! The terminal behind Quill's bottom tile.
//!
//! This crate has no user interface dependencies, for the same reason `quill-core` has none: its tests run
//! with no window and no graphics card, and the parts most likely to be wrong, which are the key encoding
//! and the colour palette, are then plain functions with plain expected values.
//!
//! What is here and what is not is set out in `tasks/quill-terminal-tdd.md`. In short: the escape sequence
//! emulation and the pseudoterminal come from `alacritty_terminal`, and the palette, the key encoding, the
//! screen the painter reads and the tabs are written here.
//!
//! - [`keys`] turns a key press into the bytes a terminal sends.
//! - [`mouse`] turns a click or a turn of the wheel into the bytes a program that asked for them expects.
//! - [`palette`] turns a terminal colour into red, green and blue.
//! - [`paths`] takes the verbatim prefix off a Windows path, which is the one form a shell cannot start in.
//! - [`reap`] makes sure a session's program goes when the session does, which Windows does not promise.
//! - [`screen`] is the snapshot the painter reads: plain data, no locks held.
//! - [`session`] is one terminal: a shell in a pseudoterminal, and the emulator behind it.
//! - [`tabs`] is several sessions with one of them showing.

pub mod keys;
pub mod mouse;
pub mod palette;
pub mod paths;
pub mod reap;
pub mod screen;
pub mod session;
pub mod tabs;

pub use keys::{encode, KeyPress, Mode};
pub use mouse::MouseMode;
pub use palette::{Palette, Rgb};
pub use screen::{Cursor, CursorShape, Screen, ScreenCell};
pub use session::{Session, SessionSettings, Size, Waker, SCROLLBACK};
pub use tabs::Tabs;
