//! The thread a turn runs on: the one place an HTTP request is made, and the one place a
//! command-line agent is started.
//!
//! Arranged as `unluminate_git::Worker`, the terminal's reader loop and `services::text_search` already
//! are: a thread, a channel and a waker. The window must not stop drawing while a model is thinking,
//! and on a long answer that is a minute — which, as `unluminate-git`'s own comment about a fetch says,
//! looks exactly like a crash.
//!
//! ## Only the newest request is answered
//!
//! Each carries a generation number, the newest is shared with the threads as an `AtomicU64`, and a
//! reply from a passed generation is dropped on arrival. That is `services::text_search`'s
//! arrangement and it is what makes "send, change your mind, send again" work with no timer
//! anywhere.
//!
//! ## Stopping is dropping, or killing
//!
//! For an address, a flag is checked between reads; when it is set the thread stops reading, the body
//! is dropped and the connection closes. There is no request to a server to cancel — HTTP has no such
//! thing, and every one of these APIs treats a closed connection as a cancellation.
//!
//! For a program the flag is not enough, because the thread reading one is asleep in a read with no
//! timeout and will not look at it until the agent speaks again. So the child is **killed**, through
//! the `agent::Running` handle this client holds; the read then ends because the pipe has closed.
//! See `agent.rs`.
//!
//! ## The timeouts are two, not one
//!
//! A **connect** timeout, because an address that is not listening should say so in seconds rather
//! than in minutes; and no **global** timeout at all, because an answer legitimately takes minutes
//! and a client that gave up at thirty seconds would be a client that cannot be used with a
//! reasoning model. What bounds a hung connection instead is the receive timeout: a server that has
//! sent nothing for that long has stopped answering, whatever it intended.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::provider::Provider;
use crate::sse;
use crate::wire::{self, Decoder};

pub use crate::wire::Reply;

/// How long to wait for a connection before saying the address is not answering.
const CONNECT: Duration = Duration::from_secs(20);

/// How long a stream may say nothing at all before it is treated as hung.
///
/// Both APIs send a keep-alive comment well inside this while a model is thinking, so a stream that
/// is silent for two minutes is a stream that has stopped rather than one that is slow.
const SILENCE: Duration = Duration::from_secs(120);

/// How much is read from the socket at a time.
///
/// A whole event is usually a few hundred bytes and a picture never comes back down, so this is
/// sized for the common case rather than for throughput.
const CHUNK: usize = 4096;

/// One reply, and which request it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrived {
    pub generation: u64,
    pub reply: Reply,
}

