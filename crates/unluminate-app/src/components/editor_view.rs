//! The editing surface: keyboard and mouse input in, painted text out.
//!
//! Nothing here decides what an edit means. Every key press becomes a `unluminate_core::Command` and the
//! document decides. Painting walks the lines the layout produced and draws one textured rectangle per
//! glyph out of the atlas, so the whole visible document is a single mesh.

use egui::{Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use unluminate_core::{Align, Command, Document, IndentUnit, Layout, Rope, Selection, StyleChange};

use crate::services::text_renderer::TextRenderer;
use crate::theme::color;

/// Space between the text and the edge of the editing area.
pub const PADDING: f32 = 16.0;
/// Width of the caret.
const CARET_WIDTH: f32 = 2.0;
/// The gap between the end of a collapsed head line and its badge.
const BADGE_GAP: f32 = 6.0;
/// How large the badge standing for a collapsed block is.
const BADGE_WIDTH: f32 = 26.0;
const BADGE_HEIGHT: f32 = 14.0;
/// The gap between the end of a line and the value painted after it while the program is paused.
///
/// Wide enough that the value plainly is not part of the code — an inline value that touched the
/// last character would read as text somebody had typed, which is the one thing it must not do.
const INLINE_GAP: f32 = 22.0;
/// How large an inline value is set. Smaller than the code, in the quiet colour, for the same reason.
const INLINE_SIZE: f32 = 11.0;

/// What the window worked out about the symbol under the pointer, before any click.
///
/// The component resolves nothing: it has no index, no project and no idea what a definition is.
/// The window asks those questions once a frame while the modifier is held — cached, so a pointer
/// resting still costs nothing — and hands the answer down, which is the same "components report,
/// the window decides" rule every component in Unluminate follows.
///
/// **Resolve before the click.** Only a word that really has somewhere to go is underlined, so the
/// promise the underline makes is one the click can keep. A word the index knows nothing about gets
/// no affordance and a modifier-click on it places the caret like any other click.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolPointer {
    /// The word under the pointer, when the modifier is held and it resolves to somewhere.
    pub word: Option<std::ops::Range<usize>>,
}

impl SymbolPointer {
    /// True when there is something to underline and something a click would do.
    pub fn resolved(&self) -> bool {
        self.word.is_some()
    }
}

/// What a click or a drag in the editing area came to.
#[derive(Debug, Default, PartialEq)]
pub struct PointerOutcome {
    /// The document changed — the caret moved, or a selection was made.
    pub changed: bool,
    /// The modifier was held over a word that resolves, so this is a jump rather than a caret.
    pub jump: Option<usize>,
}

/// What the editing surface wants the application to do after handling input.
#[derive(Debug, Default)]
pub struct ViewOutcome {
    /// The document changed, so the layout is stale and the window should be painted again.
    pub changed: bool,
    /// Text to put on the clipboard, from a copy or a cut.
    pub copy: Option<String>,
    /// Keep the caret in view, because it moved.
    pub scroll_to_caret: bool,
}

