//! Server-sent events, read out of a byte stream that arrives in pieces nobody chose.
//!
//! This looks line-oriented and is not. A read from a socket can end in the middle of a `data:`
//! line, in the middle of a UTF-8 character, or exactly on the blank line that ends an event, and
//! all three happen against a real server. So this is a buffer that is fed whatever arrived and
//! yields whole events, which is `unluminate_dap::codec`'s arrangement for `Content-Length` framing and
//! is tested the same way: the same stream split at every byte boundary has to produce the same
//! events.
//!
//! Only the two fields either API uses are read — `event:` and `data:` — and a `data:` line that
//! arrives more than once in one event is joined with a newline, which is what the specification
//! says. `id:`, `retry:` and comment lines are skipped rather than reported, because neither API
//! sends one and a reader that reported them would have a case with no caller.

/// One event: what it was called, and what came with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The `event:` field. Empty when the server sent none, which is what OpenAI does.
    pub name: String,
    /// The `data:` field, with the lines of a multi-line one joined by newlines.
    pub data: String,
}

/// The most one event may be before the stream is treated as broken.
///
/// **A server can otherwise grow this without limit by never sending a blank line**, and the process
/// ends when the allocator gives up. Four megabytes is far more than any event either API sends — the
/// largest is a whole tool call's arguments — and small enough that a stream which never frames is
/// stopped in a second rather than in a minute.
pub const LARGEST_EVENT: usize = 4 * 1024 * 1024;

/// A buffer that turns bytes into events.
#[derive(Debug, Default)]
pub struct Reader {
    /// What has arrived and not yet been made into an event.
    ///
    /// Bytes rather than a `String`, because a read can end in the middle of a character and a
    /// `String` cannot hold half of one.
    buffer: Vec<u8>,
}

impl Reader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the stream has gone past [`LARGEST_EVENT`] with no event boundary in it.
    pub fn is_overlong(&self) -> bool {
        self.buffer.len() > LARGEST_EVENT
    }

    /// Feed in what just arrived, and take whatever events are now whole.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.next_boundary() {
            let block = self.buffer.drain(..end.taken).collect::<Vec<u8>>();
            let text = String::from_utf8_lossy(&block[..end.length]).into_owned();
            if let Some(event) = parse(&text) {
                events.push(event);
            }
        }
        events
    }

    /// Whatever is left in the buffer as an event, for a stream that ended with no blank line after
    /// its last one.
    ///
    /// Real servers do this — a connection closed cleanly after `data: [DONE]` with no trailing
    /// blank line — and a reader that dropped the last event would lose the one that says the answer
    /// is finished.
    pub fn finish(&mut self) -> Option<Event> {
        let left = std::mem::take(&mut self.buffer);
        let text = String::from_utf8_lossy(&left).into_owned();
        parse(&text)
    }

    /// Where the first event in the buffer ends: how many bytes it is, and how many to drop.
    ///
    /// A blank line ends an event, and a blank line is `\n\n` or `\r\n\r\n`. Both are looked for
    /// because both are sent: OpenAI's own servers use `\n\n` and several proxies re-wrap to CRLF.
    fn next_boundary(&self) -> Option<Boundary> {
        let mut at = 0;
        while at + 1 < self.buffer.len() {
            // **Two line endings of any spelling**, which is what the specification means by a blank
            // line: a newline, a carriage return and newline, and a lone carriage return are all one,
            // and they may be mixed. An earlier version looked for the first two only, so a stream
            // that used a lone carriage return — or that a proxy had re-wrapped to one — never framed
            // an event at all and the whole answer arrived as nothing.
            if let Some(first) = ending_at(&self.buffer, at) {
                if let Some(second) = ending_at(&self.buffer, at + first) {
                    return Some(Boundary {
                        length: at,
                        taken: at + first + second,
                    });
                }
            }
            at += 1;
        }
        None
    }
}

/// Where an event ends: the length of its text, and how much of the buffer it consumed.
struct Boundary {
    length: usize,
    taken: usize,
}

/// How many bytes of line ending start at `at`, or nothing when none does.
///
/// The pair is looked for before the lone carriage return, so `\r\n` is never read as two endings.
fn ending_at(buffer: &[u8], at: usize) -> Option<usize> {
    match buffer.get(at)? {
        b'\r' if buffer.get(at + 1) == Some(&b'\n') => Some(2),
        b'\r' | b'\n' => Some(1),
        _ => None,
    }
}

