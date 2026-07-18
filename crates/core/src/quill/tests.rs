//! Tests for quill types and loading.

use super::*;
use crate::{Diagnostic, Severity};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Test helper: recursively load a directory as a FileTreeNode.
fn load_tree(path: &Path) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    let default_ignore = QuillIgnore::new(vec![
        ".git/".to_string(),
        ".gitignore".to_string(),
        ".quillignore".to_string(),
        "target/".to_string(),
        "node_modules/".to_string(),
    ]);
    let quillignore_path = path.join(".quillignore");
    let ignore = if quillignore_path.exists() {
        let content = fs::read_to_string(&quillignore_path)?;
        QuillIgnore::from_content(&content)
    } else {
        default_ignore
    };
    load_dir(path, path, &ignore)
}

fn load_dir(
    current: &Path,
    base: &Path,
    ignore: &QuillIgnore,
) -> Result<FileTreeNode, Box<dyn StdError + Send + Sync>> {
    if !current.exists() {
        return Ok(FileTreeNode::Directory {
            files: HashMap::new(),
        });
    }
    let mut files = HashMap::new();
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let p = entry.path();
        let rel = p.strip_prefix(base)?;
        if ignore.is_ignored(rel) {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid filename")?
            .to_string();
        if p.is_file() {
            files.insert(
                name,
                FileTreeNode::File {
                    contents: fs::read(&p)?,
                },
            );
        } else if p.is_dir() {
            files.insert(name, load_dir(&p, base, ignore)?);
        }
    }
    Ok(FileTreeNode::Directory { files })
}

/// Test helper: loads a `Quill` from a filesystem path via `Quill::from_tree`
/// — core is filesystem-agnostic, so production filesystem loading lives in
/// `quillmark::quill_from_path` instead.
fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Quill, Box<dyn StdError + Send + Sync>> {
    let tree = load_tree(path.as_ref())?;
    Quill::from_tree(tree).map_err(|diags| {
        diags
            .iter()
            .map(|d| d.fmt_pretty())
            .collect::<Vec<_>>()
            .join("\n")
            .into()
    })
}

#[test]
fn test_quillignore_parsing() {
    let ignore_content = r#"
# This is a comment
*.tmp
target/
node_modules/
.git/
"#;
    let ignore = QuillIgnore::from_content(ignore_content);
    assert_eq!(ignore.patterns.len(), 4);
    assert!(ignore.patterns.contains(&"*.tmp".to_string()));
    assert!(ignore.patterns.contains(&"target/".to_string()));
}

#[test]
fn test_quillignore_matching() {
    let ignore = QuillIgnore::new(vec![
        "*.tmp".to_string(),
        "target/".to_string(),
        "node_modules/".to_string(),
        ".git/".to_string(),
    ]);

    // Test file patterns
    assert!(ignore.is_ignored("test.tmp"));
    assert!(ignore.is_ignored("path/to/file.tmp"));
    assert!(!ignore.is_ignored("test.txt"));

    // Test directory patterns
    assert!(ignore.is_ignored("target"));
    assert!(ignore.is_ignored("target/debug"));
    assert!(ignore.is_ignored("target/debug/deps"));
    assert!(!ignore.is_ignored("src/target.rs"));

    assert!(ignore.is_ignored("node_modules"));
    assert!(ignore.is_ignored("node_modules/package"));
    assert!(!ignore.is_ignored("my_node_modules"));
}

#[test]
fn test_in_memory_file_system() {
    let temp_dir = TempDir::new().unwrap();
    let quill_dir = temp_dir.path();

    // Create test files
    fs::write(
            quill_dir.join("Quill.yaml"),
            "quill:\n  name: \"test\"\n  version: \"1.0\"\n  backend: \"typst\"\n  description: \"Test quill\"",
        )
        .unwrap();
    fs::write(quill_dir.join("plate.typ"), "test plate").unwrap();

    let assets_dir = quill_dir.join("assets");
    fs::create_dir_all(&assets_dir).unwrap();
    fs::write(assets_dir.join("test.txt"), "asset content").unwrap();

    let packages_dir = quill_dir.join("packages");
    fs::create_dir_all(&packages_dir).unwrap();
    fs::write(packages_dir.join("package.typ"), "package content").unwrap();

    // Load quill
    let quill = load_from_path(quill_dir).unwrap();

    // Test file access
    assert!(quill.file_exists("plate.typ"));
    assert!(quill.file_exists("assets/test.txt"));
    assert!(quill.file_exists("packages/package.typ"));
    assert!(!quill.file_exists("nonexistent.txt"));

    // Test file content
    let asset_content = quill.get_file("assets/test.txt").unwrap();
    assert_eq!(asset_content, b"asset content");
}

#[test]
fn test_quillignore_integration() {
    let temp_dir = TempDir::new().unwrap();
    let quill_dir = temp_dir.path();

    // Create .quillignore
    fs::write(quill_dir.join(".quillignore"), "*.tmp\ntarget/\n").unwrap();

    // Create test files
    fs::write(
            quill_dir.join("Quill.yaml"),
            "quill:\n  name: \"test\"\n  version: \"1.0\"\n  backend: \"typst\"\n  description: \"Test quill\"",
        )
        .unwrap();
    fs::write(quill_dir.join("plate.typ"), "test template").unwrap();
    fs::write(quill_dir.join("should_ignore.tmp"), "ignored").unwrap();

    let target_dir = quill_dir.join("target");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("debug.txt"), "also ignored").unwrap();

    // Load quill
    let quill = load_from_path(quill_dir).unwrap();

    // Test that ignored files are not loaded
    assert!(quill.file_exists("plate.typ"));
    assert!(!quill.file_exists("should_ignore.tmp"));
    assert!(!quill.file_exists("target/debug.txt"));
}

#[test]
fn test_find_files_pattern() {
    let temp_dir = TempDir::new().unwrap();
    let quill_dir = temp_dir.path();

    // Create test directory structure
    fs::write(
            quill_dir.join("Quill.yaml"),
            "quill:\n  name: \"test\"\n  version: \"1.0\"\n  backend: \"typst\"\n  description: \"Test quill\"",
        )
        .unwrap();
    fs::write(quill_dir.join("plate.typ"), "template").unwrap();

    let assets_dir = quill_dir.join("assets");
    fs::create_dir_all(&assets_dir).unwrap();
    fs::write(assets_dir.join("image.png"), "png data").unwrap();
    fs::write(assets_dir.join("data.json"), "json data").unwrap();

    let fonts_dir = assets_dir.join("fonts");
    fs::create_dir_all(&fonts_dir).unwrap();
    fs::write(fonts_dir.join("font.ttf"), "font data").unwrap();

    // Load quill
    let quill = load_from_path(quill_dir).unwrap();

    // Test pattern matching
    let all_assets = quill.find_files("assets/*");
    assert!(all_assets.len() >= 3); // At least image.png, data.json, fonts/font.ttf

    let typ_files = quill.find_files("*.typ");
    assert_eq!(typ_files.len(), 1);
    assert!(typ_files.contains(&PathBuf::from("plate.typ")));
}

#[test]
fn test_new_standardized_yaml_format() {
    let temp_dir = TempDir::new().unwrap();
    let quill_dir = temp_dir.path();

    // Create test files using new standardized format
    let yaml_content = r#"
quill:
  name: my_custom_quill
  version: "1.0"
  backend: typst
  description: Test quill with new format
  author: Test Author
"#;
    fs::write(quill_dir.join("Quill.yaml"), yaml_content).unwrap();
    fs::write(
        quill_dir.join("custom_plate.typ"),
        "= Custom Template\n\nThis is a custom template.",
    )
    .unwrap();

    // Load quill
    let quill = load_from_path(quill_dir).unwrap();

    // Test that name comes from YAML, not directory
    assert_eq!(quill.name(), "my_custom_quill");

    // Test that backend is in metadata
    assert!(quill.metadata.contains_key("backend"));
    if let Some(backend_val) = quill.metadata.get("backend") {
        if let Some(backend_str) = backend_val.as_str() {
            assert_eq!(backend_str, "typst");
        } else {
            panic!("Backend value is not a string");
        }
    }

    // Test that other fields are in metadata including version
    assert!(quill.metadata.contains_key("description"));
    assert!(quill.metadata.contains_key("author"));
    assert!(quill.metadata.contains_key("version")); // version should now be included
    if let Some(version_val) = quill.metadata.get("version") {
        if let Some(version_str) = version_val.as_str() {
            assert_eq!(version_str, "1.0");
        }
    }
}

#[test]
fn test_from_tree() {
    // Create a simple in-memory file tree
    let mut root_files = HashMap::new();

    // Add Quill.yaml
    let quill_yaml = r#"quill:
  name: "test_from_tree"
  version: "1.0"
  backend: "typst"
  description: "A test quill from tree"
"#;
    root_files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: quill_yaml.as_bytes().to_vec(),
        },
    );

    // Add plate file
    let plate_content = "= Test Template\n\nThis is a test.";
    root_files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate_content.as_bytes().to_vec(),
        },
    );

    let root = FileTreeNode::Directory { files: root_files };

    // Create Quill from tree
    let quill = Quill::from_tree(root).unwrap();

    // Validate the quill
    assert_eq!(quill.name(), "test_from_tree");
    // Non-manifest files (e.g. a backend's template) round-trip into the file
    // tree verbatim; core does not read any of them at load time.
    assert_eq!(
        quill.files().get_file("plate.typ"),
        Some(plate_content.as_bytes())
    );
    assert!(quill.metadata.contains_key("backend"));
    assert!(quill.metadata.contains_key("description"));
}

#[test]
fn test_to_tree_round_trips_from_tree() {
    // Build a tree with a nested directory to exercise the recursive flatten.
    let quill_yaml = b"quill:\n  name: roundtrip\n  version: \"1.0\"\n  backend: typst\n  description: Round-trip test\n".to_vec();
    let plate = b"= Plate".to_vec();
    let asset = b"\x00\x01\x02 binary asset".to_vec();

    let mut root_files = HashMap::new();
    root_files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: quill_yaml.clone(),
        },
    );
    root_files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate.clone(),
        },
    );
    let mut assets = HashMap::new();
    assets.insert(
        "logo.bin".to_string(),
        FileTreeNode::File {
            contents: asset.clone(),
        },
    );
    root_files.insert(
        "assets".to_string(),
        FileTreeNode::Directory { files: assets },
    );
    // An empty directory: documented to be dropped by flatten (file-addressed
    // round trip), so it must NOT appear in to_tree() output.
    root_files.insert(
        "empty".to_string(),
        FileTreeNode::Directory {
            files: HashMap::new(),
        },
    );
    let root = FileTreeNode::Directory { files: root_files };

    let quill = Quill::from_tree(root).unwrap();

    // to_tree yields every file by its "/"-joined relative path, sorted — and
    // the empty `empty/` directory is absent (only files are emitted).
    let flat = quill.to_tree();
    assert_eq!(
        flat,
        vec![
            ("Quill.yaml".to_string(), quill_yaml),
            ("assets/logo.bin".to_string(), asset),
            ("plate.typ".to_string(), plate),
        ]
    );
    assert!(!flat.iter().any(|(p, _)| p.starts_with("empty")));

    // Re-feeding the flattened tree reproduces an equivalent quill.
    let mut rebuilt_root = FileTreeNode::Directory {
        files: HashMap::new(),
    };
    for (path, contents) in quill.to_tree() {
        rebuilt_root
            .insert(&path, FileTreeNode::File { contents })
            .unwrap();
    }
    let rebuilt = Quill::from_tree(rebuilt_root).unwrap();
    assert_eq!(rebuilt.name(), quill.name());
    assert_eq!(rebuilt.to_tree(), quill.to_tree());
}

