//! Quill configuration parsing and normalization.
use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;

use indexmap::IndexMap;

use serde::{Deserialize, Serialize};

use crate::error::{Diagnostic, Severity};
use crate::value::QuillValue;

use super::types::RICHTEXT_INLINE_TOKEN_MSG;
use super::{BodyCardSchema, CardSchema, FieldSchema, FieldType, UiCardSchema, UiFieldSchema};

/// Canonical string text for a bare scalar unambiguously representable as a
/// string — a boolean (`true`/`false`) or number (`47`, `1.0`). `None` for
/// `null` (≡ absent), strings (already strings), and collections.
///
/// Shared by [`QuillConfig::coerce_value_strict`] (to adopt the value) and
/// `validation::validate_value` (to accept it), so coercion and validation
/// never disagree about which bare scalars a `string` field accepts.
pub(crate) fn scalar_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Reduce a lenient value to its authored-string form: a bare string, the
/// sole element of a length-1 array when that element is a string (the
/// array-unwrap leniency), or a bare scalar's canonical text (via
/// [`scalar_as_string`]). `None` for anything else (a multi-element array, an
/// object, null), leaving the caller's own fallback to apply.
///
/// Shared by the `String` and `RichText` coercion branches, which both reduce
/// a lenient value to a string before adopting it (as the field value itself,
/// or as markdown to import).
fn lenient_string(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(s) = value
        .as_array()
        .filter(|a| a.len() == 1)
        .and_then(|a| a[0].as_str())
    {
        return Some(s.to_string());
    }
    scalar_as_string(value)
}

/// Top-level configuration for a Quillmark project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuillConfig {
    /// Quill package name
    pub name: String,
    /// Human-readable description of the quill itself (parsed from
    /// `quill.description`). Distinct from `main.description`, which describes
    /// the main card's schema.
    pub description: String,
    /// The entry-point card schema (parsed from the Quill.yaml `main:` section).
    pub main: CardSchema,
    /// Named, composable card-kind schemas (parsed from the Quill.yaml
    /// `card_kinds:` section). Does not include `main`.
    pub card_kinds: Vec<CardSchema>,
    /// Backend to use for rendering (e.g., "typst", "html")
    pub backend: String,
    /// Version of the Quillmark spec
    pub version: String,
    /// Author of the project
    pub author: String,
    /// Backend-specific configuration parsed from the top-level YAML section
    /// whose key matches `backend` (e.g. `[typst]`, `[html]`).
    #[serde(default)]
    pub backend_config: HashMap<String, QuillValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CardSchemaDef {
    pub description: Option<String>,
    // Declared so `deny_unknown_fields` accepts a `fields:` block on a card.
    // Fields are re-parsed via `parse_fields_with_order` for ordering.
    #[allow(dead_code)]
    pub fields: Option<serde_json::Map<String, serde_json::Value>>,
    pub ui: Option<UiCardSchema>,
    pub body: Option<BodyCardSchema>,
}

/// Depth context for [`QuillConfig::validate_field_schema_shape`]. Encodes
/// which shapes are legal at the current nesting level, so the one-level
/// nesting contract is enforced by a single recursive walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShapePosition {
    /// A field declared directly on a card: scalar, object, or array.
    Top,
    /// An array's `items`: scalar or object (typed-table row), not an array.
    ArrayItem,
    /// An object's property: scalar only.
    Leaf,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CoercionError {
    #[error("cannot coerce `{value}` to type `{target}` at `{path}`: {reason}")]
    Uncoercible {
        path: String,
        value: String,
        target: String,
        reason: String,
    },
}

/// Write-side leniency mode for [`QuillConfig::conform_value`] — the one axis
/// that separates the render floor's forgiving coercion from a strict typed
/// write.
///
/// The dispatch is shared; only the arms that *defer to the validation layer*
/// or *cross type boundaries* branch on this. See `conform_value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leniency {
    /// The render floor's forgiving cascade (today's `coerce_value_strict`
    /// behavior, unchanged): cross-type scalar coercions apply and a shape a
    /// type cannot adopt falls through unchanged for the validation layer to
    /// report.
    Render,
    /// A strict typed write ([`Card::commit_field`](crate::document::Card::commit_field)):
    /// value-parsing normalizations still apply (`"3"` → `3`, a bare scalar
    /// wraps into a singleton array, richtext markdown imports to corpus), but
    /// cross-type `Boolean`↔`Number` coercions are dropped and every
    /// defer-to-validation fall-through becomes a `CoercionError` — so a
    /// mismatched value fails at the write, not silently at a later render.
    Write,
}

impl QuillConfig {
    /// Returns a named card-kind schema by name.
    pub fn card_kind(&self, name: &str) -> Option<&CardSchema> {
        self.card_kinds.iter().find(|card| card.name == name)
    }

    /// Full schema including `ui` hints.
    ///
    /// Describes the user-fillable fields of the main card and each named
    /// card kind. The quill reference (constructed as `name@version` from
    /// quill metadata) and card-kind discriminators are document-level
    /// metadata, not fields, so they do not appear here.
    pub fn schema(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        let main_value =
            serde_json::to_value(&self.main).expect("CardSchema is always serializable");
        obj.insert("main".to_string(), main_value);

        if !self.card_kinds.is_empty() {
            let card_kinds: BTreeMap<String, serde_json::Value> = self
                .card_kinds
                .iter()
                .map(|card| {
                    let card_value =
                        serde_json::to_value(card).expect("CardSchema is always serializable");
                    (card.name.clone(), card_value)
                })
                .collect();
            obj.insert(
                "card_kinds".to_string(),
                serde_json::to_value(&card_kinds).expect("card_kinds map is always serializable"),
            );
        }

        serde_json::Value::Object(obj)
    }

    /// Coerce typed payload fields (IndexMap of user fields only).
    pub fn coerce_payload(
        &self,
        payload: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in payload {
            if let Some(field_schema) = self.main.fields.get(field_name) {
                let path = field_name.as_str();
                coerced.insert(
                    field_name.clone(),
                    Self::conform_value(field_value, field_schema, path, Leniency::Render)?,
                );
            } else {
                coerced.insert(field_name.clone(), field_value.clone());
            }
        }
        Ok(coerced)
    }

    /// Coerce typed fields for a single card (IndexMap of user fields only).
    ///
    /// Returns the input unchanged when the card kind is unknown.
    pub fn coerce_card(
        &self,
        card_kind: &str,
        fields: &IndexMap<String, QuillValue>,
    ) -> Result<IndexMap<String, QuillValue>, CoercionError> {
        let Some(card_schema) = self.card_kind(card_kind) else {
            return Ok(fields.clone());
        };
        let mut coerced: IndexMap<String, QuillValue> = IndexMap::new();
        for (field_name, field_value) in fields {
            if let Some(field_schema) = card_schema.fields.get(field_name) {
                let path = format!("card_kinds.{card_kind}.{field_name}");
                coerced.insert(
                    field_name.clone(),
                    Self::conform_value(field_value, field_schema, &path, Leniency::Render)?,
                );
            } else {
                coerced.insert(field_name.clone(), field_value.clone());
            }
        }
        Ok(coerced)
    }

