//! Minimal byte-level PDF reader and incremental-update writer. Not a general
//! PDF parser — a deliberately small scanner that parses just enough of a base
//! PDF to splice a single incremental update onto it, and hard-errors on shapes
//! a modern gov PDF can carry but this V1 reader does not handle.
//!
//! ## Input contract
//!
//! The base PDF must be **traditional-xref, unencrypted, inline-annots,
//! flat-tree**: a classic `xref` table (not an xref *stream*), no `/Encrypt`,
//! page `/Annots` written inline (not as an indirect reference), and a page
//! tree shallow enough to walk. This is the precise inverse of the scanner's
//! error branches; the qualification layer (and the V1 hand-authored fixture)
//! guarantee it. `hayro-syntax` is read-only and exposes no byte spans, so it
//! cannot drive a byte-splice append — hence this bespoke scanner.

use crate::error::PdfError;

const CODE_PARSE: &str = "pdf::parse";
const CODE_XREF_STREAM: &str = "pdf::xref_stream";

/// Build a `PdfError` with `code`. Every fail site here just needs a code plus
/// a message, so this is the whole error-construction surface.
pub(crate) fn err(code: &'static str, msg: impl Into<String>) -> PdfError {
    PdfError::new(code, msg)
}

/// The offset stored after the last `startxref` marker.
pub(crate) fn find_startxref(pdf: &[u8]) -> Result<usize, PdfError> {
    let needle = b"startxref";
    let from = pdf.len().saturating_sub(1024);
    let tail = &pdf[from..];
    let pos = tail
        .windows(needle.len())
        .rposition(|w| w == needle)
        .ok_or_else(|| err(CODE_PARSE, "missing startxref marker near EOF"))?;
    let after = skip_ws(&tail[pos + needle.len()..]);
    let mut end = 0;
    while end < after.len() && after[end].is_ascii_digit() {
        end += 1;
    }
    std::str::from_utf8(&after[..end])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| err(CODE_PARSE, "startxref offset is not a valid integer"))
}

/// Bail if the base PDF stores an xref stream instead of a traditional table.
pub(crate) fn assert_traditional_xref(pdf: &[u8], xref_offset: usize) -> Result<(), PdfError> {
    if pdf.get(xref_offset..xref_offset + 4) != Some(b"xref") {
        return Err(err(
            CODE_XREF_STREAM,
            "PDF declares an xref stream; only traditional xref is supported",
        ));
    }
    Ok(())
}

/// Return the trailer dictionary bytes for the xref section at `xref_offset`.
/// The slice is the inner dict (between `<<` and `>>`) and may be queried with
/// [`find_dict_value`].
pub(crate) fn find_trailer_dict(pdf: &[u8], xref_offset: usize) -> Result<&[u8], PdfError> {
    let needle = b"trailer";
    let pos = pdf[xref_offset..]
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| err(CODE_PARSE, "trailer marker not found"))?
        + xref_offset;
    extract_outer_dict(&pdf[pos + needle.len()..])
        .ok_or_else(|| err(CODE_PARSE, "trailer dict not parseable"))
}

/// Append the `/Info` and `/ID` entries found in `prior_trailer` to `out`, so
/// an incremental-update trailer preserves them. Required because many readers
/// (and lopdf) consult only the last trailer; dropping these keys would lose
/// the document `/Info` (producer/creator) and file identifier. No-op for keys
/// that are absent. Callers append `/Size`, `/Root` and `/Prev` themselves.
fn write_preserved_trailer_keys(out: &mut Vec<u8>, prior_trailer: &[u8]) {
    for key in ["Info", "ID"] {
        if let Some(value) = find_dict_value(prior_trailer, key) {
            out.extend_from_slice(format!(" /{} ", key).as_bytes());
            out.extend_from_slice(value.trim_ascii());
        }
    }
}

