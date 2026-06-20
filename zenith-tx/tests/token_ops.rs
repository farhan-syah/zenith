//! Integration tests for the `create_token` and `update_token_value`
//! transaction ops.

mod common;
use common::*;
use zenith_tx::{Op, Permissions, Transaction, TxStatus, run_transaction};

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// Document with a color token, a dimension token, and a rect that references
/// the color token.  All post-validate constraints are satisfied.
const TOKEN_DOC: &str = r##"zenith version=1 {
  project id="proj" name="Test"
  tokens format="zenith-token-v1" {
    token id="color.accent" type="color" value="#3b82f6"
    token id="size.base" type="dimension" value=(px)16
  }
  styles { }
  document id="doc1" title="T" {
    page id="pg1" w=(px)400 h=(px)300 {
      rect id="r1" x=(px)0 y=(px)0 w=(px)100 h=(px)100 fill=(token)"color.accent"
    }
  }
}"##;

// ── create_token: color → Accepted ───────────────────────────────────────────

/// (a) create_token with a new color id is accepted; source_after contains the
/// new token id and value; the token count increases by 1.
#[test]
fn create_token_color_accepted() {
    let doc = parse(TOKEN_DOC);
    let initial_count = doc.tokens.tokens.len();

    let tx = Transaction {
        ops: vec![Op::CreateToken {
            id: "color.brand".to_owned(),
            token_type: "color".to_owned(),
            value: "#e11d48".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "expected Accepted; diagnostics: {:?}",
        result.diagnostics
    );
    assert_eq!(
        result.affected_node_ids,
        vec!["color.brand".to_owned()],
        "affected must contain the new token id"
    );
    assert!(
        result.source_after.contains("id=\"color.brand\""),
        "source_after must contain the new token id; got:\n{}",
        result.source_after
    );
    assert!(
        result.source_after.contains("#e11d48"),
        "source_after must contain the token value; got:\n{}",
        result.source_after
    );

    // Token count + 1 in the re-parsed document.
    let after_doc = parse(&result.source_after);
    assert_eq!(
        after_doc.tokens.tokens.len(),
        initial_count + 1,
        "token count should increase by 1"
    );
}

// ── create_token: dimension → Accepted with round-trip ───────────────────────

/// (b) create_token with a dimension value `(px)40` is accepted and the value
/// round-trips in source_after.
#[test]
fn create_token_dimension_accepted() {
    let doc = parse(TOKEN_DOC);

    let tx = Transaction {
        ops: vec![Op::CreateToken {
            id: "size.new".to_owned(),
            token_type: "dimension".to_owned(),
            value: "(px)40".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "expected Accepted; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result.source_after.contains("id=\"size.new\""),
        "source_after must contain the new token id; got:\n{}",
        result.source_after
    );
    // The writer emits dimension as `(px)40` (or `(px)40.0`) — verify the unit.
    assert!(
        result.source_after.contains("(px)"),
        "source_after must contain the (px) dimension unit; got:\n{}",
        result.source_after
    );
}

// ── create_token: duplicate id → Rejected ─────────────────────────────────────

/// (c) create_token with an id that already exists → Rejected (tx.duplicate_id).
#[test]
fn create_token_duplicate_id_rejected() {
    let doc = parse(TOKEN_DOC);
    // TOKEN_DOC already declares "color.accent".
    let tx = Transaction {
        ops: vec![Op::CreateToken {
            id: "color.accent".to_owned(),
            token_type: "color".to_owned(),
            value: "#ffffff".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "expected Rejected; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.duplicate_id"),
        "expected tx.duplicate_id; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}

// ── create_token: gradient type → Rejected ────────────────────────────────────

/// (d) create_token with `type="gradient"` → Rejected (tx.invalid_value).
#[test]
fn create_token_gradient_type_rejected() {
    let doc = parse(TOKEN_DOC);
    let tx = Transaction {
        ops: vec![Op::CreateToken {
            id: "grad.new".to_owned(),
            token_type: "gradient".to_owned(),
            value: "#ff0000".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "expected Rejected; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}

// ── create_token: unparseable dimension → Rejected ────────────────────────────

/// (e) create_token with a non-parseable dimension value → Rejected
/// (tx.invalid_value).
#[test]
fn create_token_unparseable_dimension_rejected() {
    let doc = parse(TOKEN_DOC);
    let tx = Transaction {
        ops: vec![Op::CreateToken {
            id: "size.bad".to_owned(),
            token_type: "dimension".to_owned(),
            value: "not-a-dimension".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "expected Rejected; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}

// ── create_token: unparseable number → Rejected ───────────────────────────────

/// (f) create_token with a non-finite / non-numeric value for a number type →
/// Rejected (tx.invalid_value).
#[test]
fn create_token_unparseable_number_rejected() {
    let doc = parse(TOKEN_DOC);
    let tx = Transaction {
        ops: vec![Op::CreateToken {
            id: "num.bad".to_owned(),
            token_type: "number".to_owned(),
            value: "NaN".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "expected Rejected; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}

// ── update_token_value: dimension token → Accepted ───────────────────────────

/// (g) update_token_value on an existing dimension token to `(px)40` → Accepted;
/// source reflects the new value, type is preserved.
#[test]
fn update_token_value_dimension_accepted() {
    let doc = parse(TOKEN_DOC);

    let tx = Transaction {
        ops: vec![Op::UpdateTokenValue {
            id: "size.base".to_owned(),
            value: "(px)40".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Accepted,
        "expected Accepted; diagnostics: {:?}",
        result.diagnostics
    );
    assert_eq!(
        result.affected_node_ids,
        vec!["size.base".to_owned()],
        "affected must contain the updated token id"
    );
    assert!(
        result.source_after.contains("id=\"size.base\""),
        "source_after must still contain size.base; got:\n{}",
        result.source_after
    );
    // Type is preserved: the token still has type="dimension"
    let after_doc = parse(&result.source_after);
    let updated = after_doc
        .tokens
        .tokens
        .iter()
        .find(|t| t.id == "size.base")
        .expect("size.base must still exist");
    assert!(
        matches!(updated.token_type, zenith_core::TokenType::Dimension),
        "token type must remain Dimension; got: {:?}",
        updated.token_type
    );
}

// ── update_token_value: unknown id → Rejected ─────────────────────────────────

/// (h) update_token_value on a non-existent token id → Rejected
/// (tx.unknown_token).
#[test]
fn update_token_value_unknown_id_rejected() {
    let doc = parse(TOKEN_DOC);
    let tx = Transaction {
        ops: vec![Op::UpdateTokenValue {
            id: "color.does_not_exist".to_owned(),
            value: "#ffffff".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "expected Rejected; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.unknown_token"),
        "expected tx.unknown_token; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}

// ── update_token_value: type-mismatched value → Rejected ─────────────────────

/// (i) update_token_value with a value that does not parse for the token's
/// existing type → Rejected (tx.invalid_value).  E.g. `"Inter"` on a dimension
/// token.
#[test]
fn update_token_value_type_mismatch_rejected() {
    let doc = parse(TOKEN_DOC);
    // size.base is a dimension token; "Inter" is not a valid dimension.
    let tx = Transaction {
        ops: vec![Op::UpdateTokenValue {
            id: "size.base".to_owned(),
            value: "Inter".to_owned(),
        }],
        permissions: Permissions::default(),
    };
    let result = run_transaction(&doc, &tx).expect("run_transaction should not error");

    assert_eq!(
        result.status,
        TxStatus::Rejected,
        "expected Rejected; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "tx.invalid_value"),
        "expected tx.invalid_value; got: {:?}",
        result.diagnostics
    );
    assert_eq!(result.source_after, result.source_before);
}
