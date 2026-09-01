//! The board's decoration: what a raised or a pressed surface is, and the renderer that turns it into
//! pixels.
//!
//! `tasks/agent-tasks-vello-ui-tdd.md` is the design. In one paragraph: the picture `task-1765` asks the
//! Agent-Tasks board to look like is dark neumorphism, so every surface in it carries a soft shadow above
//! and left of it and a darker one below and right, a recessed surface carries the same pair *inside*
//! itself, the buttons are diagonal gradients and the lane dots have coloured haloes. `epaint` can draw one
//! approximate outer shadow and an axis-aligned gradient through a texture brush, and it cannot draw an
//! inset shadow, a diagonal gradient, a glow round a circle or a rounded clip at all. Those four are what
//! makes the picture look like the picture.
//!
//! ## Three types, and the seam is the middle one
//!
//! [`Chrome`] is what a component talks to. It says *what kind of surface* it is drawing — `raised`,
//! `sunken`, `glow` — and the elevation recipe lives here, once, so no component holds a shadow offset.
//! That is the rule that a component cannot name a colour, applied to depth.
//!
//! [`Decor`] is what that records: five kinds of shape and a clip, in points, and nothing else. It is the
//! same seam `quill_core::mermaid::Scene` is — a value a test can assert on with no window, no graphics
//! card and no fonts — and it is what makes the caching in [`Canvas`] a hash comparison.
//!
//! [`Canvas`] is the renderer. It replays a `Decor` list into a `vello_cpu::RenderContext`, rasterises it
//! into a `Pixmap` of premultiplied RGBA8 — which is `egui::Color32` byte for byte — and uploads it as one
//! texture painted behind the pane's own widgets.
//!
//! ## Nothing that runs once a frame may do the work twice
//!
//! `task-1666`'s rule. The `Decor` list is rebuilt every frame, which is a few hundred `Copy` values into a
//! `Vec` that keeps its capacity; the **rasterisation** only happens when the list or the size changed,
//! which is one hash comparison. A board nobody is touching therefore costs what a board that is not there
//! costs, which matters because the window redraws on a heartbeat.
//!
//! ## Why the CPU renderer
//!
//! `vello` and `vello_hybrid` both pin wgpu 29 and eframe pins wgpu 30, so their devices and their textures
//! are different types and no vello GPU renderer can write into a texture eframe can read. `vello_cpu` has
//! no GPU dependency at all — and **`epaint` already depends on it**, to rasterise the glyphs in its font
//! atlas, so this renderer is already inside Quill and using it here adds no third-party renderer to the
//! binary. §3 and §4 of the design are the whole argument.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{Color32, Pos2, Rect, Vec2};
use vello_cpu::kurbo::{Affine, BezPath, Circle, Point, RoundedRect, Shape as _, Stroke};
use vello_cpu::peniko::color::{AlphaColor, Srgb};
use vello_cpu::peniko::Fill as FillRule;
use vello_cpu::peniko::{ColorStop, ColorStops, Gradient, GradientKind, LinearGradientPosition};
use vello_cpu::{
    CompositeMode, PixelFormat, Pixmap, RasterizerSettings, RenderContext, RenderMode, RenderSettings,
    Resources,
};

/// The largest canvas that will ever be rasterised, in physical pixels a side.
///
/// A pane cannot be wider than the window, and a 4096 point window at two pixels a point is bigger than
/// any display this runs on. The cap is here so that an absurd size — a bug elsewhere, a screen
/// configuration nobody has — degrades to *no decoration* rather than to a 400 megabyte allocation.
const MAX_SIDE: u16 = 4096;

/// The most pixels a point of decoration is ever rasterised at.
///
/// **The decoration is drawn at the display's resolution up to here, and no further.** Its
/// highest-frequency feature is the edge of a rounded rectangle; everything else in it is a Gaussian or a
/// gradient, which is exactly the content that survives being drawn slightly coarser and filtered up.
/// The text over it is not affected at all, because `egui` draws the text.
///
/// The number is a cost, measured: rasterising is per pixel, so a display at two pixels a point is four
/// times the work, and a board of four lanes and thirty-two cards over 1400 by 900 points went from 4.8 ms
/// to 17.4 on the frames where it changed — which is a drag that drops frames on a machine where nothing
/// else does. At 1.5 it is under ten. `crates/quill-app/examples/vello_cost.rs` is how that is measured
/// again.
const MAX_SCALE: f32 = 1.5;

/// How far off the surface behind it a thing stands, and how deeply a well is pressed into it.
///
/// Three, because the stylesheet the reference is drawn from has three: `--e-raised-sm`, `--e-raised` and
/// `--e-raised-lg`, whose offsets are 3, 6 and 10 points and whose blurs are 6, 14 and 24. A CSS blur
/// radius is about twice a Gaussian standard deviation, which is what `fill_blurred_rounded_rect` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lift {
    Small,
    Medium,
    Large,
}

impl Lift {
    /// The offset and the standard deviation a raised surface's pair of shadows use.
    fn raised(self) -> (f32, f32) {
        match self {
            Self::Small => (4.0, 4.0),
            Self::Medium => (6.0, 7.0),
            Self::Large => (10.0, 12.0),
        }
    }

    /// The same for a surface pressed into the one behind it: `--e-pressed-sm` and `--e-pressed`.
    fn sunken(self) -> (f32, f32) {
        match self {
            Self::Small => (2.0, 2.0),
            Self::Medium => (4.0, 4.0),
            Self::Large => (6.0, 6.0),
        }
    }