    /// Validate a typed [`crate::document::Document`] against this configuration.
    pub fn validate_document(
        &self,
        doc: &crate::document::Document,
    ) -> Result<(), Vec<super::validation::ValidationError>> {
        super::validation::validate_typed_document(self, doc)
    }

    /// The one write-side per-type dispatch: given a value, a field's schema,
    /// and a [`Leniency`] mode, validate/normalize the value to the canonical
    /// form the type stores. `Render` is the render floor's forgiving coercion
    /// (the former `coerce_value_strict`, behavior-preserving); `Write` is the
    /// strict typed-write commit driving [`Card::commit_field`](crate::document::Card::commit_field).
    ///
    /// Validation keeps its own read-only dispatch (`validation::validate_value`),
    /// synced with this via the shared helpers `scalar_as_string` /
    /// `decode_richtext_value`.
    pub(crate) fn conform_value(
        value: &QuillValue,
        field_schema: &super::FieldSchema,
        path: &str,
        mode: Leniency,
    ) -> Result<QuillValue, CoercionError> {
        use super::FieldType;

        let json_value = value.as_json();

        // Null ≡ absent: a present-null value (`field:`, `field: null`,
        // `field: ~`) carries no data, so it passes through coercion unchanged
        // for every type rather than failing as a mismatch. The render floor
        // and the validation layer treat it the same as an omitted field. This
        // also preserves a `!must_fill` marker riding on `value` (the fill flag
        // is never part of the JSON projection).
        if json_value.is_null() {
            return Ok(value.clone());
        }

        match field_schema.r#type {
            FieldType::Array => {
                let arr = if let Some(a) = json_value.as_array() {
                    a.clone()
                } else {
                    vec![json_value.clone()]
                };

                // Every array carries an element schema (`items`). Coerce each
                // element against it: scalar items (`string[]`, `integer[]`,
                // `richtext[]`) coerce element-wise; object items recurse into
                // the element's `properties` via the Object branch.
                if let Some(items) = &field_schema.items {
                    let mut out = Vec::with_capacity(arr.len());
                    for (idx, elem) in arr.iter().enumerate() {
                        let coerced = Self::conform_value(
                            &QuillValue::from_json(elem.clone()),
                            items,
                            &format!("{path}[{idx}]"),
                            mode,
                        )?;
                        out.push(coerced.into_json());
                    }
                    Ok(QuillValue::from_json(serde_json::Value::Array(out)))
                } else {
                    // Defensive fallback: schema-load rejects any array without
                    // `items` (quill::array_missing_items), so a validated
                    // config never reaches here — pass the array through as-is.
                    Ok(QuillValue::from_json(serde_json::Value::Array(arr)))
                }
            }
            FieldType::Boolean => {
                if let Some(b) = json_value.as_bool() {
                    return Ok(QuillValue::from_json(serde_json::Value::Bool(b)));
                }
                if let Some(s) = json_value.as_str() {
                    let lower = s.to_lowercase();
                    if lower == "true" {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(true)));
                    } else if lower == "false" {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(false)));
                    }
                }
                // Cross-type number→boolean is a render-floor leniency; a strict
                // write requires an actual boolean or its `"true"`/`"false"` text.
                if mode == Leniency::Render {
                    if let Some(n) = json_value.as_i64() {
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(n != 0)));
                    }
                    if let Some(n) = json_value.as_f64() {
                        if n.is_nan() {
                            return Ok(QuillValue::from_json(serde_json::Value::Bool(false)));
                        }
                        return Ok(QuillValue::from_json(serde_json::Value::Bool(
                            n.abs() > f64::EPSILON,
                        )));
                    }
                }

                Err(CoercionError::Uncoercible {
                    path: path.to_string(),
                    value: json_value.to_string(),
                    target: "boolean".to_string(),
                    reason: "value is not coercible to boolean".to_string(),
                })
            }
            FieldType::Number => {
                if json_value.is_number() {
                    return Ok(value.clone());
                }
                if let Some(s) = json_value.as_str() {
                    if let Ok(i) = s.parse::<i64>() {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    if let Ok(f) = s.parse::<f64>() {
                        if let Some(num) = serde_json::Number::from_f64(f) {
                            return Ok(QuillValue::from_json(num.into()));
                        }
                    }
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: s.to_string(),
                        target: "number".to_string(),
                        reason: "string is not a valid number".to_string(),
                    });
                }
                // Cross-type boolean→number is a render-floor leniency only.
                if mode == Leniency::Render {
                    if let Some(b) = json_value.as_bool() {
                        let n = if b { 1 } else { 0 };
                        return Ok(QuillValue::from_json(serde_json::Value::Number(
                            serde_json::Number::from(n),
                        )));
                    }
                }

                Err(CoercionError::Uncoercible {
                    path: path.to_string(),
                    value: json_value.to_string(),
                    target: "number".to_string(),
                    reason: "value is not coercible to number".to_string(),
                })
            }
            FieldType::Integer => {
                if let Some(i) = json_value.as_i64() {
                    return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                }
                if let Some(u) = json_value.as_u64() {
                    if let Ok(i) = i64::try_from(u) {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: json_value.to_string(),
                        target: "integer".to_string(),
                        reason: "integer value exceeds i64 range".to_string(),
                    });
                }
                if let Some(s) = json_value.as_str() {
                    if let Ok(i) = s.parse::<i64>() {
                        return Ok(QuillValue::from_json(serde_json::Number::from(i).into()));
                    }
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: s.to_string(),
                        target: "integer".to_string(),
                        reason: "string is not a valid integer".to_string(),
                    });
                }
                // Cross-type boolean→integer is a render-floor leniency only.
                if mode == Leniency::Render {
                    if let Some(b) = json_value.as_bool() {
                        let n = if b { 1 } else { 0 };
                        return Ok(QuillValue::from_json(serde_json::Value::Number(
                            serde_json::Number::from(n),
                        )));
                    }
                }

                Err(CoercionError::Uncoercible {
                    path: path.to_string(),
                    value: json_value.to_string(),
                    target: "integer".to_string(),
                    reason: "value is not coercible to integer".to_string(),
                })
            }
            FieldType::String => {
                if json_value.is_string() {
                    return Ok(value.clone());
                }
                // Gracious leniency: unwrap a length-1 array's sole string
                // element, or adopt a bare bool/number's canonical text (an
                // author writing `verified: true` for a `string` field), rather
                // than reject it. Null is handled above; other collections fall
                // through.
                if let Some(text) = lenient_string(json_value) {
                    return Ok(QuillValue::from_json(serde_json::Value::String(text)));
                }
                // A non-stringifiable shape (object, multi-element array): the
                // render floor defers to validation, a strict write fails now.
                match mode {
                    Leniency::Render => Ok(value.clone()),
                    Leniency::Write => Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: json_value.to_string(),
                        target: "string".to_string(),
                        reason: "value is not a string".to_string(),
                    }),
                }
            }
            FieldType::RichText { inline } => {
                // The seam carries the corpus, so coercion commits the corpus
                // form: an already-structured value (editor / re-render) is
                // validated and re-canonicalized; an authored markdown string is
                // imported. Determinism is inherited from `import` being pure.
                // An `inline` field additionally requires the resulting corpus to
                // be single-`Para` (`richtext(inline)`): editors mount a one-line
                // surface, so multi-block content is a coercion error here, in
                // lockstep with the validation-layer `richtext::not_inline` check.
                //
                // This is the deliberately-lenient sibling of
                // `document::decode_richtext_value` (used by the strict wire /
                // literal / validation sites): the string branch below reduces a
                // bare scalar or length-1 array to text before importing, which
                // the strict decoder must not do, so it stays open-coded here.
                let inline_check =
                    |rt: &quillmark_richtext::RichText| -> Result<(), CoercionError> {
                        if inline && !rt.is_inline() {
                            return Err(CoercionError::Uncoercible {
                                path: path.to_string(),
                                value: "<richtext>".to_string(),
                                target: "richtext(inline)".to_string(),
                                reason: "richtext(inline) requires a single paragraph line \
                                     with no list/quote container and no islands"
                                    .to_string(),
                            });
                        }
                        Ok(())
                    };
                // A strict write uses `decode_richtext_value` semantics — a
                // canonical corpus object or a markdown string, nothing else. No
                // scalar→string reduction (the render floor's lenient cascade
                // below): a bare scalar for a richtext field fails the write. The
                // messages mirror `Card::commit_field`'s richtext error variants,
                // which the bindings key on.
                if mode == Leniency::Write {
                    let corpus = match crate::document::decode_richtext_value(json_value) {
                        Some(result) => result.map_err(|e| CoercionError::Uncoercible {
                            path: path.to_string(),
                            value: "<richtext>".to_string(),
                            target: "richtext".to_string(),
                            reason: e.into_message(),
                        })?,
                        None => {
                            return Err(CoercionError::Uncoercible {
                                path: path.to_string(),
                                value: json_value.to_string(),
                                target: "richtext".to_string(),
                                reason: format!(
                                    "expected a richtext corpus object or a markdown string, got {}",
                                    match json_value {
                                        serde_json::Value::Bool(_) => "a boolean",
                                        serde_json::Value::Number(_) => "a number",
                                        serde_json::Value::Array(_) => "an array",
                                        _ => "an unsupported value",
                                    }
                                ),
                            })
                        }
                    };
                    inline_check(&corpus)?;
                    return Ok(QuillValue::from_json(
                        quillmark_richtext::serial::to_canonical_value(&corpus),
                    ));
                }
                if json_value.is_object() {
                    let rt = quillmark_richtext::serial::from_canonical_value(json_value).map_err(
                        |e| CoercionError::Uncoercible {
                            path: path.to_string(),
                            value: "<object>".to_string(),
                            target: "richtext".to_string(),
                            reason: format!("not a valid richtext corpus: {e}"),
                        },
                    )?;
                    inline_check(&rt)?;
                    return Ok(QuillValue::from_json(
                        quillmark_richtext::serial::to_canonical_value(&rt),
                    ));
                }
                // Reduce to the authored markdown string via the shared
                // leniency cascade (bare string, length-1 array unwrap, or bare
                // scalar), then import.
                let Some(markdown) = lenient_string(json_value) else {
                    // A shape that is neither corpus nor stringifiable (e.g. a
                    // multi-element array): leave it for the validation layer to
                    // report, matching the String branch's fall-through.
                    return Ok(value.clone());
                };
                let rt = quillmark_richtext::import::from_markdown(&markdown).map_err(|e| {
                    CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: markdown.clone(),
                        target: "richtext".to_string(),
                        reason: format!("markdown import failed: {e}"),
                    }
                })?;
                inline_check(&rt)?;
                Ok(QuillValue::from_json(
                    quillmark_richtext::serial::to_canonical_value(&rt),
                ))
            }
            FieldType::DateTime => {
                if json_value.is_null() {
                    return Ok(QuillValue::from_json(serde_json::Value::Null));
                }
                let text = if let Some(s) = json_value.as_str() {
                    if s.is_empty() {
                        return Ok(QuillValue::from_json(serde_json::Value::Null));
                    }
                    s.to_string()
                } else if let Some(arr) = json_value.as_array() {
                    if arr.len() == 1 {
                        if let Some(s) = arr[0].as_str() {
                            s.to_string()
                        } else {
                            return Err(CoercionError::Uncoercible {
                                path: path.to_string(),
                                value: json_value.to_string(),
                                target: field_schema.r#type.as_str().to_string(),
                                reason: "value must be a string".to_string(),
                            });
                        }
                    } else {
                        return Err(CoercionError::Uncoercible {
                            path: path.to_string(),
                            value: json_value.to_string(),
                            target: field_schema.r#type.as_str().to_string(),
                            reason: "value must be a single string".to_string(),
                        });
                    }
                } else {
                    return Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: json_value.to_string(),
                        target: field_schema.r#type.as_str().to_string(),
                        reason: "value must be a string".to_string(),
                    });
                };

                if super::formats::is_valid_datetime(&text) {
                    Ok(QuillValue::from_json(serde_json::Value::String(text)))
                } else {
                    Err(CoercionError::Uncoercible {
                        path: path.to_string(),
                        value: text,
                        target: field_schema.r#type.as_str().to_string(),
                        reason: "invalid datetime format".to_string(),
                    })
                }
            }
            FieldType::Object => {
                if let Some(obj) = json_value.as_object() {
                    if let Some(props) = &field_schema.properties {
                        let coerced_obj = Self::coerce_object_props(obj, props, path, mode)?;
                        Ok(QuillValue::from_json(serde_json::Value::Object(
                            coerced_obj,
                        )))
                    } else {
                        Ok(value.clone())
                    }
                } else {
                    // A non-object value: the render floor defers to validation,
                    // a strict write fails now.
                    match mode {
                        Leniency::Render => Ok(value.clone()),
                        Leniency::Write => Err(CoercionError::Uncoercible {
                            path: path.to_string(),
                            value: json_value.to_string(),
                            target: "object".to_string(),
                            reason: "value is not an object".to_string(),
                        }),
                    }
                }
            }
        }
    }

    /// Walk `obj`'s keys, coercing any that match `props` against the matching
    /// schema and copying any others through verbatim. `parent_path` is the
    /// breadcrumb for the enclosing scope (e.g. `"foo[3]"` or `"foo"`); each
    /// child's path is `"{parent_path}.{k}"`.
    fn coerce_object_props(
        obj: &serde_json::Map<String, serde_json::Value>,
        props: &std::collections::BTreeMap<String, Box<super::FieldSchema>>,
        parent_path: &str,
        mode: Leniency,
    ) -> Result<serde_json::Map<String, serde_json::Value>, CoercionError> {
        let mut out = serde_json::Map::new();
        for (k, v) in obj {
            if let Some(prop_schema) = props.get(k) {
                let child_path = format!("{parent_path}.{k}");
                out.insert(
                    k.clone(),
                    Self::conform_value(
                        &QuillValue::from_json(v.clone()),
                        prop_schema,
                        &child_path,
                        mode,
                    )?
                    .into_json(),
                );
            } else {
                out.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    }

    /// Recursively validate a field's structural shape, enforcing the
    /// one-level nesting contract in a single pass. The `position` records
    /// what shapes are legal at the current depth:
    ///
    /// - [`ShapePosition::Top`] — a field declared directly on a card: scalar,
    ///   `object` (typed dictionary), or `array` (primitive list or typed
    ///   table).
    /// - [`ShapePosition::ArrayItem`] — an array's `items`: a scalar or an
    ///   `object` (the typed-table row), but **not** another array.
    /// - [`ShapePosition::Leaf`] — an object's property (whether a top-level
    ///   typed dictionary or a typed-table row): scalar only. No deeper
    ///   containers, so `array<object<array>>` and `object<array>` are
    ///   rejected here.
    ///
    /// Returns the first violation as a ready-to-push [`Diagnostic`] whose
    /// message names `owner` (the field-name path, e.g. `rows[].tags`), or
    /// `None` when the shape is valid.
    fn validate_field_schema_shape(
        schema: &FieldSchema,
        owner: &str,
        position: ShapePosition,
    ) -> Option<Diagnostic> {
        let err = |code: &str, message: String| {
            Some(Diagnostic::new(Severity::Error, message).with_code(code.to_string()))
        };

        // `items` is only meaningful on arrays; `properties` only on objects.
        if schema.r#type != FieldType::Array && schema.items.is_some() {
            return err(
                "quill::items_not_supported",
                format!(
                    "Field '{owner}' declares 'items' but is not type: array. \
                     'items' (the element schema) is only valid on array fields."
                ),
            );
        }
        // `inline` on a non-richtext field is rejected earlier and once, when
        // `from_quill_value` folds the wire key into the `FieldType` enum
        // (`resolve_richtext_inline`); no second check belongs here.

        match schema.r#type {
            FieldType::Object => {
                // An object nested inside another object (a Leaf position) is
                // the classic "nested type: object" rejection.
                if position == ShapePosition::Leaf {
                    return err(
                        "quill::nested_object_not_supported",
                        format!(
                            "Field '{owner}' uses a nested type: object, which is not supported. \
                             An object's properties may only be scalars."
                        ),
                    );
                }
                let Some(props) = &schema.properties else {
                    return err(
                        "quill::object_missing_properties",
                        format!(
                            "Field '{owner}' has type: object but no properties defined. \
                             Declare a properties map, or use type: array with \
                             items: {{ type: object, properties: … }} for a list of objects."
                        ),
                    );
                };
                if props.is_empty() {
                    return err(
                        "quill::object_empty_properties",
                        format!(
                            "Field '{owner}' has type: object with an empty properties map. \
                             Declare at least one property, or remove the field entirely."
                        ),
                    );
                }
                // Object properties are leaves — scalars only.
                props.iter().find_map(|(name, prop)| {
                    Self::validate_field_schema_shape(
                        prop,
                        &format!("{owner}.{name}"),
                        ShapePosition::Leaf,
                    )
                })
            }
            FieldType::Array => {
                // An array may sit at the top level only; an array element may
                // not itself be an array, and neither may an object property.
                if position != ShapePosition::Top {
                    return err(
                        "quill::nested_array_not_supported",
                        format!(
                            "Field '{owner}' declares a nested array, which is not supported. \
                             Array elements must be scalars or objects, and object properties \
                             may only be scalars."
                        ),
                    );
                }
                if schema.properties.is_some() {
                    return err(
                        "quill::array_properties_not_supported",
                        format!(
                            "Field '{owner}' is type: array with a bare 'properties' map. \
                             Declare the element type under 'items' instead — for a list \
                             of objects use items: {{ type: object, properties: … }}."
                        ),
                    );
                }
                let Some(items) = &schema.items else {
                    return err(
                        "quill::array_missing_items",
                        format!(
                            "Field '{owner}' has type: array but no 'items' element schema. \
                             Declare the element type, e.g. items: {{ type: string }} \
                             for a list of strings or items: {{ type: object, \
                             properties: … }} for a list of objects."
                        ),
                    );
                };
                Self::validate_field_schema_shape(
                    items,
                    &format!("{owner}[]"),
                    ShapePosition::ArrayItem,
                )
            }
            // Scalars are leaves; nothing further to validate.
            _ => None,
        }
    }

    /// Reject multi-line descriptions. Single-line is required so the leading
    /// `# <description>` blueprint slot stays one line and the field-comment
    /// stack remains parseable for LLM consumers.
    fn validate_description_singleline(
        desc: Option<&str>,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        if let Some(d) = desc {
            if d.contains('\n') {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{} description must be a single line; multi-line \
                             descriptions are not allowed.",
                            owner_label
                        ),
                    )
                    .with_code("quill::description_multiline".to_string()),
                );
            }
        }
    }

    /// Reject `>`, `;`, `|` in enum literals. These characters are reserved by
    /// the blueprint inline annotation grammar (`<format>` close, role
    /// separator, enum value separator) and have no escape syntax.
    fn validate_enum_literals(
        field: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        if let Some(values) = &field.enum_values {
            for v in values {
                if v.contains('>') || v.contains(';') || v.contains('|') {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "{} enum value '{}' contains a reserved character \
                                 ('>', ';', or '|') that conflicts with the \
                                 blueprint inline annotation grammar.",
                                owner_label, v
                            ),
                        )
                        .with_code("quill::format_literal_reserved_char".to_string()),
                    );
                }
            }
        }
    }

    /// Recursively validate field-level blueprint constraints across the field,
    /// any nested object properties, and an array's element schema (`items`).
    fn validate_field_blueprint_constraints(
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        Self::validate_description_singleline(schema.description.as_deref(), owner_label, errors);
        Self::validate_enum_literals(schema, owner_label, errors);
        if let Some(v) = &schema.example {
            Self::validate_schema_slot("example", v, schema, owner_label, errors);
        }
        if let Some(v) = &schema.default {
            Self::validate_schema_slot("default", v, schema, owner_label, errors);
        }
        if let Some(props) = &schema.properties {
            for (name, prop) in props {
                let nested = format!("{}.{}", owner_label, name);
                Self::validate_field_blueprint_constraints(prop, &nested, errors);
            }
        }
        if let Some(items) = &schema.items {
            let nested = format!("{}[]", owner_label);
            Self::validate_field_blueprint_constraints(items, &nested, errors);
        }
    }

    /// Validate a single `example:` or `default:` literal against the declared
    /// schema, pushing `quill::*`-namespaced [`Diagnostic`]s for any violations.
    ///
    /// Delegates type/enum/format/recursion checking to
    /// [`super::validation::validate_schema_literal`] — the shared conformance
    /// primitive — then converts each [`ValidationError`] into a Quill.yaml
    /// load-time diagnostic with the appropriate `quill::{slot}_*` error code
    /// and author-friendly hint.
    fn validate_schema_slot(
        slot: &str,
        value: &QuillValue,
        schema: &FieldSchema,
        owner_label: &str,
        errors: &mut Vec<Diagnostic>,
    ) {
        use super::validation::{validate_schema_literal, ValidationError};

        for violation in validate_schema_literal(schema, value, owner_label) {
            let diag = match &violation {
                ValidationError::TypeMismatch {
                    path,
                    actual,
                    source_token,
                    ..
                } => {
                    // Use the field's declared `type:` verbatim (`datetime`,
                    // `markdown`, …); the validator's `expected` collapses those
                    // to `string`, which would misreport the author's intent.
                    let declared = schema.r#type.as_str();
                    // validation.rs uses "number" for all non-integer JSON numbers;
                    // display as "float" so messages match the YAML author's mental model.
                    let display_actual = if actual == "number" {
                        "float"
                    } else {
                        actual.as_str()
                    };
                    // Show the offending value's content. A top-level mismatch
                    // renders the original literal (so arrays/objects show their
                    // contents); a nested mismatch is always a scalar, whose
                    // verbatim token is already the full value.
                    let preview = if path.as_str() == owner_label {
                        Self::literal_preview(value.as_json())
                    } else {
                        Self::truncate_preview(source_token)
                    };
                    let hint = if actual == "number" || actual == "integer" {
                        let schema_type = if actual == "integer" {
                            "integer"
                        } else {
                            "number"
                        };
                        format!(
                            "Quote the {slot} as \"{raw}\" if the value is intentionally a \
                             string, or change the field type to '{schema_type}'.",
                            raw = source_token.trim_matches('"'),
                        )
                    } else if actual == "string" {
                        format!(
                            "Remove the quotes around the {slot} value to keep it a {declared}."
                        )
                    } else {
                        format!(
                            "Make the {slot} value a {declared}, or change the field type to match."
                        )
                    };
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{owner_label} declares type '{declared}' but {slot} is {display_actual} ({preview})."
                        ),
                    )
                    .with_code(format!("quill::{slot}_type_mismatch"))
                    .with_hint(hint)
                }
                ValidationError::EnumViolation {
                    path,
                    value: val,
                    allowed,
                } => {
                    let values_str = allowed
                        .iter()
                        .map(|v| format!("\"{}\"", v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "{path} {slot} \"{val}\" is not one of the declared enum values [{values_str}]."
                        ),
                    )
                    .with_code(format!("quill::{slot}_not_in_enum"))
                    .with_hint(format!("Set the {slot} to one of: {values_str}."))
                }
                ValidationError::FormatViolation { path, format } => Diagnostic::new(
                    Severity::Error,
                    format!("{path} {slot} has an invalid {format} format."),
                )
                .with_code(format!("quill::{slot}_format_violation"))
                .with_hint(format!("Provide a valid {format} value for the {slot}.")),
                // UnknownCard, BodyDisabled do not apply to schema literals.
                _ => continue,
            };
            errors.push(diag);
        }
    }

    /// Render a short, quoted preview of a value for an error message. Strings
    /// are quoted; everything else uses its JSON form. Long renderings are
    /// truncated (see [`Self::truncate_preview`]).
    fn literal_preview(value: &serde_json::Value) -> String {
        let raw = match value {
            serde_json::Value::String(s) => format!("\"{}\"", s),
            other => other.to_string(),
        };
        Self::truncate_preview(&raw)
    }

    /// Truncate an already-rendered preview token to at most 60 characters,
    /// appending an ellipsis when it overflows.
    fn truncate_preview(raw: &str) -> String {
        const MAX: usize = 60;
        if raw.chars().count() > MAX {
            let truncated: String = raw.chars().take(MAX).collect();
            format!("{}…", truncated)
        } else {
            raw.to_string()
        }
    }

    /// Parse fields from a JSON map into `FieldSchema`s (both `main.fields` and
    /// a card kind's `fields`), assigning each field's `ui.order` from its
    /// position in `key_order` (definition order). `context` labels error
    /// messages (e.g. `"field schema"`, `"card_kind 'note' field"`).
    fn parse_fields_with_order(
        fields_map: &serde_json::Map<String, serde_json::Value>,
        key_order: &[String],
        context: &str,
        errors: &mut Vec<Diagnostic>,
    ) -> BTreeMap<String, FieldSchema> {
        let mut fields = BTreeMap::new();
        let mut fallback_counter = 0;

        for (field_name, field_value) in fields_map {
            if !Self::is_snake_case_identifier(field_name) {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "Invalid {} '{}': field keys must be snake_case \
                             (lowercase letters, digits, and underscores only), \
                             and capitalized field keys are reserved.",
                            context, field_name
                        ),
                    )
                    .with_code("quill::invalid_field_name".to_string()),
                );
                continue;
            }

            // Determine order from key_order, or use fallback counter
            let order = if let Some(idx) = key_order.iter().position(|k| k == field_name) {
                idx as i32
            } else {
                let o = key_order.len() as i32 + fallback_counter;
                fallback_counter += 1;
                o
            };

            let quill_value = QuillValue::from_json(field_value.clone());
            match FieldSchema::from_quill_value(field_name.clone(), &quill_value) {
                Ok(mut schema) => {
                    // One recursive pass enforces the whole shape contract:
                    // containers carry the right child schema (`object` →
                    // `properties`, `array` → `items`), and nesting stops after
                    // one structural level (a typed table is the deepest shape).
                    if let Some(diag) =
                        Self::validate_field_schema_shape(&schema, field_name, ShapePosition::Top)
                    {
                        errors.push(diag);
                        continue;
                    }

                    // Always set ui.order based on position
                    if schema.ui.is_none() {
                        schema.ui = Some(UiFieldSchema {
                            title: None,
                            group: None,
                            order: Some(order),
                            compact: None,
                            multiline: None,
                        });
                    } else if let Some(ui) = &mut schema.ui {
                        if ui.order.is_none() {
                            ui.order = Some(order);
                        }
                    }

                    let owner = format!("{} '{}'", context, field_name);
                    Self::validate_field_blueprint_constraints(&schema, &owner, errors);

                    fields.insert(field_name.clone(), schema);
                }
                Err(e) => {
                    let hint = Self::field_parse_hint(field_value);
                    let mut diag = Diagnostic::new(
                        Severity::Error,
                        format!("Failed to parse {} '{}': {}", context, field_name, e),
                    )
                    .with_code("quill::field_parse_error".to_string());
                    if let Some(h) = hint {
                        diag = diag.with_hint(h);
                    }
                    errors.push(diag);
                }
            }
        }

        fields
    }

    /// Produce an actionable hint for common field schema mistakes based on the raw value.
    fn field_parse_hint(field_value: &serde_json::Value) -> Option<String> {
        if let Some(obj) = field_value.as_object() {
            if obj.contains_key("title") {
                return Some(
                    "'title' is not a valid field key; use 'description' instead.".to_string(),
                );
            }
            if obj.get("type").and_then(|v| v.as_str()) == Some("richtext(inline)") {
                return Some(format!("{RICHTEXT_INLINE_TOKEN_MSG}."));
            }
        }
        None
    }

    fn is_snake_case_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return false,
        }

        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    fn is_valid_card_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() || c == '_' => {}
            _ => return false,
        }

        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    fn is_valid_quill_name(name: &str) -> bool {
        name == "__default__" || Self::is_snake_case_identifier(name)
    }

    /// Parse QuillConfig from YAML content
    pub fn from_yaml(yaml_content: &str) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        match Self::from_yaml_with_warnings(yaml_content) {
            Ok((config, _warnings)) => Ok(config),
            Err(diags) => {
                let msg = diags
                    .iter()
                    .map(|d| d.fmt_pretty())
                    .collect::<Vec<_>>()
                    .join("\n");
                Err(msg.into())
            }
        }
    }

    /// Parse QuillConfig from YAML content while collecting non-fatal warnings.
    ///
    /// Returns `Ok((config, warnings))` on success, or `Err(errors)` containing all
    /// parse/validation errors when the config is invalid. Errors are always collected
    /// exhaustively — callers see every problem, not just the first.
    pub fn from_yaml_with_warnings(
        yaml_content: &str,
    ) -> Result<(Self, Vec<Diagnostic>), Vec<Diagnostic>> {
        let mut warnings: Vec<Diagnostic> = Vec::new();
        let mut errors: Vec<Diagnostic> = Vec::new();

        // Parse YAML into serde_json::Value via serde_saphyr. The depth budget
        // bounds nesting so an untrusted Quill.yaml cannot overflow the stack.
        // Note: serde_json with "preserve_order" feature is required for this to work as expected
        let quill_yaml_val: serde_json::Value = match serde_saphyr::from_str_with_options(
            yaml_content,
            crate::document::limits::yaml_parse_options(),
        ) {
            Ok(v) => v,
            Err(e) => {
                return Err(vec![Diagnostic::new(
                    Severity::Error,
                    format!("Failed to parse Quill.yaml: {}", e),
                )
                .with_code("quill::yaml_parse_error".to_string())]);
            }
        };

        // Extract [quill] section (required) — fail immediately if absent since all
        // subsequent validation depends on it.
        let quill_section = match quill_yaml_val.get("quill") {
            Some(v) => v,
            None => {
                return Err(vec![Diagnostic::new(
                    Severity::Error,
                    "Missing required 'quill' section in Quill.yaml".to_string(),
                )
                .with_code("quill::missing_section".to_string())
                .with_hint(
                    "Add a 'quill:' section with name, backend, version, and description."
                        .to_string(),
                )]);
            }
        };

        // Validate that no unknown keys appear in the [quill] section.
        const KNOWN_QUILL_KEYS: &[&str] =
            &["name", "backend", "description", "version", "author", "ui"];
        if let Some(quill_obj) = quill_section.as_object() {
            for key in quill_obj.keys() {
                if !KNOWN_QUILL_KEYS.contains(&key.as_str()) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Unknown key '{}' in 'quill:' section", key),
                        )
                        .with_code("quill::unknown_key".to_string())
                        .with_hint(format!("Valid keys are: {}", KNOWN_QUILL_KEYS.join(", "))),
                    );
                }
            }
        }

        // Extract required fields — collect all missing-field errors before returning.
        let name = match quill_section.get("name").and_then(|v| v.as_str()) {
            Some(n) => {
                if !Self::is_valid_quill_name(n) {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!(
                                "Invalid Quill name '{}': quill.name must be snake_case \
                                 (lowercase letters, digits, and underscores only).",
                                n
                            ),
                        )
                        .with_code("quill::invalid_name".to_string())
                        .with_hint(format!(
                            "Rename '{}' to '{}'",
                            n,
                            n.to_lowercase().replace('-', "_")
                        )),
                    );
                }
                n.to_string()
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'name' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_name".to_string())
                    .with_hint(
                        "Add 'name: your_quill_name' under the 'quill:' section.".to_string(),
                    ),
                );
                String::new()
            }
        };

        let backend = match quill_section.get("backend").and_then(|v| v.as_str()) {
            Some(b) => b.to_string(),
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'backend' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_backend".to_string())
                    .with_hint("Add 'backend: typst' (or another supported backend).".to_string()),
                );
                String::new()
            }
        };

        let description = match quill_section.get("description").and_then(|v| v.as_str()) {
            Some(d) if !d.trim().is_empty() => {
                Self::validate_description_singleline(Some(d), "quill", &mut errors);
                d.to_string()
            }
            Some(_) => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "'description' field in 'quill' section cannot be empty".to_string(),
                    )
                    .with_code("quill::empty_description".to_string()),
                );
                String::new()
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'description' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_description".to_string())
                    .with_hint("Add a brief 'description:' of what this quill is for.".to_string()),
                );
                String::new()
            }
        };

        // Extract the required `version` field.
        let version = match quill_section.get("version") {
            Some(version_val) => {
                // Handle version as string or number (YAML might parse 1.0 as number)
                let raw = if let Some(s) = version_val.as_str() {
                    s.to_string()
                } else if let Some(n) = version_val.as_f64() {
                    n.to_string()
                } else {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "Invalid 'version' field format".to_string(),
                        )
                        .with_code("quill::invalid_version".to_string())
                        .with_hint("Use semver format: '1.0' or '1.0.0'.".to_string()),
                    );
                    String::new()
                };
                if !raw.is_empty() {
                    use std::str::FromStr;
                    if let Err(e) = crate::version::Version::from_str(&raw) {
                        errors.push(
                            Diagnostic::new(
                                Severity::Error,
                                format!("Invalid version '{}': {}", raw, e),
                            )
                            .with_code("quill::invalid_version".to_string())
                            .with_hint("Use semver format: '1.0' or '1.0.0'.".to_string()),
                        );
                    }
                }
                raw
            }
            None => {
                errors.push(
                    Diagnostic::new(
                        Severity::Error,
                        "Missing required 'version' field in 'quill' section".to_string(),
                    )
                    .with_code("quill::missing_version".to_string())
                    .with_hint("Add 'version: 1.0' under the 'quill:' section.".to_string()),
                );
                String::new()
            }
        };

        let author = quill_section
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let ui_section: Option<UiCardSchema> = match quill_section.get("ui").cloned() {
            None => None,
            Some(v) => match serde_json::from_value::<UiCardSchema>(v) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Invalid 'quill.ui' block: {}", e),
                        )
                        .with_code("quill::invalid_ui".to_string())
                        .with_hint("Valid key under 'ui' is: title.".to_string()),
                    );
                    None
                }
            },
        };

        // Extract optional backend-specific section (keyed by `quill.backend`).
        let mut backend_config = HashMap::new();
        if !backend.is_empty() {
            if let Some(section_val) = quill_yaml_val.get(&backend) {
                if let Some(table) = section_val.as_object() {
                    for (key, value) in table {
                        backend_config.insert(key.clone(), QuillValue::from_json(value.clone()));
                    }
                }
            }
        }

        // Reject unknown top-level sections. Known sections are: quill, main, card_kinds,
        // and the backend name (e.g. typst). Everything else is a mistake. `fields` gets
        // a targeted hint since it's the most common shape mistake.
        if let Some(top_obj) = quill_yaml_val.as_object() {
            for key in top_obj.keys() {
                let is_known = key == "quill"
                    || key == "main"
                    || key == "card_kinds"
                    || (!backend.is_empty() && key == &backend);
                if is_known {
                    continue;
                }

                let mut diag = Diagnostic::new(
                    Severity::Error,
                    format!("Unknown top-level section '{}'", key),
                )
                .with_code("quill::unknown_section".to_string());

                diag = if key == "fields" {
                    diag.with_hint(
                        "Root-level `fields` is not supported; use `main.fields` instead."
                            .to_string(),
                    )
                } else {
                    diag.with_hint(format!(
                        "Valid top-level sections are: quill, main, card_kinds{}",
                        if backend.is_empty() {
                            String::new()
                        } else {
                            format!(", {}", backend)
                        }
                    ))
                };

                errors.push(diag);
            }
        }

        let main_obj_opt = quill_yaml_val.get("main").and_then(|v| v.as_object());

        // Extract main.fields (optional)
        let fields = if let Some(main_obj) = main_obj_opt {
            if let Some(fields_val) = main_obj.get("fields") {
                if let Some(fields_map) = fields_val.as_object() {
                    // With preserve_order feature, keys iterator respects insertion order
                    let field_order: Vec<String> = fields_map.keys().cloned().collect();
                    Self::parse_fields_with_order(
                        fields_map,
                        &field_order,
                        "field schema",
                        &mut errors,
                    )
                } else {
                    BTreeMap::new()
                }
            } else {
                BTreeMap::new()
            }
        } else {
            BTreeMap::new()
        };

        // Extract main.ui (optional). Fail loudly on malformed UI metadata rather
        // than silently dropping it — see `quill.ui` handling above.
        let main_ui: Option<UiCardSchema> = match main_obj_opt
            .and_then(|main_obj| main_obj.get("ui"))
            .cloned()
        {
            None => None,
            Some(v) => match serde_json::from_value::<UiCardSchema>(v) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    errors.push(
                        Diagnostic::new(Severity::Error, format!("Invalid 'main.ui' block: {}", e))
                            .with_code("quill::invalid_ui".to_string())
                            .with_hint("Valid key under 'ui' is: title.".to_string()),
                    );
                    None
                }
            },
        };

        // Extract main.body (optional). Fail loudly on malformed body metadata.
        let main_body: Option<BodyCardSchema> = match main_obj_opt
            .and_then(|main_obj| main_obj.get("body"))
            .cloned()
        {
            None => None,
            Some(v) => match serde_json::from_value::<BodyCardSchema>(v) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Invalid 'main.body' block: {}", e),
                        )
                        .with_code("quill::invalid_body".to_string())
                        .with_hint("Valid keys under 'body' are: enabled, example.".to_string()),
                    );
                    None
                }
            },
        };

        // Extract main.description (optional, authored under `main:` like any
        // other card kind). This is independent of `quill.description`.
        let main_description = main_obj_opt
            .and_then(|main_obj| main_obj.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Self::validate_description_singleline(main_description.as_deref(), "main", &mut errors);

        // The main entry-point card.
        let mut main = CardSchema {
            name: "main".to_string(),
            description: main_description,
            fields,
            ui: main_ui.or(ui_section),
            body: main_body,
        };

        // Extract [card_kinds] section (optional)
        let mut card_kinds: Vec<CardSchema> = Vec::new();
        if let Some(card_kinds_val) = quill_yaml_val.get("card_kinds") {
            match card_kinds_val.as_object() {
                None => {
                    errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            "'card_kinds' section must be an object (mapping of kind names to schemas)".to_string(),
                        )
                        .with_code("quill::invalid_card_kinds".to_string()),
                    );
                }
                Some(card_kinds_table) => {
                    for (card_name, card_value) in card_kinds_table {
                        if !Self::is_valid_card_identifier(card_name) {
                            errors.push(
                                Diagnostic::new(
                                    Severity::Error,
                                    format!(
                                        "Invalid card-kind name '{}': names must match \
                                         [a-z_][a-z0-9_]* (lowercase letters, digits, and underscores only).",
                                        card_name
                                    ),
                                )
                                .with_code("quill::invalid_card_name".to_string()),
                            );
                            continue;
                        }

                        // Parse card basic info using serde
                        let card_def: CardSchemaDef =
                            match serde_json::from_value(card_value.clone()) {
                                Ok(d) => d,
                                Err(e) => {
                                    errors.push(
                                        Diagnostic::new(
                                            Severity::Error,
                                            format!(
                                                "Failed to parse card_kind '{}': {}",
                                                card_name, e
                                            ),
                                        )
                                        .with_code("quill::invalid_card_schema".to_string()),
                                    );
                                    continue;
                                }
                            };

                        // Parse card fields
                        let card_fields = if let Some(card_fields_table) =
                            card_value.get("fields").and_then(|v| v.as_object())
                        {
                            let card_field_order: Vec<String> =
                                card_fields_table.keys().cloned().collect();
                            Self::parse_fields_with_order(
                                card_fields_table,
                                &card_field_order,
                                &format!("card_kind '{}' field", card_name),
                                &mut errors,
                            )
                        } else {
                            BTreeMap::new()
                        };

                        Self::validate_description_singleline(
                            card_def.description.as_deref(),
                            &format!("card_kind '{}'", card_name),
                            &mut errors,
                        );
                        card_kinds.push(CardSchema {
                            name: card_name.clone(),
                            description: card_def.description,
                            fields: card_fields,
                            ui: card_def.ui,
                            body: card_def.body,
                        });
                    }
                }
            }
        }

        // Warn when `body.example` is set together with `body.enabled: false` —
        // the example has no effect since the body editor is disabled.
        let warn_example_unused = |label: &str,
                                   body: &Option<BodyCardSchema>|
         -> Option<Diagnostic> {
            let body = body.as_ref()?;
            if body.enabled == Some(false) && body.example.is_some() {
                Some(
                    Diagnostic::new(
                        Severity::Warning,
                        format!(
                            "`{label}.body.example` is set but `{label}.body.enabled` is false; the example will have no effect"
                        ),
                    )
                    .with_code("quill::body_example_unused".to_string())
                    .with_hint(
                        "Set `body.enabled: true` to surface the example, or remove `body.example`."
                            .to_string(),
                    ),
                )
            } else {
                None
            }
        };
        if let Some(d) = warn_example_unused("main", &main.body) {
            warnings.push(d);
        }
        for card in &card_kinds {
            if let Some(d) = warn_example_unused(&format!("card_kinds.{}", card.name), &card.body) {
                warnings.push(d);
            }
        }

        // Error when `body.example` contains a line that the document parser
        // would interpret as a `~~~` card-yaml block opener. Such a line would
        // start a new metadata block, corrupting document structure.
        let err_example_contains_fence = |label: &str,
                                          body: &Option<BodyCardSchema>|
         -> Option<Diagnostic> {
            let example = body.as_ref()?.example.as_deref()?;
            if example_contains_fence_line(example) {
                Some(
                    Diagnostic::new(
                        Severity::Error,
                        format!(
                            "`{label}.body.example` contains a line that would be parsed as a `~~~` card-yaml block opener; this would corrupt the blueprint"
                        ),
                    )
                    .with_code("quill::body_example_contains_fence".to_string())
                    .with_hint(
                        "Remove or reword any column-zero line that opens a card-yaml block (`~~~`, a longer tilde run, or `~~~card-yaml`). For a literal fenced code block, use a backtick fence (```).".to_string(),
                    ),
                )
            } else {
                None
            }
        };
        if let Some(d) = err_example_contains_fence("main", &main.body) {
            errors.push(d);
        }
        for card in &card_kinds {
            if let Some(d) =
                err_example_contains_fence(&format!("card_kinds.{}", card.name), &card.body)
            {
                errors.push(d);
            }
        }

        // Import every richtext `default` / `example` / `body.example` literal
        // once into its canonical-corpus companion cache — a pure function of the
        // Quill.yaml bytes, never serialized. This is where `richtext(inline)`
        // violations and malformed richtext literals surface as load errors, and
        // where seeding and the render floor later read a pre-validated corpus
        // instead of re-importing the markdown per document.
        populate_card_corpus(&mut main, "main", &mut errors);
        for card in &mut card_kinds {
            let label = format!("card_kinds.{}", card.name);
            populate_card_corpus(card, &label, &mut errors);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok((
            QuillConfig {
                name,
                description,
                main,
                card_kinds,
                backend,
                version,
                author,
                backend_config,
            },
            warnings,
        ))
    }
}

