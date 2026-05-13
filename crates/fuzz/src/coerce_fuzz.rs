//! Property-based fuzz tests for `QuillConfig::coerce_frontmatter`.
//!
//! Targets the typed-coercion pipeline (`coerce_value_strict` /
//! `coerce_object_props`). Exercised entirely through the public
//! [`QuillConfig::coerce_frontmatter`] entry point so the tests don't depend
//! on internal helpers.
//!
//! ## Properties under test
//!
//! - **T1 (no-panic):** `coerce_frontmatter` returns `Ok | Err(_)` for any
//!   `(FieldSchema, serde_json::Value)` pair within the generator's bounded
//!   depth. No panics, no overflows.
//! - **T2 (well-formed path):** when coercion fails, the
//!   `CoercionError::Uncoercible.path` matches a small grammar
//!   `root_field ( '.' ident | '[' digits ']' )*`, where `ident` and
//!   `root_field` are drawn from the generator's character set.
//! - **T3 (idempotence):** for any input where `coerce_frontmatter` returns
//!   `Ok(x)`, `coerce_frontmatter(x) == Ok(x)`.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use proptest::prelude::*;
use quillmark_core::quill::{
    LeafSchema, CoercionError, FieldSchema, FieldType, QuillConfig,
};
use quillmark_core::QuillValue;

// -- Generators ---------------------------------------------------------------

// Root field name and property-name alphabet used in every generator. Keep
// these aligned with `validate_path_grammar` below so T2's check is precise.
const ROOT_FIELD: &str = "f";
const PROP_NAME_RE: &str = "[a-z]{1,4}";

fn arb_leaf_field_type() -> impl Strategy<Value = FieldType> {
    prop_oneof![
        Just(FieldType::String),
        Just(FieldType::Number),
        Just(FieldType::Integer),
        Just(FieldType::Boolean),
        Just(FieldType::Date),
        Just(FieldType::DateTime),
        Just(FieldType::Markdown),
    ]
}

/// `FieldSchema` of bounded depth. Both `Array` and `Object` carry a
/// `properties` map applied to object-shaped children (the same way
/// `coerce_value_strict` consumes it).
fn arb_field_schema(max_depth: u32) -> impl Strategy<Value = FieldSchema> {
    let leaf = arb_leaf_field_type()
        .prop_map(|ty| FieldSchema::new(String::new(), ty, None));
    leaf.prop_recursive(max_depth, 24, 3, |inner| {
        let props_map = prop::collection::btree_map(PROP_NAME_RE, inner, 1..=3)
            .prop_map(|m| {
                let mut props: BTreeMap<String, Box<FieldSchema>> = BTreeMap::new();
                for (k, mut v) in m {
                    v.name = k.clone();
                    props.insert(k, Box::new(v));
                }
                props
            });
        prop_oneof![
            // Object with 1-3 properties
            props_map.clone().prop_map(|props| {
                let mut schema = FieldSchema::new(String::new(), FieldType::Object, None);
                schema.properties = Some(props);
                schema
            }),
            // Array whose object-shaped elements share a 1-3 property schema
            props_map.prop_map(|props| {
                let mut schema = FieldSchema::new(String::new(), FieldType::Array, None);
                schema.properties = Some(props);
                schema
            }),
        ]
    })
}

/// Arbitrary JSON value of bounded depth, with finite numbers only
/// (non-finite `f64` can't round-trip through `serde_json::Number` anyway).
fn arb_json_value(max_depth: u32) -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|i| serde_json::Value::Number(serde_json::Number::from(i))),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(|f| serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)),
        ".{0,16}".prop_map(serde_json::Value::String),
    ];
    leaf.prop_recursive(max_depth, 32, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=3).prop_map(serde_json::Value::Array),
            prop::collection::btree_map(PROP_NAME_RE, inner, 0..=3).prop_map(|m| {
                let mut map = serde_json::Map::new();
                for (k, v) in m {
                    map.insert(k, v);
                }
                serde_json::Value::Object(map)
            }),
        ]
    })
}

// -- Harness helpers ----------------------------------------------------------

/// Build a minimal `QuillConfig` whose `main.fields` declares a single field
/// named [`ROOT_FIELD`] with the given schema. Bypasses `from_yaml` so the
/// generator is free to produce schemas the YAML parser would reject (e.g.
/// `Object` nested inside `Object`) — exactly the adversarial surface we want
/// `coerce_frontmatter` to survive.
fn config_with_one_field(schema: FieldSchema) -> QuillConfig {
    let mut schema = schema;
    schema.name = ROOT_FIELD.to_string();
    let mut fields = BTreeMap::new();
    fields.insert(ROOT_FIELD.to_string(), schema);
    let main = LeafSchema {
        name: "main".to_string(),
        description: None,
        fields,
        ui: None,
        body: None,
    };
    QuillConfig {
        name: "test".to_string(),
        description: String::new(),
        main,
        leaf_kinds: Vec::new(),
        backend: "typst".to_string(),
        version: "1.0".to_string(),
        author: String::new(),
        example_file: None,
        example_markdown: None,
        plate_file: None,
        backend_config: HashMap::new(),
    }
}

