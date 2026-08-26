//! A B-tree rope: the text buffer behind a Quill document.
//!
//! Leaf nodes hold a short run of UTF-8 bytes. Internal nodes hold their children plus, next to each
//! child, a summary of that child's text. The summary is three counts: bytes, characters and line
//! breaks. Keeping the summaries in the parent is what makes an editor's operations cheap: finding
//! where line 4000 starts walks down the tree adding line break counts without reading any text, and
//! inserting in the middle of a large file rewrites one leaf and the path above it instead of moving
//! the rest of the file.
//!
//! The design follows ropey (<https://github.com/cessen/ropey>, commit 42f6fc79), whose
//! `design/design.md` makes the case for a B-tree over a gap buffer when edits jump around, which is
//! what happens when formatting is applied across a selection. Our node sizes are smaller and simpler
//! than ropey's, which sizes nodes to fill a cache line exactly.

use std::ops::{Add, AddAssign, Range, Sub, SubAssign};

/// A leaf splits once it grows past this many bytes.
const MAX_LEAF: usize = 512;
/// A leaf is merged with a neighbour once it falls below this many bytes.
const MIN_LEAF: usize = MAX_LEAF / 4;
/// An internal node splits once it holds more children than this.
const MAX_CHILDREN: usize = 12;
/// An internal node borrows or merges once it holds fewer children than this.
const MIN_CHILDREN: usize = MAX_CHILDREN / 2;

/// The counts a node keeps about the text underneath it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextInfo {
    pub bytes: usize,
    pub chars: usize,
    pub line_breaks: usize,
}

impl TextInfo {
    fn of(text: &str) -> Self {
        Self {
            bytes: text.len(),
            chars: text.chars().count(),
            line_breaks: text.bytes().filter(|b| *b == b'\n').count(),
        }
    }
}

impl Add for TextInfo {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            bytes: self.bytes + other.bytes,
            chars: self.chars + other.chars,
            line_breaks: self.line_breaks + other.line_breaks,
        }
    }
}

impl AddAssign for TextInfo {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl Sub for TextInfo {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self {
            bytes: self.bytes - other.bytes,
            chars: self.chars - other.chars,
            line_breaks: self.line_breaks - other.line_breaks,
        }
    }
}

impl SubAssign for TextInfo {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
    }
}

#[derive(Debug, Clone)]
enum Node {
    Leaf { text: String, info: TextInfo },
    Internal { children: Vec<Node>, infos: Vec<TextInfo> },
}

