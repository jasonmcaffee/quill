//! The strip down the left of the editing area: the breakpoint dots, the line numbers, the folding
//! arrows, the change bars, and the column that annotates each line with git blame.
//!
//! It draws from the `Layout` rather than from the text, because it has to line up with rows on
//! screen and only the layout knows where those are. That also settles what a line number counts.
//! Unluminous wraps, so one paragraph is several `PlacedLine`s; a number is drawn against a visual line
//! only when its paragraph differs from the line above it, so a wrapped paragraph carries one number
//! against its first row and nothing against its continuations. That is what a line number means in
//! every other editor, and counting rows on screen instead would make the numbers change when the
//! window is made narrower.
//!
//! The 12 point gap to the right of the numbers is where the folding arrows go. It was left empty
//! for exactly that when this file was written, and `task-1686` spent it: the gutter is the same
//! width with folding as it was without, so the text did not move a point when the arrows arrived.
//! Right clicking anywhere in the gutter still opens its menu.
//!
//! `task-1687` then wanted a breakpoint column, and the gap was gone. So the dot is drawn **over the
//! line number** — which is what the reference editor itself does, and which costs the gutter nothing: the text
//! does not move, no accepted screenshot shifts sideways, and only a line that really has a
//! breakpoint looks any different. The number gives way rather than being drawn round the dot,
//! because a red circle with a numeral showing through it reads as neither. With the numbers
//! switched off there is nothing to draw over, so a column of [`BREAKPOINT_COLUMN`] points is
//! reserved in that configuration — and reserved **whether or not anything is set**, so the first
//! breakpoint never moves the text under the pointer.
//!
//! A **left click** in that column toggles one, which is new behaviour: until now the gutter took
//! only `secondary_clicked` over the whole of itself, so nothing was taken away from anything. It is
//! taken per row, the way the blame cell already takes one.
//!
//! ## A mark is centred on the letters, not on the line
//!
//! `task-1693` reported that zooming left the numbers and the dots out of line with the text, and
//! the arithmetic says why. A `PlacedLine` is taller than the glyphs in it: the baseline sits
//! `ascent` from the top and **all** the extra leading is added below, which is what makes single
//! and double spaced paragraphs start at the same place. `text_renderer::READING_LEADING` alone is
//! `0.45` of the point size, so the bottom of every line box is empty air that grows with the zoom.
//! Centring a mark in that box put it low by about a fifth of the point size — three points at the
//! default sixteen, thirty at the hundred and forty-four the settings allow.
//!
//! So every mark takes its centre from [`text_band`], which is the box the letters really occupy and
//! is the same box `layout.rs` says the caret is drawn to. Two things deliberately keep the whole
//! line: the change bar, which marks a line rather than its letters and has to meet the bar above it
//! with no gap, and the blame cell's background, for the same reason.
//!
//! The numbers and the blame column are also **set at a size that follows the editor's**, because a
//! number a third the height of the line it counts reads as a mistake however well it is aligned.
//! Both are ratios of `settings::DEFAULT_FONT_SIZE`, so at the default they are exactly the 11.5 and
//! 10.5 points the gutter has always used and only the alignment changes.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Vec2};
use unluminous_core::Layout;

use crate::theme::{color, icon};

