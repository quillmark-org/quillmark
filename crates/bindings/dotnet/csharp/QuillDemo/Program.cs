using System;
using System.IO;
using Quillmark;

// Mirrors crates/bindings/python/examples/quill_demo.py.

// Walk up from the executable to the repository root (the directory holding
// Cargo.toml), then locate the taro fixture quill.
string repoRoot = FindRepoRoot(AppContext.BaseDirectory);
string taroDir = Path.Combine(
    repoRoot, "crates", "fixtures", "resources", "quills", "taro");

static string FindRepoRoot(string start)
{
    var dir = new DirectoryInfo(start);
    while (dir is not null)
    {
        if (File.Exists(Path.Combine(dir.FullName, "Cargo.toml")) &&
            Directory.Exists(Path.Combine(dir.FullName, "crates")))
        {
            return dir.FullName;
        }
        dir = dir.Parent;
    }
    throw new DirectoryNotFoundException("Could not locate repository root from " + start);
}

if (!File.Exists(Path.Combine(taroDir, "Quill.yaml")))
{
    // Resolve a versioned subdirectory if the quill is versioned on disk.
    if (Directory.Exists(taroDir))
    {
        string[] versions = Directory.GetDirectories(taroDir);
        if (versions.Length > 0)
        {
            Array.Sort(versions, StringComparer.Ordinal);
            taroDir = versions[^1];
        }
    }
}

if (!Directory.Exists(taroDir))
{
    Console.Error.WriteLine($"Error: Could not find taro quill at {taroDir}");
    return 1;
}

Console.WriteLine("=== Quillmark .NET API Demo ===\n");

using var engine = new QuillmarkEngine();
using var quill = Quill.FromPath(taroDir);

const string markdown = """
~~~
$quill: taro
$kind: main
author: Alice
ice_cream: Taro
title: My Favorite Ice Cream
~~~

# Introduction

I love **Taro** ice cream!
""";

using var parsed = Document.FromMarkdown(markdown);

Console.WriteLine($"Loaded quill: {quill.Metadata.GetProperty("name").GetString()}");
Console.WriteLine($"Backend: {quill.BackendId}");
Console.WriteLine($"Supported formats: {string.Join(", ", engine.SupportedFormats(quill))}");

using RenderResult result = engine.Render(quill, parsed, OutputFormat.Pdf);

Console.WriteLine(
    $"Generated {result.Artifacts.Count} artifact(s) in {result.RenderTimeMs:F1} ms");
for (int i = 0; i < result.Artifacts.Count; i++)
{
    Artifact artifact = result.Artifacts[i];
    string ext = artifact.Format.ToString().ToLowerInvariant();
    string outputPath = Path.Combine(Path.GetTempPath(), $"taro_example_{i}.{ext}");
    artifact.Save(outputPath);
    Console.WriteLine($"Saved: {outputPath} ({artifact.Bytes.Length:N0} bytes)");
}

if (result.Warnings.Count > 0)
{
    Console.WriteLine($"Warnings ({result.Warnings.Count}):");
    foreach (Diagnostic warning in result.Warnings)
    {
        Console.WriteLine($"- {warning.Severity}: {warning.Message}");
    }
}

return 0;
