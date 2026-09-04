//! The window's half of the value tooltip: when to ask, where the popup hangs, and what puts it
//! away.
//!
//! `task-1696`. `quill_core::expressions` reads what the pointer is over, `app::debug::DebugState`
//! asks the debugger and holds the tree, `components::value_tooltip` draws it — and this is what
//! sits between them, which is exactly where `app::completion` sits between the same three kinds of
//! thing.
//!
//! ## The delay is the whole of the politeness
//!
//! Nothing is asked until the pointer has rested on one expression for [`HOVER_DELAY`]. Without it,
//! sweeping the pointer along one line of code fires four `evaluate` requests at a debugger — which
//! is wasteful, and against CodeLLDB 1.12.3 is worse than wasteful, because an expression that does
//! not resolve ends its session with a Python traceback and nothing on the protocol channel.
//!
//! 350 ms, and it is a constant rather than a setting: The reference editor's own `Value tooltip delay` exists
//! so somebody who finds the tooltip distracting can turn it down, and Quill's tick box already
//! turns it off. A pointer crossing a line passes over a word in far less than 350 ms, so nothing is
//! asked for a word merely passed over; a pointer that has come to rest is answered before anybody
//! notices waiting.
//!
//! ## What holds it open
//!
//! One rule: *the pointer is inside the union of the word's box and the popup's, grown by
//! [`HOVER_SLACK`]*. The union is what lets the pointer travel from the name into the popup without
//! crossing dead ground — the popup hangs `value_tooltip::GAP` below the letters, and a rule that
//! asked only about the popup would close it in the gap.
//!
//! A popup the keyboard asked for has no pointer behind it and lives until `Escape`, a resume, or
//! the caret moving.

use std::ops::Range;
use std::path::PathBuf;

use egui::{Pos2, Rect};

use crate::app::QuillApp;
use crate::components::value_tooltip;
use crate::services::file_kind;
use crate::settings::ValueTooltip;

/// How long the pointer has to rest on one expression before the debugger is asked about it.
pub const HOVER_DELAY: f64 = 0.350;
/// How far outside the word and the popup the pointer may stray before the popup is put away.
pub const HOVER_SLACK: f32 = 8.0;

/// The pointer resting on one expression, and since when.
///
/// Frame-to-frame state, and the only thing the delay needs: an expression that is not the one being
/// rested on replaces this, which starts the clock again.
#[derive(Debug, Clone, PartialEq)]
pub struct Resting {
    expression: String,
    range: Range<usize>,
    since: f64,
}

/// A value tooltip that is open, and everything about *where* it is.
///
/// What is *in* it — the question, the answer, the tree — lives on `DebugState`, because it is the
/// session's and dies with it. This is the window's half: the letters it hangs off, the pane it is
/// clamped inside, and the row being typed into.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueTooltipState {
    /// The expression it is about, which is also the root row's key.
    pub expression: String,
    /// Where in the file the letters are, so a click elsewhere and an edit both close it.
    pub range: Range<usize>,
    /// The file it is in. A tooltip belongs to one tab.
    pub path: Option<PathBuf>,
    /// The letters it was asked about, so an edit under it can be noticed.
    ///
    /// The bytes rather than `Document::text_revision`, and the reason is worth keeping: colouring
    /// the file **moves** the text revision — `Document::colour_by` counts as a change to the
    /// formatting, which is what `colour_the_file` is keyed on — so a popup keyed on it would put
    /// itself away on the first frame after it opened. What it really wants to know is whether the
    /// letters it is about are still there, and that is what this asks.
    pub was: String,
    /// True when `Debug -> Show Value` asked for it, so no pointer can take it away.
    pub by_hand: bool,
    /// The word's own box on the screen, recorded by the pane that drew it.
    pub word: Rect,
    /// The editing area it is in, which it is flipped and clamped inside.
    pub pane: Rect,
    /// Where the popup really ended up, once it has been drawn once.
    pub area: Option<Rect>,
    /// Which side of the word it went, settled the first time it was drawn and kept — see
    /// `components::value_tooltip::goes_above`.
    pub above: Option<bool>,
    /// The row being edited and what has been typed into it — `DebugPanel::editing`'s shape, and it
    /// is the same field passed to the same row-drawing function.
    pub editing: Option<(String, String)>,
}

