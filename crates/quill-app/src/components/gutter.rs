//! The strip down the left of the editing area: the line numbers, the change bars, and the column
//! that annotates each line with git blame.
//!
//! It draws from the `Layout` rather than from the text, because it has to line up with rows on
//! screen and only the layout knows where those are. That also settles what a line number counts.
//! Quill wraps, so one paragraph is several `PlacedLine`s; a number is drawn against a visual line
//! only when its paragraph differs from the line above it, so a wrapped paragraph carries one number
//! against its first row and nothing against its continuations. That is what a line number means in
//! every other editor, and counting rows on screen instead would make the numbers change when the
//! window is made narrower.
//!
//! The 12 point gap to the right of the numbers is where the folding arrows go. It was left empty
//! for exactly that when this file was written, and `task-1686` spent it: the gutter is the same
//! width with folding as it was without, so the text did not move a point when the arrows arrived.
//! Right clicking anywhere in the gutter still opens its menu.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Vec2};
use quill_core::Layout;

use crate::theme::{color, icon};

/// The empty strip between the numbers and the text.
pub const GAP: f32 = 12.0;
/// Space either side of the number column.
const NUMBER_MARGIN: f32 = 8.0;
/// How wide the blame column is when it is showing. Enough for `12/31/2026  Firstname`, measured
/// against the longest date and a nine letter name, and no wider: the column takes room the text
/// would rather have.
const BLAME_WIDTH: f32 = 118.0;
/// The stripe marking a line that differs from the version in git.
const CHANGE_BAR: f32 = 3.0;
/// The size the numbers are set at.
const NUMBER_SIZE: f32 = 11.5;
/// The size the blame column is set at.
const BLAME_SIZE: f32 = 10.5;
/// How wide the square a folding arrow is drawn and clicked in is. The whole of [`GAP`], so the
/// target is as large as the space allows — a five point arrow with a five point target is a control
/// nobody can hit.
const ARROW: f32 = GAP;

/// One line's worth of blame, as the gutter draws it.
///
/// Deliberately not `quill_git::BlameLine`: a component draws and does not know where its text came
/// from, and this way the gutter can be tested with three rows written by hand.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameRow {
    /// The commit's date, already formatted, because formatting a date is not drawing.
    pub date: String,
    pub author: String,
    /// The full hash, so a click can ask for that commit.
    pub commit: String,
    /// What the tint follows: 0.0 for the oldest commit in the file, 1.0 for the newest.
    pub age: f32,
    /// The whole commit, for the tooltip.
    pub summary: String,
}

/// How a line differs from the version git has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Modified,
}

impl Change {
    fn color(self) -> Color32 {
        match self {
            Change::Added => color::GIT_ADDED,
            Change::Modified => color::GIT_MODIFIED,
        }
    }
}

/// What the gutter is showing. Borrowed rather than owned, because it is rebuilt every frame from
/// the state of the tab that is open.
#[derive(Debug, Default, Clone, Copy)]
pub struct Gutter<'a> {
    pub numbers: bool,
    /// One row per paragraph, when the file has been annotated.
    pub blame: Option<&'a [BlameRow]>,
    /// Which paragraphs differ from the version git has, in order.
    pub changes: &'a [(usize, Change)],
    /// Which paragraphs head something that can be collapsed, and whether it is collapsed, sorted
    /// by paragraph.
    ///
    /// The gutter is told rather than asked: it has no text, no grammar and no idea what a block
    /// is, which is the rule every component in Quill follows. `quill_core::folding` works it out
    /// and the window hands the answer down.
    pub folds: &'a [(usize, bool)],
}

impl Gutter<'_> {
    /// True when there is anything to draw at all, which is what decides whether the editing area
    /// gives up any width.
    pub fn showing(&self) -> bool {
        self.numbers || self.blame.is_some() || !self.changes.is_empty() || !self.folds.is_empty()
    }

    /// Whether this paragraph heads a region, and whether that region is collapsed.
    fn fold_at(&self, paragraph: usize) -> Option<bool> {
        self.folds
            .binary_search_by_key(&paragraph, |(at, _)| *at)
            .ok()
            .map(|index| self.folds[index].1)
    }
}

/// What the user did in the gutter.
#[derive(Debug, Default, PartialEq)]
pub struct GutterOutcome {
    /// The gutter was right clicked, at this position.
    pub context_menu: Option<Pos2>,
    /// A blame row was clicked, so the window should show that commit.
    pub show_commit: Option<String>,
    /// A folding arrow was pressed, so the region headed by this paragraph should be collapsed or
    /// expanded. The component decides nothing about which.
    pub toggle_fold: Option<usize>,
}

/// How many digits the largest line number takes.
fn digits(lines: usize) -> usize {
    let mut count = 1;
    let mut value = lines.max(1);
    while value >= 10 {
        value /= 10;
        count += 1;
    }
    count
}

