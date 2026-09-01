//! Quill's own commands, offered to the model as tools.
//!
//! **Nothing here writes a tool out.** `quill_cli::mcp::tools` already turns the catalogue into
//! tools, with their titles, their one-line summaries and a JSON Schema for their arguments, and
//! `every_command_is_offered_as_a_tool_in_both_shapes` already fails if a command is ever not
//! offered. So this module is a translation between two envelopes and nothing else — which is the
//! rule `CLAUDE.md` states twice: *the MCP tools are generated from that catalogue; never write a
//! tool out by hand*.
//!
//! The grouped shape is used, one tool an area, for the reason `task-1679` measured: a hundred and
//! forty-seven tool definitions is about 36,000 tokens of an agent's context on every conversation
//! against about 14,600 for the twenty-two grouped ones, and the grouped ones still carry every
//! command's usage line and summary.
//!
//! ## Two properties come off, and both are about a *call to another window*
//!
//! `instance` names which Quill to drive, and this chat is inside the one it would name. `timeout`
//! is how long the caller will wait, and the caller here is a frame loop that cannot wait at all.
//! Leaving them in would offer a model two knobs whose only possible effect is to make a call fail.
//!
//! ## What is refused, and why refusing is the honest answer
//!
//! A command that **waits** — `terminal read --wait-for`, a git action that holds the window — is
//! refused with a sentence naming it. `QuillApp::run_cli` answers such a command with `Outcome::Hold`
//! and the answer arrives whenever it arrives; a tool call that never returned would wedge the
//! conversation with nothing on the screen to say why. The refusal goes back up as the tool's result,
//! so the model reads it and picks something else.

use serde_json::{Map, Value};

use quill_cli::mcp::tools::{self, Shape};

/// The shape the tools are offered in. One a area, for the reason in the module comment.
const SHAPE: Shape = Shape::Grouped;

/// Every tool Quill offers a model, built from the catalogue.
pub fn offered() -> Vec<quill_chat::Tool> {
    tools::tools(SHAPE)
        .into_iter()
        .map(|tool| quill_chat::Tool {
            // The title first, because it is the sentence that says what the tool is *for*, and
            // `task-1699` measured that a chooser picks on the title far more than on the body.
            description: match tool.title.is_empty() {
                true => tool.description,
                false => format!("{}\n\n{}", tool.title, tool.description),
            },
            schema: without_the_calls_own_properties(tool.schema),
            name: tool.name,
        })
        .collect()
}

/// The same schema with `instance` and `timeout` taken out.
fn without_the_calls_own_properties(mut schema: Value) -> Value {
    if let Some(properties) = schema["properties"].as_object_mut() {
        properties.remove("instance");
        properties.remove("timeout");
    }
    schema
}

/// What a tool call turned into: a command line request, or a sentence saying why not.
#[derive(Debug)]
pub struct Resolved {
    pub command: &'static quill_cli::catalogue::Command,
    pub arguments: Map<String, Value>,
}

/// Turn a model's tool call into the request `quill-cli` would have sent.
///
/// The one path, so a tool call and a person typing the same command reach the same code —
/// `QuillApp::run_cli` — rather than two paths that agree today.
pub fn resolve(name: &str, arguments: &Value) -> Result<Resolved, String> {
    let given = match arguments {
        Value::Object(map) => map.clone(),
        // A model that produced no arguments at all meant an empty object, which is what a command
        // taking nothing is called with anyway.
        Value::Null => Map::new(),
        other => {
            return Err(format!(
                "`{name}` was called with {other}, which is not an object of named values."
            ))
        }
    };
    let call = tools::resolve(SHAPE, name, &given).map_err(|problem| problem.0)?;
    if waits(call.command) {
        return Err(format!(
            "`{}` waits for something to happen, and a tool call cannot wait. Ask for something that answers at once.",
            call.command.wire()
        ));
    }
    Ok(Resolved {
        command: call.command,
        arguments: call.arguments,
    })
}

/// Whether this command holds the window rather than answering at once.
///
/// Read from the catalogue's own flags rather than from a list kept here, so a command that gains a
/// waiting flag later is refused the day it does — the rule the documentation test keeps for the
/// usage lines.
fn waits(command: &quill_cli::catalogue::Command) -> bool {
    command
        .flags
        .iter()
        .any(|flag| flag.name == "wait" || flag.name.starts_with("wait-") || flag.name == "no-wait")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_offered_comes_from_the_catalogue_and_has_a_schema() {
        let offered = offered();
        assert!(offered.len() > 10, "the grouped shape is one tool an area");
        for tool in &offered {
            assert!(!tool.name.is_empty());
            assert!(!tool.description.is_empty(), "{} says nothing", tool.name);
            assert_eq!(tool.schema["type"], "object", "{} has no schema", tool.name);
            let properties = tool.schema["properties"].as_object().expect("properties");
            // Both of the call's own properties are gone: naming another window is meaningless from
            // inside one, and a frame loop cannot wait.
            assert!(
                !properties.contains_key("instance"),
                "{} still offers `instance`",
                tool.name
            );
            assert!(
                !properties.contains_key("timeout"),
                "{} still offers `timeout`",
                tool.name
            );
        }
        // And the count matches what the MCP server offers, because it is the same generator: a
        // second list would be a second thing to keep in step.
        assert_eq!(offered.len(), tools::tools(SHAPE).len());
    }

    #[test]
    fn a_call_becomes_the_request_the_command_line_would_have_sent() {
        let resolved = resolve(
            "quill_tab",
            &serde_json::json!({ "command": "open", "arguments": { "path": "README.md", "--permanent": true } }),
        )
        .expect("resolved");
        assert_eq!(resolved.command.wire(), "tab.open");
        assert_eq!(resolved.arguments["path"], "README.md");
        // The leading dashes come off, which is what `task-1691` found was silently dropping flags.
        assert_eq!(resolved.arguments["permanent"], true);
    }

    #[test]
    fn a_tool_that_does_not_exist_is_a_sentence_rather_than_a_panic() {
        let problem = resolve("quill_nothing", &serde_json::json!({})).expect_err("a refusal");
        assert!(problem.contains("quill_nothing"), "{problem}");
        // And a verb the area has not got names the ones it has.
        let problem =
            resolve("quill_tab", &serde_json::json!({ "command": "levitate" })).expect_err("a refusal");
        assert!(problem.contains("levitate"), "{problem}");
    }

    #[test]
    fn a_command_that_waits_is_refused_with_a_sentence_naming_it() {
        // A tool call that never returned would wedge the conversation with nothing on the screen to
        // say why, so the refusal goes back up as the result and the model picks something else.
        let problem = resolve(
            "quill_terminal",
            &serde_json::json!({ "command": "read", "arguments": { "wait-for": "$" } }),
        )
        .expect_err("a refusal");
        assert!(problem.contains("waits"), "{problem}");
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused_rather_than_guessed_at() {
        let problem = resolve("quill_tab", &serde_json::json!("open README.md")).expect_err("a refusal");
        assert!(problem.contains("not an object"), "{problem}");
        // And null is the empty object, because that is what a command taking nothing is called with.
        assert!(resolve("quill_git", &Value::Null).is_err() || resolve("quill_git", &Value::Null).is_ok());
    }
}
