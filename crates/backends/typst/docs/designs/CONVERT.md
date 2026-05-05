# Markdown to Typst Conversion

This document details the design and implementation of the markdown-to-Typst conversion system in `quillmark-typst/src/convert.rs`.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Public API](#public-api)
4. [Escape Functions](#escape-functions)
5. [Event-Based Conversion Flow](#event-based-conversion-flow)
6. [Markdown Element Handling](#markdown-element-handling)
7. [Implementation Notes and Gotchas](#implementation-notes-and-gotchas)
8. [Examples](#examples)
9. [CommonMark Feature Design Reference](#commonmark-feature-design-reference)
10. [Future Enhancements](#future-enhancements)

---

## Overview

The conversion module provides functionality to transform CommonMark markdown into Typst markup language. This is a critical component of the Typst backend, enabling markdown content to be embedded in Typst templates through the `Content` filter.

**Key Design Principles:**

* **Event-based parsing** using `pulldown_cmark` for robust markdown parsing
* **Character escaping** to handle Typst's reserved characters
* **Stateful conversion** to manage context like list nesting and formatting
* **Minimal output** that leverages Typst's natural text flow where possible

---

## Architecture

The conversion system consists of three main components:

```
┌─────────────────┐
│  mark_to_typst  │  Public entry point
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   push_typst    │  Core conversion logic (private)
└────────┬────────┘
         │
         ├──► escape_markup()  (for text content)
         └──► escape_string()  (for string literals)
```

### Data Flow

1. **Input**: Raw markdown string
2. **Parse**: Create `pulldown_cmark::Parser` with strikethrough support
3. **Convert**: Process event stream, building Typst markup string
4. **Output**: Complete Typst markup ready for compilation

---

## Public API

### `mark_to_typst(markdown: &str) -> String`

Primary conversion function that transforms markdown into Typst markup.

**Parameters:**
- `markdown`: Input markdown string (CommonMark-compliant)

**Returns:**
- Typst markup string suitable for compilation or embedding

**Features Enabled:**
- Strikethrough support via `Options::ENABLE_STRIKETHROUGH`

**Example:**
```rust
use quillmark_typst::convert::mark_to_typst;

let markdown = "This is **bold** and _italic_ text.";
let typst = mark_to_typst(markdown);
// Output: "This is *bold* and _italic_ text.\n\n"
```

---

## Escape Functions

### `escape_markup(s: &str) -> String`

Escapes text content for safe use in Typst markup context. This function must be applied to all user-provided text to prevent interpretation of special characters.

**Characters Escaped:**
- `\` → `\\` (backslash - **must be first**)
- `//` → `\/\/` (line comments)
- `*` → `\*` (bold/strong markers)
- `_` → `\_` (emphasis markers)
- `` ` `` → ``\` `` (inline code markers)
- `#` → `\#` (headings and Typst functions)
- `[` → `\[` (link/reference delimiters)
- `]` → `\]` (link/reference delimiters)
- `$` → `\$` (math mode)
- `<` → `\<` (angle brackets)
- `>` → `\>` (angle brackets)
- `@` → `\@` (references)

**Critical Note:** Backslash must be escaped first to prevent double-escaping of subsequent escape sequences.

### `escape_string(s: &str) -> String`

Escapes text for embedding in Typst string literals (within quotes). Used primarily for filter outputs and JSON injection.

**Characters Escaped:**
- `\` → `\\`
- `"` → `\"`
- `\n` → `\n`
- `\r` → `\r`
- `\t` → `\t`
- Control characters → `\u{...}` (Unicode escape sequences)

**Use Case:** When wrapping Typst markup in `eval()` calls or embedding in JSON structures.

---

## Event-Based Conversion Flow

The `push_typst` function processes a stream of markdown events from `pulldown_cmark::Parser`. It maintains conversion state and builds the output string incrementally.

### State Management

The converter maintains these pieces of state:

1. **`end_newline: bool`** - Tracks whether we're currently at the end of a line (used to avoid duplicate newlines)
2. **`list_stack: Vec<ListType>`** - Stack tracking nested list contexts (bullet vs. ordered)
3. **`in_list_item: bool`** - Tracks whether we're inside a list item (affects paragraph spacing)
4. **`list_item_first_block: bool`** - Tracks whether the current block is the first in a list item (subsequent blocks get continuation indent)
5. **`in_code_block: bool`** - Tracks whether we're inside a code block (disables text escaping)

### List Type

```rust
enum ListType {
    Bullet,   // Unordered list (markdown `-` → Typst `+`)
    Ordered,  // Ordered list (markdown `1.` → Typst `1.`)
}
```

**Important:** Markdown uses `-` for bullet lists, but Typst uses `+`. This conversion is handled automatically.

---

## Markdown Element Handling

The converter handles markdown events in a match expression, processing both start and end tags for structural elements.

### Text Formatting

| Markdown | Typst | Implementation |
|----------|-------|----------------|
| `**bold**` | `*bold*` | `Tag::Strong` → `*`, `TagEnd::Strong` → `*` |
| `*italic*` or `_italic_` | `_italic_` | `Tag::Emphasis` → `_`, `TagEnd::Emphasis` → `_` |
| `~~strike~~` | `#strike[strike]` | `Tag::Strikethrough` → `#strike[`, `TagEnd::Strikethrough` → `]` |
| `` `code` `` | `` `code` `` | `Event::Code` → wrap in backticks |

### Paragraphs

**Outside Lists:**
- `Tag::Paragraph` → Add newline if not at start of line
- `TagEnd::Paragraph` → Two newlines for paragraph separation

**Inside Lists:**
- First block: `Tag::Paragraph` → No extra spacing (content follows list marker directly)
- Subsequent blocks: `Tag::Paragraph` → Blank line + continuation indent (2 spaces per nesting level)
- `TagEnd::Paragraph` → Newline, marks block as complete

This enables proper multi-paragraph list items in Typst with correct continuation indentation.

### Lists

**Unordered Lists:**
```markdown
- Item 1
- Item 2
```
↓
```typst
+ Item 1
+ Item 2
```

**Ordered Lists:**
```markdown
1. First
2. Second
```
↓
```typst
1. First
1. Second
```

**Note:** Typst auto-numbers ordered lists, so we always use `1.`

**Nested Lists:**
- Each nesting level adds 2-space indentation
- List stack tracks current nesting depth
- Example:
  ```typst
  + Level 1
    + Level 2
      + Level 3
  ```

### Links

```markdown
[Link text](https://example.com)
```
↓
```typst
#link("https://example.com")[Link text]
```

**Implementation:**
- `Tag::Link` → `#link("url")[`
- Link text (as markdown events)
- `TagEnd::Link` → `]`
- URL is escaped with `escape_markup()` to handle special characters

**Title is dropped.** CommonMark allows `[text](url "title")`; the title
is parsed but discarded. Typst's `#link` has no `title:` parameter and
the PDF output Typst produces exposes no tooltip primitive that this
backend can target, so there is nowhere to render the title.

### Line Breaks

| Markdown | Typst | Event |
|----------|-------|-------|
| Two spaces + newline | `\n` (hard break) | `Event::HardBreak` |
| Single newline | ` ` (space) | `Event::SoftBreak` |

This preserves markdown's distinction between soft line wrapping and explicit line breaks.

### Tables

Markdown GFM tables are converted to Typst's native `#table()` function.

**Example:**
```markdown
| Name | Age | City |
|------|:---:|-----:|
| Alice | 30 | NYC |
| Bob | 25 | LA |
```
↓
```typst
#table(
  columns: 3,
  align: (auto, center, right),
  table.header([Name], [Age], [City]),
  [Alice], [30], [NYC],
  [Bob], [25], [LA],
)
```

**Implementation:**
- `Tag::Table(alignments)` → `#table(\n  columns: N,\n` + optional `align: (…),\n`
- `Tag::TableHead` → `  table.header(`
- `TagEnd::TableHead` → `),\n`
- `Tag::TableRow` → `  ` (indent)
- `TagEnd::TableRow` → `\n`
- `Tag::TableCell` → `[`
- `TagEnd::TableCell` → `], `
- `TagEnd::Table` → `)\n\n`

**Alignment mapping:**

| Markdown | Typst |
|----------|-------|
| (none)   | `auto` |
| `:---`   | `left` |
| `:---:`  | `center` |
| `---:`   | `right` |

The `align:` parameter is only emitted when at least one column has a non-default (non-`None`) alignment. Cell content supports all inline formatting: bold, italic, strikethrough, underline, inline code, links.

### Text Content

- `Event::Text` → Escaped with `escape_markup()` and appended
- Updates `end_newline` based on whether text ends with newline

### Currently Unsupported Elements

The following markdown features are not currently implemented but have design considerations below:

- HTML tags (intentionally excluded)
- Math expressions (intentionally excluded)
- Footnotes (intentionally excluded)
- Images (to be handled separately by asset system)
- Block quotes (see design below)
- Horizontal rules (see design below)

---

## Implementation Notes and Gotchas

### Character Escaping Order

**Critical:** Backslash must be escaped first in `escape_markup()`:

```rust
s.replace('\\', "\\\\")  // MUST BE FIRST
 .replace('*', "\\*")    // Then other chars
 .replace('_', "\\_")
 // ...
```

If other replacements come first, you'll double-escape their backslashes.

### List Item Continuation

Multiple blocks within a list item use proper Typst continuation syntax. The first block follows the list marker directly. Subsequent blocks are separated by a blank line and indented to align with the first block's content:

```markdown
- First paragraph.

  Second paragraph.

  ```
  code block
  ```
```

The `list_item_first_block` flag tracks whether we're on the first block, and `list_stack.len()` determines the continuation indent depth (2 spaces per nesting level).

### List Marker Conversion

Markdown's `-` for bullet lists becomes Typst's `+`:

```rust
ListType::Bullet => output.push_str(&format!("{}+ ", indent))
```

This is because `-` in Typst is used for different purposes (like ranges and negative numbers).

### Ordered List Numbering

Typst automatically numbers ordered lists, so we always emit `1.`:

```rust
ListType::Ordered => output.push_str(&format!("{}1. ", indent))
```

Typst will render: 1., 2., 3., etc. automatically.

### Newline Management

The `end_newline` flag prevents duplicate newlines:

```rust
if !end_newline {
    output.push('\n');
    end_newline = true;
}
```

This ensures clean output without excessive blank lines.

### List Stack Depth Calculation

Indentation for nested lists uses:

```rust
let indent = "  ".repeat(list_stack.len().saturating_sub(1));
```

The `saturating_sub(1)` prevents underflow and starts indentation at the second level (first level has no indent).

---

## Examples

### Basic Text Formatting

**Input:**
```markdown
This is **bold**, _italic_, and ~~strikethrough~~ text.
```

**Output:**
```typst
This is *bold*, _italic_, and #strike[strikethrough] text.

```

### Lists

**Input:**
```markdown
- Item 1
- Item 2
  - Nested item
- Item 3
```

**Output:**
```typst
+ Item 1
+ Item 2
  + Nested item
+ Item 3

```

### Mixed Content

**Input:**
```markdown
A paragraph with **bold** and a [link](https://example.com).

Another paragraph with `inline code`.

- A list item
- Another item
```

**Output:**
```typst
A paragraph with *bold* and a #link("https://example.com")[link].

Another paragraph with `inline code`.

+ A list item
+ Another item

```

### Escaping Special Characters

**Input:**
```markdown
Typst uses * for bold and # for functions.
```

**Output:**
```typst
Typst uses \* for bold and \# for functions.

```

---

## Integration with Quillmark

The `mark_to_typst` function is used by the `Content` filter in `filters.rs`:

```rust
pub fn content_filter(_state: &State, value: Value, _kwargs: Kwargs) -> Result<Value, Error> {
    // ... value extraction ...
    let markup = mark_to_typst(&content);
    Ok(Value::from_safe_string(format!(
        "eval(\"{}\", mode: \"markup\")",
        escape_string(&markup)
    )))
}
```

This allows markdown body content to be embedded in Typst templates:

```typst
= {{ title | String }}

{{ body | Content }}
```

The two-stage escaping (markup → string) ensures safe evaluation in the Typst context.

---

## Testing

When testing the conversion, consider:

1. **Character escaping** - Ensure all Typst special characters are properly escaped
2. **List nesting** - Test multiple levels of nested lists (both bullet and ordered)
3. **Mixed content** - Combine various markdown features in one document
4. **Edge cases** - Empty strings, consecutive formatting markers, etc.
5. **Newline handling** - Verify no excessive blank lines in output

Example test structure:

```rust
#[test]
fn test_markdown_to_typst_conversion() {
    let markdown = "**bold** and _italic_";
    let typst = mark_to_typst(markdown);
    assert_eq!(typst, "*bold* and _italic_\n\n");
}
```

---

## CommonMark Feature Design Reference

This section provides a comprehensive analysis of all CommonMark features, their current implementation status, and design recommendations for features not yet implemented.

### 1. Document Structure

#### 1.1 Headings

**CommonMark Syntax:**
```markdown
# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6
```

**Current Status:** ✅ Implemented

**Typst Mapping:**
```typst
= Heading 1
== Heading 2
=== Heading 3
==== Heading 4
===== Heading 5
====== Heading 6
```

**Implementation:**

See `/home/user/quillmark/crates/backends/typst/src/convert.rs:210-216` and `287-291`:

```rust
Tag::Heading { level, .. } => {
    if !end_newline {
        output.push('\n');
    }
    let equals = "=".repeat(level as usize);
    output.push_str(&equals);
    output.push(' ');
    end_newline = false;
}

TagEnd::Heading(_) => {
    output.push('\n');
    output.push('\n'); // Extra newline after heading
    end_newline = true;
}
```

**Features:**
- Supports all 6 heading levels (CommonMark compliant)
- Typst headings automatically handle numbering and spacing
- Properly escapes special characters in heading text
- Works with inline formatting (bold, italic, code) within headings
- Comprehensive test coverage (12+ tests starting at line 912)

**Test Coverage:**
- Basic heading levels 1-6
- Headings with formatting (bold, italic, underline, code)
- Headings with special characters
- Multiple consecutive headings
- Headings followed by paragraphs and lists

**Notes:**
- While templates typically control document structure, headings in body content allow for flexible document organization
- Heading levels are capped at 6 per CommonMark specification
- Headings can contain inline markdown (formatting, code, links)

#### 1.2 Paragraphs

**Current Status:** ✅ Implemented

Already handled correctly with special consideration for list item context.

#### 1.3 Line Breaks

**Current Status:** ✅ Implemented

- Hard breaks (two spaces + newline or backslash): `Event::HardBreak` → `#linebreak()`
- Soft breaks (single newline): `Event::SoftBreak` → ` `

### 2. Emphasis

#### 2.1 Italic

**Current Status:** ✅ Implemented

`*italic*` or `_italic_` → `_italic_` in Typst

#### 2.2 Bold

**Current Status:** ✅ Implemented

`**bold**` or `__bold__` → `*bold*` in Typst

#### 2.3 Bold + Italic

**CommonMark Syntax:**
```markdown
***bold italic***
___bold italic___
```

**Current Status:** ✅ Partially implemented

pulldown_cmark parses this as nested `Strong` and `Emphasis` tags, which naturally produces `*_bold italic_*` in Typst. This is the correct Typst syntax for bold italic text.

**Verification:**
```markdown
Input: ***test***
Parser events: Start(Strong) → Start(Emphasis) → Text("test") → End(Emphasis) → End(Strong)
Output: *_test_*
```

✅ No additional implementation needed - already works correctly through event nesting.

### 3. Block Quotes

**CommonMark Syntax:**
```markdown
> This is a quote
> Multiple lines
>
> > Nested quotes
```

**Current Status:** Not implemented

**Typst Syntax:**
```typst
#quote[
  This is a quote
  Multiple lines
]
```

**Implementation Design:**

```rust
Tag::BlockQuote => {
    if !end_newline {
        output.push('\n');
    }
    output.push_str("#quote[\n");
    end_newline = true;
}

TagEnd::BlockQuote => {
    if !end_newline {
        output.push('\n');
    }
    output.push_str("]\n\n");
    end_newline = true;
}
```

**Nested Quotes Handling:**

Per requirements, "all nested quotes should be treated as a single block quote." pulldown_cmark emits separate `BlockQuote` start/end events for each level. To flatten:

```rust
let mut blockquote_depth = 0;

Tag::BlockQuote => {
    blockquote_depth += 1;
    if blockquote_depth == 1 {
        // Only emit #quote[ for outermost level
        if !end_newline {
            output.push('\n');
        }
        output.push_str("#quote[\n");
        end_newline = true;
    }
}

TagEnd::BlockQuote => {
    blockquote_depth -= 1;
    if blockquote_depth == 0 {
        // Only close bracket at outermost level
        if !end_newline {
            output.push('\n');
        }
        output.push_str("]\n\n");
        end_newline = true;
    }
}
```

**Considerations:**
- Typst's `#quote` applies quote-specific styling
- Nested quotes collapse into single block (per requirements)
- Quote attribution can be added: `#quote(attribution: [Author])[content]` if needed later

**Recommendation:** Implement with flattening logic for nested quotes.

### 4. Lists

**Current Status:** ✅ Implemented

- Unordered lists (-, +, *): All map to `+` in Typst
- Ordered lists: Map to `1.` (Typst auto-numbers)
- Nested lists: Properly indented with list stack

### 5. Code

#### 5.1 Inline Code

**Current Status:** ✅ Implemented

`` `code` `` → `` `code` `` (backticks preserved)

#### 5.2 Code Blocks (Fenced)

**CommonMark Syntax:**
````markdown
```rust
fn main() {
    println!("Hello");
}
```
````

**Current Status:** ✅ Implemented

**Typst Syntax:**
```typst
#raw(lang: "rust", block: true, "fn main() {\n    println!(\"Hello\");\n}")
```

Or using raw block syntax:
````typst
```rust
fn main() {
    println!("Hello");
}
```
````

**Implementation Design:**

```rust
Tag::CodeBlock(kind) => {
    if !end_newline {
        output.push('\n');
    }
    
    match kind {
        pulldown_cmark::CodeBlockKind::Fenced(lang) => {
            // Use Typst's raw block syntax
            output.push_str("```");
            if !lang.is_empty() {
                output.push_str(&lang);
            }
            output.push('\n');
        }
        pulldown_cmark::CodeBlockKind::Indented => {
            // Indented code blocks (no language)
            output.push_str("```\n");
        }
    }
    end_newline = true;
}

Event::Text(text) if in_code_block => {
    // Code block content - no escaping needed
    output.push_str(&text);
    end_newline = text.ends_with('\n');
}

TagEnd::CodeBlock(_) => {
    if !end_newline {
        output.push('\n');
    }
    output.push_str("```\n\n");
    end_newline = true;
}
```

**State Management:**

Add `in_code_block` flag to track when we're inside a code block (similar to `in_list_item`):

```rust
let mut in_code_block = false;

Tag::CodeBlock(_) => {
    in_code_block = true;
    // ... output code
}

TagEnd::CodeBlock(_) => {
    in_code_block = false;
    // ... output code
}
```

**Escaping Consideration:**

Code block content should NOT be escaped since Typst's raw blocks handle content literally. Need to check event type before escaping text:

```rust
Event::Text(text) => {
    if in_code_block {
        output.push_str(&text); // No escaping
    } else {
        let escaped = escape_markup(&text);
        output.push_str(&escaped);
    }
    end_newline = text.ends_with('\n');
}
```

**Recommendation:** Implement with language hint preservation and no escaping for code content.

### 6. Links

**Current Status:** ✅ Implemented

`[text](url)` → `#link("url")[text]`

**Note on Alternative Syntax:**

The comment mentions `<a>text</a>` - this appears to be HTML syntax, not CommonMark. CommonMark link syntax is `[text](url)` or autolinks like `<http://example.com>`.

**Autolinks Design:**

CommonMark autolinks: `<http://example.com>` or `<email@example.com>`

pulldown_cmark emits these as `Tag::Link` with `LinkType::Autolink`. Current implementation already handles this correctly via the generic `Tag::Link` match.

✅ Already supported through existing link handling.

### 7. Images

**CommonMark Syntax:**
```markdown
![Alt text](image.png)
![Alt text](image.png "Title")
```

**Current Status:** Not implemented (deferred to asset system)

**Typst Syntax:**
```typst
#image("image.png")
#image("image.png", alt: "Alt text")
```

**Implementation Design:**

```rust
Tag::Image { dest_url, title, .. } => {
    output.push_str("#image(\"");
    output.push_str(&escape_markup(&dest_url));
    output.push('"');
    // Alt text goes in the alt parameter
    image_alt_buffer.clear(); // Store alt text as we process events
    end_newline = false;
}

TagEnd::Image => {
    if !image_alt_buffer.is_empty() {
        output.push_str(", alt: \"");
        output.push_str(&escape_string(&image_alt_buffer));
        output.push('"');
    }
    output.push(')');
    end_newline = false;
}
```

**State Management:**

Add `image_alt_buffer: String` and `in_image: bool` to track image context:

```rust
let mut in_image = false;
let mut image_alt_buffer = String::new();

Tag::Image { .. } => {
    in_image = true;
    image_alt_buffer.clear();
    // ... output
}

Event::Text(text) if in_image => {
    image_alt_buffer.push_str(&text);
    // Don't output text directly - it goes in alt parameter
}

TagEnd::Image => {
    in_image = false;
    // ... use image_alt_buffer
}
```

**Asset Path Consideration:**

Images reference files in the asset system. The current Quillmark architecture handles assets through `QuillWorld`. Image paths might need resolution:

- Relative paths: `image.png` → `assets/image.png`
- Absolute paths: `/assets/image.png`
- URLs: `http://example.com/image.png` (remote)

**Recommendation:** Implement with asset path resolution integration. Coordinate with `QuillWorld` asset handling to ensure paths resolve correctly in the Typst compilation context.

### 8. Horizontal Rules

**CommonMark Syntax:**
```markdown
---
***
___
```

**Current Status:** Not implemented

**Typst Syntax:**
```typst
#line(length: 100%)
```

Or simply:
```typst
#v(0.5em)
#line(length: 100%)
#v(0.5em)
```

**Implementation Design:**

```rust
Event::Rule => {
    if !end_newline {
        output.push('\n');
    }
    output.push_str("#line(length: 100%)\n\n");
    end_newline = true;
}
```

**Considerations:**
- `Event::Rule` is emitted by pulldown_cmark for horizontal rules
- Typst's `#line()` draws a horizontal line
- `length: 100%` makes it span the full width
- Vertical spacing (`#v()`) can be added for visual separation if needed

**Recommendation:** Simple implementation - maps directly to `#line(length: 100%)`.

### 9. Escaping

**CommonMark Backslash Escapes:**
```markdown
\* \_ \# \\ \! \( \) \[ \] \{ \} \. \+ \- \` \| \< \> \= \~ \^ \& \$ \% \@ \" \'
```

**Current Status:** ✅ Implemented (partially)

The `escape_markup()` function handles Typst's reserved characters. CommonMark backslash escapes are handled by pulldown_cmark parser before conversion.

**How It Works:**

1. pulldown_cmark parses markdown and processes backslash escapes
2. Escaped characters come through as plain text in `Event::Text`
3. Our `escape_markup()` then escapes them for Typst if they're Typst reserved chars

**Example Flow:**
```
Input markdown: "Use \* for lists"
Parser: Event::Text("Use * for lists")  // backslash removed
Converter: escape_markup("Use * for lists") → "Use \* for lists"
Output Typst: "Use \* for lists"
```

✅ Already working correctly - no additional implementation needed.

**Edge Case - Literal Backslashes:**

```
Input: "Path: C:\\Users\\file"
Parser: Event::Text("Path: C:\Users\file")  // One backslash removed
Converter: escape_markup() → "Path: C:\\Users\\file"  // Escaped for Typst
Output: "Path: C:\\Users\\file"
```

The double backslash in markdown becomes single backslash in parser output, which is then escaped to double backslash for Typst. This is correct behavior.

### Feature Implementation Priority Summary

| Feature | Status | Priority | Complexity | Notes |
|---------|--------|----------|------------|-------|
| Headings | ✅ Done | - | - | All 6 levels supported with full test coverage |
| Paragraphs | ✅ Done | - | - | Fully implemented |
| Line breaks | ✅ Done | - | - | Hard & soft breaks work |
| Italic | ✅ Done | - | - | Working |
| Bold | ✅ Done | - | - | Working |
| Bold+Italic | ✅ Done | - | - | Works via nesting |
| Tables (GFM) | ✅ Done | - | - | Maps to Typst #table() with alignment support |
| Block quotes | Not impl. | Medium | Medium | Needs depth tracking for flattening |
| Lists | ✅ Done | - | - | Fully implemented with nesting |
| Inline code | ✅ Done | - | - | Working |
| Code blocks | ✅ Done | - | - | Fenced and indented, with language hints |
| Links | ✅ Done | - | - | Working with autolinks |
| Images | Not impl. | Medium | High | Needs asset system coordination |
| Horizontal rules | Not impl. | Low | Simple | Direct mapping to #line() |
| Escaping | ✅ Done | - | - | Handled by parser + escape_markup() |

### Implementation Recommendations

**High Priority (Should Implement):**
1. **Code blocks** - Common in technical documentation, straightforward implementation
2. **Horizontal rules** - Trivial to implement, useful for document structure

**Medium Priority (Consider Based on Use Cases):**
3. **Block quotes** - Useful for citations and callouts, requires depth tracking
4. **Images** - Requires asset system coordination, high value for rich documents

**No Action Needed:**
5. All core features already work correctly: headings, emphasis, lists, links, escaping

---

## Future Enhancements

For detailed designs of unimplemented CommonMark features, see the [CommonMark Feature Design Reference](#commonmark-feature-design-reference) section above.

Additional potential enhancements beyond CommonMark:

1. **Task lists** - Checkboxes in lists `- [ ]` and `- [x]` (GFM extension)
3. **Footnotes** - Reference-style footnotes
4. **Definition lists** - Term and definition pairs
5. **Custom extensions** - Plugin system for domain-specific markdown features
6. **Math blocks** - LaTeX-style math for scientific documents
7. **Diagrams** - Mermaid or similar diagram syntax

These would require careful consideration of how they interact with the template system and Quillmark's overall architecture. Refer to the design reference section for implementation patterns and considerations that can be applied to these features.
