"""Tests for Document."""

import pytest

from quillmark import Document, QuillmarkError

import os
from pathlib import Path


def field(card, key):
    """Return the value of a named field from a card's payload_items list."""
    for item in card["payload_items"]:
        if item["type"] == "field" and item["key"] == key:
            return item["value"]
    return None


def has_field(card, key):
    """True when a named field exists in a card's payload_items."""
    return any(
        i["type"] == "field" and i["key"] == key for i in card["payload_items"]
    )

WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
RESOURCES_PATH = WORKSPACE_ROOT / "crates" / "fixtures" / "resources"
QUILLS_PATH = RESOURCES_PATH / "quills"


def test_parse_markdown(taro_md):
    """Test parsing markdown with payload."""
    doc = Document.from_markdown(taro_md)
    assert "Ice Cream" in str(field(doc.main, "title") or "")


def test_parse_invalid_yaml():
    """Test parsing invalid YAML payload."""
    invalid_md = (
        "~~~card-yaml\n"
        "$quill: test_quill\n"
        "$kind: main\n"
        "title: [unclosed bracket\n"
        "~~~\n\nContent\n"
    )
    with pytest.raises(QuillmarkError):
        Document.from_markdown(invalid_md)


def test_payload_access(taro_md):
    """Test accessing typed payload_items (no $-prefixed metadata as fields)."""
    doc = Document.from_markdown(taro_md)
    assert "Ice Cream" in (field(doc.main, "title") or "")
    # `$`-prefixed metadata is not exposed as payload fields
    assert not has_field(doc.main, "$body")
    assert not has_field(doc.main, "$cards")
    assert not has_field(doc.main, "$quill")


def test_body_is_str(taro_md):
    """Test that body is a str (not None)."""
    doc = Document.from_markdown(taro_md)
    assert isinstance(doc.body, str)
    assert "nutty" in doc.body


def test_body_empty_when_absent():
    """Test that body is empty string when no body content."""
    md = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n~~~\n"
    doc = Document.from_markdown(md)
    assert doc.body == ""


def test_cards_access():
    """Test accessing typed cards list."""
    md = (
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Main\n~~~\n\nGlobal body.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\n~~~\n\nCard body.\n"
    )
    doc = Document.from_markdown(md)
    assert len(doc.cards) == 1
    card = doc.cards[0]
    assert card["kind"] == "note"
    assert field(card, "foo") == "bar"
    assert "Card body." in card["body"]


def test_cards_empty_when_none():
    """Test that cards is an empty list when no cards present."""
    md = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n~~~\n\nBody.\n"
    doc = Document.from_markdown(md)
    assert doc.cards == []


def test_quill_ref(taro_md):
    """Test that quill_ref returns the QUILL reference, including version."""
    doc = Document.from_markdown(taro_md)
    assert doc.quill_ref() == "taro@0.1"


def test_warnings_empty_on_clean_doc(taro_md):
    """Test that warnings is empty for a well-formed document."""
    doc = Document.from_markdown(taro_md)
    assert doc.warnings == []


def test_to_markdown_emits_string(taro_md):
    """Test that to_markdown emits a non-empty markdown string."""
    doc = Document.from_markdown(taro_md)
    emitted = doc.to_markdown()
    assert isinstance(emitted, str)
    assert emitted.strip() != ""


def test_json_dto_round_trip(taro_md):
    """to_json emits a versioned DTO string that from_json round-trips."""
    doc = Document.from_markdown(taro_md)

    dto = doc.to_json()
    assert isinstance(dto, str)
    assert "quillmark/document@0.82.0" in dto

    restored = Document.from_json(dto)
    assert restored.quill_ref() == doc.quill_ref()
    assert restored.to_markdown() == doc.to_markdown()


def test_json_dto_rejects_invalid_input():
    """from_json rejects an unknown schema tag and malformed JSON."""
    with pytest.raises(QuillmarkError):
        Document.from_json('{"schema":"quillmark/document@0.99.0","main":{}}')
    with pytest.raises(QuillmarkError):
        Document.from_json("not json at all")


def test_json_dto_drops_parse_warnings():
    """A DTO-reconstructed document carries no parse-time warnings."""
    # An unknown YAML tag triggers a `parse::unsupported_yaml_tag` warning.
    warn_md = "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hi\nweird: !custom value\n~~~\n\nBody\n"
    doc = Document.from_markdown(warn_md)
    assert len(doc.warnings) > 0, "source document should have a parse warning"

    restored = Document.from_json(doc.to_json())
    assert restored.warnings == []


