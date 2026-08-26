//! The channel a running Quill is driven down.
//!
//! A thread listens on `127.0.0.1` on a port the operating system chose, reads one JSON object a
//! line, and puts what it read on a queue for the window to answer on its next frame. That is the
//! whole mechanism; what the commands *mean* is `app::cli`, and what they are *called* is
//! `quill_cli::catalogue`, which both halves share so they cannot drift apart.
//!
//! ## Why a socket on the loopback interface
//!
//! It is the only transport that is one piece of code on both platforms. A Unix domain socket is
//! the obvious answer on macOS and the standard library has no portable way to open one on Windows,
//! where the equivalent is a named pipe with a different API and different semantics — two
//! implementations, two sets of bugs, and a dependency to paper over them. A loopback socket is
//! `std::net` on both, and it has a second advantage that matters for what `task-1661` is for: any
//! language with a socket and a JSON library can drive Quill in three lines, so `quill-cli` is the
//! comfortable way in rather than the only one.
//!
//! What it costs is that every program running as this person can reach the port, which a socket
//! with file permissions would not allow. That is what the token answers: it is written into the
//! instance file, which lives in the person's own settings folder, and a request without it is
//! refused. It does not defend against another program running as them — nothing on a desktop does
//! — but it does stop a page in a browser, which can post to a loopback port and cannot read a
//! file, from driving somebody's editor.
//!
//! Nothing is ever bound to anything but `127.0.0.1`. There is a test for it.
//!
//! ## Why the window answers rather than the thread
//!
//! Every command changes or reads the window's own state, and the window is a single thread with a
//! frame loop. So the listener does not touch it: it queues the request, wakes the window, and
//! waits for the answer. The window drains the queue at the top of a frame, which is also what
//! makes a command's effect visible in the very next screenshot.
//!
//! Some answers cannot be given on the frame they were asked on — a screenshot arrives a frame
//! later, a search is still running, git is on its own thread — so a [`Pending`] can be held and
//! answered whenever it is ready. That is why it carries its own channel rather than being answered
//! by returning a value.
//!
//! ## Why the wake repeats
//!
//! `task-1691` measured a window that answered nothing for sixty-five seconds while sleeping at no
//! processor use, still listening, with four connection threads parked on their deadlines and the
//! frame loop parked in its ordinary event wait. The queue was full and nothing drained it.
//!
//! The wake was one call to `egui::Context::request_repaint`, and one repaint request is not a
//! reliable way to wake a window. Two mechanisms lose it, both in code Quill does not own.
//! `ContextImpl::request_repaint_after` calls the backend's callback only `if delay <
//! viewport.repaint.repaint_delay`, so a request made while the window is already repainting sends
//! nothing at all; and `eframe`'s `user_event` throws away a request whose pass number is more than
//! one behind the current one — "Got outdated UserEvent::RequestRepaint" — on the assumption that
//! the repaint it asked for has already happened. That assumption is right for a repaint and wrong
//! for a wake: the work is still on the queue and the wake is gone.
//!
//! So the wake repeats. While a request is still on the queue the window is woken again every
//! [`NUDGE`], and the moment the window picks the request up the nudging stops — which is the
//! ticket's "set the control flow to `Poll` while a request is outstanding" reached from outside
//! the event loop, where Quill sits. `app::cli::pump_control` already does the same thing one step
//! later, for a request the window is holding.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_cli::instances::{self, Instance};
use quill_cli::protocol::{code, Reply, Request};

/// How long the listener will hold a connection open waiting for the window to answer.
///
/// Longer than any command's own wait, because the command's own timeout is the one that should
/// fire and say what it was waiting for. This is the backstop for a window that has stopped drawing
/// altogether, and for a client too old to say how long it will wait.
const BACKSTOP: Duration = Duration::from_secs(120);

/// How often the window is woken again while a request is still on the queue.
///
/// Twenty times a second. A window that is drawing discards a repaint request for nothing, and a
/// window that is not drawing needs one of them to get through — see the module comment. It stops
/// the moment the request is picked up, so the ordinary case costs one extra wake at most.
const NUDGE: Duration = Duration::from_millis(50);

/// The largest amount taken off the caller's own deadline so that the window's refusal beats the
/// client's socket timeout.
///
/// The two say different things. The client's says only that nothing came back; the window's says
/// how long it has been since a frame was drawn and how many requests are queued, which is the
/// difference between "it is busy" and "it has stopped".
///
/// A **share** of the deadline rather than a fixed amount, because both ends of the range matter: a
/// tenth of a second off a fifteen-second wait is inside the granularity of the nudge loop and the
/// client wins the race, and half a second off a five-hundred-millisecond wait would answer before
/// the window had a chance to. See [`margin_for`].
const MARGIN: Duration = Duration::from_millis(500);