/// One object emitted into an incremental update: its number and full
/// serialized form (`<id> 0 obj … endobj`).
pub(crate) struct UpdatedObject {
    pub id: u32,
    pub bytes: Vec<u8>,
}

/// Append a single incremental update to `pdf`: write each object in
/// `objects`, then an xref subsection table (contiguous ids grouped) and a
/// trailer chaining to the prior xref at `prev_xref` via `/Prev`.
///
/// `/Info` and `/ID` are forwarded from the prior trailer so readers that
/// consult only the last trailer keep them; `extra_info_ref` adds an explicit
/// `/Info <id> 0 R` for the case where the prior trailer had none (a fresh
/// `/Info` object was created). `new_size` is the updated `/Size` (highest
/// object number + 1) and `root_id` the document catalog.
pub(crate) fn append_incremental_update(
    mut pdf: Vec<u8>,
    prev_xref: usize,
    root_id: u32,
    new_size: u32,
    extra_info_ref: Option<u32>,
    objects: &[UpdatedObject],
) -> Result<Vec<u8>, PdfError> {
    // Built while the prior trailer (still intact at `prev_xref`) is borrowed,
    // before we append anything.
    let mut trailer_tail = Vec::new();
    write_preserved_trailer_keys(&mut trailer_tail, find_trailer_dict(&pdf, prev_xref)?);
    if let Some(id) = extra_info_ref {
        trailer_tail.extend_from_slice(format!(" /Info {id} 0 R").as_bytes());
    }

    if !pdf.ends_with(b"\n") {
        pdf.push(b'\n');
    }
    let mut entries: Vec<(u32, usize)> = Vec::with_capacity(objects.len());
    for obj in objects {
        let off = pdf.len();
        entries.push((obj.id, off));
        pdf.extend_from_slice(&obj.bytes);
        // Keep object bodies newline-separated so each `N 0 obj` header stays a
        // distinct token for any parser (caller-built bytes already end in `\n`;
        // pdf_writer chunks may not).
        if !pdf.ends_with(b"\n") {
            pdf.push(b'\n');
        }
    }

    let new_xref_off = pdf.len();
    entries.sort_by_key(|(id, _)| *id);
    pdf.extend_from_slice(b"xref\n");
    // A traditional xref table is a series of subsections, each headed by
    // `<first-id> <count>` and followed by one 20-byte `OOOOOOOOOO GGGGG n `
    // entry per object. An incremental update lists only the changed objects,
    // so coalesce them into runs of consecutive ids (the inner loop extends
    // `j`) to emit the fewest subsections.
    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        pdf.extend_from_slice(format!("{} {}\n", entries[i].0, j - i + 1).as_bytes());
        for &(_, off) in &entries[i..=j] {
            pdf.extend_from_slice(format!("{:010} {:05} n \n", off, 0).as_bytes());
        }
        i = j + 1;
    }

    pdf.extend_from_slice(format!("trailer\n<< /Size {new_size} /Root {root_id} 0 R").as_bytes());
    pdf.extend_from_slice(&trailer_tail);
    pdf.extend_from_slice(
        format!(" /Prev {prev_xref} >>\nstartxref\n{new_xref_off}\n%%EOF\n").as_bytes(),
    );
    Ok(pdf)
}

