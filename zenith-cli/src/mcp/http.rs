//! Native Streamable-HTTP transport for the MCP server (`zenith mcp --http`).
//!
//! Gated behind the `http` Cargo feature so the default build stays
//! dependency-light and C-free. Uses `tiny_http` — a small, synchronous,
//! pure-Rust HTTP server — so this transport mirrors the stdio loop's
//! single-threaded simplicity and drives the very same [`super::handle_message`]
//! seam. No async runtime, no C dependencies.
//!
//! The client POSTs one JSON-RPC message to a single endpoint and receives the
//! JSON-RPC response as `application/json`. Notifications (no `id`) get `202
//! Accepted` with no body. SSE streaming is not used (every Zenith tool is a
//! simple request/response), so a `GET` is answered `405`.

use tiny_http::{Header, Method, Response, Server};

/// Serve the MCP protocol over HTTP at `addr` until the process is killed.
///
/// Returns a non-zero exit code only if the listener cannot be bound.
pub fn serve(addr: &str) -> u8 {
    let server = match Server::http(addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zenith mcp: cannot bind '{addr}': {e}");
            return 1;
        }
    };
    eprintln!("zenith mcp: HTTP transport listening on {addr}");

    for mut request in server.incoming_requests() {
        if *request.method() != Method::Post {
            let _ = request.respond(Response::empty(405));
            continue;
        }

        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            let _ = request.respond(Response::empty(400));
            continue;
        }

        match super::handle_message(&body) {
            Some(response) => {
                let mut http_response = Response::from_string(response.to_string());
                if let Ok(header) =
                    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                {
                    http_response.add_header(header);
                }
                let _ = request.respond(http_response);
            }
            // A notification produced no reply.
            None => {
                let _ = request.respond(Response::empty(202));
            }
        }
    }
    0
}
