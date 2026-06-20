using System;
using System.Collections.Generic;
using System.Text.Json.Nodes;

namespace Quillmark;

/// <summary>
/// A typed, in-memory Quillmark document. The .NET analogue of the Python
/// <c>Document</c> class: constructors from Markdown / storage JSON, readers
/// for the card model, and the full mutator surface. Field values cross the
/// boundary as JSON, so any <see cref="JsonNode"/> / scalar may be passed.
/// </summary>
public sealed class Document : NativeObject, IEquatable<Document>
{
    internal Document(IntPtr handle) : base(handle)
    {
    }

    private protected override void Free(IntPtr handle) => NativeMethods.qm_document_free(handle);

    // ── Constructors & statics ──────────────────────────────────────────────

    /// <summary>Parse Markdown into a typed document. Throws on parse errors.</summary>
    public static Document FromMarkdown(string markdown) =>
        new(Interop.CallHandle(NativeMethods.qm_document_from_markdown(Interop.ToUtf8(markdown)),
            "from_markdown"));

    /// <summary>Reconstruct a document from a versioned storage DTO string.
    /// Throws on malformed input.</summary>
    public static Document FromJson(string json) =>
        new(Interop.CallHandle(NativeMethods.qm_document_from_json(Interop.ToUtf8(json)),
            "from_json"));

    /// <summary>Like <see cref="FromJson"/> but returns <c>null</c> instead of
    /// throwing when <paramref name="json"/> is not a storage DTO.</summary>
    public static Document? TryFromJson(string json)
    {
        IntPtr handle = NativeMethods.qm_document_try_from_json(Interop.ToUtf8(json));
        return handle == IntPtr.Zero ? null : new Document(handle);
    }

    /// <summary>The <c>schema</c> version tag of a raw DTO string, or <c>null</c>.</summary>
    public static string? SchemaVersionOf(string json) =>
        Interop.FromJson<string>(
            Interop.TakeString(NativeMethods.qm_document_schema_version_of(Interop.ToUtf8(json)))
                ?? "null");

    /// <summary>The schema version this build writes.</summary>
    public static string CurrentSchemaVersion() =>
        Interop.TakeString(NativeMethods.qm_document_current_schema_version())!;

    /// <summary>Canonical card-yaml authoring rules (static text).</summary>
    public static string FormatRules() =>
        Interop.TakeString(NativeMethods.qm_document_format_rules())!;

    /// <summary>Authoring-ergonomics blueprint header for a quill (static text).</summary>
    public static string BlueprintInstruction(string quillName) =>
        Interop.TakeString(NativeMethods.qm_document_blueprint_instruction(Interop.ToUtf8(quillName)))!;

    /// <summary>The canonical <c>$quill</c> reference grammar (static text).</summary>
    public static string QuillRefHint() =>
        Interop.TakeString(NativeMethods.qm_document_quill_ref_hint())!;

    /// <summary>Build a fresh card from a kind and a flat field map. Mirrors the
    /// Python <c>Document.make_card</c>.</summary>
    public static Card MakeCard(string kind, IReadOnlyDictionary<string, object?>? fields = null,
        string? body = null)
    {
        string? fieldsJson = fields is null ? null : Interop.SerializeValue(fields, "make_card");
        string json = Interop.CallString(
            NativeMethods.qm_document_make_card_json(
                Interop.ToUtf8(kind), Interop.ToUtf8OrNull(fieldsJson), Interop.ToUtf8OrNull(body)),
            "make_card");
        return Card.FromJson(json);
    }

    // ── Lifecycle & readers ─────────────────────────────────────────────────

    /// <summary>Return a fresh document handle with the same parsed state.</summary>
    public Document Clone() =>
        new(Interop.CallHandle(NativeMethods.qm_document_clone(Handle), "clone"));

    /// <summary>Structural equality (parse warnings excluded). A null other is
    /// never equal, mirroring the Python binding's <c>__eq__</c>.</summary>
    public bool Equals(Document? other)
    {
        if (other is null)
        {
            return false;
        }
        if (ReferenceEquals(this, other))
        {
            return true;
        }
        int r = NativeMethods.qm_document_equals(Handle, other.Handle);
        return r switch
        {
            1 => true,
            0 => false,
            _ => throw new QuillmarkException("equals: null handle"),
        };
    }

