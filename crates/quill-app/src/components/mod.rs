//! The pieces of the window, one file each.
//!
//! Every component is a function that takes a `Ui` and the rectangle it is to fill, draws itself, and
//! returns what the user did in it. None of them changes the document or the window's state directly, so
//! the state changes in one place, `app`, and two components cannot disagree about what happened.

pub mod controls;
pub mod editor_view;
pub mod explorer;
pub mod menu_bar;
pub mod settings_dialog;
pub mod splitter;
pub mod status_bar;
pub mod terminal_panel;
pub mod title_bar;
pub mod toolbar;
