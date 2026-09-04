//! The file explorer on the left of the window.
//!
//! A tree of directories and files. A directory's children are read from disk the first time it is
//! expanded rather than at startup, so opening Unluminate on a large folder is not slow.
//!
//! Every file is listed, not only the ones Unluminate can open. An earlier version listed `.md` and `.txt` only,
//! on the grounds that showing a file that cannot be opened is worse than not showing it. That was
//! overruled: an explorer that hides most of a folder does not tell you what is in the folder. So all files
//! are shown, and the ones Unluminate cannot open are drawn dimmed and do not respond to a click, which says
//! what is there without pretending it can all be opened.
//!
//! Which files those are is decided by [`crate::services::file_kind`], and it is now most of them: any
//! file holding text opens, whether or not Unluminate has any special handling for that kind of text.

use std::path::{Path, PathBuf};

use crate::services::file_kind::{self, Kind, Refusal};
use crate::services::ignore::Ignores;

/// Whether Unluminate can open this file.
pub fn is_openable(path: &Path) -> bool {
    file_kind::is_openable(path)
}

/// One directory or file in the tree.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_directory: bool,
    /// True when Unluminate can open this file. A file it cannot open is still listed, but dimmed.
    pub openable: bool,
    /// Why it cannot be opened, when it cannot, so the row can say which reason it is.
    pub refusal: Option<Refusal>,
    pub expanded: bool,
    /// The children of a directory, or `None` when they have not been read yet.
    pub children: Option<Vec<Entry>>,
}

impl Entry {
    /// One entry, from what the directory read already knows about it.
    ///
    /// `kind` and `size` come from the one `metadata` call [`read_directory`] makes per child, and nothing
    /// here asks the file system anything. `task-28`: this used to call `file_kind::openable`, which does
    /// its own `metadata` call and then, for a name with an unknown extension, **opens the file and reads
    /// it**. That is what froze the window on a large folder and hung on `/dev`, where a read from a FIFO
    /// never returns.
    fn new(path: PathBuf, kind: Kind, size: Option<u64>) -> Self {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.display().to_string());
        let is_directory = kind == Kind::Directory;
        let refusal = match is_directory {
            true => None,
            false => file_kind::openable_in_a_listing(&path, kind, size).err(),
        };
        let openable = !is_directory && refusal.is_none();
        Self { path, name, is_directory, openable, refusal, expanded: false, children: None }
    }
}

/// One row to draw: an entry and how far to indent it.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub entry: &'a Entry,
    pub depth: usize,
}

/// The file explorer's state.
/// How deep the filter and the file count look, so that pointing Unluminate at a very large folder cannot make
/// it walk the whole disk.
const SEARCH_DEPTH: usize = 8;
/// The most results the filter reports.
const SEARCH_LIMIT: usize = 300;

#[derive(Debug, Clone)]
pub struct FileTree {
    root: PathBuf,
    entries: Vec<Entry>,
    /// Every file under the root, found once when the tree is loaded. The filter searches this, so it
    /// finds files inside folders that have never been opened.
    all_files: Vec<PathBuf>,
    /// What this project leaves out of `all_files`: its `.gitignore`, its `.git/info/exclude` and
    /// the `editor.exclude` setting. `task-1804` §7.3.
    ///
    /// It is read once per reload rather than per file, because it is two small files and a walk
    /// asks the question thousands of times.
    ignores: Ignores,
    /// The `editor.exclude` setting as it was last given, so a reload keeps it.
    exclude: String,
    /// The last error from reading a directory, so the window can say why a folder looks empty.
    pub last_error: Option<String>,
    /// When each folder that is **showing** was last written to, as the disk said at the moment it
    /// was read. The root and every folder that is opened out.
    ///
    /// This is what makes a file another program made appear without anybody asking. See
    /// [`FileTree::changed_on_disk`].
    folder_times: Vec<(PathBuf, Option<std::time::SystemTime>)>,
}

