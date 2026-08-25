//! Draws the Quill application icon, once, at 1024 points, and writes every size and both container
//! formats the two installers ask for.
//!
//! Quill's rule is that icons are drawn rather than lettered, and the application's own icon is held
//! to the same rule. It is two colours out of `theme::color` — the window's own darks behind, and
//! `ACCENT` for the ink — so that the thing on the taskbar is recognisably the thing in the window.
//!
//! It is drawn **once, large, and downsampled**. A 3 point stroke drawn at 16 points is a smear; the
//! same stroke drawn at 1024 and averaged down is the soft legible mark that every other
//! application's small icon is.
//!
//! Run it with `cargo run --release --manifest-path installer/icon/Cargo.toml`. Its output is
//! committed, because `quill.ico` is a build input for `quill.exe` itself and a checkout must build
//! without running this first.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tiny_skia::{
    Color, FillRule, LinearGradient, Paint, PathBuilder, Pixmap, Point, Shader, Stroke, StrokeDash,
    Transform,
};

/// The side of the drawing everything else is averaged down from.
const MASTER: u32 = 1024;

/// The sizes that go into `quill.ico`. Windows asks for these seven; the three smallest are written
/// as a DIB and the rest as PNG, for the reason in `tasks/quill-installer-tdd.md`.
const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// The sizes an `.iconset` folder holds, and so the sizes `quill.icns` is built from.
const ICONSET_SIZES: [u32; 6] = [16, 32, 64, 128, 256, 512];

fn main() {
    let out = output_folder();
    let master = draw(MASTER as f32);

    let mut sizes: Vec<u32> = ICO_SIZES.to_vec();
    sizes.extend(ICONSET_SIZES);
    sizes.push(MASTER);
    sizes.sort_unstable();
    sizes.dedup();

    // One resample per size, reused by all three outputs.
    let images: Vec<(u32, Vec<u8>)> =
        sizes.iter().map(|&size| (size, resample(&master, MASTER, size))).collect();
    write_iconset(&out, &images);
    write_ico(&out.join("quill.ico"), &images);
    write_icns(&out.join("quill.icns"), &images);

    println!("wrote {}", out.display());
}

/// The folder the files are written to: `installer/icon`, found from this program's own manifest so
/// that it does not matter which directory it was run from.
fn output_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------------------------
// The drawing
// ---------------------------------------------------------------------------------------------

/// `theme::color::EDITOR`, the dark behind the text, which is the bottom of the plate.
const EDITOR: [u8; 3] = [0x1A, 0x1F, 0x26];
/// `theme::color::TITLE_BAR`, the lighter bar along the top of the window, which is the top of it.
const TITLE_BAR: [u8; 3] = [0x2A, 0x31, 0x3D];
/// `theme::color::CONTROL_BORDER`, the line round a control, and here round the plate.
const CONTROL_BORDER: [u8; 3] = [0x38, 0x3F, 0x4B];
/// `theme::color::TEXT`, ordinary text in the editor, and the feather.
const TEXT: [u8; 3] = [0xE8, 0xEB, 0xF1];
/// `theme::color::TEXT_DIM`, which shades the underside of the feather so it is not a flat cut-out.
const TEXT_DIM: [u8; 3] = [0x8B, 0x93, 0xA3];
/// `theme::color::ACCENT`, which everything switched on is drawn in, and here the ink.
const ACCENT: [u8; 3] = [0x48, 0x9F, 0xF8];

/// One of the palette's colours, opaque. They are held as bytes because `Color` cannot be built in a
/// constant.
fn colour(rgb: [u8; 3]) -> Color {
    Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255)
}

/// Draw the mark at `side` points square, and hand back the pixels.
///
/// Everything below is expressed against a 1024 point square and scaled, so the numbers can be read
/// against each other rather than against whatever size is being asked for.
fn draw(side: f32) -> Pixmap {
    let mut pixmap = Pixmap::new(side as u32, side as u32).expect("a square pixmap");
    let scale = side / 1024.0;
    let t = Transform::from_scale(scale, scale);

    plate(&mut pixmap, t);
    feather(&mut pixmap, t);
    ink(&mut pixmap, t);
    pixmap
}

