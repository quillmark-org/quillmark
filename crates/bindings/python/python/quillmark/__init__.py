"""Quillmark - Python bindings for Quillmark."""

from ._quillmark import (
    Artifact,
    CardView,
    CardWriter,
    Diagnostic,
    Document,
    Location,
    OutputFormat,
    Quill,
    Quillmark,
    QuillmarkError,
    RenderResult,
    Severity,
    View,
    Writer,
)

__all__ = [
    "Artifact",
    "CardView",
    "CardWriter",
    "Diagnostic",
    "Document",
    "Location",
    "OutputFormat",
    "Quill",
    "Quillmark",
    "QuillmarkError",
    "RenderResult",
    "Severity",
    "View",
    "Writer",
]

try:
    from importlib.metadata import version as _version

    __version__ = _version("quillmark")
except Exception:  # pragma: no cover — source tree without installed metadata
    __version__ = "0.0.0"

