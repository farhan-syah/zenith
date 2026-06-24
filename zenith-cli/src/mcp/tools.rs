//! The MCP tool catalog: 13 tools with token-lean JSON-Schema inputs.
//!
//! The surface is deliberately small and stable (clients cache `tools/list`
//! once). All node/op/surface schema detail lives behind the single
//! `zenith_schema` meta-tool — agents fetch one node kind or one tx op on demand
//! instead of carrying every schema in context. Every other tool returns a
//! trimmed structured result and expands only on opt-in params.
//!
//! Every tool accepts `doc` as either a filesystem path or a 26-char `doc-id`
//! (see [`super::doc_ref`]), so an agent can stop passing paths after the first
//! call.

use serde_json::{Value, json};

/// A single MCP tool definition.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// The `doc` argument schema fragment shared by every document-scoped tool.
fn doc_arg() -> Value {
    json!({ "type": "string", "description": "Document path or 26-char doc-id." })
}

/// The full tool catalog.
pub fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "zenith_schema",
            description: "Look up the document schema ON DEMAND: node kinds, a node's attributes, \
tx ops, or one op's fields + a ready-to-edit JSON example. Call this before building a zenith_tx \
instead of guessing.",
            schema: json!({
                "type": "object",
                "properties": {
                    "surface": {
                        "type": "string",
                        "enum": ["overview", "nodes", "node", "ops", "op", "page", "asset", "document", "diagnostics"],
                        "description": "Which schema surface to fetch."
                    },
                    "name": { "type": "string", "description": "Node kind (for surface=node) or op name (for surface=op)." }
                },
                "required": ["surface"]
            }),
        },
        Tool {
            name: "zenith_validate",
            description: "Validate a .zen document. Returns counts + the blocking errors only; \
hard (Error) diagnostics block rendering — fix them first. Set severity to also see warnings/advisories.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "severity": {
                        "type": "string",
                        "enum": ["error", "warning", "advisory"],
                        "description": "Lowest severity to include in the diagnostics array (default: error)."
                    }
                },
                "required": ["doc"]
            }),
        },
        Tool {
            name: "zenith_inspect",
            description: "Discover node ids and structure (read-only). Returns a shallow tree by \
default (deeper levels collapse to child_count). Use node/depth to drill in, detail for geometry. \
Large trees are returned as a resource link.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "node": { "type": "string", "description": "Only the subtree at this node id." },
                    "depth": { "type": "integer", "minimum": 0, "description": "Levels to expand (default 1)." },
                    "detail": { "type": "boolean", "description": "Include geometry/visible/locked per node." }
                },
                "required": ["doc"]
            }),
        },
        Tool {
            name: "zenith_tokens",
            description: "List every design token and its resolved value. Visual properties must \
reference tokens, so this reveals the palette/type/spacing a document exposes.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "diagnostics": { "type": "boolean", "description": "Include token diagnostics (default false)." }
                },
                "required": ["doc"]
            }),
        },
        Tool {
            name: "zenith_tx",
            description: "Apply a typed transaction (JSON edit script) to a document. Dry-run by \
default (returns status + affected ids); set apply=true to write. Set diff=true to also get the \
resulting (after) source as a resource link. Enforces id-uniqueness and referential integrity. \
Use zenith_schema op to learn an op's shape.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "transaction": {
                        "description": "The transaction as a JSON object, array, or string.",
                        "type": ["object", "string", "array"]
                    },
                    "apply": { "type": "boolean", "description": "Write the result to disk." },
                    "diff": { "type": "boolean", "description": "Return a resource link to the before→after diff." }
                },
                "required": ["doc", "transaction"]
            }),
        },
        Tool {
            name: "zenith_render",
            description: "Render a document deterministically to png, pdf, or scene (display-list \
JSON). Returns a resource link to the artifact (never inlines bytes); blocked by hard diagnostics \
— validate first. Pass out to also write the file to a path you choose.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "format": { "type": "string", "enum": ["png", "pdf", "scene"] },
                    "page": { "type": "integer", "minimum": 1, "description": "1-based page (default 1)." },
                    "locked": { "type": "boolean", "description": "Verify asset sha256 and fail on mismatch." },
                    "out": { "type": "string", "description": "Optional path to also write the artifact to." },
                    "diagnostics": { "type": "boolean", "description": "Include soft diagnostics (default false)." }
                },
                "required": ["doc", "format"]
            }),
        },
        Tool {
            name: "zenith_fmt",
            description: "Canonicalize a document in place (idempotent). Returns whether the file \
changed and its content hash.",
            schema: json!({
                "type": "object",
                "properties": { "doc": doc_arg() },
                "required": ["doc"]
            }),
        },
        Tool {
            name: "zenith_merge",
            description: "Mail-merge a .zen template with a CSV, writing one PNG per row. Mark \
variable nodes with role=\"data.<column>\". Use for localized/personalized/batch variants.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "data": { "type": "string", "description": "CSV data file path." },
                    "out_dir": { "type": "string", "description": "Directory for the output PNGs." },
                    "name_by": { "type": "string", "description": "CSV column to name files by." },
                    "manifest": { "type": "string", "description": "Write a reproducibility manifest here." }
                },
                "required": ["doc", "data", "out_dir"]
            }),
        },
        Tool {
            name: "zenith_theme_new",
            description: "Synthesize a complete token-only theme pack (.zen) from brand colours, \
with APCA-correct content pairings for WCAG 3 contrast. Returns the generated source; pass out to \
write it to a file instead.",
            schema: theme_schema(),
        },
        Tool {
            name: "zenith_workspace_scratch",
            description: "Manage scratch candidates — point-in-time .zen snapshots that keep design \
iteration out of the deliverable file. op=new snapshots the current doc; op=list/show enumerate. \
Each candidate is addressable as a resource.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "op": { "type": "string", "enum": ["new", "list", "show"] },
                    "page": { "type": "string", "description": "Page id this candidate captures (default whole doc)." },
                    "candidate_id": { "type": "string", "description": "Candidate id (for op=show)." },
                    "status": { "type": "string", "enum": ["draft", "selected", "rejected"], "description": "Initial status for op=new." },
                    "notes": { "type": "string" },
                    "workspace_role": { "type": "string", "description": "e.g. hero, fallback." },
                    "promotion_target": { "type": "string" },
                    "cleanup_policy": { "type": "string", "description": "e.g. delete." }
                },
                "required": ["doc", "op"]
            }),
        },
        Tool {
            name: "zenith_workspace_candidate",
            description: "Transition a scratch candidate's lifecycle: draft → selected | rejected. \
Only a selected candidate can be promoted.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "candidate_id": { "type": "string" },
                    "status": { "type": "string", "enum": ["draft", "selected", "rejected"] }
                },
                "required": ["doc", "candidate_id", "status"]
            }),
        },
        Tool {
            name: "zenith_workspace_promote",
            description: "Promote a selected candidate's page into a target page of the deliverable \
document (deep-copies, suffixes ids, validates, writes in place).",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "candidate_id": { "type": "string" },
                    "target_page": { "type": "string", "description": "Destination page id." },
                    "id_suffix": { "type": "string", "description": "Suffix appended to cloned ids (default .promoted)." }
                },
                "required": ["doc", "candidate_id", "target_page"]
            }),
        },
        Tool {
            name: "zenith_workspace_finalize",
            description: "op=finalize cleans up rejected candidates per cleanup-policy. op=bundle \
packs the doc's whole session store into a portable .zenithbundle (returns a resource link). \
op=unbundle restores one from a bundle path. Read any preview resources BEFORE finalizing.",
            schema: json!({
                "type": "object",
                "properties": {
                    "doc": doc_arg(),
                    "op": { "type": "string", "enum": ["finalize", "bundle", "unbundle"] },
                    "bundle": { "type": "string", "description": "Bundle file path (out for op=bundle, in for op=unbundle)." }
                },
                "required": ["op"]
            }),
        },
    ]
}

/// The (necessarily larger) input schema for `zenith_theme_new`.
fn theme_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "scheme": { "type": "string", "enum": ["light", "dark"] },
            "primary": { "type": "string", "description": "#rrggbb" },
            "secondary": { "type": "string" },
            "accent": { "type": "string" },
            "neutral": { "type": "string" },
            "info": { "type": "string" },
            "success": { "type": "string" },
            "warning": { "type": "string" },
            "error": { "type": "string" },
            "radius_box": { "type": "number" },
            "radius_field": { "type": "number" },
            "radius_selector": { "type": "number" },
            "border": { "type": "number" },
            "depth": { "type": "boolean" },
            "noise": { "type": "boolean" },
            "out": { "type": "string", "description": "Write the theme here instead of returning the source." }
        },
        "required": ["name", "scheme", "primary"]
    })
}

/// Render the catalog as the `tools/list` result payload.
pub fn list_payload() -> Value {
    let tools: Vec<Value> = catalog()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.schema,
            })
        })
        .collect();
    json!({ "tools": tools })
}
