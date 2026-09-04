//! The connection: opening one, running a statement on it, and stopping one that is running.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::postgres::scram;
use crate::postgres::wire::{Field, Frames, Message, Out};
use crate::rows::{Answer, Failure, Rows};
use crate::source::{Secret, Source, SslMode};
use crate::value::{postgres_type_name, Column, Value};

/// How long a connection waits for the server before giving up.
///
/// On the socket rather than in a loop of its own, because a thread parked in `read` cannot look at a
/// flag — which is the fault `unluminous-chat` records about stopping an agent, and the reason stopping a
/// query here opens a second connection rather than setting one.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a read waits before it is treated as a server that has stopped answering.
///
/// Long, because a query may legitimately take minutes and the person watching it can press Stop. It
/// is here so that a server which has gone away is eventually reported rather than holding a worker
/// thread for the life of the window.
const READ_TIMEOUT: Duration = Duration::from_secs(60 * 30);

/// The wire protocol version: 3.0, as one number.
const PROTOCOL: i32 = 196_608;
const SSL_REQUEST: i32 = 80_877_103;
const CANCEL_REQUEST: i32 = 80_877_102;

/// A socket, encrypted or not.
///
/// An enum rather than a boxed `Read + Write`, because there are two cases and the compiler then
/// names every place that has to answer for a third.
enum Transport {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Transport {
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Transport::Plain(stream) => stream.write_all(bytes),
            Transport::Tls(stream) => stream.write_all(bytes),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Transport::Plain(stream) => stream.flush(),
            Transport::Tls(stream) => stream.flush(),
        }
    }

    fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(stream) => stream.read(into),
            Transport::Tls(stream) => stream.read(into),
        }
    }

    fn is_encrypted(&self) -> bool {
        matches!(self, Transport::Tls(_))
    }
}

/// One PostgreSQL connection.
pub struct Session {
    stream: Transport,
    frames: Frames,
    /// What the server said about itself: `server_version`, `server_encoding` and the rest.
    pub parameters: Vec<(String, String)>,
    /// What a cancellation has to quote back, on a connection of its own.
    key: Option<(u32, u32)>,
    /// Where to open that second connection.
    address: String,
    ssl: SslMode,
    /// Anything the server said that was not an error since the last statement.
    notices: Vec<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Session")
            .field("address", &self.address)
            .field("encrypted", &self.stream.is_encrypted())
            .field("version", &self.version())
            .finish()
    }
}

impl Session {
    /// Open a connection and get as far as the server saying it is ready.
    ///
    /// `password` is fetched by the caller at this moment and dropped as soon as this returns — see
    /// `source::Secret`, which records *where* a password is and never what it is.
    pub fn connect(source: &Source, password: Option<&str>) -> Answer<Session> {
        let address = format!("{}:{}", source.host, source.port);
        let stream = connect_to(&address)?;
        let stream = start_tls(stream, source, &address)?;
        let mut session = Session {
            stream,
            frames: Frames::new(),
            parameters: Vec::new(),
            key: None,
            address,
            ssl: source.ssl,
            notices: Vec::new(),
        };
        session.start_up(source, password)?;
        if source.read_only {
            // **The guarantee is the server's, not a parser's.** Unluminous also hides the editing
            // controls, but a statement that got past the check in `sql::only_reads` is still
            // refused here, by PostgreSQL, with its own message. See the TDD §7.
            session.simple("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY", usize::MAX)?;
        }
        Ok(session)
    }

