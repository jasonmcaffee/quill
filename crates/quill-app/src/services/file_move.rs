//! Moving a file, and taking the code that names it with it.
//!
//! `task-1681` asks that dragging a file in the explorer move it *and* fix up whatever refers to
//! it. This module is the "whatever refers to it" half: it is handed the project as it is now, a
//! list of files that are about to move, and a way of reading any file's text, and it answers with
//! a [`Plan`] — a set of byte ranges to replace, in files named by where they will be **after** the
//! move.
//!
//! It changes nothing itself. `QuillApp::move_path` moves the bytes and applies the plan, following
//! the ownership rule `task-1675` set: an open file is edited as a document and left modified, and
//! a closed file is read, checked and written once.
//!
//! ## The tier is syntactic, and it is the one Quill is already on
//!
//! `tasks/task-1681-file-operations-tdd.md` §4 is the argument. In one sentence: a language server
//! would work on a machine that happened to have one and silently do nothing on a machine that did
//! not, and silently doing nothing is the worst possible outcome when the file has already been
//! dropped somewhere else.
//!
//! So the two readings are `quill_core::imports`' own — `specifiers_in` for the quoted family and
//! `use_statements_in`/`paths_in` for the path family — which is what makes it impossible for the
//! completion popup and this to disagree about what an import is.
//!
//! ## One rule decides every case
//!
//! *Work out what the written text will mean **after** the move. If that is not what it means now,
//! rewrite it; if it is, leave it exactly as it is.*
//!
//! That one rule is what makes moving a whole folder cheap — every specifier inside it still points
//! where it did, so there is nothing to write — and it is what keeps a `super::sibling` in a moved
//! Rust module untouched while rewriting a `super::sibling` in a file that stayed behind.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use quill_core::imports::{self as reading, UseLeaf};
use quill_core::syntax::{Grammar, ImportStyle, PathRoot};

use crate::services::imports::{self, Module, Project};
use crate::services::plugins::Grammars;

/// One file's share of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdits {
    /// Where the file is now, which is where its text was read from.
    pub was: PathBuf,
    /// Where it will be when the edits are applied. The same as `was` for every file that is not
    /// itself moving, which is nearly all of them.
    pub path: PathBuf,
    /// Byte ranges into the text as it is now, and what each becomes. Sorted, never overlapping.
    pub edits: Vec<(Range<usize>, String)>,
}

/// What moving a path would change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Every file that moves: where it is, and where it is going.
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// Every file whose text changes.
    pub files: Vec<FileEdits>,
    /// What could not be done, said plainly. Shown beside what was done, never instead of it.
    pub notes: Vec<String>,
}

impl Plan {
    /// How many references would be rewritten.
    pub fn references(&self) -> usize {
        self.files.iter().map(|file| file.edits.len()).sum()
    }

    /// What the status bar says about it.
    pub fn sentence(&self) -> String {
        let references = self.references();
        let files = self.files.len();
        if references == 0 {
            return "nothing refers to it".to_owned();
        }
        format!(
            "{references} reference{} in {files} file{}",
            if references == 1 { "" } else { "s" },
            if files == 1 { "" } else { "s" },
        )
    }
}

/// Where a module really is: which source root it hangs off, and the segments below it.
///
/// Two of these are equal when they name the same module, whatever either was written as, which is
/// what lets `super::a`, `crate::b::a` and `quill_app::b::a` all be compared with one another.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Abs {
    root: PathBuf,
    segments: Vec<String>,
}

impl Abs {
    fn of(module: &Module) -> Self {
        Self { root: module.root.clone(), segments: module.segments.clone() }
    }

    fn starts_with(&self, other: &Abs) -> bool {
        self.root == other.root && self.segments.starts_with(&other.segments)
    }
}

/// Work out what moving `from` to `to` would change.
///
/// `to` is the path `from` itself ends up at, not the folder it is dropped into — the caller has
/// already decided what the file will be called, because a paste that had to avoid a name already
/// taken decides that too.
///
/// `text_of` answers with a file's text, and is where the ownership rule enters: the window hands
/// in one that prefers an open tab's live text and falls back to the disk, and a test hands in one
/// backed by a map, which is what lets every case below be tested with no window and no disk.
pub fn plan(
    project: &Project,
    grammars: &Grammars,
    from: &Path,
    to: &Path,
    text_of: &dyn Fn(&Path) -> Option<String>,
) -> Plan {
    let mut plan = Plan { moved: moved_files(project, from, to), ..Plan::default() };
    if plan.moved.is_empty() {
        return plan;
    }
    let mut edits: BTreeMap<PathBuf, Vec<(Range<usize>, String)>> = BTreeMap::new();
    let after = |path: &Path| -> PathBuf { moved_to(&plan.moved, path) };
    quoted_family(project, grammars, &plan.moved, text_of, &after, &mut edits);
    path_family(project, grammars, from, to, text_of, &after, &mut edits, &mut plan.notes);
    for (path, mut found) in edits {
        found.sort_by_key(|(range, _)| (range.start, range.end));
        found.dedup_by(|left, right| left.0 == right.0);
        plan.files.push(FileEdits { path: after(&path), was: path, edits: found });
    }
    plan
}

/// Every file the move takes with it, with where each one lands.
///
/// Derived from the project's own file list rather than from the disk, so a folder move is every
/// file the explorer would have shown inside it and nothing a build wrote.
fn moved_files(project: &Project, from: &Path, to: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut moved = Vec::new();
    for file in project.files {
        if file == from {
            moved.push((file.clone(), to.to_path_buf()));
        } else if let Ok(under) = file.strip_prefix(from) {
            moved.push((file.clone(), to.join(under)));
        }
    }
    moved
}

