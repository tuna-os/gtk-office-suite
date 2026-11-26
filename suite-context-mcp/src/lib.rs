// SPDX-License-Identifier: GPL-3.0-or-later
//! Prototype for #1786: a local, read-only MCP server over stdio that hands an
//! external agent the context of ONE document the user chose.
//!
//! Boundaries (see `docs/rfc/0006-mcp-context-adapter.md`):
//!
//! - Disabled by default ([`Config::default`]); [`serve`] refuses to read a byte
//!   while disabled. No listener, no network, no discovery: the only input is
//!   the stdio pair the app hands in.
//! - The adapter never opens a document file. Content comes from the app's live
//!   model through [`ContextSource`], so the agent sees what the GUI shows
//!   (unsaved edits included) and nothing can bypass undo.
//! - The agent names no path. A [`Grant`] is made by the user's own action from
//!   a path the app already holds; it refuses symlinks and non-files and is
//!   re-checked on every call (revoked, replaced, swapped for a link, or no
//!   longer the open document all fail closed).
//! - Every answer carries a revision; a caller that passes `expected_revision`
//!   gets `stale_revision` instead of a mismatched snapshot.
//! - Text and table sizes are capped; a cut is reported, never silent.
//! - Only read tools exist. Any other `tools/call` is an unknown tool.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Opt-in switch and size limits. `Default` is disabled.
#[derive(Clone, Debug)]
pub struct Config {
    pub enabled: bool,
    pub max_request_bytes: usize,
    pub max_text_bytes: usize,
    pub max_table_cells: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: false, max_request_bytes: 64 * 1024, max_text_bytes: 256 * 1024, max_table_cells: 10_000 }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum GrantError {
    NotFound,
    Symlink,
    NotRegularFile,
}

/// The user's explicit choice of one document. Cloning shares the revocation flag.
#[derive(Clone, Debug)]
pub struct Grant {
    path: PathBuf,
    revoked: Arc<AtomicBool>,
}

impl Grant {
    /// `path` must be a regular file, not a symlink and not reached through one.
    pub fn new(path: &Path) -> Result<Grant, GrantError> {
        let meta = std::fs::symlink_metadata(path).map_err(|_| GrantError::NotFound)?;
        if meta.file_type().is_symlink() {
            return Err(GrantError::Symlink);
        }
        if !meta.is_file() {
            return Err(GrantError::NotRegularFile);
        }
        let canon = std::fs::canonicalize(path).map_err(|_| GrantError::NotFound)?;
        // Any symlink on the way (not just the last component) makes the
        // canonical path differ from the lexically normalised absolute one.
        // `..` is folded lexically, so `sub/../f` still names the same file.
        let abs = std::path::absolute(path).map_err(|_| GrantError::NotFound)?;
        let mut lexical = PathBuf::new();
        for c in abs.components() {
            match c {
                std::path::Component::ParentDir => {
                    lexical.pop();
                }
                std::path::Component::CurDir => {}
                other => lexical.push(other),
            }
        }
        if lexical != canon {
            return Err(GrantError::Symlink);
        }
        Ok(Grant { path: canon, revoked: Arc::new(AtomicBool::new(false)) })
    }

    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
    }

    fn check(&self, open_path: Option<&Path>) -> Result<(), &'static str> {
        if self.revoked.load(Ordering::SeqCst) {
            return Err("selection_revoked");
        }
        // Re-validate: the file may have been swapped for a link since the grant.
        match Grant::new(&self.path) {
            Ok(g) if g.path == self.path => {}
            _ => return Err("selection_invalid"),
        }
        match open_path.and_then(|p| std::fs::canonicalize(p).ok()) {
            Some(p) if p == self.path => Ok(()),
            _ => Err("document_not_open"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableSnapshot {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// What the live model reports. `revision` must change on every edit
/// (the app's undo-stack/edit counter is the natural value).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocumentContext {
    pub path: Option<PathBuf>,
    pub title: String,
    pub mime: String,
    pub revision: u64,
    pub dirty: bool,
    /// The user's selection, if any; `None` means "whole document" text below.
    pub selection: Option<String>,
    pub text: String,
    pub tables: Vec<TableSnapshot>,
}

/// Implemented by the app shell over its live document model. Read-only by type.
pub trait ContextSource {
    fn current(&self) -> Option<DocumentContext>;
}

fn cut_at_char(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    (&s[..i], true)
}

fn tools() -> Value {
    let schema = json!({"type":"object","properties":{"expected_revision":{"type":"integer"}},"additionalProperties":false});
    json!([
        {"name":"office_get_metadata","description":"Title, type, revision and dirty flag of the document the user shared.","inputSchema":schema,"annotations":{"readOnlyHint":true}},
        {"name":"office_get_context","description":"Bounded text (the selection when there is one) and table snapshot of the shared document.","inputSchema":schema,"annotations":{"readOnlyHint":true}}
    ])
}

fn tool_error(code: &str, msg: &str) -> Value {
    json!({"isError":true,"content":[{"type":"text","text":msg}],"structuredContent":{"error":code}})
}

fn call_tool(cfg: &Config, grant: &Grant, src: &dyn ContextSource, name: &str, args: &Value) -> Value {
    if name != "office_get_metadata" && name != "office_get_context" {
        return json!({"unknown":true});
    }
    let doc = src.current();
    if let Err(code) = grant.check(doc.as_ref().and_then(|d| d.path.as_deref())) {
        return tool_error(code, code);
    }
    let doc = doc.expect("checked by grant.check");
    if let Some(exp) = args.get("expected_revision").and_then(Value::as_u64) {
        if exp != doc.revision {
            return tool_error("stale_revision", &format!("document is at revision {}, not {exp}", doc.revision));
        }
    }
    let mut out = json!({"title":doc.title,"mime":doc.mime,"revision":doc.revision,"dirty":doc.dirty});
    if name == "office_get_context" {
        let body = doc.selection.as_deref().unwrap_or(&doc.text);
        let (text, cut) = cut_at_char(body, cfg.max_text_bytes);
        let mut budget = cfg.max_table_cells;
        let mut tables_cut = false;
        let mut tables = Vec::new();
        for t in &doc.tables {
            let mut rows = Vec::new();
            for r in &t.rows {
                if r.len() > budget {
                    tables_cut = true;
                    break;
                }
                budget -= r.len();
                rows.push(r.clone());
            }
            tables.push(json!({"name":t.name,"rows":rows}));
            if tables_cut {
                break;
            }
        }
        out["scope"] = json!(if doc.selection.is_some() { "selection" } else { "document" });
        out["text"] = json!(text);
        out["truncated"] = json!(cut || tables_cut);
        out["tables"] = json!(tables);
    }
    json!({"content":[{"type":"text","text":out.to_string()}],"structuredContent":out})
}

fn reply(id: &Value, result: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"result":result}).to_string()
}

