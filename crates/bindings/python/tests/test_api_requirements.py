"""Tests for the API requirements."""

import pytest
from quillmark import Quillmark, Document, OutputFormat, ParseError, EditError
from conftest import QUILLS_PATH, _latest_version


def test_parsed_document_quill_ref():
    """Test that Document exposes quill_ref method."""
    markdown_with_quill = "---\nQUILL: my_quill\ntitle: Test\n---\n\n# Content\n"
    parsed = Document.from_markdown(markdown_with_quill)
    assert parsed.quill_ref() == "my_quill"

    markdown_without_quill = "---\ntitle: Test\n---\n\n# Content\n"
    with pytest.raises(ParseError):
        Document.from_markdown(markdown_without_quill)


def test_quill_properties(taro_quill_dir):
    """Test that Quill exposes all required properties."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    assert quill.name == "taro"
    assert quill.backend == "typst"
    assert quill.plate is not None
    assert isinstance(quill.plate, str)

    metadata = quill.metadata
    assert isinstance(metadata, dict)

    schema = quill.schema
    assert isinstance(schema, str)
    assert "fields:" in schema

    example = quill.example
    assert example is not None

    supported_formats = quill.supported_formats()
    assert isinstance(supported_formats, list)
    assert OutputFormat.PDF in supported_formats


def test_full_workflow():
    """Test loading quill via engine and rendering."""
    engine = Quillmark()
    taro_dir = QUILLS_PATH / "taro"
    quill = engine.quill_from_path(str(_latest_version(taro_dir)))

    markdown = "---\nQUILL: taro\nauthor: Test Author\nice_cream: Chocolate\ntitle: Test\n---\n\nContent.\n"
    parsed = Document.from_markdown(markdown)
    assert parsed.quill_ref() == "taro"

    assert "taro" in quill.quill_ref
    assert quill.backend == "typst"
    assert OutputFormat.PDF in quill.supported_formats()

    result = quill.render(parsed, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert result.artifacts[0].output_format == OutputFormat.PDF
    assert len(result.artifacts[0].bytes) > 0


# ---------------------------------------------------------------------------
# Phase 3 — editor surface tests
# ---------------------------------------------------------------------------

SIMPLE_MD = "---\nQUILL: test_quill\ntitle: Hello\nauthor: Alice\n---\n\nBody text.\n"

MD_WITH_LEAVES = """\
---
QUILL: test_quill
title: Hello
---

Body.

```leaf
KIND: note
foo: bar
```

Leaf one.

```leaf
KIND: summary
```

Leaf two.
"""


def test_set_field_inserts():
    """set_field adds a new frontmatter field."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_field("subtitle", "A subtitle")
    assert doc.frontmatter["subtitle"] == "A subtitle"


def test_set_field_updates():
    """set_field updates an existing frontmatter field."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_field("title", "New Title")
    assert doc.frontmatter["title"] == "New Title"


def test_set_field_reserved_name_matrix():
    """set_field raises EditError for all four reserved names."""
    for name in ("BODY", "LEAVES", "QUILL", "KIND"):
        doc = Document.from_markdown(SIMPLE_MD)
        with pytest.raises(EditError, match="ReservedName"):
            doc.set_field(name, "value")


def test_leaf_set_field_reserved_name_matrix():
    """Leaf set_field raises EditError for all four reserved names."""
    for name in ("BODY", "LEAVES", "QUILL", "KIND"):
        doc = Document.from_markdown(MD_WITH_LEAVES)
        with pytest.raises(EditError, match="ReservedName"):
            doc.update_leaf_field(0, name, "value")


def test_set_field_invalid_field_name():
    """set_field raises EditError for an uppercase/invalid name."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(EditError, match="InvalidFieldName"):
        doc.set_field("Title", "value")


def test_remove_field_existing():
    """remove_field removes and returns an existing field."""
    doc = Document.from_markdown(SIMPLE_MD)
    val = doc.remove_field("title")
    assert val == "Hello"
    assert "title" not in doc.frontmatter


