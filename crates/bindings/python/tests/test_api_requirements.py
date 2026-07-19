"""Tests for the API requirements.

Python is a Tier-1 binding: field I/O flows through `quill.writer(doc)` /
`quill.view(doc)`. `Document` carries the quill-free surface — parse, storage,
structure, `$ext` / `$seed`, and `remove_field`. There is no opaque field store
and no content lane (`install` / `revise` / `apply_change` + codec); those are
WASM-only by scope.
"""

import pytest
from quillmark import (
    Quillmark,
    Quill,
    Document,
    OutputFormat,
    QuillmarkError,
)
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


def _richtext_form_quill():
    """The richtext_form fixture quill (headline: richtext inline, bio: richtext)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "richtext_form")))


def _taro_quill():
    """The taro fixture quill (main string fields; a `quotes` card kind)."""
    return Quill.from_path(str(_latest_version(QUILLS_PATH / "taro")))


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
    """Document(quill_ref) starts blank: main card only, no fields, no cards. A
    field write needs a quill (the writer); the blank canvas itself is quill-free."""
    doc = Document("taro@0.1.0")
    assert doc.quill_ref == "taro@0.1.0"
    assert doc.card_count == 0
    assert doc.body["text"] == ""
    assert field_keys(doc.main) == []

    _taro_quill().writer(doc).set("title", "Hello")
    assert field(doc.main, "title") == "Hello"

    with pytest.raises(ValueError, match="QuillReference"):
        Document("not a valid ref!!")


def test_blank_document_renders(engine):
    """The programmatic flow end-to-end: blank canvas → typed writer → render."""
    quill = _taro_quill()

    doc = Document("taro@0.1.0")
    w = quill.writer(doc)
    w.set_all({"title": "Test", "author": "Test Author", "ice_cream": "Chocolate"})
    w.set_body("Content.")

    result = engine.render(quill, doc, OutputFormat.PDF)
    assert len(result.artifacts) > 0
    assert len(result.artifacts[0].bytes) > 0


# ---------------------------------------------------------------------------
# Document surface — quill-free structure, removal, and $ext
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


def test_remove_field_existing():
    """remove_field removes and returns an existing main-card field."""
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


def test_insert_card_appends():
    """insert_card (absent `at`) appends a card to the list."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card({"kind": "note", "body": "Card body."})
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "note"
    assert doc.cards[0]["body"]["text"] == "Card body."


def test_insert_card_invalid_kind():
    """insert_card raises EditError for an invalid kind."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(QuillmarkError, match="InvalidKindName"):
        doc.insert_card({"kind": "BadKind"})


def test_remove_card_then_insert_card_round_trips_fields():
    """A card returned by remove_card feeds straight back into insert_card with
    its fields intact — the one-Card-shape contract. Exercises the explicit
    quill/id/ext=None keys the dict carries against `deny_unknown_fields`."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card(Document.make_card("note", {"author": "Alice"}, "Body"))

    removed = doc.remove_card(0)
    # The returned dict carries explicit None for the absent $ entries.
    assert removed["quill"] is None and removed["id"] is None
    assert field(removed, "author") == "Alice"

    doc.insert_card(removed)  # must not raise (deny_unknown_fields accepts the shape)
    assert len(doc.cards) == 1
    assert doc.cards[0]["kind"] == "note"
    assert field(doc.cards[0], "author") == "Alice"  # field survived the round-trip
    assert doc.cards[0]["body"]["text"] == "Body"


def test_insert_card_accepts_content_dict_body():
    """`body` on an inserted card may be the canonical content dict (the shape
    `cards()`/`remove_card` emit), not just a markdown string — exercises
    `py_dict_to_card`'s content-dict input path."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card({"kind": "note", "body": "**Bold** body."})
    content_body = doc.cards[0]["body"]
    assert isinstance(content_body, dict)

    doc.insert_card({"kind": "note", "body": content_body})
    assert doc.cards[1]["body"]["text"] == doc.cards[0]["body"]["text"]


def test_make_card_accepts_any_kind_insert_card_is_the_gate():
    """make_card is permissive data-shaping; the kind invariant is enforced at
    insert_card, not construction."""
    card = Document.make_card("BadKind", {"x": 1})
    assert card["kind"] == "BadKind"  # construction succeeds
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(QuillmarkError, match="InvalidKindName"):
        doc.insert_card(card)


def test_stale_flat_input_is_a_loud_error():
    """A stale {kind, fields} dict fails loudly rather than yielding an empty
    card: `deny_unknown_fields` on the wire type rejects the unknown `fields`
    key at deserialize time (a ValueError), before any edit."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="fields"):
        doc.insert_card({"kind": "note", "fields": {"x": 1}})


