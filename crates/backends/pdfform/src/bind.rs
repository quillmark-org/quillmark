//! The **bind** step: at quill load, resolve every `form.json` field against the
//! two static inputs — the quill schema and the background geometry — into the
//! session's value-free widget layer. Everything here is a pure function of the
//! quill (not the document), so it runs once at open, never per render:
//! `form.json` never restates what the schema carries, and a widget bound to a
//! nonexistent field or an out-of-range page is a load error, not a silent blank.
//!
//! Two products, one per field population ([`crate::form`]):
//! - A **bound** field names a `schema_field`. [`bind`] walks that path against
//!   the [`QuillConfig`] to the leaf [`FieldSchema`], and [`project_kind`]
//!   projects that resolved field to a widget kind (choice/checkbox/text). The
//!   projection is **total or it is a load error**: a field that resolves to an
//!   `object` or an array of objects/arrays has no widget shape
//!   ([`BindError::Unbindable`]).
//! - An **unbound** widget carries its own declared `type`, copied straight
//!   through (no schema to consult).
//!
//! Both collapse to a [`BoundWidget`] — the value-free intrinsic layer the
//! session holds for its lifetime, its `rect` already flipped to final PDF
//! geometry. Per-document value resolution ([`crate::resolve`]) runs against it
//! and touches nothing but the value.

use quillmark_core::quill::{FieldSchema, FieldType as SchemaType, QuillConfig};
use quillmark_pdf::FieldType as WidgetType;

use crate::form::{BoundField, FormSpec, Rect, UnboundWidget, WidgetKind};

/// A `form.json` field with everything static about it resolved: **final**
/// geometry (bottom-left `[x0, y0, x1, y1]`, page-range validated) plus a
/// value-free [`WidgetType`]. A bound field carries `Some(schema_field)` (its
/// value is resolved per document); an unbound widget carries `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundWidget {
    pub name: String,
    pub schema_field: Option<String>,
    pub page: usize,
    pub rect: [f32; 4],
    pub field_type: WidgetType,
    pub tooltip: Option<String>,
}

/// Why binding a `form.json` field failed. Every variant is a load error — the
/// point of binding at load is to turn what was a silently-blank (or misplaced)
/// widget into a diagnostic.
#[derive(Debug)]
pub enum BindError {
    /// A `schema_field` path does not resolve: a missing root, or a `.segment`
    /// that descends into the wrong shape or a nonexistent key. Names the failing
    /// segment.
    Dangling {
        name: String,
        path: String,
        segment: String,
    },
    /// A `schema_field` resolves, but to a schema type with no widget shape (an
    /// `object`, or an array of objects/arrays).
    Unbindable {
        name: String,
        path: String,
        ty: String,
    },
    /// A widget targets a page the background PDF does not have.
    PageOutOfRange {
        name: String,
        page: usize,
        page_count: usize,
    },
}

impl BindError {
    /// The stable error code a caller stamps on the surfaced diagnostic.
    pub fn code(&self) -> &'static str {
        match self {
            BindError::Dangling { .. } => "pdfform::dangling_binding",
            BindError::Unbindable { .. } => "pdfform::unbindable_field",
            BindError::PageOutOfRange { .. } => "pdfform::field_page_out_of_range",
        }
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Dangling {
                name,
                path,
                segment,
            } => write!(
                f,
                "form.json field {name:?} binds schema_field {path:?}, but segment {segment:?} \
                 does not resolve against the quill schema"
            ),
            BindError::Unbindable { name, path, ty } => write!(
                f,
                "form.json field {name:?} binds schema_field {path:?}, which resolves to schema \
                 type `{ty}` — no widget can render it; bind a scalar, enum, boolean, or an array \
                 of those instead"
            ),
            BindError::PageOutOfRange {
                name,
                page,
                page_count,
            } => write!(
                f,
                "form.json field {name:?} targets page {page} but `form.pdf` has {page_count} page(s)"
            ),
        }
    }
}

