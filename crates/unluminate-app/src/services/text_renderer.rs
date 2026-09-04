//! Real fonts behind the editor's measurements, and rasterised glyphs for painting.
//!
//! `unluminate-core` measures text through the [`unluminate_core::FontMetrics`] trait and never asks how a glyph
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
use unluminate_core::{CharStyle, FontMetrics, LineMetrics};

/// Families Unluminate offers in the toolbar. Only the ones the operating system actually has are shown, so
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

/// Families to look in for a character the chosen family has no shape for.
///
/// A terminal needs this more than a document does. `claude` and `codex` draw with box drawing characters,
/// arrows, ticks and spinners, and a text face such as Helvetica or a monospaced one such as Menlo does not
/// have all of them. Without somewhere else to look, a missing character comes out as the empty box a font
/// puts at glyph zero, which is what every terminal that has no fallback shows and what no terminal should.
///
/// The list is tried in order and the ones this system does not have are skipped, so it can hold the usual
/// families of both platforms.
const FALLBACK_FAMILIES: &[&str] = &[
    // macOS.
    "Menlo",
    "Apple Symbols",
    "Arial Unicode MS",
    // The media symbols a program uses for pause and play, which the text faces do not carry. macOS itself
    // draws these from Apple Color Emoji, which is a colour bitmap font and has no outline to rasterise, so
    // the maths face is used instead and they come out in the text colour.
    "STIX Two Math",
    "STIXGeneral",
    "Hiragino Sans",
    "PingFang SC",
    "Zapf Dingbats",
    // Windows.
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Cambria Math",
    "MS Gothic",
    // Anywhere.
    "DejaVu Sans",
    "Noto Sans Symbols 2",
];

/// Extra space between one line and the next, as a fraction of the point size.
///
/// A font's own line height sets the lines as close together as the shapes allow, which is tiring to read
/// at length, so every editor adds some. The design's lines sit about half the point size further
/// apart than Helvetica's own metrics ask for, and this is that extra. It is asked for here rather than in
/// `unluminate-core` so that the layout arithmetic and its tests stay exact and platform independent.
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

    /// Whether this is the key a style would build, without building it.
    ///
    /// `of` allocates, because the key owns the family name. Measuring a letter and drawing a letter
    /// each asked for a key, so laying out a file allocated and threw away a `String` per grapheme —
    /// 167 ns a call, times a hundred and sixteen thousand. The memo in [`TextRenderer`] compares
    /// with this instead.
    fn is(&self, style: &CharStyle) -> bool {
        self.bold == style.bold && self.italic == style.italic && self.family == style.family
    }
}

/// The size of one cell of the terminal grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    /// How wide one cell is, which is one character's advance in a monospaced family.
    pub width: f32,
    /// How tall one cell is, from the top of one line to the top of the next.
    pub height: f32,
    /// How far below the top of the cell the baseline sits.
    pub ascent: f32,
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
///
/// The face is a small number rather than a family name and two flags, because this key is built and
/// hashed once for every character on the screen every frame, and hashing a `String` there meant
/// allocating one first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: FaceId,
    character: char,
    quarter_points: u32,
}

