//! The window's half of go to definition, find all references and rename.
//!
//! `quill_core::symbols` says what a definition is; `services::symbol_index` says where the
//! project's are; this is what sits between them and the screen. Nothing here draws — the modal is
//! `components::references` and the underline is the editing area's painter — and nothing here
//! decides what a definition is.
//!
//! ## The one rule underneath all of it
//!
//! **A file that is open is owned by its `Document`, and every other file is owned by the index.**
//! It is the rule `services::file_marks` already keeps for the marked passages, and it settles every
//! awkward case here without any of them having to be thought about again: an open tab's
//! definitions are read from its live text and cached on the tab keyed on `text_revision()` — the
//! same key `colour_the_file` is keyed on — so an edit that did not change the text recomputes
//! nothing, and a reference search reads a tab's text rather than the bytes on the disk under it.
//!
//! ## And the one thing it does not trust
//!
//! The index is a picture of the disk as it was when it was last read. A file changed outside Quill
//! is therefore briefly stale, and rather than notice — a watcher on every file in a project, which
//! is a great deal of machinery to be wrong in new ways — the recorded range is **re-checked at the
//! moment it is used**: before jumping into a closed file, its text is read again and the name is
//! confirmed to still be there, re-found by [`quill_core::symbols::file_definitions`] if it has
//! moved. `QuillApp::open_the_match` already re-checks a search hit the same way. A stale entry
//! therefore costs one file read at the moment of a click and can never land a jump on the wrong
//! bytes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use quill_core::symbols::{self, Confidence, FileSymbols, RankKey, Role, SymbolKind};
use quill_core::{Command, Grammar};

use crate::app::QuillApp;
use crate::components::references::{self, Purpose, References};
use crate::services::file_kind;
use crate::services::symbol_index::Indexer;
use crate::services::text_search::Hit;

/// How many places the back stack remembers.
///
/// It is travel history rather than state: not written to disk, not restored when a project opens,
/// and bounded so that a long session cannot grow it without limit.
const HISTORY: usize = 64;

/// Somewhere the caret has been, so it can be gone back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    pub offset: usize,
}

/// One candidate definition: where it is, what it names, and how sure the mechanism is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub name_range: std::ops::Range<usize>,
    pub kind: SymbolKind,
    pub confidence: Confidence,
    /// True when the file it is in is open, which is what decides whether its range has to be
    /// re-checked against the disk before it is jumped to.
    pub open: bool,
}

/// What one open tab's live text says about itself, kept until that text changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabSymbols {
    /// The `text_revision` this was read at.
    pub revision: u64,
    pub read: FileSymbols,
    /// The names this file defines, beside their definitions.
    ///
    /// Kept as names rather than looked up through the text each time, because the question asked
    /// of this is always "what defines *this* name" and answering it should not need the file.
    pub named: Vec<(String, symbols::Definition)>,
    /// Every distinct spelling of an identifier in this file, sorted, each one once.
    ///
    /// What completion offers for the locals, the parameters and the field names — everything the
    /// definers cannot see — and the only thing it can offer in a stylesheet, where `task-1675`
    /// deliberately named no definers at all. Built here rather than per keystroke, keyed on the
    /// same `text_revision` as everything else on this structure, so a caret moving recomputes
    /// nothing.
    pub words: Vec<String>,
}

/// The word under the pointer while the modifier is held, and where a click on it would go.
///
/// Cached against the text revision and the word, which is what makes a pointer resting still cost
/// nothing at all and a pointer moving within one word cost one comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub revision: u64,
    pub word: std::ops::Range<usize>,
    pub name: String,
    /// Where the click would land. Empty means no underline is drawn: the promise the affordance
    /// makes has to be one the click can keep.
    pub candidates: Vec<Candidate>,
    /// True when the pointer is on the definition itself, where the gesture pivots to the
    /// references instead — IntelliJ calls the whole command "Go to Declaration or Usages".
    pub at_definition: bool,
}

/// Convert a byte range from disk text into the LF-only range a `Document` opens.
fn document_range(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let returns_before_start = text[..range.start].matches("\r\n").count();
    let returns_before_end = text[..range.end].matches("\r\n").count();
    range.start - returns_before_start..range.end - returns_before_end
}

impl QuillApp {
    /// Start the index if it has not been started, and read the project again when it has changed.
    ///
    /// Called once a frame from the same place `colour_the_open_file` is called from. The thread is
    /// started lazily rather than in `QuillApp::new`, because a window that never asks a question
    /// about a symbol — every unit test that builds one — should not have a thread reading a folder
    /// behind it.
    ///
    /// Public because the measuring instruments drive a real window with no frames in it, and a
    /// window whose index has never been asked for would measure a project with nothing in it.
    pub fn keep_the_symbol_index_fresh(&mut self) {
        let root = self.tree.root().to_path_buf();
        let files = self.tree.file_count();
        // The file list and the plugins together are what the answer depends on, so a plugin
        // switched off or a folder that grew is what asks for another read. A file *changed* is not
        // in there, deliberately: saving is what says so, through `self.symbols_stale`.
        let asked = (root, files, self.plugins.enabled_count());
        if self.symbols_asked.as_ref() == Some(&asked) && !self.symbols_stale {
            if let Some(indexer) = self.symbols.as_mut() {
                indexer.poll();
            }
            return;
        }
        let waker = self.thread_waker();
        let grammars = Arc::new(self.plugins.grammars());
        let list = self.tree.all_files().to_vec();
        let indexer = self.symbols.get_or_insert_with(|| Indexer::start(waker));
        indexer.rebuild(list, grammars);
        self.symbols_asked = Some(asked);
        self.symbols_stale = false;
    }

    /// Say that a file on the disk has changed, so the index is read again on the next frame.
    pub fn the_project_changed_on_disk(&mut self) {
        self.symbols_stale = true;
    }

    /// The index and the thread it is read on, once something has asked for one.
    ///
    /// Public because a screenshot test has to wait for the read the way it waits for git and for
    /// the text search, and because `symbol_cost` reports what it holds.
    pub fn symbols_indexer(&self) -> Option<&Indexer> {
        self.symbols.as_ref()
    }

    /// The grammar that reads a file, if a plugin that is switched on claims it.
    pub(crate) fn grammar_for(&self, path: Option<&Path>) -> Option<&Grammar> {
        self.plugins.for_path(path?).map(|plugin| &plugin.grammar)
    }

    /// What the tab at `index` defines, read from its live text and kept until that text changes.
    pub(crate) fn tab_symbols(&mut self, index: usize) -> &TabSymbols {
        let revision = self.files.at(index).document.text_revision();
        let fresh = self
            .files
            .at(index)
            .cached
            .symbols
            .as_ref()
            .is_some_and(|read| read.revision == revision);
        if !fresh {
            let grammar = self
                .files
                .at(index)
                .path()
                .and_then(|path| self.plugins.for_path(path))
                .map(|plugin| plugin.grammar.clone())
                .unwrap_or_default();
            let text = self.files.at(index).document.text().to_string();
            let read = FileSymbols::read(&text, &grammar);
            let named = read
                .definitions()
                .iter()
                .map(|definition| {
                    (text[definition.name_range.clone()].to_owned(), definition.clone())
                })
                .collect();
            let words = read.distinct_words(&text);
            self.files.at_mut(index).cached.symbols =
                Some(TabSymbols { revision, read, named, words });
        }
        self.files.at(index).cached.symbols.as_ref().expect("just read")
    }