/// Where a path ends up. Itself, for the great majority of files, which never move.
fn moved_to(moved: &[(PathBuf, PathBuf)], path: &Path) -> PathBuf {
    moved
        .iter()
        .find(|(old, _)| old == path)
        .map(|(_, new)| new.clone())
        .unwrap_or_else(|| path.to_path_buf())
}

// --------------------------------------------------------------------------- the quoted family

/// TypeScript, JavaScript and CSS: a module is a string resolved against the file system.
fn quoted_family(
    project: &Project,
    grammars: &Grammars,
    moved: &[(PathBuf, PathBuf)],
    text_of: &dyn Fn(&Path) -> Option<String>,
    after: &dyn Fn(&Path) -> PathBuf,
    edits: &mut BTreeMap<PathBuf, Vec<(Range<usize>, String)>>,
) {
    for file in project.files {
        let Some(grammar) = grammars.for_path(file) else {
            continue;
        };
        if grammar.imports != Some(ImportStyle::Quoted) {
            continue;
        }
        let here = after(file);
        let Some(text) = text_of(file) else {
            continue;
        };
        let Some(base) = here.parent() else {
            continue;
        };
        for written in reading::specifiers_in(&text, grammar) {
            let Some(target) = imports::resolve_specifier(project, file, &written.text, grammar)
            else {
                continue;
            };
            let target_after = after(&target);
            if target_after == target && here == *file {
                continue;
            }
            let Some(wanted) =
                imports::rewrite_specifier(base, &target_after, &written.text, grammar)
            else {
                continue;
            };
            if wanted == written.text {
                continue;
            }
            edits.entry(file.clone()).or_default().push((written.range, wanted));
        }
        let _ = moved;
    }
}

// ----------------------------------------------------------------------------- the path family

/// Rust: a module is a chain of segments resolved against the module tree.
///
/// Four things follow a moved module, and this arranges all four: the `use` statements that name
/// it, the qualified paths in the bodies of functions, the `mod` declaration that made it a module
/// in the first place, and the note that has to be written when one of those cannot be done.
#[allow(clippy::too_many_arguments)]
fn path_family(
    project: &Project,
    grammars: &Grammars,
    from: &Path,
    to: &Path,
    text_of: &dyn Fn(&Path) -> Option<String>,
    after: &dyn Fn(&Path) -> PathBuf,
    edits: &mut BTreeMap<PathBuf, Vec<(Range<usize>, String)>>,
    notes: &mut Vec<String>,
) {
    let Some(grammar) = path_grammar(grammars, project, from) else {
        return;
    };
    let separator = grammar.path_separator.clone().unwrap_or_else(|| "::".to_owned());
    let Some(old) = imports::module_of(from, grammar) else {
        return;
    };
    let Some(new) = imports::module_of(to, grammar) else {
        notes.push(format!(
            "{} is not inside a source root, so nothing that names it was changed",
            to.display()
        ));
        return;
    };
    let (old_abs, new_abs) = (Abs::of(&old), Abs::of(&new));
    if old_abs == new_abs {
        return;
    }
    let packages = imports::package_roots(project, grammar);
    for file in project.files {
        if grammars.for_path(file).map(|found| found.imports) != Some(Some(ImportStyle::Path)) {
            continue;
        }
        let Some(text) = text_of(file) else {
            continue;
        };
        let before = module_or_outsider(file, grammar);
        let now = module_or_outsider(&after(file), grammar);
        let found = rewrite_one_file(
            &text, &before, &now, &old_abs, &new_abs, &packages, &separator, grammar,
        );
        if !found.is_empty() {
            edits.entry(file.clone()).or_default().extend(found);
        }
    }
    declare_the_module(project, grammars, grammar, &old, &new, after, text_of, edits, notes);
}

/// The module a file is, or a stand-in for a file that is not in the module tree at all.
///
/// An integration test, an example and a build script all live outside `language.source_roots` and
/// are not modules of the crate beside them — but they still name that crate, by its package name,
/// and those references have to follow a move as much as any other do. `quill-app`'s own screenshot
/// tests are 5 such references, and skipping the file left them naming a module that had moved.
///
/// The stand-in roots the file on itself, so `crate`, `self` and `super` written in it resolve to a
/// place nothing else is and are never rewritten. That is right rather than convenient: `crate` in
/// an integration test means the test.
fn module_or_outsider(path: &Path, grammar: &Grammar) -> Module {
    imports::module_of(path, grammar).unwrap_or_else(|| Module {
        root: path.to_path_buf(),
        package: String::new(),
        segments: Vec::new(),
    })
}

/// The grammar of the thing being moved, which for a folder is the grammar of a file inside it.
fn path_grammar<'a>(
    grammars: &'a Grammars,
    project: &Project,
    from: &Path,
) -> Option<&'a Grammar> {
    if let Some(grammar) = grammars.for_path(from) {
        return (grammar.imports == Some(ImportStyle::Path)).then_some(grammar);
    }
    project
        .files
        .iter()
        .filter(|file| file.starts_with(from))
        .find_map(|file| grammars.for_path(file))
        .filter(|grammar| grammar.imports == Some(ImportStyle::Path))
}

