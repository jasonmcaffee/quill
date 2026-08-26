//! Searching every file in the project for some text, on a thread.
//!
//! `task-1659` asks for IntelliJ's `Find in Files`: a box that narrows the answer as you type. What
//! that means in a project of any size is reading every file on every key press, and a window that
//! did that where it draws would stop drawing while it read — which on a large folder looks exactly
//! like a crash. So the reading happens on a thread, the same arrangement `quill_git::Worker` and
//! the terminal already use, with a waker that asks the window to draw again when an answer is
//! ready.
//!
//! Four decisions worth writing down.
//!
//! **Only the newest question is answered.** A person typing `search` asks six questions in a
//! second, and five of them are already wrong by the time they are asked. Each request carries a
//! number, the newest number is shared with the thread, and the search checks it as it goes: a
//! search whose number has been passed stops where it is and its part-finished answer is thrown
//! away. That is what keeps typing quick without a timer to debounce it.
//!
//! **The answer arrives in pieces.** A file that matches is sent as soon as it is found rather than
//! at the end, so results fill in from the top while the rest of the project is still being read.
//! [`Reply::done`] is what says there is no more coming.
//!
//! **A file that is not text is not read at all**, and neither is one larger than
//! [`crate::services::file_kind`] will open. Both questions are answered from the extension where
//! they can be, so a project full of pictures costs no reading.
//!
//! **The counts are capped.** Past a few hundred results nobody is reading the list, and the honest
//! thing is to say so rather than to spend a second collecting an answer that will not be looked at.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use quill_core::symbols::{self, Role};

use crate::services::file_kind;
use crate::services::plugins::Grammars;

/// The most matches that are collected before the search gives up and says how many it found.
pub const LIMIT: usize = 500;
/// The most matches collected from any one file, so one generated file cannot fill the list.
const PER_FILE: usize = 50;
/// How much of a matching line is kept. A minified file is one line a megabyte long, and the row
/// shows a line of it.
const LINE_LIMIT: usize = 400;

/// What is being looked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub needle: String,
    /// False when `readme` should find `README`, which is what a search box does unless told
    /// otherwise.
    pub match_case: bool,
    /// True when only whole words count and each match is labelled with the role of the token it
    /// fell inside.
    ///
    /// This is the whole of what `Find References` adds to `Find in Files`, which is why it is a
    /// mode of this searcher rather than a second one: the thread, the generation counter, the
    /// batching, the caps and the line shortening are all the same machinery, and a second copy of
    /// them would be a second place for a cancellation to be got wrong. `task-1675` §3.4 records why
    /// the answer is a search at all rather than a stored index of every occurrence — the search
    /// reads what is on the disk now, and an index would have to be told when a build, a branch
    /// switch or another editor moved it.
    pub words: bool,
}

impl Query {
    /// True when there is nothing to search for, which is what an empty results list means rather
    /// than "nothing matched".
    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }
}

/// One match: which file, which line, and where in the line and in the file it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// The line the match is on, counting from one, which is what a person means by a line number.
    pub line: usize,
    /// The text of that line, without its line break and cut short if it is very long.
    pub text: String,
    /// Where in `text` the match sits, in bytes, so the row can pick it out.
    pub range: Range<usize>,
    /// Where in the whole file the match sits, in bytes, which is what the editor selects when the
    /// result is opened.
    pub offset: Range<usize>,
    /// What the match was found inside. Always [`Role::Code`] for a plain text search, which has no
    /// grammar behind it and makes no claim about the file's own reading of itself.
    pub role: Role,
}

/// Everything one search found, or as much of it as has arrived.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reply {
    /// Which question this answers.
    pub generation: u64,
    pub hits: Vec<Hit>,
    /// How many files were read.
    pub files: usize,
    /// True when the whole project has been read and there is no more coming.
    pub done: bool,
    /// True when the search stopped at [`LIMIT`] rather than at the end of the project.
    pub capped: bool,
}

/// A thread that searches, and the newest question put to it.
pub struct Searcher {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
    /// The number of the newest question. The thread reads it as it goes and abandons a search that
    /// has been overtaken, which is what makes typing quick.
    newest: Arc<AtomicU64>,
    generation: u64,
}

