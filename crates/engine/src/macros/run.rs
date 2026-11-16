//! The script runner: a stored step list played through the engine (adoption of upstream's
//! `macros/run.rs`).
//!
//! One run is one undo step: the steps run inside an undo group, and any failure rolls the whole
//! group back, so a script that stops at step 5 leaves the workbook as it was before step 1. The
//! error names the step and the command ("Script 'Build' stopped at step 3 (cell.set): …").
//!
//! A run is bounded: at most [`MAX_EXECUTED_STEPS`] steps execute and at most [`MAX_PARAM_BYTES`] of
//! parameters are handed to commands. A script that runs a script is refused
//! (`Session::enter_script`) instead of recursing, and the policy refuses `automate.*` / `macro.*`
//! steps outright.
//!
//! Every step but an approved file step runs inside [`crate::io::ScriptSandbox`], so a command that
//! reads or writes a file under a parameter name the policy doesn't know still can't.

use serde_json::{Value as Json, json};

use super::policy::{self, Policy};
use crate::cmd::{bad, str_param};
use crate::{EngineError, Result, Session, UiRequest};

/// The most steps one run executes.
pub const MAX_EXECUTED_STEPS: u64 = 10_000;

/// The most bytes of parameters one run passes to its commands.
pub const MAX_PARAM_BYTES: u64 = 64 * 1024 * 1024;

/// What one step did wrong: its place in the list, the command, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepError {
    /// Zero-based index in the step list.
    pub index: usize,
    pub command: Option<String>,
    pub message: String,
}

impl StepError {
    /// "step 3 (cell.set)"
    pub fn describe(&self) -> String {
        match &self.command {
            Some(c) => format!("step {} ({c})", self.index.saturating_add(1)),
            None => format!("step {}", self.index.saturating_add(1)),
        }
    }

    /// What the command that ran the script reports: the step, and that nothing changed.
    pub fn stopped(&self, name: &str) -> EngineError {
        EngineError::Other(format!("Script '{name}' stopped at {}: {}. Nothing was changed.", self.describe(), self.message))
    }
}

/// A run's budget: steps executed and bytes of parameters passed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Budget {
    pub executed: u64,
    pub param_bytes: u64,
}

/// The size of `v` as JSON text, near enough to bound how much a run passes to its commands.
fn json_bytes(v: &Json) -> u64 {
    match v {
        Json::String(t) => (t.len() as u64).saturating_add(2),
        Json::Array(a) => a.iter().map(json_bytes).fold(2, u64::saturating_add),
        Json::Object(o) => o.iter().map(|(k, v)| (k.len() as u64).saturating_add(3).saturating_add(json_bytes(v))).fold(2, u64::saturating_add),
        _ => 8,
    }
}

/// The refusal of a workbook that came from a file: nothing runs until it is trusted.
fn untrusted(name: &str) -> EngineError {
    EngineError::Other(format!(
        "Script '{name}' can't run: its workbook came from a file and its scripts are disabled. Trust the workbook first \
         (Trust Scripts in View › Macros, Alt+F8; `automate.trustScripts`; or `trusted: true` on this call)."
    ))
}

/// Checks every step against the policy before anything runs: a denied step, or a file step in an
/// untrusted workbook, refuses the whole script (so no step runs at all).
pub fn gate(name: &str, steps: &[Json], trusted: bool) -> Result<()> {
    for (i, step) in steps.iter().enumerate() {
        let id = step.get("command").and_then(Json::as_str).unwrap_or("");
        let params = step.get("params").cloned().unwrap_or(json!({}));
        let at = |why: String| EngineError::Other(format!("Script '{name}' can't run: step {} ({id}) {why}.", i.saturating_add(1)));
        match policy::script_policy(id, &params, trusted) {
            // A file step reaching here is in a trusted workbook: the trust flag is the approval.
            Policy::Allow | Policy::NeedsPermission(_) => {}
            Policy::Ask(_) => return Err(untrusted(name)),
            Policy::Deny(why) => return Err(at(format!("can't run: {why}"))),
        }
    }
    Ok(())
}

