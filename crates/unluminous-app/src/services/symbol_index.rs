//! The project's definitions, held in memory and built on a thread.
//!
//! `unluminous_core::symbols` says what a definition *is*; this says where the project's are. It is the
//! app-side half of `task-1675`, and it is arranged exactly as `services::text_search` already is,
//! for the same reason: reading every file in a project where the window draws would stop it
//! drawing, which on a large folder looks exactly like a crash.
//!
//! Four decisions worth writing down.
//!
//! **Definitions are indexed and occurrences are not.** A `name -> where it is defined` table is
//! small — a name, a range and two discriminants a definition, a few hundred kilobytes for a
//! project this size — and it answers the question a pointer asks sixty times a second while the
//! modifier is held. Every *occurrence* of every identifier would be a far larger table, and it
//! would cost the one thing a search never has to pay: invalidation. `Find References` is therefore
//! a search of what is on the disk now, in `text_search`'s whole-word mode, and a build that
//! rewrote a generated file needs to tell nobody. `tasks/task-1675-code-editing-tdd.md` §3.4
//! records the crossover at which that stops being the better answer.
//!
//! **Only the files a language claims are read at all.** A project full of pictures, JSON and
//! Markdown costs the walk and nothing else, because [`Grammars::defines_symbols`] answers from the
//! extension before anything is opened.
//!
//! **Only the newest question is answered.** Each request carries a number, the newest is shared
//! with the thread as an `AtomicU64`, and a build that has been overtaken stops where it is — the
//! rule `text_search` already keeps, and what makes reopening a project while one is running cost
//! nothing.
//!
//! **A file that is open is not in here.** It is owned by its `Document` and its definitions are
//! worked out from the live text and cached on its tab, so what the index holds for it would be the
//! disk's stale answer. [`Index::definitions_of`] therefore hands back everything it knows and the
//! window drops the open paths, which is one rule in one place rather than an index that has to be
//! told every time a tab opens.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use unluminous_core::symbols::{self, Confidence, SymbolKind};

use crate::services::file_kind;
use crate::services::plugins::Grammars;

/// The most definitions one project's index holds.
///
/// A project past this has a problem an index is not going to fix, and a table that grows without
/// limit is a window that runs out of memory on a folder somebody pointed it at by accident.
const LIMIT: usize = 200_000;

/// One definition, and which file it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name_range: std::ops::Range<usize>,
    pub kind: SymbolKind,
    pub confidence: Confidence,
    /// True when another file could import this name. `task-1680`.
    pub exported: bool,
}

/// One name a file exports, which is what a list offered inside an import is made of.
///
/// A second, smaller table beside [`Index::by_name`], and it holds only the **exported**
/// definitions, which is what keeps it small. The alternative was reading the module off the disk on
/// each keystroke: 1.8 ms for the largest file in this repository, paid on every letter typed
/// between the braces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub kind: SymbolKind,
    pub confidence: Confidence,
}

/// Everything the project defines, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    /// The table itself. Keyed by the name so that the question a pointer asks is one hash probe
    /// and no allocation: `HashMap<String, _>` is looked up by `&str`, so nothing is built to ask.
    by_name: HashMap<String, Vec<Entry>>,
    /// The files that were read, in the order they were walked, so a tie between two candidates in
    /// two files is broken the same way every time.
    order: Vec<PathBuf>,
    /// How many files were read, which is what the measuring instrument and the tests report.
    files: usize,
    /// True when the walk stopped at [`LIMIT`] rather than at the end of the project.
    capped: bool,
    /// Every name the table holds, sorted, each one once.
    ///
    /// The hash table answers "where is *this* name defined" and cannot answer "which names could
    /// `lyt` become", which is the question completion asks a keystroke at a time. A sorted list
    /// answers it with one walk — and, for a stem that is a prefix, with a binary search into the
    /// range it starts. Built once when the index is built rather than collected per keystroke,
    /// because `task-1666`'s rule is that nothing running that often may allocate: on Unluminous's own
    /// repository this is 4,445 strings the table already owns a copy of.
    names: Vec<String>,
    /// What each file exports. `task-1680`: the question an import asks is the other way round from
    /// the one a jump asks — not "where is this name" but "what does this file have" — and the
    /// table above cannot answer it without being walked whole.
    by_file: HashMap<PathBuf, Vec<Export>>,
}

