"""Tests for the API requirements."""

import re

import pytest
from quillmark import Quillmark, Quill, Document, OutputFormat, QuillmarkError
from conftest import QUILLS_PATH, _latest_version


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


def field_keys(card):
    """Iterable of all field keys in a card, in source order."""
    return [i["key"] for i in card["payload_items"] if i["type"] == "field"]


def test_parsed_document_quill_ref():
    """Test that Document exposes quill_ref method."""
    markdown_with_quill = "~~~card-yaml\n$quill: my_quill\n$kind: main\ntitle: Test\n~~~\n\n# Content\n"
    parsed = Document.from_markdown(markdown_with_quill)
    assert parsed.quill_ref == "my_quill"

    markdown_without_quill = "# Just content\n\nNo card-yaml block here.\n"
    with pytest.raises(QuillmarkError):
        Document.from_markdown(markdown_without_quill)


def test_quill_properties(engine, taro_quill_dir):
    """Quill exposes its engine-free config surface; capability lives on the
    engine, not the quill."""
    quill = Quill.from_path(str(taro_quill_dir))

    metadata = quill.metadata
    assert isinstance(metadata, dict)
    assert metadata["name"] == "taro"
    # metadata is a pure config snapshot — no capability key baked in.
    assert "supportedFormats" not in metadata
    assert quill.backend_id == "typst"
    assert isinstance(quill.blueprint, str) and quill.blueprint != ""

    schema = quill.schema
    assert isinstance(schema, dict)
    assert "main" in schema
    assert "fields" in schema["main"]

    # Capability is resolved by the engine, against the quill.
    supported_formats = engine.supported_formats(quill)
    assert isinstance(supported_formats, list)
    assert OutputFormat.PDF in supported_formats


