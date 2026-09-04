//! Finding an Unluminate to talk to, and starting one.
//!
//! Everything here is about the connection rather than about the commands: which of the running
//! windows a request goes to, what to say when the answer is none or too many, and how to start a
//! window and wait until it is ready to be driven.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::instances::{self, Instance};
use crate::protocol::{self, code, Reply, Request};

/// How long to wait for a reply when nobody said.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(15_000);
/// How long to wait for a new window to start answering.
pub const DEFAULT_LAUNCH_TIMEOUT: Duration = Duration::from_millis(20_000);
/// How long a liveness probe is given. It is a connect on the loopback interface, which either
/// answers at once or is not there.
const PROBE: Duration = Duration::from_millis(400);

/// Why a request could not be sent.
#[derive(Debug, Clone)]
pub struct Unreachable {
    pub code: &'static str,
    pub message: String,
}

impl Unreachable {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// The Unluminates that are running, stale files swept as they are found.
///
/// A file naming a port nothing answers on is a window that was killed rather than closed. It is
/// removed here rather than on a timer, because the only moment it matters is the moment somebody
/// is looking at the list.
pub fn running() -> Vec<Instance> {
    let mut alive = Vec::new();
    for instance in instances::listed() {
        if answers(&instance) {
            alive.push(instance);
        } else {
            instances::forget(&instance);
        }
    }
    alive
}

/// True when this instance's window is still there.
///
/// **The process is asked first, and the port only if the process is there.** Dialling the port is
/// the authority — a process id can be handed to something else entirely — but it is also the only
/// slow thing here, and a window that has gone has no process to have inherited its id. `task-1805`
/// measured what asking in the other order cost: a dead loopback port on this machine does not
/// answer with a refusal, it answers with nothing, so one stale file spent the whole [`PROBE`] —
/// 431 ms on the next `unluminate-cli` command and 414 ms of the next window's startup.
///
/// A killed Unluminate leaves a stale file behind, and Unluminate is killed rather than closed often
/// enough that this was the ordinary case rather than the rare one.
fn answers(instance: &Instance) -> bool {
    if !instances::is_running(instance.pid) {
        return false;
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], instance.port));
    std::net::TcpStream::connect_timeout(&address, PROBE).is_ok()
}

/// The instance a request should go to.
///
/// With one running Unluminate, that one. With several and nothing named, a refusal that lists them:
/// guessing which window somebody meant would be a command landing in the wrong project, which is
/// exactly the mistake that is hard to notice. `--instance` accepts a process id, a port, or any
/// part of a project's path, because those are the three things somebody looking at the list has.
pub fn choose(wanted: Option<&str>) -> Result<Instance, Unreachable> {
    let alive = running();
    let Some(wanted) = wanted else {
        return match alive.len() {
            0 => Err(Unreachable::new(
                code::NOT_RUNNING,
                "No Unluminate is running. Start one with `unluminate-cli launch <folder>`.",
            )),
            1 => Ok(alive.into_iter().next().expect("one")),
            _ => Err(Unreachable::new(
                code::SEVERAL,
                format!(
                    "{} Unluminates are running, so say which one with --instance:\n{}",
                    alive.len(),
                    alive
                        .iter()
                        .map(|instance| format!(
                            "  --instance {}   port {}   {}",
                            instance.pid,
                            instance.port,
                            instance.folder.display()
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            )),
        };
    };
    let matched: Vec<Instance> = alive
        .iter()
        .filter(|instance| {
            instance.pid.to_string() == wanted
                || instance.port.to_string() == wanted
                || instance
                    .folder
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&wanted.to_lowercase())
        })
        .cloned()
        .collect();
    match matched.len() {
        0 => Err(Unreachable::new(
            code::NOT_RUNNING,
            format!("No running Unluminate matches --instance {wanted}."),
        )),
        1 => Ok(matched.into_iter().next().expect("one")),
        _ => Err(Unreachable::new(
            code::SEVERAL,
            format!("--instance {wanted} matches {} running Unluminates.", matched.len()),
        )),
    }
}

/// Send one command to one instance.
pub fn ask(
    instance: &Instance,
    command: &str,
    arguments: Map<String, Value>,
    timeout: Duration,
) -> Result<Reply, Unreachable> {
    let request = Request::new(&instance.token, command, arguments);
    protocol::ask(instance.port, &request, timeout).map_err(|problem| {
        if problem.kind() == std::io::ErrorKind::WouldBlock
            || problem.kind() == std::io::ErrorKind::TimedOut
        {
            Unreachable::new(
                code::TIMED_OUT,
                format!("Unluminate did not answer {command} within {} ms.", timeout.as_millis()),
            )
        } else {
            Unreachable::new(
                code::NOT_RUNNING,
                format!(
                    "Could not reach the Unluminate on port {}: {problem}. It may have just closed.",
                    instance.port
                ),
            )
        }
    })
}

/// Where the `unluminate` program is.
///
/// Next to this one first, because an installed Unluminate puts them in the same folder and a build puts
/// them in the same `target` folder, so that answer is right without anything being configured.
/// `UNLUMINATE_BIN` overrides it, and the last resort is whatever is on the path.
pub fn unluminate_program() -> PathBuf {
    if let Some(named) = std::env::var_os("UNLUMINATE_BIN") {
        return PathBuf::from(named);
    }
    let name = if cfg!(windows) { "unluminate.exe" } else { "unluminate" };
    if let Some(beside) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|f| f.join(name))) {
        if beside.is_file() {
            return beside;
        }
    }
    PathBuf::from(name)
}

