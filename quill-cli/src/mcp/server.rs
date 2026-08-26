//! The protocol: what an MCP client asks, and what it is told.
//!
//! JSON-RPC 2.0 over whichever transport carried it. `mcp::stdio` and `mcp::http` both hand one
//! parsed message to [`Server::answer`] and write back what it returns, so there is one
//! implementation of the protocol and two ways of getting bytes to it.
//!
//! ## It is stateless, and that is a decision rather than a shortcut
//!
//! MCP has moved twice while Quill has been written. `2025-06-18` is what every shipping client
//! speaks: an `initialize` handshake, an optional `Mcp-Session-Id`, a negotiated protocol version.
//! `2026-07-28` **deletes** the handshake and the session — every request carries its own version,
//! any request may land on any server, capabilities move to an optional `server/discover`.
//!
//! A server that never *requires* `initialize`, never issues a session id and echoes back whatever
//! version the client named satisfies both at once. So that is what this is: `initialize` is
//! answered because a 2025-06-18 client sends it first and waits, and `tools/list` and `tools/call`
//! are answered identically whether it happened or not. There is no version switch in the code and
//! nothing to migrate when a client moves.
//!
//! ## Every call goes down the control channel
//!
//! A tool call becomes exactly the request `quill-cli` would have sent — the same wire name, the
//! same arguments object, the same token from the same instance file, to the same port. Nothing new
//! reaches the window's state, so `QuillApp::run_cli` stays the one place a command becomes a
//! change and a command run by an agent is the same command a person types. [`Driver`] is that
//! step, behind a trait, so the protocol can be tested with no Quill running.

use serde_json::{json, Map, Value};

use crate::catalogue::Command;
use crate::mcp::tools::{self, Shape};
use crate::protocol::Reply;

/// What this server names when a client asks for a version it has never heard of.
///
/// The newest revision it was written against. A client that names its own is answered with that
/// instead, which is what the specification's version negotiation asks for and what stops this
/// number needing to be right for ever.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The name a client shows for this server.
pub const SERVER_NAME: &str = "quill";

/// The whole of `docs/commands.md`, as a resource.
///
/// Embedded rather than read from disk: the installed `quill-cli` is one file and the documentation
/// is not beside it. It cannot go stale, because `documentation.rs` fails while this file disagrees
/// with the catalogue.
const COMMANDS_DOCUMENTATION: &str = include_str!("../../docs/commands.md");

const COMMANDS_URI: &str = "quill://commands.md";

/// Something that can run a Quill command and give back what the window said.
///
/// A trait for two reasons. The protocol is worth testing without a window, a socket or an instance
/// file, which a stub driver gives; and the window itself hosts this server, where "find a Quill"
/// wants a different preference from the one a spawned server has.
pub trait Driver {
    /// Run one command against whichever Quill `instance` names, or the one the driver would
    /// choose. `Err` is for not being able to reach a Quill at all, which is a different thing from
    /// a Quill refusing the command.
    fn run(
        &self,
        command: &'static Command,
        arguments: Map<String, Value>,
        instance: Option<&str>,
    ) -> Result<Reply, Failure>;

    /// Read a file the window has just written, which is how a screenshot becomes a picture the
    /// agent can see. On the driver because a server for a Quill somewhere else could not.
    fn read_file(&self, _path: &std::path::Path) -> Option<Vec<u8>> {
        None
    }
}

/// Why no Quill could be reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}

/// The server. It holds no session and no conversation, only what shape its tools are.
pub struct Server<D: Driver> {
    shape: Shape,
    driver: D,
}

impl<D: Driver> Server<D> {
    pub fn new(shape: Shape, driver: D) -> Self {
        Self { shape, driver }
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    /// Answer one JSON-RPC message.
    ///
    /// `None` for a notification, which by definition has no answer — the transports turn that into
    /// silence on a pipe and `202 Accepted` over HTTP.
    pub fn answer(&self, message: &Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
        let id = message.get("id").cloned();
        // A message with no id is a notification or a response to something this server never
        // asked. Either way there is nothing to send back, and sending an error to a notification
        // is the one thing JSON-RPC says not to do.
        let id = id.filter(|id| !id.is_null())?;
        let parameters = message.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => Some(result(&id, self.initialize(&parameters))),
            "ping" => Some(result(&id, json!({}))),
            "tools/list" => Some(result(&id, json!({ "tools": tools::as_json(self.shape) }))),
            "tools/call" => Some(self.call(&id, &parameters)),
            "resources/list" => Some(result(&id, json!({ "resources": [resource()] }))),
            "resources/templates/list" => Some(result(&id, json!({ "resourceTemplates": [] }))),
            "resources/read" => Some(self.read(&id, &parameters)),
            "prompts/list" => Some(result(&id, json!({ "prompts": [] }))),
            "completion/complete" => {
                Some(result(&id, json!({ "completion": { "values": [], "hasMore": false } })))
            }
            other => Some(error(
                &id,
                -32601,
                format!("Quill's MCP server has no `{other}` method."),
            )),
        }
    }