def test_full_workflow(engine):
    """Test loading a quill engine-free and rendering through the engine."""
    taro_dir = QUILLS_PATH / "taro"
    quill = Quill.from_path(str(_latest_version(taro_dir)))

    markdown = "~~~card-yaml\n$quill: taro\n$kind: main\nauthor: Test Author\nice_cream: Chocolate\ntitle: Test\n~~~\n\nContent.\n"
    parsed = Document.from_markdown(markdown)
    assert parsed.quill_ref == "taro"

    assert "taro" in quill.quill_ref
    assert quill.backend_id == "typst"
    assert OutputFormat.PDF in engine.supported_formats(quill)

    result = engine.render(quill, parsed, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert result.artifacts[0].format == OutputFormat.PDF
    assert len(result.artifacts[0].bytes) > 0


def test_blank_document_constructor():
    """Document(quill_ref) starts blank: main card only, no fields, no cards."""
    doc = Document("test_quill")
    assert doc.quill_ref == "test_quill"
    assert doc.card_count == 0
    assert doc.body_markdown == ""
    assert field_keys(doc.main) == []
    doc.set_fields({"title": "Hello"})
    assert field(doc.main, "title") == "Hello"
    with pytest.raises(ValueError, match="QuillReference"):
        Document("not a valid ref!!")


def test_blank_document_renders(engine):
    """The programmatic flow end-to-end: blank canvas → set_fields → render."""
    taro_dir = QUILLS_PATH / "taro"
    quill = Quill.from_path(str(_latest_version(taro_dir)))

    doc = Document("taro")
    doc.set_fields({"title": "Test", "author": "Test Author", "ice_cream": "Chocolate"})
    doc.replace_body("Content.")

    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert len(result.artifacts[0].bytes) > 0


# ---------------------------------------------------------------------------
# Editor surface tests
# ---------------------------------------------------------------------------

SIMPLE_MD = "~~~card-yaml\n$quill: test_quill\n$kind: main\ntitle: Hello\nauthor: Alice\n~~~\n\nBody text.\n"

MD_WITH_CARDS = """\
~~~card-yaml
$quill: test_quill
$kind: main
title: Hello
~~~

Body.

~~~card-yaml
$kind: note
foo: bar
~~~

Card one.

~~~card-yaml
$kind: summary
~~~

Card two.
"""


def test_set_field_inserts():
    """set_field adds a new payload field."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_field("subtitle", "A subtitle")
    assert field(doc.main, "subtitle") == "A subtitle"


def test_set_field_updates():
    """set_field updates an existing payload field."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_field("title", "New Title")
    assert field(doc.main, "title") == "New Title"


def test_set_field_uppercase_accepted():
    """set_field accepts uppercase field names verbatim (lowercase is canonical
    but not enforced); only `$`-prefixed keys stay reserved."""
    doc = Document.from_markdown(SIMPLE_MD)
    for name in ("BODY", "CARDS", "Title", "MixedCase_1"):
        doc.set_field(name, "value")
        assert field(doc.main, name) == "value"


def test_set_field_dollar_prefix_rejected_matrix():
    """set_field raises InvalidFieldName for `$`-prefixed names."""
    for name in ("$body", "$cards", "$quill", "$kind"):
        doc = Document.from_markdown(SIMPLE_MD)
        with pytest.raises(QuillmarkError, match="InvalidFieldName"):
            doc.set_field(name, "value")


def test_set_field_invalid_field_name():
    """set_field raises EditError for an invalid name (hyphen not allowed)."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(QuillmarkError, match="InvalidFieldName"):
        doc.set_field("bad-name", "value")


def test_set_fields_inserts_batch_in_order():
    """set_fields applies every entry, in dict order."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_fields({"subtitle": "A subtitle", "pages": 3})
    assert field(doc.main, "subtitle") == "A subtitle"
    assert field(doc.main, "pages") == 3
    assert field_keys(doc.main) == ["title", "author", "subtitle", "pages"]


def test_set_fields_reports_every_violation_and_applies_nothing():
    """A failed batch raises one diagnostic per bad field (path = name) and
    leaves the document untouched — including the batch's valid entries."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(QuillmarkError, match="InvalidFieldName") as exc_info:
        doc.set_fields({"ok_field": "v", "bad-name": "v", "also bad": "v"})
    diags = exc_info.value.diagnostics
    assert [d.path for d in diags] == ["bad-name", "also bad"]
    assert not has_field(doc.main, "ok_field")


def test_update_card_fields_batch():
    """update_card_fields is the card-indexed twin of set_fields."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.update_card_fields(0, {"foo": "baz", "extra": 1})
    assert field(doc.cards[0], "foo") == "baz"
    assert field(doc.cards[0], "extra") == 1
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.update_card_fields(99, {"foo": "v"})


def test_remove_field_existing():
    """remove_field removes and returns an existing field."""
    doc = Document.from_markdown(SIMPLE_MD)
    val = doc.remove_field("title")
    assert val == "Hello"
    assert not has_field(doc.main, "title")


def test_remove_field_absent():
    """remove_field returns None when the field doesn't exist."""
    doc = Document.from_markdown(SIMPLE_MD)
    assert doc.remove_field("nonexistent") is None


def test_set_quill_ref():
    """set_quill_ref changes the quill reference."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_quill_ref("new_quill")
    assert doc.quill_ref == "new_quill"


def test_replace_body():
    """replace_body replaces the global Markdown body."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.replace_body("New body content.")
    assert doc.body_markdown == "New body content.\n"


def test_push_card():
    """push_card appends a card to the list."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.push_card({"kind": "note", "body": "Card body."})
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "note"
    assert doc.cards[0]["body_markdown"] == "Card body.\n"


def test_push_card_invalid_kind():
    """push_card raises EditError for an invalid kind."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(QuillmarkError, match="InvalidKindName"):
        doc.push_card({"kind": "BadKind"})


def test_remove_card_then_push_card_round_trips_fields():
    """A card returned by remove_card feeds straight back into push_card with
    its fields intact — the one-Card-shape contract. Exercises the explicit
    quill/id/ext=None keys the dict carries against `deny_unknown_fields`."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.push_card(Document.make_card("note", {"author": "Alice"}, "Body"))

    removed = doc.remove_card(0)
    # The returned dict carries explicit None for the absent $ entries.
    assert removed["quill"] is None and removed["id"] is None
    assert field(removed, "author") == "Alice"

    doc.push_card(removed)  # must not raise (deny_unknown_fields accepts the shape)
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "note"
    assert field(doc.cards[0], "author") == "Alice"  # field survived the round-trip
    assert doc.cards[0]["body_markdown"] == "Body\n"


def test_make_card_accepts_any_kind_push_card_is_the_gate():
    """make_card is permissive data-shaping; the kind invariant is enforced at
    push_card, not construction."""
    card = Document.make_card("BadKind", {"x": 1})
    assert card["kind"] == "BadKind"  # construction succeeds
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(QuillmarkError, match="InvalidKindName"):
        doc.push_card(card)


def test_stale_flat_input_is_a_loud_error():
    """A stale {kind, fields} dict fails loudly rather than yielding an empty
    card: `deny_unknown_fields` on the wire type rejects the unknown `fields`
    key at deserialize time (a ValueError), before any edit."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="fields"):
        doc.push_card({"kind": "note", "fields": {"x": 1}})


def test_insert_card_at_front():
    """insert_card at index 0 prepends the card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.insert_card(0, {"kind": "intro"})
    assert doc.cards[0]["kind"] == "intro"
    assert doc.cards[1]["kind"] == "note"


def test_insert_card_out_of_range():
    """insert_card raises EditError when index > len."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.insert_card(5, {"kind": "note"})


def test_remove_card():
    """remove_card removes and returns the card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    removed = doc.remove_card(0)
    assert removed is not None
    assert removed["kind"] == "note"
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "summary"


def test_remove_card_out_of_range():
    """remove_card returns None for an out-of-range index."""
    doc = Document.from_markdown(SIMPLE_MD)
    assert doc.remove_card(0) is None


def test_move_card_no_op():
    """move_card(0, 0) is a no-op."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.move_card(0, 0)
    assert doc.cards[0]["kind"] == "note"
    assert doc.cards[1]["kind"] == "summary"


def test_move_card_last_to_first():
    """move_card rotates the last card to the front."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    last = len(doc.cards) - 1
    doc.move_card(last, 0)
    assert doc.cards[0]["kind"] == "summary"
    assert doc.cards[1]["kind"] == "note"


def test_move_card_out_of_range():
    """move_card raises EditError for an out-of-range index."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.move_card(10, 0)


def test_update_card_field():
    """update_card_field sets a field on a specific card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.update_card_field(0, "content", "hello")
    assert field(doc.cards[0], "content") == "hello"


def test_update_card_field_out_of_range():
    """update_card_field raises EditError when card index is out of range."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.update_card_field(0, "title", "x")


def test_update_card_body():
    """update_card_body replaces the card body."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.update_card_body(0, "New card body.")
    assert doc.cards[0]["body_markdown"] == "New card body.\n"


def test_update_card_body_out_of_range():
    """update_card_body raises EditError when card index is out of range."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.update_card_body(0, "x")


def test_set_ext_adds_map():
    """set_ext stores an opaque map readable via card['ext']."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_ext({"presentation": {"title": "Greeting"}})
    assert doc.main["ext"] == {"presentation": {"title": "Greeting"}}


def test_set_ext_rejects_non_dict():
    """set_ext raises for non-dict values."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="must be a dict"):
        doc.set_ext("nope")


def test_ext_round_trips_through_markdown():
    """$ext survives emit → re-parse."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_ext({"agent": {"pinned": True}})
    reparsed = Document.from_markdown(doc.to_markdown())
    assert reparsed.main["ext"]["agent"]["pinned"] is True


def test_set_ext_namespace_preserves_siblings():
    """set_ext_namespace merges without clobbering other namespaces."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_ext_namespace("presentation", {"title": "A"})
    doc.set_ext_namespace("agent", {"pinned": True})
    doc.set_ext_namespace("presentation", {"title": "B"})
    assert doc.main["ext"] == {
        "presentation": {"title": "B"},
        "agent": {"pinned": True},
    }


def test_remove_ext_namespace_clears_one_slot_and_drops_when_empty():
    """remove_ext_namespace clears one slot; the last one drops $ext."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_ext_namespace("presentation", {"title": "A"})
    doc.set_ext_namespace("tutorial", ["step-1", "step-2"])
    # Returns the removed value; siblings survive.
    assert doc.remove_ext_namespace("tutorial") == ["step-1", "step-2"]
    assert doc.main["ext"] == {"presentation": {"title": "A"}}
    # Removing the last namespace clears $ext entirely.
    doc.remove_ext_namespace("presentation")
    assert doc.main["ext"] is None
    # Absent namespace is a no-op returning None.
    assert doc.remove_ext_namespace("nope") is None


def test_remove_ext_returns_previous_and_clears():
    """remove_ext returns the previous map, then None."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_ext({"agent": {"n": 1}})
    assert doc.remove_ext() == {"agent": {"n": 1}}
    assert doc.main["ext"] is None
    assert doc.remove_ext() is None


def test_card_ext_mutators():
    """set_card_ext / remove_card_ext target the card at index."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.set_card_ext(0, {"agent": {"note": "y"}})
    assert doc.cards[0]["ext"] == {"agent": {"note": "y"}}
    assert doc.remove_card_ext(0) == {"agent": {"note": "y"}}
    assert doc.cards[0]["ext"] is None


def test_card_ext_namespace_mutators():
    """set/remove_card_ext_namespace preserve siblings and clear when empty."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.set_card_ext_namespace(0, "presentation", {"title": "A"})
    doc.set_card_ext_namespace(0, "tutorial", ["step-1"])
    assert doc.remove_card_ext_namespace(0, "tutorial") == ["step-1"]
    assert doc.cards[0]["ext"] == {"presentation": {"title": "A"}}
    doc.remove_card_ext_namespace(0, "presentation")
    assert doc.cards[0]["ext"] is None


def test_card_ext_mutators_out_of_range():
    """Card ext mutators raise IndexOutOfRange for a bad index."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.set_card_ext(0, {})
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.remove_card_ext(0)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.set_card_ext_namespace(0, "a", {})
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.remove_card_ext_namespace(0, "a")


def test_mutators_do_not_touch_warnings():
    """Mutators must not modify the warnings list."""
    doc = Document.from_markdown(SIMPLE_MD)
    initial = list(doc.warnings)
    doc.set_field("extra", "value")
    doc.replace_body("New body.")
    doc.push_card({"kind": "new_card"})
    assert list(doc.warnings) == initial


def test_invariants_after_mutation_sequence():
    """After a sequence of mutations the document must be internally consistent."""
    doc = Document.from_markdown(SIMPLE_MD)

    # Add and manipulate cards
    doc.push_card(Document.make_card("note", {"text": "hi"}))
    doc.push_card({"kind": "summary"})
    doc.push_card({"kind": "appendix"})
    doc.insert_card(1, {"kind": "intro"})  # note, intro, summary, appendix
    doc.move_card(3, 0)                    # appendix, note, intro, summary
    doc.remove_card(2)                     # appendix, note, summary

    # Mutate payload
    doc.set_field("extra_author", "Bob")
    doc.remove_field("extra_author")

    # Assertions: every payload key passes the user-field regex.
    field_name_re = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
    for key in field_keys(doc.main):
        assert field_name_re.match(key), f"invalid key '{key}' found in payload"

    # Every card kind is lowercase-valid (just check non-empty and lowercase)
    for card in doc.cards:
        kind = card["kind"]
        assert kind and kind == kind.lower(), f"invalid kind '{kind}'"

    # Document identity preserved
    assert doc.quill_ref == "test_quill"


# ---------------------------------------------------------------------------
# Emitter integration tests (fromMarkdown → mutate → emit → re-parse)
# ---------------------------------------------------------------------------


def test_to_markdown_general_round_trip():
    """Mutated document survives emit → re-parse with structure intact."""
    doc = Document.from_markdown(SIMPLE_MD)
    original_card_count = len(doc.cards)  # 0 for SIMPLE_MD

    # Mutate
    doc.set_field("title", "New Title")
    doc.push_card(Document.make_card("note", {"author": "Alice"}, "Hello"))
    doc.replace_body("Updated body")

    # Emit
    emitted = doc.to_markdown()
    assert isinstance(emitted, str)
    assert len(emitted) > 0

    # Re-parse and assert structure survives
    doc2 = Document.from_markdown(emitted)
    assert field(doc2.main, "title") == "New Title"
    assert doc2.body_markdown.rstrip("\n") == "Updated body"
    assert len(doc2.cards) == original_card_count + 1
    assert doc2.cards[0]["kind"] == "note"
    assert field(doc2.cards[0], "author") == "Alice"
    assert doc2.cards[0]["body_markdown"] == "Hello\n"


def test_to_markdown_ambiguous_string_survival():
    """YAML-keyword values set via set_field survive emit → re-parse as strings.

    "on", "off", "yes", "no", "true", "false", "null" are all YAML
    booleans/null in permissive parsers. The emitter must double-quote them
    so they survive through a re-parse as strings, not bools or null.
    """
    doc = Document.from_markdown(SIMPLE_MD)
    doc.set_field("flag_on", "on")
    doc.set_field("flag_off", "off")
    doc.set_field("flag_yes", "yes")
    doc.set_field("flag_no", "no")
    doc.set_field("str_true", "true")
    doc.set_field("str_false", "false")
    doc.set_field("str_null", "null")
    doc.set_field("octal_str", "01234")
    doc.set_field("date_str", "2024-01-15")

    emitted = doc.to_markdown()
    doc2 = Document.from_markdown(emitted)

    # Every value must survive as a string, not be re-interpreted
    assert field(doc2.main, "flag_on") == "on"
    assert field(doc2.main, "flag_off") == "off"
    assert field(doc2.main, "flag_yes") == "yes"
    assert field(doc2.main, "flag_no") == "no"
    assert field(doc2.main, "str_true") == "true"
    assert field(doc2.main, "str_false") == "false"
    assert field(doc2.main, "str_null") == "null"
    assert field(doc2.main, "octal_str") == "01234"
    assert field(doc2.main, "date_str") == "2024-01-15"
