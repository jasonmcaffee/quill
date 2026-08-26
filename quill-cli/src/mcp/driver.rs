//! Finding the Quill a tool call is for, and sending it there.
//!
//! This is the only part of the MCP server that knows a Quill exists. It sends exactly what
//! `quill-cli` sends — the same wire name, the same arguments object, the same token out of the
//! same instance file, to the same port — so `QuillApp::run_cli` stays the one place a command
//! turns into a change and a command run by an agent is the same command a person types.
//!
//! ## Which window
//!
//! A project is a window, so several Quills run at once and something has to choose. In order,
//! first answer wins:
//!
//! 1. What the tool call said in `instance`, or `--instance` on `mcp serve`, or `QUILL_INSTANCE` in
//!    the environment. A process id, a port, or part of a project's path — the three things
//!    `client::choose` already accepts.
//! 2. Exactly one Quill running: that one.
//! 3. Exactly one running Quill whose project folder is this server's own **preference** — the
//!    folder it was started in, or `CLAUDE_PROJECT_DIR` which Claude Code sets in a spawned
//!    server's environment, or the window's own folder when a window is hosting the server.
//! 4. Otherwise the refusal that lists them. Guessing which window somebody meant is a command
//!    landing in the wrong project, which is exactly the mistake that is hard to notice.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use crate::catalogue::Command;
use crate::client;
use crate::mcp::server::{Driver, Failure};
use crate::protocol::{code, Reply};

/// How long a tool call waits when the command itself did not say.
///
/// The same fifteen seconds `quill-cli` uses, extended by whatever the command was told to wait
/// for, for the same reason: the window's own timeout should be the one that fires, so that what
/// comes back says what it was waiting for rather than "no answer".
const DEFAULT_TIMEOUT: Duration = client::DEFAULT_TIMEOUT;

/// The driver a real server uses.
pub struct Quills {
    /// The project this server would rather drive when several are running. See the module comment.
    preference: Option<PathBuf>,
    /// A window named on the command line or in the environment, which beats everything.
    named: Option<String>,
}

impl Quills {
    /// A driver with the preference a spawned server has: what it was told, then what the agent
    /// told it, then wherever it was started.
    pub fn new(named: Option<String>) -> Self {
        let named = named.or_else(|| non_empty(std::env::var("QUILL_INSTANCE").ok()));
        Self { preference: preferred_folder(), named }
    }

    /// A driver for a window hosting the server itself, which prefers its own project.
    pub fn for_window(folder: PathBuf) -> Self {
        Self { preference: Some(folder), named: None }
    }

    /// The instance a call goes to.
    fn choose(&self, asked: Option<&str>) -> Result<crate::instances::Instance, Failure> {
        let wanted = asked.or(self.named.as_deref());
        match client::choose(wanted) {
            Ok(instance) => Ok(instance),
            Err(problem) => {
                // Several are running and none was named. Before repeating the client's refusal,
                // see whether this server has a preference that picks exactly one of them out.
                if problem.code == code::SEVERAL && wanted.is_none() {
                    if let Some(instance) = self.preferred_among(&client::running()) {
                        return Ok(instance);
                    }
                }
                Err(Failure { code: problem.code, message: problem.message })
            }
        }
    }

    /// The one running Quill on the folder this server prefers, when there is exactly one.
    fn preferred_among(
        &self,
        running: &[crate::instances::Instance],
    ) -> Option<crate::instances::Instance> {
        let wanted = self.preference.as_ref()?;
        let mut matched = running.iter().filter(|instance| same_folder(&instance.folder, wanted));
        let first = matched.next()?.clone();
        matched.next().is_none().then_some(first)
    }
}

impl Driver for Quills {
    fn run(
        &self,
        command: &'static Command,
        arguments: Map<String, Value>,
        instance: Option<&str>,
    ) -> Result<Reply, Failure> {
        // The commands the client answers on its own are answered here too, without a Quill. It is
        // what makes `commands` and `instances` usable at the start of a conversation, before the
        // agent knows whether anything is running.
        if let Some(reply) = locally(command, &arguments) {
            return Ok(reply);
        }
        let instance = self.choose(instance)?;
        let timeout = timeout_for(&arguments);
        client::ask(&instance, &command.wire(), arguments, timeout)
            .map_err(|problem| Failure { code: problem.code, message: problem.message })
    }

    fn read_file(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }
}