/// How wide the gutter is, which the window needs before it can lay the editing area out.
///
/// The number column is sized for the largest line number the file has rather than for the largest
/// on screen, so the text does not shift sideways as the file is scrolled past line 99.
pub fn width(ui: &egui::Ui, gutter: &Gutter, lines: usize) -> f32 {
    if !gutter.showing() {
        return 0.0;
    }
    let mut width = CHANGE_BAR + 2.0 + GAP;
    if gutter.numbers {
        let font = egui::FontId::monospace(NUMBER_SIZE);
        let digit = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        width += digit * digits(lines) as f32 + NUMBER_MARGIN * 2.0;
    }
    if gutter.blame.is_some() {
        width += BLAME_WIDTH;
    }
    width
}

/// Draw the gutter into `area`.
///
/// `top` is where the first line of the layout sits on screen, which is the same origin the text is
/// painted from, so the numbers cannot drift away from the lines they belong to. `caret_line` is the
/// paragraph the caret is in, which is drawn brighter.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    gutter: &Gutter,
    layout: &Layout,
    top: f32,
    caret_line: usize,
) -> GutterOutcome {
    let mut outcome = GutterOutcome::default();
    if !gutter.showing() {
        return outcome;
    }
    let response = ui.interact(area, ui.id().with("gutter"), Sense::click());
    if response.secondary_clicked() {
        outcome.context_menu = response.interact_pointer_pos().or_else(|| response.hover_pos());
    }

    let mut pen = area.left();
    let blame_rect = gutter.blame.map(|_| {
        let rect = Rect::from_min_size(Pos2::new(pen, area.top()), Vec2::new(BLAME_WIDTH, area.height()));
        pen += BLAME_WIDTH;
        rect
    });
    let numbers_rect = gutter.numbers.then(|| {
        let width = area.right() - pen - GAP - CHANGE_BAR - 2.0;
        let rect = Rect::from_min_size(Pos2::new(pen, area.top()), Vec2::new(width, area.height()));
        pen += width;
        rect
    });
    let change_x = area.right() - CHANGE_BAR - 2.0;

    // Clipped to the gutter, so a line scrolled above the editing area does not paint over the
    // toolbar.
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(area));
    inner.set_clip_rect(ui.painter().clip_rect().intersect(area));

    let mut previous: Option<usize> = None;
    for line in &layout.lines {
        let y = top + line.y;
        if y + line.height < area.top() || y > area.bottom() {
            previous = Some(line.paragraph);
            continue;
        }
        let first_row = previous != Some(line.paragraph);
        previous = Some(line.paragraph);
        let row = Rect::from_min_size(
            Pos2::new(area.left(), y),
            Vec2::new(area.width(), line.height),
        );
        if let (Some(rect), true) = (blame_rect, first_row) {
            draw_blame(&mut inner, rect, row, gutter.blame, line.paragraph, &mut outcome);
        }
        if let (Some(rect), true) = (numbers_rect, first_row) {
            draw_number(&inner, rect, row, line.paragraph + 1, line.paragraph == caret_line);
        }
        if let (Some(collapsed), true) = (gutter.fold_at(line.paragraph), first_row) {
            let centre = Pos2::new(change_x - ARROW / 2.0, y + line.height / 2.0);
            if draw_arrow(&mut inner, centre, line.paragraph, collapsed) {
                outcome.toggle_fold = Some(line.paragraph);
            }
        }
        if let Some((_, change)) = gutter.changes.iter().find(|(at, _)| *at == line.paragraph) {
            inner.painter().rect_filled(
                Rect::from_min_size(Pos2::new(change_x, y), Vec2::new(CHANGE_BAR, line.height)),
                CornerRadius::same(1),
                change.color(),
            );
        }
    }
    outcome
}

/// The folding arrow against one line: down while the block is showing, right while it is
/// collapsed.
///
/// Drawn rather than lettered, which is what `design/style-guide.md` asks for and what the
/// explorer's own disclosure triangles already are — and it is the same shape, so a triangle means
/// the same thing in both places. A collapsed block's arrow is never faint: it is the only thing on
/// the screen saying that a stretch of the file is missing.
fn draw_arrow(ui: &mut egui::Ui, centre: Pos2, paragraph: usize, collapsed: bool) -> bool {
    let area = Rect::from_center_size(centre, Vec2::splat(ARROW));
    let name = if collapsed {
        format!("Expand block at line {}", paragraph + 1)
    } else {
        format!("Collapse block at line {}", paragraph + 1)
    };
    let response = ui.interact(area, ui.id().with(("fold", paragraph)), Sense::click());
    let tint = if collapsed || response.hovered() { color::TEXT_CONTROL } else { color::TEXT_FAINT };
    icon::disclosure(ui.painter(), centre, !collapsed, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, collapsed, &name)
    });
    response.clicked()
}

