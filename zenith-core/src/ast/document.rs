//! Top-level document AST types.

use super::Span;
use super::asset::AssetBlock;
use super::node::Node;
use super::style::StyleBlock;
use super::token::TokenBlock;
use super::value::Dimension;
use super::value::PropertyValue;

/// Metadata for the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
}

/// A single page within a document body.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub id: String,
    pub name: Option<String>,
    /// Page width — required.
    pub width: Dimension,
    /// Page height — required.
    pub height: Dimension,
    pub background: Option<PropertyValue>,
    /// Optional uniform print-bleed margin applied to all four sides. When this
    /// resolves to a positive pixel value `b`, the rendered media box expands to
    /// `(width + 2b) × (height + 2b)`, all page content shifts into the inner
    /// trim box `[b, b, width, height]`, the background fills the entire media
    /// box (bleeding off the trim edge), and crop/trim marks are auto-drawn in
    /// the bleed margin at the four trim corners. `None` or a non-positive value
    /// renders byte-identically to a page with no bleed.
    pub bleed: Option<Dimension>,
    /// Book live-area margin (gutter side). With document `mirror_margins=true`
    /// this is the BINDING-side margin: it sits on the LEFT for a recto (odd,
    /// 1-based) page and on the RIGHT for a verso (even) page. Without mirroring
    /// it is treated uniformly as the left margin. `None` → no inner margin.
    ///
    /// Margins are v0 METADATA + VALIDATION ONLY: they describe the intended
    /// live area and drive the `margin.violation` advisory, but they do NOT
    /// auto-reposition arbitrary page nodes (that is the job of master pages /
    /// flow frames). See [`super::super::validate`]'s margin check.
    pub margin_inner: Option<Dimension>,
    /// Book live-area margin (fore-edge side). The mirror of [`Page::margin_inner`]:
    /// with `mirror_margins=true` it sits on the RIGHT for a recto page and on
    /// the LEFT for a verso page; without mirroring it is the right margin.
    /// `None` → no outer margin. Metadata + validation only (see `margin_inner`).
    pub margin_outer: Option<Dimension>,
    /// Book live-area top margin. `None` → no top margin. Metadata + validation
    /// only (see [`Page::margin_inner`]).
    pub margin_top: Option<Dimension>,
    /// Book live-area bottom margin. `None` → no bottom margin. Metadata +
    /// validation only (see [`Page::margin_inner`]).
    pub margin_bottom: Option<Dimension>,
    /// Optional baseline-grid pitch in pixels. When this resolves to a positive
    /// pixel value `g`, every text node on this page snaps its line baselines onto
    /// the grid `{ 0, g, 2g, ... }` measured from the page top (y=0): the first
    /// line's baseline moves DOWN to the next grid line at or below its natural
    /// position, and the effective inter-line advance becomes the smallest multiple
    /// of `g` that is ≥ the resolved line-height, so corresponding lines align
    /// horizontally across columns. `None` or a non-positive value renders
    /// byte-identically to a page with no grid. KDL: `baseline-grid=(px)14`.
    pub baseline_grid: Option<Dimension>,
    /// Author-declared safe/dead zones for this page. These are not rendering
    /// nodes; the validator checks page children against them.
    pub safe_zones: Vec<SafeZone>,
    /// Author-declared fold-line positions for this page (tri-fold/bi-fold
    /// print). These are non-printing page metadata, not rendering nodes; the
    /// validator advises when content crosses a fold line.
    pub folds: Vec<Fold>,
    /// Optional explicit recto/verso parity OVERRIDE for this page. `Some("recto")`
    /// or `Some("verso")` forces this page's parity regardless of its 1-based
    /// position and the document `page_parity_start`. `None` (default) → parity is
    /// derived from the page position and the document start parity. An invalid
    /// value is preserved verbatim and surfaced as a validation warning
    /// (`page.invalid_parity`); it then falls through to the derived parity. See
    /// [`Document::page_is_recto`].
    pub parity: Option<String>,
    /// Optional master-page reference. When `Some(id)` names a declared
    /// [`MasterDef`], the master's nodes (running heads, folios, TOC refs) are
    /// projected UNDER this page's own children at compile time — the master's
    /// [`Field`](super::node::Node::Field) nodes are resolved against this page's
    /// index/parity. An unknown reference is a hard `master.unknown_reference`
    /// validation error. `None` → the page has no master (renders as before).
    pub master: Option<String>,
    /// Child content nodes in z-order (first = bottommost, last = topmost).
    pub children: Vec<Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// The kind of a [`SafeZone`].
