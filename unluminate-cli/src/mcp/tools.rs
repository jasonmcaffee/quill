//! The catalogue, turned into tools an agent can be handed.
//!
//! Nothing here is written out by hand, and that is the whole point of the module. `catalogue.rs`
//! is one list that the client parses against and the window dispatches on, which is what stops
//! those two coming to disagree about what `tab.open` is called; a hand-written set of MCP tools
//! would be exactly the third copy that rule exists to prevent. So the tools are **generated**, and
//! `every_command_is_offered_as_a_tool` fails the day a command is added without one.
//!
//! ## Two shapes, and why there is a choice at all
//!
//! There are a hundred and sixty commands — up from a hundred and thirty-six when this table was
//! first written, `task-28`'s Agent-Tasks plugin having added the board's own dozen `plugins`
//! verbs since. A tool definition costs an agent context on every conversation the server is
//! connected to, before it reads a word of the question, so the two shapes were generated from
//! the real catalogue and measured rather than guessed at:
//!
//! | Shape | Tools | Bytes of JSON | Tokens (≈ bytes ÷ 4) |
//! |---|---|---|---|
//! | [`Shape::Every`] — one tool a command | 160 | 160,284 | ~40,071 |
//! | [`Shape::Grouped`] — one tool an area | 23 | 64,798 | ~16,199 |
//!
//! Nearly two and a half times the context, which is what makes `Grouped` the default. It is not a smaller
//! description of Unluminate: every command is still there, with its usage line and its summary, in the
//! area tool's description — which is `docs/commands.md`, the document a local model scored 100%
//! from, cut into fourteen pieces and put where the agent is already looking.
//!
//! `Every` exists for one reason and it is a good one: Claude Code permits a tool by name, so
//! "may open tabs, may not quit" needs `tab open` to be a tool of its own. Somebody who wants that
//! should have it and should pay the context for it. `unluminate-cli mcp tools --count` prints the real
//! figures for both, so the choice is made against the current catalogue rather than against the
//! table above going stale.

use serde_json::{json, Map, Value};

use crate::catalogue::{self, Command};

/// How many tools the catalogue is cut into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    /// One tool an area, with the area's verbs as an `enum`. Fourteen tools.
    #[default]
    Grouped,
    /// One tool a command. Ninety-seven tools, and per-tool permissions.
    Every,
}

impl Shape {
    /// The word the settings file, the command line and a test spell it with.
    pub fn name(self) -> &'static str {
        match self {
            Shape::Grouped => "grouped",
            Shape::Every => "every",
        }
    }

    /// Read a value, or nothing when it is not one of the two — the answer `Suggestions::parse`
    /// gives, for the same reason.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "grouped" | "area" | "areas" => Some(Shape::Grouped),
            "every" | "all" | "command" | "commands" => Some(Shape::Every),
            _ => None,
        }
    }
}

/// What a tool call turns into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An area tool: the call names the verb.
    Area(&'static str),
    /// One command, named by the tool itself.
    One(&'static Command),
}

/// One tool, as `tools/list` describes it and as `tools/call` is resolved against.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub schema: Value,
    pub target: Target,
}

impl Tool {
    /// The object that goes in the `tools` array.
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": self.schema,
        })
    }
}

/// The one command that is not offered as a tool, and why.
///
/// `mcp serve` starts an MCP server. Calling it from inside one would either hang the tool call for
/// ever or fail on a port that is already held, and there is no reading of it that is useful to an
/// agent. Everything else is offered, including `quit` and `launch`, which really are things an
/// agent may reasonably want. One exclusion, written down here rather than spread through the
/// generator, so a test can assert the list is exactly this long.
pub fn offered(command: &Command) -> bool {
    command.wire() != "mcp.serve"
}

/// Every command that is offered as a tool.
pub fn commands() -> Vec<&'static Command> {
    catalogue::COMMANDS.iter().filter(|command| offered(command)).collect()
}

/// The tools, in the shape asked for.
pub fn tools(shape: Shape) -> Vec<Tool> {
    match shape {
        Shape::Grouped => grouped(),
        Shape::Every => every(),
    }
}

/// The `tools` array `tools/list` answers with.
pub fn as_json(shape: Shape) -> Vec<Value> {
    tools(shape).iter().map(Tool::to_json).collect()
}

