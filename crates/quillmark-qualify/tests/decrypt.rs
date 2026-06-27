//! Decrypt: build a form with lopdf, encrypt it (RC4 V1), save it so the bytes
//! are a genuinely encrypted PDF, then `qualify(&encrypted, Some(pw))` and
//! assert success — and that the stripped output carries no `/Encrypt`.
//!
//! Note on lopdf 0.36 reader behaviour: `Document::load_mem` *auto-decrypts* a
//! PDF protected by an empty user password (dropping `/Encrypt` at load). To
//! genuinely exercise `qualify`'s explicit `doc.decrypt(password)` branch we use
//! a NON-empty user password, which `load_mem` leaves encrypted; `qualify` then
//! decrypts it. The empty-password case is covered too (it flows through the
//! auto-decrypted-at-load path).
//!
//! Also asserts a non-encrypted input qualifies cleanly through the same path.

use lopdf::{
    dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, StringFormat,
};
use quillmark_pdfform::FormSpec;
use quillmark_qualify::qualify;

fn pdf_str(s: &str) -> Object {
    Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
}

/// A minimal one-page AcroForm PDF with a single text field, traditional xref.
fn build_form() -> Document {
    let mut doc = Document::with_version("1.7");

    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let field_id = doc.new_object_id();

    doc.set_object(
        field_id,
        dictionary! {
            "Type" => Object::Name(b"Annot".to_vec()),
            "Subtype" => Object::Name(b"Widget".to_vec()),
            "FT" => Object::Name(b"Tx".to_vec()),
            "T" => pdf_str("FullName"),
            "TU" => pdf_str("Full legal name"),
            "Rect" => Object::Array(vec![
                Object::Integer(100), Object::Integer(700),
                Object::Integer(400), Object::Integer(720),
            ]),
            "P" => Object::Reference(page_id),
        },
    );

    let acroform_id = doc.add_object(dictionary! {
        "Fields" => Object::Array(vec![Object::Reference(field_id)]),
        "NeedAppearances" => Object::Boolean(true),
    });

    doc.set_object(
        page_id,
        dictionary! {
            "Type" => Object::Name(b"Page".to_vec()),
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => Object::Array(vec![
                Object::Integer(0), Object::Integer(0),
                Object::Integer(612), Object::Integer(792),
            ]),
            "Annots" => Object::Array(vec![Object::Reference(field_id)]),
        },
    );

    doc.set_object(
        pages_id,
        dictionary! {
            "Type" => Object::Name(b"Pages".to_vec()),
            "Kids" => Object::Array(vec![Object::Reference(page_id)]),
            "Count" => Object::Integer(1),
        },
    );

    let catalog_id = doc.add_object(dictionary! {
        "Type" => Object::Name(b"Catalog".to_vec()),
        "Pages" => Object::Reference(pages_id),
        "AcroForm" => Object::Reference(acroform_id),
    });

    doc.trailer.set("Root", Object::Reference(catalog_id));
    // A document /ID is required by the encryption key derivation.
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal),
            Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal),
        ]),
    );
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    doc
}

fn save(doc: &mut Document) -> Vec<u8> {
    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save");
    buf
}

/// Build the form and encrypt it with RC4 V1 under the given passwords.
fn build_encrypted(owner: &str, user: &str) -> Vec<u8> {
    let mut doc = build_form();
    let version = EncryptionVersion::V1 {
        document: &doc,
        owner_password: owner,
        user_password: user,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).expect("build encryption state");
    doc.encrypt(&state).expect("encrypt");
    save(&mut doc)
}

/// Assert the qualified output reconstructs the field and is unencrypted.
fn assert_qualified_ok(qualified: &quillmark_qualify::Qualified) {
    let spec = FormSpec::parse(&qualified.form_json).expect("parse form.json");
    assert_eq!(spec.fields.len(), 1);
    assert_eq!(spec.fields[0].name, "FullName");
    assert_eq!(spec.fields[0].tooltip.as_deref(), Some("Full legal name"));

    let out = Document::load_mem(&qualified.form_pdf).expect("reparse stripped");
    assert!(!out.is_encrypted(), "stripped output must be unencrypted");
    assert!(
        out.trailer.get(b"Encrypt").is_err(),
        "stripped output trailer must have no /Encrypt"
    );
}

#[test]
fn encrypted_input_qualifies_with_nonempty_password() {
    // Non-empty user password: load_mem leaves the doc encrypted, so qualify's
    // explicit `doc.decrypt(password)` branch is exercised.
    let encrypted_bytes = build_encrypted("ownerpw", "userpw");
    let reloaded = Document::load_mem(&encrypted_bytes).expect("reload encrypted");
    assert!(
        reloaded.is_encrypted(),
        "non-empty-password input must reload as encrypted"
    );

    let qualified = qualify(&encrypted_bytes, Some("userpw")).expect("qualify encrypted");
    assert_qualified_ok(&qualified);
}

#[test]
fn encrypted_input_qualifies_with_empty_password() {
    // Empty user password: load_mem auto-decrypts at load. qualify still
    // succeeds and emits an unencrypted background.
    let encrypted_bytes = build_encrypted("", "");
    let qualified = qualify(&encrypted_bytes, Some("")).expect("qualify encrypted");
    assert_qualified_ok(&qualified);
}

#[test]
fn wrong_password_is_a_clean_error() {
    let encrypted_bytes = build_encrypted("ownerpw", "userpw");
    let err = qualify(&encrypted_bytes, Some("wrong")).expect_err("must fail to decrypt");
    assert!(
        matches!(err, quillmark_qualify::QualifyError::Decrypt(_)),
        "wrong password must surface a Decrypt error, got {err:?}"
    );
}

#[test]
fn unencrypted_input_qualifies_cleanly() {
    let mut doc = build_form();
    let bytes = save(&mut doc);
    assert!(!Document::load_mem(&bytes).unwrap().is_encrypted());

    let qualified = qualify(&bytes, None).expect("qualify plain");
    let spec = FormSpec::parse(&qualified.form_json).expect("parse form.json");
    assert_eq!(spec.fields.len(), 1);
    assert_eq!(spec.fields[0].name, "FullName");
}