/// The client: the newest generation, the stop flag, and the channel replies come back on.
pub struct Client {
    newest: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
    /// The command-line agent this turn started, so stopping can kill it.
    ///
    /// A flag is enough to stop reading a socket, because the reads are made with a timeout; it is
    /// **not** enough to stop a program, because the thread reading one is asleep in a read that has
    /// no timeout at all. See `agent::Running`.
    running: Arc<crate::agent::Running>,
    to: Sender<Arrived>,
    from: Receiver<Arrived>,
    /// How to ask the window to draw again, once a reply has been pushed.
    ///
    /// `None` in a test, which is what makes every one of these run with no window behind it.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Client {
    /// Written by hand because the waker is a closure and a closure has no `Debug`. What is printed
    /// is what a test wants to see: which request is the current one, and whether a window is behind
    /// it at all.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Client")
            .field("generation", &self.generation())
            .field("wake", &self.wake.is_some())
            .finish()
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let (to, from) = channel();
        Self {
            newest: Arc::new(AtomicU64::new(0)),
            stopping: Arc::new(AtomicBool::new(false)),
            running: Arc::new(crate::agent::Running::default()),
            to,
            from,
            wake: None,
        }
    }

    /// How a reply asks the window for a frame. Handed on from `plugin_ui::Context`.
    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.wake = wake;
    }

    /// Which request is the current one.
    pub fn generation(&self) -> u64 {
        self.newest.load(Ordering::SeqCst)
    }

    /// Send `body` to `provider` and stream the answer back. Answers the new generation.
    ///
    /// Whatever was in flight is abandoned first: its generation is passed and its stop flag is set,
    /// so its thread stops at its next read and nothing it says afterwards is acted on.
    pub fn send(&mut self, provider: &Provider, body: String, stream: bool) -> u64 {
        self.stop();
        let generation = self.newest.fetch_add(1, Ordering::SeqCst) + 1;
        // A flag of its own for this request, so stopping *this* one later cannot also be read by a
        // thread started after it.
        let stopping = Arc::new(AtomicBool::new(false));
        self.stopping = Arc::clone(&stopping);
        let newest = Arc::clone(&self.newest);
        let to = self.to.clone();
        let wake = self.wake.clone();
        let url = provider.url.clone();
        let headers = provider.headers();
        let wire = provider.wire;
        std::thread::Builder::new()
            .name(format!("unluminate-chat {generation}"))
            .spawn(move || {
                let say = |reply: Reply| {
                    // A reply from a request that has been overtaken is dropped at the point it is
                    // made rather than at the point it is read, so an abandoned thread stops costing
                    // the channel anything.
                    if newest.load(Ordering::SeqCst) != generation || stopping.load(Ordering::SeqCst) {
                        return false;
                    }
                    if to.send(Arrived { generation, reply }).is_err() {
                        return false;
                    }
                    if let Some(wake) = &wake {
                        wake();
                    }
                    true
                };
                run(&url, &headers, wire, &body, stream, &stopping, &say);
            })
            .expect("a thread for a chat request");
        generation
    }

    /// Run a command-line agent for one turn and stream what it says back. Answers the generation.
    ///
    /// The other half of [`send`](Self::send), and deliberately a second entry point rather than a
    /// flag on the first: a program takes a prompt, a folder and a session where an endpoint takes a
    /// body, headers and a URL, and one function taking the union of those would be a function whose
    /// arguments are half ignored on every call. Everything **after** the transport is identical —
    /// the same generation, the same stop flag, the same channel, the same `Reply` values — so
    /// nothing above `Client` knows which of the two it asked for.
    pub fn ask(&mut self, provider: &Provider, ask: crate::agent::Ask) -> u64 {
        self.stop();
        let generation = self.newest.fetch_add(1, Ordering::SeqCst) + 1;
        let stopping = Arc::new(AtomicBool::new(false));
        self.stopping = Arc::clone(&stopping);
        let running = Arc::clone(&self.running);
        let newest = Arc::clone(&self.newest);
        let to = self.to.clone();
        let wake = self.wake.clone();
        let provider = provider.clone();
        std::thread::Builder::new()
            .name(format!("unluminate-chat agent {generation}"))
            .spawn(move || {
                let say = |reply: Reply| {
                    // **A session id is dropped with everything else when the turn is overtaken.**
                    // An earlier version kept it, on the grounds that the agent holds that session
                    // whether or not this window waited — but `Session::reply` writes it into
                    // *whichever conversation is current*, so an abandoned turn could attach its
                    // agent session to a conversation that had nothing to do with it and the next
                    // question would resume the wrong thread. The id arrives in the first few bytes
                    // of a turn, before anything can have overtaken it, so nothing real is lost.
                    if newest.load(Ordering::SeqCst) != generation || stopping.load(Ordering::SeqCst)
                    {
                        return false;
                    }
                    if to.send(Arrived { generation, reply }).is_err() {
                        return false;
                    }
                    if let Some(wake) = &wake {
                        wake();
                    }
                    true
                };
                crate::agent::run(&provider, &ask, &stopping, &running, &say);
            })
            .expect("a thread for a chat turn");
        generation
    }

    /// Stop whatever is in flight, keeping what has already arrived.
    ///
    /// The flag stops a socket, which is read with a timeout; the kill stops a program, whose reader
    /// has none and would otherwise sit in a read until the agent said something of its own accord.
    pub fn stop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.running.stop();
    }

    /// Every reply that has arrived since the last time this was asked, newest request only.
    ///
    /// Called once a frame from `UiProvider::catch_up`, which is why it drains rather than blocks.
    pub fn take(&mut self) -> Vec<Reply> {
        let generation = self.generation();
        self.from
            .try_iter()
            .filter(|arrived| arrived.generation == generation)
            .map(|arrived| arrived.reply)
            .collect()
    }
}