/// Every edit one file needs: its `use` statements first, then the chains in its body.
///
/// One rule decides both: work out what the written text will mean once the move has happened, and
/// rewrite it only when that is not what it means now. A file that moved has to be looked at even
/// when the module it names did not, because a `super::` written in it climbs from somewhere else
/// now.
#[allow(clippy::too_many_arguments)]
fn rewrite_one_file(
    text: &str,
    before: &Module,
    now: &Module,
    old_abs: &Abs,
    new_abs: &Abs,
    packages: &[(String, PathBuf)],
    separator: &str,
    grammar: &Grammar,
) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    // One reading of the tokens, asked three questions. `syntax::scan` is the whole of the grammar
    // applied to every byte, and this runs over every file in the project.
    let read = reading::tokens(text, grammar);
    let statements = reading::use_statements_in(text, grammar, &read);
    // A `use` inside an inline `mod` — `mod tests { use super::*; }`, which nearly every Rust file
    // in this repository ends with — is anchored somewhere this reading cannot see: `super` there
    // means the module the file *is*. So a relative path below the top level is left exactly as it
    // is, and only `crate::` and a package name, which mean the same thing at any depth, are
    // rewritten.
    let nesting = reading::nesting(text, &read);
    // What each name in this file means now, and what it will mean once the statements below have
    // been rewritten. Both, because a chain in the body of a function is resolved through them and
    // the question asked of it is what it *changes* to.
    let mut was: BTreeMap<String, Abs> = BTreeMap::new();
    let mut will_be: BTreeMap<String, Abs> = BTreeMap::new();
    for statement in &statements {
        let mut wanted: Vec<UseLeaf> = Vec::new();
        let mut changed = false;
        for leaf in &statement.leaves {
            if let Some(abs) = resolve(&leaf.segments, before, packages, grammar) {
                if let Some(name) = leaf_name(leaf) {
                    was.insert(name, abs);
                }
            }
            let nested = nesting.at(statement.range.start) > 0;
            let (leaf_after, moved) = match nested && anchored_relatively(&leaf.segments, grammar) {
                true => (leaf.clone(), false),
                false => settle(leaf, before, now, old_abs, new_abs, packages, grammar),
            };
            changed |= moved;
            if let Some(abs) = resolve(&leaf_after.segments, now, packages, grammar) {
                if let Some(name) = leaf_name(&leaf_after) {
                    will_be.insert(name, abs);
                }
            }
            wanted.push(leaf_after);
        }
        if changed {
            edits.push((statement.range.clone(), write_statement(statement, &wanted, separator)));
        }
    }
    for path in reading::paths_in(text, grammar, &read) {
        if statements.iter().any(|statement| statement.range.contains(&path.range.start)) {
            continue;
        }
        let words = path.words(text);
        if nesting.at(path.range.start) > 0 && anchored_relatively(&words, grammar) {
            continue;
        }
        let Some(abs) = resolve_in_scope(&words, before, &was, packages, grammar) else {
            continue;
        };
        let wanted = match abs.starts_with(old_abs) {
            true => shifted(&abs, old_abs, new_abs),
            false => abs.clone(),
        };
        if resolve_in_scope(&words, now, &will_be, packages, grammar) == Some(wanted.clone()) {
            continue;
        }
        edits.push((path.range.clone(), write_abs(&wanted, now, packages).join(separator)));
    }
    edits
}

/// Whether a written path is anchored on the module it is written in, rather than on the crate.
///
/// `self::` and `super::` are; `crate::`, a package name and a name brought into scope are not, and
/// mean the same thing however deeply nested the line is.
fn anchored_relatively(segments: &[String], grammar: &Grammar) -> bool {
    segments
        .first()
        .and_then(|first| grammar.path_root(first))
        .is_some_and(|root| matches!(root, PathRoot::Module | PathRoot::Parent))
}

/// What one `use` leaf becomes, and whether it changed at all.
fn settle(
    leaf: &UseLeaf,
    before: &Module,
    now: &Module,
    old_abs: &Abs,
    new_abs: &Abs,
    packages: &[(String, PathBuf)],
    grammar: &Grammar,
) -> (UseLeaf, bool) {
    let Some(abs) = resolve(&leaf.segments, before, packages, grammar) else {
        return (leaf.clone(), false);
    };
    // Where it will be: shifted when the module it names is the one moving, and where it already
    // is otherwise — a file that moved can break a path to something that did not.
    let wanted = match abs.starts_with(old_abs) {
        true => shifted(&abs, old_abs, new_abs),
        false => abs.clone(),
    };
    // The written text may already be right: a `super::sibling` inside a folder that moved whole
    // still names the same module, and rewriting it would be churn.
    if resolve(&leaf.segments, now, packages, grammar) == Some(wanted.clone()) {
        return (leaf.clone(), false);
    }
    let segments = write_abs(&wanted, now, packages);
    (UseLeaf { segments, alias: leaf.alias.clone(), glob: leaf.glob }, true)
}

/// The same module, said from where it has moved to.
fn shifted(abs: &Abs, old_abs: &Abs, new_abs: &Abs) -> Abs {
    let mut segments = new_abs.segments.clone();
    segments.extend_from_slice(&abs.segments[old_abs.segments.len()..]);
    Abs { root: new_abs.root.clone(), segments }
}

/// What a written chain of segments names, read from inside `here`.
fn resolve(
    segments: &[String],
    here: &Module,
    packages: &[(String, PathBuf)],
    grammar: &Grammar,
) -> Option<Abs> {
    let first = segments.first()?;
    let (mut abs, mut at) = match grammar.path_root(first) {
        Some(PathRoot::Package) => (Abs { root: here.root.clone(), segments: Vec::new() }, 1),
        Some(PathRoot::Module) => {
            (Abs { root: here.root.clone(), segments: here.segments.clone() }, 1)
        }
        Some(PathRoot::Parent) => {
            let mut segments = here.segments.clone();
            segments.pop()?;
            (Abs { root: here.root.clone(), segments }, 1)
        }
        None => {
            let root = packages.iter().find(|(name, _)| name == first).map(|(_, root)| root)?;
            (Abs { root: root.clone(), segments: Vec::new() }, 1)
        }
    };
    // `super::super::x` climbs twice, so the reserved words are consumed until one is not.
    while at < segments.len() && grammar.path_root(&segments[at]) == Some(PathRoot::Parent) {
        abs.segments.pop()?;
        at += 1;
    }
    for segment in &segments[at..] {
        abs.segments.push(segment.clone());
    }
    Some(abs)
}