impl FileTree {
    /// Open a folder. The root's own children are read at once, because an explorer showing nothing at
    /// startup looks broken.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut tree = Self {
            root,
            entries: Vec::new(),
            all_files: Vec::new(),
            ignores: Ignores::default(),
            exclude: String::new(),
            last_error: None,
            folder_times: Vec::new(),
        };
        tree.reload();
        tree
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The `editor.exclude` patterns, applied from the next reload.
    ///
    /// Reloads at once when they changed, because a setting that only takes effect after something
    /// else happens is a setting a person tries once and decides is broken.
    pub fn set_exclude(&mut self, patterns: &str) {
        if self.exclude == patterns {
            return;
        }
        self.exclude = patterns.to_owned();
        self.reload();
    }

    /// What this project leaves out of the searchable list.
    pub fn ignores(&self) -> &Ignores {
        &self.ignores
    }

    /// Read the root's children again, keeping which folders were open.
    pub fn reload(&mut self) {
        let expanded = self.expanded_paths();
        match read_directory(&self.root) {
            Ok(entries) => {
                self.entries = entries;
                self.last_error = None;
            }
            Err(error) => {
                self.entries = Vec::new();
                self.last_error = Some(format!("{}: {error}", self.root.display()));
            }
        }
        for path in expanded {
            self.expand(&path);
        }
        // Read fresh on every reload, so editing `.gitignore` and pressing refresh takes effect --
        // which is what a person who has just added a line to it expects. The rules belong to the
        // repository rather than to whichever folder was opened, so a checkout's own file is honoured
        // when a subfolder of it is the root.
        let repository = crate::services::ignore::repository_root(&self.root);
        self.ignores = Ignores::read(&repository, &self.exclude);
        self.all_files = walk_files(&self.root, SEARCH_DEPTH, &self.ignores);
        self.folder_times = self.read_folder_times();
    }

    /// True when a folder that is showing has been written to since the tree was last read.
    ///
    /// `task-1693`: a file or a folder made by something other than Unluminate — an agent, a build, a
    /// terminal in the tile below — never appeared, because the tree is only read when Unluminate is told
    /// to read it. Creating, deleting or renaming an entry changes the modification time of the
    /// folder it is in, on Windows and on macOS both, so the root plus each folder that is opened out
    /// is the complete set of places a change anybody can *see* could happen.
    ///
    /// One `metadata` call per open folder, which for a tree with twenty folders open at the rate the
    /// window asks is a few dozen calls a second and nothing measurable. Watching the tree properly
    /// would be a dependency, a thread, a channel and a debounce — and a debounce is needed because
    /// `ReadDirectoryChangesW` on a `target` folder during a build produces thousands of events a
    /// second, each of which would cost a walk. Unluminate does not need to watch a tree; it needs to
    /// notice that one of a few dozen visible folders has changed.
    pub fn changed_on_disk(&self) -> bool {
        self.read_folder_times() != self.folder_times
    }

    /// The root and every folder that is opened out, each with the time the disk says it was last
    /// written to. A folder that cannot be read at all answers `None`, which is a change if it used
    /// to answer with a time.
    fn read_folder_times(&self) -> Vec<(PathBuf, Option<std::time::SystemTime>)> {
        let mut folders = vec![self.root.clone()];
        folders.extend(self.expanded_paths());
        folders
            .into_iter()
            .map(|folder| {
                let at = std::fs::metadata(&folder).and_then(|data| data.modified()).ok();
                (folder, at)
            })
            .collect()
    }

    /// Every file under the root, in order.
    pub fn all_files(&self) -> &[PathBuf] {
        &self.all_files
    }

    /// How many files there are under the root, whether or not Unluminate can open them.
    pub fn file_count(&self) -> usize {
        self.all_files.len()
    }

    /// How many of those files Unluminate can open.
    pub fn openable_count(&self) -> usize {
        self.all_files.iter().filter(|path| is_openable(path)).count()
    }

    /// The files whose name contains `filter`, ignoring case. Searching the whole folder rather than only
    /// the parts that have been opened is the point of a filter: a name typed in should find the file
    /// wherever it is.
    pub fn matching(&self, filter: &str) -> Vec<&Path> {
        let needle = filter.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.all_files
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_lowercase().contains(&needle))
            })
            .map(PathBuf::as_path)
            .take(SEARCH_LIMIT)
            .collect()
    }

    /// How far a path sits below the root, used to indent a filter result under its folder.
    pub fn depth_of(&self, path: &Path) -> usize {
        path.strip_prefix(&self.root).map(|rest| rest.components().count() - 1).unwrap_or(0)
    }

    fn expanded_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        fn walk(entries: &[Entry], out: &mut Vec<PathBuf>) {
            for entry in entries {
                if entry.is_directory && entry.expanded {
                    out.push(entry.path.clone());
                    if let Some(children) = &entry.children {
                        walk(children, out);
                    }
                }
            }
        }
        walk(&self.entries, &mut out);
        out
    }

    /// Open or close a directory. Opening one reads its children if they have not been read yet.
    pub fn toggle(&mut self, path: &Path) {
        let mut error = None;
        Self::visit(&mut self.entries, path, &mut |entry| {
            if !entry.is_directory {
                return;
            }
            entry.expanded = !entry.expanded;
            if entry.expanded && entry.children.is_none() {
                match read_directory(&entry.path) {
                    Ok(children) => entry.children = Some(children),
                    Err(problem) => {
                        entry.children = Some(Vec::new());
                        error = Some(format!("{}: {problem}", entry.path.display()));
                    }
                }
            }
        });
        if error.is_some() {
            self.last_error = error;
        }
        // The set of folders that are showing has just changed, so what is being watched has too.
        // Without this, opening a folder would look like a change on disk on the very next tick.
        self.folder_times = self.read_folder_times();
    }

    /// Open a directory, and every directory above it, so that a path can be revealed.
    pub fn expand(&mut self, path: &Path) {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return;
        };
        let mut current = self.root.clone();
        for part in relative.components() {
            current = current.join(part);
            let already_open = self.find(&current).is_some_and(|entry| entry.expanded);
            if !already_open {
                self.toggle(&current);
            }
        }
    }

    /// Every directory that is open, in the order the rows are drawn in.
    ///
    /// This is what `services::project_state` writes down, so that a project reopened tomorrow shows the
    /// same folders opened out that it showed when it was closed.
    pub fn expanded_folders(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        fn walk(entries: &[Entry], out: &mut Vec<PathBuf>) {
            for entry in entries {
                if entry.is_directory && entry.expanded {
                    out.push(entry.path.clone());
                    if let Some(children) = &entry.children {
                        walk(children, out);
                    }
                }
            }
        }
        walk(&self.entries, &mut out);
        out
    }

    pub fn find(&self, path: &Path) -> Option<&Entry> {
        fn walk<'a>(entries: &'a [Entry], path: &Path) -> Option<&'a Entry> {
            for entry in entries {
                if entry.path == path {
                    return Some(entry);
                }
                if let Some(children) = &entry.children {
                    if let Some(found) = walk(children, path) {
                        return Some(found);
                    }
                }
            }
            None
        }
        walk(&self.entries, path)
    }

    fn visit(entries: &mut [Entry], path: &Path, action: &mut impl FnMut(&mut Entry)) -> bool {
        for entry in entries.iter_mut() {
            if entry.path == path {
                action(entry);
                return true;
            }
            if let Some(children) = &mut entry.children {
                if Self::visit(children, path, action) {
                    return true;
                }
            }
        }
        false
    }

    /// Every row to draw, in order, with its indent. A closed directory's children are not in the list.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut out = Vec::new();
        fn walk<'a>(entries: &'a [Entry], depth: usize, out: &mut Vec<Row<'a>>) {
            for entry in entries {
                out.push(Row { entry, depth });
                if entry.expanded {
                    if let Some(children) = &entry.children {
                        walk(children, depth + 1, out);
                    }
                }
            }
        }
        walk(&self.entries, 0, &mut out);
        out
    }
}

