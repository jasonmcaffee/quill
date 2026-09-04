//! The rule that keeps the name of the tool Quill was measured against out of what ships.
//!
//! Quill's editing area, its tabs, its explorer menu, its debugger, its run configurations and the
//! whole of the Database plugin were each designed by reading what one particular commercial IDE
//! does and deciding, in writing, which part of it was worth having. That is a good way to build an
//! editor and a bad thing to put in front of somebody using one: `task-1795` asks that the product
//! *"should not be mentioned anywhere whatsoever in the settings, plugin, etc."*
//!
//! What each of those sentences was **for** is kept — the reason a decision was made is the entire
//! point of the comment it sits in — with the product called `the reference editor` instead. Two
//! hundred and thirty-three mentions across sixty-eight files went that way in one pass, and the
//! only reason this module exists is that a scrub nothing enforces lasts until the next person
//! writes a comment from memory.
//!
//! **What is deliberately not covered.** `tasks/*.md` and `_agent_output/` are the record of how
//! Quill was designed: they are not shipped, are not reachable from the product, and rewriting a
//! design document to say something it did not say is worse than leaving it alone. `CLAUDE.md` keeps
//! the name for the same reason and one more — it is where the next agent is told which name not to
//! reintroduce, which it cannot do without saying it.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// The workspace root: this crate is `crates/quill-app`, so it is two folders up.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root is two folders above this crate")
            .to_path_buf()
    }

    /// What ships: every crate, the command line and its reference, and the written documentation.
    const SHIPPED: &[&str] = &["crates", "quill-cli", "documentation", "README.md"];

    /// What is not read. `target` is a build, `snapshots` are accepted pictures, and
    /// `_agent_output` is scratch — see this module's own comment for why the last one is left.
    const SKIPPED: &[&str] = &["target", "node_modules", "_agent_output", "snapshots"];

    /// The spellings a search has to catch, including the one a file name would use.
    ///
    /// They are halves that are joined at the moment of use, so that this file — which is walked
    /// like every other — does not itself spell the thing it is banning and fail its own test.
    fn names() -> Vec<String> {
        [("intel", "lij"), ("jet", "brains"), ("data", "grip")]
            .iter()
            .map(|(first, rest)| format!("{first}{rest}"))
            .collect()
    }

    fn read(path: &Path, names: &[String], found: &mut Vec<String>) {
        if SKIPPED.iter().any(|skip| path.ends_with(skip)) {
            return;
        }
        if path.is_dir() {
            let Ok(entries) = std::fs::read_dir(path) else { return };
            for entry in entries.flatten() {
                read(&entry.path(), names, found);
            }
            return;
        }
        let readable = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "md" | "conf" | "toml")
        );
        if !readable {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let lowered = text.to_lowercase();
        for (number, line) in lowered.lines().enumerate() {
            if names.iter().any(|name| line.contains(name.as_str())) {
                found.push(format!("{}:{}", path.display(), number + 1));
            }
        }
    }

    /// Nothing that ships names the tool Quill was measured against.
    ///
    /// It fails with the file and the line, because the person it is talking to has just written the
    /// sentence and is about to rewrite it. `the reference editor` is what to say instead.
    #[test]
    fn no_shipped_text_names_the_other_tool() {
        let root = root();
        let names = names();
        let mut found = Vec::new();
        for shipped in SHIPPED {
            read(&root.join(shipped), &names, &mut found);
        }
        assert!(
            found.is_empty(),
            "the tool Quill was measured against is named in what ships. Say `the reference \
             editor` instead — `task-1795` and `quill_app::naming`:\n  {}",
            found.join("\n  "),
        );
    }
}
