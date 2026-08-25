//! What kind of file a path is, whether Quill can open it, and what to call it.
//!
//! Quill opens text. It used to open `.md` and `.txt` and nothing else, and dim everything else in the
//! explorer. `tasks/improvements.md` asks for the rest: a file Quill has no special handling for, such as
//! a `.js` or a `.rs` file, opens as plain text, which is what a text editor should do with text.
//!
//! So the question changed from "is this one of two extensions" to "is this text". Three rules answer it,
//! in order:
//!
//! 1. An extension known to hold text, which is most of what a person opens, is text. No reading needed.
//! 2. An extension known to hold something else, such as `.png` or `.zip`, is not text. Also no reading.
//! 3. Anything else, which means an unknown extension or no extension at all, is decided by reading the
//!    first few thousand bytes: a file holding a zero byte, or bytes that are not valid UTF-8, is not
//!    text.
//!
//! Rule 3 is last because it touches the disk, and the explorer asks this question about every file in a
//! folder. Rules 1 and 2 answer it for nearly every file without reading anything.
//!
//! ## Pictures
//!
//! A picture is not text and never will be, but `task-1658` asks to be able to look at one, so it is a
//! second kind of thing Quill opens rather than a file it refuses. [`is_image`] is the whole of the
//! question, answered from the extension alone: a decoder is what decides whether the bytes really are
//! a picture, and it says so when the tab is opened rather than on every row of the explorer.

use std::path::Path;

/// How much of a file is read to decide whether it is text.
const SNIFF: usize = 4096;

/// A file larger than this is not offered, because reading it into the editor would stop the window for
/// as long as it took. Sixteen megabytes of text is around four million words.
pub const SIZE_LIMIT: u64 = 16 * 1024 * 1024;

/// Extensions that hold text. Not exhaustive, and it does not need to be: an extension that is missing
/// falls through to rule 3 and is read.
const TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdx", "txt", "text", "rst", "adoc", "asciidoc", "tex", "bib", "log", "csv",
    "tsv", "rs", "toml", "lock", "json", "jsonc", "json5", "yaml", "yml", "xml", "svg", "html",
    "htm", "css", "scss", "sass", "less", "js", "cjs", "mjs", "jsx", "ts", "tsx", "vue", "svelte",
    "py", "pyi", "rb", "go", "java", "kt", "kts", "scala", "clj", "cljs", "swift", "m", "mm", "c",
    "h", "cc", "cpp", "cxx", "hpp", "hh", "cs", "fs", "php", "pl", "pm", "lua", "r", "dart", "ex",
    "exs", "erl", "hrl", "hs", "ml", "nim", "zig", "v", "sh", "bash", "zsh", "fish", "ps1", "bat",
    "cmd", "make", "mk", "cmake", "gradle", "properties", "ini", "cfg", "conf", "env", "editorconfig",
    "gitignore", "gitattributes", "dockerfile", "sql", "graphql", "gql", "proto", "diff", "patch",
    "plist", "srt", "vtt", "ipynb", "mmd", "mermaid",
];

/// Extensions that hold something other than text, so there is no point reading them.
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "icns", "webp", "tif", "tiff", "avif", "heic", "psd",
    "pdf", "zip", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "jar", "war", "dmg", "iso", "pkg",
    "deb", "rpm", "mp3", "m4a", "wav", "flac", "ogg", "opus", "aac", "mp4", "m4v", "mov", "avi",
    "mkv", "webm", "wmv", "ttf", "otf", "ttc", "woff", "woff2", "eot", "so", "dylib", "dll", "exe",
    "o", "a", "rlib", "rmeta", "class", "pyc", "pyo", "wasm", "bin", "dat", "db", "sqlite",
    "sqlite3", "bundle", "keystore", "p12", "pfx", "der",
];

/// Extensions Quill can show as a picture. Every one of them is a format the `image` crate is built
/// with, so a name here that the decoder does not know would be a tab that opens and stays empty.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "tif", "tiff",
];

