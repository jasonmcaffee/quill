//! Turning a document into positioned glyphs.
//!
//! Layout takes the text, the character formatting, the paragraph formatting and an available width,
//! and produces lines. Each line holds runs, and each run holds positioned clusters that share one
//! style, so one run is one font at one size in one colour. Painting walks that list. Hit testing
//! walks it backwards to turn a mouse position into a position in the text.
//!
//! The shape of this interface follows parley (<https://github.com/linebender/parley>, commit
//! 1aba7cac): styles as ranges over the text, layout producing lines that hold runs that hold
//! positioned glyphs.
//!
//! Version one places one cluster after another from left to right. That is correct for Latin, Greek
//! and Cyrillic and wrong for Arabic and Hindi, which need a shaping step. The `FontMetrics` boundary
//! is where that step goes.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::metrics::FontMetrics;
use crate::rope::Rope;
use crate::style::{Align, CharStyle, ParagraphStyle, ParagraphStyles, StyleSpans};

/// A rectangle in the editor's own coordinates, with the origin at the top left of the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// The text of one grapheme cluster.
///
/// Nearly every cluster in nearly every document is one to four bytes, and layout makes one of these
/// for every grapheme in the file — so a `String` here was a heap allocation per letter, and laying
/// out a file the size of `app/mod.rs` made a hundred and sixteen thousand of them. Up to
/// [`ClusterText::INLINE`] bytes are held in the value itself, which is every cluster anybody
/// writes; a longer one spills to the heap, so nothing is ever truncated.
///
/// It dereferences to `str` and compares against one, so everything that read a `String` here still
/// reads it the same way.
#[derive(Clone)]
pub enum ClusterText {
    Inline { bytes: [u8; ClusterText::INLINE], len: u8 },
    Long(Box<str>),
}

impl ClusterText {
    /// How many bytes fit without touching the heap. Twenty-two is what leaves this the same size a
    /// `String` was, and it is far past the longest grapheme cluster in ordinary writing — a family
    /// emoji with four members and three joiners is twenty-five, and that spills.
    pub const INLINE: usize = 22;

    pub fn as_str(&self) -> &str {
        match self {
            Self::Inline { bytes, len } => {
                // Safe because the only way to build one is from a `&str`, which is valid UTF-8.
                std::str::from_utf8(&bytes[..*len as usize]).unwrap_or("")
            }
            Self::Long(text) => text,
        }
    }
}

impl From<&str> for ClusterText {
    fn from(text: &str) -> Self {
        if text.len() <= Self::INLINE {
            let mut bytes = [0u8; Self::INLINE];
            bytes[..text.len()].copy_from_slice(text.as_bytes());
            Self::Inline { bytes, len: text.len() as u8 }
        } else {
            Self::Long(text.into())
        }
    }
}

impl std::ops::Deref for ClusterText {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for ClusterText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl PartialEq for ClusterText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for ClusterText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ClusterText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// One grapheme cluster, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedCluster {
    /// The text of the cluster, which is what the painter asks the font for.
    pub text: ClusterText,
    /// Where the cluster came from in the document, so a click can be turned back into an offset.
    pub bytes: Range<usize>,
    /// Left edge, relative to the left edge of the text area.
    pub x: f32,
    pub advance: f32,
}

impl PlacedCluster {
    pub fn right(&self) -> f32 {
        self.x + self.advance
    }
}

/// A stretch of one line that shares a single style.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedRun {
    pub style: CharStyle,
    pub clusters: Vec<PlacedCluster>,
}

impl PlacedRun {
    pub fn left(&self) -> f32 {
        self.clusters.first().map(|c| c.x).unwrap_or(0.0)
    }

    pub fn right(&self) -> f32 {
        self.clusters.last().map(PlacedCluster::right).unwrap_or(0.0)
    }
}

/// One line on screen. With word wrap one paragraph is several lines.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLine {
    /// Top edge of the line.
    pub y: f32,
    /// Full height of the line, including the paragraph's line spacing.
    pub height: f32,
    /// Where the baseline sits, measured from the top of the line.
    pub baseline: f32,
    /// The tallest ascent of any style on the line, and the deepest descent.
    ///
    /// Together they are the box the glyphs actually occupy, which is not the same as `height`:
    /// that carries the font's line gap, the reading leading the application adds on top of it for
    /// prose, and the paragraph's line spacing, none of which any glyph reaches into. The caret is
    /// drawn to this box rather than to the line, so it stands exactly as tall as the letters
    /// beside it instead of towering over them by the air that was added between the lines.
    pub ascent: f32,
    pub descent: f32,
    /// The bytes of the document this line covers, not including a trailing line break.
    pub bytes: Range<usize>,
    /// Which paragraph this line belongs to.
    pub paragraph: usize,
    /// True when this is the last line of its paragraph, which justified alignment needs to know.
    pub last_in_paragraph: bool,
    pub runs: Vec<PlacedRun>,
    /// The style to use for a caret sitting on an otherwise empty line.
    pub empty_style: CharStyle,
}

impl PlacedLine {
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Move a line that did not itself change to where it now sits.
    ///
    /// An edit above a paragraph moves it down or up the page, moves every byte it covers along, and
    /// may renumber the paragraph it belongs to — but changes nothing about what is on it. The
    /// clusters carry document offsets too, so they move with it; forgetting them was a real fault,
    /// caught by `relayout_agrees_with_layout_after_every_shape_of_edit`.
    fn move_to(&mut self, y: f32, bytes: isize, paragraph: isize) {
        let shift = |offset: usize| (offset as isize + bytes) as usize;
        self.y = y;
        self.bytes = shift(self.bytes.start)..shift(self.bytes.end);
        self.paragraph = (self.paragraph as isize + paragraph) as usize;
        if bytes == 0 {
            return;
        }
        for run in &mut self.runs {
            for cluster in &mut run.clusters {
                cluster.bytes = shift(cluster.bytes.start)..shift(cluster.bytes.end);
            }
        }
    }

    fn clusters(&self) -> impl Iterator<Item = &PlacedCluster> {
        self.runs.iter().flat_map(|run| run.clusters.iter())
    }

    /// Left edge of the first cluster, which alignment moves around.
    pub fn left(&self) -> f32 {
        self.runs.first().map(PlacedRun::left).unwrap_or(0.0)
    }

    /// Right edge of the last cluster.
    pub fn right(&self) -> f32 {
        self.runs.last().map(PlacedRun::right).unwrap_or(self.left())
    }
}

/// A whole document, laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub lines: Vec<PlacedLine>,
    /// The width layout was asked to fit into.
    pub width: f32,
    /// The total height of every line.
    pub height: f32,
    /// One number per paragraph, boiled down from everything that paragraph was laid out from: its
    /// text, the formatting over it and its own paragraph style. [`relayout`] compares these against
    /// the document as it is now to find what actually changed, so that typing a letter costs the
    /// paragraph it was typed into rather than the whole file.
    fingerprints: Vec<u64>,
    /// Where each paragraph's lines start in `lines`, with one more entry holding `lines.len()`, so a
    /// paragraph's lines are always `starts[p]..starts[p + 1]`.
    starts: Vec<usize>,
}

/// Where a caret should be drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub line: usize,
}

/// A point down the page, held as something that survives the text being laid out again.
///
/// A scroll position is a number of points from the top of the document, so it means something
/// different the moment the text is laid out at a different size: zooming in from twelve points to
/// twenty at the same scroll offset leaves the reader looking at a line a third of the way further
/// up the file. What does not change is *which line* was being looked at, so that is what an anchor
/// holds — where the line starts, and how far down it the point sat. Take one with
/// [`Layout::anchor_at_y`] before the size changes and ask the new layout for
/// [`Layout::y_of_anchor`] after it, and the same text is under the same point on the screen.
///
/// The offset is the **start of the line**, not the offset under the point, so a line that wraps
/// differently at the new size still has an answer: [`Layout::line_of_offset`] gives whichever of
/// its lines that byte now falls on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Where the line the point fell on starts.
    pub offset: usize,
    /// How far down that line the point sat, from 0 at its top edge to 1 at its bottom, so a line
    /// drawn taller keeps the point in the same place within it.
    pub fraction: f32,
}

/// Lay `text` out into `width`, measuring with `metrics`.
///
/// Every paragraph is laid out from four things and nothing else: its text, the character formatting
/// over it, its own paragraph style, and the width. That is what makes [`relayout`] possible, and it
/// is worth keeping true.
pub fn layout(
    text: &Rope,
    chars: &StyleSpans,
    paragraphs: &ParagraphStyles,
    metrics: &dyn FontMetrics,
    width: f32,
) -> Layout {
    let width = width.max(1.0);
    // The spans, read once with their absolute positions, so each paragraph can find its own with a
    // binary search. Asking `runs_in` per paragraph walked the list from byte zero every time, which
    // made layout O(paragraphs x spans) — thirty seven million iterations on a coloured file of this
    // repository's own source. See `tasks/task-1666-performance-tdd.md` section 4.
    let spans: Vec<(Range<usize>, &CharStyle)> = chars.spans().collect();
    let count = text.len_lines();
    let mut work = Work::with_capacity(count);
    let mut buffers = Buffers::default();
    for paragraph in 0..count {
        // The fingerprint comes back from laying the paragraph out rather than being worked out
        // beforehand, because the two want the same text and the same runs and there is no reason to
        // read them twice.
        let mark =
            lay_out_paragraph(paragraph, text, &spans, paragraphs, metrics, width, &mut buffers, &mut work);
        work.fingerprints.push(mark);
    }
    work.finish(width)
}

