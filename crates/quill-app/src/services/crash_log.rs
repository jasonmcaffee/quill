//! Writing down what Quill was doing when it stopped.
//!
//! A panic in a graphical application on macOS leaves nothing behind. The message goes to standard
//! error, and an application launched from the Finder has no standard error — launchd sends it
//! nowhere — so the window disappears and there is not a line anywhere saying why. It does not even
//! leave a crash report: the panic unwinds out of the event loop and the process *exits*, and macOS
//! only files a report for a process that aborts or is killed by a signal.
//!
//! That is how a real report arrived — a crash while typing in a JavaScript file — with no message,
//! no location and nothing in `~/Library/Logs/DiagnosticReports`, and it could not be reproduced from
//! the description. A fault nobody can see is a fault nobody can fix, so this makes the next one
//! visible: every panic, on any thread, is appended to `crash.log` in the folder Quill keeps its
//! settings in, with the version, the time, where in the code it happened and a backtrace.
//!
//! It changes nothing else. The message still goes to standard error for anybody running Quill from a
//! terminal, and the panic still does what it did before.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The file panics are appended to, inside the folder given to [`install`].
pub const FILE: &str = "crash.log";
/// The file the log is moved to when it grows past [`LIMIT`], so one crash a day for a year cannot
/// fill a disk and the most recent ones are always the ones kept.
pub const OLD_FILE: &str = "crash.log.old";
/// How large the log may get before it is rotated.
pub const LIMIT: u64 = 256 * 1024;

/// Start writing panics to `folder`.
///
/// Chains to whatever hook was there before, so the message still reaches standard error exactly as
/// it did. Called once, from `main`, before the window is built — a panic while the window is being
/// built is one of the ones worth catching.
pub fn install(folder: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        // Written first, because formatting a backtrace is the slow part and a second panic inside
        // this hook would leave nothing at all.
        let report = report(panic, &std::backtrace::Backtrace::force_capture().to_string());
        if let Err(problem) = append(&folder, &report) {
            eprintln!("Quill could not write {}: {problem}", folder.join(FILE).display());
        } else {
            eprintln!("Quill wrote what happened to {}", folder.join(FILE).display());
        }
        previous(panic);
    }));
}

/// What one panic is written as.
///
/// Split from the hook so that a test can read it without panicking: the thread's name is in it
/// because a panic on the symbol indexer's thread and a panic in the window are different faults,
/// and the version is in it because a report about a version nobody is running any more is the
/// commonest kind of report there is.
fn report(panic: &std::panic::PanicHookInfo<'_>, backtrace: &str) -> String {
    let thread = std::thread::current();
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("Quill {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("at   {}\n", now()));
    out.push_str(&format!("on   the {} thread\n", thread.name().unwrap_or("unnamed")));
    match panic.location() {
        Some(location) => out.push_str(&format!(
            "in   {}:{}:{}\n",
            location.file(),
            location.line(),
            location.column()
        )),
        None => out.push_str("in   somewhere with no location\n"),
    }
    out.push_str(&format!("said {}\n", message(panic)));
    out.push_str("\n");
    out.push_str(backtrace);
    if !backtrace.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// What the panic said, whichever of the two shapes a payload takes.
fn message(panic: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = panic.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "a panic with nothing to say".to_owned()
}

/// The time, as far as the standard library alone can tell it: seconds since the epoch.
///
/// A civil date would need a calendar, and `quill-core` has one for git dates but this crate would
/// have to reach for it for one line in a file a person reads next to `date -r <seconds>`. The
/// seconds are unambiguous, which is what a log wants.
fn now() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => format!("{} seconds past the epoch", since.as_secs()),
        Err(_) => "a time before the epoch".to_owned(),
    }
}

/// Add one report to the log, rotating it first if it has grown too large.
fn append(folder: &Path, report: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(FILE);
    if std::fs::metadata(&path).map(|about| about.len() > LIMIT).unwrap_or(false) {
        // A rename rather than a truncation, so the crash that filled it is still readable.
        let _ = std::fs::rename(&path, folder.join(OLD_FILE));
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(report.as_bytes())?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        std::fs::remove_dir_all(&folder).ok();
        folder
    }

    /// The hook is a process-wide thing, so this is one test rather than several: two tests setting a
    /// hook at the same time would each see the other's panics.
    #[test]
    fn a_panic_is_written_down_with_where_and_what_and_a_backtrace() {
        let folder = temporary("quill-crash-log");
        install(folder.clone());

        let outcome = std::panic::catch_unwind(|| {
            panic!("the fault this exists for");
        });
        assert!(outcome.is_err(), "the panic still happens; the hook only writes it down");

        let written = std::fs::read_to_string(folder.join(FILE)).expect("the log was written");
        assert!(written.contains("the fault this exists for"), "what it said: {written}");
        assert!(written.contains(env!("CARGO_PKG_VERSION")), "which version it was");
        assert!(written.contains("crash_log.rs:"), "where in the code, so it can be found");
        assert!(written.contains("seconds past the epoch"), "when");
        assert!(
            written.contains("quill_app") || written.contains("backtrace"),
            "and a backtrace: {written}"
        );

        // A second panic is appended rather than replacing the first, because the interesting one is
        // often not the last one.
        let _ = std::panic::catch_unwind(|| panic!("the second one"));
        let written = std::fs::read_to_string(folder.join(FILE)).expect("still there");
        assert!(written.contains("the fault this exists for"));
        assert!(written.contains("the second one"));
        // Counted by message rather than by how many blocks the file holds. The hook is process
        // wide, so a *different* test failing writes a block here too, and counting blocks made this
        // test fail whenever anything else in the crate did — which is a test that reports somebody
        // else's fault as its own.
        assert_eq!(written.matches("said the fault this exists for\n").count(), 1);
        assert_eq!(written.matches("said the second one\n").count(), 1);

        // A panic on another thread is caught too, which is where the indexer, the git worker and the
        // terminal's reader all live.
        let elsewhere = std::thread::Builder::new()
            .name("a-worker".to_owned())
            .spawn(|| panic!("from a thread of its own"))
            .expect("start the thread");
        assert!(elsewhere.join().is_err());
        let written = std::fs::read_to_string(folder.join(FILE)).expect("still there");
        assert!(written.contains("from a thread of its own"));
        assert!(written.contains("on   the a-worker thread"), "which thread it was: {written}");

        let _ = std::panic::take_hook();
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_log_that_has_grown_too_large_is_rotated_rather_than_growing_for_ever() {
        let folder = temporary("quill-crash-log-rotate");
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::fs::write(folder.join(FILE), vec![b'x'; LIMIT as usize + 1]).expect("a full log");

        append(&folder, "a new report\n").expect("append");

        let now = std::fs::read_to_string(folder.join(FILE)).expect("the new log");
        assert_eq!(now, "a new report\n", "the new one starts again");
        let old = std::fs::metadata(folder.join(OLD_FILE)).expect("the old one was kept");
        assert!(old.len() > LIMIT, "and it still holds what filled it");
        std::fs::remove_dir_all(&folder).ok();
    }
}
