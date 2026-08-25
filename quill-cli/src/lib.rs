//! The Quill command line: the catalogue of commands, the wire format, and the client's own parts.
//!
//! Two targets are built from this crate and they need different halves of it.
//!
//! - `quill-cli`, the program somebody types at, uses all of it.
//! - `quill-app`, the window, uses [`catalogue`], [`protocol`] and [`instances`]: the list of what
//!   commands exist, the shape of a request and a reply, and where a running Quill says how to
//!   reach it.
//!
//! Nothing here depends on `quill-app`, which is what keeps the client a small program with no
//! window, no graphics card and no fonts behind it — and what keeps the dependency pointing one
//! way, so the two can never disagree about what `tab.open` is called.

pub mod catalogue;
pub mod client;
pub mod help;
pub mod instances;
pub mod parse;
pub mod protocol;

/// The version both halves report, which is the workspace's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod documentation;
