//! The thread the adapter is spoken to on, arranged exactly as `quill_git::Worker` is.
//!
//! The window never blocks on an adapter. A `stackTrace` over a deep stack, an adapter still loading
//! a twenty-megabyte binary's debug information, a `node dapDebugServer.js` that has not opened its
//! port yet — a window that waited for any of them would stop drawing, which on this machine looks
//! exactly like a crash. So one reader thread parses frames and pushes [`Reply`] values onto a
//! channel, calling a [`Waker`] after each, and the window drains the channel once at the top of a
//! frame where `Worker::poll` and `Session::pump` are already called.
//!
//! Writing is done from the window's own thread. It is a handful of small writes to a pipe or a
//! loopback socket, which never blocks in practice, and a second thread to do it would need a second
//! channel and a lock — for a `continue` request that is eighty bytes long.
//!
//! Stopping is soft then hard, the shape `RunPanel::stop` already has: the session asks the adapter
//! to end, and an adapter that is still there once the grace has run out is killed. It is a child
//! process Quill started, the same as any run.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::adapter::{self, AdapterCommand};
use crate::codec::{self, Decoder};
use crate::messages::Message;

/// How long an adapter is given to end politely before its process is killed.
///
/// Two seconds, which is `RunPanel::GRACE` — the same number for the same reason, and it is short on
/// purpose: the person pressing stop has already decided.
pub const GRACE: Duration = Duration::from_secs(2);

/// Whether every frame in both directions is written to standard error.
///
/// `QUILL_DAP_TRACE=1` switches it on, read once. It exists because a protocol client with no way to
/// see the conversation is guesswork to work on, and this is not hypothetical: it is what found
/// `is_the_node_runtime` — the frames showed js-debug being told to run the node binary as a
/// JavaScript file, which no state in the window could have shown. It is the same argument as reading
/// the adapter's standard error rather than swallowing it, one level down.
///
/// Read through a `OnceLock` rather than per frame, because `variables` over a large structure is
/// hundreds of frames and asking the environment each time would be work done for nothing in a
/// session that is not being traced.
fn tracing() -> bool {
    static TRACING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACING.get_or_init(|| {
        std::env::var("QUILL_DAP_TRACE").is_ok_and(|value| !matches!(value.as_str(), "" | "0"))
    })
}

/// A function the thread calls to have the window drawn again.
///
/// The same `Arc<dyn Fn() + Send + Sync>` the terminal and git already take. A `stopped` event that
/// arrived while the window was idle has to draw itself rather than waiting for the next mouse move.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the thread sends back.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// One message the adapter sent. Boxed for the reason `quill_git::Reply::Snapshot` is: a
    /// `variables` answer over a large structure is far bigger than every other variant, and an enum
    /// is as large as its largest arm.
    Message(Box<Message>),
    /// One message the **child** session sent, once [`Client::adopt_child`] has opened one. A
    /// variant rather than a flag because the two connections share one channel and their sequence
    /// numbers are their own: a parent's late response fed to the child's state machine would be
    /// matched to whatever the child happened to be waiting for.
    FromChild(Box<Message>),
    /// The adapter wrote something that is not the protocol. The session ends, because there is no
    /// way to know where the next frame starts.
    Broken(String),
    /// The adapter's output ended: its process died, or it closed the connection.
    Gone,
}

/// The thread, the channel from it, and the adapter's own process.
pub struct Client {
    replies: Receiver<Reply>,
    writer: Box<dyn Write + Send>,
    child: Option<std::process::Child>,
    /// What was started, for the status bar and for a test.
    described: String,
    /// When the polite stop was sent, so the hard one can follow after [`GRACE`].
    stopping: Option<Instant>,
    /// Set once writing has failed, so a window that goes on sending does not report the same broken
    /// pipe on every frame.
    broken: bool,
    /// Every frame written, kept only by a **detached** client. `None` for a real one, so an
    /// ordinary session never grows a second copy of everything it has said.
    ///
    /// It is what makes a scripted session real rather than approximate: a test reads what the
    /// session actually asked for and answers *that*, with the seq the session really used, exactly
    /// as an adapter would. Nothing is assumed about the order or the numbering.
    written: Option<Vec<Value>>,
    /// The sender the reader threads push onto, kept so a **second** connection can be read onto the
    /// same channel. See [`Client::adopt_child`].
    sender: Sender<Reply>,
    /// The parent's writer, once the client has moved to a child session.
    ///
    /// Kept because the request that asked for the child arrived on the parent and has to be
    /// answered there, and because letting it drop would close a socket the adapter is still using.
    parent: Option<Box<dyn Write + Send>>,
}