/// Every file under `root`, to a depth of `depth`, folders before files and both by name.
///
/// This is a separate walk from the tree itself because the tree only reads a folder when it is opened,
/// and the filter and the file count have to know about files the user has not gone looking for yet.
fn walk_files(root: &Path, depth: usize, ignores: &Ignores) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(
        root: &Path,
        directory: &Path,
        remaining: usize,
        ignores: &Ignores,
        out: &mut Vec<PathBuf>,
    ) {
        if remaining == 0 || out.len() >= SEARCH_LIMIT * 4 {
            return;
        }
        let Ok(entries) = read_directory(directory) else {
            return;
        };
        for entry in entries {
            let relative = match entry.path.strip_prefix(root) {
                Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
                // Outside the root entirely, which a symbolic link can produce. Left in, because the
                // rules are written about paths inside the project and say nothing about this one.
                Err(_) => String::new(),
            };
            if entry.is_directory {
                // `.gitignore` first, and the three build folder names only where the project has
                // not said -- which is what `skips_folder` decides. `task-1804` §7.3.
                if !relative.is_empty() && ignores.skips_folder(&relative, &entry.name) {
                    continue;
                }
                walk(root, &entry.path, remaining - 1, ignores, out);
            } else if entry.refusal != Some(Refusal::NotAFile) {
                // Regular files only. A device, a pipe or a socket is drawn in the explorer because the
                // panel is a picture of the folder, and it is not a file anybody searches for by name.
                if !relative.is_empty() && ignores.ignores(&relative, false) {
                    continue;
                }
                out.push(entry.path);
            }
        }
    }
    walk(root, root, depth, ignores, &mut out);
    out
}