#[test]
fn test_dir_exists_and_list_apis() {
    let mut root_files = HashMap::new();

    // Add Quill.yaml
    root_files.insert(
            "Quill.yaml".to_string(),
            FileTreeNode::File {
                contents: b"quill:\n  name: test\n  version: \"1.0\"\n  backend: typst\n  description: Test quill\n"
                    .to_vec(),
            },
        );

    // Add plate file
    root_files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: b"plate content".to_vec(),
        },
    );

    // Add assets directory with files
    let mut assets_files = HashMap::new();
    assets_files.insert(
        "logo.png".to_string(),
        FileTreeNode::File {
            contents: vec![137, 80, 78, 71],
        },
    );
    assets_files.insert(
        "icon.svg".to_string(),
        FileTreeNode::File {
            contents: b"<svg></svg>".to_vec(),
        },
    );

    // Add subdirectory in assets
    let mut fonts_files = HashMap::new();
    fonts_files.insert(
        "font.ttf".to_string(),
        FileTreeNode::File {
            contents: b"font data".to_vec(),
        },
    );
    assets_files.insert(
        "fonts".to_string(),
        FileTreeNode::Directory { files: fonts_files },
    );

    root_files.insert(
        "assets".to_string(),
        FileTreeNode::Directory {
            files: assets_files,
        },
    );

    // Add empty directory
    root_files.insert(
        "empty".to_string(),
        FileTreeNode::Directory {
            files: HashMap::new(),
        },
    );

    let root = FileTreeNode::Directory { files: root_files };
    let quill = Quill::from_tree(root).unwrap();

    // Test dir_exists
    assert!(quill.dir_exists("assets"));
    assert!(quill.dir_exists("assets/fonts"));
    assert!(quill.dir_exists("empty"));
    assert!(!quill.dir_exists("nonexistent"));
    assert!(!quill.dir_exists("plate.typ")); // file, not directory

    // Test file_exists
    assert!(quill.file_exists("plate.typ"));
    assert!(quill.file_exists("assets/logo.png"));
    assert!(quill.file_exists("assets/fonts/font.ttf"));
    assert!(!quill.file_exists("assets")); // directory, not file

    // Test list_files
    let root_files_list = quill.list_files("");
    assert_eq!(root_files_list.len(), 2); // Quill.yaml and plate.typ
    assert!(root_files_list.contains(&"Quill.yaml".to_string()));
    assert!(root_files_list.contains(&"plate.typ".to_string()));

    let assets_files_list = quill.list_files("assets");
    assert_eq!(assets_files_list.len(), 2); // logo.png and icon.svg
    assert!(assets_files_list.contains(&"logo.png".to_string()));
    assert!(assets_files_list.contains(&"icon.svg".to_string()));

    // Test list_subdirectories
    let root_subdirs = quill.list_subdirectories("");
    assert_eq!(root_subdirs.len(), 2); // assets and empty
    assert!(root_subdirs.contains(&"assets".to_string()));
    assert!(root_subdirs.contains(&"empty".to_string()));

    let assets_subdirs = quill.list_subdirectories("assets");
    assert_eq!(assets_subdirs.len(), 1); // fonts
    assert!(assets_subdirs.contains(&"fonts".to_string()));

    let empty_subdirs = quill.list_subdirectories("empty");
    assert_eq!(empty_subdirs.len(), 0);
}

#[test]
fn test_field_schemas_parsing() {
    let mut root_files = HashMap::new();

    // Add Quill.yaml with field schemas
    let quill_yaml = r#"quill:
  name: "taro"
  version: "1.0"
  backend: "typst"
  description: "Test template for field schemas"

main:
  fields:
    author:
      type: "string"
      description: "Author of document"
    ice_cream:
      type: "string"
      description: "favorite ice cream flavor"
    title:
      type: "string"
      description: "title of document"
"#;
    root_files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: quill_yaml.as_bytes().to_vec(),
        },
    );

    // Add plate file
    let plate_content = "= Test Template\n\nThis is a test.";
    root_files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: plate_content.as_bytes().to_vec(),
        },
    );

    let root = FileTreeNode::Directory { files: root_files };

    // Create Quill from tree
    let quill = Quill::from_tree(root).unwrap();

    // Validate field schemas were parsed from QuillConfig
    assert_eq!(quill.config.main.fields.len(), 3);
    assert!(quill.config.main.fields.contains_key("author"));
    assert!(quill.config.main.fields.contains_key("ice_cream"));
    assert!(quill.config.main.fields.contains_key("title"));

    // Verify author field schema
    let author_schema = quill.config.main.fields.get("author").unwrap();
    assert_eq!(
        author_schema.description.as_deref(),
        Some("Author of document")
    );

    // Verify ice_cream field schema (no required field, should default to false)
    let ice_cream_schema = quill.config.main.fields.get("ice_cream").unwrap();
    assert_eq!(
        ice_cream_schema.description.as_deref(),
        Some("favorite ice cream flavor")
    );

    // Verify title field schema
    let title_schema = quill.config.main.fields.get("title").unwrap();
    assert_eq!(
        title_schema.description.as_deref(),
        Some("title of document")
    );
}

#[test]
fn test_field_schema_struct() {
    // Parse a FieldSchema from YAML with every field set.
    let yaml_str = r#"
description: "Full field schema"
type: "string"
example: "Example value"
default: "Default value"
"#;
    let quill_value = QuillValue::from_yaml_str(yaml_str).unwrap();
    let schema2 = FieldSchema::from_quill_value("test_name".to_string(), &quill_value).unwrap();
    assert_eq!(schema2.name, "test_name");
    assert_eq!(schema2.description, Some("Full field schema".to_string()));
    assert_eq!(schema2.r#type, FieldType::String);
    assert_eq!(
        schema2.example.as_ref().and_then(|v| v.as_str()),
        Some("Example value")
    );
    assert_eq!(
        schema2.default.as_ref().and_then(|v| v.as_str()),
        Some("Default value")
    );
}

#[test]
fn test_field_schema_ui_compact() {
    let yaml_str = r#"
type: "string"
description: "A compact field"
ui:
  compact: true
"#;
    let quill_value = QuillValue::from_yaml_str(yaml_str).unwrap();
    let schema = FieldSchema::from_quill_value("compact_field".to_string(), &quill_value).unwrap();
    assert_eq!(schema.ui.as_ref().unwrap().compact, Some(true));
}

#[test]
fn test_quill_without_plate_file() {
    // A quill that declares no backend template loads fine — plate selection is
    // a backend's private concern, not a load-time requirement of core.
    let mut root_files = HashMap::new();

    // Add Quill.yaml with no backend section at all
    let quill_yaml = r#"quill:
  name: "test_no_plate"
  version: "1.0"
  backend: "typst"
  description: "Test quill without plate file"
"#;
    root_files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: quill_yaml.as_bytes().to_vec(),
        },
    );

    let root = FileTreeNode::Directory { files: root_files };

    // Create Quill from tree
    let quill = Quill::from_tree(root).unwrap();

    // No `typst:` section means no `typst_plate_file` metadata surfaces.
    assert!(!quill.metadata.contains_key("typst_plate_file"));
    assert_eq!(quill.name(), "test_no_plate");
}

#[test]
fn test_quill_config_from_yaml() {
    // Test parsing QuillConfig from YAML content
    let yaml_content = r#"
quill:
  name: test_config
  version: "1.0"
  backend: typst
  description: Test configuration parsing
  author: Test Author

typst:
  plate_file: plate.typ
  packages:
    - "@preview/bubble:0.2.2"

main:
  fields:
    title:
      description: Document title
      type: string
    author:
      type: string
      description: Document author
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    // Verify required fields
    assert_eq!(config.name, "test_config");
    assert_eq!(config.main.name, "main");
    assert_eq!(config.backend, "typst");
    assert_eq!(config.description, "Test configuration parsing");
    // `main.description` is independent of `quill.description`; this fixture
    // does not declare one under `main:`, so it stays absent.
    assert_eq!(config.main.description, None);

    // Verify optional fields
    assert_eq!(config.version, "1.0");
    assert_eq!(config.author, "Test Author");

    // Verify backend-specific config (parsed from the [typst] section). The
    // Typst plate lives here alongside `packages`, not as a top-level key.
    assert_eq!(
        config
            .backend_config
            .get("plate_file")
            .and_then(|v| v.as_str()),
        Some("plate.typ")
    );
    assert!(config.backend_config.contains_key("packages"));

    // Verify field schemas
    assert_eq!(config.main.fields.len(), 2);
    assert!(config.main.fields.contains_key("title"));
    assert!(config.main.fields.contains_key("author"));

    let title_field = &config.main.fields["title"];
    assert_eq!(title_field.description, Some("Document title".to_string()));
    assert_eq!(title_field.r#type, FieldType::String);
}

#[test]
fn test_quill_config_missing_required_fields() {
    // Test that missing required fields result in error
    let yaml_missing_name = r#"
quill:
  backend: typst
  description: Missing name
"#;
    let result = QuillConfig::from_yaml(yaml_missing_name);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required 'name'"));

    let yaml_missing_backend = r#"
quill:
  name: test
  description: Missing backend
"#;
    let result = QuillConfig::from_yaml(yaml_missing_backend);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required 'backend'"));

    let yaml_missing_description = r#"
quill:
  name: test
  version: "1.0"
  backend: typst
"#;
    let result = QuillConfig::from_yaml(yaml_missing_description);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required 'description'"));
}

#[test]
fn test_quill_config_empty_description() {
    // Test that empty description results in error
    let yaml_empty_description = r#"
quill:
  name: test
  version: "1.0"
  backend: typst
  description: "   "
"#;
    let result = QuillConfig::from_yaml(yaml_empty_description);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("description' field in 'quill' section cannot be empty"));
}

#[test]
fn test_quill_config_missing_quill_section() {
    // Test that missing [quill] section results in error
    let yaml_no_section = r#"
fields:
  title:
    description: Title
"#;
    let result = QuillConfig::from_yaml(yaml_no_section);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Missing required 'quill' section"));
}