    /// Every definition of `name` the window knows about, best first.
    ///
    /// The open tabs are asked first and the index supplies everything else, with the open files'
    /// paths dropped from what it says: an open file is owned by its document, so the index's copy
    /// of it is the disk's stale answer and must never be offered beside the live one.
    pub(crate) fn candidates_for(
        &mut self,
        name: &str,
        asked_in: Option<&Path>,
        asked_at: usize,
    ) -> Vec<Candidate> {
        let mut candidates: Vec<Candidate> = Vec::new();
        let open: Vec<PathBuf> = self
            .files
            .iter()
            .filter_map(|file| file.path().map(Path::to_path_buf))
            .collect();
        for index in 0..self.files.len() {
            let Some(path) = self.files.at(index).path().map(Path::to_path_buf) else {
                continue;
            };
            for (known, definition) in &self.tab_symbols(index).named {
                if known != name {
                    continue;
                }
                candidates.push(Candidate {
                    path: path.clone(),
                    name_range: definition.name_range.clone(),
                    kind: definition.kind,
                    confidence: definition.confidence,
                    open: true,
                });
            }
        }
        if let Some(indexer) = self.symbols.as_ref() {
            for entry in indexer.index().definitions_of(name) {
                if open.iter().any(|known| known == &entry.path) {
                    continue;
                }
                candidates.push(Candidate {
                    path: entry.path.clone(),
                    name_range: entry.name_range.clone(),
                    kind: entry.kind,
                    confidence: entry.confidence,
                    open: false,
                });
            }
        }
        self.rank_candidates(candidates, asked_in, asked_at)
    }

    /// Put candidates in the order they should be offered. The order itself is
    /// `quill_core::symbols::rank`, which is where it can be tested with no window.
    fn rank_candidates(
        &self,
        candidates: Vec<Candidate>,
        asked_in: Option<&Path>,
        asked_at: usize,
    ) -> Vec<Candidate> {
        let order = self.symbols.as_ref();
        let keys: Vec<RankKey> = candidates
            .iter()
            .map(|candidate| RankKey {
                same_file: asked_in == Some(candidate.path.as_path()),
                start: candidate.name_range.start,
                kind: candidate.kind,
                confidence: candidate.confidence,
                file_order: order.map_or(0, |indexer| indexer.index().file_order(&candidate.path)),
            })
            .collect();
        let mut ordered: Vec<Candidate> = Vec::with_capacity(candidates.len());
        let mut taken: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
        for index in symbols::rank(&keys, asked_at) {
            if let Some(candidate) = taken[index].take() {
                ordered.push(candidate);
            }
        }
        ordered
    }

    /// Work out what the pointer is over while the modifier is held.
    ///
    /// Cached against `(text_revision, word)`, so a pointer resting still costs one comparison and
    /// a pointer moving within one word costs the same. Only a word that really resolves is
    /// returned, because the underline this draws is a promise the click has to keep.
    pub fn resolve_under_the_pointer(&mut self, offset: usize) -> Option<Hover> {
        let index = self.files.active_index();
        let revision = self.files.at(index).document.text_revision();
        if let Some(hover) = &self.hover {
            if hover.revision == revision && hover.word.start <= offset && offset <= hover.word.end
            {
                return (!hover.candidates.is_empty()).then(|| hover.clone());
            }
        }
        let path = self.files.at(index).path().map(Path::to_path_buf);
        let word = self.tab_symbols(index).read.identifier_at(offset)?;
        let name = self.text_in(index, &word);
        let candidates = self.candidates_for(&name, path.as_deref(), offset);
        let at_definition = candidates.iter().any(|candidate| {
            Some(candidate.path.as_path()) == path.as_deref() && candidate.name_range == word
        });
        let hover = Hover { revision, word, name, candidates, at_definition };
        let resolved = !hover.candidates.is_empty();
        self.hover = Some(hover.clone());
        resolved.then_some(hover)
    }

    /// Forget what the pointer was over, which is what letting go of the modifier does.
    pub fn forget_the_hover(&mut self) {
        self.hover = None;
    }

    /// A stretch of one tab's text.
    fn text_in(&self, index: usize, range: &std::ops::Range<usize>) -> String {
        self.files.at(index).document.text().byte_slice(range.clone())
    }

    /// `Go to Definition`, from the menu, the keyboard, a modifier-click or the command line.
    ///
    /// One candidate is a jump. Several is the modal, listing them ranked, because a picker for
    /// "which `new` did you mean" and a reference list are the same furniture and because nothing
    /// here silently jumps to a guess. None says so in the status bar, which is the mechanism's own
    /// honest answer rather than an invented one.
    ///
    /// A point that is already **on** a definition pivots to the references: going to a definition
    /// from the definition has no other meaning, and the pivot is what makes one gesture serve both
    /// directions of the question.
    pub(crate) fn go_to_definition(&mut self, offset: usize) {
        let index = self.files.active_index();
        let path = self.files.at(index).path().map(Path::to_path_buf);
        let Some(word) = self.tab_symbols(index).read.identifier_at(offset) else {
            self.message = Some("There is no symbol here to go to.".to_owned());
            return;
        };
        let name = self.text_in(index, &word);
        let candidates = self.candidates_for(&name, path.as_deref(), offset);
        let here = candidates.iter().any(|candidate| {
            Some(candidate.path.as_path()) == path.as_deref() && candidate.name_range == word
        });
        if here {
            self.find_references(offset);
            return;
        }
        self.open_definition_candidates(&name, candidates);
    }

    /// Navigate to candidates found from an explicit name rather than an occurrence in the file.
    pub(crate) fn go_to_named_definition(&mut self, name: &str, candidates: Vec<Candidate>) {
        self.open_definition_candidates(name, candidates);
    }

    /// Apply Quill's honest navigation rule to a ranked set of definition candidates.
    fn open_definition_candidates(&mut self, name: &str, candidates: Vec<Candidate>) {
        match candidates.len() {
            0 => self.message = Some(format!("No definition found for '{name}'.")),
            1 => self.jump_to(&candidates[0], &name),
            _ => self.open_candidates(&name, candidates),
        }
    }

    /// Open one candidate, re-checking a closed file's recorded range before trusting it.
    fn jump_to(&mut self, candidate: &Candidate, name: &str) {
        let range = match candidate.open {
            true => Some(candidate.name_range.clone()),
            false => self.confirm_on_disk(candidate, name),
        };
        let Some(range) = range else {
            self.message = Some(format!(
                "'{name}' is no longer in {}, so Quill could not go to it.",
                candidate.path.display()
            ));
            return;
        };
        self.remember_where_we_are();
        self.forward.clear();
        self.open_the_match(&candidate.path.clone(), range);
    }

    /// Read a closed file again and say where `name` really is now.
    ///
    /// The recorded range first, because it is nearly always still right and confirming it is a
    /// comparison; then the file's definitions, in case an edit outside Quill moved it; and nothing
    /// at all when the name has gone, which is reported rather than jumped to.
    fn confirm_on_disk(
        &self,
        candidate: &Candidate,
        name: &str,
    ) -> Option<std::ops::Range<usize>> {
        let text = std::fs::read_to_string(&candidate.path).ok()?;
        if text.get(candidate.name_range.clone()) == Some(name) {
            return Some(document_range(&text, candidate.name_range.clone()));
        }
        let grammar = self.grammar_for(Some(&candidate.path))?;
        symbols::file_definitions(&text, grammar)
            .into_iter()
            .find(|definition| text.get(definition.name_range.clone()) == Some(name))
            .map(|definition| document_range(&text, definition.name_range))
    }

    /// Put where the caret is now on the back stack.
    pub(crate) fn remember_where_we_are(&mut self) {
        let Some(path) = self.files.active().path().map(Path::to_path_buf) else {
            return;
        };
        let offset = self.document().selection().head;
        let place = Place { path, offset };
        if self.back.last() == Some(&place) {
            return;
        }
        self.back.push(place);
        if self.back.len() > HISTORY {
            self.back.remove(0);
        }
    }

