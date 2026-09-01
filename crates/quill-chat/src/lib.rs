//! A streaming chat client: the endpoints, the five shapes, the framing, the conversation and the
//! thread a request runs on.
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
//! ## Five shapes, and the two that matter are programs rather than addresses
//!
//! The ticket asks for a *"connection to Claude and codex etc through cli"*, and that is what the two
//! rows it names are: **`claude-cli`** and **`codex-cli`** run the command-line agent already
//! installed on this machine. Nothing about that is a cheaper version of talking to the API — it is
//! the better one. Quill holds **no key at all**; the agent brings its own tools, its own sandbox and
//! its own permission model, so the question of what a model may do to this machine is answered by
//! the program a person already trusts with it; started in the project the window has open it reads
//! that project's own `CLAUDE.md` or `AGENTS.md`; and a second question is `--resume <session>`
//! rather than the whole transcript sent again. [`agent`] is all of it.
//!
//! Three address shapes remain, because a row can be pointed at one. **OpenAI's
//! `/v1/chat/completions`** is what llama.cpp, LM Studio, Ollama, vLLM and every gateway on this
//! machine speak, so one shape reaches almost everything and the `local` row that ships uses it.
//! **Anthropic's `/v1/messages`** and **OpenAI's `/v1/responses`** are there for a person who would
//! rather spend an API key than run an agent. All five are read into the same [`Reply`] values, so
//! nothing above this crate has heard of any of them.
//!
//! ## Nothing here fetches anything it was not asked to
//!
//! One function starts a turn, [`client::Client::send`], and it is called when somebody presses
//! send. There is no polling, no discovery, no telemetry and no fetch on startup — which is the rule
//! the Markdown preview, the Mermaid reader and the plugin loader all keep, stated once more for the
//! one part of Quill that opens a socket or starts a program.

pub mod agent;
pub mod base64;
pub mod client;
pub mod model;
pub mod provider;
pub mod session;
pub mod sse;
pub mod wire;

pub use agent::{Ask, Permission, PERMISSIONS};
pub use client::Client;
pub use model::{Conversation, Message, Part, Role, ToolCall, Usage};
pub use provider::{Provider, Wire};
pub use session::{Session, State};
pub use wire::{Reply, Tool};
