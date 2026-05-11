//! Minimal byte-level PDF scanner used to parse a typst_pdf output well
//! enough to do an incremental update.
//!
//! Scope is deliberately narrow: typst_pdf emits a traditional xref table
//! (not an xref stream) and a small catalog/page tree. We don't aim to parse
//! every PDF in the wild.

use super::SigOverlayError;

/// The offset stored after the last `startxref` marker.
pub(super) fn find_startxref(pdf: &[u8]) -> Result<usize, SigOverlayError> {
    let needle = b"startxref";
    let from = pdf.len().saturating_sub(1024);
    let tail = &pdf[from..];
    let pos = tail
        .windows(needle.len())
        .rposition(|w| w == needle)
        .ok_or(SigOverlayError::MissingStartxref)?;
    let after = skip_ws(&tail[pos + needle.len()..]);
    let mut end = 0;
    while end < after.len() && after[end].is_ascii_digit() {
        end += 1;
    }
    if end == 0 {
        return Err(SigOverlayError::MissingStartxref);
    }
    std::str::from_utf8(&after[..end])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(SigOverlayError::MissingStartxref)
}

/// Confirm typst_pdf is emitting a traditional `xref` table at `xref_offset`.
/// If it's emitting an xref stream we bail — we don't handle that.
pub(super) fn assert_traditional_xref(
    pdf: &[u8],
    xref_offset: usize,
) -> Result<(), SigOverlayError> {
    if pdf.get(xref_offset..xref_offset + 4) != Some(b"xref") {
        return Err(SigOverlayError::XrefStreamUnsupported);
    }
    Ok(())
}

