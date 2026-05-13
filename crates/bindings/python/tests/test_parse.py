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
    """Test accessing typed frontmatter (no BODY/LEAVES/QUILL)."""
    doc = Document.from_markdown(taro_md)
    fm = doc.frontmatter
    assert "title" in fm
    assert "Ice Cream" in fm["title"]
    # BODY, LEAVES, QUILL must NOT appear in frontmatter
    assert "BODY" not in fm
    assert "LEAVES" not in fm
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
    """Test accessing typed leaves list."""
    md = (
        "---\nQUILL: my_quill\ntitle: Main\n---\n\nGlobal body.\n\n"
        "```leaf\nKIND: note\nfoo: bar\n```\n\nCard body.\n"
    )
    doc = Document.from_markdown(md)
    assert len(doc.leaves) == 1
    leaf = doc.leaves[0]
    assert leaf["tag"] == "note"
    assert leaf["fields"]["foo"] == "bar"
    assert "Leaf body." in leaf["body"]


def test_cards_empty_when_none():
    """Test that leaves is an empty list when no leaves present."""
    md = "---\nQUILL: taro\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n---\n\nBody.\n"
    doc = Document.from_markdown(md)
    assert doc.leaves == []


def test_quill_ref(taro_md):
    """Test that quill_ref returns the QUILL field value."""
    doc = Document.from_markdown(taro_md)
    assert doc.quill_ref() == "taro"


def test_warnings_empty_on_clean_doc(taro_md):
    """Test that warnings is empty for a well-formed document."""
    doc = Document.from_markdown(taro_md)
    assert doc.warnings == []


def test_to_markdown_is_stub(taro_md):
    """Test that to_markdown raises NotImplementedError (phase 4 stub)."""
    doc = Document.from_markdown(taro_md)
    with pytest.raises(NotImplementedError):
        doc.to_markdown()
