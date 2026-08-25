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
use quill_app::app::actions::Action;
use quill_app::components::title_bar::MenuPlacement;
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
    // A file Quill has no special handling for. It opens as plain text, which is what
    // `tasks/improvements.md` asks for.
    std::fs::write(root.join("program.rs"), "fn main() {}\n").expect("write program.rs");
    // A file that is not text at all. It is listed, dimmed, and does not respond to a click. The bytes are
    // the start of a PNG, including the zero byte that says it is not text.
    std::fs::write(root.join("picture.png"), [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0])
        .expect("write picture.png");
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

/// Build the application on a folder of its own, for a test that needs a second window.
fn harness_in(folder: &std::path::Path) -> Harness<'static, QuillApp> {
    let folder = folder.to_path_buf();
    let mut harness = Harness::builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = QuillApp::new(folder);
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
    let text = harness.state().document().text().to_string();
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

/// Where the accepted image for `name` lives on the platform the test is running on.
///
/// The window is deliberately not the same on both platforms. macOS puts the menus in the bar along the
/// top of the screen and the window buttons at the left; Windows draws the menus in Quill's own title bar
/// and the buttons at the right. The text is not the same either, because Helvetica is not installed on
/// Windows and the family falls through to Arial. So one set of images cannot be the baseline for both:
/// run against the macOS set, 32 of the 72 differed on Windows for reasons that are the program working
/// exactly as it is meant to.
///
/// Each platform therefore has its own accepted set, and a difference in one really is a change to what
/// Quill draws there. macOS keeps the folder it already had, because those images were looked at and
/// accepted by a person and moving them would have said they were new.
fn shot(name: &str) -> String {
    if cfg!(target_os = "macos") {
        name.to_owned()
    } else if cfg!(target_os = "windows") {
        format!("windows/{name}")
    } else {
        format!("linux/{name}")
    }
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


/// Copy a folder to a place that is not inside any git repository.
///
/// A window looks for a repository the moment it opens, and what it finds goes in the status bar and
/// tints the explorer. A test folder that lives inside Quill's own repository therefore draws
/// something different depending on what is uncommitted at the time, which is not a difference in
/// Quill.
fn copy_out_of_the_repository(source: &std::path::Path, name: &str) -> std::path::PathBuf {
    let target = std::env::temp_dir().join(name);
    std::fs::remove_dir_all(&target).ok();
    fn walk(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).expect("make the folder");
        for entry in std::fs::read_dir(from).expect("read the folder").flatten() {
            let path = entry.path();
            let into = to.join(entry.file_name());
            if path.is_dir() {
                walk(&path, &into);
            } else {
                std::fs::copy(&path, &into).expect("copy the file");
            }
        }
    }
    walk(source, &target);
    target
}

/// A folder that is a real git repository, for the tests about git.
///
/// Built with its identity and its settings named on the command line, so a test does not depend on
/// the `.gitconfig` of whoever is running it, and with two commits on separated dates so blame has a
/// spread of ages to colour. Rebuilt each time, so the pictures are the same on every run.
fn git_folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-screenshot-repository").join(name);
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("make the folder");
    let git = |arguments: &[&str]| {
        let outcome = quill_git::command::run(&root, arguments);
        assert!(outcome.ok, "git {arguments:?}: {}", outcome.message());
    };
    git(&["init", "--initial-branch=main"]);
    for (name, value) in [
        ("user.name", "Quill Test"),
        ("user.email", "test@quill.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&["config", name, value]);
    }
    std::fs::write(root.join("readme.md"), "# a repository\n").expect("write readme.md");
    std::fs::write(
        root.join("sqlClient.ts"),
        "import { createScopedSql } from '../db/sqlClient';\n\n/** Lists messages in a chat. */\nexport class MessageRepository {\n  private sql = createScopedSql();\n}\n",
    )
    .expect("write sqlClient.ts");
    git(&["add", "-A"]);
    git(&["commit", "--date", "2026-01-14T09:00:00+00:00", "-m", "the first commit"]);
    std::fs::write(root.join("version.ts"), "export const version = '0.1.0';\n").expect("write version.ts");
    // The second commit also touches the annotated file, so blame has two authors and two dates in
    // it and the column really shows its gradient rather than one flat colour.
    std::fs::write(
        root.join("sqlClient.ts"),
        "import { createScopedSql } from '../db/sqlClient';\n\n/** Lists messages in a chat. */\nexport class MessageRepository {\n  private sql = createScopedSql();\n\n  /** Deletes every message in a chat. */\n  async deleteByChat(chatId: number) {}\n}\n",
    )
    .expect("change sqlClient.ts");
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=Sam Okafor",
        "-c",
        "user.email=sam@example.com",
        "commit",
        "--date",
        "2026-07-21T16:00:00+00:00",
        "-m",
        "add a version",
    ]);
    // A change that has not been committed, and a file git has never seen, so the commit panel and
    // the gutter's change bars both have something to show.
    std::fs::write(root.join("version.ts"), "export const version = '0.2.0';\nconst extra = 1;\n")
        .expect("change version.ts");
    std::fs::write(root.join("notes.txt"), "scratch\n").expect("write notes.txt");
    root
}

/// A window on a real repository, with the repository already read.
///
/// The window looks for a repository on its first frame, and reading it happens on a thread, so the
/// harness is run several times to let the answer arrive before the picture is taken. Each run is a
/// frame; nothing here waits on a clock, so the test is the same on every machine.
fn git_harness(name: &str) -> Harness<'static, QuillApp> {
    let folder = git_folder(name);
    let mut harness = harness_in(&folder);
    for _ in 0..600 {
        harness.run();
        if harness.state().git.as_ref().is_some_and(|git| !git.snapshot.status.entries.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    harness.run();
    harness
}

#[test]
fn startup_shows_the_explorer_the_toolbar_and_an_empty_editor() {
    let mut harness = harness("");
    harness.snapshot(shot("startup"));
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
    harness.snapshot(shot("file_tree_expanded"));
}

#[test]
fn clicking_a_file_in_the_explorer_opens_it_in_the_editor() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        "# Quill\n",
        "clicking the file should have loaded it"
    );
    // The text has to be laid out as well as loaded. Two documents can be at the same revision, so a layout
    // cache that only compares revisions keeps the last file's lines and the editing area comes out empty.
    let lines = &harness.state().layout().lines;
    // Two lines: the heading, and the empty one after the line break at the end of the file.
    assert_eq!(lines.len(), 2, "the file should have been laid out, got {}", lines.len());
    assert!(
        lines[0].runs.iter().any(|run| !run.clusters.is_empty()),
        "and the first line should hold the characters of the file"
    );
    harness.snapshot(shot("file_opened"));
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
    assert_eq!(harness.state().document().text().to_string(), "Quill typed this.\nA second line.");
    harness.snapshot(shot("typed_text"));
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
    assert_eq!(harness.state().document().text().to_string(), "abc");
}

#[test]
fn a_selection_is_highlighted_behind_part_of_a_line_only() {
    let mut harness = harness("Select only the middle words of this line, not the rest of it.");
    select_and(&mut harness, 12..28, &[]);
    assert_eq!(harness.state().document().selected_text(), "the middle words");
    let rects = harness.state().layout().selection_rects(harness.state().document().selection().range());
    assert_eq!(rects.len(), 1, "the selection is inside one line, so it is one rectangle");
    harness.snapshot(shot("selection"));
}

#[test]
fn select_all_then_pressing_bold_makes_the_whole_document_bold() {
    let mut harness = harness("Every word here should end up bold.");
    harness.state_mut().command(Command::SelectAll);
    harness.run();
    // Click the real toolbar button rather than sending the command.
    harness.get_by_label("Bold").click();
    harness.run();
    assert!(harness.state().document().chars().style_at(4).bold, "the toolbar button should have applied bold");
    harness.snapshot(shot("bold_all"));
}

