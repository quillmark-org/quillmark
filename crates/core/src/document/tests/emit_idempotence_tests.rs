//! Canonical-form convergence content tests
//!
//! For every fixture, the markdown and JSON persistence paths must
//! canonicalise to the same in-memory document. This is the non-trivial
//! consequence of byte-stable, lossless round-trips through both formats.

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect all `.md` files reachable from `root`, walking recursively.
fn collect_md_files(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

// ── Markdown↔JSON canonical convergence (markdown-spec.md §9.1) ──────────────

/// `to_markdown(from_json(to_json(from_markdown(x)))) == to_markdown(from_markdown(x))`
/// for every fixture: the markdown and JSON persistence paths canonicalise
/// to the same in-memory document.
#[test]
fn markdown_and_json_converge_on_canonical_form() {
    use crate::document::Document;

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir has parent")
        .parent()
        .expect("crates dir has parent");
    let fixtures_root = workspace_root.join("crates/fixtures/resources");

    let mut all_md = Vec::new();
    collect_md_files(&fixtures_root, &mut all_md);

    let mut passed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for path in &all_md {
        let label = path
            .strip_prefix(workspace_root)
            .unwrap_or(path)
            .display()
            .to_string();

        let Ok(src) = std::fs::read_to_string(path) else {
            skipped += 1;
            continue;
        };
        let Ok(doc) = Document::parse(&src).map(|p| p.document) else {
            skipped += 1;
            continue;
        };

        let md_canonical = doc.to_markdown();

        // Round through the versioned JSON DTO.
        let json = serde_json::to_string(&doc).expect("to_json should succeed");
        let restored: Document = serde_json::from_str(&json).expect("from_json should round-trip");
        let md_after_json_round = restored.to_markdown();

        if md_canonical == md_after_json_round {
            passed += 1;
        } else {
            failures.push(format!(
                "FAIL {}: markdown/JSON canonical forms diverge\nMarkdown direct:    {:.400}\nThrough JSON DTO:   {:.400}",
                label, md_canonical, md_after_json_round
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "Canonical-convergence failures ({} failed, {} passed, {} skipped):\n{}",
            failures.len(),
            passed,
            skipped,
            failures.join("\n\n")
        );
    }

    assert!(passed > 0, "no fixtures passed convergence check");
    eprintln!(
        "markdown_and_json_converge_on_canonical_form: {} passed, {} skipped",
        passed, skipped
    );
}