    /// What the server calls itself, which is what Test Connection reports.
    pub fn version(&self) -> String {
        self.parameter("server_version").unwrap_or_default().to_owned()
    }

    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
    }

    /// True when this connection is encrypted, which the settings page shows and a test asserts.
    pub fn is_encrypted(&self) -> bool {
        self.stream.is_encrypted()
    }

    /// Run one statement and read everything it answers.
    ///
    /// `limit` is asked for as `limit + 1` rows and cut back, which is what makes `1-200 of 200+`
    /// honest: nobody counted the rest. `usize::MAX` means every row.
    pub fn simple(&mut self, statement: &str, limit: usize) -> Answer<Rows> {
        let started = Instant::now();
        self.notices.clear();
        self.send(Out::tagged(b'Q').string(statement).finish())?;
        let mut rows = self.read_a_result(limit)?;
        rows.elapsed = started.elapsed();
        rows.notices = std::mem::take(&mut self.notices);
        Ok(rows)
    }

    /// Run one statement with its values bound as parameters.
    ///
    /// **Nothing is quoted by hand anywhere**, which is what makes a value containing a quote, a
    /// newline or a backslash a non-event rather than the fault every implementation that builds SQL
    /// by concatenation has. Every write the row editor makes goes through here.
    ///
    /// The parameters go up as text, with `None` for NULL, and the result comes back as text, which is
    /// the one format decision this client makes — see `value.rs`.
    pub fn extended(&mut self, statement: &str, values: &[Value], limit: usize) -> Answer<Rows> {
        let started = Instant::now();
        self.notices.clear();
        let mut out = Vec::new();
        // An unnamed statement and an unnamed portal, both replaced by the next use, which is what a
        // client that never reuses a plan wants.
        out.extend(Out::tagged(b'P').string("").string(statement).int16(0).finish());
        let mut bind = Out::tagged(b'B')
            .string("")
            .string("")
            // No format codes for the parameters, which means text for all of them.
            .int16(0)
            .int16(values.len() as i16);
        for value in values {
            bind = match value {
                Value::Null => bind.int32(-1),
                Value::Text(text) => bind.int32(text.len() as i32).bytes(text.as_bytes()),
                Value::Bytes(bytes) => {
                    // A `bytea` parameter sent as text is PostgreSQL's hex form, which is the one
                    // encoding every server since 9.0 reads.
                    let hex: String =
                        format!("\\x{}", bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
                    bind.int32(hex.len() as i32).bytes(hex.as_bytes())
                }
            };
        }
        // No format codes for the results either: text, for the same reason.
        out.extend(bind.int16(0).finish());
        out.extend(Out::tagged(b'D').bytes(b"P").string("").finish());
        out.extend(Out::tagged(b'E').string("").int32(0).finish());
        out.extend(Out::tagged(b'S').finish());
        self.send(out)?;
        let mut rows = self.read_a_result(limit)?;
        rows.elapsed = started.elapsed();
        rows.notices = std::mem::take(&mut self.notices);
        Ok(rows)
    }

    /// What another thread can stop this connection with.
    ///
    /// **The parts rather than a borrow of the session**, because the thread that would stop a
    /// statement is by definition not the thread holding the connection: that one is asleep in a
    /// `read` inside the engine. See [`cancel`].
    pub fn stopper(&self) -> crate::engine::Stopper {
        let host = self.address.rsplit_once(':').map(|(host, _)| host.to_owned()).unwrap_or_else(|| self.address.clone());
        let port = self
            .address
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .unwrap_or(5432);
        crate::engine::Stopper::Postgres { host, port, key: self.key, ssl: self.ssl }
    }

    /// Say goodbye, so the server does not have to notice the socket closing.
    pub fn close(&mut self) {
        let _ = self.stream.write_all(&Out::tagged(b'X').finish());
        let _ = self.stream.flush();
    }

    /// The startup message, then whatever authentication the server asks for.
    fn start_up(&mut self, source: &Source, password: Option<&str>) -> Answer<()> {
        let startup = Out::untagged()
            .int32(PROTOCOL)
            .string("user")
            .string(&source.user)
            .string("database")
            .string(&source.database)
            // Every value arrives as text, so the encoding it is text *in* has to be said rather than
            // inherited from whatever the server's default is.
            .string("client_encoding")
            .string("UTF8")
            // What shows in `pg_stat_activity`, so somebody looking at their own server can see which
            // connection is this window's.
            .string("application_name")
            .string("Unluminous")
            .bytes(&[0])
            .finish();
        self.send(startup)?;
        loop {
            match self.next_message()? {
                Message::AuthenticationOk => break,
                Message::AuthenticationCleartext => {
                    let password = self.password_or_refuse(password, source)?;
                    // A password in the clear over an unencrypted connection to somewhere else on the
                    // network is a password on the wire. Loopback is exempt, because there is no
                    // network to be on.
                    if !self.stream.is_encrypted() && !is_loopback(&source.host) {
                        return Err(Failure::said(format!(
                            "{} asked for the password in the clear over an unencrypted connection. \
                             Set sslmode=require on this data source, or change the server's \
                             pg_hba.conf to scram-sha-256.",
                            source.host
                        )));
                    }
                    self.send(Out::tagged(b'p').string(&password).finish())?;
                }
                Message::AuthenticationMd5(salt) => {
                    let password = self.password_or_refuse(password, source)?;
                    let answer = scram::md5_password(&password, &source.user, salt);
                    self.send(Out::tagged(b'p').string(&answer).finish())?;
                }
                Message::AuthenticationSasl(mechanisms) => {
                    if !mechanisms.iter().any(|name| name == scram::MECHANISM) {
                        return Err(Failure::said(format!(
                            "the server offers {} and Unluminous speaks {}.",
                            mechanisms.join(", "),
                            scram::MECHANISM
                        )));
                    }
                    let password = self.password_or_refuse(password, source)?;
                    let mut exchange = scram::Exchange::begin(&password)?;
                    let first = exchange.first();
                    self.send(
                        Out::tagged(b'p')
                            .string(scram::MECHANISM)
                            .int32(first.len() as i32)
                            .bytes(first.as_bytes())
                            .finish(),
                    )?;
                    let server_first = match self.next_message()? {
                        Message::AuthenticationSaslContinue(text) => text,
                        // A server that refuses here refuses in its own words — `28P01` for a
                        // password it does not know — and quoting that is far more use than saying
                        // an unexpected message arrived.
                        Message::ErrorResponse(failure) => return Err(failure),
                        other => return Err(unexpected(&other, "the server's first SCRAM message")),
                    };
                    let final_message = exchange.respond(&server_first)?;
                    self.send(Out::tagged(b'p').bytes(final_message.as_bytes()).finish())?;
                    match self.next_message()? {
                        Message::AuthenticationSaslFinal(text) => exchange.finish(&text)?,
                        Message::ErrorResponse(failure) => return Err(failure),
                        other => return Err(unexpected(&other, "the server's final SCRAM message")),
                    }
                }
                Message::ErrorResponse(failure) => return Err(failure),
                Message::NoticeResponse(_) | Message::ParameterStatus { .. } => {}
                other => return Err(unexpected(&other, "an authentication request")),
            }
        }
        // Then the parameters, the key and `ReadyForQuery`.
        loop {
            match self.next_message()? {
                Message::ParameterStatus { name, value } => self.parameters.push((name, value)),
                Message::BackendKeyData { process, secret } => self.key = Some((process, secret)),
                Message::ReadyForQuery(_) => return Ok(()),
                Message::NoticeResponse(_) => {}
                Message::ErrorResponse(failure) => return Err(failure),
                other => return Err(unexpected(&other, "the server becoming ready")),
            }
        }
    }

    /// The password, or a refusal naming where one is meant to come from.
    ///
    /// **The refusal names the place, never the value**, which is the rule every secret in this
    /// repository keeps.
    fn password_or_refuse(&self, password: Option<&str>, source: &Source) -> Answer<String> {
        match password {
            Some(password) => Ok(password.to_owned()),
            None => Err(Failure::said(match &source.secret {
                Secret::Environment(name) => format!(
                    "{} asked for a password, and the environment variable `{name}` this data source \
                     names is not set.",
                    source.host
                ),
                Secret::Keychain(name) => format!(
                    "{} asked for a password, and the keychain entry `{name}` this data source names \
                     could not be read.",
                    source.host
                ),
                _ => format!(
                    "{} asked for a password and this data source names nowhere to get one. Name an \
                     environment variable on it, or type one for this window.",
                    source.host
                ),
            })),
        }
    }

    /// Read messages until the server is ready again, building one result out of them.
    fn read_a_result(&mut self, limit: usize) -> Answer<Rows> {
        let mut rows = Rows::default();
        let mut fields: Vec<Field> = Vec::new();
        let mut failure: Option<Failure> = None;
        loop {
            match self.next_message()? {
                Message::RowDescription(described) => {
                    rows.columns = described
                        .iter()
                        .map(|field| Column::new(&field.name, postgres_type_name(field.type_oid)))
                        .collect();
                    fields = described;
                }
                Message::DataRow(values) => {
                    if rows.rows.len() >= limit {
                        // Every row still has to be read off the wire — the server is sending them
                        // and the connection is unusable until it has finished — but past the limit
                        // they are counted rather than kept.
                        rows.more = true;
                        continue;
                    }
                    rows.rows.push(
                        values
                            .into_iter()
                            .enumerate()
                            .map(|(index, value)| read_value(value, fields.get(index)))
                            .collect(),
                    );
                }
                Message::CommandComplete(tag) => {
                    rows.affected = affected_from(&tag);
                    rows.tag = tag;
                }
                Message::EmptyQueryResponse => rows.tag = "empty query".to_owned(),
                Message::NoticeResponse(notice) => self.notices.push(notice.to_string()),
                Message::ErrorResponse(said) => failure = Some(said),
                // The server is ready again: the statement is over, whatever happened.
                Message::ReadyForQuery(_) => break,
                Message::ParameterStatus { name, value } => {
                    // A `SET` changes one, and the server says so. Keeping it means `search_path`
                    // reads back correctly after the schema switcher has been used.
                    self.parameters.retain(|(known, _)| *known != name);
                    self.parameters.push((name, value));
                }
                Message::ParseComplete
                | Message::BindComplete
                | Message::CloseComplete
                | Message::NoData
                | Message::PortalSuspended
                | Message::BackendKeyData { .. }
                | Message::Other(_) => {}
                other => return Err(unexpected(&other, "a result")),
            }
        }
        match failure {
            // The failure is answered **after** reading to `ReadyForQuery`, not the moment it
            // arrives: leaving the rest of a statement's messages on the socket would make the next
            // statement read this one's tail and see something impossible.
            Some(failure) => Err(failure),
            None => Ok(rows),
        }
    }

    fn next_message(&mut self) -> Answer<Message> {
        loop {
            if let Some(message) = self.frames.next()? {
                return Ok(message);
            }
            let mut buffer = [0_u8; 16 * 1024];
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    return Err(Failure::said(
                        "the server closed the connection. It may have been restarted, or the \
                         statement may have crashed the session.",
                    ))
                }
                Ok(read) => self.frames.feed(&buffer[..read]),
                Err(why) => {
                    return Err(Failure::said(format!("the server stopped answering: {why}")))
                }
            }
        }
    }

    fn send(&mut self, bytes: Vec<u8>) -> Answer<()> {
        self.stream.write_all(&bytes).map_err(sending)?;
        self.stream.flush().map_err(sending)
    }
}

