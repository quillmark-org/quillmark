//! Low-level PDF byte-serialization shared by the stamp and flatten paths.
//!
//! This is the single home for "how we serialize a PDF object, a text string,
//! and the `/Info` `/Producer` stamp." Both the AcroForm stamp path (this crate)
//! and the value-flatten path (`quillmark-pdfform`) emit identical bytes here,
//! so the two can never drift.

use crate::error::PdfError;
use crate::reader::{
    assert_overwrite_gen_zero, err, extract_outer_dict, find_dict_value, find_object_bytes,
    splice_dict_value, UpdatedObject,
};

const CODE_PARSE: &str = "pdf::write";

/// Serialize one indirect object from its inner dict bytes:
/// `<id> 0 obj\n<< <inner> >>\nendobj\n`.
pub fn dict_object(id: u32, inner: &[u8]) -> UpdatedObject {
    let mut bytes = format!("{id} 0 obj\n<< ").into_bytes();
    bytes.extend_from_slice(inner);
    bytes.extend_from_slice(b" >>\nendobj\n");
    UpdatedObject { id, bytes }
}

/// Hand out the next object id from `next`, checked so a malformed near-`u32::MAX`
/// `/Size` yields a clean error rather than an overflow panic (debug) or a
/// silently-wrapped, colliding id (release).
pub fn alloc_id(next: &mut u32) -> Result<u32, PdfError> {
    let id = *next;
    *next = id.checked_add(1).ok_or_else(|| {
        err(
            CODE_PARSE,
            "PDF object id space exhausted (/Size too large)",
        )
    })?;
    Ok(id)
}

/// Escape bytes for a PDF literal string `( … )`: `(`, `)`, `\` → `\x`.
pub fn pdf_escape(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
}

/// Encode `s` as a PDF text string. ASCII uses a literal `( … )` with `(`, `)`
/// and `\` escaped; anything else uses a UTF-16BE hex string with a BOM.
pub fn pdf_text_string(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        let mut out = Vec::with_capacity(s.len() + 2);
        out.push(b'(');
        pdf_escape(&mut out, s.as_bytes());
        out.push(b')');
        out
    } else {
        let mut out = Vec::new();
        out.push(b'<');
        out.extend_from_slice(b"FEFF");
        for unit in s.encode_utf16() {
            out.extend_from_slice(format!("{unit:04X}").as_bytes());
        }
        out.push(b'>');
        out
    }
}

/// Replace `/Producer`'s value if present, else append the entry.
pub fn upsert_producer(info_dict: &[u8], literal: &[u8]) -> Vec<u8> {
    let key = b"/Producer";
    match find_dict_value(info_dict, "Producer") {
        None => {
            let mut out = info_dict.to_vec();
            out.extend_from_slice(b" /Producer ");
            out.extend_from_slice(literal);
            out
        }
        Some(value) => splice_dict_value(info_dict, key, value, literal),
    }
}

/// Stamp `/Info` `/Producer = producer`, pushing the updated (or freshly
/// created) `/Info` object onto `objects`.
///
/// `info_ref` is the trailer's `/Info` reference, if any. Returns `Some(info_id)`
/// when a *new* `/Info` object was allocated (the caller threads it into the
/// trailer), or `None` when the existing `/Info` was updated in place.
pub fn apply_producer_stamp(
    pdf: &[u8],
    info_ref: Option<(u32, u16)>,
    producer: &str,
    next_id: &mut u32,
    objects: &mut Vec<UpdatedObject>,
) -> Result<Option<u32>, PdfError> {
    let literal = pdf_text_string(producer);
    match info_ref {
        Some((info_id, _)) => {
            // The existing `/Info` object is overwritten in place at gen 0; a
            // non-zero-generation `/Info` would be silently corrupted.
            assert_overwrite_gen_zero(pdf, info_id, "/Info")?;
            let (s, e) = find_object_bytes(pdf, info_id)
                .ok_or_else(|| err(CODE_PARSE, format!("/Info object {info_id} not found")))?;
            let info_dict = extract_outer_dict(&pdf[s..e])
                .ok_or_else(|| err(CODE_PARSE, "/Info dict not parseable"))?;
            objects.push(dict_object(info_id, &upsert_producer(info_dict, &literal)));
            Ok(None)
        }
        None => {
            let info_id = alloc_id(next_id)?;
            let mut inner = b"/Producer ".to_vec();
            inner.extend_from_slice(&literal);
            objects.push(dict_object(info_id, &inner));
            Ok(Some(info_id))
        }
    }
}