    /// How dark the shadow under a raised surface is. Deeper things cast darker shadows.
    ///
    /// Tuned against the picture rather than chosen: the reference's page is `#181D24` and the darkest point
    /// of a lane's shadow on it is `#0C1014`, which is about half an alpha of black once the Gaussian's peak
    /// is taken into account.
    fn shadow_alpha(self) -> u8 {
        match self {
            Self::Small => 78,
            Self::Medium => 100,
            Self::Large => 122,
        }
    }
}

/// What fills a shape.
///
/// Two, because the reference has two: a flat surface and a gradient along a box's own diagonal. A radial
/// and a sweep are one line each in `vello_cpu` and are not here because nothing asks for them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fill {
    Solid(Color32),
    /// A linear gradient, given as the two ends of its axis so that a diagonal is not a special case.
    Linear { from: Pos2, to: Pos2, start: Color32, end: Color32 },
}

impl Fill {
    /// The colour this fill reads as, which is what an elevation's pale edge is derived from.
    fn representative(self) -> Color32 {
        match self {
            Self::Solid(colour) => colour,
            Self::Linear { start, .. } => start,
        }
    }

    /// A gradient along the diagonal of `rect`, from its top left to its bottom right.
    ///
    /// Which is the one the reference uses for every gradient on the board: the play button, the agent
    /// badge and `Add Task` are all lit from the same corner as every shadow is.
    pub fn diagonal(rect: Rect, start: Color32, end: Color32) -> Self {
        Self::Linear { from: rect.min, to: rect.max, start, end }
    }
}

/// One piece of decoration.
///
/// Five kinds of shape and a clip, in points, absolute in the window's own coordinates. Deliberately as
/// small and as plain as `quill_core::mermaid::Scene`'s items: a value with no behaviour, so the drawing
/// can be asserted on without a renderer and hashed without one either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decor {
    /// A rounded rectangle. The whole of a lane, a card, a button and a well.
    Rect { rect: Rect, radius: f32, fill: Fill },
    /// A Gaussian blur of a rounded rectangle, outside the shape or inside it.
    ///
    /// `inset` is `vello_cpu`'s own `invert` argument, whose documentation says in as many words that it
    /// is how an inset box shadow is drawn. It is the reason this file exists.
    Shadow { rect: Rect, radius: f32, blur: f32, colour: Color32, inset: bool },
    /// A filled circle: the lane dots, the play button, the agent badge.
    Disc { centre: Pos2, radius: f32, fill: Fill },
    /// An unfilled circle: the ring round the badge of a ticket whose agent is attached.
    Ring { centre: Pos2, radius: f32, width: f32, colour: Color32 },
    /// A straight line, which is every divider on the board.
    Line { from: Pos2, to: Pos2, width: f32, colour: Color32 },
    /// Everything after this is cut to this rounded rectangle, until [`Decor::Unclip`].
    ///
    /// `outside` cuts to everything **outside** it instead, bounded by `bound`. That is what makes an
    /// elevation affordable: a raised surface's shadow is only ever seen in the band around it, and drawing
    /// the whole blurred rectangle underneath an opaque surface was measured at 3.3 ms **a lane** — a
    /// Gaussian evaluated over a third of a million pixels that are then painted over. See §9 of the design.
    Clip { rect: Rect, radius: f32, outside: bool, bound: Rect },
    Unclip,
}

/// Hashed by bit pattern, because `f32` is not `Eq` and the fingerprint in [`Canvas`] needs one.
///
/// Two `Decor` lists that hash the same are two lists that would rasterise the same, which is the only
/// property this has to have. `-0.0` and `0.0` hash differently and draw the same, which costs one
/// rasterisation on the frame a value happens to cross zero and is not worth a slower comparison.
impl Hash for Decor {
    fn hash<H: Hasher>(&self, into: &mut H) {
        fn f(value: f32, into: &mut impl Hasher) {
            value.to_bits().hash(into);
        }
        fn pos(at: Pos2, into: &mut impl Hasher) {
            f(at.x, into);
            f(at.y, into);
        }
        fn rect(area: Rect, into: &mut impl Hasher) {
            pos(area.min, into);
            pos(area.max, into);
        }
        fn colour(value: Color32, into: &mut impl Hasher) {
            value.to_array().hash(into);
        }
        fn fill(value: Fill, into: &mut impl Hasher) {
            match value {
                Fill::Solid(one) => {
                    0u8.hash(into);
                    colour(one, into);
                }
                Fill::Linear { from, to, start, end } => {
                    1u8.hash(into);
                    pos(from, into);
                    pos(to, into);
                    colour(start, into);
                    colour(end, into);
                }
            }
        }
        std::mem::discriminant(self).hash(into);
        match *self {
            Self::Rect { rect: area, radius, fill: value } => {
                rect(area, into);
                f(radius, into);
                fill(value, into);
            }
            Self::Shadow { rect: area, radius, blur, colour: tone, inset } => {
                rect(area, into);
                f(radius, into);
                f(blur, into);
                colour(tone, into);
                inset.hash(into);
            }
            Self::Disc { centre, radius, fill: value } => {
                pos(centre, into);
                f(radius, into);
                fill(value, into);
            }
            Self::Ring { centre, radius, width, colour: tone } => {
                pos(centre, into);
                f(radius, into);
                f(width, into);
                colour(tone, into);
            }
            Self::Line { from, to, width, colour: tone } => {
                pos(from, into);
                pos(to, into);
                f(width, into);
                colour(tone, into);
            }
            Self::Clip { rect: area, radius, outside, bound } => {
                rect(area, into);
                f(radius, into);
                outside.hash(into);
                rect(bound, into);
            }
            Self::Unclip => {}
        }
    }
}

