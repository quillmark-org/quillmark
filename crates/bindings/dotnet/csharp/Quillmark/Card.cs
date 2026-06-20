using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Quillmark;

/// <summary>
/// One entry in a <see cref="Card.PayloadItems"/> list — a user field or a
/// comment, discriminated by <see cref="Type"/> (<c>"field"</c> /
/// <c>"comment"</c>). Mirrors the core <c>PayloadItemWire</c>; the irrelevant
/// fields for a given <see cref="Type"/> stay null.
/// </summary>
public sealed class PayloadItem
{
    [JsonPropertyName("type")] public string Type { get; set; } = "field";

    // Field entries
    [JsonPropertyName("key")] public string? Key { get; set; }
    [JsonPropertyName("value")] public JsonElement? Value { get; set; }
    [JsonPropertyName("fill")] public bool? Fill { get; set; }

    // Comment entries
    [JsonPropertyName("text")] public string? Text { get; set; }
    [JsonPropertyName("inline")] public bool? Inline { get; set; }
}

/// <summary>
/// The canonical card shape exchanged with the engine — the single value
/// returned by <see cref="Document.Main"/>, <see cref="Document.Cards"/>,
/// <see cref="Document.RemoveCard"/>, and the <c>Quill.Seed*</c> helpers, and
/// accepted by <see cref="Document.PushCard"/> / <see cref="Document.InsertCard"/>.
/// Build a fresh one with <see cref="Document.MakeCard"/>. Mirrors the core
/// <c>CardWire</c> (camelCase JSON).
/// </summary>
public sealed class Card
{
    [JsonPropertyName("kind")] public string Kind { get; set; } = "";
    [JsonPropertyName("quill")] public string? Quill { get; set; }
    [JsonPropertyName("id")] public string? Id { get; set; }
    [JsonPropertyName("ext")] public Dictionary<string, JsonElement>? Ext { get; set; }

    /// <summary>The block's <c>$seed</c> map (keyed by composable card-kind),
    /// present on the main card only. Each entry is the per-kind seed overlay a
    /// newly-added card of that kind starts with; pass one to
    /// <see cref="Quill.SeedCard"/>. <c>null</c> when the card declares no
    /// <c>$seed</c>.</summary>
    [JsonPropertyName("seed")] public Dictionary<string, JsonElement>? Seed { get; set; }
    [JsonPropertyName("payloadItems")] public List<PayloadItem> PayloadItems { get; set; } = new();
    [JsonPropertyName("body")] public string Body { get; set; } = "";

    internal string ToJson() => Interop.SerializeValue(this, "card");

    internal static Card FromJson(string json) =>
        JsonSerializer.Deserialize<Card>(json, Interop.Json)!;
}
