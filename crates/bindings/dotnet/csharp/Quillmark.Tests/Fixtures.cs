using System;
using System.IO;

namespace Quillmark.Tests;

/// <summary>Shared fixture-path resolution, mirroring the Python conftest.</summary>
internal static class Fixtures
{
    /// <summary>Absolute path to the taro quill directory, resolving a versioned
    /// subdirectory when present.</summary>
    public static string TaroQuill()
    {
        string root = RepoRoot();
        string taro = Path.Combine(root, "crates", "fixtures", "resources", "quills", "taro");
        if (!File.Exists(Path.Combine(taro, "Quill.yaml")) && Directory.Exists(taro))
        {
            string[] versions = Directory.GetDirectories(taro);
            if (versions.Length > 0)
            {
                Array.Sort(versions, StringComparer.Ordinal);
                taro = versions[^1];
            }
        }
        if (!Directory.Exists(taro))
        {
            throw new DirectoryNotFoundException($"taro quill not found at {taro}");
        }
        return taro;
    }

    /// <summary>Walk up from the test assembly to the repository root (the
    /// directory containing <c>Cargo.toml</c>).</summary>
    public static string RepoRoot()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "Cargo.toml")) &&
                Directory.Exists(Path.Combine(dir.FullName, "crates")))
            {
                return dir.FullName;
            }
            dir = dir.Parent;
        }
        throw new DirectoryNotFoundException("Could not locate repository root from " + AppContext.BaseDirectory);
    }

    public const string SampleMarkdown = """
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
}
