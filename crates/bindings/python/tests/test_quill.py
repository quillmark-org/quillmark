"""Tests for quill loading."""
import pytest
from quillmark import Quillmark, Quill, Document, OutputFormat, QuillmarkError


def test_quill_from_path(taro_quill_dir):
    """Quill.from_path loads engine-free, validated config data."""
    quill = Quill.from_path(str(taro_quill_dir))
    assert quill is not None
    assert quill.metadata["name"] == "taro"
    assert quill.backend_id == "typst"


def test_quill_from_path_bad_backend_loads_then_fails_at_render(tmp_path):
    """A quill whose declared backend is not registered loads fine — the
    backend is resolved at render time, not load time. The error surfaces when
    the engine is asked to render or report capability, not from from_path."""
    quill_dir = tmp_path / "test_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "test"\n  version: "1.0"\n  backend: "nonexistent"\n  description: "Test"\n'
    )

    # Engine-free load succeeds: the config is valid, the backend is not resolved.
    quill = Quill.from_path(str(quill_dir))
    assert quill.backend_id == "nonexistent"

    engine = Quillmark()
    # Capability and render both resolve the backend → raise here.
    with pytest.raises(QuillmarkError):
        engine.supported_formats(quill)

    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: test\n$kind: main\n~~~\n\nBody.\n"
    )
    with pytest.raises(QuillmarkError):
        engine.render(quill, doc, OutputFormat.PDF)