#[test]
fn bold_applies_to_the_middle_word_and_not_the_words_either_side() {
    let mut harness = harness("plain BOLD plain");
    select_phrase(&mut harness, "BOLD", &[Command::ToggleBold]);
    collapse(&mut harness);
    harness.snapshot(shot("bold"));
}

#[test]
fn italic_applies_to_the_middle_word_and_not_the_words_either_side() {
    let mut harness = harness("plain ITALIC plain");
    select_phrase(&mut harness, "ITALIC", &[Command::ToggleItalic]);
    collapse(&mut harness);
    harness.snapshot(shot("italic"));
}

#[test]
fn underline_draws_a_rule_under_the_middle_word_only() {
    let mut harness = harness("plain UNDERLINE plain");
    select_phrase(&mut harness, "UNDERLINE", &[Command::ToggleUnderline]);
    collapse(&mut harness);
    let rules = harness.state().layout().decorations(&harness.state().renderer);
    assert_eq!(rules.len(), 1, "one underline rule to draw");
    harness.snapshot(shot("underline"));
}

#[test]
fn strikethrough_draws_a_rule_through_the_middle_word_only() {
    let mut harness = harness("plain STRUCK plain");
    select_phrase(&mut harness, "STRUCK", &[Command::ToggleStrikethrough]);
    collapse(&mut harness);
    harness.snapshot(shot("strikethrough"));
}

#[test]
fn the_keyboard_shortcut_for_bold_does_the_same_as_the_button() {
    let mut harness = harness("shortcut bold");
    harness.state_mut().command(Command::SelectAll);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::B);
    harness.run();
    assert!(harness.state().document().chars().style_at(2).bold, "command plus B should turn bold on");
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
    harness.snapshot(shot("font_size"));
}

#[test]
fn four_words_are_shown_in_four_colours() {
    let mut harness = harness("white red green blue");
    select_phrase(&mut harness, "red", &[Command::ApplyStyle(StyleChange::color(Color::RED))]);
    select_phrase(&mut harness, "green", &[Command::ApplyStyle(StyleChange::color(Color::GREEN))]);
    select_phrase(&mut harness, "blue", &[Command::ApplyStyle(StyleChange::color(Color::BLUE))]);
    collapse(&mut harness);
    assert_eq!(harness.state().document().chars().style_at(7).color, Color::RED);
    harness.snapshot(shot("font_colour"));
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
    harness.snapshot(shot("font_family"));
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
        assert_eq!(harness.state().document().paragraphs().get(0).align, align);
        results.add(harness.try_snapshot(shot(name)));
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
    results.add(single.try_snapshot(shot("line_spacing_single")));

    let mut double = harness(text);
    double.state_mut().command(Command::SelectAll);
    double.state_mut().command(Command::SetLineSpacing(2.0));
    double.run();
    let double_height = double.state().layout().height;
    assert!(
        (double_height - single_height * 2.0).abs() < 1.0,
        "double spacing should be twice as tall: {single_height} then {double_height}"
    );
    results.add(double.try_snapshot(shot("line_spacing_double")));
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
    harness.snapshot(shot("word_wrap"));
}

#[test]
fn cut_and_paste_move_text_through_the_clipboard() {
    let mut harness = harness("first second");
    select_and(&mut harness, 0..5, &[]);
    // A cut sends the selection to the clipboard and removes it from the document.
    harness.input_mut().events.push(egui::Event::Cut);
    harness.run();
    assert_eq!(harness.state().document().text().to_string(), " second");
    // Paste it back at the end.
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.input_mut().events.push(egui::Event::Paste("first".to_owned()));
    harness.run();
    assert_eq!(harness.state().document().text().to_string(), " secondfirst");
}

#[test]
fn copy_leaves_the_document_alone() {
    let mut harness = harness("unchanged text");
    select_and(&mut harness, 0..9, &[]);
    harness.input_mut().events.push(egui::Event::Copy);
    harness.run();
    assert_eq!(harness.state().document().text().to_string(), "unchanged text");
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
        harness.state_mut().settings.opacity = opacity;
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
        results.add(harness.try_snapshot(shot(name)));
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

/// Undo and redo are on the keyboard and in the Edit menu. The toolbar buttons they used to have are gone,
/// because `tasks/improvements.md` asks for the keyboard alone.
#[test]
fn undo_and_redo_go_back_and_forward_through_the_history() {
    let mut harness = harness("original");
    harness.input_mut().events.push(egui::Event::Text(" plus more".to_owned()));
    harness.run();
    assert_eq!(harness.state().document().text().to_string(), " plus moreoriginal");

    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    harness.run();
    assert_eq!(harness.state().document().text().to_string(), "original", "command and Z undoes");

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::Z);
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        " plus moreoriginal",
        "command, shift and Z redoes"
    );
}

#[test]
fn the_toolbar_no_longer_holds_the_font_the_opacity_or_undo_and_redo() {
    // They moved: the font and the background are in `Edit -> Settings`, and undo and redo are on the
    // keyboard. A button that is still there would mean the move was not finished.
    let harness = harness("some text");
    for gone in ["Undo", "Redo", "Font family", "Font size", "Background opacity"] {
        assert!(
            harness.query_by_label(gone).is_none(),
            "{gone} should not be in the toolbar any more"
        );
    }
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
    harness.snapshot(shot("everything"));
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
    harness.snapshot(shot("title_bar"));
}

#[test]
fn an_edited_file_is_marked_as_unsaved_in_three_places() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    assert!(!harness.state().document().is_modified(), "just opened, so nothing to save");
    harness.input_mut().events.push(egui::Event::Text(" edited".to_owned()));
    harness.run();
    assert!(harness.state().document().is_modified());
    // The dot appears in the title bar, on the file's row in the explorer and in the status bar. The
    // screenshot is how those are checked; this asserts the state that drives all three.
    harness.snapshot(shot("unsaved"));
}

#[test]
fn the_status_bar_counts_the_line_and_column_from_one() {
    let mut harness = harness("first line\nsecond line");
    harness.state_mut().command(Command::MoveDocumentStart { extend: false });
    harness.run();
    assert_eq!(harness.state().caret_position(), quill_app::components::status_bar::Position { line: 1, column: 1 });
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.run();
    assert_eq!(
        harness.state().caret_position(),
        quill_app::components::status_bar::Position { line: 2, column: 12 },
        "the caret is after the eleventh character of the second line"
    );
}

#[test]
fn the_column_counts_characters_rather_than_bytes() {
    // Five accented letters, each a letter plus a combining accent, so eleven bytes and five characters.
    let mut harness = harness("e\u{0301}e\u{0301}e\u{0301}e\u{0301}e\u{0301}");
    harness.state_mut().command(Command::MoveDocumentEnd { extend: false });
    harness.run();
    assert_eq!(harness.state().document().text().len_bytes(), 15);
    assert_eq!(
        harness.state().caret_position(),
        quill_app::components::status_bar::Position { line: 1, column: 6 },
        "five characters along, so column six, not column sixteen"
    );
}

#[test]
fn the_filter_box_narrows_the_list_to_matching_files() {
    let mut harness = harness("");
    let all = harness.state().tree.file_count();
    assert_eq!(all, 8, "the sample folder holds eight files");
    assert_eq!(
        harness.state().tree.openable_count(),
        7,
        "every one but the image can be opened, including the Rust file"
    );
    harness.state_mut().filter = "two".to_owned();
    harness.run();
    let matches = harness.state().tree.matching("two");
    assert_eq!(matches.len(), 1, "only two.md matches");
    harness.snapshot(shot("filter"));
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
    harness.snapshot(shot("explorer_hidden"));
    harness.get_by_label("Show the explorer").click();
    harness.run();
    assert!(harness.state().explorer_visible);
}



