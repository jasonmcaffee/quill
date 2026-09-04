//! What Unluminous remembers between runs: the settings, and the projects that have been open.
//!
//! Two files in one folder, both plain text, both written by hand rather than through a format library.
//! `settings.conf` holds one `name = value` a line and `recent.txt` holds one folder a line, newest
//! first. Neither needs a parser worth a dependency, and both can be read and corrected in a text
//! editor, which is fitting for a text editor's own settings.
//!
//! A file that cannot be read is treated as a file that is not there. Unluminous starting with its defaults is
//! better than Unluminous refusing to start because a settings file has a stray line in it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many projects the recent list holds. Fifteen fills a menu without needing to scroll.
pub const RECENT_LIMIT: usize = 15;

const SETTINGS_FILE: &str = "settings.conf";
const RECENT_FILE: &str = "recent.txt";
const SESSION_FILE: &str = "session.txt";

/// How many windows the session list holds.
///
/// Eight is more windows than anybody has open at once and few enough that a folder opened once by
/// hand falls off the list after a week of ordinary work. See [`Store::open_windows`].
pub const SESSION_LIMIT: usize = 8;

/// Named values read from or written to the settings file.
///
/// The store knows nothing about what the names mean; `crate::settings` owns that. Keeping the two apart
/// means the settings can grow a value without the file handling changing at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values(BTreeMap<String, String>);

impl Values {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        self.0.insert(name.to_owned(), value.into());
    }

    /// Take a name out, so the file no longer holds it.
    ///
    /// **What a setting that has gone back to its default needs**, and it is not the same as setting it
    /// to an empty string: several settings here mean "whatever this Unluminous's own default is" by having
    /// no line at all — `terminal.shell`, `appearance.theme`, `appearance.icons` — and an empty line
    /// would read as a shell called nothing. Saving merges over the file that is already there
    /// (`settings::save_with`), so without this a value that was cleared would stay in the file and come
    /// back at the next start. See [`Values::set_or_clear`].
    pub fn remove(&mut self, name: &str) {
        self.0.remove(name);
    }

    /// Write a value, or take the name out when it is empty.
    ///
    /// One function rather than an `if` at each of the seven places that mean "empty is the default", so
    /// a later one cannot forget the second half and leave a setting that cannot be un-chosen.
    pub fn set_or_clear(&mut self, name: &str, value: &str) {
        match value.is_empty() {
            true => self.remove(name),
            false => self.set(name, value.to_owned()),
        }
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// Every name that begins with `prefix`, with the prefix removed, in name order.
    ///
    /// What reads a family of keys whose names are not known in advance, which is what a plugin's
    /// submenus are: `menu.submenu.new` and `menu.submenu.new.entries` are two members of one family
    /// and nothing in Unluminous knows the word `new` until the manifest is read. The order is the map's
    /// order, so a family read twice is read the same way both times and a menu built from one is the
    /// same shape every time.
    pub fn starting_with(&self, prefix: &str) -> Vec<(String, String)> {
        self.0
            .iter()
            .filter_map(|(name, value)| {
                name.strip_prefix(prefix).map(|rest| (rest.to_owned(), value.clone()))
            })
            .collect()
    }

    pub fn number(&self, name: &str) -> Option<f32> {
        self.text(name).and_then(|value| value.trim().parse().ok())
    }

    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.text(name)?.trim() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        }
    }

    /// Read `name = value` lines. A line without an `=` is ignored rather than making the whole file
    /// unreadable.
    ///
    /// A `#` starts a comment **when it is followed by a space or ends the line**. That rule is a
    /// little more particular than "everything after a hash", and it is that way because of colours:
    /// a plugin's colour scheme is written `theme.keyword = #FF79C6`, and the plain rule ate the
    /// value and left the plugin with no colours at all. Writing the hash is what anybody would do,
    /// so the format accommodates it rather than making it a trap. `size = 20  # after the value`
    /// still reads as a comment, because that hash is followed by a space.
    pub fn parse(text: &str) -> Self {
        let mut values = Self::new();
        for line in text.lines() {
            let line = match Self::comment_at(line) {
                Some(at) => &line[..at],
                None => line,
            };
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            values.set(name, value.trim().to_owned());
        }
        values
    }

    /// Where the comment starts on this line, if it has one.
    fn comment_at(line: &str) -> Option<usize> {
        line.char_indices().find(|(at, character)| {
            *character == '#'
                && line[at + 1..].chars().next().map(char::is_whitespace).unwrap_or(true)
        })
        .map(|(at, _)| at)
    }

    pub fn to_text(&self) -> String {
        self.to_text_headed("# Unluminous settings. Written by Unluminous, and safe to edit by hand.")
    }

    /// The same, under a heading of the caller's own. The project state is written in this format too
    /// and is not the settings, so it says so at the top of its own file.
    pub fn to_text_headed(&self, heading: &str) -> String {
        let mut out = format!("{heading}\n");
        for (name, value) in &self.0 {
            out.push_str(name);
            out.push_str(" = ");
            out.push_str(value);
            out.push('\n');
        }
        out
    }
}

