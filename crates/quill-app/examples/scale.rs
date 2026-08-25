//! Blow a picture up so single pixels can be seen, for looking closely at a glyph or a divider.
//!
//! `cargo run --example scale -- <in.png> <out.png> <factor>`
//!
//! Nearest neighbour, so a pixel becomes a square of pixels rather than being blurred into its neighbours.
//! Used with `crop` this is how a screenshot test's image is examined: cut out the part in question, blow it
//! up, and look at it.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let image = image::open(&args[0]).expect("open");
    let factor: u32 = args[2].parse().unwrap_or(3);
    let scaled = image::imageops::resize(&image, image.width()*factor, image.height()*factor, image::imageops::FilterType::Nearest);
    scaled.save(&args[1]).expect("save");
}
