//! What a frame of the Agent-Chat pane costs while an answer is arriving, measured rather than asserted.
//!
//! `cargo run --release -p unluminous-app --example chat_cost -- [messages] [width] [height]`
//!
//! In the shape of `frame_cost.rs`, `vello_cost.rs`, `completion_cost.rs` and `folding_cost.rs`, and for
//! the same reason: a threshold in milliseconds would be a different number on every machine, so this
//! prints what it found rather than failing. What *is* a test is the work itself — that a finished
//! message is not rendered again, and that a frame in which nothing changed rasterises nothing.
//!
//! ## The one thing that changes sixty times a second
//!
//! Everything in a conversation is still except the message being written into, and that one changes on
//! every chunk. `tasks/task-1767-agent-chat-tdd.md` §5 says two things bound it: `markdown_text::Cache`
//! re-renders only when the source or the width has moved, so the ceiling is one render of one message a
//! frame; and every finished message keeps its own render, so a conversation of forty costs nothing while
//! the forty-first arrives. This measures both halves.
//!
//! ## What is outside it
//!
//! The GPU upload, `egui`'s own tessellation, and the window's other panels. So the numbers are a floor
//! for the pane rather than a frame time — which is what `vello_cost.rs` says about the board.

use std::time::Instant;

use egui::{Pos2, Rect, Vec2};
use unluminous_app::components::markdown_text::{Cache, Colors};
use unluminous_app::services::text_renderer::TextRenderer;
use unluminous_app::services::vello_canvas::{Canvas, Chrome, Fill, Lift, MAX_SCALE};
use unluminous_app::theme::color;

/// A message with the things a real answer has in it: prose, a fenced block and a table.
fn an_answer(number: usize) -> String {
    format!(
        "Answer {number}. The fingerprint does not carry the hidden flag, so `relayout` keeps a \
         paragraph whose lines have just been thrown away. That is the whole of it.\n\n\
         ```rust\nlet fingerprint = (text, style, hidden);\nif fingerprint == before {{ keep(line); }}\n```\n\n\
         | Case | Kept |\n|---|---|\n| edited | no |\n| folded | yes, which is the fault |\n\n\
         The two lines above are what `a_layout_that_changed_means_the_text_revision_moved` would have \
         caught if folding had moved the text revision, which it deliberately does not.\n"
    )
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let messages: usize = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(40);
    let width: f32 = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(404.0);
    let height: f32 = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(660.0);
    let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
    let bubble = width * 0.96 - 24.0;

    let renderer = TextRenderer::new();
    let family = "Arial".to_owned();
    let size = 14.4;
    let colours = Colors {
        text: color::text_control(),
        strong: color::text_strong(),
        code: color::git_added(),
        link: color::accent(),
        quiet: color::text_dim(),
        rule: color::divider(),
    };

    println!("A conversation of {messages} messages in a pane {width} x {height} points.");
    println!("Excluded: the GPU upload, egui's tessellation, and the rest of the window.\n");

    // Rendering one message: the parse, the style spans and the layout. This is what a frame pays for
    // the message being written into, and nothing else.
    let mut cache = Cache::default();
    let one = an_answer(0);
    let began = Instant::now();
    for round in 0..50 {
        // A different key every round, so the cache cannot answer and the render is really done.
        cache.rendered(
            &format!("cold-{round}"),
            &one,
            &renderer,
            &family,
            size,
            colours,
            bubble,
            None,
        );
    }
    let cold = began.elapsed().as_secs_f64() * 1000.0 / 50.0;
    println!("  {cold:>9.3} ms to render and lay out one {} byte answer", one.len());

    // The same message asked for again, which is what every finished message in the conversation costs
    // on every frame. It has to be nothing.
    let began = Instant::now();
    for _ in 0..10_000 {
        cache.rendered("cold-0", &one, &renderer, &family, size, colours, bubble, None);
    }
    let warm = began.elapsed().as_secs_f64() * 1000.0 / 10_000.0;
    println!("  {warm:>9.3} ms to ask for one that has not changed");

    // A whole conversation, measured the way a frame really asks: every message, in order, with only the
    // last one having changed. The first pass fills the cache; the second is the frame.
    let bodies: Vec<String> = (0..messages).map(an_answer).collect();
    let mut cache = Cache::default();
    for (index, body) in bodies.iter().enumerate() {
        cache.rendered(&format!("message-{index}"), body, &renderer, &family, size, colours, bubble, None);
    }
    let arriving = format!("{}…", &bodies[messages - 1][..bodies[messages - 1].len() / 2]);
    let began = Instant::now();
    for round in 0..50 {
        for (index, body) in bodies.iter().enumerate().take(messages - 1) {
            cache.rendered(&format!("message-{index}"), body, &renderer, &family, size, colours, bubble, None);
        }
        // The one that is arriving, a little longer every round, which is what a chunk does.
        let growing = format!("{arriving}{}", "x".repeat(round));
        cache.rendered(
            &format!("message-{}", messages - 1),
            &growing,
            &renderer,
            &family,
            size,
            colours,
            bubble,
            None,
        );
    }
    let frame = began.elapsed().as_secs_f64() * 1000.0 / 50.0;
    println!("  {frame:>9.3} ms for a whole frame of {messages} messages with the last one arriving");

    // The decoration: recording it, which every frame pays, and rasterising it, which only a frame where
    // the drawing changed pays.
    let began = Instant::now();
    let mut items = Vec::new();
    for _ in 0..100 {
        let chrome = Chrome::recording();
        draw_a_conversation(&chrome, area, messages);
        items = chrome.take();
    }
    let recording = began.elapsed().as_secs_f64() * 1000.0 / 100.0;
    println!("\n  {:>6} decoration items", items.len());
    println!("  {recording:>9.3} ms to record them, which every frame pays");

    let mut canvas = Canvas::default();
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(2.0);
    canvas.texture_for(&ctx, egui::Id::new("chat-cost"), area, 2.0, &items);
    let lists: Vec<Vec<_>> = (1..=10)
        .map(|round| {
            let chrome = Chrome::recording();
            draw_a_conversation(&chrome, area.translate(Vec2::new(0.0, round as f32 * 0.01)), messages);
            chrome.take()
        })
        .collect();
    let began = Instant::now();
    for list in &lists {
        canvas.texture_for(&ctx, egui::Id::new("chat-cost"), area, 2.0, list);
    }
    let rasterising = began.elapsed().as_secs_f64() * 1000.0 / lists.len() as f64;
    println!(
        "  {rasterising:>9.3} ms to rasterise at {} pixels a point on a changed frame",
        2.0_f32.min(MAX_SCALE)
    );
    let began = Instant::now();
    for _ in 0..1000 {
        canvas.texture_for(&ctx, egui::Id::new("chat-cost"), area, 2.0, &items);
    }
    let still = began.elapsed().as_secs_f64() * 1000.0 / 1000.0;
    println!("  {still:>9.3} ms to ask for it on a frame where nothing moved");
}