/// Turn one frame's input events into commands and run them.
///
/// The events are read rather than the key state, because the order matters: typing a letter and then
/// pressing an arrow key must happen in that order.
///
/// `formatting` is whether bold, italic and the rest apply to this file at all, and it decides one
/// thing: whether the command key and `B` mean bold. On a source file that key is `Go to
/// Definition`, which is a menu entry — and the two can never both apply, because
/// `services::file_kind` answers them from the same file: formatting is for prose, and a definition
/// needs a language that says what one looks like. Without this the two would both fire on one
/// press, which is exactly the fault the `Tab` guard below records.
pub fn handle_input(
    ui: &egui::Ui,
    document: &mut Document,
    layout: &Layout,
    has_focus: bool,
    formatting: bool,
) -> ViewOutcome {
    let mut outcome = ViewOutcome::default();
    if !has_focus {
        return outcome;
    }
    // A box that takes typing has been clicked into: the explorer's filter, the commit message, the
    // rename prompt, one of the searches. It keeps the keyboard until it is clicked away from, so
    // the document stands aside. egui leaves the events that box consumed in the frame's list, so
    // reading them here as well put every character into the file behind the box and left it marked
    // as having unsaved changes. The terminal stands aside for the same reason and in the same way.
    //
    // The guard is here rather than in the caller so that a later caller cannot forget it, and so a
    // test that calls this function is testing the rule the window really follows. The mouse is not
    // guarded: clicking in the document is how the document takes the keyboard back, and egui
    // surrenders the box's focus on that same click.
    if crate::app::text_box_has_the_keyboard(ui.ctx()) {
        return outcome;
    }
    // A modal is open. Most of them have no field in them at all, so the question above says
    // nothing about them, and a document that went on reading the keyboard behind a dialog would
    // take the `Enter` that answers it as a new line in the file.
    if crate::app::a_modal_has_the_keyboard(ui.ctx()) {
        return outcome;
    }
    let events = ui.input(|input| input.events.clone());
    for event in events {
        match event {
            egui::Event::Text(text) => {
                if !text.chars().any(|c| c.is_control()) {
                    // A single space over a selection is an indent of one space per line, the same
                    // rule the `Tab` key follows: the selection is what makes the key an indent
                    // rather than a type. The modifiers are read off the frame's input state, because
                    // a `Text` event carries none of its own, and the check is what keeps
                    // `Ctrl+Space` — the completion's key — from indenting.
                    let indenting = text == " "
                        && !document.selection().is_empty()
                        && !ui.input(|input| input.modifiers.command)
                        && !ui.input(|input| input.modifiers.ctrl);
                    if indenting {
                        // Shift dedents by one space instead of indenting by one, the reverse half
                        // `Shift+Space` asks for beside `Space`'s own.
                        let unit = IndentUnit::Space;
                        let command = if ui.input(|input| input.modifiers.shift) {
                            Command::Dedent { unit }
                        } else {
                            Command::Indent { unit }
                        };
                        outcome.changed |= document.apply(command);
                    } else {
                        outcome.changed |= document.apply(Command::Insert(text));
                    }
                    outcome.scroll_to_caret = true;
                }
            }
            egui::Event::Paste(text) => {
                // Line breaks from another application may be a carriage return and a line feed. The
                // buffer stores line feeds only, so that offsets and line counts have one meaning.
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                outcome.changed |= document.apply(Command::Insert(text));
                outcome.scroll_to_caret = true;
            }
            egui::Event::Copy => {
                if !document.selection().is_empty() {
                    outcome.copy = Some(document.selected_text());
                }
            }
            egui::Event::Cut => {
                if !document.selection().is_empty() {
                    outcome.copy = Some(document.selected_text());
                    outcome.changed |= document.apply(Command::DeleteBackward);
                    outcome.scroll_to_caret = true;
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let shift = modifiers.shift;
                // `command` is the Apple key on macOS and the control key on Windows, which is what
                // egui reports for a shortcut on either platform.
                let shortcut = modifiers.command;
                let word = modifiers.alt || (modifiers.ctrl && !modifiers.mac_cmd);
                // The modifier and alt together with an arrow is `Navigate Back` and
                // `Navigate Forward`, which are menu entries. The menu's keyboard watcher does not
                // consume the press — nothing in Unluminate does — so without this guard one press of
                // `Ctrl+Alt+Left` went back **and** moved a word, which is the same shape of fault
                // as the `Tab` one below.
                let navigating = shortcut && modifiers.alt;
                let handled = match key {
                    egui::Key::ArrowLeft | egui::Key::ArrowRight if navigating => false,
                    egui::Key::ArrowLeft if word => {
                        document.apply(Command::MoveWordLeft { extend: shift })
                    }
                    egui::Key::ArrowRight if word => {
                        document.apply(Command::MoveWordRight { extend: shift })
                    }
                    egui::Key::ArrowLeft if shortcut => {
                        document.apply(Command::MoveLineStart { extend: shift })
                    }
                    egui::Key::ArrowRight if shortcut => {
                        document.apply(Command::MoveLineEnd { extend: shift })
                    }
                    egui::Key::ArrowLeft => document.apply(Command::MoveLeft { extend: shift }),
                    egui::Key::ArrowRight => document.apply(Command::MoveRight { extend: shift }),
                    egui::Key::ArrowUp if shortcut => {
                        document.apply(Command::MoveDocumentStart { extend: shift })
                    }
                    egui::Key::ArrowDown if shortcut => {
                        document.apply(Command::MoveDocumentEnd { extend: shift })
                    }
                    egui::Key::ArrowUp => document.move_vertically(layout, -1, shift),
                    egui::Key::ArrowDown => document.move_vertically(layout, 1, shift),
                    egui::Key::Home => document.apply(Command::MoveLineStart { extend: shift }),
                    egui::Key::End => document.apply(Command::MoveLineEnd { extend: shift }),
                    egui::Key::Backspace if word => {
                        document.apply(Command::DeleteWordBackward)
                    }
                    egui::Key::Backspace => document.apply(Command::DeleteBackward),
                    egui::Key::Delete => document.apply(Command::DeleteForward),
                    egui::Key::Enter => document.apply(Command::Insert("\n".to_owned())),
                    // A bare Tab. With control held it belongs to `Next Tab` and `Previous Tab` on
                    // the View menu, and finding the action there does not consume the key press, so
                    // without this guard control and Tab moved to the next file **and** typed a tab
                    // into the one it left — which is how the file tabs capture for the
                    // documentation came out with two files marked as having unsaved changes that
                    // nobody had touched.
                    egui::Key::Tab if !shortcut && !modifiers.ctrl => {
                        // A selection makes the key an indent rather than a type: every line the
                        // selection touches gets one tab at its start, and the selection stays over
                        // the text it covered. With no selection the tab is typed where the caret
                        // is, inside the run of typing. `Shift+Tab` dedents instead — the reverse
                        // half of the same key, removing one tab from each touched line rather than
                        // adding one.
                        if document.selection().is_empty() {
                            document.apply(Command::Insert("\t".to_owned()))
                        } else if shift {
                            document.apply(Command::Dedent { unit: IndentUnit::Tab })
                        } else {
                            document.apply(Command::Indent { unit: IndentUnit::Tab })
                        }
                    }
                    // Undo, redo, select all, save and the clipboard are menu entries, and the menu owns
                    // their shortcuts. On macOS the menu bar takes those key presses before the window sees
                    // them, so handling them here as well would do the work twice on one platform and once
                    // on the other. The formatting shortcuts below are in no menu, so they are handled here.
                    egui::Key::B if shortcut && formatting => document.apply(Command::ToggleBold),
                    egui::Key::I if shortcut => document.apply(Command::ToggleItalic),
                    egui::Key::U if shortcut => document.apply(Command::ToggleUnderline),
                    egui::Key::X if shortcut && modifiers.shift => {
                        document.apply(Command::ToggleStrikethrough)
                    }
                    egui::Key::L if shortcut => document.apply(Command::SetAlign(Align::Left)),
                    egui::Key::E if shortcut => document.apply(Command::SetAlign(Align::Center)),
                    egui::Key::R if shortcut => document.apply(Command::SetAlign(Align::Right)),
                    egui::Key::J if shortcut => document.apply(Command::SetAlign(Align::Justify)),
                    _ => false,
                };
                if handled {
                    outcome.changed = true;
                    outcome.scroll_to_caret = true;
                }
            }
            _ => {}
        }
    }
    outcome
}

/// Turn a click or a drag into a caret position, a selection, or a jump.
///
/// The jump is the one thing here that is not an edit: with the modifier held over a word the
/// window has already resolved, the click is reported rather than acted on, and the window decides
/// what opening that definition means. A modifier-click on anything else is an ordinary click.
pub fn handle_pointer(
    response: &egui::Response,
    document: &mut Document,
    layout: &Layout,
    text_origin: Pos2,
    symbol: &SymbolPointer,
) -> PointerOutcome {
    let mut outcome = PointerOutcome::default();
    let mut changed = false;
    let position = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos())
        .map(|p| p - text_origin);
    if let Some(local) = position {
        if response.clicked() && !response.dragged() && symbol.resolved() {
            // The word was resolved from where the pointer was, so the click is about that word
            // whatever the click's own arithmetic makes of the point.
            let word = symbol.word.clone().unwrap_or_default();
            outcome.jump = Some(word.start);
            return outcome;
        }
        if response.drag_started() || (response.clicked() && !response.dragged()) {
            let offset = layout.offset_at(local.x, local.y);
            let extend = response.ctx.input(|input| input.modifiers.shift);
            changed |= document.apply(Command::PlaceCaret { offset, extend });
        } else if response.dragged() {
            let offset = layout.offset_at(local.x, local.y);
            changed |= document.apply(Command::PlaceCaret { offset, extend: true });
        } else if response.double_clicked() {
            // A double click selects the word under the pointer.
            let offset = layout.offset_at(local.x, local.y);
            document.apply(Command::PlaceCaret { offset, extend: false });
            document.apply(Command::MoveWordLeft { extend: false });
            changed |= document.apply(Command::MoveWordRight { extend: true });
        }
    }
    outcome.changed = changed;
    outcome
}