def test_try_from_json_round_trip(taro_md):
    """try_from_json returns a Document for valid DTOs."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_json()

    restored = Document.try_from_json(dto)
    assert restored is not None
    assert restored.quill_ref() == doc.quill_ref()


def test_try_from_json_returns_none_on_markdown(taro_md):
    """try_from_json returns None for non-DTO content (e.g. raw markdown)."""
    assert Document.try_from_json(taro_md) is None
    assert Document.try_from_json("not json at all") is None
    assert Document.try_from_json('{"schema":"quillmark/document@0.99.0"}') is None


def test_schema_version_of_reads_dto(taro_md):
    """schema_version_of returns the schema tag from a stored DTO."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_json()

    assert Document.schema_version_of(dto) == "quillmark/document@0.82.0"


def test_schema_version_of_returns_unknown_future_versions():
    """schema_version_of returns the raw tag, even for unsupported versions."""
    # Note: this would be rejected by from_json, but schema_version_of returns it
    # so callers can distinguish "build too old" from "payload corrupt".
    future = '{"schema":"quillmark/document@0.99.0"}'
    assert Document.schema_version_of(future) == "quillmark/document@0.99.0"


def test_schema_version_of_returns_none_for_non_dto():
    """schema_version_of returns None when the input is not a schema-tagged object."""
    assert Document.schema_version_of("not json") is None
    assert Document.schema_version_of('{"foo":"bar"}') is None


def test_current_schema_version_matches_emitted_tag(taro_md):
    """current_schema_version equals the tag emitted by to_json."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_json()

    current = Document.current_schema_version()
    assert isinstance(current, str)
    assert Document.schema_version_of(dto) == current


def test_clone_preserves_state(taro_md):
    """clone returns a fresh handle with the same parsed state."""
    doc = Document.from_markdown(taro_md)
    cloned = doc.clone()

    assert cloned.quill_ref() == doc.quill_ref()
    assert cloned == doc


def test_clone_isolates_mutations(taro_md):
    """Mutating a clone does not affect the original."""
    doc = Document.from_markdown(taro_md)
    cloned = doc.clone()

    cloned.set_field("title", "Updated Title")
    assert field(doc.main, "title") != "Updated Title"
    assert field(cloned.main, "title") == "Updated Title"


def test_copy_module_clones(taro_md):
    """The standard library copy module works on Document."""
    import copy

    doc = Document.from_markdown(taro_md)

    shallow = copy.copy(doc)
    deep = copy.deepcopy(doc)

    assert shallow == doc
    assert deep == doc

    shallow.set_field("title", "Shallow Edit")
    assert field(doc.main, "title") != "Shallow Edit"


def test_equals_and_eq(taro_md):
    """equals and == both compare structurally; warnings are ignored."""
    doc1 = Document.from_markdown(taro_md)
    doc2 = Document.from_markdown(taro_md)

    assert doc1.equals(doc2)
    assert doc1 == doc2


def test_eq_after_mutation(taro_md):
    """Mutation breaks equality."""
    doc1 = Document.from_markdown(taro_md)
    doc2 = Document.from_markdown(taro_md)

    doc2.set_field("title", "Different")
    assert doc1 != doc2


def test_card_count_matches_cards_len():
    """card_count is O(1) shortcut for len(cards)."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\ntitle: T\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n\nFirst.\n\n"
        "~~~card-yaml\n$kind: summary\n~~~\n\nSecond.\n"
    )
    doc = Document.from_markdown(md)
    assert doc.card_count == 2
    assert doc.card_count == len(doc.cards)


def test_repr_includes_quill_ref(taro_md):
    """__repr__ surfaces the quill ref and card count."""
    doc = Document.from_markdown(taro_md)
    text = repr(doc)
    assert "Document(" in text
    assert "taro" in text


def test_remove_card_field_returns_value():
    """remove_card_field removes and returns the field's value."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\nbaz: qux\n~~~\n"
    )
    doc = Document.from_markdown(md)

    removed = doc.remove_card_field(0, "foo")
    assert removed == "bar"
    assert not has_field(doc.cards[0], "foo")
    assert field(doc.cards[0], "baz") == "qux"


def test_remove_card_field_absent_returns_none():
    """remove_card_field returns None when the field doesn't exist."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n"
    )
    doc = Document.from_markdown(md)
    assert doc.remove_card_field(0, "missing") is None


def test_remove_card_field_out_of_range():
    """remove_card_field raises EditError for an out-of-range card index."""

    md = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n"
    doc = Document.from_markdown(md)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.remove_card_field(0, "foo")


def test_remove_card_field_legacy_uppercase_rejected():
    """remove_card_field rejects legacy uppercase names as InvalidFieldName."""

    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n"
    )
    doc = Document.from_markdown(md)
    with pytest.raises(QuillmarkError, match="InvalidFieldName"):
        doc.remove_card_field(0, "BODY")
