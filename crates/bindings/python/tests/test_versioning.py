"""Tests for quill loading."""
import pytest
from quillmark import Quillmark, Quill


def test_quill_from_path(taro_quill_dir):
    """Test loading a quill via engine."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))
    assert quill is not None
    assert quill.metadata["name"] == "taro"
    assert quill.backend_id == "typst"


def test_quill_from_path_bad_backend(tmp_path):
    """Test that loading a quill with unknown backend raises error."""
    from quillmark import QuillmarkError
    quill_dir = tmp_path / "test_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "test"\n  version: "1.0"\n  backend: "nonexistent"\n  description: "Test"\n'
    )
    engine = Quillmark()
    with pytest.raises(QuillmarkError):
        engine.quill_from_path(str(quill_dir))