/// Paint the document.
///
/// The order matters: the selection goes behind the text, the text next, the underline and
/// strikethrough rules over the text so they are visible against it, and the caret last so it is never
/// hidden by a glyph.
/// How the editing surface is painted: the two colours it needs, whether the caret is shown, and
/// the word the modifier is hovering over.
#[derive(Debug, Clone, Default)]
pub struct PaintStyle<'a> {
    /// Behind selected text.
    pub selection: Color32,
    /// The caret itself.
    pub caret: Color32,
    /// False when the editing area does not have the keyboard, so no caret is drawn.
    pub show_caret: bool,
    /// The word to underline, which is the affordance for `Ctrl/Cmd+Click`.
    ///
    /// Drawn by the painter under the word's own glyphs rather than by a widget of its own: it
    /// appears and goes on a modifier being held, and a widget that came and went sixty times a
    /// second would be a widget egui had to lay out sixty times a second.
    pub underline: Option<std::ops::Range<usize>>,
    /// The paragraph the program is stopped on, when it is stopped in this file.
    ///
    /// Painted as a band behind the whole line, in a colour of its own so it cannot be mistaken for
    /// a passage somebody marked — the four highlight colours are all at one alpha and this is
    /// deliberately not one of them.
    pub execution_point: Option<usize>,
    /// Values to paint after the end of the lines they belong to, while the program is paused.
    ///
    /// `(paragraph, text)`, sorted by paragraph. **Painted decoration, never text in the document**:
    /// it does not select, it does not copy, and no byte offset crosses it. The window works out
    /// which name is on which line and this draws the answer, which is the rule every component in
    /// Unluminate follows.
    pub inline_values: &'a [(usize, String)],
    /// Every match of the Find bar's search, painted as a band behind the text.
    ///
    /// **The current one is not in this list** -- it is the document's selection, so the bar's own
    /// match is drawn by the selection above and copies, and is where the caret is left when the bar
    /// is shut. Two colours for two meanings, which is what `color::find_match` says about itself.
    /// `task-1804` §3.1.
    pub find_matches: &'a [std::ops::Range<usize>],
}

