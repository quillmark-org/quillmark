"""Tests for Quillmark engine."""

from quillmark import Quillmark


def test_quill_metadata_from_engine(taro_quill_dir):
    """Engine.quill_from_path returns a renderable Quill with backend metadata."""
    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_quill_dir))

    assert quill.metadata["name"] in quill.quill_ref
    assert quill.backend_id == "typst"