/// `macro.run` / `automate.runScript`: runs script `name` as one undo step.
///
/// `{name, trusted?: bool}` — `trusted: true` runs a script from a workbook opened from a file this
/// once, without trusting the workbook for the rest of the session.
pub fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let cmd = "macro.run";
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "missing `name`"))?.to_string();
    let once = match p.get("trusted") {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`trusted` must be true or false")),
    };
    let (steps, trusted) = {
        let d = s.doc()?;
        let steps = super::capped_steps(&d.wb, &name).ok_or_else(|| bad(cmd, format!("no script named \"{name}\"")))?;
        (steps, d.scripts_trusted || once)
    };
    gate(&name, &steps, trusted)?;
    if !s.enter_script() {
        return Err(bad(cmd, "a script can't run another script"));
    }
    let label = format!("Run Script: {name}");
    let out = s.grouped(&label, |s| run_steps(s, &name, &steps));
    s.leave_script();
    let ran = out?;
    Ok(json!({ "ran": ran, "script": name, "trusted": trusted }))
}

/// Runs `steps` in the order they are stored, inside the caller's undo group.
pub fn run_steps(s: &mut Session, name: &str, steps: &[Json]) -> Result<usize> {
    let mut budget = Budget::default();
    for (i, step) in steps.iter().enumerate() {
        if let Err(e) = run_one(s, i, step, &mut budget) {
            return Err(e.stopped(name));
        }
    }
    Ok(steps.len())
}

/// Runs one step: checks the policy and the budget, runs the command without a sandbox only when
/// the policy approved it for a file, and refuses a step that would open a dialog or a web page.
pub fn run_one(s: &mut Session, index: usize, step: &Json, budget: &mut Budget) -> std::result::Result<(), StepError> {
    let id = step.get("command").and_then(Json::as_str).unwrap_or("").to_string();
    let at = |msg: String| StepError { index, command: (!id.is_empty()).then(|| id.clone()), message: msg };
    if id.is_empty() {
        return Err(at("it names no `command`".into()));
    }
    if crate::find_command(&id).is_none() {
        return Err(at(format!("`{id}` is not a command")));
    }
    let params = match step.get("params") {
        None | Some(Json::Null) => json!({}),
        Some(v @ Json::Object(_)) => v.clone(),
        Some(_) => return Err(at("its `params` are not a JSON object".into())),
    };
    budget.executed = budget.executed.saturating_add(1);
    if budget.executed > MAX_EXECUTED_STEPS {
        return Err(at(format!("the run went past its budget of {MAX_EXECUTED_STEPS} steps")));
    }
    budget.param_bytes = budget.param_bytes.saturating_add(json_bytes(&params));
    if budget.param_bytes > MAX_PARAM_BYTES {
        return Err(at(format!("the run went past its budget of {} MB of parameters", MAX_PARAM_BYTES / (1024 * 1024))));
    }
    // Checked again per step: `macro.debug` runs steps one at a time and never goes through
    // [`gate`] for the whole list.
    let file_step = match policy::script_policy(&id, &params, true) {
        Policy::Deny(why) => return Err(at(format!("`{id}` can't run in a script: {why}"))),
        Policy::NeedsPermission(_) => true,
        Policy::Allow | Policy::Ask(_) => false,
    };
    let queued = s.ui_requests.len();
    let r = {
        // Only a file step the policy approved reaches the file system.
        let _sandbox = (!file_step).then(crate::io::ScriptSandbox::enter);
        s.execute(&id, params)
    };
    // A script never opens a dialog or a web page: messages for the person stay queued.
    let asked: Vec<UiRequest> = if s.ui_requests.len() > queued { s.ui_requests.drain(queued..).collect() } else { Vec::new() };
    let mut blocked = None;
    for q in asked {
        match q {
            UiRequest::Dialog(n, _) => blocked = Some(format!("`{id}` would open the {n} dialog; give the step the parameters it needs")),
            UiRequest::OpenUrl(_) => blocked = Some(format!("`{id}` would open a web page, which a script doesn't do")),
            UiRequest::EditCell(_) | UiRequest::InsertName(_) => {}
            m @ UiRequest::Message(_) => s.ui_requests.push(m),
        }
    }
    r.map_err(|e| at(e.to_string()))?;
    match blocked {
        Some(why) => Err(at(why)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_size_budget_is_additive() {
        assert_eq!(json_bytes(&json!({})), 2);
        assert_eq!(json_bytes(&json!("ab")), 4);
        assert_eq!(json_bytes(&json!(true)), 8);
        // A key costs its length plus "":` and the separator.
        assert_eq!(json_bytes(&json!({"a": 1})), 2 + 1 + 3 + 8);
    }

    #[test]
    fn a_step_error_names_the_step_and_the_command() {
        let e = StepError { index: 2, command: Some("cell.set".into()), message: "no such cell".into() };
        assert_eq!(e.describe(), "step 3 (cell.set)");
        let text = e.stopped("Build").to_string();
        assert!(text.contains("step 3 (cell.set)") && text.contains("Nothing was changed"), "{text}");
    }
}