/// How much of the caller's deadline to keep back: a tenth of it, and never more than [`MARGIN`].
fn margin_for(deadline: Duration) -> Duration {
    (deadline / 10).min(MARGIN)
}

/// A frame drawn within this is a window that is plainly still drawing.
const STOPPED: Duration = Duration::from_secs(1);

/// One request, waiting for the window to answer it.
///
/// Dropping it without answering sends a failure rather than nothing, so a caller is never left
/// waiting because an arm of a match forgot to reply.
pub struct Pending {
    pub request: Request,
    reply: Option<Sender<Reply>>,
    /// Set by [`Server::take`], so the connection thread knows to stop waking the window.
    taken: Arc<AtomicBool>,
    /// Set by the connection thread when the caller's deadline passed with this still on the
    /// queue. The window throws such a request away instead of running it.
    abandoned: Arc<AtomicBool>,
    /// When it went on the queue, and how long the caller was prepared to wait from then.
    ///
    /// The flag above is the connection thread *reporting* that the caller has gone; these two let
    /// the window **derive** it, which is the rule `follow_the_open_file` and the relayout
    /// fingerprint already keep. It is not belt and braces: a process whose threads all stop
    /// together — suspended, swapped out, a machine that slept — comes back with the queue full and
    /// nobody having had a chance to set anything, and the frame loop can reach the request before
    /// the connection thread reaches its own deadline. Measured on Windows with the whole process
    /// suspended: the flag alone let `run add` be applied a second after the caller had been told
    /// it timed out.
    queued_at: Instant,
    patience: Duration,
}

impl Pending {
    /// Answer it. There is one answer and then the connection closes.
    pub fn answer(mut self, reply: Reply) {
        if let Some(channel) = self.reply.take() {
            let _ = channel.send(reply);
        }
    }

    /// The command's wire name, such as `tab.open`.
    pub fn command(&self) -> &str {
        &self.request.command
    }

    /// True when the caller gave up before the window ever picked this up.
    ///
    /// It has already been told the command did not happen, so running it now would make that a
    /// lie — which is what `task-1691` measured three times over: `run add`, `run remove` and `run
    /// rerun` each reported a timeout and each had been applied.
    ///
    /// Asked at the moment the window is about to run the command, so the deadline it compares
    /// against is only ever the one for a request that sat on the queue too long. A command the
    /// window took at once and is *holding* — `terminal read --wait-for`, a git action — was not
    /// abandoned however long it goes on to wait, because it is past this point already.
    pub fn was_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::SeqCst) || self.queued_at.elapsed() > self.patience
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(channel) = self.reply.take() {
            let _ = channel.send(Reply::failed(
                "",
                code::FAILED,
                "Quill dropped the request without answering it.",
            ));
        }
    }
}

/// What the listener knows about the window without asking it anything.
///
/// Three numbers, and between them they say whether a window that has not answered is busy or has
/// stopped — which is what a caller that times out actually needs to be told, and what
/// `task-1691`'s agent had to run `sample` on the process to find out.
struct Health {
    /// When the channel opened, which every other measurement here is against.
    started: Instant,
    /// Frames the window has drawn since then.
    frames: AtomicU64,
    /// Milliseconds since [`Self::started`] at the top of the last frame, or zero for none yet.
    last_frame_ms: AtomicU64,
    /// Requests queued and not yet picked up by the window.
    queued: AtomicUsize,
}

impl Health {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            frames: AtomicU64::new(0),
            last_frame_ms: AtomicU64::new(0),
            queued: AtomicUsize::new(0),
        }
    }

    /// Note that a frame has begun. [`Server::take`] is the only caller, because it is called once
    /// at the top of every frame and by nothing else — so counting frames needs no new call site
    /// and no rule anybody has to remember.
    fn a_frame_began(&self) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        let since = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.last_frame_ms.store(since.max(1), Ordering::Relaxed);
    }

    /// How long ago the last frame was, or nothing when there has never been one.
    fn since_the_last_frame(&self) -> Option<Duration> {
        let last = self.last_frame_ms.load(Ordering::Relaxed);
        if last == 0 {
            return None;
        }
        Some(self.started.elapsed().saturating_sub(Duration::from_millis(last)))
    }

    /// The sentence a timeout carries, said in what is actually known rather than in guesses.
    fn what_was_seen(&self) -> String {
        let queued = self.queued.load(Ordering::Relaxed);
        let waiting = match queued {
            1 => "1 request is queued".to_owned(),
            several => format!("{several} requests are queued"),
        };
        match self.since_the_last_frame() {
            None => format!(
                "It has drawn no frame at all since the command channel opened, and {waiting}."
            ),
            Some(since) if since < STOPPED => {
                format!("It drew a frame {} ago and {waiting}, so it is busy rather than stopped.", spell(since))
            }
            Some(since) => format!(
                "It has not drawn a frame for {} and {waiting}, so it is not drawing rather than busy.",
                spell(since)
            ),
        }
    }
}

