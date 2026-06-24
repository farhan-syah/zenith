//! Integration tests for the canonical writer: nodes.
//!
//! Leaf and decorative nodes — images, ellipses, assets, safe-zones, folds, and
//! unknown properties — parse, serialize, and round-trip.
//!
//! Moved verbatim from the former in-`src` `format/writer/tests.rs`; the body of
//! every test is unchanged — only import paths were rewritten to the public
//! `zenith_core` surface. Span-stripping helpers live in `common`.

mod common;

use common::*;
use zenith_core::format::format_document;

/// **Image clip round-trip**: `clip="rounded"` + `clip-radius=(token)"..."`
/// must parse onto the `ImageNode`, be re-emitted by the formatter, and survive
/// a format → re-parse round-trip.
#[test]
fn test_image_clip_parse_format_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.iclip" name="IClip"
  assets {
    asset id="asset.pfp" kind="image" src="assets/pfp.png"
  }
  tokens format="zenith-token-v1" {
    token id="size.radius.avatar" type="dimension" value=(px)24
  }
  styles {
  }
  document id="doc.iclip" title="IClip" {
    page id="page.iclip" w=(px)400 h=(px)300 {
      image id="av" asset="asset.pfp" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fit="cover" clip="rounded" clip-radius=(token)"size.radius.avatar"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let image_node = match &doc.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image node, got {other:?}"),
    };
    assert_eq!(image_node.clip.as_deref(), Some("rounded"));
    use zenith_core::PropertyValue;
    assert_eq!(
        image_node.clip_radius,
        Some(PropertyValue::TokenRef("size.radius.avatar".to_owned())),
        "clip-radius must parse as a token ref"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("clip=\"rounded\""),
        "formatter must emit clip=\"rounded\"; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("clip-radius=(token)\"size.radius.avatar\""),
        "formatter must emit clip-radius token; got:\n{formatted_str}"
    );

    let doc2 = adapter
        .parse(formatted_str.as_bytes())
        .expect("re-parse after format");
    let image2 = match &doc2.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image node on re-parse, got {other:?}"),
    };
    assert_eq!(image2.clip.as_deref(), Some("rounded"));
    assert_eq!(
        image2.clip_radius,
        Some(PropertyValue::TokenRef("size.radius.avatar".to_owned())),
        "clip-radius must survive a format → re-parse round-trip"
    );
}

/// A `.zen` document with an image node exercising the string and `(pct)`
/// object-position forms.
const WITH_IMAGE: &str = r##"zenith version=1 {
  project id="proj.img" name="Image Test"
  assets {
    asset id="asset.logo" kind="image" src="assets/logo.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.img" title="Image Test" {
    page id="page.one" w=(px)320 h=(px)200 {
      image id="img.logo" asset="asset.logo" x=(px)80 y=(px)60 w=(px)160 h=(px)48 fit="contain" object-position-x="center" object-position-y=(pct)25
    }
  }
}
"##;

/// Image node parses all fields including both object-position forms.
#[test]
fn image_parses_fields() {
    use zenith_core::{Node, ObjectPosition, Unit};
    let adapter = KdlAdapter;
    let doc = adapter.parse(WITH_IMAGE.as_bytes()).expect("parse");
    let node = &doc.body.pages[0].children[0];
    let img = match node {
        Node::Image(i) => i,
        other => panic!("expected Image, got {other:?}"),
    };
    assert_eq!(img.id, "img.logo");
    assert_eq!(img.asset, "asset.logo");
    assert_eq!(img.x.as_ref().map(|d| d.value), Some(80.0));
    assert_eq!(img.y.as_ref().map(|d| d.value), Some(60.0));
    assert_eq!(img.w.as_ref().map(|d| d.value), Some(160.0));
    assert_eq!(img.h.as_ref().map(|d| d.value), Some(48.0));
    assert!(matches!(img.x.as_ref().map(|d| &d.unit), Some(Unit::Px)));
    assert_eq!(img.fit.as_deref(), Some("contain"));
    assert_eq!(img.object_position_x, Some(ObjectPosition::Center));
    assert_eq!(img.object_position_y, Some(ObjectPosition::Pct(25.0)));
}

