//! Extensions: named command sequences a workbook carries, offered to whoever opens it.
//!
//! An extension is GridCraft's answer to an add-in **without executable code**: a list of the
//! product's own commands with their parameters, saved in the workbook, run as one. That is the
//! declarative tier of the extension model — it cannot reach the file system, the network or
//! another process, because every step is a command the engine already offers.
//!
//! The executable tier (a WASM plugin host) is deliberately absent: it needs a runtime dependency
//! (a compiler and a sandbox), which is a decision for the project rather than a crate. What is
//! here is the surface a plugin would use anyway: a named thing in the workbook, a trust flag and
//! a runner.

use serde_json::{Value as Json, json};

use super::*;

/// Where a workbook's extensions live: the stash key of the hidden names holding their JSON, like
/// scripts and custom views (`_gc_extensions_<n>`, or `_sc_extensions` in an older file; see
/// [`super::stash`]).
const EXTENSIONS_KEY: &str = "extensions";

/// Steps one extension may hold: a menu entry is a shortcut, not a program.
const MAX_STEPS: usize = 200;

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "ext.add",
            "Add Extension",
            ["Insert", "Add-ins"],
            None,
            "{name, description?, commands: [{command, params}]} -> {name} (every command id is checked before it is kept)",
            has_doc,
            add
        ),
        cmd!(noundo "ext.remove", "Remove Extension", ["Insert", "Add-ins"], None, "{name}", has_doc, remove),
        cmd!(query "ext.list", "Extensions", ["Insert", "Add-ins"], None, "{} -> [{name, description, steps, trusted}]", has_doc, list),
        cmd!(
            "ext.run",
            "Run Extension",
            ["Insert", "Add-ins"],
            None,
            "{name, trusted?: bool} -> {ran} - refuses a workbook's extensions until it is trusted, and any step that reaches outside the workbook",
            has_doc,
            run
        ),
        // Home > Add-ins: the menu entry Excel has. GridCraft's add-ins are the declarative
        // extensions above (plus MCP for anything else), so this lists them and asks the UI for the
        // dialog; nothing runs.
        cmd!(
            noundo "home.addIns",
            "Add-ins",
            ["Home", "Add-ins"],
            None,
            "{} -> the workbook's extensions (what it carries, nothing is run)",
            has_doc,
            add_ins
        ),
    ]
}

fn extensions(wb: &gridcraft_model::Workbook) -> Json {
    let v = super::extra::stash_get_shared(wb, EXTENSIONS_KEY);
    if v.is_object() { v } else { json!({}) }
}

fn set_extensions(cx: &mut Ctx<'_>, v: &Json) -> Result<()> {
    super::extra::stash_set_shared(&mut cx.wb, EXTENSIONS_KEY, v)
}

/// Whether a workbook carries any extension: part of the trust default for a file that arrived
/// with automation in it.
pub fn has_extensions(wb: &gridcraft_model::Workbook) -> bool {
    extensions(wb).as_object().is_some_and(|o| !o.is_empty())
}

fn add(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("ext.add", "missing name"))?.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(bad("ext.add", "an extension needs a name of 100 characters or fewer"));
    }
    let description = str_param(p, "description").unwrap_or("").trim().to_string();
    let commands =
        p.get("commands").and_then(Json::as_array).cloned().ok_or_else(|| bad("ext.add", "missing commands (a list of {command, params})"))?;
    if commands.is_empty() || commands.len() > MAX_STEPS {
        return Err(bad("ext.add", format!("an extension holds 1 to {MAX_STEPS} steps")));
    }
    // Every command must exist now: an extension naming a command the engine does not have is one
    // that fails at the worst possible moment instead of here.
    for (i, st) in commands.iter().enumerate() {
        let id = st.get("command").and_then(Json::as_str).unwrap_or("");
        if crate::find_command(id).is_none() {
            return Err(bad("ext.add", format!("step {} names \"{id}\", which is not a command", i + 1)));
        }
        if st.get("params").is_some_and(|v| !v.is_object()) {
            return Err(bad("ext.add", format!("step {} has params that are not an object", i + 1)));
        }
    }
    edit(s, |cx| {
        let mut all = extensions(&cx.wb);
        if let Some(o) = all.as_object_mut() {
            o.insert(name.clone(), json!({"description": description, "commands": commands}));
        }
        let v = all.clone();
        set_extensions(cx, &v)?;
        Ok(Json::Null)
    })?;
    Ok(json!({"name": name}))
}

fn remove(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("ext.remove", "missing name"))?.to_string();
    edit(s, |cx| {
        let mut all = extensions(&cx.wb);
        let existed = all.as_object_mut().and_then(|o| o.remove(&name)).is_some();
        if !existed {
            return Err(bad("ext.remove", format!("there is no extension named \"{name}\"")));
        }
        let v = all.clone();
        set_extensions(cx, &v)?;
        Ok(Json::Null)
    })?;
    Ok(json!({"removed": name}))
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let all = extensions(&d.wb);
    let trusted = d.scripts_trusted;
    Ok(Json::Array(
        all.as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| {
                        json!({
                            "name": k,
                            "description": v["description"].as_str().unwrap_or(""),
                            "steps": v["commands"].as_array().map_or(0, Vec::len),
                            "trusted": trusted,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    ))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("ext.run", "missing name"))?;
    let d = s.doc()?;
    let all = extensions(&d.wb);
    let steps = all.get(name).and_then(|v| v.get("commands")).and_then(Json::as_array).cloned().ok_or_else(|| bad("ext.run", "no such extension"))?;
    if !d.scripts_trusted && !bool_param(p, "trusted").unwrap_or(false) {
        return Err(bad(
            "ext.run",
            "extensions in this workbook came from a file and are disabled; trust the workbook first (Trust Scripts in Automate > Macros, automate.trustScripts, or trusted: true)",
        ));
    }
    let steps: Vec<Json> = steps.into_iter().take(MAX_STEPS).collect();
    // The same permission check a script step passes, and for the same reason: an extension is a
    // shortcut list, not a way around the sandbox.
    for st in &steps {
        let id = st["command"].as_str().unwrap_or("");
        if let Some(why) = super::extra::denied_script_step_shared(id, &st["params"]) {
            return Err(bad("ext.run", why));
        }
    }
    let _sandbox = crate::io::ScriptSandbox::enter();
    let mut ran = 0;
    for st in &steps {
        let id = st["command"].as_str().unwrap_or("");
        // An extension cannot call another extension or a script: one level, always, so a workbook
        // cannot build a loop (or a runaway) out of them.
        if id.starts_with("ext.") || id.starts_with("automate.") {
            return Err(bad("ext.run", format!("an extension cannot run \"{id}\"")));
        }
        s.execute(id, st["params"].clone())?;
        ran += 1;
    }
    Ok(json!({"ran": ran}))
}

/// Insert > Add-ins: what this workbook carries, and what each one would do. Nothing runs.
fn add_ins(s: &mut Session, p: &Json) -> Result<Json> {
    s.ui_requests.push(crate::UiRequest::Dialog("addIns".into(), json!({})));
    list(s, p)
}
