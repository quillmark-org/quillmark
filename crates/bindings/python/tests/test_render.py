"""Tests for rendering workflow."""

import pytest

from quillmark import OutputFormat, Document, Quillmark, QuillmarkError


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
    assert result.format == OutputFormat.SVG
    assert result.artifacts[0].format == OutputFormat.SVG


def test_render_result_carries_render_time(taro_quill_dir, taro_md):
    """RenderResult.render_time_ms mirrors WASM `renderTimeMs`."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    result = quill.render(parsed, OutputFormat.PDF)
    assert isinstance(result.render_time_ms, float)
    assert result.render_time_ms >= 0.0


def test_quill_render_ref_mismatch_warning(taro_quill_dir):
    """Rendering a Document with a mismatched QUILL ref emits a warning."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    # Build a document that names a different quill
    mismatch_md = (
        "~~~card-yaml\n"
        "$quill: completely_different_quill\n"
        "$kind: main\n"
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

    subset = session.render(OutputFormat.SVG, pages=[0])
    assert len(subset.artifacts) == 1
    assert subset.format == OutputFormat.SVG


def test_render_session_metadata(taro_quill_dir, taro_md):
    """RenderSession exposes backend_id, supports_canvas, and warnings."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    session = quill.open(parsed)
    assert session.backend_id == quill.backend_id
    assert session.supports_canvas == quill.supports_canvas
    assert isinstance(session.warnings, list)


def test_quill_supports_canvas(taro_quill_dir):
    """Quill.supports_canvas is True for the typst backend."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))
    # The fixture quill uses the typst backend, which is canvas-capable.
    assert quill.supports_canvas is True


def test_quill_render_full_document(taro_quill_dir, taro_md):
    """quill.render(doc) renders successfully."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = quill.render(parsed, OutputFormat.PDF)

    assert len(result.artifacts) > 0
    assert result.format == OutputFormat.PDF
    assert result.artifacts[0].format == OutputFormat.PDF
    assert result.artifacts[0].mime_type == "application/pdf"


def test_parse_error_carries_diagnostics():
    """Parse failures raise QuillmarkError with a non-empty `.diagnostics` list.

    Matches WASM contract: single exception type, diagnostics uniformly attached.
    """
    invalid_md = """~~~card-yaml
$quill: test_quill
$kind: main
title: [unclosed bracket
~~~

Content
"""
    with pytest.raises(QuillmarkError) as exc_info:
        Document.from_markdown(invalid_md)

    exc = exc_info.value
    assert hasattr(exc, "diagnostics"), "exception should carry .diagnostics list"
    assert len(exc.diagnostics) >= 1, "diagnostics must be non-empty"
    assert all(hasattr(d, "message") for d in exc.diagnostics)


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