/// Lay out again, keeping every paragraph that has not changed.
///
/// Typing a letter changes one paragraph. Laying the whole document out again for it costs tens of
/// milliseconds on a large file, which is a stutter a person feels on every key press. This compares
/// each paragraph's fingerprint against the previous layout's, keeps the longest run that matches at the
/// start and the longest that matches at the end, and lays out only what is between them.
///
/// **A prefix and a suffix rather than a general diff**, because that is the shape of an edit: type a
/// letter and one paragraph changes, press Enter and one becomes two, delete a selection and twenty
/// become none.
///
/// The answer is the same as [`layout`] gives, exactly. The kept lines carry their own heights, and
/// their vertical positions are added up again in the same order rather than shifted by a difference,
/// so not even the last bit of a float can drift.
/// `relayout_agrees_with_layout_after_every_shape_of_edit` is what holds that.
///
/// The previous layout is **taken** rather than borrowed, so that the lines that did not change are
/// moved into the answer instead of being copied into it. Copying them would mean copying every run
/// and every cluster of nearly the whole document, which is most of what laying it out again cost.
pub fn relayout(
    previous: Layout,
    text: &Rope,
    chars: &StyleSpans,
    paragraphs: &ParagraphStyles,
    metrics: &dyn FontMetrics,
    width: f32,
) -> Layout {
    let width = width.max(1.0);
    // A layout from before this existed, or one at another width, is not something to build on.
    if previous.width != width || previous.starts.len() != previous.fingerprints.len() + 1 {
        return layout(text, chars, paragraphs, metrics, width);
    }
    let spans: Vec<(Range<usize>, &CharStyle)> = chars.spans().collect();
    let count = text.len_lines();
    let mut buffers = Buffers::default();
    let fingerprints: Vec<u64> = (0..count)
        .map(|paragraph| fingerprint_of(paragraph, text, &spans, paragraphs, &mut buffers))
        .collect();

    let Layout { mut lines, starts: was, fingerprints: old, .. } = previous;
    let shortest = count.min(old.len());
    let prefix = (0..shortest).take_while(|&i| fingerprints[i] == old[i]).count();
    let suffix = (0..shortest - prefix)
        .take_while(|&i| fingerprints[count - 1 - i] == old[old.len() - 1 - i])
        .count();

    // The kept lines are moved out of the previous layout rather than copied out of it. Cloning them
    // would mean cloning every run and every cluster of nearly the whole document, which is most of
    // what laying it out again cost in the first place.
    let first_after = old.len() - suffix;
    let after: Vec<PlacedLine> = lines.split_off(was[first_after]);
    lines.truncate(was[prefix]);

    let mut work = Work::with_capacity(count);
    work.fingerprints = fingerprints;
    work.starts.extend_from_slice(&was[..prefix]);
    work.y = lines.last().map(PlacedLine::bottom).unwrap_or(0.0);
    work.lines = lines;
    // The paragraphs that changed.
    for paragraph in prefix..count - suffix {
        // The fingerprint it gives back is thrown away: this already has all of them, from the pass
        // that decided which paragraphs these are.
        let _ = lay_out_paragraph(
            paragraph, text, &spans, paragraphs, metrics, width, &mut buffers, &mut work,
        );
    }
    // The paragraphs after the change: the same lines, moved. Their contents and their heights are by
    // definition unchanged, so only where they sit, which bytes they cover and which paragraph they
    // belong to have to be worked out again.
    let byte_shift = if suffix > 0 {
        text.line_range(count - suffix).start as isize - after[0].bytes.start as isize
    } else {
        0
    };
    let paragraph_shift = count as isize - old.len() as isize;
    let mut moving = after.into_iter();
    for paragraph in first_after..old.len() {
        work.starts.push(work.lines.len());
        for _ in was[paragraph]..was[paragraph + 1] {
            let Some(mut line) = moving.next() else { break };
            line.move_to(work.y, byte_shift, paragraph_shift);
            work.y += line.height;
            work.lines.push(line);
        }
    }
    work.finish(width)
}

/// The lines built so far, and where the next one goes.
struct Work {
    lines: Vec<PlacedLine>,
    fingerprints: Vec<u64>,
    starts: Vec<usize>,
    y: f32,
}

impl Work {
    fn with_capacity(paragraphs: usize) -> Self {
        Self {
            lines: Vec::with_capacity(paragraphs),
            fingerprints: Vec::with_capacity(paragraphs),
            starts: Vec::with_capacity(paragraphs + 1),
            y: 0.0,
        }
    }

    fn finish(mut self, width: f32) -> Layout {
        self.starts.push(self.lines.len());
        Layout {
            height: self.y,
            lines: self.lines,
            width,
            fingerprints: self.fingerprints,
            starts: self.starts,
        }
    }
}

/// Room to work in, handed from one paragraph to the next so that laying out three thousand
/// paragraphs is not three thousand allocations of each of these.
struct Buffers<'a> {
    source: String,
    runs: Vec<(Range<usize>, &'a CharStyle)>,
    clusters: Vec<(usize, PlacedCluster)>,
    breaks: Vec<Range<usize>>,
}

impl Default for Buffers<'_> {
    fn default() -> Self {
        Self {
            source: String::new(),
            runs: Vec::new(),
            clusters: Vec::new(),
            breaks: Vec::new(),
        }
    }
}

/// The runs of formatting covering `bytes`, found by binary search over the whole document's spans.
fn runs_over<'a>(
    spans: &[(Range<usize>, &'a CharStyle)],
    bytes: &Range<usize>,
    out: &mut Vec<(Range<usize>, &'a CharStyle)>,
) {
    out.clear();
    let first = spans.partition_point(|(range, _)| range.end <= bytes.start);
    for (range, style) in &spans[first..] {
        if range.start >= bytes.end {
            break;
        }
        let from = range.start.max(bytes.start);
        let to = range.end.min(bytes.end);
        if from < to {
            out.push((from..to, style));
        }
    }
}

/// The formatting at a byte offset, taking the earlier span at a boundary.
///
/// The same rule [`StyleSpans::style_at`] follows — so that a caret at the end of a bold word stays
/// bold — but over the positioned spans layout has already collected, which makes it a binary search
/// rather than a walk from byte zero.
fn style_at<'a>(spans: &[(Range<usize>, &'a CharStyle)], offset: usize) -> CharStyle {
    let index = spans.partition_point(|(range, _)| range.end < offset);
    spans
        .get(index)
        .or_else(|| spans.last())
        .map(|(_, style)| (*style).clone())
        .unwrap_or_default()
}

/// Boil one paragraph's inputs down to a number, so that [`relayout`] can tell whether it changed.
///
/// Everything [`lay_out_paragraph`] reads is in here, and nothing else is. A field added to
/// `ParagraphStyle` or to `CharStyle` belongs here too, or a change to it would leave a stale line on
/// the screen.
fn fingerprint_of<'a>(
    paragraph: usize,
    text: &Rope,
    spans: &[(Range<usize>, &'a CharStyle)],
    paragraphs: &ParagraphStyles,
    buffers: &mut Buffers<'a>,
) -> u64 {
    let bytes = text.line_range(paragraph);
    text.slice_into(bytes.clone(), &mut buffers.source);
    runs_over(spans, &bytes, &mut buffers.runs);
    fingerprint(&buffers.source, &bytes, &buffers.runs, spans, paragraphs.get(paragraph))
}

/// The fingerprint of a paragraph whose text and runs have already been read.
///
/// One definition, called both from the pass [`relayout`] makes over the whole document before it
/// decides anything and from [`lay_out_paragraph`] as it goes, so the two cannot come to different
/// answers about whether a paragraph changed.
fn fingerprint(
    source: &str,
    bytes: &Range<usize>,
    runs: &[(Range<usize>, &CharStyle)],
    spans: &[(Range<usize>, &CharStyle)],
    paragraph_style: ParagraphStyle,
) -> u64 {
    let mut hash = Fingerprint::default();
    hash.write(source.as_bytes());
    // Relative to the paragraph, because a paragraph that has not itself changed must fingerprint the
    // same wherever an edit above it has moved it to.
    for (range, style) in runs {
        hash.number((range.start - bytes.start) as u64);
        hash.number((range.end - bytes.start) as u64);
        hash.style(style);
    }
    if runs.is_empty() {
        // An empty paragraph has no runs but is still drawn at the height of the formatting the caret
        // would take there, so that formatting is part of what it was laid out from.
        hash.style(&style_at(spans, bytes.start));
    }
    hash.number(paragraph_style.align as u64);
    hash.float(paragraph_style.line_spacing);
    hash.float(paragraph_style.min_height);
    hash.0
}

/// A small, fast, non-cryptographic hash, written here rather than taken from a crate because
/// `quill-core` deliberately carries almost no dependencies and this is a dozen lines.
///
/// It decides whether a paragraph is the one that was laid out last time. A collision would leave a
/// stale line on the screen; at sixty four bits, over any document a person will open, it will not
/// happen.
#[derive(Default)]
struct Fingerprint(u64);

impl Fingerprint {
    const MIX: u64 = 0x517c_c1b7_2722_0a95;

    fn number(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(Self::MIX);
    }

    fn float(&mut self, value: f32) {
        self.number(value.to_bits() as u64);
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.number(u64::from_le_bytes(chunk.try_into().expect("eight bytes")));
        }
        let rest = chunks.remainder();
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        self.number(u64::from_le_bytes(tail));
        self.number(bytes.len() as u64);
    }

    fn style(&mut self, style: &CharStyle) {
        self.write(style.family.as_bytes());
        self.float(style.size);
        self.number(u64::from(style.bold));
        self.number(u64::from(style.italic));
        self.number(u64::from(style.underline));
        self.number(u64::from(style.strikethrough));
        self.number(
            (u64::from(style.color.r) << 16)
                | (u64::from(style.color.g) << 8)
                | u64::from(style.color.b),
        );
    }
}

