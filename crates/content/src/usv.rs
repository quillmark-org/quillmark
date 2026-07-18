//! USV → UTF-8 byte conversion, the one coordinate crossing storage needs.
//!
//! [`Content`](crate::Content) positions count Unicode scalar values (USV,
//! Rust `char`) throughout, including at the WASM boundary — the `wasm`
//! binding's delta and hit-test APIs pass raw USV positions through and leave
//! any UTF-16 conversion to the JS caller. Two coordinate spaces disagree here:
//!
//! - **Rust / storage** — UTF-8 bytes. Slicing the content needs a byte offset.
//! - **USV** — the model's coordinate. One astral char is 1 USV / 4 UTF-8 bytes.

/// USV index → UTF-8 byte offset into `text`. Saturates to `text.len()` for an
/// index at or past the end, so it is safe to use as a slice bound.
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASTRAL: &str = "a😀b"; // 'a'(1u16) '😀'(2u16) 'b'(1u16) => 3 USV, 4 UTF-16

    #[test]
    fn char_to_byte_astral() {
        assert_eq!(char_to_byte(ASTRAL, 0), 0);
        assert_eq!(char_to_byte(ASTRAL, 1), 1); // start of 😀
        assert_eq!(char_to_byte(ASTRAL, 2), 5); // start of b (😀 is 4 bytes)
        assert_eq!(char_to_byte(ASTRAL, 3), 6); // end
        assert_eq!(char_to_byte(ASTRAL, 99), 6); // saturates
    }
}
