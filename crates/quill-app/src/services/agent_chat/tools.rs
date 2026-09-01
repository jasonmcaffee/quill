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

/// The shape the tools are offered in. One an area, for the reason in the module comment.
const SHAPE: Shape = Shape::Grouped;

/// The commands that hand the model a program of its own choosing, held back unless asked for.
///
/// **The one place the chat's trust boundary differs from the MCP server's, and it differs for a
/// reason.** `mcp::tools::offered` holds back one command and offers the rest, because the agent on
/// the other end of an MCP connection is Claude Code running on this machine, launched by the person,
/// with their own credentials. The other end of *this* connection is a server: it is handed the tools
/// by a switch in a composer, and `terminal send` would let it type `echo $ANTHROPIC_API_KEY` and
/// then read the answer back with `terminal read`. The key Quill takes such care never to write down
/// would be in the transcript.
///
/// So these are refused with a sentence naming the setting, and `chat.shell` in the Settings page is
/// how somebody who wants an agent that can really drive the machine says so. They are **refused
/// rather than hidden**, because the tools are generated from the catalogue and filtering the
/// generator would be a second generator; and because a model told why can say so, where a model that
/// simply cannot see the command tries something worse — which is `task-1695`'s own finding.
///
/// Written down here with the reason for each, and `every_command_that_runs_a_program_is_held_back`
/// fails if the list drifts from what the catalogue holds.
pub const RUNS_A_PROGRAM: &[&str] = &[
    // Types into a shell, which is the whole machine and the whole environment with it.
    "terminal.send",
    // A run configuration is a command line, so adding or starting one runs a program.
    "run.add",
    "run.start",
    "run.rerun",
    // Starts a package manager against the network.
    "debug.install",
    // Starts another Quill on a folder of the model's choosing.
    "launch",
];

/// Whether `command` is one that hands the model a program of its own choosing.
pub fn runs_a_program(command: &quill_cli::catalogue::Command) -> bool {
    RUNS_A_PROGRAM.contains(&command.wire().as_str())
}

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
pub fn resolve(name: &str, arguments: &Value, shell: bool) -> Result<Resolved, String> {
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
    if !shell && runs_a_program(call.command) {
        return Err(format!(
            "`{}` runs a program of your choosing, and this window does not offer that to a model. \
             Switch on `Let the model run programs` in Settings -> Agent-Chat if you want it.",
            call.command.wire()
        ));
    }
    if let Some(flag) = asks_to_wait(&call.arguments) {
        return Err(format!(
            "`{}` was asked to wait with `--{flag}`, and a tool call cannot wait: it is one turn of a              conversation rather than a script. Ask for it without waiting.",
            call.command.wire()
        ));
    }
    Ok(Resolved {
        command: call.command,
        arguments: call.arguments,
    })
}