/// Resolve and place every field in `spec` against the schema and the page
/// geometry, yielding the session's value-free widget layer. Bound fields
/// inherit their kind, choice options, multiline, and tooltip from the schema;
/// unbound widgets pass their declared kind through unchanged; every widget's
/// rect is flipped to final geometry and its page validated — all of it once,
/// here, so nothing downstream repeats it per document.
pub fn bind_widgets(
    spec: &FormSpec,
    config: &QuillConfig,
    page_boxes: &[[f32; 4]],
) -> Result<Vec<BoundWidget>, BindError> {
    let mut bound = Vec::with_capacity(spec.fields.len() + spec.widgets.len());
    for field in &spec.fields {
        bound.push(bind_field(field, config, page_boxes)?);
    }
    for widget in &spec.widgets {
        bound.push(bind_unbound(widget, page_boxes)?);
    }
    Ok(bound)
}

/// Bind one schema-bound field: resolve its path, project the widget kind, place
/// its geometry, and inherit the tooltip (an explicit `tooltip` overrides the
/// schema `description`).
fn bind_field(
    field: &BoundField,
    config: &QuillConfig,
    page_boxes: &[[f32; 4]],
) -> Result<BoundWidget, BindError> {
    let schema = bind(config, &field.name, &field.schema_field)?;
    let field_type = project_kind(schema, &field.name, &field.schema_field)?;
    let tooltip = field
        .tooltip
        .clone()
        .or_else(|| schema.description.clone());
    Ok(BoundWidget {
        name: field.name.clone(),
        schema_field: Some(field.schema_field.clone()),
        page: field.page,
        rect: place(&field.name, field.page, field.rect, page_boxes)?,
        field_type,
        tooltip,
    })
}

/// Lift one unbound widget: its declared kind maps straight to a [`WidgetType`]
/// (no schema consulted) and its geometry is placed.
fn bind_unbound(
    widget: &UnboundWidget,
    page_boxes: &[[f32; 4]],
) -> Result<BoundWidget, BindError> {
    Ok(BoundWidget {
        name: widget.name.clone(),
        schema_field: None,
        page: widget.page,
        rect: place(&widget.name, widget.page, widget.rect, page_boxes)?,
        field_type: widget_type(&widget.kind),
        tooltip: widget.tooltip.clone(),
    })
}

/// Flip a widget's top-left, page-relative rect to final bottom-left PDF
/// geometry against its page's media box, validating the page exists. Runs once
/// per widget at load — the geometry is fixed for the session's lifetime.
fn place(
    name: &str,
    page: usize,
    rect: Rect,
    page_boxes: &[[f32; 4]],
) -> Result<[f32; 4], BindError> {
    let media_box = page_boxes.get(page).ok_or_else(|| BindError::PageOutOfRange {
        name: name.to_string(),
        page,
        page_count: page_boxes.len(),
    })?;
    Ok(flip_rect(rect, *media_box))
}

/// Page-relative top-left `{x,y,w,h}` → spine bottom-left `[x0, y0, x1, y1]` in
/// PDF user space. Honours a non-zero page origin: the left edge is the MediaBox
/// `x0` and `y` is measured down from the top edge (MediaBox `y1`), so a
/// translated MediaBox (e.g. `[10 20 622 812]`) places widgets correctly rather
/// than shifting them by the origin. This is the single biggest hand-authoring
/// footgun, defused structurally in one place.
fn flip_rect(r: Rect, media_box: [f32; 4]) -> [f32; 4] {
    let left = media_box[0];
    let top = media_box[3];
    [left + r.x, top - (r.y + r.h), left + r.x + r.w, top - r.y]
}