/// Image node round-trips through format → parse with fields intact, and
/// the formatter is idempotent (incl. an object-position `(pct)25`).
#[test]
fn image_format_round_trip_and_idempotency() {
    use zenith_core::{Node, ObjectPosition};
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(WITH_IMAGE.as_bytes()).expect("parse 1");
    let s1 = format_document(&doc1).expect("format 1");

    // The (pct)25 must survive as an annotated number, not a string.
    let text = String::from_utf8(s1.clone()).unwrap();
    assert!(
        text.contains("object-position-y=(pct)25"),
        "object-position (pct) must format as annotated number; got:\n{text}"
    );
    assert!(
        text.contains("object-position-x=\"center\""),
        "object-position anchor must format as string; got:\n{text}"
    );

    let doc2 = adapter.parse(&s1).expect("parse 2");
    let img2 = match &doc2.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image, got {other:?}"),
    };
    assert_eq!(img2.asset, "asset.logo");
    assert_eq!(img2.fit.as_deref(), Some("contain"));
    assert_eq!(img2.object_position_x, Some(ObjectPosition::Center));
    assert_eq!(img2.object_position_y, Some(ObjectPosition::Pct(25.0)));

    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        String::from_utf8(s1).unwrap(),
        String::from_utf8(s2).unwrap(),
        "image format must be idempotent"
    );
}

// ── Style block parse + format tests ──────────────────────────────────

/// **src-rect round-trip**: an image node with `src-x`/`src-y`/`src-w`/`src-h`
/// must parse → format → re-parse byte-identically (all four src-* fields
/// survive the round-trip).
#[test]
fn test_image_src_rect_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.srcrt" name="SrcRt"
  assets {
    asset id="asset.photo" kind="image" src="assets/photo.png"
  }
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.srcrt" title="SrcRt" {
    page id="page.srcrt" w=(px)400 h=(px)300 {
      image id="img.srcrt" asset="asset.photo" x=(px)0 y=(px)0 w=(px)200 h=(px)100 src-x=(px)10 src-y=(px)20 src-w=(px)50 src-h=(px)60 fit="stretch"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");
    let image_node = match &doc.body.pages[0].children[0] {
        Node::Image(i) => i,
        other => panic!("expected Image node, got {other:?}"),
    };

    use zenith_core::{Dimension, Unit};
    assert_eq!(
        image_node.src_x,
        Some(Dimension {
            value: 10.0,
            unit: Unit::Px
        }),
        "src-x must parse to (px)10"
    );
    assert_eq!(
        image_node.src_y,
        Some(Dimension {
            value: 20.0,
            unit: Unit::Px
        }),
        "src-y must parse to (px)20"
    );
    assert_eq!(
        image_node.src_w,
        Some(Dimension {
            value: 50.0,
            unit: Unit::Px
        }),
        "src-w must parse to (px)50"
    );
    assert_eq!(
        image_node.src_h,
        Some(Dimension {
            value: 60.0,
            unit: Unit::Px
        }),
        "src-h must parse to (px)60"
    );

    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted.clone()).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("src-x=(px)10"),
        "formatter must emit src-x=(px)10; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("src-y=(px)20"),
        "formatter must emit src-y=(px)20; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("src-w=(px)50"),
        "formatter must emit src-w=(px)50; got:\n{formatted_str}"
    );
    assert!(
        formatted_str.contains("src-h=(px)60"),
        "formatter must emit src-h=(px)60; got:\n{formatted_str}"
    );

    let reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc),
        strip_spans(reparsed),
        "src-rect image must round-trip identically"
    );
}

// ── sections: parse, serialize, and round-trip ────────────────────────

