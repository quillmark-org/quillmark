"""Tests for Document."""

import pytest

from quillmark import (
    Document,
    QuillmarkError,
    import_markdown,
    export_markdown,
    rebase,
    map_pos,
)

import os
from pathlib import Path


def field(card, key):
    """Return the value of a named field from a card's payload_items list."""
    for item in card["payload_items"]:
        if item["type"] == "field" and item["key"] == key:
            return item["value"]
    return None


def has_field(card, key):
    """True when a named field exists in a card's payload_items."""
    return any(
        i["type"] == "field" and i["key"] == key for i in card["payload_items"]
    )

WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
RESOURCES_PATH = WORKSPACE_ROOT / "crates" / "fixtures" / "resources"
QUILLS_PATH = RESOURCES_PATH / "quills"


def test_payload_access(taro_md):
    """Test accessing typed payload_items (no $-prefixed metadata as fields)."""
    doc = Document.from_markdown(taro_md)
    assert "Ice Cream" in (field(doc.main, "title") or "")
    # `$`-prefixed metadata is not exposed as payload fields
    assert not has_field(doc.main, "$body")
    assert not has_field(doc.main, "$cards")
    assert not has_field(doc.main, "$quill")


def test_body_is_str(taro_md):
    """Test that body is a str (not None)."""
    doc = Document.from_markdown(taro_md)
    # `body` is the canonical content (a dict); its markdown projection is the
    # on-demand `export_markdown(body)` codec.
    assert isinstance(doc.body, dict)
    assert isinstance(export_markdown(doc.body), str)
    assert "nutty" in export_markdown(doc.body)


def test_body_empty_when_absent():
    """Test that body is empty string when no body content."""
    md = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n~~~\n"
    doc = Document.from_markdown(md)
    assert export_markdown(doc.body) == ""


def test_cards_access():
    """Test accessing typed cards list."""
    md = (
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Main\n~~~\n\nGlobal body.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\n~~~\n\nCard body.\n"
    )
    doc = Document.from_markdown(md)
    assert len(doc.cards) == 1
    card = doc.cards[0]
    assert card["kind"] == "note"
    assert field(card, "foo") == "bar"
    assert "Card body." in export_markdown(card["body"])


def test_cards_empty_when_none():
    """Test that cards is an empty list when no cards present."""
    md = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test\ntitle: Test\nice_cream: Vanilla\n~~~\n\nBody.\n"
    doc = Document.from_markdown(md)
    assert doc.cards == []


def test_quill_ref(taro_md):
    """Test that quill_ref returns the QUILL reference, including version."""
    doc = Document.from_markdown(taro_md)
    assert doc.quill_ref == "taro@0.1"


def test_warnings_empty_on_clean_doc(taro_md):
    """Test that warnings is empty for a well-formed document."""
    doc = Document.from_markdown(taro_md)
    assert doc.warnings == []


def test_to_markdown_emits_string(taro_md):
    """Test that to_markdown emits a non-empty markdown string."""
    doc = Document.from_markdown(taro_md)
    emitted = doc.to_markdown()
    assert isinstance(emitted, str)
    assert emitted.strip() != ""


def test_json_dto_round_trip(taro_md):
    """to_json emits a versioned DTO string that from_json round-trips."""
    doc = Document.from_markdown(taro_md)

    dto = doc.to_json()
    assert isinstance(dto, str)
    assert "quillmark/document@0.93.0" in dto

    restored = Document.from_json(dto)
    assert restored.quill_ref == doc.quill_ref
    assert restored.to_markdown() == doc.to_markdown()


def test_json_dto_rejects_invalid_input():
    """from_json rejects an unknown schema tag and malformed JSON."""
    with pytest.raises(QuillmarkError):
        Document.from_json('{"schema":"quillmark/document@0.99.0","main":{}}')
    with pytest.raises(QuillmarkError):
        Document.from_json("not json at all")


def test_json_dto_drops_parse_warnings():
    """A DTO-reconstructed document carries no parse-time warnings."""
    # An unknown YAML tag triggers a `parse::unsupported_yaml_tag` warning.
    warn_md = "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hi\nweird: !custom value\n~~~\n\nBody\n"
    doc = Document.from_markdown(warn_md)
    assert len(doc.warnings) > 0, "source document should have a parse warning"

    restored = Document.from_json(doc.to_json())
    assert restored.warnings == []


def test_try_from_json_round_trip(taro_md):
    """try_from_json returns a Document for valid DTOs."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_json()

    restored = Document.try_from_json(dto)
    assert restored is not None
    assert restored.quill_ref == doc.quill_ref


def test_try_from_json_returns_none_on_markdown(taro_md):
    """try_from_json returns None for non-DTO content (e.g. raw markdown)."""
    assert Document.try_from_json(taro_md) is None
    assert Document.try_from_json("not json at all") is None
    assert Document.try_from_json('{"schema":"quillmark/document@0.99.0"}') is None


def test_schema_version_of_reads_dto(taro_md):
    """schema_version_of returns the schema tag from a stored DTO."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_json()

    assert Document.schema_version_of(dto) == "quillmark/document@0.93.0"


