use std::collections::HashMap;
use std::path::Path;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{package::PackageSpec, FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, World};

use crate::helper;
use quillmark_core::QuillSource;

static FALLBACK_REGULAR: &[u8] = include_bytes!("fonts/Figtree-Regular.ttf");
static FALLBACK_BOLD: &[u8] = include_bytes!("fonts/Figtree-Bold.ttf");
static FALLBACK_ITALIC: &[u8] = include_bytes!("fonts/Figtree-Italic.ttf");

/// Typst World implementation for quill-based compilation.
///
/// Implements the Typst `World` trait to provide dynamic package loading,
/// virtual path handling, and asset management for quill templates.
/// Packages are loaded from `{quill}/packages/` and assets from `{quill}/assets/`.
pub struct QuillWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>, // For fonts loaded from assets
    source: Source,
    sources: HashMap<FileId, Source>,
    binaries: HashMap<FileId, Bytes>,
}

impl QuillWorld {
    /// Create a new QuillWorld from a quill template and Typst content
    pub fn new(
        source: &QuillSource,
        main: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut sources = HashMap::new();
        let mut binaries = HashMap::new();

        // Create a new empty FontBook to ensure proper ordering
        let mut book = FontBook::new();
        let mut fonts = Vec::new();

        // Load fonts from quill assets (eagerly loaded).
        let font_data_list = Self::load_fonts_from_quill(source)?;
        for font_data in font_data_list {
            let font_bytes = Bytes::new(font_data);
            for font in Font::iter(font_bytes) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }

        // Fall back to the embedded Figtree faces when the quill ships no fonts.
        if fonts.is_empty() {
            for data in [FALLBACK_REGULAR, FALLBACK_BOLD, FALLBACK_ITALIC] {
                let font_bytes = Bytes::new(data.to_vec());
                for font in Font::iter(font_bytes) {
                    book.push(font.info().clone());
                    fonts.push(font);
                }
            }
        }

        // Load assets from quill's in-memory file system
        Self::load_assets_from_quill(source, &mut binaries)?;

        // Load packages from quill's in-memory file system. Quillmark does
        // not download external packages — every package a quill imports
        // must be vendored under `packages/` in the quill tree.
        Self::load_packages_from_quill(source, &mut sources, &mut binaries)?;

        // Create main source
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let source = Source::new(main_id, main.to_string());

        Ok(Self {
            library: LazyHash::new(<Library as typst::LibraryExt>::default()),
            book: LazyHash::new(book),
            fonts,
            source,
            sources,
            binaries,
        })
    }

    /// Create a new QuillWorld with JSON data injected as a helper package.
    ///
    /// This method creates a virtual `@local/quillmark-helper:0.1.0` package
    /// containing the JSON data and helper functions. The main file can
    /// import this package to access document data.
    ///
    /// # Arguments
    ///
    /// * `quill` - The quill template
    /// * `main` - The main Typst content to compile
    /// * `json_data` - JSON string containing document data
    pub fn new_with_data(
        source: &QuillSource,
        main: &str,
        json_data: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut world = Self::new(source, main)?;

        // Inject the quillmark-helper package
        world.inject_helper_package(json_data);

        Ok(world)
    }

    /// Inject the quillmark-helper package with JSON data.
    fn inject_helper_package(&mut self, json_data: &str) {
        // Create the package spec
        let spec = PackageSpec {
            namespace: helper::HELPER_NAMESPACE.into(),
            name: helper::HELPER_NAME.into(),
            version: helper::HELPER_VERSION
                .parse()
                .expect("Invalid helper version"),
        };

        // Generate and inject lib.typ
        let lib_content = helper::generate_lib_typ(json_data);
        let lib_path = VirtualPath::new("lib.typ");
        let lib_id = FileId::new(Some(spec.clone()), lib_path);
        self.sources
            .insert(lib_id, Source::new(lib_id, lib_content));

        // Generate and inject typst.toml (as binary)
        let toml_content = helper::generate_typst_toml();
        let toml_path = VirtualPath::new("typst.toml");
        let toml_id = FileId::new(Some(spec), toml_path);
        self.binaries
            .insert(toml_id, Bytes::new(toml_content.into_bytes()));
    }

    /// Loads fonts from quill's in-memory file system.
    fn load_fonts_from_quill(
        source: &QuillSource,
    ) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let mut font_data = Vec::new();

        // Look for fonts in assets/fonts/ first
        let fonts_paths = source.find_files("assets/fonts/*");
        for font_path in fonts_paths {
            if let Some(ext) = font_path.extension() {
                if matches!(
                    ext.to_string_lossy().to_lowercase().as_str(),
                    "ttf" | "otf" | "woff" | "woff2"
                ) {
                    if let Some(contents) = source.get_file(&font_path) {
                        font_data.push(contents.to_vec());
                    }
                }
            }
        }

