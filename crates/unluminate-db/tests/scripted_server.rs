//! The whole PostgreSQL client, end to end, against a server made of fixed bytes.
//!
//! This is `unluminate-dap`'s scripted adapters with a socket instead of a pipe, and `unluminate-chat`'s
//! scripted server with a different protocol on it. A `TcpListener` on `127.0.0.1:0` speaks just
//! enough of the wire to be a server, on a thread of its own, and the real `Session` connects to it —
//! so "connect, authenticate, run a statement, read the rows" is a unit test with no PostgreSQL
//! installed and nothing about the machine it runs on.
//!
//! **The SCRAM half is verified rather than replayed**, which is the part worth knowing about. The
//! client's nonce is random, so there is no recording to play back; instead the test server does what
//! a real server does — salts the password, and checks the client's proof by the *inverse* of the way
//! the client made it, recovering `ClientKey` from the proof and hashing it to see whether it is the
//! `StoredKey`. A client that computed a proof any other way fails here. The arithmetic itself is
//! separately pinned against RFC 7677's own published vector, in `scram.rs`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

use unluminate_db::postgres::scram::pbkdf2_sha256;
use unluminate_db::postgres::wire::Out;
use unluminate_db::postgres::Session;
use unluminate_db::source::{Source, SslMode};
use unluminate_db::value::Value;

/// What the scripted server should do after authenticating.
enum Script {
    /// Answer every `Query` with these bytes, in order, one per statement.
    Answers(Vec<Vec<u8>>),
    /// Refuse the request to encrypt, and go no further.
    RefuseTls,
}

/// A server on a port the operating system chose.
struct Scripted {
    port: u16,
    thread: Option<JoinHandle<()>>,
}

impl Scripted {
    /// Start one. `password` is what it will authenticate against; `None` means it answers
    /// `AuthenticationOk` at once, which is what a `trust` line in `pg_hba.conf` does.
    fn start(password: Option<&'static str>, script: Script) -> Scripted {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("an address").port();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut held = Vec::new();
            // The first thing a client sends is either `SSLRequest` or the startup message, and both
            // are untagged: a four-byte length and then a code.
            let first = read_untagged(&mut stream, &mut held);
            let mut startup = first;
            if is_ssl_request(&startup) {
                let refusing = matches!(script, Script::RefuseTls);
                // `N` is "no, carry on in the clear", which is what a server with no TLS built in
                // says. This test server has none either way.
                stream.write_all(b"N").expect("written");
                if refusing {
                    return;
                }
                startup = read_untagged(&mut stream, &mut held);
            }
            assert!(!startup.is_empty(), "a startup message");
            if let Some(password) = password {
                if !authenticate(&mut stream, &mut held, password) {
                    return;
                }
            }
            stream.write_all(&Out::tagged(b'R').int32(0).finish()).expect("written");
            stream
                .write_all(&Out::tagged(b'S').string("server_version").string("17.2").finish())
                .expect("written");
            stream.write_all(&Out::tagged(b'K').int32(4242).int32(9999).finish()).expect("written");
            stream.write_all(&Out::tagged(b'Z').bytes(b"I").finish()).expect("written");
            let Script::Answers(answers) = script else { return };
            for answer in answers {
                // Wait for the client's statement, whatever it is, then answer with the bytes this
                // step was given. A `Sync` (`S`) at the end of an extended query counts as the end
                // of the statement too.
                if read_a_statement(&mut stream, &mut held).is_none() {
                    return;
                }
                if stream.write_all(&answer).is_err() {
                    return;
                }
            }
        });
        Scripted { port, thread: Some(thread) }
    }

    /// A data source pointed at it.
    fn source(&self) -> Source {
        Source {
            name: "scripted".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: self.port,
            database: "test".to_owned(),
            user: "me".to_owned(),
            ssl: SslMode::Prefer,
            ..Source::default()
        }
    }
}

