# Making this plugin's icon again

`icon.png` (32 by 32) and `icon-128.png` are a **programmatic placeholder**, not a generated picture.
The other bundled plugins' icons are rendered through the AI service (`plugins/css/icon.md` records
that recipe); when this one was made the image service's upstream was failing and the GPU was fully
in use, so the mark was drawn instead. `task-1717` swaps in the generated picture the way the others
have theirs, and this file is overwritten with that recipe then.

The mark is `< / >` in HTML5's orange, which is the shortest way to say "this is markup" at 32 by 32,
the size a tab and an explorer row draw it at. Two colours are not needed; one is enough, because the
shape carries the meaning rather than the palette.

## 1. Draw the mark

The drawing is a distance field over five line segments, so the strokes are even and the edges
anti-aliased, which is what a mark needs at 32 by 32. It is a one-file Rust program against the
`image` crate, which Quill already depends on. Written out here in full so the icon can be made again
without guessing:

```rust
//! Draw the HTML plugin's icon: the `< / >` mark in HTML5 orange.
use image::{Rgba, RgbaImage};

/// The mark's colour, HTML5's own orange.
const ORANGE: [u8; 3] = [227, 79, 38];
/// The canvas it is drawn on.
const SIDE: u32 = 256;
/// How thick a stroke is, as a radius in canvas pixels.
const RADIUS: f32 = 15.0;

/// The segments that make up `< / >`.
const SEGMENTS: &[(f32, f32, f32, f32)] = &[
    // The left angle bracket, two strokes meeting at the point.
    (100.0, 64.0, 58.0, 128.0),
    (58.0, 128.0, 100.0, 192.0),
    // The slash.
    (146.0, 56.0, 110.0, 200.0),
    // The right angle bracket.
    (156.0, 64.0, 198.0, 128.0),
    (198.0, 128.0, 156.0, 192.0),
];

fn main() {
    let mut canvas = RgbaImage::from_pixel(SIDE, SIDE, Rgba([0, 0, 0, 0]));
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        let mut nearest = f32::INFINITY;
        for &(ax, ay, bx, by) in SEGMENTS {
            nearest = nearest.min(distance_to_segment(px, py, ax, ay, bx, by));
        }
        // One pixel of anti-aliasing either side of the stroke's edge.
        let alpha = (RADIUS + 0.5 - nearest).clamp(0.0, 1.0) * 255.0;
        if alpha > 0.0 {
            *pixel = Rgba([ORANGE[0], ORANGE[1], ORANGE[2], alpha as u8]);
        }
    }
    canvas.save("icon-source.png").expect("save the mark");
}

/// The shortest distance from a point to a line segment.
fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let apx = px - ax;
    let apy = py - ay;
    let length_squared = abx * abx + aby * aby;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        (apx * abx + apy * aby) / length_squared
    }
    .clamp(0.0, 1.0);
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    let dx = px - cx;
    let dy = py - cy;
    (dx * dx + dy * dy).sqrt()
}
```

Save it as `src/main.rs` of a small crate whose `Cargo.toml` depends on
`image = { version = "0.25", default-features = false, features = ["png"] }`, and run
`cargo run --release` from a scratch folder so the `icon-source.png` it writes does not land in the
repository. Move the result to `_agent_output/task-1694-html-plugin/icon-source.png`.

## 2. Turn it into the two icons

```bash
cargo run --example plugin_icon -- _agent_output/task-1694-html-plugin/icon-source.png crates/quill-app/plugins/html
```

`examples/plugin_icon.rs` keys the background out by flood filling from the four corners (a no-op
here, the canvas is already transparent), crops to the mark with an even margin, squares it, and
scales it to 128 and to 32 with a smooth filter. The folder it writes into has to exist already.

## 3. Look at it

```bash
cargo run --example scale -- crates/quill-app/plugins/html/icon.png _agent_output/task-1694-html-plugin/icon-large.png 8
```

The 32 by 32 one is the one to check, because it is the size a tab and an explorer row use and it is
where a mark with too much detail in it turns to mush.