/// **Ellipse stroke + stroke-width round-trip**: an ellipse with both
/// `stroke` and `stroke-width` tokens must survive parse→format→parse with
/// those fields preserved in the canonical position (after `fill`).
#[test]
fn ellipse_stroke_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.es" name="ES"
  tokens format="zenith-token-v1" {
    token id="color.border" type="color" value="#334155"
    token id="size.border" type="dimension" value=(px)3
  }
  styles {
  }
  document id="doc.es" title="ES" {
    page id="p" w=(px)200 h=(px)200 {
      ellipse id="e" x=(px)10 y=(px)10 w=(px)80 h=(px)80 stroke=(token)"color.border" stroke-width=(token)"size.border"
    }
  }
}
"##;
    use zenith_core::{Node, PropertyValue};
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");

    // Verify AST fields are set.
    match &doc.body.pages[0].children[0] {
        Node::Ellipse(e) => {
            assert_eq!(
                e.stroke,
                Some(PropertyValue::TokenRef("color.border".to_owned())),
                "stroke must parse to TokenRef(color.border)"
            );
            assert_eq!(
                e.stroke_width,
                Some(PropertyValue::TokenRef("size.border".to_owned())),
                "stroke_width must parse to TokenRef(size.border)"
            );
            assert!(e.fill.is_none(), "fill must be absent");
        }
        other => panic!("expected Ellipse, got {other:?}"),
    }

    // Format and re-parse — the tokens must survive.
    let formatted = format_document(&doc).expect("format");
    let formatted_str = String::from_utf8(formatted.clone()).expect("utf8");
    let doc2 = adapter.parse(&formatted).expect("re-parse");
    match &doc2.body.pages[0].children[0] {
        Node::Ellipse(e) => {
            assert_eq!(
                e.stroke,
                Some(PropertyValue::TokenRef("color.border".to_owned())),
                "stroke must survive format round-trip"
            );
            assert_eq!(
                e.stroke_width,
                Some(PropertyValue::TokenRef("size.border".to_owned())),
                "stroke_width must survive format round-trip"
            );
        }
        other => panic!("expected Ellipse on re-parse, got {other:?}"),
    }

    // Canonical position: stroke comes after fill.
    let ellipse_line = formatted_str
        .lines()
        .find(|l| l.trim_start().starts_with("ellipse"))
        .expect("must find ellipse line");
    assert!(
        ellipse_line.contains("stroke=(token)\"color.border\""),
        "formatted line must contain stroke token; got: {ellipse_line}"
    );
    assert!(
        ellipse_line.contains("stroke-width=(token)\"size.border\""),
        "formatted line must contain stroke-width token; got: {ellipse_line}"
    );
    // stroke must come before stroke-width (canonical order).
    let pos_stroke = ellipse_line.find(" stroke=").expect("must have stroke=");
    let pos_sw = ellipse_line
        .find(" stroke-width=")
        .expect("must have stroke-width=");
    assert!(
        pos_stroke < pos_sw,
        "stroke= must appear before stroke-width= in canonical output"
    );

    // Idempotency: format(format(doc)) == format(doc).
    let s2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted_str,
        String::from_utf8(s2).unwrap(),
        "ellipse stroke formatting must be idempotent"
    );
}

// ── Image node parse + format tests ───────────────────────────────────

/// A `.zen` document with a `safe-zone` declared as a page child.
const SAFE_ZONE_DOC: &str = r##"zenith version=1 {
  project id="proj.sz" name="Safe Zone Project"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.sz" title="Safe Zone Doc" {
    page id="page.one" w=(px)1500 h=(px)500 {
      safe-zone id="sz.avatar" type="exclusion" x=(px)0 y=(px)358 w=(px)175 h=(px)142 label="X avatar dead zone"
      rect id="logo" x=(px)600 y=(px)40 w=(px)200 h=(px)80 fill="#ffffff"
    }
  }
}
"##;

/// **Parse**: a `safe-zone` page child lands in `page.safe_zones`, NOT in
/// `page.children`.
#[test]
fn test_safe_zone_parses_into_page_not_children() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(SAFE_ZONE_DOC.as_bytes())
        .expect("parse must succeed");
    let page = &doc.body.pages[0];

    assert_eq!(page.safe_zones.len(), 1, "exactly one safe-zone parsed");
    let zone = &page.safe_zones[0];
    assert_eq!(zone.id, "sz.avatar");
    assert_eq!(zone.zone_type, zenith_core::SafeZoneType::Exclusion);
    assert_eq!(zone.label.as_deref(), Some("X avatar dead zone"));

    // The renderable rect is the ONLY child; the safe-zone is not a child.
    assert_eq!(page.children.len(), 1, "only the rect is a child node");
    match &page.children[0] {
        Node::Rect(r) => assert_eq!(r.id, "logo"),
        other => panic!("expected Rect, got {other:?}"),
    }
}

