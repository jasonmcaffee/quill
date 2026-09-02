//! A picture opened in a tab: the decoded pixels, how far it is zoomed, and where it has been dragged to.
//!
//! `task-1658` asks to be able to look at an image without leaving the editor, so a picture is a second
//! kind of thing a tab can hold. Everything about one file lives in `app::files::OpenFile`, and this is
//! what that holds when the file is a picture rather than text.
//!
//! Three decisions worth writing down.
//!
//! **It is decoded once, when the tab is opened.** Decoding on every frame would decode a photograph
//! sixty times a second, and the texture has to be uploaded to the graphics card anyway. The pixels are
//! kept only until the upload, which needs an `egui::Context` and so cannot happen where the file is read.
//!
//! **A file that will not decode is a tab that says so**, rather than a row in the explorer that quietly
//! does nothing when it is clicked. `services::file_kind::is_image` answers from the extension alone
//! because the explorer asks it of every row; the decoder is what really knows, and this is where its
//! answer is kept.
//!
//! **Zoom starts as "fit", not as one to one.** Opening a photograph four thousand pixels across into a
//! window nine hundred points wide and showing its top left corner would be a picture viewer that has to
//! be zoomed out before it shows anything. So the picture is scaled to fit until the first time it is
//! zoomed, and from then on it holds the scale it was given.

use std::path::Path;

/// How much one step of the keyboard, or one notch of the wheel, changes the scale.
///
/// A quarter larger each time, which is what a picture viewer usually does and is a step a person can
/// see without it jumping past what they wanted.
const ZOOM_STEP: f32 = 1.25;

/// The smallest and largest a picture can be scaled to, whatever asked for it.
pub const MIN_ZOOM: f32 = 0.02;
pub const MAX_ZOOM: f32 = 40.0;

/// A picture in a tab.
pub struct Picture {
    /// How big the picture is, in its own pixels.
    pub size: [usize; 2],
    /// The pixels, held until they have been uploaded to the graphics card and then dropped.
    pending: Option<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
    /// The scale the picture is drawn at, in points to a pixel. `None` means scaled to fit, which is
    /// what a picture starts at.
    zoom: Option<f32>,
    /// How far the picture has been dragged from the middle of the viewing area, in points.
    pub offset: egui::Vec2,
    /// Why the file could not be read, when it could not be.
    pub problem: Option<String>,
    /// What a pinch has asked for that has not been given to it yet. A pinch arrives as a stream of
    /// small multipliers, and applying each one on its own is what makes a gesture feel jumpy.
    pending_zoom: f32,
}

impl Picture {
    /// Read a picture from disk. A file that will not decode becomes a picture with a reason in it,
    /// because the tab is a better place to say so than the explorer row that was clicked.
    pub fn open(path: &Path) -> Self {
        match decode(path) {
            Ok(image) => Self {
                size: image.size,
                pending: Some(image),
                texture: None,
                zoom: None,
                offset: egui::Vec2::ZERO,
                problem: None,
                pending_zoom: 1.0,
            },
            Err(problem) => Self {
                size: [0, 0],
                pending: None,
                texture: None,
                zoom: None,
                offset: egui::Vec2::ZERO,
                problem: Some(problem),
                pending_zoom: 1.0,
            },
        }
    }

