//! The transport an agent launches: JSON-RPC over this process's own pipes.
//!
//! The client starts `unluminate-cli mcp serve` as a subprocess, writes one JSON message a line to its
//! standard input and reads one a line back from its standard output. That is the whole framing,
//! and it is what both agents on this machine use.
//!
//! **Nothing but MCP messages may reach standard output.** A stray line — a warning, a progress
//! note, a `dbg!` left behind — is not noise to a client, it is a parse failure that takes the
//! connection down. Anything this server has to say goes to standard error, which the client is
//! free to capture, forward or ignore, and which is where the specification says logging belongs.

use std::io::{BufRead, BufReader, Write};

use crate::mcp::server::{Driver, Server};

/// Read messages until the client closes the pipe.
///
/// Returns when standard input ends, which is how a client says the conversation is over. A line
/// that is not JSON is answered with a JSON-RPC parse error rather than ignored: a client that sent
/// something malformed is a client waiting for a reply, and silence would hang it.
pub fn serve<D: Driver>(server: &Server<D>) -> std::io::Result<()> {
    let input = BufReader::new(std::io::stdin());
    let mut output = std::io::stdout();
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = answer_line(server, &line) {
            writeln!(output, "{answer}")?;
            // Flushed on every message, because the client is blocked reading a line and a buffered
            // reply is a conversation that stops for no reason anybody can see.
            output.flush()?;
        }
    }
    Ok(())
}

/// One line in, at most one line out. Split out from the loop so it can be tested without pipes.
pub fn answer_line<D: Driver>(server: &Server<D>, line: &str) -> Option<String> {
    let message: serde_json::Value = match serde_json::from_str(line.trim()) {
        Ok(message) => message,
        Err(problem) => {
            return Some(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": serde_json::Value::Null,
                    "error": { "code": -32700, "message": format!("That was not JSON: {problem}") },
                })
                .to_string(),
            )
        }
    };
    // A batch is an array of messages. It was removed in 2025-06-18 and no client sends one, but
    // answering it is four lines and refusing it would be a client that hangs.
    if let serde_json::Value::Array(messages) = &message {
        let answers: Vec<serde_json::Value> =
            messages.iter().filter_map(|one| server.answer(one)).collect();
        return (!answers.is_empty()).then(|| serde_json::Value::Array(answers).to_string());
    }
    server.answer(&message).map(|answer| answer.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Command;
    use crate::mcp::server::Failure;
    use crate::mcp::tools::Shape;
    use crate::protocol::Reply;
    use serde_json::{json, Map, Value};

    struct Nothing;

    impl Driver for Nothing {
        fn run(
            &self,
            command: &'static Command,
            _arguments: Map<String, Value>,
            _instance: Option<&str>,
        ) -> Result<Reply, Failure> {
            Ok(Reply::done(&command.wire(), "Done", Value::Null))
        }
    }

    fn a_server() -> Server<Nothing> {
        Server::new(Shape::Grouped, Nothing)
    }

    #[test]
    fn a_message_is_answered_on_one_line_with_no_newline_inside_it() {
        // The framing is the newline, so an answer holding one would be two messages.
        let answer = answer_line(&a_server(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .expect("an answer");
        assert!(!answer.contains('\n'), "an answer must be one line");
        let parsed: Value = serde_json::from_str(&answer).expect("json");
        assert!(parsed["result"]["tools"].as_array().expect("tools").len() > 1);
    }

    #[test]
    fn a_notification_produces_no_line_at_all() {
        assert!(answer_line(&a_server(), r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
    }

    #[test]
    fn a_line_that_is_not_json_is_answered_rather_than_leaving_the_client_waiting() {
        let answer = answer_line(&a_server(), "not json").expect("an answer");
        let parsed: Value = serde_json::from_str(&answer).expect("json");
        assert_eq!(parsed["error"]["code"], json!(-32700));
    }

    #[test]
    fn a_batch_is_answered_as_a_batch() {
        let answer = answer_line(
            &a_server(),
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
        )
        .expect("an answer");
        let parsed: Value = serde_json::from_str(&answer).expect("json");
        let answers = parsed.as_array().expect("an array");
        assert_eq!(answers.len(), 1, "the notification is not answered");
        assert_eq!(answers[0]["id"], json!(1));
    }
}