/// The folder Unluminous keeps its settings in, and the two files inside it.
#[derive(Debug, Clone)]
pub struct Store {
    folder: PathBuf,
}

impl Store {
    /// The store in the place the operating system keeps an application's settings.
    pub fn open() -> Self {
        Self::at(settings_folder())
    }

    /// A store in a named folder, which is how the tests use one without touching the real settings.
    pub fn at(folder: impl Into<PathBuf>) -> Self {
        Self { folder: folder.into() }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn settings_path(&self) -> PathBuf {
        self.folder.join(SETTINGS_FILE)
    }

    pub fn session_path(&self) -> PathBuf {
        self.folder.join(SESSION_FILE)
    }

    /// The projects Unluminous had a window open on, oldest first.
    ///
    /// `task-1693` asks that quitting and starting again bring back "the windows/projects I had
    /// open". An Unluminous window is a **process** — `services::launcher` records why — so the only place
    /// both of them can see is a file here, beside `recent.txt`.
    ///
    /// **A line is kept when a window closes.** That is the trade-off, and it is stated rather than
    /// hidden: closing one window while another is open still brings both back next time, which is
    /// what the ticket asks for in as many words and is the only rule available. Unluminous has no
    /// application-wide quit to hang the question on, and by the time the last window closes the
    /// ones that closed before it are long gone from any live registry. The cost is that a folder
    /// opened once stays in the list until [`SESSION_LIMIT`] others push it out.
    ///
    /// A folder that is no longer there is left out, for the reason [`Self::recent_projects`] leaves
    /// one out of the menu.
    pub fn open_windows(&self) -> Vec<PathBuf> {
        let Ok(text) = std::fs::read_to_string(self.session_path()) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let path = unluminous_terminal::paths::plain(Path::new(line));
            if out.contains(&path) || !path.is_dir() {
                continue;
            }
            out.push(path);
        }
        out.truncate(SESSION_LIMIT);
        out
    }

    /// Add `folder` to the session list if it is not in it already.
    ///
    /// Newest **last**, which is the other way round from `recent.txt`: the list is restored in
    /// order and the last entry is the one the restoring process opens itself, so oldest-first is
    /// also what makes truncating the front of the list drop the oldest.
    pub fn remember_open_window(&self, folder: &Path) {
        let folder = plain_absolute(folder);
        let mut windows = self.open_windows();
        // Appended only when it is not there already, which is what keeps three windows starting at
        // once from fighting over the file: restoring a session opens two or three processes within
        // a few hundred milliseconds and every one of them reads this list and writes it back. A
        // window that is already in it writes nothing at all, so the only moment two processes can
        // both write is when two brand new projects are opened in the same instant.
        if windows.contains(&folder) {
            return;
        }
        windows.push(folder);
        while windows.len() > SESSION_LIMIT {
            windows.remove(0);
        }
        self.write_session(&windows);
    }

    /// Write the session list out as exactly `windows`.
    ///
    /// What restoring does once it has started them all, so the list is what was really restored
    /// rather than growing for ever.
    pub fn write_session(&self, windows: &[PathBuf]) {
        let text: String = windows.iter().map(|path| format!("{}\n", path.display())).collect();
        if let Err(problem) = self.write(&self.session_path(), &text) {
            eprintln!("Unluminous could not write its open windows: {problem}");
        }
    }