/// Split `text` at a character boundary at or after `target`, so that no leaf ever holds a partial
/// UTF-8 sequence.
fn boundary_at_or_after(text: &str, target: usize) -> usize {
    let mut at = target.min(text.len());
    while at < text.len() && !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

impl Node {
    fn leaf(text: String) -> Self {
        let info = TextInfo::of(&text);
        Self::Leaf { text, info }
    }

    fn internal(children: Vec<Node>) -> Self {
        let infos = children.iter().map(Node::info).collect();
        Self::Internal { children, infos }
    }

    fn info(&self) -> TextInfo {
        match self {
            Self::Leaf { info, .. } => *info,
            Self::Internal { infos, .. } => {
                infos.iter().fold(TextInfo::default(), |acc, i| acc + *i)
            }
        }
    }

    #[cfg(test)]
    fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf { .. })
    }

    fn underflows(&self) -> bool {
        match self {
            Self::Leaf { text, .. } => text.len() < MIN_LEAF,
            Self::Internal { children, .. } => children.len() < MIN_CHILDREN,
        }
    }

    /// Find which child owns `byte_idx`, returning the child index and the offset within it.
    ///
    /// A position exactly on the boundary between two children is given to the earlier one, so that
    /// text typed at the end of a child stays with that child.
    fn child_for_byte(infos: &[TextInfo], byte_idx: usize) -> (usize, usize) {
        let mut acc = 0;
        for (i, info) in infos.iter().enumerate() {
            if byte_idx <= acc + info.bytes {
                return (i, byte_idx - acc);
            }
            acc += info.bytes;
        }
        let last = infos.len() - 1;
        (last, infos[last].bytes)
    }

    /// Insert `text` at `byte_idx`. Returns a new right hand sibling when this node had to split.
    fn insert(&mut self, byte_idx: usize, text: &str) -> Option<Node> {
        match self {
            Self::Leaf { text: leaf, info } => {
                leaf.insert_str(byte_idx, text);
                if leaf.len() <= MAX_LEAF {
                    *info = TextInfo::of(leaf);
                    return None;
                }
                let split = boundary_at_or_after(leaf, leaf.len() / 2);
                let right = leaf.split_off(split);
                *info = TextInfo::of(leaf);
                Some(Node::leaf(right))
            }
            Self::Internal { children, infos } => {
                let (i, offset) = Self::child_for_byte(infos, byte_idx);
                let split = children[i].insert(offset, text);
                infos[i] = children[i].info();
                if let Some(new_node) = split {
                    children.insert(i + 1, new_node);
                    infos.insert(i + 1, children[i + 1].info());
                }
                if children.len() <= MAX_CHILDREN {
                    return None;
                }
                let at = children.len() / 2;
                let right_children = children.split_off(at);
                infos.truncate(at);
                Some(Node::internal(right_children))
            }
        }
    }

    /// Remove the bytes in `range`, which is relative to this node.
    fn remove(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        match self {
            Self::Leaf { text, info } => {
                text.replace_range(range, "");
                *info = TextInfo::of(text);
            }
            Self::Internal { children, infos } => {
                // Walk the children from last to first so that dropping a child does not shift the
                // index of one we have not visited yet.
                let mut starts = Vec::with_capacity(children.len());
                let mut acc = 0;
                for info in infos.iter() {
                    starts.push(acc);
                    acc += info.bytes;
                }
                for i in (0..children.len()).rev() {
                    let start = starts[i];
                    let end = start + infos[i].bytes;
                    let from = range.start.max(start);
                    let to = range.end.min(end);
                    if from >= to {
                        continue;
                    }
                    if from == start && to == end {
                        children.remove(i);
                        infos.remove(i);
                        continue;
                    }
                    children[i].remove((from - start)..(to - start));
                    infos[i] = children[i].info();
                }
                if children.is_empty() {
                    *self = Node::leaf(String::new());
                    return;
                }
                for i in (0..children.len()).rev() {
                    self.fix_child(i);
                }
            }
        }
    }

    /// Repair child `i` if it holds too little, by merging it with a neighbour or borrowing from one.
    fn fix_child(&mut self, i: usize) {
        let Self::Internal { children, infos } = self else {
            return;
        };
        if i >= children.len() || !children[i].underflows() || children.len() == 1 {
            return;
        }
        // Merge with, or borrow from, the neighbour on the right when there is one.
        let (left, right) = if i + 1 < children.len() { (i, i + 1) } else { (i - 1, i) };
        let fits = match (&children[left], &children[right]) {
            (Self::Leaf { text: a, .. }, Self::Leaf { text: b, .. }) => a.len() + b.len() <= MAX_LEAF,
            (Self::Internal { children: a, .. }, Self::Internal { children: b, .. }) => {
                a.len() + b.len() <= MAX_CHILDREN
            }
            _ => false,
        };
        if fits {
            let moved = children.remove(right);
            infos.remove(right);
            match (&mut children[left], moved) {
                (Self::Leaf { text, info }, Self::Leaf { text: other, .. }) => {
                    text.push_str(&other);
                    *info = TextInfo::of(text);
                }
                (
                    Self::Internal { children: c, infos: f },
                    Self::Internal { children: oc, infos: of },
                ) => {
                    c.extend(oc);
                    f.extend(of);
                }
                _ => unreachable!("a leaf and an internal node are never siblings"),
            }
            infos[left] = children[left].info();
            return;
        }
        // They do not fit together, so move text or a child across to even them out.
        match (&children[left], &children[right]) {
            (Self::Leaf { .. }, Self::Leaf { .. }) => {
                let total = {
                    let (Self::Leaf { text: a, .. }, Self::Leaf { text: b, .. }) =
                        (&children[left], &children[right])
                    else {
                        unreachable!()
                    };
                    a.len() + b.len()
                };
                let target = total / 2;
                let left_len = match &children[left] {
                    Self::Leaf { text, .. } => text.len(),
                    _ => unreachable!(),
                };
                if left_len < target {
                    // Move the start of the right leaf onto the end of the left one.
                    let take = {
                        let Self::Leaf { text, .. } = &children[right] else { unreachable!() };
                        boundary_at_or_after(text, target - left_len)
                    };
                    let moved = {
                        let Self::Leaf { text, info } = &mut children[right] else { unreachable!() };
                        let rest = text.split_off(take);
                        let moved = std::mem::replace(text, rest);
                        *info = TextInfo::of(text);
                        moved
                    };
                    let Self::Leaf { text, info } = &mut children[left] else { unreachable!() };
                    text.push_str(&moved);
                    *info = TextInfo::of(text);
                } else {
                    // Move the end of the left leaf onto the start of the right one.
                    let split = {
                        let Self::Leaf { text, .. } = &children[left] else { unreachable!() };
                        boundary_at_or_after(text, target)
                    };
                    let moved = {
                        let Self::Leaf { text, info } = &mut children[left] else { unreachable!() };
                        let moved = text.split_off(split);
                        *info = TextInfo::of(text);
                        moved
                    };
                    let Self::Leaf { text, info } = &mut children[right] else { unreachable!() };
                    text.insert_str(0, &moved);
                    *info = TextInfo::of(text);
                }
            }
            (Self::Internal { .. }, Self::Internal { .. }) => {
                let left_len = match &children[left] {
                    Self::Internal { children, .. } => children.len(),
                    _ => unreachable!(),
                };
                let right_len = match &children[right] {
                    Self::Internal { children, .. } => children.len(),
                    _ => unreachable!(),
                };
                if left_len < right_len {
                    let (node, info) = {
                        let Self::Internal { children: c, infos: f } = &mut children[right] else {
                            unreachable!()
                        };
                        (c.remove(0), f.remove(0))
                    };
                    let Self::Internal { children: c, infos: f } = &mut children[left] else {
                        unreachable!()
                    };
                    c.push(node);
                    f.push(info);
                } else {
                    let (node, info) = {
                        let Self::Internal { children: c, infos: f } = &mut children[left] else {
                            unreachable!()
                        };
                        (c.pop().expect("non empty"), f.pop().expect("non empty"))
                    };
                    let Self::Internal { children: c, infos: f } = &mut children[right] else {
                        unreachable!()
                    };
                    c.insert(0, node);
                    f.insert(0, info);
                }
            }
            _ => unreachable!("a leaf and an internal node are never siblings"),
        }
        infos[left] = children[left].info();
        infos[right] = children[right].info();
    }

    fn for_each_chunk(&self, mut f: impl FnMut(&str)) {
        self.walk(&mut f);
    }

    fn walk(&self, f: &mut impl FnMut(&str)) {
        match self {
            Self::Leaf { text, .. } => f(text),
            Self::Internal { children, .. } => {
                for child in children {
                    child.walk(f);
                }
            }
        }
    }

    /// Append the bytes of `range` to `out`.
    fn slice_into(&self, range: Range<usize>, out: &mut String) {
        if range.is_empty() {
            return;
        }
        match self {
            Self::Leaf { text, .. } => out.push_str(&text[range]),
            Self::Internal { children, infos } => {
                let mut acc = 0;
                for (child, info) in children.iter().zip(infos) {
                    let start = acc;
                    let end = acc + info.bytes;
                    acc = end;
                    let from = range.start.max(start);
                    let to = range.end.min(end);
                    if from < to {
                        child.slice_into((from - start)..(to - start), out);
                    }
                    if acc >= range.end {
                        break;
                    }
                }
            }
        }
    }

    /// The raw byte at `idx`, without going through a `&str` slice.
    fn byte_at(&self, idx: usize) -> u8 {
        match self {
            Self::Leaf { text, .. } => text.as_bytes()[idx],
            Self::Internal { children, infos } => {
                let mut acc = 0;
                for (child, info) in children.iter().zip(infos) {
                    if idx < acc + info.bytes {
                        return child.byte_at(idx - acc);
                    }
                    acc += info.bytes;
                }
                panic!("byte {idx} is past the end of the text");
            }
        }
    }

    /// Byte offset where the line at index `line` starts.
    fn line_to_byte(&self, line: usize) -> usize {
        match self {
            Self::Leaf { text, .. } => {
                if line == 0 {
                    return 0;
                }
                let mut seen = 0;
                for (i, b) in text.bytes().enumerate() {
                    if b == b'\n' {
                        seen += 1;
                        if seen == line {
                            return i + 1;
                        }
                    }
                }
                text.len()
            }
            Self::Internal { children, infos } => {
                let mut bytes_before = 0;
                let mut lines_before = 0;
                for (child, info) in children.iter().zip(infos) {
                    if lines_before + info.line_breaks >= line && line > lines_before {
                        return bytes_before + child.line_to_byte(line - lines_before);
                    }
                    if line == lines_before {
                        return bytes_before + child.line_to_byte(0);
                    }
                    bytes_before += info.bytes;
                    lines_before += info.line_breaks;
                }
                bytes_before
            }
        }
    }

    /// Which line the byte at `byte_idx` sits on.
    fn byte_to_line(&self, byte_idx: usize) -> usize {
        match self {
            Self::Leaf { text, .. } => {
                text[..byte_idx].bytes().filter(|b| *b == b'\n').count()
            }
            Self::Internal { children, infos } => {
                let mut acc = 0;
                let mut lines = 0;
                for (child, info) in children.iter().zip(infos) {
                    if byte_idx <= acc + info.bytes {
                        return lines + child.byte_to_line(byte_idx - acc);
                    }
                    acc += info.bytes;
                    lines += info.line_breaks;
                }
                lines
            }
        }
    }

    #[cfg(test)]
    fn check(&self, is_root: bool) -> TextInfo {
        match self {
            Self::Leaf { text, info } => {
                assert_eq!(*info, TextInfo::of(text), "leaf summary is stale");
                assert!(text.len() <= MAX_LEAF, "leaf holds {} bytes", text.len());
                *info
            }
            Self::Internal { children, infos } => {
                assert_eq!(children.len(), infos.len(), "child and summary counts differ");
                assert!(!children.is_empty(), "internal node with no children");
                assert!(children.len() <= MAX_CHILDREN, "internal node holds too many children");
                if !is_root {
                    assert!(children.len() >= MIN_CHILDREN, "internal node holds too few children");
                }
                let mut total = TextInfo::default();
                for (child, info) in children.iter().zip(infos) {
                    let real = child.check(false);
                    assert_eq!(real, *info, "child summary is stale");
                    total += real;
                }
                total
            }
        }
    }
}