/// The decoration a conversation of `messages` really records: the panel, the bubbles, the composer's
/// pill and its prompt well.
///
/// Only what the pane can show is drawn, which is what the drawing does — a bubble scrolled out of view
/// is skipped. So this is a screenful of bubbles whatever the conversation's length, which is the whole
/// point of the culling.
fn draw_a_conversation(chrome: &Chrome, area: Rect, messages: usize) {
    let panel = area.shrink(8.0);
    chrome.raised(panel, 18.0, Fill::Solid(color::explorer()), Lift::Small);
    let inner = panel.shrink(10.0);
    let body = Rect::from_min_max(
        Pos2::new(inner.left(), inner.top() + 36.0),
        Pos2::new(inner.right(), inner.bottom() - 90.0),
    );
    chrome.clip(body, 0.0);
    let mut top = body.top();
    let mut index = 0;
    while top < body.bottom() && index < messages {
        let tall = match index % 3 {
            0 => 44.0,
            _ => 96.0,
        };
        let rect = Rect::from_min_size(Pos2::new(body.left(), top), Vec2::new(body.width() * 0.9, tall));
        match index % 2 {
            0 => chrome.raised(rect, 14.0, Fill::Solid(color::code_panel()), Lift::Small),
            _ => chrome.sunken(rect, 14.0, color::code_panel(), Lift::Small),
        }
        top += tall + 12.0;
        index += 1;
    }
    chrome.unclip();
    // The composer: the pill, the prompt well, and the gradient disc with its glow.
    let pill = Rect::from_center_size(Pos2::new(inner.center().x, inner.bottom() - 76.0), Vec2::new(86.0, 28.0));
    chrome.sunken(pill, 14.0, color::field(), Lift::Small);
    let well = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - 48.0), inner.max);
    chrome.sunken(well, 18.0, color::field(), Lift::Medium);
    let disc = Rect::from_center_size(Pos2::new(well.right() - 21.0, well.center().y), Vec2::splat(32.0));
    chrome.glow(disc, 16.0, color::board_accent().gamma_multiply(0.45), 7.0);
    chrome.disc(disc.center(), 16.0, Fill::diagonal(disc, color::board_accent(), color::accent()));
}
