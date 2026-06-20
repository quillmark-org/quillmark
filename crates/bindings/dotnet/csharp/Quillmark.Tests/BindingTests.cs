using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace Quillmark.Tests;

/// <summary>
/// Smoke tests mirroring the Python binding's suite (test_engine, test_quill,
/// test_render, test_parse, test_versioning, test_validate). They exercise the
/// full FFI round-trip against the real typst backend.
/// </summary>
public class BindingTests
{
    [Fact]
    public void Engine_RegistersTypstBackend()
    {
        using var engine = new Quillmark();
        Assert.Contains("typst", engine.RegisteredBackends());
    }

    [Fact]
    public void Quill_ExposesMetadataAndBackend()
    {
        using var quill = Quill.FromPath(Fixtures.TaroQuill());
        Assert.Equal("typst", quill.BackendId);
        Assert.Equal("taro", quill.Metadata.GetProperty("name").GetString());
        Assert.Contains("@", quill.QuillRef);
    }

    [Fact]
    public void Engine_SupportedFormats_IncludesPdf()
    {
        using var engine = new Quillmark();
        using var quill = Quill.FromPath(Fixtures.TaroQuill());
        Assert.Contains(OutputFormat.Pdf, engine.SupportedFormats(quill));
    }

    [Fact]
    public void Render_ProducesPdfArtifact()
    {
        using var engine = new Quillmark();
        using var quill = Quill.FromPath(Fixtures.TaroQuill());
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);

        using RenderResult result = engine.Render(quill, doc, OutputFormat.Pdf);

        Assert.Equal(OutputFormat.Pdf, result.Format);
        Assert.NotEmpty(result.Artifacts);
        Artifact artifact = result.Artifacts[0];
        Assert.Equal(OutputFormat.Pdf, artifact.Format);
        Assert.Equal("application/pdf", artifact.MimeType);
        // Every PDF begins with the %PDF- signature.
        Assert.True(artifact.Bytes.Length > 4);
        Assert.Equal("%PDF", System.Text.Encoding.ASCII.GetString(artifact.Bytes, 0, 4));
        Assert.True(result.RenderTimeMs >= 0);
    }

    [Fact]
    public void Document_JsonRoundTrip_IsEqual()
    {
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        string json = doc.ToJson();
        using var restored = Document.FromJson(json);
        Assert.True(doc.Equals(restored));
        Assert.Equal(Document.CurrentSchemaVersion(), Document.SchemaVersionOf(json));
    }

    [Fact]
    public void Document_MarkdownRoundTrip_IsEqual()
    {
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        using var reparsed = Document.FromMarkdown(doc.ToMarkdown());
        Assert.True(doc.Equals(reparsed));
    }

    [Fact]
    public void Document_SetField_Mutates()
    {
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        doc.SetField("title", "Changed");
        Assert.Contains("Changed", doc.ToMarkdown());
    }

    [Fact]
    public void Document_MakeAndPushCard_IncrementsCount()
    {
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        int before = doc.CardCount;
        Card card = Document.MakeCard("note", new Dictionary<string, object?> { ["x"] = 1 }, "body");
        Assert.Equal("note", card.Kind);
        doc.PushCard(card);
        Assert.Equal(before + 1, doc.CardCount);
    }

    [Fact]
    public void Quill_Validate_ReturnsList()
    {
        using var quill = Quill.FromPath(Fixtures.TaroQuill());
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        IReadOnlyList<Diagnostic> diags = quill.Validate(doc);
        // A complete document validates clean (or only non-fatal signals).
        Assert.DoesNotContain(diags, d => d.Severity == Severity.Error);
    }

    [Fact]
    public void TryFromJson_ReturnsNull_ForNonDto()
    {
        Assert.Null(Document.TryFromJson("not a dto"));
    }

    [Fact]
    public void ErrorContract_CarriesDiagnostics()
    {
        var ex = Assert.Throws<QuillmarkException>(() => Document.FromJson("{ not valid"));
        Assert.NotEmpty(ex.Diagnostics);
        Assert.Equal(Severity.Error, ex.Diagnostics[0].Severity);
    }

    [Fact]
    public void StaticText_IsNonEmpty()
    {
        Assert.False(string.IsNullOrEmpty(Document.FormatRules()));
        Assert.False(string.IsNullOrEmpty(Document.QuillRefHint()));
        Assert.Contains("taro", Document.BlueprintInstruction("taro"));
    }

    [Fact]
    public void Equals_Null_IsFalse()
    {
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        Assert.False(doc.Equals(null));
        Assert.False(doc.Equals((object?)null));
        // Equal documents hash identically (consistent with Equals).
        using var copy = doc.Clone();
        Assert.True(doc.Equals(copy));
        Assert.Equal(doc.GetHashCode(), copy.GetHashCode());
    }

    [Fact]
    public void SetField_DeepValue_WithinCoreLimit_Accepted()
    {
        // System.Text.Json's default depth (64) must not pre-empt core's limit
        // of 100; a value ~50 deep is valid and should round-trip.
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        doc.SetField("notes", BuildNested(50));
        Assert.Contains("notes", doc.ToMarkdown());
    }

    [Fact]
    public void SetField_DeepValue_BeyondCoreLimit_ThrowsQuillmarkException()
    {
        // A value past core's depth limit must surface as the single binding
        // exception type — never a raw System.Text.Json.JsonException.
        using var doc = Document.FromMarkdown(Fixtures.SampleMarkdown);
        Assert.Throws<QuillmarkException>(() => doc.SetField("notes", BuildNested(200)));
    }

    private static object BuildNested(int depth)
    {
        object value = "x";
        for (int i = 0; i < depth; i++)
        {
            value = new List<object> { value };
        }
        return value;
    }
}
