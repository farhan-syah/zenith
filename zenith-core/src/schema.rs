//! Static schema metadata for the authorable node kinds and non-node surfaces.
//!
//! Exposes the canonical list of node kinds, one-line summaries, and the
//! recognized attribute names for each kind. The attribute list is derived
//! directly from the parser's own `known_props_for_kind` table so the two
//! can never silently diverge.
//!
//! Also exposes `page_attributes`, `asset_attributes`, and
//! `document_attributes` for the three non-node authorable surfaces, derived
//! from the same parser-side `PAGE_KNOWN_PROPS`, `ASSET_KNOWN_PROPS`, and
//! `DOCUMENT_KNOWN_PROPS` constants.
//!
//! Token-type schema (`token_types`, `token_type_summary`, `token_type_descriptor`)
//! mirrors the node-kind surface and provides agent-readable value-form, child-node
//! structure, and minimal correct examples for every authorable token type.

use crate::diag_catalog::{DIAGNOSTIC_CODES, DIAGNOSTIC_VERBS, DiagnosticCodeInfo};
use crate::parse::transform::PAGE_KNOWN_PROPS;
use crate::parse::transform::{ASSET_KNOWN_PROPS, DOCUMENT_KNOWN_PROPS, known_props_for_kind};

// ── Canonical kind list ───────────────────────────────────────────────────────

/// All authorable node kinds in their canonical KDL-name form.
///
/// `Unknown` is excluded: it is a forward-compat placeholder, not an authorable
/// kind. The list is sorted for deterministic output.
pub fn node_kinds() -> &'static [&'static str] {
    // Exhaustive correspondence is enforced by the `node_variant_count_exhaustive`
    // helper in the `#[cfg(test)]` drift-guard below: adding a new `Node` variant
    // without updating that match causes a compile error in the tests module.
    &[
        "code",
        "connector",
        "ellipse",
        "field",
        "footnote",
        "frame",
        "group",
        "image",
        "chart",
        "instance",
        "line",
        "pattern",
        "polygon",
        "polyline",
        "rect",
        "shape",
        "table",
        "text",
        "toc",
    ]
}

// ── One-line summaries ────────────────────────────────────────────────────────

/// Return a one-line description of the named node kind, or `None` if the kind
/// is not recognised.
///
/// The `match` arm set here must stay exhaustive over `node_kinds()`. The
/// drift-guard test `node_summary_covers_every_node_kind` enforces that.
pub fn node_summary(kind: &str) -> Option<&'static str> {
    match kind {
        "rect" => Some("Rectangle with optional fill, stroke, and rounded corners."),
        "ellipse" => Some("Ellipse or circle with optional fill and stroke."),
        "line" => Some("Straight line segment between two endpoints."),
        "text" => Some("Multi-span text block with typography and layout properties."),
        "code" => Some("Monospace code block with syntax-theme highlighting."),
        "frame" => Some("Container that clips and positions its children within a fixed box."),
        "group" => Some("Transparent grouping container for related nodes."),
        "image" => Some("Raster or SVG image positioned within a bounding box."),
        "polygon" => Some("Closed polygon defined by an ordered vertex list."),
        "polyline" => Some("Open polyline defined by an ordered vertex list."),
        "instance" => Some("Reference to a master component, optionally with overrides."),
        "field" => Some("Editable variable-data text field bound to a named slot."),
        "footnote" => Some("Page-level footnote referenced by text span markers."),
        "toc" => Some("Table-of-contents placeholder resolved to text by the scene compiler."),
        "table" => Some("Structured data table with columns, rows, and cells."),
        "shape" => Some("Preset geometric shape with an optional text label."),
        "connector" => Some("Directed connector line between two anchor points on nodes."),
        "pattern" => Some("Procedural grid or scatter tiling of one motif node."),
        "chart" => Some(
            "Data-visualization chart (bar, line, area, sparkline, pie, donut) with inline series data.",
        ),
        _ => None,
    }
}

// ── Child content descriptors ─────────────────────────────────────────────────

/// Full content descriptor for a node kind that accepts authorable child content.
///
/// Returned by [`node_content`].
pub struct NodeContentDescriptor {
    /// Short prose description of what the child content represents.
    pub description: &'static str,
    /// A minimal, syntactically correct example of the child content written
    /// inside the parent node's block (without the surrounding node wrapper).
    pub example: &'static str,
}

/// Return the child-content descriptor for the given node kind, or `None` if
/// the kind accepts no authorable child content (e.g. `rect`, `ellipse`, `line`).
///
/// The match here is exhaustive over all authorable node kinds so that adding a
/// new kind forces a deliberate decision about child content at compile time.
/// Kinds with no child content return `None`.
pub fn node_content(kind: &str) -> Option<NodeContentDescriptor> {
    match kind {
        // ── Span-bearing kinds ────────────────────────────────────────────────
        "text" => Some(NodeContentDescriptor {
            description: "One or more `span` children carry the text runs. \
                Each span takes a string argument and optional inline style props: \
                fill, font-weight, italic, underline, strikethrough, vertical-align, footnote-ref.",
            example: concat!(
                "span \"Hello \"\n",
                "span \"world\" font-weight=(token)\"weight.bold\" italic=#true",
            ),
        }),
        "shape" => Some(NodeContentDescriptor {
            description: "Optional `span` children form a text label rendered centered inside the \
                shape. Use h-align/v-align on the shape node to adjust alignment. \
                Omit the block entirely for an unlabelled shape.",
            example: "span \"Approve\"",
        }),
        "footnote" => Some(NodeContentDescriptor {
            description: "One or more `span` children carry the footnote body text, \
                using the same span model as `text`.",
            example: "span \"See also Chapter 3.\"",
        }),

        // ── Vertex-bearing kinds ──────────────────────────────────────────────
        "polygon" => Some(NodeContentDescriptor {
            description: "Two or more `point` children define the closed vertex list in order. \
                Each point carries `x` and `y` as px-literal dimensions.",
            example: concat!(
                "point x=(px)0 y=(px)0\n",
                "point x=(px)100 y=(px)0\n",
                "point x=(px)50 y=(px)86",
            ),
        }),
        "polyline" => Some(NodeContentDescriptor {
            description: "Two or more `point` children define the open vertex list in order. \
                Each point carries `x` and `y` as px-literal dimensions.",
            example: concat!(
                "point x=(px)0 y=(px)0\n",
                "point x=(px)100 y=(px)50\n",
                "point x=(px)200 y=(px)0",
            ),
        }),

        // ── Structured container kinds ────────────────────────────────────────
        "table" => Some(NodeContentDescriptor {
            description: "Optional `column` children (each with `width=(px)N`) declare column \
                widths; then `row` children each containing `cell` children. \
                Each cell accepts colspan, rowspan, fill, border, h-align, v-align, \
                and arbitrary renderable child nodes for cell content.",
            example: concat!(
                "column width=(px)120\n",
                "column width=(px)80\n",
                "row {\n",
                "    cell { text id=\"h1\" x=(px)0 y=(px)0 w=(px)120 h=(px)24 { span \"Name\" } }\n",
                "    cell { text id=\"h2\" x=(px)0 y=(px)0 w=(px)80 h=(px)24 { span \"Score\" } }\n",
                "}",
            ),
        }),

        // ── Generic container kinds ───────────────────────────────────────────
        "frame" => Some(NodeContentDescriptor {
            description: "Arbitrary renderable child nodes (any node kind). \
                The frame clips its children to its bounding box. \
                Use layout=\"grid\" with columns/rows attrs for grid layout.",
            example: "rect id=\"bg\" x=(px)0 y=(px)0 w=(px)400 h=(px)300 fill=(token)\"color.bg\"",
        }),
        "group" => Some(NodeContentDescriptor {
            description: "Arbitrary renderable child nodes (any node kind). \
                May also include `protected-region id=... x=... y=... w=... h=...` \
                and `editable-param id=...` metadata children.",
            example: "rect id=\"box\" x=(px)0 y=(px)0 w=(px)100 h=(px)100",
        }),

        // ── Series-bearing kind ───────────────────────────────────────────────
        "chart" => Some(NodeContentDescriptor {
            description: "Optional `categories` child carries the X-axis category labels as \
                positional string arguments (one per slot; absent = derive index labels at render). \
                Optional `label-colors` child carries per-slice value-label colors as positional \
                PropertyValue arguments (e.g. `(token)\"color.x\"`; one per category in order; \
                absent = use the chart value-color or the white on-fill default). \
                Zero or more `series` children carry the numeric data. \
                Each series node takes its f64 data values as positional arguments \
                and optional named props: label, color (token ref), label-color (token ref), data-ref. \
                Emit `categories` then `label-colors` before any `series` children.",
            example: concat!(
                "categories \"Q1\" \"Q2\" \"Q3\" \"Q4\"\n",
                "label-colors (token)\"color.c1\" (token)\"color.c2\" (token)\"color.c3\" (token)\"color.c4\"\n",
                "series label=\"Revenue\" color=(token)\"color.primary\" label-color=(token)\"color.lbl\" 120.0 200.0 150.0 310.0\n",
                "series label=\"Costs\" color=(token)\"color.secondary\" 80.0 90.0 100.0 120.0",
            ),
        }),

        // ── Motif-bearing kind ────────────────────────────────────────────────
        "pattern" => Some(NodeContentDescriptor {
            description: "Exactly one required child node — the motif — which is the template \
                node that gets tiled. Any authorable node kind is valid as the motif.",
            example: "rect id=\"dot\" x=(px)0 y=(px)0 w=(px)8 h=(px)8 fill=(token)\"color.accent\"",
        }),

        // ── Override-bearing kind ─────────────────────────────────────────────
        "instance" => Some(NodeContentDescriptor {
            description: "Zero or more `override` children apply per-node property overrides \
                to descendants of the referenced component. Each override targets a node by \
                `ref=\"id\"` and accepts fill, visible, and optional `span` children to \
                replace text content.",
            example: concat!(
                "override ref=\"headline\" fill=(token)\"color.alt\" {\n",
                "    span \"New headline text\"\n",
                "}",
            ),
        }),

        // ── Verbatim-content kind ─────────────────────────────────────────────
        "code" => Some(NodeContentDescriptor {
            description: "A single `content` child carries the verbatim source string as its \
                first positional argument. Newlines and tabs are expressed as \\n and \\t \
                escape sequences in the string literal.",
            example: "content \"fn main() {\\n    println!(\\\"hello\\\");\\n}\"",
        }),

        // ── Connector label ───────────────────────────────────────────────────
        "connector" => Some(NodeContentDescriptor {
            description: "Optional `span` children form a text label rendered at the \
                connector's geometric midpoint (the mid-point of the routed polyline). \
                Use `text-style` on the connector node to apply a style ref to the label. \
                Omit the block entirely (or author no `span` children) for an unlabelled \
                connector — the render output is byte-identical when no spans are present.",
            example: "span \"Yes\"",
        }),

        // ── No authorable child content ───────────────────────────────────────
        "rect" | "ellipse" | "line" | "image" | "field" | "toc" => None,

        // Any unrecognised kind also has no content description.
        _ => None,
    }
}

