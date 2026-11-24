//! Base64, which ACP carries images in: what goes to an agent is encoded,
//! what an agent shows is decoded.

const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in base64, with its padding.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes `text` holds in base64; `None` if it is not base64. Space in
/// it is passed over, padding may be left off.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut n, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        n = n << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(n >> bits & 0xff).unwrap_or(0));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_go_to_base64_and_back_with_or_without_padding() {
        for (bytes, text) in [
            (&b""[..], ""),
            (b"a", "YQ=="),
            (b"ab", "YWI="),
            (b"abc", "YWJj"),
            (b"abcdef", "YWJjZGVm"),
        ] {
            assert_eq!(encode(bytes), text);
            assert_eq!(decode(text).unwrap(), bytes);
            assert_eq!(decode(text.trim_end_matches('=')).unwrap(), bytes);
        }
        assert!(decode("not base64!").is_none());
    }
}
