//! Integration tests for the `zenith mcp` JSON-RPC server.
//!
//! Store-free tools are driven through the pure `handle_message` entry point.
//! Tools that touch the content-addressed session store (render, workspace,
//! resources) are driven through a real `zenith mcp` subprocess with
//! `ZENITH_DATA_DIR` pointed at a tempdir — `std::env::set_var` is unavailable
//! (the workspace forbids `unsafe`), and a subprocess also exercises the stdio
//! transport end to end.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use zenith_cli::mcp::handle_message;

/// A minimal, valid .zen document: two pages, one rect for structure tests.
const DOC: &str = r##"zenith version=1 {
  project id="proj.t" name="T"
  tokens format="zenith-token-v1" {
    token id="color.bg" type="color" value="#f8fafc"
    token id="color.fg" type="color" value="#111111"
  }
  document id="doc.t" title="T" {
    page id="page.a" w=(px)100 h=(px)100 background=(token)"color.bg" {
      rect id="r.1" x=(px)10 y=(px)10 w=(px)50 h=(px)50 fill=(token)"color.fg"
    }
    page id="page.b" w=(px)100 h=(px)100 background=(token)"color.bg" {
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

/// The structured result of a `tools/call` response.
fn structured(resp: &Value) -> &Value {
    &resp["result"]["structuredContent"]
}

// ── Protocol ──────────────────────────────────────────────────────────────

#[test]
fn initialize_advertises_tools_and_resources() {
    let resp = call(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {} }
    }));
    let result = &resp["result"];
    assert_eq!(result["serverInfo"]["name"], "zenith");
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    // Positioning: prefer the CLI when usable, MCP for CLI-unsuitable environments;
    // and the doc-id workflow is steered.
    let instructions = result["instructions"].as_str().unwrap_or("");
    assert!(instructions.to_lowercase().contains("prefer it"));
    assert!(instructions.contains("doc-id"));
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
fn tools_list_is_the_small_stable_surface() {
    let resp = call(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }));
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 13, "expected 13 top-level tools");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "zenith_schema",
        "zenith_validate",
        "zenith_inspect",
        "zenith_tx",
        "zenith_render",
        "zenith_workspace_scratch",
        "zenith_workspace_promote",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
    // Progressive disclosure: no per-node / per-op schema leaks as its own tool.
    assert!(
        !names.iter().any(|n| n.starts_with("zenith_schema_")),
        "schema detail must live behind the single zenith_schema tool"
    );
    assert!(tools.iter().all(|t| t["inputSchema"].is_object()));
}

// ── tools/call: errors ────────────────────────────────────────────────────

#[test]
fn missing_argument_is_tool_error_not_protocol_error() {
    let resp = tool_call("zenith_validate", json!({}));
    assert!(resp.get("error").is_none(), "should be a tool result");
    assert_eq!(resp["result"]["isError"], true);
    assert!(
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("doc")
    );
}

#[test]
fn unknown_tool_is_tool_error() {
    let resp = tool_call("zenith_bogus", json!({}));
    assert_eq!(resp["result"]["isError"], true);
}

// ── tools/call: trimmed structured results ────────────────────────────────

#[test]
fn validate_returns_trimmed_counts() {
    let dir = write_doc();
    let path = dir.path().join("d.zen");
    let resp = tool_call("zenith_validate", json!({ "doc": path.to_str().unwrap() }));
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let s = structured(&resp);
    assert_eq!(s["valid"], true);
    assert_eq!(s["error_count"], 0);
    assert_eq!(s["diagnostics"].as_array().map(|a| a.len()), Some(0));
    // The text mirror is the same compact object.
    assert!(
        resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("\"valid\":true")
    );
}

