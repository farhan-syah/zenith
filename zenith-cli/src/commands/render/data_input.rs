//! Data-context loading from JSON or CSV files for `zenith render --data`.
//!
//! [`load_data_context`] reads a JSON object/array or CSV file and returns a
//! [`DataContext`] populated with flat string fields.  JSON nested objects are
//! flattened to dot-paths (`revenue.total`); CSV takes the first data row.

use std::collections::BTreeMap;
use std::path::Path;

use zenith_core::DataContext;

// ── Error type ─────────────────────────────────────────────────────────────

/// Error produced while loading a data context file.
#[derive(Debug)]
pub struct DataInputError {
    /// Human-readable description of the failure.
    pub message: String,
}

impl DataInputError {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl std::fmt::Display for DataInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

// ── Public entry point ─────────────────────────────────────────────────────

/// Load a [`DataContext`] from `path`.
///
/// The file format is inferred from the extension:
/// - `.json` — a JSON **object** (used directly) or a JSON **array** (first
///   element must be an object; empty array or non-object first element →
///   error). Nested objects are flattened to dot-path keys
///   (`{"a":{"b":1}}` → `"a.b" => "1"`). Scalar values: strings are used
///   as-is; numbers and booleans are converted via `to_string`; `null` →
///   empty string. Arrays nested *inside* a data object are skipped.
/// - `.csv` — header row gives field names; the **first data row** supplies
///   values. No data rows → error.
/// - Any other extension → error.
///
/// Returns `Err(DataInputError)` on any I/O, parse, or shape failure.
pub fn load_data_context(path: &Path) -> Result<DataContext, DataInputError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "json" => load_from_json(path),
        "csv" => load_from_csv(path),
        other => Err(DataInputError::new(format!(
            "--data: unsupported file extension '.{other}'; expected .json or .csv"
        ))),
    }
}

// ── JSON loader ────────────────────────────────────────────────────────────

fn load_from_json(path: &Path) -> Result<DataContext, DataInputError> {
    let bytes = std::fs::read(path).map_err(|e| {
        DataInputError::new(format!("--data: cannot read '{}': {}", path.display(), e))
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|e| {
        DataInputError::new(format!(
            "--data: '{}' is not valid UTF-8: {}",
            path.display(),
            e
        ))
    })?;
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        DataInputError::new(format!(
            "--data: '{}' is not valid JSON: {}",
            path.display(),
            e
        ))
    })?;

    // Accept a top-level object or a top-level array (use first element).
    let obj = match value {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Array(arr) => {
            let first = arr.into_iter().next().ok_or_else(|| {
                DataInputError::new(format!(
                    "--data: '{}' is an empty JSON array; expected a non-empty array or object",
                    path.display()
                ))
            })?;
            match first {
                serde_json::Value::Object(map) => map,
                other => {
                    return Err(DataInputError::new(format!(
                        "--data: first element of '{}' is {} not an object",
                        path.display(),
                        json_kind_name(&other)
                    )));
                }
            }
        }
        other => {
            return Err(DataInputError::new(format!(
                "--data: '{}' contains {} not a JSON object or array",
                path.display(),
                json_kind_name(&other)
            )));
        }
    };

    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    flatten_object(&obj, String::new(), &mut fields);
    Ok(DataContext { fields })
}

/// Recursively flatten a JSON object into dot-path keys.
/// Arrays inside the object are skipped (documented behaviour).
fn flatten_object(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: String,
    out: &mut BTreeMap<String, String>,
) {
    for (key, val) in obj {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match val {
            serde_json::Value::Object(inner) => {
                flatten_object(inner, path, out);
            }
            serde_json::Value::Array(_) => {
                // Skip nested arrays — documented, no insertion.
            }
            serde_json::Value::String(s) => {
                out.insert(path, s.clone());
            }
            serde_json::Value::Number(n) => {
                out.insert(path, n.to_string());
            }
            serde_json::Value::Bool(b) => {
                out.insert(path, b.to_string());
            }
            serde_json::Value::Null => {
                out.insert(path, String::new());
            }
        }
    }
}

/// Return a short human-readable type name for error messages.
fn json_kind_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

// ── CSV loader ─────────────────────────────────────────────────────────────

fn load_from_csv(path: &Path) -> Result<DataContext, DataInputError> {
    let bytes = std::fs::read(path).map_err(|e| {
        DataInputError::new(format!("--data: cannot read '{}': {}", path.display(), e))
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|e| {
        DataInputError::new(format!(
            "--data: '{}' is not valid UTF-8: {}",
            path.display(),
            e
        ))
    })?;

    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| {
            DataInputError::new(format!(
                "--data: CSV header error in '{}': {}",
                path.display(),
                e
            ))
        })?
        .clone();

    let first_record = reader
        .records()
        .next()
        .ok_or_else(|| {
            DataInputError::new(format!(
                "--data: '{}' has a header but no data rows",
                path.display()
            ))
        })?
        .map_err(|e| {
            DataInputError::new(format!(
                "--data: CSV parse error in '{}': {}",
                path.display(),
                e
            ))
        })?;

    let fields: BTreeMap<String, String> = headers
        .iter()
        .zip(first_record.iter())
        .map(|(h, v)| (h.to_owned(), v.to_owned()))
        .collect();

    Ok(DataContext { fields })
}

