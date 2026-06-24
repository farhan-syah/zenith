//! Minimal, dependency-free base64 encoder (RFC 4648, standard alphabet with
//! padding).
//!
//! The MCP `resources/read` response carries binary artifacts (PNG/PDF) as a
//! base64 `blob` string. Rather than add a `base64` crate dependency, this is a
//! ~30-line pure-Rust encoder — keeping the dependency graph flat and trivially
//! C-free (the repo invariant). Only encoding is needed; nothing here decodes.

/// The standard RFC 4648 alphabet (index → output character).
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode `input` as standard base64 with `=` padding.
pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);

    for chunk in input.chunks(3) {
        // Pack up to three bytes into a 24-bit big-endian group. Missing bytes
        // (in the final short chunk) are treated as zero.
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let group = (b0 << 16) | (b1 << 8) | b2;

        // Emit four 6-bit symbols; pad the tail when the chunk was short.
        out.push(ALPHABET[((group >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((group >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((group >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(group & 0x3f) as usize] as char
        } else {
            '='
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 4648 §10 test vectors.
    #[test]
    fn rfc4648_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encodes_high_bytes() {
        // 0xFF 0xFF 0xFF -> all ones -> "////".
        assert_eq!(encode(&[0xff, 0xff, 0xff]), "////");
        // 0x00 0x00 0x00 -> "AAAA".
        assert_eq!(encode(&[0x00, 0x00, 0x00]), "AAAA");
        // Single 0xFF -> "/w==".
        assert_eq!(encode(&[0xff]), "/w==");
    }

    #[test]
    fn output_len_is_multiple_of_four() {
        for n in 0..50 {
            let bytes = vec![0xab; n];
            assert_eq!(encode(&bytes).len() % 4, 0, "len {n}");
        }
    }
}
