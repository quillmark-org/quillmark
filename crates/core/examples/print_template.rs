//! Prints the auto-generated Markdown template for a quill fixture.
//!
//! Usage:
//!   cargo run -p quillmark-core --example print_template
//!   cargo run -p quillmark-core --example print_template -- classic_resume
//!   cargo run -p quillmark-core --example print_template -- usaf_memo 0.1.0

use quillmark_core::quill::QuillConfig;
use quillmark_fixtures::quills_path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let quill_name = args.first().map(|s| s.as_str()).unwrap_or("usaf_memo");

    let quill_dir = if let Some(version) = args.get(1) {
        quills_path(quill_name).parent().unwrap().join(version)
    } else {
        quills_path(quill_name)
    };

    let yaml_path = quill_dir.join("Quill.yaml");
    let yaml = std::fs::read_to_string(&yaml_path)
        .unwrap_or_else(|e| panic!("could not read {}: {}", yaml_path.display(), e));

    let cfg = QuillConfig::from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("could not parse {}: {}", yaml_path.display(), e));

    print!("{}", cfg.template());
}
