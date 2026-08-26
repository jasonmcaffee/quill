//! The Quill editor.
//!
//! This crate holds the whole editor: the text buffer, the character and paragraph formatting, the
//! caret and selection, layout, and undo. It has no user interface dependencies, so its tests run with
//! no window, no graphics card and no fonts.
//!
//! That boundary is deliberate. `quill-app` supplies a window and a way to draw, and it implements
//! [`metrics::FontMetrics`] against real font files. Replacing the whole user interface layer would
//! not touch this crate.

pub mod completion;
pub mod cursor;
pub mod document;
pub mod highlights;
pub mod imports;
pub mod layout;
pub mod markdown;
pub mod mermaid;
pub mod metrics;
pub mod rope;
pub mod scroll_sync;
pub mod style;
pub mod symbols;
pub mod syntax;

pub use completion::{Candidate, Row as CompletionRow, Source as CompletionSource};
pub use cursor::Selection;
pub use document::{Command, Document};
pub use highlights::{Highlight, Highlights, Rgba};
pub use imports::Context as ImportContext;
pub use layout::{
    layout, relayout, Anchor, Caret, ClusterText, Layout, PlacedCluster, PlacedLine, PlacedRun,
    Rect,
};
pub use metrics::{FixedMetrics, FontMetrics, LineMetrics, ScaledMetrics};
pub use markdown::{Preview, PreviewColors, PreviewDiagram, PreviewImage};
pub use rope::Rope;
pub use scroll_sync::{preview_y_for_source_y, source_y_for_preview_y};
pub use style::{Align, CharStyle, Color, ParagraphStyle, ParagraphStyles, StyleChange, StyleSpans};
pub use symbols::{Confidence, Definition, Occurrence, Role, SymbolKind};
pub use syntax::{highlight, Grammar, ImportStyle, PathRoot, Token};
