//! Where a running Unluminate says how to reach it, and how the client finds one.
//!
//! An Unluminate that is listening writes one small file into `<settings folder>/instances`, named after
//! its process id. The file holds the port it is listening on, the token a request has to carry,
//! the folder it has open and when it started. It is the same `name = value` format the settings
//! file uses, so it can be read by eye and by three lines of any language.
//!
//! ```text
//! # A running Unluminate. Written by Unluminate, removed when it stops.
//! folder = C:\jason\dev\unluminate
//! pid = 24196
//! port = 51234
//! started = 1756139112
//! token = 4f1a...
//! ```
//!
//! ## Why a file rather than a fixed port
//!
//! Several Unluminates run at once — a project is a window, and `File -> New Window` starts a second
//! process — so a fixed port would be a fixed collision. Each window asks the operating system for
//! a free port, and the file is how the client is told which one. It is also how `unluminate-cli
//! instances` can list every running Unluminate without talking to any of them.
//!
//! ## Why a token
//!
//! The listener is on `127.0.0.1`, so nothing off the machine can reach it, but every program the
//! person is running can. The token is a per-run secret in a file only they can read, and a request
//! without it is refused. It is not protection against somebody who is already running as them —
//! nothing on a desktop is — but it stops a page in a browser, which can post to a loopback port
//! but cannot read a file, from driving the editor.
//!
//! ## Stale files
//!
//! An Unluminate that is killed rather than closed leaves its file behind. Nothing sweeps them on a
//! timer: `client::running` treats a file whose window is not there as not being an instance, and
//! removes it. That is the only sweep there is, and it happens when somebody is looking.
//!
//! **What "not there" means is the process, not the port**, and `task-1805` is what that is worth.
//! The question used to be answered by dialling the recorded port with a 400 ms timeout — and a dead
//! loopback port does not always answer with a refusal. Measured on this machine it answers with
//! nothing at all, so a single stale file cost the **whole** timeout: **431 ms on the next
//! `unluminate-cli` command**, and **414 ms of the next window's startup**, which was a third of it.
//! Unluminate is killed rather than closed routinely — the task manager, a crash, a script — so this was
//! not a rare path.
//!
//! [`is_running`] asks the operating system instead, which takes microseconds and is the question
//! that was actually being asked. The port is still dialled when the process **is** there, because a
//! live process id can have been handed to something else entirely and only the port can settle
//! that; what has gone is paying a network timeout to learn something the process table knows.

use std::path::{Path, PathBuf};

/// Whether a process with this id is running.
///
/// The fast half of "is that window still there". A process that has gone is a window that has gone,
/// with no room for doubt and nothing to wait for; a process that is there might be an unrelated
/// program that inherited the id, which is why the caller still dials the port.
///
/// `kill(pid, 0)` sends no signal — it only reports whether the process exists and whether this user
/// may signal it, which is exactly the question, and it cannot affect the process. `OpenProcess` with
/// `PROCESS_QUERY_LIMITED_INFORMATION` is Windows' equivalent: it opens no access to affect the
/// process, only to ask whether it is there.
pub fn is_running(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    running_process(pid)
}


/// A zombie answers "yes" here too — `kill(pid, 0)` succeeds until the parent reaps it — and that is
/// left alone. An Unluminate window is started from a desktop, a terminal or another Unluminate, and
/// each of those reaps or is itself long gone; the Windows case above is the one that was measured
/// failing, because a process object there outlives the process for as long as any handle exists.
#[cfg(unix)]
fn running_process(pid: u32) -> bool {
    // Safety: signal 0 is the no-op form `kill` documents for existence checks.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// **Opening a handle is not enough, and that was measured rather than reasoned about.**
///
/// Windows keeps a process *object* alive for as long as anything holds a handle to it, so a
/// process that has been killed is still openable — by its parent shell, by a task manager, by
/// whatever started it. `OpenProcess` on one of those succeeds and answers "yes", which is exactly
/// the case this whole function exists to catch: the window that was killed a moment ago.
///
/// The measurement: with the process asked about only by `OpenProcess`, a run of the startup
/// harness answered in **1399 ms** where the first run of the batch — the only one with no
/// just-killed window behind it — answered in 937. Every later run was paying the port probe's whole
/// 400 ms for a window that had been dead for half a second.
///
/// So the exit code is what is asked. A process still running has none, and Windows says so with
/// `STILL_ACTIVE`. A process that really did exit with 259 is read as running and falls through to
/// the port probe, which is what this replaced — so the ambiguity Windows is known for costs a
/// little time in a rare case and can never give a wrong answer.
#[cfg(windows)]
fn running_process(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // Safety: the handle carries no permission to affect the process, only to ask about it, and it
    // is closed as soon as it has been read. `GetExitCodeProcess` writes one `u32`.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let read = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        // A handle that will not answer is a process nothing can be said about, and the port probe
        // is the honest place to settle that.
        read == 0 || code == STILL_ACTIVE as u32
    }
}

#[cfg(not(any(unix, windows)))]
fn running_process(_pid: u32) -> bool {
    // Nowhere else is a target here. Answering "yes" leaves the port probe as the only judge, which
    // is exactly the behaviour this replaced, so a platform with no answer loses nothing.
    true
}


/// What one running Unluminate advertises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub folder: PathBuf,
    /// Seconds since the epoch, so the newest window can be told from the oldest.
    pub started: u64,
}