/// Read one directory: folders first, then files, both sorted by name.
///
/// Folders before files, because a reader scanning for somewhere to go looks for folders. Hidden
/// entries are left out, so a folder under version control does not show its `.git` directory.
///
/// ## One `metadata` call per child, and no file is ever opened
///
/// `task-28`. This is the function that froze the window, and the arithmetic is worth writing down. It
/// used to cost, per child: `file_type`, then a `metadata` inside `file_kind::openable`, then for a name
/// with an unknown extension an `open` and a `read` of four kilobytes. A folder of twenty thousand
/// extensionless files was forty thousand syscalls and twenty thousand file reads before one row was
/// drawn, and in `/dev` the children are character devices and FIFOs, where the read blocks and never
/// returns and the only way out is force quitting.
///
/// It now costs one `metadata` per child, which answers both what the child is and how big it is, and
/// nothing else. A child that is neither a directory nor a regular file — a device, a pipe, a socket — is
/// **listed and not openable**, because the explorer is a picture of the folder and hiding most of one is
/// what this file's own opening comment refuses to do.
///
/// `DirEntry::metadata` does not follow symlinks, which is exactly what `DirEntry::file_type` did not do
/// either, so a link to a folder is drawn as an entry rather than opened out, as it always was.
fn read_directory(path: &Path) -> std::io::Result<Vec<Entry>> {
    let mut directories = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        // A child whose metadata cannot be read at all is listed as something that is not a file, which is
        // the honest answer: something is there and Unluminate cannot say what.
        let (kind, size) = match entry.metadata() {
            Ok(data) if data.is_dir() => (Kind::Directory, None),
            Ok(data) if data.is_file() => (Kind::File, Some(data.len())),
            Ok(_) => (Kind::Other, None),
            Err(_) => (Kind::Other, None),
        };
        match kind {
            Kind::Directory => directories.push(Entry::new(path, kind, size)),
            _ => files.push(Entry::new(path, kind, size)),
        }
    }
    let by_name = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    directories.sort_by(by_name);
    files.sort_by(by_name);
    directories.extend(files);
    Ok(directories)
}

#[cfg(test)]
mod tests_task_28 {
    use super::*;