/// Resolve a `schema_field` path to the leaf [`FieldSchema`] it addresses.
///
/// The root segment resolves in `main.fields`, or is the reserved `$cards`
/// (below). Thereafter a `.N` segment requires the current schema be `array`
/// (descend into `items`) and a `.key` segment requires `object` (descend into
/// `properties[key]`). Any miss — a bad root, a wrong-shape descent, a missing
/// key, or segments left dangling past a leaf — is a [`BindError::Dangling`].
///
/// `$cards.<kind>.<i>.<field>…` addresses a card field: `<kind>` names a card
/// kind, `<i>` is the (numeric) instance index selected at value time, and
/// `<field>…` descends into that kind's schema. **Absolute-index addressing
/// (`$cards.<i>.<field>`) is not accepted in `form@0.2.0`** — a widget kind must
/// be statically derivable, and only kind+index tells the schema which field it is.
pub fn bind<'a>(
    config: &'a QuillConfig,
    name: &str,
    path: &str,
) -> Result<&'a FieldSchema, BindError> {
    let dangling = |segment: &str| BindError::Dangling {
        name: name.to_string(),
        path: path.to_string(),
        segment: segment.to_string(),
    };

    let mut parts = path.split('.');
    // `split` on a non-empty string always yields at least one segment.
    let root = parts.next().unwrap_or("");

    let mut cur: &FieldSchema = if root == "$cards" {
        let kind = parts.next().ok_or_else(|| dangling(root))?;
        let card = config.card_kind(kind).ok_or_else(|| dangling(kind))?;
        let idx = parts.next().ok_or_else(|| dangling(kind))?;
        // The instance index selects a card at value time; it must be numeric,
        // but does not descend the schema (all instances of a kind share it).
        idx.parse::<usize>().map_err(|_| dangling(idx))?;
        let card_field = parts.next().ok_or_else(|| dangling(idx))?;
        card.fields
            .get(card_field)
            .ok_or_else(|| dangling(card_field))?
    } else {
        config.main.fields.get(root).ok_or_else(|| dangling(root))?
    };

    for seg in parts {
        cur = descend(cur, seg).ok_or_else(|| dangling(seg))?;
    }
    Ok(cur)
}

/// Descend one path segment into `cur`: a numeric segment indexes an `array`
/// (into its `items` element schema); a key segment reads an `object` property.
/// A shape mismatch or missing key returns `None`.
fn descend<'a>(cur: &'a FieldSchema, seg: &str) -> Option<&'a FieldSchema> {
    match seg.parse::<usize>() {
        Ok(_) => match cur.r#type {
            SchemaType::Array => cur.items.as_deref(),
            _ => None,
        },
        Err(_) => match cur.r#type {
            SchemaType::Object => cur.properties.as_ref()?.get(seg).map(Box::as_ref),
            _ => None,
        },
    }
}

/// Project a resolved [`FieldSchema`] to its widget kind. Keyed on the field's
/// *capability*, not its `type` token — crucially, choice keys on
/// `enum_values.is_some()`, so both `type: enum` and the deprecated `string` +
/// `enum:` modifier project to a dropdown (keying on the `Enum` variant would
/// silently demote the latter to a text box). Total or a load error: an
/// `object`, or an array of objects/arrays, has no widget shape.
pub fn project_kind(
    field: &FieldSchema,
    name: &str,
    path: &str,
) -> Result<WidgetType, BindError> {
    // A finite string domain, however spelled, is a dropdown.
    if let Some(values) = &field.enum_values {
        return Ok(WidgetType::Choice {
            options: values.clone(),
        });
    }
    let unbindable = || BindError::Unbindable {
        name: name.to_string(),
        path: path.to_string(),
        ty: type_desc(field),
    };
    Ok(match &field.r#type {
        SchemaType::Boolean => WidgetType::Checkbox,
        SchemaType::String
        | SchemaType::Number
        | SchemaType::Integer
        | SchemaType::DateTime
        | SchemaType::RichText { .. }
        | SchemaType::PlainText { .. } => WidgetType::Text {
            multiline: is_multiline(field),
        },
        // `Enum` without `enum_values` is unreachable (the loader requires
        // `values:`); handled above. This arm keeps the match total.
        SchemaType::Enum => WidgetType::Choice {
            options: field.enum_values.clone().unwrap_or_default(),
        },
        // An array binds as text (element texts joined with newlines, matching
        // `resolve::coerce_text`) only when its element is a scalar or prose;
        // an array of objects/arrays, or an itemless array, has no widget shape.
        SchemaType::Array => match field.items.as_deref() {
            Some(items) if is_scalar_or_prose(items) => WidgetType::Text {
                multiline: is_multiline(field),
            },
            _ => return Err(unbindable()),
        },
        SchemaType::Object => return Err(unbindable()),
    })
}

/// Whether `field`'s widget is multi-line — inherited from the schema's
/// `ui.multiline` hint.
fn is_multiline(field: &FieldSchema) -> bool {
    field
        .ui
        .as_ref()
        .and_then(|u| u.multiline)
        .unwrap_or(false)
}