/// Ask the server to stop what a connection is doing.
///
/// **A second connection, not a flag.** The protocol has no way to interrupt a statement on the
/// connection running it — that socket is busy carrying the answer — and a flag on this side would
/// leave the server working for as long as the query takes while the pane claimed it had stopped.
/// This is what PostgreSQL asks for: a new socket, a `CancelRequest` carrying the process id and
/// secret from `BackendKeyData`, and no answer at all, because the server closes it either way.
pub fn cancel(host: &str, port: u16, key: Option<(u32, u32)>, ssl: SslMode) -> Answer<()> {
    let Some((process, secret)) = key else {
        return Err(Failure::said("the server never sent a key, so there is nothing to cancel with."));
    };
    let address = format!("{host}:{port}");
    let mut stream = connect_to(&address)?;
    let bytes = Out::untagged().int32(CANCEL_REQUEST).int32(process as i32).int32(secret as i32).finish();
    // A cancellation is sent in the clear even on an encrypted connection when the server allowed
    // one — but a `require` data source has said the network is not to be trusted, so its request is
    // wrapped too rather than putting the key on the wire.
    match ssl {
        SslMode::Require => {
            let mut secure = start_tls_on(stream, &address, true)?;
            secure.write_all(&bytes).map_err(sending)?;
            secure.flush().map_err(sending)
        }
        _ => {
            stream.write_all(&bytes).map_err(sending)?;
            stream.flush().map_err(sending)
        }
    }
}