/// A duration in the units a person reads it in.
fn spell(how_long: Duration) -> String {
    let millis = how_long.as_millis();
    if millis < 1_000 {
        format!("{millis} ms")
    } else {
        format!("{:.1} s", how_long.as_secs_f64())
    }
}

/// The listening thread, and the file that says how to reach it.
pub struct Server {
    instance: Instance,
    requests: Receiver<Pending>,
    /// Where the instance file was written, so the same one is removed when Quill stops.
    file: PathBuf,
    health: Arc<Health>,
}

impl Server {
    /// Start listening, and advertise it.
    ///
    /// `wake` is called when a request arrives, so the window draws a frame and answers it. Failing
    /// to start is reported and returns `None`: a Quill that cannot open a port is still a text
    /// editor, and refusing to start would be the wrong trade.
    pub fn start(folder: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let listener = match bind() {
            Ok(listener) => listener,
            Err(problem) => {
                eprintln!("Quill could not open its command channel: {problem}");
                return None;
            }
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(problem) => {
                eprintln!("Quill could not read its own command channel's port: {problem}");
                return None;
            }
        };
        let instance = Instance {
            pid: std::process::id(),
            port,
            token: token(),
            folder,
            started: instances::now(),
        };
        let file = match advertise(&instance) {
            Ok(file) => file,
            Err(problem) => {
                eprintln!("Quill could not write its instance file: {problem}");
                return None;
            }
        };

        let (sender, requests) = mpsc::channel();
        let expected = instance.token.clone();
        let health = Arc::new(Health::new());
        let theirs = health.clone();
        std::thread::Builder::new()
            .name("quill-control".to_owned())
            .spawn(move || accept(listener, sender, expected, wake, theirs))
            .ok()?;
        Some(Server { instance, requests, file, health })
    }

    /// Which port this Quill is listening on, for a test and for the status command.
    pub fn port(&self) -> u16 {
        self.instance.port
    }

    /// The token a request has to carry, for a test.
    pub fn token(&self) -> &str {
        &self.instance.token
    }

