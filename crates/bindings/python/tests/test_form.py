"""Smoke tests for quill.form / blank_main / blank_card.

NOTE: These tests cannot run in the devcontainer because the Python binding
is not built with `maturin develop` in this environment.  They are written
to run in CI where `maturin develop` (or `pip install -e .`) is available.

Expected environment: `quillmark` importable from a maturin-built wheel.
"""

import json
import pytest

try:
    from quillmark import Document, Quillmark
    QUILLMARK_AVAILABLE = True
except ImportError:
    QUILLMARK_AVAILABLE = False

pytestmark = pytest.mark.skipif(
    not QUILLMARK_AVAILABLE,
    reason="quillmark native module not available in this environment",
)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

QUILL_YAML_CONTENT = """quill:
  name: py_form_smoke
  version: "1.0"
  backend: typst
  description: Python form smoke test

main:
  fields:
    title:
      type: string
      default: Untitled
    count:
      type: integer

card_kinds:
  note:
    fields:
      body:
        type: string
        default: TBD
      tag:
        type: string
"""

MD_WITH_TITLE = "---\nQUILL: py_form_smoke\ntitle: \"Hello\"\n---\n"
MD_EMPTY = "---\nQUILL: py_form_smoke\n---\n"


def make_quill(tmp_path, yaml_content=QUILL_YAML_CONTENT):
    """Write a minimal quill directory and load it."""
    quill_dir = tmp_path / "quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(yaml_content)
    engine = Quillmark()
    return engine.quill_from_path(quill_dir)


# ---------------------------------------------------------------------------
# Tests: form()
# ---------------------------------------------------------------------------

def test_form_returns_dict(tmp_path):
    """form returns a dict with main, cards, diagnostics."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(MD_WITH_TITLE)

    form = quill.form(doc)

    assert isinstance(form, dict)
    assert "main" in form
    assert "cards" in form
    assert "diagnostics" in form
    assert isinstance(form["cards"], list)
    assert isinstance(form["diagnostics"], list)


def test_form_document_source(tmp_path):
    """Fields present in the document get source='document'."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(MD_WITH_TITLE)

    form = quill.form(doc)
    values = form["main"]["values"]

    assert "title" in values
    assert values["title"]["source"] == "document"
    assert values["title"]["value"] == "Hello"


def test_form_missing_source(tmp_path):
    """Fields absent from doc with no schema default get source='missing'."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(MD_EMPTY)

    form = quill.form(doc)
    values = form["main"]["values"]

    assert "count" in values
    assert values["count"]["source"] == "missing"
    assert values["count"]["value"] is None
    assert values["count"]["default"] is None


def test_form_default_source(tmp_path):
    """Fields absent from doc with a schema default get source='default'."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(MD_EMPTY)

    form = quill.form(doc)
    values = form["main"]["values"]

    assert "title" in values
    assert values["title"]["source"] == "default"
    assert values["title"]["value"] is None
    assert values["title"]["default"] == "Untitled"


def test_form_json_serializable(tmp_path):
    """Form is fully JSON-serializable via json.dumps."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(MD_WITH_TITLE)

    form = quill.form(doc)
    dumped = json.dumps(form)

    assert isinstance(dumped, str)
    assert len(dumped) > 0

    parsed = json.loads(dumped)
    assert parsed["main"]["values"]["title"]["source"] == "document"


def test_form_unknown_card_diagnostic(tmp_path):
    """Unknown card tags produce a diagnostic and are excluded from cards."""
    quill = make_quill(tmp_path)
    md = (
        "---\nQUILL: py_form_smoke\ntitle: \"T\"\n---\n\n"
        "```card ghost_card\nnote: \"B\"\n```\n"
    )
    doc = Document.from_markdown(md)

    form = quill.form(doc)

    assert form["cards"] == [], "unknown-tag card must be excluded"
    diag_codes = [d.get("code") for d in form["diagnostics"]]
    assert "form::unknown_card_tag" in diag_codes, (
        f"expected form::unknown_card_tag diagnostic; got: {diag_codes}"
    )


# ---------------------------------------------------------------------------
# Tests: blank_main / blank_card
# ---------------------------------------------------------------------------

def test_blank_main_returns_card_with_no_document_values(tmp_path):
    """blank_main returns a card with every value at default or missing."""
    quill = make_quill(tmp_path)

    blank = quill.blank_main()

    assert isinstance(blank, dict)
    values = blank["values"]

    assert values["title"]["source"] == "default"
    assert values["title"]["value"] is None
    assert values["title"]["default"] == "Untitled"

    assert values["count"]["source"] == "missing"
    assert values["count"]["value"] is None
    assert values["count"]["default"] is None


def test_blank_card_known_type(tmp_path):
    """blank_card returns a dict for a known card kind."""
    quill = make_quill(tmp_path)

    blank = quill.blank_card("note")

    assert blank is not None
    values = blank["values"]
    assert values["body"]["source"] == "default"
    assert values["body"]["default"] == "TBD"
    assert values["tag"]["source"] == "missing"


def test_blank_card_unknown_type(tmp_path):
    """blank_card returns None for an unknown card kind."""
    quill = make_quill(tmp_path)

    assert quill.blank_card("does_not_exist") is None