    /// The picture as a texture, uploading it the first time it is asked for.
    pub fn texture(&mut self, ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
        if let Some(image) = self.pending.take() {
            // Two different answers for the two directions, which is what a picture viewer wants.
            // Zoomed in, nearest neighbour, so a picture magnified far enough to see its pixels shows
            // its pixels rather than a blur of them. Scaled down — which is what a photograph fitted
            // into the editing area is — linear, because nearest neighbour throws away most of the rows
            // and columns and leaves a large picture looking ragged and full of stray speckles.
            let options = egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..egui::TextureOptions::LINEAR
            };
            self.texture = Some(upload(ctx, format!("quill-picture-{name}"), image, options));
        }
        self.texture.clone()
    }

    /// The scale the picture is drawn at in an area this size, whether it was chosen or fitted.
    pub fn scale_in(&self, area: egui::Vec2) -> f32 {
        self.zoom.unwrap_or_else(|| self.fitted_scale(area))
    }

    /// True while the picture is scaled to fit rather than to a scale that was asked for.
    pub fn is_fitted(&self) -> bool {
        self.zoom.is_none()
    }

    /// The scale that makes the picture fit inside `area` without cropping it.
    ///
    /// A picture smaller than the area is left at its own size rather than blown up, because a small
    /// icon stretched to fill the window is not what "fit" means to anybody.
    pub fn fitted_scale(&self, area: egui::Vec2) -> f32 {
        let (width, height) = (self.size[0] as f32, self.size[1] as f32);
        if width <= 0.0 || height <= 0.0 || area.x <= 0.0 || area.y <= 0.0 {
            return 1.0;
        }
        (area.x / width).min(area.y / height).min(1.0).clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// Scale the picture by `factor`, about the middle of an area this size.
    ///
    /// The first zoom takes the scale it was being fitted at as its starting point, so pressing the
    /// keyboard's plus makes the picture a quarter bigger than it looks rather than jumping to
    /// whatever one to one happens to be.
    pub fn zoom_by(&mut self, factor: f32, area: egui::Vec2) {
        let from = self.scale_in(area);
        let to = (from * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        // Whatever has been dragged is scaled with the picture, so the point in the middle of the
        // view stays in the middle of the view.
        if from > 0.0 {
            self.offset *= to / from;
        }
        self.zoom = Some(to);
    }

    /// One step larger, or one smaller, which is what the keyboard asks for.
    pub fn step_zoom(&mut self, larger: bool, area: egui::Vec2) {
        self.zoom_by(if larger { ZOOM_STEP } else { 1.0 / ZOOM_STEP }, area);
    }

    /// A pinch, or the wheel with the zoom modifier held, which arrives as a stream of multipliers.
    ///
    /// They are gathered rather than applied one at a time for the reason the editor's own zoom gathers
    /// them: a gesture is many small multipliers a frame apart, and a picture that jumps by a quarter on
    /// each of them is unusable. Anything under a per cent is left to gather.
    pub fn zoom_by_gesture(&mut self, gesture: f32, area: egui::Vec2) {
        if (gesture - 1.0).abs() < f32::EPSILON {
            return;
        }
        self.pending_zoom *= gesture;
        if (self.pending_zoom - 1.0).abs() < 0.01 {
            return;
        }
        let factor = self.pending_zoom;
        self.pending_zoom = 1.0;
        self.zoom_by(factor, area);
    }

    /// Back to filling the viewing area, which is what `Reset Font Size` means for a picture.
    pub fn fit(&mut self) {
        self.zoom = None;
        self.offset = egui::Vec2::ZERO;
        self.pending_zoom = 1.0;
    }

    /// What the status bar says about the picture: how big it is and how far it is zoomed.
    pub fn description(&self, area: egui::Vec2) -> String {
        if let Some(problem) = &self.problem {
            return problem.clone();
        }
        format!(
            "{} \u{00D7} {} \u{00B7} {:.0}%",
            self.size[0],
            self.size[1],
            self.scale_in(area) * 100.0
        )
    }
}

/// Put a picture on the graphics card, shrinking it first if the card will not hold it.
///
/// Every graphics device has a largest texture it will take, and egui **panics** when it is handed
/// one bigger — `Texture has size 4000x1000, but the maximum texture side is 2048`. Four thousand
/// pixels across is an ordinary screenshot, so without this, opening one in a tab or writing one
/// into a Markdown document would end the program on any machine whose limit is low.
///
/// What is uploaded is smaller; what the picture *is* does not change. The status bar still says how
/// many pixels the file holds and the zoom still counts from that, because those are facts about the
/// file rather than about this machine's graphics card.
pub fn upload(
    ctx: &egui::Context,
    name: String,
    image: egui::ColorImage,
    options: egui::TextureOptions,
) -> egui::TextureHandle {
    let limit = ctx.input(|input| input.max_texture_side);
    ctx.load_texture(name, shrink_to_fit(image, limit), options)
}

/// A picture no larger than `limit` on either side, by averaging the pixels that fall into each new
/// one.
///
/// Averaging rather than taking every nth pixel: a screenshot of text reduced by dropping rows and
/// columns comes back as a mess of speckles, and the whole point of showing a picture in a preview is
/// that it can be recognised.
fn shrink_to_fit(image: egui::ColorImage, limit: usize) -> egui::ColorImage {
    let [width, height] = image.size;
    let longest = width.max(height);
    if longest <= limit || limit == 0 || width == 0 || height == 0 {
        return image;
    }
    let scale = limit as f32 / longest as f32;
    let across = ((width as f32 * scale).floor() as usize).clamp(1, limit);
    let down = ((height as f32 * scale).floor() as usize).clamp(1, limit);
    let mut pixels = Vec::with_capacity(across * down);
    for row in 0..down {
        let top = row * height / down;
        let bottom = (((row + 1) * height) / down).max(top + 1).min(height);
        for column in 0..across {
            let left = column * width / across;
            let right = (((column + 1) * width) / across).max(left + 1).min(width);
            let (mut r, mut g, mut b, mut a, mut count) = (0_u32, 0_u32, 0_u32, 0_u32, 0_u32);
            for y in top..bottom {
                for x in left..right {
                    let pixel = image.pixels[y * width + x];
                    r += pixel.r() as u32;
                    g += pixel.g() as u32;
                    b += pixel.b() as u32;
                    a += pixel.a() as u32;
                    count += 1;
                }
            }
            // The pixels are stored premultiplied, so averaging them is averaging the colour and the
            // transparency together, which is what a box filter on premultiplied pixels means.
            pixels.push(egui::Color32::from_rgba_premultiplied(
                (r / count) as u8,
                (g / count) as u8,
                (b / count) as u8,
                (a / count) as u8,
            ));
        }
    }
    egui::ColorImage {
        size: [across, down],
        pixels,
        source_size: egui::vec2(across as f32, down as f32),
    }
}

/// The picture on the clipboard, as the media type and bytes a plugin can send.
///
/// **PNG, whatever was copied**, because a clipboard holds raw pixels rather than a file: what a
/// screenshot tool put there has no format at all until something writes one, and PNG is the format
/// both APIs take and the one that does not lose anything.
///
/// **`null` rather than a refusal when there is no picture there at all**, and that changed with
/// `task-1771`. It used to answer "there is no picture on the clipboard", on the understanding that this
/// was only ever asked after a paste that carried nothing — but the only report of the paste chord that
/// reaches Quill is the key going back up (see `agent_chat::pasting`), which arrives after an ordinary
/// **text** paste too. Refusing there would put "there is no picture on the clipboard" under the composer
/// every time somebody pasted a sentence into it. Nothing to attach is not a fault; a picture that is
/// there and cannot be read still is, and still says so.
pub fn from_the_clipboard() -> Result<serde_json::Value, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|problem| format!("the clipboard could not be opened: {problem}"))?;
    let held = match clipboard.get_image() {
        Ok(held) => held,
        // **`null` for "there is none", a refusal for "there is one and it will not read".** `arboard`
        // tells the two apart and this used to throw the distinction away, so a picture the platform could
        // not convert was reported as no picture at all — a silent nothing where a person had every reason
        // to expect an attachment. Found by the `task-1771` review.
        Err(arboard::Error::ContentNotAvailable) => return Ok(serde_json::Value::Null),
        Err(problem) => {
            return Err(format!("the picture on the clipboard could not be read: {problem}"))
        }
    };
    let width = held.width as u32;
    let height = held.height as u32;
    let buffer = image::RgbaImage::from_raw(width, height, held.bytes.into_owned())
        .ok_or_else(|| "the picture on the clipboard could not be read.".to_owned())?;
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|problem| format!("the picture could not be encoded: {problem}"))?;
    Ok(serde_json::json!({
        "media": "image/png",
        "name": format!("pasted-{width}x{height}.png"),
        "data": quill_chat::base64::encode(&bytes),
    }))
}