/// Names with no extension that are text, so the file does not have to be read to find out.
const TEXT_NAMES: &[&str] = &[
    "Makefile", "makefile", "GNUmakefile", "Dockerfile", "Cargo.lock", "LICENSE", "LICENCE",
    "README", "CHANGELOG", "AUTHORS", "NOTICE", "COPYING", "Rakefile", "Gemfile", "Procfile",
    "Brewfile", "Justfile", "justfile", "CODEOWNERS",
];

/// Why a file cannot be opened, so the explorer can say which of the two reasons it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The file holds something other than text.
    NotText,
    /// The file is text, but larger than [`SIZE_LIMIT`].
    TooLarge,
}

impl Refusal {
    /// What the explorer says when the pointer rests on the row.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::NotText => "Quill opens text files. This one is not text.",
            Refusal::TooLarge => "This file is larger than 16 MB, which is more than Quill opens.",
        }
    }
}

/// Whether Quill can open this file, and why not when it cannot.
pub fn openable(path: &Path) -> Result<(), Refusal> {
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.is_file() && metadata.len() > SIZE_LIMIT {
            return Err(Refusal::TooLarge);
        }
    }
    if is_text(path) || is_image(path) {
        Ok(())
    } else {
        Err(Refusal::NotText)
    }
}

/// True for a file Quill shows as a picture rather than as text.
///
/// Decided from the extension alone. The explorer asks this of every row, so it must not read the
/// file, and a `.png` that turns out not to be a PNG is a tab that says so — which is a better place
/// for that answer than a row that quietly refuses to open.
pub fn is_image(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some(found) if IMAGE_EXTENSIONS.contains(&found))
}

/// Whether Quill can open this file.
pub fn is_openable(path: &Path) -> bool {
    openable(path).is_ok()
}

/// Whether the file holds text, by the three rules in this module's own documentation.
pub fn is_text(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if TEXT_NAMES.contains(&name) {
            return true;
        }
        // A name that is only an extension, such as `.gitignore`, has no extension as far as the
        // standard library is concerned, so it is matched here.
        if let Some(rest) = name.strip_prefix('.') {
            if !rest.contains('.') && TEXT_EXTENSIONS.contains(&rest.to_ascii_lowercase().as_str()) {
                return true;
            }
        }
    }
    match extension(path) {
        Some(extension) if TEXT_EXTENSIONS.contains(&extension.as_str()) => true,
        Some(extension) if BINARY_EXTENSIONS.contains(&extension.as_str()) => false,
        _ => looks_like_text(path),
    }
}

/// Read the first few thousand bytes and decide. A file that cannot be read is not offered, because
/// clicking it would fail anyway.
fn looks_like_text(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buffer = vec![0_u8; SNIFF];
    let Ok(read) = file.read(&mut buffer) else {
        return false;
    };
    buffer.truncate(read);
    if buffer.contains(&0) {
        return false;
    }
    match std::str::from_utf8(&buffer) {
        Ok(_) => true,
        // The last character may have been cut in half by the read, which is not a reason to call the
        // file binary. Anything else is.
        Err(error) => error.error_len().is_none() && error.valid_up_to() + 4 >= buffer.len(),
    }
}