pub fn paint(
    ui: &egui::Ui,
    renderer: &TextRenderer,
    document: &Document,
    layout: &Layout,
    text_origin: Pos2,
    style: PaintStyle<'_>,
) {
    let PaintStyle {
        selection: selection_color,
        caret: caret_color,
        show_caret,
        underline,
        execution_point,
        inline_values,
        find_matches,
    } = style;
    let painter = ui.painter();
    let to_screen = |x: f32, y: f32| Pos2::new(text_origin.x + x, text_origin.y + y);
    let visible = visible_lines(ui, layout, text_origin);

    // The band behind the stopped line, first of all: under the selection, under the marks and under
    // the glyphs, because it is the furthest back thing on the line.
    if let Some(paragraph) = execution_point {
        paint_execution_point(ui, layout, text_origin, visible.clone(), paragraph);
    }

    for rect in layout.selection_rects_in(visible.clone(), document.selection().range()) {
        painter.rect_filled(
            Rect::from_min_size(to_screen(rect.x, rect.y), Vec2::new(rect.width, rect.height)),
            2.0,
            selection_color,
        );
    }

    // Every other match of the search, over the selection so the current one is not covered by a
    // band, and under the text like every other background. Only the matches on the screen are laid
    // out: `selection_rects_in` is cut to the visible lines already, and a file with ten thousand
    // matches in it costs one comparison each for the ones that are not showing.
    for range in find_matches {
        for rect in layout.selection_rects_in(visible.clone(), range.clone()) {
            painter.rect_filled(
                Rect::from_min_size(to_screen(rect.x, rect.y), Vec2::new(rect.width, rect.height)),
                2.0,
                color::find_match(),
            );
        }
    }

    paint_highlights(ui, document, layout, text_origin, visible.clone());
    paint_text(ui, renderer, layout, text_origin);

    // Under the word, over the text: a rule a point tall at the bottom of the word's own box, in
    // the accent colour, which is what a link looks like everywhere and is what says the click
    // would go somewhere.
    if let Some(word) = underline {
        for rect in layout.selection_rects_in(visible.clone(), word) {
            painter.rect_filled(
                Rect::from_min_size(
                    to_screen(rect.x, rect.y + rect.height - 1.0),
                    Vec2::new(rect.width, 1.0),
                ),
                0.0,
                caret_color,
            );
        }
    }

    if show_caret {
        let caret = layout.caret_at(document.selection().head);
        painter.rect_filled(
            Rect::from_min_size(to_screen(caret.x, caret.y), Vec2::new(CARET_WIDTH, caret.height)),
            0.0,
            caret_color,
        );
    }

    // Last of all, because they are painted **over** the text at the end of its own line and must
    // never be hidden by a glyph — and because a value is decoration rather than something the
    // caret can be put into.
    paint_inline_values(ui, layout, text_origin, visible, inline_values);
}

/// The band behind the line the program is stopped on.
///
/// The whole width of the editing area rather than the width of the text, which is what makes it read
/// as "the program is here" rather than as a passage somebody marked: a highlight is the shape of the
/// words it covers and this is the shape of the line.
fn paint_execution_point(
    ui: &egui::Ui,
    layout: &Layout,
    text_origin: Pos2,
    lines: std::ops::Range<usize>,
    paragraph: usize,
) {
    let clip = ui.painter().clip_rect();
    for line in &layout.lines[lines.start..lines.end.min(layout.lines.len())] {
        if line.paragraph != paragraph {
            continue;
        }
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(clip.left(), text_origin.y + line.y),
                Vec2::new(clip.width(), line.height),
            ),
            0.0,
            crate::theme::color::execution_point(),
        );
    }
}

/// The values painted after the ends of the lines they belong to.
///
/// A value is put after the **last visual line** of its paragraph, because that is where the line
/// ends on the screen — a wrapped paragraph would otherwise have its value drawn over its own second
/// row. A line that fills the pane is left alone rather than having a value drawn off the edge of it.
fn paint_inline_values(
    ui: &egui::Ui,
    layout: &Layout,
    text_origin: Pos2,
    lines: std::ops::Range<usize>,
    values: &[(usize, String)],
) {
    if values.is_empty() {
        return;
    }
    let painter = ui.painter();
    let right = painter.clip_rect().right();
    let last = lines.end.min(layout.lines.len());
    for (index, line) in layout.lines[lines.start..last].iter().enumerate() {
        // The last visual row of the paragraph, which is the one whose end is the end of the line.
        let next = layout.lines.get(lines.start + index + 1);
        if next.is_some_and(|after| after.paragraph == line.paragraph) {
            continue;
        }
        let Ok(at) = values.binary_search_by_key(&line.paragraph, |(paragraph, _)| *paragraph) else {
            continue;
        };
        let x = text_origin.x + line.right() + INLINE_GAP;
        if x > right - 24.0 {
            continue;
        }
        let galley = painter.layout_no_wrap(
            values[at].1.clone(),
            egui::FontId::monospace(INLINE_SIZE),
            crate::theme::color::inline_value(),
        );
        painter.galley(
            Pos2::new(x, text_origin.y + line.y + (line.height - galley.size().y) / 2.0),
            galley,
            crate::theme::color::inline_value(),
        );
    }
}

