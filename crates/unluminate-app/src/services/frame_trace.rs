//! Where the milliseconds of a frame went, written down when something asks.
//!
//! `task-1805` began with a measurement from outside the process: an idle window, with nobody
//! touching it, was using **77 ms of processor time a second** — a thirteenth of a core, for ever,
//! to show a picture that was not changing. Nothing inside the window could say why. `frame_cost`,
//! `completion_cost`, `symbol_cost` and the rest each measure one piece with no window behind it,
//! and every one of them said its piece was cheap, which was true and did not add up to the number
//! the operating system was reporting.
//!
//! So this is the missing instrument: the real binary, on this machine, saying what each part of a
//! real frame cost. `UNLUMINATE_FRAME_TRACE=<file> unluminate` writes one line per frame —
//!
//! ```text
//! frame 21.482 outside 3.104 | control 0.021 colour 0.004 index 0.002 watch 6.914 menus 4.201 ...
//! ```
//!
//! — where `frame` is [`UnluminateApp::ui`](crate::UnluminateApp::ui) end to end, `outside` is
//! everything between one frame's end and the next one's start (egui's own tessellation, the
//! graphics card, and the wait), and each name after the bar is one phase of the frame.
//!
//! It is a **diagnostic and not a test**, for the reason `frame_cost` gives: a threshold in
//! milliseconds would be a different number on every machine. What it buys is that the next person
//! to ask "why is an idle window warm" has an answer in one run instead of a week of guessing, and
//! that a change made for speed can be shown to have worked rather than argued to have.
//!
//! **It costs nothing when it is off**, which is the only way an instrument may live in a hot path:
//! [`begin`] loads one atomic and returns, and every other call reads one thread local that is
//! `None`. The switch is read once, on the first frame, and never again.
//!
//! `tools/frame-trace.mjs` reads a written trace and prints the median of each phase, which is what
//! turns a few thousand lines into the four numbers worth reading.

use std::cell::RefCell;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Whether the trace is on. Read on every `phase` call, so it is an atomic rather than a lock.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// The file being written to, opened once on the first frame. `None` when the switch is not set.
static SINK: OnceLock<Option<Mutex<std::io::BufWriter<std::fs::File>>>> = OnceLock::new();