// ── Attribute names ───────────────────────────────────────────────────────────

/// Return the recognized attribute names for the given node kind.
///
/// Derived from the parser's own known-props table (same source of truth as
/// the validator's "did you mean?" helper). Alias spellings (e.g. `stroke_width`
/// alongside `stroke-width`) are de-duplicated to their canonical kebab-case
/// form and the result is sorted for deterministic output.
///
/// Returns an empty slice for unrecognised kinds or kinds without a fixed
/// prop list (e.g. "cell", "row", "column").
pub fn node_attributes(kind: &str) -> Vec<&'static str> {
    // The parser's known-props table carries BOTH spellings of hyphenated
    // attributes (e.g. `stroke-width` and `stroke_width`) for lenient parsing.
    // For the schema surface we collapse each pair to its canonical kebab-case
    // form via `dedupe_to_kebab`, then sort + dedup for deterministic output.
    dedupe_to_kebab(known_props_for_kind(kind))
}

// ── Non-node surface summaries ────────────────────────────────────────────────

/// One-line description of the `page` surface.
pub fn page_summary() -> &'static str {
    "Page declaration — geometry (w/h), margins, bleed, baseline grid, and workflow metadata."
}

/// One-line description of the `asset` surface.
pub fn asset_summary() -> &'static str {
    "Asset declaration (image/svg/font) — provenance including sha256 and AI-generation fields."
}

/// One-line description of the `document` surface (the root `zenith` node).
pub fn document_summary() -> &'static str {
    "Document root — colorspace, pagination, spread gutter, and document-level default margins."
}

// ── Non-node surface attribute lists ─────────────────────────────────────────

/// Return the recognized attribute names for a `page` node.
///
/// Derived from the parser's own `PAGE_KNOWN_PROPS` constant. Alias spellings
/// (e.g. `margin_inner` alongside `margin-inner`) are de-duplicated to their
/// canonical kebab-case form; the result is sorted for deterministic output.
pub fn page_attributes() -> Vec<&'static str> {
    dedupe_to_kebab(PAGE_KNOWN_PROPS)
}

/// Return the recognized attribute names for an `asset` declaration node.
///
/// Derived from the parser's own `ASSET_KNOWN_PROPS` constant, sorted and
/// de-duplicated for deterministic output.
pub fn asset_attributes() -> Vec<&'static str> {
    dedupe_to_kebab(ASSET_KNOWN_PROPS)
}

/// Return the recognized attribute names for the root `zenith` document node.
///
/// Derived from the parser's own `DOCUMENT_KNOWN_PROPS` constant. Alias
/// spellings (e.g. `doc_id` alongside `doc-id`) are de-duplicated to their
/// canonical kebab-case form; the result is sorted for deterministic output.
pub fn document_attributes() -> Vec<&'static str> {
    dedupe_to_kebab(DOCUMENT_KNOWN_PROPS)
}

// ── Attribute type hints ──────────────────────────────────────────────────────

/// Return a concise, agent-readable type hint for the named attribute on the
/// given node kind.
///
/// This is the accurate, kind-aware entry point.  For attributes whose type
/// depends on the node kind (primarily the paint/visual attributes) the hint
/// reflects what the validator actually enforces for that specific kind.
///
/// Use `attribute_type` for non-node surfaces (page, asset, document) where
/// the attribute name alone is sufficient.
///
/// The hint describes *what value to write*, not the Rust representation.
/// Categories used:
/// - `"px literal"` — bare number suffixed `px` (e.g. `x=100px`).
/// - `"token ref: <kind>"` — a token identifier from the token block (must
///   reference a declared token, never a raw literal; e.g. `fill="color.brand"`).
/// - `"f64 (0.0–1.0)"` — bare floating-point ratio.
/// - `"i64"` — integer.
/// - `"bool"` — `true` or `false`.
/// - `"string"` — arbitrary string.
/// - `"node id"` — the `id` of another node in the document.
/// - `"string (enum)"` — one of a fixed set of values (exact set confirmed in
///   the validator; use `zenith validate` for the authoritative list).
/// - `"enum: a|b|…"` — one of the explicitly-listed values.
///
/// Returns `"string"` as a safe fallback for anything not in the dictionary.
pub fn attribute_type_for_kind(kind: &str, name: &str) -> &'static str {
    attribute_type_for_kind_inner(kind, name, "string")
}

