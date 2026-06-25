//! Regenerates the committed stripped-background `form.pdf` for the
//! `simple_form` fixture. This stands in for the upstream qualification layer:
//! it emits a normalized page (labels + field-box chrome drawn into the content
//! stream) with **no** `/AcroForm` and no widget annotations — the backend
//! rebuilds the form fresh from `form.json`.
//!
//! Run with: `cargo test -p quillmark-pdfform --test gen_background -- --ignored`

use std::path::Path;

#[test]
#[ignore = "regenerates the committed form.pdf fixture; run with --ignored"]
fn generate_simple_form_background() {
    // Labels and field-box outlines, positioned to match form.json rects.
    let content = concat!(
        "BT /F1 16 Tf 72 760 Td (Simple Application Form) Tj ET\n",
        "BT /F1 12 Tf 72 720 Td (Full Name:) Tj ET\n",
        "BT /F1 12 Tf 72 690 Td (Agree to terms:) Tj ET\n",
        "BT /F1 12 Tf 72 660 Td (Favorite color:) Tj ET\n",
        "BT /F1 12 Tf 72 618 Td (Signature:) Tj ET\n",
        "0.5 w\n",
        "180 715 340 20 re S\n",
        "180 685 20 20 re S\n",
        "180 655 340 20 re S\n",
        "180 600 340 40 re S\n",
    );
    let pdf = build_background(content);

    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple_form/0.1.0/form.pdf");
    std::fs::write(&path, &pdf).expect("write form.pdf");
    eprintln!("wrote {} bytes to {}", pdf.len(), path.display());
}

/// Build a single-page PDF with a traditional xref table and a drawn content
/// stream, computing object offsets so the xref is valid.
fn build_background(content: &str) -> Vec<u8> {
    let stream = format!(
        "<< /Length {} >>\nstream\n{}endstream",
        content.len(),
        content
    );
    let bodies: [String; 5] = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .to_string(),
        stream,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];

    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body.as_bytes());
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref_off = pdf.len();
    pdf.extend_from_slice(b"xref\n");
    pdf.extend_from_slice(format!("0 {}\n", bodies.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        pdf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }
    pdf.extend_from_slice(b"trailer\n");
    pdf.extend_from_slice(format!("<< /Size {} /Root 1 0 R >>\n", bodies.len() + 1).as_bytes());
    pdf.extend_from_slice(format!("startxref\n{xref_off}\n%%EOF\n").as_bytes());
    pdf
}
