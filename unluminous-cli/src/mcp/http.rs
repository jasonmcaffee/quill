//! The Streamable HTTP transport: one endpoint at `/mcp`, on the loopback interface.
//!
//! For an agent that would rather have a URL than a program to launch, and for the endpoint a
//! running Unluminous hosts when `mcp.enabled` is on. `POST` carries a JSON-RPC message and gets one
//! back; there is nothing else.
//!
//! ## What it does and does not implement, and why that is the whole specification
//!
//! Streamable HTTP allows a server to answer a POST with `application/json` **or** with an SSE
//! stream, and allows a `GET` to open a stream the server pushes down. Both of those are for a
//! server with something to say unprompted. Unluminous has nothing: every answer is the answer to a
//! question just asked, and the one command that takes a while — `terminal read --wait-for` —
//! already holds its own connection open with its own timeout. So a POST is answered with one JSON
//! object and a `GET` is answered `405`, which the specification names as the correct answer for a
//! server that does not offer a stream.
//!
//! No session id is ever issued, so there is nothing for `DELETE` to end and it is `405` too. That
//! is also what makes this server stateless in the sense `2026-07-28` means, and it is why the same
//! endpoint serves a client of either revision.
//!
//! ## Why it is written out rather than depended on
//!
//! The same reason `services::control` gives for its own socket: what is needed is to read a
//! `Content-Length`, read that many bytes, and write a response with one. That is one piece of code
//! on both platforms, and a framework would be a dependency to paper over thirty lines.
//!
//! ## What defends it
//!
//! It is bound to `127.0.0.1` and never to anything else, with a test — the same assertion
//! `services::control` carries about its own listener.
//!
//! The thing a local port genuinely has to defend against on a desktop is **a page in a browser**,
//! which can post to loopback. The control channel answers that with a token in a file, because a
//! page can post and cannot read a file. This endpoint cannot use a token — the configuration an
//! agent copies would have to carry it — so it uses the defence the specification mandates for
//! exactly this attack: a request whose `Origin` is not loopback is refused, and so is one whose
//! `Sec-Fetch-Site` says it came from another site. A page cannot set either header, and a browser
//! attaches `Origin` to every cross-origin POST, so that closes it. Both have tests.
//!
//! What it does not defend against is another program running as you. Nothing on a desktop does,
//! and anything that can reach this port can `terminal send`, which is to say run a shell command.
//! That is why the port is off until somebody turns it on and why stdio is the recommended way in.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::mcp::server::{Driver, Server};

/// The one path. A single endpoint is what the specification asks for.
pub const PATH: &str = "/mcp";

/// The largest message that will be read. A tool call is a few hundred bytes; a megabyte is a
/// mistake or a probe, and reading it would be the whole defence against one.
const LARGEST: usize = 1024 * 1024;

/// How long a connection may take to say what it wants.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How often a listener that is being shut down notices.
const POLL: Duration = Duration::from_millis(25);

/// A running endpoint. Dropping it stops the listener, and **waits for it to stop**.
pub struct Endpoint {
    port: u16,
    running: Arc<AtomicBool>,
    /// The accept loop, kept so it can be waited for. See [`Endpoint::drop`].
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Endpoint {
    /// Start listening. `Err` says why not, which is nearly always that the port is already held —
    /// usually by the Unluminous in the next window, which is not an error so much as a fact the window
    /// should report.
    pub fn start<D: Driver + Send + Sync + 'static>(
        port: u16,
        server: Server<D>,
    ) -> std::io::Result<Self> {
        let listener = bind(port)?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let server = Arc::new(server);
        let thread = std::thread::Builder::new()
            .name("unluminous-mcp-http".to_owned())
            .spawn(move || accept(listener, flag, server))?;
        Ok(Endpoint { port, running, thread: Some(thread) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// True while the thread is still taking connections. False the moment it has been asked to
    /// stop, which is what the window's status line reads.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for Endpoint {
    /// Stop taking connections, and wait until the port is genuinely free.
    ///
    /// The flag is what the accept loop reads — the listener is non-blocking and polls, so it
    /// notices within one [`POLL`] and there is no self-connect trick.
    ///
    /// **Then it waits for that thread**, and the waiting is the point rather than tidiness. The
    /// listener is owned by the thread, so setting the flag only *asks*: the socket is released
    /// when the loop returns. Without the join, changing `mcp.port` or `mcp.tools` dropped the old
    /// endpoint and started a new one on the same port in the same breath, and the bind lost the
    /// race against a socket that was still open — measured on a real window, which reported the
    /// port as taken by another Unluminous when the other Unluminous was itself a moment ago. One poll is the
    /// most this costs, and it buys "the port is free" being true rather than likely.
    ///
    /// Only the accept loop is waited for. A connection already being served has a thread of its
    /// own and holds no listener, so a command still waiting on the window is left to finish.
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Open the listener.
///
/// Loopback only. Split out so a test can assert on the address without starting a thread: an MCP
/// endpoint reachable from the network would be an editor — and a shell — anybody could drive, and
/// that is the one thing here that must never quietly change.
fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

fn accept<D: Driver + Send + Sync + 'static>(
    listener: TcpListener,
    running: Arc<AtomicBool>,
    server: Arc<Server<D>>,
) {
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let server = server.clone();
                // A thread each, because a command may be asked to wait and one connection waiting
                // must not stop the next one being read — the rule `services::control` already
                // keeps for the same reason.
                let _ = std::thread::Builder::new()
                    .name("unluminous-mcp-connection".to_owned())
                    .spawn(move || serve(stream, &server));
            }
            Err(problem) if problem.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL)
            }
            Err(_) => std::thread::sleep(POLL),
        }
    }
}

