//! What a file was on disk, kept so that writing it back does not change it.
//!
//! A `Document` holds a `String`, and a `String` says nothing about the bytes it was decoded from.
//! Two facts about those bytes have to survive a round trip or an edit silently rewrites the whole
//! file: **what its line breaks were**, and **what its characters were encoded as**.
//!
//! `task-1804` measured the first one. A three line file written with `\r\n`, opened, one character
//! typed, saved, came back with every line ending rewritten to `\n` — a one character edit producing
//! a whole file diff. On this machine, where `core.autocrlf` is set, a git checkout is enough to put
//! every file in the working tree into that state; on a repository not using it the conversion is
//! permanent.
//!
//! The normalisation itself is right and `task-1794` explains why: offsets and line counts need one
//! meaning, and a breakpoint sent to a debugger from a file read raw was landing about fifty bytes
//! early. The fault was only that the write did not undo it. So the reading stays exactly as it was
//! and what it *found* is now a value the document carries.

/// What a file's line breaks were.
///
/// Every one of these is normalised to `\n` in the text a `Document` holds — see
/// [`crate::document::read_to_normalised_string`] for why there is only ever one meaning of an
/// offset — and written back out as whatever was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// `\n`. Everything but Windows, and a good deal of Windows.
    #[default]
    Lf,
    /// `\r\n`.
    Crlf,
    /// A lone `\r`, which is what a Macintosh wrote until 2001. Rare enough that it is here to be
    /// *kept* rather than to be catered for: a file that arrives this way is not silently converted
    /// into one of the other two by being opened and saved.
    Cr,
}

impl LineEnding {
    /// What a new file gets: the platform's own, because a file this machine creates should look
    /// like the files this machine already has.
    pub fn platform_default() -> Self {
        if cfg!(windows) {
            Self::Crlf
        } else {
            Self::Lf
        }
    }

    /// The bytes this ending is written as.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
            Self::Cr => "\r",
        }
    }

    /// What the status bar and `editor info` call it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
            Self::Cr => "CR",
        }
    }

    /// Read a name back, for a settings key and for `editor line-ending --set`. Case is not
    /// significant and the long spellings are accepted, because they are what a person types.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "lf" | "unix" | "\n" => Some(Self::Lf),
            "crlf" | "windows" | "dos" | "\r\n" => Some(Self::Crlf),
            "cr" | "mac" | "classic-mac" | "\r" => Some(Self::Cr),
            _ => None,
        }
    }

    /// Which ending dominates `text`, as read from disk before it was normalised.
    ///
    /// **Whichever there are most of wins, and a file with none at all gets the platform's.** A file
    /// is very often mixed — a generator wrote one kind and a person's editor wrote another — and in
    /// that case there is no answer that leaves every line alone. Counting is the reading that
    /// changes the fewest of them.
    ///
    /// A lone `\r` is only counted where it is not part of a `\r\n`, which is what makes the three
    /// counts add up to the number of line breaks in the file.
    pub fn dominant_in(text: &str) -> Self {
        let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\r' => {
                    if bytes.get(index + 1) == Some(&b'\n') {
                        crlf += 1;
                        index += 1;
                    } else {
                        cr += 1;
                    }
                }
                b'\n' => lf += 1,
                _ => {}
            }
            index += 1;
        }
        if crlf == 0 && lf == 0 && cr == 0 {
            return Self::platform_default();
        }
        if crlf >= lf && crlf >= cr {
            Self::Crlf
        } else if lf >= cr {
            Self::Lf
        } else {
            Self::Cr
        }
    }

    /// Turn the normalised text a `Document` holds back into the bytes of a file with this ending.
    ///
    /// Nothing is allocated for `Lf`, which is the overwhelmingly common case and also the case a
    /// large file is most likely to be.
    pub fn apply(self, normalised: &str) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Lf => std::borrow::Cow::Borrowed(normalised),
            Self::Crlf => std::borrow::Cow::Owned(normalised.replace('\n', "\r\n")),
            Self::Cr => std::borrow::Cow::Owned(normalised.replace('\n', "\r")),
        }
    }
}

/// What a file's characters were encoded as.
///
/// `task-1804` §7.6: `Document::open` used `read_to_string`, so anything that was not UTF-8 could not
/// be opened at all, and the only place that was said was a line in the status bar. Refusing is
/// defensible — mangling bytes into replacement characters and writing them back would be worse —
/// but being unable to *look* at a file is a poor answer when the bytes are unambiguous.
///
/// So the two shapes that *are* unambiguous are read, and read **read-only**: a file with a UTF-16
/// byte order mark, and a file that is not valid UTF-8 at all, which is read as Latin-1 because every
/// byte has a meaning there and so the reading cannot fail. Neither can be written back, and
/// [`Encoding::writable`] is what says so — a re-encoding this version has not been asked to get
/// right is a re-encoding it should not attempt on somebody's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// UTF-8 with no byte order mark. Everything, nearly.
    #[default]
    Utf8,
    /// UTF-8 with a byte order mark, which is written back out again so the file is unchanged.
    Utf8Bom,
    /// UTF-16, little endian, with a byte order mark. Read-only.
    Utf16Le,
    /// UTF-16, big endian, with a byte order mark. Read-only.
    Utf16Be,
    /// Not valid UTF-8, read a byte to a code point. Read-only.
    ///
    /// This is a guess and it is named as one wherever it is shown, because Latin-1 is what a byte
    /// *can* always be read as rather than what the file necessarily is.
    Latin1,
}

