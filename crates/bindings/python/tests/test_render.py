"""Tests for rendering workflow."""

import pytest

from quillmark import OutputFormat, Document, Quill, QuillmarkError


def test_save_artifact(engine, taro_quill_dir, taro_md, tmp_path):
    """Test saving an artifact to file."""
    quill = Quill.from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = engine.render(quill, parsed, OutputFormat.PDF)

    output_path = tmp_path / "output.pdf"
    result.artifacts[0].save(str(output_path))

    assert output_path.exists()
    assert output_path.stat().st_size > 0


def test_engine_render_from_parsed_document(engine, taro_quill_dir, taro_md):
    """engine.render(quill, Document) accepts a pre-parsed document."""
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    result = engine.render(quill, parsed)

    assert len(result.artifacts) > 0
    assert len(result.artifacts[0].bytes) > 0


def test_engine_render_with_explicit_format(engine, taro_quill_dir, taro_md):
    """engine.render() honours an explicit OutputFormat argument."""
    quill = Quill.from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = engine.render(quill, parsed, OutputFormat.SVG)

    assert len(result.artifacts) > 0
    assert result.format == OutputFormat.SVG
    assert result.artifacts[0].format == OutputFormat.SVG


def test_render_result_carries_render_time(engine, taro_quill_dir, taro_md):
    """RenderResult.render_time_ms mirrors WASM `renderTimeMs`."""
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    result = engine.render(quill, parsed, OutputFormat.PDF)
    assert isinstance(result.render_time_ms, float)
    assert result.render_time_ms >= 0.0


def test_engine_render_name_mismatch_errors(engine, taro_quill_dir):
    """Rendering a Document whose $quill names a different quill is a hard error."""
    quill = Quill.from_path(str(taro_quill_dir))

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

    with pytest.raises(QuillmarkError) as exc_info:
        engine.render(quill, parsed)

    codes = [d.code for d in exc_info.value.diagnostics]
    assert "quill::name_mismatch" in codes, f"expected name_mismatch error, got: {codes}"


def test_engine_open_session_page_selection(engine, taro_quill_dir, taro_md):
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    session = engine.open(quill, parsed)
    assert session.page_count > 0

    subset = session.render(OutputFormat.SVG, pages=[0])
    assert len(subset.artifacts) == 1
    assert subset.format == OutputFormat.SVG


def test_render_session_metadata(engine, taro_quill_dir, taro_md):
    """RenderSession exposes backend_id, supports_canvas, and warnings."""
    quill = Quill.from_path(str(taro_quill_dir))
    parsed = Document.from_markdown(taro_md)

    session = engine.open(quill, parsed)
    assert session.backend_id == quill.backend_id
    assert session.supports_canvas == engine.supports_canvas(quill)
    assert isinstance(session.warnings, list)


def test_engine_supports_canvas(engine, taro_quill_dir):
    """engine.supports_canvas(quill) is True for the typst backend."""
    quill = Quill.from_path(str(taro_quill_dir))
    # The fixture quill uses the typst backend, which is canvas-capable.
    assert engine.supports_canvas(quill) is True


def test_engine_render_full_document(engine, taro_quill_dir, taro_md):
    """engine.render(quill, doc) renders successfully."""
    quill = Quill.from_path(str(taro_quill_dir))

    parsed = Document.from_markdown(taro_md)
    result = engine.render(quill, parsed, OutputFormat.PDF)

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
    """Quill-loading failures surface as QuillmarkError with diagnostics.

    A malformed *config* still fails at load time — `Quill.from_path` validates
    the config eagerly; only backend resolution is deferred to render.
    """
    bogus = tmp_path / "not_a_quill"
    bogus.mkdir()
    (bogus / "Quill.yaml").write_text("quill: { name: x }\n")  # missing required keys

    with pytest.raises(QuillmarkError) as exc_info:
        Quill.from_path(str(bogus))

    exc = exc_info.value
    assert hasattr(exc, "diagnostics") and len(exc.diagnostics) >= 1, (
        "quill-load failure must expose at least one diagnostic"
    )
