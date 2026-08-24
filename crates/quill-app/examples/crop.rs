//! Cut a region out of a screen capture, so the window can be looked at closely.
//!
//! `cargo run --example crop -- <in.png> <out.png> <x> <y> <width> <height>`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, x, y, width, height] = args.as_slice() else {
        eprintln!("usage: crop <in.png> <out.png> <x> <y> <width> <height>");
        std::process::exit(2);
    };
    let image = image::open(input).expect("open the capture");
    let number = |s: &String| s.parse::<u32>().expect("a whole number");
    let cropped = image::imageops::crop_imm(
        &image,
        number(x),
        number(y),
        number(width),
        number(height),
    )
    .to_image();
    cropped.save(output).expect("save the crop");
    println!("wrote {output} at {}x{}", cropped.width(), cropped.height());
}