/// A pipe and a thread have nothing worth printing, so this says what was started and stops there.
/// It exists because a test asserting that starting failed needs the success arm to be printable.
impl std::fmt::Debug for Client {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "a client talking to {}", self.described)
    }
}

impl Client {
    /// Start an adapter and the thread that reads it.
    ///
    /// The reply is the client, or the reason it could not be started — which the caller puts in the
    /// status bar. **Nothing is invented and nothing is fetched.**
    pub fn start(command: &AdapterCommand, waker: Waker) -> Result<Self, String> {
        let connection = adapter::start(command)?;
        let (sender, receiver) = std::sync::mpsc::channel::<Reply>();
        let kept = sender.clone();
        let reader = connection.reader;
        // The adapter's own standard error, on a thread of its own. It is not the protocol and it
        // never completes a frame, so it cannot share the decoder — but it is where an adapter
        // explains itself, and swallowing that is the one thing `quill-git`'s rule about git's
        // stderr says never to do.
        if let Some(errors) = connection.errors {
            let said = sender.clone();
            let woken = waker.clone();
            std::thread::Builder::new()
                .name("quill-dap-errors".to_owned())
                .spawn(move || read_errors(errors, said, woken))
                .map_err(|problem| {
                    format!("Quill could not start a thread to read the debugger: {problem}")
                })?;
        }
        std::thread::Builder::new()
            .name("quill-dap".to_owned())
            .spawn(move || read_frames(reader, sender, waker))
            .map_err(|problem| format!("Quill could not start a thread to read the debugger: {problem}"))?;
        Ok(Self {
            replies: receiver,
            writer: connection.writer,
            child: connection.child,
            described: command.described(),
            stopping: None,
            broken: false,
            written: None,
            sender: kept,
            parent: None,
        })
    }

    /// Open the child session an adapter asked for, and speak to **that** from now on.
    ///
    /// js-debug puts the program in a session of its own and sends `startDebugging`; the client
    /// dials the same server again, and everything after that — the handshake, the breakpoints, the
    /// stops — happens on the new connection. `task-1692` measured what happens without it: a parent
    /// session with no threads, no stops, and a breakpoint that answers `provisionalBreakpoint` and
    /// never binds.
    ///
    /// The parent's writer is kept rather than dropped, because the request came from there and
    /// closing that socket would take the adapter down with it. Both connections are read onto the
    /// **same channel**, so the window still drains one queue in one place: after the child is open
    /// the parent says nothing but telemetry, which is already an ordinary `output` event.
    ///
    /// Only a server-shaped adapter can do this — there is one socket to dial again, where a stdio
    /// child has only the pipes it was started with — and js-debug is the only adapter here that
    /// asks. So a `program` is never started a second time: `connect` is what this dials with.
    pub fn adopt_child(&mut self, command: &AdapterCommand, waker: Waker) -> Result<(), String> {
        let connection = adapter::connect(command)?;
        let reader = connection.reader;
        let sender = self.sender.clone();
        std::thread::Builder::new()
            .name("quill-dap-child".to_owned())
            .spawn(move || read_child_frames(reader, sender, waker))
            .map_err(|problem| {
                format!("Quill could not start a thread to read the child session: {problem}")
            })?;
        let parent = std::mem::replace(&mut self.writer, connection.writer);
        self.parent = Some(parent);
        self.broken = false;
        Ok(())
    }

    /// Write one frame to the **parent** connection, which is where a reverse request came from.
    ///
    /// Nothing at all when there is no parent, which is every session that never adopted a child:
    /// the ordinary writer is the only one there is, and [`Client::write`] is what answers on it.
    pub fn write_to_parent(&mut self, frame: &Value) -> bool {
        let Some(parent) = self.parent.as_mut() else {
            return self.write(frame);
        };
        let body = codec::encode(frame);
        parent.write_all(&body).and_then(|()| parent.flush()).is_ok()
    }

