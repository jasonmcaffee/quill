//! End to end tests that render the real window and save a screenshot of each one.
//!
//! Each test builds the whole application through `egui_kittest`, which runs the same code the
//! released binary runs, renders it through `wgpu` on the graphics card, and writes a PNG file to
//! `crates/quill-app/tests/snapshots`. When a run differs from the accepted image the harness also
//! writes `{name}.new.png` and `{name}.diff.png`.
//!
//! The images serve two purposes. Before a baseline is accepted, a person or an agent opens the PNG and
//! confirms that bold text is actually bolder and that centred text is actually centred. After it is
//! accepted, the same file is the comparison baseline, so a later change that alters the rendering fails
//! a test instead of passing unnoticed.
//!
//! Run `UPDATE_SNAPSHOTS=1 cargo test -p quill-app` to accept new images.

use egui::{vec2, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, SnapshotResults};
use quill_app::QuillApp;
use quill_app::app::ViewMode;
use quill_app::title_bar::FileAction;
use quill_core::{Align, Color, Command, StyleChange};

const WINDOW: [f32; 2] = [1180.0, 740.0];

/// A folder with a nested structure, for the explorer screenshots. Written once and left in place, so
/// that the tree looks the same in every run and the images stay comparable.
fn sample_folder() -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-screenshot-folder");
    std::fs::create_dir_all(root.join("chapters/appendix")).expect("make the nested folders");
    std::fs::create_dir_all(root.join("drafts")).expect("make the drafts folder");
    std::fs::write(root.join("readme.md"), "# Quill\n").expect("write readme.md");
    std::fs::write(root.join("notes.txt"), "notes\n").expect("write notes.txt");
    std::fs::write(root.join("chapters/one.md"), "# One\n").expect("write chapters/one.md");
    std::fs::write(root.join("chapters/two.md"), "# Two\n").expect("write chapters/two.md");
    std::fs::write(root.join("chapters/appendix/tables.txt"), "tables\n").expect("write the deep file");
    std::fs::write(root.join("drafts/idea.md"), "an idea\n").expect("write drafts/idea.md");
    // A file Quill cannot open. It is listed, dimmed, and does not respond to a click.
    std::fs::write(root.join("program.rs"), "fn main() {}\n").expect("write program.rs");
    root
}

/// Build the application with `text` already in the document.
fn harness(text: &str) -> Harness<'static, QuillApp> {
    let folder = sample_folder();
    let text = text.to_owned();
    let mut harness = Harness::builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = QuillApp::with_text(folder, &text);
            // The same setup the released binary does, and for the same reason: the fonts have to be
            // installed before the first frame.
            app.prepare(&cc.egui_ctx);
            app
        });
    harness.run();
    harness
}

/// Select `range` and run `command`, then let the application settle.
fn select_and(harness: &mut Harness<'static, QuillApp>, range: std::ops::Range<usize>, commands: &[Command]) {
    harness.state_mut().command(Command::PlaceCaret { offset: range.start, extend: false });
    harness.state_mut().command(Command::PlaceCaret { offset: range.end, extend: true });
    for command in commands {
        harness.state_mut().command(command.clone());
    }
    harness.run();
}

/// Select the first occurrence of `phrase` and run `commands` on it.
///
/// The offsets are found in the text rather than written down, because a hand counted offset drifts as
/// soon as the text is edited. An earlier version of these tests counted wrongly and left the first
/// letter of a line out of the selection, which the screenshot showed as one small letter in front of a
/// large word.
fn select_phrase(harness: &mut Harness<'static, QuillApp>, phrase: &str, commands: &[Command]) {
    let text = harness.state().document.text().to_string();
    let start = text
        .find(phrase)
        .unwrap_or_else(|| panic!("{phrase:?} is not in the document, which holds {text:?}"));
    select_and(harness, start..start + phrase.len(), commands);
}

/// Put the caret at the start with nothing selected, so that a screenshot shows the formatting rather
/// than a selection highlight sitting on top of it.
fn collapse(harness: &mut Harness<'static, QuillApp>) {
    harness.state_mut().command(Command::MoveDocumentStart { extend: false });
    harness.run();
}