fn rpc_error(id: &Value, code: i64, msg: &str) -> String {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}}).to_string()
}

/// One JSON-RPC line in, at most one line out (notifications get none).
pub fn handle_line(cfg: &Config, grant: &Grant, src: &dyn ContextSource, line: &str) -> Option<String> {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(rpc_error(&Value::Null, -32700, "parse error"));
    };
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    Some(match method {
        "initialize" => reply(&id, json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name":"gtk-office-suite-context","version":env!("CARGO_PKG_VERSION")}
        })),
        "ping" => reply(&id, json!({})),
        "tools/list" => reply(&id, json!({"tools": tools()})),
        "tools/call" => {
            let p = msg.get("params").cloned().unwrap_or(Value::Null);
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            let r = call_tool(cfg, grant, src, name, p.get("arguments").unwrap_or(&Value::Null));
            if r.get("unknown").is_some() {
                rpc_error(&id, -32602, "unknown tool")
            } else {
                reply(&id, r)
            }
        }
        _ => rpc_error(&id, -32601, "method not found"),
    })
}

#[derive(Debug)]
pub enum ServeError {
    Disabled,
    Io(std::io::Error),
}

/// Serve until EOF. Refuses before reading anything when `cfg.enabled` is false.
pub fn serve<R: BufRead, W: Write>(cfg: &Config, grant: &Grant, src: &dyn ContextSource, mut input: R, mut out: W) -> Result<(), ServeError> {
    if !cfg.enabled {
        return Err(ServeError::Disabled);
    }
    loop {
        let mut buf = Vec::new();
        // Bounded read: never buffer more than max_request_bytes + 1.
        let n = (&mut input).take(cfg.max_request_bytes as u64 + 1).read_until(b'\n', &mut buf).map_err(ServeError::Io)?;
        if n == 0 {
            return Ok(());
        }
        if buf.last() != Some(&b'\n') && buf.len() > cfg.max_request_bytes {
            // Oversized line: answer once and stop; resyncing an unbounded line is not worth it.
            let _ = writeln!(out, "{}", rpc_error(&Value::Null, -32600, "request too large"));
            return Ok(());
        }
        let line = String::from_utf8_lossy(&buf);
        if let Some(r) = handle_line(cfg, grant, src, line.trim()) {
            writeln!(out, "{r}").map_err(ServeError::Io)?;
            out.flush().map_err(ServeError::Io)?;
        }
    }
}

#[cfg(test)]
mod tests;