/// Which face, as a number. Handed out in the order the faces are first asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FaceId(u32);

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
    /// Which family a character was found in when the chosen one had no shape for it, so the search runs
    /// once for each character rather than on every measurement.
    fallbacks: RefCell<HashMap<(char, bool, bool), Option<String>>>,
    /// A number for each face that has been asked for, so that the atlas can be keyed on something
    /// small. The face itself is `faces`; this is only the naming.
    ids: RefCell<HashMap<FaceKey, FaceId>>,
    /// The last face resolved, and the key it answers to.
    ///
    /// Layout and painting both walk run by run, and every character of a run has the same style, so
    /// one entry answers nearly every question. It is compared with [`FaceKey::is`], which looks at
    /// the family name rather than copying it.
    memo: RefCell<Option<(FaceKey, FaceId, Option<Arc<FontVec>>)>>,
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
            fallbacks: RefCell::new(HashMap::new()),
            ids: RefCell::new(HashMap::new()),
            memo: RefCell::new(None),
            atlas: RefCell::new(Atlas::new()),
        }
    }

    /// The families the toolbar offers.
    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// A monospaced family this system has, for setting code in the Markdown preview. `None` when it has
    /// none of them, in which case code is set in the ordinary family.
    /// The order is this list's own rather than the order the families are offered in, because for a
    /// terminal and for code the choice matters: Menlo and Consolas are designed for it and have far wider
    /// coverage of the box drawing characters and arrows a program draws its own screen with than Courier
    /// does.
    pub fn monospaced_family(&self) -> Option<String> {
        const MONOSPACED: &[&str] = &["Menlo", "Consolas", "Courier New", "Courier"];
        MONOSPACED
            .iter()
            .find(|wanted| self.families.iter().any(|family| family == *wanted))
            .map(|family| (*family).to_owned())
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
        self.resolve(style).1
    }

    /// The face for a style and the number it answers to, remembering the last one asked for.
    fn resolve(&self, style: &CharStyle) -> (FaceId, Option<Arc<FontVec>>) {
        if let Some((key, id, face)) = self.memo.borrow().as_ref() {
            if key.is(style) {
                return (*id, face.clone());
            }
        }
        let key = FaceKey::of(style);
        let face = self.search(&key);
        let next = self.ids.borrow().len() as u32;
        let id = *self.ids.borrow_mut().entry(key.clone()).or_insert(FaceId(next));
        *self.memo.borrow_mut() = Some((key, id, face.clone()));
        (id, face)
    }

    /// Find the face for a style, falling back as [`Self::face_for`] describes. Called once per style
    /// rather than once per letter, because [`Self::resolve`] remembers the answer.
    fn search(&self, key: &FaceKey) -> Option<Arc<FontVec>> {
        let key = key.clone();
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

    /// The face to draw one character with: the style's own family when it has a shape for the character,
    /// and the first family in [`FALLBACK_FAMILIES`] that does when it has not.
    ///
    /// Glyph zero is the empty box a font uses for a character it has no shape for, so asking for a
    /// character and being given glyph zero is how a missing character is found.
    fn face_for_character(&self, character: char, style: &CharStyle) -> Option<Arc<FontVec>> {
        let chosen = self.face_for(style)?;
        if chosen.glyph_id(character).0 != 0 || character.is_whitespace() {
            return Some(chosen);
        }
        let key = (character, style.bold, style.italic);
        if let Some(found) = self.fallbacks.borrow().get(&key) {
            return match found {
                Some(family) => self
                    .face(&FaceKey { family: family.clone(), bold: style.bold, italic: style.italic })
                    .or(Some(chosen)),
                None => Some(chosen),
            };
        }
        let mut answer = None;
        for family in FALLBACK_FAMILIES.iter().map(|family| (*family).to_owned()) {
            let candidate = FaceKey { family: family.clone(), bold: style.bold, italic: style.italic };
            if let Some(face) = self.face(&candidate) {
                if face.glyph_id(character).0 != 0 {
                    answer = Some((family, face));
                    break;
                }
            }
        }
        self.fallbacks
            .borrow_mut()
            .insert(key, answer.as_ref().map(|(family, _)| family.clone()));
        Some(answer.map(|(_, face)| face).unwrap_or(chosen))
    }

    /// Find or rasterise one glyph.
    pub fn glyph(&self, character: char, style: &CharStyle) -> Option<AtlasGlyph> {
        let key = GlyphKey {
            face: self.resolve(style).0,
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
        let Some(face) = self.face_for_character(character, style) else {
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

    /// The size of one cell of the terminal grid, at `size` points.
    ///
    /// The terminal is a grid, so every cell is the same size and it is worked out once: the width is what
    /// one character advances in the monospaced family, and the height is what the font itself asks for
    /// between one line and the next. The reading leading that `line_metrics` adds for prose is left out
    /// here, because a terminal's lines belong close together and because a program drawing a box expects
    /// the lines to meet.
    ///
    /// Both are rounded to whole points, so that a column lands on a whole pixel and the grid stays sharp.
    pub fn cell_metrics(&self, size: f32) -> CellMetrics {
        let style = self.terminal_style(size, false, false);
        let Some(face) = self.face_for(&style) else {
            return CellMetrics { width: size * 0.6, height: size * 1.25, ascent: size };
        };
        let scaled = face.as_scaled(PxScale::from(size));
        // `M` is the widest ordinary letter, and in a monospaced family every letter is that wide.
        let width = scaled.h_advance(face.glyph_id('M')).round().max(1.0);
        let ascent = scaled.ascent();
        let height = (ascent - scaled.descent() + scaled.line_gap()).round().max(1.0);
        CellMetrics { width, height, ascent }
    }

    /// The formatting one terminal cell is drawn with, which is the monospaced family at the terminal's
    /// own size.
    pub fn terminal_style(&self, size: f32, bold: bool, italic: bool) -> CharStyle {
        CharStyle {
            family: self.monospaced_family().unwrap_or_else(|| self.default_family()),
            size,
            bold,
            italic,
            ..CharStyle::default()
        }
    }

    /// The atlas as an image, which a test uses to look at the pixels of one glyph.
    pub fn atlas_image(&self) -> egui::ColorImage {
        self.atlas.borrow().image.clone()
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
            atlas.texture = Some(ctx.load_texture("unluminate-glyphs", image, TextureOptions::NEAREST));
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
        // A cluster of several code points, such as a letter and a combining accent, takes the width of
        // its base character: the accent is drawn over the letter rather than after it.
        let Some(base) = cluster.chars().next() else {
            return 0.0;
        };
        // The face the character is actually drawn with, which may be a fallback, so that a character the
        // chosen family has no shape for is measured as wide as it is drawn.
        let Some(face) = self.face_for_character(base, style) else {
            // With no font at all, fall back to a fixed width so that layout still works and the
            // caret still moves. Text will not appear, which is a visible failure rather than a hang.
            return style.size * 0.5 * cluster.chars().count().max(1) as f32;
        };
        let scaled = face.as_scaled(PxScale::from(style.size));
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

    /// The renderer remembers the last face it resolved, because layout and painting both walk run by
    /// run and every character of a run shares a style. A memo that answered with the previous
    /// style's face would draw a whole run in the wrong font, so this asks the same two styles over
    /// and over in turn.
    #[test]
    fn alternating_between_two_styles_still_gives_each_its_own_glyph() {
        let renderer = TextRenderer::new();
        let regular = CharStyle { size: 20.0, ..CharStyle::default() };
        let bold = CharStyle { size: 20.0, bold: true, ..CharStyle::default() };
        let large = CharStyle { size: 40.0, ..CharStyle::default() };

        let first_regular = renderer.glyph('R', &regular).expect("a shape for R");
        let first_bold = renderer.glyph('R', &bold).expect("a shape for R in bold");
        let first_large = renderer.glyph('R', &large).expect("a shape for R at 40 points");
        for _ in 0..10 {
            let again_regular = renderer.glyph('R', &regular).expect("a shape for R");
            let again_bold = renderer.glyph('R', &bold).expect("a shape for R in bold");
            let again_large = renderer.glyph('R', &large).expect("a shape for R at 40 points");
            assert_eq!(again_regular.uv, first_regular.uv, "the same style gives the same glyph");
            assert_eq!(again_bold.uv, first_bold.uv);
            assert_eq!(again_large.uv, first_large.uv);
        }
        assert_ne!(first_regular.uv, first_bold.uv, "bold is a different entry in the atlas");
        assert_ne!(first_regular.uv, first_large.uv, "and so is another size");
        assert!(first_large.size.y > first_regular.size.y, "and it is drawn larger");
    }

    /// The same for measuring, which is what layout asks once for every grapheme cluster.
    #[test]
    fn alternating_between_two_styles_still_measures_each_on_its_own() {
        let renderer = TextRenderer::new();
        let small = CharStyle { size: 12.0, ..CharStyle::default() };
        let large = CharStyle { size: 48.0, ..CharStyle::default() };
        let small_first = renderer.advance("m", &small);
        let large_first = renderer.advance("m", &large);
        assert!(large_first > small_first);
        for _ in 0..10 {
            assert_eq!(renderer.advance("m", &small), small_first);
            assert_eq!(renderer.advance("m", &large), large_first);
        }
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
    fn a_terminal_cell_is_wider_and_taller_at_a_bigger_size() {
        let renderer = TextRenderer::new();
        let small = renderer.cell_metrics(11.0);
        let large = renderer.cell_metrics(20.0);
        assert!(small.width > 0.0 && small.height > 0.0);
        assert!(large.width > small.width, "a bigger font makes a wider cell");
        assert!(large.height > small.height);
        assert_eq!(large.width, large.width.round(), "a cell is a whole number of points wide");
        assert_eq!(large.height, large.height.round());
        assert!(large.ascent < large.height, "the baseline sits inside the cell");
    }

    #[test]
    fn every_letter_in_the_terminal_family_is_the_same_width() {
        // A terminal is a grid, so this has to hold for the grid to line up. It is checked rather than
        // assumed, because the family is whichever monospaced one the system has.
        let renderer = TextRenderer::new();
        let style = renderer.terminal_style(14.0, false, false);
        let width = renderer.advance("M", &style);
        for letter in ["i", "W", "0", ".", "@"] {
            let other = renderer.advance(letter, &style);
            assert!(
                (other - width).abs() < 0.01,
                "{letter} is {other} wide and M is {width}, so the grid would not line up"
            );
        }
    }

    #[test]
    fn a_character_the_family_has_no_shape_for_is_found_in_another_family() {
        // The characters `claude` and `codex` draw with. A monospaced text face has some of them and not
        // others, and the ones it does not have would otherwise come out as an empty box.
        let renderer = TextRenderer::new();
        let style = renderer.terminal_style(14.0, false, false);
        for character in ['\u{25b6}', '\u{2713}', '\u{2502}', '\u{250c}', '\u{2588}'] {
            let face = renderer
                .face_for_character(character, &style)
                .expect("there should be a face to draw with");
            assert_ne!(
                face.glyph_id(character).0,
                0,
                "{character:?} came out as the empty box a font uses for a character it does not have"
            );
            assert!(
                renderer.glyph(character, &style).is_some(),
                "{character:?} should have pixels to draw"
            );
        }
    }

    #[test]
    fn a_character_no_family_has_is_still_measured_and_still_drawn() {
        // A character in a private use area, which nothing has a shape for. It falls back to the chosen
        // family's own empty box, which is what a terminal shows, rather than to nothing at all.
        let renderer = TextRenderer::new();
        let style = renderer.terminal_style(14.0, false, false);
        assert!(renderer.advance("\u{f8ff}", &style) > 0.0);
        assert!(renderer.face_for_character('\u{101234}', &style).is_some());
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
