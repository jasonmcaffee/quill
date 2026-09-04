//! What the symbol mechanism costs on a real project, measured on this machine.
//!
//! `task-1675` sets budgets — an index build under half a second cold, a hover resolve under a tenth
//! of a millisecond, a reference search that streams — and a budget nobody measures is a wish. This
//! is the `frame_cost` pattern applied to the new machinery: it reads a real folder, builds the
//! index, times a thousand hover lookups against it, and runs a reference search, printing what each
//! part cost.
//!
//! `cargo run --release -p unluminate-app --example symbol_cost -- <folder> [name]`
//!
//! It is **not a test and nothing fails it**: a threshold in milliseconds is a different number on
//! every machine, which is the same reason `frame_cost` is not one. What *is* a test is the work
//! itself — how many files the index read, how many definitions one file produced, that a hover
//! lookup performs no allocation — because those are the same everywhere, and they live beside the
//! code they are about.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use unluminate_app::services::file_tree::FileTree;
use unluminate_app::services::plugins::Plugins;
use unluminate_app::services::symbol_index::Index;
use unluminate_app::services::text_search;
use unluminate_core::symbols::FileSymbols;

/// Run `body` `runs` times and give back the mean in milliseconds.
fn timed(runs: usize, mut body: impl FnMut()) -> f64 {
    // One untimed pass first, so a cache that fills on first use is not charged to the measurement.
    body();
    let start = Instant::now();
    for _ in 0..runs {
        body();
    }
    start.elapsed().as_secs_f64() * 1000.0 / runs as f64
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let folder = arguments.next().map_or_else(
        || std::env::current_dir().expect("a current folder"),
        PathBuf::from,
    );
    let wanted = arguments.next();

    let tree = FileTree::new(&folder);
    let files = tree.all_files().to_vec();
    let (plugins, problems) = Plugins::load(None);
    for problem in &problems {
        eprintln!("plugin: {problem}");
    }
    let grammars = Arc::new(plugins.grammars());
    let readable = files.iter().filter(|path| grammars.defines_symbols(path)).count();

    println!("Project: {}", folder.display());
    println!(
        "  {} files walked, {} of them in a language that says what a definition is",
        files.len(),
        readable
    );

    // ---------------------------------------------------------------- building the index
    let start = Instant::now();
    let index = Index::build(&files, &grammars, &|| false).expect("a build");
    let build = start.elapsed().as_secs_f64() * 1000.0;
    println!("\nIndex");
    println!("  build              {build:8.1} ms  (budget: under 500 cold, on the thread)");
    println!("  files read         {:8}", index.files());
    println!("  definitions        {:8}", index.len());
    println!("  names              {:8}", index.names());
    if index.capped() {
        println!("  capped             the project holds more than the index keeps");
    }

    // The name to ask about: the one on the command line, or the commonest one in the project,
    // which is the worst case for a lookup that has to walk a list of candidates.
    let name = wanted.unwrap_or_else(|| commonest(&index));
    let candidates = index.definitions_of(&name).len();
    println!("\nAsking about '{name}' ({candidates} definitions)");

    // ---------------------------------------------------------------- one file, read once
    let biggest = files
        .iter()
        .filter(|path| grammars.defines_symbols(path))
        .filter_map(|path| Some((path, std::fs::metadata(path).ok()?.len())))
        .max_by_key(|(_, size)| *size);
    if let Some((path, size)) = biggest {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let grammar = grammars.for_path(path).expect("a grammar").clone();
        let read = timed(20, || {
            std::hint::black_box(FileSymbols::read(&text, &grammar));
        });
        let once = FileSymbols::read(&text, &grammar);
        let hover = timed(1000, || {
            std::hint::black_box(once.identifier_at(text.len() / 2));
        });
        let occurrences = timed(20, || {
            std::hint::black_box(once.occurrences(&text, &name, &grammar));
        });
        println!("\nThe largest file: {} ({} KB)", path.display(), size / 1024);
        println!("  read once          {read:8.3} ms  (per text revision, on the tab)");
        println!("  definitions        {:8}", once.definitions().len());
        println!("  words              {:8}", once.words());
        println!("  hover lookup       {hover:8.4} ms  (budget: under 0.1, once a frame)");
        println!("  occurrences        {occurrences:8.3} ms  (a person's deliberate action)");
    }

    // ---------------------------------------------------------------- the reference search
    let mut hits = 0;
    let mut read = 0;
    let start = Instant::now();
    for path in &files {
        let Some(grammar) = grammars.for_path(path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        read += 1;
        hits += text_search::references_in(path, &text, &name, grammar, 500).len();
    }
    let search = start.elapsed().as_secs_f64() * 1000.0;
    println!("\nReferences to '{name}'");
    println!("  whole search       {search:8.1} ms  (budget: streams on arrival, whatever it is)");
    println!("  files read         {read:8}");
    println!("  references         {hits:8}");

    // ---------------------------------------------------------------- what it all comes to
    println!("\nWhat a person waits for");
    println!("  opening a project  {build:8.1} ms  on the thread, with the window drawing");
    println!("  a modifier-hover   under a millisecond, cached against the text revision");
    println!("  Find References    {search:8.1} ms  streamed, so the first rows arrive sooner");
}

/// The name with the most definitions, which is the worst case a lookup has.
fn commonest(index: &Index) -> String {
    let mut best = ("new".to_owned(), 0);
    for name in candidate_names() {
        let found = index.definitions_of(name).len();
        if found > best.1 {
            best = ((*name).to_owned(), found);
        }
    }
    best.0
}

/// The names worth trying, since the index does not hand its keys out.
fn candidate_names() -> &'static [&'static str] {
    &[
        "new", "show", "draw", "path", "name", "text", "index", "state", "value", "count", "at",
        "open", "read", "write", "range", "layout", "width", "height",
    ]
}
