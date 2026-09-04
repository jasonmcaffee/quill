//! Where a running Unluminate says how to reach it, and how the client finds one.
//!
//! A Unluminate that is listening writes one small file into `<settings folder>/instances`, named after
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
//! A Unluminate that is killed rather than closed leaves its file behind. Nothing sweeps them on a
//! timer: [`running`] treats a file it cannot connect to as not being an instance, and removes it.
//! That is the only sweep there is, and it happens when somebody is looking.

use std::path::{Path, PathBuf};

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

/// Remove an instance's file, which a Unluminate does as it stops and the client does when it finds one
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