/// One value off the wire.
fn read_value(value: Option<Vec<u8>>, field: Option<&Field>) -> Value {
    let Some(bytes) = value else { return Value::Null };
    let is_bytea = field.is_some_and(|field| field.type_oid == 17);
    match is_bytea {
        // `bytea` in text form is `\x` and hex digits. Turning it back into bytes is what lets the
        // grid say "12 bytes" rather than showing a hex string as though it were the value.
        true => match std::str::from_utf8(&bytes).ok().and_then(from_hex) {
            Some(bytes) => Value::Bytes(bytes),
            None => Value::Bytes(bytes),
        },
        false => Value::Text(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("\\x")?;
    if digits.len() % 2 != 0 {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).ok())
        .collect()
}

/// How many rows a completion tag says were changed.
///
/// `INSERT 0 1` carries an oid before the count and everything else carries the count alone, which is
/// the one irregularity in the tag format.
fn affected_from(tag: &str) -> Option<u64> {
    let mut words = tag.split_whitespace();
    let verb = words.next()?;
    let numbers: Vec<&str> = words.collect();
    match verb {
        "INSERT" => numbers.get(1).and_then(|count| count.parse().ok()),
        "UPDATE" | "DELETE" | "MERGE" | "MOVE" | "FETCH" | "COPY" | "SELECT" => {
            numbers.first().and_then(|count| count.parse().ok())
        }
        _ => None,
    }
}

fn connect_to(address: &str) -> Answer<TcpStream> {
    use std::net::ToSocketAddrs;
    let mut last: Option<String> = None;
    let addresses = address
        .to_socket_addrs()
        .map_err(|why| Failure::said(format!("{address} is not a name this machine can look up: {why}")))?;
    for candidate in addresses {
        match TcpStream::connect_timeout(&candidate, CONNECT_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));
                // A query answered in pieces is a query drawn in pieces, and Nagle would hold the
                // last short write of every message until the previous one was acknowledged.
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(why) => last = Some(why.to_string()),
        }
    }
    Err(Failure::said(format!(
        "nothing answered at {address}: {}",
        last.unwrap_or_else(|| "no address to try".to_owned())
    )))
}

