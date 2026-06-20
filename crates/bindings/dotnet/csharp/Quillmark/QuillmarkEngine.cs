using System;
using System.Collections.Generic;
using System.Linq;

namespace Quillmark;

/// <summary>
/// Render engine: a backend registry and render dispatcher. The .NET analogue
/// of the Python <c>Quillmark</c> class (named <c>QuillmarkEngine</c> here to
/// avoid colliding with the <c>Quillmark</c> namespace). A <see cref="Quill"/>
/// is engine-free config data; the declared backend is resolved here, at render
/// time.
/// </summary>
public sealed class QuillmarkEngine : NativeObject
{
    public QuillmarkEngine() : base(NativeMethods.qm_engine_new())
    {
    }

    private protected override void Free(IntPtr handle) => NativeMethods.qm_engine_free(handle);

    /// <summary>
    /// Render <paramref name="document"/> against <paramref name="quill"/> in
    /// one shot. An unset <paramref name="format"/> falls back to the backend's
    /// first supported format. Throws <see cref="QuillmarkException"/>
    /// (UnsupportedBackend) when the declared backend is not registered.
    /// </summary>
    public RenderResult Render(
        Quill quill,
        Document document,
        OutputFormat? format = null,
        float? ppi = null,
        IEnumerable<int>? pages = null,
        string? producer = null)
    {
        string? optsJson = BuildOptions(format, ppi, pages, producer);
        IntPtr result = NativeMethods.qm_engine_render(
            Handle, quill.Handle, document.Handle, Interop.ToUtf8OrNull(optsJson));
        return new RenderResult(Interop.CallHandle(result, "render"));
    }

    /// <summary>
    /// The output formats <paramref name="quill"/>'s backend can emit. Throws
    /// for an unregistered backend.
    /// </summary>
    public IReadOnlyList<OutputFormat> SupportedFormats(Quill quill)
    {
        string json = Interop.CallString(
            NativeMethods.qm_engine_supported_formats(Handle, quill.Handle), "supported_formats");
        var names = Interop.FromJson<List<string>>(json) ?? new List<string>();
        return names.Select(EnumMarshal.ParseFormat).ToList();
    }

    /// <summary>The ids of the backends registered on this engine (e.g. <c>["typst"]</c>).</summary>
    public IReadOnlyList<string> RegisteredBackends()
    {
        string json = Interop.TakeString(NativeMethods.qm_engine_registered_backends(Handle)) ?? "[]";
        return Interop.FromJson<List<string>>(json) ?? new List<string>();
    }

    private static string? BuildOptions(
        OutputFormat? format, float? ppi, IEnumerable<int>? pages, string? producer)
    {
        if (format is null && ppi is null && pages is null && producer is null)
        {
            return null;
        }
        var obj = new Dictionary<string, object>();
        if (format is OutputFormat f)
        {
            obj["format"] = EnumMarshal.ToWire(f);
        }
        if (ppi is float p)
        {
            obj["ppi"] = p;
        }
        if (pages is not null)
        {
            obj["pages"] = pages.ToList();
        }
        if (producer is not null)
        {
            obj["producer"] = producer;
        }
        return System.Text.Json.JsonSerializer.Serialize(obj, Interop.Json);
    }
}
