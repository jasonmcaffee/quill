//! What a written import could become, and what a written one resolves to.
//!
//! `quill_core::imports` reads the text and says what the caret is in the middle of; this is the
//! other half, and it is the half that needs the project. Turning `'./layout'` into
//! `src/app/layout.ts` and turning `src/app/layout.ts` back into `'./layout'` are both arithmetic
//! over paths, and walking `quill_core::completion` down a folder tree is the same arithmetic with
//! the language's own rules about where a module lives applied to it.
//!
//! ## It reads the project's file list, never the disk
//!
//! Every question here is answered against [`Project::files`], which is `FileTree::all_files` — the
//! same list `Go to File` searches and `Find in Files` reads. Three things follow, and all three
//! are wanted. A specifier this offers is one that really resolves, because the file is really
//! there. Nothing outside the project can be reached, so an import cannot be completed to somebody's
//! home folder. And it costs no `stat`, which matters because the first of these runs on the
//! keystroke that opens a quote.
//!
//! It also means `node_modules` and `target` are invisible, because `task-1659` left them out of
//! the walk and measured what that was worth. So a bare package specifier is not offered;
//! `tasks/task-1680-import-completion-tdd.md` §2.4 and §12 say why, and what a cheaper version
//! would look like.
//!
//! ## The specifier is always relative, and that is a decision
//!
//! VS Code makes the shape of an inserted specifier a setting with four values, because there is no
//! single right spelling of one. A relative specifier is the one that is **always right**: it needs
//! no `tsconfig.json` read, no `baseUrl`, no alias table and no `package.json` `exports` map, and it
//! resolves on the disk exactly as it is now.

use std::path::{Component, Path, PathBuf};

use quill_core::syntax::{Grammar, PathRoot};

/// The project an import is resolved inside: where it is, and every file in it.
///
/// Borrowed rather than owned, because both of them are already held by the window and neither is
/// small. `files` is `FileTree::all_files`, which is what makes this cost no disk.
#[derive(Debug, Clone, Copy)]
pub struct Project<'a> {
    pub root: &'a Path,
    pub files: &'a [PathBuf],
}

impl Project<'_> {
    /// Whether the project holds this file.
    fn holds(&self, path: &Path) -> bool {
        self.files.iter().any(|known| known == path)
    }

    /// Whether the project holds anything inside this folder.
    ///
    /// There is no list of folders: a folder in a project is somewhere a file is, which is the same
    /// thing the explorer means by one.
    fn holds_folder(&self, folder: &Path) -> bool {
        self.files.iter().any(|known| known.starts_with(folder))
    }
}

/// Where a module path has reached: the folder it is, the file it is, or both.
///
/// Both is `app.rs` beside `app/`, which is how Rust's older module style is written. What is
/// offered there is the union — showing both rather than guessing one is the rule `task-1675` set
/// for a name defined twice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reached {
    pub folder: Option<PathBuf>,
    pub file: Option<PathBuf>,
}

impl Reached {
    fn is_nothing(&self) -> bool {
        self.folder.is_none() && self.file.is_none()
    }
}

/// The file a written specifier resolves to, or nothing.
///
/// The extensions are tried in the order the manifest lists them, so `.ts` beats `.js` because
/// TypeScript's manifest says `.ts` first, and a folder is then tried against each of
/// `language.import_index`. A specifier that is not relative resolves to nothing: a bare one names a
/// package, and one with a scheme in it would be a fetch, which nothing in Quill ever does.
pub fn resolve_specifier(
    project: &Project,
    from: &Path,
    written: &str,
    grammar: &Grammar,
) -> Option<PathBuf> {
    let written = written.trim();
    if !written.starts_with('.') || written.contains("://") {
        return None;
    }
    let mut joined = from.parent()?.to_path_buf();
    for part in written.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                joined.pop();
            }
            other => joined.push(other),
        }
    }
    // Written out with its extension, which is what a stylesheet does.
    if project.holds(&joined) {
        return Some(joined);
    }
    if let Some(found) = with_each_extension(&joined, grammar).find(|path| project.holds(path)) {
        return Some(found);
    }
    module_file_in(project, &joined, grammar)
}

