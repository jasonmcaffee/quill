//! What a board's decoration costs, measured rather than asserted.
//!
//! `cargo run --release -p quill-app --example vello_cost -- [lanes] [cards] [width] [height]`
//!
//! In the shape of `frame_cost.rs`, `symbol_cost.rs`, `completion_cost.rs` and `folding_cost.rs`, and for
//! the same reason: a threshold in milliseconds would be a different number on every machine, so this is an
//! example that prints what it found rather than a test that fails. What *is* a test is the work itself —
//! how many `Decor` items a board of a given shape produces, and that a frame in which nothing moved
//! produces the same fingerprint and rasterises nothing.
//!
//! The three numbers to read:
//!
//! - **recording** — building the `Decor` list, which happens on **every** frame. It has to be almost free.
//! - **rasterising** — turning that list into pixels, which happens only on a frame where the board changed:
//!   a drag, a scroll, a letter typed into the search box, or a ticket moving. Not a hover — the decoration
//!   is deliberately the same under the pointer, and the pointer's answer is a wash `egui` paints over it.
//!   The budget is a third of a frame at sixty a second, so about 5 ms.
//! - **still** — a frame where nothing moved, which is the common case and must be a hash comparison.

use std::time::Instant;

use egui::{Color32, Pos2, Rect, Vec2};
use quill_app::services::vello_canvas::{Canvas, Chrome, Fill, Lift};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let lanes: usize = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(4);
    let cards: usize = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(8);
    let width: f32 = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(1400.0);
    let height: f32 = arguments.next().and_then(|value| value.parse().ok()).unwrap_or(900.0);
    let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));

    println!("A board of {lanes} lanes holding {cards} cards each, in {width} x {height} points.\n");

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

    // Rasterising, which only a frame where something changed pays. Twice over: with the SIMD level this
    // processor really has, which is what the window uses, and with the baseline the screenshot tests pin.
    for (what, pixels_per_point) in
        [("window", 1.0_f32), ("window", 2.0), ("pinned", 1.0), ("pinned", 2.0)]
    {
        let mut canvas = match what {
            "pinned" => Canvas::for_tests(),
            _ => Canvas::default(),
        };
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        // Once to warm the pixmap and the renderer, then measured.
        canvas.texture_for(&ctx, "cost", area, pixels_per_point, &items);
        let began = Instant::now();
        let rounds = 10;
        for round in 0..rounds {
            // A different list each time, or the fingerprint would answer instead of the renderer.
            let chrome = Chrome::recording();
            draw_a_board(&chrome, area.translate(Vec2::new(0.0, round as f32 * 0.01)), lanes, cards);
            let moved = chrome.take();
            canvas.texture_for(&ctx, "cost", area, pixels_per_point, &moved);
        }
        let rasterising = began.elapsed().as_secs_f64() * 1000.0 / f64::from(rounds);
        // What was really rasterised: the decoration's own bounding box, at a scale capped by `MAX_SCALE`.
        let scale = pixels_per_point.min(1.5);
        let drawn = canvas
            .texture_for(&ctx, "cost", area, pixels_per_point, &items)
            .map(|(_, drawn)| drawn)
            .unwrap_or(area);
        let pixels = (drawn.width() * scale) as u32 * (drawn.height() * scale) as u32;
        println!(
            "  {rasterising:>9.3} ms to rasterise at {pixels_per_point} pixels a point ({pixels} pixels drawn), on a frame that changed [{what}]"
        );

        // And a frame where nothing moved, which is the common case.
        let began = Instant::now();
        for _ in 0..1000 {
            canvas.texture_for(&ctx, "cost", area, pixels_per_point, &items);
        }
        let still = began.elapsed().as_secs_f64() * 1000.0 / 1000.0;
        println!("  {still:>9.3} ms on a frame where nothing moved [{what}]");
    }
}

/// The decoration a board of this shape records, in the order and the proportions the real one does.
fn draw_a_board(chrome: &Chrome, area: Rect, lanes: usize, cards: usize) {
    let page = Color32::from_rgb(0x1A, 0x1F, 0x26);
    let lane_colour = Color32::from_rgb(0x1F, 0x23, 0x2A);
    let card_colour = Color32::from_rgb(0x23, 0x29, 0x33);
    let well = Color32::from_rgb(0x1D, 0x21, 0x2A);
    let accent = Color32::from_rgb(0x48, 0x9F, 0xF8);
    let _ = page;

    // The header: a search well, a primary button with a glow, and four view buttons.
    let search = Rect::from_min_size(Pos2::new(area.max.x - 330.0, area.min.y + 10.0), Vec2::new(200.0, 30.0));
    chrome.sunken(search, 15.0, well, Lift::Small);
    let add = Rect::from_min_size(Pos2::new(area.max.x - 120.0, area.min.y + 10.0), Vec2::new(108.0, 30.0));
    chrome.glow(add, 14.0, accent.gamma_multiply(0.42), 9.0);
    chrome.raised(add, 14.0, Fill::diagonal(add, accent, accent), Lift::Small);
    for view in 0..4 {
        let at = Rect::from_min_size(
            Pos2::new(area.min.x + 8.0 + view as f32 * 84.0, area.min.y + 50.0),
            Vec2::new(78.0, 26.0),
        );
        match view == 0 {
            true => chrome.sunken(at, 10.0, well, Lift::Small),
            false => chrome.raised(at, 10.0, Fill::Solid(lane_colour), Lift::Small),
        }
    }

    let lane_width = 328.0;
    for lane in 0..lanes {
        let lane_area = Rect::from_min_max(
            Pos2::new(area.min.x + 8.0 + lane as f32 * (lane_width + 22.0), area.min.y + 88.0),
            Pos2::new(area.min.x + 8.0 + lane as f32 * (lane_width + 22.0) + lane_width, area.max.y - 8.0),
        );
        chrome.raised(lane_area, 18.0, Fill::Solid(lane_colour), Lift::Medium);
        chrome.clip(lane_area, 18.0);
        // The dot, its halo, and the count pressed into the lane.
        let dot = Pos2::new(lane_area.min.x + 19.0, lane_area.min.y + 23.0);
        chrome.glow(Rect::from_center_size(dot, Vec2::splat(9.0)), 4.5, accent.gamma_multiply(0.75), 3.5);
        chrome.disc(dot, 4.5, Fill::Solid(accent));
        chrome.sunken(
            Rect::from_min_size(Pos2::new(lane_area.max.x - 50.0, lane_area.min.y + 13.0), Vec2::new(36.0, 20.0)),
            10.0,
            well,
            Lift::Small,
        );
        for card in 0..cards {
            let at = Rect::from_min_size(
                Pos2::new(lane_area.min.x + 14.0, lane_area.min.y + 46.0 + card as f32 * 114.0),
                Vec2::new(lane_width - 28.0, 100.0),
            );
            chrome.raised(at, 14.0, Fill::Solid(card_colour), Lift::Small);
            let play = Rect::from_min_size(Pos2::new(at.max.x - 66.0, at.max.y - 30.0), Vec2::splat(26.0));
            chrome.glow(play, 13.0, accent.gamma_multiply(0.45), 6.0);
            chrome.disc(play.center(), 13.0, Fill::diagonal(play, accent, accent));
            let badge = Rect::from_min_size(Pos2::new(at.max.x - 34.0, at.max.y - 30.0), Vec2::splat(26.0));
            chrome.disc(badge.center(), 13.0, Fill::diagonal(badge, accent, accent));
            chrome.ring(badge.center(), 16.5, 1.6, accent);
        }
        chrome.unclip();
    }
}
