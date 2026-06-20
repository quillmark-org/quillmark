using System;
using System.Runtime.InteropServices;

namespace Quillmark;

/// <summary>
/// Raw P/Invoke declarations for the <c>quillmark_dotnet</c> C ABI. One-to-one
/// with the <c>qm_*</c> entry points in <c>crates/bindings/dotnet/src/lib.rs</c>.
/// Everything here is internal; callers use the typed wrappers
/// (<see cref="Quillmark"/>, <see cref="Quill"/>, <see cref="Document"/>, …).
/// </summary>
internal static class NativeMethods
{
    // The base library name. .NET's NativeLibrary resolver maps this to
    // libquillmark_dotnet.so / .dylib / quillmark_dotnet.dll per platform.
    internal const string Lib = "quillmark_dotnet";

    // ── Owned byte buffer returned by artifact reads ────────────────────────
    [StructLayout(LayoutKind.Sequential)]
    internal struct QmBytes
    {
        public IntPtr Ptr;
        public UIntPtr Len;
    }

    // ── Marshaling primitives ───────────────────────────────────────────────
    [DllImport(Lib)] internal static extern IntPtr qm_last_error_take();
    [DllImport(Lib)] internal static extern void qm_free_string(IntPtr ptr);
    [DllImport(Lib)] internal static extern void qm_free_bytes(QmBytes bytes);

    // ── Engine ──────────────────────────────────────────────────────────────
    [DllImport(Lib)] internal static extern IntPtr qm_engine_new();
    [DllImport(Lib)] internal static extern void qm_engine_free(IntPtr engine);
    [DllImport(Lib)] internal static extern IntPtr qm_engine_render(IntPtr engine, IntPtr quill, IntPtr doc, byte[]? optsJson);
    [DllImport(Lib)] internal static extern IntPtr qm_engine_supported_formats(IntPtr engine, IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_engine_registered_backends(IntPtr engine);

    // ── Quill ───────────────────────────────────────────────────────────────
    [DllImport(Lib)] internal static extern IntPtr qm_quill_from_path(byte[] path);
    [DllImport(Lib)] internal static extern void qm_quill_free(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_backend_id(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_quill_ref(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_metadata_json(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_schema_json(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_blueprint(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_validate_json(IntPtr quill, IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_seed_document(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_seed_main_json(IntPtr quill);
    [DllImport(Lib)] internal static extern IntPtr qm_quill_seed_card_json(IntPtr quill, byte[] kind, byte[]? overlayJson);

    // ── Document: constructors & statics ────────────────────────────────────
    [DllImport(Lib)] internal static extern IntPtr qm_document_from_markdown(byte[] markdown);
    [DllImport(Lib)] internal static extern IntPtr qm_document_from_json(byte[] json);
    [DllImport(Lib)] internal static extern IntPtr qm_document_try_from_json(byte[] json);
    [DllImport(Lib)] internal static extern IntPtr qm_document_schema_version_of(byte[] json);
    [DllImport(Lib)] internal static extern IntPtr qm_document_current_schema_version();
    [DllImport(Lib)] internal static extern IntPtr qm_document_format_rules();
    [DllImport(Lib)] internal static extern IntPtr qm_document_blueprint_instruction(byte[] quillName);
    [DllImport(Lib)] internal static extern IntPtr qm_document_quill_ref_hint();
    [DllImport(Lib)] internal static extern IntPtr qm_document_make_card_json(byte[] kind, byte[]? fieldsJson, byte[]? body);

    // ── Document: lifecycle & readers ───────────────────────────────────────
    [DllImport(Lib)] internal static extern void qm_document_free(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_clone(IntPtr doc);
    [DllImport(Lib)] internal static extern int qm_document_equals(IntPtr a, IntPtr b);
    [DllImport(Lib)] internal static extern IntPtr qm_document_to_markdown(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_to_json(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_quill_ref(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_body(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_card_count(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_warnings_json(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_main_json(IntPtr doc);
    [DllImport(Lib)] internal static extern IntPtr qm_document_cards_json(IntPtr doc);

    // ── Document: main-card mutators ────────────────────────────────────────
    [DllImport(Lib)] internal static extern int qm_document_set_field(IntPtr doc, byte[] name, byte[] valueJson);
    [DllImport(Lib)] internal static extern int qm_document_set_fill(IntPtr doc, byte[] name, byte[] valueJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_field(IntPtr doc, byte[] name);
    [DllImport(Lib)] internal static extern int qm_document_set_quill_ref(IntPtr doc, byte[] refStr);
    [DllImport(Lib)] internal static extern int qm_document_replace_body(IntPtr doc, byte[] body);
    [DllImport(Lib)] internal static extern int qm_document_set_ext(IntPtr doc, byte[] valueJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_ext(IntPtr doc);
    [DllImport(Lib)] internal static extern int qm_document_set_ext_namespace(IntPtr doc, byte[] ns, byte[] valueJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_ext_namespace(IntPtr doc, byte[] ns);
    [DllImport(Lib)] internal static extern int qm_document_set_seed_namespace(IntPtr doc, byte[] cardKind, byte[] overlayJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_seed_namespace(IntPtr doc, byte[] cardKind);

    // ── Document: composable-card mutators ──────────────────────────────────
    [DllImport(Lib)] internal static extern int qm_document_push_card(IntPtr doc, byte[] cardJson);
    [DllImport(Lib)] internal static extern int qm_document_insert_card(IntPtr doc, UIntPtr index, byte[] cardJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_card(IntPtr doc, UIntPtr index);
    [DllImport(Lib)] internal static extern int qm_document_move_card(IntPtr doc, UIntPtr fromIdx, UIntPtr toIdx);
    [DllImport(Lib)] internal static extern int qm_document_set_card_kind(IntPtr doc, UIntPtr index, byte[] newKind);
    [DllImport(Lib)] internal static extern int qm_document_update_card_field(IntPtr doc, UIntPtr index, byte[] name, byte[] valueJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_card_field(IntPtr doc, UIntPtr index, byte[] name);
    [DllImport(Lib)] internal static extern int qm_document_update_card_body(IntPtr doc, UIntPtr index, byte[] body);
    [DllImport(Lib)] internal static extern int qm_document_set_card_ext(IntPtr doc, UIntPtr index, byte[] valueJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_card_ext(IntPtr doc, UIntPtr index);
    [DllImport(Lib)] internal static extern int qm_document_set_card_ext_namespace(IntPtr doc, UIntPtr index, byte[] ns, byte[] valueJson);
    [DllImport(Lib)] internal static extern IntPtr qm_document_remove_card_ext_namespace(IntPtr doc, UIntPtr index, byte[] ns);

    // ── RenderResult / Artifact ─────────────────────────────────────────────
    [DllImport(Lib)] internal static extern void qm_render_result_free(IntPtr result);
    [DllImport(Lib)] internal static extern IntPtr qm_render_result_format(IntPtr result);
    [DllImport(Lib)] internal static extern double qm_render_result_render_time_ms(IntPtr result);
    [DllImport(Lib)] internal static extern IntPtr qm_render_result_warnings_json(IntPtr result);
    [DllImport(Lib)] internal static extern IntPtr qm_render_result_artifact_count(IntPtr result);
    [DllImport(Lib)] internal static extern IntPtr qm_render_result_artifact_format(IntPtr result, UIntPtr index);
    [DllImport(Lib)] internal static extern IntPtr qm_render_result_artifact_mime(IntPtr result, UIntPtr index);
    [DllImport(Lib)] internal static extern QmBytes qm_render_result_artifact_bytes(IntPtr result, UIntPtr index);
}