/// One frame's marks, while it is being recorded.
struct Recording {
    began: Instant,
    /// The end of the previous mark, which is where the next phase's time is measured from.
    since: Instant,
    phases: Vec<(&'static str, f64)>,
    /// How long between the last frame ending and this one starting: egui's own work, the graphics
    /// card, and the wait. Absent on the first frame, which has nothing before it.
    outside: Option<f64>,
}

thread_local! {
    static FRAME: RefCell<Option<Recording>> = const { RefCell::new(None) };
    /// When the last frame ended, so the next one can say how long the gap was.
    static ENDED: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

/// Open the file the switch names, or answer that there is no trace to write.
fn sink() -> Option<&'static Mutex<std::io::BufWriter<std::fs::File>>> {
    SINK.get_or_init(|| {
        let path = std::env::var_os("UNLUMINATE_FRAME_TRACE")?;
        let file = std::fs::File::create(&path).ok()?;
        WATCHING.store(true, Ordering::Relaxed);
        let mut writer = std::io::BufWriter::new(file);
        // A header, so a trace read a month later says what its columns are and which build wrote it.
        let _ = writeln!(
            writer,
            "# unluminate {} frame trace — `frame` is UnluminateApp::ui end to end, `outside` is everything between two frames",
            crate::build_info::VERSION
        );
        let _ = writer.flush();
        Some(Mutex::new(writer))
    })
    .as_ref()
}

/// Whether anything is being written down. Cheap enough to ask on every frame.
pub fn watching() -> bool {
    WATCHING.load(Ordering::Relaxed)
}

/// Start recording a frame. Does nothing at all unless the switch is set.
///
/// The switch is read on the first call and cached in an atomic, so every later frame costs one
/// relaxed load when the trace is off.
pub fn begin() {
    // `sink` is what reads the environment, so it has to be called once before `WATCHING` means
    // anything. After the first frame this is a single atomic load.
    if !WATCHING.load(Ordering::Relaxed) && sink().is_none() {
        return;
    }
    let now = Instant::now();
    let outside = ENDED.with(|ended| ended.borrow().map(|at| elapsed(at, now)));
    FRAME.with(|frame| {
        *frame.borrow_mut() =
            Some(Recording { began: now, since: now, phases: Vec::with_capacity(24), outside });
    });
}

/// Record that `name` has just finished, taking the time since the previous mark.
///
/// A name that appears twice in a frame is written twice, which is right: two turns round a loop
/// are two pieces of work and averaging them would hide the one that was slow.
pub fn phase(name: &'static str) {
    if !WATCHING.load(Ordering::Relaxed) {
        return;
    }
    let now = Instant::now();
    FRAME.with(|frame| {
        if let Some(recording) = frame.borrow_mut().as_mut() {
            recording.phases.push((name, elapsed(recording.since, now)));
            recording.since = now;
        }
    });
}

/// Finish the frame and write its line.
pub fn end() {
    if !WATCHING.load(Ordering::Relaxed) {
        return;
    }
    let now = Instant::now();
    let Some(recording) = FRAME.with(|frame| frame.borrow_mut().take()) else { return };
    ENDED.with(|ended| *ended.borrow_mut() = Some(now));
    let Some(sink) = sink() else { return };
    let mut line = String::with_capacity(256);
    line.push_str(&format!("frame {:.3}", elapsed(recording.began, now)));
    if let Some(outside) = recording.outside {
        line.push_str(&format!(" outside {outside:.3}"));
    }
    line.push_str(" |");
    for (name, took) in &recording.phases {
        line.push_str(&format!(" {name} {took:.3}"));
    }
    if let Ok(mut writer) = sink.lock() {
        let _ = writeln!(writer, "{line}");
        // Flushed every frame rather than buffered, because the interesting trace is very often the
        // one from a window that was killed rather than closed.
        let _ = writer.flush();
    }
}

/// Note that something happened, with the milliseconds since the program started.
///
/// This is the startup half of the instrument. A frame is a loop and is measured as one; starting up
/// happens once, in a straight line, so what is worth writing down is not how long each step took but
/// **when the program had got that far** — which is the number a person comparing two builds reads.
///
/// `task-1805` measured startup at 1234 ms to a window that answers, with nothing inside the process
/// able to say which of the dozen things `main` does was the second of it.
pub fn mark(name: &str) {
    // Unlike `phase`, this cannot short-circuit on the atomic: the marks all happen before the first
    // frame, so nothing has opened the file yet.
    let Some(sink) = sink() else { return };
    let since = STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
    if let Ok(mut writer) = sink.lock() {
        let _ = writeln!(writer, "mark {name} {since:.3}");
        let _ = writer.flush();
    }
}

/// When the program started, as near as anything here can tell: the first call to [`mark`].
static STARTED: OnceLock<Instant> = OnceLock::new();

/// Milliseconds between two instants, as a number rather than a `Duration`.
fn elapsed(from: Instant, to: Instant) -> f64 {
    to.saturating_duration_since(from).as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With the switch unset every call is a no-op, which is what makes it safe to leave the marks
    /// in the frame. Nothing is opened, nothing is allocated and nothing is written.
    #[test]
    fn nothing_is_recorded_while_the_switch_is_not_set() {
        // The switch is read once per process and the test binary has it unset, so `watching` is
        // false here however many times these are called.
        begin();
        phase("something");
        end();
        assert!(!watching(), "no trace file was named, so nothing is being watched");
        FRAME.with(|frame| assert!(frame.borrow().is_none(), "no recording was started"));
    }
}
