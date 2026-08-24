//! Real fonts behind the editor's measurements, and rasterised glyphs for painting.
//!
//! `quill-core` measures text through the [`quill_core::FontMetrics`] trait and never asks how a glyph
//! is drawn. This module is the one implementation of that trait that uses real font files. It finds
//! installed families with `fontdb`, reads and rasterises them with `ab_glyph`, and keeps the resulting
//! pixels in one texture that the editor paints from.
//!
//! Keeping every glyph in one texture and drawing each one as a textured rectangle is the approach
//! glyphon uses (<https://github.com/grovesNL/glyphon>, commit 49dc8f7b). Drawing one rectangle per
//! glyph out of one texture means the whole visible document is a single mesh.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use ab_glyph::{Font as _, FontVec, Glyph, PxScale, ScaleFont as _};
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use quill_core::{CharStyle, FontMetrics, LineMetrics};

/// Families Quill offers in the toolbar. Only the ones the operating system actually has are shown, so
/// the list is right on macOS and on Windows without asking which one we are on.
const CANDIDATE_FAMILIES: &[&str] = &[
    "Helvetica",
    "Arial",
    "Times New Roman",
    "Georgia",
    "Verdana",
    "Courier New",
    "Courier",
    "Menlo",
    "Consolas",
    "Segoe UI",
];

/// Extra space between one line and the next, as a fraction of the point size.
///
/// A font's own line height sets the lines as close together as the shapes allow, which is tiring to read
/// at length, so every editor adds some. The design's lines sit about half the point size further
/// apart than Helvetica's own metrics ask for, and this is that extra. It is asked for here rather than in
/// `quill-core` so that the layout arithmetic and its tests stay exact and platform independent.
const READING_LEADING: f32 = 0.45;

/// The atlas is one texture of this many pixels on each side. A page of text at ordinary sizes needs a
/// few hundred distinct glyphs, so this holds far more than one screen.
const ATLAS_SIDE: usize = 1024;

/// Which face of which family, which is what a font file gives us.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FaceKey {
    family: String,
    bold: bool,
    italic: bool,
}

impl FaceKey {
    fn of(style: &CharStyle) -> Self {
        Self { family: style.family.clone(), bold: style.bold, italic: style.italic }
    }
}

/// One rasterised glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct AtlasGlyph {
    /// Where the pixels are in the texture, as a fraction of the texture size.
    pub uv: egui::Rect,
    /// How large to draw it, in points.
    pub size: egui::Vec2,
    /// Where its top left corner goes relative to the pen position on the baseline.
    pub offset: egui::Vec2,
}

/// A glyph at one size, which is what the atlas is keyed by. The size is held in quarter points so that
/// the key can be hashed and so that nearly equal sizes share an entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: FaceKey,
    character: char,
    quarter_points: u32,
}

struct Atlas {
    image: ColorImage,
    texture: Option<TextureHandle>,
    entries: HashMap<GlyphKey, Option<AtlasGlyph>>,
    /// Where the next glyph goes.
    pen_x: usize,
    pen_y: usize,
    row_height: usize,
    /// True when the texture needs uploading again.
    changed: bool,
    /// Bumped whenever the atlas is cleared, so a caller holding positions from it can tell they are
    /// no longer valid.
    generation: u64,
}

impl Atlas {
    fn new() -> Self {
        Self {
            image: ColorImage::filled([ATLAS_SIDE, ATLAS_SIDE], Color32::TRANSPARENT),
            texture: None,
            entries: HashMap::new(),
            pen_x: 0,
            pen_y: 0,
            row_height: 0,
            changed: true,
            generation: 0,
        }
    }

    /// Reserve a rectangle of the atlas, moving to a new row when the current one is full.
    ///
    /// Returns `None` when the atlas is full. The caller then clears it and tries again, which loses
    /// the cache for one frame and is far better than failing to draw.
    fn reserve(&mut self, width: usize, height: usize) -> Option<(usize, usize)> {
        if width > ATLAS_SIDE || height > ATLAS_SIDE {
            return None;
        }
        if self.pen_x + width > ATLAS_SIDE {
            self.pen_x = 0;
            self.pen_y += self.row_height + 1;
            self.row_height = 0;
        }
        if self.pen_y + height > ATLAS_SIDE {
            return None;
        }
        let at = (self.pen_x, self.pen_y);
        self.pen_x += width + 1;
        self.row_height = self.row_height.max(height);
        Some(at)
    }

    fn clear(&mut self) {
        self.image = ColorImage::filled([ATLAS_SIDE, ATLAS_SIDE], Color32::TRANSPARENT);
        self.entries.clear();
        self.pen_x = 0;
        self.pen_y = 0;
        self.row_height = 0;
        self.changed = true;
        self.generation += 1;
    }
}