def test_remove_field_absent():
    """remove_field returns None when the field doesn't exist."""
    doc = Document.from_markdown(SIMPLE_MD)
    assert doc.remove_field("nonexistent") is None


def test_remove_field_reserved_returns_none():
    """remove_field returns None for reserved names (they can't be in frontmatter)."""
    doc = Document.from_markdown(SIMPLE_MD)
    assert doc.remove_field("BODY") is None


def test_set_quill_ref():
    """set_quill_ref changes the QUILL reference."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_quill_ref("new_quill")
    assert doc.quill_ref() == "new_quill"


def test_replace_body():
    """replace_body replaces the global Markdown body."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.replace_body("New body content.")
    assert doc.body == "New body content."


def test_push_leaf():
    """push_leaf appends a leaf to the list."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.push_leaf({"tag": "note", "body": "Leaf body."})
    assert len(doc.leaves) == 1
    assert doc.leaves[0]["tag"] == "note"
    assert doc.leaves[0]["body"] == "Leaf body."


def test_push_leaf_invalid_tag():
    """push_leaf raises EditError for an invalid tag."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(EditError, match="InvalidTagName"):
        doc.push_leaf({"tag": "BadTag"})


def test_insert_leaf_at_front():
    """insert_leaf at index 0 prepends the leaf."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    doc.insert_leaf(0, {"tag": "intro"})
    assert doc.leaves[0]["tag"] == "intro"
    assert doc.leaves[1]["tag"] == "note"


def test_insert_leaf_out_of_range():
    """insert_leaf raises EditError when index > len."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 leaves
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.insert_leaf(5, {"tag": "note"})


def test_remove_leaf():
    """remove_leaf removes and returns the leaf."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    removed = doc.remove_leaf(0)
    assert removed is not None
    assert removed["tag"] == "note"
    assert len(doc.leaves) == 1
    assert doc.leaves[0]["tag"] == "summary"


def test_remove_leaf_out_of_range():
    """remove_leaf returns None for an out-of-range index."""
    doc = Document.from_markdown(SIMPLE_MD)
    assert doc.remove_leaf(0) is None


def test_move_leaf_no_op():
    """move_leaf(0, 0) is a no-op."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    doc.move_leaf(0, 0)
    assert doc.leaves[0]["tag"] == "note"
    assert doc.leaves[1]["tag"] == "summary"


def test_move_leaf_last_to_first():
    """move_leaf rotates the last leaf to the front."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    last = len(doc.leaves) - 1
    doc.move_leaf(last, 0)
    assert doc.leaves[0]["tag"] == "summary"
    assert doc.leaves[1]["tag"] == "note"


def test_move_leaf_out_of_range():
    """move_leaf raises EditError for an out-of-range index."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.move_leaf(10, 0)


def test_update_leaf_field():
    """update_leaf_field sets a field on a specific leaf."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    doc.update_leaf_field(0, "content", "hello")
    assert doc.leaves[0]["fields"]["content"] == "hello"


def test_update_leaf_field_reserved_name():
    """update_leaf_field raises EditError for reserved names."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    with pytest.raises(EditError, match="ReservedName"):
        doc.update_leaf_field(0, "BODY", "value")


def test_update_leaf_field_out_of_range():
    """update_leaf_field raises EditError when leaf index is out of range."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 leaves
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.update_leaf_field(0, "title", "x")


def test_update_leaf_body():
    """update_leaf_body replaces the leaf body."""
    doc = Document.from_markdown(MD_WITH_LEAVES)
    doc.update_leaf_body(0, "New leaf body.")
    assert doc.leaves[0]["body"] == "New leaf body."


def test_update_leaf_body_out_of_range():
    """update_leaf_body raises EditError when leaf index is out of range."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 leaves
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.update_leaf_body(0, "x")


def test_mutators_do_not_touch_warnings():
    """Mutators must not modify the warnings list."""
    doc = Document.from_markdown(SIMPLE_MD)
    initial = list(doc.warnings)
    doc.set_field("extra", "value")
    doc.replace_body("New body.")
    doc.push_leaf({"tag": "new_leaf"})
    assert list(doc.warnings) == initial