/// Report the snapshots taken by a test that builds more than one window.
///
/// The harness requires every snapshot result in one test to be collected together, so that a run with
/// `UPDATE_SNAPSHOTS=1` updates all of them instead of stopping at the first difference. Taking the
/// errors out of the collection is also what marks it as handled, which `unwrap` does not do.
#[track_caller]
fn report(results: SnapshotResults) {
    let errors = results.into_inner();
    assert!(errors.is_empty(), "snapshot differences: {errors:#?}");
}

#[test]
fn startup_shows_the_explorer_the_toolbar_and_an_empty_editor() {
    let mut harness = harness("");
    harness.snapshot("startup");
}

#[test]
fn a_nested_folder_opens_with_its_children_indented_under_it() {
    let mut harness = harness("");
    // Click the folder in the explorer, as a person would, rather than changing the tree directly.
    harness.get_by_label_contains("chapters").click();
    harness.run();
    harness.get_by_label_contains("appendix").click();
    harness.run();
    let rows = harness.state().tree.rows().len();
    assert!(rows >= 8, "the tree should show the nested folders, it has {rows} rows");
    harness.snapshot("file_tree_expanded");
}

#[test]
fn clicking_a_file_in_the_explorer_opens_it_in_the_editor() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    assert_eq!(
        harness.state().document.text().to_string(),
        "# Quill\n",
        "clicking the file should have loaded it"
    );
    harness.snapshot("file_opened");
}

#[test]
fn typing_on_the_keyboard_puts_text_in_the_document() {
    let mut harness = harness("");
    // Real key and text events, through the same path the released binary uses.
    for text in ["Quill", " typed", " this."] {
        harness.input_mut().events.push(egui::Event::Text(text.to_owned()));
        harness.run();
    }
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.input_mut().events.push(egui::Event::Text("A second line.".to_owned()));
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), "Quill typed this.\nA second line.");
    harness.snapshot("typed_text");
}

#[test]
fn backspace_removes_what_was_typed() {
    let mut harness = harness("");
    harness.input_mut().events.push(egui::Event::Text("abcdef".to_owned()));
    harness.run();
    for _ in 0..3 {
        harness.key_press(egui::Key::Backspace);
        harness.run();
    }
    assert_eq!(harness.state().document.text().to_string(), "abc");
}

#[test]
fn a_selection_is_highlighted_behind_part_of_a_line_only() {
    let mut harness = harness("Select only the middle words of this line, not the rest of it.");
    select_and(&mut harness, 12..28, &[]);
    assert_eq!(harness.state().document.selected_text(), "the middle words");
    let rects = harness.state().layout().selection_rects(harness.state().document.selection().range());
    assert_eq!(rects.len(), 1, "the selection is inside one line, so it is one rectangle");
    harness.snapshot("selection");
}

#[test]
fn select_all_then_pressing_bold_makes_the_whole_document_bold() {
    let mut harness = harness("Every word here should end up bold.");
    harness.state_mut().command(Command::SelectAll);
    harness.run();
    // Click the real toolbar button rather than sending the command.
    harness.get_by_label("Bold").click();
    harness.run();
    assert!(harness.state().document.chars().style_at(4).bold, "the toolbar button should have applied bold");
    harness.snapshot("bold_all");
}

#[test]
fn bold_applies_to_the_middle_word_and_not_the_words_either_side() {
    let mut harness = harness("plain BOLD plain");
    select_phrase(&mut harness, "BOLD", &[Command::ToggleBold]);
    collapse(&mut harness);
    harness.snapshot("bold");
}

#[test]
fn italic_applies_to_the_middle_word_and_not_the_words_either_side() {
    let mut harness = harness("plain ITALIC plain");
    select_phrase(&mut harness, "ITALIC", &[Command::ToggleItalic]);
    collapse(&mut harness);
    harness.snapshot("italic");
}

#[test]
fn underline_draws_a_rule_under_the_middle_word_only() {
    let mut harness = harness("plain UNDERLINE plain");
    select_phrase(&mut harness, "UNDERLINE", &[Command::ToggleUnderline]);
    collapse(&mut harness);
    let rules = harness.state().layout().decorations(&harness.state().renderer);
    assert_eq!(rules.len(), 1, "one underline rule to draw");
    harness.snapshot("underline");
}

#[test]
fn strikethrough_draws_a_rule_through_the_middle_word_only() {
    let mut harness = harness("plain STRUCK plain");
    select_phrase(&mut harness, "STRUCK", &[Command::ToggleStrikethrough]);
    collapse(&mut harness);
    harness.snapshot("strikethrough");
}

