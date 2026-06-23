//! Pure in-memory variant generation engine.
//!
//! [`expand_variants`] is the single public entry point.  It consumes a parsed
//! [`Document`], iterates `doc.variants` in stable id order, and for each
//! definition clones the source document, builds a transaction op batch, and
//! runs it through the same [`run_transaction`] path that `zenith merge` uses.
//!
//! No file I/O, no CLI parsing, no rendering.  Those live in the CLI entry point.

use std::collections::BTreeMap;

use zenith_core::{Document, KdlAdapter, KdlSource, PropertyValue};
use zenith_tx::{Op, OpSpan, Permissions, Transaction, TxStatus, run_transaction};

// ── Result / outcome types ────────────────────────────────────────────────────

/// The complete result of one [`expand_variants`] call.
///
/// `results` is sorted by variant id (ascending), matching the deterministic
/// processing order.
#[derive(Debug)]
pub struct VariantExpansion {
    pub results: Vec<VariantResult>,
}

impl VariantExpansion {
    /// Number of successfully-generated variants.
    pub fn generated(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, VariantOutcome::Generated(_)))
            .count()
    }

    /// Number of failed variants.
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, VariantOutcome::Failed(_)))
            .count()
    }
}

/// Result for a single variant entry.
#[derive(Debug)]
pub struct VariantResult {
    /// The variant's stable id.
    pub id: String,
    /// The source page id this variant derives from.
    pub source: String,
    /// Either the materialized document or a failure reason.
    pub outcome: VariantOutcome,
}

/// Outcome of applying one variant's op batch.
#[derive(Debug)]
pub enum VariantOutcome {
    /// The transaction was accepted; contains the materialized document.
    /// Boxed: a `Document` is much larger than the `Failed` string payload.
    Generated(Box<Document>),
    /// The transaction was rejected or the engine returned a hard error.
    /// Contains a human-readable reason string.
    Failed(String),
}

// ── expand_variants ───────────────────────────────────────────────────────────

