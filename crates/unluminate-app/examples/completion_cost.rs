//! What one keystroke of auto-complete costs on a real project, measured on this machine.
//!
//! `task-1677` §7 sets one budget and one only: **gathering, scoring and sorting the candidates for
//! a stem must cost under 5 ms on the largest file in this repository**, because that work runs on
//! the window's thread at keystroke time — deliberately, since every source is already in memory and
//! a worker thread here would add generation plumbing to make an instant answer arrive a frame late.
//! A budget nobody measures is a wish, so this measures it.
//!
//! `cargo run --release -p unluminate-app --example completion_cost -- [folder] [stem]`
//!
//! It drives a **real window** — the same `UnluminateApp` the application is, with the project's index
//! built on its own thread and the largest file open in a tab — and calls the same two functions the
//! popup calls. Measuring a copy of the gathering written out again here would measure a second
//! implementation that could drift from the first.
//!
//! It is **not a test and nothing fails it**, for the reason `frame_cost` and `symbol_cost` are not
//! tests: a threshold in milliseconds is a different number on every machine. What *is* a test is
//! the work itself — how many candidates were gathered, how many survived, that the row equal to the
//! stem is not among them — and those live beside the code they are about.

use std::path::PathBuf;
use std::time::Instant;

use unluminate_app::app::UnluminateApp;
use unluminate_app::services::file_tree::FileTree;
use unluminate_app::services::plugins::Plugins;
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
    let folder = arguments
        .next()
        .map_or_else(|| std::env::current_dir().expect("a current folder"), PathBuf::from);
    let stems: Vec<String> = {
        let rest: Vec<String> = arguments.collect();
        if rest.is_empty() {
            ["d", "dr", "dra", "draw", "lyt", "pt"].iter().map(|s| (*s).to_owned()).collect()
        } else {
            rest
        }
    };

    // The largest file a language claims, which is the worst case: the most words to gather, the
    // longest `FileSymbols` read behind them.
    let tree = FileTree::new(&folder);
    let (plugins, problems) = Plugins::load(None);
    for problem in &problems {
        eprintln!("plugin: {problem}");
    }
    let grammars = plugins.grammars();
    let biggest = tree
        .all_files()
        .iter()
        .filter(|path| grammars.for_path(path).is_some())
        .filter_map(|path| Some((path.clone(), std::fs::metadata(path).ok()?.len())))
        .max_by_key(|(_, size)| *size);
    let Some((path, size)) = biggest else {
        eprintln!("No file in {} is in a language a plugin claims.", folder.display());
        return;
    };

    let mut app = UnluminateApp::new(&folder);
    app.open_path_permanently(&path);
    let start = Instant::now();
    build_the_index(&mut app);
    let index_build = start.elapsed().as_secs_f64() * 1000.0;

    let names = app.symbols_indexer().map_or(0, |indexer| indexer.index().sorted_names().len());
    let text_length = app.document().text().len_bytes();

    // The other half of a keystroke, and the half that is paid once however many questions are
    // asked at one revision: reading the file into `FileSymbols` and deriving its distinct words,
    // which the window caches on the tab keyed on `Document::text_revision()`.
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let grammar = grammars.for_path(&path).expect("a grammar").clone();
    let read = timed(10, || {
        let symbols = FileSymbols::read(&text, &grammar);
        std::hint::black_box(symbols.distinct_words(&text));
    });
    let once = FileSymbols::read(&text, &grammar);
    let distinct = once.distinct_words(&text);

    println!("Project: {}", folder.display());
    println!("  index built in {index_build:.0} ms on its own thread, {names} distinct names in it");
    println!("The largest file a language claims: {}", path.display());
    println!("  {} KB, {} bytes in the tab", size / 1024, text_length);
    println!(
        "  read into FileSymbols + distinct words: {read:.3} ms  ({} words, {} distinct spellings)",
        once.words(),
        distinct.len()
    );
    println!("  paid once a text revision, on the tab, not once a question");
    // What `task-1680` added to that read: the export marker, which is one look at the text between
    // the keyword and the token in front of it and only while a marker is armed. Measured against
    // the same file with the key taken away, because a number nobody measures is a wish.
    let bare = unluminate_core::Grammar { export_keyword: None, ..grammar.clone() };
    let without = timed(10, || {
        std::hint::black_box(FileSymbols::read(&text, &bare));
    });
    let with = timed(10, || {
        std::hint::black_box(FileSymbols::read(&text, &grammar));
    });
    println!("  of which language.export_keyword is {:.3} ms", (with - without).max(0.0));
    println!();
    println!(
        "{:<8}{:>10}{:>10}{:>12}{:>12}{:>10}",
        "stem", "gathered", "offered", "gather ms", "score ms", "total ms"
    );

    // The stem is typed at the caret, which is the point the position is asked about.
    let caret = app.document().selection().end();
    let mut worst: f64 = 0.0;
    for stem in &stems {
        // Exactly what a keystroke does: gather the pool from the four sources, then rank it.
        let gather = timed(20, || {
            std::hint::black_box(app.completion_candidates(stem, caret));
        });
        let whole = timed(20, || {
            std::hint::black_box(app.completion_rows(stem, caret));
        });
        let gathered = app.completion_candidates(stem, caret).len();
        let offered = app.completion_rows(stem, caret).len();
        let score = (whole - gather).max(0.0);
        worst = worst.max(whole);
        println!(
            "{stem:<8}{gathered:>10}{offered:>10}{gather:>12.3}{score:>12.3}{whole:>10.3}"
        );
    }

    // The import arm (`task-1680` §7). The one unbounded moment in it is a specifier with nothing
    // typed, which turns every importable file in the project into a row; the next character makes
    // `could_match` throw nearly all of them away before a candidate is built. Each sample is put
    // at the end of the open file, asked about, and undone, so what is measured is the real
    // `completion_offer` on a real tab rather than a second copy of the gathering.
    let samples: &[&str] = match grammar.imports {
        Some(unluminate_core::syntax::ImportStyle::Path) => &[
            "\nuse ",
            "\nuse unluminate_core::",
            "\nuse unluminate_core::completion::",
            "\nuse unluminate_core::completion::Can",
        ],
        Some(unluminate_core::syntax::ImportStyle::Quoted) => {
            &["\nimport { A } from './", "\nimport { A } from './la", "\nimport { La } from './"]
        }
        None => &[],
    };
    if !samples.is_empty() {
        println!();
        println!("{:<40}{:>10}{:>12}", "inside an import", "offered", "total ms");
        for sample in samples {
            let at = app.document().text().len_bytes();
            app.document_mut().apply(unluminate_core::Command::PlaceCaret { offset: at, extend: false });
            app.document_mut().apply(unluminate_core::Command::Insert((*sample).to_owned()));
            let caret = app.document().text().len_bytes();
            let whole = timed(20, || {
                std::hint::black_box(app.completion_offer(caret));
            });
            let offered = app.completion_offer(caret).rows.len();
            worst = worst.max(whole);
            let written = sample.trim_start_matches('\n');
            println!("{written:<40}{offered:>10}{whole:>12.3}");
            app.document_mut().apply(unluminate_core::Command::Undo);
        }
    }

    println!();
    println!("Worst stem measured:            {worst:8.3} ms");
    println!("Plus the read at a new revision:{read:8.3} ms");
    println!("One whole keystroke, worst case:{:8.3} ms   (budget: under 5 ms)", worst + read);
    if worst + read > 5.0 {
        println!(
            "  Over budget. §7 says the answer is capping the pool — an honest LIMIT, the \
             references modal's pattern — and never a thread."
        );
    }
}

/// Read the project and wait for the thread, which is what a frame of the real window does over
/// however many frames it takes.
fn build_the_index(app: &mut UnluminateApp) {
    app.the_project_changed_on_disk();
    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    while Instant::now() < deadline {
        app.keep_the_symbol_index_fresh();
        if app.symbols_indexer().is_some_and(|indexer| !indexer.is_building()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    eprintln!("the index did not finish building; the numbers below are without it");
}
