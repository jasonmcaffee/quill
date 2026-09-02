//! Everything the window needs that is not drawing.

// The Agent-Chat plugin: a pane you talk to a model in. `quill-chat` is the half with no window in
// it — the endpoints, the wire shapes, the framing and the thread — and this is the half that has
// one. See `tasks/task-1767-agent-chat-tdd.md`.
pub mod agent_chat;
// The Agent-Tasks plugin: the first plugin that draws. `agent_tasks::AgentTasks` is the
// `plugin_ui::UiProvider` the manifest names, and everything under it is testable with no window.
pub mod agent_tasks;
pub mod breakpoint_store;
pub mod browser;
pub mod control;
pub mod crash_log;
// The Database plugin: the data sources, the connections, the tree and the pages.
pub mod database;
pub mod debuggers;
pub mod file_clipboard;
pub mod file_kind;
pub mod file_marks;
pub mod file_move;
pub mod file_search;
pub mod file_tree;
pub mod icons;
pub mod imports;
pub mod launcher;
pub mod locators;
pub mod login_shell;
pub mod mcp;
pub mod mermaid_scene;
pub mod native_menu;
pub mod picture;
pub mod plugin_ui;
pub mod plugins;
pub mod preview_images;
pub mod project_state;
pub mod recycle;
pub mod run_configurations;
pub mod store;
pub mod symbol_index;
pub mod text_renderer;
pub mod text_search;
pub mod vello_canvas;
pub mod wake;

// Letting the desktop show through the window needs a DirectComposition swapchain and a cleared
// redirection surface on Windows, and nothing at all anywhere else.
#[cfg(windows)]
pub mod windows_transparency;