/// Which library does the TLS handshake, and which certificates it trusts.
///
/// **The provider is named rather than left to the default, and it is not a preference.** `ureq`'s
/// default TLS provider is Rustls whether or not the feature is on, and asking for an `https` URL
/// with the Rustls feature off **panics inside the transport** — not an error, a panic, on the worker
/// thread. Every request to a hosted API would have ended the request that way, and it took a
/// screenshot test driving a tool round to find it. See `tasks/task-1767-agent-chat-tdd.md` §3.1 for
/// why `native-tls` is what is compiled in: it is schannel on Windows and Security.framework on
/// macOS, so Unluminate trusts what the machine trusts.
///
/// **The roots have to be named too, and for a while that sentence was not true.** Naming
/// `native-tls` says which library does the handshake and says nothing about which certificates it
/// trusts. `ureq`'s default is `RootCerts::WebPki`, which is Mozilla's bundled list, and for
/// `native-tls` that list is installed with `disable_built_in_roots(true)` — so the machine's own
/// store is switched off. A certificate signed by a root an employer put in the System keychain then
/// fails with `native-tls: The certificate was not trusted.`, which is measured rather than reasoned
/// about: an internal Anthropic gateway on a private chain answered `curl` on this machine and
/// refused the pane. `PlatformVerifier` is the whole of the fix, and it is `unluminate-git`'s argument for
/// shelling out to the machine's real git made about a certificate store instead of a credential
/// helper.
///
/// It is a function rather than four lines inside `run` so that a test can read back what was asked
/// for. Neither half can be asserted from a scripted server on loopback, which speaks no TLS at all.
pub fn tls_config() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build()
}