/// Lay one paragraph out, appending its lines to `work`, and give back its fingerprint.
fn lay_out_paragraph<'a>(
    paragraph: usize,
    text: &Rope,
    spans: &[(Range<usize>, &'a CharStyle)],
    paragraphs: &ParagraphStyles,
    metrics: &dyn FontMetrics,
    width: f32,
    buffers: &mut Buffers<'a>,
    work: &mut Work,
) -> u64 {
    let paragraph_style = paragraphs.get(paragraph);
    let bytes = text.line_range(paragraph);
    text.slice_into(bytes.clone(), &mut buffers.source);
    runs_over(spans, &bytes, &mut buffers.runs);
    let mark = fingerprint(&buffers.source, &bytes, &buffers.runs, spans, paragraph_style);
    work.starts.push(work.lines.len());

    // Flatten the paragraph into clusters, each carrying the index of the run it came from.
    buffers.clusters.clear();
    for (run_index, (run_bytes, style)) in buffers.runs.iter().enumerate() {
        let local = (run_bytes.start - bytes.start)..(run_bytes.end - bytes.start);
        let run_text = &buffers.source[local];
        for (offset, cluster) in run_text.grapheme_indices(true) {
            let start = run_bytes.start + offset;
            buffers.clusters.push((
                run_index,
                PlacedCluster {
                    text: ClusterText::from(cluster),
                    bytes: start..start + cluster.len(),
                    x: 0.0,
                    advance: metrics.advance(cluster, style),
                },
            ));
        }
    }

    let empty_style = match buffers.runs.first() {
        Some((_, style)) => (*style).clone(),
        None => style_at(spans, bytes.start),
    };

    if buffers.clusters.is_empty() {
        // An empty paragraph is still a line, so the caret has somewhere to sit.
        let line_metrics = metrics.line_metrics(&empty_style);
        let height =
            (line_metrics.height() * paragraph_style.line_spacing).max(paragraph_style.min_height);
        work.lines.push(PlacedLine {
            y: work.y,
            height,
            baseline: line_metrics.ascent + (height - line_metrics.height()) / 2.0,
            ascent: line_metrics.ascent,
            descent: line_metrics.descent,
            bytes: bytes.clone(),
            paragraph,
            last_in_paragraph: true,
            runs: Vec::new(),
            empty_style,
        });
        work.y += height;
        return mark;
    }

    // Break the clusters into lines that fit the width, preferring to break after a space.
    let mut breaks = std::mem::take(&mut buffers.breaks);
    breaks.clear();
    let mut line_start = 0;
    let mut pen = 0.0_f32;
    let mut last_space: Option<usize> = None;
    for index in 0..buffers.clusters.len() {
        let advance = buffers.clusters[index].1.advance;
        let is_space = is_blank(&buffers.clusters[index].1);
        if pen + advance > width && index > line_start {
            // Break after the last space if there was one, otherwise break a word that is on its own
            // wider than the line, because the alternative is text running off the edge.
            let at = match last_space {
                Some(space) if space + 1 > line_start => space + 1,
                _ => index,
            };
            breaks.push(line_start..at);
            line_start = at;
            pen = buffers.clusters[line_start..index + 1].iter().map(|(_, c)| c.advance).sum();
            last_space = if is_space { Some(index) } else { None };
            continue;
        }
        pen += advance;
        if is_space {
            last_space = Some(index);
        }
    }
    breaks.push(line_start..buffers.clusters.len());

    let break_count = breaks.len();
    for (index, span) in breaks.iter().enumerate() {
        let last_in_paragraph = index + 1 == break_count;
        let slice = &mut buffers.clusters[span.clone()];

        // Trailing spaces do not count towards the width used for alignment, because a centred line
        // should look centred on its visible text.
        let visible =
            slice.iter().rposition(|(_, c)| !is_blank(c)).map(|last| last + 1).unwrap_or(0);
        let visible_width: f32 = slice[..visible].iter().map(|(_, c)| c.advance).sum();

        let mut extra_per_gap = 0.0;
        let offset = match paragraph_style.align {
            Align::Left => 0.0,
            Align::Center => ((width - visible_width) / 2.0).max(0.0),
            Align::Right => (width - visible_width).max(0.0),
            Align::Justify => {
                // The last line of a paragraph is left aligned. Stretching a short last line to the
                // full width looks broken, so no typesetter does it.
                if !last_in_paragraph {
                    let gaps = slice[..visible].iter().filter(|(_, c)| is_blank(c)).count();
                    if gaps > 0 {
                        extra_per_gap = ((width - visible_width) / gaps as f32).max(0.0);
                    }
                }
                0.0
            }
        };

        // Place the clusters, grouping neighbouring clusters that share a run into one PlacedRun.
        let mut placed_runs: Vec<PlacedRun> = Vec::new();
        let mut pen = offset;
        for (run_index, cluster) in slice.iter_mut() {
            cluster.x = pen;
            if is_blank(cluster) {
                cluster.advance += extra_per_gap;
            }
            pen += cluster.advance;
            let style = buffers.runs[*run_index].1;
            // Compared by looking at the style rather than by cloning it first. Cloning allocated a
            // family name for every cluster in the document purely to throw it away again.
            let same_run = placed_runs.last().is_some_and(|last| &last.style == style);
            if same_run {
                placed_runs.last_mut().expect("checked").clusters.push(cluster.clone());
            } else {
                placed_runs
                    .push(PlacedRun { style: style.clone(), clusters: vec![cluster.clone()] });
            }
        }

        // The line is as tall as the tallest style in it. The descent is collected alongside the
        // ascent rather than being taken from `natural`, because `natural` also carries the line gap
        // and the leading, and the caret is drawn to the glyphs rather than to the line.
        let mut ascent = 0.0_f32;
        let mut descent = 0.0_f32;
        let mut natural = 0.0_f32;
        for run in &placed_runs {
            let line_metrics = metrics.line_metrics(&run.style);
            ascent = ascent.max(line_metrics.ascent);
            descent = descent.max(line_metrics.descent);
            natural = natural.max(line_metrics.height());
        }
        if placed_runs.is_empty() {
            let line_metrics = metrics.line_metrics(&empty_style);
            ascent = line_metrics.ascent;
            descent = line_metrics.descent;
            natural = line_metrics.height();
        }
        // A paragraph may ask to be at least so tall, which is how the Markdown preview leaves room
        // for a picture. It is a floor and never a ceiling: a line of large letters is as tall as its
        // letters whatever it asked for.
        let height = (natural * paragraph_style.line_spacing).max(paragraph_style.min_height);

        let start = slice.first().map(|(_, c)| c.bytes.start).unwrap_or(bytes.start);
        let end = slice.last().map(|(_, c)| c.bytes.end).unwrap_or(bytes.start);

        work.lines.push(PlacedLine {
            y: work.y,
            height,
            // Extra line spacing is added below the text rather than above it, so single and double
            // spaced paragraphs start at the same place.
            baseline: ascent,
            ascent,
            descent,
            bytes: start..end,
            paragraph,
            last_in_paragraph,
            runs: placed_runs,
            empty_style: empty_style.clone(),
        });
        work.y += height;
    }
    buffers.breaks = breaks;
    mark
}

/// True when a cluster is whitespace, which is where a line may be broken.
fn is_blank(cluster: &PlacedCluster) -> bool {
    cluster.text.chars().all(char::is_whitespace)
}

impl Layout {
    /// Which line holds a document offset. An offset on a line break belongs to the earlier line.
    ///
    /// A binary search rather than a walk: the lines are in order and their ends do not decrease, so
    /// `partition_point` finds the first whose end reaches the offset. The caret asks this every
    /// frame, so on a long file the walk was a cost that grew with the file rather than with what is
    /// on the screen.
    pub fn line_of_offset(&self, offset: usize) -> usize {
        let index = self.lines.partition_point(|line| line.bytes.end < offset);
        index.min(self.lines.len().saturating_sub(1))
    }

    /// Which line a vertical position falls on, clamped to the first and last lines so that dragging
    /// above or below the text selects to the start or the end.
    pub fn line_at_y(&self, y: f32) -> usize {
        if self.lines.is_empty() || y < 0.0 {
            return 0;
        }
        // The tops are sorted, so the bottoms are too, and the first line whose bottom edge is past
        // `y` is the one it falls on. A walk here was charged to every mouse move.
        let index = self.lines.partition_point(|line| line.bottom() <= y);
        index.min(self.lines.len() - 1)
    }

    /// The bytes of every line between two vertical positions, which is what is on the screen.
    ///
    /// A binary search at each end rather than a walk, because this is asked once a frame while
    /// painting and a long file has a great many lines above the window. The lines are laid out top
    /// to bottom, so their tops are sorted and `partition_point` can be used on them directly.
    ///
    /// An empty range when nothing is between the two, which is what a layout with nothing in it
    /// gives and what a caller should treat as "there is nothing to draw".
    pub fn visible_bytes(&self, top: f32, bottom: f32) -> Range<usize> {
        let lines = self.visible_lines(top, bottom);
        if lines.is_empty() {
            return 0..0;
        }
        self.lines[lines.start].bytes.start..self.lines[lines.end - 1].bytes.end
    }

    /// The lines between two vertical positions, which is what the painter has to draw.
    ///
    /// The same pair of binary searches [`Self::visible_bytes`] uses, giving the indices rather than
    /// the bytes. The painter used to walk every line in the document and build a textured rectangle
    /// for every glyph in the file, which on a 169 kilobyte source file was 7 ms a frame against the
    /// 0.07 ms one screenful costs. See `tasks/task-1666-performance-tdd.md` section 5.
    ///
    /// An empty range when nothing is between the two, which is what a layout with nothing in it
    /// gives and what a caller should treat as "there is nothing to draw".
    pub fn visible_lines(&self, top: f32, bottom: f32) -> Range<usize> {
        if self.lines.is_empty() || bottom <= top {
            return 0..0;
        }
        // The first line whose bottom edge is still above `top` cannot be seen, so the first that
        // can is the one after the last of those.
        let first = self.lines.partition_point(|line| line.bottom() <= top);
        let last = self.lines.partition_point(|line| line.y < bottom);
        if first >= last {
            return 0..0;
        }
        first..last
    }

    /// The document offset closest to a point, which is what a mouse click needs.
    ///
    /// Clicking in the right half of a character puts the caret after it, which is what every editor
    /// does and what a writer expects.
    pub fn offset_at(&self, x: f32, y: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let line = &self.lines[self.line_at_y(y)];
        let mut best = line.bytes.start;
        let mut best_distance = f32::INFINITY;
        let mut consider = |offset: usize, at: f32| {
            let distance = (at - x).abs();
            if distance < best_distance {
                best_distance = distance;
                best = offset;
            }
        };
        consider(line.bytes.start, line.left());
        for cluster in line.clusters() {
            consider(cluster.bytes.start, cluster.x);
            consider(cluster.bytes.end, cluster.right());
        }
        best
    }

    /// Where to draw the caret for a document offset.
    pub fn caret_at(&self, offset: usize) -> Caret {
        let index = self.line_of_offset(offset);
        let Some(line) = self.lines.get(index) else {
            return Caret { x: 0.0, y: 0.0, height: 0.0, line: 0 };
        };
        let mut x = line.left();
        for cluster in line.clusters() {
            if offset >= cluster.bytes.end {
                x = cluster.right();
            } else if offset > cluster.bytes.start {
                x = cluster.x;
            }
        }
        if offset <= line.bytes.start {
            x = line.left();
        }
        // The glyph box rather than the line box. A line carries the font's line gap, the reading
        // leading and the paragraph's line spacing on top of the letters, and a caret drawn to the
        // whole of that stands taller than the text it is sitting in — which at double spacing is
        // twice as tall as the writing.
        Caret {
            x,
            y: line.y + line.baseline - line.ascent,
            height: line.ascent + line.descent,
            line: index,
        }
    }

    /// What is at a vertical position, as something that survives being laid out again.
    ///
    /// See [`Anchor`]. An empty layout answers with the start of the document, which is the only
    /// honest answer and is where the view is anyway.
    pub fn anchor_at_y(&self, y: f32) -> Anchor {
        let Some(line) = self.lines.get(self.line_at_y(y)) else {
            return Anchor { offset: 0, fraction: 0.0 };
        };
        let fraction =
            if line.height > 0.0 { ((y - line.y) / line.height).clamp(0.0, 1.0) } else { 0.0 };
        Anchor { offset: line.bytes.start, fraction }
    }

    /// Where an anchor has ended up in this layout.
    pub fn y_of_anchor(&self, anchor: Anchor) -> f32 {
        match self.lines.get(self.line_of_anchor(anchor.offset)) {
            Some(line) => line.y + line.height * anchor.fraction,
            None => 0.0,
        }
    }

    /// Which line an anchor's offset belongs to, which is not quite the question
    /// [`Self::line_of_offset`] answers.
    ///
    /// An anchor holds the offset a line **starts** at, and where a paragraph wraps the second line
    /// starts at the byte the first one ends at. `line_of_offset` gives the earlier of the two —
    /// deliberately, because a caret sitting on a line break belongs to the line it is ending — so
    /// an anchor taken on a wrapped continuation line would come back one line too high, and the
    /// view would creep up the file a line at a time as the size was stepped. So a line that
    /// *starts* at the offset wins; anything else falls through to the ordinary answer.
    fn line_of_anchor(&self, offset: usize) -> usize {
        let index = self.lines.partition_point(|line| line.bytes.start < offset);
        match self.lines.get(index) {
            Some(line) if line.bytes.start == offset => index,
            _ => self.line_of_offset(offset),
        }
    }

    /// The rectangles to paint behind a selected range, one per line it covers.
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Rect> {
        self.selection_rects_in(0..self.lines.len(), range)
    }

    /// The same, for the lines a caller can actually see.
    ///
    /// Selecting the whole of a long file otherwise built a rectangle for every line in it on every
    /// frame, all but a screenful of them off the top or the bottom of the window.
    pub fn selection_rects_in(&self, lines: Range<usize>, range: Range<usize>) -> Vec<Rect> {
        if range.is_empty() {
            return Vec::new();
        }
        let lines = lines.start.min(self.lines.len())..lines.end.min(self.lines.len());
        let mut rects = Vec::new();
        for line in &self.lines[lines] {
            let from = range.start.max(line.bytes.start);
            let to = range.end.min(line.bytes.end);
            if from > to {
                continue;
            }
            // A line break inside the selection is shown as a small tail past the last character, so
            // that selecting several whole lines looks continuous.
            let covers_break = range.end > line.bytes.end && !line.last_in_paragraph_end();
            if from == to && !covers_break {
                continue;
            }
            let mut left = line.right();
            let mut right = line.left();
            for cluster in line.clusters() {
                if cluster.bytes.start >= from && cluster.bytes.end <= to {
                    left = left.min(cluster.x);
                    right = right.max(cluster.right());
                }
            }
            if right < left {
                left = line.right();
                right = left;
            }
            if covers_break {
                right = right.max(left) + line.height * 0.25;
            }
            if right > left {
                rects.push(Rect { x: left, y: line.y, width: right - left, height: line.height });
            }
        }
        rects
    }

    /// The offset on `line` nearest the horizontal position `x`. Vertical movement uses this with a
    /// remembered position, so that moving down through a short line and on to a long one returns to
    /// the original column.
    pub fn offset_on_line_at_x(&self, line: usize, x: f32) -> usize {
        let Some(placed) = self.lines.get(line) else {
            return 0;
        };
        self.offset_at(x, placed.y + placed.height / 2.0)
    }

    /// The rules to draw for underline and strikethrough, as rectangles with the colour to use.
    pub fn decorations(&self, metrics: &dyn FontMetrics) -> Vec<(Rect, crate::style::Color)> {
        self.decorations_in(0..self.lines.len(), metrics)
    }

    /// The same, for the lines a caller can actually see.
    pub fn decorations_in(
        &self,
        lines: Range<usize>,
        metrics: &dyn FontMetrics,
    ) -> Vec<(Rect, crate::style::Color)> {
        let lines = lines.start.min(self.lines.len())..lines.end.min(self.lines.len());
        let mut out = Vec::new();
        for line in &self.lines[lines] {
            for run in &line.runs {
                if !run.style.underline && !run.style.strikethrough {
                    continue;
                }
                let left = run.left();
                let width = run.right() - left;
                if width <= 0.0 {
                    continue;
                }
                let thickness = metrics.rule_thickness(&run.style);
                let baseline = line.y + line.baseline;
                if run.style.underline {
                    out.push((
                        Rect {
                            x: left,
                            y: baseline + metrics.underline_offset(&run.style),
                            width,
                            height: thickness,
                        },
                        run.style.color,
                    ));
                }
                if run.style.strikethrough {
                    out.push((
                        Rect {
                            x: left,
                            y: baseline - metrics.strikethrough_offset(&run.style),
                            width,
                            height: thickness,
                        },
                        run.style.color,
                    ));
                }
            }
        }
        out
    }
}

impl PlacedLine {
    /// True when the selection reaching past this line does not cross a paragraph break, meaning the
    /// line ended because of word wrap rather than because of a line break in the text.
    fn last_in_paragraph_end(&self) -> bool {
        !self.last_in_paragraph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Command, Document};
    use crate::metrics::{FixedMetrics, LineMetrics, ScaledMetrics};
    use crate::style::{Color, StyleChange};

    /// Every expected number in these tests comes from FixedMetrics: each cluster is 10 wide, the
    /// ascent is 16 and the descent is 4, so a line is 20 tall.
    fn fixture(text: &str) -> (Rope, StyleSpans, ParagraphStyles) {
        let rope = Rope::from_str(text);
        let spans = StyleSpans::new(rope.len_bytes(), CharStyle::default());
        let paragraphs = ParagraphStyles::new(rope.len_lines());
        (rope, spans, paragraphs)
    }

    fn line_texts(layout: &Layout) -> Vec<String> {
        layout
            .lines
            .iter()
            .map(|line| line.runs.iter().flat_map(|r| r.clusters.iter()).map(|c| c.text.as_str()).collect())
            .collect()
    }

    /// A document with something of everything in it: several paragraphs, one long enough to wrap,
    /// an empty one, a run of formatting in the middle and a centred paragraph.
    fn a_document_of_every_shape() -> (Rope, StyleSpans, ParagraphStyles) {
        let text = "the first paragraph\n\
                    a much longer paragraph that will certainly have to wrap more than once at this width\n\
                    \n\
                    the last paragraph";
        let rope = Rope::from_str(text);
        let mut spans = StyleSpans::new(rope.len_bytes(), CharStyle::default());
        spans.set(4..9, &StyleChange::bold(true));
        spans.set(25..40, &StyleChange::color(Color::RED));
        let mut paragraphs = ParagraphStyles::new(rope.len_lines());
        paragraphs.set(1..2, |style| style.align = Align::Center);
        paragraphs.set(3..4, |style| style.line_spacing = 2.0);
        (rope, spans, paragraphs)
    }

    /// The whole of the incremental layout rests on this: **laying out again after an edit gives
    /// exactly what laying the whole thing out from scratch gives.** Not nearly, and not to within a
    /// rounding error — the same `Layout`, fingerprints and all.
    ///
    /// Every shape of edit is tried, because the prefix and suffix match is what makes it fast and is
    /// also where it could be wrong: an edit inside a paragraph, at a paragraph boundary, one that
    /// adds a paragraph, one that takes one away, one at the very start, one at the very end, and one
    /// that replaces everything.
    #[test]
    fn relayout_agrees_with_layout_after_every_shape_of_edit() {
        let edits: Vec<(&str, Box<dyn Fn(&mut Document)>)> = vec![
            ("a letter typed in the middle", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 30, extend: false });
                d.apply(Command::Insert("X".to_owned()));
            })),
            ("a letter typed at the very start", Box::new(|d: &mut Document| {
                d.apply(Command::MoveDocumentStart { extend: false });
                d.apply(Command::Insert("X".to_owned()));
            })),
            ("a letter typed at the very end", Box::new(|d: &mut Document| {
                d.apply(Command::MoveDocumentEnd { extend: false });
                d.apply(Command::Insert("X".to_owned()));
            })),
            ("a paragraph split in two", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 8, extend: false });
                d.apply(Command::Insert("\n".to_owned()));
            })),
            ("a paragraph joined to the one before it", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 20, extend: false });
                d.apply(Command::DeleteBackward);
            })),
            ("the empty paragraph taken away", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 106, extend: false });
                d.apply(Command::PlaceCaret { offset: 107, extend: true });
                d.apply(Command::DeleteBackward);
            })),
            ("a long stretch deleted", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 5, extend: false });
                d.apply(Command::PlaceCaret { offset: 70, extend: true });
                d.apply(Command::DeleteBackward);
            })),
            ("everything replaced", Box::new(|d: &mut Document| {
                d.apply(Command::SelectAll);
                d.apply(Command::Insert("one\ntwo\nthree".to_owned()));
            })),
            ("everything deleted", Box::new(|d: &mut Document| {
                d.apply(Command::SelectAll);
                d.apply(Command::DeleteBackward);
            })),
            ("a word made bold", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 60, extend: false });
                d.apply(Command::PlaceCaret { offset: 66, extend: true });
                d.apply(Command::ToggleBold);
            })),
            ("a paragraph centred", Box::new(|d: &mut Document| {
                d.apply(Command::PlaceCaret { offset: 2, extend: false });
                d.apply(Command::SetAlign(Align::Center));
            })),
            ("the whole document coloured again", Box::new(|d: &mut Document| {
                d.set_syntax(Color::WHITE, &[(0..3, Color::BLUE), (10..14, Color::GREEN)]);
            })),
            ("the font changed everywhere", Box::new(|d: &mut Document| {
                d.set_base_style(StyleChange::size(24.0));
            })),
            ("nothing at all", Box::new(|_d: &mut Document| {})),
        ];

        for (what, edit) in edits {
            let (rope, spans, paragraphs) = a_document_of_every_shape();
            let mut document = Document::from_text(&rope.to_string());
            document.set_syntax(Color::WHITE, &[(4..9, Color::RED), (25..40, Color::BLUE)]);
            let _ = (spans, paragraphs);
            let before = layout(
                document.text(),
                document.chars(),
                document.paragraphs(),
                &FixedMetrics::default(),
                200.0,
            );
            edit(&mut document);
            let fresh = layout(
                document.text(),
                document.chars(),
                document.paragraphs(),
                &FixedMetrics::default(),
                200.0,
            );
            let incremental = relayout(
                before.clone(),
                document.text(),
                document.chars(),
                document.paragraphs(),
                &FixedMetrics::default(),
                200.0,
            );
            assert_eq!(incremental, fresh, "after {what}");
        }
    }

    /// A layout kept from a different width is not something to build on, so the whole thing is laid
    /// out again. Dragging the divider beside the editing area is what does this.
    #[test]
    fn relayout_at_another_width_lays_the_whole_thing_out_again() {
        let (rope, spans, paragraphs) = a_document_of_every_shape();
        let narrow = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 200.0);
        let wide = relayout(narrow, &rope, &spans, &paragraphs, &FixedMetrics::default(), 600.0);
        let fresh = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 600.0);
        assert_eq!(wide, fresh);
    }

    /// Metrics that count what they were asked for, so a test can see **how much** was laid out rather
    /// than only what came out of it.
    ///
    /// Comparing the answers cannot tell a document that was kept from one that was laid out again and
    /// happened to come out the same, and that is exactly the difference the incremental layout is
    /// for. One `advance` is asked for every grapheme cluster that is laid out, so counting them is
    /// counting the work.
    struct Counting {
        inner: FixedMetrics,
        advances: std::cell::Cell<usize>,
    }

    impl Counting {
        fn new() -> Self {
            Self { inner: FixedMetrics::default(), advances: std::cell::Cell::new(0) }
        }

        fn since(&self) -> usize {
            let count = self.advances.get();
            self.advances.set(0);
            count
        }
    }

    impl FontMetrics for Counting {
        fn advance(&self, cluster: &str, style: &CharStyle) -> f32 {
            self.advances.set(self.advances.get() + 1);
            self.inner.advance(cluster, style)
        }

        fn line_metrics(&self, style: &CharStyle) -> LineMetrics {
            self.inner.line_metrics(style)
        }
    }

    /// **Typing a letter lays out the paragraph it was typed into, and nothing else.**
    ///
    /// This is what the incremental layout is worth, and it is measured rather than assumed: the
    /// number of clusters measured after an edit is a handful, against the tens of thousands a full
    /// layout asks for. A `relayout` that quietly fell back to laying the whole thing out would give
    /// exactly the same picture and fail this.
    #[test]
    fn an_edit_measures_the_paragraph_it_touched_and_not_the_document() {
        let metrics = Counting::new();
        let mut document = Document::from_text(
            &(0..400).map(|i| format!("paragraph number {i}, with a few words in it\n")).collect::<String>(),
        );
        let laid = layout(
            document.text(),
            document.chars(),
            document.paragraphs(),
            &metrics,
            2000.0,
        );
        let whole = metrics.since();
        assert!(whole > 15_000, "the fixture should be big enough to matter: {whole} clusters");

        // A letter typed into the middle.
        let middle = document.text().len_bytes() / 2;
        document.apply(Command::PlaceCaret { offset: middle, extend: false });
        document.apply(Command::Insert("X".to_owned()));
        let after = relayout(
            laid,
            document.text(),
            document.chars(),
            document.paragraphs(),
            &metrics,
            2000.0,
        );
        let touched = metrics.since();
        assert!(
            touched < whole / 100,
            "one paragraph of four hundred was typed into, so nothing like {whole} clusters \
             should have been measured again: {touched} were"
        );

        // And a paragraph split in two, which changes the numbering of every paragraph below it.
        document.apply(Command::Insert("\n".to_owned()));
        let split = relayout(
            after,
            document.text(),
            document.chars(),
            document.paragraphs(),
            &metrics,
            2000.0,
        );
        let touched = metrics.since();
        assert!(
            touched < whole / 100,
            "splitting a paragraph renumbers the ones below it but lays none of them out again: \
             {touched} clusters were measured"
        );
        assert_eq!(split.lines.len(), 402, "four hundred paragraphs, one split, and the last empty one");
    }

    /// A layout that has never been laid out — the one a tab starts with — is a valid thing to hand
    /// `relayout`, and it lays the whole document out.
    #[test]
    fn relayout_from_nothing_is_a_full_layout() {
        let (rope, spans, paragraphs) = a_document_of_every_shape();
        let built =
            relayout(Layout::default(), &rope, &spans, &paragraphs, &FixedMetrics::default(), 200.0);
        let fresh = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 200.0);
        assert_eq!(built, fresh);
    }

    /// What makes it worth having: an edit to one paragraph keeps every line of every other one.
    ///
    /// The lines are compared by identity of contents rather than counted, because keeping them is
    /// the whole point and a version that quietly laid everything out again would still pass a test
    /// that only checked the answer.
    #[test]
    fn typing_into_one_paragraph_keeps_the_lines_of_the_others() {
        let mut document = Document::from_text(
            "alpha bravo charlie\ndelta echo foxtrot\ngolf hotel india\njuliett kilo lima",
        );
        let before = layout(
            document.text(),
            document.chars(),
            document.paragraphs(),
            &FixedMetrics::default(),
            2000.0,
        );
        document.apply(Command::PlaceCaret { offset: 45, extend: false });
        document.apply(Command::Insert("X".to_owned()));
        let after = relayout(
            before.clone(),
            document.text(),
            document.chars(),
            document.paragraphs(),
            &FixedMetrics::default(),
            2000.0,
        );
        assert_eq!(after.lines.len(), 4);
        assert_eq!(after.lines[0], before.lines[0], "the paragraph above is untouched");
        assert_eq!(after.lines[1], before.lines[1], "and so is the one before that");
        assert_ne!(after.lines[2], before.lines[2], "the one that was typed into is not");
        assert_eq!(after.lines[3].y, before.lines[3].y, "the one below has not moved");
        assert_eq!(
            after.lines[3].bytes,
            before.lines[3].bytes.start + 1..before.lines[3].bytes.end + 1,
            "but its bytes have"
        );
    }

    /// Only what is on the screen is asked for. This is the query the painter uses, and it is a pair
    /// of binary searches rather than a walk.
    #[test]
    fn only_the_lines_between_two_heights_are_visible() {
        let text: String = (0..500).map(|i| format!("line number {i}\n")).collect();
        let (rope, spans, paragraphs) = fixture(&text);
        let laid = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(laid.lines.len(), 501, "five hundred lines and the empty one after the last break");

        // Every line is 20 tall, so a window 700 tall shows 35 of them.
        let visible = laid.visible_lines(0.0, 700.0);
        assert_eq!(visible, 0..35);
        let scrolled = laid.visible_lines(2000.0, 2700.0);
        assert_eq!(scrolled, 100..135);
        assert!(laid.visible_lines(0.0, 0.0).is_empty());
        assert!(laid.visible_lines(1_000_000.0, 1_000_700.0).is_empty());

        // And the bytes it reports are the bytes of exactly those lines.
        assert_eq!(
            laid.visible_bytes(2000.0, 2700.0),
            laid.lines[100].bytes.start..laid.lines[134].bytes.end
        );
    }

    /// The culled queries have to give the same answer as the whole document ones for the lines they
    /// were asked about, or the picture would change when the window is scrolled.
    #[test]
    fn a_culled_query_agrees_with_the_whole_document_one() {
        let text: String = (0..200).map(|i| format!("line number {i}\n")).collect();
        let rope = Rope::from_str(&text);
        let mut spans = StyleSpans::new(rope.len_bytes(), CharStyle::default());
        spans.set(0..rope.len_bytes(), &StyleChange::underline(true));
        let paragraphs = ParagraphStyles::new(rope.len_lines());
        let laid = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);

        let all = 0..rope.len_bytes();
        let every_rect = laid.selection_rects(all.clone());
        let visible = laid.visible_lines(400.0, 1100.0);
        let some = laid.selection_rects_in(visible.clone(), all);
        assert!(!some.is_empty());
        assert!(some.len() < every_rect.len(), "there is something to be saved");
        for rect in &some {
            assert!(every_rect.contains(rect), "a culled rectangle is one of the whole set");
        }

        let every_rule = laid.decorations(&FixedMetrics::default());
        let some_rules = laid.decorations_in(visible, &FixedMetrics::default());
        assert!(!some_rules.is_empty());
        assert!(some_rules.len() < every_rule.len());
        for rule in &some_rules {
            assert!(every_rule.contains(rule));
        }
    }

    /// The cluster text holds an ordinary letter without touching the heap, which is the whole reason
    /// it is not a `String`, and it still holds a long one correctly.
    #[test]
    fn a_cluster_holds_an_ordinary_letter_without_the_heap() {
        for text in ["a", "\u{00e9}", "\t", "\u{1F600}", "e\u{0301}"] {
            let cluster = ClusterText::from(text);
            assert!(matches!(cluster, ClusterText::Inline { .. }), "{text:?} should fit inline");
            assert_eq!(cluster.as_str(), text);
            assert_eq!(cluster, text);
        }
        let long = "a".repeat(ClusterText::INLINE + 1);
        let cluster = ClusterText::from(long.as_str());
        assert!(matches!(cluster, ClusterText::Long(_)), "a cluster too long to fit spills");
        assert_eq!(cluster.as_str(), long);
        assert_eq!(ClusterText::from("ab"), ClusterText::from("ab"));
        assert_ne!(ClusterText::from("ab"), ClusterText::from("ac"));
    }

    #[test]
    fn one_short_line_is_placed_at_the_origin() {
        let (rope, spans, paragraphs) = fixture("abc");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(result.lines.len(), 1);
        let line = &result.lines[0];
        assert_eq!(line.y, 0.0);
        assert_eq!(line.height, 20.0);
        assert_eq!(line.baseline, 16.0);
        assert_eq!(line.left(), 0.0);
        assert_eq!(line.right(), 30.0, "three clusters at 10 each");
        assert_eq!(result.height, 20.0);
    }

    #[test]
    fn an_empty_document_still_has_a_line_for_the_caret() {
        let (rope, spans, paragraphs) = fixture("");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.lines.len(), 1);
        assert_eq!(result.lines[0].height, 20.0);
        assert!(result.lines[0].runs.is_empty());
        assert_eq!(result.lines[0].bytes, 0..0);
    }

    #[test]
    fn each_line_break_starts_a_new_line() {
        let (rope, spans, paragraphs) = fixture("one\ntwo\nthree");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(line_texts(&result), vec!["one", "two", "three"]);
        assert_eq!(result.lines[0].y, 0.0);
        assert_eq!(result.lines[1].y, 20.0);
        assert_eq!(result.lines[2].y, 40.0);
        assert_eq!(result.lines[1].paragraph, 1);
    }

    #[test]
    fn an_empty_paragraph_between_two_others_takes_a_full_line() {
        let (rope, spans, paragraphs) = fixture("one\n\ntwo");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(result.lines.len(), 3);
        assert_eq!(result.lines[1].height, 20.0);
        assert!(result.lines[1].runs.is_empty());
        assert_eq!(result.lines[2].y, 40.0);
    }

    #[test]
    fn a_long_paragraph_wraps_at_a_space() {
        // Width 65 fits six clusters. "the quick" would need nine, so it breaks after "the ".
        let (rope, spans, paragraphs) = fixture("the quick brown fox");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 65.0);
        let texts = line_texts(&result);
        assert!(texts.len() > 1, "it should wrap, got {texts:?}");
        for line in &result.lines {
            let trimmed = line
                .runs
                .iter()
                .flat_map(|r| r.clusters.iter())
                .filter(|c| !c.text.chars().all(char::is_whitespace))
                .map(|c| c.advance)
                .sum::<f32>();
            assert!(trimmed <= 65.0, "line wider than the width: {trimmed}");
        }
        // Every line still belongs to the one paragraph, and only the last is marked as its end.
        assert!(result.lines.iter().all(|l| l.paragraph == 0));
        assert_eq!(result.lines.iter().filter(|l| l.last_in_paragraph).count(), 1);
        // The text is not lost or reordered by wrapping.
        let rejoined: String = texts.join("");
        assert_eq!(rejoined, "the quick brown fox");
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken_rather_than_running_off_the_edge() {
        let (rope, spans, paragraphs) = fixture("abcdefghij");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 35.0);
        assert!(result.lines.len() >= 3, "a 100 wide word in a 35 wide line needs three lines");
        let rejoined: String = line_texts(&result).join("");
        assert_eq!(rejoined, "abcdefghij", "no characters lost");
    }

    #[test]
    fn alignment_moves_the_line_within_the_width() {
        let (rope, spans, mut paragraphs) = fixture("abc");
        let metrics = FixedMetrics::default();

        paragraphs.set(0..1, |p| p.align = Align::Left);
        let left = layout(&rope, &spans, &paragraphs, &metrics, 100.0);
        assert_eq!(left.lines[0].left(), 0.0);

        paragraphs.set(0..1, |p| p.align = Align::Center);
        let center = layout(&rope, &spans, &paragraphs, &metrics, 100.0);
        assert_eq!(center.lines[0].left(), 35.0, "(100 - 30) / 2");

        paragraphs.set(0..1, |p| p.align = Align::Right);
        let right = layout(&rope, &spans, &paragraphs, &metrics, 100.0);
        assert_eq!(right.lines[0].left(), 70.0, "100 - 30");
        assert_eq!(right.lines[0].right(), 100.0);
    }

    #[test]
    fn justified_text_stretches_every_line_but_the_last() {
        let (rope, spans, mut paragraphs) = fixture("aa bb cc dd ee ff");
        paragraphs.set(0..1, |p| p.align = Align::Justify);
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 90.0);
        assert!(result.lines.len() >= 2, "needs to wrap for justification to show");
        for line in &result.lines[..result.lines.len() - 1] {
            let right = line
                .runs
                .iter()
                .flat_map(|r| r.clusters.iter())
                .filter(|c| !c.text.chars().all(char::is_whitespace))
                .map(PlacedCluster::right)
                .fold(0.0_f32, f32::max);
            assert!((right - 90.0).abs() < 0.01, "a justified line should reach the full width, got {right}");
        }
        let last = result.lines.last().expect("at least one line");
        assert_eq!(last.left(), 0.0, "the last line of a justified paragraph is left aligned");
    }

    #[test]
    fn line_spacing_multiplies_the_line_height() {
        let (rope, spans, mut paragraphs) = fixture("one\ntwo");
        paragraphs.set(0..2, |p| p.line_spacing = 2.0);
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.lines[0].height, 40.0, "20 at single spacing, doubled");
        assert_eq!(result.lines[1].y, 40.0);
        assert_eq!(result.height, 80.0);
        assert_eq!(result.lines[0].baseline, 16.0, "the first baseline does not move");
    }

    #[test]
    fn a_bigger_font_makes_a_taller_line_and_wider_clusters() {
        let (rope, mut spans, paragraphs) = fixture("abcdef");
        spans.set(3..6, &StyleChange::size(32.0));
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 1000.0);
        assert_eq!(result.lines.len(), 1);
        let line = &result.lines[0];
        assert_eq!(line.runs.len(), 2, "one run at 16 point and one at 32");
        assert_eq!(line.runs[0].clusters[0].advance, 8.0, "16 point is 8 wide");
        assert_eq!(line.runs[1].clusters[0].advance, 16.0, "32 point is 16 wide");
        assert_eq!(line.height, 40.0, "the tallest run sets the line height: 32 * 1.25");
        assert_eq!(line.baseline, 32.0, "the baseline follows the tallest ascent");
    }

    #[test]
    fn one_style_change_produces_one_run_per_style() {
        let (rope, mut spans, paragraphs) = fixture("plain bold plain");
        spans.set(6..10, &StyleChange::bold(true));
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        let line = &result.lines[0];
        assert_eq!(line.runs.len(), 3);
        assert!(!line.runs[0].style.bold);
        assert!(line.runs[1].style.bold);
        assert!(!line.runs[2].style.bold);
        let bold: String = line.runs[1].clusters.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(bold, "bold");
        assert_eq!(line.runs[1].left(), 60.0, "six clusters before it");
    }

    #[test]
    fn clicking_in_the_right_half_of_a_character_puts_the_caret_after_it() {
        let (rope, spans, paragraphs) = fixture("abc");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.offset_at(0.0, 10.0), 0);
        assert_eq!(result.offset_at(3.0, 10.0), 0, "left of the middle of 'a'");
        assert_eq!(result.offset_at(7.0, 10.0), 1, "right of the middle of 'a'");
        assert_eq!(result.offset_at(14.0, 10.0), 1);
        assert_eq!(result.offset_at(1000.0, 10.0), 3, "past the end of the line");
    }

    #[test]
    fn clicking_below_the_text_lands_on_the_last_line() {
        let (rope, spans, paragraphs) = fixture("one\ntwo");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.offset_at(0.0, 25.0), 4, "the start of the second line");
        assert_eq!(result.offset_at(0.0, 900.0), 4, "clamped to the last line");
        assert_eq!(result.offset_at(0.0, -50.0), 0, "clamped to the first line");
    }

    #[test]
    fn the_caret_sits_where_the_offset_is() {
        let (rope, spans, paragraphs) = fixture("one\ntwo");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert_eq!(result.caret_at(0).x, 0.0);
        assert_eq!(result.caret_at(2).x, 20.0);
        assert_eq!(result.caret_at(3).x, 30.0, "end of the first line");
        let second = result.caret_at(4);
        assert_eq!(second.x, 0.0);
        assert_eq!(second.y, 20.0, "start of the second line");
        assert_eq!(second.line, 1);
    }

    #[test]
    fn a_caret_is_the_height_of_the_text_and_not_of_the_line_box() {
        // `ScaledMetrics` gives a line a quarter more height than the letters occupy, and double
        // spacing doubles that again. The caret must stay on the letters: at double spacing it used
        // to be two and a half times the height of the text it was sitting in.
        let (rope, spans, mut paragraphs) = fixture("one\ntwo");
        paragraphs.set(0..2, |p| p.line_spacing = 2.0);
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 500.0);
        let line = &result.lines[0];
        assert_eq!(line.height, 40.0, "16 point at 1.25 leading, doubled");
        let caret = result.caret_at(0);
        assert_eq!(caret.height, 20.0, "the ascent and the descent, and nothing else");
        assert_eq!(caret.y, 0.0, "which starts at the top of the letters");
        assert!(caret.height < line.height, "a caret must not be taller than its line");
    }

    #[test]
    fn a_caret_on_a_line_of_two_sizes_takes_the_taller() {
        let (rope, mut spans, paragraphs) = fixture("small BIG");
        spans.set(6..9, &StyleChange::size(32.0));
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 1000.0);
        let caret = result.caret_at(0);
        assert_eq!(caret.height, 40.0, "32 point: an ascent of 32 and a descent of 8");
        assert_eq!(caret.y, 0.0, "measured up from the line's own baseline");
    }

    #[test]
    fn a_caret_on_an_empty_line_sits_where_the_text_would() {
        // An empty paragraph centres its baseline in the line, so extra spacing shows above the
        // caret as well as below it. The caret is still only as tall as a letter would be.
        let (rope, spans, mut paragraphs) = fixture("one\n\ntwo");
        paragraphs.set(0..3, |p| p.line_spacing = 2.0);
        let result = layout(&rope, &spans, &paragraphs, &ScaledMetrics, 500.0);
        let empty = result.caret_at(4);
        assert_eq!(empty.line, 1, "the blank line between the two words");
        assert_eq!(empty.height, 20.0);
        assert_eq!(empty.y, 50.0, "the second line starts at 40 and the extra 20 is shared");
    }

    #[test]
    fn a_selection_within_one_line_is_one_rectangle() {
        let (rope, spans, paragraphs) = fixture("abcdef");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let rects = result.selection_rects(2..4);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, 20.0);
        assert_eq!(rects[0].width, 20.0);
        assert_eq!(rects[0].height, 20.0);
    }

    #[test]
    fn a_selection_across_lines_is_one_rectangle_per_line() {
        let (rope, spans, paragraphs) = fixture("one\ntwo\nthree");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let rects = result.selection_rects(1..10);
        assert_eq!(rects.len(), 3, "part of the first line, all of the second, part of the third");
        assert_eq!(rects[0].y, 0.0);
        assert_eq!(rects[1].y, 20.0);
        assert_eq!(rects[2].y, 40.0);
    }

    #[test]
    fn an_empty_selection_paints_nothing() {
        let (rope, spans, paragraphs) = fixture("abc");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        assert!(result.selection_rects(2..2).is_empty());
    }

    #[test]
    fn underline_and_strikethrough_become_rules_under_and_through_the_run() {
        let (rope, mut spans, paragraphs) = fixture("plain marked");
        spans.set(6..12, &StyleChange::underline(true));
        spans.set(6..12, &StyleChange::strikethrough(true));
        spans.set(6..12, &StyleChange::color(Color::RED));
        let metrics = FixedMetrics::default();
        let result = layout(&rope, &spans, &paragraphs, &metrics, 1000.0);
        let rules = result.decorations(&metrics);
        assert_eq!(rules.len(), 2, "one underline and one strikethrough");
        for (rect, color) in &rules {
            assert_eq!(rect.x, 60.0, "the rules start where the marked run starts");
            assert_eq!(rect.width, 60.0, "six clusters wide");
            assert_eq!(*color, Color::RED, "the rule takes the colour of the text");
        }
        let baseline = result.lines[0].y + result.lines[0].baseline;
        let underline = rules.iter().map(|(r, _)| r.y).fold(f32::MIN, f32::max);
        let strike = rules.iter().map(|(r, _)| r.y).fold(f32::MAX, f32::min);
        assert!(underline > baseline, "the underline sits below the baseline");
        assert!(strike < baseline, "the strikethrough sits above the baseline");
    }

    #[test]
    fn plain_text_has_no_rules_to_draw() {
        let (rope, spans, paragraphs) = fixture("nothing marked here");
        let metrics = FixedMetrics::default();
        let result = layout(&rope, &spans, &paragraphs, &metrics, 1000.0);
        assert!(result.decorations(&metrics).is_empty());
    }

    #[test]
    fn accented_letters_are_one_cluster_wide_not_one_byte_wide() {
        // "é" as a letter plus a combining accent is three bytes and one cluster.
        let (rope, spans, paragraphs) = fixture("e\u{0301}x");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let clusters: Vec<&PlacedCluster> =
            result.lines[0].runs.iter().flat_map(|r| r.clusters.iter()).collect();
        assert_eq!(clusters.len(), 2, "two clusters, not four bytes");
        assert_eq!(clusters[0].bytes, 0..3);
        assert_eq!(clusters[1].x, 10.0);
    }

    #[test]
    fn a_paragraph_can_ask_to_be_at_least_so_tall_which_is_how_a_picture_gets_its_room() {
        let (rope, spans, mut paragraphs) = fixture("above\n\nbelow");
        paragraphs.set(1..2, |style| style.min_height = 240.0);
        let laid = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 400.0);
        assert_eq!(laid.lines.len(), 3);
        assert_eq!(laid.lines[1].height, 240.0, "the empty line holding the picture");
        assert!(laid.lines[0].height < 240.0, "and no other line moved");
        assert_eq!(laid.lines[2].y, laid.lines[1].y + 240.0, "what is under it is pushed down");
    }

    #[test]
    fn asking_for_a_height_shorter_than_the_letters_changes_nothing() {
        let (rope, spans, mut paragraphs) = fixture("words");
        let natural = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 400.0).lines[0].height;
        paragraphs.set(0..1, |style| style.min_height = 1.0);
        let asked = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 400.0).lines[0].height;
        assert_eq!(asked, natural, "it is a floor, never a ceiling");
    }

    #[test]
    fn only_the_lines_between_two_heights_are_reported_as_visible() {
        // Ten lines of three letters, each 20 tall by FixedMetrics, so line n covers y 20n to 20n+20
        // and holds bytes 4n to 4n+3 with the line break after it.
        let (rope, spans, paragraphs) = fixture("abc
".repeat(10).trim_end());
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(result.lines.len(), 10);

        let whole = result.visible_bytes(0.0, 200.0);
        assert_eq!(whole, 0..rope.len_bytes(), "the whole document is on the screen");

        let middle = result.visible_bytes(40.0, 80.0);
        assert_eq!(middle, 8..15, "the third and fourth lines and nothing else");

        // A window scrolled past the end, and one above the start.
        assert_eq!(result.visible_bytes(400.0, 600.0), 0..0);
        assert_eq!(result.visible_bytes(-100.0, -50.0), 0..0);
        assert_eq!(result.visible_bytes(100.0, 100.0), 0..0, "no height is nothing to draw");
    }

    #[test]
    fn an_anchor_says_which_line_was_being_looked_at_and_where_in_it() {
        // Ten lines of three letters, 20 tall each by FixedMetrics, holding bytes 4n to 4n+3.
        let (rope, spans, paragraphs) = fixture(&"abc
".repeat(10).trim_end());
        let laid = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);

        let top = laid.anchor_at_y(0.0);
        assert_eq!(top, Anchor { offset: 0, fraction: 0.0 });

        let fifth = laid.anchor_at_y(85.0);
        assert_eq!(fifth, Anchor { offset: 16, fraction: 0.25 }, "a quarter down the fifth line");

        // Put back into the layout it came from, an anchor gives the position it was taken at.
        assert_eq!(laid.y_of_anchor(fifth), 85.0);
        assert_eq!(laid.y_of_anchor(top), 0.0);
    }

    #[test]
    fn an_anchor_holds_the_same_text_still_when_the_font_changes() {
        // The same document at two sizes, which is what zooming does. `ScaledMetrics` makes a line
        // 1.25 times the font size tall, so nothing about these numbers is guessed at.
        let text = "the first line
the second line
the third line
the fourth line";
        let rope = Rope::from_str(text);
        let paragraphs = ParagraphStyles::new(rope.len_lines());
        let at = |size: f32| {
            let spans = StyleSpans::new(
                rope.len_bytes(),
                CharStyle { size, ..CharStyle::default() },
            );
            layout(&rope, &spans, &paragraphs, &ScaledMetrics, 1000.0)
        };
        let small = at(12.0);
        let large = at(24.0);

        // The reader is looking at the third line, a third of the way down it, and it is 40 points
        // below the top of the window.
        let anchor = small.anchor_at_y(small.lines[2].y + 5.0);
        assert_eq!(anchor.offset, small.lines[2].bytes.start);

        let was = small.y_of_anchor(anchor);
        let now = large.y_of_anchor(anchor);
        assert!(now > was, "the same line is further down the page in the larger font");
        // Scrolling by the difference is what keeps it where it was, and it is the same line.
        assert_eq!(large.line_of_offset(anchor.offset), 2);
        assert!((now - (large.lines[2].y + 5.0 * 2.0)).abs() < 0.01, "a third of the way down it");
    }

    #[test]
    fn an_anchor_on_a_wrapped_line_lands_on_whichever_line_now_holds_it() {
        // One paragraph that wraps, laid out at two widths: the anchor is a byte offset, so the
        // line it names is worked out again rather than remembered.
        let (rope, spans, paragraphs) = fixture("one two three four five six seven eight");
        let wide = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        let narrow = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 100.0);
        assert_eq!(wide.lines.len(), 1);
        assert!(narrow.lines.len() > 1, "it wraps at this width");

        let anchor = wide.anchor_at_y(0.0);
        assert_eq!(narrow.y_of_anchor(anchor), 0.0, "the first line either way");

        // A point taken from the middle of the wrapped layout is put back on the line holding it,
        // and not on the one before it: a wrapped line can start at the byte the line above ends
        // at, which is the case `line_of_anchor` exists for.
        let middle = narrow.anchor_at_y(narrow.lines[1].y);
        assert_eq!(narrow.y_of_anchor(middle), narrow.lines[1].y, "the second line, not the first");
        assert_eq!(wide.y_of_anchor(middle), 0.0, "it is all one line at the wider size");

        // And an empty paragraph, whose line starts where it ends, is still a line of its own.
        let (rope, spans, paragraphs) = fixture("abc

def");
        let laid = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        let blank = laid.anchor_at_y(laid.lines[1].y);
        assert_eq!(laid.y_of_anchor(blank), laid.lines[1].y, "the empty line between the two");
    }

    #[test]
    fn an_anchor_taken_from_nothing_is_the_start_of_the_document() {
        let (rope, spans, paragraphs) = fixture("");
        let laid = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 500.0);
        let empty = Layout::default();
        assert_eq!(empty.anchor_at_y(120.0), Anchor { offset: 0, fraction: 0.0 });
        assert_eq!(empty.y_of_anchor(Anchor { offset: 900, fraction: 0.5 }), 0.0);
        // An offset past the end of a real document is its last line rather than nothing at all.
        assert_eq!(laid.y_of_anchor(Anchor { offset: 900, fraction: 0.0 }), 0.0);
    }

    #[test]
    fn nothing_laid_out_is_nothing_visible() {
        let (rope, spans, paragraphs) = fixture("");
        let result = layout(&rope, &spans, &paragraphs, &FixedMetrics::default(), 1000.0);
        assert_eq!(result.visible_bytes(0.0, 100.0).len(), 0);
    }
}