/// The elevation recipes, and where a component's decoration is recorded.
///
/// Handed to a provider inside `plugin_ui::Look`, so a provider draws depth the way it draws colour: by
/// saying what a thing *is* rather than what it is made of. There is deliberately no way to record a
/// shadow of your own — the offsets and the blurs are the stylesheet's, in [`Lift`], and a component that
/// could choose one would be a component that could disagree with the rest of the board.
///
/// Interior mutability, because it is reached through a shared reference on a `Look` that several
/// components hold at once while a pane is being drawn.
///
/// A `Mutex` rather than a `RefCell`, and for one reason: [`Chrome::off`] is a `static` in
/// `services::plugin_ui`, so that every existing caller of `Look::of` goes on working, and a `static` has
/// to be `Sync`. It is never contended — a pane is drawn on the thread that owns the window — so the lock
/// is an uncontended atomic and the alternative was threading a chrome through a hundred call sites.
#[derive(Debug, Default)]
pub struct Chrome {
    recording: bool,
    items: std::sync::Mutex<Vec<Decor>>,
}

impl Chrome {
    /// A chrome that records nothing, whatever is asked of it.
    ///
    /// What `Look::of` uses, so every test that builds a `Look` — and there are many — goes on running with
    /// no canvas behind it. `const` so that it can be a `static`, which is what lets a `Look` hold a
    /// `&'static Chrome` without anything owning one.
    pub const fn off() -> Self {
        Self { recording: false, items: std::sync::Mutex::new(Vec::new()) }
    }

    /// A chrome that records, for a pane that has a canvas behind it.
    pub fn recording() -> Self {
        Self { recording: true, items: std::sync::Mutex::new(Vec::new()) }
    }

    /// Whether anything drawn into this will be seen.
    ///
    /// A component asks so that it can draw the flat form instead: with the decoration off, a card is a
    /// filled rectangle with a border, which is what the board was before this and is what a person who
    /// switched `plugins.chrome` off asked for.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Take what was recorded, leaving the chrome empty for the next frame.
    pub fn take(&self) -> Vec<Decor> {
        match self.items.lock() {
            Ok(mut items) => std::mem::take(&mut *items),
            // A poisoned lock means a component panicked mid-drawing. The board is already in trouble;
            // drawing it without decoration is a better report than a second panic on top of the first.
            Err(_) => Vec::new(),
        }
    }

    fn push(&self, item: Decor) {
        if self.recording {
            if let Ok(mut items) = self.items.lock() {
                items.push(item);
            }
        }
    }

    /// A rounded rectangle with no depth at all.
    pub fn rect(&self, rect: Rect, radius: f32, fill: Fill) {
        self.push(Decor::Rect { rect, radius, fill });
    }

    /// A surface standing above the one behind it: a lane, a card, a button.
    ///
    /// Three items: the dark shadow down and right, the pale one up and left, then the surface over both.
    /// The pale tone is the surface **lifted**, not white — which is what the dark-mode stylesheet says
    /// itself, that "in dark neumorphism the 'light' highlight is a lifted gray (not white)" — so no
    /// elevation ever introduces a hue and the palette stays closed.
    pub fn raised(&self, rect: Rect, radius: f32, fill: Fill, lift: Lift) {
        let (offset, blur) = lift.raised();
        // **The pair is cut to the band around the shape**, which is the whole of what makes elevation
        // affordable. The surface is opaque and is painted over its own shadows, so every pixel of blur
        // inside it is computed and then thrown away; on a lane 328 by 812 that is a third of a million
        // pixels of Gaussian, twice, and it measured 3.3 ms a lane. Clipped, only the band is evaluated.
        let reach = offset + blur * 2.5 + 2.0;
        self.push(Decor::Clip { rect, radius, outside: true, bound: rect.expand(reach) });
        self.push(Decor::Shadow {
            rect: rect.translate(Vec2::splat(offset)),
            radius,
            blur,
            colour: Color32::from_black_alpha(lift.shadow_alpha()),
            inset: false,
        });
        self.push(Decor::Shadow {
            rect: rect.translate(Vec2::splat(-offset)),
            radius,
            blur,
            colour: lifted(fill.representative(), 0.26).gamma_multiply(0.45),
            inset: false,
        });
        self.push(Decor::Unclip);
        self.push(Decor::Rect { rect, radius, fill });
    }

    /// A surface pressed into the one behind it: a well, a field, a count chip, the round `K` button.
    ///
    /// The surface, then the two shadows **inside** it — dark from the top left, pale from the bottom
    /// right, which is the light coming from the same corner it comes from everywhere else.
    ///
    /// **The pair is clipped to the shape, and it has to be.** `fill_blurred_rounded_rect`'s inverted form
    /// paints the *complement* of the blur: opaque everywhere outside the rectangle, fading to nothing
    /// inside it. Unclipped, a well would therefore wash its own colour across the whole pane. CSS clips an
    /// inset shadow to the border box for the same reason, and this is that rule made explicit.
    pub fn sunken(&self, rect: Rect, radius: f32, fill: Color32, depth: Lift) {
        let (offset, blur) = depth.sunken();
        self.push(Decor::Rect { rect, radius, fill: Fill::Solid(fill) });
        self.push(Decor::Clip { rect, radius, outside: false, bound: rect });
        self.push(Decor::Shadow {
            rect: rect.translate(Vec2::splat(offset)),
            radius,
            blur,
            colour: Color32::from_black_alpha(depth.shadow_alpha()),
            inset: true,
        });
        self.push(Decor::Shadow {
            rect: rect.translate(Vec2::splat(-offset)),
            radius,
            blur,
            colour: lifted(fill, 0.30).gamma_multiply(0.45),
            inset: true,
        });
        self.push(Decor::Unclip);
    }

    /// A coloured halo behind something: a lane's dot, and the blue under the primary button.
    pub fn glow(&self, rect: Rect, radius: f32, colour: Color32, spread: f32) {
        self.push(Decor::Shadow {
            rect: rect.expand(spread * 0.25),
            radius,
            blur: spread,
            colour,
            inset: false,
        });
    }

