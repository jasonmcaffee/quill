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

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use quill_cli::instances::{self, Instance};
use quill_cli::protocol::{code, Reply, Request};

/// How long the listener will hold a connection open waiting for the window to answer.
///
/// Longer than any command's own wait, because the command's own timeout is the one that should
/// fire and say what it was waiting for. This is the backstop for a window that has stopped drawing
/// altogether, which would otherwise leave the caller hanging for ever.
const BACKSTOP: Duration = Duration::from_secs(120);

/// One request, waiting for the window to answer it.
///
/// Dropping it without answering sends a failure rather than nothing, so a caller is never left
/// waiting because an arm of a match forgot to reply.
pub struct Pending {
    pub request: Request,
    reply: Option<Sender<Reply>>,
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

/// The listening thread, and the file that says how to reach it.
pub struct Server {
    instance: Instance,
    requests: Receiver<Pending>,
    /// Where the instance file was written, so the same one is removed when Quill stops.
    file: PathBuf,
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
        std::thread::Builder::new()
            .name("quill-control".to_owned())
            .spawn(move || accept(listener, sender, expected, wake))
            .ok()?;
        Some(Server { instance, requests, file })
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
    pub fn take(&self) -> Vec<Pending> {
        let mut out = Vec::new();
        loop {
            match self.requests.try_recv() {
                Ok(pending) => out.push(pending),
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
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let sender = sender.clone();
        let expected = expected.clone();
        let wake = wake.clone();
        // A thread each, because a command may be asked to wait — `terminal read --wait-for` holds
        // on until the shell has finished — and one connection waiting must not stop the next one
        // being read.
        let _ = std::thread::Builder::new()
            .name("quill-control-connection".to_owned())
            .spawn(move || serve(stream, sender, expected, wake));
    }
}

/// One connection: read a request, have it answered, write the answer.
fn serve(
    stream: TcpStream,
    sender: Sender<Pending>,
    expected: String,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let Ok(mut writing) = stream.try_clone() else {
        return;
    };
    let mut line = String::new();
    if BufReader::new(stream).read_line(&mut line).is_err() {
        return;
    }
    let reply = read_and_queue(&line, &sender, &expected, &wake);
    let _ = writeln!(writing, "{}", reply.to_json());
    let _ = writing.flush();
}

/// Turn a line into an answer: refuse it here, or queue it and wait for the window.
fn read_and_queue(
    line: &str,
    sender: &Sender<Pending>,
    expected: &str,
    wake: &Arc<dyn Fn() + Send + Sync>,
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
    let (answer, wait) = mpsc::channel();
    if sender.send(Pending { request, reply: Some(answer) }).is_err() {
        return Reply::failed(&command, code::NOT_RUNNING, "Quill is closing.");
    }
    wake();
    match wait.recv_timeout(BACKSTOP) {
        Ok(reply) => reply,
        Err(_) => Reply::failed(
            &command,
            code::TIMED_OUT,
            "Quill did not answer. The window may be busy or may have stopped drawing.",
        ),
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
        };
        drop(pending);
        let reply = receiver.recv().expect("a reply");
        assert!(!reply.ok, "a dropped request must not leave the caller waiting");
    }
}