def test_insert_card_at_front():
    """insert_card(card, at=0) prepends the card."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.insert_card({"kind": "intro"}, at=0)
    assert doc.cards[0]["kind"] == "intro"
    assert doc.cards[1]["kind"] == "note"


def test_insert_card_out_of_range():
    """insert_card raises EditError when at > len."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.insert_card({"kind": "note"}, at=5)


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


def test_store_ext_adds_map():
    """store_ext stores an opaque map readable via card['ext']."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext({"presentation": {"title": "Greeting"}})
    assert doc.main["ext"] == {"presentation": {"title": "Greeting"}}


def test_store_ext_rejects_non_dict():
    """store_ext raises for non-dict values."""
    doc = Document.from_markdown(SIMPLE_MD)
    with pytest.raises(ValueError, match="must be a dict"):
        doc.store_ext("nope")


def test_ext_round_trips_through_markdown():
    """$ext survives emit → re-parse."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext({"agent": {"pinned": True}})
    reparsed = Document.from_markdown(doc.to_markdown())
    assert reparsed.main["ext"]["agent"]["pinned"] is True


def test_store_ext_namespace_preserves_siblings():
    """store_ext_namespace merges without clobbering other namespaces."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext_namespace("presentation", {"title": "A"})
    doc.store_ext_namespace("agent", {"pinned": True})
    doc.store_ext_namespace("presentation", {"title": "B"})
    assert doc.main["ext"] == {
        "presentation": {"title": "B"},
        "agent": {"pinned": True},
    }


def test_remove_ext_namespace_clears_one_slot_and_drops_when_empty():
    """remove_ext_namespace clears one slot; the last one drops $ext."""
    doc = Document.from_markdown(SIMPLE_MD)
    doc.store_ext_namespace("presentation", {"title": "A"})
    doc.store_ext_namespace("tutorial", ["step-1", "step-2"])
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
    doc.store_ext({"agent": {"n": 1}})
    assert doc.remove_ext() == {"agent": {"n": 1}}
    assert doc.main["ext"] is None
    assert doc.remove_ext() is None


def test_card_ext_mutators():
    """store_ext / remove_ext with card=i target the composable card at index —
    the same verbs the main card uses, one `card=` selector over the whole axis."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.store_ext({"agent": {"note": "y"}}, card=0)
    assert doc.cards[0]["ext"] == {"agent": {"note": "y"}}
    assert doc.remove_ext(card=0) == {"agent": {"note": "y"}}
    assert doc.cards[0]["ext"] is None


def test_card_ext_namespace_mutators():
    """store/remove_ext_namespace with card=i preserve siblings and clear when empty."""
    doc = Document.from_markdown(MD_WITH_CARDS)
    doc.store_ext_namespace("presentation", {"title": "A"}, card=0)
    doc.store_ext_namespace("tutorial", ["step-1"], card=0)
    assert doc.remove_ext_namespace("tutorial", card=0) == ["step-1"]
    assert doc.cards[0]["ext"] == {"presentation": {"title": "A"}}
    doc.remove_ext_namespace("presentation", card=0)
    assert doc.cards[0]["ext"] is None


def test_card_ext_mutators_out_of_range():
    """Card ext mutators raise IndexOutOfRange for a bad card index."""
    doc = Document.from_markdown(SIMPLE_MD)  # 0 cards
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.store_ext({}, card=0)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.remove_ext(card=0)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.store_ext_namespace("a", {}, card=0)
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        doc.remove_ext_namespace("a", card=0)


def test_mutators_do_not_touch_warnings():
    """Mutators must not modify the warnings list."""
    doc = Document.from_markdown(SIMPLE_MD)
    initial = list(doc.warnings)
    doc.remove_field("title")
    doc.insert_card({"kind": "new_card"})
    doc.store_ext_namespace("agent", {"n": 1})
    assert list(doc.warnings) == initial


