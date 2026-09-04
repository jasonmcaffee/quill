//! Colouring the part of a file that changed, rather than the whole of it after every keystroke.
//!
//! `task-1666` §12 named this in advance — *"the tokeniser reads the whole file after every edit…
//! it is the next thing to become the largest item"* — and `task-1804` §5.2 measured it becoming
//! that: typing one letter into a 2 MB file cost **73.6 ms** on this machine, four times what the
//! same keystroke costs in a 500 KB file, because the cost is linear in the size of the file when it
//! should be linear in the size of the edit.
//!
//! It gave the fix as well, and this is that fix carried out: **tokenise from the start of the line
//! the edit was on and stop once the tokens agree with what was there before** — which is the rule
//! `layout::relayout` already follows for the layout, made once more about the tokeniser.
//!
//! ## Why starting part way through a file is allowed at all
//!
//! Because [`crate::syntax::scan_with_embedded`]'s loop is **position-independent**: at each byte it
//! looks only at `&text[at..]`, so scanning from a byte `n` gives exactly the tokens it would have
//! given from zero — *provided the scanner was between tokens at `n`*. Inside a block comment or a
//! multi-line string it was not, and the reading from there would be nonsense.
//!
//! The previous scan is what answers that, and it answers it exactly: if no token from the last
//! reading **straddles** `n`, the scanner was between tokens there. So [`Tokens::update`] walks back
//! line by line until it finds such a byte, and the search is bounded — the whole file is always a
//! valid answer.
//!
//! ## And why it stops
//!
//! Past the edit, the file is the file it was, shifted. So the new reading is compared against the
//! old one shifted by the edit's own length, and the moment a token matches — same start, same end,
//! same kind — everything after it is copied rather than read. On an edit near the top of a file
//! that is one line's worth of scanning; on an edit that opens a block comment it is the rest of the
//! file, correctly, because the rest of the file really did change colour.
//!
//! The comparison is only allowed to start **after every byte the edits touched**, which is what
//! `Dirt::to` is for: two tokens either side of an edit can be identical by coincidence, and
//! splicing there would keep old offsets for text that has moved.
//!
//! ## What is deliberately not incremental
//!
//! **A markup grammar.** `markup::walk` is a different reading with state of its own, and the
//! straddle test above says nothing about it. HTML files therefore take the whole-file path, which
//! is exactly what they did before. It is written down rather than assumed because the fast path is
//! the kind of thing that gets "extended to markup while we're here", and it would be wrong.

use std::ops::Range;

use crate::syntax::{self, Grammar, Token};

/// What has changed since a document's syntax was last read.
///
/// Held by [`crate::Document`], which is the one place that knows the text moved — `insert` and
/// `remove_range` are the two functions that already say so for the marks, the folds and the
/// breakpoints, and this is the fourth thing they tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dirt {
    /// Nothing has changed since the syntax was last set.
    #[default]
    Clean,
    /// Everything may have changed: the file was opened, its text was replaced, or an undo restored
    /// a snapshot. There is no edit to be incremental about.
    Whole,
    /// One or more edits, whose union is described here.
    Part {
        /// The first byte an edit touched, in the text as it is **now**.
        from: usize,
        /// One past the last byte an edit touched, in the text as it is now. Nothing before this may
        /// be used as a synchronisation point.
        to: usize,
        /// How many bytes longer the text is than it was when the syntax was last read.
        delta: isize,
    },
}

impl Dirt {
    /// Fold one edit into what is already known.
    ///
    /// `at` is where it happened, `removed` how many bytes went and `added` how many arrived, all in
    /// the coordinates of the text at the moment of the edit.
    pub fn note(self, at: usize, removed: usize, added: usize) -> Self {
        let delta = added as isize - removed as isize;
        match self {
            Dirt::Whole => Dirt::Whole,
            Dirt::Clean => Dirt::Part { from: at, to: at + added, delta },
            Dirt::Part { from, to, delta: had } => {
                // Everything after this edit moved, so a watermark that was after it moves too.
                let moved = match at < to {
                    true => (to as isize + delta).max(at as isize) as usize,
                    false => to,
                };
                Dirt::Part {
                    from: from.min(at),
                    to: moved.max(at + added),
                    delta: had + delta,
                }
            }
        }
    }
}

