//! The PostgreSQL v3 frames, in both directions.
//!
//! A frame is a one-byte tag, a four-byte length that **includes itself**, and a body. The two
//! exceptions are the client's first messages — `StartupMessage`, `SSLRequest` and `CancelRequest` —
//! which have a length and a version code where the tag would be, because they are sent before the
//! connection has a protocol version.
//!
//! ## The reader is fed bytes, not a socket
//!
//! [`Frames`] takes whatever arrived and answers with whole messages, keeping the rest. That is
//! `unluminous-chat`'s server-sent-event reader arranged the same way and for the same reason: it makes
//! "the whole framing, end to end" a unit test with no socket in it, and
//! `the_same_stream_split_at_every_byte_boundary_produces_the_same_frames` is what proves a message
//! arriving in three reads is the same message as one arriving in one.
//!
//! A tag this version does not know is **skipped by its length** rather than treated as a fault,
//! which is what the protocol asks of a client and what stops a later server version breaking this
//! one.

use crate::rows::Failure;

/// The largest frame that will be assembled before the connection is given up on.
///
/// A server that framed nothing would otherwise be buffered until the allocator gives up, which ends
/// the process — the rule `unluminous_chat::sse::LARGEST_EVENT` already keeps. A row can legitimately be
/// large (a `bytea` of a file, a long `jsonb`), so this is generous rather than tight.
pub const LARGEST_FRAME: usize = 64 * 1024 * 1024;

/// One message from the server.
///
/// Only the ones this client acts on are variants; everything else is [`Message::Other`], which
/// carries the tag so that a test can assert it was seen and skipped rather than misread.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// `R` with a 0: the password was accepted.
    AuthenticationOk,
    /// `R` with a 3: send the password as it is.
    AuthenticationCleartext,
    /// `R` with a 5, and the four-byte salt.
    AuthenticationMd5([u8; 4]),
    /// `R` with a 10, and the mechanisms the server offers.
    AuthenticationSasl(Vec<String>),
    /// `R` with an 11: the server's first message of the SCRAM exchange.
    AuthenticationSaslContinue(String),
    /// `R` with a 12: the server's signature.
    AuthenticationSaslFinal(String),
    /// `S`: a run-time parameter, which is where the server version comes from.
    ParameterStatus { name: String, value: String },
    /// `K`: what a cancellation has to quote back on a second connection.
    BackendKeyData { process: u32, secret: u32 },
    /// `Z`: the server is ready, and what transaction it is in — `I`, `T` or `E`.
    ReadyForQuery(u8),
    /// `T`: the columns of the result about to arrive.
    RowDescription(Vec<Field>),
    /// `D`: one row. `None` is a NULL, which is a length of -1 on the wire rather than an empty value.
    DataRow(Vec<Option<Vec<u8>>>),
    /// `C`: the statement finished, and the tag it finished with.
    CommandComplete(String),
    /// `I`: the statement was empty.
    EmptyQueryResponse,
    /// `n`: the statement about to run returns no rows.
    NoData,
    /// `1`, `2`, `3`: the extended-query acknowledgements.
    ParseComplete,
    BindComplete,
    CloseComplete,
    /// `s`: the portal was suspended because the row limit was reached.
    PortalSuspended,
    /// `E`: it went wrong, in the server's own words.
    ErrorResponse(Failure),
    /// `N`: the server said something that is not an error.
    NoticeResponse(Failure),
    /// A tag this version has no reason to act on, skipped by its length.
    Other(u8),
}

/// One column, as `RowDescription` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// Which table it came from, or 0. This is what makes a result editable: a grid whose columns all
    /// come from one table and whose table has a key can address a row.
    pub table: u32,
    /// Which column of that table, or 0.
    pub column: i16,
    pub type_oid: u32,
    /// 0 for text, 1 for binary. Always 0 here — see `value.rs` for why every value arrives as text.
    pub format: i16,
}

/// Bytes in, whole messages out.
#[derive(Debug, Default)]
pub struct Frames {
    held: Vec<u8>,
}