/// Expand all variant definitions in `doc` into materialized documents.
///
/// Processes variants in ascending `id` order (deterministic).  A failure on
/// one variant does NOT abort the rest — every variant is attempted independently.
///
/// Returns an empty [`VariantExpansion`] when `doc.variants` is empty.
pub fn expand_variants(doc: &Document) -> VariantExpansion {
    if doc.variants.is_empty() {
        return VariantExpansion {
            results: Vec::new(),
        };
    }

    // Collect into a BTreeMap keyed by id to enforce deterministic ordering
    // without mutating the caller's slice.  Duplicate ids are caught by
    // validation and are not expected here; if they slip through the last
    // writer wins (both would produce the same key anyway since validation blocks).
    let sorted: BTreeMap<&str, _> = doc.variants.iter().map(|v| (v.id.as_str(), v)).collect();

    // Generation consumes the variants block: each materialized variant is a
    // concrete page, not a template. Strip the block from the base document the
    // transactions run against so (a) the output carries no `variants` block and
    // (b) one variant's override problems don't fail a sibling variant when the
    // post-transaction validation re-checks the (shared) variants block.
    let mut base = doc.clone();
    base.variants.clear();

    let mut results: Vec<VariantResult> = Vec::with_capacity(sorted.len());

    for variant in sorted.values() {
        // Build the op batch for this variant.
        let mut ops: Vec<Op> = Vec::new();

        // 1. Resize the source page to the variant's target dimensions.
        ops.push(Op::SetPageSize {
            page: variant.source.clone(),
            w: variant.w.to_kdl_string(),
            h: variant.h.to_kdl_string(),
        });

        // 2. Per-override ops, in stored order, sub-ordered: visible → fill → text.
        for ov in &variant.overrides {
            if let Some(visible) = ov.visible {
                ops.push(Op::SetVisible {
                    node: ov.node.clone(),
                    visible,
                });
            }
            if let Some(fill) = &ov.fill {
                ops.push(Op::SetFill {
                    node: ov.node.clone(),
                    fill: property_value_to_fill_str(fill),
                });
            }
            if let Some(text) = &ov.text {
                ops.push(Op::ReplaceText {
                    node: ov.node.clone(),
                    spans: vec![OpSpan {
                        text: text.clone(),
                        fill: None,
                        font_weight: None,
                        italic: None,
                        underline: None,
                        strikethrough: None,
                        vertical_align: None,
                        footnote_ref: None,
                    }],
                });
            }
        }

        let tx = Transaction {
            ops,
            permissions: Permissions::default(),
        };

        // 3. Run the transaction against the variants-stripped base document.
        let outcome = match run_transaction(&base, &tx) {
            Err(e) => VariantOutcome::Failed(format!("transaction engine error: {}", e.message)),
            Ok(tx_result) if tx_result.status == TxStatus::Rejected => {
                let msgs: Vec<String> = tx_result
                    .diagnostics
                    .iter()
                    .map(|d| {
                        format!(
                            "{}[{}]: {}",
                            crate::json_types::severity_str(&d.severity),
                            d.code,
                            d.message
                        )
                    })
                    .collect();
                VariantOutcome::Failed(format!("transaction rejected: {}", msgs.join("; ")))
            }
            Ok(tx_result) => {
                // Re-parse source_after into the materialized document.
                match KdlAdapter.parse(tx_result.source_after.as_bytes()) {
                    Err(e) => VariantOutcome::Failed(format!(
                        "post-transaction parse error: {}",
                        e.message
                    )),
                    Ok(materialized) => VariantOutcome::Generated(Box::new(materialized)),
                }
            }
        };

        results.push(VariantResult {
            id: variant.id.clone(),
            source: variant.source.clone(),
            outcome,
        });
    }

    VariantExpansion { results }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Extract a string to pass to [`Op::SetFill`] from a [`PropertyValue`].
///
/// [`Op::SetFill`] accepts a token id and stores it as
/// `PropertyValue::TokenRef`.  For `TokenRef` fills this is straightforward.
/// For `Literal` and `Dimension` fills the raw string is passed through; the
/// engine will still wrap it as `TokenRef`, which post-validation will then
/// reject as `token.unknown_reference` — surfacing a `Failed` outcome for that
/// variant rather than silently producing a corrupt document.
fn property_value_to_fill_str(pv: &PropertyValue) -> String {
    match pv {
        PropertyValue::TokenRef(id) => id.clone(),
        PropertyValue::Literal(s) => s.clone(),
        PropertyValue::Dimension(d) => d.to_kdl_string(),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_core::KdlAdapter;

    // ── Fixtures ──────────────────────────────────────────────────────────────

    /// A minimal document with two variants so tests can exercise independent
    /// generation in a single parse.
    ///
    /// Page `page.a` contains:
    ///   - `rect.bg`     — a background rect (has `fill`, no text)
    ///   - `text.label`  — a text node with a single span
    ///
    /// Variant `var.small` → resizes page.a to 320×180, hides `rect.bg`.
    /// Variant `var.large` → resizes page.a to 1920×1080, overrides `text.label` text.
    const DOC_TWO_VARIANTS: &str = r##"zenith version=1 {
  project id="proj.v" name="Variant Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
    token id="color.ink" type="color" value="#111111"
    token id="color.accent" type="color" value="#e11d48"
  }
  styles {}
  document id="doc.v" title="Variant Test" {
    page id="page.a" w=(px)800 h=(px)600 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)800 h=(px)600 fill=(token)"color.bg"
      text id="text.label" x=(px)10 y=(px)10 w=(px)780 h=(px)80 fill=(token)"color.ink" {
        span "original text"
      }
    }
  }
  variants {
    variant id="var.large" source="page.a" w=(px)1920 h=(px)1080 {
      override node="text.label" text="large variant"
    }
    variant id="var.small" source="page.a" w=(px)320 h=(px)180 {
      override node="rect.bg" visible=#false
    }
  }
}
"##;

    /// A document whose single variant overrides a node that does NOT exist —
    /// used to assert the tx engine's behavior on an unknown override target.
    const DOC_MISSING_NODE_VARIANT: &str = r##"zenith version=1 {
  project id="proj.mv" name="Missing Node Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.mv" title="Missing Node Test" {
    page id="page.m" w=(px)400 h=(px)300 {
      rect id="rect.only" x=(px)0 y=(px)0 w=(px)400 h=(px)300 fill=(token)"color.bg"
    }
  }
  variants {
    variant id="var.bad" source="page.m" w=(px)800 h=(px)600 {
      override node="node.does.not.exist" visible=#false
    }
    variant id="var.good" source="page.m" w=(px)200 h=(px)150 {
    }
  }
}
"##;

    /// A document with a fill-override variant.
    const DOC_FILL_VARIANT: &str = r##"zenith version=1 {
  project id="proj.fv" name="Fill Variant Test"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
    token id="color.alt" type="color" value="#3b82f6"
  }
  styles {}
  document id="doc.fv" title="Fill Variant Test" {
    page id="page.f" w=(px)400 h=(px)300 {
      rect id="rect.hero" x=(px)0 y=(px)0 w=(px)400 h=(px)300 fill=(token)"color.bg"
    }
  }
  variants {
    variant id="var.filled" source="page.f" w=(px)400 h=(px)300 {
      override node="rect.hero" fill=(token)"color.alt"
    }
  }
}
"##;

    /// A document with no variants block at all.
    const DOC_NO_VARIANTS: &str = r##"zenith version=1 {
  project id="proj.nv" name="No Variants"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#ffffff"
  }
  styles {}
  document id="doc.nv" title="No Variants" {
    page id="page.nv" w=(px)400 h=(px)300 {
      rect id="rect.bg" x=(px)0 y=(px)0 w=(px)400 h=(px)300 fill=(token)"color.bg"
    }
  }
}
"##;

    // ── Helper ────────────────────────────────────────────────────────────────

    fn parse(src: &str) -> Document {
        KdlAdapter
            .parse(src.as_bytes())
            .expect("fixture must parse")
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn empty_variants_returns_empty_expansion() {
        let doc = parse(DOC_NO_VARIANTS);
        let expansion = expand_variants(&doc);
        assert_eq!(expansion.results.len(), 0);
        assert_eq!(expansion.generated(), 0);
        assert_eq!(expansion.failed(), 0);
    }

    #[test]
    fn two_variants_both_generated_in_id_order() {
        let doc = parse(DOC_TWO_VARIANTS);
        let expansion = expand_variants(&doc);

        // Both variants should succeed.
        assert_eq!(expansion.generated(), 2);
        assert_eq!(expansion.failed(), 0);
        assert_eq!(expansion.results.len(), 2);

        // Results are sorted by id (ascending).  "var.large" < "var.small".
        assert_eq!(expansion.results[0].id, "var.large");
        assert_eq!(expansion.results[1].id, "var.small");

        // Both carry the correct source page.
        assert_eq!(expansion.results[0].source, "page.a");
        assert_eq!(expansion.results[1].source, "page.a");
    }

    #[test]
    fn var_large_page_resized_and_text_replaced() {
        let doc = parse(DOC_TWO_VARIANTS);
        let expansion = expand_variants(&doc);

        let result = expansion
            .results
            .iter()
            .find(|r| r.id == "var.large")
            .expect("var.large must be present");

        let VariantOutcome::Generated(ref materialized) = result.outcome else {
            panic!("var.large must be Generated, got failure");
        };

        // Page should be resized to 1920×1080.
        let page = materialized
            .body
            .pages
            .iter()
            .find(|p| p.id == "page.a")
            .expect("page.a must exist");
        assert_eq!(page.width.value, 1920.0);
        assert_eq!(page.height.value, 1080.0);

        // text.label should now contain "large variant".
        let text_node =
            find_text_node_by_id(materialized, "text.label").expect("text.label must exist");
        let first_span_text: String = text_node.spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(first_span_text, "large variant");
    }

    #[test]
    fn var_small_page_resized_and_node_hidden() {
        let doc = parse(DOC_TWO_VARIANTS);
        let expansion = expand_variants(&doc);

        let result = expansion
            .results
            .iter()
            .find(|r| r.id == "var.small")
            .expect("var.small must be present");

        let VariantOutcome::Generated(ref materialized) = result.outcome else {
            panic!("var.small must be Generated, got failure");
        };

        // Page should be resized to 320×180.
        let page = materialized
            .body
            .pages
            .iter()
            .find(|p| p.id == "page.a")
            .expect("page.a must exist");
        assert_eq!(page.width.value, 320.0);
        assert_eq!(page.height.value, 180.0);

        // rect.bg should be hidden (visible = Some(false)).
        let rect = find_rect_node_by_id(materialized, "rect.bg").expect("rect.bg must exist");
        assert_eq!(rect.visible, Some(false));
    }

    #[test]
    fn fill_override_applied() {
        let doc = parse(DOC_FILL_VARIANT);
        let expansion = expand_variants(&doc);

        assert_eq!(expansion.generated(), 1);
        assert_eq!(expansion.failed(), 0);

        let result = &expansion.results[0];
        assert_eq!(result.id, "var.filled");

        let VariantOutcome::Generated(ref materialized) = result.outcome else {
            panic!("var.filled must be Generated");
        };

        // rect.hero fill should be TokenRef("color.alt").
        let rect = find_rect_node_by_id(materialized, "rect.hero").expect("rect.hero must exist");
        assert_eq!(
            rect.fill,
            Some(PropertyValue::TokenRef("color.alt".to_owned()))
        );
    }

    #[test]
    fn missing_node_override_fails_sibling_still_generated() {
        let doc = parse(DOC_MISSING_NODE_VARIANT);
        let expansion = expand_variants(&doc);

        // var.bad targets a missing node → should fail.
        // var.good has no overrides → should succeed.
        assert_eq!(expansion.results.len(), 2);

        // Results sorted by id: "var.bad" < "var.good".
        let bad = &expansion.results[0];
        let good = &expansion.results[1];
        assert_eq!(bad.id, "var.bad");
        assert_eq!(good.id, "var.good");

        // var.good must be Generated regardless of var.bad's outcome.
        assert!(
            matches!(good.outcome, VariantOutcome::Generated(_)),
            "var.good must be Generated"
        );

        // var.bad: the tx engine emits tx.unknown_node for a missing override target,
        // which causes a Rejected status → Failed outcome.
        assert!(
            matches!(bad.outcome, VariantOutcome::Failed(_)),
            "var.bad must be Failed because its override target does not exist"
        );

        if let VariantOutcome::Failed(ref reason) = bad.outcome {
            assert!(
                reason.contains("node.does.not.exist"),
                "failure reason should mention the missing node id; got: {reason}"
            );
        }
    }

    #[test]
    fn source_document_not_mutated() {
        // expand_variants takes &Document; the source doc must be identical
        // after the call (no shared mutation).
        let doc = parse(DOC_TWO_VARIANTS);
        let original_page_w = doc.body.pages[0].width.value;

        let _ = expand_variants(&doc);

        // Source page width must still be 800.
        assert_eq!(
            doc.body.pages[0].width.value, original_page_w,
            "source document must not be mutated"
        );
    }

    // ── Node-finding helpers (test-only) ─────────────────────────────────────

    fn find_text_node_by_id<'a>(doc: &'a Document, id: &str) -> Option<&'a zenith_core::TextNode> {
        for page in &doc.body.pages {
            if let Some(n) = find_text_in_nodes(&page.children, id) {
                return Some(n);
            }
        }
        None
    }

    fn find_text_in_nodes<'a>(
        nodes: &'a [zenith_core::Node],
        id: &str,
    ) -> Option<&'a zenith_core::TextNode> {
        for node in nodes {
            match node {
                zenith_core::Node::Text(n) if n.id == id => return Some(n),
                zenith_core::Node::Frame(n) => {
                    if let Some(found) = find_text_in_nodes(&n.children, id) {
                        return Some(found);
                    }
                }
                zenith_core::Node::Group(n) => {
                    if let Some(found) = find_text_in_nodes(&n.children, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn find_rect_node_by_id<'a>(doc: &'a Document, id: &str) -> Option<&'a zenith_core::RectNode> {
        for page in &doc.body.pages {
            if let Some(n) = find_rect_in_nodes(&page.children, id) {
                return Some(n);
            }
        }
        None
    }

    fn find_rect_in_nodes<'a>(
        nodes: &'a [zenith_core::Node],
        id: &str,
    ) -> Option<&'a zenith_core::RectNode> {
        for node in nodes {
            match node {
                zenith_core::Node::Rect(n) if n.id == id => return Some(n),
                zenith_core::Node::Frame(n) => {
                    if let Some(found) = find_rect_in_nodes(&n.children, id) {
                        return Some(found);
                    }
                }
                zenith_core::Node::Group(n) => {
                    if let Some(found) = find_rect_in_nodes(&n.children, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
}