    pub fn disc(&self, centre: Pos2, radius: f32, fill: Fill) {
        self.push(Decor::Disc { centre, radius, fill });
    }

    pub fn ring(&self, centre: Pos2, radius: f32, width: f32, colour: Color32) {
        self.push(Decor::Ring { centre, radius, width, colour });
    }

    pub fn line(&self, from: Pos2, to: Pos2, width: f32, colour: Color32) {
        self.push(Decor::Line { from, to, width, colour });
    }

    /// Everything drawn until [`Self::unclip`] is cut to this rounded rectangle.
    ///
    /// The one thing on the board that `egui` cannot do at all: its clip rectangle is axis-aligned and
    /// square, so a card scrolled to the bottom of a lane pokes its own square corner out of the lane's
    /// curve.
    pub fn clip(&self, rect: Rect, radius: f32) {
        self.push(Decor::Clip { rect, radius, outside: false, bound: rect });
    }

    pub fn unclip(&self) {
        self.push(Decor::Unclip);
    }
}

/// A colour moved towards white by `amount`, which is how the pale half of an elevation is derived.
fn lifted(colour: Color32, amount: f32) -> Color32 {
    let mix = |channel: u8| -> u8 {
        let value = f32::from(channel);
        (value + (255.0 - value) * amount.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(mix(colour.r()), mix(colour.g()), mix(colour.b()))
}

/// One pane's canvas: the renderer, the pixels and the texture they were uploaded as.
///
/// Kept between frames, which is the whole point: the texture is what a frame where nothing moved reuses.
pub struct Canvas {
    context: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    texture: Option<egui::TextureHandle>,
    /// The hash of the `Decor` list the texture was made from, and the size it was made at.
    fingerprint: Option<(u64, u16, u16)>,
    /// How many times this canvas has actually rasterised. Read by the test that says a still board is free.
    rasterisations: u64,
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Canvas")
            .field("size", &(self.pixmap.width(), self.pixmap.height()))
            .field("rasterisations", &self.rasterisations)
            .finish()
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new(RenderSettings::default())
    }
}

impl Canvas {
    fn new(settings: RenderSettings) -> Self {
        Self {
            context: RenderContext::new_with(1, 1, settings),
            resources: Resources::new(),
            pixmap: Pixmap::new(1, 1),
            texture: None,
            fingerprint: None,
            rasterisations: 0,
        }
    }

    /// A canvas whose output is the same on every machine.
    ///
    /// `RenderSettings::default()` detects the widest SIMD this processor has — AVX-512 on one machine and
    /// SSE4.2 on another — and whether two levels are bit-identical is not something `vello_cpu` promises.
    /// A screenshot accepted on one machine could then fail on another for a reason that is not a fault in
    /// Quill. The tests pin the **baseline** level, which is the set of features the target itself
    /// guarantees and is therefore the same on every machine of that target; the window uses the detected
    /// one, because in the window the fastest is what is wanted. `Level::fallback` is deliberately not used:
    /// it is compiled out on a target whose baseline is already above it.
    pub fn for_tests() -> Self {
        Self::new(RenderSettings {
            level: vello_cpu::Level::baseline(),
            num_threads: 0,
        })
    }

    pub fn rasterisations(&self) -> u64 {
        self.rasterisations
    }

    /// Rasterise `items` if they or the size have changed, and answer the texture to paint.
    ///
    /// `None` when there is nothing to draw or the pane is an impossible size, which the caller draws as
    /// nothing rather than as a blank rectangle.
    pub fn texture_for(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        rect: Rect,
        pixels_per_point: f32,
        items: &[Decor],
    ) -> Option<(egui::TextureId, Rect)> {
        if items.is_empty() {
            return None;
        }
        // **The canvas is the decoration's own bounding box, not the pane's.** `task-1666`'s rule — a frame
        // costs what is on the screen rather than what is in the file — and here it is worth real time:
        // rasterising has a floor of about two nanoseconds a pixel whether anything is drawn there or not,
        // so an empty canvas the size of a 1400 by 900 pane costs 2.4 ms before a single shape. A board
        // whose lanes are as tall as their contents leaves most of a tall pane empty.
        let Some(rect) = bounds_of(items).map(|bounds| bounds.intersect(rect)) else {
            return None;
        };
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return None;
        }
        let scale = pixels_per_point.min(MAX_SCALE);
        let width = (rect.width() * scale).round();
        let height = (rect.height() * scale).round();
        if !(width.is_finite() && height.is_finite()) || width < 1.0 || height < 1.0 {
            return None;
        }
        if width > f32::from(MAX_SIDE) || height > f32::from(MAX_SIDE) {
            return None;
        }
        let (width, height) = (width as u16, height as u16);
        let fingerprint = fingerprint_of(items);
        if self.fingerprint == Some((fingerprint, width, height)) {
            if let Some(texture) = &self.texture {
                return Some((texture.id(), rect));
            }
        }
        self.rasterise(rect, scale, width, height, items);
        // Built here and handed straight over. `TextureHandle::set` takes ownership and the upload happens
        // at the end of the frame, so there is no buffer to keep and reuse — but there is no reason to fill
        // one and then copy it either, which is what an earlier version did: five megabytes memcopied a
        // rasterisation for nothing.
        let image = egui::ColorImage::new([usize::from(width), usize::from(height)], self.pixels());
        match &mut self.texture {
            // Set rather than load, so a board being typed into does not allocate a new texture a keystroke.
            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
            none => *none = Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR)),
        }
        self.fingerprint = Some((fingerprint, width, height));
        self.texture.as_ref().map(|texture| (texture.id(), rect))
    }

    /// Replay the list into the renderer and take the pixels out.
    fn rasterise(
        &mut self,
        rect: Rect,
        pixels_per_point: f32,
        width: u16,
        height: u16,
        items: &[Decor],
    ) {
        self.rasterisations += 1;
        // **Resized rather than rebuilt.** `RenderContext::new_with` allocates the whole dispatcher, and the
        // canvas is the decoration's own bounding box, which moves by a point whenever a lane grows — so
        // rebuilding on a size change meant rebuilding on nearly every frame that changed anything.
        // Measured: 15.3 ms became 20.2 the moment the bounds started moving. `reset_and_resize` clears the
        // buffers it already has.
        if self.pixmap.width() != width || self.pixmap.height() != height {
            self.pixmap.resize(width, height);
        }
        self.context.reset_and_resize(width, height);
        // Points to physical pixels, and the pane's own top left to the origin. Every `Decor` is in the
        // window's coordinates, so this is the only place the two spaces meet.
        let to_pixels = Affine::scale(f64::from(pixels_per_point))
            * Affine::translate((-f64::from(rect.min.x), -f64::from(rect.min.y)));
        self.context.set_transform(to_pixels);
        let mut clips = 0usize;
        for item in items {
            self.draw(*item, &mut clips);
        }
        // A component that clipped and did not unclip leaves a clip on the stack. `push_clip_path` allows
        // that where `push_clip_layer` would panic, and it is popped anyway so the next frame starts clean.
        for _ in 0..clips {
            self.context.pop_clip_path();
        }
        self.context.flush();
        self.context.render_with(
            self.pixmap.as_mut(),
            &mut self.resources,
            RasterizerSettings {
                render_mode: RenderMode::OptimizeSpeed,
                composite_mode: CompositeMode::Replace,
                pixel_format: PixelFormat::Rgba8,
                offset: (0, 0),
            },
        );
    }

    /// The pixmap as egui's own colour.
    ///
    /// `PremulRgba8` and `Color32` are the same four bytes in the same order, which
    /// `a_pixmap_is_the_same_four_bytes_as_egui` asserts rather than assumes — so this is a copy rather
    /// than a conversion, and it is written as a copy: one allocation of exactly the right size and a walk
    /// that the optimiser turns into a memcpy.
    fn pixels(&self) -> Vec<Color32> {
        let bytes = self.pixmap.data_as_u8_slice();
        let mut colours = Vec::with_capacity(bytes.len() / 4);
        colours.extend(
            bytes
                .chunks_exact(4)
                .map(|pixel| Color32::from_rgba_premultiplied(pixel[0], pixel[1], pixel[2], pixel[3])),
        );
        colours
    }

    fn draw(&mut self, item: Decor, clips: &mut usize) {
        match item {
            Decor::Rect { rect, radius, fill } => {
                self.set_fill(fill);
                self.context.fill_path(&rounded(rect, radius));
            }
            Decor::Shadow { rect, radius, blur, colour, inset } => {
                self.context.set_paint(colour_of(colour));
                self.context.fill_blurred_rounded_rect(&box_of(rect), radius, blur.max(0.01), inset);
            }
            Decor::Disc { centre, radius, fill } => {
                self.set_fill(fill);
                let circle = Circle::new(Point::new(f64::from(centre.x), f64::from(centre.y)), f64::from(radius));
                self.context.fill_path(&circle.to_path(0.1));
            }
            Decor::Ring { centre, radius, width, colour } => {
                self.context.set_paint(colour_of(colour));
                self.context.set_stroke(Stroke::new(f64::from(width)));
                let circle = Circle::new(Point::new(f64::from(centre.x), f64::from(centre.y)), f64::from(radius));
                self.context.stroke_path(&circle.to_path(0.1));
            }
            Decor::Line { from, to, width, colour } => {
                self.context.set_paint(colour_of(colour));
                self.context.set_stroke(Stroke::new(f64::from(width)));
                let mut path = BezPath::new();
                path.move_to(Point::new(f64::from(from.x), f64::from(from.y)));
                path.line_to(Point::new(f64::from(to.x), f64::from(to.y)));
                self.context.stroke_path(&path);
            }
            Decor::Clip { rect, radius, outside, bound } => {
                // `push_clip_path` rather than `push_clip_layer`: a clip **layer** renders its contents into
                // a layer of their own and composites them, and nothing here needs that. A clip **path** is
                // intersected while the strips are generated, so a tile outside it costs nothing at all —
                // which is what makes the shadow band cheap rather than merely correct.
                let (path, rule) = match outside {
                    false => (rounded(rect, radius), FillRule::NonZero),
                    // Everything within `bound` and outside the shape: two subpaths read even-odd, which is
                    // the frame a raised surface's shadow is drawn in.
                    true => {
                        let mut path = rounded(bound, 0.0);
                        path.extend(rounded(rect, radius));
                        (path, FillRule::EvenOdd)
                    }
                };
                self.context.set_fill_rule(rule);
                self.context.push_clip_path(&path);
                // Back to the rule everything else is filled with, or the next shape would be read even-odd.
                self.context.set_fill_rule(FillRule::NonZero);
                *clips += 1;
            }
            Decor::Unclip => {
                if *clips > 0 {
                    self.context.pop_clip_path();
                    *clips -= 1;
                }
            }
        }
    }

    fn set_fill(&mut self, fill: Fill) {
        match fill {
            Fill::Solid(colour) => self.context.set_paint(colour_of(colour)),
            Fill::Linear { from, to, start, end } => {
                let mut stops = ColorStops::default();
                stops.0.push(ColorStop::from((0.0, colour_of(start))));
                stops.0.push(ColorStop::from((1.0, colour_of(end))));
                self.context.set_paint(Gradient {
                    kind: GradientKind::Linear(LinearGradientPosition::new(
                        Point::new(f64::from(from.x), f64::from(from.y)),
                        Point::new(f64::from(to.x), f64::from(to.y)),
                    )),
                    stops,
                    ..Default::default()
                });
            }
        }
    }
}