/// The text of one document.
#[derive(Debug, Clone)]
pub struct Rope {
    root: Node,
}

impl Default for Rope {
    fn default() -> Self {
        Self::new()
    }
}

impl Rope {
    pub fn new() -> Self {
        Self { root: Node::leaf(String::new()) }
    }

    /// Named `from_str` to match ropey, whose interface a reader of this code is likely to know. It is
    /// not `std::str::FromStr`, which cannot fail here and would force a `Result` on every caller.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        let mut rope = Self::new();
        rope.insert(0, text);
        rope
    }

    pub fn len_bytes(&self) -> usize {
        self.root.info().bytes
    }

    pub fn len_chars(&self) -> usize {
        self.root.info().chars
    }

    /// The number of lines, counting the text after the last line break as a line. An empty document
    /// has one line.
    pub fn len_lines(&self) -> usize {
        self.root.info().line_breaks + 1
    }

    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// Insert `text` at byte offset `byte_idx`, which must be a character boundary.
    ///
    /// Long inserts are cut into leaf sized pieces so that one paste of a whole file does not build a
    /// single enormous leaf.
    pub fn insert(&mut self, byte_idx: usize, text: &str) {
        assert!(byte_idx <= self.len_bytes(), "insert past the end of the text");
        if text.is_empty() {
            return;
        }
        let mut at = byte_idx;
        let mut rest = text;
        while !rest.is_empty() {
            let take = boundary_at_or_after(rest, MAX_LEAF.min(rest.len()));
            let (piece, tail) = rest.split_at(take);
            self.insert_piece(at, piece);
            at += piece.len();
            rest = tail;
        }
    }

    fn insert_piece(&mut self, byte_idx: usize, text: &str) {
        if let Some(right) = self.root.insert(byte_idx, text) {
            let left = std::mem::replace(&mut self.root, Node::leaf(String::new()));
            self.root = Node::internal(vec![left, right]);
        }
    }

    /// Remove the bytes in `range`. Both ends must be character boundaries.
    pub fn remove(&mut self, range: Range<usize>) {
        assert!(range.end <= self.len_bytes(), "remove past the end of the text");
        if range.is_empty() {
            return;
        }
        self.root.remove(range);
        // A root that is down to a single child is one level taller than it needs to be.
        loop {
            let collapse = match &self.root {
                Node::Internal { children, .. } => children.len() == 1,
                Node::Leaf { .. } => false,
            };
            if !collapse {
                break;
            }
            let Node::Internal { children, .. } = &mut self.root else { unreachable!() };
            self.root = children.remove(0);
        }
    }

    pub fn byte_slice(&self, range: Range<usize>) -> String {
        let mut out = String::with_capacity(range.len());
        self.root.slice_into(range, &mut out);
        out
    }

    /// The same, into a buffer the caller already has.
    ///
    /// Layout walks every paragraph in turn and needs the text of each, so asking for a fresh
    /// `String` each time was one allocation per paragraph for no reason. The buffer is cleared
    /// first, so a caller can hand the same one back for the next paragraph.
    pub fn slice_into(&self, range: Range<usize>, out: &mut String) {
        out.clear();
        self.root.slice_into(range, out);
    }

    pub fn line_to_byte(&self, line: usize) -> usize {
        if line >= self.len_lines() {
            return self.len_bytes();
        }
        self.root.line_to_byte(line)
    }

    pub fn byte_to_line(&self, byte_idx: usize) -> usize {
        self.root.byte_to_line(byte_idx.min(self.len_bytes()))
    }

    /// The byte range of one line, not including its trailing line break.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        let start = self.line_to_byte(line);
        let end = if line + 1 < self.len_lines() {
            self.line_to_byte(line + 1).saturating_sub(1)
        } else {
            self.len_bytes()
        };
        start..end.max(start)
    }

    pub fn for_each_chunk(&self, f: impl FnMut(&str)) {
        self.root.for_each_chunk(f);
    }

    /// True when `byte_idx` sits on a character boundary.
    pub fn is_char_boundary(&self, byte_idx: usize) -> bool {
        if byte_idx == 0 || byte_idx == self.len_bytes() {
            return true;
        }
        if byte_idx > self.len_bytes() {
            return false;
        }
        // A continuation byte is 0b10xxxxxx; any other byte starts a character. The byte is read
        // directly rather than through `byte_slice`, because slicing a `&str` inside a character
        // panics, which is the very case this function exists to report on.
        (self.root.byte_at(byte_idx) & 0b1100_0000) != 0b1000_0000
    }

    #[cfg(test)]
    fn check(&self) {
        self.root.check(true);
    }
}