    pub fn recent_path(&self) -> PathBuf {
        self.folder.join(RECENT_FILE)
    }

    pub fn read_values(&self) -> Values {
        match std::fs::read_to_string(self.settings_path()) {
            Ok(text) => Values::parse(&text),
            Err(_) => Values::new(),
        }
    }

    /// Write the settings. A failure is reported on the error output and otherwise ignored: Unluminous going
    /// on working with the settings it has in memory is better than stopping because a disk is full.
    pub fn write_values(&self, values: &Values) {
        if let Err(problem) = self.write(&self.settings_path(), &values.to_text()) {
            eprintln!("Unluminous could not write its settings: {problem}");
        }
    }

    /// The projects that have been open, newest first.
    ///
    /// A folder that has since been removed is left out, because an entry in the menu that cannot be
    /// opened is worse than a shorter menu.
    pub fn recent_projects(&self) -> Vec<PathBuf> {
        let Ok(text) = std::fs::read_to_string(self.recent_path()) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Through `plain`, so that a list written by an earlier Unluminous — every line of which held a
            // verbatim path — is read as the folders it names rather than as nine folders nobody can
            // open a terminal in. The next `remember_project` writes the repaired list back.
            let path = unluminous_terminal::paths::plain(Path::new(line));
            if out.contains(&path) || !path.is_dir() {
                continue;
            }
            out.push(path);
        }
        out.truncate(RECENT_LIMIT);
        out
    }

    /// Put `folder` at the top of the recent list, removing an older entry for the same folder.
    ///
    /// The path is made absolute first, so that opening `.` and opening the folder it stands for are one
    /// entry rather than two.
    ///
    /// And then plain, because on Windows `canonicalize` gives back a **verbatim** path —
    /// `\\?\C:\jason\dev\unluminous` — and this list is not only read back by Unluminous. It is the explorer's
    /// root, the folder the `.unluminous` state is written beside, and the directory a shell is started in,
    /// and `cmd.exe` will not start in a verbatim path at all. `task-1670` is what that looked like from
    /// the outside: a terminal that opened in `C:\Windows` and said why in a message about network
    /// shares. `unluminous_terminal::paths` says what the prefix is and why the terminal strips it again.
    pub fn remember_project(&self, folder: &Path) {
        let folder = plain_absolute(folder);
        let mut projects = self.recent_projects();
        projects.retain(|existing| existing != &folder);
        projects.insert(0, folder);
        projects.truncate(RECENT_LIMIT);
        let text: String =
            projects.iter().map(|path| format!("{}\n", path.display())).collect();
        if let Err(problem) = self.write(&self.recent_path(), &text) {
            eprintln!("Unluminous could not write its recent projects: {problem}");
        }
    }

    fn write(&self, path: &Path, text: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.folder)?;
        std::fs::write(path, text)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::open()
    }
}

/// A folder as it should be written down: absolute, and plain rather than verbatim.
///
/// Both lists Unluminous keeps of folders go through this. Absolute, so that opening `.` and opening the
/// folder it stands for are one entry rather than two; and plain because on Windows `canonicalize`
/// gives back a **verbatim** path — `\\?\C:\jason\dev\unluminous` — which `cmd.exe` will not start in.
/// `task-1670` is what that looked like from the outside.
fn plain_absolute(folder: &Path) -> PathBuf {
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    unluminous_terminal::paths::plain(&folder)
}

/// Where the operating system expects an application to keep its settings.
///
/// macOS puts them in `Library/Application Support`, Windows in the roaming application data folder, and
/// everywhere else follows the directory specification, which is `XDG_CONFIG_HOME` when it is set and
/// `~/.config` when it is not. With no home directory at all the current folder is used, so that Unluminous
/// still runs.
/// Where Unluminous keeps its things for this person, without opening a store first.
///
/// `main` needs it before anything else exists, to point the crash log at it.
pub fn folder_for_this_person() -> PathBuf {
    settings_folder()
}