        // Also look in packages/*/fonts/ for package fonts
        let package_font_paths = source.find_files("packages/**");
        for font_path in package_font_paths {
            if let Some(ext) = font_path.extension() {
                if matches!(
                    ext.to_string_lossy().to_lowercase().as_str(),
                    "ttf" | "otf" | "woff" | "woff2"
                ) {
                    if let Some(contents) = source.get_file(&font_path) {
                        font_data.push(contents.to_vec());
                    }
                }
            }
        }

        Ok(font_data)
    }

    /// Loads assets from quill's in-memory file system.
    fn load_assets_from_quill(
        source: &QuillSource,
        binaries: &mut HashMap<FileId, Bytes>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get all files that start with "assets/"
        let asset_paths = source.find_files("assets/*");

        for asset_path in asset_paths {
            if let Some(contents) = source.get_file(&asset_path) {
                // Create virtual path for the asset
                let virtual_path = VirtualPath::new(asset_path.to_string_lossy().as_ref());
                let file_id = FileId::new(None, virtual_path);
                binaries.insert(file_id, Bytes::new(contents.to_vec()));
            }
        }

        Ok(())
    }

    /// Loads packages from quill's in-memory file system.
    fn load_packages_from_quill(
        source: &QuillSource,
        sources: &mut HashMap<FileId, Source>,
        binaries: &mut HashMap<FileId, Bytes>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get all subdirectories in packages/
        let package_dirs = source.list_directories("packages");

        for package_dir in package_dirs {
            let package_name = package_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Look for typst.toml in this package
            let toml_path = package_dir.join("typst.toml");
            if let Some(toml_contents) = source.get_file(&toml_path) {
                let toml_content = String::from_utf8_lossy(toml_contents);
                match parse_package_toml(&toml_content) {
                    Ok(package_info) => {
                        let spec = PackageSpec {
                            namespace: package_info.namespace.clone().into(),
                            name: package_info.name.clone().into(),
                            version: package_info.version.parse().map_err(|_| {
                                format!("Invalid version format: {}", package_info.version)
                            })?,
                        };

                        // Load the package files with entrypoint awareness
                        Self::load_package_files_from_quill(
                            source,
                            &package_dir,
                            sources,
                            binaries,
                            Some(spec),
                            Some(&package_info.entrypoint),
                        )?;
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to parse typst.toml for {}: {}",
                            package_name, e
                        );
                        // Continue with other packages
                    }
                }
            } else {
                // Load as a simple package directory without typst.toml
                let spec = PackageSpec {
                    namespace: "local".into(),
                    name: package_name.into(),
                    version: "0.1.0".parse().map_err(|_| "Invalid version format")?,
                };

                Self::load_package_files_from_quill(
                    source,
                    &package_dir,
                    sources,
                    binaries,
                    Some(spec),
                    None,
                )?;
            }
        }

        Ok(())
    }

    /// Loads files from a package directory in quill's in-memory file system.
    fn load_package_files_from_quill(
        source: &QuillSource,
        package_dir: &Path,
        sources: &mut HashMap<FileId, Source>,
        binaries: &mut HashMap<FileId, Bytes>,
        package_spec: Option<PackageSpec>,
        entrypoint: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Find all files in the package directory
        let package_pattern = format!("{}/*", package_dir.to_string_lossy());
        let package_files = source.find_files(&package_pattern);

        for file_path in package_files {
            if let Some(contents) = source.get_file(&file_path) {
                // Calculate the relative path within the package
                let relative_path = file_path.strip_prefix(package_dir).map_err(|_| {
                    format!("Failed to get relative path for {}", file_path.display())
                })?;

                let virtual_path = VirtualPath::new(relative_path.to_string_lossy().as_ref());
                let file_id = FileId::new(package_spec.clone(), virtual_path);

                // Check if this is a source file (.typ) or binary
                if let Some(ext) = file_path.extension() {
                    if ext == "typ" {
                        let source_content = String::from_utf8_lossy(contents);
                        let source = Source::new(file_id, source_content.to_string());
                        sources.insert(file_id, source);
                    } else {
                        binaries.insert(file_id, Bytes::new(contents.to_vec()));
                    }
                } else {
                    // No extension, treat as binary
                    binaries.insert(file_id, Bytes::new(contents.to_vec()));
                }
            }
        }

        // Verify entrypoint if specified
        if let (Some(spec), Some(entrypoint_name)) = (&package_spec, entrypoint) {
            let entrypoint_path = VirtualPath::new(entrypoint_name);
            let entrypoint_file_id = FileId::new(Some(spec.clone()), entrypoint_path);

            if !sources.contains_key(&entrypoint_file_id) {
                eprintln!(
                    "Warning: Entrypoint {} not found for package {}",
                    entrypoint_name, spec.name
                );
            }
        }

        Ok(())
    }
}