    /// What a client is told when it introduces itself.
    ///
    /// The version it named is echoed back when it looks like a version at all, rather than being
    /// argued with. A client that speaks a revision newer than this one is not a client this server
    /// has to refuse: the methods it will go on to call are the same three.
    fn initialize(&self, parameters: &Value) -> Value {
        let asked = parameters.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
        let version = if looks_like_a_version(asked) { asked } else { PROTOCOL_VERSION };
        json!({
            "protocolVersion": version,
            "capabilities": {
                // Nothing here changes while the server runs — the catalogue is compiled in — so
                // there is deliberately no `listChanged`.
                "tools": {},
                "resources": {},
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "title": "Quill",
                "version": crate::VERSION,
            },
            "instructions": INSTRUCTIONS,
        })
    }

    /// Run a tool.
    fn call(&self, id: &Value, parameters: &Value) -> Value {
        let Some(name) = parameters.get("name").and_then(Value::as_str) else {
            return error(id, -32602, "A tool call needs a `name`.");
        };
        let given = match parameters.get("arguments") {
            Some(Value::Object(map)) => map.clone(),
            None | Some(Value::Null) => Map::new(),
            Some(_) => return error(id, -32602, "`arguments` has to be an object."),
        };
        // A tool that does not exist is a protocol error, because the client asked for something
        // that was never in `tools/list`. Everything after this point is the tool running and
        // failing, which is `isError` instead — the distinction the specification draws, and the
        // one that lets an agent try something else rather than the connection being torn down.
        let call = match tools::resolve(self.shape, name, &given) {
            Ok(call) => call,
            Err(problem) => return error(id, -32602, problem.0),
        };
        match self.driver.run(call.command, call.arguments, call.instance.as_deref()) {
            Ok(reply) => result(id, self.tool_result(call.command, &reply)),
            Err(problem) => {
                result(id, refused(format!("{}: {}", problem.code, problem.message)))
            }
        }
    }

    /// Turn what the window said into what an agent reads.
    fn tool_result(&self, command: &'static Command, reply: &Reply) -> Value {
        if let Some(failure) = &reply.error {
            return refused(format!("{}: {}", failure.code, failure.message));
        }
        let mut content = vec![json!({ "type": "text", "text": spoken(reply) })];
        if let Some(picture) = self.picture_from(command, reply) {
            content.push(picture);
        }
        let mut answer = json!({ "content": content, "isError": false });
        if !reply.result.is_null() {
            answer["structuredContent"] = reply.result.clone();
        }
        answer
    }

    /// The picture a screenshot just wrote, so the agent can look at it rather than be told where
    /// it went.
    ///
    /// One command is special-cased and it is the only one. A screenshot an agent cannot see is
    /// most of a screenshot's value thrown away, and it is a rule about a reply rather than a tool
    /// of its own, so it holds in both shapes and there is no extra tool duplicating
    /// `window screenshot`.
    fn picture_from(&self, command: &'static Command, reply: &Reply) -> Option<Value> {
        if command.wire() != "window.screenshot" {
            return None;
        }
        let path = reply.result.get("path").and_then(Value::as_str)?;
        let bytes = self.driver.read_file(std::path::Path::new(path))?;
        Some(json!({
            "type": "image",
            "data": crate::mcp::base64::encode(&bytes),
            "mimeType": "image/png",
        }))
    }

    fn read(&self, id: &Value, parameters: &Value) -> Value {
        match parameters.get("uri").and_then(Value::as_str) {
            Some(COMMANDS_URI) => result(
                id,
                json!({
                    "contents": [{
                        "uri": COMMANDS_URI,
                        "mimeType": "text/markdown",
                        "text": COMMANDS_DOCUMENTATION,
                    }],
                }),
            ),
            Some(other) => error(id, -32602, format!("Quill has no resource at `{other}`.")),
            None => error(id, -32602, "A read needs a `uri`."),
        }
    }
}

/// What the client is told about the server before anything is called.
const INSTRUCTIONS: &str = "\
Quill is a text editor. These tools drive a window that is already open on this machine: they open \
files in tabs, read and change the text, run commands in its terminals, search the project, drive \
its dialogs, change its settings, work its Git menu, and take a screenshot of the real window.\n\n\
Three things worth knowing. A relative path is resolved against the project folder, never against \
your working directory. A project is a window, so several Quills may be running and `instance` says \
which one you mean. And `window screenshot` gives you the picture itself, which is how to see what \
a command actually did.";