#[test]
fn test_quill_config_rejects_root_level_fields() {
    let yaml = r#"
quill:
  name: root_fields_test
  version: "1.0"
  backend: typst
  description: Root fields must not be used

fields:
  title:
    type: string
"#;
    let result = QuillConfig::from_yaml(yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("main.fields"));
}

#[test]
fn test_quill_config_rejects_non_snake_case_quill_name() {
    let yaml = r#"
quill:
  name: BadQuill
  version: "1.0"
  backend: typst
  description: Bad quill name
"#;

    let result = QuillConfig::from_yaml(yaml);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("BadQuill"));
    assert!(err.contains("snake_case"));
}

#[test]
fn test_quill_config_rejects_non_snake_case_card_name() {
    let yaml = r#"
quill:
  name: good_quill
  version: "1.0"
  backend: typst
  description: Bad card name

card_kinds:
  BadCard:
    fields:
      title:
        type: string
"#;

    let result = QuillConfig::from_yaml(yaml);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("BadCard"));
    assert!(err.contains("[a-z_][a-z0-9_]*"));
}

#[test]
fn test_quill_config_accepts_leading_underscore_card_name() {
    let yaml = r#"
quill:
  name: good_quill
  version: "1.0"
  backend: typst
  description: Leading underscore card name

card_kinds:
  _private_card:
    fields:
      title:
        type: string
"#;

    let result = QuillConfig::from_yaml(yaml);
    assert!(result.is_ok());
}

#[test]
fn test_quill_config_rejects_non_snake_case_main_field_keys() {
    let yaml = r#"
quill:
  name: bad_field_key
  version: "1.0"
  backend: typst
  description: Bad main field key

main:
  fields:
    BadField:
      type: string
"#;

    let result = QuillConfig::from_yaml(yaml);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("BadField"));
    assert!(err.contains("snake_case"));
}

#[test]
fn test_quill_config_rejects_non_snake_case_card_field_keys() {
    let yaml = r#"
quill:
  name: bad_card_field_key
  version: "1.0"
  backend: typst
  description: Bad card field key

card_kinds:
  profile:
    fields:
      DisplayName:
        type: string
"#;

    let result = QuillConfig::from_yaml(yaml);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("DisplayName"));
    assert!(err.contains("snake_case"));
}

#[test]
fn test_quill_from_config_metadata() {
    // Test that QuillConfig metadata flows through to Quill
    let mut root_files = HashMap::new();

    let quill_yaml = r#"
quill:
  name: metadata_test
  version: "1.0"
  backend: typst
  description: Test metadata flow
  author: Test Author

typst:
  packages:
    - "@preview/bubble:0.2.2"
"#;
    root_files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: quill_yaml.as_bytes().to_vec(),
        },
    );

    let root = FileTreeNode::Directory { files: root_files };
    let quill = Quill::from_tree(root).unwrap();

    // Verify metadata includes backend and description
    assert!(quill.metadata.contains_key("backend"));
    assert!(quill.metadata.contains_key("description"));
    assert!(quill.metadata.contains_key("author"));

    // Verify typst config with typst_ prefix
    assert!(quill.metadata.contains_key("typst_packages"));
}

#[test]
fn test_config_defaults_method() {
    let yaml_content = r#"
quill:
  name: defaults_test
  version: "1.0"
  backend: typst
  description: Defaults test

main:
  fields:
    author:
      type: string
      default: Anonymous
      example: Alice
    status:
      type: string
      default: draft
    title:
      type: string
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let defaults = config.main.defaults();

    assert_eq!(defaults.len(), 2);
    assert_eq!(defaults.get("author").unwrap().as_str(), Some("Anonymous"));
    assert_eq!(defaults.get("status").unwrap().as_str(), Some("draft"));
    assert!(!defaults.contains_key("title"));

    // example takes precedence over default in template
    let author_example = config.main.fields.get("author").unwrap().example.as_ref();
    assert_eq!(author_example.and_then(|v| v.as_str()), Some("Alice"));
}

#[test]
fn test_card_defaults_method() {
    let yaml_content = r#"
quill:
  name: card_defaults_test
  version: "1.0"
  backend: typst
  description: Card defaults test

card_kinds:
  indorsement:
    fields:
      signature_block:
        type: string
        default: Commander
        example: Col Smith
      office:
        type: string
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let card = config.card_kind("indorsement").unwrap();
    let card_defaults = card.defaults();
    assert_eq!(card_defaults.len(), 1);
    assert_eq!(
        card_defaults.get("signature_block").unwrap().as_str(),
        Some("Commander")
    );

    let sig_example = card.fields.get("signature_block").unwrap().example.as_ref();
    assert_eq!(sig_example.and_then(|v| v.as_str()), Some("Col Smith"));

    assert!(config.card_kind("unknown").is_none());
}

#[test]
fn test_field_order_preservation() {
    let yaml_content = r#"
quill:
  name: order_test
  version: "1.0"
  backend: typst
  description: Test field order

main:
  fields:
    first:
      type: string
      description: First field
    second:
      type: string
      description: Second field
    third:
      type: string
      description: Third field
      ui:
        group: Test Group
    fourth:
      type: string
      description: Fourth field
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    // Declaration order is display order, carried structurally by the field
    // map — no stamped `ui.order` integer. Iterating the map yields the
    // authored order.
    let names: Vec<&str> = config.main.fields.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["first", "second", "third", "fourth"]);

    let third = config.main.fields.get("third").unwrap();
    assert_eq!(
        third.ui.as_ref().unwrap().group,
        Some("Test Group".to_string())
    );
}

#[test]
fn test_quill_with_all_ui_properties() {
    let yaml_content = r#"
quill:
  name: full_ui_test
  version: "1.0"
  backend: typst
  description: Test all UI properties

main:
  fields:
    author:
      description: The full name of the document author
      type: string
      ui:
        group: Author Info
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let author_field = &config.main.fields["author"];
    let ui = author_field.ui.as_ref().unwrap();
    assert_eq!(ui.group, Some("Author Info".to_string()));
    // Order is not a field-level knob; `author` leads because it is declared
    // first in the field map.
    assert_eq!(config.main.fields.get_index_of("author"), Some(0));
}
#[test]
fn test_field_schema_with_description() {
    let yaml = r#"
description: "Detailed field description"
type: "string"
example: "Example value"
ui:
  group: "Test Group"
"#;
    let quill_value = QuillValue::from_yaml_str(yaml).unwrap();
    let schema = FieldSchema::from_quill_value("test_field".to_string(), &quill_value).unwrap();

    assert_eq!(
        schema.description,
        Some("Detailed field description".to_string())
    );

    assert_eq!(
        schema.example.as_ref().and_then(|v| v.as_str()),
        Some("Example value")
    );

    let ui = schema.ui.as_ref().unwrap();
    assert_eq!(ui.group, Some("Test Group".to_string()));
}

