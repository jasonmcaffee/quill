//! Waking a window whose run loop has stopped answering repaint requests.
//!
//! `task-1691` measured a window that answered nothing for sixty-five seconds: visible, using no
//! processor time, its main thread asleep in the ordinary event wait, with requests queued and
//! nothing draining them. Its answer was to repeat the wake — [`super::control::NUDGE`] asks for a
//! repaint twenty times a second while a request is on the queue — on the reasoning that one repaint
//! request can be dropped and a stream of them cannot.
//!
//! It is not enough. A window was measured again, on macOS, that had not drawn a frame for **three
//! hundred and fifty-nine seconds** with four requests queued, which is seven thousand repaint
//! requests asked for and lost. `app::HEARTBEAT` did not recover it either, because a repaint asked
//! for after a delay is a timer the run loop fires from inside itself, and a run loop that is not
//! running fires nothing.
//!
//! **What did recover it was activating the window** — every time, at once, from outside the process.
//! So the thing that gets through is not a repaint request. It is an event the operating system puts
//! on the main run loop, and the fix is to put one there rather than to ask for a repaint again in a
//! louder voice.
//!
//! On macOS that is a block on the main dispatch queue. The main run loop services that queue, so
//! posting to it wakes the loop *and* runs the block on the main thread — and a `request_repaint`
//! made from the main thread while the loop is awake is handled directly rather than going through
//! the proxy and the user event that `eframe` discards as outdated. Both halves matter: the post
//! wakes it, and the closure asks for the frame from the one thread whose request cannot be thrown
//! away.
//!
//! Elsewhere this does nothing and says so. The hang has only been seen on macOS, and a wake written
//! for a mechanism nobody has measured failing would be a guess with a platform dependency attached.
//! Windows and Linux keep the repeated repaint, which is what they had.

/// Wake the window's run loop by a route that is not a repaint request.
///
/// Called from the control channel's own thread when a request has been queued for
/// [`super::control::ESCALATE`] with no frame drawn — never on the ordinary path, where the repeated
/// repaint is enough and this would be work for nothing.
///
/// `repaint` is run on the main thread, and is the ordinary `Context::request_repaint`.
pub fn the_run_loop(repaint: impl FnOnce() + Send + 'static) {
    the_run_loop_now(repaint);
}

#[cfg(target_os = "macos")]
fn the_run_loop_now(repaint: impl FnOnce() + Send + 'static) {
    // `exec_async` takes the block, so the closure is moved onto the main queue and run there. The
    // main run loop is a consumer of that queue, which is what makes this a wake rather than another
    // request to a loop that is not listening.
    dispatch2::DispatchQueue::main().exec_async(repaint);
}

#[cfg(not(target_os = "macos"))]
fn the_run_loop_now(repaint: impl FnOnce() + Send + 'static) {
    // Nothing platform-specific to do. The repaint is still asked for, so the escalation is never
    // less than what the ordinary nudge does.
    repaint();
}

/// How long with no frame before a wake from a worker thread stops being a repaint request.
///
/// The same half second the control channel uses, and for the same reason: it is long enough that an
/// ordinary idle window is never escalated and short enough that nothing waits on a lost wake.
const ESCALATE: std::time::Duration = std::time::Duration::from_millis(500);

/// When the last frame began, as milliseconds since [`STARTED`], or zero for none yet.
static LAST_FRAME_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What [`LAST_FRAME_MS`] is measured against.
static STARTED: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);

/// Note that a frame has begun, so [`from_a_worker_thread`] can tell an idle window from a stopped one.
///
/// Called from `UnluminateApp::ui` and nowhere else, which is `follow_the_open_file`'s rule: a list of the
/// places that have to say "a frame happened" is a list whose next entry will be the one that forgot.
/// `services::control` keeps a count of its own because its timeout sentence needs the queue depth
/// beside it, and it is fed from `Server::take` at the top of the same frame.
pub fn a_frame_was_drawn() {
    let since = STARTED.elapsed().as_millis().min(u64::MAX as u128) as u64;
    LAST_FRAME_MS.store(since.max(1), std::sync::atomic::Ordering::Relaxed);
}

/// How long ago the last frame was, or nothing when none has been drawn.
fn since_the_last_frame() -> Option<std::time::Duration> {
    let last = LAST_FRAME_MS.load(std::sync::atomic::Ordering::Relaxed);
    if last == 0 {
        return None;
    }
    Some(STARTED.elapsed().saturating_sub(std::time::Duration::from_millis(last)))
}

/// Ask for a frame from a thread that is not the main one, escalating once asking has stopped working.
///
/// **This is the wake every worker thread in the window uses**, and it is a repaint request until the
/// window has plainly stopped drawing. `services::control` reached this conclusion first and kept it to
/// itself; `task-28` measured what that cost everything else. A ticket's agent printed its banner, the
/// session's reader thread asked for a repaint, the request was dropped — which is what the module
/// comment above records — and no frame was drawn for forty-one seconds. The handoff line waiting for
/// that agent's prompt is typed from `AgentTasks::pump`, which runs **on a frame**, so a wake that does
/// not arrive is a ticket whose agent sits at its banner for ever while the board says it is being
/// worked on. Nothing on the screen said why, and a person watching an agent is exactly a person who is
/// not touching the window.
///
/// `None` — no frame ever drawn — is not escalated, for the reason `control::escalation_is_due` gives:
/// a window that has not drawn its first frame is starting up, and the repaint being asked for is what
/// it is waiting for.
pub fn from_a_worker_thread(repaint: impl Fn() + Send + Sync + 'static) {
    if since_the_last_frame().is_some_and(|silent| silent >= ESCALATE) {
        the_run_loop(move || repaint());
        return;
    }
    repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wake_escalates_only_once_nothing_has_been_drawn_for_long_enough() {
        // The decision, which is what a test with no run loop can assert. Whether a run loop wakes is
        // not.
        let due = |since: Option<std::time::Duration>| {
            since.is_some_and(|silent| silent >= ESCALATE)
        };
        assert!(!due(None), "a window that has drawn nothing yet is starting up");
        assert!(!due(Some(ESCALATE - std::time::Duration::from_millis(1))));
        assert!(due(Some(ESCALATE)));
        assert!(due(Some(std::time::Duration::from_secs(41))), "what task-28 measured");
    }

    #[test]
    fn a_frame_moves_the_clock_off_never() {
        assert_eq!(LAST_FRAME_MS.load(std::sync::atomic::Ordering::Relaxed), 0, "nothing drawn yet");
        a_frame_was_drawn();
        let since = since_the_last_frame().expect("a frame has been drawn");
        assert!(since < ESCALATE, "a frame just drawn is not silence: {since:?}");
    }
}
