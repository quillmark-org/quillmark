//! The backend-agnostic field specification: what a widget *is*, in PDF user
//! space. Both backends reduce to a `&[FieldSpec]` — the only thing they differ
//! on is where the geometry comes from (Typst introspection vs `form.json`).

/// One AcroForm field to stamp onto a base PDF.
///
/// Geometry is final: `rect` is in PDF user-space points, bottom-left origin,
/// on page `page` (0-based). Callers that work in a top-left coordinate system
/// (e.g. the Typst backend) convert before constructing the spec — this crate
/// never reasons about page height.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    /// Fully-qualified partial field name (`/T`). Unique within the document.
    pub name: String,
    /// 0-based page index this widget lands on.
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points, bottom-left origin.
    pub rect: [f32; 4],
    /// Field type and its type-specific data (`/FT` plus value shape).
    pub field_type: FieldType,
    /// `/Ff` field-flags bitfield (0 = none). Type-intrinsic flags such as the
    /// combo bit are added by the writer; this is for extras (ReadOnly,
    /// Required, Multiline, …).
    pub flags: u32,
    /// `/MaxLen` for text fields.
    pub max_len: Option<u32>,
    /// `/V` current value (text/choice). For checkboxes use [`FieldType::Checkbox`]'s
    /// state; this is ignored there.
    pub value: Option<String>,
    /// `/DV` default value.
    pub default_value: Option<String>,
    /// `/TU` tooltip / alternate description — load-bearing for 508 accessibility.
    pub tooltip: Option<String>,
    /// Visual styling: `/DA` text appearance and `/MK` border/background.
    pub appearance: Appearance,
}

impl FieldSpec {
    /// A minimally-specified field of `field_type` at `rect` on `page`.
    pub fn new(
        name: impl Into<String>,
        page: usize,
        rect: [f32; 4],
        field_type: FieldType,
    ) -> Self {
        Self {
            name: name.into(),
            page,
            rect,
            field_type,
            flags: 0,
            max_len: None,
            value: None,
            default_value: None,
            tooltip: None,
            appearance: Appearance::default(),
        }
    }

    /// The phase-1 regions-sidecar projection of this field: name + geometry +
    /// a discriminated `kind`. `kind` is an enum from day one so later region
    /// phases (named markup regions) are purely additive.
    pub fn to_region(&self) -> RenderedRegion {
        RenderedRegion {
            name: self.name.clone(),
            page: self.page,
            rect: self.rect,
            kind: RegionKind::Field {
                field_type: self.field_type.kind_str().to_string(),
                value: self.value.clone(),
            },
        }
    }
}

/// Field type plus the data unique to it.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// `/FT /Tx` — single- or multi-line text. Value flows through [`FieldSpec::value`].
    Text,
    /// `/FT /Btn` checkbox. `on_state` is the export value (the `/AS` / `/V`
    /// name when checked, e.g. `"Yes"`); `checked` selects on vs `/Off`.
    Checkbox { on_state: String, checked: bool },
    /// `/FT /Ch` — list (`combo == false`) or dropdown (`combo == true`).
    Choice {
        options: Vec<ChoiceOption>,
        combo: bool,
    },
    /// `/FT /Sig` — unsigned signature field.
    Signature,
}

impl FieldType {
    /// Stable lowercase discriminant for the regions sidecar / diagnostics.
    pub fn kind_str(&self) -> &'static str {
        match self {
            FieldType::Text => "text",
            FieldType::Checkbox { .. } => "checkbox",
            FieldType::Choice { .. } => "choice",
            FieldType::Signature => "signature",
        }
    }

    /// Whether stamping this field needs a text appearance (`/DA`) and a font
    /// in the AcroForm `/DR`. Signatures never paint glyphs themselves.
    pub fn needs_appearance(&self) -> bool {
        !matches!(self, FieldType::Signature)
    }
}

/// One choice option: an export value and an optional human-facing display
/// string. Maps to a `/Opt` entry — a bare string when `display` is `None`, a
/// two-element `[export display]` array otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    pub export: String,
    pub display: Option<String>,
}

impl ChoiceOption {
    pub fn new(export: impl Into<String>) -> Self {
        Self {
            export: export.into(),
            display: None,
        }
    }
}

/// Widget styling. `da` is the `/DA` default-appearance string controlling font
/// and color; `0 Tf` (the default) means viewer auto-size. The two colors map
/// to `/MK` `/BC` (border) and `/BG` (background).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Appearance {
    /// `/DA` string, e.g. `"/Helv 0 Tf 0 g"`. `None` inherits the document
    /// default appearance written into the AcroForm.
    pub da: Option<String>,
    /// `/MK /BC` border color, RGB in 0..=1.
    pub border_color: Option<[f32; 3]>,
    /// `/MK /BG` background color, RGB in 0..=1.
    pub background_color: Option<[f32; 3]>,
}

/// A field's geometry as reported back to the GUI for its interactivity overlay
/// (field↔region navigation, click-to-field). Phase 1 of the regions sidecar:
/// fields only.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRegion {
    pub name: String,
    pub page: usize,
    /// `[x0, y0, x1, y1]` in PDF points, bottom-left origin.
    pub rect: [f32; 4],
    pub kind: RegionKind,
}

/// Discriminated region kind. Only [`RegionKind::Field`] exists today; the enum
/// shape is deliberate so later phases (named markup regions) extend it without
/// breaking consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum RegionKind {
    Field {
        field_type: String,
        value: Option<String>,
    },
}