    /// A client with no adapter behind it, fed messages directly.
    ///
    /// What the screenshot tests use, exactly as `quill_terminal::Session::detached` is what the
    /// terminal's use and for exactly the same reason: **when a real adapter answers is not
    /// something a test can know**, so a picture of a paused debugger is taken of a session that was
    /// handed fixed messages. It runs the same state machine over the same values, so what it draws
    /// is what a real adapter sending those messages would have drawn, and it is the same on every
    /// run because nothing is waited for.
    ///
    /// Writes go nowhere. A detached session still *makes* requests — a stop reads the stack, and
    /// opening a row asks for its children — and the test answers those by feeding the reply in,
    /// which is what the terminal's fixed bytes are.
    pub fn detached() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<Reply>();
        // A second one is kept alive on purpose: a channel with no senders left says the thread has
        // gone, and a detached client has no thread to have gone.
        std::mem::forget(sender.clone());
        Self {
            replies: receiver,
            writer: Box::new(Vec::new()),
            child: None,
            described: "a scripted adapter".to_owned(),
            stopping: None,
            broken: false,
            written: Some(Vec::new()),
            sender,
            parent: None,
        }
    }

    /// Every frame a detached client has been asked to write, and forget them.
    ///
    /// Empty for a real client, which keeps none. See [`Client::detached`].
    pub fn take_written(&mut self) -> Vec<Value> {
        self.written.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// What was started, which is what a message about it names.
    pub fn described(&self) -> &str {
        &self.described
    }

    /// Write one frame. False when the pipe has gone, which the caller reports once.
    pub fn write(&mut self, frame: &Value) -> bool {
        if self.broken {
            return false;
        }
        if let Some(written) = self.written.as_mut() {
            written.push(frame.clone());
            return true;
        }
        if tracing() {
            eprintln!("--> {frame}");
        }
        let bytes = codec::encode(frame);
        if self.writer.write_all(&bytes).is_err() || self.writer.flush().is_err() {
            self.broken = true;
            return false;
        }
        true
    }

    /// Write several, which is what one [`crate::session::Outcome`] usually is.
    pub fn write_all(&mut self, frames: &[Value]) -> bool {
        frames.iter().all(|frame| self.write(frame))
    }

    /// Everything the thread has read since the last time this was called.
    pub fn poll(&mut self) -> Vec<Reply> {
        let mut replies = Vec::new();
        while let Ok(reply) = self.replies.try_recv() {
            replies.push(reply);
        }
        replies
    }

    /// Note that the polite stop has been sent, so [`Client::grace_ran_out`] can say when to stop
    /// waiting for it.
    pub fn stopping_now(&mut self) {
        if self.stopping.is_none() {
            self.stopping = Some(Instant::now());
        }
    }

    /// How long until the grace runs out, or `None` when nothing is waiting.
    ///
    /// The window asks egui to draw again **then** rather than on every frame until then, which is
    /// the run tile's rule: waking sixty times a second for two seconds in order to do one thing at
    /// the end of them is a busy loop a person can hear in the fan.
    pub fn stopping_in(&self) -> Option<Duration> {
        self.stopping.map(|since| GRACE.saturating_sub(since.elapsed()))
    }

    /// True once a polite stop has gone unanswered for [`GRACE`].
    pub fn grace_ran_out(&self) -> bool {
        self.stopping.is_some_and(|since| since.elapsed() >= GRACE)
    }

    /// True while the adapter's own process is still there.
    ///
    /// Asked rather than assumed, because an adapter that crashed and one that is simply slow look
    /// identical from the outside — which is a lesson this repository has already paid for.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            // An adapter Quill did not start is somebody else's process, so its being there is not
            // a question this can answer.
            None => true,
        }
    }

    /// Kill the adapter outright.
    ///
    /// What the grace running out, closing the project and closing the window all do. Nothing ever
    /// orphans a child on purpose.
    pub fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.broken = true;
    }
}