/// The same, with the names this file brought into scope with its own `use` statements.
///
/// That is what makes a chain in the body of a function resolvable at all: `services::file_tree`
/// only means anything because `use crate::services;` is at the top of the file.
fn resolve_in_scope(
    segments: &[String],
    here: &Module,
    scope: &BTreeMap<String, Abs>,
    packages: &[(String, PathBuf)],
    grammar: &Grammar,
) -> Option<Abs> {
    if let Some(abs) = resolve(segments, here, packages, grammar) {
        return Some(abs);
    }
    let first = segments.first()?;
    let anchor = scope.get(first)?;
    let mut abs = anchor.clone();
    for segment in &segments[1..] {
        abs.segments.push(segment.clone());
    }
    Some(abs)
}

/// How a module is written from inside `here`: `crate::` in the same package, the package's own
/// name from outside it.
///
/// Always absolute, even where the original was written `super::`. §4.5 of the TDD is why: writing
/// a relative path correctly needs to know where the reader is as well as where the target is, and
/// gets it wrong at exactly the moments it matters.
fn write_abs(abs: &Abs, here: &Module, packages: &[(String, PathBuf)]) -> Vec<String> {
    let mut written = Vec::new();
    if abs.root == here.root {
        written.push("crate".to_owned());
    } else {
        let name = packages
            .iter()
            .find(|(_, root)| *root == abs.root)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "crate".to_owned());
        written.push(name);
    }
    written.extend(abs.segments.iter().cloned());
    written
}

/// The name a leaf brings into scope.
fn leaf_name(leaf: &UseLeaf) -> Option<String> {
    if leaf.glob {
        return None;
    }
    leaf.alias.clone().or_else(|| leaf.segments.last().cloned())
}

/// Write a `use` statement again from its leaves, grouped by the module each one is in.
///
/// One statement per parent module, in the order the leaves were first written, so a statement
/// whose leaves all shift together comes back the same shape it went in and only the ones that
/// really had to split do.
fn write_statement(
    statement: &reading::UseStatement,
    leaves: &[UseLeaf],
    separator: &str,
) -> String {
    let mut groups: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    for leaf in leaves {
        let mut parent = leaf.segments.clone();
        let last = if leaf.glob {
            "*".to_owned()
        } else {
            match parent.pop() {
                Some(name) => name,
                None => continue,
            }
        };
        let member = match &leaf.alias {
            Some(alias) => format!("{last} as {alias}"),
            None => last,
        };
        match groups.iter_mut().find(|(known, _)| *known == parent) {
            Some((_, members)) => members.push(member),
            None => groups.push((parent, vec![member])),
        }
    }
    fold_self_back_in(&mut groups);
    let keyword = "use";
    let mut lines: Vec<String> = Vec::new();
    for (parent, members) in groups {
        let head = parent.join(separator);
        let tail = match members.len() {
            1 => members[0].clone(),
            _ => format!("{{{}}}", members.join(", ")),
        };
        let path = match head.is_empty() {
            true => tail,
            false => format!("{head}{separator}{tail}"),
        };
        let visibility =
            if statement.visibility.is_empty() { String::new() } else { format!("{} ", statement.visibility) };
        lines.push(format!("{visibility}{keyword} {path};"));
    }
    lines.join(&format!("\n{}", statement.indent))
}

/// Put a group that names a module back inside the group of that module's own children.
///
/// `use crate::theme::{self, color, size};` reads as three leaves, and grouping them by the module
/// each one is in puts `theme` under `crate` and the other two under `crate::theme` — two
/// statements where the file had one. This folds the first back into the second as `self`, which is
/// how it was written, so a statement that only had to change its prefix comes back the same shape
/// it went in.
fn fold_self_back_in(groups: &mut Vec<(Vec<String>, Vec<String>)>) {
    let mut at = 0;
    while at < groups.len() {
        let (parent, members) = groups[at].clone();
        if members.len() != 1 {
            at += 1;
            continue;
        }
        let mut whole = parent.clone();
        whole.push(members[0].clone());
        match groups.iter().position(|(known, _)| *known == whole) {
            Some(found) => {
                groups[found].1.insert(0, "self".to_owned());
                groups.remove(at);
            }
            None => at += 1,
        }
    }
}

// ------------------------------------------------------------------------ the `mod` declaration