/// Make the request and push what comes back. Runs on the worker thread.
///
/// Split out from the closure so that a test can drive the whole of it — a real socket, a real
/// stream, real framing — against a scripted server with no window and no `Client` behind it.
pub fn run(
    url: &str,
    headers: &[(String, String)],
    wire: crate::provider::Wire,
    body: &str,
    stream: bool,
    stopping: &AtomicBool,
    say: &dyn Fn(Reply) -> bool,
) {
    // **Nothing the server says is shown before the key is taken out of it.** A refusal is quoted
    // verbatim, which is `unluminate-git`'s rule and the right one; but an endpoint that echoes the
    // request back — a debugging proxy, a misconfigured gateway, or one written to do it — would put
    // the key in the pane and then in the transcript on disk, which is the one thing Unluminate promises
    // never to write down. So every header value long enough to be a secret is replaced in anything
    // that comes back.
    let secrets: Vec<String> = headers
        .iter()
        .filter(|(name, _)| name != "content-type" && name != "accept")
        .map(|(_, value)| value.trim_start_matches("Bearer ").to_owned())
        .filter(|value| value.chars().count() >= 8)
        .collect();
    let say = &move |reply: Reply| {
        say(match reply {
            Reply::Failed(said) => Reply::Failed(redacted(&said, &secrets)),
            Reply::Text(said) => Reply::Text(redacted(&said, &secrets)),
            other => other,
        })
    };
    let config = ureq::Agent::config_builder()
        .tls_config(tls_config())
        // **Without this a 401 is an `Err` with no body**, and the body is the whole of what the
        // server had to say — which is what `unluminate-git`'s rule about never inventing an error
        // message requires be shown.
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT))
        // No global timeout: an answer legitimately takes minutes. What bounds a hung connection is
        // the silence below.
        .timeout_global(None)
        .timeout_recv_body(Some(SILENCE))
        // **No redirects, and that is a security decision rather than a preference.** A redirect
        // takes the request somewhere the person did not configure, and `ureq` strips `authorization`
        // when it follows one but has never heard of Anthropic's `x-api-key` — so a redirect to
        // somewhere else would carry the key there. An endpoint that answers with a redirect is not
        // the endpoint that was typed into the Settings page, and the honest thing is to say so.
        .max_redirects(0)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut request = agent.post(url);
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    if stream {
        request = request.header("accept", "text/event-stream");
    }
    let answer = match request.send(body) {
        Ok(answer) => answer,
        Err(problem) => {
            say(Reply::Failed(readable(&problem.to_string(), url)));
            return;
        }
    };
    let status = answer.status().as_u16();
    let mut reader = answer.into_body().into_reader();
    // A failure is read whole rather than streamed: it is short, it is JSON or it is a proxy's HTML
    // page, and either way the useful thing is all of it at once.
    if !(200..300).contains(&status) {
        let mut text = String::new();
        let _ = reader.take(64 * 1024).read_to_string(&mut text);
        say(Reply::Failed(refusal(status, &text)));
        return;
    }
    if !stream {
        let mut text = String::new();
        if let Err(problem) = reader.take(16 * 1024 * 1024).read_to_string(&mut text) {
            say(Reply::Failed(problem.to_string()));
            return;
        }
        for reply in wire::whole(wire, &text) {
            if !say(reply) {
                return;
            }
        }
        return;
    }
    let mut framing = sse::Reader::new();
    let mut decoder = Decoder::new(wire);
    let mut buffer = [0_u8; CHUNK];
    loop {
        if stopping.load(Ordering::SeqCst) {
            // Nothing is said: the session already recorded that somebody stopped it, and a
            // `Finished` from here would overwrite that with an ordinary end.
            return;
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(problem) => {
                say(Reply::Failed(problem.to_string()));
                return;
            }
        };
        // A server that never sends an event boundary would otherwise grow the buffer until the
        // allocator gives up — see `sse::LARGEST_EVENT`.
        if framing.is_overlong() {
            say(Reply::Failed(format!(
                "the server sent more than {} bytes with no end to the event in it.",
                sse::LARGEST_EVENT
            )));
            return;
        }
        for event in framing.feed(&buffer[..read]) {
            for reply in decoder.event(&event) {
                if !say(reply) {
                    return;
                }
            }
        }
    }
    if let Some(event) = framing.finish() {
        for reply in decoder.event(&event) {
            if !say(reply) {
                return;
            }
        }
    }
    for reply in decoder.finish() {
        if !say(reply) {
            return;
        }
    }
}

/// A transport failure, said in a way that names what to do about it.
///
/// `ureq`'s own message is kept — it is the honest one — and a sentence is added only where the
/// message alone would leave somebody guessing, which is the two cases that really happen: nothing
/// is listening, and the name does not resolve. That is `task-1692`'s rule for a missing debug
/// adapter, which carries the command that installs one.
fn readable(problem: &str, url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(rest))
        .unwrap_or(url);
    let lower = problem.to_lowercase();
    if lower.contains("refused") || lower.contains("connect") && lower.contains("timed out") {
        return format!("{problem} — nothing is answering at {host}.");
    }
    if lower.contains("dns") || lower.contains("resolve") || lower.contains("not known") {
        return format!("{problem} — {host} does not resolve from this machine.");
    }
    problem.to_owned()
}

/// A refusal with a status code, in the server's own words.
///
/// The body first, because that is where the reason is; the code after it, because it is what a
/// person searches for. A body that is JSON is unwrapped to the message inside it, and a body that
/// is anything else — a proxy's HTML page — is shown as it is, cut short.
fn refusal(status: u16, body: &str) -> String {
    let said = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| wire::error_message(&value))
        .unwrap_or_else(|| shorten(body));
    match said.trim().is_empty() {
        true => format!("HTTP {status}"),
        false => format!("HTTP {status}: {said}"),
    }
}

/// `text` with every secret in it replaced.
///
/// Whole-value replacement rather than a pattern: what is being looked for is known exactly, so there
/// is nothing to guess at and nothing that can be nearly right. A short value is not looked for at
/// all — `redacted` would otherwise chew a sentence to pieces over a two character key.
fn redacted(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), "\u{2026}");
        }
    }
    out
}