fn settings_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return home.join("Library/Application Support/Unluminous");
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(data) = std::env::var_os("APPDATA") {
            return PathBuf::from(data).join("Unluminous");
        }
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("unluminous");
    }
    match home {
        Some(home) => home.join(".config/unluminous"),
        None => PathBuf::from(".unluminous"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        folder
    }

    /// `task-1693`: the windows Unluminous had open, so that starting it again brings them all back.
    #[test]
    fn every_window_that_was_open_is_remembered_and_a_line_is_kept_when_one_closes() {
        let folder = temporary("unluminous-store-session");
        let store = Store::at(&folder);
        let first = folder.join("first");
        let second = folder.join("second");
        std::fs::create_dir_all(&first).expect("make the first project");
        std::fs::create_dir_all(&second).expect("make the second project");

        store.remember_open_window(&first);
        store.remember_open_window(&second);
        let windows = store.open_windows();
        assert_eq!(windows.len(), 2, "both windows are in the list");
        assert!(windows.last().is_some_and(|last| last.ends_with("second")), "newest last");

        // Opening the first again writes nothing, which is what keeps three windows starting at
        // once from losing each other's lines.
        store.remember_open_window(&first);
        let windows = store.open_windows();
        assert_eq!(windows.len(), 2, "it is already there, so nothing is written");
        assert!(windows.last().is_some_and(|last| last.ends_with("second")));
    }

    /// The cap is what bounds the cost of keeping a line behind when a window closes.
    #[test]
    fn the_session_list_stops_at_its_limit_and_drops_the_oldest() {
        let folder = temporary("unluminous-store-session-limit");
        let store = Store::at(&folder);
        let mut made = Vec::new();
        for index in 0..SESSION_LIMIT + 3 {
            let project = folder.join(format!("project-{index}"));
            std::fs::create_dir_all(&project).expect("make a project");
            store.remember_open_window(&project);
            made.push(project);
        }
        let windows = store.open_windows();
        assert_eq!(windows.len(), SESSION_LIMIT);
        assert!(
            windows.first().is_some_and(|first| first.ends_with("project-3")),
            "the three oldest fell off the front, and the list is {windows:?}"
        );
    }

    /// A project that is no longer on disk is left out, for the reason one is left out of the recent
    /// menu: a window that opens on nothing is worse than one window fewer.
    #[test]
    fn a_project_that_has_gone_is_not_a_window_to_open() {
        let folder = temporary("unluminous-store-session-gone");
        let store = Store::at(&folder);
        let here = folder.join("here");
        std::fs::create_dir_all(&here).expect("make the project");
        store.remember_open_window(&here);
        store.write_session(&[here.clone(), folder.join("never-existed")]);
        assert_eq!(store.open_windows(), vec![here]);
    }

    #[test]
    fn values_survive_being_written_and_read_back() {
        let store = Store::at(temporary("unluminous-store-round-trip"));
        let mut values = Values::new();
        values.set("appearance.font.family", "Helvetica");
        values.set("appearance.font.size", "18");
        values.set("appearance.background.opacity", "0.62");
        values.set("explorer.width", "310");
        store.write_values(&values);

        let read = store.read_values();
        assert_eq!(read.text("appearance.font.family"), Some("Helvetica"));
        assert_eq!(read.number("appearance.font.size"), Some(18.0));
        assert_eq!(read.number("appearance.background.opacity"), Some(0.62));
        assert_eq!(read.number("explorer.width"), Some(310.0));
        assert_eq!(read, values, "what was written is what comes back");
        std::fs::remove_dir_all(store.folder()).ok();
    }

    #[test]
    fn a_missing_settings_file_reads_as_no_values_rather_than_failing() {
        let store = Store::at(temporary("unluminous-store-missing"));
        assert_eq!(store.read_values(), Values::new());
        assert!(store.recent_projects().is_empty());
    }

    #[test]
    fn comments_blank_lines_and_nonsense_are_skipped() {
        let values = Values::parse(
            "# a comment\n\nappearance.font.size = 20  # after the value\nno equals sign here\n = 5\n",
        );
        assert_eq!(values.number("appearance.font.size"), Some(20.0));
        assert_eq!(values.text("no equals sign here"), None);
    }

    #[test]
    fn a_hash_that_is_part_of_a_value_is_not_a_comment() {
        // A colour is written the way anybody would write one, and the value is not eaten.
        let values = Values::parse("theme.keyword = #FF79C6  # pink
theme.comment = #6272A4
");
        assert_eq!(values.text("theme.keyword"), Some("#FF79C6"));
        assert_eq!(values.text("theme.comment"), Some("#6272A4"));
    }

    #[test]
    fn a_flag_reads_the_words_and_the_numbers() {
        let values = Values::parse("a = true\nb = no\nc = 1\nd = maybe\n");
        assert_eq!(values.flag("a"), Some(true));
        assert_eq!(values.flag("b"), Some(false));
        assert_eq!(values.flag("c"), Some(true));
        assert_eq!(values.flag("d"), None, "a value that is not a flag is not guessed at");
    }

    #[test]
    fn the_newest_project_is_first_and_a_repeat_moves_up_rather_than_appearing_twice() {
        let folder = temporary("unluminous-store-recent");
        let store = Store::at(&folder);
        let first = folder.join("one");
        let second = folder.join("two");
        std::fs::create_dir_all(&first).expect("make the first project");
        std::fs::create_dir_all(&second).expect("make the second project");

        store.remember_project(&first);
        store.remember_project(&second);
        store.remember_project(&first);

        let recent = store.recent_projects();
        assert_eq!(recent.len(), 2, "two folders, opened three times, got {recent:?}");
        assert!(recent[0].ends_with("one"), "the one opened last is first, got {recent:?}");
        assert!(recent[1].ends_with("two"));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_recent_list_is_capped_and_drops_folders_that_have_gone() {
        let folder = temporary("unluminous-store-cap");
        let store = Store::at(&folder);
        for index in 0..RECENT_LIMIT + 5 {
            let project = folder.join(format!("project-{index}"));
            std::fs::create_dir_all(&project).expect("make a project");
            store.remember_project(&project);
        }
        assert_eq!(store.recent_projects().len(), RECENT_LIMIT);

        let newest = store.recent_projects()[0].clone();
        std::fs::remove_dir_all(&newest).expect("remove the newest project");
        assert!(
            !store.recent_projects().contains(&newest),
            "a folder that has been removed is not offered"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_remembered_project_is_written_down_as_a_plain_path() {
        // `task-1670`. `canonicalize` on Windows gives back `\\?\C:\...`, and this list is where the
        // explorer's root and the terminal's working directory come from, so a verbatim path here is a
        // terminal that starts in `C:\Windows`.
        let folder = temporary("unluminous-store-plain-path");
        let store = Store::at(&folder);
        let project = folder.join("project");
        std::fs::create_dir_all(&project).expect("make a project");
        store.remember_project(&project);

        let written = std::fs::read_to_string(store.recent_path()).expect("read the list");
        assert!(
            !written.contains(r"\\?\"),
            "the recent list should hold plain paths, and holds {written:?}"
        );
        assert_eq!(store.recent_projects().len(), 1);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_list_written_by_an_earlier_unluminous_is_read_as_plain_paths() {
        // Every line of the list on a machine that had run the earlier Unluminous was verbatim. Reading it
        // repairs it rather than leaving somebody to edit the file by hand.
        let folder = temporary("unluminous-store-old-list");
        let store = Store::at(&folder);
        let project = folder.join("project");
        std::fs::create_dir_all(&project).expect("make a project");
        let verbatim = std::fs::canonicalize(&project).expect("resolve the project");
        std::fs::create_dir_all(store.folder()).expect("make the settings folder");
        std::fs::write(store.recent_path(), format!("{}\n", verbatim.display()))
            .expect("write the old list");

        let recent = store.recent_projects();
        assert_eq!(recent.len(), 1, "the folder is still found, got {recent:?}");
        assert!(
            !recent[0].to_string_lossy().contains(r"\\?\"),
            "it comes back plain, got {:?}",
            recent[0]
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_settings_folder_is_under_the_home_directory() {
        let folder = settings_folder();
        assert!(
            folder.ends_with("Unluminous") || folder.ends_with("unluminous") || folder.ends_with(".unluminous"),
            "the settings folder should be named after Unluminous, it was {}",
            folder.display()
        );
    }
}