/// Every specifier that would reach a file of this language from `from`, with the file it reaches.
///
/// The one moment in this ticket that is unbounded, and it happens once per import statement: with
/// nothing typed there is nothing to filter by, so the whole project is turned into rows. The very
/// next character makes `completion::could_match` throw nearly all of them away before a candidate
/// is built for any of them.
pub fn specifiers(project: &Project, from: &Path, grammar: &Grammar) -> Vec<(String, PathBuf)> {
    let Some(base) = from.parent() else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = Vec::with_capacity(project.files.len());
    for file in project.files {
        if file == from || extension_of(file, grammar).is_none() {
            continue;
        }
        let Some(written) = specifier_for(base, file, grammar) else {
            continue;
        };
        found.push((written, file.clone()));
    }
    found
}

/// How one file is written as a specifier from `base`, or nothing when it cannot be.
fn specifier_for(base: &Path, file: &Path, grammar: &Grammar) -> Option<String> {
    let extension = extension_of(file, grammar)?;
    if !grammar.import_omit_extension {
        return relative(base, file);
    }
    let name = file.file_name()?.to_str()?;
    let stem = name.strip_suffix(extension.as_str())?;
    // A folder's own module file is written as the folder: `widgets/index.ts` is `./widgets`.
    match grammar.import_index.iter().any(|index| index == stem) {
        true => relative(base, file.parent()?),
        false => relative(base, &file.with_file_name(stem)),
    }
}

/// Where a module path has reached, or nothing when it resolves to no part of the project.
///
/// There is no partial credit. Offering the whole project's exported names because the first
/// segment was a typo would be a list that is never right.
pub fn resolve_segments(
    project: &Project,
    from: &Path,
    segments: &[String],
    grammar: &Grammar,
) -> Option<Reached> {
    let (mut reached, mut at) = first_segment(project, from, segments, grammar)?;
    while at < segments.len() {
        let base = reached.folder.take()?;
        let segment = &segments[at];
        let as_folder = base.join(segment);
        let as_file = with_each_extension(&base.join(segment), grammar)
            .find(|candidate| project.holds(candidate));
        reached.folder = project.holds_folder(&as_folder).then_some(as_folder);
        reached.file = match as_file {
            Some(file) => Some(file),
            None => reached.folder.as_deref().and_then(|f| module_file_in(project, f, grammar)),
        };
        if reached.is_nothing() {
            return None;
        }
        at += 1;
    }
    Some(reached)
}

/// The first segment, which is the only one that can be a reserved word or a package.
///
/// Answers with what it reached and how many segments it used, because `super::super::x` is one
/// step that eats two of them.
fn first_segment(
    project: &Project,
    from: &Path,
    segments: &[String],
    grammar: &Grammar,
) -> Option<(Reached, usize)> {
    let first = segments.first()?;
    match grammar.path_root(first) {
        Some(PathRoot::Package) => {
            let folder = source_root_of(project, from, grammar)?;
            Some((in_folder(project, folder, grammar), 1))
        }
        Some(PathRoot::Module) => {
            let folder = from.file_stem().map(|stem| from.with_file_name(stem));
            let folder = folder.filter(|folder| project.holds_folder(folder));
            let folder = match is_a_module_file(from, grammar) {
                true => from.parent().map(Path::to_path_buf),
                false => folder,
            };
            Some((Reached { folder, file: Some(from.to_path_buf()) }, 1))
        }
        Some(PathRoot::Parent) => {
            let mut folder = parent_module_of(from, grammar)?;
            let mut at = 1;
            while segments.get(at).and_then(|word| grammar.path_root(word))
                == Some(PathRoot::Parent)
            {
                folder = folder.parent()?.to_path_buf();
                at += 1;
            }
            Some((in_folder(project, folder, grammar), at))
        }
        // Not a reserved word, so it names another package: a folder holding a source root, spelt
        // the way a module is — its folder name with `-` read as `_`.
        None => {
            let folder = package_named(project, first, grammar)?;
            Some((in_folder(project, folder, grammar), 1))
        }
    }
}

/// A folder, with whatever module file it holds.
fn in_folder(project: &Project, folder: PathBuf, grammar: &Grammar) -> Reached {
    let file = module_file_in(project, &folder, grammar);
    Reached { folder: Some(folder), file }
}

