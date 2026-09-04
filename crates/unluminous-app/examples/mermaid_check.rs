//! Lay every sample diagram out and say what came of it.
//!
//! `cargo run --example mermaid_check -- [folder]`
//!
//! The folder defaults to `sample-diagrams`. For each `.mmd` file it says which diagram type it is,
//! how large the scene came out and how many things are in it — or the reason it could not be drawn,
//! with the line number.
//!
//! This is the quickest way to see that a change to the layout has not broken anything, before going
//! to the trouble of rendering the screenshots. It measures through the fixed width stub the layout
//! tests use rather than through real fonts, so it needs no window and runs in milliseconds.

use unluminous_core::mermaid::{self, Options};
use unluminous_core::metrics::FixedMetrics;

fn main() {
    let folder = std::env::args().nth(1).unwrap_or_else(|| "sample-diagrams".to_owned());
    let metrics = FixedMetrics::default();
    let options = Options::new(&metrics);
    let mut drawn = 0;
    let mut refused = 0;

    let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(&folder)
        .unwrap_or_else(|problem| panic!("read {folder}: {problem}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mmd"))
        .collect();
    names.sort();

    for path in names {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).expect("read the sample");
        let kind = mermaid::kind(&source).map(|kind| kind.name().to_owned()).unwrap_or_default();
        match mermaid::render(&source, &options) {
            Ok(scene) => {
                drawn += 1;
                println!(
                    "{name:<18} {kind:<20} {:>5.0} x {:<5.0} {} items",
                    scene.size.width,
                    scene.size.height,
                    scene.items.len()
                );
            }
            Err(problem) => {
                refused += 1;
                println!("{name:<18} {kind:<20} REFUSED: {}", problem.message());
            }
        }
    }
    println!("\n{drawn} drawn, {refused} refused");
    if refused > 0 {
        std::process::exit(1);
    }
}
