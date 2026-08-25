//! The pieces of the window, one file each.
//!
//! Every component is a function that takes a `Ui` and the rectangle it is to fill, draws itself, and
//! returns what the user did in it. None of them changes the document or the window's state directly, so
//! the state changes in one place, `app`, and two components cannot disagree about what happened.

pub mod context_menu;
pub mod controls;
pub mod editor_view;
pub mod explorer;
pub mod file_tabs;
pub mod git_dialogs;
pub mod git_panel;
pub mod gutter;
pub mod menu_bar;
pub mod modal;
pub mod plugins_page;
pub mod prompt_dialog;
pub mod settings_dialog;
pub mod splitter;
pub mod status_bar;
pub mod terminal_panel;
pub mod title_bar;
pub mod toolbar;