impl World for QuillWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else if let Some(source) = self.sources.get(&id) {
            Ok(source.clone())
        } else {
            Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            ))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(bytes) = self.binaries.get(&id) {
            Ok(bytes.clone())
        } else {
            Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            ))
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        // First check if we have an asset font at this index
        if let Some(font) = self.fonts.get(index) {
            return Some(font.clone());
        }

        None
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        // On native targets we can use the system clock. On wasm32 we call into
        // the JavaScript Date API via js-sys to get UTC date components.
        #[cfg(not(target_arch = "wasm32"))]
        {
            use time::{Duration, OffsetDateTime};

            // Get current UTC time and apply optional hour offset
            let now = OffsetDateTime::now_utc();
            let adjusted = if let Some(hours) = offset {
                now + Duration::hours(hours)
            } else {
                now
            };

            let date = adjusted.date();
            Datetime::from_ymd(date.year(), date.month() as u8, date.day())
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Use js-sys to access the JS Date methods. This returns components in
            // UTC using getUTCFullYear/getUTCMonth/getUTCDate.
            use js_sys::Date;
            use wasm_bindgen::JsValue;

            let d = Date::new_0();
            // get_utc_full_year returns f64
            let year = d.get_utc_full_year() as i32;
            // get_utc_month returns 0-based month
            let month = (d.get_utc_month() as u8).saturating_add(1);
            let day = d.get_utc_date() as u8;

            // Apply hour offset if requested by constructing a JS Date with hours
            if let Some(hours) = offset {
                // Create a new Date representing now + offset hours
                let millis = d.get_time() + (hours as f64) * 3_600_000.0;
                let d2 = Date::new(&JsValue::from_f64(millis));
                let year = d2.get_utc_full_year() as i32;
                let month = (d2.get_utc_month() as u8).saturating_add(1);
                let day = d2.get_utc_date() as u8;
                return Datetime::from_ymd(year, month, day);
            }

            Datetime::from_ymd(year, month, day)
        }
    }
}

/// Simplified package info structure with entrypoint support
#[derive(Debug, Clone)]
struct PackageInfo {
    namespace: String,
    name: String,
    version: String,
    entrypoint: String,
}

/// Parse a typst.toml for package information with better error handling
fn parse_package_toml(
    content: &str,
) -> Result<PackageInfo, Box<dyn std::error::Error + Send + Sync>> {
    let value: toml::Value = toml::from_str(content)?;

    let package_section = value
        .get("package")
        .ok_or("Missing [package] section in typst.toml")?;

    let namespace = package_section
        .get("namespace")
        .and_then(|v| v.as_str())
        .unwrap_or("local")
        .to_string();

    let name = package_section
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("Package name is required in typst.toml")?
        .to_string();

    let version = package_section
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();

    let entrypoint = package_section
        .get("entrypoint")
        .and_then(|v| v.as_str())
        .unwrap_or("lib.typ")
        .to_string();

    Ok(PackageInfo {
        namespace,
        name,
        version,
        entrypoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_toml() {
        let toml_content = r#"
[package]
name = "test-package"
version = "1.0.0"
namespace = "preview"
entrypoint = "src/lib.typ"
"#;

        let package_info = parse_package_toml(toml_content).unwrap();
        assert_eq!(package_info.name, "test-package");
        assert_eq!(package_info.version, "1.0.0");
        assert_eq!(package_info.namespace, "preview");
        assert_eq!(package_info.entrypoint, "src/lib.typ");
    }

    #[test]
    fn test_parse_package_toml_defaults() {
        let toml_content = r#"
[package]
name = "minimal-package"
"#;

        let package_info = parse_package_toml(toml_content).unwrap();
        assert_eq!(package_info.name, "minimal-package");
        assert_eq!(package_info.version, "0.1.0");
        assert_eq!(package_info.namespace, "local");
        assert_eq!(package_info.entrypoint, "lib.typ");
    }

    #[test]
    fn test_asset_fonts_have_priority() {
        use std::collections::HashMap;
        use std::fs;
        use std::path::{Path, PathBuf};

        use quillmark_core::{FileTreeNode, QuillSource};

        fn walk(dir: &Path, base: &Path) -> std::io::Result<FileTreeNode> {
            let mut files = HashMap::new();
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let p: PathBuf = entry.path();
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if p.is_file() {
                    files.insert(
                        name,
                        FileTreeNode::File {
                            contents: fs::read(&p)?,
                        },
                    );
                } else if p.is_dir() {
                    files.insert(name, walk(&p, base)?);
                }
            }
            Ok(FileTreeNode::Directory { files })
        }

        // Use the actual usaf_memo fixture which has real fonts
        let quill_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("resources")
            .join("quills")
            .join("usaf_memo")
            .join("0.1.0");

        if !quill_path.exists() {
            // Skip test if fixture not found
            return;
        }

        let tree = walk(&quill_path, &quill_path).expect("walk fixture");
        let source = QuillSource::from_tree(tree).expect("load source");
        let world = QuillWorld::new(&source, "// Test").unwrap();

        // Asset fonts should be loaded
        assert!(!world.fonts.is_empty(), "Should have asset fonts loaded");

        // The first fonts in the book should be the asset fonts
        // Verify that indices 0..asset_count return asset fonts from the fonts vec
        for i in 0..world.fonts.len() {
            let font = world.font(i);
            assert!(font.is_some(), "Font at index {} should be available", i);
            // This font should come from the asset fonts (world.fonts vec), not font_slots
        }
    }
}