/// Locate object `id` via linear scan and return `(obj_start, endobj_end)`.
/// Matches only at a token boundary so `19 0 obj` isn't found inside `519 0
/// obj`. Callers scan the original PDF before appending, so the first match is
/// the only copy.
pub(crate) fn find_object_bytes(pdf: &[u8], id: u32) -> Option<(usize, usize)> {
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
/// Value-terminating tokenisation handles Name values like `/Pages` so they
/// aren't mis-read as the next entry.
pub(crate) fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_marker = format!("/{}", key);
    let km = key_marker.as_bytes();
    let mut i = 0;
    let mut depth_dict = 0i32;
    let mut depth_array = 0i32;
    while i < dict_bytes.len() {
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
            _ if depth_dict == 0 && depth_array == 0 && dict_bytes[i..].starts_with(km) => {
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
            i += 1;
            while i < b.len() && !is_pdf_delim(b[i]) {
                i += 1;
            }
            Some(i)
        }
        c if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' => {
            // Number, possibly followed by `N R` (indirect reference). The
            // standalone-R check rejects `5 0 Rect`.
            let num_end = read_number_end(b, i);
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
                if b.get(j).copied() == Some(b'R') && b.get(j + 1).is_none_or(|c| is_pdf_delim(*c))
                {
                    return Some(j + 1);
                }
            }
            Some(num_end)
        }
        _ => {
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

/// `start` points at `(`. Returns index AFTER the matching `)`.
fn skip_pdf_string(b: &[u8], start: usize) -> usize {
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

/// `start` points at `<` (not `<<`). Returns index AFTER the closing `>`.
fn skip_pdf_hex_string(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < b.len() && b[i] != b'>' {
        i += 1;
    }
    (i + 1).min(b.len())
}

fn is_pdf_delim(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'/' | b'[' | b']' | b'(' | b')' | b'<' | b'>'
    )
}

pub(crate) fn parse_indirect_ref(s: &[u8]) -> Option<(u32, u16)> {
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
    // Standalone-R check rejects identifiers like `Roller`.
    if !s.get(1).is_none_or(|c| is_pdf_delim(*c)) {
        return None;
    }
    Some((id, gen))
}

/// Slice between the outermost `<< ... >>` of an indirect object's body.
pub(crate) fn extract_outer_dict(obj_bytes: &[u8]) -> Option<&[u8]> {
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
/// in document order. The recursion is defensive and capped to prevent runaway
/// on a pathological PDF.
pub(crate) fn resolve_page_ids(pdf: &[u8], catalog_id: u32) -> Result<Vec<u32>, PdfError> {
    let root_pages_id = root_pages_id(pdf, catalog_id)?;

    const MAX_NODES: usize = 100_000;
    let mut out = Vec::new();
    let mut stack = vec![root_pages_id];
    let mut visited = 0usize;
    while let Some(node_id) = stack.pop() {
        visited += 1;
        if visited > MAX_NODES {
            return Err(err(CODE_PARSE, "page tree exceeds 100 000 nodes"));
        }
        let (s, e) = find_object_bytes(pdf, node_id)
            .ok_or_else(|| err(CODE_PARSE, format!("page node {node_id} not found")))?;
        let dict = extract_outer_dict(&pdf[s..e]).ok_or_else(|| {
            err(
                CODE_PARSE,
                format!("page node {node_id} dict not parseable"),
            )
        })?;
        let typ = find_dict_value(dict, "Type")
            .map(|b| String::from_utf8_lossy(b.trim_ascii()).into_owned())
            .unwrap_or_default();
        if typ.starts_with("/Pages") {
            let kids = find_dict_value(dict, "Kids")
                .ok_or_else(|| err(CODE_PARSE, "/Pages node missing /Kids"))?;
            let mut kid_ids: Vec<u32> = parse_ref_array(kids)
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            kid_ids.reverse();
            stack.extend(kid_ids);
        } else {
            out.push(node_id);
        }
    }
    Ok(out)
}

/// The catalog's root `/Pages` node id.
fn root_pages_id(pdf: &[u8], catalog_id: u32) -> Result<u32, PdfError> {
    let (cs, ce) =
        find_object_bytes(pdf, catalog_id).ok_or_else(|| err(CODE_PARSE, "catalog not found"))?;
    let cat_dict = extract_outer_dict(&pdf[cs..ce])
        .ok_or_else(|| err(CODE_PARSE, "catalog dict not parseable"))?;
    find_dict_value(cat_dict, "Pages")
        .and_then(parse_indirect_ref)
        .map(|(id, _)| id)
        .ok_or_else(|| err(CODE_PARSE, "catalog /Pages reference not found"))
}

pub(crate) fn parse_ref_array(bytes: &[u8]) -> Vec<(u32, u16)> {
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

/// Parse a 4-number array (`[x0 y0 x1 y1]`) such as `/MediaBox`.
fn parse_rect_array(bytes: &[u8]) -> Option<[f32; 4]> {
    let trimmed = bytes.trim_ascii();
    let inner = trimmed.strip_prefix(b"[")?.strip_suffix(b"]")?;
    let mut nums = [0.0f32; 4];
    let mut count = 0;
    for tok in String::from_utf8_lossy(inner).split_whitespace() {
        if count >= 4 {
            return None;
        }
        nums[count] = tok.parse().ok()?;
        count += 1;
    }
    (count == 4).then_some(nums)
}

/// Page dimensions `(width_pt, height_pt)` for every page, in document order.
///
/// Reads each page's `/MediaBox`, falling back to the root `/Pages` node's
/// `/MediaBox` (the common inheritance case) when a page declares none.
pub(crate) fn page_sizes(pdf: &[u8]) -> Result<Vec<(f32, f32)>, PdfError> {
    let xref_offset = find_startxref(pdf)?;
    assert_traditional_xref(pdf, xref_offset)?;
    let trailer = find_trailer_dict(pdf, xref_offset)?;
    let (catalog_id, _) = find_dict_value(trailer, "Root")
        .and_then(parse_indirect_ref)
        .ok_or_else(|| err(CODE_PARSE, "/Root missing or malformed in trailer"))?;

    let inherited = root_pages_media_box(pdf, catalog_id);
    let page_ids = resolve_page_ids(pdf, catalog_id)?;
    let mut out = Vec::with_capacity(page_ids.len());
    for id in page_ids {
        let (s, e) = find_object_bytes(pdf, id)
            .ok_or_else(|| err(CODE_PARSE, format!("page node {id} not found")))?;
        let dict = extract_outer_dict(&pdf[s..e])
            .ok_or_else(|| err(CODE_PARSE, format!("page node {id} dict not parseable")))?;
        let mb = find_dict_value(dict, "MediaBox")
            .and_then(parse_rect_array)
            .or(inherited)
            .ok_or_else(|| err(CODE_PARSE, format!("page {id} has no resolvable /MediaBox")))?;
        out.push(((mb[2] - mb[0]).abs(), (mb[3] - mb[1]).abs()));
    }
    Ok(out)
}

/// The root `/Pages` node's `/MediaBox`, if present — the value pages inherit.
fn root_pages_media_box(pdf: &[u8], catalog_id: u32) -> Option<[f32; 4]> {
    let pages_id = root_pages_id(pdf, catalog_id).ok()?;
    let (s, e) = find_object_bytes(pdf, pages_id)?;
    let dict = extract_outer_dict(&pdf[s..e])?;
    find_dict_value(dict, "MediaBox").and_then(parse_rect_array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_value_handles_nested_dict() {
        let dict = b" /Resources << /ColorSpace << /Color /DeviceGray >> >> /Pages 7 0 R ";
        let v = find_dict_value(dict, "Pages").expect("found /Pages");
        let s = std::str::from_utf8(v).unwrap().trim();
        assert_eq!(s, "7 0 R");
    }

    #[test]
    fn dict_value_finds_array_value() {
        let dict = b" /MediaBox [0 0 612 792] /Other 1 ";
        let v = find_dict_value(dict, "MediaBox").expect("found");
        assert_eq!(parse_rect_array(v), Some([0.0, 0.0, 612.0, 792.0]));
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

    #[test]
    fn rect_array_rejects_wrong_arity() {
        assert_eq!(parse_rect_array(b"[0 0 612]"), None);
        assert_eq!(parse_rect_array(b"[0 0 612 792 1]"), None);
        assert_eq!(parse_rect_array(b"0 0 612 792"), None);
    }
}