def test_invariants_after_mutation_sequence():
    """After a sequence of mutations the document must be internally consistent."""
    import re

    doc = Document.from_markdown(SIMPLE_MD)

    # Add and manipulate cards
    doc.insert_card(Document.make_card("note", {"text": "hi"}))
    doc.insert_card({"kind": "summary"})
    doc.insert_card({"kind": "appendix"})
    doc.insert_card({"kind": "intro"}, at=1)  # note, intro, summary, appendix
    doc.move_card(3, 0)                        # appendix, note, intro, summary
    doc.remove_card(2)                         # appendix, note, summary

    # Mutate payload (quill-free removal)
    doc.remove_field("author")

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
    """A typed-writer mutation survives emit → re-parse with structure intact."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")

    # Mutate: typed field + typed body + a quill-free card.
    quill.writer(doc).set("title", "New Title")
    doc.insert_card(Document.make_card("note", {"author": "Alice"}, "Hello"))
    quill.writer(doc).set_body("Updated body")

    # Emit
    emitted = doc.to_markdown()
    assert isinstance(emitted, str) and len(emitted) > 0

    # Re-parse and assert structure survives
    doc2 = Document.from_markdown(emitted)
    assert field(doc2.main, "title") == "New Title"
    assert doc2.body["text"].rstrip("\n") == "Updated body"
    assert len(doc2.cards) == 1
    assert doc2.cards[0]["kind"] == "note"
    assert field(doc2.cards[0], "author") == "Alice"
    assert doc2.cards[0]["body"]["text"] == "Hello"


def test_to_markdown_ambiguous_string_survival():
    """YAML-keyword string values survive emit → re-parse as strings.

    "on", "off", "yes", "no", "true", "false", "null" are all YAML
    booleans/null in permissive parsers. The emitter must double-quote them
    so they survive a re-parse as strings, not bools or null. Seated as card
    fields via the quill-free `make_card`, whose values emit through the same
    card-yaml writer the main card uses.
    """
    doc = Document.from_markdown(SIMPLE_MD)
    doc.insert_card(
        Document.make_card(
            "note",
            {
                "flag_on": "on",
                "flag_off": "off",
                "flag_yes": "yes",
                "flag_no": "no",
                "str_true": "true",
                "str_false": "false",
                "str_null": "null",
                "octal_str": "01234",
                "date_str": "2024-01-15",
            },
        )
    )

    doc2 = Document.from_markdown(doc.to_markdown())
    card = doc2.cards[0]

    # Every value must survive as a string, not be re-interpreted
    assert field(card, "flag_on") == "on"
    assert field(card, "flag_off") == "off"
    assert field(card, "flag_yes") == "yes"
    assert field(card, "flag_no") == "no"
    assert field(card, "str_true") == "true"
    assert field(card, "str_false") == "false"
    assert field(card, "str_null") == "null"
    assert field(card, "octal_str") == "01234"
    assert field(card, "date_str") == "2024-01-15"


# ── Tier-1 typed writer — quill.writer(doc) front door ────────────────────────


def test_writer_front_door_set_and_reads():
    """quill.writer(doc).set writes; the reads live on quill.view(doc)."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.set("author", "Ada")
    ed.set_all({"title": "On Taro"})
    assert ed.document is doc  # holds the same object, mutated in place

    v = quill.view(doc)
    assert v.get("author") == "Ada"
    assert v.get("ice_cream") is None  # declared but absent → None
    ed.set_body("A **taro** essay.")
    assert v.get_body() == "A **taro** essay."


def test_writer_set_rejects_unknown_field():
    """An undeclared name is a typo on the typed path — it raises, nothing lands."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    with pytest.raises(QuillmarkError, match="UnknownField"):
        quill.writer(doc).set("stray", "x")
    assert not has_field(doc.main, "stray")


def test_writer_set_all_reports_every_unknown_field():
    """set_all is all-or-nothing and reports one diagnostic per undeclared name
    (path = field name) — externally-sourced keys surface every violation at once."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    with pytest.raises(QuillmarkError, match="UnknownField") as exc_info:
        quill.writer(doc).set_all({"title": "ok", "stray1": "x", "stray2": "y"})
    paths = [d.path for d in exc_info.value.diagnostics]
    assert "stray1" in paths and "stray2" in paths
    # All-or-nothing: even the valid `title` did not land.
    assert not has_field(doc.main, "title")


def test_writer_add_card_transactional():
    """add_card fuses make + typed commit + insert; a typo leaves the doc untouched."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"}, "A quote body.")
    assert len(doc.cards) == 1
    assert field(doc.cards[0], "author") == "Basho"
    with pytest.raises(QuillmarkError, match="UnknownField"):
        ed.add_card("quotes", {"stray": "x"})
    assert len(doc.cards) == 1  # nothing joined the document


def test_writer_add_card_positioned():
    """add_card(..., at=i) is one atomic positioned typed insert."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "First"})
    ed.add_card("quotes", {"author": "Second"}, at=0)  # insert at the front
    assert field(doc.cards[0], "author") == "Second"
    assert field(doc.cards[1], "author") == "First"
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        ed.add_card("quotes", {"author": "x"}, at=99)
    assert len(doc.cards) == 2  # the out-of-range insert landed nothing


def test_writer_card_cursor_set_and_body():
    """writer.card(i) targets the composable card; a bad index raises at the write."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"})
    ed.card(0).set("author", "Issa")
    assert field(doc.cards[0], "author") == "Issa"
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        ed.card(9).set("author", "x")


def test_writer_card_kind_getter():
    """writer.card(i).kind reads the bound card's $kind; a bad index raises."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"})
    assert ed.card(0).index == 0
    assert ed.card(0).kind == "quotes"
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        _ = ed.card(9).kind