/// The one resource: the whole written reference, for an agent that would rather read than call.
fn resource() -> Value {
    json!({
        "uri": COMMANDS_URI,
        "name": "commands.md",
        "title": "Quill's command reference",
        "description": "Every command Quill has, with its arguments, its flags and its examples. The same document a person reads.",
        "mimeType": "text/markdown",
    })
}

/// What a reply reads as.
///
/// The sentence the window wrote, then the two keys that already have a meaning: `text` for content
/// that is text all through, `lines` for a listing. Anything else is the result laid out. The rule
/// is the client's own, so what an agent sees and what `quill-cli` prints are the same thing.
fn spoken(reply: &Reply) -> String {
    let mut out = String::from(&reply.message);
    if let Some(text) = reply.result.get("text").and_then(Value::as_str) {
        push_block(&mut out, text);
    } else if let Some(lines) = reply.result.get("lines").and_then(Value::as_array) {
        let listed: Vec<String> = lines
            .iter()
            .map(|line| match line.as_str() {
                Some(line) => line.to_owned(),
                None => line.to_string(),
            })
            .collect();
        push_block(&mut out, &listed.join("\n"));
    } else if !reply.result.is_null() {
        let laid_out = serde_json::to_string_pretty(&reply.result)
            .unwrap_or_else(|_| reply.result.to_string());
        push_block(&mut out, &laid_out);
    }
    if out.is_empty() {
        out.push_str("Done.");
    }
    out
}

fn push_block(out: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(block);
}

/// A tool that ran and would not do it. Not a JSON-RPC error: see [`Server::call`].
fn refused(message: String) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn result(id: &Value, value: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": value })
}