impl Drop for Scripted {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The server's half of a SCRAM exchange, done properly.
///
/// Answers whether the client's proof verified. A proof that does not is refused with an
/// `ErrorResponse` carrying PostgreSQL's own `28P01`, which is what a real server sends — so the
/// client is tested against a refusal rather than against a socket that went quiet.
fn authenticate(stream: &mut TcpStream, held: &mut Vec<u8>, password: &str) -> bool {
    use base64::Engine as _;
    let base64 = base64::engine::general_purpose::STANDARD;
    stream
        .write_all(&Out::tagged(b'R').int32(10).string("SCRAM-SHA-256").bytes(&[0]).finish())
        .expect("written");
    let (tag, body) = read_tagged(stream, held).expect("the client's first SCRAM message");
    assert_eq!(tag, b'p');
    // `SASLInitialResponse`: the mechanism, a length, and the message.
    let text = String::from_utf8_lossy(&body).into_owned();
    let client_first = text[text.find("n,,").expect("a GS2 header")..].to_owned();
    let client_first_bare = client_first.trim_start_matches("n,,").to_owned();
    let client_nonce = field(&client_first_bare, 'r');

    let salt = b"a-fixed-salt-for-a-test!";
    let iterations = 4096_u32;
    let server_nonce = format!("{client_nonce}serverpart");
    let server_first =
        format!("r={server_nonce},s={},i={iterations}", base64.encode(salt));
    stream
        .write_all(&Out::tagged(b'R').int32(11).bytes(server_first.as_bytes()).finish())
        .expect("written");

    let (tag, body) = read_tagged(stream, held).expect("the client's final SCRAM message");
    assert_eq!(tag, b'p');
    let client_final = String::from_utf8_lossy(&body).into_owned();
    let without_proof = client_final[..client_final.find(",p=").expect("a proof")].to_owned();
    let proof = base64.decode(field(&client_final, 'p')).expect("base64");

    let salted = pbkdf2_sha256(password.as_bytes(), salt, iterations);
    let client_key = hmac(&salted, b"Client Key");
    let stored_key: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(client_key).into();
    let auth_message = format!("{client_first_bare},{server_first},{without_proof}");
    let client_signature = hmac(&stored_key, auth_message.as_bytes());

    // **The inverse of the way the client made it.** `ClientProof = ClientKey XOR ClientSignature`,
    // so recovering `ClientKey` and hashing it has to give back the `StoredKey` — which is exactly
    // what a real server checks and is what makes this a test of the client rather than a replay.
    let recovered: Vec<u8> =
        proof.iter().zip(client_signature.iter()).map(|(proof, signature)| proof ^ signature).collect();
    let hashed: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(&recovered).into();
    if hashed != stored_key {
        let refusal = Out::tagged(b'E')
            .bytes(b"S")
            .string("FATAL")
            .bytes(b"C")
            .string("28P01")
            .bytes(b"M")
            .string("password authentication failed for user \"me\"")
            .bytes(&[0])
            .finish();
        let _ = stream.write_all(&refusal);
        return false;
    }

    let server_key = hmac(&salted, b"Server Key");
    let server_signature = hmac(&server_key, auth_message.as_bytes());
    let server_final = format!("v={}", base64.encode(server_signature));
    stream
        .write_all(&Out::tagged(b'R').int32(12).bytes(server_final.as_bytes()).finish())
        .expect("written");
    true
}

fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    use hmac::Mac;
    let mut mac = <hmac::Hmac<sha2::Sha256> as hmac::Mac>::new_from_slice(key).expect("a key");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

fn field(message: &str, key: char) -> String {
    message
        .split(',')
        .find_map(|part| part.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        .unwrap_or_default()
        .to_owned()
}

/// Read until `held` has at least `want` bytes.
fn fill(stream: &mut TcpStream, held: &mut Vec<u8>, want: usize) -> bool {
    while held.len() < want {
        let mut buffer = [0_u8; 4096];
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return false,
            Ok(read) => held.extend_from_slice(&buffer[..read]),
        }
    }
    true
}

/// One of the three untagged client messages.
fn read_untagged(stream: &mut TcpStream, held: &mut Vec<u8>) -> Vec<u8> {
    if !fill(stream, held, 4) {
        return Vec::new();
    }
    let length = u32::from_be_bytes([held[0], held[1], held[2], held[3]]) as usize;
    if !fill(stream, held, length) {
        return Vec::new();
    }
    held.drain(..length).collect()
}

fn is_ssl_request(message: &[u8]) -> bool {
    message.len() == 8 && i32::from_be_bytes([message[4], message[5], message[6], message[7]]) == 80_877_103
}

/// One tagged client message: its tag and its body.
fn read_tagged(stream: &mut TcpStream, held: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    if !fill(stream, held, 5) {
        return None;
    }
    let tag = held[0];
    let length = u32::from_be_bytes([held[1], held[2], held[3], held[4]]) as usize;
    if !fill(stream, held, 1 + length) {
        return None;
    }
    let whole: Vec<u8> = held.drain(..1 + length).collect();
    Some((tag, whole[5..].to_vec()))
}

/// Read whatever the client sent for one statement: a `Query`, or an extended query up to its `Sync`.
fn read_a_statement(stream: &mut TcpStream, held: &mut Vec<u8>) -> Option<()> {
    loop {
        let (tag, _) = read_tagged(stream, held)?;
        match tag {
            b'Q' | b'S' => return Some(()),
            b'X' => return None,
            _ => continue,
        }
    }
}

/// A `SELECT` of one text column and two rows, then `ReadyForQuery`.
fn two_rows() -> Vec<u8> {
    let mut out = Out::tagged(b'T')
        .int16(1)
        .string("name")
        .int32(16385)
        .int16(1)
        .int32(25)
        .int16(-1)
        .int32(-1)
        .int16(0)
        .finish();
    out.extend(Out::tagged(b'D').int16(1).int32(5).bytes(b"Alice").finish());
    out.extend(Out::tagged(b'D').int16(1).int32(-1).finish());
    out.extend(Out::tagged(b'C').string("SELECT 2").finish());
    out.extend(Out::tagged(b'Z').bytes(b"I").finish());
    out
}

#[test]
fn the_whole_client_connects_authenticates_and_reads_a_result() {
    let server = Scripted::start(Some("pencil"), Script::Answers(vec![two_rows()]));
    let mut session = Session::connect(&server.source(), Some("pencil")).expect("connected");
    assert_eq!(session.version(), "17.2", "the server's own words");
    assert!(!session.is_encrypted(), "this server said no to encrypting, and prefer carries on");

    let rows = session.simple("select name from member", usize::MAX).expect("rows");
    assert_eq!(rows.columns.len(), 1);
    assert_eq!(rows.columns[0].name, "name");
    assert_eq!(rows.columns[0].type_name, "text");
    assert_eq!(rows.rows.len(), 2);
    assert_eq!(rows.rows[0][0], Value::typed("Alice"));
    // The NULL is a NULL and not an empty string, all the way from a length of -1 on the wire.
    assert_eq!(rows.rows[1][0], Value::Null);
    assert_eq!(rows.tag, "SELECT 2");
    session.close();
}

#[test]
fn a_password_the_server_does_not_know_is_refused_in_the_servers_own_words() {
    // The server salts `pencil` and checks the proof the way a real one does; the client is given
    // something else. So this is a genuine authentication failure rather than a canned refusal, and
    // what the client reports is the server's own `28P01`.
    let server = Scripted::start(Some("pencil"), Script::Answers(Vec::new()));
    let refused = Session::connect(&server.source(), Some("not-pencil")).expect_err("refused");
    assert_eq!(refused.code, "28P01");
    assert!(refused.message.contains("password authentication failed"), "{refused}");
    assert!(!refused.message.contains("pencil"), "and the refusal never quotes the password");
}

#[test]
fn an_error_response_is_the_servers_own_words_and_the_session_is_still_usable() {
    let mut failing = Out::tagged(b'E')
        .bytes(b"S")
        .string("ERROR")
        .bytes(b"C")
        .string("42P01")
        .bytes(b"M")
        .string("relation \"nothing\" does not exist")
        .bytes(b"P")
        .string("15")
        .bytes(&[0])
        .finish();
    failing.extend(Out::tagged(b'Z').bytes(b"I").finish());
    let server = Scripted::start(None, Script::Answers(vec![failing, two_rows()]));
    let mut session = Session::connect(&server.source(), None).expect("connected");

    let refused = session.simple("select * from nothing", usize::MAX).expect_err("refused");
    assert_eq!(refused.code, "42P01");
    assert!(refused.message.contains("does not exist"));
    assert_eq!(refused.position, Some(15));

    // And the connection still works, which is the point of reading through to `ReadyForQuery`
    // rather than answering the moment the error arrives: one bad statement in a console must not
    // cost the session.
    let rows = session.simple("select name from member", usize::MAX).expect("rows");
    assert_eq!(rows.rows.len(), 2);
    session.close();
}

#[test]
fn a_notice_in_the_middle_of_a_result_is_kept_rather_than_dropped() {
    // A `NOTICE` is often the only explanation of why a statement that succeeded did not do what was
    // meant, so it is carried on the result rather than thrown away.
    let mut answer = Out::tagged(b'N')
        .bytes(b"S")
        .string("NOTICE")
        .bytes(b"M")
        .string("identifier will be truncated")
        .bytes(&[0])
        .finish();
    answer.extend(two_rows());
    let server = Scripted::start(None, Script::Answers(vec![answer]));
    let mut session = Session::connect(&server.source(), None).expect("connected");
    let rows = session.simple("select name from member", usize::MAX).expect("rows");
    assert_eq!(rows.rows.len(), 2, "the notice did not stop the result");
    assert_eq!(rows.notices.len(), 1);
    assert!(rows.notices[0].contains("truncated"));
    session.close();
}

#[test]
fn a_server_that_will_not_encrypt_is_refused_under_require_and_allowed_under_prefer() {
    let server = Scripted::start(None, Script::RefuseTls);
    let mut source = server.source();
    source.ssl = SslMode::Require;
    let refused = Session::connect(&source, None).expect_err("refused");
    assert!(refused.message.contains("require it"), "{refused}");
    drop(server);

    // The same server under `prefer` carries on in the clear, which is PostgreSQL's own default.
    let allowed = Scripted::start(None, Script::Answers(vec![two_rows()]));
    let mut session = Session::connect(&allowed.source(), None).expect("connected");
    assert!(!session.is_encrypted());
    assert_eq!(session.simple("select 1", usize::MAX).expect("rows").rows.len(), 2);
    session.close();
}

#[test]
fn a_parameter_goes_up_bound_rather_than_pasted_into_the_statement() {
    // The extended query: `Parse`, `Bind`, `Describe`, `Execute`, `Sync`. What is being checked here
    // is that the client sends one and reads its acknowledgements back — the value never appears in
    // the statement text, which is what makes an awkward value a non-event.
    let mut answer = Out::tagged(b'1').finish();
    answer.extend(Out::tagged(b'2').finish());
    answer.extend(two_rows());
    let server = Scripted::start(None, Script::Answers(vec![answer]));
    let mut session = Session::connect(&server.source(), None).expect("connected");
    let rows = session
        .extended(
            "select name from member where name = $1",
            &[Value::typed("it's \"awkward\"; -- \\")],
            usize::MAX,
        )
        .expect("rows");
    assert_eq!(rows.rows.len(), 2);
    session.close();
}

#[test]
fn a_row_limit_is_asked_for_as_one_more_and_says_that_there_are_more() {
    let server = Scripted::start(None, Script::Answers(vec![two_rows()]));
    let mut session = Session::connect(&server.source(), None).expect("connected");
    let rows = session.simple("select name from member", 1).expect("rows");
    assert_eq!(rows.rows.len(), 1);
    assert!(rows.more, "nobody counted the rest, and `1-1 of 1+` is what the grid says");
    session.close();
}