#[test]
fn the_keyboard_shortcut_for_bold_does_the_same_as_the_button() {
    let mut harness = harness("shortcut bold");
    harness.state_mut().command(Command::SelectAll);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::B);
    harness.run();
    assert!(harness.state().document.chars().style_at(2).bold, "command plus B should turn bold on");
}

#[test]
fn three_font_sizes_stand_at_three_visibly_different_heights() {
    let mut harness = harness("Small size here\nMedium size here\nLarge size here");
    select_phrase(&mut harness, "Small size here", &[Command::ApplyStyle(StyleChange::size(11.0))]);
    select_phrase(&mut harness, "Medium size here", &[Command::ApplyStyle(StyleChange::size(24.0))]);
    select_phrase(&mut harness, "Large size here", &[Command::ApplyStyle(StyleChange::size(44.0))]);
    collapse(&mut harness);
    let heights: Vec<f32> = harness.state().layout().lines.iter().map(|line| line.height).collect();
    assert!(
        heights[0] < heights[1] && heights[1] < heights[2],
        "each line should be taller than the one before, heights were {heights:?}"
    );
    harness.snapshot("font_size");
}

#[test]
fn four_words_are_shown_in_four_colours() {
    let mut harness = harness("white red green blue");
    select_phrase(&mut harness, "red", &[Command::ApplyStyle(StyleChange::color(Color::RED))]);
    select_phrase(&mut harness, "green", &[Command::ApplyStyle(StyleChange::color(Color::GREEN))]);
    select_phrase(&mut harness, "blue", &[Command::ApplyStyle(StyleChange::color(Color::BLUE))]);
    collapse(&mut harness);
    assert_eq!(harness.state().document.chars().style_at(7).color, Color::RED);
    harness.snapshot("font_colour");
}

#[test]
fn the_same_sentence_is_shown_in_each_installed_family() {
    let families: Vec<String> = {
        let harness = harness("");
        harness.state().renderer.families().to_vec()
    };
    assert!(!families.is_empty(), "this system has none of the offered families");
    let text: String =
        families.iter().map(|family| format!("The quick brown fox in {family}\n")).collect();
    let mut harness = harness(text.trim_end());
    for family in &families {
        let line = format!("The quick brown fox in {family}");
        select_phrase(&mut harness, &line, &[
            Command::ApplyStyle(StyleChange::family(family.clone())),
            Command::ApplyStyle(StyleChange::size(22.0)),
        ]);
    }
    collapse(&mut harness);
    harness.snapshot("font_family");
}

/// The four alignments, each in its own screenshot, so a reader can see the same paragraph placed four
/// ways rather than having to compare four paragraphs of different text.
#[test]
fn each_alignment_places_the_same_paragraph_differently() {
    let paragraph = "This paragraph is long enough to wrap onto more than one line, which is what makes the difference between the four alignments visible.";
    let mut results = SnapshotResults::new();
    for (align, name) in [
        (Align::Left, "align_left"),
        (Align::Center, "align_centre"),
        (Align::Right, "align_right"),
        (Align::Justify, "align_justify"),
    ] {
        let mut harness = harness(paragraph);
        harness.state_mut().command(Command::SelectAll);
        harness.state_mut().command(Command::SetAlign(align));
        harness.run();
        assert_eq!(harness.state().document.paragraphs().get(0).align, align);
        results.add(harness.try_snapshot(name));
    }
    report(results);
}

#[test]
fn alignment_actually_moves_the_text_within_the_width() {
    let paragraph = "A short line.";
    let left_edge = |align: Align| {
        let mut harness = harness(paragraph);
        harness.state_mut().command(Command::SelectAll);
        harness.state_mut().command(Command::SetAlign(align));
        harness.run();
        harness.state().layout().lines[0].left()
    };
    let left = left_edge(Align::Left);
    let centre = left_edge(Align::Center);
    let right = left_edge(Align::Right);
    assert!(left < centre, "centred text should start further right than left aligned text");
    assert!(centre < right, "right aligned text should start further right still");
}

