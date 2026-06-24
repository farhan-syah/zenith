//! `zenith mcp` — a token-efficient MCP server over stdio.
//!
//! Speaks JSON-RPC 2.0 line-delimited on stdin/stdout (the MCP stdio transport)
//! and exposes the `zenith` command surface as MCP tools. It is hand-rolled on
//! `serde_json` (already a dependency) rather than pulling an async MCP SDK, so
//! the binary stays small.
//!
//! Design (not a thin CLI wrapper):
//! - every tool returns a **trimmed structured object** (`structuredContent` plus
//!   a compact-JSON text mirror), never raw human stdout;
//! - all node/op/surface schema detail is fetched on demand via the single
//!   `zenith_schema` meta-tool (progressive disclosure);
//! - large or binary artifacts are returned as `resources` links backed by the
//!   content-addressed session store, never inlined;
//! - documents are addressable by `doc-id`, and the scratch/candidate/promote/
//!   finalize workspace loop is drivable end-to-end.
//!
//! Logs go to stderr; stdout is reserved for the JSON-RPC framing.

mod base64;
mod doc_ref;
mod exec;
#[cfg(feature = "http")]
mod http;
mod protocol;
mod resources;
mod serialize;
mod tools;

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use protocol::{error, success};

/// The MCP protocol revision this server defaults to when the client does not
/// request one.
const DEFAULT_PROTOCOL: &str = "2025-06-18";

/// Steering shown to clients on connect.
const INSTRUCTIONS: &str = "Zenith authors, validates, and renders deterministic .zen design \
documents. If your environment can run the local `zenith` CLI, prefer it (install the binary and \
the skill, then call commands directly) — this MCP server is for environments where a local binary \
is not suitable (remote, CI, sandboxed, hosted agents). It is a first-class surface: results are \
trimmed, schema detail is on demand, and large artifacts come back as resource links. Address a \
document by its path or its doc-id (returned once identity is attached). Typical loop: zenith_schema \
(learn node/op shapes on demand) → zenith_tx to edit → zenith_validate (hard Error diagnostics block \
rendering) → zenith_render (returns a resource link; read it via resources/read). Keep design \
iterations as scratch candidates and promote the chosen one into the export page rather than editing \
the deliverable directly. Results are trimmed by default; opt into detail with the documented params.";

/// Run the stdio MCP server until stdin closes. Always returns success.
pub fn run() -> u8 {
    let stdin = io::stdin();
    let mut out = io::stdout();
    let reader = stdin.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("zenith mcp: stdin read error: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(&line) {
            if writeln!(out, "{response}").is_err() {
                break;
            }
            let _ = out.flush();
        }
    }
    0
}

/// Handle one JSON-RPC message. Returns `Some(response)` for requests and
/// `None` for notifications (and for messages that need no reply).
///
/// Exposed for integration tests so the protocol can be driven without stdio.
pub fn handle_message(line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };

    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    // Notifications (no id) get no response.
    let id = id?;

    match method {
        "initialize" => Some(success(id, initialize_result(&params))),
        "ping" => Some(success(id, json!({}))),
        "tools/list" => Some(success(id, tools::list_payload())),
        "tools/call" => Some(tools_call(id, &params)),
        "resources/list" => Some(success(id, resources::list_payload())),
        "resources/read" => Some(resources_read(id, &params)),
        other => Some(error(id, -32601, &format!("method not found: {other}"))),
    }
}

/// Build the `initialize` result, echoing the client's protocol version when valid.
fn initialize_result(params: &Value) -> Value {
    let protocol = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL);
    json!({
        "protocolVersion": protocol,
        "capabilities": {
            "tools": {},
            "resources": { "listChanged": false },
        },
        "serverInfo": { "name": "zenith", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

/// Execute a `tools/call`. Tool-execution failures are reported inside the
/// result (`isError: true`), per the MCP spec — only malformed requests are
/// JSON-RPC errors.
fn tools_call(id: Value, params: &Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error(id, -32602, "missing tool name");
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    success(id, exec::call(name, &args).into_payload())
}

/// Serve the MCP protocol over native Streamable-HTTP at `addr`. Requires the
/// `http` Cargo feature; both transports drive [`handle_message`].
#[cfg(feature = "http")]
pub fn run_http(addr: &str) -> u8 {
    http::serve(addr)
}

/// Fallback when the binary was built without the `http` feature: report and fail.
#[cfg(not(feature = "http"))]
pub fn run_http(_addr: &str) -> u8 {
    eprintln!(
        "zenith mcp: this binary was built without the `http` feature; rebuild with \
         `--features http` to use --http, or use the default stdio transport."
    );
    1
}

/// Execute a `resources/read`. A missing/unknown URI is a JSON-RPC error.
fn resources_read(id: Value, params: &Value) -> Value {
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return error(id, -32602, "missing resource uri");
    };
    match resources::read_payload(uri) {
        Ok(result) => success(id, result),
        Err(message) => error(id, -32002, &message),
    }
}