#[test]
fn inspect_is_shallow_by_default_and_expands_with_detail() {
    let dir = write_doc();
    let path = dir.path().join("d.zen");
    let doc = path.to_str().unwrap();

    let resp = tool_call("zenith_inspect", json!({ "doc": doc }));
    let s = structured(&resp);
    let first = &s["pages"][0];
    assert_eq!(first["id"], "page.a");
    // depth 1 → direct children expanded; rect has no geometry without detail.
    assert_eq!(first["children"][0]["id"], "r.1");
    assert_eq!(first["children"][0]["kind"], "rect");
    assert!(first["children"][0]["geometry"].is_null());

    let detailed = tool_call("zenith_inspect", json!({ "doc": doc, "detail": true }));
    let g = &structured(&detailed)["pages"][0]["children"][0]["geometry"];
    assert!(g.is_object(), "detail must include geometry: {detailed}");
}

#[test]
fn tokens_returns_resolved_palette() {
    let dir = write_doc();
    let path = dir.path().join("d.zen");
    let resp = tool_call("zenith_tokens", json!({ "doc": path.to_str().unwrap() }));
    let s = structured(&resp);
    assert_eq!(s["error_count"], 0);
    assert_eq!(s["tokens"].as_array().map(|a| a.len()), Some(2));
    // Trimmed by default: no diagnostics array unless asked.
    assert!(s["diagnostics"].is_null());
}

#[test]
fn schema_op_returns_fields_and_example_on_demand() {
    let resp = tool_call(
        "zenith_schema",
        json!({ "surface": "op", "name": "set_fill" }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let op = &structured(&resp)["op"];
    assert_eq!(op["op"], "set_fill");
    assert!(op["fields"].is_array());
    assert!(op["example"].is_string());
}

#[test]
fn schema_unknown_node_is_tool_error() {
    let resp = tool_call(
        "zenith_schema",
        json!({ "surface": "node", "name": "nope" }),
    );
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn tx_dry_run_returns_status_without_source() {
    let dir = write_doc();
    let path = dir.path().join("d.zen");
    let tx = json!({ "ops": [ { "op": "set_fill", "node": "r.1", "fill": "color.bg" } ] });
    let resp = tool_call(
        "zenith_tx",
        json!({ "doc": path.to_str().unwrap(), "transaction": tx }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let s = structured(&resp);
    assert!(s["status"].is_string());
    assert!(s["affected"].is_array());
    // Dry-run must not echo the full source.
    assert!(s.get("after_source").is_none());
}

#[test]
fn theme_new_returns_source() {
    let resp = tool_call(
        "zenith_theme_new",
        json!({ "name": "acme", "scheme": "light", "primary": "#3b5bdb" }),
    );
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    assert!(
        structured(&resp)["source"]
            .as_str()
            .unwrap_or("")
            .contains("token")
    );
}

// ── Store-backed: render → resource round-trip (subprocess) ────────────────

/// Drive a `zenith mcp` subprocess with `ZENITH_DATA_DIR` set, returning one
/// parsed JSON response per non-empty output line.
fn mcp_session(data_dir: &Path, requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zenith"))
        .arg("mcp")
        .env("ZENITH_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn zenith mcp");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for r in requests {
            writeln!(stdin, "{r}").expect("write request");
        }
    } // drop stdin → EOF → server loop ends
    let output = child.wait_with_output().expect("wait");
    String::from_utf8(output.stdout)
        .expect("utf8 stdout")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("parse response"))
        .collect()
}

fn req(id: u64, name: &str, args: Value) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": args }
    })
}

#[test]
fn render_returns_resource_link_that_reads_back() {
    let doc_dir = tempfile::tempdir().expect("doc dir");
    let store = tempfile::tempdir().expect("store dir");
    let path = doc_dir.path().join("d.zen");
    std::fs::write(&path, DOC).expect("write doc");
    let doc = path.to_str().unwrap();

    // Phase 1: render and capture the resource URI.
    let r1 = mcp_session(
        store.path(),
        &[req(
            1,
            "zenith_render",
            json!({ "doc": doc, "format": "png" }),
        )],
    );
    let result = &r1[0]["result"];
    assert_eq!(result["isError"], false, "{:?}", r1);
    let s = &result["structuredContent"];
    assert_eq!(s["format"], "png");
    assert_eq!(s["blocked"], false);
    let uri = s["resource"]["uri"].as_str().expect("resource uri");
    assert!(uri.starts_with("zenith://doc/"), "uri: {uri}");
    assert_eq!(s["resource"]["mimeType"], "image/png");

    // Phase 2: a fresh process reads the resource from the shared store.
    let r2 = mcp_session(
        store.path(),
        &[
            json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": { "uri": uri } }),
        ],
    );
    let content = &r2[0]["result"]["contents"][0];
    assert_eq!(content["mimeType"], "image/png");
    assert!(
        !content["blob"].as_str().unwrap_or("").is_empty(),
        "blob should be non-empty base64"
    );
}

