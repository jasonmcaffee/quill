//! What one frame of the editing area costs, measured with real fonts on this machine.
//!
//! `task-1666` began as a report that selecting text, scrolling and dragging the window were jagged
//! on Windows with a few tabs open. This is how that was turned into numbers rather than an
//! impression: it lays a real file out, collects the glyphs the painter collects, and prints the
//! milliseconds each part of a frame takes. Run it before and after a change and the difference is
//! visible.
//!
//! `cargo run --release -p unluminous-app --example frame_cost -- <file> [width]`
//!
//! The three lines at the end are the three things the ticket reported. Each is the work the window
//! really does for one frame of that gesture, and each has a comment beside it saying which.

use std::time::Instant;

use unluminous_app::services::text_renderer::TextRenderer;
use unluminous_core::{layout, relayout, Command, Document, Layout};

/// Run `body` `runs` times and give back the mean in milliseconds.
fn timed(runs: usize, mut body: impl FnMut()) -> f64 {
    // One untimed pass first, so a cache that fills on first use is not charged to the measurement.
    body();
    let start = Instant::now();
    for _ in 0..runs {
        body();
    }
    start.elapsed().as_secs_f64() * 1000.0 / runs as f64
}

/// Every glyph the painter would collect for the whole of a layout, which is what it used to do.
fn collect_every_glyph(renderer: &TextRenderer, laid: &Layout) -> usize {
    collect_glyphs(renderer, &laid.lines)
}

/// The same, for the lines that fall inside a window `height` points tall at `scroll`.
fn collect_visible_glyphs(renderer: &TextRenderer, laid: &Layout, scroll: f32, height: f32) -> usize {
    let visible = laid.visible_lines(scroll, scroll + height);
    collect_glyphs(renderer, &laid.lines[visible])
}