/// The tokens of one file, kept so the next reading can start from the edit.
#[derive(Debug, Clone, Default)]
pub struct Tokens {
    /// Every token of the file as it was last read, in order, including the plain words — because a
    /// plain word is as good a synchronisation point as any other and leaving them out would mean
    /// the straddle test had holes in it.
    tokens: Vec<(Range<usize>, Token)>,
    /// True once a reading has been done, so an empty file is told from one nothing has read.
    read: bool,
}

/// What one reading came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// The stretch of the file whose colouring may have changed. Everything outside it is the colour
    /// it already was, shifted by the edit — which `Document::insert` and `Document::remove_range`
    /// have already done to the style spans.
    pub changed: Range<usize>,
    /// How many tokens were read this time rather than copied. Nothing depends on it; it is what
    /// makes "it really was incremental" a thing a test can assert.
    pub scanned: usize,
}

impl Tokens {
    /// Read `text` again, doing as little of it as the dirt allows.
    ///
    /// **`report` is called for the tokens inside [`Update::changed`], in file order, and for no
    /// others.** That is the second half of the saving and it is worth being exact about: an earlier
    /// draft reported every token in the file so that a caller could build a whole span list without
    /// knowing which half was which, and on a 2 MB file that was a 157,000 element vector built and
    /// sorted after every keystroke — 17 ms of work to describe a change of one letter. A caller
    /// that wants the whole list has [`Tokens::all`], which is the list this already keeps.
    ///
    /// In file order, so nothing has to be sorted afterwards either.
    pub fn update(
        &mut self,
        text: &str,
        grammar: &Grammar,
        dirt: Dirt,
        mut report: impl FnMut(Range<usize>, Token),
    ) -> Update {
        let whole = 0..text.len();
        let part = match (self.read, grammar.markup, dirt) {
            (true, false, Dirt::Part { from, to, delta }) => Some((from, to, delta)),
            _ => None,
        };
        let Some((edited_from, edited_to, delta)) = part else {
            let mut fresh = Vec::with_capacity(self.tokens.len().max(64));
            syntax::scan(text, grammar, |range, token| fresh.push((range, token)));
            for (range, token) in &fresh {
                report(range.clone(), *token);
            }
            let scanned = fresh.len();
            self.tokens = fresh;
            self.read = true;
            return Update { changed: whole, scanned };
        };

        // Where it is safe to start: the beginning of a line, walked back while a token of the last
        // reading straddles it. In the old text's coordinates first, because that is what the old
        // tokens are in.
        let start = self.safe_start(text, edited_from, delta);
        let mut fresh: Vec<(Range<usize>, Token)> = Vec::with_capacity(self.tokens.len());
        // Everything before the start is what it was: the same bytes, unmoved.
        for entry in self.tokens.iter().take_while(|(range, _)| range.end <= start) {
            fresh.push(entry.clone());
        }
        let carried = fresh.len();

        // The old tokens that could still be ahead of us, shifted into the new text's coordinates.
        // Only those beginning at or after the watermark can be synchronised on.
        let tail_from = self
            .tokens
            .partition_point(|(range, _)| shift(range.start, delta) < edited_to);

        let mut scanned = 0usize;
        let mut spliced_at: Option<usize> = None;
        syntax::scan_from(text, grammar, start, |range, token| {
            scanned += 1;
            // Past everything the edits touched, look for the old reading again.
            if range.start >= edited_to {
                let looking_for = self.tokens[tail_from..]
                    .binary_search_by_key(&range.start, |(old, _)| shift(old.start, delta));
                if let Ok(found) = looking_for {
                    let (old, old_token) = &self.tokens[tail_from + found];
                    if shift(old.end, delta) == range.end && *old_token == token {
                        spliced_at = Some(tail_from + found);
                        return std::ops::ControlFlow::Break(());
                    }
                }
            }
            fresh.push((range, token));
            std::ops::ControlFlow::Continue(())
        });

        let read_to = fresh.len();
        let changed_to = match spliced_at {
            // Everything from the synchronisation point on is the old reading, moved.
            Some(at) => {
                let resume = fresh.last().map(|(range, _)| range.end).unwrap_or(start);
                for (range, token) in &self.tokens[at..] {
                    fresh.push((shift(range.start, delta)..shift(range.end, delta), *token));
                }
                resume.max(edited_to)
            }
            // Nothing matched, so the reading really did change all the way to the end.
            None => text.len(),
        };

        // The tokens inside the changed stretch, in file order. `carried..read_to` is exactly that
        // range of the list: everything before it is the untouched prefix, and everything after it
        // is the spliced tail, which by definition did not change.
        for (range, token) in &fresh[carried..read_to] {
            report(range.clone(), *token);
        }
        self.tokens = fresh;
        self.read = true;
        Update { changed: start..changed_to.min(text.len()), scanned }
    }