/// A scalar or prose type binds to text; only `array`/`object` do not.
fn is_scalar_or_prose(field: &FieldSchema) -> bool {
    !matches!(field.r#type, SchemaType::Array | SchemaType::Object)
}

/// A human-readable type name for the unbindable diagnostic, spelling an array's
/// element type (`array<object>`).
fn type_desc(field: &FieldSchema) -> String {
    match &field.r#type {
        SchemaType::Array => format!(
            "array<{}>",
            field.items.as_ref().map_or("?", |i| i.r#type.as_str())
        ),
        other => other.as_str().to_string(),
    }
}

/// Map an unbound widget's declared kind to a [`WidgetType`].
fn widget_type(kind: &WidgetKind) -> WidgetType {
    match kind {
        WidgetKind::Text { multiline } => WidgetType::Text {
            multiline: *multiline,
        },
        WidgetKind::Checkbox => WidgetType::Checkbox,
        WidgetKind::Choice { options } => WidgetType::Choice {
            options: options.clone(),
        },
        WidgetKind::Signature => WidgetType::Signature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quill schema exercising every projection arm and both card kinds.
    const YAML: &str = r#"
quill:
  name: binder
  version: 0.1.0
  backend: pdfform
  description: bind-test schema
main:
  body:
    enabled: false
  fields:
    full_name:
      type: string
    comments:
      type: array
      items:
        type: string
      ui:
        multiline: true
    agree:
      type: boolean
    favorite_color:
      type: string
      enum: [red, green, blue]
    promoted_color:
      type: enum
      values: [cyan, magenta]
    count:
      type: integer
    when:
      type: datetime
    bio:
      type: richtext
    address:
      type: object
      properties:
        street: { type: string }
        city: { type: string }
    refs:
      type: array
      items:
        type: object
        properties:
          org: { type: string }
card_kinds:
  indorsement:
    fields:
      from:
        type: string
      signed:
        type: boolean
"#;

    fn config() -> QuillConfig {
        QuillConfig::from_yaml(YAML).expect("schema parses")
    }

    fn kind(path: &str) -> Result<WidgetType, BindError> {
        let c = config();
        let schema = bind(&c, "W", path)?;
        project_kind(schema, "W", path)
    }

    #[test]
    fn scalar_and_prose_project_to_text() {
        assert_eq!(kind("full_name").unwrap(), WidgetType::Text { multiline: false });
        assert_eq!(kind("count").unwrap(), WidgetType::Text { multiline: false });
        assert_eq!(kind("when").unwrap(), WidgetType::Text { multiline: false });
        assert_eq!(kind("bio").unwrap(), WidgetType::Text { multiline: false });
    }

    #[test]
    fn boolean_projects_to_checkbox() {
        assert_eq!(kind("agree").unwrap(), WidgetType::Checkbox);
    }

    #[test]
    fn both_enum_spellings_project_to_choice() {
        // Deprecated `string` + `enum:` and promoted `type: enum` both key on
        // `enum_values.is_some()`.
        assert_eq!(
            kind("favorite_color").unwrap(),
            WidgetType::Choice {
                options: vec!["red".into(), "green".into(), "blue".into()]
            }
        );
        assert_eq!(
            kind("promoted_color").unwrap(),
            WidgetType::Choice {
                options: vec!["cyan".into(), "magenta".into()]
            }
        );
    }

    #[test]
    fn scalar_array_projects_to_multiline_text_via_ui() {
        assert_eq!(kind("comments").unwrap(), WidgetType::Text { multiline: true });
    }

    #[test]
    fn array_element_binds_to_scalar_text() {
        assert_eq!(kind("comments.0").unwrap(), WidgetType::Text { multiline: false });
    }

    #[test]
    fn object_property_binds() {
        assert_eq!(kind("address.street").unwrap(), WidgetType::Text { multiline: false });
    }

    #[test]
    fn object_and_object_array_are_unbindable() {
        // `array<array>` is rejected by the schema loader itself
        // (`quill::nested_array_not_supported`), so it can never reach `bind`;
        // the two shapes that can are a bare `object` and an `array<object>`.
        for (path, ty) in [("address", "object"), ("refs", "array<object>")] {
            match kind(path) {
                Err(e @ BindError::Unbindable { .. }) => {
                    assert_eq!(e.code(), "pdfform::unbindable_field");
                    assert!(e.to_string().contains(ty), "{path}: {e}");
                }
                other => panic!("{path}: expected Unbindable, got {other:?}"),
            }
        }
    }

    #[test]
    fn dangling_root_and_segment_error() {
        for (path, seg) in [
            ("nonesuch", "nonesuch"),
            ("full_name.0", "0"),        // scalar has no array index
            ("address.zip", "zip"),      // no such property
            ("comments.oops", "oops"),   // array wants a numeric index
        ] {
            let c = config();
            match bind(&c, "W", path) {
                Err(e @ BindError::Dangling { .. }) => {
                    assert_eq!(e.code(), "pdfform::dangling_binding");
                    assert!(e.to_string().contains(seg), "{path}: {e}");
                }
                other => panic!("{path}: expected Dangling, got {other:?}"),
            }
        }
    }

    #[test]
    fn card_paths_bind_by_kind_and_index() {
        assert_eq!(
            kind("$cards.indorsement.0.from").unwrap(),
            WidgetType::Text { multiline: false }
        );
        assert_eq!(
            kind("$cards.indorsement.1.signed").unwrap(),
            WidgetType::Checkbox
        );
    }

    #[test]
    fn card_absolute_index_and_bad_kind_are_dangling() {
        let c = config();
        // `$cards.0.from` — absolute index: `0` is not a card kind, so it dangles
        // at that segment (absolute addressing is gone in form@0.2.0).
        assert!(matches!(
            bind(&c, "W", "$cards.0.from"),
            Err(BindError::Dangling { .. })
        ));
        // Unknown kind.
        assert!(matches!(
            bind(&c, "W", "$cards.memo.0.author"),
            Err(BindError::Dangling { .. })
        ));
        // Missing field after the index.
        assert!(matches!(
            bind(&c, "W", "$cards.indorsement.0"),
            Err(BindError::Dangling { .. })
        ));
        // Non-numeric index.
        assert!(matches!(
            bind(&c, "W", "$cards.indorsement.x.from"),
            Err(BindError::Dangling { .. })
        ));
    }

    #[test]
    fn tooltip_inherits_description_unless_overridden() {
        let yaml = r#"
quill:
  name: t
  version: 0.1.0
  backend: pdfform
  description: t
main:
  body:
    enabled: false
  fields:
    a:
      type: string
      description: From the schema.
    b:
      type: string
      description: Also schema.
"#;
        let cfg = QuillConfig::from_yaml(yaml).unwrap();
        let spec = FormSpec::parse(
            br#"{
              "schema": "quillmark/form@0.2.0",
              "fields": [
                { "name": "A", "schema_field": "a", "page": 0,
                  "rect": { "x": 0, "y": 0, "w": 1, "h": 1 } },
                { "name": "B", "schema_field": "b", "page": 0,
                  "rect": { "x": 0, "y": 2, "w": 1, "h": 1 }, "tooltip": "Override." }
              ]
            }"#,
        )
        .unwrap();
        let mb = [[0.0, 0.0, 612.0, 792.0]];
        let bound = bind_widgets(&spec, &cfg, &mb).unwrap();
        assert_eq!(bound[0].tooltip.as_deref(), Some("From the schema."));
        assert_eq!(bound[1].tooltip.as_deref(), Some("Override."));
    }

    #[test]
    fn place_flips_to_bottom_left_and_honours_origin() {
        let r = Rect {
            x: 180.0,
            y: 90.0,
            w: 14.0,
            h: 14.0,
        };
        // Zero-origin page.
        assert_eq!(
            place("W", 0, r, &[[0.0, 0.0, 600.0, 800.0]]).unwrap(),
            [180.0, 800.0 - 104.0, 194.0, 800.0 - 90.0]
        );
        // Translated MediaBox [10 20 622 812]: widgets land offset by the origin.
        assert_eq!(
            place("W", 0, r, &[[10.0, 20.0, 622.0, 812.0]]).unwrap(),
            [10.0 + 180.0, 812.0 - 104.0, 10.0 + 194.0, 812.0 - 90.0]
        );
    }

    #[test]
    fn place_rejects_out_of_range_page() {
        let r = Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        };
        match place("W", 2, r, &[[0.0, 0.0, 612.0, 792.0]]) {
            Err(e @ BindError::PageOutOfRange { .. }) => {
                assert_eq!(e.code(), "pdfform::field_page_out_of_range");
            }
            other => panic!("expected PageOutOfRange, got {other:?}"),
        }
    }
}