impl Index {
    /// Where `name` is defined, or nothing.
    ///
    /// One hash probe against an interned name and a borrowed slice out: this runs while the
    /// pointer moves with the modifier held, and `task-1666`'s rule is that nothing which runs that
    /// often may allocate.
    pub fn definitions_of(&self, name: &str) -> &[Entry] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Where this file came in the walk, which is only ever used to break a tie.
    ///
    /// A file the index has never read sorts after every file it has, so a candidate from an open
    /// tab that was never on the disk still has a settled place.
    pub fn file_order(&self, path: &Path) -> usize {
        self.order.iter().position(|known| known == path).unwrap_or(usize::MAX)
    }

    /// How many files were read.
    pub fn files(&self) -> usize {
        self.files
    }

    /// How many definitions there are, across every file.
    pub fn len(&self) -> usize {
        self.by_name.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// How many names are known, which is the table's own size rather than the definitions'.
    pub fn names(&self) -> usize {
        self.by_name.len()
    }

    /// Every name the index holds, sorted and each one once.
    ///
    /// What completion scans for a stem. Borrowed rather than copied, because it runs at keystroke
    /// time and the caller only reads it.
    pub fn sorted_names(&self) -> &[String] {
        &self.names
    }

    /// What this file exports, in the order it declares them.
    ///
    /// The disk-owned half of an import's named list. The ownership rule of `task-1675` §3.3 says
    /// what the other half is: a module that is **open** is owned by its `Document`, and its
    /// exports are read from the live text on its tab instead.
    pub fn exports_of(&self, path: &Path) -> &[Export] {
        self.by_file.get(path).map_or(&[], Vec::as_slice)
    }

    /// True when the walk stopped at [`LIMIT`], so a caller can say so rather than imply the
    /// project holds no more.
    pub fn capped(&self) -> bool {
        self.capped
    }

    /// Build the index from a list of files.
    ///
    /// Pure but for reading the files themselves, so it can be tested with a folder and no thread,
    /// and so the measuring instrument can time it. `cancelled` is asked once a file: on the thread
    /// it reads the generation counter, and in a test it is a closure that never says yes.
    pub fn build(
        files: &[PathBuf],
        grammars: &Grammars,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Self> {
        let mut index = Index::default();
        let mut total = 0;
        for path in files {
            if cancelled() {
                return None;
            }
            if !grammars.defines_symbols(path) || !file_kind::is_openable(path) {
                continue;
            }
            let Some(grammar) = grammars.for_path(path) else {
                continue;
            };
            // Whatever is not valid UTF-8 is not something a definition can be read out of, so it
            // is skipped rather than mangled into replacement characters.
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            index.files += 1;
            index.order.push(path.clone());
            for definition in symbols::file_definitions(&text, grammar) {
                if total >= LIMIT {
                    index.capped = true;
                    index.settle();
                    return Some(index);
                }
                total += 1;
                let name = text[definition.name_range.clone()].to_owned();
                if definition.exported {
                    index.by_file.entry(path.clone()).or_default().push(Export {
                        name: name.clone(),
                        kind: definition.kind,
                        confidence: definition.confidence,
                    });
                }
                index.by_name.entry(name).or_default().push(Entry {
                    path: path.clone(),
                    name_range: definition.name_range,
                    kind: definition.kind,
                    confidence: definition.confidence,
                    exported: definition.exported,
                });
            }
        }
        index.settle();
        Some(index)
    }

    /// Write down the sorted name list, once the table is finished.
    ///
    /// Here rather than at each `insert`, because a sorted list kept sorted through a hundred
    /// thousand insertions is a hundred thousand searches; one sort at the end is one sort.
    fn settle(&mut self) {
        self.names = self.by_name.keys().cloned().collect();
        self.names.sort_unstable();
    }
}

/// A thread that builds the index, and the newest question put to it.
pub struct Indexer {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
    /// The number of the newest question. The thread reads it as it walks and abandons a build that
    /// has been overtaken.
    newest: Arc<AtomicU64>,
    generation: u64,
    /// The number of the answer that is in hand, so a reply from an older question is dropped.
    answered: u64,
    index: Index,
}

struct Request {
    generation: u64,
    files: Vec<PathBuf>,
    grammars: Arc<Grammars>,
}

struct Reply {
    generation: u64,
    index: Index,
}

impl Indexer {
    /// Start the thread. `wake` is called when there is a new index to draw with.
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (requests, incoming) = std::sync::mpsc::channel::<Request>();
        let (outgoing, replies) = std::sync::mpsc::channel::<Reply>();
        let newest = Arc::new(AtomicU64::new(0));
        let theirs = Arc::clone(&newest);
        std::thread::Builder::new()
            .name("unluminous-symbols".to_owned())
            .spawn(move || {
                // The loop ends when the sender is dropped, which happens when the window closes.
                for request in incoming {
                    let generation = request.generation;
                    let cancelled = || theirs.load(Ordering::Relaxed) != generation;
                    if let Some(index) = Index::build(&request.files, &request.grammars, &cancelled)
                    {
                        let _ = outgoing.send(Reply { generation, index });
                        wake();
                    }
                }
            })
            .expect("a thread to read the project's definitions on");
        Self {
            requests,
            replies,
            newest,
            generation: 0,
            answered: 0,
            index: Index::default(),
        }
    }

    /// Read the project again, abandoning whatever was being read.
    ///
    /// What a project opening asks for, and what saving a file asks for: a whole rebuild rather than
    /// one file's worth, because the walk is tens of milliseconds and a per-file update is a second
    /// code path that can disagree with the first about what is in the table.
    pub fn rebuild(&mut self, files: Vec<PathBuf>, grammars: Arc<Grammars>) -> u64 {
        self.generation += 1;
        self.newest.store(self.generation, Ordering::Relaxed);
        let _ = self.requests.send(Request { generation: self.generation, files, grammars });
        self.generation
    }

    /// Take in whatever the thread has answered. Called once a frame, before anything is drawn.
    pub fn poll(&mut self) -> bool {
        let mut took = false;
        while let Ok(reply) = self.replies.try_recv() {
            if reply.generation >= self.answered {
                self.answered = reply.generation;
                self.index = reply.index;
                took = true;
            }
        }
        took
    }

    /// What is known now, which is the last completed build.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// True while a build that has not answered yet is running.
    pub fn is_building(&self) -> bool {
        self.answered < self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::Plugins;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        folder
    }

    fn grammars() -> Grammars {
        Plugins::load(None).0.grammars().clone()
    }

    /// A little project: two Rust files that both define `new`, one TypeScript class, and two files
    /// no language claims.
    fn a_project(name: &str) -> (PathBuf, Vec<PathBuf>) {
        let folder = folder(name);
        std::fs::write(
            folder.join("layout.rs"),
            "pub struct Layout;\nimpl Layout {\n    pub fn new() -> Self { Layout }\n    pub fn draw(&self) {}\n}\n",
        )
        .expect("write layout.rs");
        std::fs::write(
            folder.join("caret.rs"),
            "pub struct Caret;\nimpl Caret {\n    pub fn new() -> Self { Caret }\n}\n// draw the caret\n",
        )
        .expect("write caret.rs");
        std::fs::write(
            folder.join("panel.ts"),
            "export class Panel {\n  render(area: Rect) {\n    return area;\n  }\n}\n",
        )
        .expect("write panel.ts");
        std::fs::write(folder.join("notes.md"), "# draw\nA note about new and draw.\n")
            .expect("write notes.md");
        std::fs::write(folder.join("data.json"), "{\"new\": 1}\n").expect("write data.json");
        let files = vec![
            folder.join("layout.rs"),
            folder.join("caret.rs"),
            folder.join("panel.ts"),
            folder.join("notes.md"),
            folder.join("data.json"),
        ];
        (folder, files)
    }

    #[test]
    fn the_index_holds_what_the_project_defines_and_says_where() {
        let (folder, files) = a_project("unluminous-symbol-index-build");
        let index = Index::build(&files, &grammars(), &|| false).expect("a build");
        let new = index.definitions_of("new");
        assert_eq!(new.len(), 2, "both files define one: {new:?}");
        assert!(new.iter().all(|entry| entry.kind == SymbolKind::Function));
        assert_eq!(index.definitions_of("Layout").len(), 1);
        assert_eq!(index.definitions_of("Panel").len(), 1);
        assert_eq!(
            index.definitions_of("render")[0].confidence,
            Confidence::Likely,
            "a class method is found by its shape, and says so"
        );
        assert!(index.definitions_of("nothing-defines-this").is_empty());
        // And the sorted list holds every name once, which is what a stem is scanned against.
        let names = index.sorted_names();
        assert_eq!(names.len(), index.names(), "one entry a name: {names:?}");
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "sorted: {names:?}");
        assert!(names.contains(&"Layout".to_owned()) && names.contains(&"new".to_owned()));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn only_the_files_a_language_claims_are_read_at_all() {
        // Scenario 31's other half: a project full of Markdown and JSON costs the walk and nothing
        // else, because the question is answered from the extension before anything is opened.
        let (folder, files) = a_project("unluminous-symbol-index-claims");
        let index = Index::build(&files, &grammars(), &|| false).expect("a build");
        assert_eq!(index.files(), 3, "the two Rust files and the TypeScript one");
        assert!(
            index.definitions_of("draw").iter().all(|entry| entry.path.ends_with("layout.rs")),
            "the `draw` in the Markdown file and in the comment are not definitions"
        );
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_build_that_has_been_overtaken_stops_where_it_is() {
        // The rule `text_search` already keeps. Nothing half-finished is ever handed over: the
        // build returns nothing at all rather than an index of the files it happened to reach.
        let (folder, files) = a_project("unluminous-symbol-index-cancel");
        assert!(Index::build(&files, &grammars(), &|| true).is_none());
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn reading_the_same_project_twice_gives_an_identical_index() {
        // Scenario 19, at the project level.
        let (folder, files) = a_project("unluminous-symbol-index-determinism");
        let once = Index::build(&files, &grammars(), &|| false).expect("a build");
        let twice = Index::build(&files, &grammars(), &|| false).expect("a build");
        assert_eq!(once, twice);
        assert_eq!(once.file_order(&files[0]), 0);
        assert!(once.file_order(Path::new("never-walked.rs")) > once.files());
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_file_that_is_not_text_or_cannot_be_read_is_skipped_rather_than_fatal() {
        let folder = folder("unluminous-symbol-index-unreadable");
        std::fs::write(folder.join("good.rs"), "fn draw() {}\n").expect("write good.rs");
        std::fs::write(folder.join("bad.rs"), [0xff_u8, 0xfe, 0x00, 0x01]).expect("write bad.rs");
        let files = vec![folder.join("bad.rs"), folder.join("missing.rs"), folder.join("good.rs")];
        let index = Index::build(&files, &grammars(), &|| false).expect("a build");
        assert_eq!(index.definitions_of("draw").len(), 1);
        assert_eq!(index.files(), 1, "only the one that could be read");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_thread_answers_the_newest_question_and_abandons_the_ones_before_it() {
        let (folder, files) = a_project("unluminous-symbol-index-thread");
        let grammars = Arc::new(grammars());
        let mut indexer = Indexer::start(Arc::new(|| {}));
        assert!(!indexer.is_building(), "nothing has been asked yet");
        indexer.rebuild(Vec::new(), Arc::clone(&grammars));
        let generation = indexer.rebuild(files, grammars);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while indexer.is_building() && std::time::Instant::now() < deadline {
            indexer.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!indexer.is_building(), "the build should have finished");
        assert_eq!(indexer.generation, generation);
        assert_eq!(
            indexer.index().definitions_of("new").len(),
            2,
            "and the answer is the newest question's, not the empty one before it"
        );
        std::fs::remove_dir_all(&folder).ok();
    }
}