/// The child modules of a folder: its subfolders, and its files of this language, minus the file
/// that is the folder's own module.
pub fn children(project: &Project, folder: &Path, grammar: &Grammar) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for file in project.files {
        let Ok(under) = file.strip_prefix(folder) else {
            continue;
        };
        let mut parts = under.components();
        let Some(Component::Normal(first)) = parts.next() else {
            continue;
        };
        let Some(name) = first.to_str() else {
            continue;
        };
        // A folder: the file is deeper than one level under this one.
        if parts.next().is_some() {
            let child = folder.join(name);
            if !found.iter().any(|(known, _)| known == name) {
                found.push((name.to_owned(), child));
            }
            continue;
        }
        let Some(extension) = extension_of(file, grammar) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(extension.as_str()) else {
            continue;
        };
        if grammar.import_index.iter().any(|index| index == stem) {
            continue;
        }
        if !found.iter().any(|(known, _)| known == stem) {
            found.push((stem.to_owned(), file.clone()));
        }
    }
    found
}

/// What is offered when nothing has been written yet: the language's reserved roots, and every
/// package in the project.
pub fn roots(project: &Project, grammar: &Grammar) -> Vec<(String, Option<PathBuf>)> {
    let mut found: Vec<(String, Option<PathBuf>)> =
        grammar.path_roots.iter().map(|(word, _)| (word.clone(), None)).collect();
    for (name, folder) in packages(project, grammar) {
        if !found.iter().any(|(known, _)| *known == name) {
            found.push((name, Some(folder)));
        }
    }
    found
}

/// Every package in the project: a folder holding a source root, named the way a module is.
fn packages(project: &Project, grammar: &Grammar) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for file in project.files {
        for source_root in source_roots_above(file, grammar) {
            let Some(folder) = source_root.parent() else {
                continue;
            };
            let Some(name) = folder.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let spelt = name.replace('-', "_");
            if !found.iter().any(|(known, _)| *known == spelt) {
                found.push((spelt, source_root));
            }
        }
    }
    found
}

/// The source root of the package holding `from` — the nearest ancestor named by
/// `language.source_roots`, or the project's own root when the language named none.
fn source_root_of(project: &Project, from: &Path, grammar: &Grammar) -> Option<PathBuf> {
    if grammar.source_roots.is_empty() {
        return Some(project.root.to_path_buf());
    }
    source_roots_above(from, grammar).into_iter().next()
}

/// Every ancestor of `path` named by `language.source_roots`, nearest first.
fn source_roots_above(path: &Path, grammar: &Grammar) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folder = path.parent();
    while let Some(here) = folder {
        if here
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| grammar.source_roots.iter().any(|root| root == name))
        {
            found.push(here.to_path_buf());
        }
        folder = here.parent();
    }
    found
}

/// The source root of the package spelt `name`.
fn package_named(project: &Project, name: &str, grammar: &Grammar) -> Option<PathBuf> {
    packages(project, grammar).into_iter().find(|(spelt, _)| spelt == name).map(|(_, root)| root)
}

/// The module above the one `from` is.
///
/// A file that **is** its folder's module — `mod.rs`, `lib.rs` — has the folder above its own as its
/// parent; every other file has its own folder.
fn parent_module_of(from: &Path, grammar: &Grammar) -> Option<PathBuf> {
    let folder = from.parent()?;
    match is_a_module_file(from, grammar) {
        true => folder.parent().map(Path::to_path_buf),
        false => Some(folder.to_path_buf()),
    }
}

/// Whether this file is the module file of the folder it is in.
fn is_a_module_file(file: &Path, grammar: &Grammar) -> bool {
    file.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| grammar.import_index.iter().any(|index| index == stem))
}

