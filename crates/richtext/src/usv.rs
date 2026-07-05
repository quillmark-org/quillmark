//! Coordinate conversions across the binding boundary.
//!
//! [`RichText`](crate::RichText) positions count Unicode scalar values (USV,
//! Rust `char`). The three boundaries that disagree:
//!
//! - **Rust / storage** — UTF-8 bytes. Slicing the corpus needs a byte offset.
//! - **JS editors** — UTF-16 code units. Every delta crossing WASM converts.
//! - **USV** — the model's coordinate. One astral char is 1 USV / 4 UTF-8 bytes
//!   / 2 UTF-16 units.
//!
//! A UTF-16 index landing on a low surrogate (the tail half of an astral pair)
//! rounds **down** to the char that owns it — the standing cross-binding rule
//! the property suite pins.

/// USV index → UTF-8 byte offset into `text`. Saturates to `text.len()` for an
/// index at or past the end, so it is safe to use as a slice bound.
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// USV index → UTF-16 code-unit index. Saturates to the UTF-16 length past the
/// end.
pub fn char_to_utf16(text: &str, char_idx: usize) -> usize {
    text.chars()
        .take(char_idx)
        .map(|c| c.len_utf16())
        .sum()
}

/// UTF-16 code-unit index → USV index. An index that lands mid-pair (on a low
/// surrogate) rounds down to the owning char. Saturates to the USV length past
/// the end.
pub fn utf16_to_char(text: &str, utf16_idx: usize) -> usize {
    let mut units = 0usize;
    for (char_idx, c) in text.chars().enumerate() {
        if units >= utf16_idx {
            return char_idx;
        }
        units += c.len_utf16();
        // If the target index fell strictly inside this char's pair, we passed
        // it — this char owns it, so round down to it.
        if units > utf16_idx {
            return char_idx;
        }
    }
    text.chars().count()
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

    #[test]
    fn char_to_utf16_astral() {
        assert_eq!(char_to_utf16(ASTRAL, 0), 0);
        assert_eq!(char_to_utf16(ASTRAL, 1), 1); // before 😀
        assert_eq!(char_to_utf16(ASTRAL, 2), 3); // after 😀 (2 units)
        assert_eq!(char_to_utf16(ASTRAL, 3), 4); // end
    }

    #[test]
    fn utf16_to_char_astral() {
        assert_eq!(utf16_to_char(ASTRAL, 0), 0);
        assert_eq!(utf16_to_char(ASTRAL, 1), 1); // before 😀
        assert_eq!(utf16_to_char(ASTRAL, 2), 1); // mid-surrogate -> rounds down to 😀
        assert_eq!(utf16_to_char(ASTRAL, 3), 2); // after 😀
        assert_eq!(utf16_to_char(ASTRAL, 4), 3); // end
        assert_eq!(utf16_to_char(ASTRAL, 99), 3); // saturates
    }

    #[test]
    fn round_trip_char_utf16() {
        // Every char boundary round-trips (mid-surrogate indices don't exist as
        // char positions, so we only check char starts).
        for i in 0..=ASTRAL.chars().count() {
            let u16i = char_to_utf16(ASTRAL, i);
            assert_eq!(utf16_to_char(ASTRAL, u16i), i, "char {i}");
        }
    }
}