/// The commands that need no window, answered from the catalogue and the instance files.
///
/// `launch` is deliberately not here: it starts a process, and it is `client::launch`'s job rather
/// than a lookup, so it goes down the same road as everything else — which for a local command
/// means it is answered by [`launched`] below.
fn locally(command: &'static Command, arguments: &Map<String, Value>) -> Option<Reply> {
    match command.wire().as_str() {
        "version" => Some(Reply::done(
            "version",
            format!("quill-cli {}", crate::VERSION),
            serde_json::json!({ "version": crate::VERSION }),
        )),
        "commands" => {
            let only = arguments.get("name").and_then(Value::as_str);
            if let Some(name) = only {
                if crate::catalogue::find(name).is_none() {
                    return Some(Reply::failed(
                        "commands",
                        code::NOT_FOUND,
                        format!("There is no command called `{name}`."),
                    ));
                }
            }
            Some(Reply::done("commands", "Quill's commands", crate::help::as_json(only)))
        }
        "instances" => {
            let running = client::running();
            let listed: Vec<Value> = running
                .iter()
                .map(|instance| {
                    serde_json::json!({
                        "pid": instance.pid,
                        "port": instance.port,
                        "folder": instance.folder.to_string_lossy(),
                        "started": instance.started,
                    })
                })
                .collect();
            Some(Reply::done(
                "instances",
                match running.len() {
                    0 => "No Quill is running.".to_owned(),
                    1 => "1 Quill is running.".to_owned(),
                    several => format!("{several} Quills are running."),
                },
                serde_json::json!({ "count": running.len(), "instances": listed }),
            ))
        }
        "launch" => Some(launched(arguments)),
        _ => None,
    }
}

/// Start another Quill and wait until it answers, which is what makes the next tool call safe.
fn launched(arguments: &Map<String, Value>) -> Reply {
    let folder = arguments
        .get("folder")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let timeout = arguments
        .get("timeout")
        .and_then(as_millis)
        .map(Duration::from_millis)
        .unwrap_or(client::DEFAULT_LAUNCH_TIMEOUT);
    let wait = !arguments.contains_key("no-wait");
    match client::launch(&folder, timeout, wait) {
        Ok((Some(instance), _)) => Reply::done(
            "launch",
            format!(
                "Quill {} is running on port {} in {}",
                instance.pid,
                instance.port,
                instance.folder.display()
            ),
            serde_json::json!({
                "pid": instance.pid,
                "port": instance.port,
                "folder": instance.folder.to_string_lossy(),
            }),
        ),
        Ok((None, pid)) => Reply::done(
            "launch",
            format!("Quill was started as process {pid}"),
            serde_json::json!({ "pid": pid }),
        ),
        Err(problem) => Reply::failed("launch", problem.code, problem.message),
    }
}

/// How long to wait for this call.
fn timeout_for(arguments: &Map<String, Value>) -> Duration {
    let waiting = ["timeout", "wait"]
        .iter()
        .filter_map(|name| arguments.get(*name))
        .filter_map(as_millis)
        .max()
        .unwrap_or(0);
    DEFAULT_TIMEOUT.max(Duration::from_millis(waiting + 5_000))
}

fn as_millis(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// The folder this server would rather drive.
///
/// `CLAUDE_PROJECT_DIR` first, because Claude Code sets it in a spawned server's environment and it
/// names the project the person is actually working in — which is a better answer than the working
/// directory a client happened to launch the process from.
fn preferred_folder() -> Option<PathBuf> {
    non_empty(std::env::var("CLAUDE_PROJECT_DIR").ok())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
}

/// Whether two paths name the same folder, as well as this can be known without touching the disk.
///
/// Compared case-insensitively, because Windows paths are, and with a trailing separator ignored.
/// `canonicalize` is deliberately not used: it touches the disk on a path that may be a network
/// share, and choosing which window to drive is not worth a stall.
fn same_folder(one: &Path, other: &Path) -> bool {
    let tidy = |path: &Path| {
        path.to_string_lossy().trim_end_matches(['/', '\\']).replace('\\', "/").to_lowercase()
    };
    tidy(one) == tidy(other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_matches_itself_however_it_is_spelled() {
        assert!(same_folder(Path::new("C:\\jason\\dev\\quill"), Path::new("c:/jason/dev/quill/")));
        assert!(!same_folder(Path::new("C:\\jason\\dev\\quill"), Path::new("C:\\jason\\dev")));
    }

    #[test]
    fn the_commands_that_need_no_window_are_answered_without_one() {
        let commands = crate::catalogue::find("commands").expect("commands");
        let reply = locally(commands, &Map::new()).expect("answered locally");
        assert!(reply.ok);
        assert!(reply.result["commands"].as_array().expect("commands").len() > 50);

        let version = crate::catalogue::find("version").expect("version");
        let reply = locally(version, &Map::new()).expect("answered locally");
        assert_eq!(reply.result["version"], serde_json::json!(crate::VERSION));
    }

    #[test]
    fn a_command_that_needs_a_window_is_not_answered_locally() {
        let open = crate::catalogue::find("tab.open").expect("tab open");
        assert!(locally(open, &Map::new()).is_none());
    }

    #[test]
    fn asking_about_a_command_that_does_not_exist_is_refused_rather_than_answered_emptily() {
        let commands = crate::catalogue::find("commands").expect("commands");
        let mut arguments = Map::new();
        arguments.insert("name".to_owned(), Value::String("tab.explode".to_owned()));
        let reply = locally(commands, &arguments).expect("answered locally");
        assert!(!reply.ok);
    }

    #[test]
    fn a_command_told_to_wait_outlasts_its_own_wait() {
        let mut arguments = Map::new();
        arguments.insert("timeout".to_owned(), Value::String("60000".to_owned()));
        assert!(timeout_for(&arguments) >= Duration::from_millis(65_000));
        assert_eq!(timeout_for(&Map::new()), DEFAULT_TIMEOUT);
    }
}