/// The file that is a folder's own module, if it has one.
fn module_file_in(project: &Project, folder: &Path, grammar: &Grammar) -> Option<PathBuf> {
    for index in &grammar.import_index {
        for extension in &grammar.import_extensions {
            let candidate = folder.join(format!("{index}{extension}"));
            if project.holds(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// The same path with each of the language's extensions appended, in the manifest's order.
fn with_each_extension<'a>(
    path: &'a Path,
    grammar: &'a Grammar,
) -> impl Iterator<Item = PathBuf> + 'a {
    grammar.import_extensions.iter().filter_map(move |extension| {
        let name = path.file_name()?.to_str()?;
        Some(path.with_file_name(format!("{name}{extension}")))
    })
}

/// Which of the language's extensions this file has, if any.
///
/// The longest match, so a language naming both `.ts` and `.d.ts` reads `layout.d.ts` as the second.
fn extension_of(path: &Path, grammar: &Grammar) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_lowercase();
    grammar
        .import_extensions
        .iter()
        .filter(|extension| name.ends_with(&extension.to_lowercase()) && name.len() > extension.len())
        .max_by_key(|extension| extension.len())
        .cloned()
}

/// How `target` is written as a relative path from the folder `base`, with `/` separators.
///
/// Always beginning `./` or `../`, which is what makes it a specifier rather than a package name.
/// Nothing when the two are the same place, because an import of oneself is not a thing anybody
/// means.
fn relative(base: &Path, target: &Path) -> Option<String> {
    let here: Vec<&std::ffi::OsStr> = parts(base);
    let there: Vec<&std::ffi::OsStr> = parts(target);
    let same = here.iter().zip(&there).take_while(|(left, right)| left == right).count();
    let up = here.len() - same;
    let down: Vec<&str> = there[same..].iter().filter_map(|part| part.to_str()).collect();
    if down.len() != there.len() - same {
        return None;
    }
    if up == 0 && down.is_empty() {
        return None;
    }
    let mut written = String::new();
    for _ in 0..up {
        written.push_str("../");
    }
    if up == 0 {
        written.push_str("./");
    }
    written.push_str(&down.join("/"));
    // `..` rather than `../`, which is what a folder above is written as.
    Some(written.trim_end_matches('/').to_owned())
}

/// A path's ordinary components, which is everything a relative path can be built out of.
fn parts(path: &Path) -> Vec<&std::ffi::OsStr> {
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part,
            other => other.as_os_str(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_core::syntax::ImportStyle;

    fn typescript() -> Grammar {
        Grammar {
            imports: Some(ImportStyle::Quoted),
            import_keywords: vec!["import".to_owned()],
            import_extensions: [".ts", ".tsx", ".js"].iter().map(|it| (*it).to_owned()).collect(),
            import_index: vec!["index".to_owned()],
            import_omit_extension: true,
            export_keyword: Some("export".to_owned()),
            ..Grammar::default()
        }
    }

    fn css() -> Grammar {
        Grammar {
            imports: Some(ImportStyle::Quoted),
            import_keywords: vec!["@import".to_owned()],
            import_extensions: vec![".css".to_owned()],
            import_omit_extension: false,
            ..Grammar::default()
        }
    }

    fn rust() -> Grammar {
        Grammar {
            imports: Some(ImportStyle::Path),
            import_keywords: vec!["use".to_owned()],
            import_extensions: vec![".rs".to_owned()],
            import_index: ["mod", "lib", "main"].iter().map(|it| (*it).to_owned()).collect(),
            export_keyword: Some("pub".to_owned()),
            path_separator: Some("::".to_owned()),
            source_roots: vec!["src".to_owned()],
            path_roots: vec![
                ("crate".to_owned(), PathRoot::Package),
                ("self".to_owned(), PathRoot::Module),
                ("super".to_owned(), PathRoot::Parent),
            ],
            ..Grammar::default()
        }
    }

    /// A little TypeScript project, as a root and a list of files. Nothing is written to disk:
    /// every question this module answers is answered against the list.
    fn web() -> (PathBuf, Vec<PathBuf>) {
        let root = PathBuf::from("/project");
        let files = [
            "src/app/mod.ts",
            "src/app/layout.ts",
            "src/app/layout.js",
            "src/app/widgets/index.ts",
            "src/app/widgets/button.tsx",
            "src/core/completion.ts",
            "src/notes.md",
            "theme.css",
        ]
        .iter()
        .map(|name| root.join(name))
        .collect();
        (root, files)
    }

    /// A little Rust workspace, shaped like Quill's own.
    fn workspace() -> (PathBuf, Vec<PathBuf>) {
        let root = PathBuf::from("/quill");
        let files = [
            "crates/quill-core/src/lib.rs",
            "crates/quill-core/src/completion.rs",
            "crates/quill-core/src/mermaid/mod.rs",
            "crates/quill-core/src/mermaid/layered.rs",
            "crates/quill-app/src/lib.rs",
            "crates/quill-app/src/app/mod.rs",
            "crates/quill-app/src/app/actions.rs",
            "crates/quill-app/src/components/editor_view.rs",
        ]
        .iter()
        .map(|name| root.join(name))
        .collect();
        (root, files)
    }

    fn project<'a>(root: &'a Path, files: &'a [PathBuf]) -> Project<'a> {
        Project { root, files }
    }

    #[test]
    fn a_written_specifier_resolves_through_the_extensions_in_the_manifests_order() {
        // Scenarios 28, 29, 30 and 31.
        let (root, files) = web();
        let project = project(&root, &files);
        let from = root.join("src/app/mod.ts");
        let resolve = |written: &str| resolve_specifier(&project, &from, written, &typescript());
        assert_eq!(resolve("./layout"), Some(root.join("src/app/layout.ts")), ".ts beats .js");
        assert_eq!(resolve("./widgets"), Some(root.join("src/app/widgets/index.ts")));
        assert_eq!(resolve("../core/completion"), Some(root.join("src/core/completion.ts")));
        assert_eq!(resolve("./widgets/button"), Some(root.join("src/app/widgets/button.tsx")));
    }

    #[test]
    fn a_specifier_that_is_not_relative_resolves_to_nothing() {
        // Scenarios 32 and 33: a bare one names a package, and one with a scheme would be a fetch.
        let (root, files) = web();
        let project = project(&root, &files);
        let from = root.join("src/app/mod.ts");
        assert_eq!(resolve_specifier(&project, &from, "react", &typescript()), None);
        assert_eq!(
            resolve_specifier(&project, &from, "https://example.com/a.js", &typescript()),
            None
        );
        assert_eq!(resolve_specifier(&project, &from, "./nowhere", &typescript()), None);
    }

    #[test]
    fn a_stylesheet_resolves_with_its_extension_written_out() {
        // Scenario 34.
        let (root, files) = web();
        let project = project(&root, &files);
        let from = root.join("src/app/mod.ts");
        assert_eq!(
            resolve_specifier(&project, &from, "../../theme.css", &css()),
            Some(root.join("theme.css"))
        );
    }

    #[test]
    fn every_importable_file_is_offered_as_the_specifier_that_would_reach_it() {
        // Scenarios 41, 42 and 43.
        let (root, files) = web();
        let project = project(&root, &files);
        let from = root.join("src/app/mod.ts");
        let written: Vec<String> =
            specifiers(&project, &from, &typescript()).into_iter().map(|(it, _)| it).collect();
        assert!(written.contains(&"./layout".to_owned()), "{written:?}");
        assert!(!written.contains(&"./layout.ts".to_owned()), "the extension is dropped");
        assert!(written.contains(&"./widgets".to_owned()), "a folder's index is the folder");
        assert!(!written.contains(&"./widgets/index".to_owned()), "{written:?}");
        assert!(written.contains(&"./widgets/button".to_owned()), "{written:?}");
        assert!(written.contains(&"../core/completion".to_owned()), "{written:?}");
        assert!(!written.iter().any(|it| it.contains("notes")), "a note is not TypeScript");
        assert!(!written.iter().any(|it| it.contains("mod")), "and never the file being edited");
    }

    #[test]
    fn a_stylesheet_is_offered_with_its_extension() {
        let (root, files) = web();
        let project = project(&root, &files);
        let from = root.join("src/app/site.css");
        let written: Vec<String> =
            specifiers(&project, &from, &css()).into_iter().map(|(it, _)| it).collect();
        assert_eq!(written, vec!["../../theme.css".to_owned()]);
    }

    #[test]
    fn a_module_path_walks_the_source_root_of_the_package_it_names() {
        // Scenarios 35 and 36.
        let (root, files) = workspace();
        let project = project(&root, &files);
        let from = root.join("crates/quill-app/src/components/editor_view.rs");
        let reach = |path: &[&str]| {
            let segments: Vec<String> = path.iter().map(|it| (*it).to_owned()).collect();
            resolve_segments(&project, &from, &segments, &rust())
        };
        assert_eq!(
            reach(&["crate", "app", "actions"]),
            Some(Reached {
                folder: None,
                file: Some(root.join("crates/quill-app/src/app/actions.rs")),
            })
        );
        assert_eq!(
            reach(&["quill_core", "completion"]),
            Some(Reached {
                folder: None,
                file: Some(root.join("crates/quill-core/src/completion.rs")),
            })
        );
        // A package folder spelt with a hyphen is a module spelt with an underscore, and nothing
        // else in the walk normalises: a module below the source root is already an identifier.
        assert_eq!(reach(&["quill-core", "completion"]), None);
    }

    #[test]
    fn a_folder_with_a_module_file_answers_with_both() {
        let (root, files) = workspace();
        let project = project(&root, &files);
        let from = root.join("crates/quill-core/src/lib.rs");
        let segments = ["crate".to_owned(), "mermaid".to_owned()];
        assert_eq!(
            resolve_segments(&project, &from, &segments, &rust()),
            Some(Reached {
                folder: Some(root.join("crates/quill-core/src/mermaid")),
                file: Some(root.join("crates/quill-core/src/mermaid/mod.rs")),
            })
        );
    }

    #[test]
    fn self_and_super_are_read_from_where_the_file_is() {
        // Scenarios 37 and 38.
        let (root, files) = workspace();
        let project = project(&root, &files);
        let grammar = rust();
        // From a file that is not its folder's module, `super` is its own folder.
        let from = root.join("crates/quill-app/src/app/actions.rs");
        let up = ["super".to_owned()];
        assert_eq!(
            resolve_segments(&project, &from, &up, &grammar).and_then(|it| it.folder),
            Some(root.join("crates/quill-app/src/app"))
        );
        // From a file that **is** its folder's module, it is the folder above.
        let from = root.join("crates/quill-app/src/app/mod.rs");
        assert_eq!(
            resolve_segments(&project, &from, &up, &grammar).and_then(|it| it.folder),
            Some(root.join("crates/quill-app/src"))
        );
        let twice = ["super".to_owned(), "super".to_owned()];
        assert_eq!(
            resolve_segments(&project, &from, &twice, &grammar).and_then(|it| it.folder),
            Some(root.join("crates/quill-app"))
        );
        let here = ["self".to_owned()];
        assert_eq!(
            resolve_segments(&project, &from, &here, &grammar).and_then(|it| it.file),
            Some(from.clone())
        );
    }

    #[test]
    fn an_unresolved_segment_answers_nothing_rather_than_everything() {
        // Scenario 40.
        let (root, files) = workspace();
        let project = project(&root, &files);
        let from = root.join("crates/quill-app/src/app/mod.rs");
        let typo = ["quil_core".to_owned(), "completion".to_owned()];
        assert_eq!(resolve_segments(&project, &from, &typo, &rust()), None);
        let deep = ["crate".to_owned(), "app".to_owned(), "nowhere".to_owned()];
        assert_eq!(resolve_segments(&project, &from, &deep, &rust()), None);
    }

    #[test]
    fn a_folders_children_are_its_subfolders_and_its_files_but_not_its_own_module() {
        let (root, files) = workspace();
        let project = project(&root, &files);
        let names: Vec<String> = children(&project, &root.join("crates/quill-core/src"), &rust())
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, vec!["completion".to_owned(), "mermaid".to_owned()]);
    }

    #[test]
    fn nothing_written_offers_the_reserved_roots_and_the_packages() {
        let (root, files) = workspace();
        let project = project(&root, &files);
        let names: Vec<String> = roots(&project, &rust()).into_iter().map(|(name, _)| name).collect();
        assert_eq!(
            names,
            vec![
                "crate".to_owned(),
                "self".to_owned(),
                "super".to_owned(),
                "quill_core".to_owned(),
                "quill_app".to_owned(),
            ]
        );
    }

    #[test]
    fn a_relative_path_is_written_with_forward_slashes_and_never_reaches_outside_the_project() {
        let base = PathBuf::from("/project/src/app");
        assert_eq!(relative(&base, Path::new("/project/src/app/layout")).as_deref(), Some("./layout"));
        assert_eq!(relative(&base, Path::new("/project/src/core/x")).as_deref(), Some("../core/x"));
        assert_eq!(relative(&base, Path::new("/project/src")).as_deref(), Some(".."));
        assert_eq!(relative(&base, &base), None, "an import of oneself is not a thing");
    }
}