#[derive(Debug, Clone, PartialEq)]
pub enum SafeZoneType {
    /// Content must NOT overlap this zone (e.g. a platform UI dead zone).
    Exclusion,
    /// Content must overlap this zone (e.g. a guaranteed-visible region).
    Required,
}

/// A named safe/dead zone declared on a [`Page`].
///
/// Declared as a `safe-zone` child of a `page`; it is a sibling of rendering
/// nodes but is itself not rendered.
#[derive(Debug, Clone, PartialEq)]
pub struct SafeZone {
    pub id: String,
    pub zone_type: SafeZoneType,
    pub x: Dimension,
    pub y: Dimension,
    pub w: Dimension,
    pub h: Dimension,
    pub label: Option<String>,
    pub source_span: Option<Span>,
}

/// A non-printing fold-line position declared on a [`Page`].
///
/// Declared as a `fold` child of a `page`; it is a sibling of rendering nodes
/// but is itself never rendered. A vertical fold has an `x` position; a
/// horizontal fold has a `y` position. Used for tri-fold / bi-fold print
/// layouts so the validator can advise when content crosses a fold line.
#[derive(Debug, Clone, PartialEq)]
pub struct Fold {
    pub id: String,
    /// `"vertical"` (position is an x coordinate) or `"horizontal"` (position
    /// is a y coordinate). Any other / absent value defaults to `"vertical"`.
    pub orientation: String,
    /// The fold-line position: x for a vertical fold, y for a horizontal fold.
    /// `None` when the author omitted `position`.
    pub position: Option<Dimension>,
    pub source_span: Option<Span>,
}

/// The `document` child of the root `zenith` node.
///
/// Named `DocumentBody` to avoid clashing with the root `Document` type.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentBody {
    pub id: String,
    pub title: Option<String>,
    pub pages: Vec<Page>,
}

/// A reusable component definition: a named child-node subtree declared once
/// (in the document-level `components` block) and instanced into multiple places
/// via [`Node::Instance`](super::node::Node::Instance).
///
/// Declared as `component id="logo.block" { <any child nodes> }`. The component's
/// child node ids are LOCAL to the component: they are validated for uniqueness
/// only WITHIN the component, not globally, and they are prefixed with the
/// instance id when an instance is expanded at compile time. The `component` id
/// itself participates in the global id-uniqueness set.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDef {
    pub id: String,
    /// The component's child nodes in source order (the reusable subtree).
    pub children: Vec<super::node::Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// A reusable master-page definition: a named child-node subtree declared once
/// (in the document-level `masters` block) and projected onto every [`Page`]
/// whose `master` attribute names it.
///
/// Declared as `master id="m.body" { <any child nodes, incl. field nodes> }`.
/// Structurally mirrors [`ComponentDef`]: the master's child node ids are LOCAL
/// to the master (validated for uniqueness only WITHIN the master) and are
/// prefixed with the page id when the master is projected at compile time. The
/// `master` id itself participates in the global id-uniqueness set.
///
/// Unlike a component, a master is not instanced explicitly: a page opts in via
/// `page ... master="m.body"`, and the master's [`Field`](super::node::Node::Field)
/// nodes are resolved against that page's index/parity/live-area at compile time.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterDef {
    pub id: String,
    /// The master's child nodes in source order (the projected subtree).
    pub children: Vec<super::node::Node>,
    /// Source declaration span, when available.
    pub source_span: Option<Span>,
}