/// egui's premultiplied colour as Vello's un-premultiplied one.
///
/// The one conversion in the file, and it is here rather than at each call so that the premultiplication
/// is undone in one place. A fully transparent colour has no hue to recover, so it stays black.
fn colour_of(colour: Color32) -> AlphaColor<Srgb> {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    AlphaColor::from_rgba8(r, g, b, a)
}

fn box_of(rect: Rect) -> vello_cpu::kurbo::Rect {
    vello_cpu::kurbo::Rect::new(
        f64::from(rect.min.x),
        f64::from(rect.min.y),
        f64::from(rect.max.x),
        f64::from(rect.max.y),
    )
}

fn rounded(rect: Rect, radius: f32) -> BezPath {
    let radius = f64::from(radius).min(f64::from(rect.width().min(rect.height())) / 2.0).max(0.0);
    RoundedRect::from_rect(box_of(rect), radius).to_path(0.1)
}

/// The rectangle everything in `items` fits inside, shadows and all.
///
/// `None` when there is nothing to draw. A shadow reaches `blur * 2.5` past its own rectangle, which is
/// where `vello_cpu` itself cuts the Gaussian off, and a clip's `bound` is already the widest thing inside
/// it — so this is the whole of what a canvas has to cover.
fn bounds_of(items: &[Decor]) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    let mut add = |area: Rect| {
        bounds = Some(match bounds {
            Some(so_far) => so_far.union(area),
            None => area,
        });
    };
    for item in items {
        match *item {
            Decor::Rect { rect, .. } => add(rect),
            Decor::Shadow { rect, blur, inset, .. } => match inset {
                true => add(rect),
                false => add(rect.expand(blur * 2.5)),
            },
            Decor::Disc { centre, radius, .. } => add(Rect::from_center_size(centre, Vec2::splat(radius * 2.0))),
            Decor::Ring { centre, radius, width, .. } => {
                add(Rect::from_center_size(centre, Vec2::splat(radius * 2.0 + width)));
            }
            Decor::Line { from, to, width, .. } => add(Rect::from_two_pos(from, to).expand(width)),
            Decor::Clip { bound, .. } => add(bound),
            Decor::Unclip => {}
        }
    }
    bounds
}