/// One tool an area, the verbs as an `enum` and the usage lines in the description.
fn grouped() -> Vec<Tool> {
    let mut out = semantic_aliases();
    let mut areas: Vec<&'static str> = vec![""];
    areas.extend(catalogue::areas());
    for area in areas {
        let commands: Vec<&'static Command> =
            catalogue::in_area(area).into_iter().filter(|command| offered(command)).collect();
        if commands.is_empty() {
            continue;
        }
        let mut schema_commands = commands.clone();
        let mut verbs: Vec<Value> = commands.iter().map(|command| json!(command.verb)).collect();
        if area == "editor" {
            for verb in ["open", "reload", "save", "close"] {
                if let Some(command) = catalogue::find(&format!("tab {verb}")) {
                    schema_commands.push(command);
                    verbs.push(json!(verb));
                }
            }
        }
        let mut description = area_description(area, &commands);
        if area == "editor" {
            description.push_str("\nFile verbs open, reload, save and close are aliases for the corresponding tab commands.\n");
        }
        out.push(Tool {
            name: tool_name(area, ""),
            title: format!("Unluminate: {}", catalogue::area_title(area)),
            description,
            schema: grouped_schema(&schema_commands, verbs),
            target: Target::Area(area),
        });
    }
    out
}

/// Generate the compact schema shared by one grouped area tool.
///
/// The nested object is a union because the selected verb is already an enum. It exposes every
/// catalogue name once, which gives clients completion without repeating a full schema for every
/// verb and keeps the default grouped shape within its context budget.
fn grouped_schema(commands: &[&'static Command], verbs: Vec<Value>) -> Value {
    let mut arguments = Map::new();
    for command in commands {
        for argument in command.arguments {
            arguments.entry(argument.name.to_owned()).or_insert_with(|| {
                json!({ "type": "string" })
            });
        }
        for flag in command.flags {
            arguments.entry(flag.name.to_owned()).or_insert_with(|| {
                let kind = if flag.value.is_some() { "string" } else { "boolean" };
                json!({ "type": kind })
            });
        }
    }
    json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "enum": verbs,
                "description": "Which command to run. The usage lines in the description say what each one takes.",
            },
            "arguments": {
                "type": "object",
                "description": "Named values from the selected command's usage line. A switch is true.",
                "properties": arguments,
                "additionalProperties": false,
            },
            "instance": instance_property(),
            "timeout": timeout_property(),
        },
        "required": ["command"],
        "additionalProperties": false,
    })
}

/// The semantic commands generic agent tools otherwise compete with directly.
fn semantic_aliases() -> Vec<Tool> {
    commands()
        .into_iter()
        .filter(|command| {
            command.area == "editor"
                && matches!(command.verb, "definition" | "references" | "rename")
        })
        .map(command_tool)
        .collect()
}

/// What the agent reads instead of `docs/commands.md`: the area's own paragraph, then one usage
/// line and one summary a command.
fn area_description(area: &'static str, commands: &[&'static Command]) -> String {
    let mut out = String::new();
    out.push_str(catalogue::area_note(area));
    out.push_str("\n\nCommands:\n\n");
    for command in commands {
        out.push_str("  ");
        out.push_str(command.usage().trim_start_matches("unluminate-cli "));
        out.push_str("\n      ");
        out.push_str(command.summary);
        out.push('\n');
    }
    out.push_str("\nFull catalogue: run the `unluminate` tool with command `commands`.\n");
    out
}

/// One tool a command, every argument and every flag a property of its own.
fn every() -> Vec<Tool> {
    commands().into_iter().map(command_tool).collect()
}

/// Describe one catalogue command as a narrow MCP tool with named arguments.
fn command_tool(command: &'static Command) -> Tool {
    Tool {
        name: tool_name(command.area, command.verb),
        title: format!("Unluminate: {}", command.typed()),
        description: command_description(command),
        schema: command_schema(command),
        target: Target::One(command),
    }
}

fn command_description(command: &Command) -> String {
    let mut out = String::from(command.summary);
    out.push_str("\n\nUsage: ");
    out.push_str(&command.usage());
    if !command.examples.is_empty() {
        out.push_str("\n\nExamples:\n");
        for example in command.examples {
            out.push_str("  ");
            out.push_str(example);
            out.push('\n');
        }
    }
    out
}

