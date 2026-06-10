"""Tests for Quillmark engine."""

from quillmark import Quill


def test_quill_metadata_engine_free(taro_quill_dir):
    """Quill.from_path returns an engine-free Quill with config metadata."""
    quill = Quill.from_path(str(taro_quill_dir))

    assert quill.metadata["name"] in quill.quill_ref
    assert quill.backend_id == "typst"


def test_metadata_has_no_supported_formats_key(taro_quill_dir):
    """metadata is a pure config snapshot — capability is not baked in.

    `supportedFormats` was removed from the snapshot; read it from the engine
    via `Quillmark.supported_formats(quill)`.
    """
    quill = Quill.from_path(str(taro_quill_dir))
    assert "supportedFormats" not in quill.metadata


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