/// **Format round-trip**: a safe-zone survives a parse → format → parse pass
/// unchanged (spans excluded).
#[test]
fn test_safe_zone_format_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter
        .parse(SAFE_ZONE_DOC.as_bytes())
        .expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");

    // The emitted line carries the canonical safe-zone shape.
    let text = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        text.contains(
            "safe-zone id=\"sz.avatar\" type=\"exclusion\" \
             x=(px)0 y=(px)358 w=(px)175 h=(px)142 label=\"X avatar dead zone\""
        ),
        "formatted safe-zone line missing/incorrect; output:\n{text}"
    );

    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc_orig),
        strip_spans(doc_reparsed),
        "safe-zone must survive a format round-trip (spans excluded)"
    );
}

/// A `.zen` document with a `fold` declared as a page child.
const FOLD_DOC: &str = r##"zenith version=1 {
  project id="proj.fold" name="Fold Project"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.fold" title="Fold Doc" {
    page id="page.one" w=(px)2480 h=(px)1000 {
      fold id="fold.1" orientation="vertical" position=(px)1169
      rect id="logo" x=(px)600 y=(px)40 w=(px)200 h=(px)80 fill="#ffffff"
    }
  }
}
"##;

/// **Parse**: a `fold` page child lands in `page.folds`, NOT in
/// `page.children`.
#[test]
fn test_fold_parses_into_page_not_children() {
    let adapter = KdlAdapter;
    let doc = adapter
        .parse(FOLD_DOC.as_bytes())
        .expect("parse must succeed");
    let page = &doc.body.pages[0];

    assert_eq!(page.folds.len(), 1, "exactly one fold parsed");
    let fold = &page.folds[0];
    assert_eq!(fold.id, "fold.1");
    assert_eq!(fold.orientation, "vertical");
    let pos = fold.position.as_ref().expect("position present");
    assert_eq!(pos.value, 1169.0);

    // The renderable rect is the ONLY child; the fold is not a child.
    assert_eq!(page.children.len(), 1, "only the rect is a child node");
    match &page.children[0] {
        Node::Rect(r) => assert_eq!(r.id, "logo"),
        other => panic!("expected Rect, got {other:?}"),
    }
}

/// **Format round-trip**: a fold survives a parse → format → parse pass
/// unchanged (spans excluded).
#[test]
fn test_fold_format_round_trip() {
    let adapter = KdlAdapter;
    let doc_orig = adapter.parse(FOLD_DOC.as_bytes()).expect("original parse");
    let formatted = format_document(&doc_orig).expect("format");

    let text = String::from_utf8(formatted.clone()).expect("utf8");
    assert!(
        text.contains("fold id=\"fold.1\" orientation=\"vertical\" position=(px)1169"),
        "formatted fold line missing/incorrect; output:\n{text}"
    );

    let doc_reparsed = adapter.parse(&formatted).expect("re-parse after format");
    assert_eq!(
        strip_spans(doc_orig),
        strip_spans(doc_reparsed),
        "fold must survive a format round-trip (spans excluded)"
    );
}