def test_schema_version_of_returns_unknown_future_versions():
    """schema_version_of returns the raw tag, even for unsupported versions."""
    # Note: this would be rejected by from_json, but schema_version_of returns it
    # so callers can distinguish "build too old" from "payload corrupt".
    future = '{"schema":"quillmark/document@0.99.0"}'
    assert Document.schema_version_of(future) == "quillmark/document@0.99.0"


def test_schema_version_of_returns_none_for_non_dto():
    """schema_version_of returns None when the input is not a schema-tagged object."""
    assert Document.schema_version_of("not json") is None
    assert Document.schema_version_of('{"foo":"bar"}') is None


def test_current_schema_version_matches_emitted_tag(taro_md):
    """current_schema_version equals the tag emitted by to_json."""
    doc = Document.from_markdown(taro_md)
    dto = doc.to_json()

    current = Document.current_schema_version()
    assert isinstance(current, str)
    assert Document.schema_version_of(dto) == current


def test_clone_preserves_state(taro_md):
    """clone returns a fresh handle with the same parsed state."""
    doc = Document.from_markdown(taro_md)
    cloned = doc.clone()

    assert cloned.quill_ref == doc.quill_ref
    assert cloned == doc


def test_clone_isolates_mutations(taro_md):
    """Mutating a clone does not affect the original."""
    doc = Document.from_markdown(taro_md)
    cloned = doc.clone()

    cloned.store_field("title", "Updated Title")
    assert field(doc.main, "title") != "Updated Title"
    assert field(cloned.main, "title") == "Updated Title"


def test_copy_module_clones(taro_md):
    """The standard library copy module works on Document."""
    import copy

    doc = Document.from_markdown(taro_md)

    shallow = copy.copy(doc)
    deep = copy.deepcopy(doc)

    assert shallow == doc
    assert deep == doc

    shallow.store_field("title", "Shallow Edit")
    assert field(doc.main, "title") != "Shallow Edit"


def test_equals_and_eq(taro_md):
    """equals and == both compare structurally; warnings are ignored."""
    doc1 = Document.from_markdown(taro_md)
    doc2 = Document.from_markdown(taro_md)

    assert doc1.equals(doc2)
    assert doc1 == doc2


def test_eq_after_mutation(taro_md):
    """Mutation breaks equality."""
    doc1 = Document.from_markdown(taro_md)
    doc2 = Document.from_markdown(taro_md)

    doc2.store_field("title", "Different")
    assert doc1 != doc2


def test_card_count_matches_cards_len():
    """card_count is O(1) shortcut for len(cards)."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\ntitle: T\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n\nFirst.\n\n"
        "~~~card-yaml\n$kind: summary\n~~~\n\nSecond.\n"
    )
    doc = Document.from_markdown(md)
    assert doc.card_count == 2
    assert doc.card_count == len(doc.cards)


def test_repr_includes_quill_ref(taro_md):
    """__repr__ surfaces the quill ref and card count."""
    doc = Document.from_markdown(taro_md)
    text = repr(doc)
    assert "Document(" in text
    assert "taro" in text


def test_remove_card_field_returns_value():
    """remove_card_field removes and returns the field's value."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\nbaz: qux\n~~~\n"
    )
    doc = Document.from_markdown(md)

    removed = doc.remove_card_field(0, "foo")
    assert removed == "bar"
    assert not has_field(doc.cards[0], "foo")
    assert field(doc.cards[0], "baz") == "qux"


def test_remove_card_field_absent_returns_none():
    """remove_card_field returns None when the field doesn't exist."""
    md = (
        "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n\nBody.\n\n"
        "~~~card-yaml\n$kind: note\n~~~\n"
    )
    doc = Document.from_markdown(md)
    assert doc.remove_card_field(0, "missing") is None


def test_remove_card_field_out_of_range():
    """remove_card_field raises EditError for an out-of-range card index."""

    md = "~~~card-yaml\n$quill: q\n$kind: main\n~~~\n"
    doc = Document.from_markdown(md)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.remove_card_field(0, "foo")


def test_diagnostic_str_is_canonical_pretty_text():
    """str(diagnostic) is the canonical pretty-printed text; repr is concise."""
    warn_md = (
        "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Hi\n"
        "weird: !custom value\n~~~\n\nBody\n"
    )
    doc = Document.from_markdown(warn_md)
    assert len(doc.warnings) > 0, "source document should have a parse warning"

    diag = doc.warnings[0]
    pretty = str(diag)
    assert isinstance(pretty, str) and pretty.strip() != ""
    assert diag.message in pretty
    assert "Diagnostic(" in repr(diag)