def test_invariants_after_mutation_sequence():
    """After a sequence of mutations the document must be internally consistent."""
    doc = Document.from_markdown(SIMPLE_MD)

    # Add and manipulate leaves
    doc.push_leaf({"tag": "note", "fields": {"text": "hi"}})
    doc.push_leaf({"tag": "summary"})
    doc.push_leaf({"tag": "appendix"})
    doc.insert_leaf(1, {"tag": "intro"})  # note, intro, summary, appendix
    doc.move_leaf(3, 0)                    # appendix, note, intro, summary
    doc.remove_leaf(2)                     # appendix, note, summary

    # Mutate frontmatter
    doc.set_field("extra_author", "Bob")
    doc.remove_field("extra_author")

    # Assertions: no reserved key in frontmatter
    RESERVED = {"BODY", "LEAVES", "QUILL", "KIND"}
    for key in doc.frontmatter:
        assert key not in RESERVED, f"reserved key '{key}' found in frontmatter"

    # Every leaf tag is lowercase-valid (just check non-empty and lowercase)
    for leaf in doc.leaves:
        tag = leaf["tag"]
        assert tag and tag == tag.lower(), f"invalid tag '{tag}'"

    # Document identity preserved
    assert doc.quill_ref() == "test_quill"


# ---------------------------------------------------------------------------
# Phase 4c — emitter integration tests (fromMarkdown → mutate → emit → re-parse)
# ---------------------------------------------------------------------------


def test_to_markdown_general_round_trip():
    """Mutated document survives emit → re-parse with structure intact."""
    doc = Document.from_markdown(SIMPLE_MD)
    original_leaf_count = len(doc.leaves)  # 0 for SIMPLE_MD

    # Mutate
    doc.set_field("title", "New Title")
    doc.push_leaf({"tag": "note", "fields": {"author": "Alice"}, "body": "Hello"})
    doc.replace_body("Updated body")

    # Emit
    emitted = doc.to_markdown()
    assert isinstance(emitted, str)
    assert len(emitted) > 0

    # Re-parse and assert structure survives
    doc2 = Document.from_markdown(emitted)
    assert doc2.frontmatter["title"] == "New Title"
    assert doc2.body == "Updated body"
    assert len(doc2.leaves) == original_leaf_count + 1
    assert doc2.leaves[0]["tag"] == "note"
    assert doc2.leaves[0]["fields"]["author"] == "Alice"
    assert doc2.leaves[0]["body"] == "Hello"


def test_to_markdown_ambiguous_string_survival():
    """YAML-keyword values set via set_field survive emit → re-parse as strings.

    "on", "off", "yes", "no", "true", "false", "null" are all YAML
    booleans/null in permissive parsers. The emitter must double-quote them
    so they survive through a re-parse as strings, not bools or null.
    """
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_field("flag_on", "on")
    doc.set_field("flag_off", "off")
    doc.set_field("flag_yes", "yes")
    doc.set_field("flag_no", "no")
    doc.set_field("str_true", "true")
    doc.set_field("str_false", "false")
    doc.set_field("str_null", "null")
    doc.set_field("octal_str", "01234")
    doc.set_field("date_str", "2024-01-15")

    emitted = doc.to_markdown()
    doc2 = Document.from_markdown(emitted)

    # Every value must survive as a string, not be re-interpreted
    assert doc2.frontmatter["flag_on"] == "on"
    assert doc2.frontmatter["flag_off"] == "off"
    assert doc2.frontmatter["flag_yes"] == "yes"
    assert doc2.frontmatter["flag_no"] == "no"
    assert doc2.frontmatter["str_true"] == "true"
    assert doc2.frontmatter["str_false"] == "false"
    assert doc2.frontmatter["str_null"] == "null"
    assert doc2.frontmatter["octal_str"] == "01234"
    assert doc2.frontmatter["date_str"] == "2024-01-15"
