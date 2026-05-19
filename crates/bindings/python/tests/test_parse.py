"""Tests for Document."""

import pytest

from quillmark import Document, ParseError

import os
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
RESOURCES_PATH = WORKSPACE_ROOT / "crates" / "fixtures" / "resources"
QUILLS_PATH = RESOURCES_PATH / "quills"


def test_parse_markdown(taro_md):
    """Test parsing markdown with frontmatter."""
    doc = Document.from_markdown(taro_md)
    assert "Ice Cream" in str(doc.frontmatter.get("title", ""))


def test_parse_invalid_yaml():
    """Test parsing invalid YAML frontmatter."""
    invalid_md = """---
title: [unclosed bracket
---

Content
"""
    with pytest.raises(ParseError):
        Document.from_markdown(invalid_md)


def test_frontmatter_access(taro_md):
    """Test accessing typed frontmatter (no BODY/CARDS/QUILL)."""
    doc = Document.from_markdown(taro_md)
    fm = doc.frontmatter
    assert "title" in fm
    assert "Ice Cream" in fm["title"]
    # BODY, CARDS, QUILL must NOT appear in frontmatter
    assert "BODY" not in fm
    assert "CARDS" not in fm
    assert "QUILL" not in fm


def test_body_is_str(taro_md):
    """Test that body is a str (not None)."""
    doc = Document.from_markdown(taro_md)
    assert isinstance(doc.body, str)
    assert "nutty" in doc.body


def test_body_empty_when_absent():
    """Test that body is empty string when no body content."""
    md = "---\nQUILL: taro\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n---\n"
    doc = Document.from_markdown(md)
    assert doc.body == ""


def test_cards_access():
    """Test accessing typed cards list."""
    md = (
        "---\nQUILL: my_quill\ntitle: Main\n---\n\nGlobal body.\n\n"
        "```card note\nfoo: bar\n```\n\nCard body.\n"
    )
    doc = Document.from_markdown(md)
    assert len(doc.cards) == 1
    card = doc.cards[0]
    assert card["tag"] == "note"
    assert card["fields"]["foo"] == "bar"
    assert "Card body." in card["body"]


def test_cards_empty_when_none():
    """Test that cards is an empty list when no cards present."""
    md = "---\nQUILL: taro\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n---\n\nBody.\n"
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
    assert "quillmark/document@0.81.0" in dto

    restored = Document.from_json(dto)
    assert restored.quill_ref() == doc.quill_ref()
    assert restored.to_markdown() == doc.to_markdown()


def test_json_dto_rejects_invalid_input():
    """from_json rejects an unknown schema tag and malformed JSON."""
    with pytest.raises(ParseError):
        Document.from_json('{"schema":"quillmark/document@0.99.0","main":{}}')
    with pytest.raises(ParseError):
        Document.from_json("not json at all")


def test_json_dto_drops_parse_warnings():
    """A DTO-reconstructed document carries no parse-time warnings."""
    # An unknown YAML tag triggers a `parse::unsupported_yaml_tag` warning.
    warn_md = "---\nQUILL: my_quill\ntitle: Hi\nweird: !custom value\n---\n\nBody\n"
    doc = Document.from_markdown(warn_md)
    assert len(doc.warnings) > 0, "source document should have a parse warning"

    restored = Document.from_json(doc.to_json())
    assert restored.warnings == []