    /// Every token of the file as it was last read, in file order.
    ///
    /// What a caller that needs the whole list reads, rather than being handed it again through
    /// `report` on every keystroke. `UnluminousApp::colour_the_file` uses it for the blocks that
    /// could be collapsed, which is a question about the whole file however small the edit was.
    pub fn all(&self) -> &[(Range<usize>, Token)] {
        &self.tokens
    }

    /// Throw away what was read, so the next update reads the whole file.
    pub fn forget(&mut self) {
        self.tokens.clear();
        self.read = false;
    }

    /// The nearest line start at or before the edit at which the scanner was **between tokens**.
    ///
    /// `edited_from` is in the new text; the old tokens are in the old text, so the comparison is
    /// made by shifting them. Bytes before the edit did not move, so for those the shift is zero and
    /// the two coordinate systems agree — which is why only the tokens *ending before* the edit are
    /// consulted here.
    fn safe_start(&self, text: &str, edited_from: usize, delta: isize) -> usize {
        let mut at = line_start(text, edited_from.min(text.len()));
        loop {
            let straddled = self
                .tokens
                .iter()
                .take_while(|(range, _)| range.start < at)
                .any(|(range, _)| range.start < at && shift(range.end, delta).max(range.end) > at);
            if !straddled || at == 0 {
                return at;
            }
            at = line_start(text, at.saturating_sub(1));
        }
    }
}

/// A byte position moved by `delta`, never below zero.
fn shift(at: usize, delta: isize) -> usize {
    (at as isize + delta).max(0) as usize
}