/// One connection: read a request, answer it, close.
fn serve<D: Driver>(stream: TcpStream, server: &Server<D>) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    // One handle for the whole conversation, read through a `BufReader` and taken back out to write
    // through — the same arrangement, and for the same reason, as `services::control::serve`: a
    // reading handle closed while the caller is still waiting is a reply that arrives as a reset
    // connection on Windows.
    let mut reading = BufReader::new(stream);
    let answer = match read_request(&mut reading) {
        Ok(request) => respond(&request, server),
        Err(response) => response,
    };
    let mut writing = reading.into_inner();
    let _ = writing.write_all(answer.as_bytes());
    let _ = writing.flush();
}

/// What arrived.
pub struct Incoming {
    pub method: String,
    pub path: String,
    pub origin: Option<String>,
    pub fetch_site: Option<String>,
    pub protocol_version: Option<String>,
    pub body: String,
}

/// Read the request line, the headers and the body.
fn read_request(reading: &mut BufReader<TcpStream>) -> Result<Incoming, String> {
    let mut line = String::new();
    if reading.read_line(&mut line).is_err() || line.trim().is_empty() {
        return Err(response(400, "text/plain", "Nothing was sent."));
    }
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or_default().to_owned();
    let path = words.next().unwrap_or_default().to_owned();
    let mut length = 0usize;
    let mut origin = None;
    let mut fetch_site = None;
    let mut protocol_version = None;
    loop {
        let mut header = String::new();
        if reading.read_line(&mut header).is_err() {
            return Err(response(400, "text/plain", "The headers ended early."));
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim().to_owned();
        match name.trim().to_lowercase().as_str() {
            "content-length" => length = value.parse().unwrap_or(0),
            "origin" => origin = Some(value),
            "sec-fetch-site" => fetch_site = Some(value),
            "mcp-protocol-version" => protocol_version = Some(value),
            _ => {}
        }
    }
    if length > LARGEST {
        return Err(response(413, "text/plain", "That message is too large."));
    }
    let mut body = vec![0u8; length];
    if length > 0 && reading.read_exact(&mut body).is_err() {
        return Err(response(400, "text/plain", "The body ended early."));
    }
    Ok(Incoming {
        method,
        path,
        origin,
        fetch_site,
        protocol_version,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// Turn a request into the bytes of a response. Split from the socket so it is all testable.
pub fn respond<D: Driver>(request: &Incoming, server: &Server<D>) -> String {
    if let Some(refusal) = refuse_a_browser(request) {
        return refusal;
    }
    if request.path.split('?').next().unwrap_or_default() != PATH {
        return response(404, "text/plain", &format!("Unluminous's MCP server is at {PATH}."));
    }
    match request.method.to_uppercase().as_str() {
        // A stream the server pushes down, and a session to end. There is neither, and 405 is the
        // answer the specification names for both.
        "GET" => response(405, "text/plain", "This endpoint does not offer a stream."),
        "DELETE" => response(405, "text/plain", "This endpoint has no sessions to end."),
        "OPTIONS" => response(204, "text/plain", ""),
        "POST" => post(request, server),
        other => response(405, "text/plain", &format!("{other} is not a method this endpoint has.")),
    }
}

fn post<D: Driver>(request: &Incoming, server: &Server<D>) -> String {
    if let Some(named) = &request.protocol_version {
        // The specification asks for 400 on a version the server does not support. It is checked by
        // shape rather than against a list, because refusing a revision that has not been published
        // yet would be this endpoint breaking on the day the specification moves.
        if !crate::mcp::server::looks_like_a_version(named) {
            return response(
                400,
                "text/plain",
                &format!("`{named}` is not a protocol version."),
            );
        }
    }
    let message: Value = match serde_json::from_str(request.body.trim()) {
        Ok(message) => message,
        Err(problem) => {
            return json_response(
                400,
                &serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("That was not JSON: {problem}") },
                }),
            )
        }
    };
    if let Value::Array(messages) = &message {
        let answers: Vec<Value> = messages.iter().filter_map(|one| server.answer(one)).collect();
        return match answers.is_empty() {
            // Every message in the batch was a notification, so there is nothing to send back.
            true => response(202, "text/plain", ""),
            false => json_response(200, &Value::Array(answers)),
        };
    }
    match server.answer(&message) {
        Some(answer) => json_response(200, &answer),
        // A notification or a response. The specification asks for 202 and no body.
        None => response(202, "text/plain", ""),
    }
}

/// Refuse anything a page in a browser could have sent.
///
/// Returns the refusal, or nothing when the request may go on. See the module comment for why this
/// is the whole defence and what it is a defence against.
pub fn refuse_a_browser(request: &Incoming) -> Option<String> {
    if let Some(origin) = &request.origin {
        if !is_loopback_origin(origin) {
            return Some(response(
                403,
                "text/plain",
                "Unluminous's MCP endpoint only answers requests from this machine.",
            ));
        }
    }
    match request.fetch_site.as_deref() {
        // A browser sets this on every request it makes. `same-origin` and `none` are a page served
        // from this endpoint and an address typed into the bar; anything else came from a site.
        Some("same-origin") | Some("none") | None => None,
        Some(_) => Some(response(
            403,
            "text/plain",
            "Unluminous's MCP endpoint does not answer requests made by a web page.",
        )),
    }
}

/// True of `http://127.0.0.1:1234`, `http://localhost`, and nothing else.
fn is_loopback_origin(origin: &str) -> bool {
    let origin = origin.trim();
    let Some(host) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        // `null`, `file://`, and anything else that is not an http origin at all.
        return false;
    };
    let host = host.split('/').next().unwrap_or_default();
    let name = host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

fn json_response(status: u16, value: &Value) -> String {
    response(status, "application/json", &value.to_string())
}

fn response(status: u16, kind: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {kind}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Command;
    use crate::mcp::server::Failure;
    use crate::mcp::tools::Shape;
    use crate::protocol::Reply;
    use serde_json::{json, Map};

    struct Nothing;

    impl Driver for Nothing {
        fn run(
            &self,
            command: &'static Command,
            _arguments: Map<String, Value>,
            _instance: Option<&str>,
        ) -> Result<Reply, Failure> {
            Ok(Reply::done(&command.wire(), "Done", Value::Null))
        }
    }

    fn a_server() -> Server<Nothing> {
        Server::new(Shape::Grouped, Nothing)
    }

    fn asked(method: &str, path: &str, body: &str) -> Incoming {
        Incoming {
            method: method.to_owned(),
            path: path.to_owned(),
            origin: None,
            fetch_site: None,
            protocol_version: None,
            body: body.to_owned(),
        }
    }

    fn status_of(response: &str) -> u16 {
        response
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("a status")
    }

    fn body_of(response: &str) -> &str {
        response.split_once("\r\n\r\n").expect("a body").1
    }

    #[test]
    fn it_listens_on_the_loopback_interface_and_nowhere_else() {
        // The one thing that must never change.
        let listener = bind(0).expect("bind");
        let address = listener.local_addr().expect("an address");
        assert!(address.ip().is_loopback(), "bound to {address}, which is not the loopback");
    }

    #[test]
    fn a_posted_message_is_answered_with_one_json_object() {
        let back = respond(
            &asked("POST", PATH, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
            &a_server(),
        );
        assert_eq!(status_of(&back), 200);
        assert!(back.contains("Content-Type: application/json"), "{back}");
        let parsed: Value = serde_json::from_str(body_of(&back)).expect("json");
        assert!(!parsed["result"]["tools"].as_array().expect("tools").is_empty());
    }

    #[test]
    fn a_notification_is_accepted_with_no_body() {
        let back = respond(
            &asked("POST", PATH, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            &a_server(),
        );
        assert_eq!(status_of(&back), 202);
        assert_eq!(body_of(&back), "");
    }

    #[test]
    fn there_is_no_stream_and_no_session_and_it_says_so_the_way_the_specification_asks() {
        assert_eq!(status_of(&respond(&asked("GET", PATH, ""), &a_server())), 405);
        assert_eq!(status_of(&respond(&asked("DELETE", PATH, ""), &a_server())), 405);
    }

    #[test]
    fn anything_that_is_not_the_endpoint_is_not_found() {
        assert_eq!(status_of(&respond(&asked("POST", "/", ""), &a_server())), 404);
        // A query string is not part of the path.
        assert_eq!(
            status_of(&respond(
                &asked("POST", "/mcp?x=1", r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#),
                &a_server()
            )),
            200
        );
    }

    #[test]
    fn a_page_on_another_site_is_refused_and_a_local_client_is_not() {
        let mut request = asked("POST", PATH, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        request.origin = Some("https://evil.example".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 403);

        request.origin = Some("null".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 403);

        request.origin = Some("http://127.0.0.1:7345".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 200);

        request.origin = Some("http://localhost".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 200);
    }

    #[test]
    fn a_request_a_browser_made_across_sites_is_refused_even_with_no_origin() {
        let mut request = asked("POST", PATH, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        request.fetch_site = Some("cross-site".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 403);

        request.fetch_site = Some("same-origin".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 200);
    }

    #[test]
    fn a_protocol_version_is_taken_by_its_shape_so_a_newer_client_still_works() {
        let mut request = asked("POST", PATH, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        for named in ["2025-06-18", "2026-07-28", "2099-12-31"] {
            request.protocol_version = Some(named.to_owned());
            assert_eq!(status_of(&respond(&request, &a_server())), 200, "asked for {named}");
        }
        request.protocol_version = Some("whatever".to_owned());
        assert_eq!(status_of(&respond(&request, &a_server())), 400);
    }

    #[test]
    fn a_body_that_is_not_json_is_answered_rather_than_dropped() {
        let back = respond(&asked("POST", PATH, "not json"), &a_server());
        assert_eq!(status_of(&back), 400);
        let parsed: Value = serde_json::from_str(body_of(&back)).expect("json");
        assert_eq!(parsed["error"]["code"], json!(-32700));
    }

    #[test]
    fn an_endpoint_can_be_restarted_on_the_same_port_at_once() {
        // What changing `mcp.port` or `mcp.tools` does: drop the old one, start a new one, same
        // breath. It failed on a real window before `Endpoint::drop` waited for its thread.
        let mut endpoint = Endpoint::start(0, a_server()).expect("it starts");
        let port = endpoint.port();
        for round in 0..5 {
            drop(endpoint);
            endpoint = Endpoint::start(port, a_server())
                .unwrap_or_else(|problem| panic!("round {round} could not take {port} back: {problem}"));
            assert_eq!(endpoint.port(), port);
        }
    }

    #[test]
    fn a_real_client_can_post_to_a_real_listener_and_dropping_it_stops_the_thread() {
        let endpoint = Endpoint::start(0, a_server()).expect("it starts");
        let port = endpoint.port();
        assert!(endpoint.is_running());
        let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .expect("connect");
        let body = r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#;
        write!(
            stream,
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .expect("write");
        stream.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
        let mut back = String::new();
        stream.read_to_string(&mut back).expect("read");
        assert_eq!(status_of(&back), 200, "{back}");
        let parsed: Value = serde_json::from_str(body_of(&back)).expect("json");
        assert_eq!(parsed["id"], json!(7));

        // Dropping waits for the accept loop, so the port is free **by the time drop returns** and
        // not merely soon afterwards. Asserted with no sleep and no retry, because that is the
        // whole difference: changing the port or the tool shape drops the old endpoint and starts
        // a new one on the same port in the next statement, and "soon" is not good enough for that.
        drop(endpoint);
        assert!(bind(port).is_ok(), "the port should be free the moment the endpoint is dropped");
    }
}
