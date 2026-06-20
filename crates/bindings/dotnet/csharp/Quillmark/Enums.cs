namespace Quillmark;

/// <summary>Output artifact formats a backend can emit. Mirrors the Python
/// <c>OutputFormat</c> enum and the core <c>OutputFormat</c>.</summary>
public enum OutputFormat
{
    Pdf,
    Svg,
    Txt,
    Png,
}

/// <summary>Diagnostic severity. Mirrors the Python <c>Severity</c> enum.</summary>
public enum Severity
{
    Error,
    Warning,
    Note,
}

internal static class EnumMarshal
{
    /// <summary>Parse a lowercase format string from the ABI.</summary>
    internal static OutputFormat ParseFormat(string s) => s switch
    {
        "pdf" => OutputFormat.Pdf,
        "svg" => OutputFormat.Svg,
        "txt" => OutputFormat.Txt,
        "png" => OutputFormat.Png,
        _ => throw new QuillmarkException($"unknown output format '{s}'"),
    };

    internal static string ToWire(OutputFormat f) => f switch
    {
        OutputFormat.Pdf => "pdf",
        OutputFormat.Svg => "svg",
        OutputFormat.Txt => "txt",
        OutputFormat.Png => "png",
        _ => "pdf",
    };
}