impl Frames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add what just arrived.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.held.extend_from_slice(bytes);
    }

    /// How much is waiting to be framed, which is what the size guard is measured against.
    pub fn held(&self) -> usize {
        self.held.len()
    }

    /// The next whole message, or `None` while one is still arriving.
    ///
    /// `Err` is a stream that cannot be read at all — a length that is impossible, or a frame past
    /// [`LARGEST_FRAME`] — which is a different thing from an `ErrorResponse`, and the two must not be
    /// confused: one is the server saying no, the other is the connection being unusable.
    pub fn next(&mut self) -> Result<Option<Message>, Failure> {
        if self.held.len() < 5 {
            return Ok(None);
        }
        let tag = self.held[0];
        let length = u32::from_be_bytes([self.held[1], self.held[2], self.held[3], self.held[4]]) as usize;
        if length < 4 {
            return Err(Failure::said(format!(
                "the server sent a frame whose length is {length}, and a frame is at least four bytes."
            )));
        }
        if length > LARGEST_FRAME {
            return Err(Failure::said(format!(
                "the server sent a frame of {length} bytes, past the {LARGEST_FRAME} this reads."
            )));
        }
        // The length counts itself but not the tag.
        let whole = 1 + length;
        if self.held.len() < whole {
            return Ok(None);
        }
        let body = self.held[5..whole].to_vec();
        self.held.drain(..whole);
        Ok(Some(read(tag, &body)?))
    }
}

/// One message, from its tag and its body.
fn read(tag: u8, body: &[u8]) -> Result<Message, Failure> {
    let mut at = Reader::new(body);
    Ok(match tag {
        b'R' => match at.int32()? {
            0 => Message::AuthenticationOk,
            3 => Message::AuthenticationCleartext,
            5 => {
                let mut salt = [0_u8; 4];
                salt.copy_from_slice(at.take(4)?);
                Message::AuthenticationMd5(salt)
            }
            10 => {
                let mut mechanisms = Vec::new();
                loop {
                    let name = at.string()?;
                    if name.is_empty() {
                        break;
                    }
                    mechanisms.push(name);
                }
                Message::AuthenticationSasl(mechanisms)
            }
            11 => Message::AuthenticationSaslContinue(at.rest_as_string()),
            12 => Message::AuthenticationSaslFinal(at.rest_as_string()),
            // 2 is Kerberos, 6, 7 and 9 are SSPI and GSSAPI. Each is a real mechanism this client
            // does not speak, and saying which is what tells a person to change `pg_hba.conf` rather
            // than wonder why the connection stopped.
            other => {
                return Err(Failure::said(format!(
                    "the server asked for authentication method {other}, and Unluminous speaks \
                     SCRAM-SHA-256, MD5 and a plain password."
                )))
            }
        },
        b'S' => Message::ParameterStatus { name: at.string()?, value: at.string()? },
        b'K' => Message::BackendKeyData { process: at.int32()? as u32, secret: at.int32()? as u32 },
        b'Z' => Message::ReadyForQuery(*at.take(1)?.first().unwrap_or(&b'I')),
        b'T' => {
            let count = counted(at.int16()?, "columns")?;
            let mut fields = Vec::with_capacity(count);
            for _ in 0..count {
                fields.push(Field {
                    name: at.string()?,
                    table: at.int32()? as u32,
                    column: at.int16()?,
                    type_oid: at.int32()? as u32,
                    // The type's size and modifier are read past: every value arrives as text, so
                    // neither is used, and skipping them by name is clearer than by an offset.
                    format: {
                        let _size = at.int16()?;
                        let _modifier = at.int32()?;
                        at.int16()?
                    },
                });
            }
            Message::RowDescription(fields)
        }
        b'D' => {
            let count = counted(at.int16()?, "values")?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                let length = at.int32()?;
                values.push(match length {
                    // -1 is NULL, which is a different thing from a value of length zero. Collapsing
                    // the two is the fault `value::Value` exists to keep out of the grid.
                    -1 => None,
                    // **Anything else negative is a broken stream, not an empty value.** Clamping it
                    // to zero would turn a fault into a value the grid cannot be told apart from a
                    // real empty string — which is the one distinction this client promises to keep.
                    length if length < 0 => {
                        return Err(Failure::said(format!(
                            "the server sent a value of length {length}, and the only negative length \
                             a value has is -1, which means NULL."
                        )))
                    }
                    length => Some(at.take(length as usize)?.to_vec()),
                });
            }
            Message::DataRow(values)
        }
        b'C' => Message::CommandComplete(at.string()?),
        b'I' => Message::EmptyQueryResponse,
        b'n' => Message::NoData,
        b'1' => Message::ParseComplete,
        b'2' => Message::BindComplete,
        b'3' => Message::CloseComplete,
        b's' => Message::PortalSuspended,
        b'E' => Message::ErrorResponse(failure(&mut at)?),
        b'N' => Message::NoticeResponse(failure(&mut at)?),
        other => Message::Other(other),
    })
}