/// The empty strip between the numbers and the text.
pub const GAP: f32 = 12.0;
/// Space either side of the number column.
const NUMBER_MARGIN: f32 = 8.0;
/// How wide the blame column is when it is showing, at the default font size. Enough for
/// `12/31/2026  Firstname`, measured against the longest date and a nine letter name, and no wider:
/// the column takes room the text would rather have.
///
/// It is a measurement of the type rather than of the column, so it follows [`BLAME_RATIO`] — see
/// [`blame_width`].
const BLAME_WIDTH: f32 = 118.0;
/// The stripe marking a line that differs from the version in git.
const CHANGE_BAR: f32 = 3.0;
/// The size the numbers are set at, as a fraction of the editor's own font size.
///
/// Written as a ratio of the default rather than as a new constant, so that a window at the default
/// size sets its numbers at exactly the 11.5 points it always has and no accepted screenshot changes
/// its type. See [`number_size`].
const NUMBER_RATIO: f32 = 11.5 / crate::settings::DEFAULT_FONT_SIZE;
/// The size the blame column is set at, on the same terms.
const BLAME_RATIO: f32 = 10.5 / crate::settings::DEFAULT_FONT_SIZE;
/// The smallest and largest the gutter's own type is allowed to get.
///
/// Six point text still needs a gutter somebody can read, and a hundred and forty-four point text
/// must not have a gutter wider than the editing area beside it.
const SMALLEST_TYPE: f32 = 9.0;
const LARGEST_TYPE: f32 = 28.0;
/// How wide the square a folding arrow is drawn and clicked in is. The whole of [`GAP`], so the
/// target is as large as the space allows — a five point arrow with a five point target is a control
/// nobody can hit.
const ARROW: f32 = GAP;
/// How wide the column the breakpoint dot is drawn in is, **when the line numbers are switched off**.
///
/// With them on the dot is drawn **over the number**, which is what the reference editor does and what costs the
/// gutter nothing: the text does not move a point, no accepted screenshot shifts sideways, and only
/// a line that really has a breakpoint looks any different. The 12 points `GAP` reserves — which
/// §6.2 of the design names — were spent by `task-1686` on the folding arrows, and a second control
/// cannot share twelve points with one that already fills them.
///
/// It is added **whenever the numbers are off**, whether or not anything is set, rather than when the
/// first breakpoint appears: a column that arrived with the first dot would move the text sideways
/// under the pointer, which is the fault `task-1658` moved the text tools into the title bar to stop.
const BREAKPOINT_COLUMN: f32 = 14.0;

/// One line's worth of blame, as the gutter draws it.
///
/// Deliberately not `unluminous_git::BlameLine`: a component draws and does not know where its text came
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
            Change::Added => color::git_added(),
            Change::Modified => color::git_modified(),
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
    /// is, which is the rule every component in Unluminous follows. `unluminous_core::folding` works it out
    /// and the window hands the answer down.
    pub folds: &'a [(usize, bool)],
    /// Which paragraphs have a breakpoint on them, and how each is drawn, sorted by paragraph.
    ///
    /// Told rather than asked, exactly as the folds are: this file knows nothing about offsets, about
    /// what a debugger is, or about whether one is running. The window turns the document's
    /// breakpoints and the adapter's answers into this list.
    pub breakpoints: &'a [(usize, BreakpointMark)],
    /// True when this file's language names a debugger at all, which is what decides whether a click
    /// in the gutter can put a breakpoint anywhere.
    ///
    /// **Absent rather than dimmed**, which is Unluminous's rule for a control that can never apply: a
    /// stylesheet has nothing to step through and never will, so clicking its gutter does nothing at
    /// all rather than making a dot no debugger would ever honour.
    pub can_debug: bool,
    /// The paragraph the program is stopped on, when it is stopped in this file. Drawn as an arrow
    /// over the breakpoint column, which is the reference editor's own mark.
    pub execution_point: Option<usize>,
    /// The size the editor sets its text at, which is what the gutter's own type follows.
    ///
    /// Zero from `Default`, which reads as the default size — a `Gutter` built by a test that does
    /// not care about type sizes gets the sizes the gutter has always used.
    pub font_size: f32,
}

/// The size the numbers are set at for an editor set at `font_size`.
fn number_size(font_size: f32) -> f32 {
    type_size(font_size, NUMBER_RATIO)
}

/// The size the blame column is set at, on the same terms.
fn blame_size(font_size: f32) -> f32 {
    type_size(font_size, BLAME_RATIO)
}

/// How wide the blame column is, which follows its type.
fn blame_width(font_size: f32) -> f32 {
    BLAME_WIDTH * blame_size(font_size) / (crate::settings::DEFAULT_FONT_SIZE * BLAME_RATIO)
}

/// One of the gutter's two type sizes, clamped so that neither extreme of the zoom is unusable.
///
/// A `font_size` of zero — which is what `Gutter::default()` gives — means the default size, so a
/// test that builds a gutter by hand gets the sizes the gutter has always used.
fn type_size(font_size: f32, ratio: f32) -> f32 {
    let size = match font_size > 0.0 {
        true => font_size,
        false => crate::settings::DEFAULT_FONT_SIZE,
    };
    (size * ratio).clamp(SMALLEST_TYPE, LARGEST_TYPE)
}