    /// `Navigate Back`, and its mirror.
    ///
    /// Reopens the tab if it was closed, because travel history that only worked while nothing had
    /// been tidied away would be history nobody could rely on.
    pub(crate) fn navigate(&mut self, back: bool) {
        let popped = match back {
            true => self.back.pop(),
            false => self.forward.pop(),
        };
        let Some(place) = popped else {
            self.message = Some(
                match back {
                    true => "There is nowhere to go back to.",
                    false => "There is nowhere to go forward to.",
                }
                .to_owned(),
            );
            return;
        };
        // Where we are now goes on the other stack, so the two walk the same list in both
        // directions rather than one of them losing an entry a step at a time.
        if let Some(here) = self.files.active().path().map(Path::to_path_buf) {
            let offset = self.document().selection().head;
            let there = Place { path: here, offset };
            match back {
                true => self.forward.push(there),
                false => self.back.push(there),
            }
        }
        self.open_the_match(&place.path.clone(), place.offset..place.offset);
    }

    /// Where the caret is in the tab that is showing, which is what the three entries act on.
    pub(crate) fn caret_offset(&self) -> usize {
        self.document().selection().head
    }

    /// The text of every open file that has a path, for a search that must read the edits rather
    /// than the disk.
    pub(crate) fn open_texts(&self) -> Vec<(PathBuf, String)> {
        self.files
            .iter()
            .filter_map(|file| {
                let path = file.path()?.to_path_buf();
                Some((path, file.document.text().to_string()))
            })
            .collect()
    }

    /// Whether the three entries apply to the file that is showing.
    pub(crate) fn definitions_apply_here(&self) -> bool {
        file_kind::definitions_apply(self.files.active().path(), &self.plugins.grammars())
    }

    pub(crate) fn symbols_apply_here(&self) -> bool {
        file_kind::symbols_apply(self.files.active().path(), &self.plugins.grammars())
    }

    /// Apply a rename to the open tabs and to the files on the disk.
    ///
    /// The two halves follow the ownership rule and are deliberately different.
    ///
    /// **An open file** is edited as a document: one `Command::ReplaceMany`, which is one undo step,
    /// and the tab is left with unsaved changes rather than being written. A rename must never
    /// silently write a buffer somebody was editing.
    ///
    /// **A closed file** is read, **every chosen range is checked to still hold the old name**, and
    /// only then is it written once. A file that changed since the search is skipped whole and
    /// reported by name — never patched on faith. Bytes outside the replaced ranges are untouched,
    /// so encodings, line endings and trailing whitespace survive byte for byte, and
    /// `services::file_marks` shifts that file's stored marks by the same edits, because a closed
    /// file's marks are the store's and a rename is the one new place a closed file's bytes move.
    ///
    /// Returns what happened, for the status bar and for the command line.
    pub(crate) fn apply_rename(&mut self, change: &RenameChange) -> RenameReport {
        let mut report = RenameReport::default();
        for (path, ranges) in &change.by_file {
            if ranges.is_empty() {
                continue;
            }
            let open = self.files.iter().position(|file| file.path() == Some(path.as_path()));
            match open {
                Some(index) => {
                    let text = self.files.at(index).document.text().to_string();
                    let edits = symbols::replacements(&text, ranges, &change.to);
                    if edits.is_empty() {
                        continue;
                    }
                    let count = edits.len();
                    if self.files.at_mut(index).document.apply(Command::ReplaceMany(edits)) {
                        report.open.push(path.clone());
                        report.changed += count;
                    }
                }
                None => match self.rewrite_closed_file(path, ranges, &change.from, &change.to) {
                    Ok(count) => {
                        report.files.push(path.clone());
                        report.changed += count;
                    }
                    Err(reason) => report.skipped.push((path.clone(), reason)),
                },
            }
        }
        // Everything the index knew about the old name is wrong now, and so is every tab's cache.
        self.the_project_changed_on_disk();
        self.hover = None;
        report
    }

    /// Read a closed file, check every range still holds the old name, and write it once.
    fn rewrite_closed_file(
        &mut self,
        path: &Path,
        ranges: &[std::ops::Range<usize>],
        from: &str,
        to: &str,
    ) -> Result<usize, String> {
        let text = std::fs::read_to_string(path).map_err(|problem| problem.to_string())?;
        for range in ranges {
            if text.get(range.clone()) != Some(from) {
                return Err("it has changed since it was searched".to_owned());
            }
        }
        let edits = symbols::replacements(&text, ranges, to);
        if edits.is_empty() {
            return Ok(0);
        }
        let after = symbols::applied(&text, &edits);
        std::fs::write(path, &after).map_err(|problem| problem.to_string())?;
        // The store owns a closed file's marks, and this is the one place a closed file's bytes
        // move, so they are shifted by the same edits — back to front, for the same reason.
        let shifted = edits.clone();
        self.marks.change(path, |marks| {
            for (range, replacement) in &shifted {
                marks.remove(range.clone());
                marks.insert(range.start, replacement.len());
            }
            marks.clamp(after.len());
        });
        Ok(edits.len())
    }

    /// `Find References`: open the modal on the word at `offset`.
    ///
    /// The folder is read again first, for the reason `Find in Files` reads it: a file made since
    /// the window opened is part of this project and has to be searched.
    pub(crate) fn find_references(&mut self, offset: usize) {
        let Some(name) = self.symbol_here(offset) else {
            self.message = Some("There is no symbol here to find references to.".to_owned());
            return;
        };
        self.tree.reload();
        let waker = self.thread_waker();
        self.references = Some(References::open(Purpose::References, &name, waker));
    }

    /// `Rename Symbol`: the same list, with a tick box on every row and a field above it.
    ///
    /// The default ticks are set once the search has answered rather than now, because there is
    /// nothing to tick yet — see [`Self::tick_the_default_rows`].
    pub(crate) fn rename_symbol(&mut self, offset: usize) {
        let Some(name) = self.symbol_here(offset) else {
            self.message = Some("There is no symbol here to rename.".to_owned());
            return;
        };
        self.tree.reload();
        let waker = self.thread_waker();
        self.references = Some(References::open(Purpose::Rename, &name, waker));
        // What the name resolves to decides how widely the rename is ticked, and the answer is
        // worked out now, while the caret is still where the question was asked from.
        let path = self.files.active().path().map(Path::to_path_buf);
        let candidates = self.candidates_for(&name, path.as_deref(), offset);
        self.rename_kind = candidates.first().map(|candidate| candidate.kind);
        self.rename_here = path;
        self.rename_ticked_up_to = 0;
    }

    /// Show several candidate definitions rather than jumping to a guess.
    fn open_candidates(&mut self, name: &str, candidates: Vec<Candidate>) {
        let hits = candidates
            .iter()
            .filter_map(|candidate| self.candidate_row(candidate, name))
            .collect();
        self.references = Some(References::candidates(name, hits));
    }

    /// One candidate as a row the modal can draw: which line it is on, and the line itself.
    ///
    /// The text comes from the open tab when the file is open and from the disk otherwise, which is
    /// the ownership rule again — a row showing the disk's version of a file somebody is editing
    /// would be a row that does not match the tab underneath it.
    fn candidate_row(&self, candidate: &Candidate, name: &str) -> Option<Hit> {
        let text = match self.files.index_of(&candidate.path) {
            Some(index) => self.files.at(index).document.text().to_string(),
            None => std::fs::read_to_string(&candidate.path).ok()?,
        };
        let start = candidate.name_range.start.min(text.len());
        let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
        let line = text[line_start..].split('\n').next().unwrap_or_default().trim_end_matches('\r');
        let begin = start - line_start;
        let end = (begin + name.len()).min(line.len());
        Some(Hit {
            path: candidate.path.clone(),
            line: text[..line_start].matches('\n').count() + 1,
            text: line.to_owned(),
            range: begin..end,
            offset: candidate.name_range.clone(),
            role: Role::Code,
        })
    }

    /// The word at an offset in the tab that is showing, if the point is a question about a symbol.
    fn symbol_here(&mut self, offset: usize) -> Option<String> {
        if !self.symbols_apply_here() {
            return None;
        }
        let index = self.files.active_index();
        let word = self.tab_symbols(index).read.identifier_at(offset)?;
        Some(self.text_in(index, &word))
    }