// ── Unit tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(suffix: &str, content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(format!("data{suffix}"));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(content)
            .unwrap();
        (dir, path)
    }

    // ── JSON: flat object ─────────────────────────────────────────────────

    #[test]
    fn json_flat_object_fields() {
        let (_dir, path) = write_temp(".json", br#"{"name": "Alice", "age": 30, "active": true}"#);
        let ctx = load_data_context(&path).unwrap();
        assert_eq!(ctx.get("name"), Some("Alice"));
        assert_eq!(ctx.get("age"), Some("30"));
        assert_eq!(ctx.get("active"), Some("true"));
    }

    #[test]
    fn json_null_becomes_empty_string() {
        let (_dir, path) = write_temp(".json", br#"{"x": null}"#);
        let ctx = load_data_context(&path).unwrap();
        assert_eq!(ctx.get("x"), Some(""));
    }

    // ── JSON: nested object flattens to dot-paths ─────────────────────────

    #[test]
    fn json_nested_object_flattens() {
        let (_dir, path) = write_temp(
            ".json",
            br#"{"revenue": {"total": 42, "tax": 3.5}, "label": "Q1"}"#,
        );
        let ctx = load_data_context(&path).unwrap();
        assert_eq!(ctx.get("revenue.total"), Some("42"));
        assert_eq!(ctx.get("revenue.tax"), Some("3.5"));
        assert_eq!(ctx.get("label"), Some("Q1"));
        // Parent key should NOT be inserted.
        assert_eq!(ctx.get("revenue"), None);
    }

    // ── JSON: array nested inside object is skipped ───────────────────────

    #[test]
    fn json_nested_array_is_skipped() {
        let (_dir, path) = write_temp(".json", br#"{"tags": [1, 2, 3], "val": "ok"}"#);
        let ctx = load_data_context(&path).unwrap();
        assert_eq!(ctx.get("val"), Some("ok"));
        assert_eq!(ctx.get("tags"), None);
    }

    // ── JSON: top-level array — first element used ────────────────────────

    #[test]
    fn json_array_first_element_used() {
        let (_dir, path) = write_temp(
            ".json",
            br##"[{"color": "#ff0000"}, {"color": "#00ff00"}]"##,
        );
        let ctx = load_data_context(&path).unwrap();
        assert_eq!(ctx.get("color"), Some("#ff0000"));
    }

    #[test]
    fn json_empty_array_is_error() {
        let (_dir, path) = write_temp(".json", b"[]");
        let err = load_data_context(&path).unwrap_err();
        assert!(
            err.message.contains("empty JSON array"),
            "expected 'empty JSON array' in error; got: {}",
            err.message
        );
    }

    #[test]
    fn json_array_non_object_first_element_is_error() {
        let (_dir, path) = write_temp(".json", b"[42]");
        let err = load_data_context(&path).unwrap_err();
        assert!(
            err.message.contains("not an object"),
            "expected 'not an object' in error; got: {}",
            err.message
        );
    }

    #[test]
    fn json_top_level_scalar_is_error() {
        let (_dir, path) = write_temp(".json", b"\"hello\"");
        let err = load_data_context(&path).unwrap_err();
        assert!(
            err.message.contains("not a JSON object or array"),
            "expected 'not a JSON object or array' in error; got: {}",
            err.message
        );
    }

    // ── CSV ───────────────────────────────────────────────────────────────

    #[test]
    fn csv_header_and_first_row() {
        let (_dir, path) = write_temp(".csv", b"name,city\nAlice,Wonderland\nBob,Nowhere");
        let ctx = load_data_context(&path).unwrap();
        assert_eq!(ctx.get("name"), Some("Alice"));
        assert_eq!(ctx.get("city"), Some("Wonderland"));
    }

    #[test]
    fn csv_no_data_rows_is_error() {
        let (_dir, path) = write_temp(".csv", b"name,city\n");
        let err = load_data_context(&path).unwrap_err();
        assert!(
            err.message.contains("no data rows"),
            "expected 'no data rows' in error; got: {}",
            err.message
        );
    }

    // ── Unknown extension ─────────────────────────────────────────────────

    #[test]
    fn unknown_extension_is_error() {
        let (_dir, path) = write_temp(".toml", b"key = \"val\"");
        let err = load_data_context(&path).unwrap_err();
        assert!(
            err.message.contains("unsupported file extension"),
            "expected 'unsupported file extension' in error; got: {}",
            err.message
        );
    }

    // ── BTreeMap determinism ──────────────────────────────────────────────

    #[test]
    fn json_fields_are_sorted() {
        let (_dir, path) = write_temp(".json", br#"{"z": "last", "a": "first", "m": "middle"}"#);
        let ctx = load_data_context(&path).unwrap();
        let keys: Vec<&str> = ctx.fields.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["a", "m", "z"]);
    }
}