/// The box the letters of `line` actually occupy on screen, given the layout's `top`.
///
/// **Not the line box.** A line is taller than its glyphs — the baseline is `ascent` from the top
/// and every scrap of extra leading is added below — so centring anything in the line box puts it
/// low, by more the larger the type. This is the box the caret is already drawn to, and it is what
/// every mark in the gutter is centred on. See the note at the top of this file.
fn text_band(line: &unluminous_core::PlacedLine, top: f32) -> egui::Rangef {
    let baseline = top + line.y + line.baseline;
    egui::Rangef::new(baseline - line.ascent, baseline + line.descent)
}

/// How one breakpoint is drawn, which is the whole of what the gutter knows about it.
///
/// **Unluminous draws the adapter's answer rather than its own hope**, which is `task-1675`'s honesty rule
/// applied to a protocol that was designed for it: a breakpoint the debugger has agreed to stop at is
/// solid, and one it could not bind stays hollow for the life of the session rather than being drawn
/// as though it worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakpointMark {
    /// False for one that has been switched off without being taken away, which is drawn hollow.
    pub enabled: bool,
    /// False while a session is running and the adapter has not bound this one. **True when no
    /// session is running at all**: an unbound breakpoint is a thing a debugger says, and with no
    /// debugger there is nobody to have said it.
    pub verified: bool,
    /// True when it carries a condition or a log message, which puts a small mark on the dot.
    pub conditional: bool,
}

impl BreakpointMark {
    /// An ordinary one, as it is drawn with no session running.
    pub fn plain() -> Self {
        Self { enabled: true, verified: true, conditional: false }
    }

    /// Solid, or a ring. Off and unbound are both hollow, because both mean the program will not
    /// stop here.
    fn is_filled(self) -> bool {
        self.enabled && self.verified
    }
}