    /// Tick the rows a rename should change by default, as they arrive.
    ///
    /// **Only rows that have not been decided yet.** The search streams, so a person can untick
    /// something while more results are still coming in; a default worked out over the whole list
    /// each time a batch landed would put back what they had just taken off. So what is ticked is
    /// the tail — the rows that arrived since the last time this ran — and everything above it is
    /// left exactly as it is, which after the first batch means it is theirs.
    fn tick_the_default_rows(&mut self) {
        let kind = self.rename_kind;
        let here = self.rename_here.clone();
        let Some(modal) = self.references.as_mut() else {
            return;
        };
        if modal.purpose != Purpose::Rename {
            return;
        }
        let decided = self.rename_ticked_up_to.min(modal.hits().len());
        if decided == modal.hits().len() {
            return;
        }
        let mut ticks: Vec<bool> = modal.ticks().to_vec();
        ticks.resize(modal.hits().len(), false);
        for (index, hit) in modal.hits().iter().enumerate().skip(decided) {
            let same_file = here.as_deref() == Some(hit.path.as_path());
            ticks[index] = super::symbols::ticked_by_default(hit.role, kind, same_file);
        }
        self.rename_ticked_up_to = ticks.len();
        modal.set_ticks(ticks);
    }

    /// Work the modal for a frame: run its search, tick its rows, check the new name, and act on
    /// what it asked for.
    ///
    /// Called from the same place the other modals are drawn, so a modal that is open takes the
    /// keyboard the way every other one does.
    pub(crate) fn show_the_references(&mut self, ui: &egui::Ui) {
        if self.references.is_none() {
            return;
        }
        self.work_the_references();
        let Some(mut modal) = self.references.take() else {
            return;
        };
        let outcome = references::show(ui.ctx(), &mut modal, self.panes.references_split);
        if outcome.drag != 0.0 && outcome.panes_height > 0.0 {
            self.panes.references_split = (self.panes.references_split
                + outcome.drag / outcome.panes_height)
                .clamp(references::SPLIT_MIN, references::SPLIT_MAX);
            self.unsaved_settings = true;
        }
        if outcome.reset_split {
            self.panes.references_split = references::SPLIT;
            self.unsaved_settings = true;
        }
        if outcome.rename {
            let change = RenameChange {
                from: modal.name.clone(),
                to: modal.new_name.clone(),
                by_file: modal.change(),
            };
            let report = self.apply_rename(&change);
            self.message = Some(report.sentence(&change.to));
            return; // the modal is dropped, because the list it was showing describes the old name
        }
        if let Some((path, range)) = outcome.open {
            self.remember_where_we_are();
            self.forward.clear();
            self.open_the_match(&path, range);
        }
        if !outcome.close {
            self.references = Some(modal);
        }
    }

    /// Everything the modal needs doing to it before it is drawn: run its search, tick the rows
    /// that have just arrived, and check the new name.
    ///
    /// Split from the drawing so that a test can work the modal without a window, which is the only
    /// way to watch what a streamed answer does to a change set somebody is already editing.
    pub(crate) fn work_the_references(&mut self) {
        let Some(mut modal) = self.references.take() else {
            return;
        };
        let files = self.tree.all_files().to_vec();
        let grammars = Arc::new(self.plugins.grammars());
        let open = Arc::new(self.open_texts());
        modal.pump(&files, grammars, open);
        self.references = Some(modal);
        self.tick_the_default_rows();
        self.check_the_new_name();
    }

    /// Say whether the new name can be used, and warn about a collision without refusing it.
    ///
    /// Two guards, both answered in the modal's footer before anything is applied. The name has to
    /// be a word of this language, because the alternative is a syntax error somebody has to find
    /// by compiling. A collision is a **warning**: the mechanism cannot know whether it shadows —
    /// that is semantic — so it says what it does know and leaves the decision with the person.
    fn check_the_new_name(&mut self) {
        let Some(modal) = self.references.as_ref() else {
            return;
        };
        if modal.purpose != Purpose::Rename {
            return;
        }
        let wanted = modal.new_name.trim().to_owned();
        let grammar = self.grammar_for(self.files.active().path()).cloned().unwrap_or_default();
        let refusal = match symbols::check_name(&wanted, &grammar) {
            Ok(()) => None,
            Err(reason) => Some(reason),
        };
        let warning = refusal.is_none().then(|| self.collision(&wanted)).flatten();
        if let Some(modal) = self.references.as_mut() {
            modal.refusal = refusal;
            modal.warning = warning;
        }
    }

    /// Whether the new name is already defined in a file this rename would touch.
    fn collision(&mut self, wanted: &str) -> Option<String> {
        // A name is always "already defined" as itself, and saying so while the field still holds
        // the name it opened with is a warning about nothing that a person then has to read past.
        if self.references.as_ref()?.name == wanted {
            return None;
        }
        let touched: Vec<PathBuf> = self
            .references
            .as_ref()?
            .change()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        if touched.is_empty() || wanted.is_empty() {
            return None;
        }
        let existing = self.candidates_for(wanted, None, 0);
        let clash = existing.into_iter().find(|candidate| touched.contains(&candidate.path))?;
        let name = clash
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| clash.path.display().to_string());
        Some(format!("'{wanted}' is already defined in {name}"))
    }
}

/// The rename that is about to be applied: which ranges in which files, and the two names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameChange {
    pub from: String,
    pub to: String,
    /// The chosen ranges, by file, in the order the modal listed them.
    pub by_file: Vec<(PathBuf, Vec<std::ops::Range<usize>>)>,
}

