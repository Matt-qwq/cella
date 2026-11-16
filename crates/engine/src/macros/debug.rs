//! `macro.debug`: a script run one step at a time (adoption of upstream's `macros/debug.rs`).
//!
//! The steps of a debug session run inside **one open undo group**: `step` runs the next step,
//! `continue` runs the rest, and `stop` ends the session either keeping what ran as a single undo
//! step (`keep: true`) or rolling the workbook back. While a session is open, Undo and Redo are
//! unavailable on that workbook (the group is one step until it ends), which is what keeps a
//! half-run script out of the history.
//!
//! A debug session runs on its own: it is never a step of a script (`macro.*` steps are denied by
//! the policy), and it is not recorded (`macro.*` is filtered by the recorder).
//!
//! What a front end still needs, and this batch does not have: a Step / Continue / Stop button
//! bound to `macro.debug` with `action` set, somewhere to show `state` between calls, and — for
//! parity with Excel's Step Into — breakpoints (which need a step shape that carries one: ours is
//! `{command, params}`, so a breakpoint would be stored next to the list, not in it) and a lock so
//! the person can't edit the workbook under a paused session.

use serde_json::{Value as Json, json};

use super::run::{self, Budget};
use crate::cmd::str_param;
use crate::{EngineError, GroupToken, Result, Session};

/// The actions `macro.debug` takes.
pub const ACTIONS: &str = "start|step|continue|stop|state";

/// A script being stepped through: where it is, and the undo group that holds what it ran.
#[derive(Debug)]
pub struct Debugger {
    pub name: String,
    pub doc: u64,
    steps: Vec<Json>,
    /// The index of the step that runs next.
    at: usize,
    /// The group holding the steps that ran ([Session::group_begin]).
    token: GroupToken,
    budget: Budget,
}

impl Debugger {
    /// The step that runs next, as `{command, params}` (the editor's shape).
    fn current(&self) -> Option<&Json> {
        self.steps.get(self.at)
    }
    fn done(&self) -> bool {
        self.at >= self.steps.len()
    }
    fn state_json(&self) -> Json {
        let mut out = json!({
            "state": "paused",
            "name": self.name,
            "step": self.at.saturating_add(1),
            "of": self.steps.len(),
            "ran": self.at,
        });
        if let Some(st) = self.current() {
            out["command"] = st.get("command").cloned().unwrap_or(Json::Null);
            out["params"] = st.get("params").cloned().unwrap_or(json!({}));
        }
        out
    }
}

/// `macro.debug`: `{action, name?, keep?, trusted?}`.
pub fn debug(s: &mut Session, p: &Json) -> Result<Json> {
    let action = str_param(p, "action").map(str::trim).unwrap_or("");
    match action {
        "start" => start(s, p),
        "step" => advance(s, 1),
        "continue" => advance(s, usize::MAX),
        "stop" => stop(s, p),
        "state" => Ok(state(s)),
        "" => Err(refuse(format!("missing `action`: one of {ACTIONS}"))),
        other => Err(refuse(format!("`{other}` is not an action: one of {ACTIONS}"))),
    }
}

fn refuse(msg: String) -> EngineError {
    EngineError::BadParams { cmd: "macro.debug".into(), msg }
}

fn start(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| refuse("missing `name`".into()))?.to_string();
    if s.debug().is_some_and(|d| s.group_is_open(&d.token)) {
        return Err(EngineError::Disabled("macro.debug".into(), "a script is already being debugged: stop it first".into()));
    }
    let once = match p.get("trusted") {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(refuse("`trusted` must be true or false".into())),
    };
    let (steps, trusted) = {
        let d = s.doc()?;
        let steps = super::capped_steps(&d.wb, &name).ok_or_else(|| refuse(format!("no script named \"{name}\"")))?;
        (steps, d.scripts_trusted || once)
    };
    run::gate(&name, &steps, trusted)?;
    let token = s.group_begin(&format!("Debug Script: {name}"))?;
    s.set_debug(Debugger { name, doc: token.doc(), steps, at: 0, token, budget: Budget::default() });
    Ok(state(s))
}

/// Runs up to `n` steps (`usize::MAX`: to the end), ending the session when the script finishes.
fn advance(s: &mut Session, mut n: usize) -> Result<Json> {
    let Some(mut d) = s.take_debug() else {
        return Err(EngineError::Disabled("macro.debug".into(), "no script is being debugged: start one first".into()));
    };
    if !s.group_is_open(&d.token) {
        // The workbook (and the group with it) closed under the session.
        return Ok(json!({ "state": "idle", "lost": d.name }));
    }
    while n > 0 && !d.done() {
        let index = d.at;
        let Some(step) = d.steps.get(index).cloned() else { break };
        if let Err(e) = run::run_one(s, index, &step, &mut d.budget) {
            // The step failed: the whole session is rolled back, as a run is.
            let _ = s.group_rollback(d.token);
            return Err(e.stopped(&d.name));
        }
        d.at = d.at.saturating_add(1);
        n = n.saturating_sub(1);
    }
    if d.done() {
        let mut out = d.state_json();
        out["state"] = json!("finished");
        // What ran stays as one undo step.
        let _ = s.group_end(d.token);
        return Ok(out);
    }
    let out = d.state_json();
    s.set_debug(d);
    Ok(out)
}

fn stop(s: &mut Session, p: &Json) -> Result<Json> {
    let keep = match p.get("keep") {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(refuse("`keep` must be true or false".into())),
    };
    let Some(d) = s.take_debug() else { return Ok(json!({ "state": "idle" })) };
    // A group on a closed workbook is already gone: there is nothing left to keep or roll back.
    if s.group_is_open(&d.token) {
        if keep {
            let _ = s.group_end(d.token);
        } else {
            let _ = s.group_rollback(d.token);
        }
    }
    Ok(json!({ "state": "stopped", "name": d.name, "ran": d.at, "kept": keep }))
}

fn state(s: &Session) -> Json {
    match s.debug() {
        Some(d) if s.group_is_open(&d.token) => d.state_json(),
        Some(d) => json!({ "state": "idle", "lost": d.name }),
        None => json!({ "state": "idle" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_call_says_which_actions_there_are() {
        let mut s = Session::new();
        s.new_workbook();
        let err = debug(&mut s, &json!({})).unwrap_err().to_string();
        assert!(err.contains("start") && err.contains("stop"), "{err}");
        let err = debug(&mut s, &json!({"action": "dance"})).unwrap_err().to_string();
        assert!(err.contains("state"), "{err}");
        assert_eq!(state(&s), json!({ "state": "idle" }));
        assert!(debug(&mut s, &json!({"action": "step"})).is_err(), "nothing is being debugged");
    }
}
