//! Base64, written by hand.
//!
//! A picture goes up inside a JSON body, so it has to be base64, and it comes back out of a `data:`
//! URL the same way. Forty lines and a round trip test rather than a dependency for an alphabet,
//! which is the decision `services::control` already made about the wire format it speaks: this is a
//! small, completely specified thing, and a crate for it would be a crate to keep up to date.
//!
//! Standard alphabet with padding, which is what both APIs and every `data:` URL use. There is no
//! URL-safe variant here because nothing asks for one.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as base64 with padding.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let a = group[0] as u32;
        let b = *group.get(1).unwrap_or(&0) as u32;
        let c = *group.get(2).unwrap_or(&0) as u32;
        let packed = (a << 16) | (b << 8) | c;
        out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
        out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
        // The last group is short when the input is not a multiple of three, and the characters it
        // would have produced are padding rather than zeroes: `=` says "this is not data".
        out.push(match group.len() > 1 {
            true => ALPHABET[(packed >> 6) as usize & 63] as char,
            false => '=',
        });
        out.push(match group.len() > 2 {
            true => ALPHABET[packed as usize & 63] as char,
            false => '=',
        });
    }
    out
}

/// The bytes `text` encodes, or `None` when it is not base64.
///
/// Whitespace is skipped, because a `data:` URL pasted out of a document arrives wrapped; padding
/// ends the data.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut packed: u32 = 0;
    let mut have = 0;
    for byte in text.bytes() {
        // Whitespace is skipped because a `data:` URL pasted out of a document arrives wrapped at
        // seventy-six characters. Padding **ends** the data rather than being skipped, because that
        // is what it means, and because skipping it would read two payloads run together as one.
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            break;
        }
        let value = value_of(byte)?;
        packed = (packed << 6) | u32::from(value);
        have += 1;
        if have == 4 {
            out.push((packed >> 16) as u8);
            out.push((packed >> 8) as u8);
            out.push(packed as u8);
            packed = 0;
            have = 0;
        }
    }
    // A trailing group of two characters is one byte and one of three is two; a group of one is not
    // base64 at all, because six bits cannot be a byte.
    match have {
        0 => Some(out),
        2 => {
            out.push((packed >> 4) as u8);
            Some(out)
        }
        3 => {
            out.push((packed >> 10) as u8);
            out.push((packed >> 2) as u8);
            Some(out)
        }
        _ => None,
    }
}

/// Where `byte` sits in the alphabet.
fn value_of(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The media type and the bytes of a `data:` URL, or `None` when it is not one.
///
/// Only the base64 form is read. A `data:` URL can also be percent-encoded text, and a picture never
/// is, so reading one would be code for a case that cannot arise here.
pub fn from_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (head, body) = rest.split_once(',')?;
    let head = head.strip_suffix(";base64")?;
    let media = match head.is_empty() {
        true => "application/octet-stream".to_owned(),
        false => head.split(';').next().unwrap_or(head).to_owned(),
    };
    Some((media, decode(body)?))
}

/// A `data:` URL for these bytes.
pub fn to_data_url(media: &str, bytes: &[u8]) -> String {
    format!("data:{media};base64,{}", encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_length_round_trips() {
        // The interesting lengths are the ones either side of a group boundary, and the way to be
        // sure none of them is wrong is not to choose: every length up to a thousand, with bytes
        // that use the whole range so a sign error shows.
        for length in 0..1000 {
            let bytes: Vec<u8> = (0..length).map(|index| (index * 7 % 256) as u8).collect();
            let text = encode(&bytes);
            assert_eq!(text.len() % 4, 0, "base64 is a multiple of four characters");
            assert_eq!(
                decode(&text).as_deref(),
                Some(bytes.as_slice()),
                "at length {length}"
            );
        }
    }

    #[test]
    fn the_known_answers_are_the_known_answers() {
        // RFC 4648's own vectors, so the alphabet and the padding are checked against something
        // other than this file's own encoder.
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(decode("Zm9vYmFy").as_deref(), Some(&b"foobar"[..]));
    }

    #[test]
    fn something_that_is_not_base64_decodes_to_nothing_rather_than_to_rubbish() {
        assert!(decode("not base64!").is_none());
        // Six bits cannot be a byte, so a trailing group of one is refused rather than rounded.
        assert!(decode("Z").is_none());
        assert!(decode("Zm9vY").is_none());
        // Wrapped at seventy-six characters, which is how a `data:` URL arrives out of a document.
        assert_eq!(decode("Zm9v\nYmFy").as_deref(), Some(&b"foobar"[..]));
    }

    #[test]
    fn a_data_url_carries_its_media_type_and_its_bytes() {
        let url = to_data_url("image/png", &[0x89, 0x50, 0x4E, 0x47]);
        assert_eq!(url, "data:image/png;base64,iVBORw==");
        let (media, bytes) = from_data_url(&url).expect("a data url");
        assert_eq!(media, "image/png");
        assert_eq!(bytes, vec![0x89, 0x50, 0x4E, 0x47]);
        // A charset after the media type is ignored rather than taken as part of it.
        let (media, _) = from_data_url("data:image/jpeg;charset=binary;base64,//8=").expect("a data url");
        assert_eq!(media, "image/jpeg");
        // And something that is not one answers nothing rather than half an answer.
        assert!(from_data_url("https://example.com/cat.png").is_none());
        assert!(from_data_url("data:image/png,not-base64").is_none());
    }
}
