//! The editing surface: keyboard and mouse input in, painted text out.
//!
//! Nothing here decides what an edit means. Every key press becomes a `quill_core::Command` and the
//! document decides. Painting walks the lines the layout produced and draws one textured rectangle per
//! glyph out of the atlas, so the whole visible document is a single mesh.

use egui::{Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use quill_core::{Align, Command, Document, Layout, StyleChange};

use crate::services::text_renderer::TextRenderer;

/// Space between the text and the edge of the editing area.
pub const PADDING: f32 = 16.0;
/// Width of the caret.
const CARET_WIDTH: f32 = 2.0;

/// What the window worked out about the symbol under the pointer, before any click.
///
/// The component resolves nothing: it has no index, no project and no idea what a definition is.
/// The window asks those questions once a frame while the modifier is held — cached, so a pointer
/// resting still costs nothing — and hands the answer down, which is the same "components report,
/// the window decides" rule every component in Quill follows.
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
    let events = ui.input(|input| input.events.clone());
    for event in events {
        match event {
            egui::Event::Text(text) => {
                if !text.chars().any(|c| c.is_control()) {
                    outcome.changed |= document.apply(Command::Insert(text));
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
                // consume the press — nothing in Quill does — so without this guard one press of
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
                        document.apply(Command::Insert("\t".to_owned()))
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
pub struct PaintStyle {
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
}

pub fn paint(
    ui: &egui::Ui,
    renderer: &TextRenderer,
    document: &Document,
    layout: &Layout,
    text_origin: Pos2,
    style: PaintStyle,
) {
    let PaintStyle { selection: selection_color, caret: caret_color, show_caret, underline } =
        style;
    let painter = ui.painter();
    let to_screen = |x: f32, y: f32| Pos2::new(text_origin.x + x, text_origin.y + y);
    let visible = visible_lines(ui, layout, text_origin);

    for rect in layout.selection_rects_in(visible.clone(), document.selection().range()) {
        painter.rect_filled(
            Rect::from_min_size(to_screen(rect.x, rect.y), Vec2::new(rect.width, rect.height)),
            2.0,
            selection_color,
        );
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
}

/// Paint the marked passages, over the selection and under the text.
///
/// The order is not arbitrary. A passage marked while it is still selected has to be visible at the
/// moment it is marked, and the selection's own colour is opaque — so the mark goes over it, at
/// whatever alpha was chosen, and the selection shows through. And it goes under the text, because a
/// highlight is a background: the writing over it is painted at full alpha like every other glyph in
/// Quill.
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
    for _ in 0..3 {
        let generation = renderer.generation();
        placed.clear();
        for line in &layout.lines[visible.clone()] {
            let baseline = line.y + line.baseline;
            for run in &line.runs {
                // Text is always painted fully opaque. The transparency slider fades the background
                // behind it and must never make the writing hard to read.
                let color = Color32::from_rgb(run.style.color.r, run.style.color.g, run.style.color.b);
                for cluster in &run.clusters {
                    for character in cluster.text.chars() {
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
    use quill_core::{layout, CharStyle, ParagraphStyles, Rope, StyleSpans};

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