/// An `ErrorResponse` or a `NoticeResponse`: fields tagged by one letter, ending at a zero byte.
fn failure(at: &mut Reader<'_>) -> Result<Failure, Failure> {
    let mut out = Failure::default();
    loop {
        let field = match at.take(1) {
            Ok(byte) => byte[0],
            Err(_) => break,
        };
        if field == 0 {
            break;
        }
        let value = at.string()?;
        match field {
            b'M' => out.message = value,
            b'C' => out.code = value,
            b'D' => out.detail = value,
            b'H' => out.hint = value,
            b'P' => out.position = value.parse().ok(),
            // `S` and `V` are the severity, which the message already carries in front of it, and the
            // rest are file, line and routine inside the server's own source.
            _ => {}
        }
    }
    Ok(out)
}

/// Reading a body, one field at a time, with the end of it a refusal rather than a panic.
struct Reader<'a> {
    body: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(body: &'a [u8]) -> Self {
        Self { body, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Failure> {
        let end = self.at.checked_add(count).ok_or_else(short)?;
        if end > self.body.len() {
            return Err(short());
        }
        let out = &self.body[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn int16(&mut self) -> Result<i16, Failure> {
        let bytes = self.take(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn int32(&mut self) -> Result<i32, Failure> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A zero-terminated string. Not UTF-8 by the protocol's rules — it is whatever the client
    /// encoding is — but the startup message asks for UTF-8, so anything that is not is the server
    /// disagreeing with what it was asked for, and lossy is the honest answer rather than a refusal
    /// that loses the whole message.
    fn string(&mut self) -> Result<String, Failure> {
        let start = self.at;
        while self.at < self.body.len() && self.body[self.at] != 0 {
            self.at += 1;
        }
        if self.at >= self.body.len() {
            return Err(short());
        }
        let out = String::from_utf8_lossy(&self.body[start..self.at]).into_owned();
        self.at += 1;
        Ok(out)
    }

    fn rest_as_string(&mut self) -> String {
        let out = String::from_utf8_lossy(&self.body[self.at..]).into_owned();
        self.at = self.body.len();
        out
    }
}

fn short() -> Failure {
    Failure::said("the server sent a frame that ends in the middle of a field.")
}

/// A count of things in a frame, refusing a negative one rather than reading it as none.
///
/// A negative count means the stream is not what it claims to be, and reading it as zero would turn
/// a broken frame into an empty result — which looks exactly like a table with nothing in it.
fn counted(count: i16, what: &str) -> Result<usize, Failure> {
    match count < 0 {
        true => Err(Failure::said(format!(
            "the server said a frame holds {count} {what}, which is not a number of anything."
        ))),
        false => Ok(count as usize),
    }
}

/// Building a message to send.
#[derive(Debug, Default)]
pub struct Out(Vec<u8>);

impl Out {
    /// A tagged message. The length is filled in by [`Out::finish`].
    pub fn tagged(tag: u8) -> Self {
        Self(vec![tag, 0, 0, 0, 0])
    }

    /// One of the three untagged messages a client sends before the protocol has started.
    pub fn untagged() -> Self {
        Self(vec![0, 0, 0, 0])
    }

    pub fn int16(mut self, value: i16) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn int32(mut self, value: i32) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// A zero-terminated string.
    pub fn string(mut self, value: &str) -> Self {
        self.0.extend_from_slice(value.as_bytes());
        self.0.push(0);
        self
    }

    pub fn bytes(mut self, value: &[u8]) -> Self {
        self.0.extend_from_slice(value);
        self
    }

    /// The bytes to write, with the length filled in.
    ///
    /// A tagged message's length starts after the tag; an untagged one's starts at the beginning. Both
    /// count the four bytes of the length itself, which is the part of this protocol everybody gets
    /// wrong once.
    pub fn finish(mut self) -> Vec<u8> {
        let tagged = self.0.len() >= 5 && self.0[1..5] == [0, 0, 0, 0] && self.0[0] != 0;
        let (start, length) = match tagged {
            true => (1, self.0.len() - 1),
            false => (0, self.0.len()),
        };
        self.0[start..start + 4].copy_from_slice(&(length as u32).to_be_bytes());
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of a small, complete server conversation: an authentication request, a parameter, the
    /// backend key, ready, a one-column description, one row, a completion and ready again.
    fn a_conversation() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(Out::tagged(b'R').int32(0).finish());
        out.extend(Out::tagged(b'S').string("server_version").string("17.2").finish());
        out.extend(Out::tagged(b'K').int32(4242).int32(9999).finish());
        out.extend(Out::tagged(b'Z').bytes(b"I").finish());
        out.extend(
            Out::tagged(b'T')
                .int16(1)
                .string("name")
                .int32(16385)
                .int16(2)
                .int32(25)
                .int16(-1)
                .int32(-1)
                .int16(0)
                .finish(),
        );
        out.extend(Out::tagged(b'D').int16(1).int32(5).bytes(b"Alice").finish());
        out.extend(Out::tagged(b'C').string("SELECT 1").finish());
        out.extend(Out::tagged(b'Z').bytes(b"I").finish());
        out
    }

    fn all_of(bytes: &[u8], chunk: usize) -> Vec<Message> {
        let mut frames = Frames::new();
        let mut messages = Vec::new();
        for piece in bytes.chunks(chunk.max(1)) {
            frames.feed(piece);
            while let Some(message) = frames.next().expect("a readable stream") {
                messages.push(message);
            }
        }
        messages
    }

    #[test]
    fn the_same_stream_split_at_every_byte_boundary_produces_the_same_frames() {
        // `unluminous-chat`'s test, applied to a different protocol. A message that arrives in three reads
        // has to be the same message as one that arrives in one, and the only way to know is to try
        // every split.
        let bytes = a_conversation();
        let whole = all_of(&bytes, bytes.len());
        assert_eq!(whole.len(), 8);
        for chunk in 1..=bytes.len() {
            assert_eq!(all_of(&bytes, chunk), whole, "split into {chunk}-byte pieces");
        }
    }

    #[test]
    fn a_row_description_and_a_row_are_read_into_their_parts() {
        let messages = all_of(&a_conversation(), 7);
        let Message::RowDescription(fields) = &messages[4] else { panic!("{:?}", messages[4]) };
        assert_eq!(fields[0].name, "name");
        assert_eq!(fields[0].table, 16385, "which table, which is what makes a result editable");
        assert_eq!(fields[0].column, 2);
        assert_eq!(fields[0].type_oid, 25);
        let Message::DataRow(values) = &messages[5] else { panic!() };
        assert_eq!(values[0].as_deref(), Some(&b"Alice"[..]));
    }

    #[test]
    fn a_null_is_a_length_of_minus_one_and_not_an_empty_value() {
        let bytes = Out::tagged(b'D').int16(2).int32(-1).int32(0).finish();
        let messages = all_of(&bytes, 3);
        let Message::DataRow(values) = &messages[0] else { panic!() };
        assert_eq!(values[0], None, "NULL");
        assert_eq!(values[1].as_deref(), Some(&b""[..]), "and the empty string, which is different");
    }

    #[test]
    fn an_error_carries_the_servers_own_fields_and_the_rest_are_passed_over() {
        let bytes = Out::tagged(b'E')
            .bytes(b"S")
            .string("ERROR")
            .bytes(b"C")
            .string("42703")
            .bytes(b"M")
            .string("column \"nmae\" does not exist")
            .bytes(b"H")
            .string("Perhaps you meant \"name\".")
            .bytes(b"P")
            .string("8")
            .bytes(b"F")
            .string("parse_relation.c")
            .bytes(&[0])
            .finish();
        let messages = all_of(&bytes, 5);
        let Message::ErrorResponse(failure) = &messages[0] else { panic!("{:?}", messages[0]) };
        assert_eq!(failure.code, "42703");
        assert!(failure.message.contains("nmae"));
        assert!(failure.hint.contains("Perhaps"));
        assert_eq!(failure.position, Some(8));
    }

    #[test]
    fn a_tag_this_version_does_not_know_is_skipped_by_its_length() {
        // What the protocol asks of a client, and what stops a later server version breaking this one.
        let mut bytes = Out::tagged(b'v').string("some future thing").finish();
        bytes.extend(Out::tagged(b'Z').bytes(b"I").finish());
        let messages = all_of(&bytes, 2);
        assert_eq!(messages, [Message::Other(b'v'), Message::ReadyForQuery(b'I')]);
    }

    #[test]
    fn a_frame_that_cannot_be_read_is_a_refusal_rather_than_a_panic() {
        // A length shorter than the length field, which no server sends and a corrupted stream does.
        let mut frames = Frames::new();
        frames.feed(&[b'Z', 0, 0, 0, 1]);
        assert!(frames.next().is_err());
        // And a frame that claims to be larger than anything will be assembled.
        let mut huge = Frames::new();
        huge.feed(&[b'D', 0xff, 0xff, 0xff, 0xff]);
        let refused = huge.next().expect_err("refused");
        assert!(refused.message.contains("past the"), "{refused}");
    }

    #[test]
    fn a_negative_length_that_is_not_minus_one_is_a_refusal_rather_than_an_empty_value() {
        // -1 is NULL and nothing else negative means anything. Clamping to zero would turn a broken
        // stream into a value the grid cannot tell from a real empty string.
        let bytes = Out::tagged(b'D').int16(1).int32(-2).finish();
        let mut frames = Frames::new();
        frames.feed(&bytes);
        let refused = frames.next().expect_err("refused");
        assert!(refused.message.contains("only negative length"), "{refused}");

        // And a negative count of values or columns is refused for the same reason.
        let mut counts = Frames::new();
        counts.feed(&Out::tagged(b'D').int16(-1).finish());
        assert!(counts.next().is_err());
        let mut columns = Frames::new();
        columns.feed(&Out::tagged(b'T').int16(-3).finish());
        assert!(columns.next().is_err());
    }

    #[test]
    fn a_field_that_ends_in_the_middle_of_the_body_is_a_refusal() {
        // A row description claiming two fields with one field's bytes behind it.
        let bytes = Out::tagged(b'T').int16(2).string("a").int32(0).int16(0).finish();
        let mut frames = Frames::new();
        frames.feed(&bytes);
        assert!(frames.next().is_err());
    }

    #[test]
    fn a_tagged_length_counts_itself_and_not_the_tag() {
        // The part of this protocol everybody gets wrong once.
        let bytes = Out::tagged(b'Q').string("select 1").finish();
        assert_eq!(bytes[0], b'Q');
        assert_eq!(u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize, bytes.len() - 1);
        // And an untagged one counts the whole thing, because there is no tag in front of it.
        let startup = Out::untagged().int32(196_608).string("user").string("me").bytes(&[0]).finish();
        assert_eq!(u32::from_be_bytes([startup[0], startup[1], startup[2], startup[3]]) as usize, startup.len());
    }
}