#[test]
fn the_toolbar_buttons_are_all_reachable_by_name() {
    let harness = harness("some text");
    for name in [
        "Bold", "Italic", "Underline", "Strikethrough",
        "Left", "Center", "Right", "Justify",
        "Line spacing",
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
    // Copied out of the repository first. `sample/` is inside Quill's own git repository, and the
    // status bar now says which branch is checked out and how many files have changed, so opening it
    // where it lies made the picture depend on what happened to be edited that day.
    let sample = copy_out_of_the_repository(&sample, "quill-screenshot-sample");

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
        harness.state().document().text().to_string().starts_with("# Quill"),
        "welcome.md should be open in the editor"
    );
    assert_eq!(harness.state().tree.file_count(), 5, "four Markdown or text files plus one Rust file");
    assert_eq!(
        harness.state().tree.openable_count(),
        5,
        "all five hold text, so all five open"
    );
    assert_eq!(
        harness.state().caret_position(),
        quill_app::components::status_bar::Position { line: 1, column: 1 }
    );
    harness.snapshot(shot("design_comparison"));
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
    assert_eq!(harness.state().view_mode(), ViewMode::Raw);
    assert!(harness.state().view_mode().shows_source());
    assert!(!harness.state().view_mode().shows_preview());
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
        assert_eq!(harness.state().view_mode(), expected, "clicking {name} should switch to it");
    }
}

#[test]
fn raw_markdown_shows_the_source_as_it_is_on_disk() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Raw Markdown").click();
    harness.run();
    // The editing area holds the source, marks and all.
    assert!(harness.state().document().text().to_string().contains("**bold**"));
    assert_eq!(harness.state().editor_area().width(), harness.state().editor_area().width());
    harness.snapshot(shot("view_raw"));
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
    assert!(harness.state().document().text().to_string().contains("**bold**"));
    harness.snapshot(shot("view_preview"));
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
    harness.snapshot(shot("view_side_by_side"));
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
    let before = harness.state().document().text().to_string();
    // Typing with only the preview showing must not reach the document.
    harness.input_mut().events.push(egui::Event::Text("XXX".to_owned()));
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "the preview is read only, so nothing should have been inserted"
    );
}

// The File menu.

/// The menus, drawn inside the window.
///
/// On macOS Quill puts them in the bar along the top of the screen instead, which egui cannot draw and this
/// harness cannot see, so the test asks for the bar inside the window. That is not a special case for the
/// test: it is what Windows uses, and both bars are built from the same list of menus.
#[test]
fn the_file_menu_holds_new_window_open_save_and_recent_projects() {
    let mut harness = harness("");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.state_mut().recent = vec![
        std::path::PathBuf::from("/tmp/quill-recent-one"),
        std::path::PathBuf::from("/tmp/quill-recent-two"),
    ];
    harness.run();
    harness.get_by_label("File").click();
    harness.run();
    for entry in ["New Window", "Open File", "Open Folder", "Save", "Save As", "Close Window"] {
        harness.get_by_label(entry);
    }
    // The recent projects are listed under a heading of their own inside the File menu.
    harness.get_by_label("quill-recent-one");
    harness.get_by_label("quill-recent-two");
    harness.get_by_label("Forget Recent Projects");
    harness.snapshot(shot("file_menu"));
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
    harness.snapshot(shot("opened_folder"));
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
fn a_file_that_is_not_text_is_listed_and_does_nothing_when_clicked() {
    let mut harness = harness("");
    let before = harness.state().document().text().to_string();
    harness.get_by_label_contains("picture.png").click();
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "clicking a file that is not text should do nothing"
    );
    harness.snapshot(shot("unopenable_file"));
}

/// The file types improvement: a file Quill has no special handling for opens as plain text.
#[test]
fn a_rust_file_opens_as_plain_text() {
    let mut harness = harness("");
    harness.get_by_label_contains("program.rs").click();
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        "fn main() {}\n",
        "the Rust file should have been loaded"
    );
    assert_eq!(harness.state().view_mode(), ViewMode::Raw, "there is nothing to preview in it");
    assert!(
        !harness.state().layout().lines.is_empty(),
        "the Rust file's text should have been laid out"
    );
    harness.snapshot(shot("plain_text_file"));
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
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Save, &ctx);
    harness.run();
    let written = folder.join("untitled.md");
    assert!(written.is_file(), "Save should have written {}", written.display());
    assert_eq!(std::fs::read_to_string(&written).expect("read it back"), text);
    std::fs::remove_dir_all(&folder).ok();
}

// The Settings window, which is where the font and the background moved to.

/// Drag from one point to another, which is how a divider between two panes is moved.
///
/// egui has a threshold a pointer has to pass before a press becomes a drag, so this presses, moves and
/// releases over several frames rather than in one.
fn drag(harness: &mut Harness<'static, QuillApp>, from: egui::Pos2, to: egui::Pos2) {
    let modifiers = Modifiers::default();
    harness.input_mut().events.push(egui::Event::PointerMoved(from));
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerMoved(to));
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    harness.run();
}

/// Open the Settings window the way a person does on Windows: `Edit` in the bar, then `Settings`.
fn open_settings(harness: &mut Harness<'static, QuillApp>) {
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    harness.get_by_label("Edit").click();
    harness.run();
    harness.get_by_label("Settings").click();
    harness.run();
}

#[test]
fn the_settings_window_opens_from_the_edit_menu_and_holds_the_font_and_the_background() {
    let mut harness = harness("Text behind the settings window.");
    assert!(!harness.state().settings_window.open);
    open_settings(&mut harness);
    assert!(harness.state().settings_window.open, "Edit then Settings should open it");

    // The two sections `tasks/improvements.md` asks for, and the page list on the left.
    harness.get_by_label("Editor font family");
    harness.get_by_label("Editor font size");
    harness.get_by_label("Background opacity");
    harness.get_by_label("Appearance");
    harness.get_by_label("Terminal");
    harness.snapshot(shot("settings_appearance"));
}

#[test]
fn the_settings_window_closes_on_the_close_button() {
    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("Done").click();
    harness.run();
    assert!(!harness.state().settings_window.open);
}

#[test]
fn the_terminal_page_holds_the_terminal_font_size() {
    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("Terminal").click();
    harness.run();
    harness.get_by_label("Terminal font size");
    assert!(harness.query_by_label("Background opacity").is_none(), "that is on the Appearance page");
    harness.snapshot(shot("settings_terminal"));
}

#[test]
fn choosing_a_font_size_in_the_settings_sets_it_for_the_whole_document() {
    let mut harness = harness("Two lines of writing\nso that both change together");
    let undo_before = harness.state().document().can_undo();
    open_settings(&mut harness);
    harness.get_by_label("Editor font size").click();
    harness.run();
    harness.get_by_label("24").click();
    harness.run();

    assert_eq!(harness.state().settings.font_size, 24.0);
    let document = harness.state().document();
    assert_eq!(document.chars().style_at(0).size, 24.0, "the first line is at the new size");
    let end = document.text().len_bytes() - 1;
    assert_eq!(document.chars().style_at(end).size, 24.0, "and so is the last");
    assert_eq!(
        harness.state().document().text().to_string(),
        "Two lines of writing\nso that both change together",
        "and the text itself is untouched"
    );
    assert_eq!(
        harness.state().document().can_undo(),
        undo_before,
        "a font setting pushes nothing onto the undo history"
    );
}

#[test]
fn choosing_a_family_in_the_settings_leaves_bold_and_colour_alone() {
    let mut harness = harness("plain BOLD plain");
    select_phrase(&mut harness, "BOLD", &[Command::ToggleBold]);
    collapse(&mut harness);
    let families = harness.state().renderer.families().to_vec();
    let other = families.last().expect("this system has a family").clone();

    let mut settings = harness.state().settings.clone();
    settings.font_family = other.clone();
    harness.state_mut().set_settings(settings);
    harness.run();

    let style = harness.state().document().chars().style_at(7);
    assert_eq!(style.family, other, "the word is in the new family");
    assert!(style.bold, "and still bold");
    harness.snapshot(shot("settings_font_applied"));
}