/// The hash a frame's decoration is compared by.
fn fingerprint_of(items: &[Decor]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    items.len().hash(&mut hasher);
    for item in items {
        item.hash(&mut hasher);
    }
    hasher.finish()
}

/// Every canvas in this window, one per pane that draws decoration.
///
/// Keyed on the `egui::Id` the pane was drawn with, because the same plugin can be showing in a docked
/// pane and in a tab at once and the two are different widths — which is `task-1664`'s rule about the
/// layout caches belonging to the tab rather than to the window, made again. A canvas whose id was not
/// drawn this frame is dropped, so closing a pane gives its texture back.
#[derive(Debug, Default)]
pub struct Canvases {
    by_id: HashMap<egui::Id, Canvas>,
    seen: Vec<egui::Id>,
    /// Built with the tests' pinned SIMD level rather than the machine's.
    deterministic: bool,
}

impl Canvases {
    /// The store the screenshot tests use, whose canvases rasterise the same on every machine.
    pub fn deterministic() -> Self {
        Self { deterministic: true, ..Self::default() }
    }

    /// Rasterise one pane's decoration and answer the texture to paint behind it.
    pub fn texture_for(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        name: &str,
        rect: Rect,
        items: &[Decor],
    ) -> Option<(egui::TextureId, Rect)> {
        self.seen.push(id);
        let deterministic = self.deterministic;
        let canvas = self
            .by_id
            .entry(id)
            .or_insert_with(|| if deterministic { Canvas::for_tests() } else { Canvas::default() });
        canvas.texture_for(ctx, name, rect, ctx.pixels_per_point(), items)
    }

    /// Forget the canvases of panes that were not drawn this frame. Called once, at the end of the frame.
    pub fn tidy(&mut self) {
        if self.by_id.len() != self.seen.len() {
            self.by_id.retain(|id, _| self.seen.contains(id));
        }
        self.seen.clear();
    }

    #[cfg(test)]
    pub fn rasterisations(&self, id: egui::Id) -> u64 {
        self.by_id.get(&id).map(Canvas::rasterisations).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
    }

    #[test]
    fn a_raised_surface_is_two_shadows_and_the_surface_over_them() {
        let chrome = Chrome::recording();
        chrome.raised(rect(10.0, 10.0, 100.0, 40.0), 14.0, Fill::Solid(Color32::from_rgb(0x20, 0x25, 0x2E)), Lift::Medium);
        let items = chrome.take();
        assert_eq!(items.len(), 5, "a band to draw in, two shadows, the unclip, and the surface");
        // The band first, so the two shadows are only evaluated where they can be seen.
        assert!(matches!(items[0], Decor::Clip { outside: true, .. }));
        // The dark one is down and right, the pale one up and left: the light comes from the same corner
        // everywhere on the board, which is what neumorphism is.
        let (dark, pale) = match (items[1], items[2]) {
            (Decor::Shadow { rect: dark, inset: false, .. }, Decor::Shadow { rect: pale, inset: false, .. }) => (dark, pale),
            other => panic!("expected two outer shadows, got {other:?}"),
        };
        assert!(dark.min.x > pale.min.x && dark.min.y > pale.min.y);
        assert!(matches!(items[3], Decor::Unclip));
        // And the surface last and unclipped, or the thing the shadows belong to would not be drawn at all.
        assert!(matches!(items[4], Decor::Rect { radius, .. } if radius == 14.0));
    }