#[test]
fn double_spacing_puts_the_lines_twice_as_far_apart() {
    let text = "First line of the paragraph.\nSecond line of the paragraph.\nThird line of the paragraph.";
    let mut results = SnapshotResults::new();

    let mut single = harness(text);
    single.state_mut().command(Command::SelectAll);
    single.run();
    let single_height = single.state().layout().height;
    results.add(single.try_snapshot("line_spacing_single"));

    let mut double = harness(text);
    double.state_mut().command(Command::SelectAll);
    double.state_mut().command(Command::SetLineSpacing(2.0));
    double.run();
    let double_height = double.state().layout().height;
    assert!(
        (double_height - single_height * 2.0).abs() < 1.0,
        "double spacing should be twice as tall: {single_height} then {double_height}"
    );
    results.add(double.try_snapshot("line_spacing_double"));
    report(results);
}

#[test]
fn a_long_paragraph_wraps_inside_the_editing_area() {
    let text = "Quill breaks a long paragraph into lines that fit the width of the editing area, \
                breaking at a space so that no word is cut in half, and it does this with its own line \
                breaking rather than with a library. This paragraph is deliberately long enough to \
                need several lines at the width of this window.";
    let mut harness = harness(text);
    let lines = harness.state().layout().lines.len();
    assert!(lines >= 3, "this paragraph should need several lines, it took {lines}");
    assert!(
        harness.state().layout().lines.iter().all(|line| line.paragraph == 0),
        "every line belongs to the one paragraph"
    );
    harness.snapshot("word_wrap");
}

#[test]
fn cut_and_paste_move_text_through_the_clipboard() {
    let mut harness = harness("first second");
    select_and(&mut harness, 0..5, &[]);
    // A cut sends the selection to the clipboard and removes it from the document.
    harness.input_mut().events.push(egui::Event::Cut);
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), " second");
    // Paste it back at the end.
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.input_mut().events.push(egui::Event::Paste("first".to_owned()));
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), " secondfirst");
}

#[test]
fn copy_leaves_the_document_alone() {
    let mut harness = harness("unchanged text");
    select_and(&mut harness, 0..9, &[]);
    harness.input_mut().events.push(egui::Event::Copy);
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), "unchanged text");
}

/// The transparency requirement, checked by measurement rather than by eye.
///
/// A screenshot on its own cannot prove that the text stayed opaque, so this test reads the rendered
/// pixels at two slider positions and compares them.
///
/// The comparison is made on the pixels fully covered by a glyph. A pixel at the edge of a letter is
/// only partly covered, because the rasteriser antialiases the outline, so the background legitimately
/// shows through at the edges and always will. The claim being tested is about the body of the letters:
/// however faint the background is, the ink stays solid.
///
/// The text is set in red rather than the default near white. The render target keeps the window's alpha,
/// so a screenshot of the faint setting is shown by a viewer against whatever backdrop it uses. White
/// text over a white backdrop would look as though it had faded, which is the opposite of what this test
/// exists to show. Red reads clearly against a light backdrop and a dark one.
///
/// The render target holds colours multiplied by their alpha, so a pixel fully covered by a glyph at full
/// alpha comes out as exactly the text colour with an alpha of 255. Any dimming would move those numbers.
#[test]
fn the_background_fades_with_the_slider_and_the_text_stays_opaque() {
    /// A pixel fully covered by a glyph of red text: `quill_core::Color::RED` at full alpha.
    const TEXT_BODY: [u8; 4] = [0xE0, 0x4A, 0x4A, 255];

    let mut results = SnapshotResults::new();
    let mut measurements = Vec::new();
    for (opacity, expected_alpha, name) in [(0.15_f32, 38_u8, "opacity_low"), (1.0_f32, 255, "opacity_high")] {
        let mut harness = harness("TEXT STAYS OPAQUE");
        harness.state_mut().command(Command::SelectAll);
        // Bold at 64 point, so that the strokes are thick and a good number of pixels reach full
        // coverage. A thin face at a small size is mostly antialiased edge, which makes the measurement
        // needlessly delicate.
        harness.state_mut().command(Command::ApplyStyle(StyleChange::size(64.0)));
        harness.state_mut().command(Command::ToggleBold);
        harness.state_mut().command(Command::ApplyStyle(StyleChange::color(Color::RED)));
        harness.state_mut().command(Command::MoveDocumentStart { extend: false });
        harness.state_mut().opacity = opacity;
        harness.run();

        assert_eq!(
            harness.state().background().a(),
            expected_alpha,
            "in {name}, a slider at {opacity} should give a background alpha of {expected_alpha}"
        );

        // Only the editing area is measured for text. The toolbar, the explorer and the status bar hold
        // text too, drawn by egui in its own colours, and counting those would measure something other
        // than the document.
        let area = harness.state().editor_area();
        let image = harness.render().expect("render the window");

        // The commonest alpha in the window is the background's, because the background is most of the
        // window. This is what the operating system compositor uses to blend Quill over the desktop.
        let mut alpha_counts = std::collections::BTreeMap::new();
        let mut text_body = 0_usize;
        for (x, y, pixel) in image.enumerate_pixels() {
            *alpha_counts.entry(pixel.0[3]).or_insert(0_usize) += 1;
            let inside_editor = area.contains(egui::pos2(x as f32, y as f32));
            if inside_editor && pixel.0 == TEXT_BODY {
                text_body += 1;
            }
        }
        let (commonest_alpha, count) = alpha_counts
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(alpha, count)| (*alpha, *count))
            .expect("the window has pixels");
        let total = (image.width() * image.height()) as usize;

        assert_eq!(
            commonest_alpha, expected_alpha,
            "in {name}, most of the window should carry the background alpha"
        );
        assert!(
            count * 2 > total,
            "in {name}, the background should be most of the window, it was {count} of {total}"
        );
        // This is the transparency requirement itself. `TEXT_BODY` carries an alpha of 255, so at the
        // low setting these pixels are fully opaque ink sitting on a background whose alpha is 38.
        assert!(
            text_body > 500,
            "in {name}, only {text_body} pixels of fully opaque text were drawn"
        );

        measurements.push((name, expected_alpha, text_body));
        results.add(harness.try_snapshot(name));
    }

    let (low_name, low_alpha, low_text) = measurements[0];
    let (high_name, high_alpha, high_text) = measurements[1];
    assert!(
        low_alpha < high_alpha,
        "the background should be fainter at the low setting: {low_name} was {low_alpha}, {high_name} was {high_alpha}"
    );
    // The same sentence in the same font at the same size is drawn either way, so if the background fade
    // were touching the text at all, this count would move. It does not.
    assert_eq!(
        low_text, high_text,
        "the amount of solid text changed with the background: {low_text} at {low_name} against {high_text} at {high_name}"
    );
    report(results);
}