/// Paint the marked passages, over the selection and under the text.
///
/// The order is not arbitrary. A passage marked while it is still selected has to be visible at the
/// moment it is marked, and the selection's own colour is opaque — so the mark goes over it, at
/// whatever alpha was chosen, and the selection shows through. And it goes under the text, because a
/// highlight is a background: the writing over it is painted at full alpha like every other glyph in
/// Unluminate.
///
/// Only what is on the screen is asked for. `Highlights::overlapping` is a binary search and a walk,
/// so a file with a thousand marks costs a frame the dozen that can be seen.
fn paint_highlights(
    ui: &egui::Ui,
    document: &Document,
    layout: &Layout,
    text_origin: Pos2,
    lines: std::ops::Range<usize>,
) {
    if document.highlights().is_empty() {
        return;
    }
    let painter = ui.painter();
    let bytes = if lines.is_empty() {
        0..0
    } else {
        layout.lines[lines.start].bytes.start..layout.lines[lines.end - 1].bytes.end
    };
    for mark in document.highlights().overlapping(bytes) {
        let color = crate::theme::color32(mark.color);
        for rect in layout.selection_rects_in(lines.clone(), mark.range.clone()) {
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(text_origin.x + rect.x, text_origin.y + rect.y),
                    Vec2::new(rect.width, rect.height),
                ),
                2.0,
                color,
            );
        }
    }
}

