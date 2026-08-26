//! Everything the window needs that is not drawing.

pub mod control;
pub mod file_clipboard;
pub mod file_kind;
pub mod file_marks;
pub mod file_search;
pub mod file_tree;
pub mod icons;
pub mod imports;
pub mod launcher;
pub mod mcp;
pub mod mermaid_scene;
pub mod native_menu;
pub mod picture;
pub mod plugins;
pub mod preview_images;
pub mod project_state;
pub mod store;
pub mod symbol_index;
pub mod text_renderer;
pub mod text_search;

// Letting the desktop show through the window needs a DirectComposition swapchain and a cleared
// redirection surface on Windows, and nothing at all anywhere else.
#[cfg(windows)]
pub mod windows_transparency;