/// Returns true if any line in `text` would be parsed as a card-yaml block
/// opener by the document parser, which would corrupt the blueprint's document
/// structure when the example is embedded verbatim as body content.
///
/// Delegates to the parser's own opener predicate
/// ([`crate::document::fences::is_card_yaml_opener_line`]) so the guard stays
/// in lock-step with fence detection: a column-zero tilde fence (three or more
/// tildes) whose info string is empty or `card-yaml`. Backtick fences,
/// language-tagged `~~~` fences, and indented fences are ordinary code blocks
/// and are not flagged.
fn example_contains_fence_line(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        crate::document::fences::is_card_yaml_opener_line(line)
    })
}

/// Whether a field's type tree contains any richtext leaf — the gate for
/// caching a corpus companion. A scalar (`string`, `integer`, …) never carries
/// one; an `array<richtext>` or an `object` with a richtext property does.
fn field_contains_richtext(field: &FieldSchema) -> bool {
    match &field.r#type {
        FieldType::RichText { .. } => true,
        FieldType::Array => field.items.as_deref().is_some_and(field_contains_richtext),
        FieldType::Object => field
            .properties
            .as_ref()
            .is_some_and(|p| p.values().any(|f| field_contains_richtext(f))),
        _ => false,
    }
}

/// Populate a field's `default_corpus` / `example_corpus` companion caches from
/// its markdown literals. No-op for a non-richtext field; a failed import or a
/// `richtext(inline)` violation is appended to `errors` as a load diagnostic.
fn populate_field_corpus(field: &mut FieldSchema, owner: &str, errors: &mut Vec<Diagnostic>) {
    if !field_contains_richtext(field) {
        return;
    }
    if let Some(default) = field.default.clone() {
        match literal_corpus(&default, field, &format!("{owner} `default`")) {
            Ok(corpus) => field.default_corpus = corpus,
            Err(d) => errors.push(d),
        }
    }
    if let Some(example) = field.example.clone() {
        match literal_corpus(&example, field, &format!("{owner} `example`")) {
            Ok(corpus) => field.example_corpus = corpus,
            Err(d) => errors.push(d),
        }
    }
}