impl Drop for Client {
    /// A session dropped without being stopped still takes its adapter with it.
    fn drop(&mut self) {
        self.kill();
    }
}

/// The adapter's own standard error, a line at a time, as `console` output.
///
/// It ends quietly. A thread that stopped reading because the adapter had gone says nothing: the
/// frame reader is already saying it, and two `Gone` replies for one adapter would end the session
/// twice.
fn read_errors(mut errors: Box<dyn Read + Send>, sender: Sender<Reply>, waker: Waker) {
    let mut buffer = [0u8; 4096];
    loop {
        let read = match errors.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => read,
            Err(problem) if problem.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
        let message = Message::Output { kind: crate::messages::OutputKind::Console, text };
        if sender.send(Reply::Message(Box::new(message))).is_err() {
            return;
        }
        waker();
    }
}

/// The reader thread: bytes in, [`Reply`]s out, and the window woken after each.
fn read_frames(reader: Box<dyn Read + Send>, sender: Sender<Reply>, waker: Waker) {
    read_frames_as(reader, sender, waker, Reply::Message)
}

/// The same, tagging what it reads as the **child** session's.
///
/// Two connections push onto one channel, so the channel has to say which one a message came from:
/// after a child is adopted the parent goes on sending telemetry with a numbering of its own, and
/// feeding that to the child's state machine matches the wrong response to the wrong request. See
/// [`Client::adopt_child`].
fn read_child_frames(reader: Box<dyn Read + Send>, sender: Sender<Reply>, waker: Waker) {
    read_frames_as(reader, sender, waker, Reply::FromChild)
}