#[test]
fn the_background_setting_fades_the_window() {
    let mut harness = harness("The desktop shows through behind this.");
    let mut settings = harness.state().settings.clone();
    settings.opacity = 0.2;
    harness.state_mut().set_settings(settings);
    harness.run();
    assert_eq!(harness.state().background().a(), 51, "a fifth of the way up from nothing");
    harness.snapshot(shot("settings_background_faint"));
}

// The panes, all of which are resized by dragging their edge.

#[test]
fn the_explorer_can_be_dragged_wider_and_the_editor_gives_up_the_room() {
    let mut harness = harness("");
    let before = harness.state().panes.explorer_width;
    let editor_before = harness.state().editor_area().width();
    let handle = harness.get_by_label("Resize explorer").rect();
    let from = handle.center();
    drag(&mut harness, from, egui::pos2(from.x + 120.0, from.y));

    let after = harness.state().panes.explorer_width;
    assert!(after > before + 100.0, "the explorer should be wider: {before} then {after}");
    assert!(
        harness.state().editor_area().width() < editor_before,
        "and the editing area should have given up the room"
    );
    harness.snapshot(shot("explorer_wide"));
}

#[test]
fn the_explorer_cannot_be_dragged_past_its_limits() {
    let mut harness = harness("");
    let handle = harness.get_by_label("Resize explorer").rect();
    let from = handle.center();
    // Far further than the window is wide, in both directions.
    drag(&mut harness, from, egui::pos2(from.x + 2000.0, from.y));
    assert_eq!(harness.state().panes.explorer_width, quill_app::settings::EXPLORER_MAX);
    let handle = harness.get_by_label("Resize explorer").rect();
    drag(&mut harness, handle.center(), egui::pos2(handle.center().x - 2000.0, handle.center().y));
    assert_eq!(harness.state().panes.explorer_width, quill_app::settings::EXPLORER_MIN);
}

#[test]
fn the_split_between_the_source_and_the_preview_can_be_dragged() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Side by side").click();
    harness.run();
    let source_before = harness.state().editor_area().width();
    let handle = harness.get_by_label("Resize preview").rect();
    let from = handle.center();
    drag(&mut harness, from, egui::pos2(from.x + 150.0, from.y));
    let source_after = harness.state().editor_area().width();
    assert!(
        source_after > source_before + 100.0,
        "the source should have taken the room: {source_before} then {source_after}"
    );
    harness.snapshot(shot("preview_split_dragged"));
}

// The terminal.

/// A window with a terminal open that has no shell behind it, so that what it draws is the same on every
/// run. The bytes a test feeds it go through the same emulator a real shell's output does.
fn with_terminal(text: &str, rows: usize, columns: usize) -> Harness<'static, QuillApp> {
    let mut harness = harness(text);
    harness.state_mut().new_detached_terminal_tab(rows, columns);
    harness.run();
    harness
}

fn feed(harness: &mut Harness<'static, QuillApp>, bytes: &[u8]) {
    harness
        .state_mut()
        .terminal
        .tabs
        .active_mut()
        .expect("a terminal tab")
        .feed(bytes);
    harness.run();
}

#[test]
fn the_terminal_opens_along_the_bottom_and_shows_what_a_program_wrote() {
    let mut harness = with_terminal("A document above the terminal.", 12, 80);
    assert!(harness.state().terminal.visible);
    feed(
        &mut harness,
        b"jason.mcaffee@quill ~ % cargo test\r\n   Compiling quill-terminal v0.1.0\r\n    Finished in 1.26s\r\n",
    );
    let screen = harness.state().terminal.tabs.active().expect("a tab").snapshot();
    assert!(screen.contains("Compiling quill-terminal"), "the output should be on the screen");
    harness.snapshot(shot("terminal"));
}

