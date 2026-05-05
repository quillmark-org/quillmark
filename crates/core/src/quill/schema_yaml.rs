use super::QuillConfig;

impl QuillConfig {
    /// YAML encoding of [`QuillConfig::schema`] (structural schema, no ui).
    pub fn schema_yaml(&self) -> Result<String, serde_saphyr::ser::Error> {
        serde_saphyr::to_string(&self.schema())
    }

    /// YAML encoding of [`QuillConfig::form_schema`] (schema + ui hints).
    pub fn form_schema_yaml(&self) -> Result<String, serde_saphyr::ser::Error> {
        serde_saphyr::to_string(&self.form_schema())
    }
}

#[cfg(test)]
mod tests {
    use crate::quill::QuillConfig;

    fn cfg(yaml: &str) -> QuillConfig {
        QuillConfig::from_yaml(yaml).expect("valid quill yaml")
    }

    const FULL: &str = r#"
quill:
  name: full
  version: "1.0"
  backend: typst
  description: Full
main:
  fields:
    status:
      type: string
      enum: [draft, final]
      default: draft
      ui:
        group: Meta
    page_count:
      type: integer
card_types:
  indorsement:
    fields:
      signature_block:
        type: string
"#;

    #[test]
    fn schema_strips_ui_form_schema_keeps_it() {
        let config = cfg(FULL);
        let clean = config.schema_yaml().unwrap();
        assert!(clean.contains("enum:") && clean.contains("type: integer"));
        assert!(clean.contains("card_types:") && clean.contains("indorsement:"));
        assert!(!clean.contains("ui:"));

        let form = config.form_schema_yaml().unwrap();
        assert!(form.contains("ui:") && form.contains("group: Meta"));
    }

    #[test]
    fn omits_card_types_when_absent() {
        let yaml = cfg(r#"
quill: { name: solo, version: "1.0", backend: typst, description: x }
main:
  fields:
    title: { type: string }
"#)
        .schema_yaml()
        .unwrap();
        assert!(yaml.contains("main:") && !yaml.contains("card_types:"));
    }

    #[test]
    fn omits_example_and_ref() {
        let mut config = cfg(FULL);
        config.example_markdown = Some("# x".to_string());
        for yaml in [
            config.schema_yaml().unwrap(),
            config.form_schema_yaml().unwrap(),
        ] {
            assert!(!yaml.contains("example:"));
            assert!(!yaml.contains("ref:"));
        }
    }

    #[test]
    fn json_yaml_parity() {
        let config = cfg(FULL);
        let parse = |yaml: &str| serde_saphyr::from_str::<serde_json::Value>(yaml).unwrap();
        assert_eq!(config.schema(), parse(&config.schema_yaml().unwrap()));
        assert_eq!(
            config.form_schema(),
            parse(&config.form_schema_yaml().unwrap())
        );
    }
}
