//! Macros: upstream's `macro.*` ids as aliases of Cella's script commands (adoption of
//! upstream's `crates/engine/src/cmd/macros.rs`).
//!
//! There is one store and one editor here — the script step list in the defined-name stash and the
//! `automate.*` step commands — so the upstream ids are registered only where they map onto it:
//!
//! | upstream id | here |
//! | --- | --- |
//! | `macro.record` | `automate.recordActions` (the same recorder, saving a script) |
//! | `macro.run` | `automate.runScript` (the same runner and trust policy) |
//! | `macro.list` | `automate.allScripts` |
//! | `macro.get` | `automate.scriptSteps`, with the name and the trust flag |
//! | `macro.delete` | `automate.deleteScript` |
//! | `macro.rename` | `automate.renameScript` |
//! | `macro.debug` | the step-through of [`crate::macros::debug`], behind the same step list |
//!
//! Not registered, because they would introduce a second store or a second editor: `macro.save`
//! (upstream's macro document shape — ours is the step list, written with `automate.newScript`
//! and edited step by step), `macro.check` (the footprint dry run, which needs the macro's
//! inputs and output rule we don't store) and `macro.approve` (our approval is the workbook's
//! trust flag, `automate.trustScripts`). No alias adds a ribbon button: each one carries the
//! entry of the action it stands for, so the ribbon still shows the action once, under our name.

use serde_json::json;

use super::*;

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        // --- the recorder and the runner (aliases of our automate.* ids) ---
        cmd!(
            "macro.record",
            "Record Macro",
            ["Automate", "Scripting Tools"],
            None,
            "{stop?: bool, cancel?: bool (ends a recording without saving it), name?: script name} -> {recording: true} or {recording: false, saved, steps, skipped, dropped} — the same recorder as automate.recordActions: the commands that run become steps of a script in the workbook",
            has_doc,
            super::extra::record
        ),
        cmd!(
            "macro.run",
            "Run Macro",
            [],
            None,
            "{name, trusted?: bool (runs a script from an untrusted workbook this once)} -> {ran, script, trusted} — one undo step; the first failing step stops the run, names it, and rolls the whole run back",
            has_doc,
            crate::macros::run::run
        ),
        cmd!(query "macro.list", "List Macros", ["Automate", "Scripting Tools"], None, "{} -> [{name, steps, trusted}] — the workbook's scripts, as automate.allScripts lists them", has_doc, super::extra::all_scripts),
        cmd!(
            query "macro.get",
            "Get Macro",
            [],
            None,
            "{name} -> {name, steps: [{command, params}], trusted} — the script's steps, as automate.scriptSteps returns them",
            has_doc,
            get
        ),
        cmd!("macro.delete", "Delete Macro", [], None, "{name} — removes the script, as automate.deleteScript", has_doc, super::extra::delete_script),
        cmd!(
            "macro.rename",
            "Rename Macro",
            [],
            None,
            "{name, to} — renames the script, as automate.renameScript",
            has_doc,
            super::extra::rename_script
        ),
        // --- step-through ---
        cmd!(
            "macro.debug",
            "Step Into",
            [],
            None,
            "{action: start|step|continue|stop|state, name? (start), trusted?: bool (start), keep?: bool (stop: keep what ran as one undo step, or roll it back)} -> {state: paused|finished|stopped|idle, step?, of?, ran?, command?, params?} — runs a script one step at a time inside one open undo group",
            has_doc,
            crate::macros::debug::debug
        ),
    ]
}

/// `macro.get`: the script's name, steps and trust flag.
fn get(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("macro.get", "missing `name`"))?;
    let d = s.doc()?;
    let steps = crate::macros::steps_of(&d.wb, name).ok_or_else(|| bad("macro.get", format!("no script named \"{name}\"")))?;
    Ok(json!({ "name": name, "steps": steps, "trusted": d.scripts_trusted }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every alias is a registered command with a label, a ribbon path (of its own or of the action
    /// it stands for), a params doc and an `enabled` check that answers on a workbook.
    #[test]
    fn every_alias_is_a_registered_command() {
        let ids = ["macro.record", "macro.run", "macro.list", "macro.get", "macro.delete", "macro.rename", "macro.debug"];
        for id in ids {
            let spec = crate::find_command(id).unwrap_or_else(|| std::panic::panic_any(format!("{id} is not registered")));
            assert!(!spec.label.trim().is_empty(), "{id} has no label");
            assert!(!spec.params.trim().is_empty(), "{id} has no params doc");
            assert!(spec.params.starts_with('{'), "{id}: the params doc names its object");
            let mut s = Session::new();
            assert!((spec.enabled)(&s).is_err(), "{id} needs a workbook");
            s.new_workbook();
            assert!((spec.enabled)(&s).is_ok(), "{id} is enabled on a workbook");
        }
        // No id is registered twice.
        let mut all: Vec<&str> = crate::command_specs().iter().map(|c| c.id).collect();
        let before = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), before, "an id is registered twice");
    }

    /// Empty params work (the recorder starts) or answer with a message that names what is missing.
    #[test]
    fn empty_params_start_recording_or_say_what_is_missing() {
        let mut s = Session::new();
        s.new_workbook();
        assert_eq!(s.execute("macro.record", json!({})).expect("a recording starts")["recording"], true);
        assert_eq!(s.execute("macro.record", json!({"cancel": true})).expect("it ends")["recording"], false);
        for (id, want) in
            [("macro.run", "name"), ("macro.get", "name"), ("macro.delete", "name"), ("macro.rename", "name"), ("macro.debug", "action")]
        {
            let err = s.execute(id, json!({})).expect_err("a missing parameter is refused").to_string();
            assert!(err.contains(want), "{id}: {err}");
        }
        assert!(s.execute("macro.list", json!({})).expect("a list").is_array());
    }
}
