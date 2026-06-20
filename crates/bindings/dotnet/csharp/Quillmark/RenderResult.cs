using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

namespace Quillmark;

/// <summary>
/// The outcome of a render: the produced <see cref="Artifacts"/>, any
/// <see cref="Warnings"/>, the resolved <see cref="Format"/>, and the
/// wall-clock <see cref="RenderTimeMs"/>. The .NET analogue of the Python
/// <c>RenderResult</c>. Artifact bytes are copied out of native memory eagerly,
/// so the result stays usable after <see cref="Dispose"/>; dispose only frees
/// the native handle.
/// </summary>
public sealed class RenderResult : NativeObject
{
    private List<Artifact>? _artifacts;

    internal RenderResult(IntPtr handle) : base(handle)
    {
    }

    private protected override void Free(IntPtr handle) => NativeMethods.qm_render_result_free(handle);

    /// <summary>The resolved output format for this render.</summary>
    public OutputFormat Format =>
        EnumMarshal.ParseFormat(
            Interop.CallString(NativeMethods.qm_render_result_format(Handle), "format"));

    /// <summary>Wall-clock time spent inside <c>render</c>, in milliseconds.</summary>
    public double RenderTimeMs => NativeMethods.qm_render_result_render_time_ms(Handle);

    /// <summary>Render warnings (parse-time warnings lead).</summary>
    public IReadOnlyList<Diagnostic> Warnings
    {
        get
        {
            string json = Interop.CallString(
                NativeMethods.qm_render_result_warnings_json(Handle), "warnings");
            return Interop.FromJson<List<Diagnostic>>(json) ?? new List<Diagnostic>();
        }
    }

    /// <summary>The produced artifacts, materialized once and cached.</summary>
    public IReadOnlyList<Artifact> Artifacts => _artifacts ??= ReadArtifacts();

    private List<Artifact> ReadArtifacts()
    {
        long count = (long)NativeMethods.qm_render_result_artifact_count(Handle);
        if (count < 0)
        {
            throw new QuillmarkException("artifacts: null handle");
        }
        var list = new List<Artifact>((int)count);
        for (int i = 0; i < count; i++)
        {
            var index = (UIntPtr)i;
            string fmt = Interop.CallString(
                NativeMethods.qm_render_result_artifact_format(Handle, index), "artifact_format");
            string mime = Interop.CallString(
                NativeMethods.qm_render_result_artifact_mime(Handle, index), "artifact_mime");
            byte[] bytes = ReadBytes(index);
            list.Add(new Artifact(EnumMarshal.ParseFormat(fmt), mime, bytes));
        }
        return list;
    }

    private byte[] ReadBytes(UIntPtr index)
    {
        NativeMethods.QmBytes buf = NativeMethods.qm_render_result_artifact_bytes(Handle, index);
        try
        {
            int len = checked((int)buf.Len);
            if (len == 0 || buf.Ptr == IntPtr.Zero)
            {
                return Array.Empty<byte>();
            }
            var managed = new byte[len];
            Marshal.Copy(buf.Ptr, managed, 0, len);
            return managed;
        }
        finally
        {
            NativeMethods.qm_free_bytes(buf);
        }
    }
}

/// <summary>
/// A single rendered artifact: its <see cref="Format"/>, <see cref="MimeType"/>,
/// and immutable <see cref="Bytes"/>. The .NET analogue of the Python
/// <c>Artifact</c>.
/// </summary>
public sealed class Artifact
{
    private readonly byte[] _bytes;

    internal Artifact(OutputFormat format, string mimeType, byte[] bytes)
    {
        Format = format;
        MimeType = mimeType;
        _bytes = bytes;
    }

    public OutputFormat Format { get; }
    public string MimeType { get; }

    /// <summary>The raw artifact bytes (a fresh copy on each access).</summary>
    public byte[] Bytes => (byte[])_bytes.Clone();

    /// <summary>Write the artifact bytes to <paramref name="path"/>.</summary>
    public void Save(string path) => File.WriteAllBytes(path, _bytes);
}