impl Gutter<'_> {
    /// True when there is anything to draw at all, which is what decides whether the editing area
    /// gives up any width.
    pub fn showing(&self) -> bool {
        self.numbers
            || self.blame.is_some()
            || !self.changes.is_empty()
            || !self.folds.is_empty()
            || !self.breakpoints.is_empty()
            || self.execution_point.is_some()
    }

    /// Whether this paragraph heads a region, and whether that region is collapsed.
    fn fold_at(&self, paragraph: usize) -> Option<bool> {
        self.folds
            .binary_search_by_key(&paragraph, |(at, _)| *at)
            .ok()
            .map(|index| self.folds[index].1)
    }

    /// The breakpoint on this paragraph, if there is one.
    fn breakpoint_at(&self, paragraph: usize) -> Option<BreakpointMark> {
        self.breakpoints
            .binary_search_by_key(&paragraph, |(at, _)| *at)
            .ok()
            .map(|index| self.breakpoints[index].1)
    }

    /// True when the numbers are not there to be drawn over, so the dot needs a column of its own.
    fn needs_a_breakpoint_column(&self) -> bool {
        !self.numbers && (self.can_debug || !self.breakpoints.is_empty())
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
    /// The breakpoint column was clicked on this paragraph, so a breakpoint should be put there or
    /// taken away. The component decides nothing about which, and knows nothing about offsets.
    pub toggle_breakpoint: Option<usize>,
    /// Which paragraph a right click was over, so the menu can be about the row under the pointer
    /// rather than about the caret — which is the rule the text menu and the terminal tab menu
    /// already follow.
    pub menu_paragraph: Option<usize>,
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
        let font = egui::FontId::monospace(number_size(gutter.font_size));
        let digit = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        width += digit * digits(lines) as f32 + NUMBER_MARGIN * 2.0;
    } else if gutter.needs_a_breakpoint_column() {
        // With the numbers on there is nothing to add: the dot is drawn over the number. This is
        // the other configuration, and the column is reserved whether or not anything is set so
        // that the first breakpoint never moves the text sideways.
        width += BREAKPOINT_COLUMN;
    }
    if gutter.blame.is_some() {
        width += blame_width(gutter.font_size);
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
    let blame = blame_width(gutter.font_size);
    let blame_rect = gutter.blame.map(|_| {
        let rect = Rect::from_min_size(Pos2::new(pen, area.top()), Vec2::new(blame, area.height()));
        pen += blame;
        rect
    });
    let numbers_rect = gutter.numbers.then(|| {
        let width = area.right() - pen - GAP - CHANGE_BAR - 2.0;
        let rect = Rect::from_min_size(Pos2::new(pen, area.top()), Vec2::new(width, area.height()));
        pen += width;
        rect
    });
    // Where the dot goes: over the number column when there is one, and in the column reserved for
    // it when there is not. One rectangle either way, so the drawing and the click target cannot
    // come apart — which is what `width` above is the other half of.
    let breakpoint_rect = match numbers_rect {
        Some(rect) => Some(rect),
        None if gutter.needs_a_breakpoint_column() => {
            let rect = Rect::from_min_size(
                Pos2::new(pen, area.top()),
                Vec2::new(BREAKPOINT_COLUMN, area.height()),
            );
            // Nothing else is laid out from the pen after this — the change bar is measured from the
            // right hand edge and the fold arrow from the change bar — so it is not advanced here.
            Some(rect)
        }
        None => None,
    };
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
        // Where the letters really are on this row, which is what every mark is centred on. See the
        // note at the top of this file: the line box is taller than the glyphs and all of the extra
        // is below them, so centring in `row` puts a mark low by more the larger the type.
        let band = text_band(line, top);
        // The paragraph a right click was over, so the menu can be about the row under the pointer.
        // Taken from the row loop rather than worked out from the position afterwards, because only
        // the loop knows where each paragraph ended up on the screen.
        if let Some(at) = outcome.context_menu {
            if row.y_range().contains(at.y) {
                outcome.menu_paragraph = Some(line.paragraph);
            }
        }
        if let (Some(rect), true) = (blame_rect, first_row) {
            draw_blame(
                &mut inner,
                rect,
                row,
                band,
                gutter.blame,
                line.paragraph,
                blame_size(gutter.font_size),
                &mut outcome,
            );
        }
        let mark = gutter.breakpoint_at(line.paragraph);
        let stopped = gutter.execution_point == Some(line.paragraph);
        // The dot is drawn **instead of** the number rather than over it, which is what the reference editor
        // does: a red circle with a numeral showing round its edge reads as neither. The number is
        // the thing that gives way, because a line with a breakpoint on it is being pointed at by
        // its dot and can be counted from the lines above.
        let covered = numbers_rect.is_some() && (mark.is_some() || stopped);
        if let (Some(rect), true, false) = (numbers_rect, first_row, covered) {
            draw_number(
                &inner,
                rect,
                band,
                line.paragraph + 1,
                line.paragraph == caret_line,
                number_size(gutter.font_size),
            );
        }
        if let (Some(rect), true) = (breakpoint_rect, first_row) {
            if draw_breakpoint(
                &mut inner,
                rect,
                row,
                band,
                line.paragraph,
                mark,
                stopped,
                gutter.can_debug,
            ) {
                outcome.toggle_breakpoint = Some(line.paragraph);
            }
        }
        if let (Some(collapsed), true) = (gutter.fold_at(line.paragraph), first_row) {
            let centre = Pos2::new(change_x - ARROW / 2.0, band.center());
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
    let tint = if collapsed || response.hovered() { color::text_control() } else { color::text_faint() };
    icon::disclosure(ui.painter(), centre, !collapsed, tint);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, collapsed, &name)
    });
    response.clicked()
}