fn error(id: &Value, code: i32, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// True of `2025-06-18`, `2026-07-28`, and of the one after those that has not been written yet.
///
/// Deliberately shaped rather than a list. A server that refused every version it had not been told
/// about would be a server that stopped working the day the specification moved, which is the
/// failure this whole module is arranged to avoid.
pub fn looks_like_a_version(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(at, byte)| at == 4 || at == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A driver that records what it was asked and answers with whatever it was told to.
    struct Stub {
        answer: Reply,
        asked: RefCell<Vec<(String, Map<String, Value>, Option<String>)>>,
        file: Option<Vec<u8>>,
    }

    impl Stub {
        fn answering(reply: Reply) -> Self {
            Self { answer: reply, asked: RefCell::new(Vec::new()), file: None }
        }
    }

    impl Driver for Stub {
        fn run(
            &self,
            command: &'static Command,
            arguments: Map<String, Value>,
            instance: Option<&str>,
        ) -> Result<Reply, Failure> {
            self.asked.borrow_mut().push((
                command.wire(),
                arguments,
                instance.map(str::to_owned),
            ));
            Ok(self.answer.clone())
        }

        fn read_file(&self, _path: &std::path::Path) -> Option<Vec<u8>> {
            self.file.clone()
        }
    }

    fn ask(server: &Server<Stub>, method: &str, parameters: Value) -> Value {
        server
            .answer(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": parameters }))
            .expect("a request is answered")
    }

    fn a_server() -> Server<Stub> {
        Server::new(
            Shape::Grouped,
            Stub::answering(Reply::done("tab.open", "Opened README.md in tab 1", json!({ "tab": 1 }))),
        )
    }

    #[test]
    fn tools_are_listed_and_called_with_no_initialize_first() {
        // The 2026-07-28 shape: no handshake, no session, every request stands on its own.
        let server = a_server();
        let listed = ask(&server, "tools/list", Value::Null);
        let tools = listed["result"]["tools"].as_array().expect("tools");
        assert_eq!(
            tools.len(),
            crate::catalogue::areas().len() + 1,
            "one tool an area, plus one for the commands that have no area"
        );
        let called = ask(
            &server,
            "tools/call",
            json!({ "name": "quill_tab", "arguments": { "command": "open", "arguments": { "path": "README.md" } } }),
        );
        assert_eq!(called["result"]["isError"], json!(false));
        let (command, arguments, instance) = server.driver.asked.borrow()[0].clone();
        assert_eq!(command, "tab.open");
        assert_eq!(arguments["path"], json!("README.md"));
        assert_eq!(instance, None);
    }

    #[test]
    fn initialize_answers_with_the_version_the_client_named() {
        let server = a_server();
        for named in ["2025-06-18", "2026-07-28", "2024-11-05"] {
            let back = ask(&server, "initialize", json!({ "protocolVersion": named }));
            assert_eq!(back["result"]["protocolVersion"], json!(named), "asked for {named}");
        }
        // Something that is not a version at all falls back to ours rather than being echoed.
        let back = ask(&server, "initialize", json!({ "protocolVersion": "banana" }));
        assert_eq!(back["result"]["protocolVersion"], json!(PROTOCOL_VERSION));
        assert_eq!(back["result"]["serverInfo"]["name"], json!(SERVER_NAME));
    }

    #[test]
    fn a_notification_is_answered_with_silence() {
        let server = a_server();
        assert!(server
            .answer(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .is_none());
        assert!(server
            .answer(&json!({ "jsonrpc": "2.0", "id": null, "method": "notifications/cancelled" }))
            .is_none());
    }

    #[test]
    fn a_refusal_from_the_window_is_a_tool_error_and_not_a_protocol_error() {
        // The distinction matters: a JSON-RPC error is the client's mistake and stops the
        // conversation, and "there is no file called that" is neither.
        let server = Server::new(
            Shape::Grouped,
            Stub::answering(crate::protocol::Reply::failed(
                "tab.open",
                crate::protocol::code::NOT_FOUND,
                "There is no file at C:\\nope.md",
            )),
        );
        let back = ask(
            &server,
            "tools/call",
            json!({ "name": "quill_tab", "arguments": { "command": "open", "arguments": { "path": "nope.md" } } }),
        );
        assert!(back.get("error").is_none(), "it should not be a protocol error: {back}");
        assert_eq!(back["result"]["isError"], json!(true));
        let text = back["result"]["content"][0]["text"].as_str().expect("text");
        assert!(text.contains("not-found"), "{text}");
        assert!(text.contains("nope.md"), "{text}");
    }

    #[test]
    fn a_tool_that_does_not_exist_is_a_protocol_error() {
        let server = a_server();
        let back = ask(&server, "tools/call", json!({ "name": "quill_nonsense" }));
        assert_eq!(back["error"]["code"], json!(-32602));
    }

    #[test]
    fn an_unknown_method_is_method_not_found_rather_than_a_broken_connection() {
        let server = a_server();
        let back = ask(&server, "server/discover", Value::Null);
        assert_eq!(back["error"]["code"], json!(-32601));
    }

    #[test]
    fn a_listing_and_a_document_are_read_the_way_the_client_prints_them() {
        let server = Server::new(
            Shape::Grouped,
            Stub::answering(Reply::done(
                "tab.list",
                "3 tabs",
                json!({ "lines": ["0  README.md", "1  CLAUDE.md"] }),
            )),
        );
        let back = ask(
            &server,
            "tools/call",
            json!({ "name": "quill_tab", "arguments": { "command": "list" } }),
        );
        let text = back["result"]["content"][0]["text"].as_str().expect("text");
        assert!(text.starts_with("3 tabs"), "{text}");
        assert!(text.contains("1  CLAUDE.md"), "{text}");
        assert_eq!(back["result"]["structuredContent"]["lines"][0], json!("0  README.md"));
    }

    #[test]
    fn a_screenshot_comes_back_as_a_picture_and_nothing_else_does() {
        let mut stub = Stub::answering(Reply::done(
            "window.screenshot",
            "Wrote after.png",
            json!({ "path": "C:\\after.png" }),
        ));
        stub.file = Some(vec![0x89, b'P', b'N', b'G']);
        let server = Server::new(Shape::Grouped, stub);
        let back = ask(
            &server,
            "tools/call",
            json!({ "name": "quill_window", "arguments": { "command": "screenshot", "arguments": { "file": "after.png" } } }),
        );
        let content = back["result"]["content"].as_array().expect("content");
        assert_eq!(content.len(), 2, "the sentence and the picture");
        assert_eq!(content[1]["type"], json!("image"));
        assert_eq!(content[1]["mimeType"], json!("image/png"));
        assert_eq!(content[1]["data"], json!(crate::mcp::base64::encode(&[0x89, b'P', b'N', b'G'])));
    }

    #[test]
    fn the_written_reference_is_offered_as_a_resource_and_can_be_read() {
        let server = a_server();
        let listed = ask(&server, "resources/list", Value::Null);
        assert_eq!(listed["result"]["resources"][0]["uri"], json!(COMMANDS_URI));
        let read = ask(&server, "resources/read", json!({ "uri": COMMANDS_URI }));
        let text = read["result"]["contents"][0]["text"].as_str().expect("text");
        assert!(text.contains("quill-cli commands"), "it should be the real document");
        assert!(text.contains("### tab open"), "it should be the real document");
    }

    #[test]
    fn a_version_is_recognised_by_its_shape_so_the_next_one_still_works() {
        assert!(looks_like_a_version("2025-06-18"));
        assert!(looks_like_a_version("2026-07-28"));
        assert!(looks_like_a_version("2099-01-01"));
        assert!(!looks_like_a_version("2025-6-18"));
        assert!(!looks_like_a_version(""));
        assert!(!looks_like_a_version("latest"));
    }
}
