//! Base64, because a screenshot has to travel inside a JSON string.
//!
//! Written rather than depended on. It is the same decision `protocol.rs` made about serde's derive
//! macros and `services::control` made about its token: what is needed here is twenty lines of
//! arithmetic with no configuration and no edge case anybody argues about, and a crate for it would
//! be a dependency in the one program that is meant to stay small enough to start instantly.
//!
//! Standard alphabet, padded, no line breaks — RFC 4648 §4, which is what the Model Context
//! Protocol's `image` content block wants.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let packed = (group[0] as u32) << 16
            | (*group.get(1).unwrap_or(&0) as u32) << 8
            | *group.get(2).unwrap_or(&0) as u32;
        for sixth in 0..4 {
            // The last group is short, so the characters that stand for bytes that are not there
            // are padding instead. Three bytes give four characters, two give three, one gives two.
            if sixth <= group.len() {
                out.push(ALPHABET[(packed >> (18 - sixth * 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_agrees_with_the_examples_in_the_specification() {
        // RFC 4648 §10, which is the whole of what this has to get right.
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_png_header_encodes_the_way_every_other_encoder_spells_it() {
        assert_eq!(encode(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), "iVBORw0KGgo=");
    }

    #[test]
    fn the_length_is_always_a_multiple_of_four_and_every_byte_is_in_the_alphabet() {
        for length in 0..64usize {
            let bytes: Vec<u8> = (0..length).map(|at| (at * 7 % 251) as u8).collect();
            let encoded = encode(&bytes);
            assert_eq!(encoded.len() % 4, 0, "{length} bytes gave {}", encoded.len());
            assert!(encoded
                .bytes()
                .all(|byte| byte == b'=' || ALPHABET.contains(&byte)));
        }
    }
}