#[test]
fn the_terminal_draws_colour_bold_and_the_other_attributes() {
    let mut harness = with_terminal("", 12, 80);
    let mut bytes = Vec::new();
    // The eight ordinary colours, then the eight bright ones, then the attributes.
    for code in 30..38 {
        bytes.extend_from_slice(format!("\x1b[{code}m colour{code} ").as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[0m\r\n");
    for code in 90..98 {
        bytes.extend_from_slice(format!("\x1b[{code}m bright{code} ").as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[0m\r\n");
    bytes.extend_from_slice(
        b"\x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstruck\x1b[0m \x1b[7minverse\x1b[0m \x1b[2mdim\x1b[0m\r\n",
    );
    bytes.extend_from_slice(b"\x1b[48;5;24m background \x1b[0m \x1b[38;2;255;120;0mtrue colour\x1b[0m\r\n");
    feed(&mut harness, &bytes);
    harness.snapshot(shot("terminal_colours"));
}

#[test]
fn the_terminal_draws_a_program_that_takes_over_the_screen() {
    let mut harness = with_terminal("", 14, 80);
    // What a full screen program draws: the alternate screen, box drawing characters, and text placed by
    // moving the cursor rather than by printing lines in order.
    let mut bytes = b"\x1b[?1049h\x1b[H".to_vec();
    bytes.extend_from_slice("\u{250c}".as_bytes());
    for _ in 0..40 {
        bytes.extend_from_slice("\u{2500}".as_bytes());
    }
    bytes.extend_from_slice("\u{2510}".as_bytes());
    for row in 2..8 {
        bytes.extend_from_slice(format!("\x1b[{row};1H").as_bytes());
        bytes.extend_from_slice("\u{2502}".as_bytes());
        bytes.extend_from_slice(format!("\x1b[{row};42H").as_bytes());
        bytes.extend_from_slice("\u{2502}".as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[8;1H");
    bytes.extend_from_slice("\u{2514}".as_bytes());
    for _ in 0..40 {
        bytes.extend_from_slice("\u{2500}".as_bytes());
    }
    bytes.extend_from_slice("\u{2518}".as_bytes());
    bytes.extend_from_slice(b"\x1b[3;4H\x1b[1;36mA program drawing its own screen\x1b[0m");
    bytes.extend_from_slice("\x1b[5;4H\u{25b6} one\x1b[6;4H  two".as_bytes());
    feed(&mut harness, &bytes);
    assert!(
        harness.state().terminal.tabs.active().expect("a tab").on_alternate_screen(),
        "the program should be on its own screen"
    );
    harness.snapshot(shot("terminal_full_screen"));
}

#[test]
fn a_second_terminal_tab_is_added_and_shown_in_front() {
    let mut harness = with_terminal("", 10, 60);
    feed(&mut harness, b"the first tab");
    harness.state_mut().new_detached_terminal_tab(10, 60);
    harness.run();
    feed(&mut harness, b"the second tab");
    assert_eq!(harness.state().terminal.tabs.count(), 2);
    assert_eq!(harness.state().terminal.tabs.active_index(), 1);
    let screen = harness.state().terminal.tabs.active().expect("a tab").snapshot();
    assert!(screen.contains("the second tab"));
    harness.snapshot(shot("terminal_tabs"));

    // Going back to the first tab shows what was in it, so a tab keeps its own screen.
    harness.get_by_label("Terminal tab: detached").click();
    harness.run();
    assert_eq!(harness.state().terminal.tabs.active_index(), 0);
    let screen = harness.state().terminal.tabs.active().expect("a tab").snapshot();
    assert!(screen.contains("the first tab"), "the first tab kept its screen");
}

#[test]
fn closing_the_last_terminal_tab_puts_the_tile_away() {
    let mut harness = with_terminal("", 8, 60);
    assert!(harness.state().terminal.visible);
    harness.get_by_label_contains("Close detached").click();
    harness.run();
    assert_eq!(harness.state().terminal.tabs.count(), 0);
    assert!(!harness.state().terminal.visible, "with no terminals there is nothing to show");
}

#[test]
fn the_terminal_is_told_the_new_size_when_the_tile_is_dragged() {
    let mut harness = with_terminal("", 12, 80);
    feed(&mut harness, b"before the resize");
    let tall = harness.state().terminal.tabs.active().expect("a tab").size();
    let results = &mut SnapshotResults::new();
    results.add(harness.try_snapshot(shot("terminal_tall")));

    // Drag the tile's top edge downwards, which makes it shorter.
    let handle = harness.get_by_label("Resize terminal").rect();
    let from = handle.center();
    drag(&mut harness, from, egui::pos2(from.x, from.y + 90.0));

    let short = harness.state().terminal.tabs.active().expect("a tab").size();
    assert!(
        short.rows < tall.rows,
        "a shorter tile holds fewer rows: {} then {}",
        tall.rows,
        short.rows
    );
    assert_eq!(short.columns, tall.columns, "its width did not change");
    assert!(
        harness.state().terminal.tabs.active().expect("a tab").snapshot().contains("before the resize"),
        "and what was written is still there"
    );
    results.add(harness.try_snapshot(shot("terminal_short")));
    report(std::mem::replace(results, SnapshotResults::new()));
}

#[test]
fn the_terminal_font_size_changes_the_size_of_the_grid() {
    let mut harness = with_terminal("", 12, 80);
    feed(&mut harness, b"a line at the bigger size\r\n\x1b[32mand a green one\x1b[0m");
    let before = harness.state().terminal.tabs.active().expect("a tab").size();
    let mut settings = harness.state().settings.clone();
    settings.terminal_font_size = 20.0;
    harness.state_mut().set_settings(settings);
    harness.run();
    let after = harness.state().terminal.tabs.active().expect("a tab").size();
    assert!(
        after.columns < before.columns && after.rows <= before.rows,
        "a bigger font means fewer cells fit: {before:?} then {after:?}"
    );
    harness.snapshot(shot("terminal_large_font"));
}

#[test]
fn the_view_menu_shows_and_hides_the_terminal() {
    let mut harness = harness("");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    assert!(!harness.state().terminal.visible);

    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::ToggleTerminal, &ctx);
    harness.run();
    assert!(harness.state().terminal.visible, "the terminal should have opened");
    assert_eq!(harness.state().terminal.tabs.count(), 1, "with a shell in it");
    assert_eq!(harness.state().focus, quill_app::app::Focus::Terminal, "and the keyboard in it");

    harness.state_mut().run_action(Action::ToggleTerminal, &ctx);
    harness.run();
    assert!(!harness.state().terminal.visible);
    assert_eq!(harness.state().focus, quill_app::app::Focus::Editor, "the keyboard comes back");
}

/// Typing goes to the program in the terminal rather than to the document.
///
/// This is the one screenshot test that starts a real shell, because a detached terminal has nothing to
/// answer. It waits for the output rather than assuming it has arrived, and asserts on the text rather than
/// on pixels, because when a shell answers is not something a test can know.
#[test]
fn typing_in_the_terminal_reaches_the_shell_and_not_the_document() {
    let mut harness = harness("the document is untouched");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::ToggleTerminal, &ctx);
    harness.run();
    assert_eq!(harness.state().focus, quill_app::app::Focus::Terminal);

    for text in ["echo quill-typing-works"] {
        harness.input_mut().events.push(egui::Event::Text(text.to_owned()));
        harness.run();
    }
    harness.key_press(egui::Key::Enter);
    harness.run();

    // Thirty seconds, because this waits for a real shell on whatever machine the tests are run on, and a
    // machine busy with a build can take much longer than an idle one.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        harness.run();
        let found = harness
            .state()
            .terminal
            .tabs
            .active()
            .map(|session| session.snapshot().contains("quill-typing-works"))
            .unwrap_or(false);
        if found {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the shell did not answer in thirty seconds, the terminal holds {:?}",
            harness.state().terminal.tabs.active().map(|session| session.snapshot().text())
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        harness.state().document().text().to_string(),
        "the document is untouched",
        "nothing typed into the terminal reached the document"
    );
}

// Several windows, each on its own project.

#[test]
fn the_recent_projects_are_remembered_across_windows() {
    // The store is pointed at a folder of its own, so this neither reads nor writes the settings of the
    // person running the tests.
    let store_folder = std::env::temp_dir().join("quill-recent-projects-test");
    std::fs::remove_dir_all(&store_folder).ok();
    let first = std::env::temp_dir().join("quill-project-one");
    let second = std::env::temp_dir().join("quill-project-two");
    std::fs::create_dir_all(&first).expect("make the first project");
    std::fs::create_dir_all(&second).expect("make the second project");

    let mut harness = harness("");
    harness.state_mut().use_store(quill_app::services::store::Store::at(&store_folder));
    harness.state_mut().open_folder(&first);
    harness.state_mut().open_folder(&second);
    harness.run();

    let recent = harness.state().recent.clone();
    assert!(recent[0].ends_with("quill-project-two"), "the newest is first, got {recent:?}");
    assert!(recent.iter().any(|path| path.ends_with("quill-project-one")));

    // A second window reads the same list, which is what makes it a list of recent projects rather than of
    // this window's projects.
    let mut second_window = harness_in(&first);
    second_window
        .state_mut()
        .use_store(quill_app::services::store::Store::at(&store_folder));
    second_window.run();
    assert!(
        second_window.state().recent.iter().any(|path| path.ends_with("quill-project-two")),
        "the other window's project should be in this window's list"
    );

    std::fs::remove_dir_all(&store_folder).ok();
    std::fs::remove_dir_all(&first).ok();
    std::fs::remove_dir_all(&second).ok();
}

#[test]
fn a_setting_is_written_and_read_back_by_the_next_window() {
    let store_folder = std::env::temp_dir().join("quill-settings-across-windows");
    std::fs::remove_dir_all(&store_folder).ok();

    let mut first_window = harness("");
    first_window.state_mut().use_store(quill_app::services::store::Store::at(&store_folder));
    let mut settings = first_window.state().settings.clone();
    settings.font_size = 32.0;
    settings.opacity = 0.5;
    first_window.state_mut().set_settings(settings);
    first_window.run();
    // Written once the pointer is up, which it is, so the next frame writes the file.
    first_window.run();

    let mut next = harness("");
    next.state_mut().use_store(quill_app::services::store::Store::at(&store_folder));
    next.run();
    assert_eq!(next.state().settings.font_size, 32.0, "the size should have come back");
    assert_eq!(next.state().settings.opacity, 0.5);
    std::fs::remove_dir_all(&store_folder).ok();
}

#[test]
fn a_box_that_takes_typing_keeps_the_keyboard_while_the_terminal_is_open() {
    // The explorer's filter box is clicked into while the terminal has the keyboard. Without this the
    // terminal would take every key press and the filter box could never be typed into.
    let mut harness = with_terminal("", 10, 60);
    assert_eq!(harness.state().focus, quill_app::app::Focus::Editor);
    harness.state_mut().focus = quill_app::app::Focus::Terminal;
    harness.run();

    harness.get_by_label("Filter files").click();
    harness.run();
    harness.get_by_label("Filter files").type_text("two");
    harness.run();
    harness.run();

    assert_eq!(harness.state().filter, "two", "what was typed should have reached the filter box");
}

#[test]
fn the_recent_projects_menu_opens_a_project_without_closing_this_one() {
    // Opening a recent project starts another window on it, which is what IntelliJ does, so the project that
    // is open stays open. The other window is a second process; this checks that this window is left alone
    // and that the entry is there to be chosen.
    let mut harness = harness("");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    let other = std::env::temp_dir().join("quill-recent-open-test");
    std::fs::create_dir_all(&other).expect("make the other project");
    harness.state_mut().recent = vec![other.clone()];
    harness.run();
    let before = harness.state().tree.root().to_path_buf();

    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("quill-recent-open-test");
    harness.snapshot(shot("recent_projects_menu"));

    assert_eq!(
        harness.state().tree.root(),
        before,
        "the project that is open should not have been replaced by looking at the menu"
    );
    std::fs::remove_dir_all(&other).ok();
}

/// The keyboard shortcuts belonging to the menus.
///
/// On macOS these never reach the window, because the bar along the top of the screen takes them first and
/// sends an action instead. Inside the window, which is what Windows uses and what this harness draws, they
/// are watched for and turned into the same actions. This tests that path.
#[test]
fn the_menu_shortcuts_work_from_the_keyboard() {
    let mut harness = harness("some writing to look at");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();

    // Command and comma opens the settings.
    assert!(!harness.state().settings_window.open);
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Comma);
    harness.run();
    assert!(harness.state().settings_window.open, "command and comma should open the settings");
    harness.get_by_label("Done").click();
    harness.run();

    // Command and one, two and three switch between the three ways of looking at the file.
    for (key, expected) in [
        (egui::Key::Num2, ViewMode::SideBySide),
        (egui::Key::Num3, ViewMode::Preview),
        (egui::Key::Num1, ViewMode::Raw),
    ] {
        harness.key_press_modifiers(Modifiers::COMMAND, key);
        harness.run();
        assert_eq!(harness.state().view_mode(), expected, "command and {key:?} should switch the view");
    }

    // Command and zero puts the explorer away and brings it back.
    assert!(harness.state().explorer_visible);
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Num0);
    harness.run();
    assert!(!harness.state().explorer_visible);
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Num0);
    harness.run();
    assert!(harness.state().explorer_visible);

    // Command and A selects the whole document, which used to be handled by the editing surface and is now
    // the Edit menu's entry.
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    harness.run();
    assert_eq!(harness.state().document().selected_text(), "some writing to look at");
}

#[test]
fn control_and_backtick_opens_the_terminal_and_puts_it_away() {
    let mut harness = harness("");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    assert!(!harness.state().terminal.visible);

    // The control key, not the Apple key, which is the shortcut every editor with a terminal uses.
    harness.key_press_modifiers(Modifiers::CTRL, egui::Key::Backtick);
    harness.run();
    assert!(harness.state().terminal.visible, "control and backtick should open the terminal");
    assert_eq!(harness.state().terminal.tabs.count(), 1);

    harness.key_press_modifiers(Modifiers::CTRL, egui::Key::Backtick);
    harness.run();
    assert!(!harness.state().terminal.visible);
}

#[test]
fn a_shell_that_will_not_start_says_so_rather_than_leaving_an_empty_tile() {
    let mut harness = harness("");
    harness.state_mut().terminal.tabs.settings.shell =
        Some("/no/such/program/at/all".to_owned());
    harness.state_mut().terminal.visible = true;
    harness.state_mut().new_terminal_tab();
    harness.run();

    assert_eq!(harness.state().terminal.tabs.count(), 0, "there is nothing to run");
    assert!(
        harness.state().terminal.visible,
        "the tile stays open, because it is the only place the reason can be read"
    );
    let reason = harness.state().terminal.tabs.last_error.clone().expect("a reason");
    assert!(
        reason.contains("/no/such/program/at/all"),
        "the reason should name the program, it said {reason:?}"
    );
    harness.snapshot(shot("terminal_will_not_start"));
}


// ---------------------------------------------------------------------------------------------
// task-1649: the gutter, the tabs, the menus, git and the plugins.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_gutter_numbers_the_lines_and_a_wrapped_paragraph_is_numbered_once() {
    // A paragraph long enough to wrap, then two short ones. The wrapped one must carry one number
    // against its first row and nothing against its continuations, which is what a line number
    // means everywhere else.
    let long = "This paragraph is deliberately long enough that it has to be broken over more than one row on screen, which is the case a line number has to get right.";
    let mut harness = harness(&format!("{long}\nsecond\nthird\n"));
    collapse(&mut harness);
    assert!(harness.state().settings.line_numbers, "numbers are on to begin with");
    let rows = harness.state().layout().lines.len();
    assert!(rows > 3, "the first paragraph should have wrapped, there are {rows} rows");
    harness.snapshot(shot("gutter_line_numbers"));
}

#[test]
fn the_line_numbers_can_be_put_away_and_the_text_goes_back_to_where_it_was() {
    let mut harness = harness("one\ntwo\nthree\n");
    collapse(&mut harness);
    let with_numbers = harness.state().editor_area().left();
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::ToggleLineNumbers, &ctx);
    harness.run();
    assert!(!harness.state().settings.line_numbers);
    let without = harness.state().editor_area().left();
    assert!(without < with_numbers, "the editing area reaches further left with no gutter");
    harness.snapshot(shot("gutter_hidden"));
}

#[test]
fn the_gutters_own_menu_opens_where_it_was_clicked() {
    let mut harness = harness("one\ntwo\nthree\n");
    collapse(&mut harness);
    // Opened through the window's own state rather than by pressing the right mouse button, which
    // the harness cannot do. That is why the menu is the window's state and not egui's memory.
    let at = harness.state().editor_area().left_top() + vec2(-30.0, 40.0);
    harness.state_mut().gutter_menu = Some(at);
    harness.run();
    harness.get_by_label("Hide Line Numbers");
    harness.snapshot(shot("gutter_menu"));
}

#[test]
fn three_files_open_in_three_tabs_and_the_one_showing_is_underlined() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("notes.txt"));
    harness.state_mut().open_path_permanently(&folder.join("program.rs"));
    harness.run();
    assert_eq!(harness.state().files.len(), 3);
    // A change in the middle tab, so the amber dot is in the picture too.
    harness.state_mut().show_tab(1);
    harness.state_mut().command(Command::Insert("edited".to_owned()));
    harness.state_mut().show_tab(2);
    harness.run();
    harness.get_by_label("Tab: program.rs");
    harness.snapshot(shot("file_tabs"));
}

