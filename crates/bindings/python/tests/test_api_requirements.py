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

    supported_formats = quill.supported_formats
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
    assert OutputFormat.PDF in quill.supported_formats

    result = quill.render(parsed, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert result.artifacts[0].output_format == OutputFormat.PDF
    assert len(result.artifacts[0].bytes) > 0


# ---------------------------------------------------------------------------
# Phase 3 — editor surface tests
# ---------------------------------------------------------------------------

SIMPLE_MD = "---\nQUILL: test_quill\ntitle: Hello\nauthor: Alice\n---\n\nBody text.\n"

MD_WITH_CARDS = """\
---
QUILL: test_quill
title: Hello
---

Body.

```card note
foo: bar
```

Card one.

```card summary
```

Card two.
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
    for name in ("BODY", "CARDS", "QUILL", "CARD"):
        doc = Document.from_markdown(SIMPLE_MD)
        with pytest.raises(EditError, match="ReservedName"):
            doc.set_field(name, "value")


def test_card_set_field_reserved_name_matrix():
    """Card set_field raises EditError for all four reserved names."""
    for name in ("BODY", "CARDS", "QUILL", "CARD"):
        doc = Document.from_markdown(MD_WITH_CARDS)
        with pytest.raises(EditError, match="ReservedName"):
            doc.update_card_field(0, name, "value")


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


def test_remove_field_reserved_raises():
    """remove_field raises EditError for reserved names."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(EditError, match="ReservedName"):
        doc.remove_field("BODY")


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


def test_push_card():
    """push_card appends a card to the list."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.push_card({"tag": "note", "body": "Card body."})
    assert len(doc.cards) == 1
    assert doc.cards[0]["tag"] == "note"
    assert doc.cards[0]["body"] == "Card body."


def test_push_card_invalid_tag():
    """push_card raises EditError for an invalid tag."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(EditError, match="InvalidTagName"):
        doc.push_card({"tag": "BadTag"})


def test_insert_card_at_front():
    """insert_card at index 0 prepends the card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.insert_card(0, {"tag": "intro"})
    assert doc.cards[0]["tag"] == "intro"
    assert doc.cards[1]["tag"] == "note"


def test_insert_card_out_of_range():
    """insert_card raises EditError when index > len."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.insert_card(5, {"tag": "note"})


def test_remove_card():
    """remove_card removes and returns the card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    removed = doc.remove_card(0)
    assert removed is not None
    assert removed["tag"] == "note"
    assert len(doc.cards) == 1
    assert doc.cards[0]["tag"] == "summary"


def test_remove_card_out_of_range():
    """remove_card returns None for an out-of-range index."""
    doc = Document.from_markdown(SIMPLE_MD)
    assert doc.remove_card(0) is None


def test_move_card_no_op():
    """move_card(0, 0) is a no-op."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.move_card(0, 0)
    assert doc.cards[0]["tag"] == "note"
    assert doc.cards[1]["tag"] == "summary"


def test_move_card_last_to_first():
    """move_card rotates the last card to the front."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    last = len(doc.cards) - 1
    doc.move_card(last, 0)
    assert doc.cards[0]["tag"] == "summary"
    assert doc.cards[1]["tag"] == "note"


def test_move_card_out_of_range():
    """move_card raises EditError for an out-of-range index."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.move_card(10, 0)


def test_update_card_field():
    """update_card_field sets a field on a specific card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.update_card_field(0, "content", "hello")
    assert doc.cards[0]["fields"]["content"] == "hello"


def test_update_card_field_reserved_name():
    """update_card_field raises EditError for reserved names."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    with pytest.raises(EditError, match="ReservedName"):
        doc.update_card_field(0, "BODY", "value")


def test_update_card_field_out_of_range():
    """update_card_field raises EditError when card index is out of range."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.update_card_field(0, "title", "x")


def test_update_card_body():
    """update_card_body replaces the card body."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.update_card_body(0, "New card body.")
    assert doc.cards[0]["body"] == "New card body."


def test_update_card_body_out_of_range():
    """update_card_body raises EditError when card index is out of range."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(EditError, match="IndexOutOfRange"):
        doc.update_card_body(0, "x")


def test_mutators_do_not_touch_warnings():
    """Mutators must not modify the warnings list."""
    doc = Document.from_markdown(SIMPLE_MD)
    initial = list(doc.warnings)
    doc.set_field("extra", "value")
    doc.replace_body("New body.")
    doc.push_card({"tag": "new_card"})
    assert list(doc.warnings) == initial


def test_invariants_after_mutation_sequence():
    """After a sequence of mutations the document must be internally consistent."""
    doc = Document.from_markdown(SIMPLE_MD)

    # Add and manipulate cards
    doc.push_card({"tag": "note", "fields": {"text": "hi"}})
    doc.push_card({"tag": "summary"})
    doc.push_card({"tag": "appendix"})
    doc.insert_card(1, {"tag": "intro"})  # note, intro, summary, appendix
    doc.move_card(3, 0)                    # appendix, note, intro, summary
    doc.remove_card(2)                     # appendix, note, summary

    # Mutate frontmatter
    doc.set_field("extra_author", "Bob")
    doc.remove_field("extra_author")

    # Assertions: no reserved key in frontmatter
    RESERVED = {"BODY", "CARDS", "QUILL", "CARD"}
    for key in doc.frontmatter:
        assert key not in RESERVED, f"reserved key '{key}' found in frontmatter"

    # Every card tag is lowercase-valid (just check non-empty and lowercase)
    for card in doc.cards:
        tag = card["tag"]
        assert tag and tag == tag.lower(), f"invalid tag '{tag}'"

    # Document identity preserved
    assert doc.quill_ref() == "test_quill"


# ---------------------------------------------------------------------------
# Phase 4c — emitter integration tests (fromMarkdown → mutate → emit → re-parse)
# ---------------------------------------------------------------------------


def test_to_markdown_general_round_trip():
    """Mutated document survives emit → re-parse with structure intact."""
    doc = Document.from_markdown(SIMPLE_MD)
    original_card_count = len(doc.cards)  # 0 for SIMPLE_MD

    # Mutate
    doc.set_field("title", "New Title")
    doc.push_card({"tag": "note", "fields": {"author": "Alice"}, "body": "Hello"})
    doc.replace_body("Updated body")

    # Emit
    emitted = doc.to_markdown()
    assert isinstance(emitted, str)
    assert len(emitted) > 0

    # Re-parse and assert structure survives
    doc2 = Document.from_markdown(emitted)
    assert doc2.frontmatter["title"] == "New Title"
    assert doc2.body.rstrip("\n") == "Updated body"
    assert len(doc2.cards) == original_card_count + 1
    assert doc2.cards[0]["tag"] == "note"
    assert doc2.cards[0]["fields"]["author"] == "Alice"
    assert doc2.cards[0]["body"] == "Hello"


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