/// Take the moved module's declaration out of its old parent and put it into its new one.
///
/// This is the part that actually breaks the build if it is skipped: a Rust file is not a module
/// because of where it sits, it is a module because some other file says `mod name;`.
#[allow(clippy::too_many_arguments)]
fn declare_the_module(
    project: &Project,
    grammars: &Grammars,
    grammar: &Grammar,
    old: &Module,
    new: &Module,
    after: &dyn Fn(&Path) -> PathBuf,
    text_of: &dyn Fn(&Path) -> Option<String>,
    edits: &mut BTreeMap<PathBuf, Vec<(Range<usize>, String)>>,
    notes: &mut Vec<String>,
) {
    let _ = grammars;
    let (Some(old_name), Some(new_name)) = (old.name(), new.name()) else {
        return;
    };
    let old_parent = old.parent().and_then(|parent| imports::module_file(project, &parent, grammar));
    let new_parent = new.parent().and_then(|parent| imports::module_file(project, &parent, grammar));
    match (&old_parent, &new_parent) {
        (Some(here), Some(there)) if here == there => {
            // A rename in place: the declaration stays where it is and changes its name.
            if old_name == new_name {
                return;
            }
            let Some(text) = text_of(here) else {
                return;
            };
            match find_declaration(&text, old_name) {
                Some(found) => {
                    edits.entry(here.clone()).or_default().push((
                        found.name.clone(),
                        new_name.to_owned(),
                    ));
                }
                None => notes.push(format!(
                    "{} does not declare `mod {old_name};`, so it was left alone",
                    here.display()
                )),
            }
            return;
        }
        _ => {}
    }
    let mut declaration = format!("pub mod {new_name};");
    match &old_parent {
        Some(here) if after(here) == **here => match text_of(here) {
            Some(text) => match find_declaration(&text, old_name) {
                Some(found) => {
                    declaration = found.written(new_name);
                    edits.entry(here.clone()).or_default().push((found.whole, String::new()));
                }
                None => notes.push(format!(
                    "{} does not declare `mod {old_name};`, so nothing was taken out of it",
                    here.display()
                )),
            },
            None => {}
        },
        _ => {}
    }
    match &new_parent {
        Some(there) => {
            if let Some(text) = text_of(there) {
                let (at, line) = where_a_declaration_goes(&text, new_name, &declaration);
                edits.entry(there.clone()).or_default().push((at..at, line));
            }
        }
        None => notes.push(format!(
            "there is no module file for {}, so `mod {new_name};` was not added anywhere",
            new.segments.join("::")
        )),
    }
}

/// A `mod name;` declaration in a file: the whole of it, and just its name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Declaration {
    /// From the first of its attribute or doc comment lines to the end of its own line.
    whole: Range<usize>,
    /// Just the name, so a rename in place replaces four letters.
    name: Range<usize>,
    /// What is written in front of `mod`.
    visibility: String,
}

impl Declaration {
    fn written(&self, name: &str) -> String {
        match self.visibility.is_empty() {
            true => format!("mod {name};"),
            false => format!("{} mod {name};", self.visibility),
        }
    }
}

/// Find `mod <name>;` in a file, with whatever belongs to it above.
///
/// Line by line, because a `mod` declaration is written on a line of its own in every Rust file
/// there has ever been, and a scanner that read `mod x { }` as well would be reading a definition
/// rather than a declaration.
fn find_declaration(text: &str, name: &str) -> Option<Declaration> {
    let mut at = 0usize;
    let mut attached = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(found) = read_declaration(line, name) {
            return Some(Declaration {
                whole: attached..at + line.len(),
                name: (at + found.0)..(at + found.1),
                visibility: found.2,
            });
        }
        // An attribute or a doc comment belongs to whatever comes next, so it is carried along
        // until something that is not one is reached.
        if !(trimmed.starts_with("#[") || trimmed.starts_with("///")) {
            attached = at + line.len();
        }
        at += line.len();
    }
    None
}

/// Read one line as `[pub[(..)]] mod <name>;`, answering with where the name is and the visibility.
fn read_declaration(line: &str, name: &str) -> Option<(usize, usize, String)> {
    let body = line.trim_end();
    let mut at = body.len() - body.trim_start().len();
    let mut visibility = String::new();
    if body[at..].starts_with("pub") {
        let after = &body[at + 3..];
        let taken = match after.starts_with('(') {
            true => after.find(')')? + 1,
            false => 0,
        };
        visibility = body[at..at + 3 + taken].to_owned();
        at += 3 + taken;
        if !body[at..].starts_with(char::is_whitespace) {
            return None;
        }
        at += body[at..].len() - body[at..].trim_start().len();
    }
    if !body[at..].starts_with("mod") {
        return None;
    }
    at += 3;
    if !body[at..].starts_with(char::is_whitespace) {
        return None;
    }
    at += body[at..].len() - body[at..].trim_start().len();
    let written = body[at..].strip_suffix(';')?.trim_end();
    if written != name {
        return None;
    }
    Some((at, at + written.len(), visibility))
}

/// Where a new declaration goes in a module file, and the line to put there.
///
/// Alphabetically among the declarations already there, which is how nearly every Rust module file
/// is written. When there are none it goes under the file's heading — its `//!` comment, its inner
/// attributes and its `use` lines — because that is where the next `mod` line would have gone and
/// because putting it above them would separate a `use` from the comment that introduces it.
fn where_a_declaration_goes(text: &str, name: &str, declaration: &str) -> (usize, String) {
    let mut at = 0usize;
    let mut first_greater: Option<usize> = None;
    let mut last_end: Option<usize> = None;
    let mut heading_end = 0usize;
    let mut in_heading = true;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let heading = trimmed.starts_with("//!")
            || trimmed.starts_with("#![")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("pub use ")
            || trimmed.starts_with("extern crate");
        if in_heading && heading {
            heading_end = at + line.len();
        } else if !trimmed.is_empty() && !heading {
            in_heading = false;
        }
        if let Some(declared) = declared_name(line) {
            if first_greater.is_none() && declared.as_str() > name {
                first_greater = Some(at);
            }
            last_end = Some(at + line.len());
        }
        at += line.len();
    }
    let spot = first_greater.or(last_end).unwrap_or(heading_end);
    (spot, format!("{declaration}\n"))
}

/// The name a `mod` line declares, if the line is one.
fn declared_name(line: &str) -> Option<String> {
    let body = line.trim();
    let rest = match body.strip_prefix("pub") {
        Some(after) => {
            let taken = match after.starts_with('(') {
                true => after.find(')')? + 1,
                false => 0,
            };
            after[taken..].trim_start()
        }
        None => body,
    };
    let rest = rest.strip_prefix("mod")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim().strip_suffix(';')?.trim_end().to_owned())
}