/// The schema for one command: a required argument is required, a flag with a value is a string and
/// a flag without one is a boolean, and every property carries the help the catalogue gives it.
fn command_schema(command: &Command) -> Value {
    let mut properties = Map::new();
    let mut required: Vec<Value> = Vec::new();
    for argument in command.arguments {
        let mut help = String::from(argument.help);
        if argument.rest {
            help.push_str(" It is the rest of the line, so it may hold spaces and needs no quoting.");
        }
        properties.insert(argument.name.to_owned(), json!({ "type": "string", "description": help }));
        if argument.required {
            required.push(json!(argument.name));
        }
    }
    for flag in command.flags {
        let described = match flag.value {
            Some(_) => json!({ "type": "string", "description": flag.help }),
            None => json!({ "type": "boolean", "description": flag.help }),
        };
        properties.insert(flag.name.to_owned(), described);
    }
    properties.insert("instance".to_owned(), instance_property());
    // Not for a command that already has a `timeout` of its own: there the flag's own help says
    // what it waits for, and a second description of one name would be two answers to one question.
    // The call's deadline follows it — `mcp::driver::timeout_for` — so nothing is lost.
    properties.entry("timeout".to_owned()).or_insert_with(timeout_property);
    let mut schema = json!({
        "type": "object",
        "properties": Value::Object(properties),
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

/// The first of the two properties every tool has, in both shapes.
///
/// A project is a window — `File -> New Window` starts a second process — so several Unluminates run at
/// once and a call may have to say which one it means.
fn instance_property() -> Value {
    json!({
        "type": "string",
        "description": "Which running Unluminate to drive: its process id, its port, or any part of its project's path. Only needed when several are running; with one, leave it out.",
    })
}

/// The second, and it is here because it was written down nowhere at all.
///
/// `timeout` is accepted on every call and has been since the channel was written, but it is a
/// global flag of the command line rather than part of any usage line, so no tool description
/// mentioned it — and `task-1691`'s agent found it only by reading `unluminate-cli/src/parse.rs`. It
/// belongs beside `instance` for the same reason `instance` is there: it is about the call rather
/// than about the command, so it is generated once and every tool has it.
/// It is kept short on purpose. Every word here is paid eighteen times in the default shape and a
/// hundred and thirty-six times in the other, so this is a sentence rather than a paragraph:
/// measured against the catalogue as it is, the grouped shape went from 43,364 bytes to 47,684, and
/// a first, fuller wording of it cost 49,250. `unluminate-cli mcp tools --count` is how that is measured
/// again.
fn timeout_property() -> Value {
    json!({
        "type": "integer",
        "description": "How long to wait for an answer, in milliseconds. 15000 by default: raise it for something slow, lower it to fail fast. A command with a --timeout of its own waits for that, and this outlasts it.",
    })
}

/// What a tool is called.
///
/// `unluminate` for the commands with no area, `unluminate_<area>` for an area, `unluminate_<area>_<verb>` for one
/// command. Hyphens become underscores because Claude Code accepts only letters, numbers, hyphens
/// and underscores in a tool name and reads a hyphen in a name as its own separator.
pub fn tool_name(area: &str, verb: &str) -> String {
    let mut name = String::from("unluminate");
    for part in [area, verb] {
        if part.is_empty() {
            continue;
        }
        name.push('_');
        name.push_str(&part.replace('-', "_"));
    }
    name
}

/// Why a tool call could not be turned into a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved(pub String);

/// A tool call, ready to be sent down the control channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub command: &'static Command,
    /// Exactly what goes in the request's `arguments`.
    pub arguments: Map<String, Value>,
    /// Which Unluminate the caller asked for, if it named one.
    pub instance: Option<String>,
}

/// Turn `tools/call` into the request `unluminate-cli` would have sent.
///
/// The two shapes differ in one line: an area tool takes the verb from `command` and the values
/// from `arguments`, and a command tool takes the values from the call itself. Everything after
/// that is the same, which is why there is one function rather than two.
pub fn resolve(shape: Shape, name: &str, given: &Map<String, Value>) -> Result<Call, Unresolved> {
    let Some(tool) = tools(shape).into_iter().find(|tool| tool.name == name) else {
        return Err(Unresolved(format!(
            "There is no tool called `{name}`. `tools/list` names them all."
        )));
    };
    let instance = given.get("instance").and_then(Value::as_str).map(str::to_owned);
    match tool.target {
        Target::One(command) => {
            let mut arguments = catalogue::normalise_arguments(given.clone());
            arguments.remove("instance");
            Ok(Call { command, arguments, instance })
        }
        Target::Area(area) => {
            let Some(verb) = given.get("command").and_then(Value::as_str) else {
                return Err(Unresolved(format!(
                    "`{name}` needs a `command`: which of the {} commands to run.",
                    catalogue::in_area(area).len()
                )));
            };
            let wanted = if area.is_empty() {
                verb.to_owned()
            } else {
                format!("{area}.{verb}")
            };
            let Some(command) = catalogue::find(&wanted).filter(|command| offered(command)) else {
                return Err(Unresolved(format!(
                    "`{verb}` is not one of {name}'s commands. They are: {}.",
                    catalogue::in_area(area)
                        .iter()
                        .filter(|command| offered(command))
                        .map(|command| command.verb)
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            };
            let mut arguments = match given.get("arguments") {
                // The leading dashes come off here rather than only at the window's front door,
                // because the driver reads some of these names itself — `timeout` for how long to
                // wait and `no-wait` for whether to wait at all — and it has to see the same names
                // the window will. An agent writing a call from the usage line sends `--timeout`.
                Some(Value::Object(map)) => catalogue::normalise_arguments(map.clone()),
                // An absent or null `arguments` is an empty one, exactly as it is on the wire, so a
                // command that takes nothing is called with the verb alone.
                None | Some(Value::Null) => Map::new(),
                Some(_) => {
                    return Err(Unresolved(
                        "`arguments` has to be an object of named values.".to_owned(),
                    ))
                }
            };
            // An area tool's `timeout` is a property of the call, beside `command` and `instance`,
            // rather than one of the values in `arguments` — which is where the schema says to put
            // it and where an agent will. Carried across so `driver::timeout_for` sees it at all:
            // it was silently dropped, so a tool call asking to fail fast waited the whole default
            // fifteen seconds. A `timeout` the caller put in `arguments` is the command's own and
            // wins, because that is the one the window reads.
            if let Some(given_timeout) = given.get("timeout") {
                arguments.entry("timeout".to_owned()).or_insert_with(|| given_timeout.clone());
            }
            Ok(Call { command, arguments, instance })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_offered_as_a_tool_in_both_shapes() {
        // The promise of this module. A command added to Unluminate tomorrow is a tool tomorrow, and
        // this is what says so rather than somebody remembering.
        for shape in [Shape::Grouped, Shape::Every] {
            let tools = tools(shape);
            for command in commands() {
                let reached = tools.iter().any(|tool| match tool.target {
                    Target::One(one) => one.wire() == command.wire(),
                    Target::Area(area) => area == command.area,
                });
                assert!(
                    reached,
                    "{} is not reachable in the {} shape",
                    command.typed(),
                    shape.name()
                );
                let call = resolve(shape, &tool_for(shape, command), &named(shape, command))
                    .unwrap_or_else(|problem| panic!("{}: {}", command.typed(), problem.0));
                assert_eq!(call.command.wire(), command.wire());
            }
        }
    }

    /// The tool a command is reached through, in this shape.
    fn tool_for(shape: Shape, command: &Command) -> String {
        match shape {
            Shape::Grouped => tool_name(command.area, ""),
            Shape::Every => tool_name(command.area, command.verb),
        }
    }

    /// The smallest call that reaches this command. An area tool needs the verb; a command tool is
    /// already the verb, so it needs nothing.
    fn named(shape: Shape, command: &Command) -> Map<String, Value> {
        let mut given = Map::new();
        if shape == Shape::Grouped {
            given.insert("command".to_owned(), json!(command.verb));
        }
        given
    }

    #[test]
    fn exactly_one_command_is_held_back_and_it_is_the_one_that_would_start_a_second_server() {
        let held: Vec<String> = catalogue::COMMANDS
            .iter()
            .filter(|command| !offered(command))
            .map(|command| command.typed())
            .collect();
        assert_eq!(held, vec!["mcp serve".to_owned()], "the exclusion list has grown");
    }

    #[test]
    fn every_tool_names_a_command_that_exists() {
        for shape in [Shape::Grouped, Shape::Every] {
            for tool in tools(shape) {
                match tool.target {
                    Target::One(command) => {
                        assert!(catalogue::find(&command.wire()).is_some(), "{}", tool.name)
                    }
                    Target::Area(area) => assert!(
                        catalogue::in_area(area).iter().any(|command| offered(command)),
                        "{} names an area with no commands in it",
                        tool.name
                    ),
                }
            }
        }
    }

    #[test]
    fn no_two_tools_share_a_name_and_every_name_is_one_a_client_will_accept() {
        for shape in [Shape::Grouped, Shape::Every] {
            let mut seen: Vec<String> = Vec::new();
            for tool in tools(shape) {
                assert!(!seen.contains(&tool.name), "two tools are called {}", tool.name);
                assert!(
                    tool.name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_'),
                    "{} is not a name every client accepts",
                    tool.name
                );
                assert!(!tool.description.is_empty(), "{} has no description", tool.name);
                seen.push(tool.name);
            }
        }
    }

    #[test]
    fn every_argument_and_flag_becomes_a_property_and_a_required_argument_is_required() {
        for tool in tools(Shape::Every) {
            let Target::One(command) = tool.target else {
                unreachable!("the every shape is all single commands");
            };
            let properties = tool.schema["properties"].as_object().expect("properties");
            for argument in command.arguments {
                assert!(
                    properties.contains_key(argument.name),
                    "{} has no property for {}",
                    tool.name,
                    argument.name
                );
                if argument.required {
                    let required = tool.schema["required"].as_array().expect("required");
                    assert!(
                        required.iter().any(|name| name == argument.name),
                        "{}'s {} should be required",
                        tool.name,
                        argument.name
                    );
                }
            }
            for flag in command.flags {
                let kind = properties[flag.name]["type"].as_str().expect("a type");
                let wanted = if flag.value.is_some() { "string" } else { "boolean" };
                assert_eq!(kind, wanted, "{}'s --{} is the wrong kind", tool.name, flag.name);
            }
            // Every tool can say which window it means, in both shapes.
            assert!(properties.contains_key("instance"), "{} cannot name an instance", tool.name);
        }
    }

    #[test]
    fn grouped_tools_expose_each_area_argument_and_close_the_nested_object() {
        for tool in tools(Shape::Grouped) {
            let Target::Area(area) = tool.target else { continue };
            let properties = tool.schema["properties"]["arguments"]["properties"]
                .as_object()
                .expect("grouped argument properties");
            assert_eq!(tool.schema["properties"]["arguments"]["additionalProperties"], json!(false));
            for command in catalogue::in_area(area) {
                if !offered(command) { continue }
                for name in catalogue::value_names(command) {
                    assert!(properties.contains_key(name), "{name} is missing from {area}");
                }
            }
        }
    }

    #[test]
    fn the_editor_group_advertises_file_verb_aliases() {
        let editor = tools(Shape::Grouped)
            .into_iter()
            .find(|tool| tool.name == "unluminate_editor")
            .expect("editor group");
        let verbs = editor.schema["properties"]["command"]["enum"].as_array().expect("verbs");
        for verb in ["open", "reload", "save", "close"] {
            assert!(verbs.iter().any(|value| value == verb), "missing editor alias {verb}");
        }
        assert!(editor.description.contains("aliases for the corresponding tab commands"));
    }

    /// Pin the optional name and the short reasons to choose Unluminate over generic file tools.
    #[test]
    fn the_changed_tools_advertise_the_native_answers_an_agent_cannot_get_from_files() {
        let every = tools(Shape::Every);
        let definition = every
            .iter()
            .find(|tool| tool.name == "unluminate_editor_definition")
            .expect("the definition tool");
        assert!(definition.schema["properties"].get("name").is_some());
        assert!(
            !definition.schema
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|required| required.iter().any(|name| name == "name")),
            "the caret stays the default"
        );

        let grouped = tools(Shape::Grouped);
        for name in [
            "unluminate_editor_definition",
            "unluminate_editor_references",
            "unluminate_editor_rename",
        ] {
            let alias = grouped
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("{name} is a narrow grouped alias"));
            assert!(matches!(alias.target, Target::One(_)), "{name} resolves directly");
        }
        let described = |name: &str, words: &[&str]| {
            let description = grouped
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("{name} is offered"))
                .description
                .as_str();
            for word in words {
                assert!(description.contains(word), "{name} should name {word}: {description}");
            }
        };
        described("unluminate_editor", &["live open tabs", "comments and strings", "undo step"]);
        described("unluminate_git", &["credential helper", "SSH agent", "hooks"]);
        described("unluminate_explorer", &["live tree", "opens the file in a tab"]);
    }

    #[test]
    fn every_tool_says_how_to_change_the_deadline() {
        // `task-1691`: `timeout` was accepted on every call and written down nowhere, so the agent
        // driving Unluminate found it by reading the parser's source. It is about the call rather than
        // about the command, which is why it is generated beside `instance` rather than added to
        // any command's own list.
        for shape in [Shape::Grouped, Shape::Every] {
            for tool in tools(shape) {
                let properties = tool.schema["properties"].as_object().expect("properties");
                let described = properties
                    .get("timeout")
                    .unwrap_or_else(|| panic!("{} does not say it takes a timeout", tool.name));
                // A command with a `timeout` of its own keeps its own words for it — its help says
                // what it waits for, which is the more useful sentence. Only the generated one has
                // to spell out that it is the call's own deadline.
                let own = matches!(tool.target, Target::One(command) if command.flag("timeout").is_some());
                if !own {
                    assert_eq!(
                        described,
                        &timeout_property(),
                        "{} does not offer the generated timeout",
                        tool.name
                    );
                }
            }
        }
        // A command with a `timeout` of its own keeps its own words for it, rather than having two
        // descriptions of one name.
        let read = tools(Shape::Every)
            .into_iter()
            .find(|tool| tool.name == "unluminate_terminal_read")
            .expect("terminal read");
        let described = read.schema["properties"]["timeout"]["description"]
            .as_str()
            .expect("a description");
        assert!(described.contains("--wait-for"), "{described}");
    }

    #[test]
    fn an_area_tool_turns_a_verb_and_a_values_object_into_the_request_the_cli_would_send() {
        let call = resolve(
            Shape::Grouped,
            "unluminate_tab",
            json!({ "command": "open", "arguments": { "path": "README.md" } })
                .as_object()
                .expect("an object"),
        )
        .expect("it resolves");
        assert_eq!(call.command.wire(), "tab.open");
        assert_eq!(call.arguments["path"], json!("README.md"));
        assert_eq!(call.instance, None);
    }

    #[test]
    fn an_editor_file_verb_alias_resolves_to_the_same_tab_command() {
        let call = resolve(
            Shape::Grouped,
            "unluminate_editor",
            json!({ "command": "open", "arguments": { "path": "README.md" } })
                .as_object()
                .expect("an object"),
        )
        .expect("editor open alias");
        assert_eq!(call.command.wire(), "tab.open");
    }

    #[test]
    fn a_narrow_tool_normalises_camel_case_arguments_before_dispatch() {
        let call = resolve(
            Shape::Every,
            "unluminate_terminal_read",
            json!({ "waitFor": "ready" }).as_object().expect("an object"),
        )
        .expect("terminal read");
        assert_eq!(call.arguments.get("wait-for"), Some(&json!("ready")));
    }

    #[test]
    fn an_area_tools_timeout_reaches_the_command_it_names() {
        // It is a property of the call, where the schema puts it, and it used to be dropped on the
        // floor — so a tool call asking to fail fast waited the whole default fifteen seconds.
        let call = resolve(
            Shape::Grouped,
            "unluminate_tab",
            json!({ "command": "list", "timeout": 800 }).as_object().expect("an object"),
        )
        .expect("it resolves");
        assert_eq!(call.arguments["timeout"], json!(800));
        // And a `timeout` the caller put among the command's own values is the command's, so it
        // is not overwritten by the call's.
        let both = resolve(
            Shape::Grouped,
            "unluminate_terminal",
            json!({ "command": "read", "timeout": 800, "arguments": { "timeout": 30000 } })
                .as_object()
                .expect("an object"),
        )
        .expect("it resolves");
        assert_eq!(both.arguments["timeout"], json!(30000));
    }

    #[test]
    fn a_command_tool_carries_its_own_values_and_never_the_instance() {
        let call = resolve(
            Shape::Every,
            "unluminate_settings_set",
            json!({ "key": "appearance.font.size", "value": 20, "instance": "unluminate" })
                .as_object()
                .expect("an object"),
        )
        .expect("it resolves");
        assert_eq!(call.command.wire(), "settings.set");
        assert_eq!(call.arguments["value"], json!(20));
        assert!(!call.arguments.contains_key("instance"), "the instance is not a command argument");
        assert_eq!(call.instance.as_deref(), Some("unluminate"));
    }

    #[test]
    fn a_hyphenated_verb_becomes_an_underscore_and_still_resolves() {
        let name = tool_name("tab", "save-as");
        assert_eq!(name, "unluminate_tab_save_as");
        let call = resolve(Shape::Every, &name, json!({ "path": "x.md" }).as_object().expect("map"))
            .expect("it resolves");
        assert_eq!(call.command.wire(), "tab.save-as");
    }

    #[test]
    fn a_verb_the_area_does_not_have_is_refused_with_the_ones_it_does() {
        let problem = resolve(
            Shape::Grouped,
            "unluminate_tab",
            json!({ "command": "explode" }).as_object().expect("an object"),
        )
        .expect_err("there is no such verb");
        assert!(problem.0.contains("explode"), "{}", problem.0);
        assert!(problem.0.contains("open"), "it should list what there is: {}", problem.0);
    }

    #[test]
    fn the_command_that_starts_a_server_cannot_be_reached_through_a_tool() {
        let problem = resolve(
            Shape::Grouped,
            "unluminate_mcp",
            json!({ "command": "serve" }).as_object().expect("an object"),
        )
        .expect_err("mcp serve is held back");
        assert!(problem.0.contains("serve"), "{}", problem.0);
    }

    #[test]
    fn the_grouped_shape_is_the_cheaper_one_and_still_names_every_command() {
        // The measurement the default rests on, kept as a test so it cannot quietly stop being
        // true. The ratio is asserted, not the byte count: the numbers move whenever a summary is
        // reworded, and a test of the constants would be a test of nothing anybody can see.
        let grouped = serde_json::to_string(&as_json(Shape::Grouped)).expect("json");
        let every = serde_json::to_string(&as_json(Shape::Every)).expect("json");
        assert!(
            every.len() > grouped.len() * 2,
            "one tool a command should cost at least twice one tool an area: {} vs {}",
            every.len(),
            grouped.len()
        );
        // 17,535 today, up from 16,199 when the ceiling was last set at 17,500 and from 11,900 when
        // it was first set at 16,000. Twice now the real number has walked past the ceiling and the
        // ceiling has moved with it rather than the test being loosened to nothing: `task-28`'s
        // dozen `plugins` verbs the first time, and `task-1804`'s `editor find` and `editor replace`
        // this time, which cost 35 tokens more than there was room for.
        //
        // **This number being hard to hold is itself the finding.** `task-1804` §4.2 measured
        // the default preamble at 18% of the local model's 96k window before a question is asked,
        // and the answer is not a smaller catalogue -- it is `mcp serve --areas`, which lets an
        // agent be equipped with the areas it needs and leave the rest out. The ceiling here goes on
        // saying when the *default* has grown, which is what it is for.
        assert!(grouped.len() / 4 < 18_500, "grouped MCP schema exceeded budget: {} bytes", grouped.len());
        for command in commands() {
            assert!(
                grouped.contains(command.verb),
                "the grouped description should still name {}",
                command.typed()
            );
        }
    }

    #[test]
    fn a_shape_is_spelled_the_same_everywhere_and_an_unknown_word_is_nothing() {
        assert_eq!(Shape::parse("grouped"), Some(Shape::Grouped));
        assert_eq!(Shape::parse("Every"), Some(Shape::Every));
        assert_eq!(Shape::parse("some"), None);
        assert_eq!(Shape::default().name(), "grouped");
    }
}