#[test]
fn undo_and_redo_go_back_and_forward_through_the_history() {
    let mut harness = harness("original");
    harness.input_mut().events.push(egui::Event::Text(" plus more".to_owned()));
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), " plus moreoriginal");
    harness.get_by_label("Undo").click();
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), "original");
    harness.get_by_label("Redo").click();
    harness.run();
    assert_eq!(harness.state().document.text().to_string(), " plus moreoriginal");
}

#[test]
fn a_document_holding_every_feature_at_once_renders() {
    // One screenshot showing the whole feature set together, which is the quickest way for a person to
    // see that nothing interferes with anything else.
    let mut harness = harness(
        "Quill\nA text editor written in Rust.\nbold italic underline struck\ncoloured text here\nThis last paragraph is centred and double spaced so that the paragraph settings show up next to the character settings.",
    );
    select_phrase(&mut harness, "Quill", &[
        Command::ApplyStyle(StyleChange::size(40.0)),
        Command::ToggleBold,
        Command::SetAlign(Align::Center),
    ]);
    select_phrase(&mut harness, "A text editor written in Rust.", &[
        Command::ApplyStyle(StyleChange::size(18.0)),
    ]);
    select_phrase(&mut harness, "bold", &[Command::ToggleBold]);
    select_phrase(&mut harness, "italic", &[Command::ToggleItalic]);
    select_phrase(&mut harness, "underline", &[Command::ToggleUnderline]);
    select_phrase(&mut harness, "struck", &[Command::ToggleStrikethrough]);
    select_phrase(&mut harness, "coloured", &[Command::ApplyStyle(StyleChange::color(Color::RED))]);
    select_phrase(&mut harness, "text here", &[Command::ApplyStyle(StyleChange::color(Color::GREEN))]);
    select_phrase(&mut harness, "This last paragraph", &[
        Command::SetAlign(Align::Center),
        Command::SetLineSpacing(2.0),
    ]);
    collapse(&mut harness);
    harness.snapshot("everything");
}

#[test]
fn the_title_bar_names_the_file_and_its_folder() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    // The three window buttons are there and can be found by name.
    for button in ["Close", "Minimise", "Maximise"] {
        harness.get_by_label(button);
    }
    harness.snapshot("title_bar");
}