/// The breakpoint column for one row: the dot if there is one, the execution-point arrow if the
/// program is stopped here, and the click that toggles one.
///
/// Returns true when the row was clicked. **Left click**, which is new behaviour: until now the
/// gutter took only `secondary_clicked` over the whole of itself, so nothing is being taken away
/// from anything. The click is taken **per row**, the way the blame cell already takes one, because
/// one interaction over the whole column could not say which line it was about.
///
/// A file whose language names no debugger takes no click at all — Unluminous's rule for a control that
/// can never apply — and draws nothing, so its gutter looks exactly as it did.
fn draw_breakpoint(
    ui: &mut egui::Ui,
    column: Rect,
    row: Rect,
    band: egui::Rangef,
    paragraph: usize,
    mark: Option<BreakpointMark>,
    stopped: bool,
    can_debug: bool,
) -> bool {
    // The dot sits at the left of the column with the numbers on — over the margin the number's
    // right alignment leaves — and in the middle of its own column with them off. Its height comes
    // from the letters rather than from the line, so it stays beside the number it replaces at every
    // size. The click target is still the whole row, because a person aiming at a line means the
    // line.
    let centre = Pos2::new(
        column.left() + (column.width() / 2.0).min(NUMBER_MARGIN + icon::BREAKPOINT_RADIUS),
        band.center(),
    );
    if stopped {
        // The execution point's own mark, drawn behind the dot so a breakpoint that is also where
        // the program stopped still reads as a breakpoint. The reference editor's arrow, drawn.
        execution_arrow(ui.painter(), centre, color::accent());
    }
    if let Some(mark) = mark {
        // Both hollow, because both mean the program will not stop here — but they are not the same
        // thing and are not drawn the same. A breakpoint **switched off** is somebody's own decision
        // and is dimmed to say so; one the debugger could not **bind** is still asking to be
        // honoured, so its ring is at full strength. §6.2's "dimmed hollow" and "hollow with a quiet
        // ring", which are two states rather than one.
        let tint = match mark.enabled {
            true => color::breakpoint(),
            false => color::breakpoint().gamma_multiply(0.45),
        };
        icon::breakpoint(ui.painter(), centre, mark.is_filled(), tint);
        if mark.conditional {
            icon::breakpoint_badge(ui.painter(), centre, tint);
        }
    }
    if !can_debug {
        return false;
    }
    let target = Rect::from_min_size(
        Pos2::new(column.left(), row.top()),
        Vec2::new(column.width().min(BREAKPOINT_COLUMN + NUMBER_MARGIN), row.height()),
    );
    let name = match mark {
        Some(_) => format!("Remove breakpoint on line {}", paragraph + 1),
        None => format!("Set breakpoint on line {}", paragraph + 1),
    };
    let response = ui.interact(target, ui.id().with(("breakpoint", paragraph)), Sense::click());
    // A hovered row with nothing on it shows where the dot would go, which is how a person finds a
    // control that is otherwise invisible until it is used — VS Code's own hint.
    if response.hovered() && mark.is_none() {
        icon::breakpoint(ui.painter(), centre, false, color::breakpoint().gamma_multiply(0.45));
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, mark.is_some(), &name)
    });
    response.clicked()
}