    public override bool Equals(object? obj) => obj is Document other && Equals(other);

    /// <summary>Consistent with <see cref="Equals(Document)"/>: equal documents
    /// serialize to byte-identical storage JSON, so they hash identically. A
    /// throwing <c>GetHashCode</c> violates the contract (it is called from
    /// collections), so a serialization failure degrades to a constant rather
    /// than propagating.</summary>
    public override int GetHashCode()
    {
        try { return ToJson().GetHashCode(); }
        catch (QuillmarkException) { return 0; }
    }

    /// <summary>Emit canonical Quillmark Markdown. Round-trip safe.</summary>
    public string ToMarkdown() =>
        Interop.CallString(NativeMethods.qm_document_to_markdown(Handle), "to_markdown");

    /// <summary>Serialize to a versioned storage DTO string.</summary>
    public string ToJson() =>
        Interop.CallString(NativeMethods.qm_document_to_json(Handle), "to_json");

    /// <summary>The document's <c>name@version</c> quill reference.</summary>
    public string QuillRef =>
        Interop.CallString(NativeMethods.qm_document_quill_ref(Handle), "quill_ref");

    /// <summary>The main card's global Markdown body.</summary>
    public string Body =>
        Interop.CallString(NativeMethods.qm_document_body(Handle), "body");

    /// <summary>Number of composable cards (excludes the main card).</summary>
    public int CardCount
    {
        get
        {
            long n = (long)NativeMethods.qm_document_card_count(Handle);
            return n < 0 ? throw new QuillmarkException("card_count: null handle") : (int)n;
        }
    }

    /// <summary>Parse-time warnings.</summary>
    public IReadOnlyList<Diagnostic> Warnings
    {
        get
        {
            string json = Interop.CallString(NativeMethods.qm_document_warnings_json(Handle), "warnings");
            return Interop.FromJson<List<Diagnostic>>(json) ?? new List<Diagnostic>();
        }
    }

    /// <summary>The main (entry) card.</summary>
    public Card Main =>
        Card.FromJson(Interop.CallString(NativeMethods.qm_document_main_json(Handle), "main"));

    /// <summary>The ordered list of composable card blocks.</summary>
    public IReadOnlyList<Card> Cards
    {
        get
        {
            string json = Interop.CallString(NativeMethods.qm_document_cards_json(Handle), "cards");
            return Interop.FromJson<List<Card>>(json) ?? new List<Card>();
        }
    }

    // ── Main-card mutators ──────────────────────────────────────────────────

