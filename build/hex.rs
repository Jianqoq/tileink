//! Lowercase byte encoding shared by shader cache identities and compiler digests.

#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn encoding_preserves_leading_zeroes_and_all_byte_values() {
        assert_eq!(encode(&[]), "");
        assert_eq!(encode(&[0, 1, 15, 16, 255]), "00010f10ff");
        let bytes: Vec<_> = (0..=u8::MAX).collect();
        let text = encode(&bytes);
        assert_eq!(text.len(), bytes.len() * 2);
        assert!(
            text.bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        for (byte, encoded) in bytes.iter().zip(text.as_bytes().as_chunks::<2>().0) {
            assert_eq!(
                u8::from_str_radix(std::str::from_utf8(encoded).unwrap(), 16).unwrap(),
                *byte
            );
        }
    }
}