/// Return a concise, agent-readable type hint for the named attribute.
///
/// This is the kind-agnostic entry point intended for non-node surfaces (page,
/// asset, document) where the attribute name alone determines the type.  For
/// node attributes, prefer [`attribute_type_for_kind`] to get accurate
/// per-kind paint/visual hints.
///
/// The completeness drift test (`attribute_type_covers_all_known_attrs`) uses
/// the kind-agnostic path with the sentinel `"<unmapped>"` fallback.
pub fn attribute_type(name: &str) -> &'static str {
    // Non-node surfaces carry no fill/stroke, so the kind-agnostic path is
    // accurate for them.  We route through the kind-aware inner function with
    // an empty kind string; the paint-specific branch will fall through to the
    // generic arms.
    attribute_type_for_kind_inner("", name, "string")
}

/// Internal helper — kind-aware attribute type resolution.
///
/// `kind` is the canonical node-kind string (e.g. `"rect"`, `"text"`) or `""`
/// for non-node surfaces.  `fallback` is `"string"` for the public APIs and
/// `"<unmapped>"` in the completeness drift test.
fn attribute_type_for_kind_inner(kind: &str, name: &str, fallback: &'static str) -> &'static str {
    // ── Kind-specific overrides for paint/visual attributes ───────────────
    //
    // These must be checked first, before the generic arm, because the correct
    // token type varies by node kind.  Each entry is verified against the
    // validator's `VisualExpect` at the cited source location.
    //
    // Also covers enum attributes whose value set differs by node kind (e.g.
    // `kind` means different things on `shape` vs `pattern`).
    match (kind, name) {
        // fill: ColorOrGradient — rect (leaf.rs check_visual_props→shared.rs:804),
        //   ellipse (leaf.rs:218), polygon (special.rs:83), polyline (special.rs:213),
        //   pattern (pattern.rs:101→shared.rs:804).
        ("rect" | "ellipse" | "polygon" | "polyline" | "pattern" | "chart", "fill") => {
            "token ref: color/gradient"
        }
        // fill: Color — text (text.rs:113), shape (shape.rs:108), code (leaf.rs:561).
        // table fill is also Color (container.rs:304→312).
        ("text" | "shape" | "code" | "table", "fill") => "token ref: color",
        // stroke: Color on every node kind that has it — verified at:
        //   shared.rs:813 (rect/pattern), leaf.rs:227 (ellipse), leaf.rs:409 (line),
        //   special.rs:92 (polygon), special.rs:222 (polyline),
        //   text.rs:122, shape.rs:117, shape.rs:248 (connector).
        // There is no node kind where stroke accepts a gradient.
        (_, "stroke") => "token ref: color",
        // shadow / filter / mask: dedicated token types; NOT color/gradient.
        // Verified at shared.rs:960 (Shadow), 969 (Filter), 978 (Mask).
        // These are only present on kinds that go through check_visual_props
        // (rect, pattern) or equivalent, but the type is uniform across all kinds.
        (_, "shadow") => "token ref: shadow",
        (_, "filter") => "token ref: filter",
        (_, "mask") => "token ref: mask",
        // background: page surface only; accepts Color or Gradient (driver.rs:639).
        // The empty-kind non-node path also falls here for correctness.
        (_, "background") => "token ref: color/gradient",
        // Per-side border colors and stroke-outer: Color (shared.rs:887).
        (_, "border-top" | "border-bottom" | "border-left" | "border-right" | "stroke-outer") => {
            "token ref: color"
        }
        // border (table): Color (container.rs:305→312).
        (_, "border") => "token ref: color",
        // contrast-bg (text): Color (text.rs:139).
        // header-fill (table): Color (container.rs:306→312).
        (_, "contrast-bg" | "header-fill") => "token ref: color",
        // kind: the `kind` attribute means different things on different surfaces.
        //   shape: process/decision/terminator/ellipse (validate/check/nodes/node/shape.rs:152).
        //   pattern: grid/scatter (validate/check/nodes/node/pattern.rs:140).
        //   asset surface (kind=""): image/svg/font (ast/asset.rs:29-33).
        // The attribute name alone is insufficient — each surface has a distinct enum.
        ("shape", "kind") => "enum: process|decision|terminator|ellipse",
        ("pattern", "kind") => "enum: grid|scatter",
        ("chart", "kind") => "enum: bar|line|area|sparkline|pie|donut",
        // chart axis/legend/caption/bar-mode: chart-only attributes (validate/check/nodes/node/chart.rs).
        ("chart", "legend") => "bool",
        ("chart", "caption") => "string",
        ("chart", "axis-min" | "axis-max") => "f64",
        ("chart", "axis-style") => "string",
        ("chart", "bar-mode") => "enum: grouped|stacked",
        ("chart", "point-placement") => "enum: edge|center",
        ("chart", "value-labels") => "enum: auto|none|top|center",
        ("chart", "value-color") => "token ref: color",
        // label-color is a named prop on series children; surfaces here for type-hint purposes.
        ("chart", "label-color") => "token ref: color",
        // Asset surface (non-node): kind="" is used by attribute_type() / the
        // completeness drift test for non-node attributes.
        ("", "kind") => "enum: image|svg|font",
        // route: connector-only; values validated at shape.rs:309 as
        //   straight/orthogonal/avoid (avoid is validated; maps to straight today).
        ("connector", "route") => "enum: straight|orthogonal|avoid",
        // layout: frame-only; AST documents absolute/flow/grid (container.rs:34-39).
        //   Validator only enforces grid semantics (advisory) but all three values are
        //   spec'd. Other values fall through to absolute-positioning.
        ("frame", "layout") => "enum: absolute|flow|grid",
        // All other attributes fall through to the generic arm below.
        _ => attribute_type_generic(name, fallback),
    }
}

/// Generic attribute type resolution — kind-independent properties.
///
/// Called by `attribute_type_for_kind_inner` for every attribute that is not
/// a paint/visual property requiring per-kind disambiguation.
fn attribute_type_generic(name: &str, fallback: &'static str) -> &'static str {
    match name {
        // ── Identity / labelling ──────────────────────────────────────────
        "id" => "string",
        "name" => "string",
        "role" => "string",
        "style" => "string",

        // ── Geometry (px literals) ────────────────────────────────────────
        "x" | "y" | "w" | "h" => "px literal",
        "x1" | "y1" | "x2" | "y2" => "px literal",
        "rx" | "ry" => "px literal",
        "rotate" => "px literal",
        "spacing" => "px literal",
        "padding-left" | "text-indent" => "px literal",
        "bullet-gap" => "px literal",
        "anchor-gap" => "px literal",
        "blur" => "px literal",
        "bleed" => "px literal",
        "spread-gutter" => "px literal",
        "margin-inner" | "margin-outer" | "margin-top" | "margin-bottom" => "px literal",

        // ── Visual — token refs: dimension ────────────────────────────────
        "radius" | "radius-tl" | "radius-tr" | "radius-br" | "radius-bl" => "token ref: dimension",
        "stroke-width" | "stroke-dash" | "stroke-gap" | "stroke-outer-width" => {
            "token ref: dimension"
        }
        "border-width" => "token ref: dimension",
        "font-size" | "font-size-min" => "token ref: dimension",
        "baseline-grid" => "token ref: dimension",
        "gap" | "cell-padding" | "padding" => "token ref: dimension",
        // src-* image crop coords are px literals (geometry), not token refs.
        "src-x" | "src-y" | "src-w" | "src-h" => "px literal",
        // object-position values are f64 ratios, not token refs.
        "object-position-x" | "object-position-y" => "f64 (0.0–1.0)",
        // clip-radius is a token ref (same discipline as radius).
        "clip-radius" => "token ref: dimension",

        // ── Visual — token refs: font ─────────────────────────────────────
        "font-family" => "token ref: fontFamily",
        "font-weight" => "token ref: fontWeight",

        // ── Floating-point ratios ─────────────────────────────────────────
        "opacity" | "jitter" | "intensity" => "f64 (0.0–1.0)",

        // ── Integers ─────────────────────────────────────────────────────
        "seed" | "count" => "i64",
        "drop-cap-lines" | "widow-orphan" | "tab-width" | "line-numbers" => "i64",
        "colspan" | "rowspan" | "header-rows" | "columns" | "rows" => "i64",
        "layer-priority" => "i64",

        // ── Booleans ─────────────────────────────────────────────────────
        "visible" | "locked" | "anchor-parent" => "bool",
        "hyphenate" | "suppress-first" | "border-collapse" => "bool",
        "mirror-margins" | "facing-pages" => "bool",
        "line-jumps" => "bool",

        // ── Named enums (values confirmed in the validator) ───────────────
        "anchor" => {
            "enum: top-left|top-center|top-right|center-left|center|center-right|bottom-left|bottom-center|bottom-right"
        }
        "anchor-edge" => "enum: above|below|before|after",
        "align" => "enum: left|center|right|justify",
        "overflow" => "enum: clip|visible|scroll",
        "blend-mode" => "string (enum)",
        "stroke-alignment" => "enum: inside|center|outside",
        "stroke-linecap" => "enum: butt|round|square",
        "fill-rule" => "enum: nonzero|evenodd",
        "fit" => "enum: fill|contain|cover|none",
        "clip" => "bool",
        "parity" => "enum: left|right",
        "page-parity-start" => "enum: left|right",
        "page-progression" => "enum: ltr|rtl",
        "colorspace" => "enum: srgb|display-p3|rec2020",
        "direction" => "enum: ltr|rtl",
        "overflow-wrap" => "enum: normal|break-word",
        "h-align" => "enum: left|center|right",
        "v-align" => "enum: top|middle|bottom",

        // ── Connector-specific ────────────────────────────────────────────
        "from" | "to" => "node id",
        "from-anchor" | "to-anchor" => "string",
        "marker-start" | "marker-end" => "string",

        // ── Text-specific strings ─────────────────────────────────────────
        "chain" => "node id",
        "tab-leader" | "text-exclusion" | "bullet" => "string",
        "language" => "string",
        "syntax-theme" => "string",

        // ── Data binding (on a `span`) ────────────────────────────────────
        "data-ref" => "string",
        "format" => "enum: currency|percent|number",
        "precision" => "i64",
        "locale" => "string",

        // ── Field / TOC / Footnote ────────────────────────────────────────
        "type" => "string",
        "recto" | "verso" | "target" => "string",
        "folio-style" | "header-style" | "text-style" => "string",
        "marker" => "string",
        "match-role" | "match-style" | "leader" => "string",
        "flows" => "string",

        // ── Image ─────────────────────────────────────────────────────────
        "asset" => "asset id",

        // ── Instance / component ──────────────────────────────────────────
        "component" => "string",

        // ── Group semantic extras ─────────────────────────────────────────
        "semantic-role" => "string",

        // ── Page workflow metadata ────────────────────────────────────────
        "master" => "string",

        // ── Document root ─────────────────────────────────────────────────
        "version" => "string",
        "doc-id" => "string",
        "title" => "string",

        // ── Asset provenance ──────────────────────────────────────────────
        "src" => "string",
        "sha256" => "string",
        "ai-prompt" => "string",
        "ai-model" => "string",
        "ai-provider" => "string",
        "ai-seed" => "i64",
        "ai-generation-date" => "string",
        "ai-license" => "string",
        "ai-source-rights" => "string",
        "ai-safety-status" => "string",
        "ai-reuse-policy" => "string",

        // ── Anchor zone ───────────────────────────────────────────────────
        "anchor-zone" | "anchor-sibling" => "string",

        _ => fallback,
    }
}

// ── Token type list ───────────────────────────────────────────────────────────

/// All authorable token types in their canonical `type=` string form.
///
/// `Unknown` is excluded: it is a forward-compat placeholder, not an authorable
/// type. The list is sorted for deterministic output.
///
/// Exhaustive correspondence is enforced by the `token_type_variant_count_exhaustive`
/// helper in the `#[cfg(test)]` drift-guard below: adding a new `TokenType` variant
/// without updating that match causes a compile error in the tests module.
pub fn token_types() -> &'static [&'static str] {
    &[
        "color",
        "dimension",
        "filter",
        "fontFamily",
        "fontWeight",
        "gradient",
        "mask",
        "number",
        "shadow",
    ]
}

// ── Token type summaries ──────────────────────────────────────────────────────

/// Return a one-line description of the named token type, or `None` if the type
/// is not recognised.
///
/// The `match` arm set here must stay exhaustive over `token_types()`. The
/// drift-guard test `token_type_summary_covers_every_token_type` enforces that.
pub fn token_type_summary(ty: &str) -> Option<&'static str> {
    match ty {
        "color" => Some("sRGB hex, alpha-hex, or CMYK color constant."),
        "dimension" => Some("Typed measurement with unit: px, pt, pct, or deg."),
        "filter" => Some("Ordered stack of image filter ops (grayscale, duotone, noise, …)."),
        "fontFamily" => Some("Named font-family string used for typography."),
        "fontWeight" => Some("Integer font weight in 100–900 (e.g. 400 = regular, 700 = bold)."),
        "gradient" => Some("Linear or radial gradient built from ≥2 color-stop child nodes."),
        "mask" => Some("Spatial coverage mask: a single rect, ellipse, or rounded-rect shape."),
        "number" => Some("Unitless finite number (e.g. opacity ratio, scale factor)."),
        "shadow" => Some("Ordered stack of drop-shadow layers, each referencing a color token."),
        _ => None,
    }
}

