//! The wire: `Content-Length` framed JSON, and the two pure functions that read and write it.
//!
//! The Debug Adapter Protocol borrows the Language Server Protocol's framing, so a message is ASCII
//! headers, a blank line, and then exactly that many bytes of UTF-8 JSON:
//!
//! ```text
//! Content-Length: 119\r\n
//! \r\n
//! {"seq":1,"type":"request","command":"initialize","arguments":{ ... }}
//! ```
//!
//! Both halves are pure — bytes in, messages out, and messages in, bytes out — so every one of the
//! awkward cases a pipe really produces is a test with no process behind it: a frame split in the
//! middle of its header, a frame split in the middle of its body, two frames arriving in one read,
//! and a header carrying a field nobody asked for. That is `task-1675`'s testing answer applied
//! here, and it is what makes "when does the adapter answer" a question this crate never has to ask.
//!
//! A frame that cannot be read at all is a [`FrameError`] rather than a panic. An adapter that
//! writes rubbish is a real thing that happens — a crash report on standard output, a Node warning
//! printed before the server starts — and the honest answer to it is to say what was seen and let
//! the session end, not to take the window down.

use std::collections::VecDeque;

/// The header every frame carries.
const LENGTH: &str = "Content-Length:";
/// The blank line between the headers and the body.
const SEPARATOR: &[u8] = b"\r\n\r\n";
/// The largest frame that will be read.
///
/// A `variables` answer over a large structure is the biggest thing an adapter sends and is measured
/// in tens of kilobytes; sixteen megabytes is far past anything real and is a wall against a
/// `Content-Length` that has been corrupted into a number that would allocate the machine's memory.
const LIMIT: usize = 16 * 1024 * 1024;

/// Why a frame could not be read.
///
/// Carried rather than logged, because the caller is the one that knows where to say it — the
/// status bar for a session that is starting, the debug tile for one that is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The headers held no `Content-Length`, so there is no way to know where the body ends.
    NoLength(String),
    /// The `Content-Length` was not a number, or was past [`LIMIT`].
    BadLength(String),
    /// The body was not UTF-8, or was not JSON.
    BadBody(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::NoLength(headers) => {
                write!(out, "a message with no Content-Length: {headers}")
            }
            FrameError::BadLength(value) => write!(out, "a Content-Length of `{value}`"),
            FrameError::BadBody(problem) => write!(out, "a message that is not JSON: {problem}"),
        }
    }
}

/// Turn one JSON value into a frame.
///
/// The body is written first so that its length is the length of the bytes rather than of the
/// characters — a variable holding an accented name is two bytes for one letter, and a header
/// counting letters would tear every frame after it.
pub fn encode(value: &serde_json::Value) -> Vec<u8> {
    let body = value.to_string();
    let mut frame = format!("{LENGTH} {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(body.as_bytes());
    frame
}

/// The other half: bytes as they arrive, whole messages as they complete.
///
/// It holds what has been read and not yet used, which is what makes a torn frame ordinary rather
/// than a special case: [`Decoder::feed`] is given whatever the last read produced, however much
/// that was, and answers with the messages that are now complete.
#[derive(Debug, Default)]
pub struct Decoder {
    buffer: VecDeque<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many bytes are held waiting for the rest of their frame. What a test asserts a torn read
    /// with.
    pub fn pending(&self) -> usize {
        self.buffer.len()
    }

    /// Take some bytes and answer with every message they completed.
    ///
    /// A failure stops the pass: the buffer holds bytes that cannot be interpreted, so going on
    /// would be guessing where the next frame starts. The caller ends the session, which is what an
    /// adapter writing rubbish deserves.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<serde_json::Value>, FrameError> {
        self.buffer.extend(bytes.iter().copied());
        let mut messages = Vec::new();
        while let Some(message) = self.take_one()? {
            messages.push(message);
        }
        Ok(messages)
    }

    /// One frame, if a whole one is there.
    fn take_one(&mut self) -> Result<Option<serde_json::Value>, FrameError> {
        let Some(separator) = find(&self.buffer, SEPARATOR) else {
            return Ok(None);
        };
        let headers: String =
            self.buffer.iter().take(separator).map(|byte| *byte as char).collect();
        let length = content_length(&headers)?;
        let start = separator + SEPARATOR.len();
        if self.buffer.len() < start + length {
            // The headers are here and the body is not. Nothing is taken, so the next read simply
            // adds to what is already held and this is asked again.
            return Ok(None);
        }
        self.buffer.drain(..start);
        let body: Vec<u8> = self.buffer.drain(..length).collect();
        let text = String::from_utf8(body)
            .map_err(|problem| FrameError::BadBody(problem.to_string()))?;
        let value = serde_json::from_str(&text)
            .map_err(|problem| FrameError::BadBody(problem.to_string()))?;
        Ok(Some(value))
    }
}