/// Map one `char` to its WinAnsi (CP1252) byte, or `None` when WinAnsi cannot
/// represent it (anything outside Latin-1 plus the CP1252 `0x80..=0x9F` block).
///
/// Pairs with a base-14 font that declares `/Encoding /WinAnsiEncoding`: the
/// flatten path draws text directly into a content stream, so it must commit to
/// a byte encoding the font agrees with (unlike the stamp path, where the viewer
/// synthesizes appearances from a UTF-16 `/V`).
pub fn winansi_byte(c: char) -> Option<u8> {
    let cp = c as u32;
    match cp {
        // ASCII and the upper Latin-1 range are identity-mapped in WinAnsi.
        0x00..=0x7F | 0xA0..=0xFF => Some(cp as u8),
        // The CP1252 `0x80..=0x9F` block holds typographic punctuation at code
        // points elsewhere in Unicode.
        _ => match c {
            '\u{20AC}' => Some(0x80), // €
            '\u{201A}' => Some(0x82), // ‚
            '\u{0192}' => Some(0x83), // ƒ
            '\u{201E}' => Some(0x84), // „
            '\u{2026}' => Some(0x85), // …
            '\u{2020}' => Some(0x86), // †
            '\u{2021}' => Some(0x87), // ‡
            '\u{02C6}' => Some(0x88), // ˆ
            '\u{2030}' => Some(0x89), // ‰
            '\u{0160}' => Some(0x8A), // Š
            '\u{2039}' => Some(0x8B), // ‹
            '\u{0152}' => Some(0x8C), // Œ
            '\u{017D}' => Some(0x8E), // Ž
            '\u{2018}' => Some(0x91), // ‘
            '\u{2019}' => Some(0x92), // ’
            '\u{201C}' => Some(0x93), // “
            '\u{201D}' => Some(0x94), // ”
            '\u{2022}' => Some(0x95), // •
            '\u{2013}' => Some(0x96), // –
            '\u{2014}' => Some(0x97), // —
            '\u{02DC}' => Some(0x98), // ˜
            '\u{2122}' => Some(0x99), // ™
            '\u{0161}' => Some(0x9A), // š
            '\u{203A}' => Some(0x9B), // ›
            '\u{0153}' => Some(0x9C), // œ
            '\u{017E}' => Some(0x9E), // ž
            '\u{0178}' => Some(0x9F), // Ÿ
            _ => None,
        },
    }
}

/// Transcode `s` to WinAnsi (CP1252) bytes, substituting `?` for any code point
/// WinAnsi cannot represent. See [`winansi_byte`].
pub fn winansi_encode(s: &str) -> Vec<u8> {
    s.chars().map(|c| winansi_byte(c).unwrap_or(b'?')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winansi_ascii_is_identity() {
        assert_eq!(winansi_encode("Hello, world!"), b"Hello, world!");
    }

    #[test]
    fn winansi_latin1_and_cp1252_punctuation() {
        // é (U+00E9) → 0xE9; em-dash (U+2014) → 0x97; curly quote (U+2019) → 0x92.
        assert_eq!(
            winansi_encode("café—it’s"),
            &[b'c', b'a', b'f', 0xE9, 0x97, b'i', b't', 0x92, b's']
        );
    }

    #[test]
    fn winansi_unmappable_becomes_question_mark() {
        // CJK and emoji have no WinAnsi byte → '?'.
        assert_eq!(winansi_encode("日本語"), b"???");
        assert_eq!(winansi_encode("a😀b"), b"a?b");
    }

    #[test]
    fn pdf_text_string_escapes_ascii_literals() {
        assert_eq!(pdf_text_string("a(b)c\\d"), b"(a\\(b\\)c\\\\d)");
    }

    #[test]
    fn pdf_text_string_non_ascii_uses_utf16be_hex_with_bom() {
        // Any non-ASCII char tips the whole string into the UTF-16BE hex form:
        // `<FEFF` BOM then one 4-hex-digit code unit per UTF-16 unit.
        // "é" = U+00E9 → 00E9.
        assert_eq!(pdf_text_string("é"), b"<FEFF00E9>");
        // ASCII before/after a non-ASCII char are still emitted as their
        // UTF-16BE units (not as a literal string).
        assert_eq!(pdf_text_string("A€"), b"<FEFF004120AC>");
    }

    #[test]
    fn pdf_text_string_non_bmp_uses_surrogate_pair() {
        // U+1F600 (😀) is outside the BMP → a UTF-16 surrogate pair D83D DE00.
        assert_eq!(pdf_text_string("😀"), b"<FEFFD83DDE00>");
    }

    #[test]
    fn upsert_producer_replaces_existing_value() {
        let info = b"/Title (Hi) /Producer (Old) /Creator (X)";
        let out = upsert_producer(info, b"(New)");
        assert_eq!(&out, b"/Title (Hi) /Producer (New) /Creator (X)");
    }

    #[test]
    fn upsert_producer_appends_when_absent() {
        let info = b"/Title (Hi)";
        let out = upsert_producer(info, b"(New)");
        assert_eq!(&out, b"/Title (Hi) /Producer (New)");
    }

    #[test]
    fn upsert_producer_ignores_producer_name_in_value_position() {
        // A `/Producer` Name in *value* position (here as the value of
        // `/Marker`) must not be overwritten as if it were the key — doing so
        // would clobber the wrong token and drop a trailing entry.
        let info = b"/Title (Hi) /Marker /Producer /Creator (X)";
        let out = upsert_producer(info, b"(New)");
        // No real /Producer key exists, so the entry is appended; the /Marker
        // value and /Creator entry are left intact.
        assert_eq!(
            &out,
            b"/Title (Hi) /Marker /Producer /Creator (X) /Producer (New)"
        );
    }
}