// ── Token type descriptors ────────────────────────────────────────────────────

/// Full schema descriptor for one authorable token type.
///
/// Returned by [`token_type_descriptor`].
pub struct TokenTypeDescriptor {
    /// Canonical `type=` string (matches the entry in [`token_types()`]).
    pub type_name: &'static str,
    /// One-line summary (same text as [`token_type_summary`]).
    pub summary: &'static str,
    /// Human-readable description of the value form. Empty for types that carry
    /// no inline value (gradient, shadow, filter, mask — those use child nodes).
    pub value_form: &'static str,
    /// Human-readable description of the expected child nodes. Empty for scalar
    /// types (color, dimension, number, fontFamily, fontWeight).
    pub child_nodes: &'static str,
    /// A minimal, syntactically correct example embedded as a standalone token
    /// node (without the surrounding `tokens { }` block wrapper).
    pub example: &'static str,
}

/// Return the full descriptor for the named token type, or `None` if the type
/// is not recognised.
///
/// The `match` arm set here must stay exhaustive over `token_types()`. The
/// drift-guard tests enforce that and also parse every `example` string.
pub fn token_type_descriptor(ty: &str) -> Option<TokenTypeDescriptor> {
    match ty {
        "color" => Some(TokenTypeDescriptor {
            type_name: "color",
            summary: token_type_summary("color").unwrap_or(""),
            value_form: r##"String literal: "#rrggbb" (6-digit lowercase hex), "#rrggbbaa" (8-digit), or "cmyk(c,m,y,k)" with each channel 0–100."##,
            child_nodes: "",
            example: r##"token id="color.brand.primary" type="color" value="#1a73e8""##,
        }),
        "dimension" => Some(TokenTypeDescriptor {
            type_name: "dimension",
            summary: token_type_summary("dimension").unwrap_or(""),
            value_form: "Dimension literal: (px)N, (pt)N, (pct)N, or (deg)N — annotation then bare number, no space. E.g. (px)16, (pt)12, (pct)100, (deg)45.",
            child_nodes: "",
            example: r#"token id="dim.radius.card" type="dimension" value=(px)8"#,
        }),
        "filter" => Some(TokenTypeDescriptor {
            type_name: "filter",
            summary: token_type_summary("filter").unwrap_or(""),
            value_form: "No inline value. Defined entirely by op child nodes.",
            child_nodes: "≥1 op child node. Valid op names: grayscale, invert, sepia, saturate, brightness, contrast, hue-rotate (each accept optional amount=N); duotone (requires shadow=(token)\"id\" highlight=(token)\"id\", optional amount=N); noise (accepts seed=N scale=N, optional amount=N).",
            example: "token id=\"filter.mono\" type=\"filter\" {\n    grayscale amount=1.0\n}",
        }),
        "fontFamily" => Some(TokenTypeDescriptor {
            type_name: "fontFamily",
            summary: token_type_summary("fontFamily").unwrap_or(""),
            value_form: r#"Non-empty string literal: the font-family name as it appears in the asset block, e.g. "Inter" or "Source Serif 4"."#,
            child_nodes: "",
            example: r#"token id="font.body" type="fontFamily" value="Inter""#,
        }),
        "fontWeight" => Some(TokenTypeDescriptor {
            type_name: "fontWeight",
            summary: token_type_summary("fontWeight").unwrap_or(""),
            value_form: "Bare integer (NOT a string, NOT a dimension): an integer in 100–900 with no unit annotation. E.g. 400, 700.",
            child_nodes: "",
            example: r#"token id="weight.bold" type="fontWeight" value=700"#,
        }),
        "gradient" => Some(TokenTypeDescriptor {
            type_name: "gradient",
            summary: token_type_summary("gradient").unwrap_or(""),
            value_form: "No inline value. Defined entirely by stop child nodes plus optional angle/radial props on the token node itself.",
            child_nodes: "≥2 stop child nodes. Each stop: stop offset=0.0 color=(token)\"color-token-id\". Optional props on the token node: angle=(deg)N (linear, default 90), radial=#true, center-x=0.5 center-y=0.5 radius=1.0.",
            example: "token id=\"gradient.brand\" type=\"gradient\" angle=(deg)90 {\n    stop offset=0.0 color=(token)\"color.brand.primary\"\n    stop offset=1.0 color=(token)\"color.brand.secondary\"\n}",
        }),
        "mask" => Some(TokenTypeDescriptor {
            type_name: "mask",
            summary: token_type_summary("mask").unwrap_or(""),
            value_form: "No inline value. Defined by exactly one shape child node.",
            child_nodes: "Exactly 1 shape child: rect, ellipse, or rounded. Each accepts feather=N (Gaussian sigma px, default 0) and invert=#true/#false. rounded also accepts radius=N (corner radius px).",
            example: "token id=\"mask.card\" type=\"mask\" {\n    rounded radius=8 feather=2\n}",
        }),
        "number" => Some(TokenTypeDescriptor {
            type_name: "number",
            summary: token_type_summary("number").unwrap_or(""),
            value_form: "Bare finite number with no unit annotation. E.g. 1.0, 0.5, 1.05. NaN and ±inf are invalid.",
            child_nodes: "",
            example: r#"token id="number.line-height" type="number" value=1.4"#,
        }),
        "shadow" => Some(TokenTypeDescriptor {
            type_name: "shadow",
            summary: token_type_summary("shadow").unwrap_or(""),
            value_form: "No inline value. Defined entirely by layer child nodes.",
            child_nodes: "≥1 layer child node. Each layer: layer color=(token)\"color-token-id\" dx=(px)N dy=(px)N blur=(px)N. dx/dy can be negative (offsets); blur is clamped to ≥0.",
            example: "token id=\"shadow.card\" type=\"shadow\" {\n    layer color=(token)\"color.shadow\" dx=(px)0 dy=(px)2 blur=(px)8\n}",
        }),
        _ => None,
    }
}

