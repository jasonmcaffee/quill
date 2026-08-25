//! The files that are open, and which of them is showing.
//!
//! Everything that belongs to one file rather than to the window lives in an [`OpenFile`]: the
//! document, how far it is scrolled, which of the three ways of looking at it is showing, and what
//! git has to say about it. The window keeps one of these for each tab.
//!
//! There is always at least one. A window that has just started holds one tab with an untitled
//! document in it, and closing the last tab leaves another untitled one rather than a window with no
//! document and a special case everywhere for it.
//!
//! ## The transient tab
//!
//! At most one tab is transient, and it is the one a single click in the explorer reuses. Clicking a
//! second file replaces its contents instead of adding a tab, so reading through a folder does not
//! leave thirty tabs behind. Double clicking a file, or typing into the transient tab, makes it
//! permanent — editing a file you were only glancing at plainly means you meant to open it. This is
//! what IntelliJ does, and it is what `task-1649` describes when it says a double click opens a file
//! in a new tab.

use std::path::{Path, PathBuf};

use quill_core::Document;

use crate::app::ViewMode;
use crate::components::gutter::{BlameRow, Change};

/// One open file, and everything about it that is not about the window.
pub struct OpenFile {
    pub document: Document,
    pub view_mode: ViewMode,
    /// How far the source is scrolled.
    pub scroll: f32,
    /// How far the Markdown preview is scrolled, which is separate from the source.
    pub preview_scroll: f32,
    /// One row a paragraph, once this file has been annotated with git blame.
    pub blame: Option<Vec<BlameRow>>,
    /// Which paragraphs differ from the version git has.
    pub line_changes: Vec<(usize, Change)>,
    /// True while this is the tab a single click reuses.
    pub transient: bool,
    /// Set once git has been asked what it thinks of this file, so it is not asked every frame.
    pub git_asked: bool,
}

impl OpenFile {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            view_mode: ViewMode::Raw,
            scroll: 0.0,
            preview_scroll: 0.0,
            blame: None,
            line_changes: Vec::new(),
            transient: false,
            git_asked: false,
        }
    }

    /// What the tab is called: the file's name, or `untitled` when it has never been saved.
    pub fn name(&self) -> String {
        self.document
            .path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_owned())
    }

    pub fn path(&self) -> Option<&Path> {
        self.document.path()
    }

    /// Git's idea of this file is about the file on disk, so it is thrown away when the file
    /// changes underneath it or a different file takes the tab.
    pub fn forget_git(&mut self) {
        self.blame = None;
        self.line_changes.clear();
        self.git_asked = false;
    }
}

/// The open files, and which one is showing.
pub struct OpenFiles {
    files: Vec<OpenFile>,
    active: usize,
}

impl OpenFiles {
    /// One tab, holding `document`.
    pub fn new(document: Document) -> Self {
        Self { files: vec![OpenFile::new(document)], active: 0 }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> &OpenFile {
        &self.files[self.active]
    }

    pub fn active_mut(&mut self) -> &mut OpenFile {
        &mut self.files[self.active]
    }

    pub fn iter(&self) -> impl Iterator<Item = &OpenFile> {
        self.files.iter()
    }

    pub fn get(&self, index: usize) -> Option<&OpenFile> {
        self.files.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut OpenFile> {
        self.files.get_mut(index)
    }

    /// Which tab holds `path`, if any.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|file| file.path() == Some(path))
    }

    /// Show the tab at `index`, if there is one there.
    pub fn show(&mut self, index: usize) {
        if index < self.files.len() {
            self.active = index;
        }
    }

    /// Show the next tab, wrapping round at the end, which is what Alt and an arrow key do.
    pub fn next(&mut self) {
        if !self.files.is_empty() {
            self.active = (self.active + 1) % self.files.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.files.is_empty() {
            self.active = (self.active + self.files.len() - 1) % self.files.len();
        }
    }

    /// This tab is no longer one a single click will reuse.
    pub fn make_permanent(&mut self, index: usize) {
        if let Some(file) = self.files.get_mut(index) {
            file.transient = false;
        }
    }

    /// Open `document`.
    ///
    /// A file that is already open is shown rather than opened twice, because two tabs on one file
    /// would be two documents over one path and saving either would throw the other away. When
    /// `permanent` is false and there is a transient tab, that tab's contents are replaced; otherwise
    /// a tab is added after the one that is showing, which is where a new tab belongs when it was
    /// opened from the one before it.
    ///
    /// Returns the index of the tab the document ended up in.
    pub fn open(&mut self, document: Document, permanent: bool) -> usize {
        if let Some(path) = document.path() {
            if let Some(index) = self.index_of(path) {
                self.active = index;
                if permanent {
                    self.files[index].transient = false;
                }
                return index;
            }
        }
        let transient = self.files.iter().position(|file| file.transient);
        // An untitled tab that has never been touched is reused as well, so opening the first file
        // in a fresh window does not leave an empty tab beside it.
        let empty = self.files.iter().position(|file| {
            file.path().is_none() && file.document.text().is_empty() && !file.document.is_modified()
        });
        let reuse = if permanent { empty } else { transient.or(empty) };
        match reuse {
            Some(index) => {
                let file = &mut self.files[index];
                file.document = document;
                file.view_mode = ViewMode::Raw;
                file.scroll = 0.0;
                file.preview_scroll = 0.0;
                file.transient = !permanent;
                file.forget_git();
                self.active = index;
                index
            }
            None => {
                let mut file = OpenFile::new(document);
                file.transient = !permanent;
                let at = (self.active + 1).min(self.files.len());
                self.files.insert(at, file);
                self.active = at;
                at
            }
        }
    }

    /// Close the tab at `index`.
    ///
    /// Closing the last one leaves a fresh untitled tab rather than no tabs, so there is never a
    /// window with nothing to type into.
    pub fn close(&mut self, index: usize) {
        if index >= self.files.len() {
            return;
        }
        self.files.remove(index);
        if self.files.is_empty() {
            self.files.push(OpenFile::new(Document::new()));
            self.active = 0;
            return;
        }
        // The tab to the left, as every editor does, so closing several in a row does not jump about.
        if self.active >= index {
            self.active = self.active.saturating_sub(1).min(self.files.len() - 1);
        }
    }

    /// Forget what git said about every open file, which is what happens after an operation that
    /// could have changed any of them.
    pub fn forget_git(&mut self) {
        for file in &mut self.files {
            file.forget_git();
        }
    }

    /// Every open file's path, for a test and for the window's title.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().filter_map(|file| file.path().map(Path::to_path_buf)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(name: &str) -> Document {
        let mut document = Document::from_text(&format!("in {name}\n"));
        let path = std::env::temp_dir().join("quill-open-files").join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, format!("in {name}\n")).expect("write the file");
        document.save_as(&path).expect("save it");
        document
    }