#[test]
fn test_parse_card_with_fields_in_yaml() {
    // Test parsing [cards] section with [cards.X.fields.Y] syntax
    let yaml_content = r#"
quill:
  name: cards_fields_test
  version: "1.0"
  backend: typst
  description: Test [cards.X.fields.Y] syntax

card_kinds:
  endorsements:
    description: Chain of endorsements
    fields:
      name:
        type: string
        description: Name of the endorsing official
      org:
        type: string
        description: Endorser's organization
        default: Unknown
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    // Verify the card-kind was parsed into config.card_kinds
    assert!(config.card_kind("endorsements").is_some());
    let card = config.card_kind("endorsements").unwrap();

    assert_eq!(card.name, "endorsements");
    assert_eq!(card.description, Some("Chain of endorsements".to_string()));

    // Verify card fields
    assert_eq!(card.fields.len(), 2);

    let name_field = card.fields.get("name").unwrap();
    assert_eq!(name_field.r#type, FieldType::String);
    // Unendorsed: no default declared.
    assert!(name_field.default.is_none());

    let org_field = card.fields.get("org").unwrap();
    assert_eq!(org_field.r#type, FieldType::String);
    assert!(org_field.default.is_some());
    assert_eq!(
        org_field.default.as_ref().unwrap().as_str(),
        Some("Unknown")
    );
}

#[test]
fn test_field_schema_rejects_unknown_keys() {
    // Test that unknown keys like "invalid_key" are rejected (strict mode)
    let yaml = r#"
type: "string"
description: "A string field"
invalid_key:
  sub_field:
    type: "string"
    description: "Nested field"
"#;
    let quill_value = QuillValue::from_yaml_str(yaml).unwrap();

    let result = FieldSchema::from_quill_value("author".to_string(), &quill_value);

    // The parsing should fail due to deny_unknown_fields
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("unknown field `invalid_key`"),
        "Error was: {}",
        err
    );
}

#[test]
fn test_quill_config_with_cards_section() {
    let yaml_content = r#"
quill:
  name: cards_test
  version: "1.0"
  backend: typst
  description: Test [cards] section

main:
  fields:
    regular:
      description: Regular field
      type: string

card_kinds:
  indorsements:
    description: Chain of endorsements
    fields:
      name:
        type: string
        description: Name field
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    // Check regular field
    assert!(config.main.fields.contains_key("regular"));
    let regular = config.main.fields.get("regular").unwrap();
    assert_eq!(regular.r#type, FieldType::String);

    // Check card-kind is in config.card_kinds (not config.main.fields)
    assert!(config.card_kind("indorsements").is_some());
    let card = config.card_kind("indorsements").unwrap();
    assert_eq!(card.description, Some("Chain of endorsements".to_string()));
    assert!(card.fields.contains_key("name"));
}

#[test]
fn test_quill_config_cards_empty_fields() {
    // Test that cards with no fields section are valid
    let yaml_content = r#"
quill:
  name: cards_empty_fields_test
  version: "1.0"
  backend: typst
  description: Test cards without fields

card_kinds:
  myscope:
    description: My scope
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let card = config.card_kind("myscope").unwrap();
    assert_eq!(card.name, "myscope");
    assert_eq!(card.description, Some("My scope".to_string()));
    assert!(card.fields.is_empty());
}

#[test]
fn test_quill_config_allows_card_collision() {
    // Test that scope name colliding with field name is ALLOWED
    let yaml_content = r#"
quill:
  name: collision_test
  version: "1.0"
  backend: typst
  description: Test collision

main:
  fields:
    conflict:
      description: Field
      type: string

card_kinds:
  conflict:
    description: Card
"#;

    let result = QuillConfig::from_yaml(yaml_content);
    if let Err(e) = &result {
        panic!(
            "Card name collision should be allowed, but got error: {}",
            e
        );
    }
    assert!(result.is_ok());

    let config = result.unwrap();
    assert!(config.main.fields.contains_key("conflict"));
    assert!(config.card_kind("conflict").is_some());
}

#[test]
fn test_card_field_order_preservation() {
    // Test that card fields preserve definition order (not alphabetical)
    // defined: z_first, then a_second
    // alphabetical: a_second, then z_first
    let yaml_content = r#"
quill:
  name: card_order_test
  version: "1.0"
  backend: typst
  description: Test card field order

card_kinds:
  mycard:
    description: Test card
    fields:
      z_first:
        type: string
        description: Defined first
      a_second:
        type: string
        description: Defined second
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let card = config.card_kind("mycard").unwrap();

    // Declaration order (z_first, then a_second) is preserved, not alphabetized.
    let names: Vec<&str> = card.fields.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["z_first", "a_second"]);
}
#[test]
fn test_nested_schema_parsing() {
    let yaml_content = r#"
quill:
  name: nested_test
  version: "1.0"
  backend: typst
  description: Test nested elements

main:
  fields:
    my_list:
      type: array
      description: List of objects
      items:
        type: object
        properties:
          sub_a:
            type: string
            description: Subfield A
          sub_b:
            type: number
            description: Subfield B
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let list_field = config.main.fields.get("my_list").unwrap();
    assert_eq!(list_field.r#type, FieldType::Array);
    let items = list_field.items.as_ref().expect("array declares items");
    assert_eq!(items.r#type, FieldType::Object);

    let props = items.properties.as_ref().unwrap();
    assert!(props.contains_key("sub_a"));
    assert!(props.contains_key("sub_b"));
    assert_eq!(props["sub_a"].r#type, FieldType::String);
    assert_eq!(props["sub_b"].r#type, FieldType::Number);
}

#[test]
fn test_typed_object_field_accepted() {
    let yaml_content = r#"
quill:
  name: obj_test
  version: "1.0"
  backend: typst
  description: Test typed dictionary acceptance

main:
  fields:
    valid_field:
      type: string
      description: A normal field
    address:
      type: object
      description: Typed dictionary with properties
      properties:
        street:
          type: string
        city:
          type: string
"#;

    let (config, warnings) = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap();
    assert!(warnings.is_empty());
    let field = &config.main.fields["address"];
    assert_eq!(field.r#type, FieldType::Object);
    assert!(field.properties.as_ref().unwrap().contains_key("street"));
}

#[test]
fn test_untyped_object_field_rejected() {
    let yaml_content = r#"
quill:
  name: obj_test
  version: "1.0"
  backend: typst
  description: Test freeform object rejection

main:
  fields:
    metadata:
      type: object
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].severity, Severity::Error);
    assert_eq!(
        err[0].code.as_deref(),
        Some("quill::object_missing_properties")
    );
    assert!(err[0].message.contains("metadata"));
}

#[test]
fn test_empty_properties_object_rejected() {
    for (label, yaml_content) in [
        (
            "top-level object",
            r#"
quill:
  name: obj_test
  version: "1.0"
  backend: typst
  description: Test empty properties rejection

main:
  fields:
    metadata:
      type: object
      properties: {}
"#,
        ),
        (
            "array items object",
            r#"
quill:
  name: obj_test
  version: "1.0"
  backend: typst
  description: Test empty properties rejection in array items

main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties: {}
"#,
        ),
    ] {
        let err = QuillConfig::from_yaml_with_warnings(yaml_content)
            .expect_err(&format!("{label}: expected error for empty properties"));
        assert_eq!(err.len(), 1, "{label}");
        assert_eq!(err[0].severity, Severity::Error, "{label}");
        assert_eq!(
            err[0].code.as_deref(),
            Some("quill::object_empty_properties"),
            "{label}"
        );
    }
}

#[test]
fn test_nested_object_in_typed_table_rejected_with_error() {
    let yaml_content = r#"
quill:
  name: nested_obj_test
  version: "1.0"
  backend: typst
  description: Test nested object in typed table rejection

main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties:
          score:
            type: number
          nested:
            type: object
            properties:
              inner:
                type: string
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].severity, Severity::Error);
    assert_eq!(
        err[0].code.as_deref(),
        Some("quill::nested_object_not_supported")
    );
    assert!(err[0].message.contains("rows"));
}

#[test]
fn nested_object_properties_preserve_declaration_order() {
    // Properties render in declaration order, not the alphabetical order a
    // `BTreeMap` would impose: `zulu` is declared first, so it leads `alpha`
    // despite the reversed alphabet. The `IndexMap` carries the order.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    address:
      type: object
      properties:
        zulu: { type: string }
        alpha: { type: string }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    let props = config.main.fields["address"].properties.as_ref().unwrap();
    let names: Vec<&str> = props.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["zulu", "alpha"]);
}

#[test]
fn typed_table_row_properties_preserve_declaration_order() {
    // The synthetic row of a typed table is an object built by the same
    // `from_quill_value` recursion, so its properties keep declaration order too.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties:
          org: { type: string }
          year: { type: integer }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    let props = config.main.fields["rows"].items.as_ref().unwrap();
    let props = props.properties.as_ref().unwrap();
    let names: Vec<&str> = props.keys().map(|k| k.as_str()).collect();
    assert_eq!(names, ["org", "year"]);
}

#[test]
fn authored_ui_order_on_nested_property_is_rejected() {
    // `ui.order` is retired: display order is declaration order. An authored
    // `order` on any field — nested or top-level — is a hard load error with
    // the migration message.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string, ui: { order: 5 } }
        city: { type: string }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.message.contains("ui.order is no longer accepted")),
        "expected ui.order rejection, got: {err:?}"
    );
}

#[test]
fn authored_ui_order_on_card_field_is_rejected() {
    // Same rejection at card level, with an actionable hint.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    title: { type: string, ui: { order: 3 } }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    let hit = err
        .iter()
        .find(|d| d.message.contains("ui.order is no longer accepted"))
        .expect("expected ui.order rejection");
    assert!(
        hit.hint
            .as_deref()
            .is_some_and(|h| h.contains("reorder the fields")),
        "expected migration hint, got: {:?}",
        hit.hint
    );
}

#[test]
fn ui_group_on_object_property_is_rejected() {
    // `ui.group` clusters card-level fields only; on a typed-dictionary
    // property it was silently inert, and now loads as a hard error.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    address:
      type: object
      properties:
        street: { type: string, ui: { group: Location } }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::nested_group_not_supported")));
}

#[test]
fn ui_group_on_typed_table_property_is_rejected() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties:
          score: { type: number, ui: { group: Metrics } }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::nested_group_not_supported")));
}

#[test]
fn ui_group_on_card_level_field_is_still_accepted() {
    // Regression guard: the nested-position rejection must not touch card-level
    // fields, where grouping is the whole point.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    subject: { type: string, ui: { group: Addressing } }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    assert_eq!(
        config.main.fields["subject"]
            .ui
            .as_ref()
            .and_then(|u| u.group.as_deref()),
        Some("Addressing")
    );
}

#[test]
fn group_registry_list_form_orders_blueprint_by_declaration() {
    // The registry declares `beta` before `alpha`, but a field references
    // `alpha` first (earlier in declaration order). Clustering must follow
    // registry order (beta, then alpha), not first-appearance order.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [beta, alpha]
  fields:
    a1: { type: string, ui: { group: alpha } }
    b1: { type: string, ui: { group: beta } }
"#;
    let bp = QuillConfig::from_yaml(yaml).unwrap().blueprint();
    let a = bp.find("a1:").unwrap();
    let b = bp.find("b1:").unwrap();
    assert!(b < a, "registry order (beta<alpha) must drive clustering:\n{bp}");
}

#[test]
fn group_registry_map_form_carries_title_override() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups:
      addressing: {}
      letterhead: { title: "Letterhead & Seal" }
  fields:
    a: { type: string, ui: { group: addressing } }
    l: { type: string, ui: { group: letterhead } }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    let reg = &config.main.ui.as_ref().unwrap().groups.as_ref().unwrap().0;
    assert_eq!(reg.len(), 2);
    assert_eq!(reg[0].id, "addressing");
    assert_eq!(reg[0].title, None);
    assert_eq!(reg[1].id, "letterhead");
    assert_eq!(reg[1].title.as_deref(), Some("Letterhead & Seal"));
}

#[test]
fn unknown_group_reference_is_rejected() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [addressing]
  fields:
    subject: { type: string, ui: { group: letterhead } }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::unknown_group")));
}

#[test]
fn implicit_group_without_registry_warns() {
    // No registry + a field group is the deprecated implicit-group form.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  fields:
    subject: { type: string, ui: { group: Addressing } }
"#;
    let (_config, warnings) = QuillConfig::from_yaml_with_warnings(yaml).unwrap();
    assert!(warnings
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::implicit_group")));
}

#[test]
fn duplicate_group_id_is_rejected() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [addressing, addressing]
  fields:
    subject: { type: string, ui: { group: addressing } }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::duplicate_group")));
}

#[test]
fn non_snake_case_group_id_is_rejected() {
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [Addressing]
  fields:
    subject: { type: string, ui: { group: Addressing } }
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::invalid_group_id")));
}

#[test]
fn group_registry_round_trips_through_serde_and_schema() {
    // List-form input normalizes to the canonical map form on emit, preserving
    // declaration order, and re-parses to the identical registry.
    let yaml = r#"
quill: { name: x, version: "1.0", backend: typst, description: x }
main:
  ui:
    groups: [addressing, letterhead]
  fields:
    a: { type: string, ui: { group: addressing } }
    l: { type: string, ui: { group: letterhead } }
"#;
    let config = QuillConfig::from_yaml(yaml).unwrap();
    let ui = config.main.ui.clone().unwrap();
    let json = serde_json::to_value(&ui).unwrap();
    let back: UiCardSchema = serde_json::from_value(json).unwrap();
    assert_eq!(ui, back, "registry must survive emit → parse");

    // Emission carries order (preserve_order) and titles are omitted when derived.
    let schema = config.schema();
    let groups = schema["main"]["ui"]["groups"].as_object().unwrap();
    let keys: Vec<&str> = groups.keys().map(String::as_str).collect();
    assert_eq!(keys, ["addressing", "letterhead"]);
    assert!(groups["addressing"].as_object().unwrap().is_empty());
}

#[test]
fn migrated_group_fixtures_declare_registries_and_do_not_warn() {
    for name in ["usaf_memo/0.2.0", "cmu_letter/0.1.0", "classic_resume/0.1.0"] {
        let path = quillmark_fixtures::resource_path(&format!("quills/{name}/Quill.yaml"));
        let yaml = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        let (config, warnings) = QuillConfig::from_yaml_with_warnings(&yaml)
            .unwrap_or_else(|e| panic!("{name} must load: {e:?}"));
        assert!(
            !warnings
                .iter()
                .any(|d| d.code.as_deref() == Some("quill::implicit_group")),
            "{name} still emits implicit_group: {warnings:?}"
        );
        assert!(
            config
                .main
                .ui
                .as_ref()
                .and_then(|u| u.groups.as_ref())
                .is_some(),
            "{name} main should declare a group registry"
        );
    }
}