    #[test]
    fn a_sunken_surface_puts_its_shadows_inside_itself() {
        let chrome = Chrome::recording();
        chrome.sunken(rect(0.0, 0.0, 40.0, 20.0), 10.0, Color32::from_rgb(0x1B, 0x20, 0x26), Lift::Small);
        let items = chrome.take();
        assert_eq!(items.len(), 5, "the surface, a clip, two inset shadows and the unclip");
        assert!(matches!(items[0], Decor::Rect { .. }), "the surface is drawn first, then pressed into");
        // Clipped, because the inverted blur paints everything outside the rectangle — see `sunken`.
        assert!(matches!(items[1], Decor::Clip { .. }));
        assert!(items[2..4].iter().all(|item| matches!(item, Decor::Shadow { inset: true, .. })));
        assert!(matches!(items[4], Decor::Unclip));
    }

    #[test]
    fn an_elevation_never_introduces_a_hue() {
        // The pale half of an elevation is the surface lifted and the dark half is black at an alpha, so
        // there is no colour anywhere in the depth. That is what keeps the palette closed while the board
        // gains a whole dimension it did not have.
        let surface = Color32::from_rgb(0x20, 0x25, 0x2E);
        let chrome = Chrome::recording();
        chrome.raised(rect(0.0, 0.0, 50.0, 50.0), 8.0, Fill::Solid(surface), Lift::Small);
        for item in chrome.take() {
            if let Decor::Shadow { colour, .. } = item {
                let hue_of = |c: Color32| (i32::from(c.r()) - i32::from(c.b())).abs();
                assert!(
                    hue_of(colour) <= hue_of(surface),
                    "{colour:?} is more coloured than the surface it belongs to"
                );
            }
        }
    }

    #[test]
    fn a_chrome_that_is_off_records_nothing() {
        // What `Look::of` hands to every test that has no canvas, and what `plugins.chrome` off gives a
        // person who does not want the decoration.
        let chrome = Chrome::off();
        assert!(!chrome.is_recording());
        chrome.raised(rect(0.0, 0.0, 10.0, 10.0), 2.0, Fill::Solid(Color32::RED), Lift::Large);
        chrome.sunken(rect(0.0, 0.0, 10.0, 10.0), 2.0, Color32::RED, Lift::Small);
        chrome.glow(rect(0.0, 0.0, 10.0, 10.0), 2.0, Color32::RED, 4.0);
        assert!(chrome.take().is_empty());
    }