struct Request {
    generation: u64,
    files: Vec<PathBuf>,
    query: Query,
    /// How each file's language reads itself, for the whole-word mode. Empty otherwise.
    grammars: Arc<Grammars>,
    /// The text of the files that are open, which is searched instead of what is on the disk.
    ///
    /// The ownership rule this whole feature follows: **a file that is open is owned by its
    /// `Document`**. A reference in a file with unsaved edits is the edits' truth, not the disk's,
    /// and the alternative — searching the disk and then quietly being wrong about one file — is
    /// the sort of answer that sends somebody to the wrong line.
    open: Arc<Vec<(PathBuf, String)>>,
}

impl Searcher {
    /// Start the thread. `wake` is called when there is something new to draw.
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (requests, incoming) = std::sync::mpsc::channel::<Request>();
        let (outgoing, replies) = std::sync::mpsc::channel::<Reply>();
        let newest = Arc::new(AtomicU64::new(0));
        let theirs = Arc::clone(&newest);
        std::thread::Builder::new()
            .name("quill-search".to_owned())
            .spawn(move || {
                // The loop ends when the sender is dropped, which happens when the modal is shut.
                for request in incoming {
                    run(request, &theirs, &outgoing, &wake);
                }
            })
            .expect("a thread to search on");
        Self { requests, replies, newest, generation: 0 }
    }

    /// Ask a new question, which abandons whatever was being answered.
    pub fn send(&mut self, files: Vec<PathBuf>, query: Query) -> u64 {
        self.ask(files, query, Arc::new(Grammars::default()), Arc::new(Vec::new()))
    }

    /// The same, told how each language reads itself and what the open files really hold.
    ///
    /// What `Find References` asks. Both are `Arc`s because the same two are sent again on every
    /// question and neither belongs to the thread.
    pub fn ask(
        &mut self,
        files: Vec<PathBuf>,
        query: Query,
        grammars: Arc<Grammars>,
        open: Arc<Vec<(PathBuf, String)>>,
    ) -> u64 {
        self.generation += 1;
        self.newest.store(self.generation, Ordering::Relaxed);
        let _ = self.requests.send(Request {
            generation: self.generation,
            files,
            query,
            grammars,
            open,
        });
        self.generation
    }

    /// The number of the question being answered now.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Everything the thread has answered since the last time this was called, with anything
    /// belonging to an older question dropped.
    pub fn poll(&mut self) -> Vec<Reply> {
        let mut replies = Vec::new();
        while let Ok(reply) = self.replies.try_recv() {
            if reply.generation == self.generation {
                replies.push(reply);
            }
        }
        replies
    }
}

/// One search, on the thread. Nothing here may touch the window.
fn run(
    request: Request,
    newest: &AtomicU64,
    outgoing: &Sender<Reply>,
    wake: &Arc<dyn Fn() + Send + Sync>,
) {
    let Request { generation, files, query, grammars, open } = request;
    if query.is_empty() {
        let _ = outgoing.send(Reply { generation, done: true, ..Reply::default() });
        wake();
        return;
    }
    let mut collected = 0;
    let mut read = 0;
    let mut batch: Vec<Hit> = Vec::new();
    let mut capped = false;
    for path in &files {
        // Overtaken by a newer question: stop where we are and say nothing. What has been collected
        // answers a question nobody is asking any more.
        if newest.load(Ordering::Relaxed) != generation {
            return;
        }
        // A file that is open is searched as it stands in its tab rather than as it stands on the
        // disk, which is the ownership rule this whole feature follows.
        let opened = open.iter().find(|(known, _)| known == path).map(|(_, text)| text.clone());
        let text = match opened {
            Some(text) => text,
            None => match read_text(path) {
                Some(text) => text,
                None => continue,
            },
        };
        // The whole-word mode reads the file's own language, and a file no plugin claims cannot
        // hold a reference in code — every match in it would be an unclassifiable textual one.
        let grammar = grammars.for_path(path);
        if query.words && grammar.is_none() {
            continue;
        }
        read += 1;
        let limit = PER_FILE.min(LIMIT - collected);
        let hits = match grammar.filter(|_| query.words) {
            Some(grammar) => references_in(path, &text, &query.needle, grammar, limit),
            None => hits_in(path, &text, &query, limit),
        };
        collected += hits.len();
        batch.extend(hits);
        // Sent in batches rather than one file at a time, so a project of ten thousand files does
        // not send ten thousand messages and wake the window ten thousand times.
        if batch.len() >= 25 {
            let _ = outgoing.send(Reply {
                generation,
                hits: std::mem::take(&mut batch),
                files: read,
                done: false,
                capped: false,
            });
            wake();
        }
        if collected >= LIMIT {
            capped = true;
            break;
        }
    }
    let _ = outgoing.send(Reply { generation, hits: batch, files: read, done: true, capped });
    wake();
}