impl std::fmt::Display for Rope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = Ok(());
        self.root.for_each_chunk(|chunk| {
            if result.is_ok() {
                result = f.write_str(chunk);
            }
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator, so the randomised tests fail the same way every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            // A linear congruential generator with the constants from Numerical Recipes.
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 16
        }

        fn below(&mut self, limit: usize) -> usize {
            if limit == 0 {
                0
            } else {
                (self.next() as usize) % limit
            }
        }
    }

    #[test]
    fn empty_rope_has_one_line() {
        let rope = Rope::new();
        assert_eq!(rope.len_bytes(), 0);
        assert_eq!(rope.len_chars(), 0);
        assert_eq!(rope.len_lines(), 1);
        assert_eq!(rope.to_string(), "");
        rope.check();
    }

    #[test]
    fn insert_and_read_back_short_text() {
        let mut rope = Rope::new();
        rope.insert(0, "hello world");
        rope.insert(5, ",");
        assert_eq!(rope.to_string(), "hello, world");
        assert_eq!(rope.len_bytes(), 12);
        rope.check();
    }

    #[test]
    fn counts_characters_not_bytes() {
        let rope = Rope::from_str("café 日本語");
        assert_eq!(rope.len_chars(), 8);
        assert_eq!(rope.len_bytes(), 15);
        rope.check();
    }

    #[test]
    fn text_longer_than_one_leaf_splits_into_a_tree() {
        let line = "the quick brown fox jumps over the lazy dog\n";
        let text = line.repeat(400);
        let rope = Rope::from_str(&text);
        assert_eq!(rope.to_string(), text);
        assert_eq!(rope.len_lines(), 401);
        assert!(!rope.root.is_leaf(), "a document of {} bytes should not be one leaf", text.len());
        rope.check();
    }

    #[test]
    fn line_lookups_agree_with_a_plain_string() {
        let text = "alpha\nbeta\ngamma\ndelta";
        let rope = Rope::from_str(text);
        assert_eq!(rope.len_lines(), 4);
        assert_eq!(rope.line_to_byte(0), 0);
        assert_eq!(rope.line_to_byte(1), 6);
        assert_eq!(rope.line_to_byte(2), 11);
        assert_eq!(rope.line_to_byte(3), 17);
        assert_eq!(rope.byte_to_line(0), 0);
        assert_eq!(rope.byte_to_line(6), 1);
        assert_eq!(rope.byte_to_line(16), 2);
        assert_eq!(rope.byte_to_line(21), 3);
        assert_eq!(rope.byte_slice(rope.line_range(2)), "gamma");
        assert_eq!(rope.byte_slice(rope.line_range(3)), "delta");
    }

    #[test]
    fn line_lookups_hold_across_leaf_boundaries() {
        // Enough lines to force several levels of tree.
        let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
        let rope = Rope::from_str(&text);
        let flat: Vec<&str> = text.split('\n').collect();
        assert_eq!(rope.len_lines(), flat.len());
        for (line, expected) in flat.iter().enumerate() {
            assert_eq!(rope.byte_slice(rope.line_range(line)), *expected, "line {line} read back wrong");
            let start = rope.line_to_byte(line);
            assert_eq!(rope.byte_to_line(start), line, "line {line} start maps to the wrong line");
        }
        rope.check();
    }

    #[test]
    fn removing_everything_leaves_an_empty_rope() {
        let mut rope = Rope::from_str(&"abcdefghij".repeat(300));
        let len = rope.len_bytes();
        rope.remove(0..len);
        assert_eq!(rope.to_string(), "");
        assert_eq!(rope.len_lines(), 1);
        rope.check();
    }

    #[test]
    fn removing_a_middle_range_merges_leaves_back_together() {
        let text = "x".repeat(4000);
        let mut rope = Rope::from_str(&text);
        rope.remove(100..3900);
        assert_eq!(rope.to_string(), "x".repeat(200));
        rope.check();
    }

    #[test]
    fn slicing_never_splits_a_character() {
        let rope = Rope::from_str(&"héllo wörld ".repeat(100));
        assert!(rope.is_char_boundary(0));
        assert!(rope.is_char_boundary(1));
        assert!(!rope.is_char_boundary(2), "byte 2 is inside the é");
        assert!(rope.is_char_boundary(3));
    }

    #[test]
    fn random_edits_match_a_plain_string_oracle() {
        // The oracle is a String. Anything the rope does differently is a bug in the rope.
        let mut rng = Rng(0x5eed);
        let mut rope = Rope::new();
        let mut oracle = String::new();
        let words = ["hello ", "world\n", "café ", "日本語\n", "x", "a longer run of text "];
        for step in 0..1500 {
            let insert = oracle.is_empty() || rng.below(3) != 0;
            if insert {
                let word = words[rng.below(words.len())];
                let at = {
                    let mut at = rng.below(oracle.len() + 1);
                    while !oracle.is_char_boundary(at) {
                        at += 1;
                    }
                    at
                };
                rope.insert(at, word);
                oracle.insert_str(at, word);
            } else {
                let mut start = rng.below(oracle.len());
                while !oracle.is_char_boundary(start) {
                    start += 1;
                }
                let mut end = start + rng.below(oracle.len() - start + 1);
                while !oracle.is_char_boundary(end) {
                    end += 1;
                }
                rope.remove(start..end);
                oracle.replace_range(start..end, "");
            }
            assert_eq!(rope.to_string(), oracle, "text differs at step {step}");
            assert_eq!(rope.len_bytes(), oracle.len(), "byte count differs at step {step}");
            assert_eq!(rope.len_chars(), oracle.chars().count(), "char count differs at step {step}");
            assert_eq!(
                rope.len_lines(),
                oracle.split('\n').count(),
                "line count differs at step {step}"
            );
            rope.check();
        }
        // Line lookups on the final text, which the loop above never checked.
        let lines: Vec<&str> = oracle.split('\n').collect();
        for (line, expected) in lines.iter().enumerate() {
            assert_eq!(rope.byte_slice(rope.line_range(line)), *expected, "line {line}");
        }
    }

    #[test]
    fn slices_match_the_oracle_at_every_offset() {
        let text = "one\ntwo\nthree\nfour\nfive\n".repeat(40);
        let rope = Rope::from_str(&text);
        for start in (0..text.len()).step_by(7) {
            for len in [0, 1, 5, 40, 300] {
                let end = (start + len).min(text.len());
                if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                    continue;
                }
                assert_eq!(rope.byte_slice(start..end), &text[start..end], "slice {start}..{end}");
            }
        }
    }
}