/// Draw the badge that stands for a collapsed block, and say whether one was pressed.
///
/// The reference editor's `{...}`, Sublime's `⋯`: a small rounded rectangle after the end of the head line with
/// three dots in it, and pressing it expands the block. It is the affordance a person reaches for
/// before they think about the gutter.
///
/// **Drawn over the line rather than put into the text**, which is the one place Unluminate's Markdown
/// preview's rule — everything on the screen is real text in a real document — is deliberately not
/// followed. Three characters in the layout that are not in the file would have to be hidden from
/// the caret, the selection, the clipboard and every byte offset that crosses them; a rectangle
/// painted on top is hidden from all of them for free. So the badge is not selectable, and copying
/// the head line copies the head line.
///
/// `folds` is what the gutter draws its arrows from — every head and whether it is collapsed,
/// sorted. Only the lines on the screen are looked at, which is the same range the painter drew.
pub fn fold_badges(
    ui: &mut egui::Ui,
    layout: &Layout,
    text_origin: Pos2,
    folds: &[(usize, bool)],
    lines: std::ops::Range<usize>,
) -> Option<usize> {
    if folds.is_empty() || lines.is_empty() {
        return None;
    }
    let collapsed = |paragraph: usize| {
        folds
            .binary_search_by_key(&paragraph, |(at, _)| *at)
            .map(|at| folds[at].1)
            .unwrap_or(false)
    };
    let mut pressed = None;
    for line in &layout.lines[lines] {
        if !line.last_in_paragraph || !collapsed(line.paragraph) {
            continue;
        }
        let area = Rect::from_min_size(
            Pos2::new(text_origin.x + line.right() + BADGE_GAP, text_origin.y + line.y + (line.height - BADGE_HEIGHT) / 2.0),
            Vec2::new(BADGE_WIDTH, BADGE_HEIGHT),
        );
        let name = format!("Expand block at line {}", line.paragraph + 1);
        let response = ui.interact(area, ui.id().with(("fold-badge", line.paragraph)), Sense::click());
        let painter = ui.painter();
        painter.rect(
            area,
            egui::CornerRadius::same(4),
            crate::theme::color::control(),
            Stroke::new(1.0, if response.hovered() { crate::theme::color::accent() } else { crate::theme::color::control_border() }),
            egui::StrokeKind::Inside,
        );
        let tint = crate::theme::color::text_control();
        for step in 0..3 {
            let x = area.left() + BADGE_WIDTH / 4.0 + step as f32 * BADGE_WIDTH / 4.0;
            painter.circle_filled(Pos2::new(x, area.center().y), 1.1, tint);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        if response.clicked() {
            pressed = Some(line.paragraph);
        }
    }
    pressed
}

/// Paint a laid out text, with no selection and no caret.
///
/// The Markdown preview uses this. It has no document behind it, only a layout, because it is produced from
/// the source rather than edited.
pub fn paint_text(ui: &egui::Ui, renderer: &TextRenderer, layout: &Layout, text_origin: Pos2) -> usize {
    let painter = ui.painter();
    let to_screen = |x: f32, y: f32| Pos2::new(text_origin.x + x, text_origin.y + y);
    // Only the lines that fall inside what is being drawn into. The whole document used to be
    // collected and handed to egui as one mesh, which culls a mesh only against its bounding box —
    // and the bounding box of a whole document plainly overlaps the window, so every glyph in the
    // file was tessellated and uploaded to the graphics card on every frame. On a 169 kilobyte
    // source file that was 7 ms a frame against the 0.07 ms one screenful costs. See
    // `tasks/task-1666-performance-tdd.md` section 5.
    let visible = visible_lines(ui, layout, text_origin);

    // Every glyph on screen is collected first and the texture is uploaded afterwards. Doing it the
    // other way round draws the frame from a texture that does not yet hold the glyphs rasterised
    // during this very pass, and any letter appearing for the first time comes out blank.
    //
    // Collecting can itself fill the atlas and force it to be cleared, which moves the glyphs already
    // collected. When that happens the whole pass is repeated, because the alternative is drawing from
    // positions that no longer hold those glyphs.
    let mut placed: Vec<(Rect, egui::Rect, Color32)> = Vec::new();
    // The strokes of the box-drawing characters, which are drawn rather than lettered. See
    // `box_rules`.
    let mut rules: Vec<(Rect, Color32)> = Vec::new();
    for _ in 0..3 {
        let generation = renderer.generation();
        placed.clear();
        rules.clear();
        for line in &layout.lines[visible.clone()] {
            let baseline = line.y + line.baseline;
            for run in &line.runs {
                // Text is always painted fully opaque. The transparency slider fades the background
                // behind it and must never make the writing hard to read.
                let color = Color32::from_rgb(run.style.color.r, run.style.color.g, run.style.color.b);
                for cluster in &run.clusters {
                    for character in cluster.text.chars() {
                        // A rule is drawn rather than lettered, which is what
                        // `design/style-guide.md` already says about an icon. A box-drawing glyph
                        // cannot tile: its ink is an em box, the line it sits on is taller than
                        // that, and its bitmap is a whole pixel wider than its advance. So a rule
                        // made of them came out dotted across, and a column of them came out as a
                        // row of ticks. Drawn into the cell instead, a table's grid and a quote's
                        // bar join up exactly at any size and any line height. See `box_rules`.
                        if let Some(cell) = box_rules(character) {
                            let at = to_screen(cluster.x, line.y);
                            paint_a_box_cell(
                                &mut rules,
                                cell,
                                at,
                                cluster.advance,
                                line.height,
                                color,
                            );
                            continue;
                        }
                        let Some(glyph) = renderer.glyph(character, &run.style) else {
                            continue; // a space, or a character this font has no shape for
                        };
                        // Snap the glyph to whole pixels. A glyph is drawn at exactly the size it was
                        // rasterised at, so landing it on a fraction of a pixel would resample it and
                        // soften every letter on screen.
                        let at = to_screen(cluster.x + glyph.offset.x, baseline + glyph.offset.y);
                        let at = Pos2::new(at.x.round(), at.y.round());
                        placed.push((Rect::from_min_size(at, glyph.size), glyph.uv, color));
                    }
                }
            }
        }
        if renderer.generation() == generation {
            break;
        }
    }

    for (rect, colour) in &rules {
        painter.rect_filled(*rect, 0.0, *colour);
    }

    let texture = renderer.texture(ui.ctx());
    let mut mesh = Mesh::with_texture(texture);
    let count = placed.len();
    for (rect, uv, color) in placed {
        mesh.add_rect_with_uv(rect, uv, color);
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }

    for (rect, color) in layout.decorations_in(visible, renderer) {
        painter.rect_filled(
            Rect::from_min_size(to_screen(rect.x, rect.y), Vec2::new(rect.width, rect.height)),
            0.0,
            Color32::from_rgb(color.r, color.g, color.b),
        );
    }
    count
}

/// What the pointer did in a page that can be read but not typed into.
///
/// The Markdown preview is the one such page. It is a real `Layout` over a real rope — the same two
/// things the editing area has — so selecting in it is the same arithmetic; what it does not have is
/// a `Document`, because it is worked out from the source rather than edited. So this returns the
/// selection it would make and changes nothing, which is the rule every component in Unluminate follows.
///
/// A press puts the anchor down and clears what was selected, a drag extends it, a double click
/// takes the word and a triple click the line. `None` means the pointer did nothing this frame.
pub fn read_pointer(
    response: &egui::Response,
    layout: &Layout,
    text: &Rope,
    text_origin: Pos2,
    selection: Selection,
) -> Option<Selection> {
    let pointer = response.interact_pointer_pos()?;
    let local = pointer - text_origin;
    let offset = layout.offset_at(local.x, local.y);
    if response.triple_clicked() {
        let line = text.line_range(text.byte_to_line(offset));
        return Some(Selection::new(line.start, line.end.min(text.len_bytes())));
    }
    if response.double_clicked() {
        let word = word_at(text, offset);
        return Some(Selection::new(word.start, word.end));
    }
    if response.drag_started() {
        // Where the **press** was, not where the pointer is now. egui does not call a press a drag
        // until it has moved a few points, so by the frame that says a drag started the pointer has
        // already left the letter it was put down on — and the anchor belongs on that letter.
        let press = response.ctx.input(|input| input.pointer.press_origin()).unwrap_or(pointer);
        let from = press - text_origin;
        return Some(Selection::new(layout.offset_at(from.x, from.y), offset));
    }
    if response.dragged() {
        return Some(Selection { anchor: selection.anchor, head: offset });
    }
    if response.clicked() {
        return Some(Selection::caret(offset));
    }
    None
}

/// The word `offset` falls in, for a double click.
///
/// Word characters are letters, digits and the underscore, which is the same rule the editing area's
/// own word movement follows. The search is inside the line rather than the whole rope, because a
/// preview is one long rope and a line is the most a word can span.
pub fn word_at(text: &Rope, offset: usize) -> std::ops::Range<usize> {
    let line = text.line_range(text.byte_to_line(offset));
    let body = text.byte_slice(line.clone());
    let at = offset.saturating_sub(line.start).min(body.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = body[..at].char_indices().rev().take_while(|(_, c)| is_word(*c)).last();
    let start = start.map(|(index, _)| index).unwrap_or(at);
    let end = body[at..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map(|(index, _)| at + index)
        .unwrap_or(body.len());
    // A click on something that is not a word — a space, a table's rule — selects that one
    // character, so a drag from it still has somewhere to start.
    if start == end {
        let next = body[at..].chars().next().map(char::len_utf8).unwrap_or(0);
        return line.start + start..line.start + end + next;
    }
    line.start + start..line.start + end
}

/// Paint one range behind the text, which is what a selection and a code chip both are.
pub fn paint_behind(
    ui: &egui::Ui,
    layout: &Layout,
    text_origin: Pos2,
    range: std::ops::Range<usize>,
    color: Color32,
    rounding: f32,
) {
    if range.is_empty() {
        return;
    }
    let visible = visible_lines(ui, layout, text_origin);
    for rect in layout.selection_rects_in(visible, range) {
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(text_origin.x + rect.x, text_origin.y + rect.y),
                Vec2::new(rect.width, rect.height),
            ),
            rounding,
            color,
        );
    }
}

/// Which part of a cell each of a box-drawing character's two strokes covers.
///
/// `None` for a stroke the character does not have; `Some((from, to))` as fractions of the cell, so
/// `(0.0, 1.0)` is right across it and `(0.5, 1.0)` is from the middle to the right or bottom edge.
/// That is the whole of what a corner is.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BoxCell {
    across: Option<(f32, f32)>,
    down: Option<(f32, f32)>,
}

