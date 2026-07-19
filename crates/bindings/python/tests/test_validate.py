"""Smoke tests for quill.validate and quill.seed_document.

NOTE: These tests cannot run in the devcontainer because the Python binding
is not built with `maturin develop` in this environment.  They are written
to run in CI where `maturin develop` (or `pip install -e .`) is available.

Expected environment: `quillmark` importable from a maturin-built wheel.
"""

import json
import pytest

try:
    from quillmark import Document, Quill
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
  name: py_validate_smoke
  version: "1.0"
  backend: typst
  description: Python validate smoke test

main:
  fields:
    title:
      type: string
    count:
      type: integer
    byline:
      type: string
      example: FIRST LAST

card_kinds:
  note:
    fields:
      body:
        type: string
        default: TBD
      tag:
        type: string
        example: NOTE TAG
"""


def make_quill(tmp_path, yaml_content=QUILL_YAML_CONTENT):
    """Write a minimal quill directory and load it (engine-free)."""
    quill_dir = tmp_path / "quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(yaml_content)
    return Quill.from_path(quill_dir)


def _md(*lines):
    fields = "".join(f"{line}\n" for line in lines)
    return f"~~~card-yaml\n$quill: py_validate_smoke\n$kind: main\n{fields}~~~\n"


# ---------------------------------------------------------------------------
# Tests: validate()
# ---------------------------------------------------------------------------

def test_validate_returns_empty_list_for_clean_document(tmp_path):
    """A complete, well-formed document produces no diagnostics."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md('title: "Hello"', "count: 1", 'byline: "A B"'))

    diags = quill.validate(doc)

    assert isinstance(diags, list)
    assert diags == []


def test_validate_forwards_type_mismatch(tmp_path):
    """A bad type surfaces with its canonical code, path, and hint."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md('title: "Hello"', 'count: "not-a-number"'))

    diags = quill.validate(doc)
    mismatch = next(
        (d for d in diags if d.get("code") == "validation::type_mismatch"), None
    )
    assert mismatch is not None, f"expected type_mismatch; got: {diags}"
    assert mismatch["path"] == "count"
    assert mismatch.get("hint")


def test_validate_reports_unknown_card_kind(tmp_path):
    """An undeclared card kind surfaces under validation::unknown_card."""
    quill = make_quill(tmp_path)
    md = (
        '~~~card-yaml\n$quill: py_validate_smoke\n$kind: main\ntitle: "T"\ncount: 1\n~~~\n\n'
        '~~~card-yaml\n$kind: ghost\nbody: "B"\n~~~\n'
    )
    doc = Document.from_markdown(md)

    diags = quill.validate(doc)
    codes = [d.get("code") for d in diags]
    assert "validation::unknown_card" in codes, f"got: {codes}"


def test_validate_json_serializable(tmp_path):
    """The diagnostics list is fully JSON-serializable via json.dumps."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md('count: "nope"'))

    diags = quill.validate(doc)
    dumped = json.dumps(diags)
    assert isinstance(dumped, str)
    assert len(json.loads(dumped)) == len(diags)


# ---------------------------------------------------------------------------
# Tests: seed_document (the Document-path starter; replaces blank_main/blank_card)
# ---------------------------------------------------------------------------

def test_seed_document_commits_examples(tmp_path):
    """seed_document returns a Document committing example values and leaving
    default-only fields absent (interpolated at render, not persisted)."""
    quill = make_quill(tmp_path)

    doc = quill.seed_document()
    md = doc.to_markdown()

    assert "FIRST LAST" in md, "byline example must be committed"
    assert "TBD" not in md, "note body default must not be persisted"


def test_seed_main_and_card(tmp_path):
    """seed_main / seed_card return per-card seeds (the Document.main / cards
    dict shape), each committing its fields' example; seed_card is None for an
    unknown kind."""
    quill = make_quill(tmp_path)

    main = quill.seed_main()
    assert main["kind"] == "main"
    assert "FIRST LAST" in json.dumps(main), "byline example must be committed"

    note = quill.seed_card("note")
    assert note["kind"] == "note"
    assert "NOTE TAG" in json.dumps(note), "tag example must be committed"

    assert quill.seed_card("missing") is None, "unknown kind must be None"


def test_seed_card_with_overlay_layers_over_example(tmp_path):
    """seed_card(kind, overlay) layers the overlay over the schema example
    (overlay > example); without an overlay the bare example is used."""
    quill = make_quill(tmp_path)

    # Override `tag`; pin `body` (a default-only field with no example).
    card = quill.seed_card("note", {"tag": "PINNED", "body": "Pinned body"})
    blob = json.dumps(card)
    assert "PINNED" in blob, "overlay value wins over the example"
    assert "Pinned body" in blob, "overlay can pin a default-only field"

    bare = json.dumps(quill.seed_card("note"))
    assert "NOTE TAG" in bare and "PINNED" not in bare


def test_document_seed_and_store_seed_namespace_round_trip(tmp_path):
    """main['seed'][kind] reads what store_seed_namespace wrote; the overlay
    feeds straight back into seed_card; remove_seed_namespace clears it."""

    def seed_of(document, kind):
        # The per-kind overlay lives on the main card's `$seed` map; there is
        # no `Document.seed` convenience.
        return (document.main["seed"] or {}).get(kind)

    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md())  # empty main card

    assert seed_of(doc, "note") is None
    doc.store_seed_namespace("note", {"tag": "WRITTEN"})
    assert seed_of(doc, "note")["tag"] == "WRITTEN"

    card = quill.seed_card("note", seed_of(doc, "note"))
    assert "WRITTEN" in json.dumps(card)

    doc.remove_seed_namespace("note")
    assert seed_of(doc, "note") is None


def test_seed_overlay_validation_is_advisory(tmp_path):
    """A type-mismatched overlay surfaces a WARNING rooted at its seed path —
    seed overlays are advisory and never gate render."""
    quill = make_quill(tmp_path)
    doc = Document.from_markdown(_md("$seed:", "  note:", "    tag: 123"))

    diags = quill.validate(doc)
    seed_diags = [d for d in diags if (d.get("path") or "").startswith("$seed")]
    assert seed_diags, f"expected a seed-rooted diagnostic; got: {diags}"
    assert all(d.get("severity") == "warning" for d in seed_diags)
