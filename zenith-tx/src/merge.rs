//! Pure candidate-page merge helper.
//!
//! [`merge_candidate_page`] deep-copies a source page's content into a target
//! page in place, suffixing every descendant id. The transform is pure — no
//! filesystem access, no session I/O, no validation. The caller is responsible
//! for validating the mutated document.

use zenith_core::ast::document::Page;

use crate::engine::structure::{suffix_ids_in_children, suffix_zone_and_fold_ids};

/// Merge a candidate source page's content into `target` in place: deep-copy
/// the source's children, safe-zones, and folds with every descendant id
/// suffixed by `id_suffix`, replacing the target's content.
///
/// Pure transform — no filesystem, no validation (the caller validates the
/// resulting document).
pub fn merge_candidate_page(source: &Page, target: &mut Page, id_suffix: &str) {
    let mut children = source.children.clone();
    let mut safe_zones = source.safe_zones.clone();
    let mut folds = source.folds.clone();

    suffix_ids_in_children(&mut children, id_suffix);
    suffix_zone_and_fold_ids(&mut safe_zones, &mut folds, id_suffix);

    target.children = children;
    target.safe_zones = safe_zones;
    target.folds = folds;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use zenith_core::{KdlAdapter, KdlSource};

    use super::merge_candidate_page;

    // Parse a minimal document and return its first page (panics on error — tests only).
    fn parse_first_page(src: &str) -> zenith_core::ast::document::Page {
        KdlAdapter
            .parse(src.as_bytes())
            .expect("test doc must parse")
            .body
            .pages
            .into_iter()
            .next()
            .expect("test doc must have at least one page")
    }

    const SOURCE_DOC: &str = r##"zenith version=1 {
  project id="proj.src" name="Src"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.src" title="Src" {
    page id="page.source" w=(px)400 h=(px)300 {
      rect id="rect.a" x=(px)0 y=(px)0 w=(px)100 h=(px)100
      rect id="rect.b" x=(px)100 y=(px)0 w=(px)100 h=(px)100
    }
  }
}
"##;

    const TARGET_DOC: &str = r##"zenith version=1 {
  project id="proj.tgt" name="Tgt"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.tgt" title="Tgt" {
    page id="page.target" w=(px)400 h=(px)300 {
      rect id="old.rect" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}
"##;

    fn rect_id(node: &zenith_core::Node) -> Option<&str> {
        match node {
            zenith_core::Node::Rect(r) => Some(r.id.as_str()),
            _ => None,
        }
    }

    #[test]
    fn children_copied_with_suffixed_ids() {
        let source = parse_first_page(SOURCE_DOC);
        let mut target = parse_first_page(TARGET_DOC);

        merge_candidate_page(&source, &mut target, ".promoted");

        assert_eq!(target.children.len(), 2, "target must have 2 children");
        let ids: Vec<&str> = target.children.iter().filter_map(rect_id).collect();
        assert!(
            ids.contains(&"rect.a.promoted"),
            "rect.a must be suffixed; got {ids:?}"
        );
        assert!(
            ids.contains(&"rect.b.promoted"),
            "rect.b must be suffixed; got {ids:?}"
        );
    }

    #[test]
    fn source_unchanged_after_merge() {
        let source = parse_first_page(SOURCE_DOC);
        let source_children_len = source.children.len();
        let first_id = source
            .children
            .first()
            .and_then(rect_id)
            .unwrap()
            .to_owned();

        let mut target = parse_first_page(TARGET_DOC);
        merge_candidate_page(&source, &mut target, ".p");

        assert_eq!(
            source.children.len(),
            source_children_len,
            "source must not be mutated"
        );
        assert_eq!(
            source.children.first().and_then(rect_id),
            Some(first_id.as_str()),
        );
    }

    #[test]
    fn empty_source_replaces_target_children() {
        const EMPTY_SOURCE: &str = r##"zenith version=1 {
  project id="proj.empty" name="E"
  tokens format="zenith-token-v1" {}
  styles {}
  document id="doc.empty" title="E" {
    page id="page.source" w=(px)100 h=(px)100 {}
  }
}
"##;
        let source = parse_first_page(EMPTY_SOURCE);
        let mut target = parse_first_page(TARGET_DOC);
        assert!(
            !target.children.is_empty(),
            "target must start with children"
        );

        merge_candidate_page(&source, &mut target, ".p");

        assert!(
            target.children.is_empty(),
            "empty source must produce empty target children"
        );
    }
}