    /// Everything that has arrived and not been answered yet.
    ///
    /// Called once at the top of a frame. It never blocks: a frame that finds nothing waiting is
    /// the ordinary case, and the window has drawing to do.
    ///
    /// It is also where a frame is counted, because it is the one thing the window does once at the
    /// top of every frame and nowhere else. Each request is marked as taken on the way out, which
    /// is what stops the connection thread waking the window about it again.
    pub fn take(&self) -> Vec<Pending> {
        self.health.a_frame_began();
        let mut out = Vec::new();
        loop {
            match self.requests.try_recv() {
                Ok(pending) => {
                    pending.taken.store(true, Ordering::SeqCst);
                    self.health.queued.fetch_sub(1, Ordering::SeqCst);
                    out.push(pending);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

impl Drop for Server {
    /// Stop advertising. The thread goes with the process; what must not be left behind is a file
    /// telling the next `quill-cli instances` about a window that is no longer there.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Open the listener.
///
/// Loopback only, and port zero so the operating system finds a free one. Several Quills run at
/// once, so a fixed port would be a fixed collision. Split out from [`Server::start`] so that a
/// test can assert on the address it binds without starting a thread: a command channel reachable
/// from the network would be a text editor anybody could type into, and that is the one thing here
/// that must never quietly change.
fn bind() -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
}

/// Write the instance file, readable only by the person running Quill.
fn advertise(instance: &Instance) -> std::io::Result<PathBuf> {
    let folder = instances::folder();
    std::fs::create_dir_all(&folder)?;
    let path = instance.path_in(&folder);
    std::fs::write(&path, instance.to_text())?;
    // The token is in it, so on a system with file modes it is the person's own and nobody else's.
    // Windows has no equivalent to set here: the folder is already under the person's own
    // application data, which is what the platform's own access control covers.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

/// Take connections for as long as the process lives.
fn accept(
    listener: TcpListener,
    sender: Sender<Pending>,
    expected: String,
    wake: Arc<dyn Fn() + Send + Sync>,
    health: Arc<Health>,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let sender = sender.clone();
        let expected = expected.clone();
        let wake = wake.clone();
        let health = health.clone();
        // A thread each, because a command may be asked to wait — `terminal read --wait-for` holds
        // on until the shell has finished — and one connection waiting must not stop the next one
        // being read.
        let _ = std::thread::Builder::new()
            .name("quill-control-connection".to_owned())
            .spawn(move || serve(stream, sender, expected, wake, health));
    }
}

/// One connection: read a request, have it answered, write the answer.
fn serve(
    stream: TcpStream,
    sender: Sender<Pending>,
    expected: String,
    wake: Arc<dyn Fn() + Send + Sync>,
    health: Arc<Health>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    // One handle for the whole conversation, read through a `BufReader` and then taken back out to
    // write the answer through. It used to read through a `BufReader` built from the stream and
    // write through a `try_clone` of it, which closed the reading handle the moment the request had
    // been read — while the caller was still waiting for the answer. That is what
    // `a_request_reaches_the_window_and_the_answer_comes_back` failed on, about one run in five of
    // the whole library suite: the reply was written and flushed successfully and the caller's read
    // came back `ConnectionReset` with nothing in it, which is what Windows does when a socket is
    // torn down with anything still owing on it. One handle, closed once, after the answer has
    // gone.
    let mut reading = BufReader::new(stream);
    let mut line = String::new();
    if reading.read_line(&mut line).is_err() {
        return;
    }
    // Taken back out before the request is queued rather than after it is answered, because the
    // socket is how the caller says it has gone — see [`caller_has_gone`].
    let mut writing = reading.into_inner();
    let reply = read_and_queue(&line, &sender, &expected, &wake, &health, &writing);
    let _ = writeln!(writing, "{}", reply.to_json());
    let _ = writing.flush();
}

/// Whether the caller has closed the connection and stopped listening for an answer.
///
/// A client that gives up says nothing — it stops reading and its socket closes — so this is the
/// one signal that arrives at the moment it happens rather than being inferred from a clock. It is
/// what makes a request that reached Quill *after* its caller had gone answerable at all: the
/// deadline is measured from the moment the request was queued, and a request the listener could
/// not even accept until the window woke up looks brand new however long the caller waited for it.
/// Measured on Windows with the whole process suspended, which is exactly that shape.
///
/// The socket is put in non-blocking mode for the wait and back into blocking before the reply is
/// written, because both handles refer to one socket and the mode belongs to the socket rather than
/// to a handle. Nothing else touches it in between; this thread owns the connection.
fn caller_has_gone(stream: &TcpStream) -> bool {
    let mut byte = [0u8; 1];
    match stream.peek(&mut byte) {
        // Nought bytes with no error is the other end closed, which is what every socket API means
        // by it.
        Ok(0) => true,
        // It sent something else, which is not this protocol's business — one object a line — but
        // it is plainly still there.
        Ok(_) => false,
        Err(problem) if problem.kind() == std::io::ErrorKind::WouldBlock => false,
        // A connection that is broken is a caller that is not going to read the answer.
        Err(_) => true,
    }
}

/// Turn a line into an answer: refuse it here, or queue it and wait for the window.
fn read_and_queue(
    line: &str,
    sender: &Sender<Pending>,
    expected: &str,
    wake: &Arc<dyn Fn() + Send + Sync>,
    health: &Arc<Health>,
    stream: &TcpStream,
) -> Reply {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return Reply::failed("", code::USAGE, "That was not a JSON object.");
    };
    let Some(request) = Request::from_json(&value) else {
        return Reply::failed(
            "",
            code::USAGE,
            "A request needs a token and a command: {\"token\":\"...\",\"command\":\"status\"}.",
        );
    };
    // Compared in full and only after the request has parsed, so a wrong token gets the same
    // refusal whatever else was wrong with the line.
    if request.token != expected {
        return Reply::failed(
            &request.command,
            code::REFUSED,
            "That is not this Quill's token. It is in the instance file; `quill-cli instances` \
             finds it.",
        );
    }
    let command = request.command.clone();
    // What the caller said it would wait, less the margin that lets this answer beat its socket
    // timeout. A client that said nothing gets the backstop, exactly as every client did before
    // `deadline_ms` existed.
    let said = request.deadline_ms;
    let deadline = said
        .map(|millis| {
            let asked = Duration::from_millis(millis);
            asked.saturating_sub(margin_for(asked)).max(NUDGE)
        })
        .unwrap_or(BACKSTOP);
    let (answer, wait) = mpsc::channel();
    let taken = Arc::new(AtomicBool::new(false));
    let abandoned = Arc::new(AtomicBool::new(false));
    // Nothing is queued for a caller that has already gone. A request read off a socket that is
    // closed came from a client that gave up while the listener was not running, and queueing it
    // would be exactly the fault this is here to fix, one step earlier.
    let watched = stream.set_nonblocking(true).is_ok();
    if watched && caller_has_gone(stream) {
        let _ = stream.set_nonblocking(false);
        return Reply::failed(
            &command,
            code::TIMED_OUT,
            format!(
                "The caller had closed the connection before {command} reached Quill, so it was \
                 not run. {}",
                health.what_was_seen()
            ),
        );
    }
    let pending = Pending {
        request,
        reply: Some(answer),
        taken: taken.clone(),
        abandoned: abandoned.clone(),
        queued_at: Instant::now(),
        patience: deadline,
    };
    health.queued.fetch_add(1, Ordering::SeqCst);
    if sender.send(pending).is_err() {
        health.queued.fetch_sub(1, Ordering::SeqCst);
        let _ = stream.set_nonblocking(false);
        return Reply::failed(&command, code::NOT_RUNNING, "Quill is closing.");
    }
    wake();
    let watching = watched.then_some(stream);
    let reply =
        wait_for_the_window(&command, &wait, &taken, &abandoned, deadline, said, wake, health, watching);
    // Back to blocking before the answer is written, because the mode belongs to the socket and the
    // write below is the one thing that must not come back `WouldBlock`.
    let _ = stream.set_nonblocking(false);
    reply
}

/// Wait for the window's answer, waking it again for as long as it has not picked the request up.
///
/// Three things are being watched at once and the module comment says why each matters. While the
/// request is still on the queue the window is nudged every [`NUDGE`], because one wake can be
/// dropped and a stream of them cannot. Once the window has taken it, the nudging stops and this
/// simply waits: a command the window is holding — `terminal read --wait-for`, a git action, a
/// screenshot — owns its own deadline, and `pump_control` is already keeping the window drawing for
/// it. And when the caller's deadline passes with the request **still on the queue**, it is marked
/// abandoned so that the window throws it away rather than applying it to a caller that has gone.
fn wait_for_the_window(
    command: &str,
    wait: &mpsc::Receiver<Reply>,
    taken: &Arc<AtomicBool>,
    abandoned: &Arc<AtomicBool>,
    deadline: Duration,
    said: Option<u64>,
    wake: &Arc<dyn Fn() + Send + Sync>,
    health: &Arc<Health>,
    watching: Option<&TcpStream>,
) -> Reply {
    let began = Instant::now();
    loop {
        match wait.recv_timeout(NUDGE) {
            Ok(reply) => return reply,
            Err(RecvTimeoutError::Disconnected) => {
                return Reply::failed(command, code::NOT_RUNNING, "Quill is closing.")
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let picked_up = taken.load(Ordering::SeqCst);
        let spent = began.elapsed();
        // The caller closing the connection is the same thing as its deadline passing, and it is
        // the earlier of the two: it says so at the moment it happens rather than being inferred
        // from a clock. A request the window is already holding is left alone — it has been run.
        if !picked_up && watching.map(caller_has_gone).unwrap_or(false) {
            abandoned.store(true, Ordering::SeqCst);
            return Reply::failed(
                command,
                code::TIMED_OUT,
                format!(
                    "The caller stopped waiting for {command} after {} ms, so it was not run. {}",
                    spent.as_millis(),
                    health.what_was_seen()
                ),
            );
        }
        // A request the window is holding is given the backstop rather than the caller's deadline,
        // because a held command owns its own wait: `debug start --wait-for-pause` waits thirty
        // seconds and `git action --wait` waits thirty, and cutting those to the transport's
        // deadline would shorten every waiting command in the catalogue without saying so.
        let over = match picked_up {
            true => spent >= BACKSTOP,
            false => spent >= deadline,
        };
        if !over {
            if !picked_up {
                wake();
            }
            continue;
        }
        if !picked_up {
            // Marked before the reply is written, so a frame that lands between the two finds the
            // flag already set rather than running a command whose caller has been told it failed.
            abandoned.store(true, Ordering::SeqCst);
        }
        let ran = match picked_up {
            true => "Quill had taken it, so it may already have been run.",
            false => "The command was not run.",
        };
        let asked = said.unwrap_or_else(|| spent.as_millis().min(u64::MAX as u128) as u64);
        return Reply::failed(
            command,
            code::TIMED_OUT,
            format!(
                "Quill did not answer {command} within {asked} ms. {} {ran}",
                health.what_was_seen()
            ),
        );
    }
}

/// A token for this run.
///
/// Sixteen bytes from the operating system's own randomness, spelled in hexadecimal.
/// `RandomState` is what the standard library seeds its hash maps with and is seeded from the
/// platform's random source on both Windows and macOS, which is why it is used here rather than a
/// dependency: this is a capability token in a file only the person can read, not a key.
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    for part in 0..2u64 {
        let state = std::collections::hash_map::RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_u64(part);
        hasher.write_u64(std::process::id() as u64);
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0),
        );
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// One server at a time, in a folder of its own.
    ///
    /// Two reasons for the lock, and both are real. Where the instance files go is named by an
    /// environment variable, which belongs to the whole process rather than to one test. And an
    /// instance file is named after the process id, so two servers started in one test binary would
    /// be two servers writing one file. Tests elsewhere run side by side; these take turns.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Running {
        server: Server,
        folder: PathBuf,
        woken: Arc<AtomicUsize>,
        _turn: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.folder).ok();
        }
    }

    fn a_server(name: &str) -> Running {
        let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let folder = std::env::temp_dir().join(format!("quill-control-{name}"));
        std::fs::remove_dir_all(&folder).ok();
        std::fs::create_dir_all(&folder).expect("make the folder");
        std::env::set_var("QUILL_INSTANCES", &folder);
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = woken.clone();
        let server = Server::start(
            PathBuf::from("/a/project"),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .expect("the server should start");
        Running { server, folder, woken, _turn: turn }
    }

    fn send(port: u16, line: &str) -> String {
        use std::io::Read;
        let mut stream =
            TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
        writeln!(stream, "{line}").expect("write");
        let mut back = String::new();
        stream.read_to_string(&mut back).expect("read");
        back
    }

    #[test]
    fn a_request_reaches_the_window_and_the_answer_comes_back() {
        let running = a_server("round-trip");
        let server = &running.server;
        let port = server.port();
        let token = server.token().to_owned();
        let caller = std::thread::spawn(move || {
            send(port, &format!("{{\"token\":\"{token}\",\"command\":\"status\"}}"))
        });
        // Stand in for the window's frame loop: wait for the request, answer it.
        let mut answered = false;
        for _ in 0..200 {
            for pending in server.take() {
                assert_eq!(pending.command(), "status");
                pending.answer(Reply::done("status", "All well", serde_json::json!({ "a": 1 })));
                answered = true;
            }
            if answered {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(answered, "the request should have reached the window");
        let back = caller.join().expect("the caller");
        assert!(back.contains("\"ok\":true"), "{back}");
        assert!(back.contains("All well"), "{back}");
        assert!(running.woken.load(Ordering::SeqCst) > 0, "the window should have been woken");
    }

    #[test]
    fn a_request_with_the_wrong_token_is_refused_without_reaching_the_window() {
        let running = a_server("wrong-token");
        let back =
            send(running.server.port(), "{\"token\":\"nonsense\",\"command\":\"status\"}");
        assert!(back.contains("\"ok\":false"), "{back}");
        assert!(back.contains(code::REFUSED), "{back}");
        assert!(running.server.take().is_empty(), "nothing should have been queued");
    }

    #[test]
    fn a_line_that_is_not_a_request_is_refused_rather_than_ignored() {
        let running = a_server("nonsense");
        let back = send(running.server.port(), "not json at all");
        assert!(back.contains("\"ok\":false"), "{back}");
        assert!(back.contains(code::USAGE), "{back}");
    }

    #[test]
    fn the_instance_file_is_written_while_it_runs_and_removed_when_it_stops() {
        let running = a_server("instance-file");
        let listed = instances::listed_in(&running.folder);
        assert_eq!(listed.len(), 1, "one instance should be advertised");
        assert_eq!(listed[0].port, running.server.port());
        assert_eq!(listed[0].token, running.server.token());
        assert_eq!(listed[0].pid, std::process::id());
        let folder = running.folder.clone();
        drop(running);
        assert!(
            instances::listed_in(&folder).is_empty(),
            "the instance file should go when the server does"
        );
    }

    #[test]
    fn it_listens_on_the_loopback_interface_and_nowhere_else() {
        // The one thing that must never change: a command channel reachable from the network would
        // be a text editor anybody could type into.
        let listener = bind().expect("bind");
        let address = listener.local_addr().expect("an address");
        assert!(address.ip().is_loopback(), "bound to {address}, which is not the loopback");
        assert_ne!(address.port(), 0, "the operating system should have chosen a port");
    }

    #[test]
    fn two_tokens_are_never_the_same() {
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..50 {
            let token = token();
            assert_eq!(token.len(), 32, "a token is sixteen bytes in hexadecimal");
            assert!(!seen.contains(&token), "a token was handed out twice");
            seen.push(token);
        }
    }

    #[test]
    fn a_pending_dropped_without_an_answer_still_answers() {
        let (sender, receiver) = mpsc::channel();
        let pending = Pending {
            request: Request::new("t", "status", Default::default()),
            reply: Some(sender),
            taken: Arc::new(AtomicBool::new(false)),
            abandoned: Arc::new(AtomicBool::new(false)),
            queued_at: Instant::now(),
            patience: BACKSTOP,
        };
        drop(pending);
        let reply = receiver.recv().expect("a reply");
        assert!(!reply.ok, "a dropped request must not leave the caller waiting");
    }

    #[test]
    fn a_request_that_sat_on_the_queue_past_the_deadline_is_abandoned_without_being_told() {
        // The connection thread says so when it can. This is the case where it cannot: every
        // thread in the process stopped together — suspended, or a machine that slept — so nobody
        // set anything, and on waking the frame loop can reach the request first. Measured on
        // Windows with the whole process suspended, where the flag alone let `run add` be applied
        // a second after the caller had been told it timed out.
        let (sender, _receiver) = mpsc::channel();
        let pending = Pending {
            request: Request::new("t", "run.add", Default::default()),
            reply: Some(sender),
            taken: Arc::new(AtomicBool::new(false)),
            abandoned: Arc::new(AtomicBool::new(false)),
            queued_at: Instant::now() - Duration::from_secs(30),
            patience: Duration::from_secs(15),
        };
        assert!(pending.was_abandoned(), "nobody is waiting for this any more");
        // And one queued a moment ago is not, however long its caller said it would wait.
        let (sender, _receiver) = mpsc::channel();
        let fresh = Pending {
            request: Request::new("t", "run.add", Default::default()),
            reply: Some(sender),
            taken: Arc::new(AtomicBool::new(false)),
            abandoned: Arc::new(AtomicBool::new(false)),
            queued_at: Instant::now(),
            patience: Duration::from_millis(50),
        };
        assert!(!fresh.was_abandoned());
    }

    #[test]
    fn a_request_is_answered_even_when_the_first_wake_is_lost() {
        // `task-1691`. egui drops a repaint request it is already serving and eframe drops one it
        // decides is outdated, so a wake sent once can simply vanish — which is what left four
        // connection threads parked on their deadlines with the queue full. The stand-in window
        // here does exactly that: it ignores the first wake altogether and only looks at the queue
        // on a later one. It fails on the code as it was, because that code woke once.
        let running = a_server("lost-wake");
        let port = running.server.port();
        let token = running.server.token().to_owned();
        let caller = std::thread::spawn(move || {
            send(port, &format!("{{\"token\":\"{token}\",\"command\":\"status\"}}"))
        });
        let server = &running.server;
        let mut answered = false;
        for _ in 0..400 {
            // The first wake is thrown on the floor. Nothing drains the queue until a second one
            // arrives, which only happens because the listener keeps nudging.
            if running.woken.load(Ordering::SeqCst) > 1 {
                for pending in server.take() {
                    pending.answer(Reply::done("status", "All well", serde_json::json!({})));
                    answered = true;
                }
            }
            if answered {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(answered, "a request whose first wake was lost should still be answered");
        let back = caller.join().expect("the caller");
        assert!(back.contains("\"ok\":true"), "{back}");
        assert!(
            running.woken.load(Ordering::SeqCst) > 1,
            "the listener should wake the window again while the request is still queued"
        );
    }

    #[test]
    fn the_window_is_left_alone_once_it_has_taken_the_request() {
        // The other half of the rule above. A command the window is holding — a search, a git
        // action, `terminal read --wait-for` — must not have the window woken twenty times a
        // second for the minutes it may wait, so the nudging stops the moment it is picked up.
        let running = a_server("stop-nudging");
        let port = running.server.port();
        let token = running.server.token().to_owned();
        let caller = std::thread::spawn(move || {
            send(port, &format!("{{\"token\":\"{token}\",\"command\":\"status\"}}"))
        });
        let mut held = Vec::new();
        for _ in 0..200 {
            held.extend(running.server.take());
            if !held.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(held.len(), 1, "the request should have reached the window");
        let after_taking = running.woken.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            running.woken.load(Ordering::SeqCst),
            after_taking,
            "a request the window is holding should not go on waking it"
        );
        held.pop()
            .expect("the request")
            .answer(Reply::done("status", "All well", serde_json::Value::Null));
        caller.join().expect("the caller");
    }

    #[test]
    fn a_request_the_caller_gave_up_on_is_marked_abandoned_and_never_run() {
        // `task-1691` measured `run add`, `run remove` and `run rerun` each reporting a timeout and
        // each having been applied. The window does not drain the queue at all here, so the
        // deadline passes with the request still on it.
        let running = a_server("abandoned");
        let port = running.server.port();
        let token = running.server.token().to_owned();
        let caller = std::thread::spawn(move || {
            send(
                port,
                &format!(
                    "{{\"token\":\"{token}\",\"command\":\"run.add\",\"deadline_ms\":600}}"
                ),
            )
        });
        let back = caller.join().expect("the caller");
        assert!(back.contains("\"ok\":false"), "{back}");
        assert!(back.contains(code::TIMED_OUT), "{back}");
        assert!(back.contains("The command was not run"), "{back}");
        // Whatever the window picks up afterwards says it must be thrown away rather than applied.
        let waiting = running.server.take();
        assert_eq!(waiting.len(), 1, "the request is still on the queue");
        assert!(waiting[0].was_abandoned(), "a request the caller gave up on must not be run");
    }

    #[test]
    fn a_caller_that_closed_the_connection_is_never_waited_for() {
        // What a client that has timed out really does: it writes the request, stops reading and
        // its socket closes. That is the earliest and most certain sign the caller has gone, and it
        // is the one that covers a request which reached Quill *after* its caller gave up — where
        // the deadline, measured from the moment the request was queued, sees a brand new request.
        let running = a_server("caller-gone");
        let port = running.server.port();
        let token = running.server.token().to_owned();
        {
            let mut stream =
                TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).expect("connect");
            writeln!(stream, "{{\"token\":\"{token}\",\"command\":\"run.add\"}}").expect("write");
            stream.flush().expect("flush");
        } // and closed, with nobody reading the answer
        // Long enough for the connection thread to have looked at the socket at least twice.
        std::thread::sleep(NUDGE * 6);
        // Either it was never queued, or what was queued says it must not be run. Both are the
        // property; which one happens depends on whether the close arrived before the request had
        // been read, and that is the network's business rather than Quill's.
        for pending in running.server.take() {
            assert!(
                pending.was_abandoned(),
                "a request whose caller has gone must not be run: {}",
                pending.command()
            );
        }
    }

    #[test]
    fn the_timeout_says_whether_the_window_is_drawing_or_has_stopped() {
        // What the ticket's agent had to run `sample` on the process to find out. The listener
        // already knows all three numbers, so a caller that times out is told them.
        let health = Health::new();
        health.queued.fetch_add(4, Ordering::SeqCst);
        let never = health.what_was_seen();
        assert!(never.contains("no frame at all"), "{never}");
        assert!(never.contains("4 requests are queued"), "{never}");

        health.a_frame_began();
        let drawing = health.what_was_seen();
        assert!(drawing.contains("busy rather than stopped"), "{drawing}");

        // A frame that was drawn a minute ago is a window that has stopped, which is the whole
        // distinction. The stamp is written directly because a test cannot wait a minute.
        health.last_frame_ms.store(1, Ordering::SeqCst);
        std::thread::sleep(STOPPED + Duration::from_millis(200));
        let stopped = health.what_was_seen();
        assert!(stopped.contains("not drawing rather than busy"), "{stopped}");
    }

    #[test]
    fn a_frame_is_counted_by_the_one_thing_the_window_does_once_a_frame() {
        let running = a_server("frames");
        assert_eq!(running.server.health.frames.load(Ordering::SeqCst), 0);
        assert!(running.server.health.since_the_last_frame().is_none());
        running.server.take();
        running.server.take();
        assert_eq!(running.server.health.frames.load(Ordering::SeqCst), 2);
        assert!(running.server.health.since_the_last_frame().is_some());
    }

    #[test]
    fn a_duration_is_spelled_in_the_units_a_person_reads_it_in() {
        assert_eq!(spell(Duration::from_millis(16)), "16 ms");
        assert_eq!(spell(Duration::from_millis(65_041)), "65.0 s");
    }

    #[test]
    fn the_margin_is_a_share_of_the_deadline_up_to_half_a_second() {
        // Measured against a real window: 300 ms off fifteen seconds lost the race to the client's
        // own socket timeout often enough to matter, because the wait wakes on a 50 ms tick and the
        // client's clock starts first.
        assert_eq!(margin_for(Duration::from_millis(15_000)), MARGIN);
        assert_eq!(margin_for(Duration::from_millis(3_000)), Duration::from_millis(300));
        assert_eq!(margin_for(Duration::from_millis(800)), Duration::from_millis(80));
    }
}