/// One block of `field: value` lines as an event, or `None` when it holds no data.
///
/// An event with no `data:` is a comment or a keep-alive. Both APIs send keep-alives on a slow
/// answer, and reporting them would make every reader filter them out.
fn parse(block: &str) -> Option<Event> {
    let mut name = String::new();
    let mut data: Option<String> = None;
    // Split on every spelling of a line ending rather than on `\n` alone, for the reason
    // `next_boundary` gives: a lone `\r` is a line ending and `str::lines` does not treat it as one.
    for line in block.split(['\n', '\r']) {
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        // "If the line starts with a colon, ignore the line" — a comment, which is what a keep-alive
        // usually is.
        if field.is_empty() {
            continue;
        }
        // "If value starts with a space, remove it." One space, not all of them: the payload's own
        // leading spaces are the payload's.
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => name = value.to_owned(),
            "data" => match &mut data {
                Some(so_far) => {
                    so_far.push('\n');
                    so_far.push_str(value);
                }
                none => *none = Some(value.to_owned()),
            },
            _ => {}
        }
    }
    data.map(|data| Event { name, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events in `stream`, read by feeding it in chunks of `size` bytes.
    fn read_in_chunks(stream: &[u8], size: usize) -> Vec<Event> {
        let mut reader = Reader::new();
        let mut events = Vec::new();
        for chunk in stream.chunks(size) {
            events.extend(reader.feed(chunk));
        }
        events.extend(reader.finish());
        events
    }

    const OPENAI: &[u8] = b"data: {\"a\":1}\n\ndata: {\"a\":2}\n\ndata: [DONE]\n\n";

    #[test]
    fn a_stream_split_at_every_boundary_reads_the_same() {
        // The one property that matters, because a socket chooses where a read ends and no test
        // could enumerate the ways it might. `unluminate_dap`'s framing has the same test.
        let whole = read_in_chunks(OPENAI, OPENAI.len());
        assert_eq!(whole.len(), 3);
        assert_eq!(whole[0].data, "{\"a\":1}");
        assert_eq!(whole[2].data, "[DONE]");
        for size in 1..=OPENAI.len() {
            assert_eq!(
                read_in_chunks(OPENAI, size),
                whole,
                "split into {size} byte chunks"
            );
        }
    }

    #[test]
    fn a_named_event_carries_its_name_and_crlf_is_read_too() {
        // Anthropic names every event, and a proxy in the middle may re-wrap the lines to CRLF.
        let stream =
            b"event: content_block_delta\r\ndata: {\"i\":0}\r\n\r\nevent: message_stop\r\ndata: {}\r\n\r\n";
        let events = read_in_chunks(stream, stream.len());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].name, "content_block_delta");
        assert_eq!(events[0].data, "{\"i\":0}");
        assert_eq!(events[1].name, "message_stop");
        for size in 1..=stream.len() {
            assert_eq!(
                read_in_chunks(stream, size).len(),
                2,
                "split into {size} byte chunks"
            );
        }
    }

    #[test]
    fn a_keep_alive_and_a_comment_are_not_events() {
        // Both APIs send these on a slow answer, and a reader that reported them would make every
        // caller filter them out.
        let stream = b": keep-alive\n\nevent: ping\n\ndata: {\"a\":1}\n\n";
        let events = read_in_chunks(stream, 3);
        assert_eq!(events.len(), 1, "only the one with data in it");
        assert_eq!(events[0].data, "{\"a\":1}");
    }

    #[test]
    fn a_data_field_sent_twice_in_one_event_is_joined_with_a_newline() {
        let stream = b"data: line one\ndata: line two\n\n";
        let events = read_in_chunks(stream, 5);
        assert_eq!(events[0].data, "line one\nline two");
    }

    #[test]
    fn a_stream_that_ends_with_no_blank_line_still_yields_its_last_event() {
        // Real servers close cleanly after `data: [DONE]` with nothing after it, and dropping that
        // event would lose the one that says the answer is finished.
        let stream = b"data: {\"a\":1}\n\ndata: [DONE]";
        let events = read_in_chunks(stream, 4);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].data, "[DONE]");
    }

    #[test]
    fn a_character_split_across_two_reads_is_not_corrupted() {
        // A read really can end in the middle of a UTF-8 character, which is why the buffer holds
        // bytes rather than a `String`.
        let stream = "data: {\"t\":\"café — ok\"}\n\n".as_bytes();
        for size in 1..=stream.len() {
            let events = read_in_chunks(stream, size);
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0].data, "{\"t\":\"café — ok\"}",
                "split into {size} byte chunks"
            );
        }
    }

    #[test]
    fn one_leading_space_is_removed_from_a_value_and_no_more() {
        // "If value starts with a space, remove it" — one, because a payload's own leading spaces
        // are part of the payload, and a token really can be "  indented".
        let stream = b"data:  two spaces\n\n";
        assert_eq!(read_in_chunks(stream, 2)[0].data, " two spaces");
    }
}
