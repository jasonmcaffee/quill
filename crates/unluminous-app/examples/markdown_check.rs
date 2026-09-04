//! Read every Markdown file under a folder and say what the preview made of each.
//!
//! `cargo run --example markdown_check -- [folder]`
//!
//! The folder defaults to the checkout itself, which at the time of writing is a hundred and thirty
//! `.md` files running to a megabyte — the documents, the task designs, the plugin notes and the
//! command line reference. For each it prints how many preview lines came out, and how many
//! rules, tables, code blocks, pictures and diagrams are in it.
//!
//! It is the counterpart of `mermaid_check`: the quickest way to see that a change to the parser has
//! not broken anything before going to the trouble of rendering the screenshots, and the widest test
//! there is, because these files were written by hand over months and hold every shape of Markdown
//! anybody in this repository actually writes.
//!
//! **The four invariants are checked for every file**, the same four the battery in
//! `markdown/tests.rs` checks: the spans cover the text exactly, there is one paragraph style and one
//! source line a preview line, the source lines never go backwards, and everything a picture, a
//! diagram or a panel names is inside the text. A file that breaks one is reported and the example
//! ends with a non-zero status, so this can be run from a script.

use unluminous_core::markdown::{self, Options};
use unluminous_core::{CharStyle, PreviewColors};

fn main() {
    let folder = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    let mut files = Vec::new();
    collect(std::path::Path::new(&folder), &mut files);
    files.sort();

    println!(
        "{:<52} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "file", "lines", "rules", "table", "code", "pic", "diag"
    );
    let mut broken = 0;
    let mut bytes = 0;
    let started = std::time::Instant::now();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else { continue };
        bytes += source.len();
        let options =
            Options::new(CharStyle::default(), PreviewColors::default(), Some("Courier".to_owned()));
        let preview = markdown::render(&source, &options);
        let text = preview.text.to_string();
        let lines = preview.text.len_lines();
        let rules = text.lines().filter(|line| line.starts_with('\u{2500}')).count();
        let tables = preview
            .panels
            .iter()
            .filter(|panel| panel.kind == unluminous_core::PanelKind::Table)
            .count();
        let code = preview
            .panels
            .iter()
            .filter(|panel| panel.kind == unluminous_core::PanelKind::Code)
            .count();
        let name = path.to_string_lossy();
        let short = name.trim_start_matches("./").to_string();
        println!(
            "{short:<52} {lines:>6} {rules:>6} {tables:>6} {code:>6} {:>6} {:>6}",
            preview.images.len(),
            preview.diagrams.len()
        );
        for problem in problems(&preview) {
            broken += 1;
            println!("    ! {problem}");
        }
    }
    let elapsed = started.elapsed();
    println!(
        "\n{} files, {} kilobytes, {} ms, {broken} broken.",
        files.len(),
        bytes / 1024,
        elapsed.as_millis()
    );
    if broken > 0 {
        std::process::exit(1);
    }
}

/// The four things that must be true of every preview whatever the source was.
fn problems(preview: &unluminous_core::Preview) -> Vec<String> {
    let lines = preview.text.len_lines();
    let mut out = Vec::new();
    if preview.chars.total_len() != preview.text.len_bytes() {
        out.push(format!(
            "the spans cover {} bytes of {}",
            preview.chars.total_len(),
            preview.text.len_bytes()
        ));
    }
    if preview.paragraphs.len() != lines {
        out.push(format!("{} paragraph styles for {lines} lines", preview.paragraphs.len()));
    }
    if preview.source_lines.len() != lines {
        out.push(format!("{} source lines for {lines} lines", preview.source_lines.len()));
    }
    if !preview.source_lines.windows(2).all(|pair| pair[0] <= pair[1]) {
        out.push("the source lines go backwards".to_owned());
    }
    for image in &preview.images {
        if image.paragraph >= lines {
            out.push(format!("a picture at paragraph {} of {lines}", image.paragraph));
        }
    }
    for diagram in &preview.diagrams {
        if diagram.paragraph >= lines {
            out.push(format!("a diagram at paragraph {} of {lines}", diagram.paragraph));
        }
    }
    for panel in &preview.panels {
        if panel.paragraphs.end > lines || panel.paragraphs.start >= panel.paragraphs.end {
            out.push(format!("a panel over {:?} of {lines}", panel.paragraphs));
        }
    }
    out
}

/// Every `.md` and `.markdown` file under a folder, leaving out what a build wrote.
fn collect(folder: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules" | "releases") {
            continue;
        }
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().is_some_and(|end| end == "md" || end == "markdown") {
            into.push(path);
        }
    }
}
