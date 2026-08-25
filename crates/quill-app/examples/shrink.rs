//! Shrink a picture to a given width, for looking at a whole screen capture at once.
//!
//! `cargo run --example shrink -- <in.png> <out.png> <width>`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let image = image::open(&args[0]).expect("open");
    let width: u32 = args[2].parse().unwrap_or(1200);
    let height = image.height() * width / image.width();
    let scaled = image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle);
    scaled.save(&args[1]).expect("save");
    println!("{}x{} -> {}x{}", image.width(), image.height(), width, height);
}
