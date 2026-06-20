using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace Quillmark;

/// <summary>
/// Boundary helpers shared by the typed wrappers: UTF-8 marshaling, ownership
/// transfer of native strings, and the single-exception error contract.
/// </summary>
internal static class Interop
{
    internal static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
        // Don't let System.Text.Json's default depth (64) reject values the core
        // accepts — core's nesting limit is 100 (MAX_YAML_DEPTH) and is the real
        // boundary, enforced Rust-side with a proper diagnostic. A generous cap
        // here keeps that the single source of truth while still guarding against
        // pathologically deep input (which surfaces as a QuillmarkException via
        // SerializeValue, not a raw JsonException).
        MaxDepth = 256,
    };

    /// <summary>
    /// Serialize a user-supplied value, translating a serializer failure (a graph
    /// deeper than <see cref="JsonSerializerOptions.MaxDepth"/>, or a cycle) into a
    /// <see cref="QuillmarkException"/> so callers only ever see the single binding
    /// exception type — never a raw <c>System.Text.Json.JsonException</c>.
    /// </summary>
    internal static string SerializeValue(object? value, string context)
    {
        try
        {
            return JsonSerializer.Serialize(value, Json);
        }
        catch (JsonException ex)
        {
            throw new QuillmarkException($"{context}: {ex.Message}");
        }
    }

    /// <summary>NUL-terminated UTF-8 bytes for passing a string into the ABI.</summary>
    internal static byte[] ToUtf8(string s)
    {
        int len = Encoding.UTF8.GetByteCount(s);
        var bytes = new byte[len + 1];
        Encoding.UTF8.GetBytes(s, 0, s.Length, bytes, 0);
        bytes[len] = 0;
        return bytes;
    }

    internal static byte[]? ToUtf8OrNull(string? s) => s is null ? null : ToUtf8(s);

    /// <summary>
    /// Take ownership of a native string: decode it and free the native
    /// allocation. Null pointer becomes <c>null</c>.
    /// </summary>
    internal static string? TakeString(IntPtr ptr)
    {
        if (ptr == IntPtr.Zero)
        {
            return null;
        }
        try
        {
            return Marshal.PtrToStringUTF8(ptr);
        }
        finally
        {
            NativeMethods.qm_free_string(ptr);
        }
    }

    /// <summary>
    /// Drain the pending native error and throw it as a
    /// <see cref="QuillmarkException"/>. Falls back to a generic message when
    /// no structured error was parked (should not happen for fallible calls).
    /// </summary>
    internal static QuillmarkException TakeError(string fallback)
    {
        string? json = TakeString(NativeMethods.qm_last_error_take());
        if (json is null)
        {
            return new QuillmarkException(fallback,
                new[] { new Diagnostic { Severity = Severity.Error, Message = fallback } });
        }
        try
        {
            var payload = JsonSerializer.Deserialize<ErrorPayload>(json, Json);
            var diags = payload?.Diagnostics ?? new List<Diagnostic>();
            string message = payload?.Message ?? fallback;
            if (diags.Count == 0)
            {
                diags.Add(new Diagnostic { Severity = Severity.Error, Message = message });
            }
            return new QuillmarkException(message, diags);
        }
        catch (JsonException)
        {
            return new QuillmarkException(json,
                new[] { new Diagnostic { Severity = Severity.Error, Message = json } });
        }
    }

    /// <summary>String-returning fallible call: null pointer ⇒ throw.</summary>
    internal static string CallString(IntPtr ptr, string context)
    {
        string? s = TakeString(ptr);
        if (s is null)
        {
            throw TakeError(context);
        }
        return s;
    }

    /// <summary>Status-returning fallible call: non-zero ⇒ throw.</summary>
    internal static void CallStatus(int status, string context)
    {
        if (status != 0)
        {
            throw TakeError(context);
        }
    }

    /// <summary>Handle-returning fallible call: null handle ⇒ throw.</summary>
    internal static IntPtr CallHandle(IntPtr handle, string context)
    {
        if (handle == IntPtr.Zero)
        {
            throw TakeError(context);
        }
        return handle;
    }

    /// <summary>Deserialize a JSON value that may legitimately be <c>null</c>.</summary>
    internal static T? FromJson<T>(string json) => JsonSerializer.Deserialize<T>(json, Json);

    private sealed class ErrorPayload
    {
        public string? Message { get; set; }
        public List<Diagnostic>? Diagnostics { get; set; }
    }
}