/// Start an Unluminate on `folder`, and wait until it is answering.
///
/// Waiting is the point. A script that starts a window and sends it a command straight away used to
/// race the window's first frame; this returns when the new Unluminate has written its instance file and
/// answers on the port in it, so the next command in the script cannot be too early.
pub fn launch(
    folder: &Path,
    timeout: Duration,
    wait: bool,
) -> Result<(Option<Instance>, u32), Unreachable> {
    let before: Vec<u32> = instances::listed().iter().map(|instance| instance.pid).collect();
    let program = unluminate_program();
    let child = std::process::Command::new(&program)
        .arg(folder)
        .spawn()
        .map_err(|problem| {
            Unreachable::new(
                code::FAILED,
                format!(
                    "Could not start {}: {problem}. Name it with UNLUMINATE_BIN if it is somewhere else.",
                    program.display()
                ),
            )
        })?;
    let pid = child.id();
    if !wait {
        return Ok((None, pid));
    }
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        for instance in instances::listed() {
            let fresh = !before.contains(&instance.pid);
            if (instance.pid == pid || fresh) && answers(&instance) {
                return Ok((Some(instance), pid));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(Unreachable::new(
        code::TIMED_OUT,
        format!(
            "Unluminate was started but had not begun answering within {} ms. \
             It may still be loading fonts; try `unluminate-cli instances`.",
            timeout.as_millis()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_beside_this_one_is_preferred_and_can_be_named() {
        let previous = std::env::var_os("UNLUMINATE_BIN");
        std::env::set_var("UNLUMINATE_BIN", "/somewhere/else/unluminate");
        assert_eq!(unluminate_program(), PathBuf::from("/somewhere/else/unluminate"));
        match previous {
            Some(value) => std::env::set_var("UNLUMINATE_BIN", value),
            None => std::env::remove_var("UNLUMINATE_BIN"),
        }
    }

    #[test]
    fn no_running_unluminate_is_a_refusal_that_says_how_to_start_one() {
        let folder = std::env::temp_dir().join("unluminate-cli-choose-none");
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        let previous = std::env::var_os("UNLUMINATE_INSTANCES");
        std::env::set_var("UNLUMINATE_INSTANCES", &folder);
        let problem = choose(None).expect_err("nothing is running");
        assert_eq!(problem.code, code::NOT_RUNNING);
        assert!(problem.message.contains("unluminate-cli launch"), "{}", problem.message);
        match previous {
            Some(value) => std::env::set_var("UNLUMINATE_INSTANCES", value),
            None => std::env::remove_var("UNLUMINATE_INSTANCES"),
        }
        std::fs::remove_dir_all(&folder).ok();
    }
}
