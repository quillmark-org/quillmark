#!/usr/bin/env python3
"""Example demonstrating the quillmark-python Quill render API."""

from pathlib import Path
from quillmark import Quillmark, Document, OutputFormat


def main():
    script_dir = Path(__file__).parent
    repo_root = script_dir.parent.parent.parent.parent
    taro_dir = repo_root / "crates" / "fixtures" / "resources" / "quills" / "taro"

    if not (taro_dir / "Quill.yaml").exists():
        versions = sorted(
            (p.name for p in taro_dir.iterdir() if p.is_dir()),
            key=lambda v: [int(x) for x in v.split(".") if x.isdigit()],
        )
        if versions:
            taro_dir = taro_dir / versions[-1]

    if not taro_dir.exists():
        print(f"Error: Could not find taro quill at {taro_dir}")
        return

    print("=== Quillmark Python API Demo ===\n")

    engine = Quillmark()
    quill = engine.quill_from_path(str(taro_dir))

    markdown = """~~~
$quill: taro
$kind: main
author: Alice
ice_cream: Taro
title: My Favorite Ice Cream
~~~

# Introduction

I love **Taro** ice cream!
"""

    parsed = Document.from_markdown(markdown)

    print(f"Loaded quill: {quill.metadata['name']}")
    print(f"Backend: {quill.backend_id}")
    print(f"Supported formats: {quill.supported_formats}")

    result = quill.render(parsed, OutputFormat.PDF)

    print(f"Generated {len(result.artifacts)} artifact(s) in {result.render_time_ms:.1f} ms")
    for i, artifact in enumerate(result.artifacts):
        output_name = (
            "pdf"
            if artifact.format == OutputFormat.PDF
            else "svg"
            if artifact.format == OutputFormat.SVG
            else "txt"
        )
        output_path = Path(f"/tmp/taro_example_{i}.{output_name}")
        artifact.save(str(output_path))
        print(f"Saved: {output_path} ({len(artifact.bytes):,} bytes)")

    if result.warnings:
        print(f"Warnings ({len(result.warnings)}):")
        for warning in result.warnings:
            print(f"- {warning.severity}: {warning.message}")


if __name__ == "__main__":
    main()