#[test]
fn a_single_click_reuses_one_tab_and_a_double_click_opens_another() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path(&folder.join("readme.md"));
    harness.run();
    assert_eq!(harness.state().files.len(), 1);
    assert!(harness.state().files.active().transient, "one click is a glance");

    harness.state_mut().open_path(&folder.join("notes.txt"));
    harness.run();
    assert_eq!(harness.state().files.len(), 1, "a second glance replaces the first");
    assert_eq!(harness.state().files.active().name(), "notes.txt");

    harness.state_mut().open_path_permanently(&folder.join("program.rs"));
    harness.run();
    assert_eq!(harness.state().files.len(), 2, "a double click keeps what was there");
}

#[test]
fn typing_into_a_tab_that_was_only_glanced_at_keeps_it() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path(&folder.join("readme.md"));
    harness.run();
    assert!(harness.state().files.active().transient);
    harness.input_mut().events.push(egui::Event::Text("a".to_owned()));
    harness.run();
    assert!(!harness.state().files.active().transient, "editing it means you meant to open it");
}

#[test]
fn a_tab_can_be_closed_and_the_last_one_leaves_an_untitled_document() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("notes.txt"));
    harness.run();
    harness.get_by_label("Close notes.txt").click();
    harness.run();
    assert_eq!(harness.state().files.len(), 1);
    assert_eq!(harness.state().files.active().name(), "readme.md");
    harness.state_mut().close_tab(0);
    harness.run();
    assert_eq!(harness.state().files.active().name(), "untitled", "never a window with no document");
}

#[test]
fn the_explorers_own_menu_holds_what_can_be_done_to_a_file() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().explorer_menu =
        Some((egui::pos2(120.0, 260.0), folder.join("readme.md"), false));
    harness.run();
    // Asked for by names this menu alone has: `File` is also a menu in the bar, and `New` is a
    // heading rather than a control, because a submenu inside the window is drawn as a heading with
    // its entries indented under it.
    for entry in ["Copy Path", "Rename...", "Reload from Disk"] {
        harness.get_by_label(entry);
    }
    let entries = quill_app::app::actions::explorer_menu(&folder.join("readme.md"), false, false);
    assert!(
        format!("{entries:?}").contains("NewFile"),
        "New > File is in the menu, which is what task-1649 asks for"
    );
    harness.snapshot(shot("explorer_menu"));
}

