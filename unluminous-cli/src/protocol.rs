//! What goes over the wire between the client and a running Unluminous.
//!
//! One JSON object a line, a request and then a reply, and the connection is closed. There is no
//! session, no handshake and no framing beyond the newline, which is what makes the channel
//! reachable from a language that has a socket and a JSON library and nothing else — three lines of
//! Python drive Unluminous, and `unluminous-cli` is the friendly face rather than the only way in.
//!
//! ```text
//! -> {"token":"4f1a...","command":"tab.open","arguments":{"path":"README.md"}}
//! <- {"ok":true,"command":"tab.open","message":"Opened README.md","result":{"tab":2}}
//! ```
//!
//! A failure is the same shape with `ok` false and an `error` in place of the result:
//!
//! ```text
//! <- {"ok":false,"command":"tab.open","error":{"code":"not-found","message":"There is no file at ..."}}
//! ```
//!
//! ## Why the window writes the sentence
//!
//! `message` is what the client prints when nobody asked for JSON. It is written by the window
//! rather than by the client, because the window is the only one that knows what actually happened
//! — which tab the file landed in, what the setting was before it was changed, how many results a
//! search found. A client that made up its own sentence would be guessing at the thing it just
//! asked somebody else to do.
//!
//! ## Why the codes are words
//!
//! `error.code` is a short word, not a number. A caller matching on `not-found` is reading its own
//! program a year later; a caller matching on `4` is reading a table. The client turns the word
//! into a process exit code on the way out, because that is the one place a number is what the
//! shell understands.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Map, Value};

/// Something asked of a running Unluminous.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub token: String,
    /// The command's wire name, such as `tab.open`.
    pub command: String,
    /// Every value the command was given, positional or flag, under the name the catalogue gives it.
    pub arguments: Map<String, Value>,
    /// How long the caller is prepared to wait for an answer, in milliseconds.
    ///
    /// The window has no other way of knowing when a caller has gone: a client that gives up says
    /// nothing, it simply stops reading. So it says so in advance, and a request still sitting on
    /// the queue when the deadline passes is thrown away rather than applied to a window nobody is
    /// listening to any more — `task-1691` measured three commands that reported a timeout and had
    /// been applied. Absent from an older client, where the window's own backstop still applies.
    pub deadline_ms: Option<u64>,
}

impl Request {
    pub fn new(token: &str, command: &str, arguments: Map<String, Value>) -> Self {
        Self {
            token: token.to_owned(),
            command: command.to_owned(),
            arguments,
            deadline_ms: None,
        }
    }

    /// The same request, saying how long the caller will wait for it.
    pub fn waiting_for(mut self, deadline: Duration) -> Self {
        self.deadline_ms = Some(deadline.as_millis().min(u64::MAX as u128) as u64);
        self
    }

    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "token": self.token,
            "command": self.command,
            "arguments": Value::Object(self.arguments.clone()),
        });
        // Left out altogether when nobody said, rather than written as null, so a request from a
        // client that has never heard of it looks exactly as it did before.
        if let Some(deadline) = self.deadline_ms {
            out["deadline_ms"] = json!(deadline);
        }
        out
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        let raw_command = value.get("command")?.as_str()?;
        Some(Self {
            token: value.get("token")?.as_str()?.to_owned(),
            command: crate::catalogue::find(raw_command)
                .map(|command| command.wire())
                .unwrap_or_else(|| raw_command.to_owned()),
            arguments: match value.get("arguments") {
                // The leading dashes come off here, at the window's front door, because this is
                // where a request that somebody else wrote arrives. The usage lines say
                // `[--permanent]`, so an agent reading the catalogue sends `--permanent`, and it
                // means the same thing as `permanent`.
                Some(Value::Object(map)) => {
                    crate::catalogue::normalise_arguments(map.clone())
                }
                // An absent `arguments` is an empty one, so a command that takes nothing can be
                // sent as `{"token":"...","command":"tab.next"}`.
                None | Some(Value::Null) => Map::new(),
                Some(_) => return None,
            },
            deadline_ms: match value.get("deadline_ms") {
                Some(Value::Number(number)) => number.as_u64(),
                // A string, because a caller in another language may spell a number that way and
                // `text` already accepts both for every other value on the wire.
                Some(Value::String(text)) => text.trim().parse().ok(),
                _ => None,
            },
        })
    }

    /// A named value as text, whatever kind it arrived as.
    ///
    /// A number and a string are both accepted for the same name, because a person typing
    /// `--line 42` and a program sending `"line": 42` mean the same thing and neither should have
    /// to know what the other would have done.
    pub fn text(&self, name: &str) -> Option<String> {
        match self.arguments.get(name)? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            Value::Bool(flag) => Some(flag.to_string()),
            _ => None,
        }
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        match self.arguments.get(name)? {
            Value::Number(number) => number.as_f64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn whole(&self, name: &str) -> Option<usize> {
        self.number(name).filter(|value| *value >= 0.0).map(|value| value as usize)
    }

    /// True when a switch was given at all, however it was spelled.
    pub fn switch(&self, name: &str) -> bool {
        match self.arguments.get(name) {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::String(text)) => !matches!(text.trim(), "false" | "no" | "0" | ""),
            Some(Value::Null) | None => false,
            Some(_) => true,
        }
    }

    pub fn has(&self, name: &str) -> bool {
        !matches!(self.arguments.get(name), None | Some(Value::Null))
    }
}