// ── Variant / override surface ────────────────────────────────────────────────

/// Full schema descriptor for the `variants` / `override` surface.
pub struct VariantDescriptor {
    /// One-line summary of the surface.
    pub summary: &'static str,
    /// Description of the `variants { … }` block structure.
    pub block_structure: &'static str,
    /// Description of the `variant id=… source=… w=… h=… { … }` node.
    pub variant_node: &'static str,
    /// Description of the `override node="<id>" …` entry and its recognised keys.
    pub override_entry: &'static str,
    /// Recognised properties on an `override` entry, as `(name, type, required)` tuples.
    pub override_props: &'static [(&'static str, &'static str, bool)],
    /// A worked example of a `variants` block containing an override.
    pub example: &'static str,
}

/// Return the descriptor for the `variants` / `override` surface.
///
/// This surface is not a node kind (it is not renderable on its own), so it
/// does not appear in `node_kinds()` or `node_summary()`. It is discoverable
/// via `zenith schema variant`.
pub fn variant_descriptor() -> VariantDescriptor {
    VariantDescriptor {
        summary: "Variant system — named page-level derivatives with per-node property overrides.",
        block_structure: "A `variants { … }` block sits at the document root, as a sibling of \
            `document` (canonical order: after `provenance`, before `document`) — NOT inside a \
            page. It contains one or more `variant` entries, each with its own child block of \
            `override` entries that apply to that variant.",
        variant_node: "variant id=<id> source=<page-id> w=(px)N h=(px)N { … }\n\
            \n\
            • id         — unique identifier for this variant (string, required)\n\
            • source     — the page id to base this variant on (page id string, required)\n\
            • w          — override canvas width in pixels, e.g. (px)1920 (dimension, required)\n\
            • h          — override canvas height in pixels, e.g. (px)1080 (dimension, required)\n\
            \n\
            The child block of `variant { … }` contains `override` entries (see below).",
        override_entry: "override node=\"<id>\" …\n\
            \n\
            Targets the node whose id equals the `node=` value, and applies one or more \
            property overrides. The `node` key is the only required field; all visual/geometry \
            keys are optional and independent (omitted keys retain the source page value).\n\
            \n\
            IMPORTANT: the selector key is `node` (the target node's id string), NOT `id`.\n\
            Wrong:   override id=\"hero\" visible=#false\n\
            Correct: override node=\"hero\" visible=#false",
        override_props: &[
            ("node", "string — target node id selector (required)", true),
            ("visible", "#true or #false", false),
            ("text", "string — replacement text content", false),
            ("fill", "token ref or color string", false),
            ("x", "typed dimension, e.g. (px)100", false),
            ("y", "typed dimension, e.g. (px)50", false),
            ("w", "typed dimension, e.g. (px)800", false),
            ("h", "typed dimension, e.g. (px)600", false),
        ],
        example: concat!(
            "variants {\n",
            "  variant id=\"mobile\" source=\"page.main\" w=(px)390 h=(px)844 {\n",
            "    // hide the desktop-only sidebar\n",
            "    override node=\"sidebar\" visible=#false\n",
            "    // shrink the hero to fit the narrower canvas\n",
            "    override node=\"hero\" x=(px)0 y=(px)0 w=(px)390 h=(px)260\n",
            "    // swap the headline copy\n",
            "    override node=\"headline\" text=\"Mobile headline\"\n",
            "  }\n",
            "}",
        ),
    }
}

// ── Diagnostics surface ────────────────────────────────────────────────────────

/// One-line description of the `diagnostics` surface (the root `diagnostics { … }`
/// lint-policy block).
pub fn diagnostics_summary() -> &'static str {
    "In-file diagnostic policy — allow/deny/warn specific diagnostic codes \
     (integrity Errors cannot be suppressed)."
}

/// The policy verbs accepted inside a `diagnostics { … }` block, in canonical
/// order (`allow`, `deny`, `warn`).
///
/// Single source of truth: re-exposed from [`crate::diag_catalog`].
pub fn diagnostics_verbs() -> &'static [&'static str] {
    DIAGNOSTIC_VERBS
}

