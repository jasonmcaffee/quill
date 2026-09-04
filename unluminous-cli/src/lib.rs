//! The Unluminous command line: the catalogue of commands, the wire format, and the client's own parts.
//!
//! Two targets are built from this crate and they need different halves of it.
//!
//! - `unluminous-cli`, the program somebody types at, uses all of it.
//! - `unluminous-app`, the window, uses [`catalogue`], [`protocol`], [`instances`] and [`mcp`]: the list
//!   of what commands exist, the shape of a request and a reply, where a running Unluminous says how to
//!   reach it, and the MCP server it hosts over HTTP when somebody has switched that on.
//!
//! Nothing here depends on `unluminous-app`, which is what keeps the client a small program with no
//! window, no graphics card and no fonts behind it — and what keeps the dependency pointing one
//! way, so the two can never disagree about what `tab.open` is called.

pub mod catalogue;
pub mod client;
pub mod help;
pub mod instances;
pub mod mcp;
pub mod parse;
pub mod protocol;

/// The version both halves report, which is the workspace's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod documentation;
