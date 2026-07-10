"""Tests for Quillmark engine."""

from quillmark import Quill


def test_metadata_never_raises_for_unregistered_backend(tmp_path):
    """metadata is infallible: it never resolves a backend, so it does not
    raise even when the declared backend is not registered."""
    quill_dir = tmp_path / "test_quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(
        'quill:\n  name: "test"\n  version: "1.0"\n  backend: "nonexistent"\n  description: "Test"\n'
    )
    quill = Quill.from_path(str(quill_dir))
    # Pure config read — no UnsupportedBackend here.
    assert quill.metadata["name"] == "test"
    assert quill.metadata["backend"] == "nonexistent"