/// The first part of `text`, for a body that is a page rather than a message.
fn shorten(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.chars().count() > 400 {
        true => trimmed.chars().take(400).collect::<String>() + "…",
        false => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Wire;
    use std::io::Write;
    use std::net::TcpListener;

    /// A server that answers one request with fixed bytes, on a port the operating system chose.
    ///
    /// `unluminate-dap`'s scripted adapters with a socket instead of a pipe, and it is what makes "the
    /// whole client, end to end" a unit test: a real connection, a real chunked read, real framing,
    /// and nothing outside this machine. It writes the bytes in small pieces with a pause between
    /// them, because a stream that arrived all at once would not exercise the framing at all.
    fn scripted(status: u16, headers: &str, body: &'static [u8], pieces: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("an address");
        let headers = headers.to_owned();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            // Read the request head so the client is not writing into a socket nobody is reading.
            let mut seen = Vec::new();
            let mut byte = [0_u8; 1];
            while !seen.ends_with(b"\r\n\r\n") {
                match socket.read(&mut byte) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => seen.push(byte[0]),
                }
            }
            let length = String::from_utf8_lossy(&seen)
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .map(str::trim)
                        .map(str::to_owned)
                })
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let mut sent = vec![0_u8; length];
            let _ = socket.read_exact(&mut sent);
            let reason = match status {
                200 => "OK",
                302 => "Found",
                401 => "Unauthorized",
                _ => "Error",
            };
            let _ = write!(
                socket,
                "HTTP/1.1 {status} {reason}\r\n{headers}transfer-encoding: chunked\r\n\r\n"
            );
            for piece in body.chunks(body.len().div_ceil(pieces.max(1))) {
                let _ = write!(socket, "{:x}\r\n", piece.len());
                let _ = socket.write_all(piece);
                let _ = socket.write_all(b"\r\n");
                let _ = socket.flush();
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = socket.write_all(b"0\r\n\r\n");
            let _ = socket.flush();
        });
        format!("http://{address}/v1/chat/completions")
    }

    /// Every reply `run` produces against a scripted server.
    fn against(url: &str, wire: Wire, stream: bool) -> Vec<Reply> {
        let replies = std::sync::Mutex::new(Vec::new());
        let stopping = AtomicBool::new(false);
        run(
            url,
            &[("content-type".to_owned(), "application/json".to_owned())],
            wire,
            "{}",
            stream,
            &stopping,
            &|reply| {
                replies.lock().expect("the replies").push(reply);
                true
            },
        );
        replies.into_inner().expect("the replies")
    }

    const STREAM: &[u8] = b"data: {\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"there\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";

    #[test]
    fn the_whole_client_reads_a_real_streamed_answer_off_a_real_socket() {
        // A real connection, chunked transfer, the stream split across seventeen writes, and the
        // framing and the decoder both driven by whatever the socket happened to deliver.
        let url = scripted(200, "content-type: text/event-stream\r\n", STREAM, 17);
        let replies = against(&url, Wire::OpenAi, true);
        assert_eq!(
            replies[0],
            Reply::Started {
                model: "m".to_owned()
            }
        );
        assert_eq!(replies[1], Reply::Text("Hello ".to_owned()));
        assert_eq!(replies[2], Reply::Text("there".to_owned()));
        assert_eq!(
            replies[3],
            Reply::Finished {
                reason: "stop".to_owned()
            }
        );
        assert_eq!(replies.len(), 4, "{replies:?}");
    }

    #[test]
    fn a_refusal_carries_the_servers_own_words_and_its_status() {
        const REFUSED: &[u8] =
            b"{\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}";
        let url = scripted(401, "content-type: application/json\r\n", REFUSED, 1);
        let replies = against(&url, Wire::Anthropic, true);
        assert_eq!(replies.len(), 1);
        assert_eq!(
            replies[0],
            Reply::Failed("HTTP 401: authentication_error: invalid x-api-key".to_owned())
        );
    }

    #[test]
    fn a_proxys_html_page_is_shown_as_it_is_rather_than_swallowed() {
        const PAGE: &[u8] = b"<html><head><title>502 Bad Gateway</title></head></html>";
        let url = scripted(502, "content-type: text/html\r\n", PAGE, 2);
        let replies = against(&url, Wire::OpenAi, true);
        assert!(
            matches!(&replies[0], Reply::Failed(said) if said.starts_with("HTTP 502: <html>")),
            "{replies:?}"
        );
    }

    #[test]
    fn an_unstreamed_answer_goes_down_the_same_path() {
        const WHOLE: &[u8] =
            b"{\"model\":\"m\",\"choices\":[{\"message\":{\"content\":\"Hi\"},\"finish_reason\":\"stop\"}]}";
        let url = scripted(200, "content-type: application/json\r\n", WHOLE, 3);
        let replies = against(&url, Wire::OpenAi, false);
        assert_eq!(replies[1], Reply::Text("Hi".to_owned()));
        assert_eq!(
            replies[2],
            Reply::Finished {
                reason: "stop".to_owned()
            }
        );
    }

    #[test]
    fn nothing_listening_says_so_rather_than_printing_an_error_number() {
        // A port nothing is bound to, which is what a mistyped URL really looks like.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("an address");
        drop(listener);
        let replies = against(
            &format!("http://{address}/v1/chat/completions"),
            Wire::OpenAi,
            true,
        );
        assert_eq!(replies.len(), 1);
        let Reply::Failed(said) = &replies[0] else {
            panic!("{replies:?}");
        };
        assert!(said.contains(&address.to_string()), "{said}");
        assert!(said.contains("nothing is answering"), "{said}");
    }

    #[test]
    fn a_redirect_is_refused_rather_than_followed() {
        // **A redirect takes the request somewhere the person did not configure**, and `ureq` strips
        // `authorization` when it follows one but has never heard of Anthropic's `x-api-key` — so a
        // redirect to somewhere else would carry the key there. An endpoint that answers with one is
        // not the endpoint that was typed into the Settings page.
        let url = scripted(302, "location: http://example.invalid/v1/messages\r\n", b"", 1);
        let replies = against(&url, Wire::Anthropic, true);
        assert_eq!(replies.len(), 1);
        let Reply::Failed(said) = &replies[0] else {
            panic!("{replies:?}");
        };
        assert!(
            said.contains("302"),
            "the refusal says what the server answered: {said}"
        );
    }

    #[test]
    fn a_key_the_server_echoes_back_never_reaches_the_pane() {
        // A refusal is quoted verbatim, which is the right rule; but a gateway that echoes the request
        // back would put the key in the conversation and then in the transcript on disk, which is the
        // one thing Unluminate promises never to write down.
        const ECHOED: &[u8] = b"{\"error\":{\"message\":\"bad key sk-secret-value-1234 rejected\"}}";
        let url = scripted(401, "content-type: application/json\r\n", ECHOED, 1);
        let replies = std::sync::Mutex::new(Vec::new());
        let stopping = AtomicBool::new(false);
        run(
            &url,
            &[
                ("content-type".to_owned(), "application/json".to_owned()),
                ("x-api-key".to_owned(), "sk-secret-value-1234".to_owned()),
            ],
            Wire::Anthropic,
            "{}",
            true,
            &stopping,
            &|reply| {
                replies.lock().expect("replies").push(reply);
                true
            },
        );
        let said = replies.into_inner().expect("replies");
        let Reply::Failed(message) = &said[0] else {
            panic!("{said:?}");
        };
        assert!(
            !message.contains("sk-secret-value-1234"),
            "the key came back: {message}"
        );
        assert!(
            message.contains("bad key"),
            "the rest of what the server said is kept: {message}"
        );
        // And `content-type` is not treated as a secret, or every message would lose the words
        // `application/json` out of the middle of it.
        assert!(!message.contains("\u{2026}/json"));
    }

    #[test]
    fn a_stream_that_never_frames_an_event_is_stopped_rather_than_growing_for_ever() {
        // A server can otherwise grow the buffer until the allocator gives up, which ends the process.
        // Half a megabyte of `x` with no blank line in it, fed until the ceiling is passed.
        let mut framing = sse::Reader::new();
        let block = vec![b'x'; 64 * 1024];
        let mut fed = 0;
        while !framing.is_overlong() {
            framing.feed(&block);
            fed += block.len();
            assert!(fed < sse::LARGEST_EVENT * 2, "the ceiling was never reached");
        }
        assert!(fed > sse::LARGEST_EVENT);
    }

    #[test]
    fn only_the_newest_request_is_answered() {
        // Send, change your mind, send again — with no timer anywhere, which is
        // `services::text_search`'s own arrangement.
        let mut client = Client::new();
        let first = scripted(200, "content-type: text/event-stream\r\n", STREAM, 40);
        let second = scripted(200, "content-type: text/event-stream\r\n", STREAM, 2);
        let provider = crate::provider::Provider::defaults()[2].clone();
        let mut one = provider.clone();
        one.url = first;
        let mut two = provider;
        two.url = second;
        let older = client.send(&one, "{}".to_owned(), true);
        let newer = client.send(&two, "{}".to_owned(), true);
        assert!(newer > older);
        // Wait for the second to finish, then take: nothing from the first can be in what comes out.
        let mut said = Vec::new();
        for _ in 0..400 {
            said.extend(client.take());
            if said.iter().any(|reply| matches!(reply, Reply::Finished { .. })) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(client.generation(), newer);
        assert!(
            said.iter().any(|reply| matches!(reply, Reply::Finished { .. })),
            "{said:?}"
        );
        // Whatever the first thread manages to say is filtered out on arrival.
        std::thread::sleep(Duration::from_millis(150));
        let late = client.take();
        assert!(
            !late.iter().any(|reply| matches!(reply, Reply::Started { .. })),
            "{late:?}"
        );
    }

    #[test]
    fn an_https_address_is_refused_rather_than_panicking_inside_the_transport() {
        // `ureq`'s default TLS provider is Rustls whether or not that feature is on, and an `https`
        // request made with it off **panics** rather than failing — on the worker thread, where the
        // whole answer is lost with it. Every request to a hosted API went that way until the provider
        // was named. A closed port on loopback, so this leaves the machine no more than the tests
        // above do.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("an address");
        drop(listener);
        let replies = against(&format!("https://{address}/v1/messages"), Wire::Anthropic, true);
        assert_eq!(replies.len(), 1);
        assert!(matches!(&replies[0], Reply::Failed(_)), "{replies:?}");
    }

    #[test]
    fn the_certificates_trusted_are_the_machine_s_own_rather_than_a_bundled_list() {
        // `ureq`'s default is `RootCerts::WebPki`, which for `native-tls` means Mozilla's list
        // installed with the machine's own store switched off — so a certificate signed by a root an
        // employer put in the System keychain came back as `The certificate was not trusted.` while
        // `curl` on the same machine reached the same address. There is nothing on loopback that can
        // show that, because a scripted server here speaks no TLS, so what is asserted is what was
        // asked for.
        let config = tls_config();
        assert!(
            matches!(config.provider(), ureq::tls::TlsProvider::NativeTls),
            "{:?}",
            config.provider()
        );
        assert!(
            matches!(config.root_certs(), ureq::tls::RootCerts::PlatformVerifier),
            "{:?}",
            config.root_certs()
        );
    }

    #[test]
    fn stopping_ends_the_thread_without_ending_the_turn() {
        // Nothing is said on the way out: the session has already recorded that somebody stopped
        // it, and a `Finished` from the thread would overwrite that with an ordinary end.
        let stopping = AtomicBool::new(true);
        let url = scripted(200, "content-type: text/event-stream\r\n", STREAM, 40);
        let replies = std::sync::Mutex::new(Vec::new());
        run(&url, &[], Wire::OpenAi, "{}", true, &stopping, &|reply| {
            replies.lock().expect("replies").push(reply);
            true
        });
        let said = replies.into_inner().expect("replies");
        assert!(
            !said.iter().any(|reply| matches!(reply, Reply::Finished { .. })),
            "{said:?}"
        );
    }
}