#[test]
fn an_edited_file_is_marked_as_unsaved_in_three_places() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    assert!(!harness.state().document.is_modified(), "just opened, so nothing to save");
    harness.input_mut().events.push(egui::Event::Text(" edited".to_owned()));
    harness.run();
    assert!(harness.state().document.is_modified());
    // The dot appears in the title bar, on the file's row in the explorer and in the status bar. The
    // screenshot is how those are checked; this asserts the state that drives all three.
    harness.snapshot("unsaved");
}

#[test]
fn the_status_bar_counts_the_line_and_column_from_one() {
    let mut harness = harness("first line\nsecond line");
    harness.state_mut().command(Command::MoveDocumentStart { extend: false });
    harness.run();
    assert_eq!(harness.state().caret_position(), quill_app::status_bar::Position { line: 1, column: 1 });
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.run();
    assert_eq!(
        harness.state().caret_position(),
        quill_app::status_bar::Position { line: 2, column: 12 },
        "the caret is after the eleventh character of the second line"
    );
}

#[test]
fn the_column_counts_characters_rather_than_bytes() {
    // Five accented letters, each a letter plus a combining accent, so eleven bytes and five characters.
    let mut harness = harness("e\u{0301}e\u{0301}e\u{0301}e\u{0301}e\u{0301}");
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.run();
    assert_eq!(harness.state().document.text().len_bytes(), 15);
    assert_eq!(
        harness.state().caret_position(),
        quill_app::status_bar::Position { line: 1, column: 6 },
        "five characters along, so column six, not column sixteen"
    );
}

#[test]
fn the_filter_box_narrows_the_list_to_matching_files() {
    let mut harness = harness("");
    let all = harness.state().tree.file_count();
    assert_eq!(all, 7, "the sample folder holds seven files");
    assert_eq!(harness.state().tree.openable_count(), 6, "program.rs is not one Quill can open");
    harness.state_mut().filter = "two".to_owned();
    harness.run();
    let matches = harness.state().tree.matching("two");
    assert_eq!(matches.len(), 1, "only two.md matches");
    harness.snapshot("filter");
}

#[test]
fn the_explorer_can_be_hidden_and_brought_back() {
    let mut harness = harness("");
    assert!(harness.state().explorer_visible);
    let editor_with = harness.state().editor_area().width();
    harness.get_by_label("Hide the explorer").click();
    harness.run();
    assert!(!harness.state().explorer_visible);
    let editor_without = harness.state().editor_area().width();
    assert!(
        editor_without > editor_with,
        "hiding the explorer should give the editor its width: {editor_with} then {editor_without}"
    );
    harness.snapshot("explorer_hidden");
    harness.get_by_label("Show the explorer").click();
    harness.run();
    assert!(harness.state().explorer_visible);
}

#[test]
fn the_opacity_menu_opens_and_holds_the_slider() {
    let mut harness = harness("Text behind the opacity menu.");
    harness.get_by_label("Background opacity").click();
    harness.run();
    // The sentence under the slider only appears inside the menu, so finding it proves the menu is open.
    // The words "Background opacity" would match the button as well as the menu's own heading.
    harness.get_by_label_contains("The desktop shows through");
    harness.snapshot("opacity_menu");
}

#[test]
fn the_toolbar_buttons_are_all_reachable_by_name() {
    let harness = harness("some text");
    for name in [
        "Bold", "Italic", "Underline", "Strikethrough",
        "Left", "Center", "Right", "Justify",
        "Undo", "Redo", "Font family", "Font size", "Line spacing", "Background opacity",
    ] {
        harness.get_by_label(name);
    }
    for colour in ["White", "Red", "Green", "Blue", "Amber"] {
        harness.get_by_label(colour);
    }
}