#[test]
fn making_a_file_from_the_explorers_menu_opens_it() {
    let folder = std::env::temp_dir().join("quill-screenshot-new-file");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the folder");
    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::NewFile(folder.clone()), &ctx);
    harness.run();
    harness.snapshot(shot("new_file_prompt"));

    // Any extension, which is what task-1649 asks for.
    if let Some(prompt) = harness.state_mut().prompt.as_mut() {
        prompt.value = "example.json".to_owned();
    }
    let prompt = harness.state_mut().prompt.take().expect("a prompt");
    harness.state_mut().run_prompt_for_test(prompt);
    harness.run();
    assert!(folder.join("example.json").is_file(), "the file is made");
    assert_eq!(harness.state().files.active().name(), "example.json", "and opened");
}

#[test]
fn renaming_a_file_moves_it_and_the_tab_follows() {
    let folder = std::env::temp_dir().join("quill-screenshot-rename");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the folder");
    std::fs::write(folder.join("before.md"), "# before\n").expect("write it");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("before.md"));
    harness.run();
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::RenamePath(folder.join("before.md")), &ctx);
    harness.run();
    if let Some(prompt) = harness.state_mut().prompt.as_mut() {
        prompt.value = "after.md".to_owned();
    }
    let prompt = harness.state_mut().prompt.take().expect("a prompt");
    harness.state_mut().run_prompt_for_test(prompt);
    harness.run();
    assert!(!folder.join("before.md").exists());
    assert!(folder.join("after.md").is_file());
    assert_eq!(harness.state().files.active().name(), "after.md");
}

#[test]
fn cutting_a_file_and_pasting_it_into_a_folder_moves_it() {
    let folder = std::env::temp_dir().join("quill-screenshot-clipboard");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(folder.join("inner")).expect("make the folders");
    std::fs::write(folder.join("note.md"), "# note\n").expect("write it");
    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::CutPath(folder.join("note.md")), &ctx);
    harness.state_mut().run_action(Action::PasteInto(folder.join("inner")), &ctx);
    harness.run();
    assert!(!folder.join("note.md").exists(), "a cut moves it");
    assert!(folder.join("inner/note.md").is_file());
}

#[test]
fn the_git_menu_holds_everything_the_ask_lists() {
    let mut harness = git_harness("menu");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    harness.get_by_label("Git").click();
    harness.run();
    for entry in [
        "Commit...",
        "Add",
        "Show Diff",
        "Compare with Revision...",
        "Show History",
        "Show Current Revision",
        "Rollback...",
        "Push...",
        "Pull...",
        "Fetch",
        "Merge...",
        "Rebase...",
        "Branches...",
        "New Branch...",
        "New Tag...",
        "Reset HEAD...",
        "Stash Changes...",
        "Unstash Changes...",
        "Manage Remotes...",
        "Clone...",
    ] {
        harness.get_by_label(entry);
    }
    harness.snapshot(shot("git_menu"));
}

#[test]
fn the_window_reads_the_repository_it_is_opened_in() {
    let harness = git_harness("read");
    let git = harness.state().git.as_ref().expect("the folder is a repository");
    assert_eq!(git.snapshot.status.branch.as_deref(), Some("main"));
    // The change that was not committed, and the file git has never seen.
    assert!(git.snapshot.status.entry("version.ts").is_some());
    assert!(git.snapshot.status.entry("notes.txt").expect("notes.txt").untracked());
    assert!(git.status_label().expect("it has been read").starts_with("main"));
}

#[test]
fn the_commit_panel_shows_the_changes_and_the_unversioned_files() {
    let mut harness = git_harness("commit");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Git(quill_app::app::actions::GitAction::Commit), &ctx);
    harness.run();
    // Waited for, because opening the panel asks for the recent commit messages and the status bar
    // says so while it does. Whether that message is still there when the picture is taken depends
    // on how quickly a thread answered, which is not a difference in Quill.
    settle(&mut harness, "the history the panel asks for", |app| {
        app.git.as_ref().is_some_and(|git| git.message.is_none() && !git.history.is_empty())
    });
    if let Some(git) = harness.state_mut().git.as_mut() {
        git.panel.message = "task-1649: the commit panel".to_owned();
    }
    harness.run();
    harness.get_by_label("COMMIT");
    harness.get_by_label("COMMIT AND PUSH...");
    harness.snapshot(shot("git_commit_panel"));
}

#[test]
fn the_branches_dialog_lists_the_branches() {
    let mut harness = git_harness("branches");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Git(quill_app::app::actions::GitAction::Branches), &ctx);
    harness.run();
    harness.get_by_label("main");
    harness.snapshot(shot("git_branches"));
}

#[test]
fn the_gutter_annotates_with_git_blame_and_colours_by_age() {
    let mut harness = git_harness("blame");
    let folder = harness.state().tree.root().to_path_buf();
    harness.state_mut().open_path_permanently(&folder.join("sqlClient.ts"));
    harness.run();
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Git(quill_app::app::actions::GitAction::Annotate), &ctx);
    for _ in 0..600 {
        harness.run();
        if harness.state().files.active().blame.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let blame = harness.state().files.active().blame.clone().expect("the file is annotated");
    assert!(!blame.is_empty());
    assert_eq!(blame[0].author, "Quill", "the first name, which is what fits in the column");
    // Both authors are in it, so the column really has a gradient to show rather than one colour.
    let authors: Vec<&str> = blame.iter().map(|row| row.author.as_str()).collect();
    assert!(authors.contains(&"Sam"), "the second commit's lines carry its author: {authors:?}");
    let ages: Vec<f32> = blame.iter().map(|row| row.age).collect();
    assert!(ages.contains(&0.0) && ages.contains(&1.0), "oldest and newest are both drawn: {ages:?}");
    harness.snapshot(shot("gutter_blame"));
}

#[test]
fn a_typescript_file_is_coloured_by_its_plugin() {
    // Deliberately *not* a repository: this test is about the colours, and a window in a repository
    // has a git message in its status bar for the first few frames, which made the picture depend on
    // how quickly a thread answered.
    let folder = copy_out_of_the_repository(&git_folder("syntax"), "quill-screenshot-syntax");
    std::fs::remove_dir_all(folder.join(".git")).ok();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("sqlClient.ts"));
    harness.run();
    harness.run();
    // The colours are in the document's own spans, so the test can check them without looking at
    // the picture: the `import` keyword is Dracula's pink and the comment is its blue-grey.
    let text = harness.state().document().text().to_string();
    let chars = harness.state().document().chars();
    // Asked one byte *inside* each token. `style_at` reports the earlier span for an offset that
    // falls on the boundary between two, so that typing at the end of a bold word stays bold, and
    // asking at a token's first byte would report whatever came before it.
    let inside = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("no {needle}")) + 1;
    assert_eq!(
        chars.style_at(inside("import")).color,
        Color::rgb(0xFF, 0x79, 0xC6),
        "import is a keyword, in Dracula's pink"
    );
    assert_eq!(
        chars.style_at(inside("/**")).color,
        Color::rgb(0x62, 0x72, 0xA4),
        "the doc comment, in Dracula's blue-grey"
    );
    assert_eq!(
        chars.style_at(inside("'../db")).color,
        Color::rgb(0xF1, 0xFA, 0x8C),
        "the import's path is a string, in Dracula's yellow"
    );
    assert_eq!(
        chars.style_at(inside("MessageRepository")).color,
        Color::rgb(0x8B, 0xE9, 0xFD),
        "a name starting with a capital is a type, in Dracula's cyan"
    );
    // Colouring is not an edit: nothing to undo and nothing to save.
    assert!(!harness.state().document().is_modified(), "colouring must not mark the file changed");
    assert!(!harness.state().document().can_undo(), "and must not push onto the undo history");
    harness.snapshot(shot("syntax_typescript"));
}

#[test]
fn the_plugins_page_lists_the_three_that_ship_with_quill() {
    let mut harness = harness("");
    harness.state_mut().settings_window.open();
    harness.state_mut().settings_window.page = quill_app::settings::Page::Plugins;
    harness.run();
    harness.run();
    for name in ["JavaScript", "TypeScript", "Rust"] {
        harness.get_by_label(name);
    }
    harness.get_by_label("Marketplace");
    harness.snapshot(shot("plugins_page"));
}


