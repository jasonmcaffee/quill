//! A Debug Adapter Protocol client: the framing, the messages, the session and the thread.
//!
//! `task-1687` weighed the ways an editor can debug a program and chose the one the protocol exists
//! to make possible: **one client, every language**. A *debug adapter* is a separate program that
//! speaks DAP on one side and drives a real debugger on the other, and the debuggers have already
//! made this choice themselves — lldb ships `lldb-dap` inside every LLVM distribution, Python's
//! debugpy *is* an adapter, Microsoft's js-debug publishes a standalone DAP server, and Go's delve
//! serves the protocol natively. Speaking DAP is not adding a translation layer; it is speaking the
//! native protocol of the programs that already exist. Driving gdb's MI, the V8 inspector's CDP and
//! pydevd's own wire would have been three clients, three test rigs and three sets of faults, and
//! the fourth language would have cost a fourth.
//!
//! ## What is in here, and what is not
//!
//! - [`codec`] — `Content-Length` framed JSON, two pure functions.
//! - [`messages`] — the requests, responses and events Unluminous uses, typed, with the JSON written by
//!   hand for the reason `unluminous-cli/src/protocol.rs` writes its own by hand: this is an open
//!   protocol other people's programs speak rather than a Rust type that happens to serialise.
//! - [`session`] — the state machine. **It does no input or output at all**: messages in, frames to
//!   write and events to act on out, which is what makes every case in §12 of the design a test with
//!   no process behind it.
//! - [`adapter`] and [`client`] — the half that owns a pipe or a socket, and the thread it is read
//!   on, arranged exactly as `unluminous_git::Worker` is.
//!
//! **No user interface dependency**, for the reason `unluminous-core` and `unluminous-terminal` have none:
//! its tests run with no window, no graphics card and no fonts. And no knowledge of *which* adapter
//! to start — where `lldb-dap` lives on this machine is knowledge about the machine, and it lives in
//! `unluminous_app::services::debuggers` beside the settings file that can override it.

pub mod adapter;
pub mod client;
pub mod codec;
pub mod messages;
pub mod session;

pub use adapter::{AdapterCommand, Transport};
pub use client::{Client, Reply, Waker, GRACE};
pub use codec::{Decoder, FrameError};
pub use messages::{
    Capabilities, ExceptionFilter, Frame, Message, OutputKind, Request, Scope, SourceBreakpoint,
    Stopped, Thread, Variable, VerifiedBreakpoint,
};
pub use session::{Event, Outcome, Session, State, Step};