/// The root `zenith` node — the complete parsed `.zen` document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Must be `1` in v0.
    pub version: u32,
    /// Declared export color space: `Some("srgb")` (default) or `Some("cmyk")`.
    /// `None` when the author omitted the `colorspace` attribute. In v0 this is
    /// informational export metadata only — it does NOT change PNG output (the
    /// PNG is always sRGB); a future PDF backend consults it. An invalid value
    /// is preserved here verbatim and surfaced as a validation warning.
    pub colorspace: Option<String>,
    /// Mirrored book margins toggle. `Some(true)` → page margins mirror by page
    /// parity (recto = odd 1-based page → inner margin on LEFT; verso = even →
    /// inner margin on RIGHT). `Some(false)` or `None` (default) → margins are
    /// uniform (inner = left, outer = right on every page). This only affects
    /// how [`Page::margin_inner`]/[`Page::margin_outer`] are interpreted by the
    /// `margin.violation` validation advisory; it is metadata, not layout.
    pub mirror_margins: Option<bool>,
    /// Declared page progression for export: `Some("ltr")` (default) or
    /// `Some("rtl")` (right-to-left book page order). `None` when the author
    /// omitted the attribute. v0: metadata for export (e.g. a PDF
    /// `/ViewerPreferences /Direction /R2L`); it does NOT change page render
    /// order or PNG output. An invalid value is preserved verbatim and surfaced
    /// as a validation warning.
    pub page_progression: Option<String>,
    /// Declared STARTING parity for page 1: `Some("recto")` (default behavior) or
    /// `Some("verso")` (page 1 is a verso, shifting the whole recto/verso sequence
    /// by one). `None` when the author omitted the attribute — page 1 is then a
    /// recto, exactly as before. An invalid value is preserved verbatim and
    /// surfaced as a validation warning (`document.invalid_page_parity_start`); it
    /// then falls through to the default (page 1 = recto). This drives the
    /// mirrored-margin binding side and the master/field running-head recto/verso
    /// selection via [`Document::page_is_recto`].
    pub page_parity_start: Option<String>,
    /// Document-level DEFAULT book live-area inner (gutter/binding) margin. When
    /// a [`Page`] omits its own [`Page::margin_inner`], it inherits this value.
    /// `None` (default) → no document default; the page's own value (possibly
    /// `None`) is used verbatim, so a document with no margins is byte-identical
    /// to before this attribute existed. Same KDL syntax as on a page
    /// (`margin-inner=(px)225`). See [`Document::effective_margins`].
    pub margin_inner: Option<Dimension>,
    /// Document-level DEFAULT book live-area outer (fore-edge) margin. Cascades
    /// to a page that omits [`Page::margin_outer`]. See [`Document::margin_inner`].
    pub margin_outer: Option<Dimension>,
    /// Document-level DEFAULT book live-area top margin. Cascades to a page that
    /// omits [`Page::margin_top`]. See [`Document::margin_inner`].
    pub margin_top: Option<Dimension>,
    /// Document-level DEFAULT book live-area bottom margin. Cascades to a page
    /// that omits [`Page::margin_bottom`]. See [`Document::margin_inner`].
    pub margin_bottom: Option<Dimension>,
    pub project: Option<Project>,
    /// Asset declarations; empty when the `assets` block is absent.
    pub assets: AssetBlock,
    pub tokens: TokenBlock,
    pub styles: StyleBlock,
    /// Reusable component definitions; empty when the `components` block is
    /// absent. Instanced via [`Node::Instance`](super::node::Node::Instance).
    pub components: Vec<ComponentDef>,
    /// Reusable master-page definitions; empty when the `masters` block is
    /// absent. Projected onto pages via [`Page::master`].
    pub masters: Vec<MasterDef>,
    pub body: DocumentBody,
}

impl Document {
    /// True when the given page (at its 1-based position in document order) is a
    /// recto (right-hand) page; false for a verso (left-hand) page. This is the
    /// SINGLE source of truth for page parity across the workspace (mirrored
    /// margins + master/field running-head selection).
    ///
    /// Precedence (highest first):
    /// 1. An explicit per-page [`Page::parity`] override (`"recto"`/`"verso"`).
    ///    Any value other than `"verso"` (case-insensitive) — including an
    ///    invalid one — is treated as recto, matching the validator's
    ///    forward-compatible warning behavior.
    /// 2. The document [`Document::page_parity_start`] offset: `"verso"`
    ///    (case-insensitive) makes page 1 a verso and shifts the whole sequence
    ///    by one; any other / absent value keeps the default.
    /// 3. Default: page 1 is a recto — `page_index_1based % 2 == 1`, exactly the
    ///    pre-feature behavior. With no parity attributes this returns
    ///    `index % 2 == 1` byte-identically.
    ///
    /// Pure and deterministic.
    pub fn page_is_recto(&self, page: &Page, page_index_1based: usize) -> bool {
        if let Some(p) = page.parity.as_deref() {
            // Explicit per-page override: "verso" → verso, anything else → recto.
            return !p.eq_ignore_ascii_case("verso");
        }
        let base_recto = page_index_1based % 2 == 1;
        match self.page_parity_start.as_deref() {
            Some(s) if s.eq_ignore_ascii_case("verso") => !base_recto,
            _ => base_recto,
        }
    }