/// Fonts, measurements and rasterised glyphs.
pub struct TextRenderer {
    database: fontdb::Database,
    /// Families that are installed, in the order the toolbar shows them.
    families: Vec<String>,
    /// Faces already read from disk. An entry holding `None` is a face this system does not have, kept
    /// so that we do not search the database for it again on every frame.
    faces: RefCell<HashMap<FaceKey, Option<Arc<FontVec>>>>,
    atlas: RefCell<Atlas>,
}

impl TextRenderer {
    pub fn new() -> Self {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let installed: Vec<String> = CANDIDATE_FAMILIES
            .iter()
            .filter(|family| {
                database
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name(family)],
                        ..Default::default()
                    })
                    .is_some()
            })
            .map(|family| (*family).to_owned())
            .collect();
        Self {
            database,
            families: installed,
            faces: RefCell::new(HashMap::new()),
            atlas: RefCell::new(Atlas::new()),
        }
    }

    /// The families the toolbar offers.
    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// A monospaced family this system has, for setting code in the Markdown preview. `None` when it has
    /// none of them, in which case code is set in the ordinary family.
    pub fn monospaced_family(&self) -> Option<String> {
        const MONOSPACED: &[&str] = &["Menlo", "Consolas", "Courier New", "Courier"];
        self.families.iter().find(|family| MONOSPACED.contains(&family.as_str())).cloned()
    }

    /// The family to start a new document in: the first candidate this system has.
    pub fn default_family(&self) -> String {
        self.families.first().cloned().unwrap_or_else(|| "Helvetica".to_owned())
    }

    /// Read a face from disk, or report that this system does not have it.
    ///
    /// Bold and italic pick a real face of the family rather than slanting or thickening the regular
    /// one. Helvetica on macOS ships regular, bold, oblique and bold oblique in one collection file, so
    /// the real faces are there to be used.
    fn face(&self, key: &FaceKey) -> Option<Arc<FontVec>> {
        if let Some(found) = self.faces.borrow().get(key) {
            return found.clone();
        }
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(&key.family)],
            weight: if key.bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
            style: if key.italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
            stretch: fontdb::Stretch::Normal,
        };
        let loaded = self
            .database
            .query(&query)
            .and_then(|id| {
                self.database.with_face_data(id, |data, index| {
                    FontVec::try_from_vec_and_index(data.to_vec(), index).ok()
                })
            })
            .flatten()
            .map(Arc::new);
        self.faces.borrow_mut().insert(key.clone(), loaded.clone());
        loaded
    }

    /// The face to use for a style, falling back through the italic and bold variants to the regular
    /// one, and then to any family this system has. A missing font must not stop text appearing.
    fn face_for(&self, style: &CharStyle) -> Option<Arc<FontVec>> {
        let key = FaceKey::of(style);
        if let Some(face) = self.face(&key) {
            return Some(face);
        }
        for fallback in [
            FaceKey { italic: false, ..key.clone() },
            FaceKey { bold: false, italic: false, ..key.clone() },
        ] {
            if let Some(face) = self.face(&fallback) {
                return Some(face);
            }
        }
        self.families.first().and_then(|family| {
            self.face(&FaceKey { family: family.clone(), bold: false, italic: false })
        })
    }

    /// Find or rasterise one glyph.
    pub fn glyph(&self, character: char, style: &CharStyle) -> Option<AtlasGlyph> {
        let key = GlyphKey {
            face: FaceKey::of(style),
            character,
            quarter_points: (style.size * 4.0).round() as u32,
        };
        if let Some(found) = self.atlas.borrow().entries.get(&key) {
            return *found;
        }
        let entry = self.rasterise(character, style);
        if entry.is_none_full() {
            // The atlas ran out of room. Start it again rather than stop drawing.
            self.atlas.borrow_mut().clear();
            let retry = self.rasterise(character, style);
            self.atlas.borrow_mut().entries.insert(key, retry.glyph());
            return retry.glyph();
        }
        self.atlas.borrow_mut().entries.insert(key, entry.glyph());
        entry.glyph()
    }

    fn rasterise(&self, character: char, style: &CharStyle) -> Rasterised {
        let Some(face) = self.face_for(style) else {
            return Rasterised::NoFont;
        };
        let scaled = face.as_scaled(PxScale::from(style.size));
        let glyph: Glyph = face.glyph_id(character).with_scale_and_position(
            PxScale::from(style.size),
            ab_glyph::point(0.0, 0.0),
        );
        let Some(outlined) = face.outline_glyph(glyph) else {
            // A space has no outline. It still advances the pen, which the metrics report separately.
            let _ = scaled;
            return Rasterised::Blank;
        };
        let bounds = outlined.px_bounds();
        let width = bounds.width().ceil() as usize;
        let height = bounds.height().ceil() as usize;
        if width == 0 || height == 0 {
            return Rasterised::Blank;
        }
        let mut atlas = self.atlas.borrow_mut();
        let Some((x, y)) = atlas.reserve(width, height) else {
            return Rasterised::AtlasFull;
        };
        // The rasteriser reports coverage from 0 to 1 for each pixel. The atlas holds white pixels
        // whose alpha is that coverage, so that painting can tint one texture any colour.
        outlined.draw(|dx, dy, coverage| {
            let px = x + dx as usize;
            let py = y + dy as usize;
            if px < ATLAS_SIDE && py < ATLAS_SIDE {
                let alpha = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                atlas.image[(px, py)] = Color32::from_white_alpha(alpha);
            }
        });
        atlas.changed = true;
        let side = ATLAS_SIDE as f32;
        Rasterised::Glyph(AtlasGlyph {
            uv: egui::Rect::from_min_max(
                egui::pos2(x as f32 / side, y as f32 / side),
                egui::pos2((x + width) as f32 / side, (y + height) as f32 / side),
            ),
            size: egui::vec2(width as f32, height as f32),
            offset: egui::vec2(bounds.min.x, bounds.min.y),
        })
    }

    /// The bytes of a face, for handing to egui so that the interface itself is set in a real font rather
    /// than egui's built in one. egui needs the file contents; it does its own parsing.
    pub fn face_bytes(&self, family: &str, bold: bool) -> Option<Vec<u8>> {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
            style: fontdb::Style::Normal,
            stretch: fontdb::Stretch::Normal,
        };
        let id = self.database.query(&query)?;
        self.database.with_face_data(id, |data, _index| data.to_vec())
    }

    /// How many times the atlas has been cleared. A caller that collected glyph positions and then sees
    /// this change knows those positions point at pixels that have been overwritten.
    pub fn generation(&self) -> u64 {
        self.atlas.borrow().generation
    }

    /// The texture to paint glyphs from, uploaded again only when new glyphs were added.
    ///
    /// Every glyph the caller intends to draw must be asked for through [`Self::glyph`] before this is
    /// called. Uploading first and rasterising afterwards would draw this frame from a texture that
    /// does not yet hold the new glyphs, and the letters would be missing.
    pub fn texture(&self, ctx: &egui::Context) -> egui::TextureId {
        let mut atlas = self.atlas.borrow_mut();
        if atlas.texture.is_none() {
            let image = atlas.image.clone();
            atlas.texture = Some(ctx.load_texture("quill-glyphs", image, TextureOptions::NEAREST));
            atlas.changed = false;
        } else if atlas.changed {
            let image = atlas.image.clone();
            atlas.texture.as_mut().expect("just checked").set(image, TextureOptions::NEAREST);
            atlas.changed = false;
        }
        atlas.texture.as_ref().expect("set above").id()
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// What came back from trying to rasterise a glyph. `AtlasFull` is separated from the other failures
/// because it is the one worth retrying after clearing the atlas.
enum Rasterised {
    Glyph(AtlasGlyph),
    /// No outline to draw, such as a space.
    Blank,
    /// This system has no font at all.
    NoFont,
    AtlasFull,
}

impl Rasterised {
    fn glyph(&self) -> Option<AtlasGlyph> {
        match self {
            Self::Glyph(glyph) => Some(*glyph),
            _ => None,
        }
    }

    fn is_none_full(&self) -> bool {
        matches!(self, Self::AtlasFull)
    }
}

impl FontMetrics for TextRenderer {
    fn advance(&self, cluster: &str, style: &CharStyle) -> f32 {
        let Some(face) = self.face_for(style) else {
            // With no font at all, fall back to a fixed width so that layout still works and the
            // caret still moves. Text will not appear, which is a visible failure rather than a hang.
            return style.size * 0.5 * cluster.chars().count().max(1) as f32;
        };
        let scaled = face.as_scaled(PxScale::from(style.size));
        // A cluster of several code points, such as a letter and a combining accent, takes the width of
        // its base character: the accent is drawn over the letter rather than after it.
        let Some(base) = cluster.chars().next() else {
            return 0.0;
        };
        if base == '\t' {
            return scaled.h_advance(face.glyph_id(' ')) * 4.0;
        }
        scaled.h_advance(face.glyph_id(base))
    }

    fn line_metrics(&self, style: &CharStyle) -> LineMetrics {
        let Some(face) = self.face_for(style) else {
            return LineMetrics { ascent: style.size, descent: style.size * 0.25, line_gap: 0.0 };
        };
        let scaled = face.as_scaled(PxScale::from(style.size));
        LineMetrics {
            ascent: scaled.ascent(),
            // ab_glyph reports the descent as a negative number, below the baseline.
            descent: -scaled.descent(),
            line_gap: scaled.line_gap() + style.size * READING_LEADING,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_system_has_at_least_one_of_the_offered_families() {
        let renderer = TextRenderer::new();
        assert!(
            !renderer.families().is_empty(),
            "none of {CANDIDATE_FAMILIES:?} is installed, so no text could be drawn"
        );
    }

    #[test]
    fn a_wider_letter_advances_further_than_a_narrow_one() {
        let renderer = TextRenderer::new();
        let style = CharStyle { family: renderer.default_family(), ..CharStyle::default() };
        let narrow = renderer.advance("i", &style);
        let wide = renderer.advance("W", &style);
        assert!(wide > narrow, "W ({wide}) should be wider than i ({narrow})");
    }

    #[test]
    fn a_bigger_font_size_advances_further_and_stands_taller() {
        let renderer = TextRenderer::new();
        let small = CharStyle { family: renderer.default_family(), size: 12.0, ..CharStyle::default() };
        let large = CharStyle { size: 36.0, ..small.clone() };
        assert!(renderer.advance("m", &large) > renderer.advance("m", &small));
        assert!(renderer.line_metrics(&large).height() > renderer.line_metrics(&small).height());
    }

    #[test]
    fn bold_text_is_at_least_as_wide_as_regular_text() {
        let renderer = TextRenderer::new();
        let regular = CharStyle { family: renderer.default_family(), ..CharStyle::default() };
        let bold = CharStyle { bold: true, ..regular.clone() };
        assert!(renderer.advance("mmmm", &bold) >= renderer.advance("mmmm", &regular));
    }

    #[test]
    fn an_unknown_family_still_measures_and_still_draws() {
        let renderer = TextRenderer::new();
        let style = CharStyle { family: "No Such Font At All".to_owned(), ..CharStyle::default() };
        assert!(renderer.advance("a", &style) > 0.0, "a missing family must not stop layout");
        assert!(renderer.line_metrics(&style).height() > 0.0);
        assert!(renderer.glyph('a', &style).is_some(), "it should fall back to a family we have");
    }

    #[test]
    fn glyphs_are_rasterised_once_and_then_reused() {
        let renderer = TextRenderer::new();
        let style = CharStyle { family: renderer.default_family(), ..CharStyle::default() };
        let first = renderer.glyph('A', &style).expect("A should rasterise");
        let second = renderer.glyph('A', &style).expect("A should still be there");
        assert_eq!(renderer.atlas.borrow().entries.len(), 1, "asked twice, stored once");
        assert_eq!(first.uv, second.uv, "the same glyph comes back from the same place");
        assert!(first.size.x > 0.0 && first.size.y > 0.0);
    }

    #[test]
    fn the_same_letter_at_two_sizes_is_two_entries() {
        let renderer = TextRenderer::new();
        let small = CharStyle { family: renderer.default_family(), size: 12.0, ..CharStyle::default() };
        let large = CharStyle { size: 40.0, ..small.clone() };
        let small_glyph = renderer.glyph('B', &small).expect("rasterise at 12");
        let large_glyph = renderer.glyph('B', &large).expect("rasterise at 40");
        assert_eq!(renderer.atlas.borrow().entries.len(), 2);
        assert!(large_glyph.size.y > small_glyph.size.y, "40 point should be taller than 12 point");
    }

    #[test]
    fn a_space_has_width_but_nothing_to_draw() {
        let renderer = TextRenderer::new();
        let style = CharStyle { family: renderer.default_family(), ..CharStyle::default() };
        assert!(renderer.advance(" ", &style) > 0.0, "a space advances the pen");
        assert!(renderer.glyph(' ', &style).is_none(), "a space has no pixels");
    }

    #[test]
    fn the_atlas_never_overlaps_two_glyphs() {
        let renderer = TextRenderer::new();
        let style = CharStyle { family: renderer.default_family(), ..CharStyle::default() };
        for character in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            renderer.glyph(character, &style);
        }
        let atlas = renderer.atlas.borrow();
        let placed: Vec<AtlasGlyph> = atlas.entries.values().flatten().copied().collect();
        assert!(placed.len() > 50, "most of those characters should have pixels");
        for (index, one) in placed.iter().enumerate() {
            for other in &placed[index + 1..] {
                assert!(
                    !one.uv.intersects(other.uv),
                    "two glyphs were given overlapping room in the atlas: {:?} and {:?}",
                    one.uv,
                    other.uv
                );
            }
        }
    }
}