/// What the eleven box-drawing characters Unluminate writes are made of.
///
/// Only those: a rule, a quote's bar, and the ten pieces of a table's grid. Anything else in the
/// block is left to the font, because a character with a curve, a double line or a deliberate dash
/// in it is not two rectangles.
fn box_rules(character: char) -> Option<BoxCell> {
    let all = Some((0.0, 1.0));
    let first = Some((0.0, 0.5));
    let second = Some((0.5, 1.0));
    let cell = match character {
        '\u{2500}' => BoxCell { across: all, down: None },
        '\u{2502}' => BoxCell { across: None, down: all },
        '\u{250C}' => BoxCell { across: second, down: second },
        '\u{2510}' => BoxCell { across: first, down: second },
        '\u{2514}' => BoxCell { across: second, down: first },
        '\u{2518}' => BoxCell { across: first, down: first },
        '\u{251C}' => BoxCell { across: second, down: all },
        '\u{2524}' => BoxCell { across: first, down: all },
        '\u{252C}' => BoxCell { across: all, down: second },
        '\u{2534}' => BoxCell { across: all, down: first },
        '\u{253C}' => BoxCell { across: all, down: all },
        _ => return None,
    };
    Some(cell)
}

/// Put one cell's strokes into the list, joining each to the one before it where they touch.
///
/// Every edge is snapped to a whole pixel, and the bottom of one line is the top of the next by
/// construction, so a bar down a quote or a column of a table is continuous however tall the lines
/// are. A run of the same rule becomes one rectangle rather than forty-eight.
fn paint_a_box_cell(
    rules: &mut Vec<(Rect, Color32)>,
    cell: BoxCell,
    at: Pos2,
    width: f32,
    height: f32,
    color: Color32,
) {
    // A hairline at the ordinary reading size and thicker as the text grows, so a grid stays a grid
    // rather than becoming a fence.
    let thickness = (height * 0.05).round().max(1.0);
    let middle_x = (at.x + width / 2.0 - thickness / 2.0).round();
    let middle_y = (at.y + height / 2.0 - thickness / 2.0).round();
    if let Some((from, to)) = cell.across {
        let left = if from == 0.0 { at.x.floor() } else { middle_x };
        let right = if to == 1.0 { (at.x + width).ceil() } else { middle_x + thickness };
        add_rule(
            rules,
            Rect::from_min_max(
                Pos2::new(left, middle_y),
                Pos2::new(right, middle_y + thickness),
            ),
            color,
        );
    }
    if let Some((from, to)) = cell.down {
        let top = if from == 0.0 { at.y.round() } else { middle_y };
        let bottom = if to == 1.0 { (at.y + height).round() } else { middle_y + thickness };
        add_rule(
            rules,
            Rect::from_min_max(
                Pos2::new(middle_x, top),
                Pos2::new(middle_x + thickness, bottom),
            ),
            color,
        );
    }
}