/// What the status bar calls this kind of file.
///
/// Markdown is named because Quill treats it differently, having a preview for it. The rest are named
/// because a reader who opened `main.rs` would rather be told it is Rust than be told it is text.
pub fn kind_name(path: Option<&Path>) -> &'static str {
    let Some(path) = path else {
        return "Plain text";
    };
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if name.eq_ignore_ascii_case("Makefile") || name.eq_ignore_ascii_case("GNUmakefile") {
            return "Makefile";
        }
        if name.eq_ignore_ascii_case("Dockerfile") {
            return "Dockerfile";
        }
    }
    if is_image(path) {
        return "Image";
    }
    match extension(path).as_deref() {
        Some("md" | "markdown" | "mdx") => "Markdown",
        Some("mmd" | "mermaid") => "Mermaid",
        Some("txt" | "text") => "Plain text",
        Some("rs") => "Rust",
        Some("toml") => "TOML",
        Some("json" | "jsonc" | "json5") => "JSON",
        Some("yaml" | "yml") => "YAML",
        Some("xml") => "XML",
        Some("html" | "htm") => "HTML",
        Some("css" | "scss" | "sass" | "less") => "CSS",
        Some("js" | "cjs" | "mjs" | "jsx") => "JavaScript",
        Some("ts" | "tsx") => "TypeScript",
        Some("py" | "pyi") => "Python",
        Some("rb") => "Ruby",
        Some("go") => "Go",
        Some("java") => "Java",
        Some("kt" | "kts") => "Kotlin",
        Some("swift") => "Swift",
        Some("c" | "h") => "C",
        Some("cc" | "cpp" | "cxx" | "hpp" | "hh") => "C++",
        Some("cs") => "C#",
        Some("php") => "PHP",
        Some("lua") => "Lua",
        Some("sql") => "SQL",
        Some("sh" | "bash" | "zsh" | "fish") => "Shell",
        Some("ps1" | "bat" | "cmd") => "Batch",
        Some("lock") => "Lock file",
        Some("log") => "Log",
        Some("csv" | "tsv") => "Table",
        Some("svg") => "SVG",
        Some("ini" | "cfg" | "conf" | "env" | "properties") => "Settings",
        _ => "Plain text",
    }
}

/// True for the files the character and paragraph formatting is worth offering for.
///
/// Quill saves plain text and carries no formatting to disk, so bold, a colour, an alignment and a
/// line spacing are all about how a document is **shown** rather than about what is in it. That is
/// worth having for prose — Markdown, a text file, a document that has not been saved anywhere yet —
/// and is noise above a source file, where what the reader wants is the code and what decides how it
/// looks is the plugin colouring it.
///
/// So the formatting controls, and the strip that used to hold them, are drawn only for the files
/// this is true of. Decided from the same table [`kind_name`] uses, so a file becomes prose or stops
/// being prose in one place.
pub fn formatting_applies(path: Option<&Path>) -> bool {
    matches!(kind_name(path), "Markdown" | "Plain text")
}

/// True when the three view mode buttons and their menu entries are worth offering.
///
/// A file whose source is neither Markdown nor Mermaid has no preview to show, so switching to one
/// would show the Markdown parser's reading of a file that was never Markdown. That leaves the two
/// kinds the preview is meant for, and a document that has not been saved anywhere yet: it has no
/// extension to go on, it is very often the beginning of a Markdown file, and it is the one Quill
/// starts with.
pub fn preview_applies(path: Option<&Path>) -> bool {
    path.is_none() || is_markdown(path) || is_mermaid(path)
}

/// True for the files the Markdown preview is meant for.
pub fn is_markdown(path: Option<&Path>) -> bool {
    matches!(
        path.and_then(extension).as_deref(),
        Some("md" | "markdown" | "mdx")
    )
}

/// True for the files that are a Mermaid diagram all the way through.
///
/// A `.mmd` file is text, so it opens and is edited like any other; what this decides is that its
/// preview is a **drawn diagram** rather than the Markdown parser's reading of it. `task-1660`
/// asks for the same three view modes a `.md` file has, and this is the one function that gives
/// them, because the buttons, the `View` menu, the keyboard and `quill-cli editor view` all ask
/// [`preview_applies`] rather than looking at the extension themselves.
pub fn is_mermaid(path: Option<&Path>) -> bool {
    matches!(path.and_then(extension).as_deref(), Some("mmd" | "mermaid"))
}