#[test]
fn test_array_items_recursive_coercion() {
    let yaml_content = r#"
quill:
  name: coerce_test
  version: "1.0"
  backend: typst
  description: Test recursive coercion for array items

main:
  fields:
    scores:
      type: array
      items:
        type: object
        properties:
          name:
            type: string
          value:
            type: number
          active:
            type: boolean
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "scores".to_string(),
        crate::value::QuillValue::from_json(serde_json::json!([
            {"name": "Math", "value": "95", "active": "true"},
            {"name": "Science", "value": "88.5", "active": "false"}
        ])),
    );

    let coerced = config.coerce_payload(&payload).unwrap();
    let scores = coerced.get("scores").unwrap();
    let arr = scores.as_array().unwrap();

    let first = arr[0].as_object().unwrap();
    assert_eq!(first["name"], serde_json::json!("Math"));
    assert_eq!(first["value"], serde_json::json!(95)); // coerced from "95"
    assert_eq!(first["active"], serde_json::json!(true)); // coerced from "true"

    let second = arr[1].as_object().unwrap();
    assert_eq!(second["value"], serde_json::json!(88.5)); // coerced from "88.5"
    assert_eq!(second["active"], serde_json::json!(false)); // coerced from "false"
}

#[test]
fn test_config_coerce_number_boolean_date_datetime_success() {
    let yaml_content = r#"
quill:
  name: coerce_success_test
  version: "1.0"
  backend: typst
  description: Coerce success

main:
  fields:
    count:
      type: number
    active:
      type: boolean
    signed_on:
      type: datetime
    created_at:
      type: datetime
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("42")),
    );
    payload.insert(
        "active".to_string(),
        QuillValue::from_json(serde_json::json!("true")),
    );
    payload.insert(
        "signed_on".to_string(),
        QuillValue::from_json(serde_json::json!("2026-04-13")),
    );
    payload.insert(
        "created_at".to_string(),
        QuillValue::from_json(serde_json::json!("2026-04-13T20:00:00Z")),
    );

    let coerced = config.coerce_payload(&payload).unwrap();
    assert_eq!(coerced.get("count").unwrap().as_i64(), Some(42));
    assert_eq!(coerced.get("active").unwrap().as_bool(), Some(true));
    assert_eq!(
        coerced.get("signed_on").unwrap().as_str(),
        Some("2026-04-13")
    );
    assert_eq!(
        coerced.get("created_at").unwrap().as_str(),
        Some("2026-04-13T20:00:00Z")
    );
}

#[test]
fn test_config_coerce_bare_scalar_into_string_uses_canonical_token() {
    // Gracious scalar→string: a bare boolean/integer/number written into a
    // `string` field is adopted as its canonical scalar text, losslessly.
    // (verified: true → "true", build_number: 47 → "47", ratio: 1.5 → "1.5").
    let yaml_content = r#"
quill:
  name: coerce_string_test
  version: "1.0"
  backend: typst
  description: Coerce bare scalars into strings

main:
  fields:
    verified:
      type: string
    build_number:
      type: string
    ratio:
      type: string
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "verified".to_string(),
        QuillValue::from_json(serde_json::json!(true)),
    );
    payload.insert(
        "build_number".to_string(),
        QuillValue::from_json(serde_json::json!(47)),
    );
    payload.insert(
        "ratio".to_string(),
        QuillValue::from_json(serde_json::json!(1.5)),
    );

    let coerced = config.coerce_payload(&payload).unwrap();
    assert_eq!(coerced.get("verified").unwrap().as_str(), Some("true"));
    assert_eq!(coerced.get("build_number").unwrap().as_str(), Some("47"));
    assert_eq!(coerced.get("ratio").unwrap().as_str(), Some("1.5"));
}

#[test]
fn test_config_coerce_integer_success() {
    let yaml_content = r#"
quill:
  name: coerce_integer_success_test
  version: "1.0"
  backend: typst
  description: Coerce integer success

main:
  fields:
    count:
      type: integer
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("42")),
    );

    let coerced = config.coerce_payload(&payload).unwrap();
    assert_eq!(coerced.get("count").unwrap().as_i64(), Some(42));
}

#[test]
fn test_config_coerce_integer_rejects_decimal() {
    let yaml_content = r#"
quill:
  name: coerce_integer_error_test
  version: "1.0"
  backend: typst
  description: Coerce integer errors

main:
  fields:
    count:
      type: integer
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("42.5")),
    );

    let error = config.coerce_payload(&payload).unwrap_err();
    assert!(matches!(
        error,
        super::CoercionError::Uncoercible { ref path, ref target, .. }
        if path == "count" && target == "integer"
    ));
}

#[test]
fn test_coerce_scalar_array_elements() {
    // A primitive `integer[]` coerces each element (string → integer), the
    // same coercion scalar fields get.
    let yaml_content = r#"
quill:
  name: scalar_array_coerce
  version: "1.0"
  backend: typst
  description: Coerce primitive arrays element-wise

main:
  fields:
    counts:
      type: array
      items:
        type: integer
"#;
    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "counts".to_string(),
        QuillValue::from_json(serde_json::json!(["1", "2", "3"])),
    );
    let coerced = config.coerce_payload(&payload).unwrap();
    assert_eq!(
        coerced.get("counts").unwrap().as_json(),
        &serde_json::json!([1, 2, 3])
    );
}

#[test]
fn test_coerce_scalar_array_reports_bad_element_path() {
    // An uncoercible element fails with the element's indexed path, just like
    // a typed-table leaf does.
    let yaml_content = r#"
quill:
  name: scalar_array_bad
  version: "1.0"
  backend: typst
  description: Bad primitive array element

main:
  fields:
    counts:
      type: array
      items:
        type: integer
"#;
    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "counts".to_string(),
        QuillValue::from_json(serde_json::json!([1, "nope"])),
    );
    let err = config.coerce_payload(&payload).unwrap_err();
    assert!(matches!(
        err,
        super::CoercionError::Uncoercible { ref path, ref target, .. }
        if path == "counts[1]" && target == "integer"
    ));
}

#[test]
fn test_array_missing_items_rejected() {
    // Every array must declare its element type via `items`.
    let yaml_content = r#"
quill:
  name: array_no_items
  version: "1.0"
  backend: typst
  description: Array without items

main:
  fields:
    tags:
      type: array
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();
    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::array_missing_items")
            && d.message.contains("tags")));
}

#[test]
fn test_array_bare_properties_rejected() {
    // A bare `properties` map on an array is rejected — element typing goes
    // under `items`.
    let yaml_content = r#"
quill:
  name: array_bare_props
  version: "1.0"
  backend: typst
  description: Array with bare properties

main:
  fields:
    rows:
      type: array
      properties:
        org:
          type: string
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();
    assert!(err.iter().any(
        |d| d.code.as_deref() == Some("quill::array_properties_not_supported")
            && d.message.contains("rows")
    ));
}

#[test]
fn test_nested_array_rejected() {
    // Array elements may be scalars or objects, but not arrays.
    let yaml_content = r#"
quill:
  name: nested_array
  version: "1.0"
  backend: typst
  description: Array of arrays

main:
  fields:
    grid:
      type: array
      items:
        type: array
        items:
          type: integer
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();
    assert!(err.iter().any(
        |d| d.code.as_deref() == Some("quill::nested_array_not_supported")
            && d.message.contains("grid")
    ));
}

#[test]
fn test_array_of_objects_with_array_property_rejected() {
    // The one-level rule: a typed-table row may carry scalar columns only, so
    // an array nested inside a table row is rejected.
    let yaml_content = r#"
quill:
  name: table_with_array
  version: "1.0"
  backend: typst
  description: Typed table row with an array column

main:
  fields:
    rows:
      type: array
      items:
        type: object
        properties:
          tags:
            type: array
            items:
              type: string
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();
    assert!(err.iter().any(
        |d| d.code.as_deref() == Some("quill::nested_array_not_supported")
            && d.message.contains("rows")
    ));
}

#[test]
fn test_object_with_array_property_rejected() {
    // Symmetric rule for typed dictionaries: an object property may only be a
    // scalar, so an array-valued property is rejected.
    let yaml_content = r#"
quill:
  name: object_with_array
  version: "1.0"
  backend: typst
  description: Typed dictionary with an array property

main:
  fields:
    address:
      type: object
      properties:
        lines:
          type: array
          items:
            type: string
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();
    assert!(err.iter().any(
        |d| d.code.as_deref() == Some("quill::nested_array_not_supported")
            && d.message.contains("address")
    ));
}

#[test]
fn test_config_coerce_cards_item_wise() {
    let yaml_content = r#"
quill:
  name: coerce_cards_items_test
  version: "1.0"
  backend: typst
  description: Coerce cards

card_kinds:
  indorsement:
    fields:
      score:
        type: number
      active:
        type: boolean
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut card_fields = indexmap::IndexMap::new();
    card_fields.insert(
        "score".to_string(),
        QuillValue::from_json(serde_json::json!("100")),
    );
    card_fields.insert(
        "active".to_string(),
        QuillValue::from_json(serde_json::json!("false")),
    );

    let coerced = config.coerce_card("indorsement", &card_fields).unwrap();
    assert_eq!(coerced.get("score").unwrap().as_i64(), Some(100));
    assert_eq!(coerced.get("active").unwrap().as_bool(), Some(false));
}

#[test]
fn test_config_coerce_error_unparseable_date() {
    let yaml_content = r#"
quill:
  name: coerce_date_error_test
  version: "1.0"
  backend: typst
  description: Coerce date errors

main:
  fields:
    signed_on:
      type: datetime
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "signed_on".to_string(),
        QuillValue::from_json(serde_json::json!("13-04-2026")),
    );

    let error = config.coerce_payload(&payload).unwrap_err();
    assert!(matches!(
        error,
        super::CoercionError::Uncoercible { ref path, ref target, .. }
        if path == "signed_on" && target == "datetime"
    ));
}

#[test]
fn test_config_coerce_error_unparseable_number() {
    let yaml_content = r#"
quill:
  name: coerce_number_error_test
  version: "1.0"
  backend: typst
  description: Coerce number errors

main:
  fields:
    count:
      type: number
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut payload = indexmap::IndexMap::new();
    payload.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("forty-two")),
    );

    let error = config.coerce_payload(&payload).unwrap_err();
    assert!(matches!(
        error,
        super::CoercionError::Uncoercible { ref path, ref target, .. }
        if path == "count" && target == "number"
    ));
}

