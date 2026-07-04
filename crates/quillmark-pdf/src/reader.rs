//! Minimal byte-level PDF reader and incremental-update writer. Not a general
//! PDF parser — a deliberately small scanner that parses just enough of a base
//! PDF to splice a single incremental update onto it, and hard-errors on shapes
//! a modern PDF can carry but this V1 reader does not handle.
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

use std::collections::HashSet;

use crate::error::PdfError;

const CODE_PARSE: &str = "pdf::parse";
const CODE_XREF_STREAM: &str = "pdf::xref_stream";

/// Build a `PdfError` with `code`. Every fail site here just needs a code plus
/// a message, so this is the whole error-construction surface.
pub fn err(code: &'static str, msg: impl Into<String>) -> PdfError {
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
    let offset: usize = std::str::from_utf8(&after[..end])
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| err(CODE_PARSE, "startxref offset is not a valid integer"))?;
    // Bound the offset so every downstream `pdf[offset..]` / `offset + N` slice
    // is in range — an out-of-range value (e.g. a ~20-digit near-usize::MAX
    // offset) becomes a clean parse error rather than an overflow/panic.
    if offset >= pdf.len() {
        return Err(err(CODE_PARSE, "startxref offset is past end of file"));
    }
    Ok(offset)
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
pub struct UpdatedObject {
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
///
/// Matches the object header `<id> <gen> obj` at a token boundary (so `19 0 obj`
/// isn't found inside `519 0 obj`) for *any* generation — re-saved PDFs can
/// carry non-zero generations. When a base PDF carries prior incremental updates
/// the same id can be serialized more than once; the live copy is the *last* one
/// (xref liveness), so this returns the last match. For the common
/// single-revision, generation-0 input there is exactly one match.
pub fn find_object_bytes(pdf: &[u8], id: u32) -> Option<(usize, usize)> {
    let prefix = format!("{id} ");
    let p = prefix.as_bytes();
    let mut last_start = None;
    let mut i = 0;
    while i + p.len() <= pdf.len() {
        if pdf[i..].starts_with(p)
            && (i == 0 || matches!(pdf[i - 1], b'\n' | b'\r' | b' '))
            && is_obj_header_tail(&pdf[i + p.len()..])
        {
            last_start = Some(i);
        }
        i += 1;
    }
    let start = last_start?;
    let end = find_endobj_end(pdf, start + p.len())?;
    Some((start, end))
}

/// Find the `endobj` keyword that closes the object body starting at `from`,
/// returning the index just past it. Literal `( … )` strings and `%`-comments
/// are skipped so the bytes `endobj` appearing inside a string value (e.g. an
/// `/Info` `/Title`) or a comment cannot truncate the object early — the same
/// string-aware skip [`extract_outer_dict`] already relies on.
fn find_endobj_end(pdf: &[u8], from: usize) -> Option<usize> {
    let needle = b"endobj";
    let mut i = from;
    while i < pdf.len() {
        if pdf[i] == b'(' {
            i = skip_pdf_string(pdf, i);
        } else if pdf[i] == b'%' {
            i = skip_ws_and_comments(pdf, i);
        } else if pdf[i..].starts_with(needle) {
            return Some(i + needle.len());
        } else {
            i += 1;
        }
    }
    None
}

/// The generation number in object `id`'s header (`<id> <gen> obj`), or `None`
/// when the object is absent or its header is malformed. Reads the *live* copy
/// (the last serialized revision), matching [`find_object_bytes`].
pub(crate) fn object_generation(pdf: &[u8], id: u32) -> Option<u16> {
    let (start, _) = find_object_bytes(pdf, id)?;
    let after_id = start + format!("{id} ").len();
    let rest = &pdf[after_id..];
    let n = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    std::str::from_utf8(&rest[..n]).ok()?.parse().ok()
}

/// Reject overwriting a base object that lives at a **non-zero generation**.
///
/// The incremental-update writer always re-emits an overwritten object at
/// generation 0 ([`dict_object`](crate::writer::dict_object)) and references it
/// as gen 0 (the trailer's `/Root … 0 R`, page/widget refs). The reader, by
/// contrast, accepts an object header at *any* generation. So a base whose
/// catalog / page / `/Info` lives at a non-zero generation parses fine yet would
/// produce a malformed update — the new xref/`/Root` would point at gen 0 while
/// the prior xref still resolves the object at its true generation. This guard
/// closes that one gap in the spine's "reject out-of-contract input cleanly"
/// posture, consistent with the xref-stream / `/Encrypt` rejections.
///
/// `None` (object absent) is left for the caller's own not-found error path.
pub(crate) fn assert_overwrite_gen_zero(pdf: &[u8], id: u32, what: &str) -> Result<(), PdfError> {
    match object_generation(pdf, id) {
        Some(0) | None => Ok(()),
        Some(gen) => Err(err(
            "pdf::nonzero_generation",
            format!(
                "{what} object {id} is at generation {gen}; the stamp spine re-emits \
                 overwritten objects at generation 0 and cannot preserve a non-zero generation"
            ),
        )),
    }
}

/// After the `<id> ` prefix, confirm an object header continues as `<gen> obj`:
/// one or more digits, whitespace, then the `obj` keyword as a whole token.
fn is_obj_header_tail(rest: &[u8]) -> bool {
    let gen_digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if gen_digits == 0 {
        return false;
    }
    let after_gen = &rest[gen_digits..];
    let ws = after_gen
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .count();
    if ws == 0 {
        return false;
    }
    let after_ws = &after_gen[ws..];
    after_ws.starts_with(b"obj") && after_ws.get(3).is_none_or(|b| !b.is_ascii_alphanumeric())
}

/// Within a dict's inner bytes, locate `/Key` and return its raw value slice.
///
/// `dict_bytes` is the *inner* content of one dict (between its `<<` / `>>`),
/// where entries strictly alternate `key value key value …` and every key is a
/// Name. The scan walks that key→value rhythm: at each step it reads the key
/// Name, then consumes its value wholesale via `read_value_end` (which steps
/// over nested `<<>>` / `[]` / `()` / `<>` as a unit). Because every value is
/// consumed as a value, a Name that appears in *value* position (e.g.
/// `/Subtype /Producer`) is never mistaken for a key — only keys are tested
/// against `km`. The returned slice begins exactly after the matched key token
/// (callers such as `upsert_producer` derive the key span by subtraction).
pub fn find_dict_value<'a>(dict_bytes: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_marker = format!("/{}", key);
    let km = key_marker.as_bytes();
    let mut i = 0;
    loop {
        i = skip_ws_and_comments(dict_bytes, i);
        // A well-formed flat dict yields a Name key here. Anything else (end of
        // input, or a stray token) means there is no further key to match.
        if dict_bytes.get(i) != Some(&b'/') {
            return None;
        }
        let key_start = i;
        i += 1;
        while i < dict_bytes.len() && !is_pdf_delim(dict_bytes[i]) {
            i += 1;
        }
        let after_key = i;
        let matched = &dict_bytes[key_start..after_key] == km;
        let value_start = skip_ws_and_comments(dict_bytes, after_key);
        let value_end = read_value_end(dict_bytes, value_start)?;
        if matched {
            // Slice from immediately after the key (not `value_start`) so the
            // key span is `[after_key - km.len, value_end)` by subtraction.
            return Some(&dict_bytes[after_key..value_end]);
        }
        i = value_end;
    }
}

