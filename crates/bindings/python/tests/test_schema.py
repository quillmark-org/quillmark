"""Tests for the Unendorsed / Endorsed schema surface.

A field's *cell* is determined by whether the schema declares a `default:`.

- No `default:` -> **Unendorsed**: the blueprint renders `<must-fill>` and
  validation reports ``validation::field_absent`` if the field
  is absent at validate time and ``validation::must_fill_sentinel`` if
  the `<must-fill>` sentinel survives into the rendered document.
- With `default:` -> **Endorsed**: the blueprint renders the default
  value with a ``; delete-ok`` annotation; the field is optional and the
  default is used when absent.
"""

import pytest

from quillmark import Document, OutputFormat, Quillmark, QuillmarkError


QUILL_YAML_CONTENT = """quill:
  name: py_schema_smoke
  version: "1.0"
  backend: typst
  plate_file: plate.typ
  description: Python schema/blueprint smoke test

main:
  fields:
    title:
      description: Document title
      type: string
    status:
      description: Document status
      type: string
      default: draft
    count:
      type: integer
"""

PLATE_TYP = "Title: {{ title }} / Status: {{ status }} / Count: {{ count }}"


def make_quill(tmp_path, yaml_content=QUILL_YAML_CONTENT, plate=PLATE_TYP):
    quill_dir = tmp_path / "quill"
    quill_dir.mkdir()
    (quill_dir / "Quill.yaml").write_text(yaml_content)
    (quill_dir / "plate.typ").write_text(plate)
    engine = Quillmark()
    return engine.quill_from_path(str(quill_dir))


# ---------------------------------------------------------------------------
# Schema surface — `required:` is gone; cells are inferred from `default:`.
# ---------------------------------------------------------------------------

def test_schema_has_no_required_key(tmp_path):
    """The schema dict never carries a `required:` key on a field.

    Cell is inferred from the presence/absence of `default:`.
    """
    quill = make_quill(tmp_path)
    schema = quill.schema

    fields = schema["main"]["fields"]
    for name, field in fields.items():
        assert "required" not in field, (
            f"field {name!r} unexpectedly carries `required`; "
            "the schema axis is now `default`-driven"
        )


def test_schema_default_marks_endorsed(tmp_path):
    """Endorsed fields carry the `default` key; Unendorsed fields don't."""
    quill = make_quill(tmp_path)
    fields = quill.schema["main"]["fields"]

    # Unendorsed: no `default`
    assert "default" not in fields["title"], (
        "title is Unendorsed — no default should be reported"
    )
    assert "default" not in fields["count"], (
        "count is Unendorsed — no default should be reported"
    )

    # Endorsed: schema carries `default`
    assert fields["status"]["default"] == "draft"


# ---------------------------------------------------------------------------
# Blueprint surface — annotations and sentinels
# ---------------------------------------------------------------------------

def test_blueprint_must_fill_sentinel(tmp_path):
    """Unendorsed cells render the literal `<must-fill>` sentinel."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # Unendorsed fields carry the sentinel
    assert "title: <must-fill>" in bp, (
        f"expected `title: <must-fill>` in blueprint; got:\n{bp}"
    )
    assert "count: <must-fill>" in bp, (
        f"expected `count: <must-fill>` in blueprint; got:\n{bp}"
    )


def test_blueprint_endorsed_delete_ok(tmp_path):
    """Endorsed cells carry `; delete-ok` after the type annotation."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # The Endorsed `status` field renders its default value with `; delete-ok`.
    # The exact format is `status: draft  # string; delete-ok`.
    assert "status: draft" in bp, f"expected default in blueprint; got:\n{bp}"
    assert "delete-ok" in bp, (
        f"expected `; delete-ok` annotation on Endorsed cell; got:\n{bp}"
    )


def test_blueprint_no_legacy_required_optional_tags(tmp_path):
    """Old `; required` / `; optional` role tags are gone from the grammar."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # The legacy role tags are dead — never emitted.
    assert "; required" not in bp, (
        f"legacy `; required` tag must not appear in blueprint:\n{bp}"
    )
    assert "; optional" not in bp, (
        f"legacy `; optional` tag must not appear in blueprint:\n{bp}"
    )


# ---------------------------------------------------------------------------
# Validation surface — new diagnostic codes
# ---------------------------------------------------------------------------

def _diag_codes(exc):
    return [d.code for d in exc.diagnostics]


def test_absent_unendorsed_is_nonfatal_signal(tmp_path):
    """An absent Unendorsed field is a non-fatal completeness signal, not a
    render gate.

    Per the zero-filled-render contract (``prose/canon/SCHEMAS.md``), render
    succeeds — each absent field is zero-filled in the ephemeral plate
    projection — and ``validation::field_absent`` surfaces through
    ``quill.validate``, which render demotes.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "status: ready\n"          # Endorsed override
        # title and count omitted — Unendorsed, no defaults
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    # Absence does not gate render: a merely incomplete document renders fine.
    result = quill.render(doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0

    # The completeness signal is surfaced by validate, not render.
    codes = [d.get("code") for d in quill.validate(doc)]
    assert "validation::field_absent" in codes, (
        f"expected validate to surface field_absent; got: {codes}"
    )


def test_render_reports_must_fill_sentinel(tmp_path):
    """A field whose value is still the literal `<must-fill>` sentinel
    fails validation with ``validation::must_fill_sentinel``.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "title: <must-fill>\n"      # sentinel left in place
        "count: 1\n"
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    with pytest.raises(QuillmarkError) as exc_info:
        quill.render(doc, OutputFormat.PDF)

    codes = _diag_codes(exc_info.value)
    assert "validation::must_fill_sentinel" in codes, (
        f"expected validation::must_fill_sentinel; got: {codes}"
    )


def test_absent_unendorsed_does_not_emit_legacy_codes(tmp_path):
    """The legacy ``validation::missing_required`` code must be gone.

    The same condition (absent Unendorsed field) must surface the new
    ``validation::field_absent`` code instead. The intermediate codes
    ``validation::required_field_absent`` and ``validation::unfilled_placeholder``
    were also retired in favor of ``validation::field_absent`` and
    ``validation::must_fill_sentinel``. Absence is a non-fatal signal (zero-filled
    render — ``prose/canon/SCHEMAS.md``) carried by ``quill.validate``, so the
    codes are checked there rather than on a render error.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    # render demotes field_absent → zero-fills and succeeds
    result = quill.render(doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0

    # the validate surface carries the canonical code, never the legacy ones
    codes = [d.get("code") for d in quill.validate(doc)]
    assert "validation::field_absent" in codes, (
        f"expected canonical field_absent; got: {codes}"
    )
    assert "validation::missing_required" not in codes, (
        "legacy code `validation::missing_required` must no longer appear; "
        f"got: {codes}"
    )
    assert "validation::required_field_absent" not in codes, (
        "retired code `validation::required_field_absent` must no longer "
        f"appear; got: {codes}"
    )
    assert "validation::unfilled_placeholder" not in codes, (
        "retired code `validation::unfilled_placeholder` must no longer "
        f"appear; got: {codes}"
    )


def test_render_succeeds_when_unendorsed_supplied(tmp_path):
    """Filling every Unendorsed field renders successfully — Endorsed
    fields fall back to their declared default."""
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "title: Hello\n"
        "count: 7\n"
        # status omitted → falls back to its default "draft"
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    result = quill.render(doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert result.artifacts[0].format == OutputFormat.PDF