impl Encoding {
    /// What the status bar calls it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 with BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Latin1 => "Latin-1",
        }
    }

    /// Whether Unluminous will write a file back in this encoding.
    ///
    /// Only the two UTF-8 shapes, and the reason is in the type's own comment: the others are opened
    /// so that a file can be read, not so that it can be re-encoded.
    pub fn writable(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf8Bom)
    }

    /// The bytes that go in front of the text when it is written.
    pub fn prefix(self) -> &'static [u8] {
        match self {
            Self::Utf8Bom => &[0xEF, 0xBB, 0xBF],
            _ => &[],
        }
    }
}

/// A file's bytes, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The text, with every line break normalised to `\n`.
    pub text: String,
    /// What its line breaks were, so they can be written back.
    pub line_ending: LineEnding,
    /// What its characters were.
    pub encoding: Encoding,
}

/// Read bytes into text, saying what they were.
///
/// The order is the order the answers are certain in: a byte order mark says what a file is and is
/// believed, valid UTF-8 is UTF-8, and anything left is read as Latin-1 rather than refused.
pub fn decode(bytes: &[u8]) -> Decoded {
    let (text, encoding) = decode_characters(bytes);
    let line_ending = LineEnding::dominant_in(&text);
    Decoded { text: normalise_line_breaks(&text), line_ending, encoding }
}

/// Every line break turned into `\n`, whichever of the three it was.
///
/// `\r\n` first and the remaining lone `\r` after it, so that a Windows break is never read as two
/// breaks. `task-1804` §7.5: before this, `printf 'one\rtwo\r'` opened as one line, so a
/// classic-Macintosh file was a single line however long it was.
pub fn normalise_line_breaks(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The characters, without touching the line breaks.
fn decode_characters(bytes: &[u8]) -> (String, Encoding) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(text) = std::str::from_utf8(&bytes[3..]) {
            return (text.to_owned(), Encoding::Utf8Bom);
        }
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (decode_utf16(&bytes[2..], false), Encoding::Utf16Le);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return (decode_utf16(&bytes[2..], true), Encoding::Utf16Be);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), Encoding::Utf8),
        // Every byte is a code point in Latin-1, so this reading cannot fail — which is the whole
        // reason it is the fallback rather than one of the many encodings that can.
        Err(_) => (bytes.iter().map(|&byte| byte as char).collect(), Encoding::Latin1),
    }
}

/// UTF-16 code units into a string, replacing an unpaired surrogate rather than refusing the file.
fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    char::decode_utf16(units).map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_file_is_read_as_crlf_and_written_back_as_crlf() {
        let decoded = decode(b"one\r\ntwo\r\n");
        assert_eq!(decoded.line_ending, LineEnding::Crlf);
        assert_eq!(decoded.text, "one\ntwo\n");
        assert_eq!(decoded.line_ending.apply(&decoded.text), "one\r\ntwo\r\n");
    }

    #[test]
    fn a_unix_file_is_read_as_lf_and_costs_no_allocation_to_write() {
        let decoded = decode(b"one\ntwo\n");
        assert_eq!(decoded.line_ending, LineEnding::Lf);
        assert!(matches!(decoded.line_ending.apply(&decoded.text), std::borrow::Cow::Borrowed(_)));
    }

    /// `task-1804` §7.5. A lone `\r` used to leave the whole file on one line.
    #[test]
    fn a_classic_macintosh_file_is_three_lines_and_stays_one_carriage_return_a_line() {
        let decoded = decode(b"one\rtwo\rthree\r");
        assert_eq!(decoded.line_ending, LineEnding::Cr);
        assert_eq!(decoded.text.lines().count(), 3);
        assert_eq!(decoded.line_ending.apply(&decoded.text), "one\rtwo\rthree\r");
    }

    #[test]
    fn a_mixed_file_takes_whichever_ending_there_are_most_of() {
        assert_eq!(LineEnding::dominant_in("a\r\nb\r\nc\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::dominant_in("a\nb\nc\r\n"), LineEnding::Lf);
    }

    #[test]
    fn a_file_with_no_line_breaks_at_all_gets_the_platform_ending() {
        assert_eq!(LineEnding::dominant_in("no breaks here"), LineEnding::platform_default());
    }

    #[test]
    fn a_byte_order_mark_is_read_and_written_back() {
        let decoded = decode(b"\xEF\xBB\xBFhello");
        assert_eq!(decoded.encoding, Encoding::Utf8Bom);
        assert_eq!(decoded.text, "hello");
        assert_eq!(decoded.encoding.prefix(), b"\xEF\xBB\xBF");
        assert!(decoded.encoding.writable());
    }

    #[test]
    fn utf16_with_a_mark_is_read_and_is_read_only() {
        let little = decode(&[0xFF, 0xFE, b'h', 0, b'i', 0]);
        assert_eq!(little.encoding, Encoding::Utf16Le);
        assert_eq!(little.text, "hi");
        assert!(!little.encoding.writable());

        let big = decode(&[0xFE, 0xFF, 0, b'h', 0, b'i']);
        assert_eq!(big.encoding, Encoding::Utf16Be);
        assert_eq!(big.text, "hi");
    }

    /// The measurement in §7.2: this is the file `tab open` used to report success for.
    #[test]
    fn bytes_that_are_not_utf8_are_read_as_latin1_rather_than_refused() {
        let decoded = decode(b"caf\xE9\n");
        assert_eq!(decoded.encoding, Encoding::Latin1);
        assert_eq!(decoded.text, "café\n");
        assert!(!decoded.encoding.writable());
    }

    #[test]
    fn a_name_is_read_back_however_it_is_spelt() {
        assert_eq!(LineEnding::from_name("CRLF"), Some(LineEnding::Crlf));
        assert_eq!(LineEnding::from_name(" windows "), Some(LineEnding::Crlf));
        assert_eq!(LineEnding::from_name("unix"), Some(LineEnding::Lf));
        assert_eq!(LineEnding::from_name("ebcdic"), None);
    }
}
