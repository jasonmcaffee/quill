//! The pieces of the window, one file each.
//!
//! Every component is a function that takes a `Ui` and the rectangle it is to fill, draws itself, and
//! returns what the user did in it. None of them changes the document or the window's state directly, so
//! the state changes in one place, `app`, and two components cannot disagree about what happened.

pub mod activity_bar;
pub mod context_menu;
pub mod diagram_view;
pub mod controls;
pub mod editor_view;
pub mod explorer;
pub mod file_tabs;
pub mod find_in_files;
pub mod git_dialogs;
pub mod git_panel;
pub mod go_to_file;
pub mod gutter;
pub mod menu_bar;
pub mod modal;
pub mod picture_view;
pub mod plugins_page;
pub mod prompt_dialog;
pub mod resize_edges;
pub mod settings_dialog;
pub mod splitter;
pub mod status_bar;
pub mod terminal_panel;
pub mod text_tools;
pub mod title_bar;