/// Replace `key`'s value in a flat dict, given the current `value` slice — which
/// MUST be the subslice [`find_dict_value`] returned for that key. Its start
/// locates the key span by pointer subtraction (`[value_start - key.len,
/// value_end)`), not by re-scanning, so a `key` token appearing inside another
/// value can't be matched by accident. The `key`+value span is rewritten as
/// `key` + one space + `new_value`; the rest of `dict` is copied verbatim.
///
/// `key` is the on-page byte form including the leading slash (`b"/Producer"`).
/// Callers that build the replacement value from `value`'s own bytes must do so
/// before calling — `dict` is borrowed immutably here.
pub fn splice_dict_value(dict: &[u8], key: &[u8], value: &[u8], new_value: &[u8]) -> Vec<u8> {
    let value_start = value.as_ptr() as usize - dict.as_ptr() as usize;
    let value_end = value_start + value.len();
    let key_at = value_start - key.len();
    let mut out = Vec::with_capacity(key_at + key.len() + 1 + new_value.len() + dict.len() - value_end);
    out.extend_from_slice(&dict[..key_at]);
    out.extend_from_slice(key);
    out.push(b' ');
    out.extend_from_slice(new_value);
    out.extend_from_slice(&dict[value_end..]);
    out
}

/// Skip PDF whitespace and `%`-comments (which run to end-of-line) starting at
/// `start`; returns the index of the first significant byte at or after it.
fn skip_ws_and_comments(b: &[u8], start: usize) -> usize {
    let mut i = start;
    loop {
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0c') {
            i += 1;
        }
        if b.get(i) == Some(&b'%') {
            while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        return i;
    }
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
pub fn extract_outer_dict(obj_bytes: &[u8]) -> Option<&[u8]> {
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
    // A node id reached twice means the `/Pages` tree is cyclic or shares a node
    // across parents — malformed, and an amplification vector without this
    // guard: every visit re-scans the whole file via `find_object_bytes`, so a
    // tiny `/Kids` self-cycle would otherwise drive up to MAX_NODES full-file
    // scans before the count cap trips.
    let mut seen: HashSet<u32> = HashSet::new();
    while let Some(node_id) = stack.pop() {
        if !seen.insert(node_id) {
            return Err(err(
                CODE_PARSE,
                format!("page tree revisits node {node_id} (cycle or shared node)"),
            ));
        }
        if seen.len() > MAX_NODES {
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

/// Reject a page with a non-zero `/Rotate` (its own value, else the inherited
/// root `/Pages` value).
///
/// The stamp/flatten paths write widget and content geometry in *unrotated*
/// user space and do not compensate for page rotation, so a rotated base page
/// would display every widget rotated away from its intended box. `/Rotate` is
/// not part of the Typst producer's output (its pages are always unrotated);
/// this guard turns a rotated `pdfform` base into a clean rejection rather than
/// silently mis-placed output, consistent with the reader's other hard
/// rejections.
pub(crate) fn assert_unrotated_page(
    pdf: &[u8],
    catalog_id: u32,
    page_id: u32,
) -> Result<(), PdfError> {
    let read_rotate = |id: u32| -> Option<i64> {
        let (s, e) = find_object_bytes(pdf, id)?;
        let dict = extract_outer_dict(&pdf[s..e])?;
        let raw = find_dict_value(dict, "Rotate")?;
        std::str::from_utf8(raw.trim_ascii())
            .ok()?
            .trim()
            .parse::<i64>()
            .ok()
    };
    let rotate = read_rotate(page_id)
        .or_else(|| root_pages_id(pdf, catalog_id).ok().and_then(read_rotate))
        .unwrap_or(0);
    if rotate.rem_euclid(360) != 0 {
        return Err(err(
            "pdf::rotated_page",
            format!(
                "page object {page_id} has /Rotate {rotate}; the stamp spine only \
                 handles unrotated pages"
            ),
        ));
    }
    Ok(())
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
                if let Some(pos) = cur.iter().position(|&b| b == b'R') {
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
        nums[count] = tok.parse().ok().filter(|f: &f32| f.is_finite())?;
        count += 1;
    }
    (count == 4).then_some(nums)
}

/// Normalize a `/MediaBox` to `[x0, y0, x1, y1]` with `x0 <= x1` and
/// `y0 <= y1`, so `(x0, y0)` is the page's lower-left and `(x1, y1)` its
/// upper-right regardless of which corners the array listed.
fn normalize_rect(mb: [f32; 4]) -> [f32; 4] {
    [
        mb[0].min(mb[2]),
        mb[1].min(mb[3]),
        mb[0].max(mb[2]),
        mb[1].max(mb[3]),
    ]
}

/// The `/MediaBox` of every page, normalized to `[x0, y0, x1, y1]`, in document
/// order.
///
/// Reads each page's `/MediaBox`, falling back to the root `/Pages` node's
/// `/MediaBox` (the common inheritance case) when a page declares none. The
/// full rect — not just width/height — is returned so a caller that owns
/// page-relative top-left geometry can honour a non-zero page origin when
/// flipping to bottom-left PDF user space.
pub(crate) fn page_media_boxes(pdf: &[u8]) -> Result<Vec<[f32; 4]>, PdfError> {
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
        out.push(normalize_rect(mb));
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
    fn dict_value_ignores_name_in_value_position() {
        // A Name that appears as a *value* (`/Subtype /Producer`) must not be
        // mistaken for the `/Producer` key. The real key wins.
        let dict = b" /Subtype /Producer /Producer (real) /Creator (X) ";
        let v = find_dict_value(dict, "Producer").expect("found the key, not the value");
        assert_eq!(v.trim_ascii(), b"(real)");
    }

    #[test]
    fn dict_value_absent_key_with_matching_name_value_is_none() {
        // The key is genuinely absent; the only occurrence of the token is a
        // value Name. The walk must report None, not the spurious value.
        let dict = b" /Subtype /Producer /Creator (X) ";
        assert!(find_dict_value(dict, "Producer").is_none());
    }

    #[test]
    fn dict_value_skips_comments_between_entries() {
        // A `%`-comment between entries must not derail the key→value walk, and
        // a `/Producer` token sitting inside that comment must not be matched.
        let dict = b" /A 1 %decoy /Producer (decoy)\n /Producer (real) ";
        let v = find_dict_value(dict, "Producer").expect("found");
        assert_eq!(v.trim_ascii(), b"(real)");
    }

    #[test]
    fn endobj_inside_comment_does_not_truncate_object() {
        // `endobj` appearing inside a `%`-comment must not close the object at
        // the in-comment occurrence.
        let pdf = b"%PDF\n3 0 obj\n<< /A 1 >> %endobj in a comment\n/B 2 >>\nendobj\n";
        let (s, e) = find_object_bytes(pdf, 3).expect("found object 3");
        assert_eq!(&pdf[e - 6..e], b"endobj");
        // The real terminator is the standalone `endobj`, past the comment.
        assert!(&pdf[s..e].ends_with(b"/B 2 >>\nendobj"));
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

    #[test]
    fn normalize_rect_orders_corners() {
        // Already lower-left/upper-right: unchanged.
        assert_eq!(
            normalize_rect([10.0, 20.0, 622.0, 812.0]),
            [10.0, 20.0, 622.0, 812.0]
        );
        // Swapped corners normalize so (x0,y0) is lower-left.
        assert_eq!(
            normalize_rect([622.0, 812.0, 10.0, 20.0]),
            [10.0, 20.0, 622.0, 812.0]
        );
    }

    #[test]
    fn rect_array_rejects_non_finite() {
        assert_eq!(parse_rect_array(b"[0 0 inf 792]"), None);
        assert_eq!(parse_rect_array(b"[0 0 612 nan]"), None);
        assert_eq!(parse_rect_array(b"[-inf 0 612 792]"), None);
    }

    #[test]
    fn find_object_at_token_boundary() {
        let pdf = b"%PDF\n519 0 obj\n<< /A 1 >>\nendobj\n19 0 obj\n<< /B 2 >>\nendobj\n";
        let (s, e) = find_object_bytes(pdf, 19).expect("found object 19");
        assert_eq!(&pdf[s..e], b"19 0 obj\n<< /B 2 >>\nendobj");
    }

    #[test]
    fn find_object_matches_nonzero_generation() {
        let pdf = b"%PDF\n7 2 obj\n<< /C 3 >>\nendobj\n";
        let (s, e) = find_object_bytes(pdf, 7).expect("found object 7 gen 2");
        assert_eq!(&pdf[s..e], b"7 2 obj\n<< /C 3 >>\nendobj");
    }

    #[test]
    fn object_generation_reads_header_gen() {
        let pdf = b"%PDF\n7 2 obj\n<< /C 3 >>\nendobj\n4 0 obj\n<< /D 1 >>\nendobj\n";
        assert_eq!(object_generation(pdf, 7), Some(2));
        assert_eq!(object_generation(pdf, 4), Some(0));
        assert_eq!(object_generation(pdf, 99), None);
    }

    #[test]
    fn assert_overwrite_gen_zero_rejects_nonzero() {
        let pdf = b"%PDF\n7 2 obj\n<< /C 3 >>\nendobj\n4 0 obj\n<< /D 1 >>\nendobj\n";
        // gen 0 and absent are accepted; the caller owns the not-found path.
        assert!(assert_overwrite_gen_zero(pdf, 4, "x").is_ok());
        assert!(assert_overwrite_gen_zero(pdf, 99, "x").is_ok());
        // gen != 0 is a clean error tagged with the dedicated code.
        let e = assert_overwrite_gen_zero(pdf, 7, "catalog").expect_err("gen 2 rejected");
        assert_eq!(e.code, "pdf::nonzero_generation");
        assert!(e.message.contains("generation 2"), "{}", e.message);
    }

    #[test]
    fn find_object_returns_last_revision() {
        // Same id serialized twice (an incremental update): the live copy is the
        // later one.
        let pdf = b"%PDF\n4 0 obj\n<< /V (old) >>\nendobj\n4 0 obj\n<< /V (new) >>\nendobj\n";
        let (s, e) = find_object_bytes(pdf, 4).expect("found object 4");
        assert_eq!(&pdf[s..e], b"4 0 obj\n<< /V (new) >>\nendobj");
    }

    #[test]
    fn endobj_inside_string_does_not_truncate_object() {
        // A literal string value containing the bytes "endobj" (e.g. an /Info
        // /Title) must not end the object at the in-string occurrence.
        let pdf = b"%PDF\n3 0 obj\n<< /Title (My endobj report) /Author (X) >>\nendobj\n";
        let (s, e) = find_object_bytes(pdf, 3).expect("found object 3");
        assert_eq!(
            &pdf[s..e],
            b"3 0 obj\n<< /Title (My endobj report) /Author (X) >>\nendobj"
        );
        // …and the dict still extracts cleanly with the full /Title value.
        let dict = extract_outer_dict(&pdf[s..e]).expect("dict parses");
        let title = find_dict_value(dict, "Title").expect("/Title");
        assert_eq!(title.trim_ascii(), b"(My endobj report)");
    }

    #[test]
    fn page_tree_cycle_is_rejected() {
        // A /Pages node whose /Kids references itself must error cleanly, not
        // loop to the node cap re-scanning the whole file each visit.
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [2 0 R] /Count 1 >>\nendobj\n";
        let e = resolve_page_ids(pdf, 1).expect_err("cycle rejected");
        assert_eq!(e.code, CODE_PARSE);
        assert!(e.message.contains("revisits"), "{}", e.message);
    }

    #[test]
    fn rotated_page_is_rejected() {
        // /Rotate inherited from the root /Pages node is honoured.
        let pdf = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                    2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /Rotate 90 >>\nendobj\n\
                    3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        let e = assert_unrotated_page(pdf, 1, 3).expect_err("rotated page rejected");
        assert_eq!(e.code, "pdf::rotated_page");
        // A page with no rotation (own or inherited) is accepted.
        let flat = b"%PDF\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                     2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                     3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n";
        assert!(assert_unrotated_page(flat, 1, 3).is_ok());
    }
}
