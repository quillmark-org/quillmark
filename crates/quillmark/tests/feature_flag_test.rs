//! Conditional backend registration based on cargo feature flags.

use quillmark::Quillmark;

#[test]
#[cfg(feature = "typst")]
fn test_typst_backend_auto_registered() {
    let engine = Quillmark::new();
    let backends = engine.registered_backends();

    assert!(
        backends.contains(&"typst"),
        "Typst backend should be auto-registered when feature is enabled"
    );
}

#[test]
#[cfg(not(feature = "typst"))]
fn test_typst_backend_not_registered() {
    let engine = Quillmark::new();
    let backends = engine.registered_backends();

    assert!(
        !backends.contains(&"typst"),
        "Typst backend should not be registered when feature is disabled"
    );
    assert_eq!(
        backends.len(),
        0,
        "No backends should be registered when all features are disabled"
    );
}