    public void SetField(string name, object? value) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_field(Handle, Interop.ToUtf8(name), ValueJson(value)),
            "set_field");

    public void SetFill(string name, object? value) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_fill(Handle, Interop.ToUtf8(name), ValueJson(value)),
            "set_fill");

    /// <summary>Remove a main-card field, returning the removed value (or
    /// <c>null</c> when absent).</summary>
    public JsonNode? RemoveField(string name)
    {
        string json = Interop.CallString(
            NativeMethods.qm_document_remove_field(Handle, Interop.ToUtf8(name)), "remove_field");
        return JsonNode.Parse(json);
    }

    public void SetQuillRef(string reference) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_quill_ref(Handle, Interop.ToUtf8(reference)), "set_quill_ref");

    public void ReplaceBody(string body) =>
        Interop.CallStatus(
            NativeMethods.qm_document_replace_body(Handle, Interop.ToUtf8(body)), "replace_body");

    public void SetExt(IReadOnlyDictionary<string, object?> ext) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_ext(Handle, ObjectJson(ext, "set_ext")), "set_ext");

    public JsonNode? RemoveExt() =>
        JsonNode.Parse(Interop.CallString(NativeMethods.qm_document_remove_ext(Handle), "remove_ext"));

    public void SetExtNamespace(string ns, object? value) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_ext_namespace(Handle, Interop.ToUtf8(ns), ValueJson(value)),
            "set_ext_namespace");

    public JsonNode? RemoveExtNamespace(string ns) =>
        JsonNode.Parse(Interop.CallString(
            NativeMethods.qm_document_remove_ext_namespace(Handle, Interop.ToUtf8(ns)),
            "remove_ext_namespace"));

    /// <summary>Merge a card-kind's seed <paramref name="overlay"/> into the main
    /// card's <c>$seed</c> map under <paramref name="cardKind"/>, preserving
    /// sibling kinds — the starting values new cards of that kind spawn with.
    /// <c>$seed</c> is root-only, so this targets the main card.</summary>
    public void SetSeedNamespace(string cardKind, object? overlay) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_seed_namespace(Handle, Interop.ToUtf8(cardKind), ValueJson(overlay)),
            "set_seed_namespace");

    /// <summary>Remove <paramref name="cardKind"/> from the main card's
    /// <c>$seed</c> map, returning the overlay stored there (or <c>null</c>).
    /// Emptying the map drops the <c>$seed</c> entry entirely.</summary>
    public JsonNode? RemoveSeedNamespace(string cardKind) =>
        JsonNode.Parse(Interop.CallString(
            NativeMethods.qm_document_remove_seed_namespace(Handle, Interop.ToUtf8(cardKind)),
            "remove_seed_namespace"));

    // ── Composable-card mutators ────────────────────────────────────────────

    public void PushCard(Card card) =>
        Interop.CallStatus(
            NativeMethods.qm_document_push_card(Handle, Interop.ToUtf8(card.ToJson())), "push_card");

    public void InsertCard(int index, Card card) =>
        Interop.CallStatus(
            NativeMethods.qm_document_insert_card(Handle, (UIntPtr)index, Interop.ToUtf8(card.ToJson())),
            "insert_card");

    /// <summary>Remove and return the card at <paramref name="index"/>, or
    /// <c>null</c> when out of range.</summary>
    public Card? RemoveCard(int index)
    {
        string json = Interop.CallString(
            NativeMethods.qm_document_remove_card(Handle, (UIntPtr)index), "remove_card");
        return Interop.FromJson<Card>(json);
    }

    public void MoveCard(int fromIndex, int toIndex) =>
        Interop.CallStatus(
            NativeMethods.qm_document_move_card(Handle, (UIntPtr)fromIndex, (UIntPtr)toIndex),
            "move_card");

    public void SetCardKind(int index, string newKind) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_card_kind(Handle, (UIntPtr)index, Interop.ToUtf8(newKind)),
            "set_card_kind");

    public void UpdateCardField(int index, string name, object? value) =>
        Interop.CallStatus(
            NativeMethods.qm_document_update_card_field(
                Handle, (UIntPtr)index, Interop.ToUtf8(name), ValueJson(value)),
            "update_card_field");

    public JsonNode? RemoveCardField(int index, string name)
    {
        string json = Interop.CallString(
            NativeMethods.qm_document_remove_card_field(Handle, (UIntPtr)index, Interop.ToUtf8(name)),
            "remove_card_field");
        return JsonNode.Parse(json);
    }

    public void UpdateCardBody(int index, string body) =>
        Interop.CallStatus(
            NativeMethods.qm_document_update_card_body(Handle, (UIntPtr)index, Interop.ToUtf8(body)),
            "update_card_body");

    public void SetCardExt(int index, IReadOnlyDictionary<string, object?> ext) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_card_ext(Handle, (UIntPtr)index, ObjectJson(ext, "set_card_ext")),
            "set_card_ext");

    public JsonNode? RemoveCardExt(int index) =>
        JsonNode.Parse(Interop.CallString(
            NativeMethods.qm_document_remove_card_ext(Handle, (UIntPtr)index), "remove_card_ext"));

    public void SetCardExtNamespace(int index, string ns, object? value) =>
        Interop.CallStatus(
            NativeMethods.qm_document_set_card_ext_namespace(
                Handle, (UIntPtr)index, Interop.ToUtf8(ns), ValueJson(value)),
            "set_card_ext_namespace");

    public JsonNode? RemoveCardExtNamespace(int index, string ns) =>
        JsonNode.Parse(Interop.CallString(
            NativeMethods.qm_document_remove_card_ext_namespace(Handle, (UIntPtr)index, Interop.ToUtf8(ns)),
            "remove_card_ext_namespace"));

    // ── Marshaling helpers ──────────────────────────────────────────────────

    private static byte[] ValueJson(object? value) =>
        Interop.ToUtf8(Interop.SerializeValue(value, "value"));

    private static byte[] ObjectJson(IReadOnlyDictionary<string, object?> map, string context) =>
        Interop.ToUtf8(Interop.SerializeValue(map, context));
}
