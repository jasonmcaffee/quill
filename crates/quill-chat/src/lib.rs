//! A streaming chat client: the endpoints, the two wire shapes, the framing, the conversation and
//! the thread a request runs on.
//!
//! `tasks/task-1767-agent-chat-tdd.md` is the design. This crate is the half of the Agent-Chat
//! plugin that can be tested with no window, no graphics card and no fonts — which is the half most
//! likely to be wrong, because it is a stream of somebody else's bytes arriving in pieces nobody
//! chose. `quill-dap` is arranged the same way and for the same reason: its tests run against
//! scripted adapters with no process, and these run against a scripted server with nothing beyond
//! loopback.
//!
//! ## What crosses the wire is a value
//!
//! Both shapes are open protocols other people's servers speak, so a request is built as a
//! `serde_json::Value` and a reply is read out of one. Nothing here is a Rust type that happens to
//! serialise. That is the choice `quill-cli/src/protocol.rs` and `quill-dap` already made.
//!
//! ## The two shapes, and why there are exactly two
//!
//! **OpenAI's `/v1/chat/completions`** is what llama.cpp, LM Studio, Ollama, vLLM, every gateway on
//! this machine and OpenAI itself speak, so one shape reaches almost everything. **Anthropic's
//! `/v1/messages`** is genuinely a different protocol — named events, indexed content blocks,
//! system as a field of its own — and it is the one the ticket names first. A third was weighed and
//! left out; [`Wire`] is a two-armed enum and adding one is an arm and a test rather than a
//! redesign.
//!
//! ## Nothing here fetches anything it was not asked to
//!
//! One function makes a request, [`client::Client::send`], and it is called when somebody presses
//! send. There is no polling, no discovery, no telemetry and no fetch on startup — which is the rule
//! the Markdown preview, the Mermaid reader and the plugin loader all keep, stated once more for the
//! one part of Quill that has a socket in it.

pub mod base64;
pub mod client;
pub mod model;
pub mod provider;
pub mod session;
pub mod sse;
pub mod wire;

pub use client::Client;
pub use model::{Conversation, Message, Part, Role, ToolCall, Usage};
pub use provider::{Provider, Wire};
pub use session::{Session, State};
pub use wire::{Reply, Tool};