    /// The page's EFFECTIVE book live-area margins, as
    /// `(inner, outer, top, bottom)`: each side is the page's own value when set,
    /// else the document-level default ([`Document::margin_inner`] etc.). This is
    /// the SINGLE source of truth for the document→page margin cascade; every
    /// live-area / margin computation reads margins through here so per-page
    /// overrides and document defaults resolve identically everywhere.
    ///
    /// With no document margins set, this returns exactly the page's own values
    /// (including `None`), so the default-off path is byte-identical to reading
    /// `page.margin_*` directly. Pure and deterministic.
    pub fn effective_margins(
        &self,
        page: &Page,
    ) -> (
        Option<Dimension>,
        Option<Dimension>,
        Option<Dimension>,
        Option<Dimension>,
    ) {
        (
            page.margin_inner
                .clone()
                .or_else(|| self.margin_inner.clone()),
            page.margin_outer
                .clone()
                .or_else(|| self.margin_outer.clone()),
            page.margin_top.clone().or_else(|| self.margin_top.clone()),
            page.margin_bottom
                .clone()
                .or_else(|| self.margin_bottom.clone()),
        )
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::ast::value::Dimension;
    use crate::ast::value::Unit;

    fn px(v: f64) -> Dimension {
        Dimension {
            value: v,
            unit: Unit::Px,
        }
    }

    fn page(id: &str, parity: Option<&str>) -> Page {
        Page {
            id: id.to_owned(),
            name: None,
            width: px(100.0),
            height: px(100.0),
            background: None,
            bleed: None,
            margin_inner: None,
            margin_outer: None,
            margin_top: None,
            margin_bottom: None,
            baseline_grid: None,
            parity: parity.map(str::to_owned),
            master: None,
            safe_zones: Vec::new(),
            folds: Vec::new(),
            children: Vec::new(),
            source_span: None,
        }
    }

    fn doc(start: Option<&str>) -> Document {
        Document {
            version: 1,
            colorspace: None,
            mirror_margins: None,
            page_progression: None,
            page_parity_start: start.map(str::to_owned),
            margin_inner: None,
            margin_outer: None,
            margin_top: None,
            margin_bottom: None,
            project: None,
            assets: AssetBlock::default(),
            tokens: TokenBlock::default(),
            styles: StyleBlock::default(),
            components: Vec::new(),
            masters: Vec::new(),
            body: DocumentBody {
                id: "body".to_owned(),
                title: None,
                pages: Vec::new(),
            },
        }
    }

    #[test]
    fn default_page_one_recto_page_two_verso() {
        let d = doc(None);
        assert!(d.page_is_recto(&page("p1", None), 1), "page 1 is recto");
        assert!(!d.page_is_recto(&page("p2", None), 2), "page 2 is verso");
        assert!(d.page_is_recto(&page("p3", None), 3), "page 3 is recto");
    }

    #[test]
    fn start_verso_flips_the_sequence() {
        let d = doc(Some("verso"));
        assert!(!d.page_is_recto(&page("p1", None), 1), "page 1 is verso");
        assert!(d.page_is_recto(&page("p2", None), 2), "page 2 is recto");
    }

    #[test]
    fn start_recto_matches_default() {
        let d = doc(Some("recto"));
        assert!(d.page_is_recto(&page("p1", None), 1));
        assert!(!d.page_is_recto(&page("p2", None), 2));
    }

    #[test]
    fn page_override_verso_wins_over_start() {
        // Default start (recto), but page 1 forced to verso.
        let d = doc(None);
        assert!(!d.page_is_recto(&page("p1", Some("verso")), 1));
        // Even with start=verso, an explicit recto on page 1 forces recto.
        let d2 = doc(Some("verso"));
        assert!(d2.page_is_recto(&page("p1", Some("recto")), 1));
    }

    #[test]
    fn page_override_recto_on_even_page() {
        let d = doc(None);
        assert!(
            d.page_is_recto(&page("p2", Some("recto")), 2),
            "page 2 forced recto"
        );
    }

    #[test]
    fn invalid_start_falls_back_to_default() {
        let d = doc(Some("sideways"));
        assert!(d.page_is_recto(&page("p1", None), 1), "page 1 stays recto");
        assert!(!d.page_is_recto(&page("p2", None), 2));
    }

    #[test]
    fn invalid_page_parity_treated_as_recto() {
        let d = doc(None);
        assert!(
            d.page_is_recto(&page("p2", Some("nonsense")), 2),
            "an invalid override is treated as recto"
        );
    }

    #[test]
    fn effective_margins_page_value_wins_when_both_set() {
        let mut d = doc(None);
        d.margin_inner = Some(px(10.0));
        d.margin_outer = Some(px(20.0));
        d.margin_top = Some(px(30.0));
        d.margin_bottom = Some(px(40.0));
        let mut p = page("p", None);
        p.margin_inner = Some(px(1.0));
        p.margin_outer = Some(px(2.0));
        p.margin_top = Some(px(3.0));
        p.margin_bottom = Some(px(4.0));
        let (i, o, t, b) = d.effective_margins(&p);
        assert_eq!(i, Some(px(1.0)));
        assert_eq!(o, Some(px(2.0)));
        assert_eq!(t, Some(px(3.0)));
        assert_eq!(b, Some(px(4.0)));
    }

    #[test]
    fn effective_margins_doc_default_used_when_page_none() {
        let mut d = doc(None);
        d.margin_inner = Some(px(10.0));
        d.margin_outer = Some(px(20.0));
        d.margin_top = Some(px(30.0));
        d.margin_bottom = Some(px(40.0));
        let p = page("p", None);
        let (i, o, t, b) = d.effective_margins(&p);
        assert_eq!(i, Some(px(10.0)));
        assert_eq!(o, Some(px(20.0)));
        assert_eq!(t, Some(px(30.0)));
        assert_eq!(b, Some(px(40.0)));
    }

    #[test]
    fn effective_margins_mixed_override() {
        // Doc sets all four; page overrides only inner → page inner + doc rest.
        let mut d = doc(None);
        d.margin_inner = Some(px(10.0));
        d.margin_outer = Some(px(20.0));
        d.margin_top = Some(px(30.0));
        d.margin_bottom = Some(px(40.0));
        let mut p = page("p", None);
        p.margin_inner = Some(px(99.0));
        let (i, o, t, b) = d.effective_margins(&p);
        assert_eq!(i, Some(px(99.0)));
        assert_eq!(o, Some(px(20.0)));
        assert_eq!(t, Some(px(30.0)));
        assert_eq!(b, Some(px(40.0)));
    }

    #[test]
    fn effective_margins_none_when_both_none() {
        let d = doc(None);
        let p = page("p", None);
        assert_eq!(d.effective_margins(&p), (None, None, None, None));
    }

    #[test]
    fn effective_margins_default_off_is_page_values_verbatim() {
        // The regression guard: with NO doc margins, effective == page's own
        // values exactly (including None), so the default-off path is identical.
        let d = doc(None);
        let mut p = page("p", None);
        p.margin_inner = Some(px(225.0));
        p.margin_top = Some(px(210.0));
        let (i, o, t, b) = d.effective_margins(&p);
        assert_eq!(i, p.margin_inner);
        assert_eq!(o, p.margin_outer);
        assert_eq!(t, p.margin_top);
        assert_eq!(b, p.margin_bottom);
    }

    #[test]
    fn default_is_byte_identical_to_index_parity() {
        // The regression guard: with no parity attrs anywhere, page_is_recto MUST
        // equal `index % 2 == 1` for every index.
        let d = doc(None);
        for idx in 1..=64usize {
            assert_eq!(
                d.page_is_recto(&page("p", None), idx),
                idx % 2 == 1,
                "default parity must equal index%2==1 at index {idx}"
            );
        }
    }
}