fn single_field_frontmatter(value: serde_json::Value) -> IndexMap<String, QuillValue> {
    let mut fm = IndexMap::new();
    fm.insert(ROOT_FIELD.to_string(), QuillValue::from_json(value));
    fm
}

/// Validate that `path` matches `ROOT_FIELD ( '.' ident | '[' digits ']' )*`,
/// where `ident` is 1-4 lowercase ASCII letters (matching `PROP_NAME_RE`).
fn validate_path_grammar(path: &str) -> bool {
    let mut rest = match path.strip_prefix(ROOT_FIELD) {
        Some(r) => r,
        None => return false,
    };
    while !rest.is_empty() {
        if let Some(after_dot) = rest.strip_prefix('.') {
            let end = after_dot
                .bytes()
                .take_while(|b| b.is_ascii_lowercase())
                .count();
            if end == 0 || end > 4 {
                return false;
            }
            rest = &after_dot[end..];
        } else if let Some(after_lbrack) = rest.strip_prefix('[') {
            let end = after_lbrack
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            if end == 0 {
                return false;
            }
            let after_num = &after_lbrack[end..];
            match after_num.strip_prefix(']') {
                Some(r) => rest = r,
                None => return false,
            }
        } else {
            return false;
        }
    }
    true
}

// -- Properties ---------------------------------------------------------------

proptest! {
    // T1 — never panic, regardless of how adversarial the (schema, value) pair is.
    #[test]
    fn coerce_never_panics(
        schema in arb_field_schema(4),
        value in arb_json_value(4),
    ) {
        let config = config_with_one_field(schema);
        let fm = single_field_frontmatter(value);
        let _ = config.coerce_frontmatter(&fm);
    }

    // T2 — when coercion fails, the error path is structurally well-formed.
    #[test]
    fn coerce_error_path_well_formed(
        schema in arb_field_schema(4),
        value in arb_json_value(4),
    ) {
        let config = config_with_one_field(schema);
        let fm = single_field_frontmatter(value);
        if let Err(CoercionError::Uncoercible { path, .. }) = config.coerce_frontmatter(&fm) {
            prop_assert!(
                validate_path_grammar(&path),
                "path `{}` does not match `{} ( '.' [a-z]{{1,4}} | '[' digits ']' )*`",
                path,
                ROOT_FIELD,
            );
        }
    }

    // T3 — idempotence on Ok: re-coercing the output yields the same output.
    #[test]
    fn coerce_is_idempotent(
        schema in arb_field_schema(4),
        value in arb_json_value(4),
    ) {
        let config = config_with_one_field(schema);
        let fm = single_field_frontmatter(value);
        if let Ok(first) = config.coerce_frontmatter(&fm) {
            let second = config
                .coerce_frontmatter(&first)
                .expect("second coerce must succeed when first did");
            prop_assert_eq!(first, second);
        }
    }
}

// -- Hand-rolled regression cases --------------------------------------------
//
// Anchor cases the generator might rarely hit; documenting the invariants in
// concrete form makes future regressions easier to spot.

#[test]
fn regression_t2_array_of_object_path() {
    // Schema: { f: array, items: { x: integer } }
    let mut inner = BTreeMap::new();
    inner.insert(
        "x".to_string(),
        Box::new(FieldSchema::new("x".to_string(), FieldType::Integer, None)),
    );
    let mut arr = FieldSchema::new(ROOT_FIELD.to_string(), FieldType::Array, None);
    arr.properties = Some(inner);
    let config = config_with_one_field(arr);

    // [ { "x": "not-an-int" } ] — should fail at f[0].x
    let val = serde_json::json!([{ "x": "not-an-int" }]);
    let err = config
        .coerce_frontmatter(&single_field_frontmatter(val))
        .expect_err("string-to-integer should fail");
    let CoercionError::Uncoercible { path, .. } = err;
    assert_eq!(path, "f[0].x");
    assert!(validate_path_grammar(&path));
}

#[test]
fn regression_t3_string_array_singleton_collapses_once() {
    // String schema with ["x"] input should collapse to "x" on the first pass
    // and stay "x" on the second.
    let schema = FieldSchema::new(ROOT_FIELD.to_string(), FieldType::String, None);
    let config = config_with_one_field(schema);
    let fm = single_field_frontmatter(serde_json::json!(["hello"]));
    let first = config.coerce_frontmatter(&fm).unwrap();
    let second = config.coerce_frontmatter(&first).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.get(ROOT_FIELD).unwrap().as_json(),
        &serde_json::Value::String("hello".to_string())
    );
}