#[test]
fn test_multiline_ui_field_parses() {
    let yaml_content = r#"
quill:
  name: multiline_test
  version: "1.0"
  backend: typst
  description: Test multiline ui hint

main:
  fields:
    summary:
      type: richtext
      description: Document summary
      ui:
        multiline: true
    notes:
      type: richtext
      description: Short notes
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    // `richtext` (block) carries the `multiline` ui hint like `string` does.
    let summary = config.main.fields.get("summary").unwrap();
    assert_eq!(summary.r#type, FieldType::RichText { inline: false });
    assert_eq!(summary.ui.as_ref().unwrap().multiline, Some(true));

    // A field with no authored `ui:` block now carries `ui: None` — order is
    // no longer stamped, so nothing fabricates a `ui` block.
    let notes = config.main.fields.get("notes").unwrap();
    assert_eq!(notes.r#type, FieldType::RichText { inline: false });
    assert!(notes.ui.is_none());
}

#[test]
fn test_card_ui_title_parses_literal_and_template_forms() {
    let yaml_content = r#"
quill:
  name: card_title_test
  version: "1.0"
  backend: typst
  description: Test ui.title on cards

main:
  ui:
    title: Memorandum
  fields:
    subject:
      type: string

card_kinds:
  indorsement:
    ui:
      title: "{from} → {for}"
    fields:
      from:
        type: string
      for:
        type: string
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    assert_eq!(
        config.main.ui.as_ref().unwrap().title.as_deref(),
        Some("Memorandum"),
        "literal main.ui.title"
    );
    let indorsement = config.card_kind("indorsement").unwrap();
    assert_eq!(
        indorsement.ui.as_ref().unwrap().title.as_deref(),
        Some("{from} → {for}"),
        "template card ui.title carried verbatim"
    );

    let schema = config.schema();
    assert_eq!(schema["main"]["ui"]["title"].as_str(), Some("Memorandum"));
    assert_eq!(
        schema["card_kinds"]["indorsement"]["ui"]["title"].as_str(),
        Some("{from} → {for}")
    );
}

#[test]
fn test_card_ui_title_omitted_when_absent() {
    let yaml_content = r#"
quill:
  name: no_title_test
  version: "1.0"
  backend: typst
  description: ui.title omitted when not declared

main:
  fields:
    subject:
      type: string
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    assert!(config.main.ui.as_ref().is_none_or(|ui| ui.title.is_none()));
}

#[test]
fn test_quill_config_from_yaml_errors_on_invalid_field() {
    let yaml_content = r#"
quill:
  name: error_config
  version: "1.0"
  backend: typst
  description: Error on invalid field test

main:
  fields:
    valid_field:
      type: string
      description: Valid
    broken_field:
      description: Missing required type
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].severity, Severity::Error);
    assert_eq!(err[0].code.as_deref(), Some("quill::field_parse_error"));
    assert!(err[0].message.contains("broken_field"));
}

#[test]
fn test_unknown_key_in_quill_section_errors() {
    // Typos like 'auther' should fail loudly, not silently land in metadata.
    let yaml_content = r#"
quill:
  name: unk_key
  version: "1.0"
  backend: typst
  description: Unknown key test
  auther: Jane Doe
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code.as_deref(), Some("quill::unknown_key"));
    assert!(err[0].message.contains("auther"));
    assert!(err[0].hint.as_deref().unwrap_or("").contains("author"));
}

#[test]
fn test_unknown_top_level_section_errors() {
    // 'card_kind' is a common typo for 'card_kinds'. Must not be silently ignored.
    let yaml_content = r#"
quill:
  name: unk_section
  version: "1.0"
  backend: typst
  description: Unknown section test

card_kind:
  foo:
    description: Should not silently disappear
    fields:
      bar:
        type: string
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert!(err.iter().any(|d| {
        d.code.as_deref() == Some("quill::unknown_section") && d.message.contains("card_kind")
    }));
}

#[test]
fn test_root_level_fields_gets_targeted_hint() {
    // Root-level `fields:` (instead of `main.fields:`) should produce a single
    // unknown_section error with a targeted hint, not a duplicate error.
    let yaml_content = r#"
quill:
  name: root_fields
  version: "1.0"
  backend: typst
  description: Root fields test

fields:
  author:
    type: string
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    let fields_errors: Vec<&Diagnostic> = err
        .iter()
        .filter(|d| d.message.contains("fields"))
        .collect();
    assert_eq!(
        fields_errors.len(),
        1,
        "expected exactly one error for root-level `fields`, got {} ({:?})",
        fields_errors.len(),
        fields_errors
    );
    assert_eq!(
        fields_errors[0].code.as_deref(),
        Some("quill::unknown_section")
    );
    assert!(fields_errors[0]
        .hint
        .as_deref()
        .unwrap_or("")
        .contains("main.fields"));
}

#[test]
fn test_multiple_errors_collected_in_one_pass() {
    // The headline DX behavior: an author with several mistakes should see
    // them all in one shot, not fix-rerun-fix-rerun.
    let yaml_content = r#"
quill:
  name: BadName
  version: "1.0"
  backend: typst
  description: Multi-error test
  platefile: foo.typ

main:
  fields:
    BadFieldName:
      type: string
    legit:
      title: Bad legacy key
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    // We expect at least: invalid_name + unknown_key + invalid_field_name + field_parse_error
    assert!(
        err.len() >= 4,
        "expected >=4 errors collected at once, got {}: {:?}",
        err.len(),
        err.iter().map(|d| d.code.as_deref()).collect::<Vec<_>>()
    );
    let codes: Vec<&str> = err.iter().filter_map(|d| d.code.as_deref()).collect();
    assert!(
        codes.contains(&"quill::invalid_name"),
        "missing invalid_name: {:?}",
        codes
    );
    assert!(
        codes.contains(&"quill::unknown_key"),
        "missing unknown_key: {:?}",
        codes
    );
    assert!(
        codes.contains(&"quill::invalid_field_name"),
        "missing invalid_field_name: {:?}",
        codes
    );
    assert!(
        codes.contains(&"quill::field_parse_error"),
        "missing field_parse_error: {:?}",
        codes
    );
}

#[test]
fn test_main_ui_malformed_errors_with_hint() {
    // main.ui should fail loudly when malformed, not be silently dropped.
    let yaml_content = r#"
quill:
  name: bad_ui
  version: "1.0"
  backend: typst
  description: Bad UI test

main:
  ui:
    bogus_key: nope
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert!(err
        .iter()
        .any(|d| d.code.as_deref() == Some("quill::invalid_ui")));
}

#[test]
fn test_field_with_title_key_errors_with_hint() {
    // 'title' is a common mistake — authors expect it to work like 'description'.
    // We must fail loudly with an actionable hint rather than silently dropping the field.
    let yaml_content = r#"
quill:
  name: hint_test
  version: "1.0"
  backend: typst
  description: Hint test

main:
  fields:
    author:
      type: string
      title: The document author
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code.as_deref(), Some("quill::field_parse_error"));
    assert_eq!(
        err[0].hint.as_deref(),
        Some("'title' is not a valid field key; use 'description' instead.")
    );
}

#[test]
fn test_field_ui_title_is_valid() {
    let yaml_content = r#"
quill:
  name: ui_title_test
  version: "1.0"
  backend: typst
  description: ui.title is valid on individual fields

main:
  fields:
    status:
      type: string
      ui:
        title: Status Label
"#;

    let config = QuillConfig::from_yaml(yaml_content).expect("ui.title on field should parse");
    assert_eq!(
        config.main.fields["status"]
            .ui
            .as_ref()
            .unwrap()
            .title
            .as_deref(),
        Some("Status Label")
    );
    assert_eq!(
        config.schema()["main"]["fields"]["status"]["ui"]["title"].as_str(),
        Some("Status Label")
    );
}

fn check_schema_snapshot(
    yaml_of: impl Fn(&QuillConfig) -> String,
    json_of: impl Fn(&QuillConfig) -> serde_json::Value,
    golden: &str,
) {
    let quill = load_from_path(quillmark_fixtures::resource_path("quills/usaf_memo/0.2.0"))
        .expect("load usaf_memo fixture");
    let yaml = yaml_of(&quill.config);
    let golden_path =
        quillmark_fixtures::resource_path(&format!("quills/usaf_memo/0.2.0/__golden__/{golden}"));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        fs::write(&golden_path, &yaml).expect("write golden");
    }
    assert_eq!(
        yaml,
        fs::read_to_string(&golden_path).expect("read golden"),
        "{golden} drifted"
    );

    let parsed: serde_json::Value = serde_saphyr::from_str(&yaml).expect("parse yaml");
    assert_eq!(json_of(&quill.config), parsed, "{golden} json/yaml parity");
    assert!(parsed.get("main").and_then(|v| v.get("fields")).is_some());
    assert!(parsed.get("card_kinds").is_some());
    assert!(parsed.get("ref").is_none() && parsed.get("example").is_none());
    assert!(yaml.contains("ui:"), "{golden} must include ui hints");
}

#[test]
fn schema_snapshot_usaf_memo_0_2_0() {
    check_schema_snapshot(|c| c.schema_yaml().unwrap(), |c| c.schema(), "schema.yaml");
}

#[test]
fn body_example_with_body_disabled_emits_warning() {
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  skills:
    body:
      enabled: false
      example: This example is unused
    fields:
      items: { type: array, items: { type: string } }
"#;
    let (_config, warnings) = QuillConfig::from_yaml_with_warnings(yaml).unwrap();
    assert!(
        warnings.iter().any(|d| d
            .code
            .as_deref()
            .map(|c| c == "quill::body_example_unused")
            .unwrap_or(false)),
        "expected body_example_unused warning, got: {:?}",
        warnings
    );
}

#[test]
fn body_example_with_card_yaml_fence_line_is_an_error() {
    // A `~~~card-yaml` opener in a body example would be parsed as a block.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Opening paragraph.\n\n~~~card-yaml\n$kind: note\n~~~\n\nClosing paragraph."
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|d| d
            .code
            .as_deref()
            .map(|c| c == "quill::body_example_contains_fence")
            .unwrap_or(false)),
        "expected body_example_contains_fence error, got: {:?}",
        errors
    );
}

#[test]
fn body_example_indented_fence_line_is_not_an_error() {
    // Card openers are at column zero; an indented `~~~card-yaml` (1–3 spaces)
    // is an ordinary code block and cannot corrupt the blueprint, so it is not
    // flagged.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "text\n   ~~~card-yaml\nmore text"
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(
        result.is_ok(),
        "an indented ~~~card-yaml is a code block, not a card opener"
    );
}

#[test]
fn body_example_four_leading_spaces_is_not_a_fence() {
    // Four leading spaces = indented code block, not a fence marker.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "text\n    ~~~card-yaml\nmore text"
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(
        result.is_ok(),
        "four-space indented ~~~card-yaml should not trigger fence error"
    );
}

#[test]
fn body_example_bare_triple_dash_is_not_a_fence() {
    // A bare `---` thematic break is not a metadata fence — allowed.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Opening paragraph.\n\n---\n\nClosing paragraph."
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(
        result.is_ok(),
        "a bare --- thematic break should not trigger a fence error"
    );
}

#[test]
fn body_example_bare_tilde_fence_line_is_an_error() {
    // A bare `~~~` opener (the canonical card-yaml fence) in a body example
    // would be parsed as a block and corrupt the blueprint.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Opening paragraph.\n\n~~~\n$kind: note\n~~~\n\nClosing paragraph."
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|d| d
            .code
            .as_deref()
            .map(|c| c == "quill::body_example_contains_fence")
            .unwrap_or(false)),
        "expected body_example_contains_fence error for bare ~~~, got: {:?}",
        errors
    );
}

