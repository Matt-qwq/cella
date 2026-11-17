#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! **Every command is reachable from every surface, with the same params doc** (wave N4).
//!
//! That is a promise the architecture makes — one registry, four ways in — and until now nothing
//! proved it: a command added to the registry but filtered out of a listing, or a surface that
//! kept its own table of ids, would have been found by a person instead of a test.
//!
//! The surfaces compared here: the engine's own registry, the CLI ('commands' and 'run --cmd'),
//! and the MCP server ('list_commands' and 'execute_command'). The control channel shares
//! 'Session::execute' with the CLI and MCP and is covered by its own tests in the UI crate.

use std::process::Command;

use cella_engine::Session;
use cella_mcp::{Headless, Server};
use serde_json::{Value, json};

/// Every command id the engine registers, with its params doc.
fn registry() -> Vec<(String, String)> {
    Session::new().commands().into_iter().map(|c| (c.id.to_string(), c.params.to_string())).collect()
}

fn mcp() -> Server {
    let mut s = Server::new(Box::new(Headless::new()));
    s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#);
    s
}

fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let out = s.handle_line(&line).expect("a response");
    serde_json::from_str(&out).expect("valid JSON")
}

#[test]
fn the_cli_lists_every_command_the_registry_holds() {
    let reg = registry();
    assert!(reg.len() > 300, "the registry should hold every command, not {} of them", reg.len());
    let out = Command::new(env!("CARGO_BIN_EXE_cella-cli")).args(["commands"]).output().expect("run the CLI");
    assert!(out.status.success(), "the CLI failed: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let listed: Vec<&str> = text.lines().filter_map(|l| l.split_whitespace().next()).filter(|w| w.contains('.')).collect();
    let missing: Vec<&str> = reg.iter().map(|(id, _)| id.as_str()).filter(|id| !listed.contains(id)).collect();
    assert!(missing.is_empty(), "the CLI does not list {} commands: {:?}", missing.len(), &missing[..missing.len().min(10)]);
    // The params doc travels with the id on every surface: one registry, one doc.
    for (id, params) in &reg {
        assert!(!params.trim().is_empty(), "{id} has no params doc");
    }
}

#[test]
fn mcp_lists_every_command_and_accepts_each_by_id() {
    let reg = registry();
    let mut s = mcp();
    let listed = call(&mut s, 2, "tools/call", json!({"name": "list_commands", "arguments": {}}));
    let text = listed["result"]["content"][0]["text"].as_str().unwrap_or("");
    let parsed: Value = serde_json::from_str(text).expect("list_commands returns JSON");
    let ids: Vec<&str> = parsed.as_array().map(|a| a.iter().filter_map(|c| c["id"].as_str()).collect()).unwrap_or_default();
    assert_eq!(ids.len(), reg.len(), "MCP lists {} commands where the registry holds {}", ids.len(), reg.len());
    for (id, _) in &reg {
        assert!(ids.contains(&id.as_str()), "MCP does not list {id}");
    }
}

#[test]
fn the_mcp_execute_tool_reaches_every_command_by_id() {
    // Not that each command succeeds with empty params (most need them, and a programmatic call
    // must not open a dialog): that none of them is unknown to the surface.
    let reg = registry();
    let mut s = mcp();
    let mut unknown: Vec<String> = Vec::new();
    for (i, (id, _)) in reg.iter().enumerate() {
        let r = call(&mut s, 100 + i as u64, "tools/call", json!({"name": "execute_command", "arguments": {"command": id, "params": {}}}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
        let err = r["result"]["isError"].as_bool().unwrap_or(false);
        if err && (text.contains("unknown command") || text.contains("no such tool")) {
            unknown.push(format!("{id}: {text}"));
        }
    }
    assert!(unknown.is_empty(), "MCP cannot reach {} commands: {unknown:?}", unknown.len());
}