fn read_frames_as(
    mut reader: Box<dyn Read + Send>,
    sender: Sender<Reply>,
    waker: Waker,
    wrap: fn(Box<Message>) -> Reply,
) {
    let mut decoder = Decoder::new();
    // Eight kilobytes, which holds every frame an adapter sends but the largest `variables` answer
    // in one read. The decoder does not care how much arrives at once; this is simply less work per
    // frame than a smaller buffer would be.
    let mut buffer = [0u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(problem) if problem.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        match decoder.feed(&buffer[..read]) {
            Ok(messages) => {
                for value in messages {
                    if tracing() {
                        eprintln!("<-- {value}");
                    }
                    let message = Message::read(&value);
                    if sender.send(wrap(Box::new(message))).is_err() {
                        return;
                    }
                }
                waker();
            }
            Err(problem) => {
                let _ = sender.send(Reply::Broken(problem.to_string()));
                waker();
                return;
            }
        }
    }
    let _ = sender.send(Reply::Gone);
    waker();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::Transport;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A pipe with a scripted adapter's bytes already in it, which is the whole of what the reader
    /// thread needs: no process, no port, and the same code path a real adapter takes.
    struct Scripted {
        bytes: Vec<u8>,
        at: usize,
    }

    impl Read for Scripted {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.at >= self.bytes.len() {
                return Ok(0);
            }
            // A few bytes at a time, so the test really exercises a torn frame rather than handing
            // the decoder everything at once.
            let take = out.len().min(7).min(self.bytes.len() - self.at);
            out[..take].copy_from_slice(&self.bytes[self.at..self.at + take]);
            self.at += take;
            Ok(take)
        }
    }

    fn drain(receiver: &Receiver<Reply>) -> Vec<Reply> {
        let mut replies = Vec::new();
        // The thread is doing the reading, so the first one is waited for rather than polled.
        while let Ok(reply) = receiver.recv_timeout(Duration::from_secs(5)) {
            let gone = matches!(reply, Reply::Gone | Reply::Broken(_));
            replies.push(reply);
            if gone {
                break;
            }
        }
        replies
    }

    #[test]
    fn the_reader_turns_a_stream_of_bytes_into_messages_and_then_says_it_ended() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&codec::encode(
            &serde_json::json!({ "seq": 1, "type": "event", "event": "initialized" }),
        ));
        bytes.extend_from_slice(&codec::encode(
            &serde_json::json!({ "seq": 2, "type": "event", "event": "terminated" }),
        ));
        let (sender, receiver) = std::sync::mpsc::channel();
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = woken.clone();
        let waker: Waker = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        read_frames(Box::new(Scripted { bytes, at: 0 }), sender, waker);
        let replies = drain(&receiver);
        assert_eq!(replies.len(), 3, "two messages and the ending");
        assert_eq!(replies[0], Reply::Message(Box::new(Message::Initialized)));
        assert_eq!(replies[1], Reply::Message(Box::new(Message::Terminated)));
        assert_eq!(replies[2], Reply::Gone);
        assert!(woken.load(Ordering::SeqCst) > 0, "the window is woken for what arrived");
    }

    /// An adapter that writes rubbish is a real thing — a crash report on standard output, a Node
    /// warning printed before the server starts. The session ends and says what was seen.
    #[test]
    fn an_adapter_that_writes_something_that_is_not_the_protocol_breaks_the_session() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let waker: Waker = Arc::new(|| {});
        read_frames(
            Box::new(Scripted { bytes: b"Segmentation fault\n".to_vec(), at: 0 }),
            sender,
            waker,
        );
        let replies = drain(&receiver);
        // Nothing complete arrived, so the stream simply ended: there is no `Content-Length` and no
        // separator, so the decoder is still waiting when the bytes run out.
        assert_eq!(replies, vec![Reply::Gone]);
    }

    #[test]
    fn a_frame_with_a_length_that_is_not_a_number_is_reported_rather_than_guessed_at() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let waker: Waker = Arc::new(|| {});
        read_frames(
            Box::new(Scripted { bytes: b"Content-Length: lots\r\n\r\n{}".to_vec(), at: 0 }),
            sender,
            waker,
        );
        let replies = drain(&receiver);
        assert_eq!(replies.len(), 1);
        assert!(matches!(replies[0], Reply::Broken(_)), "{:?}", replies[0]);
    }

    #[test]
    fn an_adapter_that_will_not_start_is_a_message_rather_than_a_client() {
        let waker: Waker = Arc::new(|| {});
        let command = AdapterCommand::stdio("quill-no-such-debug-adapter", Vec::new());
        let problem = Client::start(&command, waker).expect_err("no such program");
        assert!(problem.contains("could not start"), "{problem}");
    }

    /// The grace is the run tile's, reused, so a person pressing stop twice never waits.
    #[test]
    fn the_grace_only_starts_running_once_the_polite_stop_has_been_sent() {
        // A client over a socket nothing answers on cannot be built, so this exercises the timing
        // through the same fields with a client that was built from a scripted pipe.
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut client = Client {
            replies: receiver,
            writer: Box::new(Vec::new()),
            child: None,
            described: "scripted".to_owned(),
            stopping: None,
            broken: false,
            written: None,
            sender,
            parent: None,
        };
        assert!(client.stopping_in().is_none(), "nothing is waiting yet");
        assert!(!client.grace_ran_out());
        client.stopping_now();
        assert!(client.stopping_in().is_some());
        assert!(!client.grace_ran_out(), "two seconds have not passed");
        let first = client.stopping_in();
        client.stopping_now();
        assert!(client.stopping_in() <= first, "a second press does not restart the clock");
    }

    #[test]
    fn a_write_to_a_pipe_that_has_gone_is_false_rather_than_a_panic() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut client = Client {
            replies: receiver,
            writer: Box::new(Broken),
            child: None,
            described: "scripted".to_owned(),
            stopping: None,
            broken: false,
            written: None,
            sender,
            parent: None,
        };
        assert!(!client.write(&serde_json::json!({ "seq": 1 })));
        assert!(!client.write(&serde_json::json!({ "seq": 2 })), "and stays false");
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "gone"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_adapter_quill_did_not_start_is_never_reported_as_dead() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut client = Client {
            replies: receiver,
            writer: Box::new(Vec::new()),
            child: None,
            described: "somebody else's".to_owned(),
            stopping: None,
            broken: false,
            written: None,
            sender,
            parent: None,
        };
        assert!(client.is_running(), "its being there is not a question this can answer");
    }

    #[test]
    fn the_two_transports_are_both_startable_shapes() {
        assert_eq!(AdapterCommand::stdio("a", Vec::new()).transport, Transport::Stdio);
    }
}