/// The mark on the line the program is stopped on: a filled arrow pointing at the code.
///
/// Drawn rather than lettered, in the manner of every other mark in the gutter, and it is the reference editor's
/// own shape.
fn execution_arrow(painter: &egui::Painter, centre: Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(centre.x - 5.0, centre.y - 5.0),
            Pos2::new(centre.x + 5.0, centre.y),
            Pos2::new(centre.x - 5.0, centre.y + 5.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// One line number, right aligned in its column and centred on the letters of its line.
///
/// `band` rather than the row, which is the whole of `task-1693`'s first report: see the note at the
/// top of this file.
fn draw_number(
    ui: &egui::Ui,
    column: Rect,
    band: egui::Rangef,
    number: usize,
    current: bool,
    size: f32,
) {
    let tint = if current { color::text_control() } else { color::text_faint() };
    let galley =
        ui.painter().layout_no_wrap(number.to_string(), egui::FontId::monospace(size), tint);
    ui.painter().galley(
        Pos2::new(
            column.right() - NUMBER_MARGIN - galley.size().x,
            band.center() - galley.size().y / 2.0,
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
#[allow(clippy::too_many_arguments)]
fn draw_blame(
    ui: &mut egui::Ui,
    column: Rect,
    row: Rect,
    band: egui::Rangef,
    blame: Option<&[BlameRow]>,
    paragraph: usize,
    size: f32,
    outcome: &mut GutterOutcome,
) {
    let Some(entry) = blame.and_then(|rows| rows.get(paragraph)) else {
        return;
    };
    let cell = Rect::from_min_size(
        Pos2::new(column.left(), row.top()),
        Vec2::new(column.width() - 4.0, row.height()),
    );
    let tint = mix(color::blame_old(), color::blame_new(), entry.age);
    ui.painter().rect_filled(cell, CornerRadius::ZERO, tint);

    let name = format!("Blame: {} {}", entry.date, entry.author);
    let response = ui
        .interact(cell, ui.id().with(("blame", paragraph)), Sense::click())
        .on_hover_text(format!("{}\n{} \u{00B7} {}", entry.summary, entry.author, entry.date));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    if response.clicked() {
        outcome.show_commit = Some(entry.commit.clone());
    }

    // The cell is the whole row — it is a background and has to meet the cell above it — but its
    // words are centred on the letters beside them, like every other mark in the gutter.
    let font = egui::FontId::proportional(size);
    let date = ui.painter().layout_no_wrap(entry.date.clone(), font.clone(), color::text_strong());
    let y = band.center() - date.size().y / 2.0;
    ui.painter().galley(Pos2::new(cell.left() + 6.0, y), date.clone(), color::text_strong());
    let author = ui.painter().layout_no_wrap(entry.author.clone(), font, color::text_strong());
    ui.painter().galley(
        Pos2::new(cell.left() + 12.0 + date.size().x, y),
        author,
        color::text_strong(),
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

    /// One line as `unluminous_core::layout` really builds it: the baseline `ascent` from the top, and
    /// every scrap of extra leading below the letters.
    fn line(y: f32, ascent: f32, descent: f32, leading: f32) -> unluminous_core::PlacedLine {
        unluminous_core::PlacedLine {
            y,
            height: ascent + descent + leading,
            baseline: ascent,
            ascent,
            descent,
            bytes: 0..0,
            paragraph: 0,
            last_in_paragraph: true,
            runs: Vec::new(),
            empty_style: unluminous_core::CharStyle::default(),
        }
    }

    /// `task-1693`: a mark centred on the line box sits low, because the bottom of a line box is
    /// empty air. The band is the letters, and its centre is where a mark goes.
    #[test]
    fn a_mark_is_centred_on_the_letters_rather_than_on_the_line() {
        let line = line(0.0, 15.0, 4.0, 7.2);
        let band = text_band(&line, 100.0);
        assert_eq!(band.min, 100.0, "the letters start at the top of the line");
        assert_eq!(band.max, 119.0, "and end at the descender");
        assert_eq!(band.center(), 109.5);
        let line_centre = 100.0 + line.height / 2.0;
        assert!(
            line_centre > band.center() + 3.0,
            "the line box's centre is more than three points below the letters', which is the drift              that was reported: {line_centre} against {}",
            band.center()
        );
    }

    /// The drift the ticket reported grows with the zoom, which is why it was noticed while zooming
    /// rather than at rest. Twice the type, twice the error.
    #[test]
    fn the_drift_grows_with_the_type_size() {
        let small = line(0.0, 15.0, 4.0, 7.2);
        let large = line(0.0, 30.0, 8.0, 14.4);
        let error = |line: &unluminous_core::PlacedLine| {
            line.height / 2.0 - text_band(line, 0.0).center()
        };
        assert!(error(&large) > error(&small) * 1.9, "the error roughly doubles with the size");
    }

    /// Expressed as a ratio of the default so that a window at the default size sets its numbers at
    /// exactly the sizes the gutter has always used, and only the alignment changes.
    #[test]
    fn the_gutter_sets_its_type_at_the_old_sizes_at_the_default_font() {
        assert_eq!(number_size(crate::settings::DEFAULT_FONT_SIZE), 11.5);
        assert_eq!(blame_size(crate::settings::DEFAULT_FONT_SIZE), 10.5);
        assert_eq!(blame_width(crate::settings::DEFAULT_FONT_SIZE), BLAME_WIDTH);
        // A gutter that was never told a size — which is every one a unit test builds — reads as the
        // default rather than as nothing.
        assert_eq!(number_size(0.0), 11.5);
    }

    #[test]
    fn the_gutters_type_follows_the_editors_and_stops_at_both_ends() {
        assert!(number_size(32.0) > number_size(16.0), "it grows with the editor's font");
        assert_eq!(number_size(6.0), SMALLEST_TYPE, "six point text still needs a legible gutter");
        assert_eq!(number_size(144.0), LARGEST_TYPE, "and the gutter never takes the window");
    }

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
        let gutter = Gutter { numbers: false, blame: None, changes: &[], folds: &[], ..Gutter::default() };
        assert!(!gutter.showing());
    }

    #[test]
    fn a_change_bar_alone_is_enough_to_show_the_gutter() {
        let changes = [(3, Change::Modified)];
        let gutter = Gutter { numbers: false, blame: None, changes: &changes, folds: &[], ..Gutter::default() };
        assert!(gutter.showing(), "a file with changes shows its change bars even with numbers off");
    }

    #[test]
    fn a_folding_arrow_alone_is_enough_to_show_the_gutter() {
        let folds = [(4usize, false)];
        let gutter = Gutter { numbers: false, blame: None, changes: &[], folds: &folds, ..Gutter::default() };
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
    fn a_breakpoint_alone_is_enough_to_show_the_gutter() {
        let breakpoints = [(2usize, BreakpointMark::plain())];
        let gutter = Gutter { breakpoints: &breakpoints, ..Gutter::default() };
        assert!(gutter.showing(), "a file with a breakpoint in it shows its dot");
        assert_eq!(gutter.breakpoint_at(2), Some(BreakpointMark::plain()));
        assert_eq!(gutter.breakpoint_at(1), None);
    }

    #[test]
    fn the_line_the_program_is_stopped_on_is_enough_on_its_own() {
        let gutter = Gutter { execution_point: Some(4), ..Gutter::default() };
        assert!(gutter.showing());
    }

    /// The dot is drawn over the number, so with the numbers on it costs the gutter nothing at all —
    /// which is the whole reason no accepted screenshot had to move.
    #[test]
    fn the_dot_takes_no_width_of_its_own_while_the_numbers_are_showing() {
        let breakpoints = [(0usize, BreakpointMark::plain())];
        let with = Gutter { numbers: true, breakpoints: &breakpoints, can_debug: true, ..Gutter::default() };
        let without = Gutter { numbers: true, can_debug: true, ..Gutter::default() };
        assert!(!with.needs_a_breakpoint_column());
        assert!(!without.needs_a_breakpoint_column());
    }

    /// And with them off it gets a column, reserved whether or not anything is set: a column that
    /// arrived with the first dot would move the text sideways under the pointer.
    #[test]
    fn with_the_numbers_off_the_column_is_reserved_before_anything_is_set() {
        let empty = Gutter { numbers: false, can_debug: true, ..Gutter::default() };
        assert!(empty.needs_a_breakpoint_column(), "reserved before the first breakpoint");
        let breakpoints = [(0usize, BreakpointMark::plain())];
        let one = Gutter { numbers: false, breakpoints: &breakpoints, can_debug: true, ..Gutter::default() };
        assert!(one.needs_a_breakpoint_column(), "and still reserved with one");
    }

    /// A file whose language names no debugger gets no column and no click, which is Unluminous's rule
    /// for a control that can never apply — so a stylesheet's gutter is exactly what it was.
    #[test]
    fn a_file_that_cannot_be_debugged_gets_no_breakpoint_column() {
        let css = Gutter { numbers: false, can_debug: false, ..Gutter::default() };
        assert!(!css.needs_a_breakpoint_column());
        assert!(!css.showing(), "and nothing else about it changed either");
    }

    /// Off and unbound are both hollow, because both mean the program will not stop here.
    #[test]
    fn a_breakpoint_is_solid_only_when_it_is_on_and_the_debugger_agreed_to_it() {
        assert!(BreakpointMark::plain().is_filled());
        assert!(!BreakpointMark { enabled: false, ..BreakpointMark::plain() }.is_filled());
        assert!(!BreakpointMark { verified: false, ..BreakpointMark::plain() }.is_filled());
    }

    #[test]
    fn the_blame_tint_runs_from_the_oldest_colour_to_the_newest() {
        assert_eq!(mix(color::blame_old(), color::blame_new(), 0.0), color::blame_old());
        assert_eq!(mix(color::blame_old(), color::blame_new(), 1.0), color::blame_new());
        let middle = mix(color::blame_old(), color::blame_new(), 0.5);
        assert!(middle.r() > color::blame_old().r() && middle.r() < color::blame_new().r());
        // Out of range is clamped rather than producing a colour that is not on the line.
        assert_eq!(mix(color::blame_old(), color::blame_new(), 2.0), color::blame_new());
    }
}