/// Read a file if it is one worth searching: text, and not larger than Quill will open.
fn read_text(path: &Path) -> Option<String> {
    if !file_kind::is_openable(path) || file_kind::is_image(path) {
        return None;
    }
    // Whatever is not valid UTF-8 is not something a search box can show, so it is skipped rather
    // than mangled into replacement characters that would then match nothing anybody typed.
    std::fs::read(path).ok().and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Every match in one file's text, up to `limit` of them.
///
/// Pure, so the matching can be tested without a thread or a disk behind it.
pub fn hits_in(path: &Path, text: &str, query: &Query, limit: usize) -> Vec<Hit> {
    let mut hits = Vec::new();
    if query.is_empty() || limit == 0 {
        return hits;
    }
    let needle = if query.match_case { query.needle.clone() } else { query.needle.to_lowercase() };
    let mut start_of_line = 0;
    for (index, line) in text.split('\n').enumerate() {
        let haystack = if query.match_case { line.to_owned() } else { line.to_lowercase() };
        // The lower case of a string can be a different length from the string — the Turkish dotted
        // capital is three bytes lower cased and two upper — so a position in the lower cased line
        // is only a position in the line itself when the two are the same length. When they differ,
        // the line is matched with its own case instead, which finds less rather than lying about
        // where the match is.
        let usable = haystack.len() == line.len();
        let searched = if usable { haystack.as_str() } else { line };
        let mut at = 0;
        while let Some(found) = searched[at..].find(&needle) {
            let begin = at + found;
            let end = begin + needle.len();
            let (text, range) = shorten(line, begin..end);
            hits.push(Hit {
                path: path.to_path_buf(),
                line: index + 1,
                text,
                range,
                offset: (start_of_line + begin)..(start_of_line + end),
                role: Role::Code,
            });
            if hits.len() >= limit {
                return hits;
            }
            at = end.max(begin + 1);
            if at >= searched.len() {
                break;
            }
        }
        // The line break itself is one byte, because the buffer holds line feeds only.
        start_of_line += line.len() + 1;
    }
    hits
}

/// Every whole-word occurrence of `name` in one file's text, up to `limit` of them.
///
/// The reference half of the searcher. `quill_core::symbols::occurrences` decides what a whole word
/// is and what each one was found inside — the same function `quill_core` tests with no window —
/// and everything here is the turning of a byte range into the row a person reads: which line it is
/// on, that line shortened if it is a minified megabyte, and where in what is left the match sits.
///
/// A plain substring test comes first, because most files do not hold the word at all and
/// tokenising one that does not would be the cost of the whole search.
pub fn references_in(
    path: &Path,
    text: &str,
    name: &str,
    grammar: &quill_core::Grammar,
    limit: usize,
) -> Vec<Hit> {
    if name.is_empty() || limit == 0 || !text.contains(name) {
        return Vec::new();
    }
    // Where each line starts, so a byte offset becomes a line number by binary search rather than
    // by counting from the top of the file once a match.
    let mut starts: Vec<usize> = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    let mut hits = Vec::new();
    for occurrence in symbols::occurrences(text, name, grammar) {
        let index = match starts.binary_search(&occurrence.range.start) {
            Ok(exact) => exact,
            Err(after) => after - 1,
        };
        let start_of_line = starts[index];
        let line = text[start_of_line..]
            .split('\n')
            .next()
            .unwrap_or_default()
            .trim_end_matches('\r');
        let begin = occurrence.range.start - start_of_line;
        let end = begin + name.len();
        // A match beyond the end of its own line means the name held a line break, which no
        // identifier does; it is dropped rather than trusted.
        if end > line.len() {
            continue;
        }
        let (shortened, range) = shorten(line, begin..end);
        hits.push(Hit {
            path: path.to_path_buf(),
            line: index + 1,
            text: shortened,
            range,
            offset: occurrence.range,
            role: occurrence.role,
        });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

/// Cut a very long line down to something a row can show, keeping the match inside it.
///
/// Returns the shortened line and where the match sits in it. A minified file is one line a
/// megabyte long, and laying a megabyte of text out to draw twenty points of it is the sort of thing
/// that makes a search box feel broken.
fn shorten(line: &str, range: Range<usize>) -> (String, Range<usize>) {
    if line.len() <= LINE_LIMIT {
        return (line.to_owned(), range);
    }
    // Keep a little in front of the match, so it is not left against the left edge with no context.
    let before = 60.min(range.start);
    let from = floor_char(line, range.start - before);
    let to = floor_char(line, (from + LINE_LIMIT).min(line.len()));
    let kept = &line[from..to];
    let start = range.start - from;
    let end = (range.end - from).min(kept.len());
    (format!("{kept}\u{2026}"), start.min(kept.len())..end)
}

/// The nearest byte position at or below `at` that a character starts on.
fn floor_char(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(needle: &str) -> Query {
        Query { needle: needle.to_owned(), match_case: false, words: false }
    }

    #[test]
    fn a_match_says_which_line_it_is_on_counting_from_one() {
        let text = "first\nsecond\nthird\n";
        let hits = hits_in(Path::new("a.txt"), text, &query("second"), 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].text, "second");
    }

    #[test]
    fn a_match_says_where_it_sits_in_the_whole_file_so_the_editor_can_select_it() {
        let text = "first\nsecond\nthird\n";
        let hits = hits_in(Path::new("a.txt"), text, &query("third"), 10);
        assert_eq!(&text[hits[0].offset.clone()], "third");
    }

    #[test]
    fn every_match_on_one_line_is_its_own_result() {
        let hits = hits_in(Path::new("a.txt"), "one two one two one", &query("one"), 10);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].range, 0..3);
        assert_eq!(hits[1].range, 8..11);
    }

    #[test]
    fn case_is_ignored_unless_it_is_asked_about() {
        let text = "The README file";
        assert_eq!(hits_in(Path::new("a.txt"), text, &query("readme"), 10).len(), 1);
        let exact = Query { needle: "readme".to_owned(), match_case: true, words: false };
        assert!(hits_in(Path::new("a.txt"), text, &exact, 10).is_empty());
    }

    #[test]
    fn the_match_is_found_at_the_right_place_whatever_the_case_was() {
        let hits = hits_in(Path::new("a.txt"), "The README file", &query("readme"), 10);
        assert_eq!(hits[0].range, 4..10, "the position is in the line as it is written");
        assert_eq!(&"The README file"[hits[0].range.clone()], "README");
    }

    #[test]
    fn nothing_is_searched_for_when_the_box_is_empty() {
        assert!(hits_in(Path::new("a.txt"), "anything at all", &query(""), 10).is_empty());
    }

    #[test]
    fn one_file_cannot_fill_the_whole_list() {
        let text = "match\n".repeat(100);
        assert_eq!(hits_in(Path::new("a.txt"), &text, &query("match"), 5).len(), 5);
    }

    #[test]
    fn a_very_long_line_is_cut_down_with_the_match_still_inside_it() {
        let mut line = "x".repeat(5000);
        line.push_str("needle");
        let hits = hits_in(Path::new("a.txt"), &line, &query("needle"), 10);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.len() <= LINE_LIMIT + 4, "kept short: {}", hits[0].text.len());
        assert_eq!(&hits[0].text[hits[0].range.clone()], "needle", "and the match is still in it");
        assert_eq!(&line[hits[0].offset.clone()], "needle", "the file position is untouched");
    }

    #[test]
    fn a_line_holding_letters_wider_than_one_byte_is_still_cut_on_a_character() {
        let mut line = "\u{00E9}".repeat(3000);
        line.push_str("needle");
        let hits = hits_in(Path::new("a.txt"), &line, &query("needle"), 10);
        assert_eq!(&hits[0].text[hits[0].range.clone()], "needle");
    }

    /// The Rust grammar as the bundled plugin describes it, which is what the reference mode reads.
    fn rust() -> quill_core::Grammar {
        crate::services::plugins::Plugins::load(None)
            .0
            .grammars()
            .for_path(Path::new("a.rs"))
            .expect("the rust plugin claims .rs")
            .clone()
    }

    fn references(text: &str, name: &str) -> Vec<Hit> {
        references_in(Path::new("a.rs"), text, name, &rust(), 100)
    }

    #[test]
    fn a_reference_is_a_whole_word_and_says_which_line_it_is_on() {
        // Scenario 23 through the app's own half of it: the same rule `quill_core` tests, turned
        // into the rows a person reads.
        let text = "let count = 1;\nlet counter = count + 1;\n";
        let found = references(text, "count");
        assert_eq!(found.len(), 2, "`counter` is not a `count`: {found:?}");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[1].line, 2);
        assert_eq!(&text[found[1].offset.clone()], "count");
        assert_eq!(&found[1].text[found[1].range.clone()], "count", "and the row picks it out");
    }

    #[test]
    fn a_reference_carries_the_role_of_what_it_was_found_inside() {
        // Scenario 24. Shown, because a rename that must update a doc comment needs to find it;
        // told apart, because they are textual matches and the modal does not pretend otherwise.
        let text = "// draw the thing\nfn draw() {}\nlet s = \"draw\";\n";
        let roles: Vec<Role> = references(text, "draw").iter().map(|hit| hit.role).collect();
        assert_eq!(roles, vec![Role::Comment, Role::Code, Role::String]);
        // A plain text search makes no claim about any of that, because it has no grammar behind it.
        let plain = hits_in(Path::new("a.rs"), text, &query("draw"), 100);
        assert!(plain.iter().all(|hit| hit.role == Role::Code));
    }

    #[test]
    fn a_file_that_does_not_hold_the_word_at_all_is_not_read_into_tokens() {
        // The plain substring test that comes first: most files do not hold the word, and
        // tokenising one that does not would be the cost of the whole search.
        assert!(references("nothing of the sort here\n", "draw").is_empty());
        assert!(references("", "draw").is_empty());
        assert!(references("draw", "").is_empty());
    }

    #[test]
    fn one_file_cannot_fill_the_reference_list_either() {
        // Scenario 25 at this end: the cap is the same one `Find in Files` already has.
        let text = "draw();\n".repeat(100);
        assert_eq!(references_in(Path::new("a.rs"), &text, "draw", &rust(), 5).len(), 5);
    }

    #[test]
    fn a_minified_line_is_shortened_with_the_reference_still_inside_it() {
        // Scenario 29: one line a megabyte long, and the row shows a line of it.
        let mut line = "let x = 1; ".repeat(600);
        line.push_str("draw();\n");
        let found = references(&line, "draw");
        assert_eq!(found.len(), 1);
        assert!(found[0].text.len() <= LINE_LIMIT + 4, "kept short: {}", found[0].text.len());
        assert_eq!(&found[0].text[found[0].range.clone()], "draw", "and the match is still in it");
        assert_eq!(&line[found[0].offset.clone()], "draw", "the file position is untouched");
    }

    #[test]
    fn a_reference_on_a_line_with_a_carriage_return_lands_on_the_right_bytes() {
        // A file written on Windows: the line the row shows has no carriage return in it, and the
        // offset the editor selects is still the one in the file.
        let text = "let value = 1;\r\nlet other = value;\r\n";
        let found = references(text, "value");
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].line, 2);
        assert!(!found[1].text.contains('\r'));
        assert_eq!(&text[found[1].offset.clone()], "value");
    }

    #[test]
    fn the_thread_answers_the_newest_question_and_abandons_the_ones_before_it() {
        let folder = std::env::temp_dir().join("quill-text-search");
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(folder.join("a.txt"), "alpha\nbeta\n").expect("write a.txt");
        std::fs::write(folder.join("b.txt"), "beta\ngamma\n").expect("write b.txt");
        let files = vec![folder.join("a.txt"), folder.join("b.txt")];

        let mut searcher = Searcher::start(Arc::new(|| {}));
        searcher.send(files.clone(), query("alpha"));
        let generation = searcher.send(files, query("beta"));

        let mut hits = Vec::new();
        let mut done = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !done && std::time::Instant::now() < deadline {
            for reply in searcher.poll() {
                assert_eq!(reply.generation, generation, "an old answer must not be handed over");
                hits.extend(reply.hits);
                done |= reply.done;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(done, "the search should have finished");
        assert_eq!(hits.len(), 2, "beta is in both files: {hits:?}");
    }
}