def test_document_authoring_text_helpers():
    """Document exposes the canonical core authoring texts (WASM parity)."""
    rules = Document.format_rules()
    assert isinstance(rules, str) and rules.strip() != ""

    hint = Document.quill_ref_hint()
    assert isinstance(hint, str) and hint.strip() != ""

    instr = Document.blueprint_instruction("taro")
    assert isinstance(instr, str) and "taro" in instr


def test_install_body_corpus_round_trip():
    """`install(rt)` installs the content dict `body` reads back — value semantics,
    lossless (not markdown-forced). The kwargs idiom of WASM `install`."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: T\n~~~\n\n**bold** body\n"
    )
    content = doc.body
    assert isinstance(content, dict)
    # Install the read-back content into a fresh doc: lossless.
    doc2 = Document.from_markdown("~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: T\n~~~\n")
    doc2.install(content)
    assert doc2.body == content
    # The cold markdown path is spelled with the codec; clearing installs empty.
    doc2.install(import_markdown("plain text"))
    assert "plain text" in export_markdown(doc2.body)
    doc2.install(import_markdown(""))
    assert export_markdown(doc2.body) == ""
    # A markdown string cannot be installed directly (value semantics, content only).
    with pytest.raises((QuillmarkError, ValueError)):
        doc2.install("plain markdown")


def test_addressed_card_body_and_field_projection():
    """`install(rt, card=i)` writes a composable card's content body; a committed
    richtext field projects through `export_markdown` (the codec that replaces
    the retired `field_markdown` / `card_field_markdown`)."""
    md = (
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: Main\n~~~\n\nMain body.\n\n"
        "~~~card-yaml\n$kind: note\nfoo: bar\n~~~\n\nCard body.\n"
    )
    doc = Document.from_markdown(md)
    doc.install(import_markdown("**new** card body"), card=0)
    assert "**new** card body" in export_markdown(doc.cards[0]["body"])
    # A revise on a card body returns a Delta receipt.
    delta = doc.revise("plain card body", card=0)
    assert isinstance(delta["ops"], list)
    # An absent field has no value to project.
    assert field(doc.main, "missing") is None


def test_nested_fill_exposed_as_nested_fills():
    """A nested !must_fill marker is exposed as `nestedFills` on the field item
    and survives the storage round-trip (binding parity for nested fills)."""
    md = (
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\n"
        "addr:\n  street: !must_fill\n  city: Anytown\n~~~\n"
    )
    doc = Document.from_markdown(md)
    addr = next(i for i in doc.main["payload_items"] if i.get("key") == "addr")
    assert addr.get("nestedFills") == [["street"]], addr
    # Storage round-trip preserves the nested marker.
    restored = Document.from_json(doc.to_json())
    assert "street: !must_fill" in restored.to_markdown()


def _nest(levels, leaf):
    """Wrap `leaf` in `levels` nested {"a": …} objects, built iteratively."""
    v = leaf
    for _ in range(levels):
        v = {"a": v}
    return v


def test_depth_bound_matches_core_container_levels():
    """py_to_json_at and core's json_depth_exceeds reject the identical shape.

    The cutoff is container levels (100), not nodes: a scalar leaf at the
    bottom is not charged a level, so exactly 100 nested objects are accepted
    and 101 are rejected — whether the deepest container holds a scalar or
    another (non-empty) container. Pins the scalar-leaf boundary the core test
    `value.rs::depth_check_counts_container_levels_not_the_scalar_leaf` also pins.
    """
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: x\n~~~\n"
    )

    # Scalar-terminated: 100 objects with a scalar leaf is at the limit.
    doc.store_field("ok_scalar", _nest(100, 1))
    with pytest.raises((QuillmarkError, ValueError)):
        doc.store_field("deep_scalar", _nest(101, 1))

    # Container-terminated: the deepest container, not its contents, occupies
    # the last level, so the boundary is identical.
    doc.store_field("ok_container", _nest(99, [1, 2, 3]))
    with pytest.raises((QuillmarkError, ValueError)):
        doc.store_field("deep_container", _nest(100, [1, 2, 3]))


def test_nested_fill_push_card_round_trip():
    """A card dict carrying `nestedFills` can be pushed and the nested marker
    is reconstructed on emit (the dict -> Card serde path reads it back)."""
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: q@0.1\n$kind: main\ntitle: x\n~~~\n"
    )
    doc.push_card(
        {
            "kind": "note",
            "payload_items": [
                {
                    "type": "field",
                    "key": "addr",
                    "value": {"street": None, "city": "A"},
                    "nestedFills": [["street"]],
                }
            ],
            "body": "",
        }
    )
    assert "street: !must_fill" in doc.to_markdown()
