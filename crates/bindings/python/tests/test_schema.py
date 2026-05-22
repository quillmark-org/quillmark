"""Tests for the Must Fill / Endorsed schema surface.

A field's *cell* is determined by whether the schema declares a `default:`.

- No `default:` -> **Must Fill**: the blueprint renders `<must-fill>` and
  validation reports ``validation::required_field_absent`` if the field
  is absent at validate time and ``validation::unfilled_placeholder`` if
  the `<must-fill>` sentinel survives into the rendered document.
- With `default:` -> **Endorsed**: the blueprint renders the default
  value with a ``; skip-ok`` annotation; the field is optional and the
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
    """Endorsed fields carry the `default` key; Must Fill fields don't."""
    quill = make_quill(tmp_path)
    fields = quill.schema["main"]["fields"]

    # Must Fill: no `default`
    assert "default" not in fields["title"], (
        "title is Must Fill — no default should be reported"
    )
    assert "default" not in fields["count"], (
        "count is Must Fill — no default should be reported"
    )

    # Endorsed: schema carries `default`
    assert fields["status"]["default"] == "draft"


# ---------------------------------------------------------------------------
# Blueprint surface — annotations and sentinels
# ---------------------------------------------------------------------------

def test_blueprint_must_fill_sentinel(tmp_path):
    """Must Fill cells render the literal `<must-fill>` sentinel."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # Must Fill fields carry the sentinel
    assert "title: <must-fill>" in bp, (
        f"expected `title: <must-fill>` in blueprint; got:\n{bp}"
    )
    assert "count: <must-fill>" in bp, (
        f"expected `count: <must-fill>` in blueprint; got:\n{bp}"
    )


def test_blueprint_endorsed_skip_ok(tmp_path):
    """Endorsed cells carry `; skip-ok` after the type annotation."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # The Endorsed `status` field renders its default value with `; skip-ok`.
    # The exact format is `status: draft  # string; skip-ok`.
    assert "status: draft" in bp, f"expected default in blueprint; got:\n{bp}"
    assert "skip-ok" in bp, (
        f"expected `; skip-ok` annotation on Endorsed cell; got:\n{bp}"
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


def test_render_reports_required_field_absent(tmp_path):
    """A Must Fill field absent from the document fails validation with
    ``validation::required_field_absent``.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "status: ready\n"          # Endorsed override
        # title and count omitted — Must Fill, no defaults
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    with pytest.raises(QuillmarkError) as exc_info:
        quill.render(doc, OutputFormat.PDF)

    codes = _diag_codes(exc_info.value)
    assert "validation::required_field_absent" in codes, (
        f"expected validation::required_field_absent; got: {codes}"
    )


def test_render_reports_unfilled_placeholder(tmp_path):
    """A field whose value is still the literal `<must-fill>` sentinel
    fails validation with ``validation::unfilled_placeholder``.
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
    assert "validation::unfilled_placeholder" in codes, (
        f"expected validation::unfilled_placeholder; got: {codes}"
    )


def test_render_does_not_emit_legacy_missing_required(tmp_path):
    """The legacy ``validation::missing_required`` code must be gone.

    Triggering the same condition (absent Must Fill field) must surface
    the new ``validation::required_field_absent`` code instead.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    with pytest.raises(QuillmarkError) as exc_info:
        quill.render(doc, OutputFormat.PDF)

    codes = _diag_codes(exc_info.value)
    assert "validation::missing_required" not in codes, (
        "legacy code `validation::missing_required` must no longer appear; "
        f"got: {codes}"
    )


def test_render_succeeds_when_must_fill_supplied(tmp_path):
    """Filling every Must Fill field renders successfully — Endorsed
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