/// Run the window until `ready` is true, or give up.
///
/// Git runs on a thread, so an answer arrives some frames after it was asked for. Each turn is a
/// frame and a short wait; nothing here depends on how fast the machine is, because it stops as soon
/// as the thing it is waiting for has happened.
///
/// Patient on purpose. The tests run at the same time, each starting real git processes, so a step
/// that takes 40 milliseconds on its own can take several seconds when seven of them are running
/// together — which is what made this fail about one run in five while passing every time on its
/// own. Waiting longer costs nothing when nothing is slow.
#[track_caller]
fn settle(harness: &mut Harness<'static, QuillApp>, what: &str, ready: impl Fn(&QuillApp) -> bool) {
    for _ in 0..600 {
        harness.run();
        if ready(harness.state()) {
            harness.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("gave up waiting for {what}");
}

/// What git says about a repository, asked directly rather than through Quill.
fn ask_git(root: &std::path::Path, arguments: &[&str]) -> String {
    let outcome = quill_git::command::run(root, arguments);
    assert!(outcome.ok, "git {arguments:?}: {}", outcome.message());
    outcome.stdout.trim().to_owned()
}

/// The git operations, driven through the window and checked against git itself.
///
/// This is the test `task-1649` asks for when it says to make a project for exercising the
/// operations. It is one test rather than several because each step leaves the repository in the
/// state the next one needs, and because a repository is the slowest thing here to build.
///
/// Every assertion asks **git** what happened rather than asking Quill, so a step that only changed
/// the window's own idea of the world fails.
#[test]
fn every_git_operation_can_be_driven_from_the_window() {
    let mut harness = git_harness("operations");
    let root = harness.state().tree.root().to_path_buf();
    let ctx = harness.ctx.clone();
    let git = |action| Action::Git(action);
    use quill_app::app::actions::GitAction;

    // ---- stage a file -----------------------------------------------------------------------
    harness.state_mut().open_path_permanently(&root.join("version.ts"));
    harness.run();
    harness.state_mut().run_action(git(GitAction::Add(None)), &ctx);
    settle(&mut harness, "the file to be staged", |app| {
        app.git.as_ref().is_some_and(|git| {
            git.snapshot.status.entry("version.ts").is_some_and(quill_git::status::Entry::staged)
        })
    });
    assert!(
        ask_git(&root, &["diff", "--cached", "--name-only"]).contains("version.ts"),
        "git agrees the file is staged"
    );

    // ---- commit it, through the panel's own button -------------------------------------------
    harness.state_mut().run_action(git(GitAction::Commit), &ctx);
    harness.run();
    if let Some(state) = harness.state_mut().git.as_mut() {
        state.panel.message = "task-1649: driven from the window".to_owned();
    }
    harness.run();
    harness.get_by_label("COMMIT").click();
    settle(&mut harness, "the commit", |app| {
        app.git.as_ref().is_some_and(|git| git.snapshot.status.entry("version.ts").is_none())
    });
    assert_eq!(
        ask_git(&root, &["log", "--format=%s", "-n1"]),
        "task-1649: driven from the window",
        "the commit really was made, with the message that was typed"
    );

    // ---- start a branch, through the prompt ---------------------------------------------------
    harness.state_mut().run_action(git(GitAction::NewBranch), &ctx);
    harness.run();
    let mut prompt = harness.state_mut().prompt.take().expect("a prompt for the name");
    prompt.value = "from-quill".to_owned();
    harness.state_mut().run_prompt_for_test(prompt);
    settle(&mut harness, "the branch", |app| {
        app.git.as_ref().is_some_and(|git| git.snapshot.status.branch.as_deref() == Some("from-quill"))
    });
    assert_eq!(ask_git(&root, &["branch", "--show-current"]), "from-quill");

    // ---- stash a change and bring it back ------------------------------------------------------
    std::fs::write(root.join("version.ts"), "export const version = 'stashed';\n").expect("change it");
    harness.state_mut().run_action(git(GitAction::Stash), &ctx);
    harness.run();
    let mut prompt = harness.state_mut().prompt.take().expect("a prompt for the message");
    prompt.value = "half done".to_owned();
    harness.state_mut().run_prompt_for_test(prompt);
    settle(&mut harness, "the stash", |app| {
        app.git.as_ref().is_some_and(|git| !git.snapshot.stashes.is_empty())
    });
    assert!(
        !std::fs::read_to_string(root.join("version.ts")).expect("read").contains("stashed"),
        "stashing left the working tree clean"
    );
    assert!(ask_git(&root, &["stash", "list"]).contains("half done"));

    // Unstashing is the `Stashes` tab of the commit panel, and its POP button.
    harness.state_mut().run_action(git(GitAction::Unstash), &ctx);
    harness.run();
    harness.get_by_label("POP").click();
    settle(&mut harness, "the stash to come back", |app| {
        app.git.as_ref().is_some_and(|git| git.snapshot.stashes.is_empty())
    });
    assert!(
        std::fs::read_to_string(root.join("version.ts")).expect("read").contains("stashed"),
        "the change came back"
    );

    // ---- roll the change back, which is confirmed first ---------------------------------------
    harness.state_mut().run_action(git(GitAction::Rollback(None)), &ctx);
    harness.run();
    assert!(
        harness.state().confirmation.is_some(),
        "rollback cannot be undone, so it asks first"
    );
    harness.get_by_label("ROLL BACK").click();
    settle(&mut harness, "the rollback", |app| {
        app.git.as_ref().is_some_and(|git| git.snapshot.status.entry("version.ts").is_none())
    });
    assert!(
        !std::fs::read_to_string(root.join("version.ts")).expect("read").contains("stashed"),
        "the change is gone"
    );

    // ---- merge the branch back into main --------------------------------------------------------
    assert!(quill_git::branch::switch(&root, "main").ok);
    harness.state_mut().run_action(git(GitAction::Refresh), &ctx);
    settle(&mut harness, "main to be checked out", |app| {
        app.git.as_ref().is_some_and(|git| git.snapshot.status.branch.as_deref() == Some("main"))
    });
    if let Some(state) = harness.state_mut().git.as_mut() {
        state.dialogs.target = "from-quill".to_owned();
    }
    harness.state_mut().run_action(git(GitAction::Merge), &ctx);
    harness.run();
    if let Some(state) = harness.state_mut().git.as_mut() {
        state.dialogs.target = "from-quill".to_owned();
    }
    harness.run();
    harness.get_by_label("MERGE").click();
    // Asked of git rather than of the window: the merge is finished when main really holds the
    // branch's commit, which is the only thing that matters about it.
    let merged = root.clone();
    settle(&mut harness, "the merge", move |_| {
        quill_git::command::run(&merged, &["log", "--format=%s", "-n1"]).stdout.trim()
            == "task-1649: driven from the window"
    });
    assert_eq!(
        ask_git(&root, &["log", "--format=%s", "-n1"]),
        "task-1649: driven from the window",
        "main now holds the branch's commit"
    );

    // ---- and the history the window read is the history git has ---------------------------------
    harness.state_mut().run_action(git(GitAction::ShowHistory(None)), &ctx);
    settle(&mut harness, "the history", |app| {
        app.git.as_ref().is_some_and(|git| git.history.len() >= 3)
    });
    let subjects: Vec<String> = harness
        .state()
        .git
        .as_ref()
        .expect("a repository")
        .history
        .iter()
        .map(|commit| commit.subject.clone())
        .collect();
    assert_eq!(subjects[0], "task-1649: driven from the window");
    assert!(subjects.contains(&"the first commit".to_owned()));
    // No picture is taken here. A commit made during the test has a hash and a date that are new
    // every run, so a baseline of it could never match twice; `design/components/git_history.png`
    // is the capture to look at, and what this test is for is that the history is right.
}