fn collect_glyphs(renderer: &TextRenderer, lines: &[unluminous_core::PlacedLine]) -> usize {
    let mut placed = 0usize;
    for line in lines {
        for run in &line.runs {
            for cluster in &run.clusters {
                for character in cluster.text.chars() {
                    if renderer.glyph(character, &run.style).is_some() {
                        placed += 1;
                    }
                }
            }
        }
    }
    placed
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().unwrap_or_else(|| "crates/unluminous-app/src/app/mod.rs".to_owned());
    let width: f32 = arguments.next().and_then(|w| w.parse().ok()).unwrap_or(900.0);
    let view_height = 720.0_f32;

    let source = std::fs::read_to_string(&path).expect("the file to measure against");
    let renderer = TextRenderer::new();
    let mut document = Document::from_text(&source);

    // Coloured, because that is how a source file is really shown and the colouring is what turns one
    // style span into thousands. Measuring against a plain document flatters every number here.
    let (plugins, _) = unluminous_app::services::plugins::Plugins::load(None);
    let coloured = plugins.for_path(std::path::Path::new(&path)).map(|plugin| {
        let base = unluminous_core::Color::rgb(0xF2, 0xF2, 0xF2);
        let start = Instant::now();
        let tokens = unluminous_core::syntax::highlight(&source, &plugin.grammar);
        let tokenised = start.elapsed().as_secs_f64() * 1000.0;
        let spans: Vec<(std::ops::Range<usize>, unluminous_core::Color)> = tokens
            .into_iter()
            .filter_map(|(range, token)| plugin.theme.colour(token).map(|colour| (range, colour)))
            .collect();
        let count = spans.len();
        let start = Instant::now();
        document.set_syntax(base, &spans);
        let applied = start.elapsed().as_secs_f64() * 1000.0;
        (tokenised, applied, count)
    });

    println!(
        "{path}: {} bytes, {} lines, laid out at {width} points wide",
        source.len(),
        document.text().len_lines()
    );
    if let Some((tokenised, applied, count)) = coloured {
        println!("  syntax highlight:        {tokenised:8.2} ms  ({count} coloured spans)");
        println!(
            "  set_syntax:              {applied:8.2} ms  ({} style spans after)",
            document.chars().span_count()
        );
    }

    let laid = layout(document.text(), document.chars(), document.paragraphs(), &renderer, width);
    println!("            lines laid out: {}", laid.lines.len());

    let ms = timed(5, || {
        let _ = layout(document.text(), document.chars(), document.paragraphs(), &renderer, width);
    });
    println!("  layout, whole document:  {ms:8.2} ms");

    let every = collect_every_glyph(&renderer, &laid);
    let ms = timed(10, || {
        std::hint::black_box(collect_every_glyph(&renderer, &laid));
    });
    println!("  glyphs, whole document:  {ms:8.2} ms  ({every} glyphs)");

    let visible = collect_visible_glyphs(&renderer, &laid, 0.0, view_height);
    let ms = timed(200, || {
        std::hint::black_box(collect_visible_glyphs(&renderer, &laid, 0.0, view_height));
    });
    println!("  glyphs, one screenful:   {ms:8.2} ms  ({visible} glyphs)");

    let end = document.text().len_bytes();
    let ms = timed(20, || {
        std::hint::black_box(laid.selection_rects(0..end));
    });
    println!("  selection_rects, all:    {ms:8.2} ms");

    let onscreen = laid.visible_lines(0.0, view_height);
    let ms = timed(2000, || {
        std::hint::black_box(laid.selection_rects_in(onscreen.clone(), 0..end));
    });
    println!("  selection_rects, shown:  {ms:8.2} ms");

    let ms = timed(20, || {
        std::hint::black_box(laid.decorations(&renderer));
    });
    println!("  decorations, whole doc:  {ms:8.2} ms");

    let style = unluminous_core::CharStyle::default();
    let ms = timed(200_000, || {
        std::hint::black_box(unluminous_core::FontMetrics::advance(&renderer, "a", &style));
    });
    println!("  one advance:             {:8.1} ns", ms * 1_000_000.0);
    let ms = timed(200_000, || {
        std::hint::black_box(renderer.glyph('a', &style));
    });
    println!("  one glyph lookup:        {:8.1} ns", ms * 1_000_000.0);

    let ms = timed(2000, || {
        std::hint::black_box(laid.line_of_offset(end));
    });
    println!("  line_of_offset, last:    {ms:8.4} ms");

    let ms = timed(2000, || {
        std::hint::black_box(laid.line_at_y(laid.height));
    });
    println!("  line_at_y, bottom:       {ms:8.4} ms");

    println!();
    println!("  and the three gestures the ticket reported:");

    // Dragging a selection. The caret moves, so the text revision does not, so nothing is laid out
    // and nothing is coloured: the frame is the selection rectangles and a screenful of glyphs.
    let ms = timed(500, || {
        std::hint::black_box(laid.selection_rects_in(onscreen.clone(), 0..end / 2));
        std::hint::black_box(collect_visible_glyphs(&renderer, &laid, 0.0, view_height));
    });
    println!("  dragging a selection:    {ms:8.2} ms  ({:.0} frames a second)", 1000.0 / ms);

    // Scrolling, and dragging the window: nothing about the document changes at all, so the frame is
    // a screenful of glyphs and nothing else.
    let ms = timed(500, || {
        std::hint::black_box(collect_visible_glyphs(&renderer, &laid, 4000.0, view_height));
    });
    println!("  scrolling or dragging:   {ms:8.2} ms  ({:.0} frames a second)", 1000.0 / ms);

    // Typing a letter. The text really did change, so it is laid out again — but only the paragraph
    // that changed — and then a screenful of glyphs is collected.
    document.apply(Command::PlaceCaret { offset: end / 2, extend: false });
    let mut carried =
        layout(document.text(), document.chars(), document.paragraphs(), &renderer, width);
    let ms = timed(50, || {
        document.apply(Command::Insert("x".to_owned()));
        carried = relayout(
            std::mem::take(&mut carried),
            document.text(),
            document.chars(),
            document.paragraphs(),
            &renderer,
            width,
            &unluminous_core::folding::Hidden::none(),
        );
        std::hint::black_box(collect_visible_glyphs(&renderer, &carried, 0.0, view_height));
    });
    println!("  typing a letter:         {ms:8.2} ms  ({:.0} frames a second)", 1000.0 / ms);

    // The same, as the window really does it: a source file is coloured again after every edit.
    //
    // **Both readings, in one run**, because the point of `task-1804` §5.2 is the difference
    // between them and a number with nothing beside it is a number nobody can judge. The first is
    // what the window did until then -- read the whole file, lay every span back over it -- and the
    // second is what `UnluminousApp::colour_the_file` does now.
    if let Some(plugin) = plugins.for_path(std::path::Path::new(&path)) {
        let base = unluminous_core::Color::rgb(0xF2, 0xF2, 0xF2);
        let ms = timed(20, || {
            document.apply(Command::Insert("y".to_owned()));
            let text = document.text().to_string();
            let spans: Vec<(std::ops::Range<usize>, unluminous_core::Color)> =
                unluminous_core::syntax::highlight(&text, &plugin.grammar)
                    .into_iter()
                    .filter_map(|(range, token)| plugin.theme.colour(token).map(|c| (range, c)))
                    .collect();
            document.set_syntax(base, &spans);
            carried = relayout(
                std::mem::take(&mut carried),
                document.text(),
                document.chars(),
                document.paragraphs(),
                &renderer,
                width,
                &unluminous_core::folding::Hidden::none(),
            );
            std::hint::black_box(collect_visible_glyphs(&renderer, &carried, 0.0, view_height));
        });
        println!("  typing, whole file read: {ms:8.2} ms  ({:.0} frames a second)", 1000.0 / ms);

        let mut cache = unluminous_core::IncrementalTokens::default();
        // One reading first, so what is timed is the incremental case rather than the first one.
        {
            let text = document.text().to_string();
            let mut spans = Vec::new();
            cache.update(&text, &plugin.grammar, document.syntax_dirt(), |range, token| {
                if let Some(colour) = plugin.theme.colour(token) {
                    spans.push((range, colour));
                }
            });
            let _: &Vec<(std::ops::Range<usize>, unluminous_core::Color)> = &spans;
            document.set_syntax(base, &spans);
        }
        let mut scanned = 0usize;
        let ms = timed(20, || {
            document.apply(Command::Insert("z".to_owned()));
            let text = document.text().to_string();
            let mut spans: Vec<(std::ops::Range<usize>, unluminous_core::Color)> = Vec::new();
            let update =
                cache.update(&text, &plugin.grammar, document.syntax_dirt(), |range, token| {
                    if let Some(colour) = plugin.theme.colour(token) {
                        spans.push((range, colour));
                    }
                });
            scanned = update.scanned;
            document.set_syntax_in(base, &spans, update.changed);
            carried = relayout(
                std::mem::take(&mut carried),
                document.text(),
                document.chars(),
                document.paragraphs(),
                &renderer,
                width,
                &unluminous_core::folding::Hidden::none(),
            );
            std::hint::black_box(collect_visible_glyphs(&renderer, &carried, 0.0, view_height));
        });
        println!(
            "  typing, coloured again:  {ms:8.2} ms  ({:.0} frames a second, {scanned} tokens read)",
            1000.0 / ms
        );
    }
}