impl ValueTooltipState {
    /// True while the pointer is somewhere that should hold the popup open.
    ///
    /// A pure function of two rectangles and a point, which is what makes it testable with no
    /// window. A popup that has not been drawn yet is held open by the word alone: it was asked for
    /// on the frame the pointer was on the name, and one frame later it will have an area.
    pub fn is_held_by(&self, pointer: Option<Pos2>) -> bool {
        if self.by_hand {
            return true;
        }
        let Some(pointer) = pointer else {
            return false;
        };
        let mut alive = self.word;
        if let Some(area) = self.area {
            alive = alive.union(area);
        }
        alive.expand(HOVER_SLACK).contains(pointer)
    }
}

/// Whether a pointer resting on a name is allowed to ask the debugger anything.
///
/// Two conditions and no more. `manual` is the reference editor's `Show value tooltip` switched off, and it stops
/// only the *unasked* popup — `Debug -> Show Value` and `quill-cli debug hover` work either way,
/// which is why the setting has two values rather than three. The modifier is Go to Definition's, and
/// two affordances on one word would be two promises the one click cannot both keep.
pub fn the_pointer_may_ask(setting: ValueTooltip, modifier_held: bool) -> bool {
    setting.is_automatic() && !modifier_held
}

impl QuillApp {
    /// Whether the value tooltip can apply to the file that is showing at all.
    ///
    /// The same two questions `Go to Definition` asks — this is a reading of identifiers, so a file
    /// whose language cannot answer them cannot answer this either — plus the one that matters most:
    /// **the program has to be stopped in this file**. A local of the paused frame means nothing at a
    /// word in another file that happens to be spelled the same, which is the rule the inline values
    /// already keep.
    pub fn value_tooltip_applies_here(&self) -> bool {
        let Some(debug) = self.debug.as_ref() else {
            return false;
        };
        if !debug.is_paused() {
            return false;
        }
        let Some(path) = self.files.active().path().map(PathBuf::from) else {
            return false;
        };
        if !file_kind::definitions_apply(Some(path.as_path()), &self.plugins.grammars()) {
            return false;
        }
        debug
            .location()
            .is_some_and(|(stopped_in, _)| crate::app::same_file(&path, &stopped_in))
    }

    /// The expression at a byte offset of the tab that is showing, as the tooltip reads it.
    ///
    /// One function, so the pointer, `Debug -> Show Value` and `quill-cli debug hover` cannot come to
    /// three different conclusions about what a point in a file is a question about.
    pub fn expression_at(&mut self, index: usize, offset: usize) -> Option<Range<usize>> {
        let grammar = self.grammar_for(self.files.at(index).path())?.clone();
        let text = self.files.at(index).document.text().to_string();
        let symbols = &self.tab_symbols(index).read;
        quill_core::expressions::at(&text, symbols, &grammar, offset)
    }

