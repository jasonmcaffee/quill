//! The pictures the Markdown preview draws, decoded once and kept.
//!
//! `unluminate_core::markdown` says where a picture goes and what file it names; it cannot read a file or
//! decode one, because that crate has no user interface dependency. This is the other half: it turns
//! the name into a texture on the graphics card, and remembers it so a preview redrawn sixty times a
//! second decodes a photograph once.
//!
//! Three decisions worth writing down.
//!
//! **Nothing is fetched.** A source with a scheme in it — `https:`, `data:` — is refused rather than
//! read. Unluminate makes no network requests, and a preview that quietly fetched from the internet while
//! a file was being read would be a surprise, and a way of tracking who is reading what.
//!
//! **A picture is re-read when the file underneath it changes.** The entry remembers when the file
//! was last written, so a screenshot that has been changed on disk is decoded again rather than
//! drawn from what was read the first time. The question is asked when the preview is worked out
//! again — when the source, the width or the font changes, or on `Reload from Disk` — and not on
//! every frame, so a preview of a document full of pictures costs one `metadata` call each and
//! nothing at all while it is only being looked at.
//!
//! **A picture that will not decode leaves its alt text**, which is what the preview showed before
//! `task-1659` and what every other previewer does. The entry is kept either way, so a missing file
//! is not retried on every frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A picture that is ready to draw.
#[derive(Clone)]
pub struct Ready {
    pub texture: egui::TextureHandle,
    /// How big the picture is in its own pixels, which is what the drawn size is worked out from.
    pub size: [usize; 2],
}

/// One picture, whether or not it decoded.
struct Entry {
    ready: Option<Ready>,
    /// When the file was last written, so a picture that has been changed is read again.
    modified: Option<SystemTime>,
}

/// Every picture the preview has asked for.
#[derive(Default)]
pub struct PreviewImages {
    known: HashMap<PathBuf, Entry>,
}

impl PreviewImages {
    pub fn new() -> Self {
        Self::default()
    }

    /// The picture `source` names, read relative to `folder` — the folder holding the document.
    ///
    /// `None` means there is nothing to draw and the alt text should be shown instead: no folder, a
    /// source that is not a path, a file that is not there, or one that will not decode.
    pub fn ready(
        &mut self,
        ctx: &egui::Context,
        folder: Option<&Path>,
        source: &str,
    ) -> Option<Ready> {
        let path = resolve(folder, source)?;
        let modified = std::fs::metadata(&path).ok().and_then(|meta| meta.modified().ok());
        if let Some(entry) = self.known.get(&path) {
            if entry.modified == modified {
                return entry.ready.clone();
            }
        }
        let name = format!("unluminate-preview-{}", path.display());
        let ready = crate::services::picture::decode(&path).ok().map(|image| {
            let size = image.size;
            // Linear both ways. A picture in a preview is nearly always being scaled *down* to the
            // width of the pane, and nearest neighbour throws away most of the rows and columns and
            // leaves a screenshot ragged and full of speckles.
            let texture =
                crate::services::picture::upload(ctx, name, image, egui::TextureOptions::LINEAR);
            Ready { texture, size }
        });
        self.known.insert(path, Entry { ready: ready.clone(), modified });
        ready
    }

    /// How many pictures are being held, for a test.
    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }
}

/// Turn what was written between the brackets into a file on this machine, or nothing.
///
/// Handles the three things a Markdown source is allowed to carry beyond a path: angle brackets
/// round it, a title in quotes after it, and `%20` where a space is. Anything with a scheme is
/// refused. An absolute path is allowed and is used as it is, including one outside the project:
/// somebody who writes one means that file, and Unluminate already opens any file the person running it
/// can read.
pub fn resolve(folder: Option<&Path>, source: &str) -> Option<PathBuf> {
    let source = source.trim();
    // A title after the path: `![alt](picture.png "A title")`.
    let source = match source.find(['"', '\'']) {
        Some(quote) => source[..quote].trim(),
        None => source,
    };
    let source = source.trim_start_matches('<').trim_end_matches('>').trim();
    if source.is_empty() {
        return None;
    }
    // Nothing with a scheme is read. `://` catches `https://`, and the bare `data:` and `mailto:`
    // forms are caught by the colon before any separator.
    if source.contains("://") || source.split_once(':').is_some_and(|(head, _)| looks_like_scheme(head)) {
        return None;
    }
    let decoded = source.replace("%20", " ");
    let path = Path::new(&decoded);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        folder?.join(path)
    };
    path.is_file().then_some(path)
}

/// True when what comes before a colon reads as a scheme rather than as a Windows drive letter.
///
/// `C:/pictures/one.png` is a path and `data:image/png` is not, and the only difference is the
/// length of what is in front of the colon.
fn looks_like_scheme(head: &str) -> bool {
    head.len() > 1 && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let folder = std::env::temp_dir().join("unluminate-preview-images");
        std::fs::create_dir_all(folder.join("pictures")).expect("make the folders");
        std::fs::write(folder.join("pictures/one.png"), [0_u8]).expect("write a file");
        std::fs::write(folder.join("with space.png"), [0_u8]).expect("write another");
        folder
    }

    #[test]
    fn a_relative_source_is_read_beside_the_document() {
        let folder = folder();
        assert_eq!(
            resolve(Some(&folder), "pictures/one.png"),
            Some(folder.join("pictures/one.png"))
        );
    }

    #[test]
    fn a_source_that_is_not_there_is_nothing_to_draw() {
        assert_eq!(resolve(Some(&folder()), "pictures/missing.png"), None);
    }

    #[test]
    fn a_title_after_the_path_is_not_part_of_the_path() {
        let folder = folder();
        assert_eq!(
            resolve(Some(&folder), "pictures/one.png \"A title\""),
            Some(folder.join("pictures/one.png"))
        );
    }

    #[test]
    fn angle_brackets_round_the_path_are_not_part_of_the_path() {
        let folder = folder();
        assert_eq!(resolve(Some(&folder), "<pictures/one.png>"), Some(folder.join("pictures/one.png")));
    }

    #[test]
    fn a_space_written_as_a_percent_escape_is_still_a_space() {
        let folder = folder();
        assert_eq!(resolve(Some(&folder), "with%20space.png"), Some(folder.join("with space.png")));
    }

    #[test]
    fn nothing_is_fetched_over_a_network() {
        let folder = folder();
        assert_eq!(resolve(Some(&folder), "https://example.com/one.png"), None);
        assert_eq!(resolve(Some(&folder), "data:image/png;base64,AAAA"), None);
    }

    #[test]
    fn an_absolute_path_is_used_as_it_is_even_with_no_folder_behind_it() {
        let folder = folder();
        let absolute = folder.join("pictures/one.png");
        assert_eq!(resolve(None, &absolute.display().to_string()), Some(absolute));
    }

    #[test]
    fn a_windows_drive_letter_is_a_path_and_not_a_scheme() {
        assert!(!looks_like_scheme("C"));
        assert!(looks_like_scheme("https"));
        assert!(looks_like_scheme("data"));
    }

    #[test]
    fn a_document_that_has_never_been_saved_has_nowhere_to_read_a_picture_from() {
        assert_eq!(resolve(None, "pictures/one.png"), None);
    }
}