/// The rounded square behind everything, in the window's own two darks, lighter at the top the way
/// the window is, with a hairline round it so the shape survives a light wallpaper.
fn plate(pixmap: &mut Pixmap, t: Transform) {
    let plate = rounded_rect(40.0, 40.0, 984.0, 984.0, 196.0);

    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = LinearGradient::new(
        Point::from_xy(512.0, 40.0),
        Point::from_xy(512.0, 984.0),
        vec![
            tiny_skia::GradientStop::new(0.0, colour(TITLE_BAR)),
            tiny_skia::GradientStop::new(1.0, colour(EDITOR)),
        ],
        tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(colour(EDITOR)));
    pixmap.fill_path(&plate, &paint, FillRule::Winding, t, None);

    let mut edge = Paint::default();
    edge.anti_alias = true;
    edge.shader = Shader::SolidColor(colour(CONTROL_BORDER));
    pixmap.stroke_path(&plate, &edge, &stroke(6.0), t, None);
}

/// The quill: a vane from the nib at the lower left up to the tip at the upper right, the barbs cut
/// into its lower edge, and the shaft down the middle of it.
fn feather(pixmap: &mut Pixmap, t: Transform) {
    // The two ends of the feather. Everything else is placed between them.
    let nib = Point::from_xy(286.0, 812.0);
    let tip = Point::from_xy(818.0, 202.0);

    // The vane: one edge up the back of the feather, the other back down its front.
    let mut vane = PathBuilder::new();
    vane.move_to(nib.x, nib.y);
    vane.cubic_to(322.0, 520.0, 470.0, 268.0, tip.x, tip.y);
    vane.cubic_to(706.0, 452.0, 566.0, 654.0, nib.x, nib.y);
    vane.close();
    let vane = vane.finish().expect("the vane is a closed path");

    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = Shader::SolidColor(colour(TEXT));
    pixmap.fill_path(&vane, &paint, FillRule::Winding, t, None);

    // The underside, shaded so the feather reads as a surface rather than a cut-out. It is the same
    // front edge, filled back to the shaft.
    let mut under = PathBuilder::new();
    under.move_to(nib.x, nib.y);
    under.cubic_to(566.0, 654.0, 706.0, 452.0, tip.x, tip.y);
    under.cubic_to(660.0, 400.0, 480.0, 630.0, nib.x, nib.y);
    under.close();
    if let Some(under) = under.finish() {
        let mut shade = Paint::default();
        shade.anti_alias = true;
        shade.shader = Shader::SolidColor(colour(TEXT_DIM));
        pixmap.fill_path(&under, &shade, FillRule::Winding, t, None);
    }

    // The barbs: short cuts back towards the shaft, in the plate's own dark, so they read as the
    // separations in a feather rather than as lines drawn on one. A dash along the front edge does
    // it in one stroke and keeps them evenly spaced however long the edge is.
    let mut edge = PathBuilder::new();
    edge.move_to(tip.x, tip.y);
    edge.cubic_to(706.0, 452.0, 566.0, 654.0, nib.x, nib.y);
    if let Some(edge) = edge.finish() {
        let mut cut = Paint::default();
        cut.anti_alias = true;
        cut.shader = Shader::SolidColor(colour(EDITOR));
        let mut dashed = stroke(30.0);
        dashed.dash = StrokeDash::new(vec![10.0, 62.0], 34.0);
        dashed.line_cap = tiny_skia::LineCap::Butt;
        pixmap.stroke_path(&edge, &cut, &dashed, t, None);
    }

    // The shaft down the middle, stopping short of the nib so the point stays solid.
    let mut shaft = PathBuilder::new();
    shaft.move_to(786.0, 246.0);
    shaft.quad_to(560.0, 470.0, 352.0, 742.0);
    if let Some(shaft) = shaft.finish() {
        let mut line = Paint::default();
        line.anti_alias = true;
        line.shader = Shader::SolidColor(colour(EDITOR));
        pixmap.stroke_path(&shaft, &line, &stroke(16.0), t, None);
    }
}

