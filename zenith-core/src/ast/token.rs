//! Token block and token AST types.

use super::Span;
use super::value::Dimension;

/// The five v0 token types, plus an extensibility variant for unknown types.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    Color,
    Dimension,
    Number,
    FontFamily,
    FontWeight,
    Gradient,
    Shadow,
    Filter,
    /// An unrecognized token type (forward-compat; version-relative).
    Unknown(String),
}

impl TokenType {
    /// Parse the token type from the `type` property string. Infallible: an
    /// unrecognized type is preserved as `TokenType::Unknown` (forward-compat).
    pub fn from_type_name(s: &str) -> Self {
        match s {
            "color" => Self::Color,
            "dimension" => Self::Dimension,
            "number" => Self::Number,
            "fontFamily" => Self::FontFamily,
            "fontWeight" => Self::FontWeight,
            "gradient" => Self::Gradient,
            "shadow" => Self::Shadow,
            "filter" => Self::Filter,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

/// A literal value held by a token definition.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenLiteral {
    /// A quoted string, e.g. `"#f8fafc"` or `"Inter"`.
    String(String),
    /// A dimensioned number, e.g. `(pt)48` or `(px)28`.
    Dimension(Dimension),
    /// An unannotated finite number, e.g. `1.05` or `700`.
    Number(f64),
    /// A gradient definition built from child `stop` nodes plus an optional
    /// `angle`. Gradients have no scalar value; they are carried by this
    /// dedicated literal variant.
    Gradient(GradientLiteral),
    /// A shadow definition built from child `layer` nodes. Shadows have no
    /// scalar value; they are carried by this dedicated literal variant.
    Shadow(ShadowLiteral),
    /// A filter definition built from child op nodes. Filters have no scalar
    /// value; they are carried by this dedicated literal variant.
    Filter(FilterLiteral),
}

/// The color/image filter operations, applied in source order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterKind {
    Grayscale,
    Invert,
    Sepia,
    Saturate,
    Brightness,
    Contrast,
    HueRotate,
    /// Duotone: maps each pixel's luma to a blend between a shadow color (dark
    /// areas) and a highlight color (light areas), with an optional `amount` mix
    /// factor against the original. This is the only kind that references color
    /// tokens (carried on [`FilterOp::shadow`] / [`FilterOp::highlight`]).
    Duotone,
}

impl FilterKind {
    /// Parse a filter kind from its KDL op-node name, or `None` if unrecognized.
    pub fn from_op_name(s: &str) -> Option<Self> {
        match s {
            "grayscale" => Some(Self::Grayscale),
            "invert" => Some(Self::Invert),
            "sepia" => Some(Self::Sepia),
            "saturate" => Some(Self::Saturate),
            "brightness" => Some(Self::Brightness),
            "contrast" => Some(Self::Contrast),
            "hue-rotate" => Some(Self::HueRotate),
            "duotone" => Some(Self::Duotone),
            _ => None,
        }
    }

    /// The canonical KDL op-node name for this filter kind (inverse of
    /// [`FilterKind::from_op_name`]).
    pub fn as_op_name(&self) -> &'static str {
        match self {
            Self::Grayscale => "grayscale",
            Self::Invert => "invert",
            Self::Sepia => "sepia",
            Self::Saturate => "saturate",
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::HueRotate => "hue-rotate",
            Self::Duotone => "duotone",
        }
    }
}

/// A filter token literal: an ordered list of filter operations, applied in
/// source order. At least one op is required (enforced at resolution).
#[derive(Debug, Clone, PartialEq)]
pub struct FilterLiteral {
    /// Ordered list of filter ops, in source order.
    pub ops: Vec<FilterOp>,
}

