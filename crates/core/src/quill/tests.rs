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

/// Test helper: filesystem equivalent of the old `Quill::from_path`.
fn load_from_path<P: AsRef<Path>>(path: P) -> Result<QuillSource, Box<dyn StdError + Send + Sync>> {
    let tree = load_tree(path.as_ref())?;
    QuillSource::from_tree(tree).map_err(|diags| {
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
            "quill:\n  name: \"test\"\n  version: \"1.0\"\n  backend: \"typst\"\n  plate_file: \"plate.typ\"\n  description: \"Test quill\"",
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

    // Test directory listing
    let asset_files = quill.list_directory("assets");
    assert_eq!(asset_files.len(), 1);
    assert!(asset_files.contains(&PathBuf::from("assets/test.txt")));
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
            "quill:\n  name: \"test\"\n  version: \"1.0\"\n  backend: \"typst\"\n  plate_file: \"plate.typ\"\n  description: \"Test quill\"",
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
            "quill:\n  name: \"test\"\n  version: \"1.0\"\n  backend: \"typst\"\n  plate_file: \"plate.typ\"\n  description: \"Test quill\"",
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
  plate_file: custom_plate.typ
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
    assert_eq!(quill.name, "my_custom_quill");

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

    // Test that plate template content is loaded correctly
    assert!(quill.plate.unwrap().contains("Custom Template"));
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
  plate_file: "plate.typ"
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
    let quill = QuillSource::from_tree(root).unwrap();

    // Validate the quill
    assert_eq!(quill.name, "test_from_tree");
    assert_eq!(quill.plate.unwrap(), plate_content);
    assert!(quill.metadata.contains_key("backend"));
    assert!(quill.metadata.contains_key("description"));
}

#[test]
fn test_from_tree_structure_direct() {
    // Test using from_tree_structure directly
    let mut root_files = HashMap::new();

    root_files.insert(
            "Quill.yaml".to_string(),
            FileTreeNode::File {
                contents:
                    b"quill:\n  name: direct_tree\n  version: \"1.0\"\n  backend: typst\n  plate_file: plate.typ\n  description: Direct tree test\n"
                        .to_vec(),
            },
        );

    root_files.insert(
        "plate.typ".to_string(),
        FileTreeNode::File {
            contents: b"plate content".to_vec(),
        },
    );

    // Add a nested directory
    let mut src_files = HashMap::new();
    src_files.insert(
        "main.rs".to_string(),
        FileTreeNode::File {
            contents: b"fn main() {}".to_vec(),
        },
    );

    root_files.insert(
        "src".to_string(),
        FileTreeNode::Directory { files: src_files },
    );

    let root = FileTreeNode::Directory { files: root_files };

    let quill = QuillSource::from_tree(root).unwrap();

    assert_eq!(quill.name, "direct_tree");
    assert!(quill.file_exists("src/main.rs"));
    assert!(quill.file_exists("plate.typ"));
}

#[test]
fn test_dir_exists_and_list_apis() {
    let mut root_files = HashMap::new();

    // Add Quill.yaml
    root_files.insert(
            "Quill.yaml".to_string(),
            FileTreeNode::File {
                contents: b"quill:\n  name: test\n  version: \"1.0\"\n  backend: typst\n  plate_file: plate.typ\n  description: Test quill\n"
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
    let quill = QuillSource::from_tree(root).unwrap();

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
  plate_file: "plate.typ"
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
    let quill = QuillSource::from_tree(root).unwrap();

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
    // Test creating FieldSchema with minimal fields
    let schema1 = FieldSchema::new(
        "test_name".to_string(),
        FieldType::String,
        Some("Test description".to_string()),
    );
    assert_eq!(schema1.description, Some("Test description".to_string()));
    assert_eq!(schema1.r#type, FieldType::String);
    assert_eq!(schema1.example, None);
    assert_eq!(schema1.default, None);

    // Test parsing FieldSchema from YAML with all fields
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
fn test_field_schema_single_example() {
    let yaml_str = r#"
description: "Field schema with single example"
type: "date"
example: "2024-01-15"
"#;
    let quill_value = QuillValue::from_yaml_str(yaml_str).unwrap();
    let schema = FieldSchema::from_quill_value("effective_date".to_string(), &quill_value).unwrap();

    assert_eq!(
        schema.example.as_ref().and_then(|v| v.as_str()),
        Some("2024-01-15")
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
    // Test creating a Quill without specifying a plate file
    let mut root_files = HashMap::new();

    // Add Quill.yaml without plate field
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
    let quill = QuillSource::from_tree(root).unwrap();

    // Validate that plate is null (will use auto plate)
    assert!(quill.plate.clone().is_none());
    assert_eq!(quill.name, "test_no_plate");
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
  plate_file: plate.typ

typst:
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
    assert_eq!(config.plate_file, Some("plate.typ".to_string()));

    // Verify backend-specific config (parsed from the [typst] section).
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

card_types:
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

card_types:
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

card_types:
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
    let quill = QuillSource::from_tree(root).unwrap();

    // Verify metadata includes backend and description
    assert!(quill.metadata.contains_key("backend"));
    assert!(quill.metadata.contains_key("description"));
    assert!(quill.metadata.contains_key("author"));

    // Verify typst config with typst_ prefix
    assert!(quill.metadata.contains_key("typst_packages"));
}

#[test]
fn test_config_defaults() {
    // Test defaults extraction via QuillConfig
    let mut root_files = HashMap::new();

    let quill_yaml = r#"
quill:
  name: metadata_test_yaml
  version: "1.0"
  backend: typst
  description: Test metadata flow
  author: Test Author

typst:
  packages:
    - "@preview/bubble:0.2.2"

main:
  fields:
    author:
      type: string
      default: Anonymous
    status:
      type: string
      default: draft
    title:
      type: string
"#;
    root_files.insert(
        "Quill.yaml".to_string(),
        FileTreeNode::File {
            contents: quill_yaml.as_bytes().to_vec(),
        },
    );

    let root = FileTreeNode::Directory { files: root_files };
    let quill = QuillSource::from_tree(root).unwrap();

    // Extract defaults
    let defaults = quill.config.main.defaults();

    // Verify only fields with defaults are returned
    assert_eq!(defaults.len(), 2);
    assert!(!defaults.contains_key("title")); // no default
    assert!(defaults.contains_key("author"));
    assert!(defaults.contains_key("status"));

    // Verify default values
    assert_eq!(defaults.get("author").unwrap().as_str(), Some("Anonymous"));
    assert_eq!(defaults.get("status").unwrap().as_str(), Some("draft"));
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

card_types:
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

    let card = config.card_type("indorsement").unwrap();
    let card_defaults = card.defaults();
    assert_eq!(card_defaults.len(), 1);
    assert_eq!(
        card_defaults.get("signature_block").unwrap().as_str(),
        Some("Commander")
    );

    let sig_example = card.fields.get("signature_block").unwrap().example.as_ref();
    assert_eq!(sig_example.and_then(|v| v.as_str()), Some("Col Smith"));

    assert!(config.card_type("unknown").is_none());
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

    // Check that fields have correct order based on TOML position
    // Order is automatically generated based on field position

    let first = config.main.fields.get("first").unwrap();
    assert_eq!(first.ui.as_ref().unwrap().order, Some(0));

    let second = config.main.fields.get("second").unwrap();
    assert_eq!(second.ui.as_ref().unwrap().order, Some(1));

    let third = config.main.fields.get("third").unwrap();
    assert_eq!(third.ui.as_ref().unwrap().order, Some(2));
    assert_eq!(
        third.ui.as_ref().unwrap().group,
        Some("Test Group".to_string())
    );

    let fourth = config.main.fields.get("fourth").unwrap();
    assert_eq!(fourth.ui.as_ref().unwrap().order, Some(3));
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
    assert_eq!(ui.order, Some(0)); // First field should have order 0
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
fn test_parse_card_field_type() {
    // Test that FieldSchema no longer supports type = "card" (cards are in CardSchema now)
    let yaml = r#"
type: "string"
description: "A simple string field"
"#;
    let quill_value = QuillValue::from_yaml_str(yaml).unwrap();
    let schema = FieldSchema::from_quill_value("simple_field".to_string(), &quill_value).unwrap();

    assert_eq!(schema.name, "simple_field");
    assert_eq!(schema.r#type, FieldType::String);
    assert_eq!(
        schema.description,
        Some("A simple string field".to_string())
    );
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

card_types:
  endorsements:
    description: Chain of endorsements
    fields:
      name:
        type: string
        description: Name of the endorsing official
        required: true
      org:
        type: string
        description: Endorser's organization
        default: Unknown
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    // Verify the card-type was parsed into config.card_types
    assert!(config.card_type("endorsements").is_some());
    let card = config.card_type("endorsements").unwrap();

    assert_eq!(card.name, "endorsements");
    assert_eq!(card.description, Some("Chain of endorsements".to_string()));

    // Verify card fields
    assert_eq!(card.fields.len(), 2);

    let name_field = card.fields.get("name").unwrap();
    assert_eq!(name_field.r#type, FieldType::String);
    assert!(name_field.required);

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

card_types:
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

    // Check card-type is in config.card_types (not config.main.fields)
    assert!(config.card_type("indorsements").is_some());
    let card = config.card_type("indorsements").unwrap();
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

card_types:
  myscope:
    description: My scope
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let card = config.card_type("myscope").unwrap();
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

card_types:
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
    assert!(config.card_type("conflict").is_some());
}

#[test]
fn test_quill_config_ordering_with_cards() {
    // Test that fields have proper UI ordering (cards no longer have card-level ordering)
    let yaml_content = r#"
quill:
  name: ordering_test
  version: "1.0"
  backend: typst
  description: Test ordering

main:
  fields:
    first:
      type: string
      description: First
    zero:
      type: string
      description: Zero

card_types:
  second:
    description: Second
    fields:
      card_field:
        type: string
        description: A card field
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let first = config.main.fields.get("first").unwrap();
    let zero = config.main.fields.get("zero").unwrap();
    let second = config.card_type("second").unwrap();

    // Check field ordering
    let ord_first = first.ui.as_ref().unwrap().order.unwrap();
    let ord_zero = zero.ui.as_ref().unwrap().order.unwrap();

    // Within fields, "first" is before "zero"
    assert!(ord_first < ord_zero);
    assert_eq!(ord_first, 0);
    assert_eq!(ord_zero, 1);

    // Card fields should also have ordering
    let card_field = second.fields.get("card_field").unwrap();
    let ord_card_field = card_field.ui.as_ref().unwrap().order.unwrap();
    assert_eq!(ord_card_field, 0); // First (and only) field in this card
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

card_types:
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
    let card = config.card_type("mycard").unwrap();

    let z_first = card.fields.get("z_first").unwrap();
    let a_second = card.fields.get("a_second").unwrap();

    // Check orders
    let z_order = z_first.ui.as_ref().unwrap().order.unwrap();
    let a_order = a_second.ui.as_ref().unwrap().order.unwrap();

    // If strict file order is preserved:
    // z_first should be 0, a_second should be 1
    assert_eq!(z_order, 0, "z_first should be 0 (defined first)");
    assert_eq!(a_order, 1, "a_second should be 1 (defined second)");
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
    assert!(list_field.properties.is_some());

    let props = list_field.properties.as_ref().unwrap();
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
          required: true
        city:
          type: string
          required: true
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
      properties:
        name:
          type: string
        value:
          type: number
        active:
          type: boolean
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "scores".to_string(),
        crate::value::QuillValue::from_json(serde_json::json!([
            {"name": "Math", "value": "95", "active": "true"},
            {"name": "Science", "value": "88.5", "active": "false"}
        ])),
    );

    let coerced = config.coerce_frontmatter(&frontmatter).unwrap();
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
      type: date
    created_at:
      type: datetime
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("42")),
    );
    frontmatter.insert(
        "active".to_string(),
        QuillValue::from_json(serde_json::json!("true")),
    );
    frontmatter.insert(
        "signed_on".to_string(),
        QuillValue::from_json(serde_json::json!("2026-04-13")),
    );
    frontmatter.insert(
        "created_at".to_string(),
        QuillValue::from_json(serde_json::json!("2026-04-13T20:00:00Z")),
    );

    let coerced = config.coerce_frontmatter(&frontmatter).unwrap();
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
    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("42")),
    );

    let coerced = config.coerce_frontmatter(&frontmatter).unwrap();
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
    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("42.5")),
    );

    let error = config.coerce_frontmatter(&frontmatter).unwrap_err();
    assert!(matches!(
        error,
        super::CoercionError::Uncoercible { ref path, ref target, .. }
        if path == "count" && target == "integer"
    ));
}

#[test]
fn test_config_coerce_array_item_wise() {
    let yaml_content = r#"
quill:
  name: coerce_array_items_test
  version: "1.0"
  backend: typst
  description: Coerce arrays

main:
  fields:
    items:
      type: array
      properties:
        score:
          type: number
        active:
          type: boolean
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "items".to_string(),
        QuillValue::from_json(serde_json::json!([
            {"score": "90", "active": "true"},
            {"score": "87.5", "active": "false"}
        ])),
    );

    let coerced = config.coerce_frontmatter(&frontmatter).unwrap();
    let items = coerced.get("items").unwrap().as_array().unwrap();
    let first = items[0].as_object().unwrap();
    let second = items[1].as_object().unwrap();
    assert_eq!(first["score"], serde_json::json!(90));
    assert_eq!(first["active"], serde_json::json!(true));
    assert_eq!(second["score"], serde_json::json!(87.5));
    assert_eq!(second["active"], serde_json::json!(false));
}

#[test]
fn test_config_coerce_cards_item_wise() {
    let yaml_content = r#"
quill:
  name: coerce_cards_items_test
  version: "1.0"
  backend: typst
  description: Coerce cards

card_types:
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
      type: date
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();
    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "signed_on".to_string(),
        QuillValue::from_json(serde_json::json!("13-04-2026")),
    );

    let error = config.coerce_frontmatter(&frontmatter).unwrap_err();
    assert!(matches!(
        error,
        super::CoercionError::Uncoercible { ref path, ref target, .. }
        if path == "signed_on" && target == "date"
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
    let mut frontmatter = indexmap::IndexMap::new();
    frontmatter.insert(
        "count".to_string(),
        QuillValue::from_json(serde_json::json!("forty-two")),
    );

    let error = config.coerce_frontmatter(&frontmatter).unwrap_err();
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
      type: markdown
      description: Document summary
      ui:
        multiline: true
    notes:
      type: markdown
      description: Short notes
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let summary = config.main.fields.get("summary").unwrap();
    assert_eq!(summary.r#type, FieldType::Markdown);
    assert_eq!(summary.ui.as_ref().unwrap().multiline, Some(true));

    let notes = config.main.fields.get("notes").unwrap();
    assert_eq!(notes.r#type, FieldType::Markdown);
    assert_eq!(notes.ui.as_ref().unwrap().multiline, None);
}

#[test]
fn test_multiline_ui_field_on_string_type() {
    let yaml_content = r#"
quill:
  name: multiline_string_test
  version: "1.0"
  backend: typst
  description: Test multiline ui hint on string field

main:
  fields:
    address:
      type: string
      description: Mailing address
      ui:
        multiline: true
    name:
      type: string
      description: Full name
"#;

    let config = QuillConfig::from_yaml(yaml_content).unwrap();

    let address = config.main.fields.get("address").unwrap();
    assert_eq!(address.r#type, FieldType::String);
    assert_eq!(address.ui.as_ref().unwrap().multiline, Some(true));

    let name = config.main.fields.get("name").unwrap();
    assert_eq!(name.r#type, FieldType::String);
    assert!(name.ui.as_ref().map_or(true, |ui| ui.multiline.is_none()));
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

card_types:
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
    let indorsement = config.card_type("indorsement").unwrap();
    assert_eq!(
        indorsement.ui.as_ref().unwrap().title.as_deref(),
        Some("{from} → {for}"),
        "template card ui.title carried verbatim"
    );

    let schema = config.schema();
    assert_eq!(schema["main"]["ui"]["title"].as_str(), Some("Memorandum"));
    assert_eq!(
        schema["card_types"]["indorsement"]["ui"]["title"].as_str(),
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
    assert!(config
        .main
        .ui
        .as_ref()
        .map_or(true, |ui| ui.title.is_none()));
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
    // Typos like 'platefile' should fail loudly, not silently land in metadata.
    let yaml_content = r#"
quill:
  name: unk_key
  version: "1.0"
  backend: typst
  description: Unknown key test
  platefile: foo.typ
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code.as_deref(), Some("quill::unknown_key"));
    assert!(err[0].message.contains("platefile"));
    assert!(err[0].hint.as_deref().unwrap_or("").contains("plate_file"));
}

#[test]
fn test_unknown_top_level_section_errors() {
    // 'card_type' is a common typo for 'card_types'. Must not be silently ignored.
    let yaml_content = r#"
quill:
  name: unk_section
  version: "1.0"
  backend: typst
  description: Unknown section test

card_type:
  foo:
    description: Should not silently disappear
    fields:
      bar:
        type: string
"#;

    let err = QuillConfig::from_yaml_with_warnings(yaml_content).unwrap_err();

    assert!(err.iter().any(|d| {
        d.code.as_deref() == Some("quill::unknown_section") && d.message.contains("card_type")
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
    let quill = load_from_path(quillmark_fixtures::resource_path("quills/usaf_memo/0.1.0"))
        .expect("load usaf_memo fixture");
    let yaml = yaml_of(&quill.config);
    let golden_path =
        quillmark_fixtures::resource_path(&format!("quills/usaf_memo/0.1.0/__golden__/{golden}"));

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
    assert!(parsed.get("card_types").is_some());
    assert!(parsed.get("ref").is_none() && parsed.get("example").is_none());
    assert!(yaml.contains("ui:"), "{golden} must include ui hints");
}

#[test]
fn schema_snapshot_usaf_memo_0_1_0() {
    check_schema_snapshot(|c| c.schema_yaml().unwrap(), |c| c.schema(), "schema.yaml");
}

#[test]
fn body_example_with_body_disabled_emits_warning() {
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  skills:
    body:
      enabled: false
      example: This example is unused
    fields:
      items: { type: array, required: true }
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
fn body_example_with_bare_fence_line_is_an_error() {
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Opening paragraph.\n\n---\n\nClosing paragraph."
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
fn body_example_fence_line_with_leading_spaces_is_an_error() {
    // Up to 3 leading spaces still counts as a fence marker.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "text\n   ---\nmore text"
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(result.is_err());
}

#[test]
fn body_example_four_leading_spaces_is_not_a_fence() {
    // Four leading spaces = indented code block, not a fence marker.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "text\n    ---\nmore text"
  fields:
    title: { type: string }
"#;
    let result = QuillConfig::from_yaml_with_warnings(yaml);
    assert!(
        result.is_ok(),
        "four-space indented --- should not trigger fence error"
    );
}

#[test]
fn body_example_card_fence_line_is_an_error() {
    // A ```card <kind> opener in a body example would be parsed as a card.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  body:
    example: "Intro.\n\n```card note\nauthor: x\n```"
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
        "expected body_example_contains_fence error for card fence, got: {:?}",
        errors
    );
}

#[test]
fn body_example_card_type_fence_line_is_an_error() {
    // The fence check applies to card-type body examples too.
    let yaml = r#"
quill: { name: x, version: 1.0.0, backend: typst, description: x }
main:
  fields:
    title: { type: string }
card_types:
  note:
    body:
      example: "See below:\n---\nEnd."
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
        "expected body_example_contains_fence error for card type, got: {:?}",
        errors
    );
}
