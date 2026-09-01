//! What a board's decoration costs, measured rather than asserted.
//!
//! `cargo run --release -p quill-app --example vello_cost -- [lanes] [cards] [width] [height]`
//!
//! In the shape of `frame_cost.rs`, `symbol_cost.rs`, `completion_cost.rs` and `folding_cost.rs`, and for
//! the same reason: a threshold in milliseconds would be a different number on every machine, so this is an
//! example that prints what it found rather than a test that fails. What *is* a test is the work itself —
//! how many `Decor` items a board of a given shape produces, that the same board twice gives an identical
//! list, and that a frame in which nothing moved rasterises nothing.
//!
//! ## What it does and does not measure, said plainly
//!
//! It is a **synthetic CPU rasterisation benchmark of the decoration alone**. It builds a `Decor` list of
//! the shape a real board produces and pushes it through the same `Canvas` the window uses. Three things
//! are deliberately outside it, and each of them is a real cost the window pays:
//!
//! - **The GPU upload.** `TextureHandle::set` queues a texture delta; eframe and wgpu upload it later in
//!   the frame. Nothing here measures that.
//! - **The rest of the board's frame.** Laying the lanes out, filtering the tickets, laying out every
//!   galley of text, and egui's own tessellation. The `still` figure is the cost of *asking for the
//!   decoration* on a frame where nothing changed, not the cost of that frame.
//! - **The `ColorImage` the raster is copied into**, one allocation of width x height x 4 bytes on every
//!   changed frame, is inside the changed-frame figure rather than broken out of it.
//!
//! So the changed-frame number is a floor for the decoration, not a frame time.
//!
//! ## The three numbers
//!
//! - **recording** — building the `Decor` list, which happens on **every** frame. It has to be almost free.
//! - **rasterising** — turning that list into pixels, which happens only on a frame where the drawing
//!   changed: a drag, a scroll, a letter typed into the search box, or a ticket moving. Not a hover — the
//!   decoration is deliberately the same under the pointer, and the pointer's answer is a wash `egui`
//!   paints over it.
//! - **still** — asking for it on a frame where nothing moved, which is the common case.

use std::time::Instant;

use egui::{Color32, Pos2, Rect, Vec2};
use quill_app::services::vello_canvas::{Canvas, Chrome, Fill, Lift, MAX_SCALE};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let lanes: usize = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(4);
    let cards: usize = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(6);
    let width: f32 = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(1400.0);
    let height: f32 = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(900.0);
    let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));

    println!("A board of {lanes} lanes holding {cards} cards each, in {width} x {height} points.");
    println!("Excluded: the GPU upload, the board's own layout and text, and everything egui does.\n");

    // Recording, which is what every frame pays.
    let began = Instant::now();
    let mut items = Vec::new();
    for _ in 0..100 {
        let chrome = Chrome::recording();
        draw_a_board(&chrome, area, lanes, cards);
        items = chrome.take();
    }
    let recording = began.elapsed().as_secs_f64() * 1000.0 / 100.0;
    println!("  {:>6} decoration items", items.len());
    println!("  {recording:>9.3} ms to record them, which every frame pays");

    // Rasterising, which only a frame where the drawing changed pays. Twice over: with the SIMD level this
    // processor really has, which is what the window uses, and with the baseline the screenshot tests pin.
    for (what, asked_for) in [("window", 1.0_f32), ("window", 2.0), ("pinned", 1.0), ("pinned", 2.0)] {
        let mut canvas = match what {
            "pinned" => Canvas::for_tests(),
            _ => Canvas::default(),
        };
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(asked_for);
        // Once to build the pixmap and the renderer, then measured.
        canvas.texture_for(&ctx, egui::Id::new("cost"), area, asked_for, &items);

        // **Every measured round is a different drawing.** An earlier version started at zero, so its first
        // round asked for the list it had just warmed with — a cache hit divided into the total, which made
        // the average about a tenth light. The lists are built before the clock starts, so the recording is
        // not counted twice either.
        let rounds = 10;
        let lists: Vec<Vec<_>> = (1..=rounds)
            .map(|round| {
                let chrome = Chrome::recording();
                draw_a_board(&chrome, area.translate(Vec2::new(0.0, round as f32 * 0.01)), lanes, cards);
                chrome.take()
            })
            .collect();
        let began = Instant::now();
        for list in &lists {
            canvas.texture_for(&ctx, egui::Id::new("cost"), area, asked_for, list);
        }
        let rasterising = began.elapsed().as_secs_f64() * 1000.0 / rounds as f64;

        // What was really rasterised: the decoration's own bounding box, at a scale capped by `MAX_SCALE`.
        let scale = asked_for.min(MAX_SCALE);
        let drawn = canvas
            .texture_for(&ctx, egui::Id::new("cost"), area, asked_for, &items)
            .map(|(_, drawn)| drawn)
            .unwrap_or(area);
        let pixels = (drawn.width() * scale) as u32 * (drawn.height() * scale) as u32;
        let capped = match scale < asked_for {
            true => format!(", capped from the {asked_for} the display asked for"),
            false => String::new(),
        };
        println!(
            "  {rasterising:>9.3} ms to rasterise at {scale} pixels a point{capped} \u{2014} {pixels} pixels \u{2014} on a changed frame [{what}]"
        );

        // And a frame where nothing moved, which is the common case. This is `Canvas::texture_for` alone:
        // the real frame also rebuilds the `Decor` list, which is the `recording` figure above, and lays the
        // board out, which nothing here measures.
        let began = Instant::now();
        for _ in 0..1000 {
            canvas.texture_for(&ctx, egui::Id::new("cost"), area, asked_for, &items);
        }
        let still = began.elapsed().as_secs_f64() * 1000.0 / 1000.0;
        println!("  {still:>9.3} ms to ask for it on a frame where nothing moved [{what}]");
    }
}