/// Apply a file's edits to its text, back to front so the earlier ranges keep their positions.
pub fn applied(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut after = text.to_owned();
    for (range, replacement) in edits.iter().rev() {
        if range.end > after.len() {
            continue;
        }
        after.replace_range(range.clone(), replacement);
    }
    after
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A project held in memory: the file list, and each file's text.
    ///
    /// Everything below runs with no window and no disk, which is what `plan` taking a reader
    /// rather than reading for itself is for.
    struct Folder {
        root: PathBuf,
        texts: HashMap<PathBuf, String>,
    }

    impl Folder {
        fn new(pairs: &[(&str, &str)]) -> Self {
            let root = PathBuf::from("/p");
            let texts = pairs
                .iter()
                .map(|(path, text)| (root.join(path), (*text).to_owned()))
                .collect();
            Self { root, texts }
        }

        fn files(&self) -> Vec<PathBuf> {
            let mut files: Vec<PathBuf> = self.texts.keys().cloned().collect();
            files.sort();
            files
        }

        /// Move `from` to `to` and answer with every file's text afterwards.
        fn after(&self, from: &str, to: &str) -> (HashMap<String, String>, Plan) {
            let files = self.files();
            let project = Project { root: &self.root, files: &files };
            let read = |path: &Path| self.texts.get(path).cloned();
            let plan = plan(
                &project,
                &grammars(),
                &self.root.join(from),
                &self.root.join(to),
                &read,
            );
            let mut out: HashMap<String, String> = HashMap::new();
            for (path, text) in &self.texts {
                let mut here = path.clone();
                for (old, new) in &plan.moved {
                    if old == path {
                        here = new.clone();
                    }
                }
                let edits = plan
                    .files
                    .iter()
                    .find(|file| file.was == *path)
                    .map(|file| file.edits.clone())
                    .unwrap_or_default();
                let name = here
                    .strip_prefix(&self.root)
                    .expect("inside the project")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(name, applied(text, &edits));
            }
            (out, plan)
        }
    }

    fn typescript() -> Grammar {
        Grammar {
            keywords: ["import", "export", "from", "const", "require"]
                .iter()
                .map(|it| (*it).to_owned())
                .collect(),
            line_comment: Some("//".to_owned()),
            strings: vec!['"', '\''],
            escapes: true,
            imports: Some(ImportStyle::Quoted),
            import_keywords: ["import", "export", "require"]
                .iter()
                .map(|it| (*it).to_owned())
                .collect(),
            import_extensions: [".ts", ".js"].iter().map(|it| (*it).to_owned()).collect(),
            import_index: vec!["index".to_owned()],
            import_omit_extension: true,
            export_keyword: Some("export".to_owned()),
            ..Grammar::default()
        }
    }

    fn rust() -> Grammar {
        Grammar {
            keywords: ["use", "pub", "mod", "fn", "as", "let"]
                .iter()
                .map(|it| (*it).to_owned())
                .collect(),
            line_comment: Some("//".to_owned()),
            strings: vec!['"'],
            escapes: true,
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

    fn grammars() -> Grammars {
        Grammars::of(vec![
            ("ts".to_owned(), typescript()),
            ("js".to_owned(), typescript()),
            ("rs".to_owned(), rust()),
        ])
    }

    // ------------------------------------------------------------------ the quoted family

    #[test]
    fn moving_a_file_rewrites_what_imports_it_and_what_it_imports() {
        let folder = Folder::new(&[
            ("src/app/main.ts", "import { draw } from './layout';\n"),
            ("src/app/layout.ts", "import { size } from '../theme';\n"),
            ("src/theme.ts", "export const size = 1;\n"),
        ]);
        let (after, _) = folder.after("src/app/layout.ts", "src/draw/layout.ts");
        assert_eq!(after["src/app/main.ts"], "import { draw } from '../draw/layout';\n");
        assert_eq!(after["src/draw/layout.ts"], "import { size } from '../theme';\n");
    }

    #[test]
    fn a_file_moved_beside_what_it_imports_says_so_the_short_way() {
        let folder = Folder::new(&[
            ("src/app/main.ts", "import { size } from '../theme/size';\n"),
            ("src/theme/size.ts", "export const size = 1;\n"),
        ]);
        let (after, _) = folder.after("src/app/main.ts", "src/theme/main.ts");
        assert_eq!(after["src/theme/main.ts"], "import { size } from './size';\n");
    }

    #[test]
    fn a_folder_moved_whole_leaves_every_specifier_inside_it_alone() {
        let folder = Folder::new(&[
            ("src/app/main.ts", "import { draw } from './layout';\n"),
            ("src/app/layout.ts", "export function draw() {}\n"),
            ("src/index.ts", "import './app/main';\n"),
        ]);
        let (after, plan) = folder.after("src/app", "src/window/app");
        assert_eq!(
            after["src/window/app/main.ts"], "import { draw } from './layout';\n",
            "the two files moved together, so the specifier between them still points at it"
        );
        assert_eq!(after["src/index.ts"], "import './window/app/main';\n");
        assert_eq!(plan.references(), 1, "only the file outside the folder had to change");
    }

    #[test]
    fn a_specifier_written_with_its_extension_keeps_it() {
        let folder = Folder::new(&[
            ("src/a/main.js", "import './b.js';\nimport './c';\n"),
            ("src/a/b.js", "export const b = 1;\n"),
            ("src/a/c.js", "export const c = 1;\n"),
        ]);
        let (after, _) = folder.after("src/a/main.js", "src/main.js");
        assert_eq!(after["src/main.js"], "import './a/b.js';\nimport './a/c';\n");
    }

    #[test]
    fn a_folder_written_as_its_index_file_stays_written_as_the_folder() {
        let folder = Folder::new(&[
            ("src/app/main.ts", "import { w } from '../widgets';\n"),
            ("src/widgets/index.ts", "export const w = 1;\n"),
        ]);
        let (after, _) = folder.after("src/app/main.ts", "src/main.ts");
        assert_eq!(after["src/main.ts"], "import { w } from './widgets';\n");
    }

    #[test]
    fn a_specifier_that_resolves_to_nothing_is_left_exactly_as_it_is() {
        let folder = Folder::new(&[
            ("src/a/main.ts", "import react from 'react';\nimport './b';\n"),
            ("src/a/b.ts", "export const b = 1;\n"),
        ]);
        let (after, _) = folder.after("src/a/main.ts", "src/main.ts");
        assert_eq!(after["src/main.ts"], "import react from 'react';\nimport './a/b';\n");
    }

    // -------------------------------------------------------------------- the path family

    #[test]
    fn moving_a_rust_module_rewrites_the_use_lines_that_name_it() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod file_tree;\npub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            ("q/src/services/file_tree.rs", "pub struct Tree;\n"),
            ("q/src/app/mod.rs", "use crate::services::paste::free_name;\n"),
        ]);
        let (after, plan) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert_eq!(after["q/src/app/mod.rs"], "use crate::app::paste::free_name;\npub mod paste;\n");
        assert_eq!(
            after["q/src/services/mod.rs"], "pub mod file_tree;\n",
            "the declaration is taken out of the module it left"
        );
        assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    }

    #[test]
    fn a_grouped_use_with_one_moved_member_is_split_and_the_others_are_untouched() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod file_tree;\npub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            ("q/src/services/file_tree.rs", "pub struct Tree;\n"),
            ("q/src/app/mod.rs", "use crate::services::{file_tree, paste};\n"),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert_eq!(
            after["q/src/app/mod.rs"],
            "use crate::services::file_tree;\nuse crate::app::paste;\npub mod paste;\n"
        );
    }

    #[test]
    fn a_relative_path_that_still_resolves_is_left_alone_and_one_that_does_not_is_rewritten() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod services;\npub mod window;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\npub mod tree;\n"),
            ("q/src/services/paste.rs", "use super::tree::Tree;\n"),
            ("q/src/services/tree.rs", "pub struct Tree;\n"),
            ("q/src/window/mod.rs", "\n"),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/window/paste.rs");
        assert_eq!(
            after["q/src/window/paste.rs"], "use crate::services::tree::Tree;\n",
            "the file moved, so `super` no longer means what it did"
        );

        let whole = Folder::new(&[
            ("q/src/lib.rs", "pub mod services;\npub mod window;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\npub mod tree;\n"),
            ("q/src/services/paste.rs", "use super::tree::Tree;\n"),
            ("q/src/services/tree.rs", "pub struct Tree;\n"),
            ("q/src/window/mod.rs", "\n"),
        ]);
        let (after, _) = whole.after("q/src/services", "q/src/window/services");
        assert_eq!(
            after["q/src/window/services/paste.rs"], "use super::tree::Tree;\n",
            "the folder moved whole, so `super` still means its sibling"
        );
    }

    #[test]
    fn a_path_in_the_body_of_a_function_follows_the_module_it_names() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            (
                "q/src/app/mod.rs",
                "use crate::services;\n\nfn go() {\n    services::paste::free_name();\n}\n",
            ),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert!(
            after["q/src/app/mod.rs"].contains("crate::app::paste::free_name();"),
            "{}",
            after["q/src/app/mod.rs"]
        );
    }

    #[test]
    fn a_name_a_use_line_brought_into_scope_is_not_rewritten_twice() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            (
                "q/src/app/mod.rs",
                "use crate::services::paste;\n\nfn go() {\n    paste::free_name();\n}\n",
            ),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert!(
            after["q/src/app/mod.rs"].contains("use crate::app::paste;"),
            "{}",
            after["q/src/app/mod.rs"]
        );
        assert!(
            after["q/src/app/mod.rs"].contains("    paste::free_name();"),
            "the `use` line was fixed, so the call still says what it said: {}",
            after["q/src/app/mod.rs"]
        );
    }

    #[test]
    fn another_crate_naming_the_module_is_rewritten_with_the_package_spelling() {
        let folder = Folder::new(&[
            ("core/src/lib.rs", "pub mod imports;\npub mod syntax;\n"),
            ("core/src/imports.rs", "pub struct Context;\n"),
            ("core/src/syntax.rs", "pub struct Grammar;\n"),
            ("app/src/lib.rs", "use core::imports::Context;\n"),
        ]);
        let (after, _) = folder.after("core/src/imports.rs", "core/src/syntax/imports.rs");
        assert_eq!(after["app/src/lib.rs"], "use core::syntax::imports::Context;\n");
    }

    #[test]
    fn the_declaration_goes_in_alphabetically_and_keeps_its_visibility() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub(crate) mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            ("q/src/app/mod.rs", "pub mod alpha;\npub mod zulu;\n"),
            ("q/src/app/alpha.rs", "\n"),
            ("q/src/app/zulu.rs", "\n"),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert_eq!(
            after["q/src/app/mod.rs"],
            "pub mod alpha;\npub(crate) mod paste;\npub mod zulu;\n"
        );
        assert_eq!(after["q/src/services/mod.rs"], "");
    }

    #[test]
    fn an_attribute_above_a_declaration_goes_with_it() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            (
                "q/src/services/mod.rs",
                "pub mod alpha;\n#[cfg(windows)]\npub mod paste;\n",
            ),
            ("q/src/services/alpha.rs", "\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            ("q/src/app/mod.rs", "\n"),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert_eq!(after["q/src/services/mod.rs"], "pub mod alpha;\n");
        assert_eq!(after["q/src/app/mod.rs"], "pub mod paste;\n\n");
    }

    #[test]
    fn a_destination_with_no_module_file_is_a_note_rather_than_a_guess() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            ("q/src/loose/note.txt", "nothing\n"),
        ]);
        let (_, plan) = folder.after("q/src/services/paste.rs", "q/src/loose/paste.rs");
        assert!(
            plan.notes.iter().any(|note| note.contains("mod paste;")),
            "{:?}",
            plan.notes
        );
    }

    #[test]
    fn renaming_a_module_in_place_changes_the_declaration_and_the_paths() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            ("q/src/app/mod.rs", "use crate::services::paste::free_name;\n"),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/services/clipboard.rs");
        assert_eq!(after["q/src/services/mod.rs"], "pub mod clipboard;\n");
        assert_eq!(after["q/src/app/mod.rs"], "use crate::services::clipboard::free_name;\n");
    }

    #[test]
    fn a_test_beside_the_crate_names_it_by_its_package_and_follows_the_move() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;