/// The full catalog of diagnostic codes the engine can emit, each with its
/// severity and a one-line summary.
///
/// Single source of truth: re-exposed from [`crate::diag_catalog`]. The same
/// table drives the diagnostic-policy validator in [`crate::validate`], so the
/// `zenith schema diagnostics` surface and the policy checker can never diverge.
pub fn diagnostic_codes() -> &'static [DiagnosticCodeInfo] {
    DIAGNOSTIC_CODES
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Collapse a raw known-props slice (which may contain both `foo-bar` and
/// `foo_bar` spellings) to sorted, deduplicated kebab-case names.
///
/// For every raw name: map underscores to hyphens to get the kebab form; then
/// find the first entry in the slice that exactly equals that kebab string.
/// If found, use that interned static str; otherwise keep the raw entry as-is.
/// After collecting, sort and dedup.
fn dedupe_to_kebab(raw: &'static [&'static str]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = raw
        .iter()
        .map(|&name| {
            let kebab = name.replace('_', "-");
            raw.iter().copied().find(|n| *n == kebab).unwrap_or(name)
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

// ── Drift-guard tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Node;
    use crate::ast::token::TokenType;
    use crate::parse::KdlSource;
    use crate::parse::kdl_adapter::KdlAdapter;

    /// Exhaustive match over every `Node` variant: the compile-time drift guard.
    ///
    /// When a new variant `Node::Foo(…)` is added:
    /// 1. The `match` here becomes non-exhaustive → **compile error**.
    /// 2. Developer adds a `Node::Foo(_) => 1` arm here.
    /// 3. The developer also updates `TOTAL_NODE_VARIANTS`.
    /// 4. The `assert_eq` in `node_summary_covers_every_node_kind` then fails,
    ///    prompting the developer to add `"foo"` to `node_kinds()` and `node_summary()`.
    ///
    /// This function is only ever referenced via a function pointer in the test
    /// body (never actually called); the pointer reference forces the compiler to
    /// type-check the exhaustive match.
    fn node_variant_count_exhaustive(node: &Node) -> usize {
        match node {
            Node::Rect(_) => 1,
            Node::Ellipse(_) => 1,
            Node::Line(_) => 1,
            Node::Text(_) => 1,
            Node::Code(_) => 1,
            Node::Frame(_) => 1,
            Node::Group(_) => 1,
            Node::Image(_) => 1,
            Node::Polygon(_) => 1,
            Node::Polyline(_) => 1,
            Node::Instance(_) => 1,
            Node::Field(_) => 1,
            Node::Footnote(_) => 1,
            Node::Toc(_) => 1,
            Node::Table(_) => 1,
            Node::Shape(_) => 1,
            Node::Connector(_) => 1,
            Node::Pattern(_) => 1,
            Node::Chart(_) => 1,
            // Unknown is intentionally excluded from the authorable kind list.
            // This arm is required for exhaustiveness; the count still returns 1
            // so the total reflects all variants (authorable + Unknown).
            Node::Unknown(_) => 1,
        }
    }

    /// Total number of `Node` variants as recorded in the exhaustive match above.
    ///
    /// This is the count returned by `node_variant_count_exhaustive` for any
    /// `Node`, summed across all variants — i.e. the total variant count.
    /// Updated by hand when a variant is added (compile error forces it).
    const TOTAL_NODE_VARIANTS: usize = 20; // 19 authorable + 1 Unknown

    #[test]
    fn node_summary_covers_every_node_kind() {
        // Cross-check: node_kinds() must have exactly TOTAL_NODE_VARIANTS − 1
        // entries (all variants except Unknown).
        let expected_authorable = TOTAL_NODE_VARIANTS - 1; // subtract Unknown
        assert_eq!(
            node_kinds().len(),
            expected_authorable,
            "node_kinds() has {} entries but the exhaustive Node match covers {} authorable \
             variants (plus Unknown). Update node_kinds() and node_summary() when adding a variant.",
            node_kinds().len(),
            expected_authorable,
        );

        // Suppress the "never used" lint on node_variant_count_exhaustive by
        // taking a function pointer — this forces the compiler to type-check the
        // fn's exhaustive match without calling it.
        let _guard: fn(&Node) -> usize = node_variant_count_exhaustive;

        // Every listed kind must have a summary.
        for kind in node_kinds() {
            assert!(
                node_summary(kind).is_some(),
                "node_summary(\"{kind}\") returned None — add a one-liner to node_summary()",
            );
        }
    }

    // ── node_content drift guard ──────────────────────────────────────────────

    /// Every authorable node kind that is expected to have child content must
    /// return `Some` from `node_content`, and the example must be non-empty.
    ///
    /// Kinds confirmed to carry authorable child content (parser-verified):
    /// text, shape, footnote, polygon, polyline, table, frame, group, pattern, chart, instance,
    /// code, connector (optional span label).
    #[test]
    fn node_content_returns_some_for_content_bearing_kinds() {
        let content_kinds = &[
            "text",
            "shape",
            "footnote",
            "polygon",
            "polyline",
            "table",
            "frame",
            "group",
            "pattern",
            "chart",
            "instance",
            "code",
            "connector",
        ];
        for &kind in content_kinds {
            let desc = node_content(kind);
            assert!(
                desc.is_some(),
                "node_content(\"{kind}\") returned None — expected Some for a content-bearing kind",
            );
            let d = desc.unwrap();
            assert!(
                !d.description.is_empty(),
                "node_content(\"{kind}\").description is empty",
            );
            assert!(
                !d.example.is_empty(),
                "node_content(\"{kind}\").example is empty",
            );
        }
    }

    /// Kinds with no authorable child content must return `None` from `node_content`.
    #[test]
    fn node_content_returns_none_for_no_content_kinds() {
        let no_content_kinds = &["rect", "ellipse", "line", "image", "field", "toc"];
        for &kind in no_content_kinds {
            assert!(
                node_content(kind).is_none(),
                "node_content(\"{kind}\") returned Some — expected None for a leaf-only kind",
            );
        }
    }

    #[test]
    fn node_attributes_nonempty_for_geometry_kinds() {
        // rect must include "fill", "x", and "w".
        let rect_attrs = node_attributes("rect");
        assert!(!rect_attrs.is_empty(), "rect attributes must not be empty");
        assert!(
            rect_attrs.contains(&"fill"),
            "rect attributes must contain \"fill\"; got: {:?}",
            rect_attrs
        );
        assert!(
            rect_attrs.contains(&"x"),
            "rect attributes must contain \"x\"; got: {:?}",
            rect_attrs
        );
        assert!(
            rect_attrs.contains(&"w"),
            "rect attributes must contain \"w\"; got: {:?}",
            rect_attrs
        );

        // text must include "x", "y", "w", "h".
        let text_attrs = node_attributes("text");
        assert!(!text_attrs.is_empty(), "text attributes must not be empty");
        assert!(
            text_attrs.contains(&"x"),
            "text attributes must contain \"x\"; got: {:?}",
            text_attrs
        );

        // pattern must include "kind" and "spacing".
        let pattern_attrs = node_attributes("pattern");
        assert!(
            !pattern_attrs.is_empty(),
            "pattern attributes must not be empty"
        );
        assert!(
            pattern_attrs.contains(&"kind"),
            "pattern attributes must contain \"kind\"; got: {:?}",
            pattern_attrs
        );
        assert!(
            pattern_attrs.contains(&"spacing"),
            "pattern attributes must contain \"spacing\"; got: {:?}",
            pattern_attrs
        );

        // frame must include "x", "y", "w", "h".
        let frame_attrs = node_attributes("frame");
        assert!(
            !frame_attrs.is_empty(),
            "frame attributes must not be empty"
        );
        assert!(
            frame_attrs.contains(&"x"),
            "frame attributes must contain \"x\"; got: {:?}",
            frame_attrs
        );
        assert!(
            frame_attrs.contains(&"w"),
            "frame attributes must contain \"w\"; got: {:?}",
            frame_attrs
        );
    }

    #[test]
    fn node_attributes_empty_for_unknown_kind() {
        assert!(
            node_attributes("not-a-real-kind").is_empty(),
            "unrecognised kinds must return an empty slice"
        );
    }

    // ── Non-node surface drift guards ─────────────────────────────────────────

    /// Anchor check: `page_attributes()` must be non-empty and contain the
    /// key geometry and workflow attrs we know the parser reads. This ensures
    /// `PAGE_KNOWN_PROPS` is not accidentally emptied or truncated.
    #[test]
    fn page_attributes_anchor_check() {
        let attrs = page_attributes();
        assert!(!attrs.is_empty(), "page_attributes() must not be empty");
        for anchor in &["w", "h", "line-jumps"] {
            assert!(
                attrs.contains(anchor),
                "page_attributes() must contain \"{anchor}\"; got: {attrs:?}",
            );
        }
        // Alias spellings must be collapsed: only the kebab form should appear.
        assert!(
            !attrs.contains(&"line_jumps"),
            "underscore alias \"line_jumps\" must be collapsed; got: {attrs:?}",
        );
    }

    /// Anchor check: `asset_attributes()` must be non-empty and contain the
    /// provenance fields the parser reads.
    #[test]
    fn asset_attributes_anchor_check() {
        let attrs = asset_attributes();
        assert!(!attrs.is_empty(), "asset_attributes() must not be empty");
        for anchor in &["sha256", "ai-prompt", "ai-model", "src", "kind"] {
            assert!(
                attrs.contains(anchor),
                "asset_attributes() must contain \"{anchor}\"; got: {attrs:?}",
            );
        }
    }

    /// Anchor check: `document_attributes()` must be non-empty and contain the
    /// root-node fields the parser reads.
    #[test]
    fn document_attributes_anchor_check() {
        let attrs = document_attributes();
        assert!(!attrs.is_empty(), "document_attributes() must not be empty");
        for anchor in &["title", "colorspace", "doc-id", "spread-gutter"] {
            assert!(
                attrs.contains(anchor),
                "document_attributes() must contain \"{anchor}\"; got: {attrs:?}",
            );
        }
        // Alias spellings must be collapsed: only the kebab form should appear.
        assert!(
            !attrs.contains(&"doc_id"),
            "underscore alias \"doc_id\" must be collapsed; got: {attrs:?}",
        );
    }

    // ── Attribute type completeness drift guard ───────────────────────────

    /// Every attribute returned by any of the four public attribute-list
    /// functions must have an explicit entry in `attribute_type_for_kind_inner`
    /// or `attribute_type_generic` — not just the silent `"string"` fallback.
    ///
    /// When a new attribute is added to a KNOWN_PROPS constant, this test
    /// fails with a list of unmapped names, forcing the developer to add a
    /// corresponding arm to the appropriate function.
    ///
    /// The sentinel `"<unmapped>"` is used here instead of `"string"` so the
    /// test can distinguish "no entry at all" from a deliberate `"string"`
    /// annotation on reference/metadata fields.
    ///
    /// For node attributes the test probes with the first kind that lists the
    /// attribute; the completeness check just needs at least one mapped path.
    /// The per-kind accuracy tests below verify the per-kind correctness.
    #[test]
    fn attribute_type_covers_all_known_attrs() {
        use std::collections::BTreeMap;
        use std::collections::BTreeSet;

        // Build a map from each attribute name to the first kind that lists it
        // (so we can probe with a real kind).
        let mut attr_to_kind: BTreeMap<&'static str, &'static str> = BTreeMap::new();
        for &kind in node_kinds() {
            for attr in node_attributes(kind) {
                attr_to_kind.entry(attr).or_insert(kind);
            }
        }

        // Non-node surface attributes: probe with empty kind (kind-agnostic path).
        let mut surface_attrs: BTreeSet<&'static str> = BTreeSet::new();
        for attr in page_attributes() {
            surface_attrs.insert(attr);
        }
        for attr in asset_attributes() {
            surface_attrs.insert(attr);
        }
        for attr in document_attributes() {
            surface_attrs.insert(attr);
        }

        // Collect any attribute whose type resolves to the unmapped sentinel.
        let mut unmapped: Vec<String> = Vec::new();

        for (attr, kind) in &attr_to_kind {
            if attribute_type_for_kind_inner(kind, attr, "<unmapped>") == "<unmapped>" {
                unmapped.push(format!("{attr} (on {kind})"));
            }
        }
        for attr in &surface_attrs {
            // Only probe surface-only attrs (those not already covered via a node kind).
            if !attr_to_kind.contains_key(attr)
                && attribute_type_for_kind_inner("", attr, "<unmapped>") == "<unmapped>"
            {
                unmapped.push(format!("{attr} (surface)"));
            }
        }

        assert!(
            unmapped.is_empty(),
            "attribute_type_for_kind_inner() has no entry for {} attribute(s): {:?}\n\
             Add an arm to `attribute_type_for_kind_inner` or `attribute_type_generic` \
             in zenith-core/src/schema.rs.",
            unmapped.len(),
            unmapped,
        );
    }

    // ── Paint-attribute accuracy tests ────────────────────────────────────────

    /// `fill` must report color/gradient for geometry kinds and color-only for
    /// text, shape, and code — matching what the validator enforces via
    /// `VisualExpect`.
    #[test]
    fn fill_type_hint_is_kind_accurate() {
        // ColorOrGradient kinds: rect (shared.rs:804), ellipse (leaf.rs:218),
        // polygon (special.rs:83), polyline (special.rs:213), pattern (pattern.rs:101).
        for kind in &["rect", "ellipse", "polygon", "polyline", "pattern"] {
            assert_eq!(
                attribute_type_for_kind(kind, "fill"),
                "token ref: color/gradient",
                "fill on {kind} should accept color/gradient (validator uses ColorOrGradient)",
            );
        }
        // Color-only kinds: text (text.rs:113), shape (shape.rs:108), code (leaf.rs:561),
        // table (container.rs:304).
        for kind in &["text", "shape", "code", "table"] {
            assert_eq!(
                attribute_type_for_kind(kind, "fill"),
                "token ref: color",
                "fill on {kind} should be color-only (validator uses VisualExpect::Color)",
            );
        }
    }

    /// `stroke` is Color on every kind that has it — never color/gradient.
    /// Verified at shared.rs:813, leaf.rs:227/409, special.rs:92/222,
    /// text.rs:122, shape.rs:117/248.
    #[test]
    fn stroke_type_hint_is_color_only() {
        for kind in &[
            "rect",
            "ellipse",
            "line",
            "polygon",
            "polyline",
            "pattern",
            "text",
            "shape",
            "connector",
        ] {
            assert_eq!(
                attribute_type_for_kind(kind, "stroke"),
                "token ref: color",
                "stroke on {kind} must be color-only (validator uses VisualExpect::Color)",
            );
        }
    }

    /// `shadow`, `filter`, and `mask` must each report their own dedicated token
    /// type — NOT color or color/gradient.
    /// Verified at shared.rs:960 (Shadow), 969 (Filter), 978 (Mask).
    #[test]
    fn shadow_filter_mask_report_own_token_types() {
        for kind in &["rect", "pattern"] {
            assert_eq!(
                attribute_type_for_kind(kind, "shadow"),
                "token ref: shadow",
                "shadow on {kind} must reference a shadow token",
            );
            assert_eq!(
                attribute_type_for_kind(kind, "filter"),
                "token ref: filter",
                "filter on {kind} must reference a filter token",
            );
            assert_eq!(
                attribute_type_for_kind(kind, "mask"),
                "token ref: mask",
                "mask on {kind} must reference a mask token",
            );
        }
        // Verify the kind-agnostic public API also gives the right answer.
        assert_eq!(attribute_type("shadow"), "token ref: shadow");
        assert_eq!(attribute_type("filter"), "token ref: filter");
        assert_eq!(attribute_type("mask"), "token ref: mask");
    }

    /// `background` (page surface) reports color/gradient — driver.rs:639 uses
    /// VisualExpect::ColorOrGradient.
    #[test]
    fn background_type_hint_is_color_or_gradient() {
        assert_eq!(attribute_type("background"), "token ref: color/gradient");
        assert_eq!(
            attribute_type_for_kind("", "background"),
            "token ref: color/gradient"
        );
    }

    /// Border/stroke-outer type hints are color-only — shared.rs:887 uses
    /// VisualExpect::Color for all per-side border props.
    #[test]
    fn border_and_stroke_outer_are_color_only() {
        for attr in &[
            "border-top",
            "border-bottom",
            "border-left",
            "border-right",
            "stroke-outer",
        ] {
            assert_eq!(
                attribute_type_for_kind("rect", attr),
                "token ref: color",
                "{attr} on rect must be color-only (validator uses VisualExpect::Color)",
            );
        }
    }

    // ── kind / route / layout node-aware accuracy tests ──────────────────────

    /// `kind` must report node-specific enum strings per kind.
    ///
    /// shape.kind: process/decision/terminator/ellipse (shape.rs:152).
    /// pattern.kind: grid/scatter (pattern.rs:140).
    /// These must NOT cross-contaminate: the same attribute name, different enums.
    #[test]
    fn kind_type_hint_is_node_aware() {
        assert_eq!(
            attribute_type_for_kind("shape", "kind"),
            "enum: process|decision|terminator|ellipse",
            "shape.kind must enumerate the flowchart shape variants",
        );
        assert_eq!(
            attribute_type_for_kind("pattern", "kind"),
            "enum: grid|scatter",
            "pattern.kind must enumerate the tiling mode variants",
        );
        // The two must differ — this is the root bug being fixed.
        assert_ne!(
            attribute_type_for_kind("shape", "kind"),
            attribute_type_for_kind("pattern", "kind"),
            "shape.kind and pattern.kind must NOT return the same hint (they are different enums)",
        );
    }

    /// `route` on connector must enumerate straight/orthogonal/avoid.
    ///
    /// Validated at validate/check/nodes/node/shape.rs:309.
    #[test]
    fn route_type_hint_is_connector_specific() {
        assert_eq!(
            attribute_type_for_kind("connector", "route"),
            "enum: straight|orthogonal|avoid",
            "connector.route must enumerate all three routing modes",
        );
    }

    /// `layout` on frame must enumerate absolute/flow/grid.
    ///
    /// Documented in ast/node/container.rs:34-39; grid semantics validated
    /// at validate/check/nodes/node/container.rs:122.
    #[test]
    fn layout_type_hint_is_frame_specific() {
        assert_eq!(
            attribute_type_for_kind("frame", "layout"),
            "enum: absolute|flow|grid",
            "frame.layout must enumerate the three layout modes",
        );
    }

    // ── Token type drift guards ───────────────────────────────────────────────

    /// Exhaustive match over every `TokenType` variant: the compile-time drift guard.
    ///
    /// When a new variant `TokenType::Foo` is added:
    /// 1. The `match` here becomes non-exhaustive → **compile error**.
    /// 2. Developer adds a `TokenType::Foo => 1` arm here.
    /// 3. The developer also updates `TOTAL_TOKEN_TYPE_VARIANTS`.
    /// 4. The `assert_eq` in `token_type_summary_covers_every_token_type` then
    ///    fails, prompting the developer to add `"foo"` to `token_types()`,
    ///    `token_type_summary()`, and `token_type_descriptor()`.
    ///
    /// This function is only ever referenced via a function pointer in the test
    /// body (never actually called); the pointer reference forces the compiler to
    /// type-check the exhaustive match.
    fn token_type_variant_count_exhaustive(ty: &TokenType) -> usize {
        match ty {
            TokenType::Color => 1,
            TokenType::Dimension => 1,
            TokenType::Number => 1,
            TokenType::FontFamily => 1,
            TokenType::FontWeight => 1,
            TokenType::Gradient => 1,
            TokenType::Shadow => 1,
            TokenType::Filter => 1,
            TokenType::Mask => 1,
            // Unknown is intentionally excluded from the authorable type list.
            // This arm is required for exhaustiveness.
            TokenType::Unknown(_) => 1,
        }
    }

    /// Total number of `TokenType` variants as recorded in the exhaustive match above.
    /// Updated by hand when a variant is added (compile error forces it).
    const TOTAL_TOKEN_TYPE_VARIANTS: usize = 10; // 9 authorable + 1 Unknown

    #[test]
    fn token_type_summary_covers_every_token_type() {
        // Cross-check: token_types() must have exactly TOTAL_TOKEN_TYPE_VARIANTS − 1
        // entries (all variants except Unknown).
        let expected_authorable = TOTAL_TOKEN_TYPE_VARIANTS - 1;
        assert_eq!(
            token_types().len(),
            expected_authorable,
            "token_types() has {} entries but the exhaustive TokenType match covers {} authorable \
             variants (plus Unknown). Update token_types(), token_type_summary(), and \
             token_type_descriptor() when adding a variant.",
            token_types().len(),
            expected_authorable,
        );

        // Suppress the "never used" lint on token_type_variant_count_exhaustive by
        // taking a function pointer — this forces the compiler to type-check the
        // fn's exhaustive match without calling it.
        let _guard: fn(&TokenType) -> usize = token_type_variant_count_exhaustive;

        // Every listed type must have a summary.
        for ty in token_types() {
            assert!(
                token_type_summary(ty).is_some(),
                "token_type_summary(\"{ty}\") returned None — add a one-liner to token_type_summary()",
            );
        }
    }

    #[test]
    fn token_type_descriptor_covers_every_token_type() {
        // Every listed type must have a descriptor.
        for ty in token_types() {
            assert!(
                token_type_descriptor(ty).is_some(),
                "token_type_descriptor(\"{ty}\") returned None — add a descriptor to token_type_descriptor()",
            );
        }

        // Every descriptor's type_name must match its key.
        for ty in token_types() {
            let desc = token_type_descriptor(ty).unwrap();
            assert_eq!(
                desc.type_name, *ty,
                "token_type_descriptor(\"{ty}\").type_name is \"{}\", expected \"{ty}\"",
                desc.type_name,
            );
            // summary must be non-empty.
            assert!(
                !desc.summary.is_empty(),
                "token_type_descriptor(\"{ty}\").summary is empty",
            );
            // value_form and child_nodes may not both be empty (every type has one or the other).
            assert!(
                !desc.value_form.is_empty() || !desc.child_nodes.is_empty(),
                "token_type_descriptor(\"{ty}\") has both empty value_form and child_nodes",
            );
            // example must be non-empty.
            assert!(
                !desc.example.is_empty(),
                "token_type_descriptor(\"{ty}\").example is empty",
            );
        }
    }

    /// Example-accuracy guard: each token example must parse as part of a minimal
    /// document without a parse error.
    ///
    /// We do NOT assert that validation is clean — compound examples reference
    /// other token ids that won't resolve standalone, and that is expected. We
    /// only assert syntax correctness: if this fails, the schema is showing agents
    /// syntactically wrong examples.
    #[test]
    fn token_type_examples_parse_without_syntax_errors() {
        for ty in token_types() {
            let desc = token_type_descriptor(ty).unwrap();
            // Wrap the token example in a minimal document.
            // Only `document` is required by the parser; `tokens` is optional
            // but must carry `format="zenith-token-v1"` when present.
            let doc_src = format!(
                "zenith version=1 {{\n\
                 \x20 tokens format=\"zenith-token-v1\" {{\n\
                 \x20   {}\n\
                 \x20 }}\n\
                 \x20 document id=\"doc\" {{\n\
                 \x20   page id=\"pg\" w=(px)1 h=(px)1 {{}}\n\
                 \x20 }}\n\
                 }}\n",
                desc.example,
            );
            let result = KdlAdapter.parse(doc_src.as_bytes());
            assert!(
                result.is_ok(),
                "token_type_descriptor(\"{ty}\").example failed to parse:\n\
                 example:\n  {}\n\
                 wrapped doc:\n{doc_src}\n\
                 parse error: {:?}",
                desc.example,
                result.err(),
            );
        }
    }
}