/// Read a picture file into pixels egui can upload.
///
/// Public inside the crate because the Markdown preview decodes pictures too, and one decoder is
/// what stops a picture in a tab and the same picture in a preview coming out differently.
pub fn decode(path: &Path) -> Result<egui::ColorImage, String> {
    let bytes = std::fs::read(path).map_err(|problem| format!("{problem}"))?;
    decode_bytes(&bytes)
}

/// How big a picture is, without decoding a pixel of it.
///
/// **Because the row has to be measured before it is drawn.** A message's height is worked out in
/// `message::pieces`, which has no `egui::Ui` and so cannot upload a texture; reserving the tallest a
/// picture may be drawn instead left a landscape photograph sitting in a column of empty pane. Every
/// format `image` reads carries its size in a header, so this is a few dozen bytes rather than a
/// decode, and it is cached beside the texture for the same reason the texture is.
pub fn dimensions_of(bytes: &[u8]) -> Option<(f32, f32)> {
    let read = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (width, height) = read.into_dimensions().ok()?;
    Some((width as f32, height as f32))
}

/// The same, from bytes already in memory.
///
/// The Agent-Chat pane holds a picture as its own bytes rather than as a path — a conversation
/// reopened after the file has moved still shows what was really sent — so it has nothing to hand a
/// path-taking decoder. One decoder rather than two, for the reason [`decode`] already gives.
pub fn decode_bytes(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let decoded = image::load_from_memory(bytes).map_err(|problem| format!("{problem}"))?;
    let decoded = decoded.to_rgba8();
    let size = [decoded.width() as usize, decoded.height() as usize];
    let pixels = decoded
        .pixels()
        // Premultiplied, because that is what egui's painter expects and a picture with transparency
        // drawn unmultiplied has a pale halo round every edge.
        .map(|pixel| egui::Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3]))
        .collect();
    Ok(egui::ColorImage {
        size,
        pixels,
        source_size: egui::vec2(size[0] as f32, size[1] as f32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A four by two PNG, written out so the decoder has something real to read.
    fn sample(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join("quill-picture-tests");
        std::fs::create_dir_all(&folder).expect("make the folder");
        let path = folder.join(name);
        let mut pixels = Vec::new();
        for _ in 0..(4 * 2) {
            pixels.extend_from_slice(&[0x48, 0x9F, 0xF8, 0xFF]);
        }
        let buffer = image::RgbaImage::from_raw(4, 2, pixels).expect("build the picture");
        buffer.save(&path).expect("write the picture");
        path
    }

    #[test]
    fn a_picture_is_read_at_its_own_size() {
        let picture = Picture::open(&sample("four-by-two.png"));
        assert!(picture.problem.is_none(), "{:?}", picture.problem);
        assert_eq!(picture.size, [4, 2]);
    }

    #[test]
    fn a_file_that_is_not_a_picture_gives_a_reason_rather_than_nothing() {
        let folder = std::env::temp_dir().join("quill-picture-tests");
        std::fs::create_dir_all(&folder).expect("make the folder");
        let path = folder.join("not-a-picture.png");
        std::fs::write(&path, b"this is not a png").expect("write it");
        let picture = Picture::open(&path);
        assert!(picture.problem.is_some(), "a file that will not decode says why");
    }

    #[test]
    fn a_picture_starts_fitted_and_is_never_blown_up_to_fill_the_window() {
        let picture = Picture::open(&sample("small.png"));
        assert!(picture.is_fitted());
        // Four pixels across in a nine hundred point window stays four points across.
        assert_eq!(picture.scale_in(egui::vec2(900.0, 600.0)), 1.0);
    }

    #[test]
    fn a_picture_larger_than_the_graphics_card_will_hold_is_shrunk_before_it_is_uploaded() {
        let wide = egui::ColorImage {
            size: [4000, 1000],
            pixels: vec![egui::Color32::from_rgb(10, 20, 30); 4000 * 1000],
            source_size: egui::vec2(4000.0, 1000.0),
        };
        let smaller = shrink_to_fit(wide, 2048);
        assert_eq!(smaller.size, [2048, 512], "no side past the limit, and the shape is kept");
        assert_eq!(smaller.pixels.len(), 2048 * 512);
        assert_eq!(smaller.pixels[0], egui::Color32::from_rgb(10, 20, 30), "and the colour with it");
    }

    #[test]
    fn a_picture_the_card_will_hold_is_uploaded_as_it_is() {
        let small = egui::ColorImage {
            size: [8, 4],
            pixels: vec![egui::Color32::WHITE; 32],
            source_size: egui::vec2(8.0, 4.0),
        };
        assert_eq!(shrink_to_fit(small, 2048).size, [8, 4]);
    }

    #[test]
    fn a_picture_larger_than_the_area_is_scaled_down_to_fit_it() {
        let mut picture = Picture::open(&sample("wide.png"));
        picture.size = [4000, 2000];
        // The narrower of the two ratios wins, so nothing is cropped.
        assert_eq!(picture.fitted_scale(egui::vec2(1000.0, 1000.0)), 0.25);
    }

    #[test]
    fn zooming_starts_from_the_scale_it_was_being_fitted_at() {
        let mut picture = Picture::open(&sample("step.png"));
        picture.size = [4000, 2000];
        let area = egui::vec2(1000.0, 1000.0);
        assert_eq!(picture.scale_in(area), 0.25);
        picture.step_zoom(true, area);
        assert!(!picture.is_fitted());
        assert!((picture.scale_in(area) - 0.25 * ZOOM_STEP).abs() < 0.001);
        picture.step_zoom(false, area);
        assert!((picture.scale_in(area) - 0.25).abs() < 0.001);
        picture.fit();
        assert!(picture.is_fitted(), "resetting puts it back to filling the area");
    }

    #[test]
    fn a_pinch_is_gathered_rather_than_applied_a_frame_at_a_time() {
        let mut picture = Picture::open(&sample("pinch.png"));
        let area = egui::vec2(1000.0, 1000.0);
        // A hundredth of a per cent at a time is a real gesture, and none of them alone should move it.
        for _ in 0..5 {
            picture.zoom_by_gesture(1.0005, area);
        }
        assert!(picture.is_fitted(), "nothing has asked for a whole step yet");
        picture.zoom_by_gesture(1.2, area);
        assert!(!picture.is_fitted(), "a gesture past the threshold moves it");
    }

    #[test]
    fn the_scale_cannot_be_driven_past_its_limits() {
        let mut picture = Picture::open(&sample("limits.png"));
        let area = egui::vec2(1000.0, 1000.0);
        for _ in 0..200 {
            picture.step_zoom(true, area);
        }
        assert_eq!(picture.scale_in(area), MAX_ZOOM);
        for _ in 0..400 {
            picture.step_zoom(false, area);
        }
        assert_eq!(picture.scale_in(area), MIN_ZOOM);
    }

    #[test]
    fn the_status_bar_is_told_how_big_it_is_and_how_far_it_is_zoomed() {
        let mut picture = Picture::open(&sample("described.png"));
        let area = egui::vec2(1000.0, 1000.0);
        assert_eq!(picture.description(area), "4 \u{00D7} 2 \u{00B7} 100%");
        picture.step_zoom(true, area);
        assert_eq!(picture.description(area), "4 \u{00D7} 2 \u{00B7} 125%");
    }
}