    /// `task-28`: expanding a folder froze Unluminate and it had to be force quit.
    ///
    /// **This is the test that would have hung.** A FIFO with no writer blocks a `read` for ever, and
    /// reading one is what `Entry::new` used to do for any name with an extension it did not recognise. So
    /// the assertion is almost beside the point: the test returning at all is the result.
    #[cfg(unix)]
    #[test]
    fn a_folder_holding_a_pipe_is_read_without_opening_it() {
        let root = std::env::temp_dir().join(format!("unluminate-tree-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("make the folder");
        std::fs::write(root.join("ordinary.txt"), "text").expect("write ordinary.txt");
        let pipe = root.join("a-pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            eprintln!("mkfifo is not available here, so there is nothing to read");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }

        let tree = FileTree::new(&root);
        let rows = tree.rows();
        let entry = rows
            .iter()
            .find(|row| row.entry.name == "a-pipe")
            .expect("the pipe is listed, because the explorer is a picture of the folder");
        assert!(!entry.entry.openable, "a pipe is not something Unluminate opens");
        assert_eq!(
            entry.entry.refusal,
            Some(Refusal::NotAFile),
            "and the row says which of the reasons it is"
        );
        // It is not in the list the filter and `Go to File` search either.
        assert!(
            !tree.all_files().iter().any(|path| path == &pipe),
            "a pipe is not a file anybody looks for by name"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `task-1804` §7.3. A fixture project with a `.gitignore`, asserting both halves: what it
    /// names is out of the searchable list, **and the explorer still lists it**.
    ///
    /// The second half is the one worth having a test for. It is the rule at the top of this file --
    /// the explorer is a picture of the folder -- and it is the rule a later change that made the
    /// ignore rules "simply hide things" would break without any other test noticing.
    #[test]
    fn a_gitignore_is_honoured_by_the_index_and_the_explorer_still_shows_everything() {
        let root = std::env::temp_dir().join(format!("unluminate-tree-ignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git/info")).expect("make the repository");
        std::fs::create_dir_all(root.join("src")).expect("make src");
        std::fs::create_dir_all(root.join("dist")).expect("make dist");
        std::fs::create_dir_all(root.join("node_modules/left-pad")).expect("make node_modules");
        std::fs::create_dir_all(root.join("scratch")).expect("make scratch");
        std::fs::create_dir_all(root.join("vendor")).expect("make vendor");
        std::fs::write(root.join(".gitignore"), "dist/\n*.log\n").expect("write .gitignore");
        std::fs::write(root.join(".git/info/exclude"), "scratch/\n").expect("write exclude");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("write main.rs");
        std::fs::write(root.join("dist/bundle.js"), "//").expect("write bundle.js");
        std::fs::write(root.join("node_modules/left-pad/index.js"), "//").expect("write index.js");
        std::fs::write(root.join("scratch/notes.txt"), "notes").expect("write notes.txt");
        std::fs::write(root.join("vendor/lib.rs"), "//").expect("write lib.rs");
        std::fs::write(root.join("debug.log"), "log").expect("write debug.log");

        let mut tree = FileTree::new(&root);
        let named = |tree: &FileTree, name: &str| {
            tree.all_files().iter().any(|path| path.ends_with(name))
        };

        assert!(named(&tree, "main.rs"), "a source file is in the index");
        assert!(!named(&tree, "bundle.js"), "`dist/` is named in .gitignore");
        assert!(!named(&tree, "debug.log"), "and so is `*.log`");
        assert!(!named(&tree, "notes.txt"), ".git/info/exclude is read too");
        assert!(named(&tree, "lib.rs"), "vendor is not ignored by this project, so it stays");
        // **A repository that does not ignore `node_modules` means it.** The three hardcoded names
        // are the fallback for a folder that is not a repository, and this one is.
        assert!(named(&tree, "index.js"), "the project's own file is what decides inside a repository");

        // The `editor.exclude` setting is the person's own line beside the project's.
        tree.set_exclude("vendor/");
        assert!(!named(&tree, "lib.rs"), "the setting leaves it out");
        assert!(named(&tree, "main.rs"), "and leaves everything else alone");

        // And the explorer is unchanged: every folder is still there to be opened.
        let names: Vec<&str> = tree.rows().iter().map(|row| row.entry.name.as_str()).collect();
        for folder in ["dist", "node_modules", "scratch", "vendor", "src"] {
            assert!(names.contains(&folder), "{folder} is still shown in the explorer: {names:?}");
        }
        assert!(names.contains(&"debug.log"), "and so is an ignored file");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Outside a repository the three build folder names are still what is skipped, which is what
    /// they always were and what every project with no `.gitignore` relies on.
    #[test]
    fn a_folder_that_is_not_a_repository_still_skips_the_three_build_folders() {
        let root = std::env::temp_dir().join(format!("unluminate-tree-plain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("target/debug")).expect("make target");
        std::fs::create_dir_all(root.join("src")).expect("make src");
        std::fs::write(root.join("target/debug/thing.rs"), "//").expect("write thing.rs");
        std::fs::write(root.join("src/main.rs"), "fn main() {}").expect("write main.rs");

        let tree = FileTree::new(&root);
        assert!(tree.all_files().iter().any(|path| path.ends_with("main.rs")));
        assert!(
            !tree.all_files().iter().any(|path| path.ends_with("thing.rs")),
            "target is still skipped when nothing has said otherwise"
        );
        assert!(!tree.ignores().is_repository());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// And the slow half of the same fault: a folder of files with no extension used to cost one `open` and
    /// one four kilobyte `read` each before a row was drawn.
    ///
    /// A test cannot see a syscall, so what is asserted is elapsed time against a ceiling three orders of
    /// magnitude above what the read costs and well below what two thousand file reads cost. The point is
    /// the order of magnitude, not the number.
    #[test]
    fn a_folder_of_two_thousand_extensionless_files_is_read_quickly() {
        let root = std::env::temp_dir().join(format!("unluminate-tree-many-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("make the folder");
        for index in 0..2000 {
            std::fs::write(root.join(format!("entry{index}")), "some text in it").expect("write a file");
        }
        let started = std::time::Instant::now();
        let tree = FileTree::new(&root);
        let took = started.elapsed();
        assert_eq!(tree.rows().len(), 2000, "every file is listed");
        assert!(
            took < std::time::Duration::from_secs(2),
            "reading a folder must not read the files in it: took {took:?}"
        );
        // A name with no extension is **offered**, and whether it really holds text is decided when the tab
        // is opened. That is the bargain that makes the read cheap.
        assert!(tree.rows()[0].entry.openable, "an unknown name is offered rather than refused");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests_task_1693 {
    use super::*;

    /// `task-1693`: a file another program makes appears in the explorer, because the folders that
    /// are showing are asked whether they have changed.
    #[test]
    fn a_file_made_by_something_else_shows_as_a_change() {
        let root = std::env::temp_dir().join("unluminate-tree-watch-new-file");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("make the folder");
        std::fs::write(root.join("one.txt"), "one").expect("write one.txt");
        let tree = FileTree::new(&root);
        assert!(!tree.changed_on_disk(), "nothing has happened yet");

        // A folder's modification time has whole-second resolution on some file systems, so the
        // check is that the *set* of what is watched and its times differ — which writing a second
        // file does whatever the clock says, because the time is read again from the same folder.
        std::fs::write(root.join("two.txt"), "two").expect("write two.txt");
        // Give the file system a moment to settle its timestamp, then ask.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(root.join("three.txt"), "three").expect("write three.txt");
        assert!(tree.changed_on_disk(), "the root was written to, so the tree is out of date");
    }

    /// And a folder that is opened out is watched too, because a file made inside one is a file
    /// somebody can see.
    #[test]
    fn a_folder_that_is_open_is_watched_and_one_that_is_shut_is_not() {
        let root = std::env::temp_dir().join("unluminate-tree-watch-open-folder");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("open")).expect("make the open folder");
        std::fs::create_dir_all(root.join("shut")).expect("make the shut folder");
        let mut tree = FileTree::new(&root);
        tree.expand(&root.join("open"));
        assert!(!tree.changed_on_disk(), "opening a folder is not a change on disk");

        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(root.join("shut/hidden.txt"), "x").expect("write into the shut folder");
        assert!(
            !tree.changed_on_disk(),
            "nothing that is showing has changed, so nothing needs reading again"
        );
        std::fs::write(root.join("open/seen.txt"), "x").expect("write into the open folder");
        assert!(tree.changed_on_disk(), "a folder that is showing gained a file");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a folder to explore. Returns its path; the caller removes it.
    fn sample_folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("notes/deeper")).expect("make the nested folders");
        std::fs::create_dir_all(root.join("archive")).expect("make the archive folder");
        std::fs::create_dir_all(root.join(".hidden")).expect("make a hidden folder");
        std::fs::write(root.join("readme.md"), "# readme").expect("write readme.md");
        std::fs::write(root.join("notes.txt"), "notes").expect("write notes.txt");
        std::fs::write(root.join("program.rs"), "fn main() {}").expect("write program.rs");
        std::fs::write(root.join("notes/one.md"), "one").expect("write notes/one.md");
        std::fs::write(root.join("notes/deeper/two.txt"), "two").expect("write the deep file");
        // A picture. It is not text, and since `task-1658` it opens all the same, in a tab that shows it.
        // The bytes are the start of a PNG, including the zero byte that says it is not text.
        std::fs::write(root.join("picture.png"), [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0])
            .expect("write picture.png");
        // A file that is neither text nor a picture, so a test can check that one of those is listed
        // and dimmed.
        std::fs::write(root.join("bundle.zip"), [0x50, 0x4B, 0x03, 0x04, 0])
            .expect("write bundle.zip");
        root
    }

    #[test]
    fn every_kind_of_text_file_can_be_opened_and_other_files_cannot() {
        assert!(is_openable(Path::new("a.md")));
        assert!(is_openable(Path::new("a.txt")));
        assert!(is_openable(Path::new("a.MD")), "the extension check ignores case");
        assert!(is_openable(Path::new("a.rs")), "a source file opens as plain text");
        assert!(is_openable(Path::new("a.js")));
        assert!(is_openable(Path::new("a.png")), "a picture is not text but it opens all the same");
        assert!(!is_openable(Path::new("a.zip")), "an archive is neither text nor a picture");
    }

    #[test]
    fn a_new_tree_shows_folders_first_then_every_file() {
        let root = sample_folder("unluminate-tree-order");
        let tree = FileTree::new(&root);
        let names: Vec<&str> = tree.rows().iter().map(|row| row.entry.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "archive", "notes", "bundle.zip", "notes.txt", "picture.png", "program.rs",
                "readme.md"
            ]
        );
        assert!(!names.contains(&".hidden"), "hidden entries are not listed");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn what_a_build_wrote_is_shown_in_the_tree_and_left_out_of_what_is_searched() {
        let root = sample_folder("unluminate-tree-build-output");
        std::fs::create_dir_all(root.join("target/debug")).expect("make a build folder");
        std::fs::write(root.join("target/debug/library.rlib"), "not source").expect("write it");
        let tree = FileTree::new(&root);
        let names: Vec<&str> = tree.rows().iter().map(|row| row.entry.name.as_str()).collect();
        assert!(names.contains(&"target"), "the explorer is a picture of the folder: {names:?}");
        let searched: Vec<&str> = tree
            .all_files()
            .iter()
            .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
            .collect();
        assert!(
            !searched.contains(&"library.rlib"),
            "but a search that answers with build output has not answered: {searched:?}"
        );
        assert!(searched.contains(&"readme.md"), "and the project's own files are still there");
        assert!(tree.matching("library").is_empty(), "the filter box searches the same list");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_unluminate_cannot_open_is_listed_but_marked_as_not_openable() {
        let root = sample_folder("unluminate-tree-openable");
        let tree = FileTree::new(&root);
        let entry = |name: &str| {
            tree.rows()
                .iter()
                .find(|row| row.entry.name == name)
                .map(|row| row.entry.clone())
                .unwrap_or_else(|| panic!("{name} is not listed"))
        };
        assert!(entry("readme.md").openable);
        assert!(entry("notes.txt").openable);
        assert!(entry("program.rs").openable, "a Rust file opens as plain text");
        assert!(entry("picture.png").openable, "a picture opens in a tab that shows it");
        let archive = entry("bundle.zip");
        assert!(!archive.openable, "an archive is shown but cannot be opened");
        assert_eq!(archive.refusal, Some(Refusal::NotText), "and it says why");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_folder_is_closed_until_it_is_opened() {
        let root = sample_folder("unluminate-tree-closed");
        let mut tree = FileTree::new(&root);
        assert_eq!(tree.rows().len(), 7);
        tree.toggle(&root.join("notes"));
        let rows = tree.rows();
        let names: Vec<&str> = rows.iter().map(|row| row.entry.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "archive", "notes", "deeper", "one.md", "bundle.zip", "notes.txt", "picture.png",
                "program.rs", "readme.md"
            ]
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn opening_a_folder_indents_its_children_one_step() {
        let root = sample_folder("unluminate-tree-indent");
        let mut tree = FileTree::new(&root);
        tree.toggle(&root.join("notes"));
        tree.toggle(&root.join("notes/deeper"));
        let rows = tree.rows();
        let shown: Vec<(usize, &str)> =
            rows.iter().map(|row| (row.depth, row.entry.name.as_str())).collect();
        assert_eq!(
            shown,
            vec![
                (0, "archive"),
                (0, "notes"),
                (1, "deeper"),
                (2, "two.txt"),
                (1, "one.md"),
                (0, "bundle.zip"),
                (0, "notes.txt"),
                (0, "picture.png"),
                (0, "program.rs"),
                (0, "readme.md"),
            ],
            "folders nest to any depth and each level indents one more step"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn closing_a_folder_hides_its_children_again() {
        let root = sample_folder("unluminate-tree-close");
        let mut tree = FileTree::new(&root);
        tree.toggle(&root.join("notes"));
        assert_eq!(tree.rows().len(), 9);
        tree.toggle(&root.join("notes"));
        assert_eq!(tree.rows().len(), 7);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn expanding_a_deep_path_opens_every_folder_above_it() {
        let root = sample_folder("unluminate-tree-expand");
        let mut tree = FileTree::new(&root);
        tree.expand(&root.join("notes/deeper"));
        let names: Vec<&str> = tree.rows().iter().map(|row| row.entry.name.as_str()).collect();
        assert!(names.contains(&"two.txt"), "the deep file should be visible, rows were {names:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn children_are_read_only_when_a_folder_is_opened() {
        let root = sample_folder("unluminate-tree-lazy");
        let mut tree = FileTree::new(&root);
        let before = tree.find(&root.join("notes")).expect("notes is listed");
        assert!(before.children.is_none(), "a closed folder has not been read from disk");
        tree.toggle(&root.join("notes"));
        let after = tree.find(&root.join("notes")).expect("notes is still listed");
        assert!(after.children.is_some());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reloading_keeps_the_folders_that_were_open() {
        let root = sample_folder("unluminate-tree-reload");
        let mut tree = FileTree::new(&root);
        tree.toggle(&root.join("notes"));
        std::fs::write(root.join("added.md"), "new").expect("add a file");
        tree.reload();
        let names: Vec<&str> = tree.rows().iter().map(|row| row.entry.name.as_str()).collect();
        assert!(names.contains(&"added.md"), "the new file shows up");
        assert!(names.contains(&"one.md"), "the folder that was open is still open");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_file_count_includes_files_in_folders_that_have_not_been_opened() {
        let root = sample_folder("unluminate-tree-count");
        let tree = FileTree::new(&root);
        // readme.md, notes.txt, picture.png, program.rs, bundle.zip, notes/one.md, notes/deeper/two.txt.
        assert_eq!(tree.file_count(), 7, "found {:?}", tree.all_files());
        assert_eq!(tree.openable_count(), 6, "every file but the archive can be opened");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_filter_finds_a_file_inside_a_folder_that_was_never_opened() {
        let root = sample_folder("unluminate-tree-filter");
        let tree = FileTree::new(&root);
        assert!(
            tree.find(&root.join("notes")).is_some_and(|entry| entry.children.is_none()),
            "the folder must still be closed for this test to mean anything"
        );
        let found: Vec<String> = tree
            .matching("two")
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(found, vec!["two.txt"], "the filter should reach into unopened folders");
    }

    #[test]
    fn the_filter_ignores_case_and_matches_part_of_a_name() {
        let root = sample_folder("unluminate-tree-filter-case");
        let tree = FileTree::new(&root);
        let names = |filter: &str| -> Vec<String> {
            tree.matching(filter)
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
                .collect()
        };
        assert_eq!(names("README"), vec!["readme.md"]);
        assert_eq!(names(".md").len(), 2, "readme.md and notes/one.md");
        assert_eq!(names(".png"), vec!["picture.png"], "the filter finds files Unluminate cannot open too");
        assert!(names("").is_empty(), "an empty filter matches nothing, so the tree is shown instead");
        assert!(names("nothing at all").is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_filter_result_reports_how_deep_it_sits() {
        let root = sample_folder("unluminate-tree-depth");
        let tree = FileTree::new(&root);
        assert_eq!(tree.depth_of(&root.join("readme.md")), 0);
        assert_eq!(tree.depth_of(&root.join("notes/one.md")), 1);
        assert_eq!(tree.depth_of(&root.join("notes/deeper/two.txt")), 2);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_folder_that_cannot_be_read_reports_why_rather_than_looking_empty() {
        let missing = std::env::temp_dir().join("unluminate-tree-does-not-exist-at-all");
        std::fs::remove_dir_all(&missing).ok();
        let tree = FileTree::new(&missing);
        assert!(tree.rows().is_empty());
        assert!(tree.last_error.is_some(), "the reason should be reported");
    }
}