// ── Store-backed: workspace loop + doc-id addressing (subprocess) ──────────

#[test]
fn workspace_loop_and_doc_id_addressing() {
    let doc_dir = tempfile::tempdir().expect("doc dir");
    let store = tempfile::tempdir().expect("store dir");
    let path = doc_dir.path().join("d.zen");
    std::fs::write(&path, DOC).expect("write doc");
    let doc = path.to_str().unwrap();

    let resp = mcp_session(
        store.path(),
        &[
            req(
                1,
                "zenith_workspace_scratch",
                json!({ "doc": doc, "op": "new", "page": "page.a", "status": "draft" }),
            ),
            req(
                2,
                "zenith_workspace_scratch",
                json!({ "doc": doc, "op": "list" }),
            ),
            req(
                3,
                "zenith_workspace_candidate",
                json!({ "doc": doc, "candidate_id": "cand0", "status": "selected" }),
            ),
            req(
                4,
                "zenith_workspace_promote",
                json!({ "doc": doc, "candidate_id": "cand0", "target_page": "page.b" }),
            ),
            req(
                5,
                "zenith_workspace_finalize",
                json!({ "doc": doc, "op": "finalize" }),
            ),
        ],
    );
    assert_eq!(resp.len(), 5, "expected five responses: {resp:?}");
    for (i, r) in resp.iter().enumerate() {
        assert_eq!(r["result"]["isError"], false, "step {} failed: {r}", i + 1);
    }
    assert_eq!(
        resp[0]["result"]["structuredContent"]["candidate_id"],
        "cand0"
    );
    let candidates = resp[1]["result"]["structuredContent"]["candidates"]
        .as_array()
        .expect("candidates array");
    assert_eq!(candidates.len(), 1);

    // The doc now carries a stamped doc-id: address it by id in a fresh process.
    let stamped = std::fs::read_to_string(&path).expect("reread doc");
    let doc_id = stamped
        .split("doc-id=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("doc-id present after attach");
    let by_id = mcp_session(
        store.path(),
        &[req(9, "zenith_validate", json!({ "doc": doc_id }))],
    );
    assert_eq!(
        by_id[0]["result"]["isError"], false,
        "validate by doc-id: {by_id:?}"
    );
    assert_eq!(by_id[0]["result"]["structuredContent"]["valid"], true);
}

// ── HTTP transport (only with `--features http`) ───────────────────────────

/// POST one JSON-RPC message to the HTTP transport and return the parsed reply.
#[cfg(feature = "http")]
fn http_post(addr: &str, message: &Value) -> Value {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let body = message.to_string();
    let mut stream = TcpStream::connect(addr).expect("connect");
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let payload = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or("");
    serde_json::from_str(payload).expect("parse http body")
}

#[cfg(feature = "http")]
#[test]
fn http_transport_matches_stdio() {
    use std::net::TcpListener;

    // Reserve a free port, then hand it to the server.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("probe")
        .local_addr()
        .expect("addr")
        .port();
    let addr = format!("127.0.0.1:{port}");

    let mut child = Command::new(env!("CARGO_BIN_EXE_zenith"))
        .args(["mcp", "--http", &addr])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn http server");

    // Poll until the listener is accepting connections.
    let mut connected = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect(&addr).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(connected, "http server never came up");

    let init = http_post(
        &addr,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "zenith");

    let list = http_post(
        &addr,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    );
    assert_eq!(
        list["result"]["tools"].as_array().map(|a| a.len()),
        Some(13),
        "http tools/list must match stdio"
    );

    let _ = child.kill();
    let _ = child.wait();
}