/// Why something could not be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

/// What a running Unluminous answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub ok: bool,
    pub command: String,
    /// The sentence the client prints when nobody asked for JSON.
    pub message: String,
    pub result: Value,
    pub error: Option<Failure>,
}

impl Reply {
    pub fn done(command: &str, message: impl Into<String>, result: Value) -> Self {
        Self {
            ok: true,
            command: command.to_owned(),
            message: message.into(),
            result,
            error: None,
        }
    }

    pub fn failed(command: &str, code: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            ok: false,
            command: command.to_owned(),
            message: message.clone(),
            result: Value::Null,
            error: Some(Failure { code: code.to_owned(), message }),
        }
    }

    pub fn to_json(&self) -> Value {
        match &self.error {
            Some(failure) => json!({
                "ok": false,
                "command": self.command,
                "error": { "code": failure.code, "message": failure.message },
            }),
            None => json!({
                "ok": true,
                "command": self.command,
                "message": self.message,
                "result": self.result,
            }),
        }
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        let ok = value.get("ok")?.as_bool()?;
        let command = value.get("command").and_then(Value::as_str).unwrap_or_default().to_owned();
        if ok {
            Some(Self {
                ok,
                command,
                message: value.get("message").and_then(Value::as_str).unwrap_or_default().to_owned(),
                result: value.get("result").cloned().unwrap_or(Value::Null),
                error: None,
            })
        } else {
            let error = value.get("error")?;
            let failure = Failure {
                code: error.get("code").and_then(Value::as_str).unwrap_or("failed").to_owned(),
                message: error.get("message").and_then(Value::as_str).unwrap_or_default().to_owned(),
            };
            Some(Self {
                ok,
                command,
                message: failure.message.clone(),
                result: Value::Null,
                error: Some(failure),
            })
        }
    }
}

/// The codes an `error` can carry, and what each one means.
///
/// Written down here rather than left to be discovered from the source, because a caller matching
/// on them needs the list and because the client turns each one into a process exit code.
pub mod code {
    /// No Unluminous is running, or the one named is not answering.
    pub const NOT_RUNNING: &str = "not-running";
    /// Several Unluminouss are running and none was named.
    pub const SEVERAL: &str = "several-instances";
    /// There is no command by that name.
    pub const UNKNOWN_COMMAND: &str = "unknown-command";
    /// The command exists but was given the wrong thing.
    pub const USAGE: &str = "usage";
    /// The token was missing or wrong.
    pub const REFUSED: &str = "refused";
    /// What was asked for is not there: a file, a tab, a setting, a result.
    pub const NOT_FOUND: &str = "not-found";
    /// The command cannot apply to what is showing, such as a preview of a `.rs` file.
    pub const NOT_APPLICABLE: &str = "not-applicable";
    /// It was tried and it did not work. The message is what went wrong.
    pub const FAILED: &str = "failed";
    /// It was still going when the time ran out.
    pub const TIMED_OUT: &str = "timed-out";
}

