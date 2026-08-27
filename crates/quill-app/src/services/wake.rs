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
