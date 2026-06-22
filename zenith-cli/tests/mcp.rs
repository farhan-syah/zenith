//! Integration tests for the `zenith mcp` JSON-RPC server, driven through the
//! pure `handle_message` entry point (no stdio needed).

use serde_json::{Value, json};
use zenith_cli::mcp::handle_message;

/// A minimal, valid .zen document (tokens + one empty page) for tool tests.
const DOC: &str = r##"zenith version=1 {
  project id="proj.t" name="T"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
  }
  styles {
  }
  document id="doc.t" title="T" {
    page id="page.t" w=(px)100 h=(px)100 background=(token)"color.bg" {
    }
  }
}
"##;

fn call(line: Value) -> Value {
    handle_message(&line.to_string()).expect("request should produce a response")
}

fn tool_call(name: &str, args: Value) -> Value {
    call(json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    }))
}

fn write_doc() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("d.zen"), DOC).expect("write doc");
    dir
}

// ── Protocol ──────────────────────────────────────────────────────────────

#[test]
fn initialize_reports_server_and_echoes_protocol() {
    let resp = call(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {} }
    }));
    let result = &resp["result"];
    assert_eq!(result["serverInfo"]["name"], "zenith");
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(
        result["instructions"]
            .as_str()
            .unwrap_or("")
            .contains("local")
    );
}

#[test]
fn notification_gets_no_response() {
    assert!(handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
}

#[test]
fn parse_error_is_reported() {
    let resp = handle_message("not json").expect("parse error still responds");
    assert_eq!(resp["error"]["code"], -32700);
}

#[test]
fn unknown_method_is_method_not_found() {
    let resp = call(json!({ "jsonrpc": "2.0", "id": 7, "method": "frobnicate" }));
    assert_eq!(resp["error"]["code"], -32601);
    assert_eq!(resp["id"], 7);
}

#[test]
fn ping_returns_empty_result() {
    let resp = call(json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }));
    assert_eq!(resp["result"], json!({}));
}

// ── tools/list ────────────────────────────────────────────────────────────

#[test]
fn tools_list_advertises_the_surface() {
    let resp = call(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }));
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 8);
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "zenith_validate",
        "zenith_render",
        "zenith_tx",
        "zenith_theme_new",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    // Every tool carries an inputSchema object.
    assert!(tools.iter().all(|t| t["inputSchema"].is_object()));
}

// ── tools/call ──────────────────────────────────────────────────────────────

#[test]
fn validate_tool_runs() {
    let dir = write_doc();
    let path = dir.path().join("d.zen");
    let resp = tool_call("zenith_validate", json!({ "path": path.to_str().unwrap() }));
    assert_eq!(resp["result"]["isError"], false);
    assert!(resp["result"]["content"][0]["text"].is_string());
}

#[test]
fn missing_argument_is_tool_error_not_protocol_error() {
    let resp = tool_call("zenith_validate", json!({}));
    assert!(resp.get("error").is_none(), "should be a tool result");
    assert_eq!(resp["result"]["isError"], true);
    assert!(
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("path")
    );
}

#[test]
fn unknown_tool_is_tool_error() {
    let resp = tool_call("zenith_bogus", json!({}));
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn render_tool_writes_a_file() {
    let dir = write_doc();
    let path = dir.path().join("d.zen");
    let out = dir.path().join("out.png");
    let resp = tool_call(
        "zenith_render",
        json!({ "path": path.to_str().unwrap(), "format": "png", "out": out.to_str().unwrap() }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    assert!(out.is_file(), "render should have written the PNG");
}

#[test]
fn theme_new_tool_returns_source() {
    let resp = tool_call(
        "zenith_theme_new",
        json!({ "name": "acme", "scheme": "light", "primary": "#3b5bdb" }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("token"), "theme source should contain tokens");
}