/// The `SSLRequest` handshake, which is one byte of answer and then an ordinary TLS handshake.
fn start_tls(stream: TcpStream, source: &Source, address: &str) -> Answer<Transport> {
    if source.ssl == SslMode::Disable {
        return Ok(Transport::Plain(stream));
    }
    let mut stream = stream;
    stream.write_all(&Out::untagged().int32(SSL_REQUEST).finish()).map_err(sending)?;
    stream.flush().map_err(sending)?;
    let mut answer = [0_u8; 1];
    stream
        .read_exact(&mut answer)
        .map_err(|why| Failure::said(format!("the server did not answer the request to encrypt: {why}")))?;
    match (answer[0], source.ssl) {
        (b'S', _) => Ok(Transport::Tls(Box::new(handshake(stream, &source.host)?))),
        (_, SslMode::Require) => Err(Failure::said(format!(
            "{} will not encrypt this connection, and this data source is set to require it.",
            source.host
        ))),
        // `prefer`: carry on in the clear, which is PostgreSQL's own default behaviour.
        _ => {
            let _ = address;
            Ok(Transport::Plain(stream))
        }
    }
}

/// The same handshake for the cancellation connection, which has already decided it wants one.
fn start_tls_on(mut stream: TcpStream, address: &str, required: bool) -> Answer<Transport> {
    stream.write_all(&Out::untagged().int32(SSL_REQUEST).finish()).map_err(sending)?;
    stream.flush().map_err(sending)?;
    let mut answer = [0_u8; 1];
    stream
        .read_exact(&mut answer)
        .map_err(|why| Failure::said(format!("the server did not answer the request to encrypt: {why}")))?;
    let host = address.rsplit_once(':').map(|(host, _)| host).unwrap_or(address);
    match answer[0] {
        b'S' => Ok(Transport::Tls(Box::new(handshake(stream, host)?))),
        _ if required => Err(Failure::said("the server will not encrypt the connection a cancellation needs.")),
        _ => Ok(Transport::Plain(stream)),
    }
}