/// The stroke of ink under the nib, in `ACCENT`, tapering away to the right the way a pen leaves it.
fn ink(pixmap: &mut Pixmap, t: Transform) {
    let mut path = PathBuilder::new();
    path.move_to(284.0, 818.0);
    path.quad_to(508.0, 930.0, 796.0, 846.0);
    path.quad_to(528.0, 908.0, 272.0, 874.0);
    path.close();
    let Some(path) = path.finish() else { return };

    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = Shader::SolidColor(colour(ACCENT));
    pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
}

/// A rounded rectangle, which `tiny_skia` has no constructor for.
fn rounded_rect(left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> tiny_skia::Path {
    let r = radius.min((right - left) / 2.0).min((bottom - top) / 2.0);
    // The control point that turns a quadratic corner into something close to a quarter circle.
    let k = r * 0.5523;
    let mut path = PathBuilder::new();
    path.move_to(left + r, top);
    path.line_to(right - r, top);
    path.cubic_to(right - r + k, top, right, top + r - k, right, top + r);
    path.line_to(right, bottom - r);
    path.cubic_to(right, bottom - r + k, right - r + k, bottom, right - r, bottom);
    path.line_to(left + r, bottom);
    path.cubic_to(left + r - k, bottom, left, bottom - r + k, left, bottom - r);
    path.line_to(left, top + r);
    path.cubic_to(left, top + r - k, left + r - k, top, left + r, top);
    path.close();
    path.finish().expect("a rounded rectangle is a closed path")
}

/// A round-ended stroke of the given width. Named because every stroke here wants the same defaults.
fn stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Resampling
// ---------------------------------------------------------------------------------------------

/// Average the master drawing down to `size` points square, and hand back straight (unpremultiplied)
/// RGBA, which is what both PNG and a DIB want.
///
/// The averaging is done on the premultiplied pixels `tiny_skia` produced, because averaging colours
/// that have already been divided by their own alpha smears the colour of the transparent pixels
/// round an edge into the visible ones. Dividing happens once, at the end.
fn resample(master: &Pixmap, from: u32, size: u32) -> Vec<u8> {
    let source = master.data();
    let ratio = from as f32 / size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        let top = (y as f32 * ratio).floor() as u32;
        let bottom = (((y + 1) as f32 * ratio).ceil() as u32).min(from);
        for x in 0..size {
            let left = (x as f32 * ratio).floor() as u32;
            let right = (((x + 1) as f32 * ratio).ceil() as u32).min(from);

            let mut sums = [0f32; 4];
            let mut count = 0f32;
            for sy in top..bottom {
                for sx in left..right {
                    let at = ((sy * from + sx) * 4) as usize;
                    for channel in 0..4 {
                        sums[channel] += source[at + channel] as f32;
                    }
                    count += 1.0;
                }
            }

            let at = ((y * size + x) * 4) as usize;
            let alpha = sums[3] / count;
            if alpha <= 0.5 {
                continue; // Already zero, and dividing by it would be a division by nothing.
            }
            for channel in 0..3 {
                let straight = (sums[channel] / count) * 255.0 / alpha;
                out[at + channel] = straight.round().clamp(0.0, 255.0) as u8;
            }
            out[at + 3] = alpha.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The three outputs
// ---------------------------------------------------------------------------------------------

/// The straight RGBA of one already-resampled size.
fn at(images: &[(u32, Vec<u8>)], size: u32) -> &[u8] {
    &images.iter().find(|(known, _)| *known == size).expect("every size was resampled").1
}

/// Encode straight RGBA as a PNG in memory.
fn png(pixels: &[u8], size: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Best);
        let mut writer = encoder.write_header().expect("a PNG header");
        writer.write_image_data(pixels).expect("PNG pixels");
    }
    bytes
}

/// Write `Quill.iconset`, which is the folder `iconutil` on a Mac turns into an `.icns`, and which is
/// also the plain PNG set anything else can read.
fn write_iconset(out: &Path, images: &[(u32, Vec<u8>)]) {
    let folder = out.join("Quill.iconset");
    fs::create_dir_all(&folder).expect("the iconset folder");
    // Apple's names: a size, and the same picture again at twice it for a retina screen.
    for (name, size) in [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ] {
        fs::write(folder.join(name), png(at(images, size), size)).expect("an iconset picture");
    }
}

/// Write `quill.ico`.
///
/// Six bytes of header, sixteen per image, then the images. The three smallest are a DIB — a
/// `BITMAPINFOHEADER` with double height, bottom-up BGRA rows and an all-zero AND mask, because the
/// alpha channel does the masking — and the rest are PNG, which Windows has read since Vista and
/// which is a great deal smaller at 128 and 256.
fn write_ico(path: &Path, images: &[(u32, Vec<u8>)]) {
    let bodies: Vec<(u32, Vec<u8>)> = ICO_SIZES
        .iter()
        .map(|&size| {
            let body =
                if size <= 32 { dib(at(images, size), size) } else { png(at(images, size), size) };
            (size, body)
        })
        .collect();

    let mut file = Vec::new();
    file.extend_from_slice(&0u16.to_le_bytes()); // reserved
    file.extend_from_slice(&1u16.to_le_bytes()); // 1 = an icon rather than a cursor
    file.extend_from_slice(&(bodies.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * bodies.len() as u32;
    for (size, body) in &bodies {
        // 256 is written as zero, which is the format's way of saying "the big one".
        let side = if *size >= 256 { 0u8 } else { *size as u8 };
        file.push(side);
        file.push(side);
        file.push(0); // no colour palette
        file.push(0); // reserved
        file.extend_from_slice(&1u16.to_le_bytes()); // planes
        file.extend_from_slice(&32u16.to_le_bytes()); // bits a pixel
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&offset.to_le_bytes());
        offset += body.len() as u32;
    }
    for (_, body) in &bodies {
        file.extend_from_slice(body);
    }

    let mut handle = fs::File::create(path).expect("quill.ico");
    handle.write_all(&file).expect("quill.ico");
}

/// One image inside an `.ico`, in the device independent bitmap form.
fn dib(pixels: &[u8], size: u32) -> Vec<u8> {
    let mask_row = (((size + 31) / 32) * 4) as usize; // each row padded to four bytes
    let mask_len = mask_row * size as usize;
    let colour_len = (size * size * 4) as usize;

    let mut out = Vec::with_capacity(40 + colour_len + mask_len);
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // colour rows and mask rows together
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits a pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // no compression
    out.extend_from_slice(&((colour_len + mask_len) as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes()); // pixels a metre across, not stated
    out.extend_from_slice(&0i32.to_le_bytes()); // and down
    out.extend_from_slice(&0u32.to_le_bytes()); // colours used
    out.extend_from_slice(&0u32.to_le_bytes()); // colours that matter

    for y in (0..size).rev() {
        for x in 0..size {
            let at = ((y * size + x) * 4) as usize;
            out.push(pixels[at + 2]); // blue
            out.push(pixels[at + 1]); // green
            out.push(pixels[at]); // red
            out.push(pixels[at + 3]); // alpha
        }
    }
    out.extend(std::iter::repeat(0u8).take(mask_len));
    out
}

/// Write `quill.icns`.
///
/// The four bytes `icns`, a big-endian total length, then one chunk per picture: a four byte type, a
/// big-endian length that counts the eight bytes of its own header, and a PNG. macOS has taken PNG in
/// these chunks since 10.8; the run-length encoded `is32`/`s8mk` pair it wanted before that is not
/// written, because nothing left reads it.
fn write_icns(path: &Path, images: &[(u32, Vec<u8>)]) {
    // The type each size goes in. `ic11` to `ic14` are the retina pairs: the same picture, filed
    // under the smaller size it stands in for.
    let chunks: [(&[u8; 4], u32); 10] = [
        (b"ic04", 16),
        (b"ic05", 32),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ];

    let mut body = Vec::new();
    for (kind, size) in chunks {
        let picture = png(at(images, size), size);
        body.extend_from_slice(kind);
        body.extend_from_slice(&((picture.len() + 8) as u32).to_be_bytes());
        body.extend_from_slice(&picture);
    }

    let mut file = Vec::with_capacity(body.len() + 8);
    file.extend_from_slice(b"icns");
    file.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    file.extend_from_slice(&body);

    fs::write(path, file).expect("quill.icns");
}