#[test]
fn body_example_four_tilde_fence_is_an_error() {
    // A four-tilde fence is a (non-canonical) card opener, not a code block, so
    // it would corrupt the blueprint and must be rejected in a body example.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "See code:\n\n~~~~\n$kind: note\n~~~~\n\nEnd."
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|d| d
            .code
            .as_deref()
            .map(|c| c == "quill::body_example_contains_fence")
            .unwrap_or(false)),
        "expected body_example_contains_fence error for ~~~~, got: {:?}",
        errors
    );
}

#[test]
fn body_example_backtick_fence_is_allowed() {
    // A backtick fence is the escape hatch for a literal code block — it never
    // opens a card-yaml block, so it is allowed in a body example. (The guard
    // is line-based and conservative, so the example itself avoids bare `~~~`
    // lines, which it would flag regardless of an enclosing fence.)
    let yaml = "
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: \"See code:\\n\\n```rust\\nlet x = 1;\\n```\\n\\nEnd.\"
  fields:
    title: { type: string }
";
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(
        result.is_ok(),
        "a backtick code fence should not trigger a card-fence error: {result:?}"
    );
}

#[test]
fn body_example_card_yaml_fence_line_in_card_kind_is_an_error() {
    // The fence check applies to card-kind body examples too.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_kinds:
  note:
    body:
      example: "See below:\n~~~card-yaml\n$kind: other\n~~~\nEnd."
    fields:
      author: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|d| d
            .code
            .as_deref()
            .map(|c| c == "quill::body_example_contains_fence")
            .unwrap_or(false)),
        "expected body_example_contains_fence error for card kind, got: {:?}",
        errors
    );
}

#[test]
fn quill_yaml_deep_nesting_is_rejected() {
    // Mirrors the card-yaml payload depth-budget regression in
    // crates/quillmark/tests/security_tests.rs::test_yaml_depth_limit_attack.
    // Deeply nested YAML under any `quill` subtree must be refused by the
    // shared depth budget (`MAX_YAML_DEPTH`).
    let mut deep = String::from(
        "quill:\n  name: bomb\n  version: 1.0.0\n  backend: typst\n  description: bomb\n  payload:\n",
    );
    for i in 0..150 {
        deep.push_str(&"  ".repeat(i + 1));
        deep.push_str("nest:\n");
    }
    let result = QuillConfig::from_yaml(&deep);
    assert!(result.is_err(), "deeply nested Quill.yaml must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("depth") || msg.contains("YAML") || msg.contains("limit"),
        "error should reference the depth/YAML limit, got: {msg}"
    );
}

// ---------- example/default type-compatibility validation ----------

fn example_default_yaml(field_yaml: &str) -> String {
    format!(
        r#"
quill:
  name: example_default_test
  version: "1.0"
  backend: typst
  description: example/default type-compat tests

main:
  fields:
{}
"#,
        field_yaml
    )
}

#[test]
fn example_integer_type_rejects_float_example() {
    // type: integer with example: 20.04 fails — float is not an integer.
    let yaml = example_default_yaml("    year:\n      type: integer\n      example: 20.04\n");
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    assert!(
        errors.iter().any(
            |d| d.code.as_deref() == Some("quill::example_type_mismatch")
                && d.message.contains("year")
                && d.message.contains("integer")
                && d.message.contains("float")
        ),
        "expected example_type_mismatch error for integer/float, got: {:?}",
        errors
    );
}

#[test]
fn example_string_type_rejects_unquoted_decimal_example() {
    // The canonical bug: type: string with example: 20.04 — YAML parses the
    // bare token as a float, and the LLM would copy it back unquoted.
    let yaml =
        example_default_yaml("    min_os_version:\n      type: string\n      example: 20.04\n");
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::example_type_mismatch"))
        .expect("expected example_type_mismatch error");
    assert!(diag.message.contains("min_os_version"));
    assert!(diag.message.contains("string"));
    assert!(diag.message.contains("float"));
    let hint = diag.hint.as_deref().unwrap_or("");
    assert!(
        hint.contains("Quote") && hint.contains("\"20.04\""),
        "hint should suggest quoting, got: {}",
        hint
    );
}

#[test]
fn example_string_type_accepts_quoted_decimal_example() {
    // The fix: quoting forces the YAML parser to keep it as a string.
    let yaml =
        example_default_yaml("    min_os_version:\n      type: string\n      example: \"20.04\"\n");
    QuillConfig::from_yaml(&yaml).expect("quoted string example should load");
}

#[test]
fn example_boolean_type_rejects_string_example() {
    // type: boolean with example: "true" — the LLM would emit it as a string.
    let yaml = example_default_yaml("    flag:\n      type: boolean\n      example: \"true\"\n");
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    assert!(
        errors.iter().any(
            |d| d.code.as_deref() == Some("quill::example_type_mismatch")
                && d.message.contains("flag")
                && d.message.contains("boolean")
                && d.message.contains("string")
        ),
        "expected example_type_mismatch error for boolean/string, got: {:?}",
        errors
    );
}

#[test]
fn example_array_type_rejects_string_example() {
    // type: array with example: foo — a sequence is required.
    let yaml = example_default_yaml(
        "    tags:\n      type: array\n      items:\n        type: string\n      example: foo\n",
    );
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    assert!(
        errors.iter().any(
            |d| d.code.as_deref() == Some("quill::example_type_mismatch")
                && d.message.contains("tags")
                && d.message.contains("array")
                && d.message.contains("string")
        ),
        "expected example_type_mismatch error for array/string, got: {:?}",
        errors
    );
}

#[test]
fn example_not_in_enum_is_rejected() {
    let yaml = example_default_yaml(
        "    color:\n      type: string\n      enum: [a, b]\n      example: c\n",
    );
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::example_not_in_enum"))
        .expect("expected example_not_in_enum error");
    assert!(diag.message.contains("color"));
    assert!(diag.message.contains("\"c\""));
    assert!(diag.message.contains("\"a\""));
}

#[test]
fn example_in_enum_loads_successfully() {
    let yaml = example_default_yaml(
        "    color:\n      type: string\n      enum: [a, b]\n      example: a\n",
    );
    QuillConfig::from_yaml(&yaml).expect("enum-member example should load");
}

#[test]
fn default_with_type_mismatch_is_rejected() {
    // Defaults are validated the same way as examples.
    let yaml = example_default_yaml("    version:\n      type: string\n      default: 20.04\n");
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::default_type_mismatch"))
        .expect("expected default_type_mismatch error");
    assert!(diag.message.contains("version"));
    assert!(diag.message.contains("string"));
    assert!(diag.message.contains("float"));
}

#[test]
fn field_with_no_example_or_default_loads_successfully() {
    let yaml = example_default_yaml(
        "    bare:\n      type: string\n      description: nothing to check\n",
    );
    QuillConfig::from_yaml(&yaml).expect("field with no example/default should load");
}

#[test]
fn datetime_type_mismatch_reports_datetime_not_string() {
    // The mismatch message must name the field's declared type verbatim
    // (`datetime`), not the internal string-family collapse — otherwise the
    // author is told they declared `string` when they wrote `datetime`.
    let yaml = example_default_yaml("    signed_on:\n      type: datetime\n      example: 42\n");
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::example_type_mismatch"))
        .expect("expected example_type_mismatch error");
    assert!(
        diag.message.contains("declares type 'datetime'"),
        "message should name the datetime type, got: {}",
        diag.message
    );
    assert!(!diag.message.contains("type 'string'"));
}

#[test]
fn richtext_type_mismatch_reports_richtext_not_string() {
    // The mismatch names the declared type verbatim (`richtext`), not the
    // internal string-family collapse — a non-string, non-content default fails.
    let yaml = example_default_yaml("    body:\n      type: richtext\n      default: 42\n");
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::default_type_mismatch"))
        .expect("expected default_type_mismatch error");
    assert!(
        diag.message.contains("declares type 'richtext'"),
        "message should name the richtext type, got: {}",
        diag.message
    );
}

#[test]
fn type_date_is_rejected() {
    // `type: date` is not a valid field type; schemas declaring it must fail to
    // load with a parse error, not silently accept or coerce the field.
    let yaml = r#"
quill:
  name: old_date_field
  version: "1.0"
  backend: typst
  description: Schema using the removed date type

main:
  fields:
    due:
      type: date
"#;
    let err = QuillConfig::from_yaml_with_warnings(yaml).unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("quill::field_parse_error")
                && d.message.contains("due")),
        "expected field_parse_error for 'due' using removed type: date, got: {err:?}"
    );
}

#[test]
fn type_mismatch_preview_shows_array_contents() {
    // A compound value's preview should show its contents, not a `[…]`
    // placeholder, so the author can see what they wrote.
    let yaml = example_default_yaml(
        "    title:\n      type: string\n      example:\n        - one\n        - two\n",
    );
    let errors = QuillConfig::from_yaml_with_warnings(&yaml).unwrap_err();
    let diag = errors
        .iter()
        .find(|d| d.code.as_deref() == Some("quill::example_type_mismatch"))
        .expect("expected example_type_mismatch error");
    assert!(
        diag.message.contains("one") && diag.message.contains("two"),
        "preview should render array contents, got: {}",
        diag.message
    );
}

// ── Richtext inline field: load-time example import + cache ───────────────────

/// A minimal quill declaring one field, for the richtext field-type tests.
fn quill_with_field(field_yaml: &str) -> Result<QuillConfig, Vec<Diagnostic>> {
    let yaml = format!(
        "quill:\n  name: rt\n  version: \"1.0\"\n  backend: typst\n  description: rt\nmain:\n  fields:\n{field_yaml}"
    );
    QuillConfig::from_yaml_with_warnings(&yaml).map(|(c, _)| c)
}

#[test]
fn markdown_type_is_unknown_at_load() {
    let err = quill_with_field("    body:\n      type: markdown\n").unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("quill::field_parse_error")
                && d.message.contains("markdown")),
        "type: markdown should be a load error naming the type, got: {err:?}"
    );
}

#[test]
fn richtext_inline_type_token_is_rejected_at_load() {
    let err = quill_with_field("    tag:\n      type: richtext(inline)\n").unwrap_err();
    assert!(
        err.iter().any(|d| {
            d.code.as_deref() == Some("quill::field_parse_error")
                && d.message.contains("richtext(inline)")
        }),
        "type: richtext(inline) should be a load error, got: {err:?}"
    );
    assert!(
        err.iter().any(|d| {
            d.hint
                .as_deref()
                .is_some_and(|h| h.contains("inline: true"))
        }),
        "load error should hint at inline: true, got: {err:?}"
    );
}

#[test]
fn inline_on_non_richtext_field_is_rejected_at_load() {
    let err = quill_with_field("    name:\n      type: string\n      inline: true\n").unwrap_err();
    assert!(
        err.iter().any(|d| {
            d.code.as_deref() == Some("quill::field_parse_error") && d.message.contains("inline")
        }),
        "inline on string should fail parse, got: {err:?}"
    );
}