    fn names(files: &OpenFiles) -> Vec<String> {
        files.iter().map(OpenFile::name).collect()
    }

    #[test]
    fn a_new_window_has_one_untitled_tab() {
        let files = OpenFiles::new(Document::new());
        assert_eq!(files.len(), 1);
        assert_eq!(files.active().name(), "untitled");
    }

    #[test]
    fn a_single_click_reuses_the_transient_tab_and_a_double_click_adds_one() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), false);
        assert_eq!(names(&files), vec!["one.md"], "the empty untitled tab was reused");
        files.open(document("two.md"), false);
        assert_eq!(names(&files), vec!["two.md"], "a second glance replaces the transient tab");
        files.open(document("three.md"), true);
        assert_eq!(names(&files), vec!["two.md", "three.md"], "a double click adds a tab");
        assert_eq!(files.active_index(), 1);
    }

    #[test]
    fn a_file_that_is_already_open_is_shown_rather_than_opened_twice() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        assert_eq!(files.active_index(), 1);
        files.open(document("one.md"), true);
        assert_eq!(files.len(), 2, "two tabs on one file would be two documents over one path");
        assert_eq!(files.active_index(), 0);
    }

    #[test]
    fn typing_in_the_transient_tab_makes_it_permanent() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), false);
        assert!(files.active().transient);
        files.make_permanent(files.active_index());
        files.open(document("two.md"), false);
        assert_eq!(names(&files), vec!["one.md", "two.md"], "the tab that was typed into is kept");
    }

    #[test]
    fn a_new_tab_opens_beside_the_one_it_was_opened_from() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files.open(document("three.md"), true);
        files.show(0);
        files.open(document("four.md"), true);
        assert_eq!(names(&files), vec!["one.md", "four.md", "two.md", "three.md"]);
    }

    #[test]
    fn closing_the_last_tab_leaves_an_untitled_one() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.close(0);
        assert_eq!(files.len(), 1);
        assert_eq!(files.active().name(), "untitled");
    }

    #[test]
    fn closing_a_tab_shows_the_one_to_its_left() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files.open(document("three.md"), true);
        assert_eq!(files.active_index(), 2);
        files.close(2);
        assert_eq!(names(&files), vec!["one.md", "two.md"]);
        assert_eq!(files.active().name(), "two.md");
        files.close(0);
        assert_eq!(files.active().name(), "two.md", "closing a tab before the open one keeps it open");
    }

    #[test]
    fn the_next_tab_wraps_round() {
        let mut files = OpenFiles::new(Document::new());
        files.open(document("one.md"), true);
        files.open(document("two.md"), true);
        files.show(1);
        files.next();
        assert_eq!(files.active_index(), 0);
        files.previous();
        assert_eq!(files.active_index(), 1);
    }
}