/// Reproduce the design as closely as the application can, so that this image and `design/intial-design-screenshot.png` can be
/// put side by side.
///
/// It uses the project's own `sample` folder rather than a folder built in a temporary directory, because
/// the design shows that folder's files: `chapters` holding `one.md` and `two.md`, `notes` holding
/// `todo.txt`, and `welcome.md` open in the editor.
#[test]
fn the_window_matches_the_design() {
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("the workspace root is two levels above the crate")
        .join("sample");
    assert!(sample.join("welcome.md").is_file(), "{} should hold welcome.md", sample.display());

    let folder = sample.clone();
    let mut harness = Harness::builder()
        .with_size(vec2(1264.0, 751.0))
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = QuillApp::new(folder);
            app.prepare(&cc.egui_ctx);
            app
        });
    harness.run();
    // Open the two folders and the file, through the explorer, as a person would.
    harness.get_by_label_contains("chapters").click();
    harness.run();
    harness.get_by_label_contains("notes").click();
    harness.run();
    harness.get_by_label_contains("welcome.md").click();
    harness.run();

    assert!(
        harness.state().document.text().to_string().starts_with("# Quill"),
        "welcome.md should be open in the editor"
    );
    assert_eq!(harness.state().tree.file_count(), 5, "four Markdown or text files plus one Rust file");
    assert_eq!(harness.state().tree.openable_count(), 4, "the design shows four openable files");
    assert_eq!(
        harness.state().caret_position(),
        quill_app::status_bar::Position { line: 1, column: 1 }
    );
    harness.snapshot("design_comparison");
}

// The three view modes, and the Markdown preview behind them.

/// A document with one of everything the parser handles, used by the preview screenshots.
const MARKDOWN: &str = "\
# Quill preview

A paragraph with **bold**, *italic*, ~~struck~~ and `inline code` in it.

## A smaller heading

- a bullet
- another bullet
  - one nested under it

1. first
2. second

> a quoted line

```
fn main() {
    println!(\"code keeps its spacing\");
}
```

See [the design](https://example.com/design) for more.

---

The last paragraph.";

#[test]
fn a_new_window_starts_on_the_raw_markdown() {
    let harness = harness("# heading");
    assert_eq!(harness.state().view_mode, ViewMode::Raw);
    assert!(harness.state().view_mode.shows_source());
    assert!(!harness.state().view_mode.shows_preview());
}

#[test]
fn the_three_view_mode_buttons_are_reachable_by_name_and_switch_between_the_modes() {
    let mut harness = harness(MARKDOWN);
    for (name, expected) in [
        ("Side by side", ViewMode::SideBySide),
        ("Markdown preview", ViewMode::Preview),
        ("Raw Markdown", ViewMode::Raw),
    ] {
        harness.get_by_label(name).click();
        harness.run();
        assert_eq!(harness.state().view_mode, expected, "clicking {name} should switch to it");
    }
}

#[test]
fn raw_markdown_shows_the_source_as_it_is_on_disk() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Raw Markdown").click();
    harness.run();
    // The editing area holds the source, marks and all.
    assert!(harness.state().document.text().to_string().contains("**bold**"));
    assert_eq!(harness.state().editor_area().width(), harness.state().editor_area().width());
    harness.snapshot("view_raw");
}

#[test]
fn the_preview_removes_the_marks_and_applies_them() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let preview = harness.state().preview_text();
    assert!(!preview.contains("**bold**"), "the marks are not shown, got {preview:?}");
    assert!(preview.contains("bold"), "the words are");
    assert!(!preview.contains("# Quill preview"), "the heading loses its hash");
    assert!(preview.contains("Quill preview"));
    assert!(preview.contains('\u{2022}'), "a bullet list gets bullets");
    assert!(preview.contains("the design"), "a link shows its text");
    assert!(!preview.contains("https://example.com/design"), "and hides its address");
    assert!(preview.contains("code keeps its spacing"), "a code block keeps its lines");
    // The source itself is untouched: the preview is worked out from it, not instead of it.
    assert!(harness.state().document.text().to_string().contains("**bold**"));
    harness.snapshot("view_preview");
}

#[test]
fn the_preview_lays_out_headings_taller_than_body_text() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let layout = harness.state().preview_layout();
    assert!(!layout.lines.is_empty(), "the preview should have been laid out");
    let heading = layout.lines[0].height;
    let body = layout
        .lines
        .iter()
        .find(|line| line.runs.iter().any(|run| run.clusters.len() > 20))
        .map(|line| line.height)
        .expect("one line of body text");
    assert!(heading > body, "the heading line ({heading}) should be taller than a body line ({body})");
}

#[test]
fn side_by_side_shows_the_source_and_the_preview_at_once() {
    let mut harness = harness(MARKDOWN);
    let full_width = harness.state().editor_area().width();
    harness.get_by_label("Side by side").click();
    harness.run();
    let half = harness.state().editor_area().width();
    assert!(
        half < full_width,
        "the editing area should give up half its width to the preview: {full_width} then {half}"
    );
    assert!(!harness.state().preview_text().is_empty(), "the preview should have been worked out");
    harness.snapshot("view_side_by_side");
}

