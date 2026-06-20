using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace Quillmark;

/// <summary>
/// A text anchor (file/line/column) on a <see cref="Diagnostic"/>. Mirrors the
/// core <c>Location</c> (camelCase JSON).
/// </summary>
public sealed class Location
{
    [JsonPropertyName("file")] public string File { get; set; } = "";
    [JsonPropertyName("line")] public int Line { get; set; }
    [JsonPropertyName("column")] public int Column { get; set; }
}

/// <summary>
/// A structured diagnostic, deserialized from the same <c>serde</c> JSON the
/// Python and WASM bindings surface. Mirrors the Python <c>Diagnostic</c>:
/// <see cref="Severity"/>, optional <see cref="Code"/>, <see cref="Message"/>,
/// <see cref="Location"/>, <see cref="Path"/>, <see cref="Hint"/>, and the
/// flattened <see cref="SourceChain"/>.
/// </summary>
public sealed class Diagnostic
{
    [JsonPropertyName("severity")]
    [JsonConverter(typeof(JsonStringEnumConverter<Severity>))]
    public Severity Severity { get; set; }

    [JsonPropertyName("code")] public string? Code { get; set; }
    [JsonPropertyName("message")] public string Message { get; set; } = "";
    [JsonPropertyName("location")] public Location? Location { get; set; }
    [JsonPropertyName("path")] public string? Path { get; set; }
    [JsonPropertyName("hint")] public string? Hint { get; set; }
    [JsonPropertyName("sourceChain")] public List<string> SourceChain { get; set; } = new();

    public override string ToString()
    {
        string prefix = Severity switch
        {
            Severity.Error => "error",
            Severity.Warning => "warning",
            _ => "note",
        };
        string code = Code is null ? "" : $"[{Code}] ";
        string loc = Location is null ? "" : $" ({Location.File}:{Location.Line}:{Location.Column})";
        return $"{prefix}: {code}{Message}{loc}";
    }
}
