//! Binary bytes as Bovnar-safe text.
//!
//! Bovnar has no byte-string type, so the compact binary bodies of the wire codec travel in a
//! string. Base 85 with the Z85 alphabet is the densest plain encoding that needs no escaping in a
//! Bovnar string (no quote, no backslash, no control characters): 4 bytes become 5 characters,
//! a cost of 25 %, against 33 % for base 64. A trailing group of 1 to 3 bytes becomes 2 to 4
//! characters, so any length round-trips without padding.

const ALPHABET: &[u8; 85] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

/// The value of each ASCII character, or 0xff when it is not in the alphabet.
const VALUES: [u8; 128] = {
    let mut v = [0xff; 128];
    let mut i = 0;
    while i < ALPHABET.len() {
        v[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    v
};

/// The encoded length of `n` bytes.
pub const fn encoded_len(n: usize) -> usize {
    n / 4 * 5 + if n.is_multiple_of(4) { 0 } else { n % 4 + 1 }
}

/// The most bytes whose encoding fits in `chars` characters.
pub const fn max_bytes(chars: usize) -> usize {
    let tail = chars % 5;
    chars / 5 * 4 + if tail == 0 { 0 } else { tail - 1 }
}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(encoded_len(bytes.len()));
    for chunk in bytes.chunks(4) {
        let mut group = [0u8; 4];
        group[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(group);
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = ALPHABET[(value % 85) as usize];
            value /= 85;
        }
        // A short group keeps its leading digits: zero padding only touches the low digits.
        for &d in &digits[..chunk.len() + 1] {
            out.push(char::from(d));
        }
    }
    out
}

/// Decodes text from [`encode`]. Anything else (foreign characters, a lone trailing character,
/// a group above 2^32 - 1, or a short group that does not come from real bytes) is `None`.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    let mut out = Vec::with_capacity(max_bytes(text.len()));
    for chunk in text.chunks(5) {
        if chunk.len() == 1 {
            return None;
        }
        let mut value: u64 = 0;
        for i in 0..5 {
            // A short group is padded with the highest digit, so truncation rounds back down.
            let digit = match chunk.get(i) {
                Some(&c) => *VALUES.get(usize::from(c)).filter(|&&v| v != 0xff)?,
                None => 84,
            };
            value = value * 85 + u64::from(digit);
        }
        let value = u32::try_from(value).ok()?;
        let bytes = value.to_be_bytes();
        let n = chunk.len() - 1;
        out.extend_from_slice(&bytes[..n]);
        // Canonical form only: a short group must re-encode to exactly the same characters.
        if chunk.len() < 5 && encode(&bytes[..n]).as_bytes() != chunk {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn known_values() {
        // Z85's reference vector.
        let hello = [0x86, 0x4f, 0xd2, 0x6f, 0xb5, 0x59, 0xf7, 0x5b];
        assert_eq!(encode(&hello), "HelloWorld");
        assert_eq!(decode("HelloWorld").unwrap(), hello);
        assert_eq!(encode(&[]), "");
        assert_eq!(decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_bad_text() {
        assert_eq!(decode("a"), None);
        assert_eq!(decode("abcde1"), None);
        assert_eq!(decode("ab\"cd"), None);
        assert_eq!(decode("é"), None);
        // Above 2^32 - 1.
        assert_eq!(decode("#####"), None);
    }

    #[test]
    fn lengths_agree() {
        for n in 0..64 {
            assert_eq!(encode(&vec![0xab; n]).len(), encoded_len(n));
            assert!(encoded_len(max_bytes(n)) <= n);
            assert!(encoded_len(max_bytes(n) + 1) > n);
        }
    }

    proptest! {
        #[test]
        fn round_trips(bytes in prop::collection::vec(any::<u8>(), 0..300)) {
            let text = encode(&bytes);
            prop_assert!(text.bytes().all(|c| c.is_ascii_graphic() && c != b'"' && c != b'\\'));
            prop_assert_eq!(decode(&text), Some(bytes));
        }

        #[test]
        fn decode_never_panics(text in "\\PC{0,40}") {
            if let Some(bytes) = decode(&text) {
                prop_assert_eq!(encode(&bytes), text);
            }
        }
    }
}
