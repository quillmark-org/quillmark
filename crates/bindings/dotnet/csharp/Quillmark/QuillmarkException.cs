using System;
using System.Collections.Generic;
using System.Linq;

namespace Quillmark;

/// <summary>
/// The single exception type raised for every Quillmark failure — the .NET
/// analogue of the Python binding's <c>QuillmarkError</c> and the WASM
/// binding's thrown error. Always carries a non-empty
/// <see cref="Diagnostics"/> list; read <c>Diagnostics[0]</c> for the primary
/// diagnostic and iterate for backend compilation failures.
/// </summary>
public sealed class QuillmarkException : Exception
{
    /// <summary>Every diagnostic the underlying error carried (never empty).</summary>
    public IReadOnlyList<Diagnostic> Diagnostics { get; }

    public QuillmarkException(string message)
        : this(message, new[] { new Diagnostic { Severity = Severity.Error, Message = message } })
    {
    }

    public QuillmarkException(string message, IEnumerable<Diagnostic> diagnostics)
        : base(message)
    {
        var list = diagnostics.ToList();
        if (list.Count == 0)
        {
            list.Add(new Diagnostic { Severity = Severity.Error, Message = message });
        }
        Diagnostics = list;
    }
}