/// One line number, right aligned in its column.
fn draw_number(ui: &egui::Ui, column: Rect, row: Rect, number: usize, current: bool) {
    let tint = if current { color::TEXT_CONTROL } else { color::TEXT_FAINT };
    let galley = ui.painter().layout_no_wrap(
        number.to_string(),
        egui::FontId::monospace(NUMBER_SIZE),
        tint,
    );
    ui.painter().galley(
        Pos2::new(
            column.right() - NUMBER_MARGIN - galley.size().x,
            row.top() + (row.height() - galley.size().y) / 2.0,
        ),
        galley,
        tint,
    );
}

/// One row of the blame column: a tinted background, the date, and the author.
///
/// The tint runs from `BLAME_OLD` for the oldest commit in the file to `BLAME_NEW` for the newest,
/// by rank rather than by date, so a file whose history is one recent burst and one ancient commit
/// still reads as a gradient rather than as two colours.
fn draw_blame(
    ui: &mut egui::Ui,
    column: Rect,
    row: Rect,
    blame: Option<&[BlameRow]>,
    paragraph: usize,
    outcome: &mut GutterOutcome,
) {
    let Some(entry) = blame.and_then(|rows| rows.get(paragraph)) else {
        return;
    };
    let cell = Rect::from_min_size(
        Pos2::new(column.left(), row.top()),
        Vec2::new(column.width() - 4.0, row.height()),
    );
    let tint = mix(color::BLAME_OLD, color::BLAME_NEW, entry.age);
    ui.painter().rect_filled(cell, CornerRadius::ZERO, tint);

    let name = format!("Blame: {} {}", entry.date, entry.author);
    let response = ui
        .interact(cell, ui.id().with(("blame", paragraph)), Sense::click())
        .on_hover_text(format!("{}\n{} \u{00B7} {}", entry.summary, entry.author, entry.date));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    if response.clicked() {
        outcome.show_commit = Some(entry.commit.clone());
    }

    let font = egui::FontId::proportional(BLAME_SIZE);
    let date = ui.painter().layout_no_wrap(entry.date.clone(), font.clone(), color::TEXT_STRONG);
    let y = row.top() + (row.height() - date.size().y) / 2.0;
    ui.painter().galley(Pos2::new(cell.left() + 6.0, y), date.clone(), color::TEXT_STRONG);
    let author = ui.painter().layout_no_wrap(entry.author.clone(), font, color::TEXT_STRONG);
    ui.painter().galley(
        Pos2::new(cell.left() + 12.0 + date.size().x, y),
        author,
        color::TEXT_STRONG,
    );
}

/// Blend two colours, which is how the blame tint follows a commit's age.
fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Color32::from_rgb(blend(from.r(), to.r()), blend(from.g(), to.g()), blend(from.b(), to.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_number_column_is_sized_for_the_largest_line_number() {
        assert_eq!(digits(1), 1);
        assert_eq!(digits(9), 1);
        assert_eq!(digits(10), 2);
        assert_eq!(digits(99), 2);
        assert_eq!(digits(100), 3);
        assert_eq!(digits(1234), 4);
        // An empty document still has a line one, so it gets a column rather than none.
        assert_eq!(digits(0), 1);
    }

    #[test]
    fn a_gutter_showing_nothing_takes_no_width() {
        let gutter = Gutter { numbers: false, blame: None, changes: &[], folds: &[] };
        assert!(!gutter.showing());
    }

    #[test]
    fn a_change_bar_alone_is_enough_to_show_the_gutter() {
        let changes = [(3, Change::Modified)];
        let gutter = Gutter { numbers: false, blame: None, changes: &changes, folds: &[] };
        assert!(gutter.showing(), "a file with changes shows its change bars even with numbers off");
    }

    #[test]
    fn a_folding_arrow_alone_is_enough_to_show_the_gutter() {
        let folds = [(4usize, false)];
        let gutter = Gutter { numbers: false, blame: None, changes: &[], folds: &folds };
        assert!(gutter.showing(), "a file with something to fold shows the arrows");
        assert_eq!(gutter.fold_at(4), Some(false));
        assert_eq!(gutter.fold_at(3), None, "no region is headed by that line");
    }

    /// The arrows go in the gap that was left for them, so the gutter is exactly as wide with
    /// folding as it was without — which is why no accepted screenshot moved sideways.
    #[test]
    fn the_arrows_take_no_width_of_their_own() {
        assert_eq!(ARROW, GAP);
    }

    #[test]
    fn the_blame_tint_runs_from_the_oldest_colour_to_the_newest() {
        assert_eq!(mix(color::BLAME_OLD, color::BLAME_NEW, 0.0), color::BLAME_OLD);
        assert_eq!(mix(color::BLAME_OLD, color::BLAME_NEW, 1.0), color::BLAME_NEW);
        let middle = mix(color::BLAME_OLD, color::BLAME_NEW, 0.5);
        assert!(middle.r() > color::BLAME_OLD.r() && middle.r() < color::BLAME_NEW.r());
        // Out of range is clamped rather than producing a colour that is not on the line.
        assert_eq!(mix(color::BLAME_OLD, color::BLAME_NEW, 2.0), color::BLAME_NEW);
    }
}
