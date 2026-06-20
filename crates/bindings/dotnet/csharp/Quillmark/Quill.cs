using System;
using System.Collections.Generic;
using System.Text.Json;

namespace Quillmark;

/// <summary>
/// Engine-free, validated quill config data. The declared backend is resolved
/// later, at render time, by a <see cref="QuillmarkEngine"/> engine — never here.
/// The .NET analogue of the Python <c>Quill</c> class.
/// </summary>
public sealed class Quill : NativeObject
{
    private Quill(IntPtr handle) : base(handle)
    {
    }

    private protected override void Free(IntPtr handle) => NativeMethods.qm_quill_free(handle);

    /// <summary>Load a quill from a filesystem directory. Pure config load — no
    /// backend, no engine.</summary>
    public static Quill FromPath(string path)
    {
        IntPtr handle = NativeMethods.qm_quill_from_path(Interop.ToUtf8(path));
        return new Quill(Interop.CallHandle(handle, "Quill.from_path"));
    }

    /// <summary>The declared backend identifier (e.g. <c>"typst"</c>).</summary>
    public string BackendId =>
        Interop.CallString(NativeMethods.qm_quill_backend_id(Handle), "backend_id");

    /// <summary>The quill's <c>name@version</c> reference.</summary>
    public string QuillRef =>
        Interop.CallString(NativeMethods.qm_quill_quill_ref(Handle), "quill_ref");

    /// <summary>Identity snapshot of the <c>quill:</c> section as a JSON object.
    /// A pure config read; never resolves a backend.</summary>
    public JsonElement Metadata =>
        JsonSerializer.Deserialize<JsonElement>(
            Interop.CallString(NativeMethods.qm_quill_metadata_json(Handle), "metadata"));

    /// <summary>The document schema as a structured JSON value.</summary>
    public JsonElement Schema =>
        JsonSerializer.Deserialize<JsonElement>(
            Interop.CallString(NativeMethods.qm_quill_schema_json(Handle), "schema"));

    /// <summary>The auto-generated annotated Markdown blueprint.</summary>
    public string Blueprint =>
        Interop.CallString(NativeMethods.qm_quill_blueprint(Handle), "blueprint");

    /// <summary>Validate <paramref name="document"/> against this quill's schema,
    /// returning every diagnostic (empty when valid).</summary>
    public IReadOnlyList<Diagnostic> Validate(Document document)
    {
        string json = Interop.CallString(
            NativeMethods.qm_quill_validate_json(Handle, document.Handle), "validate");
        return Interop.FromJson<List<Diagnostic>>(json) ?? new List<Diagnostic>();
    }

    /// <summary>Seed a starter <see cref="Document"/> from the schema.</summary>
    public Document SeedDocument() =>
        new(Interop.CallHandle(NativeMethods.qm_quill_seed_document(Handle), "seed_document"));

    /// <summary>Seed a starter main card (carries <c>$quill</c>) from the schema.</summary>
    public Card SeedMain() =>
        Card.FromJson(Interop.CallString(NativeMethods.qm_quill_seed_main_json(Handle), "seed_main"));

    /// <summary>Seed a starter composable card of the given kind, or <c>null</c>
    /// when the kind is not declared.</summary>
    public Card? SeedCard(string cardKind)
    {
        string json = Interop.CallString(
            NativeMethods.qm_quill_seed_card_json(Handle, Interop.ToUtf8(cardKind)), "seed_card");
        return Interop.FromJson<Card>(json);
    }
}
