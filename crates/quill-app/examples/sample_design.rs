//! Reads the design image and reports, for each region of interest, the colour that covers most of it and
//! the most saturated colour in it. The dominant colour gives backgrounds; the most saturated gives
//! accents such as the blue of an active button or the amber of the unsaved dot.

use std::collections::HashMap;

fn saturation(p: [u8; 4]) -> i32 {
    let max = p[0].max(p[1]).max(p[2]) as i32;
    let min = p[0].min(p[1]).min(p[2]) as i32;
    max - min
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "design/intial-design-screenshot.png".to_owned());
    let image = image::open(&path).expect("open the design image").to_rgba8();
    println!("{} is {}x{}", path, image.width(), image.height());
    // name, x, y, width, height
    let regions = [
        ("window edge, outside", 0, 0, 1264, 8),
        ("title bar", 300, 20, 200, 30),
        ("title bar unsaved dot", 700, 27, 20, 16),
        ("toolbar background", 800, 60, 250, 40),
        ("dropdown", 30, 68, 160, 24),
        ("active format button", 280, 68, 24, 24),
        ("inactive format button", 314, 68, 20, 24),
        ("active alignment button", 566, 68, 24, 24),
        ("opacity pill", 1166, 66, 56, 28),
        ("undo redo group", 1092, 66, 60, 28),
        ("explorer background", 40, 400, 200, 200),
        ("explorer heading text", 28, 116, 70, 14),
        ("filter box", 30, 144, 220, 20),
        ("folder row text", 60, 184, 60, 14),
        ("selected file pill", 30, 322, 220, 18),
        ("selected pill dot", 236, 325, 12, 12),
        ("md file square", 62, 214, 10, 10),
        ("txt file square", 62, 298, 10, 10),
        ("explorer footer", 20, 680, 220, 20),
        ("panel divider", 262, 300, 3, 200),
        ("editor background", 700, 600, 400, 100),
        ("editor heading text", 305, 138, 120, 22),
        ("editor body text", 305, 186, 560, 18),
        ("caret", 1040, 542, 8, 20),
        ("status bar", 300, 712, 400, 28),
        ("status unsaved dot", 108, 713, 12, 12),
    ];
    for (name, x, y, w, h) in regions {
        let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
        let mut most_saturated = [0u8; 4];
        let mut brightest = [0u8; 4];
        for py in y..(y + h).min(image.height()) {
            for px in x..(x + w).min(image.width()) {
                let p = image.get_pixel(px, py).0;
                *counts.entry(p).or_insert(0) += 1;
                if saturation(p) > saturation(most_saturated) {
                    most_saturated = p;
                }
                let sum = p[0] as u32 + p[1] as u32 + p[2] as u32;
                if sum > brightest[0] as u32 + brightest[1] as u32 + brightest[2] as u32 {
                    brightest = p;
                }
            }
        }
        let (dominant, count) = counts.iter().max_by_key(|(_, n)| **n).expect("region has pixels");
        let total: usize = counts.values().sum();
        println!(
            "  {name:<26} dominant #{:02X}{:02X}{:02X} ({}%)  most saturated #{:02X}{:02X}{:02X}  brightest #{:02X}{:02X}{:02X}",
            dominant[0], dominant[1], dominant[2],
            count * 100 / total,
            most_saturated[0], most_saturated[1], most_saturated[2],
            brightest[0], brightest[1], brightest[2],
        );
    }
}