/// Send one request to a port on this machine and read the one reply.
///
/// The connection is opened, used and closed. Keeping it open would buy nothing: a CLI process
/// sends one command and stops, and a program driving Unluminous in a loop opening a socket per command
/// on the loopback interface is measured in tens of microseconds.
pub fn ask(port: u16, request: &Request, timeout: Duration) -> std::io::Result<Reply> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let stream = TcpStream::connect_timeout(&address, timeout.min(Duration::from_secs(5)))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    // The deadline goes with the request, so the window stops waiting when the caller does and a
    // request nobody is listening for any more is thrown away rather than applied later. Set here
    // rather than by each caller, because this is the one function that knows both the request and
    // how long the caller will wait for it.
    let request = request.clone().waiting_for(timeout);
    let mut writing = stream.try_clone()?;
    writeln!(writing, "{}", request.to_json())?;
    writing.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "Unluminous closed the connection without answering",
        ));
    }
    let value: Value = serde_json::from_str(line.trim())
        .map_err(|problem| std::io::Error::new(std::io::ErrorKind::InvalidData, problem))?;
    Reply::from_json(&value).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "Unluminous's answer was not a reply")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs.iter().map(|(name, value)| ((*name).to_owned(), value.clone())).collect()
    }

    #[test]
    fn a_request_survives_being_written_and_read_back() {
        let request = Request::new(
            "abc",
            "tab.open",
            arguments(&[("path", json!("README.md")), ("permanent", json!(true))]),
        );
        assert_eq!(Request::from_json(&request.to_json()), Some(request));
    }

    #[test]
    fn a_request_with_no_arguments_may_leave_them_out_altogether() {
        let value = json!({ "token": "abc", "command": "tab.next" });
        let request = Request::from_json(&value).expect("a request");
        assert!(request.arguments.is_empty());
        assert_eq!(request.deadline_ms, None, "a client that says nothing asks for nothing");
    }

    #[test]
    fn a_wire_request_canonicalises_command_and_argument_aliases() {
        let request = Request::from_json(&json!({
            "token": "abc",
            "command": "editor.open",
            "arguments": { "waitFor": "ready", "from_line": 4 },
        }))
        .expect("a request");
        assert_eq!(request.command, "tab.open");
        assert_eq!(request.arguments["wait-for"], json!("ready"));
        assert_eq!(request.arguments["from-line"], json!(4));
    }

    #[test]
    fn a_deadline_that_arrives_with_the_request_is_read_back() {
        let request = Request::new("abc", "tab.list", Map::new())
            .waiting_for(Duration::from_millis(15_000));
        let line = request.to_json();
        assert_eq!(line["deadline_ms"], json!(15_000));
        assert_eq!(Request::from_json(&line), Some(request));
        // An older client leaves it out altogether rather than writing null, so what it sends is
        // byte for byte what it sent before.
        let without = Request::new("abc", "tab.list", Map::new());
        assert!(without.to_json().get("deadline_ms").is_none());
        assert_eq!(Request::from_json(&without.to_json()), Some(without));
        // And a caller in another language may spell the number as text.
        let spelled = json!({ "token": "abc", "command": "tab.list", "deadline_ms": "2500" });
        assert_eq!(Request::from_json(&spelled).expect("a request").deadline_ms, Some(2_500));
    }

    #[test]
    fn a_value_reads_the_same_whether_it_arrived_as_a_number_or_as_text() {
        // The person types `--line 42` and the client sends a string; a program sends 42. Neither
        // should have to know what the other would have done.
        let typed = Request::new("t", "editor.caret", arguments(&[("line", json!("42"))]));
        let sent = Request::new("t", "editor.caret", arguments(&[("line", json!(42))]));
        assert_eq!(typed.whole("line"), Some(42));
        assert_eq!(sent.whole("line"), Some(42));
        assert_eq!(typed.text("line").as_deref(), Some("42"));
        assert_eq!(sent.text("line").as_deref(), Some("42"));
    }

    #[test]
    fn a_switch_is_off_when_it_is_absent_and_on_when_it_is_given() {
        let given = Request::new("t", "tab.open", arguments(&[("permanent", json!(true))]));
        let absent = Request::new("t", "tab.open", arguments(&[("path", json!("a"))]));
        let denied = Request::new("t", "tab.open", arguments(&[("permanent", json!(false))]));
        assert!(given.switch("permanent"));
        assert!(!absent.switch("permanent"));
        assert!(!denied.switch("permanent"), "a switch sent as false is off");
    }

    #[test]
    fn a_reply_survives_being_written_and_read_back() {
        let done = Reply::done("tab.open", "Opened README.md", json!({ "tab": 2 }));
        assert_eq!(Reply::from_json(&done.to_json()), Some(done));
        let failed = Reply::failed("tab.open", code::NOT_FOUND, "There is no file at nope.md");
        let read = Reply::from_json(&failed.to_json()).expect("a reply");
        assert!(!read.ok);
        assert_eq!(read.error.as_ref().map(|e| e.code.as_str()), Some(code::NOT_FOUND));
        assert_eq!(read.message, "There is no file at nope.md");
    }

    #[test]
    fn a_reply_is_one_line_however_odd_the_text_in_it_is() {
        // Terminal output carries new lines, quotation marks and control characters, and the wire
        // format is one object a line. This is what would break it if the text were pasted in
        // rather than encoded.
        let reply = Reply::done(
            "terminal.read",
            "Read the screen",
            json!({ "text": "one\ntwo\t\"three\"\u{1b}[0m\r\n" }),
        );
        let line = reply.to_json().to_string();
        assert!(!line.contains('\n'), "the encoded reply must be one line");
        let back = Reply::from_json(&serde_json::from_str(&line).expect("json")).expect("a reply");
        assert_eq!(back.result["text"], json!("one\ntwo\t\"three\"\u{1b}[0m\r\n"));
    }
}