/// A single filter operation: a kind plus an optional amount. The amount is a
/// unitless number whose meaning depends on the kind (e.g. fraction for
/// `grayscale`, degrees for `hue-rotate`); `None` means "use the op's default".
#[derive(Debug, Clone, PartialEq)]
pub struct FilterOp {
    /// The filter operation kind.
    pub kind: FilterKind,
    /// The optional amount for this op. For `Duotone` this is the mix factor
    /// against the original (default applied later); for scalar kinds it is the
    /// op's own amount.
    pub amount: Option<f64>,
    /// Shadow color token id (dark-area color) — `Some` only for `Duotone` ops.
    pub shadow: Option<String>,
    /// Highlight color token id (light-area color) — `Some` only for `Duotone`
    /// ops.
    pub highlight: Option<String>,
}

/// Whether a gradient token is linear or radial.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GradientKind {
    /// A linear gradient (default). An `angle_deg` controls the gradient line.
    #[default]
    Linear,
    /// A radial gradient. `center_x`, `center_y`, and `radius` control the
    /// origin and extent, each as a fraction of the bounding box.
    Radial,
}

/// A gradient token literal: either linear (angle + stops) or radial
/// (center + radius + stops).
#[derive(Debug, Clone, PartialEq)]
pub struct GradientLiteral {
    /// Whether the gradient is linear or radial.
    pub kind: GradientKind,
    /// Angle in degrees, clockwise from +x (0 = left→right, 90 = top→bottom).
    /// Relevant only for `kind == Linear`; ignored for radial.
    pub angle_deg: f64,
    /// Radial gradient center X as a fraction of the bounding-box width (0..1).
    /// `None` defaults to `0.5` (center). Ignored for linear.
    pub center_x: Option<f64>,
    /// Radial gradient center Y as a fraction of the bounding-box height (0..1).
    /// `None` defaults to `0.5` (center). Ignored for linear.
    pub center_y: Option<f64>,
    /// Radial gradient radius as a fraction of the box diagonal (`hypot(w,h)/2`).
    /// `None` defaults to `1.0`. Must be > 0 if specified.
    pub radius: Option<f64>,
    /// Ordered list of stop references, in source order.
    pub stops: Vec<GradientStopRef>,
}

/// A single gradient stop: an offset in `0..1` and a reference to a color token.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientStopRef {
    /// Position of the stop along the gradient axis, in `0.0..=1.0`.
    pub offset: f64,
    /// The id of the color token this stop renders with.
    pub color_token: String,
}

/// A shadow token literal: an ordered list of shadow layers (e.g. a drop
/// shadow plus an outer glow). At least one layer is required (enforced at
/// resolution).
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowLiteral {
    /// Ordered list of layer references, in source order.
    pub layers: Vec<ShadowLayerRef>,
}

/// A single shadow layer: x/y offsets and blur radius (pixels) plus a reference
/// to a color token. A layer with nonzero dx/dy is a drop shadow; a layer with
/// dx=dy=0 and a blur is an outer glow.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowLayerRef {
    /// Horizontal offset in pixels.
    pub dx: f64,
    /// Vertical offset in pixels.
    pub dy: f64,
    /// Blur radius in pixels.
    pub blur: f64,
    /// The id of the color token this layer renders with.
    pub color_token: String,
}

/// The value of a token — either an inline literal or an alias to another token.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenValue {
    /// A literal token value.
    Literal(TokenLiteral),
    /// An alias to another token, e.g. `(token)"color.text.primary"`.
    Reference { token_id: String },
}

/// A single design token within a `tokens` block.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Globally unique token ID.
    pub id: String,
    /// The token's declared type.
    pub token_type: TokenType,
    /// The token's declared value (literal or reference).
    pub value: TokenValue,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// The top-level `tokens` block with its required `format` attribute.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenBlock {
    /// Must be `"zenith-token-v1"` in v0.
    pub format: String,
    /// The ordered list of token definitions.
    pub tokens: Vec<Token>,
}

impl Default for TokenBlock {
    fn default() -> Self {
        Self {
            format: "zenith-token-v1".to_owned(),
            tokens: Vec::new(),
        }
    }
}