impl RenameChange {
    /// How many places it changes, across every file.
    pub fn count(&self) -> usize {
        self.by_file.iter().map(|(_, ranges)| ranges.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

/// What applying a rename came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameReport {
    /// How many places were changed.
    pub changed: usize,
    /// The files on the disk that were rewritten.
    pub files: Vec<PathBuf>,
    /// The open tabs that were edited, which have unsaved changes now.
    pub open: Vec<PathBuf>,
    /// The files that were skipped, and why.
    pub skipped: Vec<(PathBuf, String)>,
}

impl RenameReport {
    /// One sentence for the status bar, saying what happened including what did not.
    pub fn sentence(&self, to: &str) -> String {
        if self.changed == 0 && self.skipped.is_empty() {
            return "Nothing was renamed.".to_owned();
        }
        let places = match self.changed {
            1 => "1 place".to_owned(),
            many => format!("{many} places"),
        };
        let files = self.files.len() + self.open.len();
        let mut sentence = format!("Renamed {places} to '{to}' in {files} files");
        if !self.open.is_empty() {
            sentence.push_str(&format!(
                " \u{00B7} {} open, save when ready",
                self.open.len()
            ));
        }
        for (path, reason) in &self.skipped {
            sentence.push_str(&format!(" \u{00B7} skipped {}: {reason}", path.display()));
        }
        sentence
    }
}

/// Whether a role is one a rename ticks by default. Never: they are textual matches, and the
/// mechanism cannot tell a doc comment mentioning `draw` from prose that happens to use the word.
pub fn ticked_by_default(role: Role, kind: Option<SymbolKind>, same_file: bool) -> bool {
    if role != Role::Code {
        return false;
    }
    match kind {
        // A function, a type, a constant or a module is named once and used everywhere.
        Some(kind) if kind.renames_the_project() => true,
        // A variable, a parameter, or a name with no known definition at all: this file only. A
        // parameter is the second of those working as intended — `fn draw(area: Rect)` gives `area`
        // no definer-keyword definition — and the project-wide rows are still there to tick when the
        // same name in another file really is the same thing.
        _ => same_file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::{Action, MenuState};
    use crate::components::references::Purpose;

    /// A little project to ask questions of: two Rust files, a stylesheet and a note.
    ///
    /// `layout.rs` defines `Layout`, `draw` and `new`; `caret.rs` defines `Caret` and its own `new`,
    /// and uses `draw` twice — once in code and once in a comment.
    fn a_project(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(
            folder.join("layout.rs"),
            "pub struct Layout;\n\nimpl Layout {\n    pub fn new() -> Self {\n        Layout\n    }\n\n    pub fn draw(&self) {}\n}\n",
        )
        .expect("write layout.rs");
        std::fs::write(
            folder.join("caret.rs"),
            "pub struct Caret;\n\nimpl Caret {\n    pub fn new() -> Self {\n        Caret\n    }\n\n    // draw the caret\n    pub fn paint(&self, layout: &Layout) {\n        layout.draw();\n    }\n}\n",
        )
        .expect("write caret.rs");
        std::fs::write(folder.join("site.css"), ".card { color: red; }\n").expect("write site.css");
        std::fs::write(folder.join("notes.md"), "# draw\nA note about draw.\n")
            .expect("write notes.md");
        folder
    }

    /// A window on that project, with its index built and ready to answer.
    fn a_window(name: &str) -> (PathBuf, QuillApp) {
        let folder = a_project(name);
        let mut app = QuillApp::new(&folder);
        build_the_index(&mut app);
        (folder, app)
    }

    /// Read the project and wait for the thread, which is what a frame of the real window does over
    /// however many frames it takes.
    fn build_the_index(app: &mut QuillApp) {
        app.the_project_changed_on_disk();
        app.keep_the_symbol_index_fresh();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            app.keep_the_symbol_index_fresh();
            if app.symbols.as_ref().is_some_and(|indexer| !indexer.is_building()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the index should have been built");
    }

    /// Where a name is in the file that is showing, so a test can ask about it by name rather than
    /// by counting bytes.
    fn at(app: &QuillApp, needle: &str) -> usize {
        let text = app.document().text().to_string();
        text.find(needle).unwrap_or_else(|| panic!("{needle} is not in this file")) + 1
    }

    #[test]
    fn a_definition_in_another_file_opens_that_file_with_the_name_selected() {
        // Scenario 2.
        let (folder, mut app) = a_window("quill-symbols-another-file");
        app.open_path_permanently(&folder.join("caret.rs"));
        let offset = at(&app, "layout.draw()") + "layout.".len();
        app.go_to_definition(offset);
        assert_eq!(
            app.files.active().path(),
            Some(folder.join("layout.rs").as_path()),
            "the tab it opened: {:?}",
            app.message
        );
        assert_eq!(app.document().selected_text(), "draw", "with the name selected");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_word_with_no_definition_says_so_rather_than_guessing() {
        // Scenario 6, the menu path. The click path shows no underline and so rarely gets here.
        let (folder, mut app) = a_window("quill-symbols-no-definition");
        app.open_path_permanently(&folder.join("caret.rs"));
        // `Self` is a keyword, so asking about it is not a question about a symbol at all.
        let keyword = at(&app, "-> Self") + 3;
        app.go_to_definition(keyword);
        assert!(
            app.message.as_deref().is_some_and(|said| said.contains("no symbol")),
            "{:?}",
            app.message
        );
        // And a word the project really does not define says which word. It is written at a place
        // where nothing declares it, because a word that *is* a definition pivots to the references
        // instead — which is scenario 8 rather than this one.
        let use_site = app.document().text().to_string().find("layout.draw()").expect("the call");
        app.document_mut().apply(quill_core::Command::PlaceCaret { offset: use_site, extend: false });
        app.document_mut().apply(quill_core::Command::Insert("nowhere_at_all;\n        ".to_owned()));
        let offset = at(&app, "nowhere_at_all");
        app.go_to_definition(offset);
        assert!(
            app.message.as_deref().is_some_and(|said| said.contains("nowhere_at_all")),
            "{:?}",
            app.message
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn asking_from_the_definition_itself_pivots_to_the_references() {
        // Scenario 8. Going to a definition from the definition has no other meaning.
        let (folder, mut app) = a_window("quill-symbols-pivot");
        app.open_path_permanently(&folder.join("layout.rs"));
        let offset = at(&app, "fn draw") + 3;
        app.go_to_definition(offset);
        let modal = app.references.as_ref().expect("the references modal opened");
        assert_eq!(modal.purpose, Purpose::References);
        assert_eq!(modal.name, "draw");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn two_files_defining_one_name_offer_both_rather_than_choosing() {
        // Scenario 3. Nothing silently jumps to a guess when the mechanism knows it guessed.
        let (folder, mut app) = a_window("quill-symbols-two-candidates");
        app.open_path_permanently(&folder.join("caret.rs"));
        // `new` is defined in both files. Write a call to it somewhere that is neither definition,
        // so the question is asked from a use rather than from one of the answers.
        let use_site = app.document().text().to_string().find("layout.draw()").expect("the call");
        app.document_mut()
            .apply(quill_core::Command::PlaceCaret { offset: use_site, extend: false });
        app.document_mut().apply(quill_core::Command::Insert("new();\n        ".to_owned()));
        let offset = app.document().text().to_string().find("new();").expect("the call") + 1;

        let here = app.files.active().path().map(Path::to_path_buf);
        let candidates = app.candidates_for("new", here.as_deref(), offset);
        assert_eq!(candidates.len(), 2, "both files define one: {candidates:?}");
        assert_eq!(
            candidates[0].path,
            folder.join("caret.rs"),
            "the one in this file is offered first"
        );

        app.go_to_definition(offset);
        let modal = app.references.as_ref().expect("the candidate list opened");
        assert_eq!(modal.purpose, Purpose::Definitions);
        assert_eq!(modal.hits().len(), 2, "and it lists both rather than jumping to one");
        assert_eq!(
            app.files.active().path(),
            Some(folder.join("caret.rs").as_path()),
            "nothing was opened, because nothing was chosen"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn an_open_tab_owns_its_definitions_and_the_index_does_not_answer_for_it() {
        // Scenario 13. The one rule underneath all of this.
        let (folder, mut app) = a_window("quill-symbols-open-owns");
        app.open_path_permanently(&folder.join("layout.rs"));
        // Rename the definition in the tab without saving it.
        let offset =
            app.document().text().to_string().find("fn draw").expect("the definition") + "fn ".len();
        let range = offset..offset + "draw".len();
        app.document_mut().apply(quill_core::Command::ReplaceMany(vec![(range, "sketch".to_owned())]));
        let live = app.candidates_for("sketch", None, 0);
        assert_eq!(live.len(), 1, "the tab's live text is what answers: {live:?}");
        assert!(live[0].open, "and it is marked as coming from an open tab");
        let stale = app.candidates_for("draw", None, 0);
        assert!(
            stale.iter().all(|candidate| candidate.path != folder.join("layout.rs")),
            "the index's copy of an open file must never be offered beside the live one: {stale:?}"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_definition_that_moved_on_the_disk_is_found_again_at_the_moment_of_the_jump() {
        // Scenario 11. A stale index entry costs one file read and can never land on the wrong bytes.
        let (folder, mut app) = a_window("quill-symbols-moved-on-disk");
        app.open_path_permanently(&folder.join("caret.rs"));
        // Push `draw` a long way down layout.rs behind the index's back.
        let moved = format!("{}\n{}", "// a new comment line\n".repeat(20), std::fs::read_to_string(folder.join("layout.rs")).expect("read"));
        std::fs::write(folder.join("layout.rs"), &moved).expect("write");
        let offset = at(&app, "layout.draw()") + "layout.".len();
        app.go_to_definition(offset);
        assert_eq!(app.files.active().path(), Some(folder.join("layout.rs").as_path()));
        assert_eq!(
            app.document().selected_text(),
            "draw",
            "the range was re-checked and re-found: {:?}",
            app.message
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_file_deleted_since_it_was_indexed_reports_and_does_not_crash() {
        // Scenario 12.
        let (folder, mut app) = a_window("quill-symbols-deleted");
        app.open_path_permanently(&folder.join("caret.rs"));
        std::fs::remove_file(folder.join("layout.rs")).expect("delete it");
        let offset = at(&app, "layout.draw()") + "layout.".len();
        app.go_to_definition(offset);
        assert!(app.message.is_some(), "it says what happened");
        assert_eq!(
            app.files.active().path(),
            Some(folder.join("caret.rs").as_path()),
            "and the tab that was open is still open"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn going_back_through_two_jumps_and_forward_again() {
        // Scenario 18.
        let (folder, mut app) = a_window("quill-symbols-navigate");
        app.open_path_permanently(&folder.join("caret.rs"));
        let started = app.document().selection().head;
        let offset = at(&app, "layout.draw()") + "layout.".len();
        app.document_mut().apply(quill_core::Command::PlaceCaret { offset, extend: false });
        let left_from = app.document().selection().head;
        app.go_to_definition(offset);
        assert_eq!(app.files.active().path(), Some(folder.join("layout.rs").as_path()));
        assert_eq!(app.back.len(), 1);

        app.navigate(true);
        assert_eq!(app.files.active().path(), Some(folder.join("caret.rs").as_path()));
        assert_eq!(app.document().selection().head, left_from, "back to where the caret was");
        assert_eq!(app.forward.len(), 1);

        app.navigate(false);
        assert_eq!(app.files.active().path(), Some(folder.join("layout.rs").as_path()), "forward again");

        // A new jump clears the forward stack, exactly as a browser's does.
        app.navigate(true);
        assert_eq!(app.forward.len(), 1);
        let offset = at(&app, "layout.draw()") + "layout.".len();
        app.go_to_definition(offset);
        assert!(app.forward.is_empty(), "a new jump clears it");
        let _ = started;
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn there_is_nowhere_to_go_back_to_at_the_start_and_it_says_so() {
        let (folder, mut app) = a_window("quill-symbols-nowhere");
        app.open_path_permanently(&folder.join("caret.rs"));
        app.navigate(true);
        assert!(app.message.as_deref().is_some_and(|said| said.contains("nowhere")), "{:?}", app.message);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn asking_with_the_caret_on_nothing_says_so_and_opens_no_modal() {
        // Scenario 26.
        let (folder, mut app) = a_window("quill-symbols-whitespace");
        app.open_path_permanently(&folder.join("layout.rs"));
        let blank = at(&app, "\n\nimpl");
        app.find_references(blank);
        assert!(app.references.is_none(), "no modal opened");
        assert!(app.message.is_some(), "and it said why");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_symbol_entries_are_absent_for_a_file_whose_language_cannot_answer() {
        // Scenarios 16 and 17, through the menu the window really builds. `Complete Word` sits with
        // them and asks a wider question — a plugin claiming the file at all — so a stylesheet has
        // it and a note does not.
        let (folder, mut app) = a_window("quill-symbols-absent");
        let names = |state: &MenuState| -> Vec<String> {
            crate::app::actions::symbol_entries(state)
                .iter()
                .filter_map(|entry| match entry {
                    crate::app::actions::Entry::Item { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect()
        };
        app.open_path_permanently(&folder.join("layout.rs"));
        assert_eq!(
            names(&app.menu_state()),
            vec!["Go to Definition", "Find References", "Rename Symbol...", "Complete Word"],
            "a Rust file has all four"
        );
        app.open_path_permanently(&folder.join("site.css"));
        assert_eq!(
            names(&app.menu_state()),
            vec!["Find References", "Rename Symbol...", "Complete Word"],
            "a stylesheet has no definitions, and keeps the other three"
        );
        app.open_path_permanently(&folder.join("notes.md"));
        assert!(names(&app.menu_state()).is_empty(), "a note has none of them");
        // And the editing area's own menu asks the same function, so it cannot disagree.
        let state = app.menu_state();
        let text_menu = crate::app::actions::text_menu(&state);
        assert!(
            !text_menu.iter().any(|entry| matches!(
                entry,
                crate::app::actions::Entry::Item { action: Action::GoToDefinition, .. }
            )),
            "the right click menu is absent for a note too"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_rename_edits_the_open_tab_as_a_document_and_the_closed_files_on_the_disk() {
        // Scenarios 33, 40 and 50's split: the open half is one undo step and is left unsaved; the
        // closed half is written once and its bytes outside the ranges are untouched.
        let (folder, mut app) = a_window("quill-symbols-rename-split");
        app.open_path_permanently(&folder.join("layout.rs"));
        let before = std::fs::read_to_string(folder.join("caret.rs")).expect("read caret.rs");
        let open_before = app.document().text().to_string();
        let change = RenameChange {
            from: "draw".to_owned(),
            to: "sketch".to_owned(),
            by_file: vec![
                (folder.join("layout.rs"), vec![ranges_of(&open_before, "draw")[0].clone()]),
                (folder.join("caret.rs"), vec![ranges_of(&before, "draw")[1].clone()]),
            ],
        };
        let report = app.apply_rename(&change);
        assert_eq!(report.changed, 2, "{report:?}");
        assert_eq!(report.open, vec![folder.join("layout.rs")]);
        assert_eq!(report.files, vec![folder.join("caret.rs")]);
        assert!(report.skipped.is_empty());

        // The open tab: edited, unsaved, and one step to undo.
        assert!(app.document().text().to_string().contains("fn sketch"));
        assert!(app.document().is_modified(), "a rename must never silently write a buffer");
        assert_eq!(
            std::fs::read_to_string(folder.join("layout.rs")).expect("read"),
            open_before,
            "and the file on the disk is untouched until it is saved"
        );
        app.document_mut().apply(quill_core::Command::Undo);
        assert_eq!(app.document().text().to_string(), open_before, "one step puts it back");

        // The closed file: written once, and every other byte identical.
        let after = std::fs::read_to_string(folder.join("caret.rs")).expect("read caret.rs");
        assert_eq!(after, before.replacen("layout.draw()", "layout.sketch()", 1));
        assert!(after.contains("// draw the caret"), "the comment was not ticked, so it did not change");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_closed_file_that_changed_since_the_search_is_skipped_whole_and_reported() {
        // Scenario 42. Never patched on faith.
        let (folder, mut app) = a_window("quill-symbols-changed-underneath");
        let caret = std::fs::read_to_string(folder.join("caret.rs")).expect("read");
        let range = ranges_of(&caret, "draw")[1].clone();
        // Rewrite the file so the recorded range no longer holds the old name.
        std::fs::write(folder.join("caret.rs"), format!("// changed\n{caret}")).expect("write");
        let layout = std::fs::read_to_string(folder.join("layout.rs")).expect("read");
        let change = RenameChange {
            from: "draw".to_owned(),
            to: "sketch".to_owned(),
            by_file: vec![
                (folder.join("caret.rs"), vec![range]),
                (folder.join("layout.rs"), vec![ranges_of(&layout, "draw")[0].clone()]),
            ],
        };
        let report = app.apply_rename(&change);
        assert_eq!(report.skipped.len(), 1, "{report:?}");
        assert_eq!(report.skipped[0].0, folder.join("caret.rs"));
        assert!(report.skipped[0].1.contains("changed"));
        assert_eq!(report.files, vec![folder.join("layout.rs")], "the other file was still applied");
        assert!(report.sentence("sketch").contains("skipped"), "and the sentence says so");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_closed_files_stored_marks_are_shifted_by_the_rename_that_moved_its_bytes() {
        // Scenario 43. `FileMarks` owns a closed file's marks, and this is the one new place a
        // closed file's bytes move.
        let (folder, mut app) = a_window("quill-symbols-rename-marks");
        let caret = folder.join("caret.rs");
        let text = std::fs::read_to_string(&caret).expect("read");
        let word = text.find("paint").expect("paint");
        app.marks.change(&caret, |marks| {
            marks.add(word..word + "paint".len(), quill_core::Rgba::parse("#ffff0080").expect("colour"));
        });
        // Rename something *before* the mark, so the mark has to move.
        let change = RenameChange {
            from: "Caret".to_owned(),
            to: "TextCaret".to_owned(),
            by_file: vec![(caret.clone(), vec![ranges_of(&text, "Caret")[0].clone()])],
        };
        let report = app.apply_rename(&change);
        assert_eq!(report.changed, 1, "{report:?}");
        let after = std::fs::read_to_string(&caret).expect("read");
        let mark = app.marks.highlights(&caret).expect("the mark").iter().next().expect("one").range.clone();
        assert_eq!(
            &after[mark.clone()],
            "paint",
            "the mark moved with the text: {mark:?} of {after:?}"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_rename_of_a_variable_defaults_to_this_file_and_of_a_function_to_the_project() {
        // Scenarios 34, 35 and 36, at the rule that decides them.
        assert!(ticked_by_default(Role::Code, Some(SymbolKind::Function), false));
        assert!(ticked_by_default(Role::Code, Some(SymbolKind::Type), false));
        assert!(ticked_by_default(Role::Code, Some(SymbolKind::Constant), false));
        assert!(ticked_by_default(Role::Code, Some(SymbolKind::Module), false));
        // A local, and a parameter — which has no definer-keyword definition at all — are this file.
        assert!(!ticked_by_default(Role::Code, Some(SymbolKind::Variable), false));
        assert!(ticked_by_default(Role::Code, Some(SymbolKind::Variable), true));
        assert!(!ticked_by_default(Role::Code, None, false));
        assert!(ticked_by_default(Role::Code, None, true));
        // And in every case, a comment or a string is left unticked.
        for kind in [None, Some(SymbolKind::Function), Some(SymbolKind::Variable)] {
            assert!(!ticked_by_default(Role::Comment, kind, true));
            assert!(!ticked_by_default(Role::String, kind, true));
        }
    }

    #[test]
    fn a_comment_or_a_string_ticked_by_hand_is_applied_like_any_other_row() {
        // Scenario 44. They are unticked by **default** because they are textual matches, not
        // because they cannot be changed: a doc comment naming the thing being renamed is very
        // often exactly what a person wants updated, and once it is ticked it is an ordinary row.
        let (folder, mut app) = a_window("quill-symbols-rename-a-comment");
        let caret = folder.join("caret.rs");
        let text = std::fs::read_to_string(&caret).expect("read");
        // The first `draw` in caret.rs is the one inside `// draw the caret`.
        let comment = ranges_of(&text, "draw")[0].clone();
        assert_eq!(
            symbols::occurrences(&text, "draw", &a_rust_grammar())[0].role,
            Role::Comment,
            "the fixture's first one really is in a comment"
        );
        let report = app.apply_rename(&RenameChange {
            from: "draw".to_owned(),
            to: "sketch".to_owned(),
            by_file: vec![(caret.clone(), vec![comment])],
        });
        assert_eq!(report.changed, 1, "{report:?}");
        let after = std::fs::read_to_string(&caret).expect("read");
        assert!(after.contains("// sketch the caret"), "{after:?}");
        assert!(after.contains("layout.draw()"), "and nothing else moved");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_large_project_costs_definitions_rather_than_occurrences_and_still_cancels_at_once() {
        // Scenario 32. What keeps a large project from costing memory is **what is stored**: the
        // index holds where each name is *defined*, and nothing at all about where it is used. A
        // table of every occurrence would be the far larger one, and §3.4 records why it would also
        // be the one that has to be invalidated. The other half is that a build which has been
        // overtaken hands nothing over rather than a part-finished answer.
        let folder = std::env::temp_dir().join("quill-symbols-large");
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        // Every file defines one name of its own and uses one nothing defines, five times over.
        for number in 0..1500 {
            std::fs::write(
                folder.join(format!("file{number}.rs")),
                format!("pub fn thing{number}() {{\n{}}}\n", "    shared();\n".repeat(5)),
            )
            .expect("write one of them");
        }
        let mut app = QuillApp::new(&folder);
        let walked = app.tree.all_files().len();
        build_the_index(&mut app);
        let index = app.symbols.as_ref().expect("an indexer").index();
        assert_eq!(index.files(), walked, "the index read the list the walker handed it");
        assert_eq!(index.names(), walked, "one name a file, because that is what they define");
        assert_eq!(
            index.len(),
            walked,
            "and one entry a definition \u{2014} the {} uses of `shared` are not in here at all",
            walked * 5
        );
        assert!(index.definitions_of("shared").is_empty(), "nothing defines it, so it is absent");
        assert!(!index.capped());

        // A build that has been overtaken stops where it is and hands nothing over, so the memory a
        // half-finished one would have held is never kept either.
        let files = app.tree.all_files().to_vec();
        let grammars = app.plugins.grammars();
        assert!(crate::services::symbol_index::Index::build(&files, &grammars, &|| true).is_none());
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_row_unticked_by_hand_survives_the_rest_of_the_answer_arriving() {
        // The search streams, so results land under a person who is already deciding what to change.
        // A default worked out over the whole list each time a batch arrived would put back what
        // they had just taken off, which on a long list is a change nobody asked for and nobody
        // sees.
        let (folder, mut app) = a_window("quill-symbols-streaming-ticks");
        app.open_path_permanently(&folder.join("layout.rs"));
        let offset =
            app.document().text().to_string().find("fn draw").expect("the definition") + 3;
        app.rename_symbol(offset);
        // Work it as the window does, a frame at a time, until the search has answered.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            app.work_the_references();
            if !app.references.as_ref().expect("the modal").is_searching() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let modal = app.references.as_ref().expect("the modal");
        assert!(!modal.is_searching(), "the search should have finished");
        assert!(modal.ticked_count() > 1, "a function is ticked across the project by default");

        // Untick the first row by hand, then let more frames go by.
        let mut ticks = modal.ticks().to_vec();
        ticks[0] = false;
        app.references.as_mut().expect("the modal").set_ticks(ticks);
        for _ in 0..5 {
            app.work_the_references();
        }
        assert!(
            !app.references.as_ref().expect("the modal").ticks()[0],
            "the row somebody unticked stays unticked"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_rename_with_no_files_in_it_changes_nothing_and_says_so() {
        // Scenario 47's other half, and the empty case every applier needs.
        let (folder, mut app) = a_window("quill-symbols-rename-nothing");
        let report = app.apply_rename(&RenameChange {
            from: "draw".to_owned(),
            to: "sketch".to_owned(),
            by_file: Vec::new(),
        });
        assert_eq!(report.changed, 0);
        assert_eq!(report.sentence("sketch"), "Nothing was renamed.");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn two_panes_showing_two_affected_files_both_take_the_edit_once() {
        // Scenario 50. Each pane's document is edited once, because a rename is applied per file
        // rather than per pane and a file has one document however many panes are showing it.
        let (folder, mut app) = a_window("quill-symbols-two-panes");
        app.open_path_permanently(&folder.join("layout.rs"));
        app.files.split_right();
        app.open_path_permanently(&folder.join("caret.rs"));
        assert_eq!(app.files.pane_count(), 2, "two panes");
        let layout = app.files.index_of(&folder.join("layout.rs")).expect("layout.rs is open");
        let caret = app.files.index_of(&folder.join("caret.rs")).expect("caret.rs is open");
        let layout_text = app.files.at(layout).document.text().to_string();
        let caret_text = app.files.at(caret).document.text().to_string();
        let report = app.apply_rename(&RenameChange {
            from: "draw".to_owned(),
            to: "sketch".to_owned(),
            by_file: vec![
                (folder.join("layout.rs"), vec![ranges_of(&layout_text, "draw")[0].clone()]),
                (folder.join("caret.rs"), vec![ranges_of(&caret_text, "draw")[1].clone()]),
            ],
        });
        assert_eq!(report.open.len(), 2, "both tabs were edited: {report:?}");
        assert_eq!(report.changed, 2, "and each once");
        assert!(app.files.at(layout).document.text().to_string().contains("fn sketch"));
        assert!(app.files.at(caret).document.text().to_string().contains("layout.sketch()"));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn what_a_build_wrote_is_never_indexed_or_searched() {
        // Scenario 31. The walker already leaves them out, and this is what says the index inherits
        // that rather than walking the disk itself.
        let folder = a_project("quill-symbols-generated");
        let built = folder.join("target");
        std::fs::create_dir_all(&built).expect("make target");
        std::fs::write(built.join("generated.rs"), "pub fn draw() {}\n").expect("write");
        let mut app = QuillApp::new(&folder);
        build_the_index(&mut app);
        let candidates = app.candidates_for("draw", None, 0);
        assert!(
            candidates.iter().all(|candidate| !candidate.path.starts_with(&built)),
            "nothing under target is indexed: {candidates:?}"
        );
        assert!(!candidates.is_empty(), "and the real one still is");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_reference_search_reads_an_open_tab_as_it_stands_and_the_disk_for_the_rest() {
        // Scenario 27. What the search is handed is what the window really holds.
        let (folder, mut app) = a_window("quill-symbols-open-texts");
        app.open_path_permanently(&folder.join("layout.rs"));
        app.document_mut().apply(quill_core::Command::MoveDocumentEnd { extend: false });
        app.document_mut().apply(quill_core::Command::Insert("\n// draw again\n".to_owned()));
        let open = app.open_texts();
        let (path, text) = open.iter().find(|(path, _)| path.ends_with("layout.rs")).expect("the tab");
        assert!(text.contains("draw again"), "the unsaved edit is what would be searched");
        assert_ne!(
            *text,
            std::fs::read_to_string(path).expect("read"),
            "and it is not what is on the disk"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_index_is_read_again_when_a_file_is_saved_and_not_on_every_frame() {
        let (folder, mut app) = a_window("quill-symbols-staleness");
        // Nothing changed: asking again does not start another build.
        app.keep_the_symbol_index_fresh();
        assert!(!app.symbols.as_ref().expect("an indexer").is_building());
        // A save says the disk moved, and the next frame reads it again.
        app.the_project_changed_on_disk();
        app.keep_the_symbol_index_fresh();
        assert!(app.symbols.as_ref().expect("an indexer").is_building() || !app.symbols_stale);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_command_line_asks_the_same_question_the_menu_does() {
        // The rule the whole command line keeps: a thing done from a script and the same thing done
        // by hand are the same thing. This goes down the real parser, the real dispatch and the
        // real reply — everything but the socket.
        let (folder, mut app) = a_window("quill-symbols-cli");
        let context = egui::Context::default();
        app.open_path_permanently(&folder.join("caret.rs"));
        let line = app.document().text().to_string()[..at(&app, "layout.draw()")]
            .matches('\n')
            .count()
            + 1;
        let column = "        layout.".len() + 2;
        let reply = app
            .run_command_line(&format!("editor definition --line {line} --column {column}"), &context)
            .expect("an answer");
        assert!(reply.ok, "{reply:?}");
        assert_eq!(reply.result["name"], "draw");
        assert_eq!(reply.result["candidates"].as_array().expect("a list").len(), 1);
        assert_eq!(reply.result["candidates"][0]["kind"], "function");
        assert_eq!(reply.result["candidates"][0]["confidence"], "sure");

        // And `--open` is the jump itself, through the same function the menu entry goes through.
        let reply = app
            .run_command_line(
                &format!("editor definition --line {line} --column {column} --open"),
                &context,
            )
            .expect("an answer");
        assert!(reply.ok, "{reply:?}");
        assert_eq!(app.files.active().path(), Some(folder.join("layout.rs").as_path()));
        assert_eq!(app.document().selected_text(), "draw");

        // Back again, also from the command line.
        let reply = app.run_command_line("editor navigate-back", &context).expect("an answer");
        assert!(reply.ok, "{reply:?}");
        assert_eq!(app.files.active().path(), Some(folder.join("caret.rs").as_path()));
        let reply = app.run_command_line("editor navigate-back", &context).expect("an answer");
        assert!(!reply.ok, "a command that did nothing says so rather than reporting success");
        std::fs::remove_dir_all(&folder).ok();
    }

    /// A direct name reaches the project index even when the active file cannot define symbols.
    #[test]
    fn the_command_line_can_open_a_definition_by_name_without_finding_an_occurrence_first() {
        let (folder, mut app) = a_window("quill-symbols-cli-name");
        let context = egui::Context::default();
        let layout = folder.join("layout.rs");
        let text = std::fs::read_to_string(&layout).expect("read layout.rs");
        std::fs::write(&layout, text.replace('\n', "\r\n")).expect("write Windows line endings");
        app.open_path_permanently(&folder.join("notes.md"));
        let reply = app
            .run_command_line("editor definition draw --open", &context)
            .expect("an answer");
        assert!(reply.ok, "{reply:?}");
        assert_eq!(reply.result["name"], "draw");
        assert_eq!(reply.result["candidates"].as_array().expect("a list").len(), 1);
        assert_eq!(app.files.active().path(), Some(folder.join("layout.rs").as_path()));
        assert_eq!(app.document().selected_text(), "draw");

        let reply = app.run_command_line("editor navigate-back", &context).expect("an answer");
        assert!(reply.ok, "{reply:?}");
        assert_eq!(app.files.active().path(), Some(folder.join("notes.md").as_path()));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_command_line_refuses_a_new_name_this_language_could_not_hold() {
        // Scenario 38 at the command line's end: refused before anything is searched for, because
        // the alternative is a syntax error somebody has to find by compiling.
        let (folder, mut app) = a_window("quill-symbols-cli-refusal");
        let context = egui::Context::default();
        app.open_path_permanently(&folder.join("layout.rs"));
        let offset = app.document().text().to_string().find("fn draw").expect("it") + 4;
        app.document_mut().apply(quill_core::Command::PlaceCaret { offset, extend: false });
        let reply = app.run_command_line("editor rename match", &context).expect("an answer");
        assert!(!reply.ok);
        assert!(reply.message.contains("keyword"), "{}", reply.message);
        assert!(app.references.is_none(), "and nothing was even searched for");
        // A scope that is not one of the two is refused the same way.
        let reply = app
            .run_command_line("editor rename sketch --scope everywhere", &context)
            .expect("an answer");
        assert!(!reply.ok);
        assert!(reply.message.contains("file") && reply.message.contains("project"));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_file_whose_language_says_nothing_refuses_the_symbol_commands_rather_than_answering_emptily()
    {
        let (folder, mut app) = a_window("quill-symbols-cli-not-applicable");
        let context = egui::Context::default();
        app.open_path_permanently(&folder.join("notes.md"));
        let reply = app.run_command_line("editor definition", &context).expect("an answer");
        assert!(!reply.ok);
        assert!(reply.message.contains("definition"), "{}", reply.message);
        let reply = app.run_command_line("editor references", &context).expect("an answer");
        assert!(!reply.ok, "no plugin claims a note, so one of its words is not a symbol");
        // A stylesheet has no definitions and keeps the other two.
        app.open_path_permanently(&folder.join("site.css"));
        let reply = app.run_command_line("editor definition", &context).expect("an answer");
        assert!(!reply.ok, "a custom property is defined by position rather than by a keyword");
        std::fs::remove_dir_all(&folder).ok();
    }

    /// The Rust grammar as the bundled plugin describes it.
    fn a_rust_grammar() -> Grammar {
        crate::services::plugins::Plugins::load(None)
            .0
            .grammars()
            .for_path(Path::new("a.rs"))
            .expect("the rust plugin")
            .clone()
    }

    /// Every whole-word range of a name in a piece of text, the way the search finds them.
    fn ranges_of(text: &str, name: &str) -> Vec<std::ops::Range<usize>> {
        symbols::occurrences(text, name, &a_rust_grammar())
            .into_iter()
            .map(|found| found.range)
            .collect()
    }
}