/// Parse the traditional trailer that follows the xref table.
/// Returns (catalog_id, /Size, encrypted?).
pub(super) fn parse_traditional_trailer(
    pdf: &[u8],
    xref_offset: usize,
) -> Result<(u32, u32, bool), SigOverlayError> {
    let needle = b"trailer";
    let from = xref_offset;
    let pos = pdf[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or(SigOverlayError::MissingTrailer)?
        + from;
    let dict =
        extract_outer_dict(&pdf[pos + needle.len()..]).ok_or(SigOverlayError::MissingTrailer)?;
    let root_bytes = find_dict_value(dict, "Root").ok_or(SigOverlayError::MissingRoot)?;
    let (root_id, _) = parse_indirect_ref(root_bytes).ok_or(SigOverlayError::MissingRoot)?;
    let size = find_dict_value(dict, "Size")
        .and_then(parse_int)
        .ok_or(SigOverlayError::MissingSize)? as u32;
    let encrypted = find_dict_value(dict, "Encrypt").is_some();
    Ok((root_id, size, encrypted))
}

/// Locate object `id` via linear scan and return `(obj_start, endobj_end)`.
pub(super) fn find_object_bytes(pdf: &[u8], id: u32) -> Option<(usize, usize)> {
    let header = format!("{} 0 obj", id);
    let h = header.as_bytes();
    let mut i = 0;
    while i + h.len() < pdf.len() {
        if pdf[i..].starts_with(h) && (i == 0 || matches!(pdf[i - 1], b'\n' | b'\r' | b' ')) {
            let needle = b"endobj";
            let from = i + h.len();
            let rest = &pdf[from..];
            let end_rel = rest.windows(needle.len()).position(|w| w == needle)?;
            return Some((i, from + end_rel + needle.len()));
        }
        i += 1;
    }
    None
}

/// Within a dict's inner bytes, locate `/Key` and return its raw value slice.
///
/// Tokenises the value so that Name values (`/Foo`), arrays (`[...]`), inner
/// dicts (`<<...>>`), strings (`(...)`), numbers, and indirect refs
/// (`N G R`) all terminate cleanly. The spike's shallow scanner mistook the
/// `/Pages` in `/Type /Pages` for the next dict entry.
pub(super) fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_marker = format!("/{}", key);
    let km = key_marker.as_bytes();
    let mut i = 0;
    let mut depth_dict = 0i32;
    let mut depth_array = 0i32;
    while i < dict_bytes.len() {
        // Literal strings can contain anything (including `/`, `<<`, `[`, `]`).
        // Skip them as opaque tokens before we try to match the key.
        if dict_bytes[i] == b'(' {
            i = skip_pdf_string(dict_bytes, i);
            continue;
        }
        if dict_bytes[i..].starts_with(b"<<") {
            depth_dict += 1;
            i += 2;
            continue;
        }
        if dict_bytes[i..].starts_with(b">>") {
            depth_dict -= 1;
            i += 2;
            continue;
        }
        // Hex strings (`<deadbeef>`) — single `<` not part of `<<`.
        if dict_bytes[i] == b'<' {
            i = skip_pdf_hex_string(dict_bytes, i);
            continue;
        }
        match dict_bytes[i] {
            b'[' => {
                depth_array += 1;
                i += 1;
            }
            b']' => {
                depth_array -= 1;
                i += 1;
            }
            _ if depth_dict == 0
                && depth_array == 0
                && dict_bytes[i..].starts_with(km) =>
            {
                let next = dict_bytes.get(i + km.len()).copied();
                if matches!(
                    next,
                    Some(b' ')
                        | Some(b'\t')
                        | Some(b'\n')
                        | Some(b'\r')
                        | Some(b'/')
                        | Some(b'[')
                        | Some(b'<')
                        | Some(b'(')
                ) {
                    let start = i + km.len();
                    let end = read_value_end(dict_bytes, start)?;
                    return Some(&dict_bytes[start..end]);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Find the byte index where a value beginning at `start` ends. Returns the
/// index AFTER the value's last byte. Whitespace at `start` is skipped before
/// classifying the value type.
fn read_value_end(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
        i += 1;
    }
    if i >= b.len() {
        return Some(i);
    }
    match b[i] {
        b'[' => {
            let mut depth = 1;
            i += 1;
            while i < b.len() {
                if b[i] == b'(' {
                    i = skip_pdf_string(b, i);
                    continue;
                }
                if b[i] == b'[' {
                    depth += 1;
                } else if b[i] == b']' {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                i += 1;
            }
            Some(i)
        }
        b'(' => Some(skip_pdf_string(b, i)),
        b'<' if b[i..].starts_with(b"<<") => {
            let mut depth = 1;
            i += 2;
            while i + 1 < b.len() && depth > 0 {
                if b[i..].starts_with(b"<<") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b">>") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(i)
        }
        b'<' => Some(skip_pdf_hex_string(b, i)),
        b'/' => {
            // Name — read until whitespace or delimiter.
            i += 1;
            while i < b.len() && !is_pdf_delim(b[i]) {
                i += 1;
            }
            Some(i)
        }
        c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
            // Number, possibly followed by `G R` for an indirect reference.
            let num_end = read_number_end(b, i);
            let save = num_end;
            let mut j = num_end;
            while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            let n2_start = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > n2_start {
                while j < b.len() && matches!(b[j], b' ' | b'\t' | b'\n' | b'\r') {
                    j += 1;
                }
                if b.get(j).copied() == Some(b'R') {
                    // Confirm the `R` is standalone — guards against e.g.
                    // `5 0 Rect` being misread as an indirect ref.
                    let stands_alone = b
                        .get(j + 1)
                        .map_or(true, |c| is_pdf_delim(*c));
                    if stands_alone {
                        return Some(j + 1);
                    }
                }
            }
            Some(save)
        }
        _ => {
            // Boolean / null / unknown — read one word.
            while i < b.len() && !is_pdf_delim(b[i]) {
                i += 1;
            }
            Some(i)
        }
    }
}

fn read_number_end(b: &[u8], start: usize) -> usize {
    let mut i = start;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    i
}

fn skip_pdf_string(b: &[u8], start: usize) -> usize {
    // start should point at `(`. Returns index AFTER matching `)`.
    let mut i = start + 1;
    let mut depth = 1;
    while i < b.len() && depth > 0 {
        match b[i] {
            b'\\' => i = (i + 2).min(b.len()),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    i
}

/// Skip a hex string `<...>`. `start` points at the leading `<`. Returns
/// index AFTER the closing `>`. The caller has already confirmed this is
/// NOT a `<<` token.
fn skip_pdf_hex_string(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < b.len() && b[i] != b'>' {
        i += 1;
    }
    if i < b.len() {
        i + 1
    } else {
        i
    }
}

fn is_pdf_delim(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'\x0c'
            | b'/'
            | b'['
            | b']'
            | b'('
            | b')'
            | b'<'
            | b'>'
    )
}

pub(super) fn parse_indirect_ref(s: &[u8]) -> Option<(u32, u16)> {
    let s = skip_ws(s);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let id: u32 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let gen: u16 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    let s = skip_ws(&s[i..]);
    if !s.starts_with(b"R") {
        return None;
    }
    // `R` must stand alone — not be a prefix of an identifier like `Roller`.
    match s.get(1).copied() {
        None => {}
        Some(c) if is_pdf_delim(c) => {}
        _ => return None,
    }
    Some((id, gen))
}

pub(super) fn parse_int(s: &[u8]) -> Option<i64> {
    let s = skip_ws(s);
    let (negate, s) = if s.starts_with(b"-") {
        (true, &s[1..])
    } else {
        (false, s)
    };
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let n: i64 = std::str::from_utf8(&s[..i]).ok()?.parse().ok()?;
    Some(if negate { -n } else { n })
}

/// Slice between the outermost `<< ... >>` of an indirect object's body.
pub(super) fn extract_outer_dict(obj_bytes: &[u8]) -> Option<&[u8]> {
    let open = obj_bytes.windows(2).position(|w| w == b"<<")?;
    let mut depth = 0i32;
    let mut i = open;
    while i + 1 < obj_bytes.len() {
        // Skip literal strings — they can contain `<<` / `>>` as raw bytes.
        if obj_bytes[i] == b'(' {
            i = skip_pdf_string(obj_bytes, i);
            continue;
        }
        if obj_bytes[i..].starts_with(b"<<") {
            depth += 1;
            i += 2;
        } else if obj_bytes[i..].starts_with(b">>") {
            depth -= 1;
            if depth == 0 {
                return Some(&obj_bytes[open + 2..i]);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

fn skip_ws(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
        i += 1;
    }
    &s[i..]
}

/// Resolve the catalog's `/Pages` tree into a flat list of page object IDs,
/// in document order. Recurses into `/Type /Pages` nodes via `/Kids`.
pub(super) fn resolve_page_ids(pdf: &[u8], catalog_id: u32) -> Result<Vec<u32>, SigOverlayError> {
    let (cs, ce) = find_object_bytes(pdf, catalog_id).ok_or(SigOverlayError::MissingCatalog)?;
    let cat_dict = extract_outer_dict(&pdf[cs..ce]).ok_or(SigOverlayError::MissingCatalog)?;
    let (root_pages_id, _) = parse_indirect_ref(
        find_dict_value(cat_dict, "Pages").ok_or(SigOverlayError::MissingPagesRoot)?,
    )
    .ok_or(SigOverlayError::MissingPagesRoot)?;

    let mut out = Vec::new();
    let mut stack = vec![root_pages_id];
    let mut visited = Vec::new();

    while let Some(node_id) = stack.pop() {
        if visited.contains(&node_id) {
            return Err(SigOverlayError::PageTreeCycle { node: node_id });
        }
        visited.push(node_id);
        let (s, e) = find_object_bytes(pdf, node_id).ok_or(SigOverlayError::MissingPageNode {
            id: node_id,
        })?;
        let dict = extract_outer_dict(&pdf[s..e]).ok_or(SigOverlayError::MissingPageNode {
            id: node_id,
        })?;
        let typ = find_dict_value(dict, "Type")
            .map(|b| String::from_utf8_lossy(b.trim_ascii()).into_owned())
            .unwrap_or_default();
        if typ.starts_with("/Pages") {
            // Internal node — recurse into Kids in document order. We
            // pushed via stack so reverse for left-to-right order.
            let kids = find_dict_value(dict, "Kids")
                .ok_or(SigOverlayError::MissingPageNode { id: node_id })?;
            let mut kid_ids: Vec<u32> = parse_ref_array(kids).into_iter().map(|(id, _)| id).collect();
            kid_ids.reverse();
            for k in kid_ids {
                stack.push(k);
            }
        } else {
            // Leaf — treat as a /Type /Page (default).
            out.push(node_id);
        }
    }
    Ok(out)
}

/// Parse `[N G R N G R ...]` into a vec of `(id, gen)`.
pub(super) fn parse_ref_array(bytes: &[u8]) -> Vec<(u32, u16)> {
    let mut s = bytes;
    if let Some(l) = s.iter().position(|&b| b == b'[') {
        s = &s[l + 1..];
    }
    if let Some(r) = s.iter().position(|&b| b == b']') {
        s = &s[..r];
    }
    let mut out = Vec::new();
    let mut cur = s;
    loop {
        cur = skip_ws(cur);
        if cur.is_empty() {
            break;
        }
        match parse_indirect_ref(cur) {
            Some((id, gen)) => {
                out.push((id, gen));
                // Advance past the parsed ref: find " R" and step past it.
                if let Some(pos) = cur.windows(1).position(|w| w == b"R") {
                    cur = &cur[pos + 1..];
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_value_handles_nested_dict() {
        // Key we want appears AFTER a nested dict — shallow scan would
        // mis-fire on the inner /Color.
        let dict = b" /Resources << /ColorSpace << /Color /DeviceGray >> >> /Pages 7 0 R ";
        let v = find_dict_value(dict, "Pages").expect("found /Pages");
        let s = std::str::from_utf8(v).unwrap().trim();
        assert_eq!(s, "7 0 R");
    }

    #[test]
    fn dict_value_handles_nested_array() {
        let dict = b" /MediaBox [0 0 612 792] /Pages 7 0 R ";
        let v = find_dict_value(dict, "Pages").expect("found /Pages");
        let s = std::str::from_utf8(v).unwrap().trim();
        assert_eq!(s, "7 0 R");
    }

    #[test]
    fn dict_value_finds_array_value() {
        let dict = b" /MediaBox [0 0 612 792] /Other 1 ";
        let v = find_dict_value(dict, "MediaBox").expect("found");
        let s = std::str::from_utf8(v).unwrap().trim();
        assert_eq!(s, "[0 0 612 792]");
    }

    #[test]
    fn ref_array_parses_basic() {
        let bytes = b"[5 0 R 7 0 R 9 0 R]";
        let v = parse_ref_array(bytes);
        assert_eq!(v, vec![(5u32, 0u16), (7, 0), (9, 0)]);
    }

    #[test]
    fn indirect_ref_rejects_non_ref() {
        assert!(parse_indirect_ref(b"5 0 R").is_some());
        assert!(parse_indirect_ref(b"5 0 G").is_none());
        assert!(parse_indirect_ref(b"abc").is_none());
    }
}