impl Instance {
    /// Read one from the `name = value` text of an instance file.
    ///
    /// `None` when a value is missing or will not parse, which is what a half-written file looks
    /// like: the client then treats it as no instance rather than as a broken one.
    pub fn parse(text: &str) -> Option<Self> {
        let mut pid = None;
        let mut port = None;
        let mut token = None;
        let mut folder = None;
        let mut started = 0;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match name.trim() {
                "pid" => pid = value.parse().ok(),
                "port" => port = value.parse().ok(),
                "token" => token = Some(value.to_owned()),
                "folder" => folder = Some(PathBuf::from(value)),
                "started" => started = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        Some(Instance {
            pid: pid?,
            port: port?,
            token: token?,
            folder: folder.unwrap_or_default(),
            started,
        })
    }

    /// The text of the file, which is what the window writes.
    pub fn to_text(&self) -> String {
        format!(
            "# A running Unluminate. Written by Unluminate, removed when it stops.\n\
             folder = {}\n\
             pid = {}\n\
             port = {}\n\
             started = {}\n\
             token = {}\n",
            self.folder.display(),
            self.pid,
            self.port,
            self.started,
            self.token
        )
    }

    /// Where this instance's file goes.
    pub fn path_in(&self, folder: &Path) -> PathBuf {
        folder.join(format!("{}.conf", self.pid))
    }
}

/// The folder the instance files live in.
///
/// Inside the folder Unluminate already keeps its settings in, so there is one place per person that
/// belongs to Unluminate rather than two. `UNLUMINATE_INSTANCES` names another folder, which is what the
/// tests use so that they never see the person's real windows.
pub fn folder() -> PathBuf {
    if let Some(named) = std::env::var_os("UNLUMINATE_INSTANCES") {
        return PathBuf::from(named);
    }
    settings_folder().join("instances")
}

/// Where the operating system expects an application to keep its settings.
///
/// This is deliberately the same rule as `unluminate_app::services::store::settings_folder`, and there
/// is a test in `unluminate-app` that fails if the two ever come to disagree. It is written twice
/// because this crate has to answer it with nothing of the window behind it: the client is a small
/// program with no graphics card and no fonts, and making it depend on the window to find a folder
/// would be the wrong way round.
pub fn settings_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return home.join("Library/Application Support/Unluminate");
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(data) = std::env::var_os("APPDATA") {
            return PathBuf::from(data).join("Unluminate");
        }
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("unluminate");
    }
    match home {
        Some(home) => home.join(".config/unluminate"),
        None => PathBuf::from(".unluminate"),
    }
}

/// Every instance file that can be read, newest first.
///
/// Nothing is checked here beyond the file parsing. Whether the Unluminate it names is still running is
/// [`running`]'s question, because answering it means opening a connection.
pub fn listed() -> Vec<Instance> {
    listed_in(&folder())
}

/// The same, in a named folder, which is how a test looks at instances of its own.
pub fn listed_in(folder: &Path) -> Vec<Instance> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut out: Vec<Instance> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("conf") {
            continue;
        }
        if let Some(instance) = std::fs::read_to_string(&path).ok().and_then(|t| Instance::parse(&t)) {
            out.push(instance);
        }
    }
    out.sort_by(|a, b| b.started.cmp(&a.started).then(b.pid.cmp(&a.pid)));
    out
}

/// Remove an instance's file, which an Unluminate does as it stops and the client does when it finds one
/// that no longer answers.
pub fn forget(instance: &Instance) {
    let _ = std::fs::remove_file(instance.path_in(&folder()));
}

/// Seconds since the epoch, or zero if the clock will not say.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_survives_being_written_and_read_back() {
        let instance = Instance {
            pid: 24196,
            port: 51234,
            token: "4f1abc".to_owned(),
            folder: PathBuf::from("/projects/book"),
            started: 1756139112,
        };
        assert_eq!(Instance::parse(&instance.to_text()), Some(instance));
    }

    #[test]
    fn a_half_written_file_is_not_an_instance() {
        // The window writes the file in one go, but a reader can still catch it between the file
        // being made and the bytes landing. A file with no port is no use, so it is not an answer.
        assert_eq!(Instance::parse("pid = 1\ntoken = abc\n"), None);
        assert_eq!(Instance::parse(""), None);
    }

    #[test]
    fn the_newest_instance_is_listed_first() {
        let folder = std::env::temp_dir().join("unluminate-cli-instances-order");
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        for (pid, started) in [(10u32, 100u64), (11, 300), (12, 200)] {
            let instance = Instance {
                pid,
                port: 40000 + pid as u16,
                token: "t".to_owned(),
                folder: PathBuf::from("/p"),
                started,
            };
            std::fs::write(instance.path_in(&folder), instance.to_text()).expect("write");
        }
        let listed = listed_in(&folder);
        assert_eq!(listed.iter().map(|i| i.pid).collect::<Vec<_>>(), vec![11, 12, 10]);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_file_that_is_not_an_instance_file_is_ignored() {
        let folder = std::env::temp_dir().join("unluminate-cli-instances-other");
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(folder.join("notes.txt"), "port = 1\n").expect("write");
        assert!(listed_in(&folder).is_empty(), "only .conf files are instances");
        std::fs::remove_dir_all(&folder).ok();
    }
}
