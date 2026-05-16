//! # Backend Registration Tests

use quillmark::{Document, OutputFormat, Quillmark, RenderError};
use quillmark_core::{
    session::SessionHandle, Artifact, Backend, QuillSource, RenderOptions, RenderResult,
};
use std::fs;
use tempfile::TempDir;

#[derive(Debug)]
struct MockBackend {
    id: &'static str,
}

impl Backend for MockBackend {
    fn id(&self) -> &'static str {
        self.id
    }

    fn supported_formats(&self) -> &'static [OutputFormat] {
        &[OutputFormat::Txt]
    }

    fn open(
        &self,
        main: &str,
        _source: &QuillSource,
        _json_data: &serde_json::Value,
    ) -> Result<quillmark::RenderSession, RenderError> {
        Ok(quillmark::RenderSession::new(Box::new(MockSession {
            bytes: main.as_bytes().to_vec(),
        })))
    }
}

#[derive(Debug)]
struct MockSession {
    bytes: Vec<u8>,
}

impl SessionHandle for MockSession {
    fn render(&self, _opts: &RenderOptions) -> Result<RenderResult, RenderError> {
        let artifacts = vec![Artifact {
            bytes: self.bytes.clone(),
            output_format: OutputFormat::Txt,
        }];
        Ok(RenderResult::new(artifacts, OutputFormat::Txt))
    }

    fn page_count(&self) -> usize {
        1
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_register_backend_basic() {
    let mut engine = Quillmark::new();
    engine.register_backend(Box::new(MockBackend { id: "mock" }));
    let backends = engine.registered_backends();
    assert!(backends.contains(&"mock"));
}

#[test]
fn test_register_multiple_backends() {
    let mut engine = Quillmark::new();
    engine.register_backend(Box::new(MockBackend { id: "mock1" }));
    engine.register_backend(Box::new(MockBackend { id: "mock2" }));
    let backends = engine.registered_backends();
    assert!(backends.contains(&"mock1"));
    assert!(backends.contains(&"mock2"));
}

#[test]
fn test_register_backend_replaces_existing() {
    let mut engine = Quillmark::new();
    engine.register_backend(Box::new(MockBackend { id: "custom" }));
    engine.register_backend(Box::new(MockBackend { id: "custom" }));
    let backends = engine.registered_backends();
    assert_eq!(backends.iter().filter(|&&b| b == "custom").count(), 1);
}

#[test]
fn test_render_with_custom_backend() {
    let mut engine = Quillmark::new();
    engine.register_backend(Box::new(MockBackend { id: "mock-txt" }));

    let temp_dir = TempDir::new().unwrap();
    let quill_path = temp_dir.path().join("test_quill");
    fs::create_dir_all(&quill_path).unwrap();
    fs::write(
        quill_path.join("Quill.yaml"),
        "quill:\n  name: \"custom_backend_quill\"\n  version: \"1.0\"\n  backend: \"mock-txt\"\n  main_file: \"main.txt\"\n  description: \"Test\"\n",
    ).unwrap();
    fs::write(quill_path.join("main.txt"), "Test template: {{ title }}").unwrap();

    let quill = engine
        .quill_from_path(&quill_path)
        .expect("quill_from_path failed");

    assert_eq!(quill.backend_id(), "mock-txt");
    assert_eq!(quill.name(), "custom_backend_quill");
    assert!(quill.supported_formats().contains(&OutputFormat::Txt));

    let markdown = "---\nQUILL: custom_backend_quill\ntitle: Hello Custom Backend\n---\n\n# Test\n";
    let parsed = Document::from_markdown(markdown).expect("parse failed");
    let result = quill
        .render(
            &parsed,
            &RenderOptions {
                output_format: Some(OutputFormat::Txt),
                ..Default::default()
            },
        )
        .expect("render failed");

    assert!(!result.artifacts.is_empty());
    assert_eq!(result.artifacts[0].output_format, OutputFormat::Txt);
}

#[test]
fn test_register_backend_after_new() {
    let mut engine = Quillmark::new();
    let initial_count = engine.registered_backends().len();
    engine.register_backend(Box::new(MockBackend { id: "added-later" }));
    let backends = engine.registered_backends();
    assert_eq!(backends.len(), initial_count + 1);
    assert!(backends.contains(&"added-later"));
}
