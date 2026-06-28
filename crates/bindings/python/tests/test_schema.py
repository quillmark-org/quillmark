"""Tests for the Unendorsed / Endorsed schema surface.

A field's *cell* is determined by whether the schema declares a `default:`.

- No `default:` -> **Unendorsed**: the blueprint renders the ``!must_fill``
  marker. A marker left in the document is non-fatal: validate reports a
  ``validation::must_fill`` warning and render still succeeds (the field
  zero-fills or uses its suggested value).
- With `default:` -> **Endorsed**: the blueprint renders the default
  value with a type-only ``# <type>`` annotation; the field is optional and
  the default is used when absent.
"""

from quillmark import Document, OutputFormat, Quill


QUILL_YAML_CONTENT = """quill:
  name: py_schema_smoke
  version: "1.0"
  backend: typst
  description: Python schema/blueprint smoke test

typst:
  plate_file: plate.typ

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
    return Quill.from_path(str(quill_dir))


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
# Blueprint surface — annotations and markers
# ---------------------------------------------------------------------------

def test_blueprint_must_fill_marker(tmp_path):
    """Unendorsed cells render the `!must_fill` marker."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # Unendorsed fields carry the marker
    assert "title: !must_fill" in bp, (
        f"expected `title: !must_fill` in blueprint; got:\n{bp}"
    )
    assert "count: !must_fill" in bp, (
        f"expected `count: !must_fill` in blueprint; got:\n{bp}"
    )


def test_blueprint_endorsed_value(tmp_path):
    """Endorsed cells render the concrete default with a type-only annotation."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # The Endorsed `status` field renders its default value with a type-only
    # annotation. The exact format is `status: draft # string`.
    assert "status: draft" in bp, f"expected default in blueprint; got:\n{bp}"
    # Shippability is the value cell — the `; delete-ok` tag is gone entirely.
    assert "delete-ok" not in bp, (
        f"expected no `; delete-ok` tag in blueprint; got:\n{bp}"
    )


def test_blueprint_no_legacy_required_optional_tags(tmp_path):
    """The blueprint grammar has no `; required` / `; optional` role tags."""
    quill = make_quill(tmp_path)
    bp = quill.blueprint

    # Role tags are never emitted.
    assert "; required" not in bp, (
        f"`; required` tag must not appear in blueprint:\n{bp}"
    )
    assert "; optional" not in bp, (
        f"`; optional` tag must not appear in blueprint:\n{bp}"
    )


# ---------------------------------------------------------------------------
# Validation surface — new diagnostic codes
# ---------------------------------------------------------------------------

def test_absent_unendorsed_is_nonfatal(engine, tmp_path):
    """An absent Unendorsed field is not a render gate.

    Per the zero-filled-render contract (``prose/canon/SCHEMAS.md``), render
    succeeds — each absent field is zero-filled in the ephemeral plate
    projection. Absence is silent: ``validation::field_absent`` is not emitted by
    ``quill.validate``.
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
    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0

    # Absence is silent — field_absent is removed and never surfaced.
    codes = [d.get("code") for d in quill.validate(doc)]
    assert "validation::field_absent" not in codes, (
        f"field_absent is removed and must not be surfaced; got: {codes}"
    )


def test_render_tolerates_must_fill_marker(engine, tmp_path):
    """A ``!must_fill`` marker left in the document is non-fatal.

    Render still succeeds (the field zero-fills or uses its suggested value),
    and ``quill.validate`` surfaces a non-fatal ``validation::must_fill``
    warning for the marker.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "title: !must_fill\n"       # marker left in place
        "count: 1\n"
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    # The marker does not gate render.
    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0

    # validate surfaces a non-fatal warning for the marker.
    diags = quill.validate(doc)
    fill = [d for d in diags if d.get("code") == "validation::must_fill"]
    assert any(d.get("path") == "title" for d in fill), (
        f"expected a validation::must_fill warning on `title`; got: {diags}"
    )
    assert all(d.get("severity") == "warning" for d in fill), (
        f"validation::must_fill must be a non-fatal warning; got: {fill}"
    )


def test_absent_unendorsed_does_not_emit_legacy_codes(engine, tmp_path):
    """An absent Unendorsed field emits no completeness/required codes.

    Absence is silent under zero-filled render (``prose/canon/SCHEMAS.md``):
    render succeeds and ``quill.validate`` surfaces no ``field_absent`` or
    legacy ``required`` codes. The removed ``validation::field_absent`` and
    the legacy ``validation::missing_required``,
    ``validation::required_field_absent``, and
    ``validation::unfilled_placeholder`` codes never appear.
    """
    quill = make_quill(tmp_path)
    md = (
        "~~~card-yaml\n"
        "$quill: py_schema_smoke\n"
        "$kind: main\n"
        "~~~\n"
    )
    doc = Document.from_markdown(md)

    # render zero-fills absent fields and succeeds
    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0

    # the validate surface carries none of these codes
    codes = [d.get("code") for d in quill.validate(doc)]
    assert "validation::field_absent" not in codes, (
        f"`validation::field_absent` is removed; got: {codes}"
    )
    assert "validation::missing_required" not in codes, (
        f"`validation::missing_required` must not appear; got: {codes}"
    )
    assert "validation::required_field_absent" not in codes, (
        f"`validation::required_field_absent` must not appear; got: {codes}"
    )
    assert "validation::unfilled_placeholder" not in codes, (
        f"`validation::unfilled_placeholder` must not appear; got: {codes}"
    )


def test_render_succeeds_when_unendorsed_supplied(engine, tmp_path):
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

    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert result.artifacts[0].format == OutputFormat.PDF