/// The start of the line `at` is on.
fn line_start(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    text[..at].rfind('\n').map(|found| found + 1).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grammar with everything the incremental path has to survive: line comments, block comments
    /// and strings, so a test can open one and watch the rest of the file change colour.
    fn rust_like() -> Grammar {
        let mut grammar = Grammar::default();
        grammar.line_comment = Some("//".to_owned());
        grammar.block_comment = Some(("/*".to_owned(), "*/".to_owned()));
        grammar.strings = vec!['"'];
        grammar.numbers = true;
        grammar.keywords = ["fn", "let", "pub", "struct"].iter().map(|k| (*k).to_owned()).collect();
        grammar
    }

    /// Every token, read the way `syntax::scan` reads them, which is the answer to be matched.
    fn whole(text: &str, grammar: &Grammar) -> Vec<(Range<usize>, Token)> {
        let mut out = Vec::new();
        syntax::scan(text, grammar, |range, token| out.push((range, token)));
        out
    }

    /// The same, through the incremental reader. The answer is [`Tokens::all`], which is the whole
    /// file; what `report` said is checked separately by
    /// [`only_the_tokens_inside_the_changed_stretch_are_reported`].
    fn incrementally(
        cache: &mut Tokens,
        text: &str,
        grammar: &Grammar,
        dirt: Dirt,
    ) -> (Vec<(Range<usize>, Token)>, Update) {
        let update = cache.update(text, grammar, dirt, |_, _| {});
        (cache.all().to_vec(), update)
    }

    /// What `report` says, for the test that is about `report`.
    fn reported(
        cache: &mut Tokens,
        text: &str,
        grammar: &Grammar,
        dirt: Dirt,
    ) -> (Vec<(Range<usize>, Token)>, Update) {
        let mut out = Vec::new();
        let update = cache.update(text, grammar, dirt, |range, token| out.push((range, token)));
        (out, update)
    }

    #[test]
    fn the_first_reading_is_the_whole_file_and_matches_the_ordinary_scan() {
        let grammar = rust_like();
        let text = "pub fn one() {}\n// a comment\nlet x = 1;\n";
        let mut cache = Tokens::default();
        let (read, update) = incrementally(&mut cache, text, &grammar, Dirt::Clean);
        assert_eq!(read, whole(text, &grammar));
        assert_eq!(update.changed, 0..text.len());
    }

    /// The point of the whole file: an edit on one line reads that line, not the file.
    #[test]
    fn an_edit_on_one_line_reads_that_line_rather_than_the_file() {
        let grammar = rust_like();
        let mut before = String::new();
        for index in 0..2000 {
            before.push_str(&format!("pub fn name{index}() {{ let x = {index}; }}\n"));
        }
        let mut cache = Tokens::default();
        let (_, first) = incrementally(&mut cache, &before, &grammar, Dirt::Clean);
        let full = first.scanned;
        assert!(full > 10_000, "the file really is large: {full} tokens");

        // One character typed in the middle.
        let at = before.find("name1000").expect("it is there");
        let mut after = before.clone();
        after.insert(at, 'Z');
        let (read, update) =
            incrementally(&mut cache, &after, &grammar, Dirt::Clean.note(at, 0, 1));
        assert_eq!(read, whole(&after, &grammar), "the answer is the same answer");
        assert!(
            update.scanned < full / 100,
            "it read a line, not the file: {} of {full}",
            update.scanned
        );
        assert!(update.changed.start <= at && update.changed.end >= at + 1);
        assert!(
            update.changed.len() < 200,
            "and it says only that line changed colour: {:?}",
            update.changed
        );
    }

    /// And when an edit really does change the rest of the file, it says so.
    #[test]
    fn opening_a_block_comment_changes_the_colour_of_everything_after_it() {
        let grammar = rust_like();
        let before = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\n";
        let mut cache = Tokens::default();
        incrementally(&mut cache, before, &grammar, Dirt::Clean);

        let after = "let a = 1;\n/*\nlet c = 3;\nlet d = 4;\n";
        // `let b = 2;` became `/*`, which is eight bytes shorter.
        let at = before.find("let b").expect("it is there");
        let (read, update) =
            incrementally(&mut cache, after, &grammar, Dirt::Clean.note(at, 10, 2));
        assert_eq!(read, whole(after, &grammar));
        assert_eq!(update.changed.end, after.len(), "the rest of the file is inside the comment now");
    }

    /// An edit **inside** a block comment starts from before the comment, because the line the edit
    /// is on begins in the middle of a token.
    #[test]
    fn an_edit_inside_a_block_comment_starts_from_before_the_comment() {
        let grammar = rust_like();
        let before = "let a = 1;\n/* one\ntwo\nthree */\nlet b = 2;\n";
        let mut cache = Tokens::default();
        incrementally(&mut cache, before, &grammar, Dirt::Clean);
        let at = before.find("two").expect("it is there");
        let after = before.replacen("two", "twoX", 1);
        let (read, update) =
            incrementally(&mut cache, &after, &grammar, Dirt::Clean.note(at + 3, 0, 1));
        assert_eq!(read, whole(&after, &grammar), "the comment is still one token");
        assert!(
            update.changed.start <= before.find("/*").expect("it is there"),
            "it walked back past the line the edit was on: {:?}",
            update.changed
        );
    }

    /// Deleting is the same shape, and the answer still matches.
    #[test]
    fn a_deletion_is_read_the_same_way() {
        let grammar = rust_like();
        let before = "let alpha = 1;\nlet beta = 2;\nlet gamma = 3;\n";
        let mut cache = Tokens::default();
        incrementally(&mut cache, before, &grammar, Dirt::Clean);
        let at = before.find("beta").expect("it is there");
        let after = before.replacen("beta", "b", 1);
        let (read, _) = incrementally(&mut cache, &after, &grammar, Dirt::Clean.note(at, 4, 1));
        assert_eq!(read, whole(&after, &grammar));
    }

    /// Two edits between one colouring and the next: the answer still matches, which is what the
    /// widening in [`Dirt::note`] is for.
    #[test]
    fn several_edits_since_the_last_reading_are_all_covered() {
        let grammar = rust_like();
        let before = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\n";
        let mut cache = Tokens::default();
        incrementally(&mut cache, before, &grammar, Dirt::Clean);

        // `a` -> `aa` on the first line, and `d` -> `dd` on the last.
        let after = "let aa = 1;\nlet b = 2;\nlet c = 3;\nlet dd = 4;\n";
        let first = before.find("a =").expect("it is there");
        let second = after.find("d =").expect("it is there");
        let dirt = Dirt::Clean.note(first, 0, 1).note(second, 0, 1);
        let (read, _) = incrementally(&mut cache, after, &grammar, dirt);
        assert_eq!(read, whole(after, &grammar));
    }

    /// `Whole` is what an undo and a fresh file give, and it reads everything.
    #[test]
    fn whole_dirt_reads_the_file_again() {
        let grammar = rust_like();
        let text = "let a = 1;\nlet b = 2;\n";
        let mut cache = Tokens::default();
        incrementally(&mut cache, text, &grammar, Dirt::Clean);
        let other = "// completely different\n";
        let (read, update) = incrementally(&mut cache, other, &grammar, Dirt::Whole);
        assert_eq!(read, whole(other, &grammar));
        assert_eq!(update.changed, 0..other.len());
    }

    /// A markup grammar never takes the fast path, because `markup::walk` is a different reading.
    #[test]
    fn a_markup_grammar_always_reads_the_whole_file() {
        let mut grammar = rust_like();
        grammar.markup = true;
        let text = "<p>one</p>\n<p>two</p>\n";
        let mut cache = Tokens::default();
        incrementally(&mut cache, text, &grammar, Dirt::Clean);
        let (read, update) =
            incrementally(&mut cache, text, &grammar, Dirt::Clean.note(4, 0, 1));
        assert_eq!(read, whole(text, &grammar));
        assert_eq!(update.changed, 0..text.len(), "the whole file, every time");
    }

    /// **Five hundred random edits, each checked against the whole-file reading.**
    ///
    /// The incremental path is the kind of thing that is right on the cases somebody thought of and
    /// wrong on the one they did not, and the consequence -- a file coloured wrongly from the middle
    /// down -- is visible but hard to attribute. So the invariant is asserted directly and often:
    /// **whatever the edit, the answer is the answer `syntax::scan` gives.**
    ///
    /// The alphabet is chosen to make the awkward cases likely rather than possible: quotes that open
    /// and close strings, `/*` and `*/` that open and close block comments, `//` that runs to the end
    /// of a line, and newlines that move where a line starts.
    #[test]
    fn five_hundred_random_edits_all_agree_with_reading_the_whole_file() {
        let grammar = rust_like();
        let pieces = [
            "pub fn f() {", "}", "let x = 1;", "// note", "/*", "*/", "\"a string\"",
            "\n", " ", "\"", "*", "/", "name", "42", "(", ")", ";",
        ];
        let mut text = String::new();
        for index in 0..80 {
            text.push_str(pieces[index * 7 % pieces.len()]);
            text.push('\n');
        }
        let mut cache = Tokens::default();
        incrementally(&mut cache, &text, &grammar, Dirt::Clean);

        // A tiny deterministic generator, so a failure can be reproduced from the seed alone.
        let mut seed = 0x5EED_1804u64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for round in 0..500 {
            let insert = round % 3 != 0;
            let mut at = next() % (text.len() + 1);
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            let dirt = if insert {
                let piece = pieces[next() % pieces.len()];
                text.insert_str(at, piece);
                Dirt::Clean.note(at, 0, piece.len())
            } else {
                let mut end = (at + 1 + next() % 12).min(text.len());
                while !text.is_char_boundary(end) {
                    end += 1;
                }
                if end <= at {
                    continue;
                }
                let removed = end - at;
                text.replace_range(at..end, "");
                Dirt::Clean.note(at, removed, 0)
            };
            let (read, update) = incrementally(&mut cache, &text, &grammar, dirt);
            assert_eq!(
                read,
                whole(&text, &grammar),
                "round {round} disagreed with the whole-file reading; changed was {:?}",
                update.changed
            );
        }
    }

    /// And the same for the *changed range*, which is the half a wrong answer would show as a stale
    /// colour rather than as a wrong token: everything outside it must really be unchanged.
    #[test]
    fn nothing_outside_the_changed_range_moved() {
        let grammar = rust_like();
        let mut text = String::new();
        for index in 0..300 {
            text.push_str(&format!("let name{index} = \"value {index}\"; // note {index}\n"));
        }
        let mut cache = Tokens::default();
        let (before, _) = incrementally(&mut cache, &text, &grammar, Dirt::Clean);

        let at = text.find("name150").expect("it is there");
        text.insert(at, 'Z');
        let (after, update) =
            incrementally(&mut cache, &text, &grammar, Dirt::Clean.note(at, 0, 1));
        assert_eq!(after, whole(&text, &grammar));

        // Every token that ends before the changed range is exactly where it was.
        let kept: Vec<_> = after.iter().filter(|(range, _)| range.end <= update.changed.start).collect();
        assert!(!kept.is_empty(), "there is something before the edit");
        for (range, token) in kept {
            assert!(
                before.iter().any(|(was, kind)| was == range && kind == token),
                "{range:?} {token:?} claims to be unchanged and was not there before"
            );
        }
    }

    /// `report` says what changed, in order, and nothing else.
    ///
    /// The whole point of the second half of the saving: a caller building a span list for
    /// `Document::set_syntax_in` gets a handful of entries after a keystroke rather than the file's
    /// worth, and never has to sort them.
    #[test]
    fn only_the_tokens_inside_the_changed_stretch_are_reported() {
        let grammar = rust_like();
        let mut text = String::new();
        for index in 0..500 {
            text.push_str(&format!("let name{index} = \"value {index}\"; // note {index}\n"));
        }
        let mut cache = Tokens::default();
        let (all, first) = reported(&mut cache, &text, &grammar, Dirt::Clean);
        assert_eq!(all.len(), cache.all().len(), "the first reading reports the whole file");
        assert_eq!(first.changed, 0..text.len());

        let at = text.find("name250").expect("it is there");
        text.insert(at, 'Z');
        let (said, update) = reported(&mut cache, &text, &grammar, Dirt::Clean.note(at, 0, 1));
        assert!(!said.is_empty());
        assert!(
            said.len() < cache.all().len() / 100,
            "a handful, not the file: {} of {}",
            said.len(),
            cache.all().len()
        );
        assert!(
            said.windows(2).all(|pair| pair[0].0.start <= pair[1].0.start),
            "in file order, so nothing has to be sorted"
        );
        for (range, _) in &said {
            assert!(
                range.start >= update.changed.start && range.end <= update.changed.end,
                "{range:?} is outside {:?}",
                update.changed
            );
        }
    }

    #[test]
    fn dirt_widens_rather_than_replacing() {
        assert_eq!(Dirt::Clean.note(10, 0, 1), Dirt::Part { from: 10, to: 11, delta: 1 });
        // A second edit before the first widens `from` and moves the watermark by its own length.
        assert_eq!(
            Dirt::Clean.note(10, 0, 1).note(4, 0, 2),
            Dirt::Part { from: 4, to: 13, delta: 3 }
        );
        // One after it only moves the watermark forward.
        assert_eq!(
            Dirt::Clean.note(10, 0, 1).note(40, 0, 2),
            Dirt::Part { from: 10, to: 42, delta: 3 }
        );
        assert_eq!(Dirt::Whole.note(1, 0, 1), Dirt::Whole, "whole stays whole");
    }
}
