use super::QuillConfig;

impl QuillConfig {
    /// YAML encoding of [`QuillConfig::schema`].
    pub fn schema_yaml(&self) -> Result<String, serde_saphyr::ser::Error> {
        serde_saphyr::to_string(&self.schema())
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
cards:
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
  indorsement:
    fields:
      signature_block:
        type: string
"#;

    #[test]
    fn schema_includes_ui() {
        let config = cfg(FULL);
        let yaml = config.schema_yaml().unwrap();
        assert!(yaml.contains("enum:") && yaml.contains("type: integer"));
        assert!(yaml.contains("cards:") && yaml.contains("indorsement:"));
        assert!(yaml.contains("ui:") && yaml.contains("group: Meta"));
    }

    #[test]
    fn cards_holds_only_main_when_no_inline_kinds() {
        let yaml = cfg(r#"
quill: { name: solo, version: "1.0", backend: typst, description: x }
cards:
  main:
    fields:
      title: { type: string }
"#)
        .schema_yaml()
        .unwrap();
        assert!(yaml.contains("cards:") && yaml.contains("main:"));
        assert!(!yaml.contains("indorsement:"));
    }

    #[test]
    fn omits_ref() {
        let config = cfg(FULL);
        let yaml = config.schema_yaml().unwrap();
        assert!(!yaml.contains("ref:"));
    }

    #[test]
    fn json_yaml_parity() {
        let config = cfg(FULL);
        let parse = |yaml: &str| serde_saphyr::from_str::<serde_json::Value>(yaml).unwrap();
        assert_eq!(config.schema(), parse(&config.schema_yaml().unwrap()));
    }
}
