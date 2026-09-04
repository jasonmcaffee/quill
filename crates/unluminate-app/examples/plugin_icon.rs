//! Turn a generated picture into a plugin's two icons.
//!
//! `cargo run --example plugin_icon -- <in.png> <folder>`
//!
//! The generated picture is a square with the mark in the middle of a flat background, at whatever
//! size the renderer produced. A plugin wants two files: `icon-128.png`, which is what the plugins
//! page shows, and `icon.png` at 32 by 32, which is what a tab and an explorer row need.
//!
//! Three steps, and each is here rather than done by hand so the icons can be made again exactly:
//!
//! 1. **Key the background out.** The renderer emits opaque pixels, so the flat background is
//!    flood-filled from the four corners to transparent. Done by colour distance rather than by an
//!    exact match, because the render has a little noise in it.
//! 2. **Crop to the mark**, with a small even margin, so the icon fills its square rather than
//!    floating in the middle of one.
//! 3. **Scale to each size** with a smooth filter, because these are being made much smaller and
//!    nearest neighbour would leave the arrows ragged.

use image::{Rgba, RgbaImage};

/// How far a pixel's colour may be from the background's and still count as background.
const TOLERANCE: i32 = 60;
/// How much clear space is left round the mark, as a fraction of the cropped size.
const MARGIN: f32 = 0.06;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, folder] = args.as_slice() else {
        eprintln!("usage: plugin_icon <in.png> <folder>");
        std::process::exit(2);
    };
    let image = image::open(input).expect("open the picture").to_rgba8();
    let keyed = key_out_the_background(&image);
    let cropped = crop_to_the_mark(&keyed);
    for size in [128_u32, 32] {
        let scaled = image::imageops::resize(
            &cropped,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        );
        let name = if size == 32 { "icon.png".to_owned() } else { format!("icon-{size}.png") };
        let path = std::path::Path::new(folder).join(&name);
        scaled.save(&path).expect("save the icon");
        println!("{} {}x{}", path.display(), size, size);
    }
}

/// Flood fill from the four corners, so the flat background becomes transparent.
///
/// From the corners rather than by matching a colour everywhere, because a node in the mark may be
/// close to the background's own colour and must not be eaten.
fn key_out_the_background(image: &RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    let background = image.get_pixel(0, 0).0;
    let mut out = image.clone();
    let mut seen = vec![false; (width * height) as usize];
    let mut queue: Vec<(u32, u32)> = vec![
        (0, 0),
        (width - 1, 0),
        (0, height - 1),
        (width - 1, height - 1),
    ];
    while let Some((x, y)) = queue.pop() {
        let at = (y * width + x) as usize;
        if seen[at] {
            continue;
        }
        seen[at] = true;
        let here = image.get_pixel(x, y).0;
        if distance(here, background) > TOLERANCE {
            continue;
        }
        out.put_pixel(x, y, Rgba([here[0], here[1], here[2], 0]));
        if x > 0 {
            queue.push((x - 1, y));
        }
        if x + 1 < width {
            queue.push((x + 1, y));
        }
        if y > 0 {
            queue.push((x, y - 1));
        }
        if y + 1 < height {
            queue.push((x, y + 1));
        }
    }
    out
}

fn distance(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).sum()
}

/// Cut the picture down to what is not transparent, leaving an even margin, and make it square.
fn crop_to_the_mark(image: &RgbaImage) -> RgbaImage {
    let (width, height) = image.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0_u32, 0_u32);
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel.0[3] < 16 {
            continue;
        }
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    if right <= left || bottom <= top {
        return image.clone();
    }
    // Square, so the icon is not stretched when it is scaled to a square.
    let across = right - left + 1;
    let down = bottom - top + 1;
    let side = across.max(down);
    let side = side + (side as f32 * MARGIN * 2.0) as u32;
    let centre_x = (left + right) / 2;
    let centre_y = (top + bottom) / 2;
    let mut out = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    let from_x = centre_x as i64 - side as i64 / 2;
    let from_y = centre_y as i64 - side as i64 / 2;
    for y in 0..side {
        for x in 0..side {
            let source_x = from_x + x as i64;
            let source_y = from_y + y as i64;
            if source_x < 0 || source_y < 0 {
                continue;
            }
            let (source_x, source_y) = (source_x as u32, source_y as u32);
            if source_x >= width || source_y >= height {
                continue;
            }
            out.put_pixel(x, y, *image.get_pixel(source_x, source_y));
        }
    }
    out
}
