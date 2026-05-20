"""Tests for rendering workflow."""

import pytest

from quillmark import OutputFormat, Document, ParseError, Quillmark, QuillmarkError


def test_save_artifact(taro_quill_dir, taro_md, tmp_path):
    """Test saving an artifact to file."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = quill.render(parsed, OutputFormat.PDF)

    output_path = tmp_path / "output.pdf"
    result.artifacts[0].save(str(output_path))

    assert output_path.exists()
    assert output_path.stat().st_size > 0


def test_quill_render_from_parsed_document(taro_quill_dir, taro_md):
    """quill.render(Document) accepts a pre-parsed document."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    result = quill.render(parsed)

    assert len(result.artifacts) > 0
    assert len(result.artifacts[0].bytes) > 0


def test_quill_render_with_explicit_format(taro_quill_dir, taro_md):
    """quill.render() honours an explicit OutputFormat argument."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = quill.render(parsed, OutputFormat.SVG)

    assert len(result.artifacts) > 0
    assert result.output_format == OutputFormat.SVG


def test_quill_render_ref_mismatch_warning(taro_quill_dir):
    """Rendering a Document with a mismatched QUILL ref emits a warning."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    # Build a document that names a different quill
    mismatch_md = (
        "~~~card-yaml\n"
        "#@quill: completely_different_quill\n"
        "author: Test Author\n"
        "ice_cream: Chocolate\n"
        "title: Mismatch Test\n"
        "~~~\n\nContent.\n"
    )
    parsed = Document.from_markdown(mismatch_md)
    result = quill.render(parsed)

    codes = [w.code for w in result.warnings]
    assert "quill::ref_mismatch" in codes, f"expected ref_mismatch warning, got: {codes}"
    assert len(result.artifacts) > 0, "artifact must still be produced"


def test_quill_open_session_page_selection(taro_quill_dir, taro_md):
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    session = quill.open(parsed)
    assert session.page_count > 0

    subset = session.render(OutputFormat.SVG, [0])
    assert len(subset.artifacts) == 1
    assert subset.output_format == OutputFormat.SVG


def test_quill_render_full_document(taro_quill_dir, taro_md):
    """quill.render(doc) renders successfully."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = quill.render(parsed, OutputFormat.PDF)

    assert len(result.artifacts) > 0
    assert result.output_format == OutputFormat.PDF


def test_parse_error_carries_diagnostic_payload():
    """ParseError exposes both `.diagnostic` (singular) and `.diagnostics` (list).

    Locks in the v0.81 RenderError unification contract: every binding
    exception carries the diagnostic list; the singular shim is set only
    when there is exactly one diagnostic.
    """
    invalid_md = """---
title: [unclosed bracket
---

Content
"""
    with pytest.raises(ParseError) as exc_info:
        Document.from_markdown(invalid_md)

    exc = exc_info.value
    assert hasattr(exc, "diagnostics"), "exception should carry .diagnostics list"
    assert len(exc.diagnostics) >= 1, "diagnostics must be non-empty"
    assert all(hasattr(d, "message") for d in exc.diagnostics)

    if len(exc.diagnostics) == 1:
        assert hasattr(exc, "diagnostic"), (
            "single-diagnostic exceptions must set the .diagnostic singular shim"
        )
        assert exc.diagnostic.message == exc.diagnostics[0].message


def test_quill_load_error_carries_diagnostics(tmp_path):
    """Quill-loading failures surface as QuillmarkError with diagnostics."""
    bogus = tmp_path / "not_a_quill"
    bogus.mkdir()
    (bogus / "Quill.yaml").write_text("quill: { name: x }\n")  # missing required keys

    engine = Quillmark()
    with pytest.raises(QuillmarkError) as exc_info:
        engine.quill_from_path(str(bogus))

    exc = exc_info.value
    assert hasattr(exc, "diagnostics") and len(exc.diagnostics) >= 1, (
        "quill-load failure must expose at least one diagnostic"
    )
