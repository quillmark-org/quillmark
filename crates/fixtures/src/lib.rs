use std::path::{Path, PathBuf};

/// Get the path to a resource file in the fixtures
pub fn resource_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("resources").join(name)
}

/// Get the path to a quill fixture in `resources/quills/`.
///
/// If the quill directory contains versioned subdirectories (e.g. `0.1.0/`),
/// the latest version directory is returned automatically.
pub fn quills_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let quill_dir = Path::new(manifest_dir)
        .join("resources")
        .join("quills")
        .join(name);

    // If Quill.yaml lives directly in the directory, return as-is.
    if quill_dir.join("Quill.yaml").exists() {
        return quill_dir;
    }

    // Otherwise look for versioned subdirectories and pick the latest.
    if let Ok(entries) = std::fs::read_dir(&quill_dir) {
        let mut versions: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        versions.sort_by(|a, b| {
            let parse =
                |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
            parse(a).cmp(&parse(b))
        });
        if let Some(latest) = versions.last() {
            return quill_dir.join(latest);
        }
    }

    quill_dir
}

/// Get the example output directory path
pub fn example_output_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("output")
}

/// Write example output to the examples directory
pub fn write_example_output(name: &str, content: &[u8]) -> Result<(), std::io::Error> {
    use std::fs;

    let output_dir = example_output_dir();
    fs::create_dir_all(&output_dir)?;

    let output_path = output_dir.join(name);
    fs::write(output_path, content)?;

    Ok(())
}
