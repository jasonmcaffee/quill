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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use eframe::egui_wgpu::{RenderState, Renderer, RendererOptions};
use egui::epaint::mutex::RwLock;
use egui::{vec2, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::wgpu::WgpuTestRenderer;
use egui_kittest::{Harness, SnapshotResults};
use quill_app::QuillApp;
use quill_app::app::ViewMode;
use quill_app::app::actions::{Action, FoldAction, HighlightColor, RunAction};
use quill_app::components::about_dialog::About;
use quill_app::components::title_bar::MenuPlacement;
use quill_app::settings;
use quill_core::{Align, Color, Command, StyleChange};

const WINDOW: [f32; 2] = [1180.0, 740.0];

/// How many graphics devices the ninety one screenshots share between them.
///
/// One would do, and one is what the first fix for `task-1654` used, but a single device made the
/// run four times slower — 27 seconds against 7 — because every test's renderer has a shader to
/// compile and a pipeline to build, and on one device those queue up behind each other. A handful of
/// devices gives the tests somewhere to spread out while still being a fixed number built once, which
/// is the part that matters. Measured on this machine: ninety one devices 7.00 s, one device 26.77 s,
/// eight devices 5.97 s — so this is quicker than what it replaces as well as safer.
const DEVICES: usize = 8;

/// A graphics device for one harness, taken from the small set the whole test binary shares.
///
/// `egui_kittest`'s `.wgpu()` builds a **new** graphics instance, adapter and device for each
/// harness. There are ninety one tests here and the test runner gives each one a thread, so that was
/// ninety one devices built and torn down across thirty two threads inside eight seconds, with the
/// Vulkan loader, both vendors' drivers, the Direct3D runtime and the software rasteriser loading and
/// unloading underneath. `task-1654` is what that cost: the process died of an access violation on
/// about one run in nine, part way through the run — eight tests in, once — while every test that had
/// finished said `ok`.
///
/// Every test wants the same thing, a device to draw the window into and read the pixels back, so
/// [`DEVICES`] of them are built on the first call and handed out in turn from there. Nothing is ever
/// torn down until the process ends, which is what removes the fault. The adapter is still chosen by
/// `egui_kittest`'s own selector, so which card draws the screenshots has not changed and neither
/// have the accepted images.
///
/// Each harness still gets a **renderer** of its own, which is what keeps the tests independent: the
/// font atlas and the textures a test uploads belong to that test and are freed with it.
fn shared_render_state() -> RenderState {
    static SHARED: OnceLock<Vec<RenderState>> = OnceLock::new();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let shared = SHARED.get_or_init(|| {
        (0..DEVICES)
            .map(|_| {
                egui_kittest::wgpu::create_render_state(
                    egui_kittest::wgpu::default_wgpu_setup(),
                    RendererOptions::PREDICTABLE,
                )
            })
            .collect()
    });
    let mut state = shared[NEXT.fetch_add(1, Ordering::Relaxed) % DEVICES].clone();
    state.renderer = Arc::new(RwLock::new(Renderer::new(
        &state.device,
        state.target_format,
        RendererOptions::PREDICTABLE,
    )));
    state
}

/// A harness builder that draws on a shared device rather than making a device of its own.
///
/// Every harness in this file is built through here rather than through `Harness::builder`, so that a
/// test added later cannot go back to a device of its own without meaning to. See
/// [`shared_render_state`].
fn builder<State>() -> egui_kittest::HarnessBuilder<State> {
    egui_kittest::HarnessBuilder::default()
        .renderer(WgpuTestRenderer::from_render_state(shared_render_state()))
}

/// A folder with a nested structure, for the explorer screenshots. Written once and left in place, so
/// that the tree looks the same in every run and the images stay comparable.
///
/// Written once **per run** as well, which it was not before. Most of the tests in this file want this
/// folder, they run at the same time, and every one of them used to rewrite it — so a test could be
/// reading `readme.md` at the moment another test's `File::create` had truncated it and not yet
/// written the bytes back. That is what
/// `clicking_a_file_in_the_explorer_opens_it_in_the_editor` failing with
///
/// ```text
/// assertion `left == right` failed: clicking the file should have loaded it
///   left: ""
///  right: "# Quill\n"
/// ```
///
/// was: not a fault in the explorer, a fixture being written out from underneath it. The lock builds
/// the folder once and everyone else waits for it and then reads a file nobody is writing.
fn sample_folder() -> std::path::PathBuf {
    static FOLDER: OnceLock<std::path::PathBuf> = OnceLock::new();
    FOLDER.get_or_init(build_sample_folder).clone()
}

/// Write the sample folder out. Called once, through [`sample_folder`].
fn build_sample_folder() -> std::path::PathBuf {
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
    // A real picture. It is not text, and since `task-1658` it opens all the same, in a tab that shows
    // it. Written rather than checked in, so the tests carry no binary fixture, and drawn as a plain
    // gradient with a band across it so that a screenshot of it is obviously the picture and obviously
    // the right way up.
    write_sample_picture(&root.join("picture.png"));
    // A file that is neither text nor a picture. It is listed, dimmed, and does not respond to a click.
    // The bytes are the start of a zip, including the zero byte that says it is not text.
    std::fs::write(root.join("bundle.zip"), [0x50, 0x4B, 0x03, 0x04, 0]).expect("write bundle.zip");
    root
}

/// Write a small PNG for the explorer's picture row and the picture tab to show.
///
/// A hundred and sixty by a hundred, which is smaller than the editing area, so the tab shows it at its
/// own size and a test that zooms has somewhere to go. A blue to green gradient with a lighter band
/// across the top third, so that a person looking at the screenshot can see at a glance that it is the
/// right picture, the right way up and the right size.
fn write_sample_picture(path: &std::path::Path) {
    let (width, height) = (160_u32, 100_u32);
    let mut picture = image::RgbaImage::new(width, height);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        let across = x as f32 / width as f32;
        let down = y as f32 / height as f32;
        let band = if (0.30..0.42).contains(&down) { 70 } else { 0 };
        *pixel = image::Rgba([
            (0x28 as f32 + across * 40.0) as u8 + band,
            (0x60 as f32 + down * 90.0) as u8 + band,
            (0xF0 as f32 - across * 110.0) as u8,
            255,
        ]);
    }
    picture.save(path).expect("write picture.png");
}

/// Build the application with `text` already in the document.
fn harness(text: &str) -> Harness<'static, QuillApp> {
    let folder = sample_folder();
    let text = text.to_owned();
    let mut harness = builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
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
    let mut harness = builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
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

/// Open the panel behind the toolbar's `F` button, which is where the formatting controls live.
///
/// `task-1657` moved them there: bold, the colours, the alignments and the line spacings are all one
/// click further away than they were, and a test that wants to press one presses this first. The
/// names did not change, so what a test asks for afterwards is what it always asked for.
fn open_text_options(harness: &mut Harness<'static, QuillApp>) {
    harness.get_by_label("Text options").click();
    harness.run();
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

/// Draw the window while a loop above waits for something a thread is still working on.
///
/// A polling loop cannot use `Harness::run`. That gives the window four steps to go quiet and panics
/// if it has not — the right budget for a settled window, and the wrong one here, because while git
/// is still running or a picture is still being decoded the window is *meant* to keep asking to be
/// drawn, and on a loaded machine it can ask for longer than four steps. Under a debugger, which
/// slows the run by about two and a half times, that is exactly how
/// `every_git_operation_can_be_driven_from_the_window` failed:
///
/// ```text
/// Harness::run exceeded max_steps (4). Repaint causes: []
/// ```
///
/// The waiting is what the loop is for, so running out of steps inside one attempt is not a failure.
/// Running out of *attempts* is, and the caller says so.
fn pump(harness: &mut Harness<'static, QuillApp>) {
    let _ = harness.try_run();
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
        pump(&mut harness);
        if harness.state().git.as_ref().is_some_and(|git| !git.snapshot.status.entries.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    harness.run();
    harness
}

#[test]
fn startup_shows_the_rail_the_explorer_and_an_empty_editor() {
    let mut harness = harness("");
    // The rail of pane buttons down the far left, which `task-1658` asks for.
    for button in ["Project", "Version Control", "Terminal tile"] {
        harness.get_by_label(button);
    }
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
    // Click the real button rather than sending the command, which now means opening the panel it
    // moved into first.
    open_text_options(&mut harness);
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

/// Undo and redo are on the keyboard and in the Edit menu. The buttons they used to have are gone,
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
fn the_text_tools_no_longer_hold_the_font_the_opacity_or_undo_and_redo() {
    // They moved: the font and the background are in `Edit -> Settings`, and undo and redo are on the
    // keyboard. A button that is still there would mean the move was not finished.
    let harness = harness("some text");
    for gone in ["Undo", "Redo", "Font family", "Font size", "Background opacity"] {
        assert!(
            harness.query_by_label(gone).is_none(),
            "{gone} should not be among the text tools any more"
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
fn the_title_bar_names_the_project_and_carries_the_window_buttons_and_the_text_tools() {
    // `task-1658` took the open file's name out of the bar, because it is on its own tab already, and
    // put the project's name after the menus instead. The text tools moved in beside the window
    // buttons at the same time.
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    for button in ["Close", "Minimise", "Maximise"] {
        harness.get_by_label(button);
    }
    let title_bar = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(WINDOW[0], 50.0));
    for tool in ["Text options", "Raw Markdown", "Side by side", "Markdown preview"] {
        let at = harness.get_by_label(tool).rect();
        assert!(
            title_bar.contains_rect(at),
            "{tool} should be in the title bar, and it is at {at:?}"
        );
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
    assert_eq!(all, 9, "the sample folder holds nine files");
    assert_eq!(
        harness.state().tree.openable_count(),
        8,
        "every one but the archive can be opened, including the Rust file and the picture"
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
    // The rail is what brings it back. There used to be a small button floating over the editing area
    // for this, and the rail replaced it: it is in the same place whether the explorer is showing or
    // not, which a button drawn only when the pane is hidden is not.
    harness.get_by_label("Project").click();
    harness.run();
    assert!(harness.state().explorer_visible);
}



#[test]
fn the_formatting_controls_are_behind_the_font_button_and_all_reachable_by_name() {
    let mut harness = harness("some text");
    // Shut, the strip holds one control. This is the half of `task-1657` that took the formatting
    // off the top of the window: nine controls that are set rarely no longer take its whole width.
    harness.get_by_label("Text options");
    assert!(
        harness.query_by_label("Bold").is_none(),
        "the formatting is behind the button until the button is pressed"
    );
    open_text_options(&mut harness);
    for name in [
        "Bold", "Italic", "Underline", "Strikethrough",
        "Left", "Center", "Right", "Justify",
        "Single", "One and a half", "Double",
    ] {
        harness.get_by_label(name);
    }
    for colour in ["White", "Red", "Green", "Blue", "Amber"] {
        harness.get_by_label(colour);
    }
    harness.snapshot(shot("text_options"));
}

#[test]
fn a_code_file_has_no_text_tools_and_nothing_below_them_moves() {
    // Everything in the tools is about how prose is shown, and Quill saves plain text and carries no
    // formatting to disk, so bold on a `.rs` file is a decoration that lasts until the file is
    // reopened. They are not drawn at all for one.
    //
    // They used to sit in a strip of their own, forty four points tall, so switching between a `.md`
    // file and a `.rs` one moved the tabs, the explorer and the editing area up and down by forty four
    // points. `task-1658` moved them into the title bar, whose height never changes, and this is the
    // half of that which is worth a test: the window below them does not move.
    let mut prose = harness("");
    prose.get_by_label("readme.md").click();
    prose.run();
    let with_tools = prose.state().editor_area().top();
    prose.get_by_label("Text options");

    let mut code = harness("");
    code.get_by_label("program.rs").click();
    code.run();
    assert!(
        code.query_by_label("Text options").is_none(),
        "a Rust file has no formatting to offer"
    );
    assert!(
        code.query_by_label("Raw Markdown").is_none(),
        "and nothing to preview either"
    );
    let without_tools = code.state().editor_area().top();
    assert!(
        (with_tools - without_tools).abs() < 0.5,
        "the editing area should start in the same place either way: {with_tools} against {without_tools}"
    );
    code.snapshot(shot("code_no_toolbar"));
}

#[test]
fn moving_between_file_tabs_does_not_type_a_tab_into_the_file_it_leaves() {
    // Control and Tab is `Next Tab` on the View menu, and finding an action for a key press does not
    // consume it, so the editing area saw the same press and inserted a tab character. Both files
    // came out marked as having unsaved changes that nobody had made — found while retaking the
    // documentation captures for `task-1657`, and the same shape of fault as `task-1656`.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("notes.txt"));
    harness.run();
    let before: Vec<String> =
        harness.state().files.iter().map(|file| file.document.text().to_string()).collect();

    harness.key_press_modifiers(Modifiers::CTRL, egui::Key::Tab);
    harness.run();
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, egui::Key::Tab);
    harness.run();

    for (index, file) in harness.state().files.iter().enumerate() {
        assert_eq!(file.document.text().to_string(), before[index], "tab {index} was typed into");
        assert!(!file.document.is_modified(), "tab {index} should have no unsaved changes");
    }
}

#[test]
fn a_text_file_keeps_the_formatting_and_loses_the_view_modes() {
    // The two questions are asked separately. A `.txt` file is prose, so the formatting is worth
    // offering; it is not Markdown, so there is nothing to preview.
    let mut harness = harness("");
    harness.get_by_label("notes.txt").click();
    harness.run();
    harness.get_by_label("Text options");
    for mode in ["Raw Markdown", "Side by side", "Markdown preview"] {
        assert!(harness.query_by_label(mode).is_none(), "{mode} has nothing to show for a .txt file");
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
    let mut harness = builder()
        .with_size(vec2(1264.0, 751.0))
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

A paragraph with **bold**, *italic*, ~~struck~~ and `inline code` in it, wrapped
over two lines of source so that it comes out as one paragraph.

## A smaller heading

- a bullet
- another bullet
  - one nested under it

### A list of things to do

- [x] a box that is ticked
- [ ] one that is not

1. first
2. second

> a quoted line
> > and one quoted inside it

| Crate | Lines | Tests |
| ----- | ----: | :---: |
| core | 9132 | 412 |
| terminal | 3004 | 88 |

```rust
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

/// The three things `task-1685` added, in a document short enough that all of them are on the
/// screen at once. `MARKDOWN` is the everything document and is taller than the window.
const MARKDOWN_TABLE: &str = "\
## What the crates hold

| Crate | Lines | Tests |
| ----- | ----: | :---: |
| core | 9132 | 412 |
| terminal | 3004 | 88 |
| app | 17133 | 507 |

Some prose with `inline code` in it, under the table.

```rust
fn main() {
    let greeting = \"hello\";
    println!(\"{greeting}\");
}
```

- [x] a box that is ticked
- [ ] one that is not";

/// **A pipe table is drawn as a table**, which is the first thing `task-1685` asks for.
///
/// The pipes become a box of rules, the columns line up because the whole table is set in the code
/// font, and the words that were in the cells are still there to be read and copied.
#[test]
fn a_table_in_the_preview_is_drawn_in_a_box() {
    let mut harness = harness(MARKDOWN_TABLE);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let preview = harness.state().preview_text();
    assert!(!preview.contains('|'), "the pipes are the box, not the text: {preview}");
    assert!(preview.contains('\u{250C}'), "a top left corner: {preview}");
    assert!(preview.contains("Crate") && preview.contains("9132"), "the cells survive");
    // Every line of the table is the same width, which is what a table means.
    let rows: Vec<&str> = preview
        .lines()
        .filter(|line| line.starts_with('\u{2502}') && line.ends_with('\u{2502}'))
        .collect();
    assert!(rows.len() >= 3, "a head and two rows: {rows:?}");
    assert!(
        rows.windows(2).all(|pair| pair[0].chars().count() == pair[1].chars().count()),
        "{rows:?}"
    );
    harness.snapshot(shot("preview_table"));
}

/// **A tick box is a tick box**, and a quote inside a quote is two bars deep.
#[test]
fn the_preview_reads_the_things_the_old_parser_could_not() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let preview = harness.state().preview_text();
    assert!(preview.contains("\u{2611}  a box that is ticked"), "{preview}");
    assert!(preview.contains("\u{2610}  one that is not"), "{preview}");
    assert!(preview.contains("\u{2502}  \u{2502}  and one quoted inside it"), "{preview}");
    assert!(
        preview.contains("wrapped over two lines"),
        "a hand-wrapped paragraph is one paragraph: {preview}"
    );
}

/// **A fence names a language and the plugin that reads it colours the code.**
///
/// The same two calls `colour_the_file` makes for a `.rs` file, reached through the
/// `CodeHighlighter` seam, so a fence of Rust in a document looks like a Rust file.
#[test]
fn a_fence_of_rust_is_coloured_by_the_plugin_that_reads_rust() {
    let mut harness = harness(MARKDOWN_TABLE);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let preview = harness.state().preview_text();
    let at = preview.find("fn main").expect("the fence is in the preview");
    let keyword = harness.state().preview_style_at(at + 1);
    let name = harness.state().preview_style_at(at + 4);
    assert_ne!(
        keyword.color, name.color,
        "`fn` and `main` are different kinds of thing, so they are different colours"
    );
}

/// **A code block asks for a panel behind it**, which is the whole of "code blocks aren't easy to
/// read": a fence with no ground under it does not read as a block.
#[test]
fn the_preview_puts_code_on_a_panel_and_inline_code_on_a_chip() {
    let mut harness = harness(MARKDOWN_TABLE);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let panels = harness.state().preview_panels();
    assert!(panels.iter().any(|panel| panel.kind == quill_core::PanelKind::Code));
    assert!(panels.iter().any(|panel| panel.kind == quill_core::PanelKind::Table));
    assert!(!harness.state().preview_code_spans().is_empty(), "`inline code` gets a chip");
}

/// **Text in the preview can be selected with the pointer and copied**, which is the ticket's
/// second complaint. The preview is read only; reading includes taking a copy of what you read.
#[test]
fn text_in_the_preview_can_be_selected_by_dragging() {
    let mut harness = harness(MARKDOWN_TABLE);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let area = harness.state().editor_area();
    // The line of prose under the table, which is what a person would drag across.
    let line = area.top() + 303.0;
    drag(
        &mut harness,
        egui::Pos2::new(area.left() + 24.0, line),
        egui::Pos2::new(area.left() + 340.0, line),
    );
    let selected = harness.state().preview_selected_text().unwrap_or_default();
    assert!(!selected.is_empty(), "a drag across a line should have selected something");
    assert!(
        harness.state().preview_holds_the_selection(),
        "and the copy should be about the preview rather than the source"
    );
    harness.snapshot(shot("preview_selection"));
}

/// **And selecting all of it works from the menu**, so a whole page can be taken in one go.
#[test]
fn the_whole_preview_can_be_selected_and_copied() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Markdown preview").click();
    harness.run();
    let area = harness.state().editor_area();
    let at = egui::Pos2::new(area.left() + 30.0, area.top() + 60.0);
    click_at(&mut harness, at);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::SelectAll, &ctx);
    harness.run();
    let selected = harness.state().preview_selected_text().unwrap_or_default();
    assert!(selected.contains("Quill preview"), "the heading is in it");
    assert!(selected.contains("The last paragraph."), "and so is the last line");
}

/// **`Ctrl/Cmd+C` copies the preview**, and it has to be claimed before the source pane is drawn or
/// the source would take the event and copy its own selection instead. egui delivers a copy as an
/// `Event::Copy` rather than as a key press, which is why this is not simply the `Copy` menu entry.
#[test]
fn the_copy_key_in_the_preview_copies_the_preview_and_not_the_source() {
    let mut harness = harness(MARKDOWN_TABLE);
    harness.get_by_label("Side by side").click();
    harness.run();
    let source = harness.state().editor_area();
    // Something selected in the source, so a copy that went to the wrong half would be visible.
    harness.state_mut().command(Command::SelectAll);
    harness.run();
    click_at(&mut harness, egui::Pos2::new(source.right() + 60.0, source.top() + 60.0));
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::SelectAll, &ctx);
    harness.run();

    harness.input_mut().events.push(egui::Event::Copy);
    harness.step();
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(copied.contains("What the crates hold"), "the preview was copied, got {copied:?}");
    assert!(!copied.contains("| ----- |"), "and not the source, got {copied:?}");
}

/// **A click in the source takes the copy back**, so the two halves of the side-by-side view never
/// argue about what `Copy` means.
#[test]
fn a_click_in_the_source_takes_the_copy_back_from_the_preview() {
    let mut harness = harness(MARKDOWN);
    harness.get_by_label("Side by side").click();
    harness.run();
    let source = harness.state().editor_area();
    let preview = egui::Pos2::new(source.right() + 60.0, source.top() + 60.0);
    click_at(&mut harness, preview);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::SelectAll, &ctx);
    harness.run();
    assert!(harness.state().preview_holds_the_selection());
    click_at(&mut harness, egui::Pos2::new(source.left() + 30.0, source.top() + 30.0));
    assert!(
        !harness.state().preview_holds_the_selection(),
        "pressing in the source is what says the copy is about the source"
    );
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
    harness.get_by_label_contains("bundle.zip").click();
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
    let mut harness = builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
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

/// Open the About box the way a person does: `Quill` in the bar, then `About Quill`.
fn open_about(harness: &mut Harness<'static, QuillApp>) {
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    harness.get_by_label("Quill").click();
    harness.run();
    harness.get_by_label("About Quill").click();
    harness.run();
}

/// The version and the build date this build really has would put a different picture in front of
/// the comparison every time the binary was rebuilt, so the test fixes them. `About::current` is
/// covered by a unit test beside the component.
fn a_fixed_about() -> About {
    About {
        developer: "Jason McAffee".to_owned(),
        version: "0.2.0".to_owned(),
        built: "2026-08-25 10:45pm".to_owned(),
    }
}

#[test]
fn the_about_box_names_the_developer_the_version_and_the_build_date() {
    let mut harness = harness("Text behind the About box.");
    assert!(harness.state().about.is_none());
    open_about(&mut harness);
    assert!(harness.state().about.is_some(), "Quill then About Quill should open it");

    harness.state_mut().about = Some(a_fixed_about());
    harness.run();
    harness.get_by_label("Developed by Jason McAffee");
    harness.get_by_label("Version: 0.2.0");
    harness.get_by_label("Build Date: 2026-08-25 10:45pm");
    harness.snapshot(shot("about"));
}

#[test]
fn the_about_box_closes_on_its_button() {
    let mut harness = harness("");
    open_about(&mut harness);
    harness.get_by_label("Done").click();
    harness.run();
    assert!(harness.state().about.is_none());
}

#[test]
fn the_about_box_closes_on_escape() {
    let mut harness = harness("");
    open_about(&mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().about.is_none());
}

#[test]
fn opening_the_about_box_shuts_whatever_else_was_open() {
    // One modal at a time is the rule every other entry point already keeps, and the About box is
    // reached from a menu rather than from `modal open`, which is where it would be easy to forget.
    let mut harness = harness("");
    open_settings(&mut harness);
    assert!(harness.state().settings_window.open);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::About, &ctx);
    harness.run();
    assert!(harness.state().about.is_some());
    assert!(!harness.state().settings_window.open, "the Settings window went away");
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
fn the_terminal_page_holds_the_font_size_and_the_shell() {
    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("Terminal").click();
    harness.run();
    harness.get_by_label("Terminal font size");
    // `task-1670`: the shell is a setting because the default cannot be right for everybody, and this
    // is where a person who wants `cmd.exe` back asks for it.
    harness.get_by_label("Terminal shell");
    assert!(harness.query_by_label("Background opacity").is_none(), "that is on the Appearance page");
    harness.snapshot(shot("settings_terminal"));
}

#[test]
fn the_mcp_page_holds_the_install_buttons_the_server_and_the_configuration_to_copy() {
    // Two things are pinned before the page is drawn, and both are what make a picture of it the
    // same on every machine. `QUILL_HOME` is where the installers look for an agent's own
    // configuration, so a folder of its own is what makes the buttons read `Install for ...`
    // whatever this machine happens to have; `QUILL_CLI_BIN` is the path written into the
    // configuration blocks, which would otherwise be wherever this checkout is.
    let home = std::env::temp_dir().join("quill-mcp-page-home");
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(&home).expect("make the folder");
    std::env::set_var("QUILL_HOME", &home);
    std::env::set_var("QUILL_CLI_BIN", r"C:\Program Files\Quill\quill-cli.exe");

    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("MCP").click();
    harness.run();

    // `task-1679` asks for all four, and this is where a person finds each of them.
    harness.get_by_label("Install for Claude Code");
    harness.get_by_label("Install for Codex");
    harness.get_by_label("MCP port");
    harness.get_by_label("MCP tool shape");
    harness.get_by_label("Claude Code configuration");
    // One block at a time, and the other client's is a click away.
    harness.get_by_label("Codex").click();
    harness.run();
    harness.get_by_label("Codex configuration");
    harness.get_by_label("Claude Code").click();
    harness.run();
    harness.get_by_label("Copy");
    assert!(
        !harness.state().settings.mcp_enabled,
        "the HTTP endpoint is off until somebody turns it on"
    );
    harness.snapshot(shot("settings_mcp"));

    std::env::remove_var("QUILL_HOME");
    std::env::remove_var("QUILL_CLI_BIN");
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn ticking_the_mcp_box_is_a_setting_and_not_a_listener_in_a_test() {
    // A window a test builds never opens a command channel, so it never opens an MCP endpoint
    // either — the rule `open_control_channel` already keeps, for the same reason: a test must not
    // open a port or leave a listener behind when it ends. What the tick box does here is change
    // the setting, which is what is checked.
    let home = std::env::temp_dir().join("quill-mcp-tick-home");
    std::fs::remove_dir_all(&home).ok();
    std::fs::create_dir_all(&home).expect("make the folder");
    std::env::set_var("QUILL_HOME", &home);

    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("MCP").click();
    harness.run();
    harness.get_by_label("Also serve over HTTP on this machine").click();
    harness.run();
    assert!(harness.state().settings.mcp_enabled, "the tick box should have set it");
    assert!(!harness.state().is_serving_mcp(), "a test window must not have opened a port");

    std::env::remove_var("QUILL_HOME");
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn the_shell_typed_into_the_settings_is_what_a_new_terminal_runs() {
    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("Terminal").click();
    harness.run();
    harness.get_by_label("Terminal shell").click();
    harness.run();
    harness.get_by_label("Terminal shell").type_text("/no/such/program/at/all");
    harness.run();
    assert_eq!(harness.state().settings.terminal_shell, "/no/such/program/at/all");

    // Which is what a terminal opened afterwards tries to run — it cannot start, and the tile says so
    // in the shell's own words, which is how the setting is seen to have reached the tab at all.
    harness.state_mut().terminal.visible = true;
    harness.state_mut().new_terminal_tab();
    harness.run();
    let reason = harness.state().terminal.tabs.last_error.clone().expect("a reason");
    assert!(reason.contains("/no/such/program/at/all"), "it said {reason:?}");
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
fn changing_the_font_reaches_every_open_tab_and_not_only_the_one_showing() {
    // `task-1657`. The editor's font is one setting for the whole window, the way IntelliJ has one
    // editor font. It used to reach the active document alone, so opening three files and then
    // changing the font left two of them in the old one until Quill was restarted.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    for name in ["readme.md", "notes.txt", "program.rs"] {
        harness.state_mut().open_path_permanently(&folder.join(name));
    }
    harness.run();
    assert_eq!(harness.state().files.len(), 3, "one tab each, and none of them transient");

    let mut settings = harness.state().settings.clone();
    settings.font_size = 24.0;
    harness.state_mut().set_settings(settings);
    harness.run();

    for (index, file) in harness.state().files.iter().enumerate() {
        assert_eq!(
            file.document.chars().style_at(0).size,
            24.0,
            "tab {index} should be in the new size, whether or not it is the one showing"
        );
    }
}

#[test]
fn the_keyboard_makes_the_text_bigger_and_smaller_and_puts_it_back() {
    // The size the keys reach is the setting the dialog holds, so it survives a restart and reaches
    // every tab. They walk the sizes the dialog offers rather than a step of their own.
    let mut harness = harness("Zoom this line");
    let ctx = harness.ctx.clone();
    assert_eq!(harness.state().settings.font_size, 16.0);

    harness.state_mut().run_action(Action::ChangeFontSize { larger: true }, &ctx);
    harness.run();
    assert_eq!(harness.state().settings.font_size, 20.0, "the next size the dialog offers");
    assert_eq!(harness.state().document().chars().style_at(0).size, 20.0);
    let taller = harness.state().layout().lines[0].height;

    harness.state_mut().run_action(Action::ChangeFontSize { larger: false }, &ctx);
    harness.state_mut().run_action(Action::ChangeFontSize { larger: false }, &ctx);
    harness.run();
    assert_eq!(harness.state().settings.font_size, 13.0);
    assert!(
        harness.state().layout().lines[0].height < taller,
        "smaller text should make a shorter line"
    );

    harness.state_mut().run_action(Action::ResetFontSize, &ctx);
    harness.run();
    assert_eq!(harness.state().settings.font_size, 16.0, "back to what a new Quill has");
    harness.snapshot(shot("font_size_reset"));
}

#[test]
fn a_pinch_over_the_editing_area_steps_the_font_size() {
    // A pinch on a trackpad and the wheel with the zoom modifier held both reach egui as
    // `Event::Zoom`, so this is the same path both take. The pointer has to be over the editing
    // area, because zooming is the document's and not the explorer's.
    let mut harness = harness("Pinch this line");
    let middle = harness.state().editor_area().center();
    assert_eq!(harness.state().settings.font_size, 16.0);

    let pinch = |harness: &mut Harness<'static, QuillApp>, factor: f32| {
        harness.input_mut().events.push(egui::Event::PointerMoved(middle));
        harness.input_mut().events.push(egui::Event::Zoom(factor));
        harness.run();
    };

    // Enough to ask for one size, which is the smallest gap between two of the sizes offered.
    pinch(&mut harness, 1.2);
    assert_eq!(harness.state().settings.font_size, 20.0, "one step up");
    // A bigger pinch asks for more than one, and lands on a size the dialog offers rather than on
    // whatever the multiplier works out to.
    pinch(&mut harness, 1.2 * 1.2 * 1.2);
    let bigger = harness.state().settings.font_size;
    assert!(bigger > 20.0, "a pinch three times the size should go further: {bigger}");
    assert!(settings::FONT_SIZES.contains(&bigger), "{bigger} is not a size the dialog offers");
    // And pinching the other way comes back down.
    pinch(&mut harness, 1.0 / (1.2 * 1.2 * 1.2));
    let smaller = harness.state().settings.font_size;
    assert!(smaller < bigger, "pinching in should shrink it: {bigger} then {smaller}");
    // The document is shown in it, not just the setting.
    assert_eq!(harness.state().document().chars().style_at(0).size, smaller);
}

#[test]
fn a_pinch_too_small_to_ask_for_a_size_is_kept_rather_than_thrown_away() {
    // A pinch arrives as a stream of multipliers a fraction over one, so a step is taken when what
    // has been asked for adds up to one rather than on any single frame. Without the remainder
    // being carried, a slow pinch would never move anything at all.
    let mut harness = harness("Pinch this line");
    let middle = harness.state().editor_area().center();
    for _ in 0..4 {
        harness.input_mut().events.push(egui::Event::PointerMoved(middle));
        harness.input_mut().events.push(egui::Event::Zoom(1.05));
        harness.run();
        harness.run();
    }
    assert_eq!(
        harness.state().settings.font_size,
        20.0,
        "four small pinches add up to one size"
    );
}

/// A file long enough to be scrolled about in, one short line a paragraph so nothing wraps.
fn a_long_file() -> String {
    (0..200).map(|n| format!("line {n} of the file\n")).collect()
}

/// A folder of long files for the zoom tests, kept out of [`sample_folder`] because a file written
/// there is a row in the explorer of every screenshot in this file.
fn a_folder_of_long_files() -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-zoom-folder");
    std::fs::create_dir_all(&root).expect("make the zoom folder");
    std::fs::write(root.join("long.txt"), a_long_file()).expect("write long.txt");
    std::fs::write(root.join("longer.txt"), a_long_file()).expect("write longer.txt");
    std::fs::write(root.join("short.txt"), "a short file\n").expect("write short.txt");
    root
}

/// Which paragraph of `file` is drawn `above` points below the top of its view.
///
/// The pane's own rectangle does not come into it: `above` is a distance below the top of a view,
/// and every pane's view starts at the same height, so this can ask about a file in a pane the test
/// is not focused on.
fn paragraph_in(file: &quill_app::app::files::OpenFile, above: f32) -> usize {
    let offset = file.cached.layout.offset_at(0.0, file.scroll + above);
    file.document.text().byte_to_line(offset)
}

/// The open file with this name, which is how a test asks about a pane it is not focused on.
fn file_named<'a>(app: &'a QuillApp, name: &str) -> &'a quill_app::app::files::OpenFile {
    app.files
        .iter()
        .find(|file| file.name() == name)
        .unwrap_or_else(|| panic!("{name} should be open"))
}

/// Which paragraph of the open file is drawn at `screen_y`.
///
/// The paragraph rather than the line or the offset in points, because it is the same number
/// whatever size the text is drawn at — which is exactly the question `task-1672` asks: is the
/// reader still looking at what they were looking at.
fn paragraph_under(app: &QuillApp, screen_y: f32) -> usize {
    let top = app.editor_area().top() + quill_app::theme::size::EDITOR_PADDING_Y;
    let down = app.files.active().scroll + (screen_y - top);
    let offset = app.layout().offset_at(0.0, down);
    app.document().text().byte_to_line(offset)
}

/// How far below the top of the view the caret is drawn.
fn caret_below_the_top(app: &QuillApp) -> f32 {
    app.layout().caret_at(app.document().selection().head).y - app.files.active().scroll
}

#[test]
fn a_pinch_keeps_the_line_under_the_pointer_where_it_is() {
    // `task-1672`: zooming in and then having to scroll back to the line you were zooming in on is
    // the whole complaint. The text under the pointer is what the gesture is about, so it is what
    // must not move.
    let mut harness = harness(&a_long_file());
    let area = harness.state().editor_area();
    // A long way down the file, so there is room to go wrong in either direction.
    harness.state_mut().files.active_mut().scroll = 900.0;
    harness.run();
    let at = egui::pos2(area.center().x, area.top() + area.height() * 0.6);
    let was_under_the_pointer = paragraph_under(harness.state(), at.y);
    let was_scrolled_to = harness.state().files.active().scroll;

    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.input_mut().events.push(egui::Event::Zoom(1.2));
    harness.run();
    harness.run();

    assert_eq!(harness.state().settings.font_size, 20.0, "the pinch should have asked for a size");
    assert_eq!(
        paragraph_under(harness.state(), at.y),
        was_under_the_pointer,
        "the same line should still be under the pointer"
    );
    // And it took moving the view to keep it there: the same line is further down a document laid
    // out in a larger font, so a scroll position that did not change would be the fault itself.
    assert!(
        harness.state().files.active().scroll > was_scrolled_to,
        "the view should have followed the text down the file"
    );

    // Pinching back out is the same question the other way round. Two sizes' worth, because what a
    // gesture asks for and what a step costs do not divide, and the remainder of the pinch above is
    // carried — so asking for exactly one size back can land either side of the next step.
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.input_mut().events.push(egui::Event::Zoom(1.0 / (1.2 * 1.2)));
    harness.run();
    harness.run();
    let smaller = harness.state().settings.font_size;
    assert!(smaller < 20.0, "pinching out should have come back down: {smaller}");
    assert_eq!(
        paragraph_under(harness.state(), at.y),
        was_under_the_pointer,
        "and still under it on the way back out"
    );
}

#[test]
fn the_keyboards_zoom_keeps_the_caret_where_it_is() {
    // A pinch is about the pointer; the keyboard has none, so it is about the caret, which is what
    // a person pressing command and plus is working on.
    let text = a_long_file();
    let mut harness = harness(&text);
    let ctx = harness.ctx.clone();
    let offset = text.find("line 90 ").expect("the file has a ninetieth line");
    harness.state_mut().command(Command::PlaceCaret { offset, extend: false });
    harness.run();
    // Scrolled so the caret sits well inside the view rather than at either edge of it.
    let caret = harness.state().layout().caret_at(offset).y;
    harness.state_mut().files.active_mut().scroll = caret - 200.0;
    harness.run();
    let was = caret_below_the_top(harness.state());
    assert!((was - 200.0).abs() < 1.0, "the caret should be 200 points down the view, not {was}");

    harness.state_mut().run_action(Action::ChangeFontSize { larger: true }, &ctx);
    harness.run();
    harness.run();
    assert_eq!(harness.state().settings.font_size, 20.0);
    let now = caret_below_the_top(harness.state());
    assert!((now - was).abs() < 3.0, "the caret should have stayed put: {was} then {now}");

    // Back down two sizes, and it is still where it was.
    harness.state_mut().run_action(Action::ChangeFontSize { larger: false }, &ctx);
    harness.run();
    harness.state_mut().run_action(Action::ChangeFontSize { larger: false }, &ctx);
    harness.run();
    harness.run();
    assert_eq!(harness.state().settings.font_size, 13.0);
    let smaller = caret_below_the_top(harness.state());
    assert!((smaller - was).abs() < 3.0, "and still there in a smaller font: {smaller}");
}

#[test]
fn a_zoom_leaves_a_tab_that_was_not_showing_at_the_line_it_was_left_at() {
    // The font is one setting for the whole window, so every tab is laid out again — and a tab that
    // came back scrolled somewhere else would be a tab that had moved while nobody was looking at
    // it. What is kept for those is the top of their view, since there is no pointer over them and
    // no caret being typed at.
    let folder = a_folder_of_long_files();
    let long = folder.join("long.txt");
    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().open_path_permanently(&long);
    harness.run();
    harness.state_mut().files.active_mut().scroll = 900.0;
    harness.run();
    let top = harness.state().editor_area().top() + quill_app::theme::size::EDITOR_PADDING_Y;
    let was_at_the_top = paragraph_under(harness.state(), top);

    // Show a different file, change the size from there, and come back.
    harness.state_mut().open_path_permanently(&folder.join("short.txt"));
    harness.run();
    harness.state_mut().run_action(Action::ChangeFontSize { larger: true }, &ctx);
    harness.run();
    harness.state_mut().open_path_permanently(&long);
    harness.run();
    harness.run();
    assert_eq!(harness.state().files.active().name(), "long.txt");
    assert_eq!(
        paragraph_under(harness.state(), top),
        was_at_the_top,
        "the line it was left at should still be the first one showing"
    );
}

#[test]
fn a_pinch_in_a_split_is_the_pointers_pane_and_steps_the_size_once() {
    // The size is one setting for the window, so a gesture is the window's rather than a pane's.
    // Every pane used to take the same `zoom_delta` for itself, which stepped the size once for
    // each of them: with two panes one notch of the wheel took sixteen points to thirty two.
    let folder = a_folder_of_long_files();
    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().open_path_permanently(&folder.join("long.txt"));
    harness.run();
    harness.state_mut().open_path_permanently(&folder.join("longer.txt"));
    harness.state_mut().run_action(Action::SplitRight, &ctx);
    harness.run();
    assert_eq!(harness.state().files.pane_count(), 2, "there should be two panes");
    let showing = harness.state().files.active().name();
    assert_eq!(showing, "longer.txt", "the new pane has the keyboard");

    // Both panes scrolled into their files, so an anchor that was not applied shows up as a line
    // that is not the one that was there.
    for name in ["long.txt", "longer.txt"] {
        let index = harness.state().files.iter().position(|file| file.name() == name).unwrap();
        harness.state_mut().files.at_mut(index).scroll = 900.0;
    }
    harness.run();

    // The pointer over the left hand pane, which is the one **without** the keyboard. The right
    // hand pane is the focused one, so its rectangle is the one the window reports, and the left
    // one is beside it.
    let right = harness.state().editor_area();
    let down = right.top() + quill_app::theme::size::EDITOR_PADDING_Y + 300.0;
    let at = egui::pos2(right.left() - 100.0, down);
    let was_under_the_pointer = paragraph_in(file_named(harness.state(), "long.txt"), 300.0);
    let was_at_the_top = paragraph_in(file_named(harness.state(), "longer.txt"), 0.0);

    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.input_mut().events.push(egui::Event::Zoom(1.2));
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().settings.font_size,
        20.0,
        "one notch is one size, however many panes thought the gesture was theirs"
    );
    assert_eq!(
        paragraph_in(file_named(harness.state(), "long.txt"), 300.0),
        was_under_the_pointer,
        "the pane under the pointer should have kept the line the pointer was on"
    );
    assert_eq!(
        paragraph_in(file_named(harness.state(), "longer.txt"), 0.0),
        was_at_the_top,
        "and the other pane the line at the top of it, there being no pointer over it"
    );
}

#[test]
fn a_zoom_keeps_the_markdown_previews_place_too() {
    // The preview is laid out from the same base style, so it moves in exactly the same way, and it
    // scrolls on its own — so it needs its own anchor rather than the source's.
    let text: String = (0..120).map(|n| format!("Paragraph {n} of the page.\n\n")).collect();
    let mut harness = harness(&text);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    harness.state_mut().files.active_mut().preview_scroll = 700.0;
    harness.run();
    let at_the_top = |app: &QuillApp| {
        let scrolled = app.files.active().preview_scroll;
        app.preview_layout().offset_at(0.0, scrolled)
    };
    let was = at_the_top(harness.state());
    assert!(was > 0, "the preview should be scrolled into the page, not sitting at the top of it");

    harness.state_mut().run_action(Action::ChangeFontSize { larger: true }, &ctx);
    harness.run();
    harness.run();
    assert_eq!(harness.state().settings.font_size, 20.0);
    assert_eq!(at_the_top(harness.state()), was, "the same words should be at the top of the page");
}

#[test]
fn the_filter_box_puts_its_words_on_the_same_line_as_the_magnifier() {
    // It used to lay them out against the top edge of the box: a `TextEdit` with `Frame::NONE` has
    // no margin to be pushed down by, and it was given the whole height of the field to sit in.
    let harness = harness("");
    let filter = harness.get_by_label("Filter files").rect();
    // The field is 24 points tall, 36 points down the explorer, which itself starts under the title
    // bar: 50 + 36 is 86, so the middle of the field is at 98. The explorer starts after the rail, so
    // the field starts 36 + 12 points in from the left.
    let field = egui::Rect::from_min_size(egui::pos2(48.0, 86.0), vec2(224.0, 24.0));
    assert!(
        filter.height() < field.height(),
        "the box is one row of text, not the whole field: {filter:?}"
    );
    assert!(
        (filter.center().y - field.center().y).abs() < 1.5,
        "the row should sit in the middle of the field: {} against {}",
        filter.center().y,
        field.center().y
    );
}

#[test]
fn the_caret_is_no_taller_than_the_text_it_sits_in() {
    // The caret used to be drawn the full height of the line, which carries the font's line gap, the
    // reading leading Quill adds for prose and the paragraph's line spacing on top of the letters.
    let mut harness = harness("A line to put the caret in\nand a second one");
    harness.state_mut().command(Command::SelectAll);
    harness.state_mut().command(Command::SetLineSpacing(2.0));
    harness.state_mut().command(Command::MoveDocumentStart { extend: false });
    harness.run();
    let layout = harness.state().layout();
    let line = &layout.lines[0];
    let caret = layout.caret_at(0);
    assert!(
        caret.height < line.height * 0.75,
        "at double spacing the caret must be well short of the line: {} against {}",
        caret.height,
        line.height
    );
    assert!(caret.y >= line.y, "and it must start inside its own line");
    assert!(caret.y + caret.height <= line.bottom() + 0.01, "and end inside it");
    harness.snapshot(shot("caret_height"));
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

// The scrollbar, the two halves scrolling together, and dragging a tab — `task-1673`.

/// A long enough file that both halves of the side by side view have somewhere to scroll to.
fn a_long_markdown() -> String {
    let mut source = String::from("# The top of the file\n\n");
    for section in 0..40 {
        source.push_str(&format!("## Section {section}\n\nA paragraph of prose in section {section}, long enough that it is worth reading and takes a line or two of the page to say.\n\n"));
    }
    source
}

#[test]
fn a_document_taller_than_its_pane_has_a_scrollbar_that_can_be_dragged() {
    let mut harness = harness(&a_long_markdown());
    assert_eq!(harness.state().files.active().scroll, 0.0, "it starts at the top");
    let track = harness.get_by_label("Scroll untitled").rect();
    // From the thumb at the top of the track to a good way down it.
    let from = egui::pos2(track.center().x, track.top() + 10.0);
    drag(&mut harness, from, egui::pos2(track.center().x, track.center().y));
    let scrolled = harness.state().files.active().scroll;
    assert!(scrolled > 100.0, "dragging the thumb should have scrolled the file, it is at {scrolled}");
    // And it stops where the page stops rather than running off the end.
    drag(&mut harness, egui::pos2(track.center().x, track.center().y), egui::pos2(track.center().x, track.bottom() + 400.0));
    let bottom = harness.state().files.active().scroll;
    let overflow = harness.state().layout().height - harness.state().editor_area().height()
        + quill_app::theme::size::EDITOR_PADDING_Y * 2.0;
    assert!(bottom <= overflow + 1.0, "{bottom} is past the {overflow} there is to scroll");
    harness.snapshot(shot("scrollbar_dragged"));
}

#[test]
fn a_document_that_fits_its_pane_has_no_scrollbar() {
    let mut harness = harness("one line");
    harness.run();
    assert!(
        harness.query_by_label("Scroll untitled").is_none(),
        "nothing to scroll, so there is nothing to draw"
    );
}

#[test]
fn scrolling_the_source_scrolls_the_preview_with_it() {
    let mut harness = harness(&a_long_markdown());
    harness.get_by_label("Side by side").click();
    harness.run();
    assert_eq!(harness.state().files.active().preview_scroll, 0.0);
    // The wheel over the source, which is what a person does.
    let over_the_source = harness.state().editor_area().center();
    harness.input_mut().events.push(egui::Event::PointerMoved(over_the_source));
    harness.run();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -600.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::default(),
    });
    harness.run();
    harness.run();
    let source = harness.state().files.active().scroll;
    let preview = harness.state().files.active().preview_scroll;
    assert!(source > 100.0, "the source should have scrolled, it is at {source}");
    assert!(preview > 100.0, "and the preview should have followed it, it is at {preview}");
    // And they are showing the same part of the file, rather than the same number of points down two
    // pages of different heights.
    let paragraph = harness.state().layout().paragraph_at_y(source).0;
    let map = harness.state().preview_source_lines();
    let line = map.get(harness.state().preview_layout().paragraph_at_y(preview).0).copied().unwrap_or(0);
    assert!(
        line.abs_diff(paragraph) <= 1,
        "the source is at line {paragraph} and the preview is showing line {line}"
    );
    harness.snapshot(shot("side_by_side_scrolled_together"));
}

#[test]
fn scrolling_the_preview_scrolls_the_source_with_it() {
    let mut harness = harness(&a_long_markdown());
    harness.get_by_label("Side by side").click();
    harness.run();
    // The wheel over the preview, which is the right hand half of the editing area.
    let source_area = harness.state().editor_area();
    let over_the_preview = egui::pos2(source_area.right() + source_area.width() / 2.0, source_area.center().y);
    harness.input_mut().events.push(egui::Event::PointerMoved(over_the_preview));
    harness.run();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -600.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::default(),
    });
    harness.run();
    harness.run();
    assert!(harness.state().files.active().preview_scroll > 100.0);
    assert!(
        harness.state().files.active().scroll > 100.0,
        "the source should have followed the preview, it is at {}",
        harness.state().files.active().scroll
    );
}

/// **The two halves settle rather than chasing each other.** The crossing snaps to a paragraph, so a
/// rule that moved both halves every frame would creep down the file on its own for as long as the
/// window was left open. Nothing is touched, and both stay where they were.
#[test]
fn the_two_halves_do_not_chase_each_other_when_nothing_is_touched() {
    let mut harness = harness(&a_long_markdown());
    harness.get_by_label("Side by side").click();
    harness.run();
    harness.state_mut().files.active_mut().scroll = 900.0;
    harness.run();
    harness.run();
    let settled = (
        harness.state().files.active().scroll,
        harness.state().files.active().preview_scroll,
    );
    for _ in 0..30 {
        harness.run();
    }
    let after = (
        harness.state().files.active().scroll,
        harness.state().files.active().preview_scroll,
    );
    assert!(
        (settled.0 - after.0).abs() < 0.5 && (settled.1 - after.1).abs() < 0.5,
        "left alone for thirty frames the view moved from {settled:?} to {after:?}"
    );
}

#[test]
fn a_tab_can_be_dragged_along_the_strip_to_rearrange_it() {
    let mut harness = harness("");
    let folder = sample_folder();
    for name in ["notes.txt", "readme.md", "program.rs"] {
        harness.state_mut().open_path_permanently(&folder.join(name));
        harness.run();
    }
    let names = |harness: &Harness<'static, QuillApp>| -> Vec<String> {
        harness.state().files.iter().map(|file| file.name()).collect()
    };
    assert_eq!(names(&harness), vec!["notes.txt", "readme.md", "program.rs"]);
    let first = harness.get_by_label("Tab: notes.txt").rect();
    let last = harness.get_by_label("Tab: program.rs").rect();
    drag(&mut harness, first.center(), egui::pos2(last.right() - 4.0, last.center().y));
    assert_eq!(
        names(&harness),
        vec!["readme.md", "program.rs", "notes.txt"],
        "the tab should have been carried to the end of the strip"
    );
    assert_eq!(harness.state().files.active().name(), "notes.txt", "and be showing where it landed");
    harness.snapshot(shot("tab_dragged_along_the_strip"));
}

#[test]
fn a_tab_can_be_dragged_from_one_pane_into_the_other() {
    let mut harness = harness("");
    let folder = sample_folder();
    for name in ["notes.txt", "readme.md"] {
        harness.state_mut().open_path_permanently(&folder.join(name));
        harness.run();
    }
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::SplitRight, &ctx);
    harness.run();
    assert_eq!(harness.state().files.pane_count(), 2);
    assert_eq!(harness.state().files.pane_of(0), 0, "notes.txt stays on the left");
    assert_eq!(harness.state().files.pane_of(1), 1, "readme.md went into the new pane");
    // Carry notes.txt out of the pane on the left and into the pane on the right.
    let tab = harness.get_by_label("Tab: notes.txt").rect();
    let target = harness.get_by_label("Tab: readme.md").rect();
    drag(&mut harness, tab.center(), egui::pos2(target.right() - 4.0, target.center().y));
    assert_eq!(
        harness.state().files.pane_count(),
        1,
        "the pane it was carried out of held nothing else, so it went with it"
    );
    let names: Vec<String> = harness.state().files.iter().map(|file| file.name()).collect();
    assert_eq!(names, vec!["readme.md", "notes.txt"]);
}

#[test]
fn right_clicking_the_project_name_opens_the_same_menu_a_folder_does() {
    let mut harness = harness("");
    let heading = harness.get_by_label(&sample_folder().file_name().unwrap().to_string_lossy()).rect();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: heading.center(),
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::default(),
    });
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: heading.center(),
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::default(),
    });
    harness.run();
    let opened = harness.state().explorer_menu.clone();
    let (_, path, directory, _) = opened.expect("the project's name should open the explorer's menu");
    assert_eq!(path, sample_folder(), "the menu is about the project folder");
    assert!(directory, "and it is a folder, so `New -> File` makes a file inside it");
    harness.run();
    harness.snapshot(shot("project_name_menu"));
}


// ------------------------------------------------------------------- task-1693

/// The gutter's marks line up with the letters at any size, which is what `task-1693` reported was
/// wrong: a mark centred in the line box sits low, by more the larger the type, because all of a
/// line's extra leading is added below its glyphs.
#[test]
fn the_gutter_lines_up_with_the_text_at_a_large_font_size() {
    let mut harness = harness("one\ntwo\nthree\nfour\nfive\n");
    collapse(&mut harness);
    did(&mut harness, "settings set appearance.font.size 34");
    harness.run();
    assert!(harness.state().settings.line_numbers);
    // The letters of a line fill much less than the line, which is where the drift came from. Asked
    // of the layout rather than of the picture, so it is a number a reader can check.
    let layout = harness.state().layout().clone();
    let line = &layout.lines[2];
    let band = line.ascent + line.descent;
    assert!(
        band < line.height - 4.0,
        "at 34 points a line is {} tall and its letters only {band}, which is the drift",
        line.height
    );
    harness.snapshot(shot("gutter_large_font"));
}

/// A right click in the empty space below the rows opens the project folder's menu, with everything
/// that is about a particular file dimmed rather than taken away — `task-1693`, in its own words.
#[test]
fn the_explorers_menu_opens_from_the_empty_space_below_the_rows() {
    let mut harness = harness("");
    // Well below the last row and well above the footer, which is where a person aims when they mean
    // "somewhere in this panel".
    let at = egui::pos2(150.0, 470.0);
    right_click_at(&mut harness, at);
    let opened = harness.state().explorer_menu.clone();
    let (_, path, directory, aimed) =
        opened.expect("the empty space should open the explorer's menu");
    assert_eq!(path, sample_folder(), "it is the project folder");
    assert!(directory);
    assert_eq!(aimed, quill_app::app::actions::Aim::AtEmptySpace);
    harness.run();
    // The entry somebody who right clicked the empty space came for. `File` is asked for by the
    // menu's own row rather than by name, because `File` is also a menu in the bar.
    harness.get_by_label("Folder");
    // And the ones that are about a particular file are dimmed rather than absent.
    let entries = quill_app::app::actions::explorer_menu(
        sample_folder().as_path(),
        true,
        false,
        quill_app::app::actions::Aim::AtEmptySpace,
    );
    let live = |name: &str| {
        entries.iter().any(|entry| {
            matches!(
                entry,
                quill_app::app::actions::Entry::Item { name: found, enabled, .. }
                    if found == name && *enabled
            )
        })
    };
    assert!(!live("Rename..."), "Rename is dimmed, because nothing was clicked");
    assert!(!live("Delete"), "and so is Delete");
    assert!(live("Reload from Disk"), "what is about the folder stays live");
    harness.get_by_label("Rename...");
    harness.snapshot(shot("explorer_empty_space_menu"));
}

/// The file that is showing and the row the explorer's cursor is on are two marks, not one. They
/// were drawn identically until `task-1693`, so a right click on a second file left two rows looking
/// equally open — and the second one stayed that way after the first tab was closed.
#[test]
fn the_open_file_and_the_explorers_cursor_are_drawn_differently() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    did(&mut harness, "tab open readme.md");
    did(&mut harness, "explorer select notes.txt");
    harness.run();
    assert_eq!(
        harness.state().files.active().path(),
        Some(folder.join("readme.md").as_path()),
        "readme is the file that is showing"
    );
    assert_eq!(harness.state().selected, Some(folder.join("notes.txt")));
    harness.snapshot(shot("explorer_open_and_cursor"));
}

/// A maximised window offers no resize grips. `components::resize_edges` records why that matters
/// more than tidiness: a resize the window manager refuses latches a flag inside winit that no later
/// move or resize can clear.
#[test]
fn a_maximised_window_offers_no_resize_grips() {
    let mut harness = harness("");
    for grip in ["top", "bottom", "left", "right"] {
        harness.get_by_label(&format!("Resize window: {grip}"));
    }
    let ids: Vec<egui::ViewportId> =
        harness.input().viewports.keys().copied().collect();
    for id in ids {
        if let Some(viewport) = harness.input_mut().viewports.get_mut(&id) {
            viewport.maximized = Some(true);
        }
    }
    harness.run();
    for grip in ["top", "bottom", "left", "right", "top left", "bottom right"] {
        assert!(
            harness.query_by_label(&format!("Resize window: {grip}")).is_none(),
            "a maximised window has no {grip} grip to offer"
        );
    }
}

/// `New -> Folder`, which the explorer had no way to do at all.
#[test]
fn the_explorer_can_make_a_folder() {
    let folder = std::env::temp_dir().join("quill-new-folder-test");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    std::fs::write(folder.join("readme.md"), "# here\n").expect("write a file");
    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::NewFolder(folder.clone()), &ctx);
    harness.run();
    let title = harness.state().prompt.clone().expect("a prompt asking for the name").title;
    assert_eq!(title, "New Folder");
    let mut prompt = harness.state().prompt.clone().expect("the prompt");
    prompt.value = "services".to_owned();
    harness.state_mut().run_prompt_for_test(prompt);
    harness.run();
    assert!(folder.join("services").is_dir(), "the folder was made");
    assert_eq!(harness.state().selected, Some(folder.join("services")));

    // And from the command line, which is the other half of every feature in Quill.
    did(&mut harness, "explorer new-folder deep/inside");
    assert!(folder.join("deep/inside").is_dir(), "the folders above it are made too");
    did(&mut harness, "explorer new-file deep/inside/note.md");
    assert!(folder.join("deep/inside/note.md").is_file());
}

/// A file another program makes appears in the explorer without anybody asking, which is what
/// `task-1693` reported was missing: an agent's new file was invisible until the tree was reloaded.
#[test]
fn a_file_made_by_another_program_appears_in_the_explorer() {
    let folder = std::env::temp_dir().join("quill-watch-test");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    std::fs::write(folder.join("readme.md"), "# here\n").expect("write a file");
    let mut harness = harness_in(&folder);
    assert!(harness.query_by_label("made-by-an-agent.md").is_none());

    // A second's wait, because a folder's modification time has whole-second resolution on some file
    // systems and the tree has only just read it.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(folder.join("made-by-an-agent.md"), "# new\n").expect("write the new file");
    // The window asks on a timer, so it takes a few frames rather than one.
    for _ in 0..8 {
        harness.step();
        if harness.query_by_label("made-by-an-agent.md").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    harness.get_by_label("made-by-an-agent.md");
}

/// The plus beside the project's name is gone. It never made a file — it asked the window to save —
/// and `task-1673` asks for it to go.
#[test]
fn there_is_no_new_file_button_beside_the_project_name() {
    let harness = harness("");
    assert!(harness.query_by_label("New file").is_none());
    assert!(harness.query_by_label("Hide the explorer").is_some(), "the other button stays");
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
fn a_terminal_tab_is_renamed_from_its_own_menu() {
    let mut harness = with_terminal("", 8, 60);
    harness.state_mut().new_detached_terminal_tab(8, 60);
    harness.run();
    assert_eq!(harness.state().terminal.tabs.names(), vec!["detached", "detached 2"]);

    // Opened through the window's own state, as the gutter's and a file tab's menus are, because
    // the harness cannot press the right mouse button. To the right of the strip, so the picture
    // holds the tabs the menu is about as well as the menu.
    let at = harness.state().terminal.grid_area().left_top() + vec2(420.0, -14.0);
    harness.state_mut().terminal_menu = Some((at, 1));
    harness.run();
    harness.get_by_label("Rename...");
    harness.get_by_label("New Terminal Tab");
    harness.snapshot(shot("terminal_tab_menu"));

    // Choosing it puts the menu away and opens the prompt, seeded with what the tab is called now.
    harness.state_mut().terminal_menu = None;
    choose(&mut harness, Action::RenameTerminalTab);
    assert_eq!(
        harness.state().prompt.as_ref().expect("the prompt is open").value,
        "detached 2",
    );
    if let Some(prompt) = harness.state_mut().prompt.as_mut() {
        prompt.value = "the build".to_owned();
    }
    let prompt = harness.state_mut().prompt.take().expect("a prompt");
    harness.state_mut().run_prompt_for_test(prompt);
    harness.run();
    assert_eq!(harness.state().terminal.tabs.names(), vec!["detached", "the build"]);

    // And the name a person typed is not taken away again by the program setting a title of its
    // own, which is the whole reason it is held apart from the title.
    feed(&mut harness, b"\x1b]0;claude\x07");
    assert_eq!(harness.state().terminal.tabs.names(), vec!["detached", "the build"]);
    harness.snapshot(shot("terminal_tab_renamed"));
}

#[test]
fn a_terminal_tab_is_dragged_along_the_strip() {
    let mut harness = with_terminal("", 8, 60);
    harness.state_mut().new_detached_terminal_tab(8, 60);
    harness.run();
    did(&mut harness, "terminal rename --tab 0 first");
    did(&mut harness, "terminal rename --tab 1 second");
    harness.run();
    assert_eq!(harness.state().terminal.tabs.names(), vec!["first", "second"]);

    // The first tab dragged past the middle of the second, which is where a drop lands after it.
    let from = harness.get_by_label("Terminal tab: first").rect().center();
    let onto = harness.get_by_label("Terminal tab: second").rect();
    let to = egui::pos2(onto.right() - 2.0, onto.center().y);
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
    // Held, so the picture shows the tab outlined in the air and the accent mark saying where it
    // would land. It is the same mark the file tabs draw, from the same two functions.
    harness.snapshot(shot("terminal_tab_dragging"));
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    harness.run();
    assert_eq!(
        harness.state().terminal.tabs.names(),
        vec!["second", "first"],
        "the tab that was dragged is now the second one"
    );
    assert_eq!(harness.state().terminal.tabs.active_index(), 1, "and it is the one showing");
    harness.snapshot(shot("terminal_tabs_rearranged"));
}

#[test]
fn dragging_a_terminal_tab_and_the_command_line_are_the_same_rearrangement() {
    let mut harness = with_terminal("", 8, 60);
    for name in ["one", "two", "three"] {
        harness.state_mut().new_detached_terminal_tab(8, 60);
        harness.run();
        let last = harness.state().terminal.tabs.count() - 1;
        did(&mut harness, &format!("terminal rename --tab {last} {name}"));
    }
    // `terminal move` counts the tabs as they are on the screen, exactly as `tab move` does, so
    // moving the last one to the front is position 0.
    did(&mut harness, "terminal move --tab 3 0");
    assert_eq!(harness.state().terminal.tabs.names(), vec!["three", "detached", "one", "two"]);
    assert_eq!(harness.state().terminal.tabs.active_index(), 0);

    // An empty name puts a tab back to being named after the program in it, which is the one thing
    // the dialog cannot ask for, because its button needs a name in the field.
    did(&mut harness, "terminal rename --tab 0");
    assert_eq!(harness.state().terminal.tabs.names()[0], "detached");
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
    pump(&mut harness);
    assert_eq!(harness.state().focus, quill_app::app::Focus::Terminal);

    // `pump` rather than `run` from here on. A real shell is starting behind this and writes its
    // prompt whenever it is ready, and every write wakes the window, so `run`'s budget of four steps
    // to go quiet is the wrong budget — it is the rule the file's other waiting loops already follow.
    // Seen failing on a loaded machine as `Harness::run exceeded max_steps (4)` with the terminal's
    // own waker as the repaint cause.
    for text in ["echo quill-typing-works"] {
        harness.input_mut().events.push(egui::Event::Text(text.to_owned()));
        pump(&mut harness);
    }
    harness.key_press(egui::Key::Enter);
    pump(&mut harness);

    // Thirty seconds, because this waits for a real shell on whatever machine the tests are run on, and a
    // machine busy with a build can take much longer than an idle one.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        pump(&mut harness);
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

/// Click at a point in the window, which is how the editing area is given the keyboard back.
fn click_at(harness: &mut Harness<'static, QuillApp>, at: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::default(),
        });
    }
    harness.run();
}

// The next group is `task-1656`. The editing area used to read the frame's key and text events
// without asking whether another widget had the keyboard, and egui leaves the events a `TextEdit`
// consumed in that list, so typing `note` into the explorer's filter box put `note` at the caret in
// the open file as well and marked it as having unsaved changes. Each test drives a different box,
// because the fault was in the editing area rather than in any one of them.

#[test]
fn typing_in_the_explorers_filter_box_leaves_the_document_alone() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    let before = harness.state().document().text().to_string();
    assert!(!harness.state().document().is_modified(), "just opened, so nothing to save");

    harness.get_by_label("Filter files").click();
    harness.run();
    harness.get_by_label("Filter files").type_text("note");
    harness.run();
    // A key press as well as text, because the two arrive as different events and the editing area
    // used to act on both: backspace deleted a character of the file rather than of the filter.
    harness.key_press(egui::Key::Backspace);
    harness.run();
    harness.run();

    assert_eq!(harness.state().filter, "not", "what was typed should have reached the filter box");
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "and nothing should have reached the file behind it"
    );
    assert!(
        !harness.state().document().is_modified(),
        "so the file should not be marked as having unsaved changes"
    );
}

#[test]
fn clicking_back_into_the_document_takes_the_keyboard_back() {
    // The other half of the guard: it has to let go. Without this a filter that had been typed into
    // once would leave the document unable to be typed into at all.
    let mut harness = harness("");
    harness.get_by_label("Filter files").click();
    harness.run();
    harness.get_by_label("Filter files").type_text("two");
    harness.run();

    let middle = harness.state().editor_area().center();
    click_at(&mut harness, middle);
    harness.input_mut().events.push(egui::Event::Text("typed".to_owned()));
    harness.run();

    assert_eq!(harness.state().filter, "two", "the filter keeps what was typed into it");
    assert_eq!(
        harness.state().document().text().to_string(),
        "typed",
        "and the document takes typing again once it has been clicked into"
    );
}

#[test]
fn undo_in_a_text_box_does_not_undo_the_document() {
    // Control and Z used to clear the filter box and undo an edit in the file behind it with the one
    // press, because the menu's keyboard watcher reads the same events the box had just taken.
    let mut harness = harness("original");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    harness.input_mut().events.push(egui::Event::Text(" plus more".to_owned()));
    harness.run();
    let typed = harness.state().document().text().to_string();
    assert_ne!(typed, "original", "the document should have been typed into first");

    // Nothing is typed into the filter box first, deliberately. If it were, the undo would be
    // undoing the box's own insert and the assertion would hold whether or not the watcher had been
    // fixed. Focusing the box and pressing the shortcut is what tells the two apart.
    harness.get_by_label("Filter files").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    harness.run();

    assert_eq!(
        harness.state().document().text().to_string(),
        typed,
        "undo belongs to the box that has the keyboard, not to the document"
    );
}

#[test]
fn select_all_in_a_text_box_does_not_select_the_document() {
    let mut harness = harness("some writing to look at");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    harness.get_by_label("Filter files").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    harness.run();

    assert!(
        harness.state().document().selection().is_empty(),
        "select all should have selected the filter box, leaving the document alone"
    );
}

#[test]
fn the_rest_of_the_menu_still_works_while_a_text_box_has_the_keyboard() {
    // Only undo, redo and select all belong to the box. Everything else on every menu keeps working,
    // which is what stops the guard from being too broad: control and S in a search box saves the
    // file in every other editor and has to save it here.
    let folder = std::env::temp_dir().join("quill-save-while-filtering");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the folder");
    let text = "saved while the filter box had the keyboard";
    let owned = folder.clone();
    let mut harness = builder()
        .with_size(vec2(WINDOW[0], WINDOW[1]))
        .build_eframe(move |cc| {
            let mut app = QuillApp::with_text(owned, text);
            app.prepare(&cc.egui_ctx);
            app
        });
    harness.run();
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();

    harness.get_by_label("Filter files").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::S);
    harness.run();

    let written = folder.join("untitled.md");
    assert!(written.is_file(), "Save should still have written {}", written.display());
    assert_eq!(std::fs::read_to_string(&written).expect("read it back"), text);
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn typing_a_new_name_in_the_rename_prompt_leaves_the_document_alone() {
    let folder = std::env::temp_dir().join("quill-rename-prompt-keyboard");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the folder");
    std::fs::write(folder.join("before.md"), "# before\n").expect("write it");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("before.md"));
    harness.run();
    let before = harness.state().document().text().to_string();

    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::RenamePath(folder.join("before.md")), &ctx);
    harness.run();
    // The prompt asks for the keyboard as it opens, so this types without clicking, which is what a
    // person does.
    harness.get_by_label("Name").type_text("after");
    harness.run();

    assert!(
        harness.state().prompt.as_ref().is_some_and(|prompt| prompt.value.ends_with("after")),
        "what was typed should have reached the prompt: {:?}",
        harness.state().prompt.as_ref().map(|prompt| prompt.value.clone())
    );
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "and nothing should have reached the file being renamed"
    );
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn typing_in_the_plugin_search_leaves_the_document_alone() {
    let mut harness = harness("");
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    let before = harness.state().document().text().to_string();

    harness.state_mut().settings_window.open();
    harness.state_mut().settings_window.page = quill_app::settings::Page::Plugins;
    harness.run();
    harness.run();
    harness.get_by_label("Search plugins").click();
    harness.run();
    harness.get_by_label("Search plugins").type_text("rust");
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "searching the plugins should not type into the file behind the settings window"
    );
    assert!(!harness.state().document().is_modified());
}

#[test]
fn enter_in_the_commit_message_is_a_new_line_and_the_command_key_commits() {
    // The one modal whose body owns Enter. Every other one is confirmed by it; here it has to stay
    // a new line, which is what `modal::Confirm::CommandEnter` is for.
    let mut harness = git_harness("commit-enter");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Git(quill_app::app::actions::GitAction::Commit), &ctx);
    harness.run();
    settle(&mut harness, "the history the panel asks for", |app| {
        app.git.as_ref().is_some_and(|git| git.message.is_none() && !git.history.is_empty())
    });

    harness.get_by_label("Commit message").click();
    harness.run();
    harness.get_by_label("Commit message").type_text("the first line");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.get_by_label("Commit message").type_text("the second");
    harness.run();
    harness.run();
    assert_eq!(
        harness.state().git.as_ref().map(|git| git.panel.message.clone()),
        Some("the first line\nthe second".to_owned()),
        "Enter in the message is a new line"
    );
    assert!(
        harness.state().git.as_ref().is_some_and(|git| git.panel.open),
        "and the panel is still open"
    );

    // The command key with it is what presses `COMMIT`, which is IntelliJ's own chord for the same
    // dialog. Something has to be staged first, or the button is dimmed and there is nothing to
    // press.
    let root = harness.state().tree.root().to_path_buf();
    let ctx = harness.ctx.clone();
    harness.state_mut().open_path_permanently(&root.join("version.ts"));
    nudge(&mut harness);
    harness
        .state_mut()
        .run_action(Action::Git(quill_app::app::actions::GitAction::Add(None)), &ctx);
    settle(&mut harness, "the file to be staged", |app| {
        app.git.as_ref().is_some_and(|git| {
            git.snapshot.status.entry("version.ts").is_some_and(quill_git::status::Entry::staged)
        })
    });
    // The panel is still open from above — `Git -> Commit` is a toggle, so asking for it again
    // would put it away.
    if let Some(state) = harness.state_mut().git.as_mut() {
        state.panel.message = "task-1682: committed from the keyboard".to_owned();
    }
    nudge(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Enter);
    settle(&mut harness, "the commit", |app| {
        app.git.as_ref().is_some_and(|git| git.snapshot.status.entry("version.ts").is_none())
    });
    assert_eq!(
        ask_git(&root, &["log", "--format=%s", "-n1"]),
        "task-1682: committed from the keyboard",
    );
}

#[test]
fn typing_a_commit_message_leaves_the_document_alone() {
    // The worst of the boxes, because a commit message is a paragraph rather than a word: every
    // character of it used to be inserted into the file that was open behind the panel.
    let mut harness = git_harness("keyboard");
    let ctx = harness.ctx.clone();
    let before = harness.state().document().text().to_string();
    harness.state_mut().run_action(Action::Git(quill_app::app::actions::GitAction::Commit), &ctx);
    harness.run();
    settle(&mut harness, "the history the panel asks for", |app| {
        app.git.as_ref().is_some_and(|git| git.message.is_none() && !git.history.is_empty())
    });

    harness.get_by_label("Commit message").click();
    harness.run();
    harness.get_by_label("Commit message").type_text("a message, not an edit");
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().git.as_ref().map(|git| git.panel.message.clone()),
        Some("a message, not an edit".to_owned()),
        "what was typed should have reached the commit message"
    );
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "and nothing should have reached the file behind the panel"
    );
    assert!(!harness.state().document().is_modified());
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
    harness.state_mut().settings.terminal_shell = "/no/such/program/at/all".to_owned();
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
        Some((
            egui::pos2(120.0, 260.0),
            folder.join("readme.md"),
            false,
            quill_app::app::actions::Aim::AtARow,
        ));
    harness.run();
    // Asked for by names this menu alone has: `File` is also a menu in the bar, and `New` is a
    // heading rather than a control, because a submenu inside the window is drawn as a heading with
    // its entries indented under it.
    for entry in ["Copy Path", "Rename...", "Reload from Disk"] {
        harness.get_by_label(entry);
    }
    let entries = quill_app::app::actions::explorer_menu(
        &folder.join("readme.md"),
        false,
        false,
        quill_app::app::actions::Aim::AtARow,
    );
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
fn the_view_menu_holds_the_font_size_and_ticks_the_mode_that_is_showing() {
    // The one menu in Quill with a checked row in it, so it is the one that shows the tick. It used
    // to be drawn as the character at U+2713, which no font in the stack Quill hands egui has a
    // shape for, so it came out as the empty box a missing glyph renders as — the fault the style
    // guide already records for the shift symbol, found again while retaking the documentation
    // captures for `task-1657`. **Look at the image**: there should be a tick beside `Raw Markdown`.
    let mut harness = harness("");
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    harness.run();
    harness.get_by_label("View").click();
    harness.run();
    for entry in ["Increase Font Size", "Decrease Font Size", "Reset Font Size"] {
        harness.get_by_label(entry);
    }
    // `Raw Markdown` is both a button on the strip and a row on this menu, so it is asked for by
    // count rather than by name.
    assert_eq!(harness.get_all_by_label("Raw Markdown").count(), 2);
    harness.snapshot(shot("view_menu"));
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
        pump(&mut harness);
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
fn a_css_file_is_coloured_by_its_plugin() {
    // `task-1671`. Not a repository, for the reason the TypeScript one above gives: a window in one
    // has a git message in its status bar for the first few frames.
    let folder = std::env::temp_dir().join("quill-screenshot-css");
    std::fs::create_dir_all(&folder).expect("make the folder");
    let path = folder.join("site.css");
    std::fs::write(
        &path,
        "/* the card */\n@media screen and (min-width: 40rem) {\n  .card:hover {\n    background-color: #ff79c6;\n    display: flex;\n    font-family: \"Iosevka\", monospace;\n    width: calc(100% - 2rem);\n    --brand-hue: 280;\n  }\n}\n",
    )
    .expect("write site.css");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&path);
    harness.run();
    harness.run();
    // Read out of the document's own spans, so the five things the plugin had to be taught are
    // checked as colours rather than only looked at.
    let text = harness.state().document().text().to_string();
    let chars = harness.state().document().chars();
    let inside = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("no {needle}")) + 1;
    assert_eq!(
        chars.style_at(inside("@media")).color,
        Color::rgb(0xFF, 0x79, 0xC6),
        "an at-rule is a keyword, in Dracula's pink, and the at sign is part of the word"
    );
    assert_eq!(
        chars.style_at(inside("background-color")).color,
        Color::rgb(0xBD, 0x93, 0xF9),
        "a property is a builtin, in Dracula's purple, hyphen and all"
    );
    assert_eq!(
        chars.style_at(inside("flex;")).color,
        Color::rgb(0x8B, 0xE9, 0xFD),
        "a value keyword is a type, in Dracula's cyan"
    );
    assert_eq!(
        chars.style_at(inside("#ff79c6")).color,
        Color::rgb(0xFF, 0xB8, 0x6C),
        "a hex colour is a number, in Dracula's orange"
    );
    assert_eq!(
        chars.style_at(inside("calc(")).color,
        Color::rgb(0x50, 0xFA, 0x7B),
        "a word before a bracket is a function, in Dracula's green"
    );
    assert_eq!(
        chars.style_at(inside("/* the card */")).color,
        Color::rgb(0x62, 0x72, 0xA4),
        "the comment, in Dracula's blue-grey"
    );
    assert!(!harness.state().document().is_modified(), "colouring must not mark the file changed");
    harness.snapshot(shot("syntax_css"));
}

#[test]
fn the_plugins_page_lists_the_ones_that_ship_with_quill() {
    let mut harness = harness("");
    harness.state_mut().settings_window.open();
    harness.state_mut().settings_window.page = quill_app::settings::Page::Plugins;
    harness.run();
    harness.run();
    for name in ["CSS", "JavaScript", "TypeScript", "Rust"] {
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
/// A few frames, without insisting that the window goes quiet.
///
/// `Harness::run` gives the window four steps to settle and panics otherwise, which is right for a
/// settled window and wrong while git is still working: the worker thread asks for a repaint whenever a
/// command finishes, so a `run` that happens to land in the middle of one fails for a reason that is not
/// a fault in Quill. This is what a step between git operations uses instead.
fn nudge(harness: &mut Harness<'static, QuillApp>) {
    for _ in 0..4 {
        pump(harness);
    }
}

fn settle(harness: &mut Harness<'static, QuillApp>, what: &str, ready: impl Fn(&QuillApp) -> bool) {
    for _ in 0..600 {
        pump(harness);
        if ready(harness.state()) {
            pump(harness);
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
    nudge(&mut harness);
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
    nudge(&mut harness);
    if let Some(state) = harness.state_mut().git.as_mut() {
        state.panel.message = "task-1649: driven from the window".to_owned();
    }
    nudge(&mut harness);
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
    nudge(&mut harness);
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
    nudge(&mut harness);
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
    nudge(&mut harness);
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
    nudge(&mut harness);
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
    nudge(&mut harness);
    if let Some(state) = harness.state_mut().git.as_mut() {
        state.dialogs.target = "from-quill".to_owned();
    }
    nudge(&mut harness);
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

// ---------------------------------------------------------------------------------------------
// task-1658: the window's own resize grips, the rail of pane buttons, the project's state and
// pictures in a tab.
// ---------------------------------------------------------------------------------------------

/// The window is created with no operating system frame, so it has no resize grip of its own and one
/// is drawn at each edge and each corner. Before `task-1658` the only one that worked was the top,
/// and the window could not be made wider or shorter at all.
#[test]
fn the_window_can_be_resized_from_every_edge_and_every_corner() {
    let harness = harness("");
    for grip in [
        "top", "bottom", "left", "right", "top left", "top right", "bottom left", "bottom right",
    ] {
        harness.get_by_label(&format!("Resize window: {grip}"));
    }
}

/// The rail down the far left is the one place a pane is put away and brought back from.
#[test]
fn the_rail_puts_each_pane_away_and_brings_it_back() {
    let mut harness = harness("");
    assert!(harness.state().explorer_visible);
    harness.get_by_label("Project").click();
    harness.run();
    assert!(!harness.state().explorer_visible, "the rail's Project button hides the explorer");
    harness.get_by_label("Project").click();
    harness.run();
    assert!(harness.state().explorer_visible, "and brings it back");

    assert!(!harness.state().terminal.visible);
    // A detached terminal, so nothing here depends on a shell starting.
    harness.state_mut().new_detached_terminal_tab(8, 60);
    harness.run();
    assert!(harness.state().terminal.visible);
    harness.get_by_label("Terminal tile").click();
    harness.run();
    assert!(!harness.state().terminal.visible, "the rail's terminal button puts the tile away");
    harness.snapshot(shot("activity_bar"));
}

/// The commit panel is the rail's third button, and it is the same action the Git menu's `Commit...`
/// entry is, so pressing it twice puts the panel away again.
#[test]
fn the_rails_version_control_button_opens_the_commit_panel_and_shuts_it() {
    let root = git_folder("quill-rail-git");
    let mut harness = harness_in(&root);
    assert!(harness.state().git.is_some(), "the folder should be a repository");
    harness.get_by_label("Version Control").click();
    harness.run();
    assert!(
        harness.state().git.as_ref().is_some_and(|git| git.panel.open),
        "the panel should be open"
    );
    harness.get_by_label("Version Control").click();
    harness.run();
    assert!(
        harness.state().git.as_ref().is_some_and(|git| !git.panel.open),
        "and pressing it again should shut it"
    );
}

/// What was open in a project is written into a `.quill` folder beside it and read back next time.
#[test]
fn what_was_open_in_a_project_comes_back_when_it_is_opened_again() {
    let root = copy_out_of_the_repository(&sample_folder(), "quill-project-state-window");
    {
        let mut harness = harness_in(&root);
        harness.state_mut().restore_project();
        harness.state_mut().open_path_permanently(&root.join("readme.md"));
        harness.state_mut().open_path_permanently(&root.join("notes.txt"));
        harness.state_mut().tree.expand(&root.join("chapters"));
        harness.run();
        // Written when the window closes, as it is when a person shuts Quill.
        let ctx = harness.ctx.clone();
        harness.state_mut().run_action(Action::CloseWindow, &ctx);
    }
    assert!(
        root.join(".quill/open-files.txt").is_file(),
        "the project's state should be beside the project"
    );

    let mut second = harness_in(&root);
    second.state_mut().restore_project();
    second.run();
    let names: Vec<String> =
        second.state().files.iter().map(quill_app::app::files::OpenFile::name).collect();
    assert!(names.contains(&"readme.md".to_owned()), "the tabs came back, they are {names:?}");
    assert!(names.contains(&"notes.txt".to_owned()), "both of them, they are {names:?}");
    assert!(
        second.state().tree.expanded_folders().iter().any(|path| path.ends_with("chapters")),
        "and the folder that was opened out is opened out again"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// A picture opens in a tab that shows it, fitted to the editing area to begin with.
#[test]
fn a_picture_opens_in_a_tab_that_shows_it() {
    let mut harness = harness("");
    harness.get_by_label_contains("picture.png").click();
    harness.run();
    assert!(harness.state().files.active().is_picture(), "the tab should be holding a picture");
    let picture = harness.state().files.active().picture.as_ref().expect("a picture");
    assert_eq!(picture.problem, None, "it should have decoded");
    assert_eq!(picture.size, [160, 100]);
    assert!(
        harness.query_by_label("Text options").is_none(),
        "a picture has no text to format"
    );
    harness.snapshot(shot("picture"));
}

/// Control and plus zooms the picture rather than the editor's font, and `Reset Font Size` fits it
/// back into the area.
#[test]
fn the_keyboard_zooms_a_picture_and_leaves_the_editors_font_alone() {
    let mut harness = harness("");
    harness.get_by_label_contains("picture.png").click();
    harness.run();
    let font_before = harness.state().settings.font_size;
    let area = harness.state().editor_area().size();
    let scale = |harness: &Harness<'static, QuillApp>| {
        harness.state().files.active().picture.as_ref().expect("a picture").scale_in(area)
    };
    let fitted = scale(&harness);

    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Equals);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::Equals);
    harness.run();
    let zoomed = scale(&harness);
    assert!(zoomed > fitted, "two presses should have made it bigger: {fitted} then {zoomed}");
    assert_eq!(
        harness.state().settings.font_size,
        font_before,
        "and the editor's own font should not have moved"
    );
    harness.snapshot(shot("picture_zoomed"));

    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::ResetFontSize, &ctx);
    harness.run();
    assert!(
        (scale(&harness) - fitted).abs() < 0.001,
        "resetting should fit it back into the area"
    );
}

/// A picture cannot be edited, so saving one must not write an empty file over it.
#[test]
fn saving_a_tab_that_holds_a_picture_does_not_write_over_the_picture() {
    let folder = copy_out_of_the_repository(&sample_folder(), "quill-picture-save");
    let path = folder.join("picture.png");
    let before = std::fs::read(&path).expect("read the picture");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&path);
    harness.run();
    assert!(harness.state().files.active().is_picture());
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Save, &ctx);
    harness.run();
    let after = std::fs::read(&path).expect("read it again");
    assert_eq!(before, after, "the picture on disk must be untouched");
    std::fs::remove_dir_all(&folder).ok();
}

// `Go to File`, `Find in Files`, and the modals that can be moved and resized (`task-1659`).

/// Open `Go to File` the way the shortcut does, and let it settle.
fn open_go_to_file(harness: &mut Harness<'static, QuillApp>) {
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::GoToFile, &ctx);
    harness.run();
}

/// Double click a control found by name, which egui has no helper for.
///
/// Two presses and releases in one frame: egui reads a second click as a double click when it comes
/// within its own double click time of the first, and both of these carry the same frame's time.
fn double_click(harness: &mut Harness<'static, QuillApp>, label: &str) {
    let at = harness.get_by_label(label).rect().center();
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::default(),
            });
        }
    }
    harness.run();
}

/// The names the finder is currently offering, in the order it offers them.
fn found_names(harness: &Harness<'static, QuillApp>) -> Vec<String> {
    harness
        .state()
        .go_to_file
        .as_ref()
        .expect("the finder should be open")
        .results()
        .iter()
        .map(|found| found.name.clone())
        .collect()
}

#[test]
fn go_to_file_lists_the_project_before_anything_is_typed() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    harness.get_by_label("Go to file");
    let names = found_names(&harness);
    assert!(names.contains(&"readme.md".to_owned()), "it lists what is there: {names:?}");
    assert!(names.contains(&"one.md".to_owned()), "including files inside folders: {names:?}");
}

#[test]
fn go_to_file_narrows_as_a_name_is_typed() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    harness.input_mut().events.push(egui::Event::Text("one".to_owned()));
    harness.run();
    let names = found_names(&harness);
    assert_eq!(names.first().map(String::as_str), Some("one.md"), "best match first: {names:?}");
    assert!(!names.contains(&"notes.txt".to_owned()), "and what does not match is gone: {names:?}");
    harness.snapshot(shot("go_to_file"));
}

#[test]
fn double_clicking_a_row_in_go_to_file_opens_the_file_and_shuts_the_modal() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    harness.input_mut().events.push(egui::Event::Text("readme".to_owned()));
    harness.run();
    double_click(&mut harness, "Go to readme.md");
    assert!(harness.state().go_to_file.is_none(), "opening a file shuts the modal");
    assert_eq!(harness.state().document().text().to_string(), "# Quill\n");
    assert_eq!(
        harness.state().files.active().path().and_then(|path| path.file_name()),
        Some(std::ffi::OsStr::new("readme.md")),
    );
}

#[test]
fn the_arrow_keys_and_enter_open_a_file_from_go_to_file() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    harness.input_mut().events.push(egui::Event::Text("md".to_owned()));
    harness.run();
    let first = found_names(&harness)[0].clone();
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    let second = found_names(&harness)[1].clone();
    assert_ne!(first, second, "the sample folder has more than one Markdown file");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().go_to_file.is_none());
    assert_eq!(
        harness.state().files.active().name(),
        second,
        "Enter opens the row the arrow keys walked to"
    );
}

#[test]
fn escape_shuts_go_to_file_without_opening_anything() {
    let mut harness = harness("");
    let before = harness.state().files.active().name();
    open_go_to_file(&mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().go_to_file.is_none());
    assert_eq!(harness.state().files.active().name(), before);
}

/// Open `Find in Files`, type `text`, and wait for the thread to finish reading the project.
///
/// The search runs on a thread, so the test waits for an answer rather than assuming one frame is
/// enough. `pump` rather than `Harness::run` inside the loop, for the reason `task-1654` records:
/// `run` gives the window four steps to go quiet and panics otherwise, which is right for a settled
/// window and wrong while something is still being worked on.
fn search_for(harness: &mut Harness<'static, QuillApp>, text: &str) {
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::FindInFiles, &ctx);
    harness.run();
    harness.input_mut().events.push(egui::Event::Text(text.to_owned()));
    harness.run();
    for _ in 0..200 {
        if !harness
            .state()
            .find_in_files
            .as_ref()
            .expect("the search should be open")
            .is_searching()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    harness.run();
}

/// Where the search found its matches, as `name:line` for each one.
fn matches(harness: &Harness<'static, QuillApp>) -> Vec<String> {
    harness
        .state()
        .find_in_files
        .as_ref()
        .expect("the search should be open")
        .hits()
        .iter()
        .map(|hit| {
            format!("{}:{}", hit.path.file_name().unwrap().to_string_lossy(), hit.line)
        })
        .collect()
}

#[test]
fn find_in_files_finds_text_anywhere_in_the_project() {
    let mut harness = harness("");
    search_for(&mut harness, "Quill");
    let found = matches(&harness);
    assert!(found.contains(&"readme.md:1".to_owned()), "readme.md says `# Quill`: {found:?}");
    harness.get_by_label("Find in files");
    harness.get_by_label("Match case");
    harness.snapshot(shot("find_in_files"));
}

#[test]
fn find_in_files_narrows_to_nothing_when_nothing_matches() {
    let mut harness = harness("");
    search_for(&mut harness, "zzzznothinghere");
    assert!(matches(&harness).is_empty());
    assert!(!harness.state().find_in_files.as_ref().unwrap().is_searching());
}

#[test]
fn opening_a_result_selects_the_match_in_the_document() {
    let mut harness = harness("");
    search_for(&mut harness, "tables");
    let found = matches(&harness);
    assert_eq!(found, vec!["tables.txt:1".to_owned()], "one file holds the word: {found:?}");
    double_click(&mut harness, "Result tables.txt:1");
    assert!(harness.state().find_in_files.is_none(), "opening a result shuts the modal");
    assert_eq!(harness.state().files.active().name(), "tables.txt");
    assert_eq!(
        harness.state().document().selected_text(),
        "tables",
        "the match itself should be selected, which is what highlights it"
    );
}

#[test]
fn the_case_of_a_search_can_be_insisted_on() {
    let mut harness = harness("");
    search_for(&mut harness, "quill");
    assert!(
        !matches(&harness).is_empty(),
        "`quill` should find `# Quill` while case is being ignored"
    );
    harness.get_by_label("Match case").click();
    harness.run();
    for _ in 0..200 {
        if !harness.state().find_in_files.as_ref().unwrap().is_searching() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    harness.run();
    let found = matches(&harness);
    assert!(
        !found.iter().any(|hit| hit.starts_with("readme.md")),
        "with the case insisted on, `quill` no longer matches `Quill`: {found:?}"
    );
}

#[test]
fn escape_shuts_find_in_files() {
    let mut harness = harness("");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::FindInFiles, &ctx);
    harness.run();
    assert!(harness.state().find_in_files.is_some());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().find_in_files.is_none());
}

// A modal is moved by its header and resized by its edges (`task-1659`). Both live in
// `components::modal`, so `Go to File` is what they are tested through and every other modal has
// them for the same reason.

/// Where the modal with this id sits and how big it is, read back from egui's own memory.
fn placement(harness: &Harness<'static, QuillApp>, id: &str) -> quill_app::components::modal::Placement {
    quill_app::components::modal::placement(&harness.ctx, id)
}

#[test]
fn a_modal_is_moved_by_dragging_its_header() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    assert!(placement(&harness, "quill-go-to-file").is_untouched(), "it opens in the middle");
    let bar = harness.get_by_label("Move go to file").rect();
    let from = bar.center();
    drag(&mut harness, from, from + egui::vec2(-120.0, 60.0));
    let moved = placement(&harness, "quill-go-to-file");
    assert!(moved.offset.x < -100.0, "dragged left: {moved:?}");
    assert!(moved.offset.y > 40.0, "and down: {moved:?}");
    // The modal really is somewhere else, rather than only the number having changed.
    let after = harness.get_by_label("Move go to file").rect();
    assert!(after.center().x < bar.center().x - 100.0);
    harness.snapshot(shot("modal_dragged"));
}

#[test]
fn a_modal_is_resized_by_dragging_a_corner() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    let before = harness.get_by_label("Move go to file").rect().width();
    let corner = harness.get_by_label("Resize go to file: bottom right").rect().center();
    drag(&mut harness, corner, corner + egui::vec2(80.0, 40.0));
    let grown = placement(&harness, "quill-go-to-file");
    assert!(grown.grown.x > 60.0, "wider: {grown:?}");
    assert!(grown.grown.y > 20.0, "and taller: {grown:?}");
    let after = harness.get_by_label("Move go to file").rect().width();
    assert!(after > before + 60.0, "{before} then {after}");
}

#[test]
fn dragging_the_left_edge_of_a_modal_leaves_its_right_edge_where_it_was() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    let before = harness.get_by_label("Move go to file").rect();
    let edge = egui::pos2(before.left() + 3.0, before.center().y + 120.0);
    drag(&mut harness, edge, edge + egui::vec2(-60.0, 0.0));
    let after = harness.get_by_label("Move go to file").rect();
    assert!(after.left() < before.left() - 40.0, "the edge that was dragged moved");
    assert!(
        (after.right() - before.right()).abs() < 2.0,
        "and the other one did not: {} then {}",
        before.right(),
        after.right()
    );
}

#[test]
fn double_clicking_a_modals_header_puts_it_back_in_the_middle() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    let bar = harness.get_by_label("Move go to file").rect();
    let from = bar.center();
    drag(&mut harness, from, from + egui::vec2(-120.0, 60.0));
    assert!(!placement(&harness, "quill-go-to-file").is_untouched());
    double_click(&mut harness, "Move go to file");
    assert!(
        placement(&harness, "quill-go-to-file").is_untouched(),
        "a double click puts it back, as it does to a pane divider"
    );
}

#[test]
fn a_modal_cannot_be_dragged_out_of_the_window() {
    let mut harness = harness("");
    open_go_to_file(&mut harness);
    let bar = harness.get_by_label("Move go to file").rect();
    let from = bar.center();
    // Far past the right hand edge of a 1180 point window.
    drag(&mut harness, from, from + egui::vec2(4000.0, 4000.0));
    let after = harness.get_by_label("Move go to file").rect();
    assert!(after.right() <= WINDOW[0], "still inside: {after:?}");
    assert!(after.top() >= 0.0);
}

// Pictures in the Markdown preview (`task-1659`).

/// A folder of its own holding a Markdown file with a picture in it.
///
/// Its own folder rather than the shared sample, because every explorer screenshot counts the files
/// in that one, and a document made of pictures is not what the rest of the tests are about. Written
/// each time, like `git_folder`, and kept apart by its name.
fn picture_document_folder() -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-preview-pictures");
    std::fs::create_dir_all(&root).expect("make the folder");
    write_sample_picture(&root.join("picture.png"));
    std::fs::write(
        root.join("gallery.md"),
        "# A picture\n\nSome words before it.\n\n![the sample picture](picture.png)\n\nAnd some after.\n\n![missing](nowhere.png)\n",
    )
    .expect("write gallery.md");
    root
}

/// Open `gallery.md` in the preview, in a window on the folder holding it.
fn preview_of_the_gallery() -> Harness<'static, QuillApp> {
    let folder = picture_document_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("gallery.md"));
    harness.run();
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    harness.run();
    harness
}

#[test]
fn the_markdown_preview_draws_a_picture() {
    let mut harness = preview_of_the_gallery();
    let pictures = harness.state().preview_pictures();
    assert_eq!(pictures.len(), 2, "one picture that is there and one that is not");
    let drawn = &pictures[0];
    assert!(drawn.texture.is_some(), "the picture beside the document should have been read");
    assert_eq!(drawn.size, egui::vec2(160.0, 100.0), "at its own size, which fits the pane");
    harness.snapshot(shot("preview_picture"));
}

#[test]
fn a_picture_that_is_not_there_leaves_its_alt_text() {
    let harness = preview_of_the_gallery();
    let missing = &harness.state().preview_pictures()[1];
    assert!(missing.texture.is_none());
    assert_eq!(missing.alt, "missing", "which is what is drawn in its place");
    assert_eq!(missing.size, egui::vec2(0.0, 0.0), "and it takes no room of its own");
}

#[test]
fn the_line_holding_a_picture_is_as_tall_as_the_picture() {
    let harness = preview_of_the_gallery();
    let picture = &harness.state().preview_pictures()[0];
    let line = harness
        .state()
        .preview_layout()
        .lines
        .iter()
        .find(|line| line.paragraph == picture.paragraph)
        .expect("the picture's own line");
    assert!(
        line.height >= picture.size.y,
        "the room reserved ({}) has to hold the picture ({})",
        line.height,
        picture.size.y
    );
    // And what follows it really is below it, rather than drawn over it.
    let next = harness
        .state()
        .preview_layout()
        .lines
        .iter()
        .find(|line| line.paragraph > picture.paragraph)
        .expect("the line after the picture");
    assert!(next.y >= line.y + picture.size.y);
}

#[test]
fn a_wide_picture_is_scaled_down_to_the_width_of_the_pane() {
    let folder = std::env::temp_dir().join("quill-preview-wide-picture");
    std::fs::create_dir_all(&folder).expect("make the folder");
    // Four thousand pixels across, which is wider than any pane in a 1180 point window.
    let wide = image::RgbaImage::from_pixel(4000, 1000, image::Rgba([0x30, 0x70, 0xC0, 255]));
    wide.save(folder.join("wide.png")).expect("write wide.png");
    std::fs::write(folder.join("wide.md"), "![wide](wide.png)\n").expect("write wide.md");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("wide.md"));
    harness.run();
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    harness.run();
    let picture = &harness.state().preview_pictures()[0];
    assert!(picture.size.x <= WINDOW[0], "scaled to fit: {:?}", picture.size);
    assert!(
        (picture.size.x / picture.size.y - 4.0).abs() < 0.01,
        "and kept its shape: {:?}",
        picture.size
    );
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn the_shortcut_on_the_menu_opens_go_to_file() {
    let mut harness = harness("");
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::O);
    harness.run();
    assert!(harness.state().go_to_file.is_some(), "Ctrl or Cmd, Shift and O");
}

#[test]
fn the_shortcut_on_the_menu_opens_find_in_files() {
    let mut harness = harness("");
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::F);
    harness.run();
    assert!(harness.state().find_in_files.is_some(), "Ctrl or Cmd, Shift and F");
}

#[test]
fn typing_in_go_to_file_leaves_the_document_alone() {
    // The same rule `task-1656` is about: egui leaves the events a text box consumed in the frame's
    // list, so a new box is a new chance for the document behind it to read them too.
    let mut harness = harness("the document");
    let before = harness.state().document().text().to_string();
    // `with_text` types the text in, so the document starts out modified; what matters is that
    // nothing typed into the box changes it any further.
    let modified = harness.state().document().is_modified();
    open_go_to_file(&mut harness);
    harness.input_mut().events.push(egui::Event::Text("readme".to_owned()));
    harness.run();
    harness.key_press(egui::Key::Backspace);
    harness.run();
    assert_eq!(harness.state().go_to_file.as_ref().unwrap().query, "readm");
    assert_eq!(harness.state().document().text().to_string(), before);
    assert_eq!(harness.state().document().is_modified(), modified);
}

#[test]
fn typing_in_find_in_files_leaves_the_document_alone() {
    let mut harness = harness("the document");
    let before = harness.state().document().text().to_string();
    let modified = harness.state().document().is_modified();
    search_for(&mut harness, "quill");
    harness.key_press(egui::Key::Backspace);
    harness.run();
    assert_eq!(harness.state().find_in_files.as_ref().unwrap().query, "quil");
    assert_eq!(harness.state().document().text().to_string(), before);
    assert_eq!(harness.state().document().is_modified(), modified);
}

#[test]
fn the_divider_in_find_in_files_moves_the_split_between_the_results_and_the_preview() {
    let mut harness = harness("");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::FindInFiles, &ctx);
    harness.run();
    let before = harness.state().panes.find_split;
    let divider = harness.get_by_label("Resize find results").rect().center();
    drag(&mut harness, divider, divider + egui::vec2(0.0, 90.0));
    let after = harness.state().panes.find_split;
    assert!(after > before + 0.05, "the results should have grown: {before} then {after}");
    // And it is a pane like any other, so a double click puts it back.
    double_click(&mut harness, "Resize find results");
    assert!(
        (harness.state().panes.find_split - quill_app::components::find_in_files::SPLIT).abs() < 0.001
    );
}

#[test]
fn the_preview_under_the_results_follows_the_one_that_is_chosen() {
    let mut harness = harness("");
    search_for(&mut harness, "Quill");
    let find = harness.state().find_in_files.as_ref().expect("open");
    let chosen = find.chosen_hit().expect("something matched").clone();
    assert_eq!(
        find.scrolled_to(),
        Some((chosen.path.as_path(), chosen.line)),
        "the preview should have been scrolled to the result that is chosen"
    );
    // Walking the list moves the preview with it.
    if harness.state().find_in_files.as_ref().unwrap().hits().len() > 1 {
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        let find = harness.state().find_in_files.as_ref().unwrap();
        let now = find.chosen_hit().unwrap().clone();
        assert_ne!((now.path.clone(), now.line), (chosen.path, chosen.line));
        assert_eq!(find.scrolled_to(), Some((now.path.as_path(), now.line)));
    }
}

// ============================================================================================
// Running: the tile, the widget, the flyout and the dialog.
//
// `task-1683`. Every one of these is drawn from a **detached** session — one with no program behind
// it, fed fixed bytes — which is the trick the terminal's own pictures already use: when a real
// program answers is not something a test can know, and a picture that depended on it would differ
// between runs. What is being looked at is the drawing, and the drawing is the same either way.


use quill_app::services::run_configurations::{Configuration, RunConfigurations};

/// A configuration, spelled out.
fn configuration(name: &str, command: &str) -> Configuration {
    Configuration::new(name, command)
}

/// A window with a run going that has no program behind it.
fn with_run(name: &str, command: &str, rows: usize) -> Harness<'static, QuillApp> {
    let mut harness = harness("A document above the run tile.");
    harness.state_mut().new_detached_run(configuration(name, command), rows, 96);
    harness.run();
    harness
}

/// Feed bytes to the run that is showing, as a program writing them would.
fn feed_run(harness: &mut Harness<'static, QuillApp>, bytes: &[u8]) {
    harness
        .state_mut()
        .run
        .active_mut()
        .expect("a run")
        .session
        .feed(bytes);
    harness.run();
}

#[test]
fn the_run_tile_shows_a_program_running_along_the_bottom() {
    let mut harness = with_run("Dev server", "node server.js --port 3000", 12);
    assert!(harness.state().run.visible);
    assert!(!harness.state().terminal.visible, "the bottom of the window holds one tile");
    feed_run(
        &mut harness,
        b"> dev-server@1.0.0 start\r\n> node server.js --port 3000\r\n\r\n\x1b[32mListening on http://localhost:3000\x1b[0m\r\n  GET /            200  4ms\r\n  GET /style.css   200  1ms\r\n",
    );
    let screen = harness.state().run.active().expect("a run").session.snapshot();
    assert!(screen.contains("Listening on"), "the output should be on the screen");
    // The strip says it is going, and the three buttons that act on it are there.
    harness.get_by_label("Run: Dev server");
    harness.get_by_label("Rerun");
    harness.get_by_label("Stop the run");
    harness.get_by_label("Clear the run output");
    harness.snapshot(shot("run_tile"));
}

#[test]
fn a_run_that_ended_keeps_its_tab_and_the_strip_says_what_it_ended_with() {
    // IntelliJ prints its epilogue into the console; Quill puts it in the strip, because a line
    // pretending to be program output is exactly the confusion a separate strip avoids.
    let mut harness = with_run("cargo test", "cargo test", 10);
    feed_run(
        &mut harness,
        b"running 12 tests\r\n\x1b[31mtest the_thing ... FAILED\x1b[0m\r\n\r\ntest result: FAILED. 11 passed; 1 failed\r\n",
    );
    // A second run, so the picture holds a finished tab and a running one side by side.
    harness
        .state_mut()
        .new_detached_run(configuration("Dev server", "node server.js"), 10, 96);
    harness.run();
    feed_run(&mut harness, b"Listening on http://localhost:3000\r\n");
    let at = harness.state().run.index_of("cargo test").expect("the first run");
    harness.state_mut().run.end_detached(at, Some(101));
    harness.run();
    assert_eq!(
        harness.state().run.at(at).expect("a run").state().label(),
        "exit code 101",
        "the tab stays, holding what the program wrote"
    );
    harness.snapshot(shot("run_tile_finished"));
}

#[test]
fn the_run_tile_and_the_terminal_tile_take_the_same_place_and_never_both() {
    // Two grids stacked take the editing area below the fold of anything, so pressing either
    // button shows one and puts the other away.
    let mut harness = with_run("Dev server", "node server.js", 8);
    assert!(harness.state().run.visible && !harness.state().terminal.visible);
    let bottom = harness.state().run.grid_area();
    choose(&mut harness, Action::ToggleTerminal);
    assert!(harness.state().terminal.visible && !harness.state().run.visible);
    assert_eq!(
        harness.state().terminal.grid_area().bottom(),
        bottom.bottom(),
        "the same place at the bottom of the window"
    );
    choose(&mut harness, Action::ToggleRunTile);
    assert!(harness.state().run.visible && !harness.state().terminal.visible);
    // And the rail has a button for each, the run one above the terminal one.
    harness.get_by_label("Run tile");
    harness.get_by_label("Terminal tile");

    // Every path that shows either tile puts the other away, which is what the two functions on
    // `QuillApp` are for. This is the fault they were written for: `terminal show` from the command
    // line set its own flag and left the run tile up, so both grids were drawn into the same
    // rectangle, one over the other — found in the real window rather than here.
    did(&mut harness, "terminal show");
    assert!(harness.state().terminal.visible && !harness.state().run.visible, "terminal show");
    choose(&mut harness, Action::ToggleRunTile);
    assert!(harness.state().run.visible && !harness.state().terminal.visible);
    choose(&mut harness, Action::NewTerminalTab);
    assert!(harness.state().terminal.visible && !harness.state().run.visible, "New Terminal Tab");
    did(&mut harness, "terminal hide");
    assert!(!harness.state().terminal.visible && !harness.state().run.visible, "and neither is up");
}

#[test]
fn the_run_widget_draws_its_three_states_in_the_title_bar() {
    let mut results = SnapshotResults::new();

    // Idle: a configuration chosen, nothing running.
    let mut harness = harness("");
    harness.state_mut().run_configurations.add_permanent(configuration("Dev server", "node server.js --port 3000"));
    harness.state_mut().run_selected = Some("Dev server".to_owned());
    harness.run();
    harness.get_by_label("Choose a run configuration");
    harness.get_by_label("Run the selected configuration");
    results.add(harness.try_snapshot(shot("run_widget_idle")));

    // Running: the stop square appears beside the play button. A control absent when it cannot
    // apply, drawn the moment it can.
    let mut harness = with_run("Dev server", "node server.js --port 3000", 8);
    feed_run(&mut harness, b"Listening on http://localhost:3000\r\n");
    harness.get_by_label("Stop the selected configuration");
    results.add(harness.try_snapshot(shot("run_widget_running")));

    // Stopped with an error: the widget goes back to two buttons and the tile's strip carries the
    // code, which is where the eye already is.
    let at = harness.state().run.index_of("Dev server").expect("the run");
    harness.state_mut().run.end_detached(at, Some(1));
    harness.run();
    assert!(
        harness.query_by_label("Stop the selected configuration").is_none(),
        "there is nothing left to stop"
    );
    results.add(harness.try_snapshot(shot("run_widget_stopped")));

    report(results);
}

/// `task-1692`: the two buttons at the top right, in IntelliJ's order, and the rule that decides
/// whether the second one is there at all.
#[test]
fn the_title_bar_carries_a_run_button_and_a_debug_button_beside_it() {
    // The picture is `run_widget_idle`, which is this scene; what is asserted here is the rule that
    // decides whether the second button is drawn at all.
    // A configuration a debugger can take: `node server.js` names js-debug through the command line
    // itself, which is what `debuggers::adapter_for` reads.
    let mut both = harness("");
    both.state_mut()
        .run_configurations
        .add_permanent(configuration("Dev server", "node server.js --port 3000"));
    both.state_mut().run_selected = Some("Dev server".to_owned());
    both.run();
    both.get_by_label("Run the selected configuration");
    both.get_by_label("Debug the selected configuration");

    // And one nothing can debug has one button, which is Quill's rule for a control that cannot
    // apply: absent rather than dimmed. Nothing here names a debugger — not the command line, not
    // the plugins, and not the untitled document that is showing.
    let mut plain = harness("");
    plain.state_mut().run_configurations.add_permanent(configuration("Format", "black app"));
    plain.state_mut().run_selected = Some("Format".to_owned());
    plain.run();
    plain.get_by_label("Run the selected configuration");
    assert!(
        plain.query_by_label("Debug the selected configuration").is_none(),
        "there is nothing here a debugger could take"
    );
}

/// The bug button sends the same `Action` the `Run` menu and `Shift+F9` send, which is what
/// `QuillApp::debug_a_configuration` being the one place means.
#[test]
fn the_widgets_debug_button_starts_the_chosen_configuration_under_a_debugger() {
    let mut pressed = harness("");
    pressed
        .state_mut()
        .run_configurations
        .add_permanent(configuration("Dev server", "node server.js"));
    pressed.state_mut().run_selected = Some("Dev server".to_owned());
    pressed.run();
    pressed.get_by_label("Debug the selected configuration").click();
    pressed.run();
    // What happens next depends on what this machine has installed, and the test may not: what is
    // being proved is that the press reached the debugger at all, which either a session or a
    // sentence about the adapter shows.
    let said = pressed.state().message.clone().unwrap_or_default();
    assert!(
        pressed.state().debug.is_some() || said.contains("node"),
        "the press reached the debugger: {said}"
    );
}

/// A debugger this machine has not got is a panel that says what is missing and offers the command,
/// rather than an empty box — `task-1692` §7.1.
///
/// What was found is seeded rather than searched for, because a picture that depended on whether the
/// machine running the test had CodeLLDB installed would not be a baseline at all.
#[test]
fn the_debug_tile_says_what_is_missing_and_offers_to_install_it() {
    let mut harness = harness("");
    harness
        .state_mut()
        .run_configurations
        .add_permanent(configuration("App", r"target\debugpp.exe"));
    harness.state_mut().run_selected = Some("App".to_owned());
    harness.state_mut().debug_adapters.insert(
        "lldb".to_owned(),
        (
            std::time::Instant::now(),
            quill_app::services::debuggers::Report {
                name: "lldb",
                found: None,
                configured: false,
                programs: vec!["codelldb", "lldb-dap"],
                languages: vec!["Rust".to_owned()],
                comes_from: "lldb-dap ships with LLVM, and codelldb is the CodeLLDB extension's adapter",
                install: "winget install --id LLVM.LLVM -e".to_owned(),
                settings_key: "debug.lldb".to_owned(),
                caveat: "",
            },
        ),
    );
    choose(&mut harness, Action::Debug(DebugAction::ToggleTile));
    harness.run();
    assert!(harness.state().debug_panel.visible);
    harness.get_by_label("Install");
    harness.get_by_label("Copy command");
    harness.snapshot(shot("debug_tile_missing_adapter"));
}

#[test]
fn with_nothing_to_run_the_widget_is_the_play_button_that_opens_the_dialog() {
    // Present, because the way to discover the feature has to be visible; small, because it is not
    // yet in use. The sample folder holds neither a Cargo.toml nor a package.json, so no detector
    // has anything to say about it.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    assert!(harness.state().run_rows().is_empty(), "nothing to suggest in the sample folder");
    harness.get_by_label("Add a run configuration").click();
    harness.run();
    assert!(harness.state().run_dialog.open, "the play button opens the dialog when nothing is chosen");
}

#[test]
fn the_widgets_play_button_starts_the_chosen_configuration() {
    // The button is wired to the same `Action` the `Run` menu and the keyboard send, which is what
    // `QuillApp::run_action` being the one place means. A program that is not there is what is run
    // on purpose: what is being proved is that the press reaches the starting, and a test that
    // spawned a real one would be a test that waited for it.
    let mut harness = harness("");
    harness
        .state_mut()
        .run_configurations
        .add_permanent(configuration("Nothing", "quill-no-such-program-at-all"));
    harness.state_mut().run_selected = Some("Nothing".to_owned());
    harness.run();
    harness.get_by_label("Run the selected configuration").click();
    harness.run();
    let said = harness.state().message.clone().expect("the status bar says what happened");
    assert!(
        said.contains("quill-no-such-program-at-all"),
        "the press should have reached the starting, and the bar says {said:?}"
    );
    assert!(harness.state().run.is_empty(), "nothing was started");
}

#[test]
fn the_flyout_lists_the_permanents_the_temporaries_and_the_suggestions() {
    // A project with a `package.json` in it, so the npm detector has something to say — which is
    // what makes the third kind of row appear at all.
    let folder = std::env::temp_dir().join("quill-run-suggestions");
    std::fs::create_dir_all(&folder).expect("make the project");
    std::fs::write(
        folder.join("package.json"),
        "{\n  \"name\": \"site\",\n  \"scripts\": { \"dev\": \"vite\", \"build\": \"vite build\" }\n}\n",
    )
    .expect("write the package");
    std::fs::write(folder.join("server.js"), "// a server\n").expect("write a file");

    let mut harness = harness_in(&folder);
    harness
        .state_mut()
        .run_configurations
        .add_permanent(configuration("Dev server", "node server.js --port 3000"));
    harness.state_mut().run_configurations.add_temporary(configuration("server.js", "node server.js"));
    harness.state_mut().run_selected = Some("Dev server".to_owned());
    harness.run();

    let rows: Vec<String> = harness.state().run_rows().into_iter().map(|row| row.name).collect();
    assert_eq!(
        rows,
        vec!["Dev server", "server.js", "npm run build", "npm run dev"],
        "permanents, then temporaries, then what the detectors suggest, in name order"
    );

    harness.get_by_label("Choose a run configuration").click();
    harness.run();
    harness.get_by_label("npm run dev");
    harness.get_by_label("Edit Configurations...");
    harness.snapshot(shot("run_flyout"));
}

#[test]
fn running_a_suggestion_keeps_it_as_a_temporary_so_it_can_be_run_again() {
    let folder = std::env::temp_dir().join("quill-run-suggestion-kept");
    std::fs::create_dir_all(&folder).expect("make the project");
    std::fs::write(folder.join("Cargo.toml"), "[package]\nname = \"thing\"\n").expect("write it");
    let mut harness = harness_in(&folder);
    assert_eq!(
        harness.state().run_rows().into_iter().map(|row| row.name).collect::<Vec<_>>(),
        vec!["cargo run"],
        "the detector offers it"
    );
    assert!(harness.state().run_configurations.is_empty(), "and nothing is held yet");
    // Running it makes a temporary. The program itself may or may not start on this machine, which
    // is not what is being tested: what is, is that the thing that was run is now in the list.
    choose(&mut harness, Action::Run(RunAction::Start(Some("cargo run".to_owned()))));
    assert_eq!(harness.state().run_configurations.temporary().len(), 1);
    assert_eq!(harness.state().run_selected.as_deref(), Some("cargo run"));
    // And it is no longer offered as a suggestion as well, so it is one row rather than two.
    assert_eq!(harness.state().run_rows().len(), 1);
    harness.state_mut().run.kill_everything();
}

#[test]
fn run_current_file_is_offered_for_a_javascript_file_and_not_for_a_rust_one() {
    // The plugin's own `run.file`, asked at the moment of use — so switching the JavaScript plugin
    // off withdraws it in the same frame.
    let folder = std::env::temp_dir().join("quill-run-current-file");
    std::fs::create_dir_all(&folder).expect("make the project");
    std::fs::write(folder.join("server.js"), "console.log('hello')\n").expect("write it");
    std::fs::write(folder.join("main.rs"), "fn main() {}\n").expect("write it");
    let mut harness = harness_in(&folder);

    harness.state_mut().open_path_permanently(&folder.join("server.js"));
    harness.run();
    assert_eq!(harness.state().run_file_template().as_deref(), Some("node {file}"));

    harness.state_mut().open_path_permanently(&folder.join("main.rs"));
    harness.run();
    assert_eq!(
        harness.state().run_file_template(),
        None,
        "running one file of a Cargo project is not a thing cargo does"
    );

    harness.state_mut().open_path_permanently(&folder.join("server.js"));
    harness.run();
    harness.state_mut().set_plugin_enabled("javascript", false);
    harness.run();
    assert_eq!(harness.state().run_file_template(), None, "and the plugin is the switch");
}

#[test]
fn a_program_that_prints_and_stops_leaves_what_it_printed_in_its_tab() {
    // The one test here that starts a real program, and it earns it: this is the fault `task-1683`
    // spent its last hour on. A run is opened at a **guessed** size and told the real one on the
    // first frame the tile draws — and a pseudoconsole resized while its child is writing its first
    // line loses that line. `cmd /c echo something` writes and exits inside a millisecond, so it
    // was always still starting when that frame came, and its tab came up empty every single time.
    //
    // The fix is that `QuillApp::run_grid_size` works the size out from the rectangle the tile
    // really has, so there is no resize at all. What proves it is a program that prints and stops.
    let folder = std::env::temp_dir().join("quill-run-prints-and-stops");
    std::fs::create_dir_all(&folder).expect("make the project");
    let mut harness = harness_in(&folder);

    let marker = "quill-printed-this";
    let command = if cfg!(target_os = "windows") {
        format!("cmd /c echo {marker}")
    } else {
        format!("/bin/sh -c \"echo {marker}\"")
    };
    harness.state_mut().run_configurations.add_permanent(configuration("printer", &command));
    harness.state_mut().run_selected = Some("printer".to_owned());
    choose(&mut harness, Action::Run(RunAction::Start(None)));

    // `pump`, not `Harness::run`: `run` gives the window four steps to go quiet and panics
    // otherwise, which is right for a settled window and wrong while a program is being waited for.
    // The rule `task-1654` wrote down, wearing a different hat again.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        harness.step();
        let run = harness.state().run.at(0).expect("the run");
        if !run.is_running() && run.session.snapshot().contains(marker) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the program did not print and finish in thirty seconds; it is {} and the screen holds {:?}",
            run.state().label(),
            run.session.snapshot().text()
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    // And it stays there, which is what a tab outliving its program means: the frames after it
    // finished must not take it away either.
    for _ in 0..8 {
        harness.step();
    }
    let run = harness.state().run.at(0).expect("the run");
    assert_eq!(run.state().label(), "finished");
    assert!(
        run.session.snapshot().contains(marker),
        "the output should still be there, and the screen holds {:?}",
        run.session.snapshot().text()
    );
    harness.state_mut().run.kill_everything();
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn the_run_configurations_dialog_lists_them_on_the_left_and_edits_one_on_the_right() {
    let mut harness = harness("");
    harness
        .state_mut()
        .run_configurations
        .add_permanent(Configuration {
            name: "Dev server".to_owned(),
            command: "node server.js --port 3000".to_owned(),
            directory: "backend".to_owned(),
            env: "PORT=3000; DEBUG=app:*".to_owned(),
        });
    harness.state_mut().run_configurations.add_permanent(configuration("cargo run", "cargo run"));
    harness.state_mut().run_selected = Some("Dev server".to_owned());
    choose(&mut harness, Action::Run(RunAction::Edit));
    assert!(harness.state().run_dialog.open);
    harness.get_by_label("Run configuration name");
    harness.get_by_label("Run configuration command");
    harness.get_by_label("Run configuration directory");
    harness.get_by_label("Run configuration environment");
    harness.get_by_label("Add");
    harness.get_by_label("Remove");
    harness.get_by_label("Done");
    harness.snapshot(shot("run_dialog"));

    // Add makes one with a name nothing else has, and chooses it.
    harness.get_by_label("Add").click();
    harness.run();
    assert_eq!(harness.state().run_dialog.chosen.as_deref(), Some("Unnamed"));
    assert_eq!(harness.state().run_configurations.permanent().len(), 3);

    // Done shuts it.
    harness.get_by_label("Done").click();
    harness.run();
    assert!(!harness.state().run_dialog.open);
}

#[test]
fn the_dialog_asks_before_removing_a_configuration_whose_program_is_still_running() {
    // Silently killing a server somebody is watching is worse than one extra click.
    let mut harness = with_run("Dev server", "node server.js", 8);
    choose(&mut harness, Action::Run(RunAction::Edit));
    harness.get_by_label("Remove").click();
    harness.run();
    let question = harness.state().confirmation.clone().expect("a question is asked");
    assert!(question.note.contains("Dev server"), "and it says what is about to be stopped");
    assert_eq!(harness.state().run_configurations.len(), 1, "nothing has gone yet");

    harness.state_mut().answer_the_question(question.answer);
    harness.run();
    assert!(harness.state().run_configurations.is_empty());
    assert!(harness.state().run.is_empty(), "and its run went with it");
}

#[test]
fn a_project_comes_back_with_its_configurations_and_the_choice_it_still_has() {
    // What `.quill` remembers: the permanents in a file of their own, and which of them was chosen
    // in `workspace.conf` beside the terminal's flags. A **temporary** is deliberately not written
    // down, so a project that had one chosen comes back with nothing chosen rather than offering to
    // run something that is not there.
    let root = std::env::temp_dir().join("quill-run-remembered");
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("make the project");
    let mut configurations = RunConfigurations::new();
    configurations.add_permanent(Configuration {
        name: "Dev server".to_owned(),
        command: "node server.js".to_owned(),
        directory: "backend".to_owned(),
        env: "PORT=3000".to_owned(),
    });
    quill_app::services::run_configurations::save(&root, &configurations);
    let folder = quill_app::services::project_state::folder(&root);
    std::fs::write(
        folder.join("workspace.conf"),
        "run.visible = true
run.selected = Dev server
",
    )
    .expect("write the workspace");

    let mut harness = harness_in(&root);
    harness.state_mut().restore_project();
    harness.run();
    assert_eq!(harness.state().run_configurations.permanent().len(), 1);
    let held = harness.state().run_configurations.find("Dev server").expect("it came back").1.clone();
    assert_eq!(held.command, "node server.js");
    assert_eq!(held.directory, "backend");
    assert_eq!(held.env, "PORT=3000");
    assert_eq!(harness.state().run_selected.as_deref(), Some("Dev server"));
    assert!(harness.state().run.visible, "and the tile was up");
    assert!(harness.state().run.is_empty(), "with nothing in it: a run is not restarted");

    // A remembered choice that nothing answers to any more is dropped rather than offered.
    std::fs::write(folder.join("workspace.conf"), "run.selected = server.js
")
        .expect("write the workspace");
    let mut harness = harness_in(&root);
    harness.state_mut().restore_project();
    harness.run();
    assert_eq!(harness.state().run_selected, None);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_temporary_has_a_save_button_that_keeps_it_and_a_permanent_does_not() {
    let mut harness = harness("");
    harness.state_mut().run_configurations.add_temporary(configuration("server.js", "node server.js"));
    harness.state_mut().run_configurations.add_permanent(configuration("cargo run", "cargo run"));
    harness.state_mut().run_dialog.open(Some("cargo run".to_owned()));
    harness.run();
    assert!(harness.query_by_label("Save").is_none(), "a permanent is already kept");

    harness.state_mut().run_dialog.chosen = Some("server.js".to_owned());
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    assert!(harness.state().run_configurations.temporary().is_empty());
    assert_eq!(harness.state().run_configurations.permanent().len(), 2);
}

// ============================================================================================
// The command line, driven through a real window.
//
// `task-1661`. These go through the whole of the command line path apart from the socket: the
// words are parsed against `quill_cli::catalogue`, dispatched by `QuillApp::run_cli`, and what is
// checked afterwards is the window's own state — not the reply's opinion of itself. A command that
// says it opened a file and a window with that file open are two different claims, and only the
// second one is worth testing.
//
// No pictures. What a command does to the rendering is already covered by the screenshot tests
// above, which set the same states up by hand; what is unproven, and what these prove, is that the
// command line reaches those states at all.

/// Run a command line against the window and take the reply, insisting it was answered.
fn run(harness: &mut Harness<'static, QuillApp>, line: &str) -> quill_cli::protocol::Reply {
    let ctx = harness.ctx.clone();
    let reply = harness
        .state_mut()
        .run_command_line(line, &ctx)
        .unwrap_or_else(|| panic!("`{line}` was not answered on the frame it was asked"));
    harness.run();
    reply
}

/// The same, insisting it worked.
fn did(harness: &mut Harness<'static, QuillApp>, line: &str) -> serde_json::Value {
    let reply = run(harness, line);
    assert!(reply.ok, "`{line}` was refused: {}", reply.message);
    reply.result
}

/// The same, for a command that leaves the window asking to be drawn again later.
///
/// A polite stop asks for one frame two seconds hence, so the window has not gone quiet when the
/// reply lands. `Harness::run` gives it four steps to settle and panics otherwise, which is right
/// for a settled window and wrong here — the rule `task-1654` already wrote down about waiting
/// loops, wearing a different hat.
fn did_while_waiting(harness: &mut Harness<'static, QuillApp>, line: &str) -> serde_json::Value {
    let ctx = harness.ctx.clone();
    let reply = harness
        .state_mut()
        .run_command_line(line, &ctx)
        .unwrap_or_else(|| panic!("`{line}` was not answered on the frame it was asked"));
    harness.step();
    assert!(reply.ok, "`{line}` was refused: {}", reply.message);
    reply.result
}

/// The same, insisting it was refused, and returning the code it was refused with.
fn refused(harness: &mut Harness<'static, QuillApp>, line: &str) -> String {
    let reply = run(harness, line);
    assert!(!reply.ok, "`{line}` should have been refused, and was not");
    reply.error.expect("a refusal carries an error").code
}

/// Build a project that initially belongs to an ancestor repository.
fn late_repository_project() -> (std::path::PathBuf, std::path::PathBuf) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let ancestor = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("_agent_output/task-1701-git-root-refresh/tests")
        .join(format!("{}-{unique}", std::process::id()));
    let project = ancestor.join("project");
    std::fs::create_dir_all(&project).expect("make the late repository project");
    let initialized = quill_git::command::run(&ancestor, &["init", "--initial-branch=main"]);
    assert!(initialized.ok, "initialize the ancestor: {}", initialized.message());
    std::fs::write(project.join("before.txt"), "before repository creation\n").expect("write before.txt");
    (ancestor, project)
}

/// A project that becomes a repository while its window is open switches away from its ancestor.
#[test]
fn git_status_rediscovers_a_repository_created_after_the_window_opened() {
    let (ancestor, project) = late_repository_project();
    let mut harness = harness_in(&project);
    settle(&mut harness, "the ancestor repository", |app| {
        app.git.as_ref().is_some_and(|git| !git.is_busy())
    });
    let before = did(&mut harness, "git status --json");
    assert_eq!(before["rootRelation"], "ancestor");
    assert_eq!(std::path::PathBuf::from(before["root"].as_str().unwrap()).canonicalize().unwrap(), ancestor.canonicalize().unwrap());

    let initialized = quill_git::command::run(&project, &["init", "--initial-branch=master"]);
    assert!(initialized.ok, "initialize the project: {}", initialized.message());
    std::fs::write(project.join("after.txt"), "after repository creation\n").expect("write after.txt");
    let ctx = harness.ctx.clone();
    assert!(harness.state_mut().run_command_line("git status --json", &ctx).is_none());
    settle(&mut harness, "the project repository", |app| {
        app.git.as_ref().is_some_and(|git| {
            !git.is_busy()
                && quill_app::app::git::root_relation(git.repository.root(), &project)
                    == quill_app::app::git::RootRelation::Project
        })
    });

    let after = did(&mut harness, "git status --json");
    assert_eq!(after["rootRelation"], "project");
    assert_eq!(after["branch"], "master");
    assert_eq!(after["changed"].as_array().map(Vec::len), Some(2));
    assert_eq!(std::path::PathBuf::from(after["root"].as_str().unwrap()).canonicalize().unwrap(), project.canonicalize().unwrap());

    let window = did(&mut harness, "status --json");
    assert_eq!(window["git"]["rootRelation"], "project");
    assert_eq!(window["git"]["root"], after["root"]);
    did_while_waiting(&mut harness, "git action add --path after.txt");
    settle(&mut harness, "the project file to be staged", |app| {
        app.git.as_ref().is_some_and(|git| !git.is_busy())
    });
    assert_eq!(ask_git(&project, &["diff", "--cached", "--name-only"]), "after.txt");
}

/// Press at `from` and move through each of `path` before letting go.
///
/// [`drag`] moves once, which is enough for a divider: it starts the drag and ends it in the same
/// motion. The editing area needs more, because the frame a drag *starts* on is the frame the caret
/// is placed on, and it takes a second movement before there is anything selected — which is exactly
/// what a person does with the mouse and is the gesture `task-1666` reported.
fn drag_through(harness: &mut Harness<'static, QuillApp>, from: egui::Pos2, path: &[egui::Pos2]) {
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
    for at in path {
        harness.input_mut().events.push(egui::Event::PointerMoved(*at));
        harness.run();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: *path.last().unwrap_or(&from),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    harness.run();
}

/// A folder holding one long source file, for the `task-1666` performance tests.
///
/// Written fresh each time under a name of its own, which is what `git_folder` already does and what
/// keeps two tests from writing over one another.
fn a_folder_with_a_long_source_file(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-performance").join(name);
    std::fs::create_dir_all(&root).expect("make the folder");
    let source: String = (0..600)
        .map(|i| format!("/// The {i}th one.\nfn line_{i}(value: usize) -> usize {{\n    value + {i}\n}}\n\n"))
        .collect();
    std::fs::write(root.join("long.rs"), source).expect("write long.rs");
    root
}

/// `task-1666`. **Dragging a selection must lay nothing out again and colour nothing again.**
///
/// This is the gesture the ticket reported, and the fault behind it was that moving the caret counted
/// as a change to the text: `refresh_layout` and `colour_the_file` were both keyed on
/// `Document::revision()`, which a caret move bumps. So every frame of a drag re-tokenised the file,
/// rebuilt every style span and laid the whole document out — about 650 ms a frame on a file the size
/// of `app/mod.rs`.
#[test]
fn dragging_a_selection_lays_nothing_out_again_and_colours_nothing_again() {
    let folder = a_folder_with_a_long_source_file("drag-selection");
    let mut harness = harness_in(&folder);
    did(&mut harness, "tab open long.rs --permanent");
    harness.run();
    // The file is coloured on the frame after it is opened, so let that happen before anything is
    // measured; otherwise the first drag frame would be charged with work the opening owed.
    harness.run();

    let laid_out = harness.state().files.active().cached.laid_out_revision;
    let coloured = harness.state().files.active().coloured_revision;
    let text_revision = harness.state().document().text_revision();
    let revision = harness.state().document().revision();
    assert!(coloured.is_some(), "a .rs file is coloured by the bundled plugin");

    let area = harness.state().editor_area();
    drag_through(
        &mut harness,
        area.left_top() + vec2(60.0, 30.0),
        &[
            area.left_top() + vec2(120.0, 90.0),
            area.left_top() + vec2(180.0, 170.0),
            area.left_top() + vec2(220.0, 260.0),
        ],
    );
    harness.run();

    assert!(
        !harness.state().document().selection().is_empty(),
        "the drag should have selected some text"
    );
    assert_eq!(
        harness.state().document().text_revision(),
        text_revision,
        "dragging a selection changes no text"
    );
    assert_eq!(
        harness.state().files.active().cached.laid_out_revision,
        laid_out,
        "so the document was not laid out again"
    );
    assert_eq!(
        harness.state().files.active().coloured_revision,
        coloured,
        "and it was not coloured again"
    );
    assert_ne!(
        harness.state().document().revision(),
        revision,
        "the window still knows it has something new to paint"
    );
}

/// The other half: typing really does change the text, so it is laid out again — but only the
/// paragraph that changed. Every other line keeps the position it already had.
#[test]
fn typing_a_letter_lays_out_the_line_it_was_typed_into_and_leaves_the_rest_alone() {
    let folder = a_folder_with_a_long_source_file("typing");
    let mut harness = harness_in(&folder);
    did(&mut harness, "tab open long.rs --permanent");
    harness.run();
    harness.run();

    let before: Vec<f32> =
        harness.state().layout().lines.iter().map(|line| line.y).collect();
    let count = before.len();
    assert!(count > 2000, "the fixture is meant to be long: {count} lines");

    // Into the middle of the file, so there is plenty above it and plenty below it.
    let middle = harness.state().document().text().len_bytes() / 2;
    harness.state_mut().command(Command::PlaceCaret { offset: middle, extend: false });
    harness.run();
    harness.state_mut().command(Command::Insert("X".to_owned()));
    harness.run();

    let after: Vec<f32> = harness.state().layout().lines.iter().map(|line| line.y).collect();
    assert_eq!(after.len(), count, "a letter typed into a line adds no lines");
    assert_eq!(after, before, "and moves none of them");
    assert!(
        harness.state().document().text().to_string().contains("X"),
        "the letter really was typed"
    );
}

#[test]
fn the_command_line_opens_a_file_into_a_tab() {
    let mut harness = harness_in(&sample_folder());
    let result = did(&mut harness, "tab open readme.md");
    assert!(result["path"].as_str().unwrap().ends_with("readme.md"));
    let app = harness.state();
    assert_eq!(app.files.active().name(), "readme.md");
    assert!(
        app.document().text().to_string().contains('#'),
        "the file's real text should be in the document"
    );
}

#[test]
fn a_file_that_is_not_there_is_refused_with_a_code_a_script_can_match_on() {
    let mut harness = harness_in(&sample_folder());
    assert_eq!(refused(&mut harness, "tab open nowhere.md"), "not-found");
    assert_eq!(refused(&mut harness, "tab show 99"), "not-found");
    assert_eq!(refused(&mut harness, "settings get appearance.font.colour"), "not-found");
    assert_eq!(refused(&mut harness, "editor undo"), "not-applicable");
}

#[test]
fn the_command_line_moves_between_tabs_by_number_and_by_name() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    did(&mut harness, "tab open notes.txt --permanent");
    assert_eq!(harness.state().files.len(), 2);
    did(&mut harness, "tab show readme.md");
    assert_eq!(harness.state().files.active().name(), "readme.md");
    did(&mut harness, "tab next");
    assert_eq!(harness.state().files.active().name(), "notes.txt");
    did(&mut harness, "tab show 0");
    assert_eq!(harness.state().files.active().name(), "readme.md");
}

#[test]
fn the_command_line_types_into_the_document_and_undoes_it() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open notes.txt --permanent");
    let before = harness.state().document().text().to_string();
    did(&mut harness, "editor caret --line 1 --column 1");
    did(&mut harness, "editor insert MARKER");
    assert!(harness.state().document().text().to_string().starts_with("MARKER"));
    did(&mut harness, "editor undo");
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "one undo should put the whole insertion back"
    );
}

#[test]
fn the_caret_lands_where_a_line_and_a_column_say() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open notes.txt --permanent");
    // The sample files are one line each, so the document is given some lines to aim at first.
    did(&mut harness, "editor set-text alpha\\nbravo\\ncharlie");
    let result = did(&mut harness, "editor caret --line 2 --column 3");
    assert_eq!(result["line"], serde_json::json!(2));
    assert_eq!(result["column"], serde_json::json!(3));
    let at = harness.state().caret_position();
    assert_eq!((at.line, at.column), (2, 3), "the status bar should agree");
    // Past the end of a line lands at the end of it rather than being refused.
    let far = did(&mut harness, "editor caret --line 1 --column 99");
    assert_eq!(far["column"], serde_json::json!(6), "the end of `alpha`");
}

#[test]
fn the_command_line_replaces_the_whole_document_in_one_undo_step() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open notes.txt --permanent");
    let before = harness.state().document().text().to_string();
    did(&mut harness, "editor set-text one\\ntwo");
    assert_eq!(harness.state().document().text().to_string(), "one\ntwo");
    did(&mut harness, "editor undo");
    assert_eq!(harness.state().document().text().to_string(), before);
}

#[test]
fn a_view_mode_that_cannot_apply_to_this_file_is_refused_rather_than_silently_ignored() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    did(&mut harness, "editor view preview");
    assert_eq!(harness.state().view_mode(), ViewMode::Preview);
    did(&mut harness, "tab open program.rs --permanent");
    assert_eq!(refused(&mut harness, "editor view preview"), "not-applicable");
    assert_eq!(harness.state().view_mode(), ViewMode::Raw, "and nothing changed");
}

#[test]
fn a_setting_changed_from_the_command_line_reaches_every_open_tab() {
    // The same rule `set_the_font_everywhere` exists for: the editor's font is one setting for the
    // window, so a change from the command line must not reach only the tab that happens to show.
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    did(&mut harness, "tab open notes.txt --permanent");
    did(&mut harness, "settings set appearance.font.size 24");
    assert_eq!(harness.state().settings.font_size, 24.0);
    for file in harness.state().files.iter() {
        assert_eq!(
            file.document.active_style().size,
            24.0,
            "{} was left in the old size",
            file.name()
        );
    }
}

#[test]
fn a_setting_outside_its_limits_is_brought_inside_and_one_that_is_not_a_number_is_refused() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "settings set appearance.background.opacity 9");
    assert_eq!(harness.state().settings.opacity, 1.0, "clamped, not refused");
    assert_eq!(refused(&mut harness, "settings set appearance.font.size huge"), "usage");
    assert_eq!(refused(&mut harness, "settings set editor.line_numbers maybe"), "usage");
}

#[test]
fn the_command_line_puts_the_panes_where_it_is_told() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "explorer hide");
    assert!(!harness.state().explorer_visible);
    did(&mut harness, "explorer show");
    assert!(harness.state().explorer_visible);
    did(&mut harness, "explorer width 400");
    assert_eq!(harness.state().panes.explorer_width, 400.0);
    did(&mut harness, "explorer width 9999");
    assert_eq!(
        harness.state().panes.explorer_width,
        settings::EXPLORER_MAX,
        "a pane dragged past its limit comes back inside, however it was dragged"
    );
}

#[test]
fn the_explorer_filter_and_the_tree_are_readable_and_writable_from_the_command_line() {
    let mut harness = harness_in(&sample_folder());
    let result = did(&mut harness, "explorer filter notes");
    assert!(result["matches"].as_u64().unwrap() >= 1);
    assert_eq!(harness.state().filter, "notes");
    did(&mut harness, "explorer filter");
    assert!(harness.state().filter.is_empty(), "no text clears the box");
    let tree = did(&mut harness, "explorer tree --limit 5");
    assert!(tree["total"].as_u64().unwrap() > 0);
    assert!(tree["rows"].as_array().unwrap().len() <= 5, "the limit is honoured");
}

#[test]
fn go_to_file_is_opened_populated_and_accepted_from_the_command_line() {
    let mut harness = harness_in(&sample_folder());
    let opened = did(&mut harness, "modal open go-to-file --query readme");
    assert!(opened["results"].as_u64().unwrap() >= 1);
    assert!(harness.state().go_to_file.is_some(), "the modal is really open");
    let results = did(&mut harness, "modal results --limit 5");
    assert_eq!(results["results"][0]["name"], serde_json::json!("readme.md"));
    let accepted = did(&mut harness, "modal accept 0");
    assert!(accepted["path"].as_str().unwrap().ends_with("readme.md"));
    assert!(harness.state().go_to_file.is_none(), "accepting shuts it");
    assert_eq!(harness.state().files.active().name(), "readme.md");
}

#[test]
fn every_modal_reports_which_one_is_open_and_shuts_when_it_is_cancelled() {
    let mut harness = harness_in(&sample_folder());
    assert_eq!(did(&mut harness, "modal state")["open"], serde_json::Value::Null);
    for (name, extra) in [("go-to-file", ""), ("settings", ""), ("find-in-files", "")] {
        did(&mut harness, &format!("modal open {name} {extra}"));
        assert_eq!(
            did(&mut harness, "modal state")["open"],
            serde_json::json!(name),
            "{name} should say it is the one that is open"
        );
        did(&mut harness, "modal cancel");
        assert_eq!(did(&mut harness, "modal state")["open"], serde_json::Value::Null);
    }
}

#[test]
fn the_settings_modal_opens_on_the_page_it_is_asked_for() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "modal open settings --page terminal");
    assert!(harness.state().settings_window.open);
    assert_eq!(harness.state().settings_window.page, settings::Page::Terminal);
    // Every page the window has, including the one `task-1679` added: a page reachable by hand and
    // not from the command line would be the thing the rule at the top of `app/cli.rs` forbids.
    for (named, page) in [
        ("appearance", settings::Page::Appearance),
        ("editor", settings::Page::Editor),
        ("plugins", settings::Page::Plugins),
        ("mcp", settings::Page::Mcp),
    ] {
        did(&mut harness, &format!("modal open settings --page {named}"));
        assert_eq!(harness.state().settings_window.page, page, "--page {named}");
    }
    assert_eq!(refused(&mut harness, "modal open settings --page nonsense"), "usage");
}

#[test]
fn the_terminal_is_opened_and_put_away_from_the_command_line() {
    let mut harness = harness_in(&sample_folder());
    // A shell of its own is not started here: what a real shell has said by any given frame is not
    // something a test can know, which is the rule the terminal's own screenshot tests keep. What is
    // under test is that the commands reach the panel.
    harness.state_mut().new_detached_terminal_tab(6, 40);
    harness.run();
    assert!(harness.state().terminal.visible);
    did(&mut harness, "terminal hide");
    assert!(!harness.state().terminal.visible);
    did(&mut harness, "terminal height 400");
    assert_eq!(harness.state().panes.terminal_height, 400.0);
    let listed = did(&mut harness, "terminal list");
    assert_eq!(listed["count"], serde_json::json!(1));
    did(&mut harness, "terminal close");
    assert_eq!(did(&mut harness, "terminal list")["count"], serde_json::json!(0));
}

#[test]
fn every_entry_on_every_menu_is_listed_and_can_be_run_by_name() {
    // The rule `task-1661` asks for, checked against the real menus rather than against a list.
    let mut harness = harness_in(&sample_folder());
    let listed = did(&mut harness, "action list");
    let actions = listed["actions"].as_array().expect("an array").clone();
    assert!(actions.len() > 30, "the menus hold more than that");
    for name in ["toggle-explorer", "toggle-line-numbers", "about", "view-preview", "git-commit"] {
        assert!(
            actions.iter().any(|entry| entry["name"] == serde_json::json!(name)),
            "{name} should be on the list"
        );
    }
    let before = harness.state().settings.line_numbers;
    did(&mut harness, "action run toggle-line-numbers");
    assert_eq!(harness.state().settings.line_numbers, !before);
}

#[test]
fn the_three_actions_that_would_open_a_file_chooser_are_refused_with_the_command_to_use_instead() {
    // A file chooser asked for from a script is a window nobody is looking at.
    let mut harness = harness_in(&sample_folder());
    for (name, instead) in
        [("open-file", "tab open"), ("open-folder", "project open"), ("save-as", "tab save-as")]
    {
        let reply = run(&mut harness, &format!("action run {name}"));
        assert!(!reply.ok, "{name} should be refused");
        assert!(
            reply.message.contains(instead),
            "{name}'s refusal should name `{instead}`, and said: {}",
            reply.message
        );
    }
}

#[test]
fn status_answers_for_every_part_of_the_window_at_once() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    let status = did(&mut harness, "status");
    for part in ["project", "tabs", "editor", "explorer", "terminal", "modal", "settings", "git"] {
        assert!(status.get(part).is_some(), "status should carry {part}");
    }
    assert_eq!(status["tabs"].as_array().unwrap().len(), harness.state().files.len());
    assert!(status["project"].as_str().unwrap().contains("quill-screenshot-folder"));
}

#[test]
fn a_relative_path_is_relative_to_the_project_and_the_reply_says_which_path_it_used() {
    // The one rule about paths, and the reason it is safe: every reply reports the absolute path, so
    // a caller is never guessing about where a file came from or went.
    let mut harness = harness_in(&sample_folder());
    let result = did(&mut harness, "tab open notes.txt");
    let used = result["path"].as_str().expect("a path");
    assert!(std::path::Path::new(used).is_absolute(), "{used} should be absolute");
    assert!(used.starts_with(&sample_folder().to_string_lossy().to_string()));
}

#[test]
fn a_command_line_that_will_not_parse_is_refused_before_anything_happens() {
    let mut harness = harness_in(&sample_folder());
    let before = harness.state().files.active().name();
    assert_eq!(refused(&mut harness, "tab opne readme.md"), "usage");
    assert_eq!(refused(&mut harness, "tab open readme.md --purple"), "usage");
    assert_eq!(refused(&mut harness, "tab open"), "usage");
    assert_eq!(harness.state().files.active().name(), before, "and nothing was opened");
}

/// Send a request the way something other than `quill-cli` sends one: a JSON object down the
/// channel, read by `Request::from_json`.
///
/// The command line is not the only caller and the tests had only been driving it. An agent through
/// the MCP server writes the `arguments` object itself, from the usage lines, which is where the
/// two spellings of a name come from.
fn over_the_wire(
    harness: &mut Harness<'static, QuillApp>,
    command: &str,
    arguments: serde_json::Value,
) -> quill_cli::protocol::Reply {
    let ctx = harness.ctx.clone();
    let request = quill_cli::protocol::Request::from_json(&serde_json::json!({
        "token": "",
        "command": command,
        "arguments": arguments,
    }))
    .expect("a request that parses");
    let reply = harness
        .state_mut()
        .run_cli_for_test(&request, &ctx)
        .unwrap_or_else(|| panic!("`{command}` was not answered on the frame it was asked"));
    harness.run();
    reply
}

/// A flag spelled the way the usage line spells it takes effect, and a name no command has is
/// refused rather than dropped.
///
/// Both halves were real faults found by driving Quill through the MCP tools. `tab open` with
/// `--permanent` left the tab transient, so the next file opened replaced it; `run output` with
/// `--tail` returned the whole screen. Neither said anything, which is what made them expensive —
/// the reply was a success either way.
#[test]
fn a_value_named_the_way_the_usage_line_spells_it_takes_effect() {
    let mut harness = harness_in(&sample_folder());
    let readme = sample_folder().join("readme.md");
    let notes = sample_folder().join("notes.txt");

    let reply = over_the_wire(
        &mut harness,
        "tab.open",
        serde_json::json!({ "path": readme.to_string_lossy(), "--permanent": true }),
    );
    assert!(reply.ok, "{}", reply.message);
    let opened = over_the_wire(&mut harness, "tab.list", serde_json::json!({}));
    let tabs = opened.result["tabs"].as_array().expect("the tabs").clone();
    let readme_tab =
        tabs.iter().find(|tab| tab["name"] == "readme.md").expect("readme.md is open");
    assert_eq!(
        readme_tab["transient"], false,
        "--permanent is the same flag as permanent, so the tab is kept"
    );

    // And the proof that it means something: a transient tab is the one the next file replaces.
    over_the_wire(
        &mut harness,
        "tab.open",
        serde_json::json!({ "path": notes.to_string_lossy() }),
    );
    let after = over_the_wire(&mut harness, "tab.list", serde_json::json!({}));
    let names: Vec<String> = after.result["tabs"]
        .as_array()
        .expect("the tabs")
        .iter()
        .map(|tab| tab["name"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(names.contains(&"readme.md".to_owned()), "the permanent tab survived: {names:?}");
}

#[test]
fn a_value_no_command_has_a_name_for_is_refused_rather_than_dropped() {
    let mut harness = harness_in(&sample_folder());
    let readme = sample_folder().join("readme.md");
    let reply = over_the_wire(
        &mut harness,
        "tab.open",
        serde_json::json!({ "path": readme.to_string_lossy(), "permanant": true }),
    );
    assert!(!reply.ok, "a misspelled name is not a success");
    let failure = reply.error.expect("a refusal carries an error");
    assert_eq!(failure.code, "usage");
    assert!(failure.message.contains("permanant"), "{}", failure.message);
    assert!(
        failure.message.contains("permanent"),
        "the refusal says what the command does take: {}",
        failure.message
    );
}

/// The other half of the same fault, and the worse one: a value that was dropped rather than read
/// returned *more* than was asked for and called it a success. `editor text` is the same shape as the
/// `run output --tail` that found it, and needs no process to prove it.
#[test]
fn a_range_is_read_however_the_names_are_spelled() {
    let mut harness = harness(&format!("{}\n{}\n{}\n{}\n", "one", "two", "three", "four"));
    let dashed = over_the_wire(
        &mut harness,
        "editor.text",
        serde_json::json!({ "--from-line": 2, "--to-line": 3 }),
    );
    let plain = over_the_wire(
        &mut harness,
        "editor.text",
        serde_json::json!({ "from-line": 2, "to-line": 3 }),
    );
    assert_eq!(dashed.result["fromLine"], 2, "the range was read: {}", dashed.result);
    assert_eq!(dashed.result["toLine"], 3);
    assert_eq!(
        dashed.result["text"], plain.result["text"],
        "--from-line 2 and from-line 2 are one request"
    );
    let text = dashed.result["text"].as_str().expect("the text");
    assert!(text.contains("two") && text.contains("three"), "{text:?}");
    assert!(!text.contains("one"), "the whole file is not what was asked for: {text:?}");
}

/// A file changed by something other than Quill is read again before it is answered about.
///
/// Found by driving Quill through the MCP tools while editing the same files from outside: the tab
/// went on showing the version it had read, `editor text` answered with it, and the explorer did not
/// list a file that was plainly on the disk. Both are the same fault — the window is the only writer
/// it knows about — and both are fixed by the rule the symbol index already follows for a closed
/// file: the disk-owned side is re-checked at the moment of use.
#[test]
fn a_file_changed_outside_quill_is_read_again_before_it_is_answered_about() {
    let folder = std::env::temp_dir().join("quill-changed-underneath");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    let path = folder.join("notes.md");
    std::fs::write(&path, "first\n").expect("write notes.md");

    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&path);
    harness.run();
    let first = over_the_wire(&mut harness, "editor.text", serde_json::json!({}));
    assert_eq!(first.result["text"], "first\n");

    // Something else rewrites it. Nothing tells Quill.
    std::fs::write(&path, "second\n").expect("rewrite notes.md");
    let second = over_the_wire(&mut harness, "editor.text", serde_json::json!({}));
    assert_eq!(
        second.result["text"], "second\n",
        "the read is answered from the file rather than from what the tab last held"
    );

    // A file that appears is listed, which is the same fault seen through the explorer.
    let another = folder.join("appeared.md");
    std::fs::write(&another, "new\n").expect("write appeared.md");
    let listed = over_the_wire(&mut harness, "explorer.files", serde_json::json!({}));
    let files: Vec<String> = listed.result["files"]
        .as_array()
        .expect("the files")
        .iter()
        .map(|path| path.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        files.iter().any(|listed| listed.ends_with("appeared.md")),
        "a file written a moment ago is in the project: {files:#?}"
    );
    std::fs::remove_dir_all(&folder).ok();
}

/// Unsaved changes are never thrown away by the re-read. They are the person's, and there is no undo
/// for losing them — `tab reload --discard` is how somebody says they mean it.
#[test]
fn a_tab_with_unsaved_changes_is_not_reread_from_the_file() {
    let folder = std::env::temp_dir().join("quill-changed-underneath-dirty");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    let path = folder.join("notes.md");
    std::fs::write(&path, "first\n").expect("write notes.md");

    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&path);
    harness.run();
    over_the_wire(&mut harness, "editor.insert", serde_json::json!({ "text": "mine " }));
    assert!(harness.state().document().is_modified(), "the tab has unsaved changes");

    std::fs::write(&path, "second\n").expect("rewrite notes.md");
    let read = over_the_wire(&mut harness, "editor.text", serde_json::json!({}));
    let text = read.result["text"].as_str().expect("the text");
    assert!(text.contains("mine "), "the unsaved change is still there: {text:?}");
    assert!(!text.contains("second"), "and the file did not overwrite it: {text:?}");
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn every_command_in_the_catalogue_is_one_the_window_knows() {
    // The catalogue is shared, so the client will accept every command in it. This is the other half:
    // the window must not answer any of them with "there is no such command". Each is run with no
    // arguments at all, so most are refused — what is being checked is *how*.
    let mut harness = harness_in(&sample_folder());
    for command in quill_cli::catalogue::COMMANDS {
        if command.local {
            continue; // answered by the client; the window never sees it
        }
        // The ones that would take the window away from under the rest of the test.
        if matches!(command.wire().as_str(), "quit" | "explorer.reveal" | "window.screenshot") {
            continue;
        }
        let ctx = harness.ctx.clone();
        let request = quill_cli::protocol::Request::new("", &command.wire(), Default::default());
        let reply = match harness.state_mut().run_cli_for_test(&request, &ctx) {
            Some(reply) => reply,
            None => continue, // answered on a later frame, which is an answer
        };
        if let Some(failure) = reply.error {
            assert_ne!(
                failure.code, "unknown-command",
                "the window does not know `{}`, which the catalogue offers",
                command.typed()
            );
        }
        harness.run();
    }
}

// Highlighting a passage (`task-1663`).
//
// The four colour blocks, the drawn colour wheel and the command line that marks passages across as
// many files as you like. The set itself is tested in `quill-core` with no window, and the file
// beside the project in `services::file_marks`; these are for what only the real window can show —
// that the colour is behind the words, that the writing over it is still readable, and that the menu
// looks like the rest of Quill.

/// A passage to mark, and enough text round it that a screenshot shows the mark against the page.
const MARKABLE: &str = "The quick brown fox jumps over the lazy dog.\n\
                        Sphinx of black quartz, judge my vow.\n\
                        Pack my box with five dozen liquor jugs.\n\
                        How vexingly quick daft zebras jump.\n";

/// Open the editing area's own menu where the gutter's menu tests open theirs: by setting the
/// window's state, because the harness cannot press the right mouse button.
fn open_text_menu(harness: &mut Harness<'static, QuillApp>, offset: usize) {
    let at = harness.state().editor_area().left_top() + vec2(120.0, 60.0);
    harness.state_mut().text_menu =
        Some(quill_app::components::text_menu::TextMenu::new(at, offset));
    harness.run();
}

#[test]
fn the_editing_areas_own_menu_holds_four_colours_and_the_wheels_icon() {
    let mut harness = harness(MARKABLE);
    select_phrase(&mut harness, "quick brown fox", &[]);
    open_text_menu(&mut harness, 4);
    for name in ["Highlight yellow", "Highlight green", "Highlight blue", "Highlight pink"] {
        harness.get_by_label(name);
    }
    harness.get_by_label("Choose a colour");
    harness.get_by_label("Copy");
    harness.snapshot(shot("text_menu"));
}

#[test]
fn the_colour_wheel_opens_inside_the_menu_rather_than_in_a_second_popup() {
    // egui keeps one popup open at a time, so the wheel has to be part of this one. What that means
    // for a test is that the menu's own rows are still there while the wheel is showing.
    let mut harness = harness(MARKABLE);
    select_phrase(&mut harness, "quick brown fox", &[]);
    open_text_menu(&mut harness, 4);
    harness.get_by_label("Choose a colour").click();
    harness.run();
    for name in ["Highlight hue", "Highlight shade", "Highlight opacity", "Apply highlight"] {
        harness.get_by_label(name);
    }
    harness.get_by_label("Highlight yellow");
    harness.snapshot(shot("text_menu_wheel"));
}

#[test]
fn choosing_a_colour_marks_the_selection_and_shuts_the_menu() {
    let mut harness = harness(MARKABLE);
    select_phrase(&mut harness, "quick brown fox", &[]);
    open_text_menu(&mut harness, 4);
    harness.get_by_label("Highlight blue").click();
    harness.run();
    assert!(harness.state().text_menu.is_none(), "choosing a colour puts the menu away");
    let marks = harness.state().document().highlights();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks.iter().next().unwrap().color, quill_core::Rgba::new(0x48, 0x9F, 0xF8, 0x59));
}

#[test]
fn three_passages_in_three_colours_are_drawn_behind_the_writing() {
    let mut harness = harness(MARKABLE);
    let ctx = harness.ctx.clone();
    for (phrase, action) in [
        ("quick brown fox", Action::Highlight(HighlightColor::Yellow)),
        ("black quartz", Action::Highlight(HighlightColor::Green)),
        ("five dozen liquor jugs", Action::Highlight(HighlightColor::Pink)),
    ] {
        select_phrase(&mut harness, phrase, &[]);
        harness.state_mut().run_action(action, &ctx);
        harness.run();
    }
    collapse(&mut harness);
    let colours: Vec<String> = harness
        .state()
        .document()
        .highlights()
        .iter()
        .map(|mark| mark.color.to_hex())
        .collect();
    assert_eq!(colours, vec!["#FEBC2E59", "#7FCA9859", "#B4588C59"], "in the order they appear");
    harness.snapshot(shot("highlights"));
}

#[test]
fn clearing_takes_the_one_under_the_caret_and_leaves_the_others_drawn() {
    let mut harness = harness(MARKABLE);
    let ctx = harness.ctx.clone();
    for phrase in ["quick brown fox", "black quartz", "five dozen liquor jugs"] {
        select_phrase(&mut harness, phrase, &[]);
        harness
            .state_mut()
            .run_action(Action::Highlight(HighlightColor::Yellow), &ctx);
        harness.run();
    }
    // The caret inside the second one, as a right click on it would leave it.
    let text = harness.state().document().text().to_string();
    let at = text.find("black quartz").expect("the phrase") + 3;
    harness.state_mut().command(Command::PlaceCaret { offset: at, extend: false });
    harness.state_mut().run_action(Action::ClearHighlight, &ctx);
    harness.run();
    assert_eq!(harness.state().document().highlights().len(), 2);
    harness.snapshot(shot("highlight_cleared"));
}

#[test]
fn a_mark_moves_with_the_text_it_is_on() {
    let mut harness = harness(MARKABLE);
    let ctx = harness.ctx.clone();
    select_phrase(&mut harness, "black quartz", &[]);
    harness.state_mut().run_action(Action::Highlight(HighlightColor::Green), &ctx);
    let before = harness.state().document().highlights().iter().next().unwrap().range.clone();

    // Type a whole line above it.
    harness.state_mut().command(Command::MoveDocumentStart { extend: false });
    harness.state_mut().command(Command::Insert("a new first line\n".to_owned()));
    harness.run();
    let after = harness.state().document().highlights().iter().next().unwrap().range.clone();
    assert_eq!(after.start, before.start + "a new first line\n".len());
    let marked = harness.state().document().text().byte_slice(after.clone());
    assert_eq!(marked, "black quartz", "the mark is still on the words it was put on");

    // And undo puts it back where it was.
    harness.state_mut().command(Command::Undo);
    harness.run();
    assert_eq!(harness.state().document().highlights().iter().next().unwrap().range, before);
}

/// Press the right mouse button at a point in the window.
///
/// The one interaction in Quill that has to be sent as raw events: `kittest` can click a control it
/// can find by name, and the editing area is not a named control — it is the whole surface, and
/// which *point* was pressed is the whole question.
fn right_click_at(harness: &mut Harness<'static, QuillApp>, at: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::default(),
        });
    }
    harness.run();
}

#[test]
fn a_right_click_in_the_writing_opens_the_menu_where_it_was_pressed() {
    let mut harness = harness(MARKABLE);
    collapse(&mut harness);
    // Over the second line, which is where the caret should end up.
    let at = harness.state().editor_area().left_top() + vec2(120.0, 60.0);
    right_click_at(&mut harness, at);
    let menu = harness.state().text_menu.clone().expect("the right click should open the menu");
    assert_eq!(menu.at, at, "the menu opens where the pointer was");
    assert_eq!(
        harness.state().document().selection().head,
        menu.offset,
        "and a right click outside a selection puts the caret where it was pressed"
    );
    assert!(menu.offset > 0, "the pointer was over the writing, not before it");
    harness.get_by_label("Highlight yellow");
}

#[test]
fn a_right_click_inside_a_selection_leaves_the_selection_alone() {
    // Otherwise the menu would open with nothing to mark, which is the whole point of it.
    let mut harness = harness(MARKABLE);
    select_phrase(&mut harness, "The quick brown fox jumps over the lazy dog", &[]);
    let before = harness.state().document().selection();
    let at = harness.state().editor_area().left_top() + vec2(120.0, 20.0);
    right_click_at(&mut harness, at);
    assert!(harness.state().text_menu.is_some());
    assert_eq!(harness.state().document().selection(), before, "the selection is untouched");
    harness.get_by_label("Highlight blue").click();
    harness.run();
    assert_eq!(harness.state().document().highlights().len(), 1);
}

#[test]
fn the_edit_menu_holds_the_four_colours_under_a_highlight_heading() {
    // The four colours are on a menu so that each has an `Action` with a name, which is what puts
    // them on the command line. Inside the window a submenu is drawn as a heading with its rows
    // indented under it, which is what Recent Projects and the explorer's Git submenu already do.
    let mut harness = harness(MARKABLE);
    harness.state_mut().menu_placement = MenuPlacement::InWindow;
    select_phrase(&mut harness, "quick brown fox", &[]);
    harness.get_by_label("Edit").click();
    harness.run();
    for entry in ["Yellow", "Green", "Blue", "Pink", "Clear Highlight", "Clear All Highlights"] {
        harness.get_by_label(entry);
    }
    harness.snapshot(shot("edit_menu"));
}

#[test]
fn the_command_line_marks_a_passage_and_lists_it() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    let marked = did(&mut harness, "highlight add --from-line 1 --to-line 1 --color blue");
    assert_eq!(marked["marked"], 1);
    assert_eq!(marked["color"], "#489FF859");

    let listed = did(&mut harness, "highlight list");
    let rows = listed["highlights"].as_array().expect("a list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["fromLine"], 1);
    assert_eq!(rows[0]["text"], "# Quill");
    assert_eq!(harness.state().document().highlights().len(), 1);
}

#[test]
fn the_command_line_marks_every_occurrence_of_some_words() {
    let folder = copy_out_of_the_repository(&sample_folder(), "quill-highlight-occurrences");
    std::fs::write(folder.join("repeated.txt"), "one two one two one\n").expect("write it");
    let mut harness = harness_in(&folder);
    let marked = did(&mut harness, "highlight add repeated.txt --text one --color pink");
    assert_eq!(marked["marked"], 3, "every occurrence, not the first");
    let listed = did(&mut harness, "highlight list repeated.txt");
    assert_eq!(listed["highlights"].as_array().unwrap().len(), 3);
    assert!(
        harness.state().files.index_of(&folder.join("repeated.txt")).is_none(),
        "the file was never opened, which is the point of naming it"
    );
}

#[test]
fn a_bulk_request_marks_passages_across_several_files_in_one_call() {
    let folder = copy_out_of_the_repository(&sample_folder(), "quill-highlight-bulk");
    let mut harness = harness_in(&folder);
    // Two hashes on the raw string, because a colour of its own is written `"#FF00FF80"` and `"#`
    // would otherwise end the literal in the middle of the request.
    let request = r##"[
        {"path":"readme.md","fromLine":1,"toLine":1,"color":"yellow"},
        {"path":"notes.txt","fromLine":1,"toLine":1,"color":"green"},
        {"path":"chapters/one.md","fromLine":1,"toLine":1,"color":"#FF00FF80"},
        {"path":"nowhere.md","fromLine":1,"toLine":1}
    ]"##
    .split_whitespace()
    .collect::<String>();
    // In single quotes, as it would be typed at a shell: the window splits a command line the same
    // way, so the double quotes inside the JSON survive.
    let result = did(&mut harness, &format!("highlight apply --json-text '{request}'"));
    assert_eq!(result["marked"], 3);
    assert_eq!(result["files"].as_array().unwrap().len(), 3);
    assert_eq!(
        result["refused"].as_array().unwrap().len(),
        1,
        "the file that is not there is refused by number and the rest still go in"
    );

    let everywhere = did(&mut harness, "highlight list --all");
    assert_eq!(everywhere["highlights"].as_array().unwrap().len(), 3);

    // Opening one of them shows the mark that was made while it was closed.
    did(&mut harness, "tab open chapters/one.md");
    let marks = harness.state().document().highlights();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks.iter().next().unwrap().color, quill_core::Rgba::new(0xFF, 0x00, 0xFF, 0x80));

    // And clearing everything really does clear the open file as well as the closed ones.
    let cleared = did(&mut harness, "highlight clear --all");
    assert_eq!(cleared["cleared"], 3);
    assert!(harness.state().document().highlights().is_empty());
    assert_eq!(did(&mut harness, "highlight list --all")["highlights"].as_array().unwrap().len(), 0);
}

#[test]
fn a_colour_that_is_not_a_colour_is_refused_with_the_names_that_are() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    let reply = run(&mut harness, "highlight add --from-line 1 --color puce");
    assert!(!reply.ok);
    assert!(reply.message.contains("yellow"), "the refusal should name the colours: {}", reply.message);
    assert_eq!(refused(&mut harness, "highlight add --from-line 9 --to-line 2"), "usage");
}

#[test]
fn the_four_colours_and_the_two_ways_of_clearing_are_menu_entries_with_names() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    did(&mut harness, "editor select --all");
    for name in ["highlight-yellow", "highlight-green", "highlight-blue", "highlight-pink"] {
        did(&mut harness, &format!("action run {name}"));
    }
    assert_eq!(
        harness.state().document().highlights().len(),
        1,
        "each colour replaces the last over the same passage"
    );
    did(&mut harness, "action run clear-highlights");
    assert!(harness.state().document().highlights().is_empty());
    let listed = did(&mut harness, "action list");
    let names: Vec<String> = listed["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap_or_default().to_owned())
        .collect();
    for name in ["highlight-yellow", "clear-highlight", "clear-highlights"] {
        assert!(names.contains(&name.to_owned()), "`action list` should offer {name}");
    }
}

// Mermaid diagrams (`task-1660`).
//
// One image a diagram type, rendered through the real window and the graphics card. The parsers and
// the layout are tested in `quill-core` with no window at all, where the numbers can be checked by
// hand; these exist for the one thing that cannot be asserted — **what the picture looks like** —
// and every one of them was opened and looked at before it was accepted.
//
// The sources are the files in `sample-diagrams`, which are also what a person opens with
// `cargo run --release`. One set of samples rather than two, so the picture a test renders and the
// picture a person sees come from the same place.

/// The folder of sample diagrams, copied where a test can open them.
///
/// Written once per run behind a `OnceLock`, for the reason `sample_folder` already is: several of
/// these tests want it, they run at the same time, and one of them reading a file another was part
/// way through writing is a failure that has nothing to do with Quill.
fn diagram_folder() -> std::path::PathBuf {
    static FOLDER: OnceLock<std::path::PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let root = std::env::temp_dir().join("quill-mermaid-samples");
            std::fs::create_dir_all(&root).expect("make the folder");
            let from = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../sample-diagrams");
            for entry in std::fs::read_dir(&from).expect("read sample-diagrams").flatten() {
                let path = entry.path();
                if path.is_file() {
                    let name = path.file_name().expect("a name");
                    std::fs::copy(&path, root.join(name)).expect("copy the sample");
                }
            }
            root
        })
        .clone()
}

/// Open one of the sample diagrams, in whichever view mode is wanted.
fn diagram_harness(name: &str, mode: ViewMode) -> Harness<'static, QuillApp> {
    let folder = diagram_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join(name));
    harness.run();
    harness.state_mut().set_view_mode(mode);
    harness.run();
    harness
}

#[test]
fn every_diagram_type_is_drawn_in_the_real_window() {
    // Twenty images, one a diagram type. **Look at them**: this is the test that says an arrowhead
    // points the right way and that nothing overlaps anything, which no assertion about a scene
    // graph can tell you.
    let mut results = SnapshotResults::new();
    for name in [
        "flowchart", "sequence", "class", "state", "er", "requirement", "pie", "gantt", "journey",
        "gitgraph", "mindmap", "timeline", "quadrant", "xychart", "sankey", "block", "packet",
        "kanban", "radar", "treemap",
    ] {
        let mut harness = diagram_harness(&format!("{name}.mmd"), ViewMode::Preview);
        assert!(
            harness.query_by_label(&format!("Diagram: {name}.mmd")).is_some(),
            "{name} should have drawn a diagram"
        );
        results.add(harness.try_snapshot(shot(&format!("mermaid_{name}"))));
    }
    report(results);
}

#[test]
fn a_mermaid_file_gets_the_three_view_modes_named_after_what_it_is() {
    let mut harness = diagram_harness("flowchart.mmd", ViewMode::Raw);
    // The words say Mermaid, not Markdown. A button over a diagram that said `Markdown preview`
    // would be a small wrongness a reader notices at once.
    for name in ["Raw Mermaid", "Side by side", "Mermaid diagram"] {
        assert!(harness.query_by_label(name).is_some(), "{name} should be there");
    }
    assert!(
        harness.query_by_label("Raw Markdown").is_none(),
        "and the Markdown wording should not be"
    );
    // The `F` is absent: a diagram is not prose, so bold and a line spacing mean nothing in it.
    assert!(
        harness.query_by_label("Text options").is_none(),
        "a diagram has no formatting to offer"
    );
    harness.snapshot(shot("mermaid_view_raw"));
}

#[test]
fn the_three_view_mode_buttons_switch_a_mermaid_file_between_the_modes() {
    let mut harness = diagram_harness("pie.mmd", ViewMode::Raw);
    for (name, expected) in [
        ("Side by side", ViewMode::SideBySide),
        ("Mermaid diagram", ViewMode::Preview),
        ("Raw Mermaid", ViewMode::Raw),
    ] {
        harness.get_by_label(name).click();
        harness.run();
        assert_eq!(harness.state().view_mode(), expected, "clicking {name} should switch to it");
    }
}

#[test]
fn side_by_side_shows_a_mermaid_source_and_its_diagram_at_once() {
    let mut harness = diagram_harness("state.mmd", ViewMode::Raw);
    let whole = harness.state().editor_area().width();
    harness.get_by_label("Side by side").click();
    harness.run();
    let half = harness.state().editor_area().width();
    assert!(half < whole, "the source gives up half its width: {whole} then {half}");
    assert!(harness.query_by_label("Diagram: state.mmd").is_some(), "the diagram is drawn beside it");
    harness.snapshot(shot("mermaid_side_by_side"));
}

#[test]
fn a_diagram_that_will_not_parse_says_which_line_rather_than_drawing_nothing() {
    let folder = diagram_folder();
    let broken = folder.join("broken.mmd");
    std::fs::write(&broken, "flowchart LR\n  A --> B\n  C[never closed --> D\n")
        .expect("write the broken sample");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&broken);
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    harness.snapshot(shot("mermaid_problem"));
}

#[test]
fn a_diagram_type_quill_does_not_draw_is_named_rather_than_left_blank() {
    let folder = diagram_folder();
    let path = folder.join("wardley.mmd");
    std::fs::write(&path, "wardley\n  title A value chain\n  anchor Customer [0.9, 0.8]\n")
        .expect("write the sample");
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&path);
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    harness.snapshot(shot("mermaid_not_drawn"));
}

#[test]
fn mermaid_blocks_in_a_markdown_file_are_drawn_in_its_preview() {
    let folder = diagram_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("in-markdown.md"));
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();

    let diagrams = harness.state().preview_diagrams();
    assert_eq!(diagrams.len(), 3, "two that draw and one that cannot");
    assert!(diagrams[0].laid.is_ok(), "the flowchart draws");
    assert!(diagrams[1].laid.is_ok(), "the pie draws");
    assert!(diagrams[2].laid.is_err(), "the one with the unclosed bracket does not");
    // Each one's paragraph was made tall enough to hold it, which is the whole of the two-pass
    // arrangement working.
    for diagram in diagrams {
        assert!(diagram.size.y > 0.0, "a diagram with no height would be invisible");
    }
    // The `rust` fence is still code: only `mermaid` is drawn.
    assert!(
        harness.state().preview_text().contains("still code"),
        "an ordinary code fence keeps its text"
    );
    harness.snapshot(shot("mermaid_in_markdown"));
}

#[test]
fn switching_the_mermaid_plugin_off_withdraws_the_diagrams() {
    // The whole reason this is a plugin rather than a feature: turning it off has to actually take
    // the feature away, in the same frame, in both of the places it appears.
    let folder = diagram_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("in-markdown.md"));
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    assert_eq!(harness.state().preview_diagrams().len(), 3);
    assert!(harness.state().mermaid_is_enabled());

    harness.state_mut().set_plugin_enabled("mermaid", false);
    harness.run();
    assert!(!harness.state().mermaid_is_enabled());
    assert!(
        harness.state().preview_diagrams().is_empty(),
        "with the plugin off, a mermaid fence is code again"
    );

    // And a `.mmd` file says so rather than drawing.
    harness.state_mut().open_path_permanently(&folder.join("pie.mmd"));
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    assert!(
        harness.query_by_label("Diagram: pie.mmd").is_none(),
        "no diagram is drawn while the plugin is off"
    );
    harness.snapshot(shot("mermaid_plugin_off"));
}

#[test]
fn a_diagram_is_laid_out_once_however_many_frames_it_is_drawn_for() {
    // A preview is redrawn sixty times a second, so laying a diagram out on every frame would be
    // sixty layouts a second for a picture that has not changed.
    let folder = diagram_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("flowchart.mmd"));
    harness.state_mut().set_view_mode(ViewMode::Preview);
    harness.run();
    let after_first = harness.state().mermaid_scene_count();
    for _ in 0..5 {
        harness.run();
    }
    assert_eq!(
        harness.state().mermaid_scene_count(),
        after_first,
        "drawing it again should not lay it out again"
    );
    assert_eq!(after_first, 1, "one diagram, one scene");
}

#[test]
fn the_command_line_can_read_what_a_diagram_came_out_as() {
    // `task-1661` asks that every feature be reachable from the command line. A picture cannot be
    // sent down a socket, so what comes back is what it is, how large it came out and every word in
    // it — which is enough for a script to tell that the right diagram was drawn.
    let folder = diagram_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("pie.mmd"));
    harness.run();
    let answer = did(&mut harness, "editor preview");
    assert_eq!(answer["diagram"], "pie", "{answer}");
    assert!(answer["width"].as_f64().unwrap_or(0.0) > 0.0);
    let text = answer["text"].to_string();
    assert!(text.contains("Where the work went"), "it reads the words back: {text}");
}

// -------------------------------------------------------------------------------------- task-1664
//
// The explorer following the tab, and the editing area split into panes.

/// Run an action the way a menu row does.
fn choose(harness: &mut Harness<'static, QuillApp>, action: Action) {
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(action, &ctx);
    harness.run();
}

#[test]
fn the_explorer_opens_out_the_folders_above_the_file_that_is_showing_and_scrolls_to_it() {
    // The file is two folders down and both of them start shut, so before `task-1664` there was no
    // row to select at all. Opening it should open `chapters` and `chapters/appendix` and leave the
    // row on the screen.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("chapters/appendix/tables.txt"));
    harness.run();
    assert_eq!(harness.state().files.active().name(), "tables.txt");
    let open = harness.state().tree.expanded_folders();
    assert!(open.contains(&folder.join("chapters")), "chapters should be open, and {open:?} is");
    assert!(
        open.contains(&folder.join("chapters/appendix")),
        "appendix should be open, and {open:?} is"
    );
    // The row exists and is drawn as the selected one, which is what the picture is of.
    harness.get_by_label("tables.txt");
    harness.snapshot(shot("explorer_follows_the_tab"));
}

#[test]
fn a_folder_shut_by_hand_is_not_opened_again_until_the_tab_changes() {
    // The reveal is a one shot. A person who shut the folder holding the open file shut it on
    // purpose, and a reveal that ran every frame would open it again before the pointer was up.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("chapters/one.md"));
    harness.run();
    assert!(harness.state().tree.expanded_folders().contains(&folder.join("chapters")));
    harness.state_mut().tree.toggle(&folder.join("chapters"));
    harness.run();
    harness.run();
    assert!(
        !harness.state().tree.expanded_folders().contains(&folder.join("chapters")),
        "the folder should have stayed shut"
    );
    // Showing a different file and then this one again is a change, so it is revealed again.
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.run();
    harness.state_mut().open_path_permanently(&folder.join("chapters/one.md"));
    harness.run();
    assert!(
        harness.state().tree.expanded_folders().contains(&folder.join("chapters")),
        "asking for the file again opens the folder again"
    );
}

#[test]
fn splitting_a_tab_puts_two_files_side_by_side() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("program.rs"));
    harness.run();
    assert_eq!(harness.state().files.pane_count(), 1);

    choose(&mut harness, Action::SplitRight);
    assert_eq!(harness.state().files.pane_count(), 2);
    assert_eq!(harness.state().files.focused_pane(), 1);
    // One file in each pane, and each pane has a tab strip of its own.
    assert_eq!(harness.state().files.tabs_in(0).len(), 1);
    assert_eq!(harness.state().files.tabs_in(1).len(), 1);
    assert_eq!(harness.state().files.active().name(), "program.rs");
    harness.snapshot(shot("split_two_panes"));
}

#[test]
fn three_panes_each_show_a_file_of_their_own() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    for name in ["readme.md", "notes.txt", "program.rs"] {
        harness.state_mut().open_path_permanently(&folder.join(name));
    }
    harness.run();
    choose(&mut harness, Action::SplitRight);
    // The second split is on the pane that still holds two tabs, which is the one on the left.
    harness.state_mut().files.focus_pane(0);
    choose(&mut harness, Action::SplitRight);
    assert_eq!(harness.state().files.pane_count(), 3);
    for pane in 0..3 {
        assert_eq!(harness.state().files.tabs_in(pane).len(), 1, "pane {pane}");
    }
    harness.snapshot(shot("split_three_panes"));
}

#[test]
fn a_tabs_own_menu_offers_the_splits() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("program.rs"));
    harness.run();
    // Opened through the window's own state, as the gutter's menu is, because the harness cannot
    // press the right mouse button.
    harness.state_mut().tab_menu = Some((egui::pos2(360.0, 96.0), 0));
    harness.run();
    harness.get_by_label("Split Right");
    harness.get_by_label("Unsplit All");
    harness.snapshot(shot("tab_menu"));
}

#[test]
fn only_the_pane_with_the_keyboard_takes_what_is_typed() {
    // The one fault the pane loop invites: `files.active()` answers with the pane being drawn while
    // it is being drawn, so without the keyboard being passed in separately every pane would take
    // the same key presses and draw a caret.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("notes.txt"));
    harness.run();
    choose(&mut harness, Action::SplitRight);
    let left = harness.state().files.tabs_in(0)[0];
    let before = harness.state().files.at(left).document.text().to_string();

    harness.input_mut().events.push(egui::Event::Text("typed".to_owned()));
    harness.run();
    assert_eq!(
        harness.state().files.at(left).document.text().to_string(),
        before,
        "the pane without the keyboard should not have taken the text"
    );
    assert!(
        harness.state().files.active().document.text().to_string().contains("typed"),
        "the pane with the keyboard should have"
    );
}

#[test]
fn each_pane_lays_its_own_file_out_at_its_own_width() {
    // The reason the ten cache fields moved onto the tab. With one cache on the window the two panes
    // would lay their files out over each other every frame, and neither would be at the right width.
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("program.rs"));
    harness.run();
    choose(&mut harness, Action::SplitRight);
    // Two panes of unequal width, so one cache could not be right for both.
    harness.state_mut().files.set_pane_width(0, 0.3);
    harness.run();
    harness.run();
    let left = harness.state().files.tabs_in(0)[0];
    let right = harness.state().files.tabs_in(1)[0];
    let narrow = harness.state().files.at(left).cached.laid_out_width;
    let wide = harness.state().files.at(right).cached.laid_out_width;
    assert!(narrow > 0.0 && wide > 0.0, "both panes laid their file out: {narrow} and {wide}");
    assert!(wide > narrow, "the wider pane laid out at a greater width: {wide} against {narrow}");
}

#[test]
fn unsplitting_brings_every_tab_back_into_one_pane() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("readme.md"));
    harness.state_mut().open_path_permanently(&folder.join("notes.txt"));
    harness.run();
    choose(&mut harness, Action::SplitRight);
    assert_eq!(harness.state().files.pane_count(), 2);
    choose(&mut harness, Action::UnsplitAll);
    assert_eq!(harness.state().files.pane_count(), 1);
    assert_eq!(harness.state().files.tabs_in(0).len(), 2);
}

#[test]
fn the_command_line_splits_the_editing_area_and_says_what_it_did() {
    let mut harness = harness_in(&sample_folder());
    did(&mut harness, "tab open readme.md --permanent");
    did(&mut harness, "tab open program.rs --permanent");

    let result = did(&mut harness, "pane split");
    assert_eq!(result["count"].as_u64(), Some(2));
    assert_eq!(harness.state().files.pane_count(), 2);

    let result = did(&mut harness, "pane list");
    assert_eq!(result["count"].as_u64(), Some(2), "pane list should say there are two");
    assert_eq!(result["focused"].as_u64(), Some(1));

    // A pane that is not there is refused rather than clamped, so a script is told.
    assert_eq!(refused(&mut harness, "pane focus 9"), "not-found");
    assert_eq!(refused(&mut harness, "pane move sideways"), "usage");

    did(&mut harness, "pane move left");
    assert_eq!(harness.state().files.pane_count(), 1, "the pane it left was emptied");
    assert_eq!(refused(&mut harness, "pane unsplit"), "not-applicable");
}

#[test]
fn the_command_line_scrolls_the_explorer_to_the_file_that_is_showing() {
    let folder = sample_folder();
    let mut harness = harness_in(&folder);
    did(&mut harness, "tab open chapters/appendix/tables.txt --permanent");
    // Shut the folders again and put the explorer away, so the command has something to do.
    harness.state_mut().tree.toggle(&folder.join("chapters"));
    harness.state_mut().explorer_visible = false;
    harness.run();

    did(&mut harness, "explorer select-open-file");
    assert!(harness.state().explorer_visible, "it shows the explorer if it was put away");
    assert!(
        harness.state().tree.expanded_folders().contains(&folder.join("chapters/appendix")),
        "and opens the folders above the file"
    );
}

#[test]
fn a_split_project_opens_split_again() {
    // The whole round trip through `.quill`, on a folder of its own so that no other test's window
    // is reading or writing the same state file.
    let folder = copy_out_of_the_repository(&sample_folder(), "quill-screenshot-split-project");
    {
        let mut harness = harness_in(&folder);
        harness.state_mut().restore_project();
        harness.state_mut().open_path_permanently(&folder.join("readme.md"));
        harness.state_mut().open_path_permanently(&folder.join("program.rs"));
        harness.run();
        let ctx = harness.ctx.clone();
        harness.state_mut().run_action(Action::SplitRight, &ctx);
        // Written on the frame after the change, as every other piece of project state is.
        harness.run();
        harness.run();
        assert_eq!(harness.state().files.pane_count(), 2);
    }
    let mut second = harness_in(&folder);
    second.state_mut().restore_project();
    second.run();
    assert_eq!(second.state().files.pane_count(), 2, "the split should have come back");
    assert_eq!(second.state().files.tabs_in(0).len(), 1);
    assert_eq!(second.state().files.tabs_in(1).len(), 1);
    std::fs::remove_dir_all(&folder).ok();
}

// ---------------------------------------------------------------------------------------------
// Go to definition, find references and rename (`task-1675`).
//
// The pictures are what say the modal is grouped by file, that a textual match is second-class
// rather than hidden, and that the underline appears under the word the modifier is over. The
// behaviour underneath them is tested with no window in `quill_core::symbols`, `app::symbols` and
// `components::references`; what is here is what only a real window can show.

/// A small project written in a language that says what a definition is.
///
/// Built once behind a `OnceLock` for the reason `sample_folder` is: several tests want it, they run
/// at the same time, and a fixture rewritten under a test that is reading it is a failure that looks
/// like a fault in the code.
fn code_folder() -> std::path::PathBuf {
    static FOLDER: OnceLock<std::path::PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let root = std::env::temp_dir().join("quill-screenshot-code");
            std::fs::create_dir_all(&root).expect("make the code folder");
            std::fs::write(
                root.join("layout.rs"),
                "//! Laying a document out.\n\npub struct Layout;\n\nimpl Layout {\n    pub fn new() -> Self {\n        Layout\n    }\n\n    /// Draw the whole of it.\n    pub fn draw(&self) {\n        let label = \"draw\";\n        let _ = label;\n    }\n}\n",
            )
            .expect("write layout.rs");
            std::fs::write(
                root.join("caret.rs"),
                "//! The caret, and how it is drawn.\n\npub struct Caret;\n\nimpl Caret {\n    pub fn new() -> Self {\n        Caret\n    }\n\n    // draw the caret over the text\n    pub fn paint(&self, layout: &Layout) {\n        layout.draw();\n        layout.draw();\n    }\n}\n",
            )
            .expect("write caret.rs");
            std::fs::write(
                root.join("panel.rs"),
                "pub struct Panel;\n\nimpl Panel {\n    pub fn show(&self, layout: &Layout) {\n        layout.draw();\n    }\n}\n",
            )
            .expect("write panel.rs");
            std::fs::write(root.join("notes.md"), "# Notes\n\nA note that mentions draw.\n")
                .expect("write notes.md");
            root
        })
        .clone()
}

/// A window on the code folder, with its definitions index built and the named file open.
fn code_harness(open: &str) -> Harness<'static, QuillApp> {
    let folder = code_folder();
    let mut harness = harness_in(&folder);
    let path = folder.join(open);
    harness.state_mut().open_path_permanently(&path);
    // The index is read on a thread, exactly as git and the text search are, so the harness is run
    // until the answer arrives. Each run is a frame; nothing here waits on a clock.
    for _ in 0..600 {
        pump(&mut harness);
        let ready = harness
            .state()
            .symbols_indexer()
            .is_some_and(|indexer| !indexer.is_building() && !indexer.index().is_empty());
        if ready {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run();
    harness
}

/// Put the caret on the first `needle` in the file that is showing, a little way into the word.
fn caret_on(harness: &mut Harness<'static, QuillApp>, needle: &str, into: usize) -> usize {
    let text = harness.state().document().text().to_string();
    let at = text.find(needle).unwrap_or_else(|| panic!("{needle} is not in this file")) + into;
    harness.state_mut().command(Command::PlaceCaret { offset: at, extend: false });
    harness.run();
    at
}

/// Wait for the references modal's own search to finish.
fn settle_the_references(harness: &mut Harness<'static, QuillApp>) {
    for _ in 0..400 {
        let searching =
            harness.state().references.as_ref().is_some_and(quill_app::components::references::References::is_searching);
        if !searching {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    harness.run();
}

/// What the references modal found, as `name:line · role` for each row.
fn references(harness: &Harness<'static, QuillApp>) -> Vec<String> {
    harness
        .state()
        .references
        .as_ref()
        .expect("the modal should be open")
        .hits()
        .iter()
        .map(|hit| {
            format!(
                "{}:{}{}",
                hit.path.file_name().unwrap().to_string_lossy(),
                hit.line,
                match hit.role {
                    quill_core::symbols::Role::Code => String::new(),
                    other => format!(" \u{00B7} {}", other.suffix()),
                }
            )
        })
        .collect()
}

#[test]
fn go_to_definition_jumps_to_the_definition_and_selects_its_name() {
    // Scenario 1, through the real window: the caret is on a call, and the definition is what ends
    // up selected.
    let mut harness = code_harness("caret.rs");
    let ctx = harness.ctx.clone();
    caret_on(&mut harness, "layout.draw()", "layout.".len() + 1);
    harness.state_mut().run_action(Action::GoToDefinition, &ctx);
    harness.run();
    assert_eq!(harness.state().files.active().name(), "layout.rs");
    assert_eq!(harness.state().document().selected_text(), "draw");
}

#[test]
fn the_modifier_underlines_the_word_it_would_go_to_and_nothing_else() {
    // Scenario 7. The affordance is resolution-driven: only a word that really has somewhere to go
    // is underlined, so the promise it makes is one the click can keep.
    let mut harness = code_harness("caret.rs");
    let text = harness.state().document().text().to_string();
    let call = text.find("layout.draw()").expect("the call") + "layout.".len() + 1;
    // A word nothing defines: `layout` is a parameter, and a parameter has no definer keyword in
    // front of it, so the mechanism honestly knows nothing about where it comes from.
    let unknown = text.find("layout.draw()").expect("the call") + 1;

    assert!(
        harness.state_mut().resolve_under_the_pointer(call).is_some(),
        "`draw` is defined in layout.rs, so it resolves"
    );
    harness.state_mut().forget_the_hover();
    assert!(
        harness.state_mut().resolve_under_the_pointer(unknown).is_none(),
        "`layout` is a parameter, so nothing is underlined and a click places the caret"
    );
    // A definition of its own still resolves, because the click there means something: it pivots
    // to the references, which is scenario 8.
    harness.state_mut().forget_the_hover();
    let definition = text.find("fn paint").expect("paint") + 4;
    let hover = harness
        .state_mut()
        .resolve_under_the_pointer(definition)
        .expect("its own definition resolves");
    assert!(hover.at_definition, "and the window knows it is standing on it");
    // And a keyword is not a question about a symbol at all.
    harness.state_mut().forget_the_hover();
    let keyword = text.find("-> Self").expect("Self") + 4;
    assert!(harness.state_mut().resolve_under_the_pointer(keyword).is_none());
}

#[test]
fn asking_from_the_definition_opens_the_references_instead() {
    // Scenario 8, and the picture of the modal the ticket describes: the results grouped by file
    // with a count on each heading, and the file the chosen reference is in shown underneath,
    // scrolled to it with the reference picked out.
    let mut harness = code_harness("layout.rs");
    let ctx = harness.ctx.clone();
    caret_on(&mut harness, "fn draw", 4);
    harness.state_mut().run_action(Action::GoToDefinition, &ctx);
    settle_the_references(&mut harness);
    let found = references(&harness);
    assert!(found.contains(&"layout.rs:11".to_owned()), "the definition itself: {found:?}");
    assert!(found.contains(&"caret.rs:12".to_owned()), "and the calls: {found:?}");
    assert!(
        found.iter().any(|row| row.ends_with("comment")),
        "the mention in a comment is listed, second-class: {found:?}"
    );
    assert!(
        found.iter().any(|row| row.ends_with("string")),
        "and so is the one inside a string: {found:?}"
    );
    // The headings and one row of each kind name themselves, which is how a test finds them.
    harness.get_by_label(&references_heading("layout.rs"));
    harness.get_by_label("Reference layout.rs:11");
    harness.snapshot(shot("references"));
}

/// The label the references modal gives a file's heading.
///
/// The modal names the file the way the platform spells a path, so the label has a backslash in it on
/// Windows and a slash everywhere else. Written down twice as a literal, these two tests passed on
/// Windows and failed on macOS for a reason that has nothing to do with what they are testing.
fn references_heading(file: &str) -> String {
    let path = std::path::Path::new("quill-screenshot-code").join(file);
    format!("References in {}", path.display())
}

#[test]
fn choosing_a_file_heading_shows_that_files_first_reference() {
    // Scenario 21, which is the ticket's own sentence: *a modal that has the file path, then under
    // that scrolled to the first reference in that file*.
    let mut harness = code_harness("layout.rs");
    let ctx = harness.ctx.clone();
    caret_on(&mut harness, "fn draw", 4);
    harness.state_mut().run_action(Action::FindReferences, &ctx);
    settle_the_references(&mut harness);
    harness.get_by_label(&references_heading("caret.rs")).click();
    harness.run();
    harness.run();
    let (path, line) = harness
        .state()
        .references
        .as_ref()
        .expect("the modal")
        .scrolled_to()
        .expect("somewhere");
    assert_eq!(path.file_name().unwrap(), "caret.rs");
    // The first reference *in the list*, which within a file is the first code one: the textual
    // matches are listed after them, so a heading previews the answer rather than a mention of it.
    assert_eq!(line, 12, "the call on line 12, not the comment above it on line 10");
}

#[test]
fn opening_a_reference_selects_it_in_the_document() {
    // Scenario 30. The same contract `Find in Files` has: enter or a double click opens it and the
    // modal closes.
    let mut harness = code_harness("layout.rs");
    let ctx = harness.ctx.clone();
    caret_on(&mut harness, "fn draw", 4);
    harness.state_mut().run_action(Action::FindReferences, &ctx);
    settle_the_references(&mut harness);
    double_click(&mut harness, "Reference caret.rs:12");
    assert!(harness.state().references.is_none(), "opening a reference shuts the modal");
    assert_eq!(harness.state().files.active().name(), "caret.rs");
    assert_eq!(harness.state().document().selected_text(), "draw");
}

#[test]
fn the_rename_modal_is_the_preview_and_the_ticks_are_the_change_set() {
    // Scenarios 34, 38 and 39 in one picture: the field pre-filled, a tick on every row, the
    // project-wide default for a function, and the footer saying what is wrong with a bad name.
    let mut harness = code_harness("layout.rs");
    let ctx = harness.ctx.clone();
    caret_on(&mut harness, "fn draw", 4);
    harness.state_mut().run_action(Action::RenameSymbol, &ctx);
    settle_the_references(&mut harness);
    let modal = harness.state().references.as_ref().expect("the rename modal");
    assert_eq!(modal.new_name, "draw", "the field starts as the name it is about");
    let ticked: Vec<bool> = modal.ticks().to_vec();
    let roles: Vec<quill_core::symbols::Role> =
        modal.hits().iter().map(|hit| hit.role).collect();
    for (ticked, role) in ticked.iter().zip(&roles) {
        match role {
            quill_core::symbols::Role::Code => {
                assert!(ticked, "a function is renamed across the project by default")
            }
            _ => assert!(!ticked, "a comment or a string is never ticked by default"),
        }
    }
    harness.get_by_label("New name");
    harness.snapshot(shot("rename_symbol"));

    // A name this language could not hold is refused, with the reason in the footer.
    harness.state_mut().references.as_mut().expect("the modal").new_name = "match".to_owned();
    harness.run();
    let refusal = harness
        .state()
        .references
        .as_ref()
        .expect("the modal")
        .refusal
        .clone()
        .expect("a refusal");
    assert!(refusal.contains("keyword"), "{refusal}");

    // And a collision is a warning rather than a refusal, because the mechanism cannot know whether
    // it shadows — that is semantic — so it says what it does know.
    harness.state_mut().references.as_mut().expect("the modal").new_name = "new".to_owned();
    harness.run();
    let modal = harness.state().references.as_ref().expect("the modal");
    assert!(modal.refusal.is_none(), "a collision does not stop it");
    assert!(
        modal.warning.as_deref().is_some_and(|said| said.contains("already defined")),
        "{:?}",
        modal.warning
    );
}

#[test]
fn a_file_whose_language_says_nothing_has_none_of_the_three_entries() {
    // Scenario 17 through the menu the window really builds, and the reason a control that can
    // never apply is absent rather than dimmed.
    let harness = code_harness("notes.md");
    let names: Vec<String> = quill_app::app::actions::menus(&harness.state().menu_state())
        .iter()
        .find(|menu| menu.name == "Edit")
        .expect("the Edit menu")
        .entries
        .iter()
        .filter_map(|entry| match entry {
            quill_app::app::actions::Entry::Item { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    for absent in ["Go to Definition", "Find References", "Rename Symbol..."] {
        assert!(!names.contains(&absent.to_owned()), "{absent} should be absent: {names:?}");
    }
    assert!(names.contains(&"Navigate Back".to_owned()), "the history is about the window");
    // And a source file has all three.
    let mut harness = code_harness("layout.rs");
    let state = harness.state().menu_state();
    assert!(state.definitions_apply && state.symbols_apply);
    harness.run();
}

/// Where on the screen a byte of the file that is showing is drawn.
///
/// The same arithmetic `show_editor` does: the editing area's own rectangle — which is what is left
/// after the gutter has taken its column — plus the padding, less how far the file is scrolled.
fn point_of(harness: &Harness<'static, QuillApp>, offset: usize) -> egui::Pos2 {
    let area = harness.state().editor_area();
    let caret = harness.state().layout().caret_at(offset);
    let scroll = harness.state().files.active().scroll;
    egui::pos2(
        area.left() + quill_app::components::editor_view::PADDING + caret.x + 1.0,
        area.top() + quill_app::theme::size::EDITOR_PADDING_Y - scroll + caret.y
            + caret.height / 2.0,
    )
}

/// Move the pointer over a byte of the file, with the platform's modifier held or not.
///
/// `Event::ModifiersChanged` is how the modifier is said to be held: egui carries the state of the
/// modifier keys on that event rather than on the pointer's, which is what a real window sends when
/// the key goes down with the pointer already where it is — and that, rather than a click, is the
/// whole gesture up to the moment of the click.
fn hover_over(harness: &mut Harness<'static, QuillApp>, offset: usize, modifier: bool) {
    let at = point_of(harness, offset);
    let held = if modifier { Modifiers::COMMAND } else { Modifiers::NONE };
    harness.input_mut().events.push(egui::Event::ModifiersChanged(held));
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run();
}

#[test]
fn the_underline_appears_under_the_word_the_modifier_is_over_and_goes_with_it() {
    // Scenario 7 as a picture. The affordance is what the whole gesture rests on: a word that is
    // underlined is a word the click will take you somewhere from, and one that is not is a word an
    // ordinary click will put the caret in.
    let mut harness = code_harness("caret.rs");
    let text = harness.state().document().text().to_string();
    let call = text.find("layout.draw()").expect("the call") + "layout.".len() + 1;

    // The pointer over the word with nothing held: no underline, and the ordinary writing bar.
    hover_over(&mut harness, call, false);
    let plain = harness.render().expect("render the window");

    // The same point with the modifier held: the word is underlined.
    hover_over(&mut harness, call, true);
    let underlined = harness.render().expect("render the window");
    assert_ne!(
        plain.as_raw(),
        underlined.as_raw(),
        "holding the modifier over a word that resolves has to change what is drawn"
    );
    harness.snapshot(shot("go_to_definition_underline"));

    // Letting go of it takes the underline away again, which is what stops an affordance outliving
    // the gesture that asked for it.
    hover_over(&mut harness, call, false);
    let released = harness.render().expect("render the window");
    assert_eq!(
        plain.as_raw(),
        released.as_raw(),
        "letting go of the modifier puts the window back exactly as it was"
    );
}

/// Press and release the primary button where the pointer is, with the modifier held.
fn modifier_click(harness: &mut Harness<'static, QuillApp>, offset: usize) {
    let at = point_of(harness, offset);
    harness.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.run();
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::COMMAND,
        });
    }
    harness.run();
}

#[test]
fn a_modifier_click_on_a_word_goes_to_its_definition_and_an_ordinary_one_places_the_caret() {
    // Scenario 1's click half, and scenario 5's: the gesture is what a person really does, and the
    // same click without the modifier has to keep meaning what it always meant.
    let mut harness = code_harness("caret.rs");
    let text = harness.state().document().text().to_string();
    let call = text.find("layout.draw()").expect("the call") + "layout.".len() + 1;

    // Without the modifier it is an ordinary click: the caret lands in the word and nothing opens.
    hover_over(&mut harness, call, false);
    let at = point_of(&harness, call);
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    harness.run();
    assert_eq!(harness.state().files.active().name(), "caret.rs", "nothing was opened");
    assert!(
        harness.state().document().selection().is_empty(),
        "and a click places a caret rather than selecting anything"
    );

    // With it held, the same click goes to the definition and selects the name it landed on.
    modifier_click(&mut harness, call);
    assert_eq!(harness.state().files.active().name(), "layout.rs");
    assert_eq!(harness.state().document().selected_text(), "draw");
}

#[test]
fn a_modifier_click_on_the_definition_itself_opens_the_references() {
    // Scenario 8 through the gesture rather than through the menu: one gesture serves both
    // directions of the question, which is what IntelliJ calls "Go to Declaration or Usages".
    let mut harness = code_harness("layout.rs");
    let text = harness.state().document().text().to_string();
    let definition = text.find("fn draw").expect("the definition") + 4;
    modifier_click(&mut harness, definition);
    settle_the_references(&mut harness);
    let modal = harness.state().references.as_ref().expect("the references opened");
    assert_eq!(modal.name, "draw");
    assert!(!modal.hits().is_empty());
}

// Import completion (`task-1680`).
//
// Two pictures, and they are what say that a list hanging inside a pair of quotes is a list of the
// project's own files, and that a list between a pair of braces is a list of what one module
// exports with the glyph for what each thing is. Everything underneath is tested with no window in
// `quill_core::imports`, `services::imports` and `app::completion`.

/// A small TypeScript project for the import pictures.
///
/// Its own folder, for the reason [`completion_folder`] has its own: the explorer draws whatever is
/// in the folder, so adding a file to a fixture another test has already accepted a picture of would
/// change that picture for a reason that is not a change to Quill.
fn import_folder() -> std::path::PathBuf {
    static FOLDER: OnceLock<std::path::PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let root = std::env::temp_dir().join("quill-screenshot-imports");
            std::fs::create_dir_all(root.join("src/app/widgets")).expect("make the folders");
            std::fs::create_dir_all(root.join("src/core")).expect("make the core folder");
            std::fs::write(root.join("src/app/main.ts"), "").expect("write main.ts");
            std::fs::write(
                root.join("src/app/layout.ts"),
                "export class Layout {}\n\
                 \n\
                 export interface Placed {}\n\
                 \n\
                 export const LINE_HEIGHT = 18;\n\
                 \n\
                 export function drawFrame() {\n\
                 \x20   const hidden = 1;\n\
                 \x20   return hidden;\n\
                 }\n\
                 \n\
                 export function drawGutter() {}\n\
                 \n\
                 export function drawCaret() {}\n\
                 \n\
                 const secret = 2;\n",
            )
            .expect("write layout.ts");
            std::fs::write(root.join("src/app/caret.ts"), "export class Caret {}\n")
                .expect("write caret.ts");
            std::fs::write(root.join("src/app/widgets/index.ts"), "export class Button {}\n")
                .expect("write index.ts");
            std::fs::write(root.join("src/app/widgets/scrollbar.ts"), "export class Bar {}\n")
                .expect("write scrollbar.ts");
            std::fs::write(root.join("src/core/completion.ts"), "export function rank() {}\n")
                .expect("write completion.ts");
            std::fs::write(root.join("src/core/document.ts"), "export class Document {}\n")
                .expect("write document.ts");
            std::fs::write(root.join("readme.md"), "# a project\n").expect("write readme.md");
            root
        })
        .clone()
}

/// A window on it, with its index built and `src/app/main.ts` open and empty.
fn import_harness() -> Harness<'static, QuillApp> {
    let folder = import_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("src/app/main.ts"));
    for _ in 0..600 {
        pump(&mut harness);
        let ready = harness
            .state()
            .symbols_indexer()
            .is_some_and(|indexer| !indexer.is_building() && !indexer.index().is_empty());
        if ready {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run();
    harness
}

#[test]
fn typing_a_module_specifier_offers_the_projects_own_files() {
    // Scenarios 2 and 41: the quotes open and the list is the project, written as the specifier
    // that would reach each file from this one.
    let mut harness = import_harness();
    type_letters(&mut harness, "import { Layout } from '");
    let offered = completions(&harness);
    assert!(offered.contains(&"./layout".to_owned()), "{offered:?}");
    assert!(offered.contains(&"./widgets".to_owned()), "{offered:?}");
    assert!(offered.contains(&"../core/completion".to_owned()), "{offered:?}");
    assert!(!offered.iter().any(|row| row.ends_with(".ts")), "the extension is dropped");
    harness.snapshot(shot("completion_import_specifier"));
}

#[test]
fn a_name_typed_between_the_braces_offers_what_that_module_exports() {
    // Scenarios 11 and 44: the module is written after the caret, and only what `export` marks is
    // offered — `hidden` and `secret` are in the file and are not something another file can name.
    let mut harness = import_harness();
    let line = "import { draw } from './layout'";
    harness.state_mut().command(Command::Insert(line.to_owned()));
    let caret = line.find("draw").expect("the sample") + 4;
    harness.state_mut().command(Command::PlaceCaret { offset: caret, extend: false });
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::CompleteWord, &ctx);
    harness.run();
    let offered = completions(&harness);
    assert_eq!(
        offered,
        vec!["drawCaret".to_owned(), "drawFrame".to_owned(), "drawGutter".to_owned()],
        "only the exports, best first"
    );
    harness.snapshot(shot("completion_import_named"));
}

// Auto-complete (`task-1677`).
//
// The pictures are what say the list hangs under the caret, that the matched letters are picked out,
// that each row carries the glyph for what it is and a quiet word saying where it came from, and
// that the list flips above the caret rather than running off the bottom of the pane. Everything
// underneath is tested with no window in `quill_core::completion` and `app::completion`; what is
// here is what only a real window can show.

/// A small project for the completion pictures.
///
/// Separate from [`code_folder`] on purpose: the explorer draws whatever is in the folder, so adding
/// a file to a fixture another test has already accepted a picture of would change that picture for
/// a reason that is not a change to Quill.
///
/// It is built so that one stem, `dra`, offers ten rows covering all five kinds a definition can be
/// — a function, a variable, a module, a type and a constant — and more rows than the eight the list
/// draws, which is what the scrolling picture needs. `distant.rs` is never opened, so what it
/// defines can only have come from the project's index.
fn completion_folder() -> std::path::PathBuf {
    static FOLDER: OnceLock<std::path::PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let root = std::env::temp_dir().join("quill-screenshot-completion");
            std::fs::create_dir_all(&root).expect("make the completion folder");
            std::fs::write(
                root.join("layout.rs"),
                "//! Laying a document out.\n\
                 \n\
                 pub struct Layout;\n\
                 \n\
                 const DRAW_LIMIT: usize = 8;\n\
                 \n\
                 impl Layout {\n\
                 \x20   pub fn new() -> Self {\n\
                 \x20       let drawn = 0;\n\
                 \x20       let _ = drawn;\n\
                 \x20       Layout\n\
                 \x20   }\n\
                 \n\
                 \x20   /// Draw the whole of it.\n\
                 \x20   pub fn draw(&self) {}\n\
                 \n\
                 \x20   pub fn draw_frame(&self) {}\n\
                 \n\
                 \x20   pub fn draw_gutter(&self) {}\n\
                 \n\
                 \x20   pub fn draw_caret(&self) {}\n\
                 \n\
                 \x20   pub fn redraw(&self) {}\n\
                 \n\
                 \x20   pub fn paint_text(&self) {}\n\
                 }\n\
                 \n\
                 pub mod parts {\n\
                 \x20   pub fn first() {}\n\
                 \n\
                 \x20   pub fn second() {}\n\
                 \n\
                 \x20   pub fn third() {}\n\
                 \n\
                 \x20   pub fn fourth() {}\n\
                 \n\
                 \x20   pub fn fifth() {}\n\
                 \n\
                 \x20   pub fn sixth() {}\n\
                 }\n",
            )
            .expect("write layout.rs");
            std::fs::write(
                root.join("caret.rs"),
                "pub struct Caret;\n\nimpl Caret {\n    pub fn new() -> Self {\n        Caret\n    }\n\n    pub fn paint(&self, layout: &Layout) {\n        layout.draw();\n    }\n}\n",
            )
            .expect("write caret.rs");
            std::fs::write(
                root.join("distant.rs"),
                "pub struct Drawing;\n\npub mod drawings {}\n\npub fn draw_everything() {}\n",
            )
            .expect("write distant.rs");
            std::fs::write(root.join("notes.md"), "# Notes\n\nA note about drawing.\n")
                .expect("write notes.md");
            root
        })
        .clone()
}

/// A window on the completion folder, with its index built and the named file open.
fn completion_harness(open: &str) -> Harness<'static, QuillApp> {
    let folder = completion_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join(open));
    // The index is read on a thread, so the harness is run until the answer arrives. Each run is a
    // frame; nothing here waits on a clock.
    for _ in 0..600 {
        pump(&mut harness);
        let ready = harness
            .state()
            .symbols_indexer()
            .is_some_and(|indexer| !indexer.is_building() && !indexer.index().is_empty());
        if ready {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run();
    harness
}

/// Type `text` a letter at a time, as real text events, which is the path the released binary takes
/// and the only one that fires the automatic trigger.
fn type_letters(harness: &mut Harness<'static, QuillApp>, text: &str) {
    for letter in text.chars() {
        harness.input_mut().events.push(egui::Event::Text(letter.to_string()));
        harness.run();
    }
}

/// The names on offer, in the order the popup is showing them.
fn completions(harness: &Harness<'static, QuillApp>) -> Vec<String> {
    harness
        .state()
        .completion()
        .map(|state| state.rows.iter().map(|row| row.name.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn typing_a_word_offers_the_names_it_could_become() {
    // Scenarios 12 and 33: the list opens on the second character, hangs under the caret, and each
    // row carries its matched letters in the accent colour, a glyph for what it is, and a quiet word
    // saying where it came from.
    let mut harness = completion_harness("layout.rs");
    // On the blank line under the struct, so the list has room to hang below the caret.
    let text = harness.state().document().text().to_string();
    let blank = text.find("\nconst DRAW_LIMIT").expect("the blank line above the constant");
    harness.state_mut().command(Command::PlaceCaret { offset: blank, extend: false });
    harness.run();

    type_letters(&mut harness, "d");
    assert!(harness.state().completion().is_none(), "one character is noise, not an offer");
    type_letters(&mut harness, "ra");

    let offered = completions(&harness);
    assert_eq!(
        offered,
        [
            "draw",
            "drawn",
            "drawings",
            "Drawing",
            "draw_caret",
            "draw_frame",
            "draw_gutter",
            "draw_everything",
            "DRAW_LIMIT",
            "redraw",
        ],
        "the order the rubric gives"
    );
    assert_eq!(harness.state().completion().expect("open").chosen, 0, "the best row is pre-chosen");
    // Every row names itself, which is what makes it findable at all.
    harness.get_by_label("Completion draw");
    harness.get_by_label("Completion Drawing");
    // The list is drawn under the caret's own line.
    let anchor = harness.state().completion_anchor().expect("the popup was drawn");
    assert!(anchor.pane.contains_rect(
        quill_app::components::completion::where_it_goes(8, anchor.caret, anchor.pane)
    ));
    harness.snapshot(shot("completion_list"));
}

#[test]
fn the_list_flips_above_the_caret_at_the_bottom_of_the_pane() {
    // Scenario 22. Under the word is where the eye already is, so it only ever flips when the rows
    // would cross the bottom of the pane.
    let mut harness = completion_harness("layout.rs");
    let end = harness.state().document().text().len_bytes();
    harness.state_mut().command(Command::PlaceCaret { offset: end, extend: false });
    harness.run();
    type_letters(&mut harness, "dra");
    assert!(harness.state().completion().is_some(), "{:?}", completions(&harness));
    let anchor = harness.state().completion_anchor().expect("the popup was drawn");
    let rows = harness.state().completion().expect("open").shown().len();
    let area = quill_app::components::completion::where_it_goes(rows, anchor.caret, anchor.pane);
    assert!(
        area.bottom() <= anchor.caret.top(),
        "the caret is at the bottom of the pane, so the list belongs above it: {area:?} against {:?}",
        anchor.caret
    );
    assert!(anchor.pane.contains_rect(area), "and all of it is still on the screen");
    harness.snapshot(shot("completion_above_the_caret"));
}

#[test]
fn walking_past_the_eighth_row_scrolls_the_list() {
    // Scenario 25's second half: eight rows are drawn and the pill drags the rest into view.
    let mut harness = completion_harness("layout.rs");
    let text = harness.state().document().text().to_string();
    let blank = text.find("\nconst DRAW_LIMIT").expect("the blank line");
    harness.state_mut().command(Command::PlaceCaret { offset: blank, extend: false });
    harness.run();
    type_letters(&mut harness, "dra");
    assert!(completions(&harness).len() > 8, "{:?}", completions(&harness));
    for _ in 0..9 {
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
    }
    let state = harness.state().completion().expect("open");
    assert_eq!(state.chosen, 9, "the tenth row, and no further: the ends are clamped");
    assert!(state.scroll > 0, "so the list scrolled to reach it");
    assert!(state.shown().contains(&state.chosen));
    // The caret has not moved: the arrows were consumed before the editing area read them.
    assert!(
        harness.state().document().text().to_string().contains("dra\nconst DRAW_LIMIT"),
        "and nothing was typed by the arrows"
    );
    harness.snapshot(shot("completion_scrolled"));
}

#[test]
fn clicking_a_row_takes_it_and_the_click_never_reaches_the_document() {
    // Scenario 30. The list's own `Area` is in front of the editing area, so the click lands on the
    // row rather than placing a caret behind it.
    let mut harness = completion_harness("layout.rs");
    let text = harness.state().document().text().to_string();
    let blank = text.find("\nconst DRAW_LIMIT").expect("the blank line");
    harness.state_mut().command(Command::PlaceCaret { offset: blank, extend: false });
    harness.run();
    type_letters(&mut harness, "dra");
    harness.get_by_label("Completion draw_gutter").click();
    harness.run();
    assert!(harness.state().completion().is_none(), "taking a row closes the list");
    let after = harness.state().document().text().to_string();
    assert!(after.contains("draw_gutter\nconst DRAW_LIMIT"), "{after:?}");
    assert!(
        harness.state().document().selection().is_empty(),
        "and the click placed no caret in the document behind the list"
    );
}

#[test]
fn tab_takes_the_best_row_and_the_editing_area_never_sees_the_key() {
    // Scenarios 26 and 28 through the real window: `Tab` is the gesture the ticket names, and while
    // the list is open it must not also type a tab into the file.
    let mut harness = completion_harness("layout.rs");
    let text = harness.state().document().text().to_string();
    let blank = text.find("\nconst DRAW_LIMIT").expect("the blank line");
    harness.state_mut().command(Command::PlaceCaret { offset: blank, extend: false });
    harness.run();
    type_letters(&mut harness, "dra");
    harness.key_press(egui::Key::Tab);
    harness.run();
    let after = harness.state().document().text().to_string();
    assert!(after.contains("draw\nconst DRAW_LIMIT"), "{after:?}");
    assert!(!after.contains('\t'), "no tab was typed into the file");
    assert!(harness.state().completion().is_none());
    // And with the list shut, `Tab` means what it always meant.
    harness.key_press(egui::Key::Tab);
    harness.run();
    assert!(
        harness.state().document().text().to_string().contains("draw\t"),
        "{:?}",
        harness.state().document().text().to_string()
    );
}

#[test]
fn a_split_view_has_one_list_at_most_and_it_is_in_the_pane_with_the_keyboard() {
    // Scenario 23. One `Option` on the window makes "at most one" true by construction; the picture
    // is what says it is drawn in the right pane and over the divider rather than under it.
    let mut harness = completion_harness("caret.rs");
    let folder = completion_folder();
    harness.state_mut().open_path_permanently(&folder.join("layout.rs"));
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::SplitRight, &ctx);
    harness.run();
    assert_eq!(harness.state().files.pane_count(), 2);
    let text = harness.state().document().text().to_string();
    let blank = text.find("\nconst DRAW_LIMIT").expect("the blank line");
    harness.state_mut().command(Command::PlaceCaret { offset: blank, extend: false });
    harness.run();
    type_letters(&mut harness, "dra");
    assert!(harness.state().completion().is_some(), "{:?}", completions(&harness));
    let anchor = harness.state().completion_anchor().expect("the popup was drawn");
    let editing = harness.state().editor_area();
    assert!(
        anchor.pane.left() >= editing.left() - 1.0,
        "the list belongs to the pane with the keyboard: {:?} against {editing:?}",
        anchor.pane
    );
    harness.snapshot(shot("completion_split_view"));
}

#[test]
fn nothing_is_offered_in_a_file_no_plugin_claims() {
    // Scenario 13 through the menu the window really builds: absent, not dimmed.
    let harness = completion_harness("notes.md");
    let names: Vec<String> = quill_app::app::actions::menus(&harness.state().menu_state())
        .iter()
        .find(|menu| menu.name == "Edit")
        .expect("the Edit menu")
        .entries
        .iter()
        .filter_map(|entry| match entry {
            quill_app::app::actions::Entry::Item { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(!names.contains(&"Complete Word".to_owned()), "{names:?}");
    let mut harness = completion_harness("layout.rs");
    let names: Vec<String> = quill_app::app::actions::menus(&harness.state().menu_state())
        .iter()
        .find(|menu| menu.name == "Edit")
        .expect("the Edit menu")
        .entries
        .iter()
        .filter_map(|entry| match entry {
            quill_app::app::actions::Entry::Item { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"Complete Word".to_owned()), "{names:?}");
    harness.run();
}

#[test]
fn the_editor_page_holds_the_gutter_and_the_suggestions() {
    // `task-1677` §8.3 put `editor.suggestions` here, beside the line numbers, because both are
    // about what the editing area does rather than about what it looks like. The tick box is the
    // furniture `components::modal` and this dialog already had; what is new is the section.
    let mut harness = harness("");
    open_settings(&mut harness);
    harness.get_by_label("Editor").click();
    harness.run();
    harness.get_by_label("Show line numbers");
    harness.get_by_label("Suggest completions as you type");
    assert!(harness.state().settings.suggestions.is_automatic(), "on in a fresh Quill");
    harness.snapshot(shot("settings_editor"));

    // And the box really is the setting: clicking it puts the popup back to being asked for.
    harness.get_by_label("Suggest completions as you type").click();
    harness.run();
    assert!(!harness.state().settings.suggestions.is_automatic());
}

// ============================================================================================
// Deleting a file, saving a tab that is closed, and moving a file with its references.
//
// `task-1681`. Every one of these works on a folder of its own, copied out of the way, because a
// test that deletes and moves files must never be able to reach the fixture another test is
// reading — and because `services::recycle` really does put a deleted file in the Recycle Bin on
// this platform.

/// A folder of its own for a test that changes what is in it.
fn scratch_folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("quill-1681-{name}"));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("app")).expect("make the app folder");
    std::fs::create_dir_all(root.join("draw")).expect("make the draw folder");
    std::fs::write(root.join("readme.md"), "# Notes\n").expect("write readme.md");
    std::fs::write(root.join("app/main.ts"), "import { draw } from './layout';\n")
        .expect("write main.ts");
    std::fs::write(root.join("app/other.ts"), "import { draw } from './layout';\n")
        .expect("write other.ts");
    std::fs::write(root.join("app/layout.ts"), "export function draw() {}\n")
        .expect("write layout.ts");
    root
}

#[test]
fn enter_presses_the_button_that_does_the_thing() {
    // The About box has one button and no field in it, so before `task-1682` there was no way to
    // answer it from the keyboard at all. `components::modal::footer` is where that is decided, so
    // this is the rule every modal built from it follows.
    let mut harness = harness("Text behind the About box.");
    open_about(&mut harness);
    assert!(harness.state().about.is_some());
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().about.is_none(), "Enter should have pressed Done");
}

#[test]
fn enter_answers_a_question_that_has_no_field_in_it_and_reaches_nothing_behind_it() {
    let folder = scratch_folder("enter-confirms");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("tab open {}", folder.join("readme.md").display()));

    did(&mut harness, &format!("action run delete-path --path {}", folder.join("readme.md").display()));
    assert!(harness.state().confirmation.is_some(), "the question is asked");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().confirmation.is_none(), "Enter answered it");
    assert!(!folder.join("readme.md").exists(), "and the file has gone");
}

#[test]
fn a_modal_takes_the_keyboard_from_the_editing_area_and_the_explorer() {
    // The confirmation has no field in it, so nothing had egui's focus and the panes behind it went
    // on reading the frame's keys. `Enter` therefore meant three things at once: answer the
    // question, insert a new line into the file, and open the row the explorer's cursor was on.
    let folder = scratch_folder("modal-keyboard");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("tab open {}", folder.join("app/main.ts").display()));
    let before = harness.state().document().text().to_string();
    harness.state_mut().command(quill_core::Command::PlaceCaret { offset: 0, extend: false });
    harness.run();

    did(&mut harness, &format!("action run delete-path --path {}", folder.join("readme.md").display()));
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        before,
        "the file behind the question should not have gained a new line"
    );
    assert!(!harness.state().document().is_modified());
    assert!(!folder.join("readme.md").exists(), "and the question was answered");

    // A letter typed while a modal is open does not reach the document either.
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("tab open {}", folder.join("app/main.ts").display()));
    let before = harness.state().document().text().to_string();
    did(&mut harness, &format!("action run delete-path --path {}", folder.join("app/main.ts").display()));
    harness.input_mut().events.push(egui::Event::Text("typed".to_owned()));
    harness.run();
    assert_eq!(harness.state().document().text().to_string(), before);
    did(&mut harness, "modal cancel");
}

#[test]
fn the_explorers_menu_holds_delete_and_it_asks_before_anything_goes() {
    let folder = scratch_folder("menu");
    let mut harness = harness_in(&folder);
    let entries = quill_app::app::actions::explorer_menu(
        &folder.join("readme.md"),
        false,
        false,
        quill_app::app::actions::Aim::AtARow,
    );
    let names: Vec<String> = entries
        .iter()
        .filter_map(|entry| match entry {
            quill_app::app::actions::Entry::Item { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"Delete".to_owned()), "the menu holds it: {names:?}");

    did(&mut harness, &format!("action run delete-path --path {}", folder.join("readme.md").display()));
    let question = harness.state().confirmation.clone().expect("the question is asked");
    assert!(question.note.contains("readme.md"), "it names the file: {}", question.note);
    assert!(
        folder.join("readme.md").is_file(),
        "and nothing has gone while the question is still on the screen"
    );
    assert!(
        harness.state().message.is_none(),
        "and the status bar is not still saying what the last thing to happen was, which is what          made asking the question report a deletion that had not happened: {:?}",
        harness.state().message
    );
    harness.snapshot(shot("delete_confirmation"));
}

#[test]
fn confirming_the_question_takes_the_file_off_the_disk_and_closes_its_tab() {
    let folder = scratch_folder("confirm");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("tab open {}", folder.join("readme.md").display()));
    assert!(harness.state().files.paths().contains(&folder.join("readme.md")));

    did(&mut harness, &format!("action run delete-path --path {}", folder.join("readme.md").display()));
    did(&mut harness, "modal accept");
    harness.run();
    assert!(!folder.join("readme.md").exists(), "the file has gone");
    assert!(
        !harness.state().files.paths().contains(&folder.join("readme.md")),
        "and the tab that was on it has gone with it"
    );
}

#[test]
fn cancelling_the_question_leaves_the_file_exactly_where_it_was() {
    let folder = scratch_folder("cancel");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("action run delete-path --path {}", folder.join("readme.md").display()));
    did(&mut harness, "modal cancel");
    harness.run();
    assert!(folder.join("readme.md").is_file());
    assert!(harness.state().confirmation.is_none());
}

#[test]
fn delete_means_the_file_in_the_explorer_and_the_letter_in_the_editor() {
    let folder = scratch_folder("two-meanings");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("tab open {}", folder.join("readme.md").display()));

    // With the editing area holding the keyboard, `Delete` is what it has always been.
    harness.state_mut().command(quill_core::Command::PlaceCaret { offset: 0, extend: false });
    harness.run();
    harness.key_press(egui::Key::Delete);
    harness.run();
    assert_eq!(
        harness.state().document().text().to_string(),
        " Notes\n",
        "it took the letter in front of the caret"
    );
    assert!(harness.state().confirmation.is_none(), "and asked nothing");

    // With the explorer holding it, the same key is about the file.
    did(&mut harness, &format!("explorer select {}", folder.join("readme.md").display()));
    harness.key_press(egui::Key::Delete);
    harness.run();
    let question = harness.state().confirmation.clone().expect("the question is asked instead");
    assert!(question.note.contains("readme.md"));
}

#[test]
fn the_arrow_keys_walk_the_selection_and_a_letter_hands_the_keyboard_back() {
    let folder = scratch_folder("arrows");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("explorer select {}", folder.join("app").display()));
    let rows: Vec<std::path::PathBuf> =
        harness.state().tree.rows().iter().map(|row| row.entry.path.clone()).collect();
    let at = rows.iter().position(|row| *row == folder.join("app")).expect("the app folder is a row");

    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    assert_eq!(
        harness.state().selected.as_deref(),
        Some(rows[at + 1].as_path()),
        "Down moves to the next row that is showing"
    );
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(harness.state().selected.as_deref(), Some(folder.join("app").as_path()));

    // A letter belongs to the editor, so it hands the keyboard over and the letter lands in the
    // document. Without this, clicking a file in the tree and then typing would swallow the word.
    let before = harness.state().document().text().to_string();
    harness.input_mut().events.push(egui::Event::Text("x".to_owned()));
    harness.run();
    assert_eq!(harness.state().focus, quill_app::app::Focus::Editor);
    assert_eq!(
        harness.state().document().text().to_string(),
        format!("x{before}"),
        "and the letter that handed it over is the one that was typed"
    );
}

#[test]
fn closing_a_tab_that_was_edited_writes_it_and_an_untitled_one_is_not_written() {
    let folder = scratch_folder("save-on-close");
    let mut harness = harness_in(&folder);
    // A window opens with one untitled tab, which is the case that has nowhere to be written. It is
    // closed as it always was and says so, rather than putting `untitled.md` in somebody's project
    // because they shut a scratch buffer.
    harness.input_mut().events.push(egui::Event::Text("scratch".to_owned()));
    harness.run();
    did(&mut harness, "tab close");
    harness.run();
    assert!(
        harness.state().message.clone().unwrap_or_default().contains("without saving"),
        "it says what it did: {:?}",
        harness.state().message
    );
    assert!(!folder.join("untitled.md").exists(), "and wrote nothing into the project");

    did(&mut harness, &format!("tab open {}", folder.join("readme.md").display()));
    harness.state_mut().command(quill_core::Command::PlaceCaret { offset: 0, extend: false });
    harness.run();
    harness.input_mut().events.push(egui::Event::Text("Hello ".to_owned()));
    harness.run();
    assert!(harness.state().document().is_modified(), "it has changes that are not on the disk");

    did(&mut harness, "tab close");
    harness.run();
    assert_eq!(
        std::fs::read_to_string(folder.join("readme.md")).expect("read it back"),
        "Hello # Notes\n",
        "closing the tab wrote what was typed"
    );
}

#[test]
fn discarding_is_how_a_script_closes_a_tab_without_writing_it() {
    let folder = scratch_folder("discard");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("tab open {}", folder.join("readme.md").display()));
    harness.state_mut().command(quill_core::Command::PlaceCaret { offset: 0, extend: false });
    harness.run();
    harness.input_mut().events.push(egui::Event::Text("Hello ".to_owned()));
    harness.run();
    did(&mut harness, "tab close --discard");
    harness.run();
    assert_eq!(
        std::fs::read_to_string(folder.join("readme.md")).expect("read it back"),
        "# Notes\n",
        "the file on the disk is untouched"
    );
}

#[test]
fn moving_a_file_rewrites_a_closed_importer_and_leaves_an_open_one_modified() {
    let folder = scratch_folder("move");
    let mut harness = harness_in(&folder);
    // One of the two importers is open, so the ownership rule has both cases to answer.
    did(&mut harness, &format!("tab open {}", folder.join("app/main.ts").display()));

    let result = did(
        &mut harness,
        &format!(
            "explorer move {} {}",
            folder.join("app/layout.ts").display(),
            folder.join("draw").display()
        ),
    );
    assert_eq!(result["applied"], serde_json::json!(true));
    harness.run();

    assert!(folder.join("draw/layout.ts").is_file(), "the file moved");
    assert!(!folder.join("app/layout.ts").exists());
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read the closed importer"),
        "import { draw } from '../draw/layout';\n",
        "the closed file was written"
    );
    let open = harness
        .state()
        .files
        .iter()
        .find(|file| file.path() == Some(folder.join("app/main.ts").as_path()))
        .expect("the open importer is still a tab");
    assert_eq!(
        open.document.text().to_string(),
        "import { draw } from '../draw/layout';\n",
        "the open file was edited as a document"
    );
    assert!(open.document.is_modified(), "and left unsaved rather than written behind somebody");
    assert_eq!(
        std::fs::read_to_string(folder.join("app/main.ts")).expect("read what is on the disk"),
        "import { draw } from './layout';\n",
        "so the disk still holds what it held"
    );
}

#[test]
fn a_dry_run_says_what_would_change_and_changes_nothing() {
    let folder = scratch_folder("dry-run");
    let mut harness = harness_in(&folder);
    let result = did(
        &mut harness,
        &format!(
            "explorer move {} {} --dry-run",
            folder.join("app/layout.ts").display(),
            folder.join("draw").display()
        ),
    );
    assert_eq!(result["applied"], serde_json::json!(false));
    assert_eq!(result["references"], serde_json::json!(2), "both importers would change");
    assert!(folder.join("app/layout.ts").is_file(), "and nothing moved");
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read it"),
        "import { draw } from './layout';\n",
        "and nothing was written"
    );
}

#[test]
fn a_move_asked_for_without_the_refactor_leaves_every_reference_alone() {
    let folder = scratch_folder("no-refactor");
    let mut harness = harness_in(&folder);
    did(
        &mut harness,
        &format!(
            "explorer move {} {} --no-refactor",
            folder.join("app/layout.ts").display(),
            folder.join("draw").display()
        ),
    );
    assert!(folder.join("draw/layout.ts").is_file());
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read it"),
        "import { draw } from './layout';\n",
        "the import is exactly as it was, and now points at nothing"
    );
}

#[test]
fn a_move_and_the_move_back_leave_the_project_exactly_as_it_started() {
    let folder = scratch_folder("its-own-inverse");
    let before = std::fs::read_to_string(folder.join("app/other.ts")).expect("read it");
    let mut harness = harness_in(&folder);
    did(
        &mut harness,
        &format!(
            "explorer move {} {}",
            folder.join("app/layout.ts").display(),
            folder.join("draw").display()
        ),
    );
    did(
        &mut harness,
        &format!(
            "explorer move {} {}",
            folder.join("draw/layout.ts").display(),
            folder.join("app").display()
        ),
    );
    assert!(folder.join("app/layout.ts").is_file(), "it is back where it started");
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read it again"),
        before,
        "and so is every specifier, which is why a move needs no undo of its own"
    );
}

#[test]
fn renaming_a_file_takes_the_code_that_names_it_with_it() {
    let folder = scratch_folder("rename");
    let mut harness = harness_in(&folder);
    did(
        &mut harness,
        &format!("modal open rename --path {}", folder.join("app/layout.ts").display()),
    );
    did(&mut harness, "modal type page.ts");
    did(&mut harness, "modal accept");
    harness.run();
    assert!(folder.join("app/page.ts").is_file(), "the file was renamed");
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read the importer"),
        "import { draw } from './page';\n",
        "and the import followed it, because a rename is a move to a new name"
    );
}

#[test]
fn dragging_a_row_onto_a_folder_moves_it_and_rewrites_what_named_it() {
    let folder = scratch_folder("drag");
    let mut harness = harness_in(&folder);
    // The folders have to be open for their rows to be there to aim at.
    did(&mut harness, &format!("explorer expand {}", folder.join("app").display()));
    harness.run();
    let from = row_middle(&mut harness, "layout.ts");
    let to = row_middle(&mut harness, "draw");
    drag(&mut harness, from, to);
    harness.run();
    assert!(folder.join("draw/layout.ts").is_file(), "it landed in the folder it was dropped on");
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read the importer"),
        "import { draw } from '../draw/layout';\n"
    );
}

#[test]
fn a_row_dropped_where_it_already_is_does_nothing_at_all() {
    let folder = scratch_folder("no-op-drag");
    let mut harness = harness_in(&folder);
    did(&mut harness, &format!("explorer expand {}", folder.join("app").display()));
    harness.run();
    let from = row_middle(&mut harness, "layout.ts");
    let to = row_middle(&mut harness, "main.ts");
    drag(&mut harness, from, to);
    harness.run();
    assert!(folder.join("app/layout.ts").is_file(), "it is where it was");
    assert_eq!(
        std::fs::read_to_string(folder.join("app/other.ts")).expect("read the importer"),
        "import { draw } from './layout';\n",
        "and nothing was rewritten"
    );
}

/// The middle of the explorer row whose name contains `name`.
fn row_middle(harness: &mut Harness<'static, QuillApp>, name: &str) -> egui::Pos2 {
    let node = harness.get_by_label_contains(name);
    node.rect().center()
}

// -------------------------------------------------------------------------------- run

/// A window on a project that has something for the detectors to find.
fn run_project(name: &str) -> Harness<'static, QuillApp> {
    let folder = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&folder).expect("make the project");
    std::fs::write(folder.join("Cargo.toml"), "[package]\nname = \"thing\"\n").expect("write it");
    harness_in(&folder)
}

#[test]
fn the_command_line_keeps_a_configuration_and_lists_it_with_the_suggestions() {
    let mut harness = run_project("quill-cli-run-add");
    let added = did(&mut harness, "run add \"Dev server\" node server.js --port 3000");
    assert_eq!(added["selected"], "Dev server");
    let configurations = added["configurations"].as_array().expect("a list").clone();
    let first = &configurations[0];
    assert_eq!(first["name"], "Dev server");
    assert_eq!(first["command"], "node server.js --port 3000");
    assert_eq!(first["origin"], "permanent");
    assert_eq!(first["started"], false);
    // The detector's suggestion is listed after it, and says which it is.
    assert!(
        configurations.iter().any(|row| row["name"] == "cargo run" && row["origin"] == "suggested"),
        "{configurations:?}"
    );
    // The directory and the environment go in as flags, and a second one of the same name is
    // refused rather than quietly replacing what was there.
    did(&mut harness, "run add build cargo build --release --directory crates --env \"RUST_LOG=debug\"");
    let held = harness.state().run_configurations.find("build").expect("build").1.clone();
    assert_eq!(held.command, "cargo build --release");
    assert_eq!(held.directory, "crates");
    assert_eq!(held.environment(), vec![("RUST_LOG".to_owned(), "debug".to_owned())]);
    assert_eq!(refused(&mut harness, "run add build cargo test"), "usage");
}

#[test]
fn the_command_line_chooses_removes_and_refuses_a_name_nothing_holds() {
    let mut harness = run_project("quill-cli-run-select");
    did(&mut harness, "run add \"Dev server\" node server.js");
    did(&mut harness, "run add build cargo build");
    assert_eq!(did(&mut harness, "run select build")["selected"], "build");
    assert_eq!(harness.state().run_selected.as_deref(), Some("build"));
    assert_eq!(refused(&mut harness, "run select nothing"), "not-found");
    assert_eq!(refused(&mut harness, "run start nothing"), "not-found");

    did(&mut harness, "run remove build");
    assert!(harness.state().run_configurations.find("build").is_none());
    assert_eq!(refused(&mut harness, "run remove build"), "not-found");
    // Removing the chosen one leaves nothing chosen, and `run start` says so rather than guessing.
    did(&mut harness, "run select \"Dev server\"");
    did(&mut harness, "run remove \"Dev server\"");
    assert_eq!(harness.state().run_selected, None);
    assert_eq!(refused(&mut harness, "run start"), "not-applicable");
}

#[test]
fn a_run_that_could_not_start_is_a_failure_rather_than_a_success() {
    // `task-1691`. `run start` on a configuration whose program is not on the window's `PATH` used
    // to come back with `isError` false, `started` false and no reason anywhere, because the arm
    // read the reason out of the status bar and answered `ok` whatever it found. An agent holding
    // only that could not tell a program that failed to spawn from one that ran and exited at once.
    let mut harness = run_project("quill-cli-run-cannot-start");
    did(&mut harness, "run add bogus definitely-not-a-real-program");
    let reply = run(&mut harness, "run start bogus");
    assert!(!reply.ok, "a program that could not be spawned is not a success: {}", reply.message);
    let failure = reply.error.expect("a refusal carries an error");
    assert_eq!(failure.code, "failed");
    assert!(
        failure.message.contains("definitely-not-a-real-program"),
        "the refusal should carry the reason the window had: {}",
        failure.message
    );
    // And it is still in the list, because what was tried is worth keeping — the rule
    // `start_a_run` already followed.
    assert!(harness.state().run_configurations.find("bogus").is_some());
}

#[test]
fn adding_a_configuration_whose_program_cannot_be_found_says_so_and_still_adds_it() {
    // The first failure `task-1691`'s agent hit: `run add` accepted `node primes.js` without
    // comment and only `run start` failed, on a window launched from Finder with no version
    // manager's directory on its `PATH`. It is a note rather than a refusal, because a
    // configuration may name a program that will exist by the time it is run.
    let mut harness = run_project("quill-cli-run-add-path");
    let reply = run(&mut harness, "run add bogus definitely-not-a-real-program --port 3000");
    assert!(reply.ok, "it is a note, not a refusal: {}", reply.message);
    assert!(
        reply.message.contains("could not be found on this window's PATH"),
        "the reply should say the program is not there: {}",
        reply.message
    );
    assert!(harness.state().run_configurations.find("bogus").is_some(), "and it was still added");

    // A program that really is on the `PATH` says nothing about it. `cargo` is there, because
    // cargo is what started this test.
    let found = run(&mut harness, "run add build cargo build");
    assert!(found.ok, "{}", found.message);
    assert_eq!(found.message, "Added build");
}

#[test]
fn the_command_line_reads_what_a_run_has_written() {
    // A detached run, so what is being tested is the reading rather than a program's timing.
    let mut harness = harness("");
    harness
        .state_mut()
        .new_detached_run(configuration("Dev server", "node server.js"), 10, 60);
    harness.run();
    feed_run(&mut harness, b"Listening on http://localhost:3000\r\nGET / 200\r\nGET /a 200\r\n");

    let output = did(&mut harness, "run output");
    assert!(
        output["text"].as_str().expect("text").contains("Listening on http://localhost:3000"),
        "{output:?}"
    );
    // The tail is the last so many lines, which is what a long log wants.
    let tail = did(&mut harness, "run output --tail 1");
    assert_eq!(tail["text"], "GET /a 200");
    // And --wait-for is answered at once when what it is waiting for is already there.
    let found = did(&mut harness, "run output --wait-for Listening");
    assert_eq!(found["found"], true);
    // A configuration that has not been run has nothing to read.
    assert_eq!(refused(&mut harness, "run output nothing"), "not-applicable");
}

#[test]
fn the_command_line_says_whether_a_run_is_going_and_what_it_ended_with() {
    let mut harness = harness("");
    harness
        .state_mut()
        .new_detached_run(configuration("cargo test", "cargo test"), 8, 60);
    harness.run();
    let going = did(&mut harness, "run status");
    assert_eq!(going["state"], "running");
    assert_eq!(going["running"], true);

    let at = harness.state().run.index_of("cargo test").expect("the run");
    harness.state_mut().run.end_detached(at, Some(101));
    harness.run();
    let ended = did(&mut harness, "run status");
    assert_eq!(ended["state"], "exit code 101");
    assert_eq!(ended["exitCode"], 101);
    assert_eq!(ended["running"], false);

    // One that was never started says so rather than pretending.
    harness.state_mut().run_configurations.add_permanent(configuration("build", "cargo build"));
    let never = did(&mut harness, "run status build");
    assert_eq!(never["started"], false);
    assert_eq!(never["state"], "not started");
}

#[test]
fn stopping_a_run_from_the_command_line_leaves_the_tab_and_what_it_wrote() {
    let mut harness = harness("");
    harness
        .state_mut()
        .new_detached_run(configuration("Dev server", "node server.js"), 8, 60);
    harness.run();
    feed_run(&mut harness, b"Listening on http://localhost:3000\r\n");
    // The first stop is the polite one, so the run is still going.
    did_while_waiting(&mut harness, "run stop");
    assert!(harness.state().run.active().expect("a run").is_running());
    assert!(harness.state().run.is_stopping(), "and the window is waiting out the grace");
    // The second does not wait.
    did_while_waiting(&mut harness, "run stop");
    assert!(!harness.state().run.active().expect("a run").is_running());
    assert_eq!(harness.state().run.count(), 1, "the tab stays");
    let output = did(&mut harness, "run output");
    assert!(output["text"].as_str().expect("text").contains("Listening on"), "{output:?}");
}

#[test]
fn every_run_command_reaches_the_window() {
    // The rule `task-1661` asks for, checked for this area: a command the catalogue accepts is a
    // command the window knows, so none of them can answer "unknown command".
    let mut harness = run_project("quill-cli-run-known");
    for verb in ["list", "add", "remove", "start", "stop", "rerun", "select", "output", "status"] {
        let reply = run(&mut harness, &format!("run {verb} x"));
        assert_ne!(
            reply.error.as_ref().map(|error| error.code.as_str()),
            Some("unknown"),
            "`run {verb}` is in the catalogue and the window does not know it"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// task-1686: collapsing and expanding blocks.

/// A folder holding one real Rust file with blocks worth folding in it.
///
/// A folder of its own rather than an addition to `sample_folder`: that fixture's file count is in
/// the status bar of a dozen accepted screenshots, and a tenth file would change every one of them.
fn folding_folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-screenshot-folding").join(name);
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("make the folder");
    std::fs::write(
        root.join("source.rs"),
        "/// Adds two numbers together.\n\
         /// The second line of the comment.\n\
         fn add(left: usize, right: usize) -> usize {\n\
        \x20   let total = left + right;\n\
        \x20   if total > 100 {\n\
        \x20       return 100;\n\
        \x20   }\n\
        \x20   total\n\
         }\n\
         \n\
         fn subtract(left: usize, right: usize) -> usize {\n\
        \x20   left - right\n\
         }\n\
         \n\
         fn main() {\n\
        \x20   println!(\"{}\", add(1, 2));\n\
        \x20   println!(\"{}\", subtract(4, 3));\n\
         }\n",
    )
    .expect("write source.rs");
    root
}

/// Open `source.rs` in a window on a folder of its own.
fn folding_harness(name: &str) -> Harness<'static, QuillApp> {
    let folder = folding_folder(name);
    let mut harness = harness_in(&folder);
    harness.get_by_label_contains("source.rs").click();
    harness.run();
    harness.run();
    harness
}

/// How many lines of the file are on the page, which is what folding changes.
fn laid_out_paragraphs(harness: &Harness<'static, QuillApp>) -> Vec<usize> {
    harness.state().layout().lines.iter().map(|line| line.paragraph).collect()
}

#[test]
fn a_function_can_be_collapsed_from_the_gutter_and_the_line_numbers_stay_right() {
    let mut harness = folding_harness("gutter");
    // Line 3 is `fn add(...) {`, which is where the arrow goes. The numbers a person reads are one
    // more than the paragraph numbers Quill counts in.
    let before = laid_out_paragraphs(&harness);
    assert!(before.contains(&5), "the body of `add` is on the page to start with");

    harness.get_by_label("Collapse block at line 3").click();
    harness.run();
    harness.run();

    let after = laid_out_paragraphs(&harness);
    assert!(after.contains(&2), "the line the function starts on is still there");
    assert!(!after.contains(&5), "the body of `add` is not");
    assert!(after.contains(&10), "and `fn subtract` still is");
    // The whole of the ticket's sixth point: the numbers of what is still showing are unchanged.
    assert_eq!(
        after.iter().filter(|paragraph| **paragraph >= 9).copied().collect::<Vec<_>>(),
        before.iter().filter(|paragraph| **paragraph >= 9).copied().collect::<Vec<_>>(),
        "every line below the fold keeps the number it had"
    );
    // Two rows say so: the arrow in the gutter, and the badge drawn after the head line's text.
    assert_eq!(harness.get_all_by_label("Expand block at line 3").count(), 2);
    harness.snapshot(shot("folding_collapsed"));
}

#[test]
fn the_badge_on_a_collapsed_block_expands_it_again() {
    let mut harness = folding_harness("badge");
    harness.get_by_label("Collapse block at line 3").click();
    harness.run();
    harness.run();
    assert!(!laid_out_paragraphs(&harness).contains(&5));
    // Two rows now say `Expand block at line 3`: the arrow in the gutter and the badge in the text.
    // The badge is the one a person reaches for first, so it has to be the affordance it looks like.
    assert_eq!(harness.get_all_by_label("Expand block at line 3").count(), 2);
    harness.get_all_by_label("Expand block at line 3").last().expect("the badge").click();
    harness.run();
    harness.run();
    assert!(laid_out_paragraphs(&harness).contains(&5), "the body came back");
}

#[test]
fn collapse_all_then_expand_all_puts_the_file_back_exactly_as_it_was() {
    let mut harness = folding_harness("all");
    let before = laid_out_paragraphs(&harness);
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Fold(FoldAction::All), &ctx);
    harness.run();
    harness.run();
    let collapsed = laid_out_paragraphs(&harness);
    assert!(collapsed.len() < before.len(), "collapsing everything hides lines");
    assert!(collapsed.contains(&2) && collapsed.contains(&10), "every head line is still there");
    harness.snapshot(shot("folding_all_collapsed"));

    harness.state_mut().run_action(Action::Fold(FoldAction::None_), &ctx);
    harness.run();
    harness.run();
    assert_eq!(laid_out_paragraphs(&harness), before, "show all again gives back what was there");
}

#[test]
fn collapse_all_but_highlighted_leaves_the_marked_passage_showing() {
    let mut harness = folding_harness("marked");
    // Mark the `if` inside `add`, which is line 5 and is inside two blocks.
    let start = harness.state().document().text().line_to_byte(4);
    let end = harness.state().document().text().line_to_byte(6);
    harness.state_mut().document_mut().highlight(start..end, quill_core::Rgba::new(0xC9, 0xA2, 0x27, 0x66));
    harness.run();

    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Fold(FoldAction::Others), &ctx);
    harness.run();
    harness.run();

    let showing = laid_out_paragraphs(&harness);
    assert!(showing.contains(&4), "the marked line is showing");
    // Its parents had to stay open for it to be: the function, and the `if` it is the head of.
    assert!(showing.contains(&2), "the function that holds it is open");
    assert!(!showing.contains(&15), "and `fn main`, which holds nothing marked, is collapsed");
    harness.snapshot(shot("folding_all_but_marked"));
}

#[test]
fn a_caret_put_inside_a_collapsed_block_expands_it() {
    // The rule that makes everything else safe: a caret is never inside a hidden paragraph, so a
    // jump into one — go to definition, a search hit, `editor caret --line` — opens it first.
    let mut harness = folding_harness("reveal");
    harness.get_by_label("Collapse block at line 3").click();
    harness.run();
    harness.run();
    assert!(!laid_out_paragraphs(&harness).contains(&5));

    let offset = harness.state().document().text().line_to_byte(5);
    harness
        .state_mut()
        .document_mut()
        .apply(quill_core::Command::PlaceCaret { offset, extend: false });
    harness.state_mut().reveal_the_caret_from_a_fold();
    harness.run();
    harness.run();
    assert!(laid_out_paragraphs(&harness).contains(&5), "the block opened for the caret");
}

#[test]
fn collapsing_the_block_the_caret_is_in_moves_the_caret_to_its_head() {
    // The other half of the same rule. Collapsing what the caret is inside must not expand it
    // again, or `Collapse All` would do nothing whenever somebody was in the middle of a function.
    let mut harness = folding_harness("caret");
    let offset = harness.state().document().text().line_to_byte(5);
    harness
        .state_mut()
        .document_mut()
        .apply(quill_core::Command::PlaceCaret { offset, extend: false });
    harness.run();
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::Fold(FoldAction::All), &ctx);
    harness.run();
    harness.run();
    let caret = harness.state().document().selection().head;
    let line = harness.state().document().text().byte_to_line(caret);
    assert_eq!(line, 2, "the caret came out onto the line the function starts on");
    assert!(!laid_out_paragraphs(&harness).contains(&5), "and the block stayed collapsed");
}

#[test]
fn a_fold_stays_on_its_block_when_a_line_is_typed_above_it() {
    let mut harness = folding_harness("edited");
    harness.get_by_label("Collapse block at line 3").click();
    harness.run();
    harness.run();
    // A line typed at the very top of the file, which moves every byte below it.
    harness.state_mut().document_mut().apply(quill_core::Command::PlaceCaret { offset: 0, extend: false });
    harness.state_mut().document_mut().apply(quill_core::Command::Insert("// a new line\n".to_owned()));
    harness.run();
    harness.run();
    // The same function, now one line further down, and still collapsed: the arrow and the badge.
    assert_eq!(harness.get_all_by_label("Expand block at line 4").count(), 2);
    let showing = laid_out_paragraphs(&harness);
    assert!(showing.contains(&3), "the function's own line is still showing");
    assert!(!showing.contains(&6), "and its body is still hidden");
}

#[test]
fn a_picture_has_no_folding_entries_at_all() {
    // Quill's rule for a control that can never apply: absent, not dimmed.
    let mut harness = harness("");
    harness.get_by_label_contains("picture.png").click();
    harness.run();
    let state = harness.state().menu_state();
    assert!(!state.folding_applies);
    assert!(quill_app::app::actions::folding_menu(&state).is_empty());
    assert!(quill_app::app::actions::folding_here_menu(&state).is_empty());
}

// ============================================================================================
// Debugging: the gutter, the tile, the execution point and the inline values.
//
// `task-1687`. Every picture here is drawn from a **detached** session — one with no adapter behind
// it, fed fixed DAP messages — which is the trick the terminal's own pictures and the run tile's
// already use: when a real debugger answers is not something a test can know, and a picture that
// depended on it would differ between runs. The session runs the whole state machine over those
// messages, so what is drawn is what a real adapter sending them would have drawn.

use quill_app::app::actions::DebugAction;
use quill_dap::Message;

/// A folder holding one real Rust file to set breakpoints in.
///
/// Its own folder for `folding_folder`'s reason: `sample_folder`'s file count is in the status bar of
/// a dozen accepted screenshots, and another file there would change every one of them.
fn debug_folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join("quill-screenshot-debug").join(name);
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("make the folder");
    std::fs::write(
        root.join("main.rs"),
        "fn main() {\n\
        \x20   let attempts = 3;\n\
        \x20   let items = vec![1, 2, 3];\n\
        \x20   let total = attempts + items.len();\n\
        \x20   println!(\"{total}\");\n\
         }\n",
    )
    .expect("write main.rs");
    root
}

/// A window on that folder with `main.rs` open.
fn debug_harness(name: &str) -> Harness<'static, QuillApp> {
    let folder = debug_folder(name);
    let mut harness = harness_in(&folder);
    harness.get_by_label_contains("main.rs").click();
    harness.run();
    harness.run();
    harness
}

// Typing in a code file, keystroke by keystroke, with every frame painted.
//
// These come from a crash reported while typing a getter into a JavaScript class: the window went
// away with no message, because a panic in a bundled application on macOS reaches no standard error
// and unwinds out rather than aborting, so the operating system files no report either. That gap is
// closed by `services::crash_log`; these are the other half, and they exercise the paths a keystroke
// in a code file goes down, which nothing else here did:
//
//   - the file's symbols are read again on every keystroke, including from half-typed shapes,
//   - the completion list is built, opened, moved through and accepted,
//   - and the frame is **painted** after each one, which the other tests here only do at the end.
//
// The crash itself could not be reproduced from the description; what these do is make sure the
// sequence described stays exercised, so that if it is a fault in this path it fails here rather than
// on somebody's machine with nothing written down.

/// A small JavaScript project, with names a stem of `get` matches so that the list really opens.
fn javascript_folder() -> std::path::PathBuf {
    static FOLDER: OnceLock<std::path::PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let root = std::env::temp_dir().join("quill-screenshot-javascript");
            std::fs::create_dir_all(&root).expect("make the folder");
            std::fs::write(root.join("person.js"), "").expect("write person.js");
            std::fs::write(
                root.join("people.js"),
                "export function getName(person) { return person.name; }\n\
                 export function getAge(person) { return person.age; }\n\
                 export const getters = { getName, getAge };\n\
                 export class Employee {\n  get title() { return this.role; }\n  \
                 getSalary() { return 0; }\n}\n",
            )
            .expect("write people.js");
            root
        })
        .clone()
}

/// A window on that project with `person.js` open and the symbol index built, because the completion
/// list is built from the index and means nothing until it has arrived.
fn javascript_harness() -> Harness<'static, QuillApp> {
    let folder = javascript_folder();
    let mut harness = harness_in(&folder);
    harness.state_mut().open_path_permanently(&folder.join("person.js"));
    for _ in 0..600 {
        pump(&mut harness);
        if harness.state().symbols_indexer().is_some_and(|indexer| !indexer.is_building()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run();
    harness
}

/// What the adapter would have said, so a whole session is a list of values with no process in it.
fn answer(request_seq: i64, command: &str, body: serde_json::Value) -> Message {
    Message::Response {
        seq: 900 + request_seq,
        request_seq,
        command: command.to_owned(),
        success: true,
        message: None,
        body,
    }
}

/// Everything the session has asked for since this was last called, and forget it.
fn asked(harness: &mut Harness<'static, QuillApp>) -> Vec<serde_json::Value> {
    harness.state_mut().debug.as_mut().expect("a session").requested()
}

/// The seq the session really used for `command`, out of a batch of what it asked for.
///
/// Nothing about the order or the numbering is assumed: the test answers what was asked, exactly as
/// an adapter would. One `stopped` produces two requests at once, which is why the batch is read
/// whole rather than one request at a time.
fn seq_of(asked: &[serde_json::Value], command: &str) -> i64 {
    asked
        .iter()
        .find(|frame| frame["command"] == command)
        .and_then(|frame| frame["seq"].as_i64())
        .unwrap_or_else(|| panic!("the session should have asked for {command}: {asked:#?}"))
}

/// The two together, for the many places only one request is outstanding.
fn asked_for(harness: &mut Harness<'static, QuillApp>, command: &str) -> i64 {
    let batch = asked(harness);
    seq_of(&batch, command)
}

/// Hand a message to the session and let the window settle.
fn feed_debug(harness: &mut Harness<'static, QuillApp>, message: Message) {
    harness.state_mut().debug.as_mut().expect("a session").feed(message);
    harness.run();
}

/// Everything an ordinary adapter offers.
fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "supportsConfigurationDoneRequest": true,
        "supportsSetVariable": true,
        "supportsConditionalBreakpoints": true,
        "supportsLogPoints": true,
        "supportsTerminateRequest": true,
        // `task-1696`: the two the value tooltip asks about. A real CodeLLDB and a real js-debug
        // both offer them, and with them absent the popup would fall back to the `watch` context and
        // draw its root with no field — which is a different test, not this fixture's job.
        "supportsEvaluateForHovers": true,
        "supportsSetExpression": true,
    })
}

/// Drive a detached session from nothing to stopped at line 4 of `main.rs`, with three locals.
///
/// The whole lifecycle, in the protocol's own order, answering what the session really asked for.
fn paused_harness(name: &str) -> Harness<'static, QuillApp> {
    let mut harness = debug_harness(name);
    let path = debug_folder(name).join("main.rs");
    harness
        .state_mut()
        .new_detached_debug_session("lldb", configuration("app", "target/debug/app.exe"));
    harness.state_mut().show_the_debug_tile(true);
    harness.run();

    let initialize = asked_for(&mut harness, "initialize");
    feed_debug(&mut harness, answer(initialize, "initialize", capabilities()));
    // The launch went out with it; the adapter then says it is ready for breakpoints.
    asked_for(&mut harness, "launch");
    feed_debug(&mut harness, Message::Initialized);
    let done = asked_for(&mut harness, "configurationDone");
    feed_debug(&mut harness, answer(done, "configurationDone", serde_json::Value::Null));

    feed_debug(
        &mut harness,
        Message::Stopped(quill_dap::Stopped {
            reason: "breakpoint".to_owned(),
            thread: Some(1),
            description: None,
            text: None,
            all_threads: true,
        }),
    );
    // One `stopped` asks for both at once, so the batch is read whole.
    let batch = asked(&mut harness);
    let threads = seq_of(&batch, "threads");
    let stack = seq_of(&batch, "stackTrace");
    feed_debug(
        &mut harness,
        answer(threads, "threads", serde_json::json!({ "threads": [{ "id": 1, "name": "main" }] })),
    );
    feed_debug(
        &mut harness,
        answer(
            stack,
            "stackTrace",
            serde_json::json!({ "stackFrames": [
                { "id": 1000, "name": "app::main", "line": 4, "source": { "path": path.to_string_lossy() } },
                { "id": 1001, "name": "core::ops::function::FnOnce::call_once", "line": 250, "presentationHint": "subtle" }
            ]}),
        ),
    );
    let scopes = asked_for(&mut harness, "scopes");
    feed_debug(
        &mut harness,
        answer(
            scopes,
            "scopes",
            serde_json::json!({ "scopes": [
                { "name": "Locals", "variablesReference": 7, "expensive": false },
                { "name": "Registers", "variablesReference": 8, "expensive": true }
            ]}),
        ),
    );
    let variables = asked_for(&mut harness, "variables");
    feed_debug(
        &mut harness,
        answer(
            variables,
            "variables",
            serde_json::json!({ "variables": [
                { "name": "attempts", "value": "3", "type": "i32", "variablesReference": 0 },
                { "name": "items", "value": "Vec<i32>(len:3)", "type": "alloc::vec::Vec<i32>", "variablesReference": 17 },
                { "name": "total", "value": "6", "type": "usize", "variablesReference": 0 },
                // One the debugger could not read, which every real session has: it is listed in the
                // tree in the debugger's own words and **not** painted at the end of a line.
                { "name": "step", "value": "<optimized out>", "variablesReference": 0 }
            ]}),
        ),
    );
    harness
}

/// The reverse request js-debug sends, with the shape a real one has.
///
/// `__pendingTargetId` is the adapter's own handle for the program it has already started. It is
/// passed through untouched, so it is written here as a real one is: opaque.
// Two tests were written here against a `DebugState` that kept the connections a target had been
// handed over from — `a_target_handed_over_is_debugged_by_the_session_it_was_handed_to` and
// `the_run_ends_when_the_connection_it_was_launched_on_ends`. `task-1692` answers `startDebugging`
// its own way, by dialling the adapter again and reading both connections onto one channel with the
// child's replies tagged, so there is no retired connection for a test to read and the two could not
// be carried across. What they were about is covered by
// `session::tests::a_child_session_is_reported_with_the_configuration_that_opens_it`, by the rule
// that an answer which is not one for one with what was asked is not taken, and by
// `a_real_node_debugger_stops_at_a_breakpoint_and_reads_a_variable`, which runs a real
// js-debug against a real Node program.

#[test]
fn the_gutter_draws_an_enabled_a_disabled_an_unverified_and_a_conditional_breakpoint() {
    let mut harness = debug_harness("gutter");
    let folder = debug_folder("gutter");
    let path = folder.join("main.rs");
    // Line 2 plain, line 3 conditional, line 4 disabled, line 5 unverified — one of each, so the
    // picture is the whole vocabulary at once.
    did(&mut harness, &format!("debug breakpoint add {} 2", path.display()));
    did(
        &mut harness,
        &format!("debug breakpoint add {} 3 --condition \"attempts > 3\"", path.display()),
    );
    did(&mut harness, &format!("debug breakpoint add {} 4", path.display()));
    did(&mut harness, &format!("debug breakpoint disable {} 4", path.display()));
    did(&mut harness, &format!("debug breakpoint add {} 5", path.display()));
    harness.run();
    assert_eq!(harness.state().document().breakpoints().len(), 4);
    // One of each, which is what the picture is of.
    let conditional: Vec<bool> = harness
        .state()
        .document()
        .breakpoints()
        .iter()
        .map(quill_core::Breakpoint::is_conditional)
        .collect();
    assert_eq!(conditional, vec![false, true, false, false]);

    // A session that answered "I could not bind the last one", which is what makes it hollow. Quill
    // draws the adapter's answer rather than its own hope.
    harness
        .state_mut()
        .new_detached_debug_session("lldb", configuration("app", "target/debug/app.exe"));
    harness.run();
    let initialize = asked_for(&mut harness, "initialize");
    feed_debug(&mut harness, answer(initialize, "initialize", capabilities()));
    feed_debug(&mut harness, Message::Initialized);
    let breakpoints = asked_for(&mut harness, "setBreakpoints");
    feed_debug(
        &mut harness,
        answer(
            breakpoints,
            "setBreakpoints",
            // Three sent — the disabled one is not — and the last could not be bound.
            serde_json::json!({ "breakpoints": [
                { "id": 1, "verified": true, "line": 2 },
                { "id": 2, "verified": true, "line": 3 },
                { "id": 3, "verified": false, "message": "no code on that line" }
            ]}),
        ),
    );
    harness.get_by_label("Remove breakpoint on line 2");
    harness.get_by_label("Set breakpoint on line 1");
    harness.snapshot(shot("debug_gutter"));
}

#[test]
fn the_debug_tile_shows_the_frames_the_variables_and_a_watch() {
    let mut harness = paused_harness("tile");
    assert!(harness.state().debug.as_ref().expect("a session").is_paused());
    assert_eq!(harness.state().debug.as_ref().expect("a session").frames.len(), 2);

    // A watch, answered as a debugger would answer one.
    harness.state_mut().debug.as_mut().expect("a session").add_watch("items.len()");
    harness.run();
    let evaluate = asked_for(&mut harness, "evaluate");
    feed_debug(
        &mut harness,
        answer(evaluate, "evaluate", serde_json::json!({ "result": "3", "type": "usize" })),
    );

    // And a structure opened, which is the whole of the lazy model: nothing deeper was fetched
    // until this row was clicked.
    harness.state_mut().debug.as_mut().expect("a session").toggle_row("Locals/items");
    harness.run();
    let children = asked_for(&mut harness, "variables");
    feed_debug(
        &mut harness,
        answer(
            children,
            "variables",
            serde_json::json!({ "variables": [
                { "name": "[0]", "value": "1", "type": "i32", "variablesReference": 0 },
                { "name": "[1]", "value": "2", "type": "i32", "variablesReference": 0 },
                { "name": "[2]", "value": "3", "type": "i32", "variablesReference": 0 }
            ]}),
        ),
    );

    harness.get_by_label("Frame: app::main");
    harness.get_by_label("Variable: attempts = 3");
    harness.get_by_label_contains("Remove watch: items.len()");
    // The stepping buttons are all there, and so is the stop.
    for button in ["Resume", "Step Over", "Step Into", "Step Out", "Stop Debugging"] {
        harness.get_by_label(button);
    }
    harness.snapshot(shot("debug_tile"));
}

#[test]
fn the_execution_point_and_the_inline_values_are_drawn_over_the_source() {
    let mut harness = paused_harness("point");
    let folder = debug_folder("point");
    // The window jumped to the file the program stopped in and put the caret on line 4.
    assert!(harness.state().document().path().expect("a file").ends_with("main.rs"));
    let (path, line) =
        harness.state().debug.as_ref().expect("a session").location().expect("stopped somewhere");
    assert!(path.ends_with("main.rs"), "{}", path.display());
    assert_eq!(line, 4);
    assert!(path.starts_with(&folder));

    // A value the debugger could not read is not painted at the end of a line. It is still in the
    // tree, in the debugger's own words — but `step = <optimized out>` beside somebody's code is the
    // debugger declining to answer dressed as information, and IntelliJ paints nothing there either.
    let painted = harness.state_mut().inline_values_for_test();
    assert!(
        painted.iter().any(|(_, text)| text == "attempts = 3"),
        "a value the debugger read is painted: {painted:?}"
    );
    assert!(
        !painted.iter().any(|(_, text)| text.contains("step")),
        "and one it could not is not: {painted:?}"
    );

    harness.snapshot(shot("debug_execution_point"));
}

/// `task-1696`: the value tooltip, asked for at the caret and answered as a debugger answers.
///
/// It is driven through `Debug -> Show Value` rather than by moving a pointer, because what the
/// pointer adds is a 350 ms rest and a rectangle — both of which are unit tested with no window —
/// and both paths end in the same `open_the_value_tooltip`.
#[test]
fn the_value_tooltip_shows_a_structure_and_opens_it_into_its_fields() {
    let mut harness = paused_harness("hover");
    // Line 4 is `let total = attempts + items.len();`. The caret goes on `items`, which is where a
    // pointer resting on that word would put the question.
    let text = harness.state().document().text().to_string();
    let offset = text.find("items.len()").expect("the call is in the file");
    // A frame is drawn between the two on purpose. The execution point is followed **once a stop**
    // rather than once a frame — before `task-1696` this jump ran every frame it was true, so the
    // caret could not be moved at all while a program was stopped and this would ask about the first
    // word on the stopped line every time.
    harness.state_mut().document_mut().apply(quill_core::Command::PlaceCaret {
        offset: offset + 2,
        extend: false,
    });
    harness.run();
    assert_eq!(
        harness.state().document().selection().head,
        offset + 2,
        "a stopped program does not take the caret back on every frame"
    );
    choose(&mut harness, Action::Debug(DebugAction::ShowValue));
    harness.run();

    // The expression it read is the whole field path ending at the pointer, which is what IntelliJ
    // shows the value of.
    let asking = harness
        .state()
        .value_tooltip
        .as_ref()
        .unwrap_or_else(|| panic!("a tooltip: msg={:?} hover={:?}", harness.state().message, harness.state().debug.as_ref().and_then(|d| d.hover.as_ref()).map(|h| h.expression.clone())))
        .expression
        .clone();
    assert_eq!(asking, "items", "the word the caret is on");

    let evaluate = asked_for(&mut harness, "evaluate");
    feed_debug(
        &mut harness,
        answer(
            evaluate,
            "evaluate",
            serde_json::json!({
                "result": "Vec<i32>(len:3)",
                "type": "alloc::vec::Vec<i32>",
                "variablesReference": 41
            }),
        ),
    );
    // The root opens itself, which is what a person means by "show me the object" — so the children
    // are asked for with no click at all.
    let children = asked_for(&mut harness, "variables");
    feed_debug(
        &mut harness,
        answer(
            children,
            "variables",
            serde_json::json!({ "variables": [
                { "name": "[0]", "value": "1", "type": "i32", "variablesReference": 0 },
                { "name": "[1]", "value": "2", "type": "i32", "variablesReference": 0 },
                { "name": "[2]", "value": "3", "type": "i32", "variablesReference": 0 }
            ]}),
        ),
    );

    // `Value:` rather than `Variable:`, because the tile is showing the same variable at the
    // same moment and two controls must not share a name.
    harness.get_by_label("Value: items = Vec<i32>(len:3)");
    harness.get_by_label("Value: [1] = 2");
    harness.snapshot(shot("debug_value_tooltip"));
}

/// A row being typed over. The field is `show_row`'s, which is the same function the tile draws its
/// own rows with, so this is one control in two places rather than two that resemble each other.
#[test]
fn a_row_of_the_value_tooltip_can_be_typed_over() {
    let mut harness = paused_harness("hover-edit");
    let text = harness.state().document().text().to_string();
    let offset = text.find("attempts + items").expect("line 4 is in the file");
    // A frame is drawn between the two on purpose. The execution point is followed **once a stop**
    // rather than once a frame — before `task-1696` this jump ran every frame it was true, so the
    // caret could not be moved at all while a program was stopped and this would ask about the first
    // word on the stopped line every time.
    harness.state_mut().document_mut().apply(quill_core::Command::PlaceCaret {
        offset: offset + 2,
        extend: false,
    });
    harness.run();
    choose(&mut harness, Action::Debug(DebugAction::ShowValue));
    harness.run();
    let evaluate = asked_for(&mut harness, "evaluate");
    feed_debug(
        &mut harness,
        answer(evaluate, "evaluate", serde_json::json!({ "result": "3", "type": "i32" })),
    );

    // The root of a tooltip has no container reference, so `setVariable` cannot name it and
    // `setExpression` is what changes it. The adapter offered it, so the field is drawn.
    harness
        .state_mut()
        .value_tooltip
        .as_mut()
        .expect("a tooltip")
        .editing = Some(("attempts".to_owned(), "9".to_owned()));
    harness.run();
    harness.get_by_label("Set Value: attempts");
    harness.snapshot(shot("debug_value_tooltip_editing"));
}

#[test]
fn the_bottom_of_the_window_holds_one_of_three_tiles_and_never_two() {
    // Two grids stacked take the editing area below the fold of anything, so showing any of the
    // three puts the other two away. `task-1683` made this a pair; this is the trio.
    let mut harness = paused_harness("tiles");
    assert!(harness.state().debug_panel.visible);
    assert!(!harness.state().run.visible && !harness.state().terminal.visible);

    choose(&mut harness, Action::ToggleTerminal);
    assert!(harness.state().terminal.visible);
    assert!(!harness.state().debug_panel.visible && !harness.state().run.visible);

    choose(&mut harness, Action::ToggleRunTile);
    assert!(harness.state().run.visible);
    assert!(!harness.state().debug_panel.visible && !harness.state().terminal.visible);

    choose(&mut harness, Action::ToggleDebugTile);
    assert!(harness.state().debug_panel.visible);
    assert!(!harness.state().run.visible && !harness.state().terminal.visible);

    // And the rail has a button for each of the three, at the bottom of the window.
    harness.get_by_label("Debug tile");
    harness.get_by_label("Terminal tile");
    harness.get_by_label("Run tile");

    // The command line goes down the same path, which is what `show_the_*_tile` exists for.
    did(&mut harness, "terminal show");
    assert!(!harness.state().debug_panel.visible, "terminal show puts the debug tile away");
}

#[test]
fn stepping_lets_go_of_the_frame_and_the_execution_point() {
    let mut harness = paused_harness("stepping");
    assert!(!harness.state().debug.as_ref().expect("a session").rows.is_empty());

    choose(&mut harness, Action::Debug(DebugAction::StepOver));
    let debug = harness.state().debug.as_ref().expect("a session");
    assert!(!debug.is_paused(), "the program is going again");
    assert!(debug.rows.is_empty(), "every variablesReference died the moment it was told to go on");
    assert!(debug.location().is_none(), "and so did the execution point");
    // The request really went out, with the thread the adapter named.
    let stepped = harness.state_mut().debug.as_mut().expect("a session").requested();
    let next = stepped.iter().find(|frame| frame["command"] == "next").expect("a next request");
    assert_eq!(next["arguments"]["threadId"], 1);

    // Stepping again while it runs is refused with a sentence rather than sent into the dark.
    assert_eq!(refused(&mut harness, "debug step-over"), "not-applicable");
}

#[test]
fn a_breakpoint_moves_with_the_text_and_an_edit_is_not_a_reason_to_re_send_it() {
    let mut harness = paused_harness("moved");
    let path = debug_folder("moved").join("main.rs");
    did(&mut harness, &format!("debug breakpoint add {} 4", path.display()));
    harness.state_mut().debug.as_mut().expect("a session").requested();

    // A line typed at the top of the file, which moves every byte below it.
    harness.state_mut().document_mut().apply(Command::PlaceCaret { offset: 0, extend: false });
    harness.state_mut().document_mut().apply(Command::Insert("// a note\n".to_owned()));
    harness.run();
    // The dot followed the text: the same line of the program, one further down the file.
    let listed = did(&mut harness, "debug breakpoint list");
    let rows = listed["breakpoints"].as_array().expect("the list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["line"], 5, "it moved with the text: {rows:#?}");

    // **Editing text during a session does not re-send**: the running program's code has not
    // changed, so the adapter's positions stand — which is what every surveyed editor does.
    let asked = harness.state_mut().debug.as_mut().expect("a session").requested();
    assert!(
        !asked.iter().any(|frame| frame["command"] == "setBreakpoints"),
        "an edit is not a reason to tell the debugger anything: {asked:#?}"
    );

    // Toggling one **is**, and it goes out with the lines the file has now.
    did(&mut harness, &format!("debug breakpoint add {} 2", path.display()));
    let asked = harness.state_mut().debug.as_mut().expect("a session").requested();
    let sent = asked
        .iter()
        .find(|frame| frame["command"] == "setBreakpoints")
        .expect("the file was re-sent");
    let lines: Vec<i64> = sent["arguments"]["breakpoints"]
        .as_array()
        .expect("the breakpoints")
        .iter()
        .map(|one| one["line"].as_i64().unwrap_or(0))
        .collect();
    assert_eq!(lines, vec![2, 5]);
}

#[test]
fn a_file_whose_language_names_no_debugger_has_no_debug_controls_at_all() {
    // Quill's rule for a control that can never apply: absent, not dimmed. A stylesheet has nothing
    // to step through and never will.
    let mut harness = harness_in(&sample_folder());
    harness.get_by_label_contains("notes.txt").click();
    harness.run();
    let state = harness.state().menu_state();
    assert!(!state.debug_applies, "nothing claims a .txt");
    let entries = quill_app::app::actions::gutter_menu(&state);
    let names: Vec<String> = entries
        .iter()
        .filter_map(|entry| match entry {
            quill_app::app::actions::Entry::Item { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(!names.iter().any(|name| name.contains("Breakpoint")), "{names:?}");
    // And asking anyway is a sentence rather than a dot no debugger would ever honour.
    choose(&mut harness, Action::Debug(DebugAction::ToggleBreakpoint));
    assert!(harness.state().document().breakpoints().is_empty());
}

#[test]
fn setting_a_value_shows_what_the_debugger_now_holds_rather_than_what_was_typed() {
    let mut harness = paused_harness("set-value");
    did(&mut harness, "debug set-value Locals/attempts 9");
    let asked = asked_for(&mut harness, "setVariable");
    // A debugger that rounded a float, or interned a string, is telling the truth about what the
    // program holds — so its answer is what the row shows.
    feed_debug(
        &mut harness,
        answer(asked, "setVariable", serde_json::json!({ "value": "9", "type": "i32" })),
    );
    let debug = harness.state().debug.as_ref().expect("a session");
    let row = debug.rows.iter().find(|row| row.key == "Locals/attempts").expect("the row");
    assert_eq!(row.value, "9");
}

#[test]
fn the_command_line_can_set_a_breakpoint_read_the_stack_and_read_a_variable() {
    // The sequence the whole feature is an acceptance test of, and its second customer: an agent
    // driving Quill can observe a program's actual state instead of reasoning about it.
    let mut harness = paused_harness("cli");
    let path = debug_folder("cli").join("main.rs");
    did(&mut harness, &format!("debug breakpoint add {} 4", path.display()));

    let status = did(&mut harness, "debug status");
    assert_eq!(status["paused"], true);
    assert_eq!(status["line"], 4);
    assert_eq!(status["adapter"], "lldb");

    let frames = did(&mut harness, "debug frames --include-subtle");
    let listed = frames["lines"].as_array().expect("the frames");
    assert_eq!(listed.len(), 2);
    assert!(listed[0].as_str().expect("a line").contains("app::main"));

    let variables = did(&mut harness, "debug variables");
    let printed = variables["lines"].as_array().expect("the rows");
    assert!(
        printed.iter().any(|line| line.as_str().expect("a line").contains("attempts: i32 = 3")),
        "{printed:#?}"
    );

    // `evaluate` waits for the debugger's answer rather than reporting the question, which is what
    // its own `--timeout` flag promises. The answer arrives on a later frame, so the request is held
    // — which is what `run_command_line` returning `None` means.
    let ctx = harness.ctx.clone();
    let held = harness.state_mut().run_command_line("debug evaluate attempts", &ctx);
    assert!(held.is_none(), "an evaluation is answered on a later frame");
    let asked = asked_for(&mut harness, "evaluate");
    feed_debug(
        &mut harness,
        answer(asked, "evaluate", serde_json::json!({ "result": "3", "type": "i32" })),
    );

    // And the tile is reachable from the command line too, which is the fourth rule of the CLI.
    did(&mut harness, "action run toggle-debug-tile");
    assert!(!harness.state().debug_panel.visible);
}

#[test]
fn concise_debug_replies_lead_with_the_paused_frame_and_locals() {
    let mut harness = paused_harness("concise-debug-reply");

    let status = did(&mut harness, "debug status");
    assert_eq!(status["pausedFrame"]["name"], "app::main");
    assert!(
        status["locals"]
            .as_array()
            .expect("the fetched locals")
            .iter()
            .any(|row| row["name"] == "total" && row["value"] == "6"),
        "the runtime value is an immediate debugger answer: {status:#?}"
    );
    assert!(status.get("frames").is_none(), "ordinary replies do not carry a stack: {status:#?}");
    assert!(status.get("variables").is_none());
    assert!(status.get("watches").is_none());
    assert!(
        status["lines"]
            .as_array()
            .expect("spoken locals")
            .iter()
            .any(|line| line.as_str().is_some_and(|line| line.contains("total: usize = 6")))
    );

    let ordinary = did(&mut harness, "debug frames");
    assert_eq!(ordinary["lines"].as_array().map(Vec::len), Some(1));
    assert_eq!(ordinary["frames"].as_array().map(Vec::len), Some(1));
    assert_eq!(ordinary["hiddenFrames"], 1);
    assert!(ordinary.get("locals").is_none());
    assert!(ordinary.get("watches").is_none());

    let complete = did(&mut harness, "debug frames --include-subtle");
    assert_eq!(complete["lines"].as_array().map(Vec::len), Some(2));
    assert_eq!(complete["frames"].as_array().map(Vec::len), Some(2));
    assert_eq!(complete["hiddenFrames"], 0);

    let variables = did(&mut harness, "debug variables");
    assert!(variables["variables"].as_array().is_some_and(|rows| !rows.is_empty()));
    assert!(variables.get("frames").is_none());
    assert!(variables.get("locals").is_none());
    assert!(variables.get("watches").is_none());

    let watches = did(&mut harness, "debug watch list");
    assert_eq!(watches["watches"].as_array().map(Vec::len), Some(0));
    assert!(watches.get("frames").is_none());
    assert!(watches.get("locals").is_none());
    assert!(watches.get("variables").is_none());
}

/// The one debug test that starts a **real** adapter, and it earns it.
///
/// Everything above is a scripted session: the pictures have to be the same on every run, so they are
/// taken of a state machine that was handed fixed messages. That proves Quill's half of the
/// conversation and nothing about the other half. This is the other half — a real program, built
/// here, stopped at a real breakpoint by a real debugger, with a real value read out of it.
///
/// **Skipped with a message on a machine that has no adapter**, which is `task-1687` §12's own rule:
/// a skipped test that says why is honest, and a red one that lies about Quill is not. `lldb-dap`
/// ships inside every LLVM distribution and `winget install LLVM.LLVM` is how this machine got one.
///
/// It waits with `pump` and a deadline rather than `Harness::run`, which gives the window four steps
/// to go quiet and panics otherwise — right for a settled window and wrong while a debugger is
/// loading a binary's debug information. `task-1654`'s rule about waiting loops, once more.
#[test]
fn a_real_debugger_stops_at_a_breakpoint_and_reads_a_variable() {
    // `QUILL_LLDB_ADAPTER` first, which is the test's own spelling of the `debug.lldb` setting: an
    // adapter unpacked somewhere rather than installed is the ordinary case on a machine that has
    // not got LLVM, and a test that could only find one on `PATH` would skip on a machine that
    // plainly has one.
    let Some(adapter) = std::env::var_os("QUILL_LLDB_ADAPTER")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| quill_app::services::debuggers::on_path("codelldb"))
        .or_else(|| quill_app::services::debuggers::on_path("lldb-dap"))
    else {
        eprintln!(
            "skipped: no lldb adapter on this machine. `debug start` would say so too — \
             lldb-dap ships with LLVM (`winget install LLVM.LLVM`), and CodeLLDB's own \
             `codelldb.exe` is inside its .vsix. Point QUILL_LLDB_ADAPTER at either one."
        );
        return;
    };

    // A ten-line program, built here, so the test carries no binary and nothing is checked in.
    let folder = std::env::temp_dir().join("quill-real-debug");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    let source = folder.join("counter.rs");
    std::fs::write(
        &source,
        "fn main() {\n\
        \x20   let mut total: i64 = 0;\n\
        \x20   for step in 1..=4 {\n\
        \x20       total += step;\n\
        \x20   }\n\
        \x20   let answer = total;\n\
        \x20   println!(\"{answer}\");\n\
         }\n",
    )
    .expect("write counter.rs");
    let binary = folder.join(if cfg!(windows) { "counter.exe" } else { "counter" });
    // `-g` for debug information and `-C opt-level=0` so the locals are really there: an optimised
    // build has no `answer` to read, which would make this test fail for a reason that is not Quill.
    let built = std::process::Command::new("rustc")
        .arg("-g")
        .arg("-C")
        .arg("opt-level=0")
        .arg("-o")
        .arg(&binary)
        .arg(&source)
        .output()
        .expect("run rustc");
    assert!(
        built.status.success(),
        "the fixture would not build: {}",
        String::from_utf8_lossy(&built.stderr)
    );

    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    // The adapter is named explicitly rather than looked for again, so the test debugs the one it
    // decided to skip on the absence of.
    harness.state_mut().settings.debug_adapters =
        vec![("lldb".to_owned(), adapter.to_string_lossy().to_string())];
    harness.state_mut().open_path_permanently(&source);
    harness.run();

    // Line 6, `let answer = total;` — after the loop, so `total` is 10 by the time it is reached.
    let set = harness
        .state_mut()
        .run_command_line(
            &format!("debug breakpoint add {} 6", source.display()),
            &ctx,
        )
        .expect("answered at once");
    assert!(set.ok, "{}", set.message);

    harness.state_mut().run_configurations.add_permanent(configuration(
        "counter",
        &quill_app::services::run_configurations::quote_part(&binary.to_string_lossy()),
    ));
    harness.state_mut().run_selected = Some("counter".to_owned());
    choose(&mut harness, Action::Debug(DebugAction::Start(None)));
    assert!(
        harness.state().debug.is_some(),
        "the session should have started: {:?}",
        harness.state().message
    );

    // Sixty seconds, which is far past what lldb takes to load a ten-line binary and short enough
    // that a machine where the adapter never answers says so rather than hanging the suite.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        harness.step();
        let debug = harness.state().debug.as_ref().expect("the session");
        // Stopped **and** the stack read, which is what there being something to look at means.
        if debug.is_ready() {
            break;
        }
        assert!(
            debug.is_alive(),
            "the session ended without stopping: {:?}",
            harness.state().message
        );
        assert!(
            std::time::Instant::now() < deadline,
            "the program did not stop in sixty seconds; it is {}",
            debug.where_it_is()
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // Where it stopped, as the debugger says it — asserted on text, which is the only thing about a
    // real adapter that is the same on every machine.
    let status = harness
        .state_mut()
        .run_command_line("debug status", &ctx)
        .expect("answered at once");
    assert!(status.ok, "{}", status.message);
    assert_eq!(status.result["paused"], true);
    assert_eq!(status.result["line"], 6, "{}", status.message);

    let frames = harness
        .state_mut()
        .run_command_line("debug frames", &ctx)
        .expect("answered at once");
    let listed = frames.result["lines"].as_array().expect("the frames");
    assert!(
        listed
            .iter()
            .any(|line| line.as_str().expect("a line").contains("counter.rs:6")),
        "the top frame should be the line it stopped on: {listed:#?}"
    );

    // And the value the program really computed. `total` is 1+2+3+4.
    let variables = harness
        .state_mut()
        .run_command_line("debug variables", &ctx)
        .expect("answered at once");
    let printed: Vec<String> = variables.result["lines"]
        .as_array()
        .expect("the rows")
        .iter()
        .map(|line| line.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        printed.iter().any(|line| line.contains("total") && line.contains("10")),
        "the debugger should have read `total` as 10: {printed:#?}"
    );

    // Stepping over the assignment makes `answer` the same number, which is the other half of the
    // feature: the program really moved.
    let stepped = harness
        .state_mut()
        .run_command_line("debug step-over", &ctx)
        .expect("answered at once");
    assert!(stepped.ok, "{}", stepped.message);
    loop {
        harness.step();
        let debug = harness.state().debug.as_ref().expect("the session");
        // Ready, not merely paused: the same distinction the first wait draws, and the reason
        // `DebugState::is_ready` exists.
        if debug.is_ready() && debug.location().map(|(_, line)| line) == Some(7) {
            break;
        }
        assert!(debug.is_alive(), "the session ended while stepping");
        assert!(std::time::Instant::now() < deadline, "the step did not land in time");
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let variables = harness
        .state_mut()
        .run_command_line("debug variables", &ctx)
        .expect("answered at once");
    let printed: Vec<String> = variables.result["lines"]
        .as_array()
        .expect("the rows")
        .iter()
        .map(|line| line.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        printed.iter().any(|line| line.contains("answer") && line.contains("10")),
        "stepping over the assignment should have made `answer` 10: {printed:#?}"
    );

    // Nothing ever orphans a child on purpose, which for a debugger is two of them: the adapter and
    // the program it is holding.
    harness.state_mut().stop_debugging();
    harness.state_mut().run.kill_everything();
    std::fs::remove_dir_all(&folder).ok();
}

/// The same again with **js-debug**, which is the adapter that hands its target to a second session.
///
/// It earns a second real-adapter test rather than being folded into the one above, because what it
/// proves is different. CodeLLDB debugs on the connection it was launched on, so it never exercises
/// the handover; js-debug does nothing else. Before `startDebugging` was answered this test's program
/// printed `Debugger attached.`, every breakpoint stayed unverified, and it waited for the sixty
/// seconds and failed — which is exactly what a person driving Quill saw.
///
/// It also proves the address. js-debug's own default host is `localhost`, which resolves to `::1`
/// before `127.0.0.1` on macOS, so an adapter left to its default binds where `quill_dap` never
/// dials and this test cannot start a session at all.
///
/// **Skipped with a message on a machine with no js-debug.** There is nothing to look for on `PATH`:
/// it ships as a `.js` file in a GitHub release asset rather than as a program, which is why
/// `debug.node` has no default and why `tools/get-debug-adapter.sh` exists.
#[test]
fn a_real_node_debugger_stops_at_a_breakpoint_and_reads_a_variable() {
    let Some(adapter) = std::env::var_os("QUILL_NODE_ADAPTER")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!(
            "skipped: no js-debug on this machine. `debug start` would say so too. \
             `tools/get-debug-adapter.sh node` fetches one and prints the line; then point \
             QUILL_NODE_ADAPTER at its dapDebugServer.js."
        );
        return;
    };
    let Some(node) = quill_app::services::debuggers::on_path("node") else {
        eprintln!("skipped: no node on this machine, and js-debug is a script node runs.");
        return;
    };

    let folder = std::env::temp_dir().join("quill-real-node-debug");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    let source = folder.join("counter.js");
    // Line 6 is `return sum;`, reached once the loop has run, so `sum` is 1+2+3+4 by then.
    std::fs::write(
        &source,
        "function total(upTo) {\n\
        \x20 let sum = 0;\n\
        \x20 for (let step = 1; step <= upTo; step++) {\n\
        \x20   sum += step;\n\
        \x20 }\n\
        \x20 return sum;\n\
         }\n\
         \n\
         const answer = total(4);\n\
         console.log(answer);\n",
    )
    .expect("write counter.js");

    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().settings.debug_adapters =
        vec![("node".to_owned(), adapter.to_string_lossy().to_string())];
    harness.state_mut().open_path_permanently(&source);
    harness.run();

    let set = harness
        .state_mut()
        .run_command_line(&format!("debug breakpoint add {} 6", source.display()), &ctx)
        .expect("answered at once");
    assert!(set.ok, "{}", set.message);

    let command = format!(
        "{} {}",
        quill_app::services::run_configurations::quote_part(&node.to_string_lossy()),
        quill_app::services::run_configurations::quote_part(&source.to_string_lossy())
    );
    harness.state_mut().run_configurations.add_permanent(configuration("counter", &command));
    harness.state_mut().run_selected = Some("counter".to_owned());
    choose(&mut harness, Action::Debug(DebugAction::Start(None)));
    assert!(
        harness.state().debug.is_some(),
        "the session should have started: {:?}",
        harness.state().message
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        harness.step();
        let debug = harness.state().debug.as_ref().expect("the session");
        if debug.is_ready() {
            break;
        }
        assert!(
            debug.is_alive(),
            "the session ended without stopping: {:?}",
            harness.state().message
        );
        assert!(
            std::time::Instant::now() < deadline,
            "the program did not stop in sixty seconds; it is {}. \
             Before `startDebugging` was answered this is where it hung for ever.",
            debug.where_it_is()
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    let status = harness
        .state_mut()
        .run_command_line("debug status", &ctx)
        .expect("answered at once");
    assert!(status.ok, "{}", status.message);
    assert_eq!(status.result["paused"], true);
    assert_eq!(status.result["line"], 6, "{}", status.message);

    let frames = harness
        .state_mut()
        .run_command_line("debug frames", &ctx)
        .expect("answered at once");
    let listed = frames.result["lines"].as_array().expect("the frames");
    assert!(
        listed.iter().any(|line| line.as_str().expect("a line").contains("counter.js:6")),
        "the top frame should be the line it stopped on: {listed:#?}"
    );

    let variables = harness
        .state_mut()
        .run_command_line("debug variables", &ctx)
        .expect("answered at once");
    let printed: Vec<String> = variables.result["lines"]
        .as_array()
        .expect("the rows")
        .iter()
        .map(|line| line.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        printed.iter().any(|line| line.contains("sum") && line.contains("10")),
        "the debugger should have read `sum` as 10: {printed:#?}"
    );

    // And the program really moves when it is stepped, which is the stepping request going to the
    // session attached to the target rather than to the one that launched it.
    //
    // Stepped **until the line changes** rather than once, because V8 steps by statement within a
    // line: the first `next` from `return sum;` lands on line 6 again at a different column, which is
    // correct and is not something a test should assert away. Six is far more than the two it takes
    // and is a bound rather than a wait.
    let mut moved = false;
    for _ in 0..6 {
        let before = harness
            .state()
            .debug
            .as_ref()
            .expect("the session")
            .location()
            .map(|(_, line)| line);
        let stepped = harness
            .state_mut()
            .run_command_line("debug step-over", &ctx)
            .expect("answered at once");
        assert!(stepped.ok, "{}", stepped.message);
        loop {
            harness.step();
            let debug = harness.state().debug.as_ref().expect("the session");
            if debug.is_ready() {
                break;
            }
            assert!(debug.is_alive(), "the session ended while stepping");
            assert!(std::time::Instant::now() < deadline, "the step did not land in time");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let after = harness
            .state()
            .debug
            .as_ref()
            .expect("the session")
            .location()
            .map(|(_, line)| line);
        if after != before {
            moved = true;
            break;
        }
    }
    assert!(moved, "stepping should have left the line the breakpoint was on");

    harness.state_mut().stop_debugging();
    harness.state_mut().run.kill_everything();
    std::fs::remove_dir_all(&folder).ok();
}

/// A breakpoint set in one window is there in the next one, which is the whole point of writing them
/// beside the project.
///
/// This is the half a unit test cannot reach. `services::breakpoint_store` proves the file
/// round-trips and `quill_core::breakpoints` proves the offsets move with the text, and both passed
/// while **the reading half was not wired up at all**: `.quill/breakpoints.conf` was written
/// faithfully and never read back. It was found by driving a real window, which is what the fourth
/// layer of tests is for — so this is that walk, kept.
#[test]
fn a_breakpoint_is_still_there_when_the_project_is_opened_again() {
    let folder = std::env::temp_dir().join("quill-breakpoints-persist");
    std::fs::remove_dir_all(&folder).ok();
    std::fs::create_dir_all(&folder).expect("make the project");
    let source = folder.join("main.rs");
    std::fs::write(&source, "fn main() {\n    let a = 1;\n    let b = a + 1;\n}\n")
        .expect("write main.rs");

    {
        let mut harness = harness_in(&folder);
        let ctx = harness.ctx.clone();
        // `restore_project` is what turns the writing on: a test neither reads nor writes a person's
        // files unless it says so, which is the rule the project state and the marks already keep.
        harness.state_mut().restore_project();
        harness.state_mut().open_path_permanently(&source);
        harness.run();
        let set = harness
            .state_mut()
            .run_command_line(&format!("debug breakpoint add {} 3", source.display()), &ctx)
            .expect("answered at once");
        assert!(set.ok, "{}", set.message);
        // Written once the pointer is up and something has changed, which is the same terms the
        // marks are written on — so the window is run until it has settled.
        for _ in 0..8 {
            harness.step();
        }
    }

    let written = std::fs::read_to_string(folder.join(".quill/breakpoints.conf"))
        .expect("the file should have been written");
    assert!(written.contains("breakpoint.1.path = main.rs"), "{written}");

    // A second window on the same folder, which is what opening the project again is.
    let mut harness = harness_in(&folder);
    let ctx = harness.ctx.clone();
    harness.state_mut().restore_project();
    harness.run();
    let listed = harness
        .state_mut()
        .run_command_line("debug breakpoint list", &ctx)
        .expect("answered at once");
    let rows = listed.result["breakpoints"].as_array().expect("the list");
    assert_eq!(rows.len(), 1, "the breakpoint should have come back: {listed:?}");
    assert_eq!(rows[0]["line"], 3);

    // And the open document holds it, not just the store — which is the ownership rule's other half:
    // a file that is open is owned by its `Document`.
    harness.state_mut().open_path_permanently(&source);
    harness.run();
    assert_eq!(
        harness.state().document().breakpoints().len(),
        1,
        "the tab should have come up with its dot already there"
    );
    let marks = harness.state().breakpoint_marks(harness.state().files.active_index());
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].0, 2, "paragraph 2 is the third line");

    std::fs::remove_dir_all(&folder).ok();
}
/// Type `text` a character at a time, painting every frame, and walking the completion list whenever
/// it opens.
///
/// Painting is the point. `Harness::run` builds a frame's shapes; `render` is what turns them into a
/// picture, and the completion list's own drawing — a row's matched letters picked out one at a time —
/// only happens there.
fn type_and_paint(harness: &mut Harness<'static, QuillApp>, text: &str) {
    for letter in text.chars() {
        if letter == '\n' {
            harness.key_press(egui::Key::Enter);
        } else {
            harness.input_mut().events.push(egui::Event::Text(letter.to_string()));
        }
        harness.run();
        harness.render().expect("paint the frame");
        if harness.state().completion().is_some() {
            harness.key_press(egui::Key::ArrowDown);
            harness.run();
            harness.render().expect("paint the frame with a row chosen");
        }
    }
}

#[test]
fn typing_a_getter_into_a_javascript_class_survives_every_keystroke() {
    // The sequence from the report: the class, a blank line inside it, the closing brace, and then a
    // getter typed on the blank line.
    let mut harness = javascript_harness();
    type_and_paint(&mut harness, "class Person {\n\n}");
    let text = harness.state().document().text().to_string();
    let inside = text.find("{\n").expect("the brace and the line under it") + 2;
    harness.state_mut().command(Command::PlaceCaret { offset: inside, extend: false });
    harness.run();

    type_and_paint(&mut harness, "get");
    let offered = completions(&harness);
    assert!(
        offered.contains(&"getName".to_owned()),
        "the list should be open on the project's own names, so this test is exercising it: {offered:?}"
    );

    type_and_paint(&mut harness, " fullName() {\nreturn this.name;\n");
    assert!(
        harness.state().document().text().to_string().contains("get fullName()"),
        "and what was typed is what is there"
    );
}

#[test]
fn accepting_a_completion_inside_a_class_survives() {
    let mut harness = javascript_harness();
    type_and_paint(&mut harness, "class Person {\n\n}");
    let text = harness.state().document().text().to_string();
    let inside = text.find("{\n").expect("the brace") + 2;
    harness.state_mut().command(Command::PlaceCaret { offset: inside, extend: false });
    harness.run();
    type_and_paint(&mut harness, "get");
    assert!(!completions(&harness).is_empty(), "the list is open");

    // Tab takes the whole word, which is the acceptance that also has to replace what is to the right
    // of the caret, and the one that can add an import.
    harness.key_press(egui::Key::Tab);
    harness.run();
    harness.render().expect("paint what acceptance left behind");
    let after = harness.state().document().text().to_string();
    assert!(after.contains("get"), "something was accepted: {after:?}");
    assert!(harness.state().completion().is_none(), "and the list closed behind it");
}

#[test]
fn the_shapes_of_javascript_that_could_be_mis_sliced_survive_being_typed() {
    // Every one of these is a shape where a byte offset could land inside a character or past the end:
    // a template literal with expressions in it, a regular expression, an accented identifier, an
    // unclosed string, a comment naming a symbol, and spread syntax.
    let mut harness = javascript_harness();
    let shapes = [
        "import { getName } from './people.js';\n",
        "import * as people from './people.js';\n",
        "const greeting = `hello ${person.name} and ${getName(person)}`;\n",
        "const pattern = /get[A-Z]\\w+/g;\n",
        "class Person extends Employee {\n  static get kind() { return 'person'; }\n}\n",
        "const \u{00e9}quipe = { g\u{00e9}rant: getName };\n",
        "// a comment about getName\n",
        "const half = 'unclosed string\n",
        "const object = { get, getName, ...rest };\n",
        "async function* getEverything() { yield await getName(); }\n",
    ];
    for shape in shapes {
        harness.state_mut().command(Command::SelectAll);
        harness.run();
        type_and_paint(&mut harness, shape);
    }
}

// The window closing or minimising itself while somebody types.
//
// This is what the report of a crash while typing turned out to be, and it is also the report of the
// window minimising itself in the middle of a word. Neither is a crash: nothing panics, nothing is
// written to `crash.log`, and macOS files no report, because the window is asked to close in the
// ordinary way — by a button being pressed.
//
// egui moves keyboard focus when a bare `Tab` or a bare arrow key is pressed, and it keeps moving it
// unless the widget that holds focus says those keys are its own. Quill's editing area never held
// egui's focus, so nothing said that, and the focus walked out of the document and onto the three
// window buttons in the title bar. A button with keyboard focus is pressed by `Space` or `Enter`. So
// one arrow key followed by a space closed the window if the focus had landed on `Close`, minimised it
// if it had landed on `Minimise`, and resized it if it had landed on `Maximise` — while the person was
// doing nothing but typing.
//
// Every key below is one a person types constantly in a code file.

/// The commands the window sent this frame that move, close or resize the window itself.
fn window_commands(harness: &Harness<'static, QuillApp>) -> Vec<String> {
    harness
        .output()
        .viewport_output
        .values()
        .flat_map(|viewport| viewport.commands.iter())
        .filter(|command| {
            matches!(
                command,
                egui::ViewportCommand::Close
                    | egui::ViewportCommand::Minimized(_)
                    | egui::ViewportCommand::Maximized(_)
                    | egui::ViewportCommand::StartDrag
            )
        })
        .map(|command| format!("{command:?}"))
        .collect()
}

/// Whether any of the three window buttons holds the keyboard.
fn a_window_button_holds_the_keyboard(harness: &mut Harness<'static, QuillApp>) -> Option<String> {
    ["Close", "Minimise", "Maximise"].into_iter().find_map(|label| {
        let held = harness.get_all_by_label(label).any(|node| node.is_focused());
        held.then(|| label.to_owned())
    })
}

#[test]
fn typing_a_space_after_a_tab_or_an_arrow_key_cannot_close_or_minimise_the_window() {
    let mut harness = javascript_harness();
    type_and_paint(&mut harness, "class Person {\n");

    // Each of these is pressed and then a space is typed, which is what pressing a focused button
    // takes. The arrows are in it because egui moves focus on those as well as on `Tab`, and an arrow
    // key in a code file is the commonest key press there is.
    for key in [
        egui::Key::Tab,
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
    ] {
        harness.key_press(key);
        harness.run();
        assert_eq!(
            a_window_button_holds_the_keyboard(&mut harness),
            None,
            "after {key:?} the keyboard is still in the document, not on a window button"
        );
        // And it is Quill's own holder that has it, so this test is passing for the reason it is meant
        // to and not because the title bar happened not to be drawn.
        assert_eq!(
            harness.ctx.memory(|memory| memory.focused()),
            Some(egui::Id::new(quill_app::app::KEYBOARD_HOLDER)),
            "the focus stays where Quill put it after {key:?}"
        );

        // A space, as a keyboard sends it: the key press and the letter both.
        harness.key_press(egui::Key::Space);
        harness.input_mut().events.push(egui::Event::Text(" ".to_owned()));
        harness.run();
        let commands = window_commands(&harness);
        assert!(
            commands.is_empty(),
            "{key:?} then a space must type a space and nothing else, and it sent {commands:?}"
        );

        // And the same for Enter, which presses a focused button too.
        harness.key_press(key);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        let commands = window_commands(&harness);
        assert!(
            commands.is_empty(),
            "{key:?} then Enter must not reach a window button, and it sent {commands:?}"
        );
    }

    // The keys have to keep doing what they did, which is the other half of it. The arrows moved the
    // caret about as they were pressed, so where in the file each character landed is not fixed — what
    // matters is that every one of these keys still reached the document: the class was typed, `Tab`
    // still put a tab in and the spaces still went in as spaces.
    let text = harness.state().document().text().to_string();
    assert!(text.contains("Person {"), "what was typed is still there: {text:?}");
    assert!(text.contains('\t'), "Tab still types a tab: {text:?}");
    assert!(text.contains(' '), "and a space is still a space: {text:?}");
}

#[test]
fn an_idle_window_always_asks_to_be_woken_again() {
    // The window was found asleep with a command queued and no frame drawn in three seconds: it had
    // asked for no frame, so the only thing that could have drawn one was a wake from another thread,
    // and that wake never arrived. Asking for the next frame on every frame is what makes a lost wake
    // cost a quarter of a second. If this ever stops being true, a missed wake can hang the window
    // again, and there is no way to see that from a screenshot.
    let mut harness = harness("");
    harness.run();
    let asked_for = harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the window's own output")
        .repaint_delay;
    assert!(
        asked_for <= quill_app::app::HEARTBEAT,
        "an idle window asked to sleep for {asked_for:?}, which is longer than the heartbeat of {:?}",
        quill_app::app::HEARTBEAT
    );

    // And typing does not take it away: it is asked for on every frame, not on the first.
    harness.get_by_label_contains("readme.md").click();
    harness.run();
    harness.input_mut().events.push(egui::Event::Text("x".to_owned()));
    harness.run();
    harness.run();
    let asked_for = harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the window's own output")
        .repaint_delay;
    assert!(asked_for <= quill_app::app::HEARTBEAT, "and still after a frame of typing: {asked_for:?}");
}

/// What holds egui's keyboard focus this frame.
fn what_holds_the_keyboard(harness: &Harness<'static, QuillApp>) -> Option<egui::Id> {
    harness.ctx.memory(|memory| memory.focused())
}

/// Quill's own holder, which is where the focus belongs while a pane is being typed into.
fn the_holder() -> egui::Id {
    egui::Id::new(quill_app::app::KEYBOARD_HOLDER)
}

#[test]
fn a_text_box_takes_the_keyboard_from_the_holder_and_the_holder_takes_it_back() {
    // The other half of holding the focus: a box that is typed into has to be able to take it, or the
    // explorer's filter, the commit message and the rename prompt could never be typed into at all.
    let mut harness = harness("");
    harness.run();
    assert_eq!(what_holds_the_keyboard(&harness), Some(the_holder()), "it starts here");

    harness.get_by_label("Filter files").click();
    harness.run();
    assert_ne!(
        what_holds_the_keyboard(&harness),
        Some(the_holder()),
        "a click on the filter box hands the keyboard over, and it is not taken back on the next frame"
    );
    harness.get_by_label("Filter files").type_text("two");
    harness.run();
    assert_eq!(harness.state().filter, "two", "so it can be typed into");

    // Escape hands the keyboard back, and the holder has it again.
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.run();
    assert_eq!(
        what_holds_the_keyboard(&harness),
        Some(the_holder()),
        "and when the box lets go, the focus comes back rather than sitting on nothing"
    );
}

#[test]
fn a_tab_out_of_a_text_box_cannot_land_on_a_window_button() {
    // The second line of defence. `Tab` in a text box is the box's own key only while the box holds the
    // keyboard; egui moves the focus on out of it, and where it goes is whatever egui draws next that
    // can take focus. The three window buttons take no focus at all, so it cannot be one of them, and
    // whatever it is the holder takes the keyboard back on the frame after.
    let mut harness = harness("");
    harness.get_by_label("Filter files").click();
    harness.run();

    for press in 0..12 {
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert_eq!(
            a_window_button_holds_the_keyboard(&mut harness),
            None,
            "Tab number {press} out of the filter box reached a window button"
        );
        harness.key_press(egui::Key::Space);
        harness.input_mut().events.push(egui::Event::Text(" ".to_owned()));
        harness.run();
        let commands = window_commands(&harness);
        assert!(commands.is_empty(), "and a space after it sent {commands:?}");
        assert_eq!(
            what_holds_the_keyboard(&harness),
            Some(the_holder()),
            "the keyboard is back with the holder after Tab number {press}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Rearranging the panels — `task-1697`.
//
// Every one of these drives the gesture a person makes: press on the panel's header, move the
// pointer to an edge, let go. What is asserted is the window's own state read back — which side the
// panel ended up on and what rectangle it was given — rather than what the drag reported, and the
// picture is there so somebody can look at it and see that it is a panel rather than a stripe.

/// Where the pointer has to be to grab a panel by its header.
///
/// The heading word rather than the middle of the strip, because the tabs and the buttons take the
/// points they cover: the handle is added first and everything else is added on top of it, which is
/// exactly what `components::dock` says it is left with.
fn panel_handle(harness: &Harness<'static, QuillApp>, label: &str) -> egui::Pos2 {
    let header = harness.get_by_label(label).rect();
    egui::pos2(header.left() + 40.0, header.center().y)
}

/// Press on a panel's header and move the pointer to `to`, **without letting go**.
///
/// What a screenshot of the drop zones is taken of.
fn carry(harness: &mut Harness<'static, QuillApp>, from: egui::Pos2, to: egui::Pos2) {
    harness.input_mut().events.push(egui::Event::PointerMoved(from));
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::default(),
    });
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerMoved(to));
    harness.run();
}

fn side_of(
    harness: &Harness<'static, QuillApp>,
    panel: quill_app::app::dock::Panel,
) -> quill_app::app::dock::Side {
    harness.state().panes.dock.side_of(panel)
}

#[test]
fn dragging_the_terminals_header_to_the_right_makes_it_a_column_down_that_edge() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("A document with the terminal beside it.", 12, 80);
    feed(&mut harness, b"jason.mcaffee@quill ~ % cargo build\r\n    Finished in 1.26s\r\n");
    let from = panel_handle(&harness, "Move Terminal tile");
    drag(&mut harness, from, egui::pos2(1160.0, 400.0));

    assert_eq!(side_of(&harness, Panel::Terminal), Side::Right);
    let rect = harness.state().panel_area(Panel::Terminal);
    assert!(rect.height() > 400.0, "a column down the side is as tall as the body: {rect:?}");
    assert!(rect.right() > 1170.0, "and it is against the right hand edge: {rect:?}");
    // The document gave up the room rather than being covered by it.
    assert!(harness.state().editor_area().right() <= rect.left() + 1.0);
    harness.snapshot(shot("panel_terminal_docked_right"));
}

#[test]
fn dragging_the_terminal_to_the_left_puts_it_beside_the_file_panel_rather_than_over_it() {
    // The ticket's own second sentence: "drag it to the left, and it snaps to the very left, and is
    // side by side with the file panel, or to the right of the side panel". Which of the two it is
    // depends on which side of the explorer's middle the pointer let go — the rule a tab drag
    // already follows.
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    let from = panel_handle(&harness, "Move Terminal tile");
    drag(&mut harness, from, egui::pos2(200.0, 400.0));

    assert_eq!(side_of(&harness, Panel::Terminal), Side::Left);
    assert_eq!(
        harness.state().panes.dock.panels_on(Side::Left),
        vec![Panel::Explorer, Panel::Terminal],
        "let go past the explorer's middle, so it lands after it"
    );
    let explorer = harness.state().panel_area(Panel::Explorer);
    let terminal = harness.state().panel_area(Panel::Terminal);
    assert!(explorer.width() > 0.0, "the explorer is still showing beside it");
    assert!((explorer.right() - terminal.left()).abs() < 1.0, "no gap between the two columns");
    harness.snapshot(shot("panel_terminal_docked_left_of_the_editor"));
}

#[test]
fn letting_go_before_the_file_panels_middle_puts_the_terminal_in_front_of_it() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    let from = panel_handle(&harness, "Move Terminal tile");
    drag(&mut harness, from, egui::pos2(90.0, 400.0));
    assert_eq!(
        harness.state().panes.dock.panels_on(Side::Left),
        vec![Panel::Terminal, Panel::Explorer],
        "let go before the explorer's middle, so it lands in front of it"
    );
}

#[test]
fn the_four_places_a_panel_can_be_dropped_are_drawn_while_it_is_in_the_air() {
    let mut harness = with_terminal("Dragging the terminal somewhere else.", 12, 80);
    let from = panel_handle(&harness, "Move Terminal tile");
    // Held over the right hand edge rather than let go, which is the moment the ask is about:
    // "there should be blue highlighted regions to indicate where I can drag to".
    carry(&mut harness, from, egui::pos2(1160.0, 400.0));
    harness.snapshot(shot("panel_drop_zones"));
}

#[test]
fn a_panel_let_go_over_the_document_stays_where_it_was() {
    // A drag can be thought better of, which is what the explorer's row drag and the tab drag both
    // already promise. The editing area is not a dock host.
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    let from = panel_handle(&harness, "Move Terminal tile");
    drag(&mut harness, from, egui::pos2(600.0, 300.0));
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Bottom);
}

#[test]
fn the_file_panel_can_be_dragged_into_the_strip_along_the_bottom() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = harness("The explorer is going to the bottom of the window.");
    let from = panel_handle(&harness, "Move Project");
    drag(&mut harness, from, egui::pos2(600.0, 690.0));

    assert_eq!(side_of(&harness, Panel::Explorer), Side::Bottom);
    let rect = harness.state().panel_area(Panel::Explorer);
    assert!(rect.left() < 60.0, "a strip starts at the left of the panes: {rect:?}");
    assert!(rect.bottom() > 700.0, "and reaches the bottom of them: {rect:?}");
    assert!(harness.state().editor_area().left() < 90.0, "the document has the left back: {}", harness.state().editor_area().left());
    harness.snapshot(shot("panel_explorer_docked_bottom"));
}

#[test]
fn a_panel_can_be_dragged_to_the_top_of_the_window() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    let from = panel_handle(&harness, "Move Terminal tile");
    drag(&mut harness, from, egui::pos2(600.0, 60.0));
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Top);
    let rect = harness.state().panel_area(Panel::Terminal);
    assert!(rect.top() < 60.0, "a strip along the top starts at the top of the panes: {rect:?}");
    harness.snapshot(shot("panel_terminal_docked_top"));
}

#[test]
fn a_panel_that_has_moved_is_resized_by_the_edge_that_faces_the_document() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    harness.state_mut().dock_the_panel(Panel::Terminal, Side::Right, None);
    harness.run();
    let before = harness.state().panes.terminal_width;
    // The divider is on its **left** now rather than along its top, because that is the edge between
    // it and the document. Its name has not changed, which is what keeps `Resize terminal` meaning
    // the same thing to a test and to assistive technology.
    let handle = harness.get_by_label("Resize terminal").rect();
    drag(&mut harness, handle.center(), egui::pos2(handle.center().x - 120.0, handle.center().y));
    let after = harness.state().panes.terminal_width;
    assert!(after > before + 100.0, "dragging it left made the column wider: {before} to {after}");
    assert_eq!(
        harness.state().panes.terminal_height,
        settings::TERMINAL_HEIGHT,
        "its other measurement is untouched"
    );
}

#[test]
fn two_tiles_on_two_different_sides_are_both_showing_at_once() {
    // The rule `task-1683` wrote three times was "the bottom holds one of the three and never two",
    // and its reason was that two grids in one strip are two half-sized grids. Since the rule is
    // about a strip, it follows the strip.
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    harness.state_mut().dock_the_panel(Panel::Terminal, Side::Right, None);
    harness.run();
    harness.state_mut().show_the_run_tile(true);
    harness.run();
    assert!(harness.state().terminal.visible, "the terminal is on another side, so it stays");
    assert!(harness.state().run.visible);

    // And back on the same side they take turns again.
    harness.state_mut().dock_the_panel(Panel::Terminal, Side::Bottom, None);
    harness.run();
    harness.state_mut().show_the_terminal_tile(true);
    harness.run();
    assert!(!harness.state().run.visible, "two grids never share one strip");
}

#[test]
fn a_panels_own_menu_moves_it_and_reset_puts_every_panel_back() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    let header = harness.get_by_label("Move Terminal tile").rect();
    harness.state_mut().panel_menu = Some((header.center(), Panel::Terminal));
    harness.run();
    harness.get_by_label("Move to Right").click();
    harness.run();
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Right);

    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::ResetPanelLayout, &ctx);
    harness.run();
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Bottom);
    assert_eq!(side_of(&harness, Panel::Explorer), Side::Left);
}

#[test]
fn a_panel_that_is_put_away_is_moved_from_its_button_in_the_rail() {
    // A panel with no header has nothing to grab, so the rail's right click is the way back. It is
    // also the only control that is in the same place whether a panel is showing or not.
    use quill_app::app::dock::{Panel, Side};
    let mut harness = harness("");
    let ctx = harness.ctx.clone();
    harness.state_mut().run_action(Action::ToggleExplorer, &ctx);
    harness.run();
    assert!(!harness.state().explorer_visible);
    let button = harness.get_by_label("Project").rect();
    harness.state_mut().panel_menu = Some((button.center(), Panel::Explorer));
    harness.run();
    harness.get_by_label("Move to Bottom").click();
    harness.run();
    assert_eq!(side_of(&harness, Panel::Explorer), Side::Bottom);
}

#[test]
fn where_the_panels_are_survives_being_written_to_the_settings_file_and_read_back() {
    use quill_app::app::dock::{Panel, Side};
    let mut panes = settings::Panes::new();
    panes.dock.dock(Panel::Terminal, Side::Right, None);
    panes.dock.dock(Panel::Explorer, Side::Bottom, None);
    let mut values = quill_app::services::store::Values::new();
    panes.write_into(&mut values);
    let read = settings::Panes::read_from(&values);
    assert_eq!(read.dock.side_of(Panel::Terminal), Side::Right);
    assert_eq!(read.dock.side_of(Panel::Explorer), Side::Bottom);
}

#[test]
fn the_command_line_moves_a_panel_and_says_where_everything_is() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);

    let listed = did(&mut harness, "panel list");
    let panels = listed["panels"].as_array().expect("a list of panels").clone();
    assert_eq!(panels.len(), 4, "every panel is listed, showing or not");
    let terminal = panels.iter().find(|it| it["panel"] == "terminal").expect("the terminal");
    assert_eq!(terminal["side"], "bottom");
    assert_eq!(terminal["showing"], true);
    assert!(terminal["area"]["width"].as_f64().unwrap_or_default() > 100.0, "and where it is");

    let moved = did(&mut harness, "panel dock terminal right");
    assert_eq!(moved["side"], "right");
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Right);
    // And the rectangle it reports is the one it was actually given.
    let listed = did(&mut harness, "panel list");
    let terminal = listed["panels"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|it| it["panel"] == "terminal")
        .cloned()
        .expect("the terminal");
    let rect = harness.state().panel_area(Panel::Terminal);
    assert_eq!(terminal["area"]["width"].as_f64().unwrap_or_default() as f32, rect.width());

    did(&mut harness, "panel reset");
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Bottom);
}

#[test]
fn the_command_line_says_where_in_a_side_a_panel_goes() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    did(&mut harness, "panel dock terminal left --position 0");
    assert_eq!(
        harness.state().panes.dock.panels_on(Side::Left),
        vec![Panel::Terminal, Panel::Explorer],
        "position 0 is the outermost column"
    );
    did(&mut harness, "panel dock terminal left --position 1");
    assert_eq!(
        harness.state().panes.dock.panels_on(Side::Left),
        vec![Panel::Explorer, Panel::Terminal]
    );
}

#[test]
fn a_panel_size_names_the_measurement_the_side_it_is_on_reads() {
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    did(&mut harness, "panel size terminal --width 500 --height 320");
    assert_eq!(harness.state().panes.terminal_width, 500.0);
    assert_eq!(harness.state().panes.terminal_height, 320.0);
    // At the bottom the height is what is used; on the right the width is, and neither has been
    // lost by moving it.
    assert!((harness.state().panel_area(Panel::Terminal).height() - 320.0).abs() < 1.0);
    did(&mut harness, "panel dock terminal right");
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Right);
    assert!((harness.state().panel_area(Panel::Terminal).width() - 500.0).abs() < 1.0);
}

#[test]
fn a_panel_nobody_has_is_refused_with_the_ones_quill_does_have() {
    let mut harness = harness("");
    assert_eq!(refused(&mut harness, "panel dock outline left"), "not-found");
    assert_eq!(refused(&mut harness, "panel dock terminal sideways"), "usage");
}

#[test]
fn status_says_which_edge_each_panel_is_on() {
    // An agent that reads `status` and then works out where to click has to be told, because since
    // `task-1697` the terminal is not necessarily along the bottom.
    let mut harness = with_terminal("", 12, 80);
    did(&mut harness, "panel dock terminal right");
    let status = did(&mut harness, "status");
    let panels = status["panels"].as_array().expect("the panels").clone();
    let terminal = panels.iter().find(|it| it["panel"] == "terminal").expect("the terminal");
    assert_eq!(terminal["side"], "right");
}

#[test]
fn every_menu_row_for_moving_a_panel_can_be_run_from_the_command_line() {
    // The four `Move to` rows are on a context menu rather than in the bar, so `action list` does
    // not carry them — `action run` does, which is the guarantee the whole naming scheme exists for.
    use quill_app::app::dock::{Panel, Side};
    let mut harness = with_terminal("", 12, 80);
    did(&mut harness, "action run dock-terminal-top");
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Top);
    did(&mut harness, "action run dock-explorer-right");
    assert_eq!(side_of(&harness, Panel::Explorer), Side::Right);
    did(&mut harness, "action run reset-panel-layout");
    assert_eq!(side_of(&harness, Panel::Terminal), Side::Bottom);
    assert_eq!(side_of(&harness, Panel::Explorer), Side::Left);
}
