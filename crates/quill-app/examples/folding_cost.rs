//! What collapsing and expanding blocks costs on a real file, measured on this machine.
//!
//! `task-1686` puts one new piece of work in front of every text change — reading the file for the
//! blocks in it that could be collapsed — and one in front of a fold: laying the document out again
//! with some of its paragraphs hidden. `tasks/task-1686-folding-tdd.md` section 11 sets the budget,
//! and a budget nobody measures is a wish. This is the `frame_cost` and `symbol_cost` pattern
//! applied to it.
//!
//! `cargo run --release -p quill-app --example folding_cost -- <file> [width]`
//!
//! It is **not a test and nothing fails it**: a threshold in milliseconds is a different number on
//! every machine, which is why `frame_cost` is not one either. What *is* a test is the work itself —
//! that a hidden paragraph produces no lines, that `relayout` with a fold agrees exactly with a full
//! layout — and those live beside the code they are about.

use std::path::PathBuf;
use std::time::Instant;

use quill_app::services::file_kind;
use quill_app::services::plugins::Plugins;
use quill_core::folding::{self, Folds, Hidden};
use quill_core::{layout, relayout, FixedMetrics, ParagraphStyles, Rope, StyleSpans};

/// Run `body` `runs` times and give back the mean in milliseconds.
fn timed(runs: usize, mut body: impl FnMut()) -> f64 {
    body();
    let start = Instant::now();
    for _ in 0..runs {
        body();
    }
    start.elapsed().as_secs_f64() * 1000.0 / runs as f64
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("crates/quill-app/src/app/mod.rs"));
    let width: f32 = arguments.next().and_then(|it| it.parse().ok()).unwrap_or(900.0);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(problem) => {
            eprintln!("{}: {problem}", path.display());
            return;
        }
    };

    // The real plugins, so the reading is the one the window would do for this file.
    let (plugins, _) = Plugins::load(None);
    let grammars = plugins.grammars();
    let reading = file_kind::folding_reading(Some(&path), &grammars);

    println!("{}", path.display());
    println!("  {} bytes, {} lines", text.len(), text.lines().count());

    let read = timed(20, || {
        let _ = folding::regions(&text, reading);
    });
    let regions = folding::regions(&text, reading);
    println!("  reading the blocks         {read:8.3} ms   {} blocks", regions.len());
    if let Some(grammar) = grammars.for_path(&path) {
        let scan = timed(20, || {
            let _ = folding::tokens(&text, grammar);
        });
        println!("    of which the tokeniser   {scan:8.3} ms");
        // What it costs in the window, where `colour_the_file` has already run the same scan over
        // the same text at the same revision and kept what came of it.
        let read = folding::tokens(&text, grammar);
        let shared = timed(20, || {
            let _ = folding::regions_from(&text, reading, &read);
        });
        println!("    with the scan shared     {shared:8.3} ms");
    }

    // Laying it out, folded and not, so the two can be compared. The stub metrics rather than the
    // machine's fonts: what is being measured here is the folding, not the shaping.
    let rope = Rope::from_str(&text);
    let spans = StyleSpans::new(rope.len_bytes(), quill_core::CharStyle::default());
    let paragraphs = ParagraphStyles::new(rope.len_lines());
    let metrics = FixedMetrics::default();

    let open = timed(5, || {
        let _ = layout(&rope, &spans, &paragraphs, &metrics, width);
    });
    let laid = layout(&rope, &spans, &paragraphs, &metrics, width);
    println!("  laying it out, nothing folded {open:5.3} ms   {} lines", laid.lines.len());

    // Every block collapsed, which is the worst a person can ask for in one press.
    let mut folds = Folds::new();
    for region in &regions {
        folds.add(rope.line_to_byte(region.head));
    }
    let hidden = Hidden::of(regions.iter().map(|region| region.body.clone()));
    let collapse = timed(5, || {
        let _ = relayout(laid.clone(), &rope, &spans, &paragraphs, &metrics, width, &hidden);
    });
    let shut = relayout(laid.clone(), &rope, &spans, &paragraphs, &metrics, width, &hidden);
    println!(
        "  collapsing every block     {collapse:8.3} ms   {} lines left, {} hidden",
        shut.lines.len(),
        hidden.count()
    );

    let expand = timed(5, || {
        let _ = relayout(shut.clone(), &rope, &spans, &paragraphs, &metrics, width, &Hidden::none());
    });
    println!("  expanding them all again   {expand:8.3} ms");

    // What a keystroke costs while a fold is closed, which is the number that has to stay small:
    // the file is read for its blocks again and the paragraph that changed is laid out again.
    let mut edited = text.clone();
    edited.insert(edited.len() / 2, 'x');
    let rope_after = Rope::from_str(&edited);
    let spans_after = StyleSpans::new(rope_after.len_bytes(), quill_core::CharStyle::default());
    let paragraphs_after = ParagraphStyles::new(rope_after.len_lines());
    // The scan shared with the colouring, which is what the window really does.
    let read_after = match reading {
        quill_core::folding::Reading::Code(grammar) => folding::tokens(&edited, grammar),
        _ => quill_core::folding::Tokens::default(),
    };
    let keystroke = timed(5, || {
        let _ = folding::regions_from(&edited, reading, &read_after);
        let _ = relayout(
            shut.clone(),
            &rope_after,
            &spans_after,
            &paragraphs_after,
            &metrics,
            width,
            &hidden,
        );
    });
    println!("  a keystroke with a fold shut {keystroke:6.3} ms");
}