/// Which of the two kinds of preview a file has.
///
/// The three view mode buttons take this, so that a `.md` file's read `Raw Markdown` and a
/// `.mmd` file's read `Raw Mermaid`. A button that said `Markdown preview` over a Mermaid
/// diagram would be a small wrongness a reader notices immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewKind {
    #[default]
    Markdown,
    Mermaid,
}

/// What kind of preview this file has. Markdown for anything that is not plainly a diagram, which
/// includes a document that has not been saved anywhere yet.
pub fn preview_kind(path: Option<&Path>) -> PreviewKind {
    if is_mermaid(path) {
        PreviewKind::Mermaid
    } else {
        PreviewKind::Markdown
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        folder
    }

    #[test]
    fn the_file_types_quill_used_to_open_are_still_text() {
        assert!(is_text(Path::new("notes.md")));
        assert!(is_text(Path::new("notes.txt")));
        assert!(is_text(Path::new("NOTES.MD")), "the extension check ignores case");
    }

    #[test]
    fn a_source_file_quill_has_no_special_handling_for_is_text() {
        for name in ["main.rs", "app.js", "index.ts", "server.py", "Cargo.toml", "styles.css"] {
            assert!(is_text(Path::new(name)), "{name} should open as plain text");
            assert!(is_openable(Path::new(name)));
        }
    }

    #[test]
    fn a_file_that_is_not_text_is_not_offered() {
        for name in ["archive.zip", "song.mp3", "library.dylib", "book.pdf"] {
            assert!(!is_text(Path::new(name)), "{name} should not be offered");
            assert_eq!(openable(Path::new(name)), Err(Refusal::NotText));
        }
    }

    #[test]
    fn a_picture_is_offered_even_though_it_is_not_text() {
        for name in ["photo.png", "scan.JPEG", "sprite.gif", "favicon.ico", "shot.webp"] {
            let path = Path::new(name);
            assert!(!is_text(path), "{name} is not text");
            assert!(is_image(path), "{name} is a picture");
            assert_eq!(openable(path), Ok(()), "{name} opens as a picture");
            assert_eq!(kind_name(Some(path)), "Image");
        }
        // A picture Quill has no decoder for stays refused, rather than opening into an empty tab.
        assert!(!is_image(Path::new("photo.heic")));
        assert_eq!(openable(Path::new("photo.heic")), Err(Refusal::NotText));
        // `.svg` is text, and stays text: it is a file you edit rather than one you look at.
        assert!(!is_image(Path::new("logo.svg")));
        assert!(is_text(Path::new("logo.svg")));
    }

    #[test]
    fn a_name_with_no_extension_is_decided_by_reading_it() {
        let folder = folder("quill-file-kind-sniff");
        let text = folder.join("notes-without-an-extension");
        std::fs::write(&text, "a line of writing\nand another\n").expect("write the text file");
        let binary = folder.join("data-without-an-extension");
        std::fs::write(&binary, [0_u8, 1, 2, 3, 0, 255]).expect("write the binary file");

        assert!(is_text(&text), "a file of writing is text however it is named");
        assert!(!is_text(&binary), "a file holding zero bytes is not text");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_name_that_is_only_an_extension_is_text() {
        assert!(is_text(Path::new(".gitignore")));
        assert!(is_text(Path::new("Makefile")));
        assert!(is_text(Path::new("Dockerfile")));
    }

    #[test]
    fn an_unknown_extension_holding_writing_is_text() {
        let folder = folder("quill-file-kind-unknown");
        let path = folder.join("recipe.quillnotes");
        std::fs::write(&path, "flour, water, salt\n").expect("write it");
        assert!(is_text(&path));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_file_of_accented_letters_cut_in_half_by_the_sniff_is_still_text() {
        let folder = folder("quill-file-kind-utf8");
        let path = folder.join("accents.quillnotes");
        // Enough accented letters to run past the sniff, so the last one is cut in half.
        let text: String = std::iter::repeat("é").take(SNIFF).collect();
        std::fs::write(&path, text).expect("write it");
        assert!(is_text(&path), "a character split by the read is not a reason to refuse the file");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_file_too_large_to_open_says_so_rather_than_being_called_binary() {
        let folder = folder("quill-file-kind-large");
        let path = folder.join("enormous.txt");
        let file = std::fs::File::create(&path).expect("make the file");
        file.set_len(SIZE_LIMIT + 1).expect("give it a length");
        assert_eq!(openable(&path), Err(Refusal::TooLarge));
        assert!(Refusal::TooLarge.reason().contains("16 MB"));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_status_bar_names_the_kind_of_file() {
        assert_eq!(kind_name(Some(Path::new("a.md"))), "Markdown");
        assert_eq!(kind_name(Some(Path::new("a.txt"))), "Plain text");
        assert_eq!(kind_name(Some(Path::new("a.rs"))), "Rust");
        assert_eq!(kind_name(Some(Path::new("a.ts"))), "TypeScript");
        assert_eq!(kind_name(Some(Path::new("Makefile"))), "Makefile");
        assert_eq!(kind_name(Some(Path::new("a.quillnotes"))), "Plain text");
        assert_eq!(kind_name(Some(Path::new("a.png"))), "Image");
        assert_eq!(kind_name(None), "Plain text");
    }

    #[test]
    fn formatting_is_offered_for_prose_and_not_for_code() {
        for prose in ["a.md", "a.markdown", "a.txt", "a.text", "a.quillnotes"] {
            assert!(formatting_applies(Some(Path::new(prose))), "{prose} is prose");
        }
        assert!(formatting_applies(None), "a document with no path yet is prose");
        for code in ["main.rs", "Cargo.toml", "Cargo.lock", "a.json", "a.ts", "a.css", "Makefile"] {
            assert!(!formatting_applies(Some(Path::new(code))), "{code} is not prose");
        }
        // A picture is not prose either, so it gets neither the text tools nor the view modes.
        for picture in ["photo.png", "scan.jpg"] {
            assert!(!formatting_applies(Some(Path::new(picture))), "{picture} is not prose");
            assert!(!preview_applies(Some(Path::new(picture))));
        }
    }

    #[test]
    fn only_markdown_gets_the_markdown_preview() {
        assert!(is_markdown(Some(Path::new("a.md"))));
        assert!(!is_markdown(Some(Path::new("a.txt"))));
        assert!(!is_markdown(Some(Path::new("a.mmd"))));
        assert!(!is_markdown(None));
    }

    #[test]
    fn a_mermaid_file_is_text_and_has_a_preview_of_its_own() {
        for name in ["diagram.mmd", "flow.mermaid", "FLOW.MMD"] {
            let path = Path::new(name);
            assert!(is_text(path), "{name} is text and opens in the editor");
            assert!(is_mermaid(Some(path)), "{name} is a diagram");
            assert!(preview_applies(Some(path)), "{name} gets the three view modes");
            assert_eq!(preview_kind(Some(path)), PreviewKind::Mermaid);
            assert_eq!(kind_name(Some(path)), "Mermaid");
        }
    }

    #[test]
    fn a_mermaid_file_gets_no_formatting_because_it_is_not_prose() {
        // The `F` button is about how prose is shown, and none of it means anything in a diagram.
        let path = Path::new("diagram.mmd");
        assert!(!formatting_applies(Some(path)));
        assert!(preview_applies(Some(path)));
    }

    #[test]
    fn a_markdown_file_keeps_the_preview_kind_it_always_had() {
        assert_eq!(preview_kind(Some(Path::new("a.md"))), PreviewKind::Markdown);
        assert_eq!(preview_kind(None), PreviewKind::Markdown, "an unsaved document is prose");
        assert_eq!(preview_kind(Some(Path::new("a.rs"))), PreviewKind::Markdown);
    }
}
