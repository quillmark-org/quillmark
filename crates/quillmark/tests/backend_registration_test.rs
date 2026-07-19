//! # Backend Registration Tests

use quillmark::{Document, OutputFormat, Quill, Quillmark, RenderError};
use quillmark_core::{session::SessionHandle, Artifact, Backend, RenderOptions, RenderResult};
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
        source: &Quill,
        _json_data: &serde_json::Value,
    ) -> Result<quillmark::LiveSession, RenderError> {
        // Like the real backends, the mock reads its own input from the quill's
        // files — here a `plate.txt` it echoes back as the rendered bytes.
        let plated = source
            .files()
            .get_file("plate.txt")
            .map(|b| b.to_vec())
            .unwrap_or_default();
        Ok(quillmark::LiveSession::new(Box::new(MockSession {
            bytes: plated,
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
        "quill:\n  name: \"custom_backend_quill\"\n  version: \"1.0\"\n  backend: \"mock-txt\"\n  description: \"Test\"\n",
    ).unwrap();
    fs::write(quill_path.join("plate.txt"), "Test template: {{ title }}").unwrap();

    let quill = quillmark::quill_from_path(&quill_path).expect("Quill::from_path failed");

    assert_eq!(quill.backend_id(), "mock-txt");
    assert_eq!(quill.name(), "custom_backend_quill");
    assert!(engine
        .supported_formats(&quill)
        .expect("supported_formats failed")
        .contains(&OutputFormat::Txt));

    let markdown =
        "~~~card-yaml\n$quill: custom_backend_quill\n$kind: main\ntitle: Hello Custom Backend\n~~~\n\n# Test\n";
    let parsed = Document::parse(markdown).expect("parse failed").document;
    let result = engine
        .render(
            &quill,
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
