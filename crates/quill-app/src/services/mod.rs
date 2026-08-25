//! Everything the window needs that is not drawing.

pub mod file_clipboard;
pub mod file_kind;
pub mod file_tree;
pub mod icons;
pub mod launcher;
pub mod native_menu;
pub mod picture;
pub mod plugins;
pub mod project_state;
pub mod store;
pub mod text_renderer;

// Letting the desktop show through the window needs a DirectComposition swapchain and a cleared
// redirection surface on Windows, and nothing at all anywhere else.
#[cfg(windows)]
pub mod windows_transparency;