/// The decoration a board of this shape records, in the order and the proportions the real one does.
fn draw_a_board(chrome: &Chrome, area: Rect, lanes: usize, cards: usize) {
    let lane_colour = Color32::from_rgb(0x1F, 0x23, 0x2A);
    let card_colour = Color32::from_rgb(0x23, 0x29, 0x33);
    let well = Color32::from_rgb(0x1D, 0x21, 0x2A);
    let accent = Color32::from_rgb(0x48, 0x9F, 0xF8);

    // The rail down the left, with its four view buttons and the chosen one lit.
    let rail = Rect::from_min_size(Pos2::new(area.min.x + 16.0, area.min.y + 8.0), Vec2::new(52.0, 198.0));
    chrome.raised(rail, 26.0, Fill::Solid(lane_colour), Lift::Medium);
    for view in 0..4 {
        let at = Rect::from_center_size(
            Pos2::new(rail.center().x, rail.min.y + 30.0 + view as f32 * 46.0),
            Vec2::splat(36.0),
        );
        if view == 0 {
            chrome.glow(at, 12.0, accent.gamma_multiply(0.45), 8.0);
            chrome.rect(at, 12.0, Fill::diagonal(at, accent, accent));
        }
    }

    // The header: a search well and a primary button with a glow.
    let search =
        Rect::from_min_size(Pos2::new(area.max.x - 620.0, area.min.y + 10.0), Vec2::new(460.0, 44.0));
    chrome.sunken(search, 22.0, well, Lift::Small);
    let add = Rect::from_min_size(Pos2::new(area.max.x - 140.0, area.min.y + 10.0), Vec2::new(127.0, 44.0));
    chrome.glow(add, 14.0, accent.gamma_multiply(0.42), 9.0);
    chrome.raised(add, 14.0, Fill::diagonal(add, accent, accent), Lift::Small);

    let lane_width = 328.0;
    let left = area.min.x + 92.0;
    for lane in 0..lanes {
        let lane_area = Rect::from_min_max(
            Pos2::new(left + lane as f32 * (lane_width + 22.0), area.min.y + 70.0),
            Pos2::new(left + lane as f32 * (lane_width + 22.0) + lane_width, area.max.y - 8.0),
        );
        chrome.raised(lane_area, 18.0, Fill::Solid(lane_colour), Lift::Medium);
        chrome.clip(lane_area, 18.0);
        // The dot, its halo, and the count pressed into the lane.
        let dot = Pos2::new(lane_area.min.x + 19.0, lane_area.min.y + 28.0);
        chrome.glow(Rect::from_center_size(dot, Vec2::splat(9.0)), 4.5, accent.gamma_multiply(0.9), 5.0);
        chrome.disc(dot, 4.5, Fill::Solid(accent));
        chrome.sunken(
            Rect::from_min_size(
                Pos2::new(lane_area.max.x - 54.0, lane_area.min.y + 17.0),
                Vec2::new(40.0, 22.0),
            ),
            11.0,
            well,
            Lift::Small,
        );
        for card in 0..cards {
            let at = Rect::from_min_size(
                Pos2::new(lane_area.min.x + 14.0, lane_area.min.y + 57.0 + card as f32 * 124.0),
                Vec2::new(lane_width - 28.0, 100.0),
            );
            chrome.raised(at, 14.0, Fill::Solid(card_colour), Lift::Small);
            let play = Rect::from_min_size(Pos2::new(at.max.x - 74.0, at.max.y - 32.0), Vec2::splat(30.0));
            chrome.glow(play, 15.0, accent.gamma_multiply(0.45), 6.0);
            chrome.disc(play.center(), 15.0, Fill::diagonal(play, accent, accent));
            let badge = Rect::from_min_size(Pos2::new(at.max.x - 36.0, at.max.y - 31.0), Vec2::splat(28.0));
            chrome.disc(badge.center(), 14.0, Fill::diagonal(badge, accent, accent));
            chrome.ring(badge.center(), 17.5, 2.0, accent);
        }
        chrome.unclip();
    }
}