/// The waiting flag this call asked for, if it asked for one.
///
/// **The call rather than the command**, which is the difference between refusing `terminal read
/// --wait-for '$'` and refusing `terminal read` altogether. An earlier version refused any command
/// that *has* a waiting flag, which took `terminal read`, `run output` and `debug start` away from a
/// model that only wanted to read something — the opposite of `task-1695`'s own finding, that a
/// command an agent cannot reach is a command it works round with its own tools.
///
/// A command that holds the window for a reason of its own — a screenshot waiting for the window to
/// settle, a git action waiting for the worker — is caught by `QuillApp::run_cli_for_a_plugin`
/// instead, which refuses an `Outcome::Hold` with the same kind of sentence. This is the gate and
/// that is the backstop.
fn asks_to_wait(arguments: &Map<String, Value>) -> Option<&str> {
    arguments
        .iter()
        .filter(|(_, value)| !matches!(value, Value::Bool(false) | Value::Null))
        .map(|(name, _)| name.as_str())
        .find(|name| *name == "wait" || name.starts_with("wait-"))
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
            false,
        )
        .expect("resolved");
        assert_eq!(resolved.command.wire(), "tab.open");
        assert_eq!(resolved.arguments["path"], "README.md");
        // The leading dashes come off, which is what `task-1691` found was silently dropping flags.
        assert_eq!(resolved.arguments["permanent"], true);
    }

    #[test]
    fn a_tool_that_does_not_exist_is_a_sentence_rather_than_a_panic() {
        let problem = resolve("quill_nothing", &serde_json::json!({}), false).expect_err("a refusal");
        assert!(problem.contains("quill_nothing"), "{problem}");
        // And a verb the area has not got names the ones it has.
        let problem = resolve("quill_tab", &serde_json::json!({ "command": "levitate" }), false)
            .expect_err("a refusal");
        assert!(problem.contains("levitate"), "{problem}");
    }

    #[test]
    fn a_call_that_asks_to_wait_is_refused_and_the_same_command_without_it_is_not() {
        // A tool call that never returned would wedge the conversation with nothing on the screen to
        // say why, so the refusal goes back up as the result and the model picks something else. But
        // the refusal is about the **call**: reading a terminal without waiting is a thing a model
        // should be able to do, and refusing the whole command is what makes an agent reach for its
        // own tools instead — `task-1695`'s own finding.
        let problem = resolve(
            "quill_terminal",
            &serde_json::json!({ "command": "read", "arguments": { "wait-for": "$" } }),
            false,
        )
        .expect_err("a refusal");
        assert!(problem.contains("wait-for"), "{problem}");
        let allowed = resolve("quill_terminal", &serde_json::json!({ "command": "read" }), false)
            .expect("reading without waiting is allowed");
        assert_eq!(allowed.command.wire(), "terminal.read");
        // A switch given as `false` is not asking to wait either.
        assert!(resolve(
            "quill_run",
            &serde_json::json!({ "command": "output", "arguments": { "wait-for": false } }),
            false,
        )
        .is_ok());
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused_rather_than_guessed_at() {
        let problem =
            resolve("quill_tab", &serde_json::json!("open README.md"), false).expect_err("a refusal");
        assert!(problem.contains("not an object"), "{problem}");
        // And null **is** the empty object, because that is what a command taking nothing is called
        // with. An earlier version of this line asserted `is_err() || is_ok()`, which is a tautology
        // and tested nothing at all.
        let area = resolve("quill_git", &Value::Null, false).expect_err("an area tool needs a verb");
        assert!(area.contains("needs a `command`"), "{area}");
        let no_arguments =
            resolve("quill_git", &serde_json::json!({ "command": "status" }), false).expect("resolved");
        assert!(no_arguments.arguments.is_empty(), "nothing said is nothing sent");
    }

    #[test]
    fn a_command_that_runs_a_program_is_held_back_until_the_second_switch_is_on() {
        // The one place this trust boundary differs from the MCP server's, and it differs because the
        // other end of *this* connection is a server rather than an agent on this machine. See
        // `RUNS_A_PROGRAM`.
        let problem = resolve(
            "quill_terminal",
            &serde_json::json!({ "command": "send", "arguments": { "text": "echo $ANTHROPIC_API_KEY" } }),
            false,
        )
        .expect_err("a refusal");
        assert!(problem.contains("runs a program"), "{problem}");
        assert!(
            problem.contains("Settings"),
            "the refusal says what to do about it: {problem}"
        );
        // With the switch on it is offered like anything else.
        let allowed = resolve(
            "quill_terminal",
            &serde_json::json!({ "command": "send", "arguments": { "text": "ls" } }),
            true,
        )
        .expect("allowed once somebody asked for it");
        assert_eq!(allowed.command.wire(), "terminal.send");
        // And reading is never held back, because reading runs nothing.
        assert!(resolve("quill_terminal", &serde_json::json!({ "command": "read" }), false).is_ok());
    }

    #[test]
    fn every_command_held_back_is_a_command_the_catalogue_really_has() {
        // A name that drifted would hold nothing back and say nothing about it, which is the quiet
        // half of a trust boundary going wrong.
        for name in RUNS_A_PROGRAM {
            assert!(
                quill_cli::catalogue::find(name).is_some(),
                "`{name}` is held back and the catalogue has no such command"
            );
        }
    }
}