def test_writer_set_coerces_richtext_to_content():
    """A richtext field commits the canonical content, not the authored markdown."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).set("bio", "A **bold** intro.")
    value = field(doc.main, "bio")
    assert isinstance(value, dict)  # stored as the content dict, not a string
    assert value["text"] == "A bold intro."


def test_writer_set_rejects_inline_violation():
    """A richtext(inline) field rejects multi-block content at the write."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with pytest.raises(QuillmarkError, match="FieldRichtextNotInline"):
        quill.writer(doc).set("headline", "line one\n\nline two")


def test_writer_set_all_is_all_or_nothing():
    """A mid-batch inline violation aborts set_all — nothing lingers."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with pytest.raises(QuillmarkError, match="FieldRichtextNotInline"):
        quill.writer(doc).set_all({"bio": "ok", "headline": "line one\n\nline two"})
    assert not has_field(doc.main, "bio")


def test_writer_revise_field_typed_and_anchor_preserving():
    """writer.revise_field is the typed, anchor-preserving richtext field write —
    diff-imports the markdown and schema-conforms the result."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).revise_field("bio", "make it **bold**")
    assert quill.view(doc).get("bio") == "make it **bold**"


def test_writer_revise_field_rejects_inline_and_unknown():
    """revise_field conforms to the field schema (inline rejects multi-block) and
    rejects an undeclared name — same guards as `set`."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    with pytest.raises(QuillmarkError, match="FieldRichtextNotInline"):
        quill.writer(doc).revise_field("headline", "line one\n\nline two")
    with pytest.raises(QuillmarkError, match="UnknownField"):
        quill.writer(doc).revise_field("nope", "x")


def test_typed_set_clears_must_fill_marker():
    """With the opaque store gone, seed → validate(must_fill) → typed `set` is the
    fill lifecycle: the typed commit lands a real value and clears the marker."""
    quill = _taro_quill()
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: taro@0.1.0\n$kind: main\ntitle: !must_fill\n~~~\n"
    )
    codes = [d.get("code") for d in quill.validate(doc)]
    assert "validation::must_fill" in codes

    quill.writer(doc).set("title", "Real Title")
    codes_after = [d.get("code") for d in quill.validate(doc)]
    assert "validation::must_fill" not in codes_after
    assert field(doc.main, "title") == "Real Title"


# ── Tier-1 typed reader — quill.view(doc) front door ──────────────────────────


def test_view_interprets_by_declared_type():
    """view.get reads a richtext field as markdown and a scalar as its canonical value."""
    quill = _richtext_form_quill()
    doc = Document("richtext_form@0.1.0")
    quill.writer(doc).set("bio", "A **bold** intro.")
    v = quill.view(doc)
    assert v.document is doc  # holds the same object
    assert v.get("bio") == "A **bold** intro."  # richtext → markdown

    taro = _taro_quill()
    tdoc = Document("taro@0.1.0")
    taro.writer(tdoc).set("author", "Ada")
    assert taro.view(tdoc).get("author") == "Ada"  # scalar → canonical


def test_view_absence_returns_none_unknown_name_raises():
    """Absent → None; a name the schema does not declare raises (the schema authority)."""
    quill = _richtext_form_quill()
    v = quill.view(Document("richtext_form@0.1.0"))
    assert v.get("bio") is None  # absent, not a typo
    with pytest.raises(QuillmarkError, match="UnknownField"):
        v.get("nope")  # typo, not absent


def test_view_richtext_holding_scalar_raises_mismatch():
    """A present value that does not decode as richtext raises FieldRichtextDecode.

    Seated quill-free via `from_markdown` (a bare number under a richtext field),
    since the opaque store is gone."""
    quill = _richtext_form_quill()
    doc = Document.from_markdown(
        "~~~card-yaml\n$quill: richtext_form@0.1.0\n$kind: main\nbio: 3\n~~~\n"
    )
    with pytest.raises(QuillmarkError, match="FieldRichtextDecode"):
        quill.view(doc).get("bio")


def test_view_body_read_is_quill_free():
    """view.get_body reads the main body markdown — the quill-free body read."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    quill.writer(doc).set_body("A **taro** essay.")
    assert quill.view(doc).get_body() == "A **taro** essay."


def test_view_card_cursor_reads_through_kind_schema():
    """view.card(i) reads a card field through its $kind; a bad index raises at the read."""
    quill = _taro_quill()
    doc = Document("taro@0.1.0")
    ed = quill.writer(doc)
    ed.add_card("quotes", {"author": "Basho"}, "A quote body.")
    v = quill.view(doc)
    assert v.card(0).kind == "quotes"
    assert v.card(0).get("author") == "Basho"
    assert v.card(0).get_body() == "A quote body."
    with pytest.raises(QuillmarkError, match="UnknownField"):
        v.card(0).get("stray")
    with pytest.raises(QuillmarkError, match="IndexOutOfRange"):
        v.card(9).get("author")