/// **Unknown-property multi-type round-trip**: unknown properties of every
/// KDL value type survive parse→format→parse with their type intact, and
/// the output is idempotent (format twice → identical bytes).
#[test]
fn test_unknown_property_all_types_round_trip() {
    // Each property exercises one KdlValue variant.
    // Raw string r##"..."## needed because KDL v2 booleans/null use `#`.
    let src = r##"zenith version=1 {
  project id="proj.rt" name="RT"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.rt" title="RT" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 future-flag=#true future-float=1.5 future-int=42 future-null=#null future-str="hi"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");

    // Verify all five types landed correctly after the first parse.
    let rect = match &doc1.body.pages[0].children[0] {
        zenith_core::Node::Rect(r) => r,
        other => panic!("expected Rect, got {other:?}"),
    };
    assert_eq!(
        rect.unknown_props["future-flag"].value,
        zenith_core::UnknownValue::Bool(true),
        "boolean must parse as UnknownValue::Bool(true), not a string"
    );
    assert_eq!(
        rect.unknown_props["future-int"].value,
        zenith_core::UnknownValue::Integer(42),
        "integer must parse as UnknownValue::Integer(42)"
    );
    assert_eq!(
        rect.unknown_props["future-float"].value,
        zenith_core::UnknownValue::Float(1.5),
        "float must parse as UnknownValue::Float(1.5)"
    );
    assert_eq!(
        rect.unknown_props["future-str"].value,
        zenith_core::UnknownValue::String("hi".to_owned()),
        "string must parse as UnknownValue::String"
    );
    assert_eq!(
        rect.unknown_props["future-null"].value,
        zenith_core::UnknownValue::Null,
        "null must parse as UnknownValue::Null"
    );

    // Format once → parse → assert same typed values survive (round-trip).
    let formatted1 = format_document(&doc1).expect("format 1");
    let doc2 = adapter.parse(&formatted1).expect("parse 2 after format");
    let rect2 = match &doc2.body.pages[0].children[0] {
        zenith_core::Node::Rect(r) => r,
        other => panic!("expected Rect in re-parsed doc, got {other:?}"),
    };
    assert_eq!(
        rect2.unknown_props["future-flag"].value,
        zenith_core::UnknownValue::Bool(true),
        "boolean must survive format round-trip as UnknownValue::Bool(true)"
    );
    assert_eq!(
        rect2.unknown_props["future-int"].value,
        zenith_core::UnknownValue::Integer(42),
        "integer must survive format round-trip as UnknownValue::Integer(42)"
    );
    assert_eq!(
        rect2.unknown_props["future-float"].value,
        zenith_core::UnknownValue::Float(1.5),
        "float must survive format round-trip"
    );
    assert_eq!(
        rect2.unknown_props["future-str"].value,
        zenith_core::UnknownValue::String("hi".to_owned()),
        "string must survive format round-trip"
    );
    assert_eq!(
        rect2.unknown_props["future-null"].value,
        zenith_core::UnknownValue::Null,
        "null must survive format round-trip"
    );

    // Idempotence: format a second time → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted1, formatted2,
        "format must be idempotent for documents with unknown properties of all types"
    );
}

