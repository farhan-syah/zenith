//! JSON-RPC 2.0 envelope helpers and the `tools/call` result shape.
//!
//! These build the wire-level success/error objects so the dispatcher in
//! `mod.rs` stays a thin routing table. Nothing here knows what any tool does.

use serde_json::{Value, json};

/// A JSON-RPC `result` response for request `id`.
pub fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A JSON-RPC `error` response for request `id`.
pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The structured outcome of a tool invocation.
///
/// Every successful tool returns a compact structured object; the text mirror
/// (`text`) is the same object serialised compactly, so clients that only read
/// `content[].text` still receive the full machine result. Failures carry a
/// human message in `text`, no structured payload, and `is_error = true`.
pub struct ToolResult {
    /// The `structuredContent` payload (omitted from the wire when `None`).
    pub structured: Option<Value>,
    /// The `content[0].text` mirror (compact JSON for success, message for error).
    pub text: String,
    /// Whether this result represents a tool-execution error.
    pub is_error: bool,
}

impl ToolResult {
    /// A successful structured result. `text` is the compact JSON mirror.
    pub fn ok(structured: Value, text: String) -> Self {
        Self {
            structured: Some(structured),
            text,
            is_error: false,
        }
    }

    /// A tool-execution error carrying a human-readable message.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            structured: None,
            text: message.into(),
            is_error: true,
        }
    }

    /// Render this result as the `tools/call` result object.
    pub fn into_payload(self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "content".into(),
            json!([{ "type": "text", "text": self.text }]),
        );
        if let Some(structured) = self.structured {
            obj.insert("structuredContent".into(), structured);
        }
        obj.insert("isError".into(), Value::Bool(self.is_error));
        Value::Object(obj)
    }
}