#[test]
fn inline_richtext_example_over_one_para_is_a_load_error() {
    let err = quill_with_field(
        "    tag:\n      type: richtext\n      inline: true\n      example: \"one\\n\\ntwo\"\n",
    )
    .unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("richtext::not_inline")),
        "a two-paragraph inline example should fail load with richtext::not_inline, got: {err:?}"
    );
}

#[test]
fn inline_richtext_single_line_example_loads_and_caches_corpus() {
    let config = quill_with_field(
        "    tag:\n      type: richtext\n      inline: true\n      example: \"a *bold* motto\"\n",
    )
    .expect("single-line inline example loads");
    let field = config.main.fields.get("tag").unwrap();
    assert_eq!(field.r#type, FieldType::RichText { inline: true });
    // The load pass imports the markdown example into its content companion; the
    // authored `example` string is retained untouched (Alternative A).
    let content = field
        .example_corpus
        .as_ref()
        .expect("example_corpus cached");
    assert!(
        content.as_json().is_object(),
        "cached example is a content object"
    );
    assert_eq!(
        field.example.as_ref().unwrap().as_str(),
        Some("a *bold* motto")
    );
}

#[test]
fn block_richtext_default_caches_corpus() {
    let config = quill_with_field(
        "    body:\n      type: richtext\n      default: \"## Heading\\n\\nBody.\"\n",
    )
    .expect("block richtext default loads");
    let field = config.main.fields.get("body").unwrap();
    let content = field
        .default_corpus
        .as_ref()
        .expect("default_corpus cached");
    assert!(
        content.as_json().is_object(),
        "cached default is a content object"
    );
}

#[test]
fn array_of_inline_richtext_caches_each_element() {
    let config = quill_with_field(
        "    refs:\n      type: array\n      items:\n        type: richtext\n        inline: true\n      example:\n        - \"first *ref*\"\n        - \"second ref\"\n",
    )
    .expect("array<inline richtext> loads");
    let field = config.main.fields.get("refs").unwrap();
    let content = field
        .example_corpus
        .as_ref()
        .expect("example_corpus cached");
    let arr = content.as_json().as_array().expect("array of content");
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter().all(|e| e.is_object()),
        "each element is a content object"
    );
}

#[test]
fn inline_coercion_rejects_multi_block_document_value() {
    let config =
        quill_with_field("    tag:\n      type: richtext\n      inline: true\n").expect("loads");
    let mut fields: indexmap::IndexMap<String, QuillValue> = indexmap::IndexMap::new();
    fields.insert(
        "tag".to_string(),
        QuillValue::from_json(serde_json::json!("one\n\ntwo")),
    );
    let err = config.coerce_payload(&fields).unwrap_err();
    assert!(
        err.to_string().contains("richtext(inline)"),
        "coercion should reject a two-paragraph value for an inline field, got: {err}"
    );
}

#[test]
fn inline_coercion_accepts_single_line_document_value() {
    let config =
        quill_with_field("    tag:\n      type: richtext\n      inline: true\n").expect("loads");
    let mut fields: indexmap::IndexMap<String, QuillValue> = indexmap::IndexMap::new();
    fields.insert(
        "tag".to_string(),
        QuillValue::from_json(serde_json::json!("just one line")),
    );
    let coerced = config.coerce_payload(&fields).expect("single line coerces");
    assert!(
        coerced.get("tag").unwrap().as_json().is_object(),
        "coerced inline value is a content object"
    );
}

#[test]
fn richtext_zero_value_is_empty_corpus() {
    let field = FieldSchema::new("x".to_string(), FieldType::RichText { inline: false }, None);
    let zero = zero_value(&field);
    assert!(
        zero.as_json().is_object(),
        "richtext zero-fill is the empty content, not a string: {:?}",
        zero.as_json()
    );
}

// ---------------------------------------------------------------------------
// plaintext — the literal-codec content sibling of richtext
// ---------------------------------------------------------------------------

#[test]
fn plaintext_field_caches_literal_corpus() {
    // A plaintext example is imported verbatim: markdown delimiters stay literal
    // (no marks), and the cached companion is a mark-free content object.
    let config = quill_with_field(
        "    subject:\n      type: plaintext\n      example: \"a *literal* subject\"\n",
    )
    .expect("plaintext example loads");
    let field = config.main.fields.get("subject").unwrap();
    assert_eq!(field.r#type, FieldType::PlainText { inline: false });
    let content = field.example_corpus.as_ref().expect("example_corpus cached");
    let rt = quillmark_content::serial::from_canonical_value(content.as_json()).unwrap();
    assert!(rt.is_plain(), "cached plaintext content is plain");
    assert_eq!(
        quillmark_content::to_plaintext(&rt),
        "a *literal* subject",
        "the asterisks are literal, not emphasis"
    );
}

#[test]
fn plaintext_coercion_imports_verbatim_not_as_markdown() {
    let config = quill_with_field("    subject:\n      type: plaintext\n").expect("loads");
    let mut fields: indexmap::IndexMap<String, QuillValue> = indexmap::IndexMap::new();
    fields.insert(
        "subject".to_string(),
        QuillValue::from_json(serde_json::json!("*not bold* text")),
    );
    let coerced = config.coerce_payload(&fields).expect("plaintext coerces");
    let value = coerced.get("subject").unwrap();
    assert!(value.as_json().is_object(), "coerced value is a content object");
    let rt = quillmark_content::serial::from_canonical_value(value.as_json()).unwrap();
    assert!(rt.marks.is_empty(), "no marks: delimiters stayed literal");
    assert_eq!(quillmark_content::to_plaintext(&rt), "*not bold* text");
}

#[test]
fn inline_plaintext_rejects_multiline_document_value() {
    let config =
        quill_with_field("    subject:\n      type: plaintext\n      inline: true\n").expect("loads");
    let mut fields: indexmap::IndexMap<String, QuillValue> = indexmap::IndexMap::new();
    fields.insert(
        "subject".to_string(),
        QuillValue::from_json(serde_json::json!("line one\n\nline two")),
    );
    let err = config.coerce_payload(&fields).unwrap_err();
    assert!(
        err.to_string().contains("plaintext(inline)"),
        "a multi-line value should fail an inline plaintext field, got: {err}"
    );
}

#[test]
fn plaintext_wire_corpus_with_marks_is_rejected_not_stripped() {
    // A content object carrying a mark is not silently downgraded to plain — it
    // is rejected, mirroring the inline precedent and keeping coercion lossless.
    let config = quill_with_field("    subject:\n      type: plaintext\n").expect("loads");
    let mut rt = quillmark_content::from_markdown("a **bold** word").unwrap();
    rt.normalize();
    let mut fields: indexmap::IndexMap<String, QuillValue> = indexmap::IndexMap::new();
    fields.insert(
        "subject".to_string(),
        QuillValue::from_json(quillmark_content::serial::to_canonical_value(&rt)),
    );
    let err = config.coerce_payload(&fields).unwrap_err();
    assert!(
        err.to_string().contains("plaintext"),
        "a mark-bearing content should fail a plaintext field, got: {err}"
    );
}

#[test]
fn plaintext_transform_schema_carries_media_type_and_plain_annotation() {
    let config = quill_with_field("    subject:\n      type: plaintext\n      inline: true\n")
        .expect("loads");
    let schema = super::schema::build_transform_schema(&config);
    let json = schema.as_json();
    let subject = &json["properties"]["subject"];
    // Same media type as richtext → backends lower it identically, zero edits.
    assert_eq!(subject["type"], "object");
    assert_eq!(
        subject["contentMediaType"],
        super::schema::CONTENT_MEDIA_TYPE
    );
    // Plus the editor-only annotations.
    assert_eq!(subject[super::schema::QUILLMARK_PLAIN_KEY], true);
    assert_eq!(subject[super::schema::QUILLMARK_INLINE_KEY], true);
}

// ---------------------------------------------------------------------------
// enum — promoted to a first-class token
// ---------------------------------------------------------------------------

#[test]
fn enum_type_projects_to_json_schema_string_enum() {
    let config = quill_with_field(
        "    color:\n      type: enum\n      values: [red, green, blue]\n",
    )
    .expect("type: enum loads");
    let field = config.main.fields.get("color").unwrap();
    assert_eq!(field.r#type, FieldType::Enum);
    assert_eq!(
        field.enum_values.as_deref(),
        Some(["red".to_string(), "green".to_string(), "blue".to_string()].as_slice())
    );
    let schema = super::schema::build_transform_schema(&config);
    let color = &schema.as_json()["properties"]["color"];
    // Exactly the shape backends already dispatch on: a string plus its domain.
    assert_eq!(color["type"], "string");
    assert_eq!(color["enum"], serde_json::json!(["red", "green", "blue"]));
}

#[test]
fn enum_requires_a_non_empty_values_list() {
    let err = quill_with_field("    color:\n      type: enum\n").unwrap_err();
    assert!(
        err.iter().any(|d| d
            .message
            .contains("type: enum requires a non-empty values")),
        "type: enum without values should fail load, got: {err:?}"
    );
}

#[test]
fn enum_membership_is_validated_on_a_document_value() {
    let config = quill_with_field("    color:\n      type: enum\n      values: [red, blue]\n")
        .expect("loads");
    let field = config.main.fields.get("color").unwrap();
    let errs = super::validation::validate_field(
        field,
        &QuillValue::from_json(serde_json::json!("green")),
        "color",
    );
    assert!(
        errs.iter().any(|e| e.code() == "validation::enum_violation"),
        "an out-of-domain enum value should raise enum_violation, got: {errs:?}"
    );
    // An in-domain value is accepted.
    let ok = super::validation::validate_field(
        field,
        &QuillValue::from_json(serde_json::json!("red")),
        "color",
    );
    assert!(ok.is_empty(), "an in-domain value validates, got: {ok:?}");
}

#[test]
fn legacy_enum_modifier_on_string_is_still_accepted() {
    // The deprecated spelling loads for one release and populates the same store.
    let config = quill_with_field("    color:\n      type: string\n      enum: [a, b]\n")
        .expect("legacy enum: on string still loads");
    let field = config.main.fields.get("color").unwrap();
    assert_eq!(field.r#type, FieldType::String);
    assert_eq!(
        field.enum_values.as_deref(),
        Some(["a".to_string(), "b".to_string()].as_slice())
    );
}

#[test]
fn enum_or_values_on_a_non_enum_type_is_a_load_error() {
    // The old silent no-op is now loud: enum:/values: on a non-string, non-enum
    // type fails to load.
    let err = quill_with_field("    n:\n      type: integer\n      enum: [1, 2]\n").unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("quill::field_parse_error")),
        "enum: on an integer field should fail to load, got: {err:?}"
    );
    let err = quill_with_field("    s:\n      type: string\n      values: [a, b]\n").unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.code.as_deref() == Some("quill::field_parse_error")),
        "values: on a string field should fail to load, got: {err:?}"
    );
}