/// **Unknown-property type-annotation round-trip**: KDL type annotations on
/// unrecognized properties (e.g. `(px)42`, `(token)"color.brand"`) must be
/// captured on parse, re-emitted in the value position on format, and survive
/// a full parse→format→parse cycle byte-identically. Non-annotated unknown
/// values must remain unchanged.
#[test]
fn test_unknown_property_type_annotation_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.ann" name="Ann"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.ann" title="Ann" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 mystery=(px)42 magic=(token)"color.brand" plain="hello" flag=#true
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc1 = adapter.parse(src.as_bytes()).expect("parse 1");

    let rect = match &doc1.body.pages[0].children[0] {
        zenith_core::Node::Rect(r) => r,
        other => panic!("expected Rect, got {other:?}"),
    };

    // Annotations captured on parse.
    assert_eq!(
        rect.unknown_props["mystery"].ty.as_deref(),
        Some("px"),
        "`(px)42` must capture ty = Some(\"px\")"
    );
    assert_eq!(
        rect.unknown_props["mystery"].value,
        zenith_core::UnknownValue::Integer(42),
    );
    assert_eq!(
        rect.unknown_props["magic"].ty.as_deref(),
        Some("token"),
        "`(token)\"color.brand\"` must capture ty = Some(\"token\")"
    );
    assert_eq!(
        rect.unknown_props["magic"].value,
        zenith_core::UnknownValue::String("color.brand".to_owned()),
    );
    // Non-annotated unknown props have ty = None.
    assert_eq!(
        rect.unknown_props["plain"].ty, None,
        "non-annotated `plain` must have ty = None"
    );
    assert_eq!(
        rect.unknown_props["flag"].ty, None,
        "non-annotated `flag` must have ty = None"
    );

    // Format → the annotation is emitted in the value position.
    let formatted1 = format_document(&doc1).expect("format 1");
    let text = String::from_utf8_lossy(&formatted1);
    assert!(
        text.contains("mystery=(px)42"),
        "formatted output must contain `mystery=(px)42`, got:\n{text}"
    );
    assert!(
        text.contains(r#"magic=(token)"color.brand""#),
        "formatted output must contain `magic=(token)\"color.brand\"`, got:\n{text}"
    );
    assert!(
        text.contains(r#"plain="hello""#),
        "non-annotated `plain=\"hello\"` must be unchanged, got:\n{text}"
    );
    assert!(
        text.contains("flag=#true"),
        "non-annotated `flag=#true` must be unchanged, got:\n{text}"
    );

    // Re-parse → unknown_props (value + ty) are identical to the first parse.
    let doc2 = adapter.parse(&formatted1).expect("parse 2 after format");
    let rect2 = match &doc2.body.pages[0].children[0] {
        zenith_core::Node::Rect(r) => r,
        other => panic!("expected Rect in re-parsed doc, got {other:?}"),
    };
    assert_eq!(
        rect.unknown_props, rect2.unknown_props,
        "unknown_props (value + ty) must be byte-stable across parse→format→parse"
    );

    // Idempotence: format a second time → identical bytes.
    let formatted2 = format_document(&doc2).expect("format 2");
    assert_eq!(
        formatted1, formatted2,
        "format must be idempotent for annotated unknown properties"
    );
}

/// **Forward-compat preservation**: an unknown property on a rect survives
/// a format round-trip.
#[test]
fn test_unknown_property_preserved() {
    let src = r##"zenith version=1 {
  project id="proj.unk" name="Unk"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.unk" title="Unk" {
    page id="p" w=(px)100 h=(px)100 {
      rect id="r" x=(px)0 y=(px)0 w=(px)10 h=(px)10 future-prop="hello"
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse");
    let out = format_document(&doc).expect("format");
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("future-prop="),
        "unknown property `future-prop` must survive format; got:\n{text}"
    );
}

/// **anchor-sibling round-trip**: a rect with `anchor="top-left"` and
/// `anchor-sibling="some-id"` must parse onto the AST with both fields set,
/// survive `format_document`, and still carry `anchor-sibling="some-id"` after
/// a format → re-parse cycle.
#[test]
fn test_anchor_sibling_round_trip() {
    let src = r##"zenith version=1 {
  project id="proj.as" name="AS"
  tokens format="zenith-token-v1" {
  }
  styles {
  }
  document id="doc.as" title="AS" {
    page id="p" w=(px)200 h=(px)200 {
      rect id="r" anchor="top-left" anchor-sibling="some-id" x=(px)0 y=(px)0 w=(px)50 h=(px)50
    }
  }
}
"##;
    let adapter = KdlAdapter;
    let doc = adapter.parse(src.as_bytes()).expect("parse must succeed");

    // Verify AST fields are set after parse.
    match &doc.body.pages[0].children[0] {
        Node::Rect(r) => {
            assert_eq!(
                r.anchor.as_deref(),
                Some("top-left"),
                "anchor must parse to \"top-left\""
            );
            assert_eq!(
                r.anchor_sibling.as_deref(),
                Some("some-id"),
                "anchor-sibling must parse to \"some-id\""
            );
        }
        other => panic!("expected Rect, got {other:?}"),
    }

    // Format and assert the KDL text contains anchor-sibling.
    let formatted = format_document(&doc).expect("format must succeed");
    let formatted_str = String::from_utf8(formatted).expect("formatted must be utf8");
    assert!(
        formatted_str.contains("anchor-sibling=\"some-id\""),
        "formatter must emit anchor-sibling=\"some-id\"; got:\n{formatted_str}"
    );

    // Re-parse the formatted output and verify anchor-sibling survived.
    let doc2 = adapter
        .parse(formatted_str.as_bytes())
        .expect("re-parse after format must succeed");
    match &doc2.body.pages[0].children[0] {
        Node::Rect(r) => {
            assert_eq!(
                r.anchor_sibling.as_deref(),
                Some("some-id"),
                "anchor-sibling must survive a format → re-parse round-trip"
            );
            assert_eq!(
                r.anchor.as_deref(),
                Some("top-left"),
                "anchor must survive a format → re-parse round-trip"
            );
        }
        other => panic!("expected Rect on re-parse, got {other:?}"),
    }
}
