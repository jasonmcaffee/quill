//! The pieces of the window, one file each.
//!
//! Every component is a function that takes a `Ui` and the rectangle it is to fill, draws itself, and
//! returns what the user did in it. None of them changes the document or the window's state directly, so
//! the state changes in one place, `app`, and two components cannot disagree about what happened.

// The Agent-Tasks board, which is the first plugin that draws. Nothing in it decides anything: the
// lanes, the drag and the search are `services::agent_tasks`.
// The Agent-Chat pane: the panel, the conversation, the composer and its Settings page.
pub mod agent_chat;
pub mod agent_tasks;
pub mod about_dialog;
pub mod activity_bar;
pub mod browser_view;
pub mod color_wheel;
pub mod completion;
pub mod context_menu;
pub mod diagram_view;
pub mod controls;
pub mod debug_dialogs;
pub mod debug_panel;
pub mod database;
pub mod dock;
pub mod editor_view;
pub mod explorer;
pub mod file_tabs;
pub mod find_bar;
pub mod find_in_files;
pub mod git_dialogs;
pub mod git_panel;
pub mod go_to_file;
pub mod gutter;
pub mod markdown_text;
pub mod menu_bar;
pub mod modal;
pub mod picture_view;
pub mod mcp_page;
pub mod plugins_page;
pub mod prompt_dialog;
pub mod references;
pub mod resize_edges;
pub mod run_dialog;
pub mod run_panel;
pub mod run_widget;
pub mod scrollbar;
pub mod settings_dialog;
pub mod splitter;
pub mod status_bar;
pub mod terminal_panel;
pub mod text_menu;
pub mod text_tools;
pub mod title_bar;
pub mod value_tooltip;
