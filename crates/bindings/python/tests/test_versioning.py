"""Tests for quill loading."""
import pytest
from quillmark import Quillmark, Quill, QuillmarkError


def test_quill_from_path(taro_quill_dir):
    """Quill.from_path loads engine-free, validated config data."""
    quill = Quill.from_path(str(taro_quill_dir))
    assert quill is not None
    assert quill.metadata["name"] == "taro"
    assert quill.backend_id == "typst"


def test_quill_from_path_bad_backend_defers_to_render(tmp_path):
    """An unregistered backend is not an error at load time — from_path is a
    pure config load. The capability probe on the engine raises instead."""
    quill_dir = tmp_path / "test_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "test"\n  version: "1.0"\n  backend: "nonexistent"\n  description: "Test"\n'
    )
    quill = Quill.from_path(str(quill_dir))  # succeeds — engine-free
    engine = Quillmark()
    with pytest.raises(QuillmarkError):
        engine.supported_formats(quill)
