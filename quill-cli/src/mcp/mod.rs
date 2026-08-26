//! The Model Context Protocol server: how an AI agent discovers and drives Quill.
//!
//! `task-1661` gave Quill a command line and then measured an agent using it — the local Qwen 3.8
//! 27B, handed only `docs/commands.md`, carried out 64 instructions phrased as a person would say
//! them and scored 100%, against 3.13% with the document withheld. The mechanism works. What it
//! costs is that somebody has to hand the document over first, and then the agent has to know it
//! may shell out to a program it has never heard of.
//!
//! MCP removes both steps: a client asks a server what it can do and is told, in the form its own
//! tool-calling machinery understands, before the conversation starts. `task-1679` is that server.
//!
//! ## The four decisions, and where each of them lives
//!
//! **The tools are generated from the catalogue** — [`tools`]. `catalogue.rs` is one list that the
//! client parses against and the window dispatches on, and that shared list is what stops those two
//! drifting; a hand-written tool set would be exactly the third copy the rule exists to prevent. A
//! test fails the day a command is added without a tool.
//!
//! **Fourteen tools by default, not ninety-seven** — [`tools`] again, with the measurement that
//! decided it. One tool an area costs an agent roughly a third of the context one tool a command
//! does, and still names every command Quill has.
//!
//! **It is stateless** — [`server`]. `2025-06-18` is what clients speak today; `2026-07-28` deleted
//! the handshake and the session. A server that never requires `initialize` and issues no session
//! id satisfies both with one code path.
//!
//! **It is a client of the control channel, not a second way in** — [`driver`]. A tool call becomes
//! exactly the request `quill-cli` would have sent, down the same loopback socket with the same
//! token, so `QuillApp::run_cli` stays the one place a command becomes a change.
//!
//! ## Two transports
//!
//! [`stdio`] is what an agent launches: `quill-cli mcp serve`, one JSON message a line over the
//! process's own pipes, alive for as long as the conversation and listening on nothing. It is what
//! the install buttons write and what should be preferred.
//!
//! [`http`] is the Streamable HTTP endpoint, on a port somebody chose, for an agent that would
//! rather have a URL — and it is what a running Quill hosts when `mcp.enabled` is on. It is off by
//! default: a fixed open port that will run `terminal send` for anything that can reach it should
//! be a thing somebody turned on rather than a thing they were given.
//!
//! [`install`] writes Quill into Claude Code's and Codex's own configuration, which is the same
//! thing the buttons in `Settings -> Tools -> MCP` do.

pub mod base64;
pub mod driver;
pub mod http;
pub mod install;
pub mod server;
pub mod stdio;
pub mod tools;

pub use driver::Quills;
pub use server::{Driver, Server};
pub use tools::Shape;

/// The port a Quill hosting the HTTP endpoint listens on unless somebody chose another.
///
/// Fixed rather than chosen by the operating system, which is the opposite of what
/// `services::control` does and for the opposite reason: a control channel is found by reading the
/// instance file, and an MCP endpoint has to be written down in an agent's configuration before
/// either program has started. It is high, and unassigned by IANA.
pub const DEFAULT_PORT: u16 = 7345;

/// The lowest and highest port the setting will take. Below 1024 needs privileges on macOS and is
/// never what somebody meant.
pub const MIN_PORT: u16 = 1024;

/// How a client is told to talk to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    /// The agent launches `quill-cli mcp serve` and talks down its pipes. No port.
    #[default]
    Stdio,
    /// The agent posts to `http://127.0.0.1:<port>/mcp`.
    Http,
}

impl Transport {
    pub fn name(self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Http => "http",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "stdio" | "pipe" | "pipes" => Some(Transport::Stdio),
            "http" | "streamable-http" | "streamable_http" => Some(Transport::Http),
            _ => None,
        }
    }
}

/// Where an HTTP client sends its messages.
pub fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transport_is_spelled_the_same_everywhere() {
        assert_eq!(Transport::parse("stdio"), Some(Transport::Stdio));
        // The specification's own name for the HTTP transport, which is what somebody copying from
        // a client's documentation will have typed.
        assert_eq!(Transport::parse("streamable-http"), Some(Transport::Http));
        assert_eq!(Transport::parse("carrier pigeon"), None);
        assert_eq!(Transport::default().name(), "stdio");
    }

    #[test]
    fn the_endpoint_is_the_loopback_interface_and_says_so() {
        assert_eq!(endpoint(DEFAULT_PORT), "http://127.0.0.1:7345/mcp");
    }
}