pub mod services;
"),
            ("q/src/services/mod.rs", "pub mod paste;
"),
            ("q/src/services/paste.rs", "pub fn free_name() {}
"),
            ("q/src/app/mod.rs", "
"),
            // Outside `src`, so it is no module of the crate — and `crate::` in it means the test.
            (
                "q/tests/whole.rs",
                "fn go() {
    q::services::paste::free_name();
    let _ = crate::helper();
}
",
            ),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert!(
            after["q/tests/whole.rs"].contains("q::app::paste::free_name();"),
            "the package spelling followed it: {}",
            after["q/tests/whole.rs"]
        );
        assert!(
            after["q/tests/whole.rs"].contains("crate::helper()"),
            "and `crate` in a test means the test, so it was left alone"
        );
    }

    #[test]
    fn a_statement_naming_a_module_and_its_children_keeps_its_self() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;
pub mod theme;
"),
            ("q/src/theme/mod.rs", "pub mod color;
pub mod size;
"),
            ("q/src/theme/color.rs", "pub const A: u8 = 1;
"),
            ("q/src/theme/size.rs", "pub const B: u8 = 2;
"),
            ("q/src/app/mod.rs", "use crate::theme::{self, color, size};
"),
        ]);
        let (after, _) = folder.after("q/src/theme", "q/src/app/theme");
        assert_eq!(
            after["q/src/app/mod.rs"],
            "use crate::app::theme::{self, color, size};
pub mod theme;
",
            "one statement in, one statement out"
        );
    }

    #[test]
    fn a_use_inside_an_inline_mod_is_left_alone_because_its_anchor_cannot_be_seen() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;
pub mod services;
"),
            ("q/src/services/mod.rs", "pub mod paste;
pub mod tree;
"),
            (
                "q/src/services/paste.rs",
                "pub fn free_name() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn go() {
        free_name();
    }
}
",
            ),
            ("q/src/services/tree.rs", "pub struct Tree;
"),
            ("q/src/app/mod.rs", "
"),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert!(
            after["q/src/app/paste.rs"].contains("    use super::*;"),
            "`super` inside `mod tests` means the module the file is, which this reading cannot              see, so it is left exactly as it is: {}",
            after["q/src/app/paste.rs"]
        );
    }

    #[test]
    fn a_crate_path_inside_an_inline_mod_is_still_rewritten() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;
pub mod services;
"),
            ("q/src/services/mod.rs", "pub mod paste;
"),
            ("q/src/services/paste.rs", "pub fn free_name() {}
"),
            (
                "q/src/app/mod.rs",
                "#[cfg(test)]
mod tests {
    use crate::services::paste::free_name;
}
",
            ),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert!(
            after["q/src/app/mod.rs"].contains("use crate::app::paste::free_name;"),
            "`crate::` means the same thing however deeply nested it is: {}",
            after["q/src/app/mod.rs"]
        );
    }

    #[test]
    fn a_path_inside_a_comment_or_a_string_is_never_rewritten() {
        let folder = Folder::new(&[
            ("q/src/lib.rs", "pub mod app;\npub mod services;\n"),
            ("q/src/services/mod.rs", "pub mod paste;\n"),
            ("q/src/services/paste.rs", "pub fn free_name() {}\n"),
            (
                "q/src/app/mod.rs",
                "// crate::services::paste is where it lives\nlet s = \"crate::services::paste\";\n",
            ),
        ]);
        let (after, _) = folder.after("q/src/services/paste.rs", "q/src/app/paste.rs");
        assert!(
            after["q/src/app/mod.rs"].contains("// crate::services::paste is where it lives"),
            "{}",
            after["q/src/app/mod.rs"]
        );
        assert!(after["q/src/app/mod.rs"].contains("\"crate::services::paste\""));
    }

    #[test]
    fn moving_a_file_that_nothing_names_changes_nothing() {
        let folder = Folder::new(&[
            ("src/a/main.ts", "export const a = 1;\n"),
            ("src/b/other.ts", "export const b = 2;\n"),
        ]);
        let (_, plan) = folder.after("src/a/main.ts", "src/b/main.ts");
        assert_eq!(plan.references(), 0);
        assert_eq!(plan.sentence(), "nothing refers to it");
        assert_eq!(plan.moved.len(), 1);
    }
}
