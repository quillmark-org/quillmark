//! #990 region coverage on the `cmu_letter` quill's date.
//!
//! Like the USAF memo, `cmu_letter` places the letter date inside its vendored
//! package (`utils.typ`'s `display-date`), and falls back to a native
//! `datetime.today()` when the field is blank. This pins that a real date
//! surfaces a clickable `date` region through the value-object's generated
//! `display` closure, and that the blank-date `today()` fallback still renders
//! (its native `datetime` takes the shim's method-sugar branch).

#![cfg(feature = "typst")]

use quillmark::Quillmark;
use quillmark_fixtures::quills_path;

#[test]
fn cmu_letter_real_date_surfaces_a_region_and_blank_falls_back() {
    let engine = Quillmark::new();
    let quill =
        quillmark::quill_from_path(quills_path("cmu_letter")).expect("cmu_letter should load");
    let parsed = quill.seed_document();
    let mut session = engine.open(&quill, &parsed).expect("open a session");

    // Blank date → native `today()` fallback: it still renders (the shim keeps
    // native method sugar), and being native plate ink it exposes no field
    // region — there is no schema value to click.
    assert!(
        !session.regions().iter().any(|r| r.field == "date"),
        "a blank date falls back to today() and surfaces no `date` region"
    );

    // Commit a real date; it renders through the same vendored `display-date`,
    // and the value-object's closure carries the region to the schema path.
    let mut edited = quill.compile_data(&parsed).expect("compile seed data");
    edited["date"] = serde_json::json!("2026-01-02");
    session.apply(&edited).expect("apply a real letter date");

    let regions = session.regions();
    let date = regions
        .iter()
        .find(|r| r.field == "date")
        .unwrap_or_else(|| panic!("a real letter date must surface a `date` region: {regions:?}"));
    let cx = (date.rect[0] + date.rect[2]) / 2.0;
    let cy = (date.rect[1] + date.rect[3]) / 2.0;
    assert_eq!(
        session.field_at(date.page, cx, cy).as_deref(),
        Some("date"),
        "a click on the vendored-placed letter date routes to its schema path"
    );
}
