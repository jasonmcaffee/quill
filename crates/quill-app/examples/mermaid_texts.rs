//! Print every piece of text a sample diagram draws, for checking one by hand.
//!
//! `cargo run --example mermaid_texts -- <file.mmd>`

use quill_core::mermaid::{self, Options};
use quill_core::metrics::FixedMetrics;

fn main() {
    let path = std::env::args().nth(1).expect("a file");
    let source = std::fs::read_to_string(&path).expect("read it");
    let metrics = FixedMetrics::default();
    match mermaid::render(&source, &Options::new(&metrics)) {
        Ok(scene) => {
            // Where each piece of text is as well as what it says, because a label in the wrong
            // place is the fault these are usually being used to find.
            for item in &scene.items {
                if let quill_core::mermaid::scene::Item::Text { at, text, .. } = item {
                    println!("{:>7.1} {:>7.1}  {text}", at.x, at.y);
                }
            }
        }
        Err(problem) => println!("refused: {}", problem.message()),
    }
}