/// The `Content-Length` out of a block of headers.
///
/// Header names are compared without case, because the protocol says ASCII headers and nothing
/// promises which case an adapter writes them in. Any other header is ignored: `Content-Type` is
/// the one the specification mentions and it carries nothing a client needs.
fn content_length(headers: &str) -> Result<usize, FrameError> {
    for line in headers.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("Content-Length") {
            continue;
        }
        let value = value.trim();
        let length: usize =
            value.parse().map_err(|_| FrameError::BadLength(value.to_owned()))?;
        if length > LIMIT {
            return Err(FrameError::BadLength(value.to_owned()));
        }
        return Ok(length);
    }
    Err(FrameError::NoLength(headers.to_owned()))
}

/// Where `needle` starts in `haystack`, if it is in it.
///
/// A walk rather than anything cleverer: the needle is four bytes and the headers are a few dozen,
/// so the whole search is over before a smarter algorithm would have finished setting up.
fn find(haystack: &VecDeque<u8>, needle: &[u8]) -> Option<usize> {
    if haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|at| needle.iter().enumerate().all(|(step, byte)| haystack[at + step] == *byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(seq: i64) -> serde_json::Value {
        json!({ "seq": seq, "type": "event", "event": "stopped" })
    }

    #[test]
    fn a_frame_says_how_many_bytes_its_body_is() {
        let bytes = encode(&json!({ "a": 1 }));
        let text = String::from_utf8(bytes).expect("ascii headers and a JSON body");
        assert_eq!(text, "Content-Length: 7\r\n\r\n{\"a\":1}");
    }

    /// The length counts bytes rather than characters, which only shows up on a value that is not
    /// ASCII — and a variable holding an accented name is an ordinary thing for an adapter to send.
    #[test]
    fn the_length_counts_bytes_rather_than_letters() {
        let bytes = encode(&json!({ "name": "café" }));
        let mut decoder = Decoder::new();
        let read = decoder.feed(&bytes).expect("a frame it wrote itself");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0]["name"], "café");
        assert_eq!(decoder.pending(), 0, "nothing is left over");
    }

    #[test]
    fn a_whole_frame_reads_as_one_message() {
        let mut decoder = Decoder::new();
        let read = decoder.feed(&encode(&message(3))).expect("a whole frame");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0]["seq"], 3);
    }

    /// Two frames in one read, which is what a pipe does whenever an adapter answers quickly.
    #[test]
    fn two_frames_in_one_read_are_two_messages() {
        let mut bytes = encode(&message(1));
        bytes.extend_from_slice(&encode(&message(2)));
        let mut decoder = Decoder::new();
        let read = decoder.feed(&bytes).expect("two whole frames");
        assert_eq!(read.len(), 2);
        assert_eq!(read[0]["seq"], 1);
        assert_eq!(read[1]["seq"], 2);
    }

    /// A frame torn in the middle of its body: nothing is answered until the rest arrives, and the
    /// bytes that did arrive are held rather than thrown away.
    #[test]
    fn a_frame_torn_in_its_body_waits_for_the_rest() {
        let bytes = encode(&message(7));
        let split = bytes.len() - 10;
        let mut decoder = Decoder::new();
        assert!(decoder.feed(&bytes[..split]).expect("half a frame").is_empty());
        assert_eq!(decoder.pending(), split, "the half that arrived is held");
        let read = decoder.feed(&bytes[split..]).expect("the other half");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0]["seq"], 7);
        assert_eq!(decoder.pending(), 0);
    }

    /// And one torn in the middle of its *header*, which is the case a decoder that looked for the
    /// separator only once would get wrong.
    #[test]
    fn a_frame_torn_in_its_header_waits_for_the_rest() {
        let bytes = encode(&message(9));
        let mut decoder = Decoder::new();
        assert!(decoder.feed(&bytes[..6]).expect("six bytes of a header").is_empty());
        assert!(decoder.feed(&bytes[6..12]).expect("six more").is_empty());
        let read = decoder.feed(&bytes[12..]).expect("the rest");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0]["seq"], 9);
    }

    /// One byte at a time, which is the worst a pipe can do and the strongest statement that the
    /// decoder holds no assumption about how much arrives at once.
    #[test]
    fn a_frame_read_one_byte_at_a_time_still_reads() {
        let bytes = encode(&message(11));
        let mut decoder = Decoder::new();
        let mut read = Vec::new();
        for byte in &bytes {
            read.extend(decoder.feed(&[*byte]).expect("a byte at a time"));
        }
        assert_eq!(read.len(), 1);
        assert_eq!(read[0]["seq"], 11);
    }

    #[test]
    fn a_header_unluminate_does_not_read_is_ignored() {
        let body = "{\"seq\":1}";
        let frame = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut decoder = Decoder::new();
        let read = decoder.feed(frame.as_bytes()).expect("a frame with a second header");
        assert_eq!(read.len(), 1);
    }

    #[test]
    fn the_header_name_is_matched_without_case() {
        let body = "{\"seq\":1}";
        let frame = format!("content-length: {}\r\n\r\n{body}", body.len());
        let mut decoder = Decoder::new();
        assert_eq!(decoder.feed(frame.as_bytes()).expect("a lower case header").len(), 1);
    }

    #[test]
    fn a_frame_with_no_length_is_refused_rather_than_guessed_at() {
        let mut decoder = Decoder::new();
        let problem = decoder.feed(b"Content-Type: text\r\n\r\n{}").expect_err("no length");
        assert!(matches!(problem, FrameError::NoLength(_)), "{problem:?}");
        assert!(problem.to_string().contains("Content-Length"));
    }

    #[test]
    fn a_length_that_is_not_a_number_is_refused() {
        let mut decoder = Decoder::new();
        let problem = decoder.feed(b"Content-Length: lots\r\n\r\n{}").expect_err("a bad length");
        assert_eq!(problem, FrameError::BadLength("lots".to_owned()));
    }

    /// A corrupted length must not become an allocation the size of the machine's memory.
    #[test]
    fn a_length_past_the_limit_is_refused_rather_than_reserved() {
        let mut decoder = Decoder::new();
        let problem = decoder
            .feed(b"Content-Length: 99999999999\r\n\r\n{}")
            .expect_err("a length past the limit");
        assert!(matches!(problem, FrameError::BadLength(_)), "{problem:?}");
    }

    #[test]
    fn a_body_that_is_not_json_says_so() {
        let mut decoder = Decoder::new();
        let problem = decoder.feed(b"Content-Length: 5\r\n\r\nnot!!").expect_err("not JSON");
        assert!(matches!(problem, FrameError::BadBody(_)), "{problem:?}");
    }

    /// Everything this crate writes, it can read. The round trip is what stops the two halves
    /// drifting apart when one of them is changed.
    #[test]
    fn everything_encode_writes_decode_reads() {
        let values = [
            json!({ "seq": 1, "type": "request", "command": "initialize" }),
            json!({ "seq": 2, "type": "response", "request_seq": 1, "success": true }),
            json!({ "seq": 3, "type": "event", "event": "output", "body": { "output": "hello\n" } }),
        ];
        let mut bytes = Vec::new();
        for value in &values {
            bytes.extend_from_slice(&encode(value));
        }
        let mut decoder = Decoder::new();
        let read = decoder.feed(&bytes).expect("its own frames");
        assert_eq!(read, values);
    }
}
