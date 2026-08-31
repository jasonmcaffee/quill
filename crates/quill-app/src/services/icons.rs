//! The pictures plugins put in front of their files.
//!
//! A plugin's icon is a PNG in its folder, and egui draws from a texture, so each one is decoded
//! once and kept. Decoding on every frame would decode three pictures sixty times a second for no
//! reason at all.
//!
//! This is why `image` is a dependency of this crate rather than only of its tests. The alternative
//! was to colour the existing square marker in each language's own colour and draw no picture, which
//! needs no dependency and is not what `task-1649` asks for: it asks for custom icons, made on this
//! machine. `image` is taken with only its `png` feature, which is the decoder and nothing else.

use std::collections::HashMap;

/// The size an icon is drawn at in a tab and in an explorer row.
pub const SIZE: f32 = 14.0;

/// The decoded icons, by the plugin they came from.
#[derive(Default)]
pub struct Icons {
    textures: HashMap<String, Option<egui::TextureHandle>>,
}

impl Icons {
    pub fn new() -> Self {
        Self::default()
    }

    /// The texture for `id`, decoding `bytes` the first time it is asked for.
    ///
    /// A picture that will not decode is remembered as absent rather than tried again every frame,
    /// and the row falls back to the coloured square. A missing picture must never be a missing row.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        bytes: &[u8],
    ) -> Option<egui::TextureHandle> {
        if let Some(found) = self.textures.get(id) {
            return found.clone();
        }
        let decoded = decode(bytes).map(|(size, pixels)| {
            ctx.load_texture(
                format!("quill-plugin-icon-{id}"),
                egui::ColorImage { size, pixels, source_size: egui::vec2(size[0] as f32, size[1] as f32) },
                // Linear, because the picture is drawn a little smaller than it was made and nearest
                // neighbour would make its edges ragged.
                egui::TextureOptions::LINEAR,
            )
        });
        self.textures.insert(id.to_owned(), decoded.clone());
        decoded
    }
}

/// Read a PNG into the pixels egui wants.
fn decode(bytes: &[u8]) -> Option<([usize; 2], Vec<egui::Color32>)> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let decoded = decoded.to_rgba8();
    let size = [decoded.width() as usize, decoded.height() as usize];
    let pixels = decoded
        .pixels()
        .map(|pixel| {
            // Premultiplied, because that is what egui's painter expects, and an icon with a
            // transparent background drawn unmultiplied has a pale halo round every edge.
            egui::Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3])
        })
        .collect();
    Some((size, pixels))
}

/// Draw an icon centred at `centre`, at [`SIZE`] points.
pub fn draw(painter: &egui::Painter, centre: egui::Pos2, texture: &egui::TextureHandle) {
    let rect = egui::Rect::from_center_size(centre, egui::Vec2::splat(SIZE));
    painter.image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_icons_all_decode() {
        // Every plugin that ships a picture ships one that decodes. A plugin that draws ships none: its
        // rail button is `pane.icon`, which `theme::icon` draws, so it follows the window's colours and
        // the pointer's opacity the way every other button in the rail does. `the_bundled_plugins_all_
        // parse_and_claim_what_they_should` is what asserts which plugins are in which of the two.
        for (id, _, icon) in crate::services::plugins::bundled::ALL {
            let Some(bytes) = icon else {
                continue;
            };
            let (size, pixels) = decode(bytes).unwrap_or_else(|| panic!("{id} should decode"));
            assert_eq!(size, [32, 32], "{id} is drawn at 32 points across");
            assert_eq!(pixels.len(), 32 * 32);
            // Something is actually drawn in it, rather than the whole picture being transparent.
            assert!(pixels.iter().any(|pixel| pixel.a() > 200), "{id} is empty");
        }
    }

    #[test]
    fn something_that_is_not_a_png_decodes_to_nothing_rather_than_panicking() {
        assert!(decode(b"not a picture").is_none());
        assert!(decode(&[]).is_none());
    }
}