/// Add a stroke, growing the one before it instead when the two are one line carried on.
fn add_rule(rules: &mut Vec<(Rect, Color32)>, rect: Rect, color: Color32) {
    if let Some((last, colour)) = rules.last_mut() {
        let carried_on = *colour == color
            && (last.top() - rect.top()).abs() < 0.5
            && (last.bottom() - rect.bottom()).abs() < 0.5
            && (last.right() - rect.left()).abs() < 0.5;
        if carried_on {
            last.max.x = rect.max.x;
            return;
        }
    }
    rules.push((rect, color));
}

/// The lines of `layout` that fall inside what `ui` is drawing into.
///
/// The clip rectangle is what says where the pane is: every caller sets it before painting, because
/// text scrolled out of an editing area must not be drawn over the tabs above it. It is therefore
/// also the honest answer to "what can be seen", and one function so that the selection, the marks,
/// the glyphs and the rules cannot come to different answers about it.
pub fn visible_lines(
    ui: &egui::Ui,
    layout: &Layout,
    text_origin: Pos2,
) -> std::ops::Range<usize> {
    let clip = ui.painter().clip_rect();
    layout.visible_lines(clip.top() - text_origin.y, clip.bottom() - text_origin.y)
}

/// Draw a thin border round the editing area so it is clear where text can be typed.
pub fn paint_frame(ui: &egui::Ui, area: Rect, color: Color32) {
    ui.painter().rect_stroke(area, 4.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
}

/// Reserve the editing area and make it take the keyboard.
pub fn allocate(ui: &mut egui::Ui) -> (Rect, egui::Response) {
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    (rect, response)
}

/// A convenience for the toolbar: the change that a size box should send.
pub fn size_change(size: f32) -> Command {
    Command::ApplyStyle(StyleChange::size(size.clamp(6.0, 144.0)))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::text_renderer::TextRenderer;
    use unluminate_core::{layout, CharStyle, ParagraphStyles, Rope, StyleSpans};

    /// Draw into a context with no window and no graphics card behind it, and give back what the
    /// painter reported. egui's context is all on the processor: a texture handed to it is a delta to
    /// be uploaded later, so nothing here needs a device.
    fn painted(clip: Rect, laid: &Layout, origin: Pos2) -> usize {
        let renderer = TextRenderer::new();
        let context = egui::Context::default();
        let mut placed = 0;
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(clip));
            inner.set_clip_rect(clip);
            placed = paint_text(&inner, &renderer, laid, origin);
        });
        output.drop_without_applying_deltas();
        placed
    }

    fn a_long_document(lines: usize) -> Layout {
        let text: String = (0..lines).map(|i| format!("line number {i} of the document\n")).collect();
        let rope = Rope::from_str(&text);
        let spans = StyleSpans::new(rope.len_bytes(), CharStyle::default());
        let paragraphs = ParagraphStyles::new(rope.len_lines());
        layout(&rope, &spans, &paragraphs, &TextRenderer::new(), 900.0)
    }

    /// **Painting costs a screenful, not a document.**
    ///
    /// The painter used to walk every line in the file and hand egui one mesh holding every glyph in
    /// it. egui culls a mesh only against its bounding box, and the bounding box of a whole document
    /// plainly overlaps the window, so all of it was tessellated and uploaded to the graphics card
    /// sixty times a second. Before this was fixed, this test reported every glyph in the file.
    #[test]
    fn painting_a_long_document_costs_a_screenful() {
        let laid = a_long_document(5000);
        let every = laid.lines.iter().flat_map(|line| line.runs.iter()).map(|run| run.clusters.len()).sum::<usize>();
        assert!(every > 100_000, "the fixture is meant to be far larger than one screen");

        let window = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(900.0, 700.0));
        let placed = painted(window, &laid, Pos2::new(0.0, 0.0));
        assert!(placed > 0, "the top of the document is on the screen, so something is drawn");
        assert!(
            placed < every / 20,
            "a seven hundred point window holds a small part of a five thousand line file, \
             so nothing like {every} glyphs should be placed for it: {placed} were"
        );
    }

    /// Scrolled a long way down, the same is true and the glyphs drawn are the ones down there.
    #[test]
    fn painting_a_document_scrolled_down_costs_the_same_screenful() {
        let laid = a_long_document(5000);
        let window = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(900.0, 700.0));
        let every = laid.lines.iter().flat_map(|line| line.runs.iter()).map(|run| run.clusters.len()).sum::<usize>();
        let top = painted(window, &laid, Pos2::new(0.0, 0.0));
        let scrolled = painted(window, &laid, Pos2::new(0.0, -20_000.0));
        assert!(scrolled > 0, "there is text at that depth");
        assert!(scrolled < every / 20, "and it is still one screenful, not {every} glyphs");
        // Within a line or two of each other, because the same amount of window is being filled.
        let difference = top.abs_diff(scrolled);
        assert!(difference < top / 4, "{top} at the top against {scrolled} scrolled down");
    }

    /// Nothing is drawn when the document is scrolled entirely past the window, which is the case
    /// that used to cost the most: every glyph collected and every one of them thrown away by the
    /// scissor rectangle.
    #[test]
    fn a_document_scrolled_past_the_window_paints_nothing() {
        let laid = a_long_document(200);
        let window = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(900.0, 700.0));
        assert_eq!(painted(window, &laid, Pos2::new(0.0, -1_000_000.0)), 0);
    }
}
