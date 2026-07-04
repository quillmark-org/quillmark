//! The cross-binding coordinate tax. Core stores USV (char) offsets; JS editors
//! (ProseMirror/Lexical/Quill/CodeMirror) count UTF-16 code units; Rust `&str`
//! is UTF-8 bytes. Every delta crossing WASM/Python and every editor binding
//! converts at its boundary. Astral-plane characters (emoji, rare CJK) are
//! where a naive `.len()` splits a code point and corrupts every downstream
//! offset — so the property suite owns this explicitly.

/// USV (char) offset → UTF-8 byte offset into `s`. Panics only if `usv` is past
/// the end (a caller bug). This is the Rust-internal conversion (`&str`
/// slicing needs byte offsets).
pub fn usv_to_byte(s: &str, usv: usize) -> usize {
    s.char_indices()
        .nth(usv)
        .map(|(b, _)| b)
        .unwrap_or_else(|| s.len())
}

/// UTF-8 byte offset → USV (char) offset. The inverse of [`usv_to_byte`] on
/// char boundaries.
pub fn byte_to_usv(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// USV (char) offset → UTF-16 code-unit offset. This is the JS-editor boundary:
/// a ProseMirror/Quill position is a UTF-16 index, so a delta arriving from JS
/// must convert before it can index the corpus. An astral char is 1 USV but 2
/// UTF-16 units.
pub fn usv_to_utf16(s: &str, usv: usize) -> usize {
    s.chars()
        .take(usv)
        .map(|c| c.len_utf16())
        .sum()
}

/// UTF-16 code-unit offset → USV (char) offset. Rounds a mid-surrogate index
/// down to the char that owns it (a JS editor never emits one, but a fuzzer
/// will).
pub fn utf16_to_usv(s: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in s.chars().enumerate() {
        let w = c.len_utf16();
        // `utf16` falls within char `i`'s unit span [units, units+w) — that
        // char owns it, including a mid-surrogate index (rounds down).
        if utf16 < units + w {
            return i;
        }
        units += w;
    }
    s.chars().count()
}