    /// Work the pointer for a frame, inside the pane being drawn.
    ///
    /// Called from `show_editor` beside `symbol_under_the_pointer`, and it is that function's
    /// opposite number: **while the platform's modifier is held there is no value tooltip**, because
    /// the modifier means Go to Definition and two affordances on one word would be two promises the
    /// one click cannot both keep. Modifier down asks *where is this defined*; modifier up asks *what
    /// does this hold*.
    pub(crate) fn value_under_the_pointer(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        origin: Pos2,
        area: Rect,
        focused: bool,
    ) {
        if !focused {
            return;
        }
        let held = ui.input(|input| input.modifiers.command);
        if !the_pointer_may_ask(self.settings.value_tooltip, held) {
            self.hover_rest = None;
            return;
        }
        if !self.value_tooltip_applies_here() {
            self.hover_rest = None;
            return;
        }
        // The pointer being somewhere else is not a reason to work anything out — and while it is
        // over the popup itself this is `None`, because the popup's own `Area` is in front and takes
        // the point. That is what lets the pointer walk into the tree without closing it.
        let Some(at) = response.hover_pos() else {
            self.hover_rest = None;
            return;
        };
        let index = self.files.active_index();
        let local = at - origin;
        let offset = self.layout().offset_at(local.x, local.y);
        let Some(range) = self.expression_at(index, offset) else {
            self.hover_rest = None;
            return;
        };
        let expression = self.text_in_tab(index, &range);
        // Already showing this one: keep it, and keep its geometry fresh so a scroll under it moves
        // it with the letters rather than leaving it hanging in the air.
        if self.value_tooltip.as_ref().is_some_and(|open| open.expression == expression) {
            let (word, pane) = (self.word_box(index, &range, origin), area);
            if let Some(open) = self.value_tooltip.as_mut() {
                open.word = word;
                open.pane = pane;
            }
            return;
        }
        let now = ui.input(|input| input.time);
        let resting = match self.hover_rest.as_ref() {
            Some(resting) if resting.expression == expression && resting.range == range => {
                resting.since
            }
            _ => {
                self.hover_rest =
                    Some(Resting { expression: expression.clone(), range: range.clone(), since: now });
                now
            }
        };
        let waited = now - resting;
        if waited < HOVER_DELAY {
            // An idle window draws nothing, so the frame that noticed the pointer land asks to be
            // woken when the rest is over. The heartbeat would get there eventually and half a second
            // is not 350 ms.
            let left = HOVER_DELAY - waited;
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left));
            return;
        }
        let word = self.word_box(index, &range, origin);
        self.open_the_value_tooltip(index, expression, range, word, area, false);
    }

    /// Open the popup on an expression, and ask the debugger about it.
    pub(crate) fn open_the_value_tooltip(
        &mut self,
        index: usize,
        expression: String,
        range: Range<usize>,
        word: Rect,
        pane: Rect,
        by_hand: bool,
    ) {
        let path = self.files.at(index).path().map(PathBuf::from);
        let was = expression.clone();
        self.value_tooltip = Some(ValueTooltipState {
            expression: expression.clone(),
            range,
            path,
            was,
            by_hand,
            word,
            pane,
            area: None,
            above: None,
            editing: None,
        });
        self.hover_rest = None;
        if let Some(debug) = self.debug.as_mut() {
            debug.ask_the_hover(&expression);
        }
    }

    /// `Debug -> Show Value`, which is the reference editor's Quick Evaluate chord asking about the caret's own
    /// word.
    ///
    /// It hangs off the caret rather than off a pointer that may be anywhere, and it lives until
    /// `Escape` or a resume — see the module comment.
    pub(crate) fn show_the_value_at_the_caret(&mut self) {
        self.close_the_value_tooltip();
        if !self.value_tooltip_applies_here() {
            self.message = Some("The program is not stopped in this file.".to_owned());
            return;
        }
        let index = self.files.active_index();
        let caret = self.files.at(index).document.selection().head;
        let Some(range) = self.expression_at(index, caret) else {
            self.message = Some("There is no name at the caret to ask about.".to_owned());
            return;
        };
        let expression = self.text_in_tab(index, &range);
        // The pane the caret is in, as the last frame drew it, which is the same rectangle the
        // status bar reads on the frame after — `editor_area`'s own arrangement.
        let pane = self.editor_area;
        let word = Rect::from_min_size(
            Pos2::new(pane.left(), pane.top()),
            egui::Vec2::new(1.0, 1.0),
        );
        self.open_the_value_tooltip(index, expression, range, word, pane, true);
        // Where it really hangs is worked out by the pane on the next frame, from the caret's own
        // box: this is only the fallback for a window that has not drawn since.
        self.caret_tooltip = true;
    }

    /// Put it away, and let go of the question with it.
    pub(crate) fn close_the_value_tooltip(&mut self) {
        self.value_tooltip = None;
        self.caret_tooltip = false;
        self.hover_rest = None;
        if let Some(debug) = self.debug.as_mut() {
            debug.forget_the_hover();
        }
    }

    /// True while a popup is open, which is what the key routing asks.
    pub fn value_tooltip_is_open(&self) -> bool {
        self.value_tooltip.is_some()
    }

    /// `Escape`, taken out of the frame's input before any pane reads it.
    ///
    /// The one-frame ordering `Find in Files`, `Go to File` and the completion popup already rely
    /// on. The modifiers are compared **for real** rather than through `InputState::consume_key`,
    /// which matches by `Modifiers::matches_logically` — a pattern of `NONE` matches `Shift+Escape`
    /// as well, which is `task-1678`'s trap and is not walked into a third time.
    pub(crate) fn route_the_value_tooltip_keys(&mut self, ui: &mut egui::Ui) {
        if self.value_tooltip.is_none() {
            return;
        }
        // A field inside the popup owns `Escape` while it is open — it means "stop editing" there,
        // which is `show_row`'s own rule and is the narrower meaning.
        if self.value_tooltip.as_ref().is_some_and(|open| open.editing.is_some()) {
            return;
        }
        let pressed = ui.input_mut(|input| {
            let mut taken = false;
            input.events.retain(|event| {
                let escape = matches!(
                    event,
                    egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. }
                        if modifiers.is_none()
                );
                taken |= escape;
                !escape
            });
            taken
        });
        if pressed {
            self.close_the_value_tooltip();
        }
    }

    /// Everything that closes the popup other than the pointer leaving it.
    ///
    /// Asked once a frame, after the panes, for `follow_the_open_file`'s reason: a list of the places
    /// that have to remember to close it is a list whose next entry will be the one that forgot.
    fn the_value_tooltip_has_stopped_being_an_answer(&self) -> bool {
        let Some(open) = self.value_tooltip.as_ref() else {
            return false;
        };
        let index = self.files.active_index();
        if self.files.at(index).path().map(PathBuf::from) != open.path {
            return true;
        }
        let text = self.files.at(index).document.text();
        if open.range.end > text.len_bytes()
            || text.byte_slice(open.range.clone()).to_string() != open.was
        {
            return true;
        }
        // The session went on, ended, or was never there: every reference in the tree died with it,
        // and a value from the last stop drawn beside a tree that cannot be opened is worse than
        // nothing.
        self.debug.as_ref().is_none_or(|debug| !debug.is_paused() || debug.hover.is_none())
    }

    /// Draw the popup and act on what happened in it.
    ///
    /// After the pane loop, for the reason the completion popup is drawn there: this is the first
    /// moment anything knows where the pane's letters ended up, and one popup drawn here can never be
    /// underneath a divider or drawn twice in a split view.
    pub(crate) fn show_the_value_tooltip(&mut self, ui: &mut egui::Ui) {
        if self.the_value_tooltip_has_stopped_being_an_answer() {
            self.close_the_value_tooltip();
            return;
        }
        let (Some(open), Some(debug)) = (self.value_tooltip.as_ref(), self.debug.as_ref()) else {
            return;
        };
        let Some(hover) = debug.hover.as_ref() else {
            return;
        };
        let (word, pane, above) = (open.word, open.pane, open.above);
        let can_set_root = debug.can_set_the_root();
        let can_set_child = debug.capabilities().set_variable;
        let mut editing = open.editing.clone();
        let outcome = value_tooltip::show(
            ui,
            hover,
            &mut editing,
            can_set_root,
            can_set_child,
            word,
            pane,
            above,
        );
        if let Some(open) = self.value_tooltip.as_mut() {
            open.editing = editing;
            if outcome.area.is_some() {
                open.area = outcome.area;
                open.above = outcome.above;
            }
        }
        if let Some(key) = outcome.toggle_row {
            if let Some(debug) = self.debug.as_mut() {
                debug.toggle_hover_row(&key);
            }
        }
        if let Some((key, value)) = outcome.set_value {
            if let Some(debug) = self.debug.as_mut() {
                if let Err(said) = debug.set_hover_value(&key, &value) {
                    self.message = Some(said);
                }
            }
        }
        // And last, whether the pointer is still somewhere that holds it open. After the drawing,
        // because the popup's own rectangle is half of the answer and this frame is the first that
        // knows it.
        let pointer = ui.ctx().input(|input| input.pointer.latest_pos());
        let held = self
            .value_tooltip
            .as_ref()
            .is_some_and(|open| open.is_held_by(pointer) || open.editing.is_some());
        if !held {
            self.close_the_value_tooltip();
        }
    }

    /// The box on the screen of a stretch of the tab's text, which is what the popup hangs off.
    ///
    /// The same arithmetic the caret is painted with, so the popup follows the letters rather than a
    /// remembered point. A range that wraps across two lines — a field path broken over a line end —
    /// hangs off the line it **ends** on, which is the one the pointer was on.
    fn word_box(&mut self, index: usize, range: &Range<usize>, origin: Pos2) -> Rect {
        let layout = &self.files.at(index).cached.layout;
        let start = layout.caret_at(range.start);
        let end = layout.caret_at(range.end);
        let box_of = |caret: quill_core::layout::Caret, right: f32| {
            Rect::from_min_max(
                Pos2::new(origin.x + caret.x, origin.y + caret.y),
                Pos2::new(origin.x + right, origin.y + caret.y + caret.height),
            )
        };
        match start.line == end.line {
            true => box_of(start, end.x),
            false => box_of(end, end.x + 1.0),
        }
    }

    /// A stretch of one tab's text.
    fn text_in_tab(&self, index: usize, range: &Range<usize>) -> String {
        self.files.at(index).document.text().byte_slice(range.clone()).to_string()
    }

    /// Keep a keyboard-asked popup hanging off the caret, from the pane that has the keyboard.
    ///
    /// `Debug -> Show Value` is asked for outside a frame, where nothing knows where the caret ended
    /// up on the screen — the same problem `remember_where_the_completion_hangs` solves, solved the
    /// same way and in the same place.
    pub(crate) fn remember_where_the_value_tooltip_hangs(&mut self, origin: Pos2, area: Rect) {
        if !self.caret_tooltip {
            return;
        }
        // The pane loop borrows the focus, so `active()` is the pane being drawn — and the popup
        // belongs to the pane showing the file it is about, whatever has the keyboard. It is asked
        // for from a menu, and a menu can be used with the explorer focused.
        let index = self.files.active_index();
        if self.files.at(index).path().map(PathBuf::from)
            != self.value_tooltip.as_ref().and_then(|open| open.path.clone())
        {
            return;
        }
        let Some(range) = self.value_tooltip.as_ref().map(|open| open.range.clone()) else {
            return;
        };
        let word = self.word_box(index, &range, origin);
        if let Some(open) = self.value_tooltip.as_mut() {
            open.word = word;
            open.pane = area;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_at(word: Rect, area: Option<Rect>, by_hand: bool) -> ValueTooltipState {
        ValueTooltipState {
            expression: "count".to_owned(),
            range: 0..5,
            path: None,
            was: "count".to_owned(),
            by_hand,
            word,
            pane: Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(600.0, 400.0)),
            area,
            above: Some(false),
            editing: None,
        }
    }

    fn word() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 100.0), egui::Vec2::new(40.0, 16.0))
    }

    fn popup() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 120.0), egui::Vec2::new(240.0, 80.0))
    }

    /// `manual` stops the popup arriving unasked and nothing else, and the modifier belongs to Go to
    /// Definition — `task-1678`'s own test made once more, for the other popup.
    #[test]
    fn manual_stops_the_unasked_popup_and_so_does_the_definition_modifier() {
        assert!(the_pointer_may_ask(ValueTooltip::Automatic, false));
        assert!(!the_pointer_may_ask(ValueTooltip::Manual, false));
        assert!(!the_pointer_may_ask(ValueTooltip::Automatic, true));
    }

    #[test]
    fn the_pointer_on_the_word_holds_it_open() {
        let open = open_at(word(), Some(popup()), false);
        assert!(open.is_held_by(Some(Pos2::new(110.0, 105.0))));
    }

    #[test]
    fn the_pointer_in_the_popup_holds_it_open() {
        let open = open_at(word(), Some(popup()), false);
        assert!(open.is_held_by(Some(Pos2::new(200.0, 180.0))));
    }

    /// The popup hangs `GAP` below the letters, so a rule that asked only about the popup would put
    /// it away in the gap the pointer has to cross to reach it.
    #[test]
    fn the_gap_between_the_word_and_the_popup_holds_it_open() {
        let open = open_at(word(), Some(popup()), false);
        assert!(open.is_held_by(Some(Pos2::new(110.0, 118.0))));
    }

    #[test]
    fn the_pointer_away_from_both_puts_it_away() {
        let open = open_at(word(), Some(popup()), false);
        assert!(!open.is_held_by(Some(Pos2::new(400.0, 300.0))));
        assert!(!open.is_held_by(None), "and a pointer that has left the window entirely");
    }

    /// One the keyboard asked for has no pointer behind it, so no pointer can take it away.
    #[test]
    fn one_asked_for_by_hand_is_not_held_by_the_pointer_at_all() {
        let open = open_at(word(), Some(popup()), true);
        assert!(open.is_held_by(Some(Pos2::new(400.0, 300.0))));
        assert!(open.is_held_by(None));
    }

    /// It is asked for on the frame the pointer is on the name, so on that frame there is no popup
    /// rectangle yet — and closing it then would mean it never opened at all.
    #[test]
    fn a_popup_that_has_not_been_drawn_yet_is_held_by_the_word_alone() {
        let open = open_at(word(), None, false);
        assert!(open.is_held_by(Some(Pos2::new(110.0, 105.0))));
    }
}
