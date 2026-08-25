//! What Quill remembers between runs: the settings, and the projects that have been open.
//!
//! Two files in one folder, both plain text, both written by hand rather than through a format library.
//! `settings.conf` holds one `name = value` a line and `recent.txt` holds one folder a line, newest
//! first. Neither needs a parser worth a dependency, and both can be read and corrected in a text
//! editor, which is fitting for a text editor's own settings.
//!
//! A file that cannot be read is treated as a file that is not there. Quill starting with its defaults is
//! better than Quill refusing to start because a settings file has a stray line in it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many projects the recent list holds. Fifteen fills a menu without needing to scroll.
pub const RECENT_LIMIT: usize = 15;

const SETTINGS_FILE: &str = "settings.conf";
const RECENT_FILE: &str = "recent.txt";

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

    pub fn text(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
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
        let mut out = String::from("# Quill settings. Written by Quill, and safe to edit by hand.\n");
        for (name, value) in &self.0 {
            out.push_str(name);
            out.push_str(" = ");
            out.push_str(value);
            out.push('\n');
        }
        out
    }
}

/// The folder Quill keeps its settings in, and the two files inside it.
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

    pub fn recent_path(&self) -> PathBuf {
        self.folder.join(RECENT_FILE)
    }

    pub fn read_values(&self) -> Values {
        match std::fs::read_to_string(self.settings_path()) {
            Ok(text) => Values::parse(&text),
            Err(_) => Values::new(),
        }
    }

    /// Write the settings. A failure is reported on the error output and otherwise ignored: Quill going
    /// on working with the settings it has in memory is better than stopping because a disk is full.
    pub fn write_values(&self, values: &Values) {
        if let Err(problem) = self.write(&self.settings_path(), &values.to_text()) {
            eprintln!("Quill could not write its settings: {problem}");
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
            let path = PathBuf::from(line);
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
    pub fn remember_project(&self, folder: &Path) {
        let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let mut projects = self.recent_projects();
        projects.retain(|existing| existing != &folder);
        projects.insert(0, folder);
        projects.truncate(RECENT_LIMIT);
        let text: String =
            projects.iter().map(|path| format!("{}\n", path.display())).collect();
        if let Err(problem) = self.write(&self.recent_path(), &text) {
            eprintln!("Quill could not write its recent projects: {problem}");
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

/// Where the operating system expects an application to keep its settings.
///
/// macOS puts them in `Library/Application Support`, Windows in the roaming application data folder, and
/// everywhere else follows the directory specification, which is `XDG_CONFIG_HOME` when it is set and
/// `~/.config` when it is not. With no home directory at all the current folder is used, so that Quill
/// still runs.
fn settings_folder() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return home.join("Library/Application Support/Quill");
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(data) = std::env::var_os("APPDATA") {
            return PathBuf::from(data).join("Quill");
        }
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config).join("quill");
    }
    match home {
        Some(home) => home.join(".config/quill"),
        None => PathBuf::from(".quill"),
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

    #[test]
    fn values_survive_being_written_and_read_back() {
        let store = Store::at(temporary("quill-store-round-trip"));
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
        let store = Store::at(temporary("quill-store-missing"));
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
        let folder = temporary("quill-store-recent");
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
        let folder = temporary("quill-store-cap");
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
    fn the_settings_folder_is_under_the_home_directory() {
        let folder = settings_folder();
        assert!(
            folder.ends_with("Quill") || folder.ends_with("quill") || folder.ends_with(".quill"),
            "the settings folder should be named after Quill, it was {}",
            folder.display()
        );
    }
}