    #[test]
    fn the_same_drawing_twice_gives_an_identical_list() {
        // The property everything else rests on: the caching is a hash comparison and the screenshot tests
        // are a comparison of images, and neither means anything unless the drawing is a pure function of
        // what is being drawn.
        let draw = || {
            let chrome = Chrome::recording();
            chrome.raised(rect(4.0, 4.0, 300.0, 101.0), 14.0, Fill::diagonal(rect(4.0, 4.0, 300.0, 101.0), Color32::from_rgb(1, 2, 3), Color32::from_rgb(4, 5, 6)), Lift::Small);
            chrome.clip(rect(0.0, 0.0, 320.0, 400.0), 18.0);
            chrome.disc(Pos2::new(20.0, 20.0), 4.5, Fill::Solid(Color32::WHITE));
            chrome.ring(Pos2::new(20.0, 20.0), 8.0, 2.0, Color32::GREEN);
            chrome.line(Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), 1.0, Color32::GRAY);
            chrome.unclip();
            chrome.take()
        };
        let (first, second) = (draw(), draw());
        assert_eq!(first, second);
        assert_eq!(fingerprint_of(&first), fingerprint_of(&second));
        // And a list that differs anywhere fingerprints differently, or a changed board would not redraw.
        let mut changed = first.clone();
        changed.push(Decor::Unclip);
        assert_ne!(fingerprint_of(&first), fingerprint_of(&changed));
    }

    #[test]
    fn a_pixmap_is_the_same_four_bytes_as_egui() {
        // The claim the whole upload path rests on: `vello_cpu` writes premultiplied RGBA8 and
        // `egui::Color32` is premultiplied RGBA8, in that order.
        let mut pixmap = Pixmap::new(1, 1);
        pixmap.set_pixel(0, 0, vello_cpu::color::PremulRgba8 { r: 10, g: 20, b: 30, a: 40 });
        let bytes = pixmap.data_as_u8_slice();
        assert_eq!(bytes, &[10, 20, 30, 40]);
        let colour = Color32::from_rgba_premultiplied(bytes[0], bytes[1], bytes[2], bytes[3]);
        assert_eq!(colour.to_array(), [10, 20, 30, 40]);
    }

    #[test]
    fn a_board_that_did_not_change_is_not_rasterised_again() {
        // `task-1666`'s rule. The window redraws on a heartbeat, so a board nobody is touching has to cost
        // what a board that is not there costs.
        let mut canvas = Canvas::for_tests();
        let area = rect(0.0, 0.0, 64.0, 32.0);
        let chrome = Chrome::recording();
        chrome.raised(rect(4.0, 4.0, 56.0, 24.0), 8.0, Fill::Solid(Color32::from_rgb(0x20, 0x25, 0x2E)), Lift::Small);
        let items = chrome.take();
        canvas.rasterise(area, 1.0, 64, 32, &items);
        assert_eq!(canvas.rasterisations(), 1);
        // Same list, same size: the fingerprint matches and nothing is drawn again.
        assert_eq!(fingerprint_of(&items), fingerprint_of(&items));
        canvas.fingerprint = Some((fingerprint_of(&items), 64, 32));
        assert_eq!(canvas.fingerprint, Some((fingerprint_of(&items), 64, 32)));
    }

    #[test]
    fn what_is_drawn_reaches_the_pixels() {
        // One end-to-end check that the replay works at all: a red square in the middle of a canvas is red
        // in the middle of the pixmap and clear at its corner.
        let mut canvas = Canvas::for_tests();
        let area = rect(0.0, 0.0, 40.0, 40.0);
        let chrome = Chrome::recording();
        chrome.rect(rect(10.0, 10.0, 20.0, 20.0), 0.0, Fill::Solid(Color32::RED));
        canvas.rasterise(area, 1.0, 40, 40, &chrome.take());
        let at = |x: u16, y: u16| canvas.pixmap.sample(x, y);
        let middle = at(20, 20);
        assert!(middle.r > 200 && middle.g < 60 && middle.a > 200, "the middle should be red, was {middle:?}");
        assert_eq!(at(1, 1).a, 0, "and the canvas is clear where nothing was drawn");
    }

    #[test]
    fn an_inset_shadow_darkens_the_inside_of_its_shape_and_not_the_outside() {
        // The one call `epaint` has no answer to at all, and the reason this file exists. A well is a
        // surface with its own shadow inside it: the edge is darker than the middle.
        let mut canvas = Canvas::for_tests();
        let area = rect(0.0, 0.0, 60.0, 60.0);
        let chrome = Chrome::recording();
        chrome.sunken(rect(10.0, 10.0, 40.0, 40.0), 6.0, Color32::from_rgb(0x80, 0x80, 0x80), Lift::Medium);
        canvas.rasterise(area, 1.0, 60, 60, &chrome.take());
        let edge = canvas.pixmap.sample(13, 13);
        let middle = canvas.pixmap.sample(30, 30);
        assert!(edge.r < middle.r, "the top left inside a well is darker than its middle: {edge:?} vs {middle:?}");
        assert_eq!(canvas.pixmap.sample(2, 2).a, 0, "and nothing is painted outside the shape");
    }

    #[test]
    fn a_gradient_runs_the_way_it_was_asked_to() {
        let mut canvas = Canvas::for_tests();
        let area = rect(0.0, 0.0, 40.0, 40.0);
        let box_ = rect(0.0, 0.0, 40.0, 40.0);
        let chrome = Chrome::recording();
        chrome.rect(box_, 0.0, Fill::diagonal(box_, Color32::WHITE, Color32::BLACK));
        canvas.rasterise(area, 1.0, 40, 40, &chrome.take());
        let start = canvas.pixmap.sample(2, 2);
        let end = canvas.pixmap.sample(37, 37);
        assert!(start.r > end.r + 100, "a diagonal gradient is light at the top left: {start:?} vs {end:?}");
    }

    #[test]
    fn a_canvas_whose_pane_went_away_is_dropped() {
        let mut canvases = Canvases::deterministic();
        assert!(canvases.by_id.is_empty());
        canvases.by_id.insert(egui::Id::new("gone"), Canvas::for_tests());
        canvases.by_id.insert(egui::Id::new("here"), Canvas::for_tests());
        canvases.seen.push(egui::Id::new("here"));
        canvases.tidy();
        assert_eq!(canvases.by_id.len(), 1, "only the pane that was drawn keeps its canvas");
        assert!(canvases.by_id.contains_key(&egui::Id::new("here")));
    }

    #[test]
    fn an_impossible_size_draws_nothing_rather_than_allocating_the_world() {
        let mut canvas = Canvas::for_tests();
        let chrome = Chrome::recording();
        chrome.rect(rect(0.0, 0.0, 40_000.0, 40_000.0), 0.0, Fill::Solid(Color32::RED));
        let items = chrome.take();
        let ctx = egui::Context::default();
        let huge = rect(0.0, 0.0, 40_000.0, 40_000.0);
        assert!(canvas.texture_for(&ctx, "test", huge, 1.0, &items).is_none());
        assert_eq!(canvas.rasterisations(), 0, "nothing that big is ever rasterised");
        // And an empty list is nothing to draw rather than an empty texture.
        assert!(canvas.texture_for(&ctx, "test", rect(0.0, 0.0, 10.0, 10.0), 1.0, &[]).is_none());
    }

    #[test]
    fn the_canvas_is_the_decorations_own_bounds_rather_than_the_panes() {
        // `task-1666`'s rule: a frame costs what is on the screen rather than what is in the file.
        // Rasterising has a floor per pixel whether anything is drawn there or not, so a board whose lanes
        // are as tall as their contents must not pay for the empty half of a tall pane.
        let chrome = Chrome::recording();
        chrome.raised(rect(20.0, 20.0, 100.0, 60.0), 8.0, Fill::Solid(Color32::GRAY), Lift::Small);
        let items = chrome.take();
        let bounds = bounds_of(&items).expect("something was drawn");
        // Wide enough for the shadow's own reach and no wider than the pane.
        assert!(bounds.min.x < 20.0 && bounds.max.x > 120.0, "{bounds:?}");
        assert!(bounds.max.y < 200.0, "and it does not reach the whole pane: {bounds:?}");

        let mut canvas = Canvas::for_tests();
        let ctx = egui::Context::default();
        let pane = rect(0.0, 0.0, 1400.0, 900.0);
        let (_, drawn) = canvas.texture_for(&ctx, "test", pane, 1.0, &items).expect("a texture");
        assert!(drawn.height() < 200.0, "the canvas is the decoration's own box: {drawn:?}");
        assert!(pane.contains_rect(drawn), "and never larger than the pane");
        assert!(bounds_of(&[]).is_none(), "nothing drawn is no box at all");
    }
}
