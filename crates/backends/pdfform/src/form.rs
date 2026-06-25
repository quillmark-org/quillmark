//! `form.json` — the complete field reconstruction spec a `pdfform` quill ships
//! alongside its stripped background PDF. One artifact serving three roles: the
//! field map, the regions sidecar source, and the AcroForm reconstruction spec.
//!
//! This module parses it and maps each entry (plus the document's resolved
//! value for that field) into a [`quillmark_pdf::FieldSpec`].

use quillmark_pdf::{Appearance, ChoiceOption, FieldSpec, FieldType};
use serde::Deserialize;

/// The whole `form.json`: an ordered list of fields to reconstruct.
#[derive(Debug, Clone, Deserialize)]
pub struct FormSpec {
    pub fields: Vec<FormField>,
}

/// One reconstructable AcroForm field: geometry, type, accessibility metadata,
/// and the schema field whose value fills it.
#[derive(Debug, Clone, Deserialize)]
pub struct FormField {
    /// AcroForm partial field name (`/T`).
    pub name: String,
    /// Schema field whose document value fills this widget. `None` for fields
    /// with no data binding (e.g. a signature the signer fills by hand).
    #[serde(default)]
    pub schema_field: Option<String>,
    /// 0-based page index.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points, bottom-left origin (page-relative).
    pub rect: [f32; 4],
    #[serde(rename = "type")]
    pub kind: FormFieldKind,
    /// `/MaxLen` for text fields.
    #[serde(default)]
    pub max_len: Option<u32>,
    /// `/TU` tooltip / accessible description.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// `/Ff` extra flags (ReadOnly/Required/Multiline …); type-intrinsic flags
    /// such as the combo bit are added by the stamper.
    #[serde(default)]
    pub flags: u32,
    /// Checkbox export value (the `/AS` name when checked). Defaults to `Yes`.
    #[serde(default)]
    pub on_state: Option<String>,
    /// Choice options (`/Opt`).
    #[serde(default)]
    pub options: Vec<String>,
    /// Whether a choice renders as a dropdown (`true`) or list (`false`).
    #[serde(default)]
    pub combo: bool,
    /// `/DA` appearance override; defaults to the stamper's auto-size string.
    #[serde(default)]
    pub da: Option<String>,
}

/// The `type` discriminant in `form.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormFieldKind {
    Text,
    Checkbox,
    Choice,
    Signature,
}

impl FormField {
    /// Build the stamping spec for this field, binding `value` (the document's
    /// resolved value for `schema_field`, if any).
    pub fn to_field_spec(&self, value: Option<&serde_json::Value>) -> FieldSpec {
        let field_type = match self.kind {
            FormFieldKind::Text => FieldType::Text,
            FormFieldKind::Signature => FieldType::Signature,
            FormFieldKind::Choice => FieldType::Choice {
                options: self.options.iter().map(ChoiceOption::new).collect(),
                combo: self.combo,
            },
            FormFieldKind::Checkbox => FieldType::Checkbox {
                on_state: self.on_state.clone().unwrap_or_else(|| "Yes".to_string()),
                checked: value.is_some_and(is_truthy),
            },
        };

        let mut spec = FieldSpec::new(self.name.clone(), self.page, self.rect, field_type);
        spec.flags = self.flags;
        spec.max_len = self.max_len;
        spec.tooltip = self.tooltip.clone();
        spec.appearance = Appearance {
            da: self.da.clone(),
            ..Appearance::default()
        };
        // Text and choice carry their value as `/V`; checkboxes encode it in the
        // on/off state, signatures have none.
        if matches!(self.kind, FormFieldKind::Text | FormFieldKind::Choice) {
            spec.value = value.and_then(json_to_text);
        }
        spec
    }
}

/// Render a JSON scalar as the text a form field should display. Objects/arrays
/// and null yield `None`.
fn json_to_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Whether a value selects a checkbox's on-state. Booleans map directly; strings
/// accept the usual affirmatives; numbers are truthy when non-zero.
fn is_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        serde_json::Value::String(s) => {
            matches!(
                s.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1" | "x" | "checked"
            )
        }
        _ => false,
    }
}