/// TLS from the machine's own certificate store.
///
/// `native-tls` is schannel on Windows and Security.framework on macOS, so what a data source is
/// checked against is what every other program on this machine trusts — which is the argument `ureq`
/// is already chosen on, made about a certificate store rather than a credential helper.
fn handshake(stream: TcpStream, host: &str) -> Answer<native_tls::TlsStream<TcpStream>> {
    let connector = native_tls::TlsConnector::new()
        .map_err(|why| Failure::said(format!("this machine's TLS would not start: {why}")))?;
    connector.connect(host, stream).map_err(|why| match why {
        native_tls::HandshakeError::Failure(why) => Failure::said(format!(
            "the encrypted connection to {host} was refused: {why}"
        )),
        _ => Failure::said(format!("the encrypted connection to {host} did not finish.")),
    })
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn sending(why: std::io::Error) -> Failure {
    Failure::said(format!("the connection would not take what Unluminous sent: {why}"))
}

fn unexpected(message: &Message, wanted: &str) -> Failure {
    Failure::said(format!("the server sent {message:?} where Unluminous was waiting for {wanted}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_completion_tag_says_how_many_rows_changed_and_insert_is_the_odd_one() {
        assert_eq!(affected_from("UPDATE 3"), Some(3));
        assert_eq!(affected_from("DELETE 0"), Some(0));
        // `INSERT` carries an oid before the count, which is the one irregularity in the format.
        assert_eq!(affected_from("INSERT 0 7"), Some(7));
        assert_eq!(affected_from("CREATE TABLE"), None);
        assert_eq!(affected_from(""), None);
    }

    #[test]
    fn a_bytea_arrives_as_hex_and_is_read_back_into_bytes() {
        let field = Field { name: "b".to_owned(), table: 0, column: 0, type_oid: 17, format: 0 };
        let value = read_value(Some(b"\\x00ff41".to_vec()), Some(&field));
        assert_eq!(value, Value::Bytes(vec![0x00, 0xff, 0x41]));
        // And a text column is text, however it looks.
        let text = Field { type_oid: 25, ..field.clone() };
        assert_eq!(read_value(Some(b"\\x00ff41".to_vec()), Some(&text)), Value::Text("\\x00ff41".to_owned()));
        assert_eq!(read_value(None, Some(&text)), Value::Null);
    }

    #[test]
    fn loopback_is_recognised_because_a_password_in_the_clear_there_is_on_no_network() {
        assert!(is_loopback("localhost"));
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("db.example.com"));
        assert!(!is_loopback("10.0.0.4"));
    }
}