/// Populate every richtext corpus companion on a card: each field's
/// `default`/`example`, and the card's `body.example` (block richtext — no
/// inline constraint; skipped when the body is disabled, since its example is
/// inert).
fn populate_card_corpus(card: &mut CardSchema, label: &str, errors: &mut Vec<Diagnostic>) {
    for (name, field) in card.fields.iter_mut() {
        populate_field_corpus(field, &format!("{label} field `{name}`"), errors);
    }
    let body_enabled = card.body.as_ref().is_none_or(|b| b.enabled != Some(false));
    if body_enabled {
        if let Some(body) = card.body.as_mut() {
            if let Some(example) = body.example.clone() {
                match crate::document::import_body(&example) {
                    Ok(rt) => {
                        body.example_corpus = Some(QuillValue::from_json(
                            quillmark_richtext::serial::to_canonical_value(&rt),
                        ));
                    }
                    Err(e) => errors.push(
                        Diagnostic::new(
                            Severity::Error,
                            format!("Failed to import {label} `body.example`: {e}"),
                        )
                        .with_code("quill::richtext_example_import".to_string()),
                    ),
                }
            }
        }
    }
}

/// Compute the canonical-corpus form of a richtext-bearing schema literal
/// (`default` / `example`), importing every markdown leaf once and enforcing
/// `richtext(inline)`. Recurses through `array` / `object` shapes, converting
/// only their richtext leaves and passing other elements through unchanged.
/// `Ok(None)` when the literal carries no importable richtext (a null value, or
/// a field the gate already cleared as non-richtext); `Err` is a load error.
fn literal_corpus(
    value: &QuillValue,
    field: &FieldSchema,
    label: &str,
) -> Result<Option<QuillValue>, Diagnostic> {
    let json = value.as_json();
    // Null ≡ absent — no data to import, so no companion is cached.
    if json.is_null() {
        return Ok(None);
    }
    match &field.r#type {
        FieldType::RichText { inline } => {
            let rt = match crate::document::decode_richtext_value(json) {
                Some(Ok(rt)) => rt,
                Some(Err(e)) => {
                    let reason = match e {
                        crate::document::RichtextDecodeError::BadMarkdown(m) => {
                            format!("markdown import failed: {m}")
                        }
                        crate::document::RichtextDecodeError::NotCorpus(m) => {
                            format!("not a valid richtext corpus: {m}")
                        }
                    };
                    return Err(richtext_literal_error(label, &reason));
                }
                None => {
                    return Err(richtext_literal_error(
                        label,
                        "expected a markdown string (richtext literals are authored as markdown)",
                    ));
                }
            };
            if *inline && !rt.is_inline() {
                return Err(richtext_inline_error(label));
            }
            Ok(Some(QuillValue::from_json(
                quillmark_richtext::serial::to_canonical_value(&rt),
            )))
        }
        FieldType::Array => {
            let Some(items) = field.items.as_deref() else {
                return Ok(None);
            };
            if !field_contains_richtext(items) {
                return Ok(None);
            }
            let arr = json.as_array().cloned().unwrap_or_default();
            let mut out = Vec::with_capacity(arr.len());
            for (idx, elem) in arr.iter().enumerate() {
                let elem_v = QuillValue::from_json(elem.clone());
                let corpus =
                    literal_corpus(&elem_v, items, &format!("{label}[{idx}]"))?.unwrap_or(elem_v);
                out.push(corpus.into_json());
            }
            Ok(Some(QuillValue::from_json(serde_json::Value::Array(out))))
        }
        FieldType::Object => {
            let Some(props) = field.properties.as_ref() else {
                return Ok(None);
            };
            if !props.values().any(|f| field_contains_richtext(f)) {
                return Ok(None);
            }
            let obj = json.as_object().cloned().unwrap_or_default();
            let mut out = serde_json::Map::new();
            for (k, v) in &obj {
                let converted = match props.get(k) {
                    Some(pschema) => {
                        let pv = QuillValue::from_json(v.clone());
                        literal_corpus(&pv, pschema, &format!("{label}.{k}"))?
                            .map(QuillValue::into_json)
                            .unwrap_or_else(|| v.clone())
                    }
                    None => v.clone(),
                };
                out.insert(k.clone(), converted);
            }
            Ok(Some(QuillValue::from_json(serde_json::Value::Object(out))))
        }
        _ => Ok(None),
    }
}

/// A load diagnostic for a richtext schema literal that failed to import.
fn richtext_literal_error(label: &str, reason: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        format!("Failed to import richtext {label}: {reason}"),
    )
    .with_code("quill::richtext_example_import".to_string())
}

/// A load diagnostic for a `richtext(inline)` schema literal whose corpus spans
/// more than a single paragraph.
fn richtext_inline_error(label: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        format!(
            "richtext(inline) {label} must be a single paragraph (no blank lines, \
             headings, lists, quotes, or tables)"
        ),
    )
    .with_code("richtext::not_inline".to_string())
    .with_hint(
        "Reduce the value to one paragraph, or change the field `type:` to `richtext`.".to_string(),
    )
}