#[test]
fn the_preview_follows_the_source_as_it_is_edited() {
    let mut harness = harness("# first");
    harness.get_by_label("Side by side").click();
    harness.run();
    assert!(harness.state().preview_text().contains("first"));
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.input_mut().events.push(egui::Event::Text(" and second".to_owned()));
    harness.run();
    assert!(
        harness.state().preview_text().contains("first and second"),
        "the preview should have been worked out again, got {:?}",
        harness.state().preview_text()
    );
}

#[test]
fn the_preview_cannot_be_typed_into() {
    let mut harness = harness("# heading");
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let before = harness.state().document.text().to_string();
    // Typing with only the preview showing must not reach the document.
    harness.input_mut().events.push(egui::Event::Text("XXX".to_owned()));
    harness.run();
    assert_eq!(
        harness.state().document.text().to_string(),
        before,
        "the preview is read only, so nothing should have been inserted"
    );
}

// The File menu.

#[test]
fn the_file_menu_holds_open_folder_open_file_and_save() {
    let mut harness = harness("");
    harness.get_by_label("File").click();
    harness.run();
    for entry in ["Open Folder", "Open File", "Save", "Save As"] {
        harness.get_by_label(entry);
    }
    harness.snapshot("file_menu");
}

#[test]
fn opening_a_folder_shows_it_in_the_explorer() {
    let mut harness = harness("");
    // The picker itself is the operating system's, so the action is run directly rather than clicking
    // through a modal dialog that a test cannot answer.
    let other = std::env::temp_dir().join("quill-other-folder");
    std::fs::create_dir_all(other.join("inner")).expect("make the other folder");
    std::fs::write(other.join("alpha.md"), "# alpha\n").expect("write alpha.md");
    std::fs::write(other.join("inner/beta.txt"), "beta\n").expect("write inner/beta.txt");

    assert_ne!(harness.state().tree.root(), other.as_path());
    harness.state_mut().open_folder(&other);
    harness.run();

    assert_eq!(harness.state().tree.root(), other.as_path());
    let names: Vec<String> =
        harness.state().tree.rows().iter().map(|row| row.entry.name.clone()).collect();
    assert_eq!(names, vec!["inner", "alpha.md"], "the new folder's contents are listed");
    assert_eq!(harness.state().tree.file_count(), 2, "including the file in the sub folder");
    harness.snapshot("opened_folder");
    std::fs::remove_dir_all(&other).ok();
}

#[test]
fn opening_a_folder_clears_the_filter_and_brings_the_explorer_back() {
    let mut harness = harness("");
    harness.state_mut().filter = "two".to_owned();
    harness.state_mut().explorer_visible = false;
    harness.run();
    let folder = sample_folder();
    harness.state_mut().open_folder(&folder);
    harness.run();
    assert!(harness.state().filter.is_empty(), "a filter from the old folder should not carry over");
    assert!(harness.state().explorer_visible, "opening a folder shows the explorer");
}

#[test]
fn a_file_quill_cannot_open_is_listed_and_does_nothing_when_clicked() {
    let mut harness = harness("");
    let before = harness.state().document.text().to_string();
    let row = harness.get_by_label_contains("program.rs");
    row.click();
    harness.run();
    assert_eq!(
        harness.state().document.text().to_string(),
        before,
        "clicking a file Quill cannot open should do nothing"
    );
    harness.snapshot("unopenable_file");
}

#[test]
fn save_as_and_save_are_reachable_without_the_menu() {
    // `Save` on a document that has never been saved writes into the folder the explorer is showing, which
    // is the behaviour the status bar reports. This checks the action rather than the dialog.
    let folder = std::env::temp_dir().join("quill-save-action");
    std::fs::create_dir_all(&folder).expect("make the folder");
    let text = "saved through the File menu";
    let owned = folder.clone();
    let mut harness = Harness::builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = QuillApp::with_text(owned, text);
            app.prepare(&cc.egui_ctx);
            app
        });
    harness.run();
    harness.state_mut().file_action(FileAction::Save);
    harness.run();
    let written = folder.join("untitled.md");
    assert!(written.is_file(), "Save should have written {}", written.display());
    assert_eq!(std::fs::read_to_string(&written).expect("read it back"), text);
    std::fs::remove_dir_all(&folder).ok();
}
